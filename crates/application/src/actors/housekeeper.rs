//! Background housekeeping: the dead-media sweep.
//!
//! Sources can vanish between curation and publication (FA takedowns,
//! deleted tweets, nuked accounts). The sweep walks the feed entries no
//! consumer has passed yet — `(min cursor, feed end]` — and re-resolves
//! each source's media so a dead one is reported *before* a Poster trips
//! over it at fire time. Report-only: nothing is deleted or re-statused;
//! takedowns stay a human decision.
//!
//! Along the way it backfills missing perceptual hashes (entries curated
//! before the pHash era), keeping the duplicate-check corpus complete.

use domain::elements::{
    media::{MediaResolveError, MediaResolver, ResolvedMedia},
    phash::PerceptualHasher,
    post::{PostId, PostRepository},
    poster::PosterRepository,
};
use telemetry::Event;

/// Hash backfills per sweep — keeps one run from hammering the source
/// platforms; the next sweep continues where this one stopped.
const BACKFILL_CAP: usize = 100;

/// A feed entry whose upstream content is confirmed gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadEntry {
    pub post_id: PostId,
    pub source: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SweepOutcome {
    /// Feed entries examined (everything not yet consumed by every poster).
    pub scanned: usize,
    /// Entries whose upstream is confirmed gone (resolver `NotFound`).
    /// Transient failures (network, auth, parse) are NOT dead — they log
    /// and retry next sweep.
    pub dead: Vec<DeadEntry>,
    /// Perceptual hashes computed for entries that were missing one.
    pub hashes_backfilled: usize,
}

pub async fn run_sweep<R, PR, H>(
    posts: &R,
    posters: &PR,
    resolver: &dyn MediaResolver,
    hasher: &H,
) -> Result<SweepOutcome, String>
where
    R: PostRepository,
    R::Err: std::fmt::Display,
    PR: PosterRepository,
    PR::Err: std::fmt::Display,
    H: PerceptualHasher + ?Sized,
{
    // Entries every poster has already passed are history — publications
    // don't retroactively break. Only the unconsumed window matters.
    let min_cursor = posters
        .list_all()
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .map(|p| p.cursor)
        .min()
        .unwrap_or(0);
    let end = posts.feed_end().await.map_err(|e| e.to_string())?;
    let entries = posts
        .feed_after(min_cursor, end)
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!(
        event = %Event::SweepStarted, window_start = min_cursor,
        window_end = end, entries = entries.len(), "dead-media sweep started"
    );

    let mut outcome = SweepOutcome {
        scanned: entries.len(),
        ..Default::default()
    };
    for entry in entries {
        let media = match resolver.resolve(&entry.source).await {
            Ok(media) => media,
            Err(MediaResolveError::NotFound(_)) => {
                tracing::warn!(
                    event = %Event::DeadMediaFound, post_id = %entry.id,
                    source = %entry.source.as_ref(), "upstream content gone"
                );
                outcome.dead.push(DeadEntry {
                    post_id: entry.id,
                    source: entry.source.as_ref().to_string(),
                });
                continue;
            }
            // Transient or configuration trouble — not the source's fault.
            Err(e) => {
                tracing::debug!(
                    post_id = %entry.id, error = %e,
                    "sweep resolve failed transiently; retrying next sweep"
                );
                continue;
            }
        };
        if entry.phash.is_none() && outcome.hashes_backfilled < BACKFILL_CAP {
            if let ResolvedMedia::Photo(url) = &media {
                match hasher.hash_image(url).await {
                    Ok(hash) => {
                        if posts.set_phash(entry.id, Some(hash)).await.is_ok() {
                            outcome.hashes_backfilled += 1;
                            tracing::debug!(
                                event = %Event::PhashComputed, post_id = %entry.id,
                                phash = format!("{hash:016x}"), "hash backfilled by sweep"
                            );
                        }
                    }
                    Err(e) => tracing::debug!(
                        event = %Event::PhashFailed, post_id = %entry.id,
                        error = %e, "sweep backfill hash failed"
                    ),
                }
            }
        }
    }
    tracing::info!(
        event = %Event::SweepCompleted, scanned = outcome.scanned,
        dead = outcome.dead.len(), hashes_backfilled = outcome.hashes_backfilled,
        "dead-media sweep completed"
    );
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::{HashMap, HashSet};

    use async_trait::async_trait;
    use chrono::Utc;
    use domain::elements::{
        cadence::PostInterval,
        phash::PHashError,
        post::{PostStatus, Source},
    };
    use persistence::in_memory::{post::InMemoryPostRepository, poster::InMemoryPosterRepository};
    use url::Url;

    /// Resolver stub: URLs in `gone` → NotFound, in `flaky` → Network,
    /// everything else resolves as a Photo of itself.
    #[derive(Default)]
    struct StubResolver {
        gone: HashSet<String>,
        flaky: HashSet<String>,
    }
    #[async_trait]
    impl MediaResolver for StubResolver {
        async fn resolve(&self, source: &Source) -> Result<ResolvedMedia, MediaResolveError> {
            let url: &Url = source.as_ref();
            if self.gone.contains(url.as_str()) {
                return Err(MediaResolveError::NotFound(source.clone()));
            }
            if self.flaky.contains(url.as_str()) {
                return Err(MediaResolveError::Network("timeout".into()));
            }
            Ok(ResolvedMedia::Photo(url.clone()))
        }
    }

    struct StubHasher(HashMap<String, u64>);
    #[async_trait]
    impl PerceptualHasher for StubHasher {
        async fn hash_image(&self, url: &Url) -> Result<u64, PHashError> {
            self.0
                .get(url.as_str())
                .copied()
                .ok_or_else(|| PHashError::Fetch("unknown".into()))
        }
    }

    async fn feed_entry(posts: &InMemoryPostRepository, id: u64) -> domain::elements::post::Post {
        let post = posts
            .create(
                Source::try_from(Url::parse(&format!("https://e621.net/posts/{id}")).unwrap())
                    .unwrap(),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::AwaitingModeration,
            )
            .await
            .unwrap();
        posts.accept_into_feed(post.id).await.unwrap()
    }

    #[tokio::test]
    async fn gone_media_is_reported_transient_failures_are_not() {
        let posts = InMemoryPostRepository::new();
        let gone = feed_entry(&posts, 1).await;
        let flaky = feed_entry(&posts, 2).await;
        let alive = feed_entry(&posts, 3).await;
        let posters = InMemoryPosterRepository::new();

        let resolver = StubResolver {
            gone: [gone.source.as_ref().to_string()].into(),
            flaky: [flaky.source.as_ref().to_string()].into(),
        };
        let outcome = run_sweep(&posts, &posters, &resolver, &StubHasher(HashMap::new()))
            .await
            .unwrap();
        assert_eq!(outcome.scanned, 3);
        assert_eq!(
            outcome.dead,
            vec![DeadEntry {
                post_id: gone.id,
                source: gone.source.as_ref().to_string(),
            }]
        );
        // The alive entry resolved; its hash just wasn't known to the stub.
        let _ = alive;
    }

    #[tokio::test]
    async fn missing_hashes_are_backfilled() {
        let posts = InMemoryPostRepository::new();
        let entry = feed_entry(&posts, 1).await;
        let hashed_already = feed_entry(&posts, 2).await;
        posts.set_phash(hashed_already.id, Some(42)).await.unwrap();
        let posters = InMemoryPosterRepository::new();

        let hasher = StubHasher(
            [(entry.source.as_ref().to_string(), 0xABCD_u64)]
                .into_iter()
                .collect(),
        );
        let outcome = run_sweep(&posts, &posters, &StubResolver::default(), &hasher)
            .await
            .unwrap();
        assert_eq!(outcome.hashes_backfilled, 1);
        let stored = posts.find_by_id(entry.id).await.unwrap().unwrap();
        assert_eq!(stored.phash, Some(0xABCD));
        // The pre-hashed entry kept its hash (the stub would have errored).
        let stored = posts.find_by_id(hashed_already.id).await.unwrap().unwrap();
        assert_eq!(stored.phash, Some(42));
    }

    #[tokio::test]
    async fn entries_every_poster_consumed_are_not_scanned() {
        let posts = InMemoryPostRepository::new();
        let consumed = feed_entry(&posts, 1).await; // position 1
        feed_entry(&posts, 2).await; // position 2 — still pending
        let posters = InMemoryPosterRepository::new();
        // Both posters are past position 1.
        for cursor in [1, 2] {
            posters
                .create(vec![], vec![], PostInterval::new(5).unwrap(), cursor)
                .await
                .unwrap();
        }

        let resolver = StubResolver {
            gone: [consumed.source.as_ref().to_string()].into(),
            flaky: HashSet::new(),
        };
        let outcome = run_sweep(&posts, &posters, &resolver, &StubHasher(HashMap::new()))
            .await
            .unwrap();
        // The consumed (and now dead) entry is history — not reported.
        assert_eq!(outcome.scanned, 1);
        assert!(outcome.dead.is_empty());
    }
}

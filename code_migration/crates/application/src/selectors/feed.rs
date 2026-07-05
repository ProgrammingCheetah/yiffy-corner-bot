//! The feed-walk selection policy (feed model, 2026-07-05).
//!
//! One curated feed, per-consumer cursors — BSky semantics. On each fire the
//! consumer snapshots the feed end, scans `(cursor, end]` in feed order, and
//! takes the first entry matching its tag criteria:
//!
//! - the cursor lands on the matched entry's position (later matches wait for
//!   the next tick), or on the *snapshot* end when nothing matched — entries
//!   appended mid-scan sit beyond the snapshot and are never skipped;
//! - e621 entries re-validate against *fresh* e621 tags (an upstream retag
//!   can still ban an entry post-curation); other sources check their curated
//!   tags against the *current* global forbidden list;
//! - a globally forbidden tag flips the entry to `Banned` (skipped by every
//!   consumer); a `Banned` entry whose effective tags are clean again flips
//!   back to `Accepted`;
//! - consumer-local `forbidden_tags` skip without any status change;
//! - upstream fetch errors abort the scan WITHOUT advancing the cursor, so
//!   the same entry retries next tick.

use std::collections::HashSet;
use std::sync::Arc;

use domain::elements::{
    e621::E621Fetcher,
    post::{
        FeedPick, Post, PostRepository, PostSelectorStrategy, PostStatus, SelectorError, Source,
    },
    poster::Poster,
    tag::Tag,
    tag_policy::ForbiddenTagRepository,
};
use telemetry::{Event, SkipReason};

pub struct FeedSelector<P, E, F> {
    poster: Poster,
    posts: Arc<P>,
    e621: Arc<E>,
    forbidden: Arc<F>,
}

impl<P, E, F> FeedSelector<P, E, F> {
    pub fn new(poster: Poster, posts: Arc<P>, e621: Arc<E>, forbidden: Arc<F>) -> Self {
        Self {
            poster,
            posts,
            e621,
            forbidden,
        }
    }
}

impl<P, E, F> FeedSelector<P, E, F>
where
    P: PostRepository + Send + Sync,
    P::Err: std::fmt::Display,
    E: E621Fetcher,
    F: ForbiddenTagRepository,
    F::Err: std::fmt::Display,
{
    /// The tags that count for this entry right now: fresh from e621 for
    /// e621 sources, the curated set for everything else.
    async fn effective_tags(&self, entry: &Post) -> Result<HashSet<Tag>, SelectorError> {
        match &entry.source {
            Source::E621(_) => {
                let metadata = self
                    .e621
                    .fetch(&entry.source)
                    .await
                    .map_err(|e| SelectorError::Fetch(e.to_string()))?;
                tracing::debug!(
                    event = %Event::TagsFetched, post_id = %entry.id,
                    tag_count = metadata.tags.len(), "fresh e621 tags fetched"
                );
                Ok(metadata.tags.into_iter().collect())
            }
            _ => Ok(entry.tags.iter().cloned().collect()),
        }
    }

    /// Whether this consumer can take the entry; applies the status flips.
    async fn usable(
        &self,
        entry: &Post,
        global_forbidden: &HashSet<Tag>,
    ) -> Result<bool, SelectorError> {
        let tags = self.effective_tags(entry).await?;

        if let Some(hit) = tags.iter().find(|t| global_forbidden.contains(*t)) {
            if entry.status != PostStatus::Banned {
                tracing::info!(
                    event = %Event::StatusFlipped, post_id = %entry.id,
                    to = %PostStatus::Banned, tag = %hit, "globally forbidden tag → Banned"
                );
                self.posts
                    .set_status_to(entry.id, PostStatus::Banned)
                    .await
                    .map_err(|e| SelectorError::Repository(e.to_string()))?;
            } else {
                tracing::debug!(
                    event = %Event::CandidateSkipped, reason = %SkipReason::GlobalForbiddenTag,
                    post_id = %entry.id, tag = %hit, "still Banned (forbidden tag present)"
                );
            }
            return Ok(false);
        }
        if entry.status == PostStatus::Banned {
            // Effective tags are clean again: the cached verdict expires.
            tracing::info!(
                event = %Event::StatusFlipped, post_id = %entry.id,
                to = %PostStatus::Accepted, "tags clean again → Banned lifted to Accepted"
            );
            self.posts
                .set_status_to(entry.id, PostStatus::Accepted)
                .await
                .map_err(|e| SelectorError::Repository(e.to_string()))?;
        }

        if let Some(hit) = self
            .poster
            .forbidden_tags
            .iter()
            .find(|t| tags.contains(*t))
        {
            tracing::debug!(
                event = %Event::CandidateSkipped, reason = %SkipReason::PosterForbiddenTag,
                post_id = %entry.id, poster_id = %self.poster.id, tag = %hit,
                "skipped for this consumer only"
            );
            return Ok(false);
        }
        let missing: Vec<&Tag> = self
            .poster
            .subscribed_tags
            .iter()
            .filter(|t| !tags.contains(*t))
            .collect();
        if missing.is_empty() {
            Ok(true)
        } else {
            tracing::debug!(
                event = %Event::CandidateSkipped, reason = %SkipReason::MissingSubscribedTags,
                post_id = %entry.id, poster_id = %self.poster.id, missing = ?missing,
                "subscription tags not all present"
            );
            Ok(false)
        }
    }
}

/// Hands the scheduler a fresh [`FeedSelector`] per fire, wrapping shared
/// repository handles. Config comes in via the poster argument each time —
/// database-first, nothing cached.
pub struct FeedSelectorFactory<P, E, F> {
    pub posts: Arc<P>,
    pub e621: Arc<E>,
    pub forbidden: Arc<F>,
}

impl<P, E, F> crate::actors::scheduler::SelectorFactory for FeedSelectorFactory<P, E, F>
where
    P: PostRepository + Send + Sync + 'static,
    P::Err: std::fmt::Display,
    E: E621Fetcher + 'static,
    F: ForbiddenTagRepository + 'static,
    F::Err: std::fmt::Display,
{
    fn for_poster(&self, poster: Poster) -> Box<dyn PostSelectorStrategy> {
        Box::new(FeedSelector::new(
            poster,
            self.posts.clone(),
            self.e621.clone(),
            self.forbidden.clone(),
        ))
    }
}

#[async_trait::async_trait]
impl<P, E, F> PostSelectorStrategy for FeedSelector<P, E, F>
where
    P: PostRepository + Send + Sync,
    P::Err: std::fmt::Display,
    E: E621Fetcher,
    F: ForbiddenTagRepository,
    F::Err: std::fmt::Display,
{
    async fn next_post(&self, cursor: u64) -> Result<FeedPick, SelectorError> {
        // Snapshot BEFORE scanning: entries appended while we resolve stay
        // ahead of wherever the cursor lands.
        let end = self
            .posts
            .feed_end()
            .await
            .map_err(|e| SelectorError::Repository(e.to_string()))?;
        if end <= cursor {
            return Ok(FeedPick {
                post: None,
                advance_to: cursor,
            });
        }

        let entries = self
            .posts
            .feed_after(cursor, end)
            .await
            .map_err(|e| SelectorError::Repository(e.to_string()))?;
        tracing::debug!(
            event = %Event::FeedScanStarted, poster_id = %self.poster.id,
            cursor, snapshot_end = end, entries = entries.len(), "scanning feed window"
        );

        let global_forbidden: HashSet<Tag> = self
            .forbidden
            .list_all()
            .await
            .map_err(|e| SelectorError::Repository(e.to_string()))?
            .into_iter()
            .collect();

        for entry in entries {
            if self.usable(&entry, &global_forbidden).await? {
                let position = entry
                    .feed_position
                    .expect("feed_after only returns positioned entries");
                tracing::info!(
                    event = %Event::FeedMatch, poster_id = %self.poster.id,
                    post_id = %entry.id, position, "feed entry matched"
                );
                return Ok(FeedPick {
                    post: Some(entry),
                    advance_to: position,
                });
            }
        }
        Ok(FeedPick {
            post: None,
            advance_to: end,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use async_trait::async_trait;
    use chrono::Utc;
    use domain::elements::{
        cadence::PostInterval,
        e621::{E621Order, E621PostMetadata, FetchError},
        poster::PosterId,
        user::UserId,
    };
    use persistence::in_memory::{
        post::InMemoryPostRepository, tag_policy::InMemoryForbiddenTagRepository,
    };
    use url::Url;

    struct StubFetcher(HashMap<Url, Vec<Tag>>);
    #[async_trait]
    impl E621Fetcher for StubFetcher {
        async fn fetch(&self, source: &Source) -> Result<E621PostMetadata, FetchError> {
            let url: &Url = source.as_ref();
            let tags = self
                .0
                .get(url)
                .cloned()
                .ok_or_else(|| FetchError::NotFound(source.clone()))?;
            Ok(E621PostMetadata {
                source: source.clone(),
                tags,
                file_url: Url::parse("https://static1.e621.net/data/full.png").unwrap(),
                preview_url: Url::parse("https://static1.e621.net/data/preview.png").unwrap(),
                artist_sources: vec![],
            })
        }
        async fn search(
            &self,
            _tags: &[Tag],
            _order: E621Order,
            _page: u32,
        ) -> Result<Vec<E621PostMetadata>, FetchError> {
            unimplemented!("not needed by selector tests")
        }
    }

    fn tags(names: &[&str]) -> Vec<Tag> {
        names.iter().map(|n| Tag::from(*n)).collect()
    }

    fn poster(subscribed: &[&str], forbidden: &[&str]) -> Poster {
        Poster {
            id: PosterId::from(1),
            subscribed_tags: tags(subscribed),
            forbidden_tags: tags(forbidden),
            time_interval: PostInterval::new(5).unwrap(),
            cursor: 0,
        }
    }

    struct Fixture {
        posts: Arc<InMemoryPostRepository>,
        forbidden: Arc<InMemoryForbiddenTagRepository>,
        fresh_tags: HashMap<Url, Vec<Tag>>,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                posts: Arc::new(InMemoryPostRepository::new()),
                forbidden: Arc::new(InMemoryForbiddenTagRepository::new()),
                fresh_tags: HashMap::new(),
            }
        }

        /// Curated straight into the feed with `curated` tags; e621 entries
        /// also get `fresh` tags served by the stub fetcher.
        async fn feed_entry(
            &mut self,
            url: &str,
            curated: &[&str],
            fresh: Option<&[&str]>,
        ) -> Post {
            let url = Url::parse(url).unwrap();
            if let Some(fresh) = fresh {
                self.fresh_tags.insert(url.clone(), tags(fresh));
            }
            let post = self
                .posts
                .create(
                    Source::try_from(url).unwrap(),
                    tags(curated),
                    Some(UserId::from(7)),
                    Utc::now(),
                    PostStatus::AwaitingModeration,
                )
                .await
                .unwrap();
            self.posts.accept_into_feed(post.id).await.unwrap()
        }

        fn selector(
            &self,
            poster: Poster,
        ) -> FeedSelector<InMemoryPostRepository, StubFetcher, InMemoryForbiddenTagRepository>
        {
            FeedSelector::new(
                poster,
                self.posts.clone(),
                Arc::new(StubFetcher(self.fresh_tags.clone())),
                self.forbidden.clone(),
            )
        }
    }

    #[tokio::test]
    async fn walks_forward_and_advances_to_match() {
        let mut fx = Fixture::new();
        fx.feed_entry("https://e621.net/posts/1", &[], Some(&["cat"]))
            .await;
        let wolf = fx
            .feed_entry("https://e621.net/posts/2", &[], Some(&["wolf"]))
            .await;
        fx.feed_entry("https://e621.net/posts/3", &[], Some(&["wolf"]))
            .await;

        let selector = fx.selector(poster(&["wolf"], &[]));
        let pick = selector.next_post(0).await.unwrap();
        assert_eq!(pick.post.map(|p| p.id), Some(wolf.id));
        assert_eq!(pick.advance_to, 2); // lands ON the match, not the end
    }

    #[tokio::test]
    async fn no_match_advances_to_snapshot_end() {
        let mut fx = Fixture::new();
        fx.feed_entry("https://e621.net/posts/1", &[], Some(&["cat"]))
            .await;
        fx.feed_entry("https://e621.net/posts/2", &[], Some(&["dog"]))
            .await;

        let selector = fx.selector(poster(&["wolf"], &[]));
        let pick = selector.next_post(0).await.unwrap();
        assert!(pick.post.is_none());
        assert_eq!(pick.advance_to, 2);
    }

    #[tokio::test]
    async fn empty_window_keeps_cursor() {
        let fx = Fixture::new();
        let selector = fx.selector(poster(&[], &[]));
        let pick = selector.next_post(0).await.unwrap();
        assert!(pick.post.is_none());
        assert_eq!(pick.advance_to, 0);
    }

    #[tokio::test]
    async fn consumed_entries_are_never_revisited() {
        let mut fx = Fixture::new();
        let first = fx
            .feed_entry("https://e621.net/posts/1", &[], Some(&["wolf"]))
            .await;
        let second = fx
            .feed_entry("https://e621.net/posts/2", &[], Some(&["wolf"]))
            .await;

        let selector = fx.selector(poster(&["wolf"], &[]));
        let pick = selector.next_post(0).await.unwrap();
        assert_eq!(pick.post.map(|p| p.id), Some(first.id));
        let pick = selector.next_post(pick.advance_to).await.unwrap();
        assert_eq!(pick.post.map(|p| p.id), Some(second.id));
        let pick = selector.next_post(pick.advance_to).await.unwrap();
        assert!(pick.post.is_none()); // feed exhausted: quiet
    }

    #[tokio::test]
    async fn non_e621_entries_match_on_curated_tags() {
        let mut fx = Fixture::new();
        let tweet = fx
            .feed_entry("https://x.com/artist/status/1", &["wolf", "male"], None)
            .await;

        let selector = fx.selector(poster(&["wolf"], &[]));
        let pick = selector.next_post(0).await.unwrap();
        assert_eq!(pick.post.map(|p| p.id), Some(tweet.id));
    }

    #[tokio::test]
    async fn fresh_forbidden_tag_bans_and_skips() {
        let mut fx = Fixture::new();
        let dirty = fx
            .feed_entry("https://e621.net/posts/1", &[], Some(&["wolf", "gore"]))
            .await;
        fx.forbidden.add(Tag::from("gore")).await.unwrap();

        let selector = fx.selector(poster(&["wolf"], &[]));
        let pick = selector.next_post(0).await.unwrap();
        assert!(pick.post.is_none());
        assert_eq!(pick.advance_to, 1);
        let stored = fx.posts.find_by_id(dirty.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::Banned);
    }

    #[tokio::test]
    async fn curated_tags_of_non_e621_respect_global_list() {
        let mut fx = Fixture::new();
        fx.feed_entry("https://x.com/a/status/1", &["wolf", "gore"], None)
            .await;
        fx.forbidden.add(Tag::from("gore")).await.unwrap();

        let selector = fx.selector(poster(&[], &[]));
        let pick = selector.next_post(0).await.unwrap();
        assert!(pick.post.is_none());
    }

    #[tokio::test]
    async fn banned_entry_with_clean_tags_is_unbanned_and_taken() {
        let mut fx = Fixture::new();
        let entry = fx
            .feed_entry("https://e621.net/posts/1", &[], Some(&["wolf"]))
            .await;
        fx.posts
            .set_status_to(entry.id, PostStatus::Banned)
            .await
            .unwrap();

        let selector = fx.selector(poster(&["wolf"], &[]));
        let pick = selector.next_post(0).await.unwrap();
        assert_eq!(pick.post.map(|p| p.id), Some(entry.id));
        let stored = fx.posts.find_by_id(entry.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::Accepted);
    }

    #[tokio::test]
    async fn consumer_forbidden_tag_skips_without_status_change() {
        let mut fx = Fixture::new();
        let entry = fx
            .feed_entry("https://e621.net/posts/1", &[], Some(&["wolf", "vore"]))
            .await;

        let selector = fx.selector(poster(&["wolf"], &["vore"]));
        let pick = selector.next_post(0).await.unwrap();
        assert!(pick.post.is_none());
        let stored = fx.posts.find_by_id(entry.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::Accepted);
    }

    #[tokio::test]
    async fn fetch_error_propagates_without_advancing() {
        let mut fx = Fixture::new();
        // e621 entry with NO fresh tags registered → stub returns NotFound.
        fx.feed_entry("https://e621.net/posts/404", &[], None).await;

        let selector = fx.selector(poster(&[], &[]));
        let err = selector.next_post(0).await.unwrap_err();
        assert!(matches!(err, SelectorError::Fetch(_)));
        // Caller keeps the old cursor: the entry retries next tick.
    }
}

//! Out-of-band pool publication — posts a staged pool NOW instead of letting
//! it drip one-per-tick through the feed.
//!
//! The batch goes to every Poster whose full eligibility check (subscription,
//! forbidden tags, rules) matches the TRIGGERING post — "the pool goes
//! wherever the reviewed page would have gone". Within the batch each page
//! still respects a poster's own forbidden tags (channel content policy
//! outranks comic continuity), but NOT its subscription/rules — interest was
//! already decided at pool level, and per-page matching would tear comics
//! apart (page 4 has no `solo` tag, page 5 does…).
//!
//! Pacing: a pool of up to [`POOL_BURST_MAX`] pages goes out in one burst;
//! anything bigger is sent in groups of [`POOL_CHUNK_SIZE`] with
//! [`POOL_CHUNK_PAUSE`] between groups, which stays well under Telegram's
//! per-chat flood limits. Cursors are never touched — staged pool pages hold
//! no feed position, so the scheduler can't see them at all.

use std::collections::HashSet;
use std::time::Duration;

use chrono::Utc;
use domain::elements::{
    post::{Post, PostRepository, PostStatus},
    poster::PosterRepository,
    publisher::PublicationRepository,
    tag::Tag,
    tag_policy::SpoilerTagRepository,
    user::UserRepository,
};
use telemetry::{Event, SkipReason};

use crate::actors::scheduler::{SchedulerDeps, publish_resolved};
use crate::selectors::feed::refusal_for;

/// Pools up to this size post "all at the same time" (one burst).
pub const POOL_BURST_MAX: usize = 10;
/// Bigger pools go out in groups of this many pages…
pub const POOL_CHUNK_SIZE: usize = 5;
/// …with this pause between groups.
pub const POOL_CHUNK_PAUSE: Duration = Duration::from_secs(30);

/// What happened to a pool batch, for the moderator's summary DM.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PoolBatchReport {
    /// Posters whose eligibility matched the triggering post.
    pub channels: usize,
    /// Pages that reached at least one channel.
    pub published: usize,
    /// Pages whose media could not be resolved (gone or transient failure).
    pub unresolved: usize,
    /// Page×channel sends that Telegram refused.
    pub send_failures: usize,
    /// Page×channel skips from a poster's own forbidden tags.
    pub poster_skips: usize,
}

/// Publish `pages` (pool order) to every poster matching `trigger_tags`.
/// `pause` is injected so tests don't sleep; production callers pass
/// [`POOL_CHUNK_PAUSE`]. Failures never abort the batch — each page/channel
/// pair is independent, and the report carries the tallies.
pub async fn publish_pool<R, U, PR, PB, ST>(
    pages: &[Post],
    trigger_tags: &[Tag],
    deps: &SchedulerDeps<R, U, PR, PB, ST>,
    pause: Duration,
) -> PoolBatchReport
where
    R: PostRepository + Send + Sync,
    R::Err: std::fmt::Display,
    U: UserRepository,
    PR: PosterRepository,
    PR::Err: std::fmt::Display,
    PB: PublicationRepository,
    PB::Err: std::fmt::Display,
    ST: SpoilerTagRepository,
    ST::Err: std::fmt::Display,
{
    let mut report = PoolBatchReport::default();
    let posters = match deps.posters.list_all().await {
        Ok(posters) => posters,
        Err(e) => {
            tracing::error!(event = %Event::SelectorFailed, error = %e, "poster listing failed; pool batch aborted");
            return report;
        }
    };

    let trigger_tags: HashSet<Tag> = trigger_tags.iter().cloned().collect();
    let mut targets = Vec::new();
    for poster in posters {
        if let Some(refusal) = refusal_for(&poster, &trigger_tags) {
            tracing::debug!(
                event = %Event::CandidateSkipped, poster_id = %poster.id,
                detail = %refusal, "poster not eligible for this pool"
            );
            continue;
        }
        match deps.publishers.publisher_for(&poster).await {
            Ok(Some(publisher)) => targets.push((poster, publisher)),
            Ok(None) => {
                tracing::debug!(event = %Event::PosterUnbound, poster_id = %poster.id, "no channel binding; skipping")
            }
            Err(e) => {
                tracing::error!(event = %Event::PosterUnbound, poster_id = %poster.id, error = %e, "publisher construction failed")
            }
        }
    }
    report.channels = targets.len();
    if targets.is_empty() || pages.is_empty() {
        tracing::info!(
            event = %Event::PoolBatchCompleted, channels = report.channels,
            pages = pages.len(), "pool batch had nothing to do"
        );
        return report;
    }

    let chunk_size = if pages.len() <= POOL_BURST_MAX {
        pages.len()
    } else {
        POOL_CHUNK_SIZE
    };
    for (index, chunk) in pages.chunks(chunk_size).enumerate() {
        if index > 0 {
            tokio::time::sleep(pause).await;
        }
        for page in chunk {
            let media = match deps.resolver.resolve(&page.source).await {
                Ok(media) => media,
                Err(domain::elements::media::MediaResolveError::NotFound(_)) => {
                    report.unresolved += 1;
                    tracing::warn!(
                        event = %Event::DeadMediaFound, post_id = %page.id,
                        source = %page.source.as_ref(), "pool page gone upstream → MediaGone"
                    );
                    if let Err(e) = deps
                        .posts
                        .set_status_to(page.id, PostStatus::MediaGone)
                        .await
                    {
                        tracing::error!(event = %Event::MediaResolveFailed, post_id = %page.id, error = %e, "MediaGone write failed");
                    }
                    continue;
                }
                Err(e) => {
                    report.unresolved += 1;
                    tracing::error!(
                        event = %Event::MediaResolveFailed, post_id = %page.id,
                        source = %page.source.as_ref(), error = %e,
                        "pool page resolution failed; page skipped"
                    );
                    continue;
                }
            };
            let mut delivered = false;
            for (poster, publisher) in &targets {
                if let Some(hit) = poster.forbidden_tags.iter().find(|t| page.tags.contains(t)) {
                    report.poster_skips += 1;
                    tracing::debug!(
                        event = %Event::CandidateSkipped, reason = %SkipReason::PosterForbiddenTag,
                        post_id = %page.id, poster_id = %poster.id, tag = %hit,
                        "pool page skipped for this channel only"
                    );
                    continue;
                }
                match publish_resolved(page, media.clone(), poster, &**publisher, deps, Utc::now())
                    .await
                {
                    Ok(receipt) => {
                        delivered = true;
                        tracing::info!(
                            event = %Event::Published, post_id = %page.id,
                            chat_id = receipt.chat_id, "pool page published out-of-band"
                        );
                    }
                    Err(e) => {
                        report.send_failures += 1;
                        tracing::error!(
                            event = %Event::PublishFailed, post_id = %page.id,
                            poster_id = %poster.id, error = %e, "pool page send failed"
                        );
                    }
                }
            }
            if delivered {
                report.published += 1;
            }
        }
    }

    tracing::info!(
        event = %Event::PoolBatchCompleted, pages = pages.len(),
        published = report.published, channels = report.channels,
        unresolved = report.unresolved, send_failures = report.send_failures,
        poster_skips = report.poster_skips, "pool batch completed"
    );
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use domain::elements::{
        cadence::PostInterval,
        media::{MediaResolveError, MediaResolver, ResolvedMedia},
        post::{FeedPick, PostSelectorStrategy, SelectorError, Source},
        poster::Poster,
        publisher::{PublishItem, PublishReceipt, Publisher, PublisherError},
    };
    use persistence::in_memory::{
        post::InMemoryPostRepository, poster::InMemoryPosterRepository,
        publication::InMemoryPublicationRepository, tag_policy::InMemorySpoilerTagRepository,
        user::InMemoryUserRepository,
    };
    use url::Url;

    struct NoSelector;
    impl crate::actors::scheduler::SelectorFactory for NoSelector {
        fn for_poster(&self, _poster: Poster) -> Box<dyn PostSelectorStrategy> {
            Box::new(NeverPick)
        }
    }
    struct NeverPick;
    #[async_trait]
    impl PostSelectorStrategy for NeverPick {
        async fn next_post(&self, cursor: u64) -> Result<FeedPick, SelectorError> {
            Ok(FeedPick {
                post: None,
                advance_to: cursor,
            })
        }
    }

    struct FixedResolver;
    #[async_trait]
    impl MediaResolver for FixedResolver {
        async fn resolve(&self, source: &Source) -> Result<ResolvedMedia, MediaResolveError> {
            let url: &Url = source.as_ref();
            if url.path().ends_with("/404") {
                return Err(MediaResolveError::NotFound(source.clone()));
            }
            Ok(ResolvedMedia::Photo(
                Url::parse("https://static1.e621.net/data/x.png").unwrap(),
            ))
        }
    }

    #[derive(Clone, Default)]
    struct CountingPublisher {
        count: Arc<AtomicUsize>,
        order: Arc<Mutex<Vec<u64>>>,
    }
    #[async_trait]
    impl Publisher for CountingPublisher {
        async fn publish(&self, item: &PublishItem) -> Result<PublishReceipt, PublisherError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            self.order.lock().unwrap().push(*item.post_id.as_ref());
            Ok(PublishReceipt {
                chat_id: -100,
                message_id: 555,
            })
        }
    }
    struct StubPublisherFactory(CountingPublisher);
    #[async_trait]
    impl crate::actors::scheduler::PublisherFactory for StubPublisherFactory {
        async fn publisher_for(
            &self,
            _poster: &Poster,
        ) -> Result<Option<Box<dyn Publisher>>, String> {
            Ok(Some(Box::new(self.0.clone())))
        }
    }

    struct Fixture {
        posts: Arc<InMemoryPostRepository>,
        posters: Arc<InMemoryPosterRepository>,
        publications: Arc<InMemoryPublicationRepository>,
        publisher: CountingPublisher,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                posts: Arc::new(InMemoryPostRepository::new()),
                posters: Arc::new(InMemoryPosterRepository::new()),
                publications: Arc::new(InMemoryPublicationRepository::new()),
                publisher: CountingPublisher::default(),
            }
        }

        fn deps(
            &self,
        ) -> SchedulerDeps<
            InMemoryPostRepository,
            InMemoryUserRepository,
            InMemoryPosterRepository,
            InMemoryPublicationRepository,
            InMemorySpoilerTagRepository,
        > {
            SchedulerDeps {
                posts: self.posts.clone(),
                users: Arc::new(InMemoryUserRepository::new()),
                posters: self.posters.clone(),
                publications: self.publications.clone(),
                spoilers: Arc::new(InMemorySpoilerTagRepository::new()),
                selectors: Arc::new(NoSelector),
                publishers: Arc::new(StubPublisherFactory(self.publisher.clone())),
                resolver: Arc::new(FixedResolver),
                bot_username: "testbot".to_string(),
            }
        }

        async fn poster(&self, subscribed: &[&str], forbidden: &[&str]) {
            use domain::elements::tag_rule::TagTerm;
            self.posters
                .create(
                    subscribed
                        .iter()
                        .map(|t| TagTerm::from(Tag::from(*t)))
                        .collect(),
                    forbidden.iter().map(|t| Tag::from(*t)).collect(),
                    PostInterval::new(5).unwrap(),
                    0,
                )
                .await
                .unwrap();
        }

        async fn page(&self, e621_id: u64, tags: &[&str]) -> Post {
            self.posts
                .create(
                    Source::try_from(
                        Url::parse(&format!("https://e621.net/posts/{e621_id}")).unwrap(),
                    )
                    .unwrap(),
                    tags.iter().map(|t| Tag::from(*t)).collect(),
                    vec![],
                    None,
                    Utc::now(),
                    PostStatus::Accepted,
                )
                .await
                .unwrap()
        }
    }

    fn wolf() -> Vec<Tag> {
        vec![Tag::from("wolf")]
    }

    #[tokio::test]
    async fn publishes_every_page_in_order_to_matching_posters() {
        let fx = Fixture::new();
        fx.poster(&["wolf"], &[]).await;
        let pages = vec![
            fx.page(30, &["wolf"]).await,
            fx.page(10, &["wolf"]).await,
            fx.page(20, &["wolf"]).await,
        ];

        let report = publish_pool(&pages, &wolf(), &fx.deps(), Duration::ZERO).await;

        assert_eq!(report.channels, 1);
        assert_eq!(report.published, 3);
        assert_eq!(report.send_failures, 0);
        let order = fx.publisher.order.lock().unwrap().clone();
        assert_eq!(
            order,
            pages.iter().map(|p| *p.id.as_ref()).collect::<Vec<_>>(),
            "pages must go out in pool order"
        );
        // Publications recorded → channel scoreboards keep counting.
        use domain::elements::publisher::PublicationRepository as _;
        for page in &pages {
            assert_eq!(fx.publications.list_for(page.id).await.unwrap().len(), 1);
        }
        // Cursors untouched: the batch is invisible to the feed walk.
        let poster = &fx.posters.list_all().await.unwrap()[0];
        assert_eq!(poster.cursor, 0);
    }

    #[tokio::test]
    async fn eligibility_follows_the_trigger_not_each_page() {
        let fx = Fixture::new();
        fx.poster(&["wolf"], &[]).await;
        // Page 2 of the comic doesn't carry the subscribed tag — it still goes.
        let pages = vec![
            fx.page(1, &["wolf", "comic"]).await,
            fx.page(2, &["landscape"]).await,
        ];

        let report = publish_pool(&pages, &wolf(), &fx.deps(), Duration::ZERO).await;
        assert_eq!(report.published, 2);

        // A poster whose subscription does NOT match the trigger gets nothing.
        let fx = Fixture::new();
        fx.poster(&["dragon"], &[]).await;
        let pages = vec![fx.page(1, &["wolf"]).await];
        let report = publish_pool(&pages, &wolf(), &fx.deps(), Duration::ZERO).await;
        assert_eq!(report.channels, 0);
        assert_eq!(fx.publisher.count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn poster_forbidden_tags_still_skip_individual_pages() {
        let fx = Fixture::new();
        fx.poster(&["wolf"], &["vore"]).await;
        let pages = vec![
            fx.page(1, &["wolf"]).await,
            fx.page(2, &["wolf", "vore"]).await,
        ];

        let report = publish_pool(&pages, &wolf(), &fx.deps(), Duration::ZERO).await;
        assert_eq!(report.published, 1);
        assert_eq!(report.poster_skips, 1);
    }

    #[tokio::test]
    async fn dead_pages_are_shelved_and_the_batch_continues() {
        let fx = Fixture::new();
        fx.poster(&["wolf"], &[]).await;
        let dead = fx.page(404, &["wolf"]).await;
        let pages = vec![dead.clone(), fx.page(2, &["wolf"]).await];

        let report = publish_pool(&pages, &wolf(), &fx.deps(), Duration::ZERO).await;
        assert_eq!(report.published, 1);
        assert_eq!(report.unresolved, 1);
        let stored = fx.posts.find_by_id(dead.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::MediaGone);
    }

    #[tokio::test]
    async fn big_pools_are_chunked_small_ones_burst() {
        // 12 pages > POOL_BURST_MAX → chunks of 5 (5/5/2), zero pause in tests.
        let fx = Fixture::new();
        fx.poster(&["wolf"], &[]).await;
        let mut pages = Vec::new();
        for id in 1..=12 {
            pages.push(fx.page(id, &["wolf"]).await);
        }
        let report = publish_pool(&pages, &wolf(), &fx.deps(), Duration::ZERO).await;
        assert_eq!(report.published, 12);
        assert_eq!(fx.publisher.count.load(Ordering::SeqCst), 12);
    }
}

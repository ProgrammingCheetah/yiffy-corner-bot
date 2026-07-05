use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use domain::elements::{
    cadence::{PublishBlock, PublishBlockError},
    media::MediaResolver,
    post::{Post, PostRepository, PostSelectorStrategy},
    poster::Poster,
    publisher::{PublishItem, Publisher},
    user::UserRepository,
};
use tokio::time::{MissedTickBehavior, interval};

/// One unit of scheduler work: a [`Poster`] (config) paired with the per-Poster
/// [`PostSelectorStrategy`] that picks its next [`Post`], the [`MediaResolver`]
/// that turns the Post's source into publishable media, and the [`Publisher`]
/// that delivers it.
pub struct PosterRuntime {
    pub poster: Poster,
    pub selector: Box<dyn PostSelectorStrategy>,
    pub resolver: Arc<dyn MediaResolver>,
    pub publisher: Box<dyn Publisher>,
}

pub struct SchedulerDeps<R, U>
where
    R: PostRepository + Send + Sync,
    U: UserRepository,
{
    pub runtimes: Vec<PosterRuntime>,
    pub posts: Arc<R>,
    pub users: Arc<U>,
}

#[derive(Debug, thiserror::Error)]
pub enum TickError {
    #[error("invalid PublishBlock: {0}")]
    Block(#[from] PublishBlockError),
}

/// Build the caption for a Post: attribution first, source link always.
///
/// Posts submitted by a User carry "Submitted by <name>" (falling back to the
/// User's Telegram ID when no display name is cached). Admin-added posts
/// (`submitted_by: None`) get no attribution line.
pub async fn build_caption<U: UserRepository>(post: &Post, users: &U) -> String {
    let source_url = post.source.as_ref().as_str();
    match post.submitted_by {
        None => source_url.to_string(),
        Some(user_id) => {
            let name = match users.find_by_id(user_id).await {
                Ok(Some(user)) => user
                    .display_name
                    .unwrap_or_else(|| format!("user {}", user.telegram_id.as_ref())),
                Ok(None) | Err(_) => format!("user {user_id}"),
            };
            format!("Submitted by {name}\n{source_url}")
        }
    }
}

/// Run one scheduler tick: for every Poster whose interval divides the current
/// minute-of-hour, select a post, resolve its media, publish it with its
/// attribution caption, and record `last_posted`.
///
/// Per-runtime failures are logged and the tick continues with the next
/// runtime — one Poster failing must not block the others.
pub async fn run_tick<R, U>(
    now: DateTime<Utc>,
    runtimes: &[PosterRuntime],
    posts: &R,
    users: &U,
) -> Result<(), TickError>
where
    R: PostRepository + Send + Sync,
    R::Err: std::fmt::Display,
    U: UserRepository,
{
    let block = PublishBlock::try_from(now.minute_of_hour())?;
    for rt in runtimes {
        if !block.fires_for(&rt.poster.time_interval) {
            continue;
        }
        // Queue first (peeked user submissions), then the saved pool.
        let due = match rt.selector.find_due_post().await {
            Ok(due) => due,
            Err(e) => {
                tracing::error!(poster_id = %rt.poster.id, error = %e, "queue peek failed");
                continue;
            }
        };
        let post = match due {
            Some(p) => p,
            None => match rt.selector.find_post().await {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(poster_id = %rt.poster.id, error = %e, "selector failed");
                    continue;
                }
            },
        };
        let media = match rt.resolver.resolve(&post.source).await {
            Ok(m) => m,
            Err(e) => {
                tracing::error!(poster_id = %rt.poster.id, post_id = %post.id, error = %e, "media resolution failed");
                continue;
            }
        };
        let item = PublishItem {
            media,
            caption: Some(build_caption(&post, users).await),
        };
        if let Err(e) = rt.publisher.publish(&item).await {
            tracing::error!(poster_id = %rt.poster.id, post_id = %post.id, error = %e, "publish failed");
            continue;
        }
        if let Err(e) = posts.mark_posted(post.id, now).await {
            tracing::error!(poster_id = %rt.poster.id, error = %e, "mark_posted failed");
            continue;
        }
    }
    Ok(())
}

/// Loop forever, waking every minute to call [`run_tick`].
pub async fn start_scheduler<R, U>(deps: SchedulerDeps<R, U>) -> !
where
    R: PostRepository + Send + Sync + 'static,
    R::Err: std::fmt::Display,
    U: UserRepository + 'static,
{
    let mut ticker = interval(Duration::from_secs(60));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;
        let now = Utc::now();
        if let Err(e) = run_tick(now, &deps.runtimes, &*deps.posts, &*deps.users).await {
            tracing::error!(error = %e, "scheduler tick failed");
        }
    }
}

/// Trait extension so `now.minute_of_hour()` reads as intent in `run_tick`.
trait MinuteOfHour {
    fn minute_of_hour(&self) -> u8;
}
impl MinuteOfHour for DateTime<Utc> {
    fn minute_of_hour(&self) -> u8 {
        use chrono::Timelike;
        self.minute() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use chrono::TimeZone;
    use domain::elements::{
        cadence::PostInterval,
        media::{MediaResolveError, ResolvedMedia},
        post::{Post, PostId, PostRepositoryError, PostStatus, SelectorError, Source},
        poster::PosterId,
        publisher::PublisherError,
        user::UserId,
    };
    use persistence::in_memory::user::InMemoryUserRepository;
    use url::Url;

    fn make_post(id: u64) -> Post {
        Post {
            id: PostId::from(id),
            source: Source::try_from(Url::parse("https://e621.net/posts/1").unwrap()).unwrap(),
            status: PostStatus::Accepted,
            last_posted: None,
            submitted_by: None,
            submitted_at: Utc::now(),
        }
    }

    fn make_poster(id: u64, interval_minutes: u8) -> Poster {
        Poster {
            id: PosterId::from(id),
            subscribed_tags: vec![],
            forbidden_tags: vec![],
            time_interval: PostInterval::new(interval_minutes).unwrap(),
        }
    }

    /// Selector that always returns a fixed Post.
    struct FixedSelector(Post);
    #[async_trait]
    impl PostSelectorStrategy for FixedSelector {
        async fn find_due_post(&self) -> Result<Option<Post>, SelectorError> {
            Ok(Some(self.0.clone()))
        }
        async fn find_post(&self) -> Result<Post, SelectorError> {
            Ok(self.0.clone())
        }
    }

    /// Selector with a distinct queue (due) post and pool post.
    struct TwoPoolSelector {
        due: Option<Post>,
        pool: Post,
    }
    #[async_trait]
    impl PostSelectorStrategy for TwoPoolSelector {
        async fn find_due_post(&self) -> Result<Option<Post>, SelectorError> {
            Ok(self.due.clone())
        }
        async fn find_post(&self) -> Result<Post, SelectorError> {
            Ok(self.pool.clone())
        }
    }

    /// Selector that always errors.
    struct ErroringSelector;
    #[async_trait]
    impl PostSelectorStrategy for ErroringSelector {
        async fn find_due_post(&self) -> Result<Option<Post>, SelectorError> {
            Err(SelectorError::NoMatch)
        }
        async fn find_post(&self) -> Result<Post, SelectorError> {
            Err(SelectorError::NoMatch)
        }
    }

    /// Resolver that maps any source to a fixed photo URL.
    struct FixedResolver;
    #[async_trait]
    impl MediaResolver for FixedResolver {
        async fn resolve(&self, _source: &Source) -> Result<ResolvedMedia, MediaResolveError> {
            Ok(ResolvedMedia::Photo(
                Url::parse("https://static1.e621.net/data/x.png").unwrap(),
            ))
        }
    }

    /// Resolver that always fails.
    struct FailingResolver;
    #[async_trait]
    impl MediaResolver for FailingResolver {
        async fn resolve(&self, source: &Source) -> Result<ResolvedMedia, MediaResolveError> {
            Err(MediaResolveError::NotFound(source.clone()))
        }
    }

    /// Publisher that counts calls and records the last item.
    #[derive(Default)]
    struct CountingPublisher {
        count: Arc<AtomicUsize>,
        last_item: Arc<Mutex<Option<PublishItem>>>,
    }
    #[async_trait]
    impl Publisher for CountingPublisher {
        async fn publish(&self, item: &PublishItem) -> Result<(), PublisherError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            *self.last_item.lock().unwrap() = Some(item.clone());
            Ok(())
        }
    }

    /// PostRepository that only records `mark_posted` calls.
    #[derive(Default)]
    struct RecordingPostRepository {
        marked: Mutex<Vec<(PostId, DateTime<Utc>)>>,
    }
    #[async_trait]
    impl PostRepository for RecordingPostRepository {
        type Err = PostRepositoryError;
        async fn create(
            &self,
            _source: Source,
            _submitted_by: Option<UserId>,
            _submitted_at: DateTime<Utc>,
            _status: PostStatus,
        ) -> Result<Post, Self::Err> {
            unimplemented!("not needed by scheduler tests")
        }
        async fn find_by_id(&self, _id: PostId) -> Result<Option<Post>, Self::Err> {
            unimplemented!("not needed by scheduler tests")
        }
        async fn find_by_source(&self, _source: &Source) -> Result<Option<Post>, Self::Err> {
            unimplemented!("not needed by scheduler tests")
        }
        async fn remove(&self, _id: PostId) -> Result<(), Self::Err> {
            unimplemented!("not needed by scheduler tests")
        }
        async fn set_status_to(&self, _id: PostId, _status: PostStatus) -> Result<(), Self::Err> {
            unimplemented!("not needed by scheduler tests")
        }
        async fn mark_posted(&self, id: PostId, at: DateTime<Utc>) -> Result<(), Self::Err> {
            self.marked.lock().unwrap().push((id, at));
            Ok(())
        }
        async fn list_by_status(&self, _status: PostStatus) -> Result<Vec<Post>, Self::Err> {
            unimplemented!("not needed by scheduler tests")
        }
    }

    fn at_minute(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 17, 14, minute, 0).unwrap()
    }

    fn runtime_with(
        poster: Poster,
        selector: Box<dyn PostSelectorStrategy>,
        publisher: CountingPublisher,
    ) -> PosterRuntime {
        PosterRuntime {
            poster,
            selector,
            resolver: Arc::new(FixedResolver),
            publisher: Box::new(publisher),
        }
    }

    fn counting_publisher() -> (
        CountingPublisher,
        Arc<AtomicUsize>,
        Arc<Mutex<Option<PublishItem>>>,
    ) {
        let publisher = CountingPublisher::default();
        (
            CountingPublisher {
                count: publisher.count.clone(),
                last_item: publisher.last_item.clone(),
            },
            publisher.count,
            publisher.last_item,
        )
    }

    #[tokio::test]
    async fn fires_when_block_matches_interval() {
        let (publisher, count, _) = counting_publisher();
        let runtimes = vec![runtime_with(
            make_poster(1, 5),
            Box::new(FixedSelector(make_post(100))),
            publisher,
        )];
        let posts = RecordingPostRepository::default();
        let users = InMemoryUserRepository::new();
        let now = at_minute(5); // 5 % 5 == 0 → fires

        run_tick(now, &runtimes, &posts, &users).await.unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 1);
        let marked = posts.marked.lock().unwrap();
        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].0, PostId::from(100));
        assert_eq!(marked[0].1, now);
    }

    #[tokio::test]
    async fn does_not_fire_when_block_does_not_match() {
        let (publisher, count, _) = counting_publisher();
        let runtimes = vec![runtime_with(
            make_poster(1, 5),
            Box::new(FixedSelector(make_post(100))),
            publisher,
        )];
        let posts = RecordingPostRepository::default();
        let users = InMemoryUserRepository::new();
        let now = at_minute(7); // 7 % 5 != 0 → no fire

        run_tick(now, &runtimes, &posts, &users).await.unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(posts.marked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn selector_error_does_not_abort_other_runtimes() {
        let (publisher_a, count_a, _) = counting_publisher();
        let (publisher_b, count_b, _) = counting_publisher();
        let runtimes = vec![
            runtime_with(make_poster(1, 5), Box::new(ErroringSelector), publisher_a),
            runtime_with(
                make_poster(2, 5),
                Box::new(FixedSelector(make_post(200))),
                publisher_b,
            ),
        ];
        let posts = RecordingPostRepository::default();
        let users = InMemoryUserRepository::new();
        let now = at_minute(5);

        run_tick(now, &runtimes, &posts, &users).await.unwrap();

        // First runtime's selector failed → no publish.
        // Second runtime fires normally → one publish.
        assert_eq!(count_a.load(Ordering::SeqCst), 0);
        assert_eq!(count_b.load(Ordering::SeqCst), 1);
        assert_eq!(posts.marked.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn resolver_error_skips_publish_and_mark() {
        let (publisher, count, _) = counting_publisher();
        let runtimes = vec![PosterRuntime {
            poster: make_poster(1, 5),
            selector: Box::new(FixedSelector(make_post(100))),
            resolver: Arc::new(FailingResolver),
            publisher: Box::new(publisher),
        }];
        let posts = RecordingPostRepository::default();
        let users = InMemoryUserRepository::new();
        let now = at_minute(5);

        run_tick(now, &runtimes, &posts, &users).await.unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(posts.marked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn two_runtimes_with_different_intervals_both_fire_at_block_zero() {
        let (publisher_a, count_a, _) = counting_publisher();
        let (publisher_b, count_b, _) = counting_publisher();
        let runtimes = vec![
            runtime_with(
                make_poster(1, 5),
                Box::new(FixedSelector(make_post(100))),
                publisher_a,
            ),
            runtime_with(
                make_poster(2, 15),
                Box::new(FixedSelector(make_post(200))),
                publisher_b,
            ),
        ];
        let posts = RecordingPostRepository::default();
        let users = InMemoryUserRepository::new();
        let now = at_minute(0); // divisible by every valid interval

        run_tick(now, &runtimes, &posts, &users).await.unwrap();

        assert_eq!(count_a.load(Ordering::SeqCst), 1);
        assert_eq!(count_b.load(Ordering::SeqCst), 1);
        assert_eq!(posts.marked.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn due_queue_post_is_preferred_over_pool() {
        let (publisher, _, _) = counting_publisher();
        let runtimes = vec![runtime_with(
            make_poster(1, 5),
            Box::new(TwoPoolSelector {
                due: Some(make_post(1)),
                pool: make_post(2),
            }),
            publisher,
        )];
        let posts = RecordingPostRepository::default();
        let users = InMemoryUserRepository::new();

        run_tick(at_minute(5), &runtimes, &posts, &users)
            .await
            .unwrap();

        assert_eq!(posts.marked.lock().unwrap()[0].0, PostId::from(1));
    }

    #[tokio::test]
    async fn empty_queue_falls_back_to_pool() {
        let (publisher, _, _) = counting_publisher();
        let runtimes = vec![runtime_with(
            make_poster(1, 5),
            Box::new(TwoPoolSelector {
                due: None,
                pool: make_post(2),
            }),
            publisher,
        )];
        let posts = RecordingPostRepository::default();
        let users = InMemoryUserRepository::new();

        run_tick(at_minute(5), &runtimes, &posts, &users)
            .await
            .unwrap();

        assert_eq!(posts.marked.lock().unwrap()[0].0, PostId::from(2));
    }

    #[tokio::test]
    async fn published_item_credits_the_submitter() {
        use domain::elements::user::{Role, TelegramId, UserRepository as _};

        let users = InMemoryUserRepository::new();
        let submitter = users
            .create(
                TelegramId::from(42),
                Role::User,
                None,
                Some("Ziel".to_string()),
            )
            .await
            .unwrap();

        let mut post = make_post(100);
        post.submitted_by = Some(submitter.id);

        let (publisher, count, last_item) = counting_publisher();
        let runtimes = vec![runtime_with(
            make_poster(1, 5),
            Box::new(FixedSelector(post)),
            publisher,
        )];
        let posts = RecordingPostRepository::default();
        let now = at_minute(5);

        run_tick(now, &runtimes, &posts, &users).await.unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 1);
        let item = last_item.lock().unwrap().clone().unwrap();
        let caption = item.caption.unwrap();
        assert!(caption.contains("Submitted by Ziel"), "caption: {caption}");
        assert!(caption.contains("e621.net"), "caption: {caption}");
    }

    #[tokio::test]
    async fn admin_added_post_has_no_attribution_line() {
        let (publisher, _, last_item) = counting_publisher();
        let runtimes = vec![runtime_with(
            make_poster(1, 5),
            Box::new(FixedSelector(make_post(100))), // submitted_by: None
            publisher,
        )];
        let posts = RecordingPostRepository::default();
        let users = InMemoryUserRepository::new();
        let now = at_minute(5);

        run_tick(now, &runtimes, &posts, &users).await.unwrap();

        let item = last_item.lock().unwrap().clone().unwrap();
        let caption = item.caption.unwrap();
        assert!(!caption.contains("Submitted by"), "caption: {caption}");
        assert!(caption.contains("e621.net"), "caption: {caption}");
    }
}

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use domain::elements::{
    cadence::{PublishBlock, PublishBlockError},
    media::MediaResolver,
    post::{Post, PostRepository, PostSelectorStrategy},
    poster::{Poster, PosterRepository},
    publisher::{PublishItem, Publisher},
    user::UserRepository,
};
use telemetry::Event;
use tokio::time::{MissedTickBehavior, interval};
use tracing::Instrument;

/// One unit of scheduler work: a [`Poster`] (config) paired with the
/// [`PostSelectorStrategy`] that walks the feed for it, the [`MediaResolver`]
/// that turns a Post's source into publishable media, and the [`Publisher`]
/// that delivers it.
pub struct PosterRuntime {
    pub poster: Poster,
    pub selector: Box<dyn PostSelectorStrategy>,
    pub resolver: Arc<dyn MediaResolver>,
    pub publisher: Box<dyn Publisher>,
}

pub struct SchedulerDeps<R, U, PR>
where
    R: PostRepository + Send + Sync,
    U: UserRepository,
    PR: PosterRepository,
{
    pub runtimes: Vec<PosterRuntime>,
    pub posts: Arc<R>,
    pub users: Arc<U>,
    pub posters: Arc<PR>,
}

#[derive(Debug, thiserror::Error)]
pub enum TickError {
    #[error("invalid PublishBlock: {0}")]
    Block(#[from] PublishBlockError),
}

/// Build the caption for a Post: attribution first, source reference last.
///
/// Posts submitted by a User carry "Submitted by <name>" (falling back to the
/// User's Telegram ID when no display name is cached). Admin-added posts
/// (`submitted_by: None`) get no attribution line.
///
/// The source reference is the source URL — except channel-forward
/// submissions (`t.me/<channel>/<msg>`), which are published as *copies* and
/// therefore tag their origin as "Forwarded from channel: @<channel>" at the
/// bottom instead of a bare link.
pub async fn build_caption<U: UserRepository>(post: &Post, users: &U) -> String {
    let source_line = match post.source.telegram_channel() {
        Some(channel) => format!("Forwarded from channel: @{channel}"),
        None => post.source.as_ref().as_str().to_string(),
    };
    match post.submitted_by {
        None => source_line,
        Some(user_id) => {
            let name = match users.find_by_id(user_id).await {
                Ok(Some(user)) => user
                    .display_name
                    .unwrap_or_else(|| format!("user {}", user.telegram_id.as_ref())),
                Ok(None) | Err(_) => format!("user {user_id}"),
            };
            format!("Submitted by {name}\n{source_line}")
        }
    }
}

/// Run one scheduler tick: every Poster whose interval divides the current
/// minute-of-hour walks the feed from its cursor and publishes the first
/// matching entry (if any).
///
/// Per-runtime failures are logged and the tick continues with the next
/// runtime — one Poster failing must not block the others.
pub async fn run_tick<R, U, PR>(
    now: DateTime<Utc>,
    runtimes: &[PosterRuntime],
    posts: &R,
    users: &U,
    posters: &PR,
) -> Result<(), TickError>
where
    R: PostRepository + Send + Sync,
    R::Err: std::fmt::Display,
    U: UserRepository,
    PR: PosterRepository,
    PR::Err: std::fmt::Display,
{
    let block = PublishBlock::try_from(now.minute_of_hour())?;
    for rt in runtimes {
        if !block.fires_for(&rt.poster.time_interval) {
            continue;
        }
        let span = tracing::info_span!(
            "poster_fire",
            poster_id = %rt.poster.id,
            minute = now.minute_of_hour(),
        );
        fire_one(rt, posts, users, posters, now)
            .instrument(span)
            .await;
    }
    Ok(())
}

/// One consumer's full fire pipeline: read cursor → scan feed → resolve →
/// caption → publish → mark + advance cursor. The cursor only advances after
/// a successful publish (or a clean empty scan), so failures retry the same
/// entry next tick. Every exit path logs; failures never propagate.
async fn fire_one<R, U, PR>(
    rt: &PosterRuntime,
    posts: &R,
    users: &U,
    posters: &PR,
    now: DateTime<Utc>,
) where
    R: PostRepository + Send + Sync,
    R::Err: std::fmt::Display,
    U: UserRepository,
    PR: PosterRepository,
    PR::Err: std::fmt::Display,
{
    // Cursor is state, not config: read fresh every fire.
    let cursor = match posters.find_by_id(rt.poster.id).await {
        Ok(Some(current)) => current.cursor,
        Ok(None) => {
            tracing::error!(event = %Event::SelectorFailed, "poster vanished from the repository");
            return;
        }
        Err(e) => {
            tracing::error!(event = %Event::SelectorFailed, error = %e, "cursor read failed");
            return;
        }
    };
    tracing::debug!(
        event = %Event::PosterFired,
        interval_min = rt.poster.time_interval.as_ref(),
        cursor,
        "poster fires this tick"
    );

    let pick = match rt.selector.next_post(cursor).await {
        Ok(pick) => pick,
        Err(e) => {
            tracing::error!(event = %Event::SelectorFailed, error = %e, "feed scan failed; cursor kept");
            return;
        }
    };

    let Some(post) = pick.post else {
        if pick.advance_to != cursor {
            if let Err(e) = posters.set_cursor(rt.poster.id, pick.advance_to).await {
                tracing::error!(event = %Event::SelectorFailed, error = %e, "cursor write failed");
                return;
            }
            tracing::debug!(
                event = %Event::CursorAdvanced, from = cursor, to = pick.advance_to,
                "no match; cursor advanced past scanned window"
            );
        } else {
            tracing::debug!(event = %Event::FeedEndReached, cursor, "feed exhausted; staying quiet");
        }
        return;
    };

    tracing::info!(
        event = %Event::PostSelected,
        post_id = %post.id,
        source = %post.source.as_ref(),
        position = pick.advance_to,
        "feed entry selected"
    );
    let media = match rt.resolver.resolve(&post.source).await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(
                event = %Event::MediaResolveFailed, post_id = %post.id,
                source = %post.source.as_ref(), error = %e,
                "media resolution failed; cursor kept for retry"
            );
            return;
        }
    };
    tracing::debug!(event = %Event::MediaResolved, post_id = %post.id, media = ?media, "media resolved");
    let item = PublishItem {
        media,
        caption: Some(build_caption(&post, users).await),
    };
    if let Err(e) = rt.publisher.publish(&item).await {
        tracing::error!(
            event = %Event::PublishFailed, post_id = %post.id, error = %e,
            "publish failed; cursor kept for retry"
        );
        return;
    }
    if let Err(e) = posts.mark_posted(post.id, now).await {
        tracing::error!(event = %Event::MarkPostedFailed, post_id = %post.id, error = %e, "mark_posted failed");
    }
    if let Err(e) = posters.set_cursor(rt.poster.id, pick.advance_to).await {
        // Publish succeeded but the cursor didn't move: the entry may repeat.
        tracing::error!(
            event = %Event::MarkPostedFailed, post_id = %post.id, error = %e,
            "cursor write failed AFTER publish — entry may repeat"
        );
        return;
    }
    tracing::info!(
        event = %Event::Published, post_id = %post.id,
        cursor = pick.advance_to, "published and cursor advanced"
    );
}

/// Loop forever, waking every minute to call [`run_tick`].
pub async fn start_scheduler<R, U, PR>(deps: SchedulerDeps<R, U, PR>) -> !
where
    R: PostRepository + Send + Sync + 'static,
    R::Err: std::fmt::Display,
    U: UserRepository + 'static,
    PR: PosterRepository + 'static,
    PR::Err: std::fmt::Display,
{
    let mut ticker = interval(Duration::from_secs(60));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;
        let now = Utc::now();
        if let Err(e) = run_tick(
            now,
            &deps.runtimes,
            &*deps.posts,
            &*deps.users,
            &*deps.posters,
        )
        .await
        {
            tracing::error!(event = %Event::TickFailed, error = %e, "scheduler tick failed");
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
        post::{FeedPick, Post, PostId, PostStatus, SelectorError, Source},
        publisher::PublisherError,
    };
    use persistence::in_memory::{poster::InMemoryPosterRepository, user::InMemoryUserRepository};
    use url::Url;

    fn make_post(id: u64, position: u64) -> Post {
        Post {
            id: PostId::from(id),
            source: Source::try_from(Url::parse("https://e621.net/posts/1").unwrap()).unwrap(),
            status: PostStatus::Accepted,
            tags: vec![],
            feed_position: Some(position),
            last_posted: None,
            submitted_by: None,
            submitted_at: Utc::now(),
        }
    }

    /// Selector returning a fixed pick.
    struct FixedSelector(FeedPick);
    #[async_trait]
    impl PostSelectorStrategy for FixedSelector {
        async fn next_post(&self, _cursor: u64) -> Result<FeedPick, SelectorError> {
            Ok(self.0.clone())
        }
    }

    /// Selector that always errors.
    struct ErroringSelector;
    #[async_trait]
    impl PostSelectorStrategy for ErroringSelector {
        async fn next_post(&self, _cursor: u64) -> Result<FeedPick, SelectorError> {
            Err(SelectorError::Fetch("e621 down".into()))
        }
    }

    struct FixedResolver;
    #[async_trait]
    impl MediaResolver for FixedResolver {
        async fn resolve(&self, _source: &Source) -> Result<ResolvedMedia, MediaResolveError> {
            Ok(ResolvedMedia::Photo(
                Url::parse("https://static1.e621.net/data/x.png").unwrap(),
            ))
        }
    }

    #[derive(Default)]
    struct CountingPublisher {
        count: Arc<AtomicUsize>,
        fail: bool,
        last_item: Arc<Mutex<Option<PublishItem>>>,
    }
    #[async_trait]
    impl Publisher for CountingPublisher {
        async fn publish(&self, item: &PublishItem) -> Result<(), PublisherError> {
            if self.fail {
                return Err(PublisherError::Send("telegram down".into()));
            }
            self.count.fetch_add(1, Ordering::SeqCst);
            *self.last_item.lock().unwrap() = Some(item.clone());
            Ok(())
        }
    }

    /// PostRepository that only records `mark_posted` calls.
    #[derive(Default)]
    struct RecordingPostRepository {
        marked: Mutex<Vec<PostId>>,
    }
    #[async_trait]
    impl PostRepository for RecordingPostRepository {
        type Err = domain::elements::post::PostRepositoryError;
        async fn create(
            &self,
            _source: Source,
            _tags: Vec<domain::elements::tag::Tag>,
            _submitted_by: Option<domain::elements::user::UserId>,
            _submitted_at: DateTime<Utc>,
            _status: PostStatus,
        ) -> Result<Post, Self::Err> {
            unimplemented!()
        }
        async fn find_by_id(&self, _id: PostId) -> Result<Option<Post>, Self::Err> {
            unimplemented!()
        }
        async fn find_by_source(&self, _source: &Source) -> Result<Option<Post>, Self::Err> {
            unimplemented!()
        }
        async fn remove(&self, _id: PostId) -> Result<(), Self::Err> {
            unimplemented!()
        }
        async fn set_status_to(&self, _id: PostId, _status: PostStatus) -> Result<(), Self::Err> {
            unimplemented!()
        }
        async fn mark_posted(&self, id: PostId, _at: DateTime<Utc>) -> Result<(), Self::Err> {
            self.marked.lock().unwrap().push(id);
            Ok(())
        }
        async fn list_by_status(&self, _status: PostStatus) -> Result<Vec<Post>, Self::Err> {
            unimplemented!()
        }
        async fn accept_into_feed(&self, _id: PostId) -> Result<Post, Self::Err> {
            unimplemented!()
        }
        async fn feed_end(&self) -> Result<u64, Self::Err> {
            unimplemented!()
        }
        async fn feed_after(&self, _cursor: u64, _up_to: u64) -> Result<Vec<Post>, Self::Err> {
            unimplemented!()
        }
    }

    fn at_minute(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 5, 14, minute, 0).unwrap()
    }

    struct Fixture {
        posters: Arc<InMemoryPosterRepository>,
        users: InMemoryUserRepository,
        posts: RecordingPostRepository,
    }

    impl Fixture {
        async fn new() -> Self {
            Self {
                posters: Arc::new(InMemoryPosterRepository::new()),
                users: InMemoryUserRepository::new(),
                posts: RecordingPostRepository::default(),
            }
        }

        /// Register a poster (interval 5) and build a runtime around the
        /// given selector/publisher.
        async fn runtime(
            &self,
            selector: Box<dyn PostSelectorStrategy>,
            publisher: CountingPublisher,
        ) -> (PosterRuntime, domain::elements::poster::PosterId) {
            let poster = self
                .posters
                .create(vec![], vec![], PostInterval::new(5).unwrap())
                .await
                .unwrap();
            let id = poster.id;
            (
                PosterRuntime {
                    poster,
                    selector,
                    resolver: Arc::new(FixedResolver),
                    publisher: Box::new(publisher),
                },
                id,
            )
        }

        async fn cursor_of(&self, id: domain::elements::poster::PosterId) -> u64 {
            self.posters.find_by_id(id).await.unwrap().unwrap().cursor
        }
    }

    fn publisher() -> (
        CountingPublisher,
        Arc<AtomicUsize>,
        Arc<Mutex<Option<PublishItem>>>,
    ) {
        let p = CountingPublisher::default();
        (
            CountingPublisher {
                count: p.count.clone(),
                fail: false,
                last_item: p.last_item.clone(),
            },
            p.count,
            p.last_item,
        )
    }

    #[tokio::test]
    async fn publishes_match_and_advances_cursor() {
        let fx = Fixture::new().await;
        let (pub_ok, count, _) = publisher();
        let (rt, id) = fx
            .runtime(
                Box::new(FixedSelector(FeedPick {
                    post: Some(make_post(100, 7)),
                    advance_to: 7,
                })),
                pub_ok,
            )
            .await;

        run_tick(at_minute(5), &[rt], &fx.posts, &fx.users, &*fx.posters)
            .await
            .unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(fx.cursor_of(id).await, 7);
        assert_eq!(*fx.posts.marked.lock().unwrap(), vec![PostId::from(100)]);
    }

    #[tokio::test]
    async fn does_not_fire_off_interval() {
        let fx = Fixture::new().await;
        let (pub_ok, count, _) = publisher();
        let (rt, id) = fx
            .runtime(
                Box::new(FixedSelector(FeedPick {
                    post: Some(make_post(100, 7)),
                    advance_to: 7,
                })),
                pub_ok,
            )
            .await;

        run_tick(at_minute(7), &[rt], &fx.posts, &fx.users, &*fx.posters)
            .await
            .unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert_eq!(fx.cursor_of(id).await, 0);
    }

    #[tokio::test]
    async fn empty_scan_advances_cursor_without_publish() {
        let fx = Fixture::new().await;
        let (pub_ok, count, _) = publisher();
        let (rt, id) = fx
            .runtime(
                Box::new(FixedSelector(FeedPick {
                    post: None,
                    advance_to: 9,
                })),
                pub_ok,
            )
            .await;

        run_tick(at_minute(5), &[rt], &fx.posts, &fx.users, &*fx.posters)
            .await
            .unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert_eq!(fx.cursor_of(id).await, 9);
    }

    #[tokio::test]
    async fn publish_failure_keeps_cursor_for_retry() {
        let fx = Fixture::new().await;
        let (base, count, last) = publisher();
        let failing = CountingPublisher {
            count: base.count.clone(),
            fail: true,
            last_item: last,
        };
        let (rt, id) = fx
            .runtime(
                Box::new(FixedSelector(FeedPick {
                    post: Some(make_post(100, 7)),
                    advance_to: 7,
                })),
                failing,
            )
            .await;

        run_tick(at_minute(5), &[rt], &fx.posts, &fx.users, &*fx.posters)
            .await
            .unwrap();

        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert_eq!(fx.cursor_of(id).await, 0); // retry next tick
        assert!(fx.posts.marked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn selector_error_keeps_cursor_and_other_runtimes_continue() {
        let fx = Fixture::new().await;
        let (pub_a, count_a, _) = publisher();
        let (pub_b, count_b, _) = publisher();
        let (rt_err, id_err) = fx.runtime(Box::new(ErroringSelector), pub_a).await;
        let (rt_ok, _) = fx
            .runtime(
                Box::new(FixedSelector(FeedPick {
                    post: Some(make_post(200, 3)),
                    advance_to: 3,
                })),
                pub_b,
            )
            .await;

        run_tick(
            at_minute(5),
            &[rt_err, rt_ok],
            &fx.posts,
            &fx.users,
            &*fx.posters,
        )
        .await
        .unwrap();

        assert_eq!(count_a.load(Ordering::SeqCst), 0);
        assert_eq!(count_b.load(Ordering::SeqCst), 1);
        assert_eq!(fx.cursor_of(id_err).await, 0);
    }

    #[tokio::test]
    async fn published_item_credits_the_submitter() {
        use domain::elements::user::{Role, TelegramId, UserRepository as _};

        let fx = Fixture::new().await;
        let submitter = fx
            .users
            .create(
                TelegramId::from(42),
                Role::User,
                None,
                Some("Ziel".to_string()),
            )
            .await
            .unwrap();
        let mut post = make_post(100, 7);
        post.submitted_by = Some(submitter.id);

        let (pub_ok, _, last_item) = publisher();
        let (rt, _) = fx
            .runtime(
                Box::new(FixedSelector(FeedPick {
                    post: Some(post),
                    advance_to: 7,
                })),
                pub_ok,
            )
            .await;

        run_tick(at_minute(5), &[rt], &fx.posts, &fx.users, &*fx.posters)
            .await
            .unwrap();

        let item = last_item.lock().unwrap().clone().unwrap();
        let caption = item.caption.unwrap();
        assert!(caption.contains("Submitted by Ziel"), "caption: {caption}");
    }

    #[tokio::test]
    async fn telegram_forward_caption_credits_the_channel_not_the_url() {
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

        let mut post = make_post(100, 1);
        post.source = Source::try_from(Url::parse("https://t.me/somechannel/42").unwrap()).unwrap();
        post.submitted_by = Some(submitter.id);

        let caption = build_caption(&post, &users).await;
        assert_eq!(
            caption,
            "Submitted by Ziel\nForwarded from channel: @somechannel"
        );
    }

    #[tokio::test]
    async fn admin_added_post_has_no_attribution_line() {
        let users = InMemoryUserRepository::new();
        let caption = build_caption(&make_post(100, 1), &users).await;
        assert!(!caption.contains("Submitted by"), "caption: {caption}");
        assert!(caption.contains("e621.net"), "caption: {caption}");
    }
}

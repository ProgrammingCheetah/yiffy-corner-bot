use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use domain::elements::{
    cadence::{PublishBlock, PublishBlockError},
    media::MediaResolver,
    post::{Post, PostRepository, PostSelectorStrategy},
    poster::{Poster, PosterRepository},
    publisher::{Publication, PublicationRepository, PublishItem, Publisher},
    tag::Tag,
    tag_policy::SpoilerTagRepository,
    user::UserRepository,
};
use telemetry::Event;
use tokio::time::{MissedTickBehavior, interval};
use tracing::Instrument;

/// Builds the feed-walking selector for one consumer. Called EVERY fire with
/// the poster's config as read from the database that tick — this is what
/// makes `/settags` live without a restart.
pub trait SelectorFactory: Send + Sync {
    fn for_poster(&self, poster: Poster) -> Box<dyn PostSelectorStrategy>;
}

/// Resolves a Poster's delivery destination at fire time (database-first:
/// `/setchannel` binds take effect on the next tick). `Ok(None)` means the
/// poster has no channel binding yet.
#[async_trait::async_trait]
pub trait PublisherFactory: Send + Sync {
    async fn publisher_for(&self, poster: &Poster) -> Result<Option<Box<dyn Publisher>>, String>;
}

pub struct SchedulerDeps<R, U, PR, PB, ST>
where
    R: PostRepository + Send + Sync,
    U: UserRepository,
    PR: PosterRepository,
    PB: PublicationRepository,
    ST: SpoilerTagRepository,
{
    pub posts: Arc<R>,
    pub users: Arc<U>,
    pub posters: Arc<PR>,
    pub publications: Arc<PB>,
    pub spoilers: Arc<ST>,
    pub selectors: Arc<dyn SelectorFactory>,
    pub publishers: Arc<dyn PublisherFactory>,
    pub resolver: Arc<dyn MediaResolver>,
    /// The bot's @username (without `@`), for the Report deep link.
    pub bot_username: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TickError {
    #[error("invalid PublishBlock: {0}")]
    Block(#[from] PublishBlockError),
    #[error("poster listing failed: {0}")]
    Posters(String),
}

/// Whether these tags demand the spoiler blur: any listed content-warning
/// tag, or the hardcoded `cw` convention (`cw`, `cw_*`, `cw:*`).
pub fn needs_spoiler(tags: &[Tag], spoiler_tags: &[Tag]) -> bool {
    tags.iter().any(|tag| {
        let name = tag.as_ref().to_ascii_lowercase();
        name == "cw"
            || name.starts_with("cw_")
            || name.starts_with("cw:")
            || spoiler_tags.contains(tag)
    })
}

/// Fixed-size publication code shown at the top of every published caption:
/// 8 base-36 chars derived (FNV-1a) from the source URL and the consuming
/// Poster's id. Deterministic, so the same (post, channel) pair always shows
/// the same code — a stable handle for humans talking about a publication.
pub fn publish_code(post: &Post, poster_id: domain::elements::poster::PosterId) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001b3;
    let mut hash = FNV_OFFSET;
    for byte in post
        .source
        .as_ref()
        .as_str()
        .bytes()
        .chain(poster_id.to_string().bytes())
    {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut code = String::with_capacity(8);
    for _ in 0..8 {
        code.push(ALPHABET[(hash % 36) as usize] as char);
        hash /= 36;
    }
    code
}

/// Minimal HTML escaping for user-controlled text in captions.
fn escape_html(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Build the HTML caption for a publication:
///
/// ```text
/// #<8-char code>                       (source id × poster id, fixed size)
/// Submitted by <name>                  (user submissions only)
/// Forwarded from channel: @chan        (channel forwards only)
/// Source · Report                      (hyperlinks)
/// ```
///
/// "Report" is a deep link (`t.me/<bot>?start=report_<post id>`) — no bulky
/// inline button on the message.
pub async fn build_caption<U: UserRepository>(
    post: &Post,
    poster_id: domain::elements::poster::PosterId,
    users: &U,
    bot_username: &str,
    media: &domain::elements::media::ResolvedMedia,
) -> String {
    use domain::elements::media::ResolvedMedia;

    let mut header = format!("<code>#{}</code>", publish_code(post, poster_id));
    if matches!(media, ResolvedMedia::Video(_) | ResolvedMedia::Animation(_)) {
        header.push_str(" #video");
    }
    let mut lines = vec![header];

    if !post.artists.is_empty() {
        let names = post
            .artists
            .iter()
            .map(|a| escape_html(a.as_ref()))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("By {names}"));
    }

    if let Some(user_id) = post.submitted_by {
        let name = match users.find_by_id(user_id).await {
            Ok(Some(user)) => user
                .display_name
                .unwrap_or_else(|| format!("user {}", user.telegram_id.as_ref())),
            Ok(None) | Err(_) => format!("user {user_id}"),
        };
        lines.push(format!("Submitted by {}", escape_html(&name)));
    }
    if let Some(channel) = post.source.telegram_channel() {
        lines.push(format!("Forwarded from channel: @{}", escape_html(channel)));
    }
    lines.push(format!(
        "<a href=\"{}\">Source</a> · <a href=\"https://t.me/{bot_username}?start=report_{}\">Report</a>",
        post.source.as_ref(),
        post.id
    ));
    lines.join("\n")
}

/// Run one scheduler tick, DATABASE-FIRST: posters (config, tags, cursor)
/// are read fresh, so `/newposter`, `/settags` and `/setchannel` are live
/// within a minute — no restarts. Every Poster whose interval divides the
/// current minute-of-hour walks the feed from its cursor and publishes the
/// first matching entry.
///
/// Per-poster failures are logged and the tick continues with the next
/// poster — one failing must not block the others.
pub async fn run_tick<R, U, PR, PB, ST>(
    now: DateTime<Utc>,
    deps: &SchedulerDeps<R, U, PR, PB, ST>,
) -> Result<(), TickError>
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
    let block = PublishBlock::try_from(now.minute_of_hour())?;
    let posters = deps
        .posters
        .list_all()
        .await
        .map_err(|e| TickError::Posters(e.to_string()))?;
    for poster in posters {
        if !block.fires_for(&poster.time_interval) {
            continue;
        }
        let span = tracing::info_span!(
            "poster_fire",
            poster_id = %poster.id,
            minute = now.minute_of_hour(),
        );
        fire_one(poster, deps, now).instrument(span).await;
    }
    Ok(())
}

/// One consumer's full fire pipeline: resolve destination → scan feed →
/// resolve media → caption → publish → record publication + advance cursor.
/// The cursor only advances after a successful publish (or a clean empty
/// scan), so failures retry the same entry next tick.
async fn fire_one<R, U, PR, PB, ST>(
    poster: Poster,
    deps: &SchedulerDeps<R, U, PR, PB, ST>,
    now: DateTime<Utc>,
) where
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
    let cursor = poster.cursor;
    tracing::debug!(
        event = %Event::PosterFired,
        interval_min = poster.time_interval.as_ref(),
        cursor,
        "poster fires this tick"
    );

    let publisher = match deps.publishers.publisher_for(&poster).await {
        Ok(Some(publisher)) => publisher,
        Ok(None) => {
            tracing::debug!(event = %Event::PosterUnbound, "no channel binding; skipping");
            return;
        }
        Err(e) => {
            tracing::error!(event = %Event::PosterUnbound, error = %e, "publisher construction failed");
            return;
        }
    };
    let selector = deps.selectors.for_poster(poster.clone());

    let pick = match selector.next_post(cursor).await {
        Ok(pick) => pick,
        Err(e) => {
            tracing::error!(event = %Event::SelectorFailed, error = %e, "feed scan failed; cursor kept");
            return;
        }
    };

    let Some(post) = pick.post else {
        if pick.advance_to != cursor {
            if let Err(e) = deps.posters.set_cursor(poster.id, pick.advance_to).await {
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
    let media = match deps.resolver.resolve(&post.source).await {
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
    let caption = build_caption(&post, poster.id, &*deps.users, &deps.bot_username, &media).await;
    let spoiler = match deps.spoilers.list_all().await {
        Ok(spoiler_tags) => needs_spoiler(&post.tags, &spoiler_tags),
        Err(e) => {
            tracing::warn!(post_id = %post.id, error = %e, "spoiler list read failed; publishing unspoilered");
            false
        }
    };
    let item = PublishItem {
        post_id: post.id,
        media,
        caption: Some(caption),
        spoiler,
    };
    let receipt = match publisher.publish(&item).await {
        Ok(receipt) => receipt,
        Err(e) => {
            tracing::error!(
                event = %Event::PublishFailed, post_id = %post.id, error = %e,
                "publish failed; cursor kept for retry"
            );
            return;
        }
    };
    if let Err(e) = deps
        .publications
        .record(Publication {
            post_id: post.id,
            chat_id: receipt.chat_id,
            message_id: receipt.message_id,
            published_at: now,
        })
        .await
    {
        // Takedowns for this delivery won't find it; publish itself is done.
        tracing::error!(event = %Event::PublicationRecordFailed, post_id = %post.id, error = %e, "publication record failed");
    }
    if let Err(e) = deps.posts.mark_posted(post.id, now).await {
        tracing::error!(event = %Event::MarkPostedFailed, post_id = %post.id, error = %e, "mark_posted failed");
    }
    if let Err(e) = deps.posters.set_cursor(poster.id, pick.advance_to).await {
        // Publish succeeded but the cursor didn't move: the entry may repeat.
        tracing::error!(
            event = %Event::MarkPostedFailed, post_id = %post.id, error = %e,
            "cursor write failed AFTER publish — entry may repeat"
        );
        return;
    }
    tracing::info!(
        event = %Event::Published, post_id = %post.id,
        chat_id = receipt.chat_id, cursor = pick.advance_to,
        "published and cursor advanced"
    );
}

/// Loop forever, waking every minute to call [`run_tick`].
pub async fn start_scheduler<R, U, PR, PB, ST>(deps: SchedulerDeps<R, U, PR, PB, ST>) -> !
where
    R: PostRepository + Send + Sync + 'static,
    R::Err: std::fmt::Display,
    U: UserRepository + 'static,
    PR: PosterRepository + 'static,
    PR::Err: std::fmt::Display,
    PB: PublicationRepository + 'static,
    PB::Err: std::fmt::Display,
    ST: SpoilerTagRepository + 'static,
    ST::Err: std::fmt::Display,
{
    let mut ticker = interval(Duration::from_secs(60));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        ticker.tick().await;
        let now = Utc::now();
        if let Err(e) = run_tick(now, &deps).await {
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
        publisher::{PublishReceipt, PublisherError},
    };
    use persistence::in_memory::{
        poster::InMemoryPosterRepository, publication::InMemoryPublicationRepository,
        user::InMemoryUserRepository,
    };
    use url::Url;

    fn make_post(id: u64, position: u64) -> Post {
        Post {
            id: PostId::from(id),
            source: Source::try_from(Url::parse("https://e621.net/posts/1").unwrap()).unwrap(),
            status: PostStatus::Accepted,
            tags: vec![],
            artists: vec![],
            feed_position: Some(position),
            last_posted: None,
            submitted_by: None,
            submitted_at: Utc::now(),
            moderated_by: None,
            moderated_at: None,
        }
    }

    /// Factory handing out clones of a fixed pick.
    struct FixedSelectorFactory(FeedPick);
    impl SelectorFactory for FixedSelectorFactory {
        fn for_poster(&self, _poster: Poster) -> Box<dyn PostSelectorStrategy> {
            Box::new(FixedSelector(self.0.clone()))
        }
    }
    struct FixedSelector(FeedPick);
    #[async_trait]
    impl PostSelectorStrategy for FixedSelector {
        async fn next_post(&self, _cursor: u64) -> Result<FeedPick, SelectorError> {
            Ok(self.0.clone())
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

    #[derive(Clone, Default)]
    struct CountingPublisher {
        count: Arc<AtomicUsize>,
        fail: bool,
        last_item: Arc<Mutex<Option<PublishItem>>>,
    }
    #[async_trait]
    impl Publisher for CountingPublisher {
        async fn publish(&self, item: &PublishItem) -> Result<PublishReceipt, PublisherError> {
            if self.fail {
                return Err(PublisherError::Send("telegram down".into()));
            }
            self.count.fetch_add(1, Ordering::SeqCst);
            *self.last_item.lock().unwrap() = Some(item.clone());
            Ok(PublishReceipt {
                chat_id: -100,
                message_id: 555,
            })
        }
    }

    /// Factory: bound posters get the shared CountingPublisher; unbound none.
    struct StubPublisherFactory {
        publisher: CountingPublisher,
        bound: bool,
    }
    #[async_trait]
    impl PublisherFactory for StubPublisherFactory {
        async fn publisher_for(
            &self,
            _poster: &Poster,
        ) -> Result<Option<Box<dyn Publisher>>, String> {
            Ok(self
                .bound
                .then(|| Box::new(self.publisher.clone()) as Box<dyn Publisher>))
        }
    }

    /// PostRepository stub recording mark_posted.
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
            _artists: Vec<domain::elements::tag::Tag>,
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
        async fn set_tags(
            &self,
            _id: PostId,
            _tags: Vec<domain::elements::tag::Tag>,
        ) -> Result<Post, Self::Err> {
            unimplemented!()
        }
        async fn record_moderation(
            &self,
            _id: PostId,
            _by: domain::elements::user::UserId,
            _at: DateTime<Utc>,
        ) -> Result<(), Self::Err> {
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
        publisher: CountingPublisher,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                posters: Arc::new(InMemoryPosterRepository::new()),
                publisher: CountingPublisher::default(),
            }
        }

        async fn poster(&self) -> domain::elements::poster::PosterId {
            self.posters
                .create(vec![], vec![], PostInterval::new(5).unwrap())
                .await
                .unwrap()
                .id
        }

        fn deps(
            &self,
            pick: FeedPick,
            bound: bool,
        ) -> SchedulerDeps<
            RecordingPostRepository,
            InMemoryUserRepository,
            InMemoryPosterRepository,
            InMemoryPublicationRepository,
            persistence::in_memory::tag_policy::InMemorySpoilerTagRepository,
        > {
            SchedulerDeps {
                posts: Arc::new(RecordingPostRepository::default()),
                users: Arc::new(InMemoryUserRepository::new()),
                posters: self.posters.clone(),
                publications: Arc::new(InMemoryPublicationRepository::new()),
                spoilers: Arc::new(
                    persistence::in_memory::tag_policy::InMemorySpoilerTagRepository::new(),
                ),
                selectors: Arc::new(FixedSelectorFactory(pick)),
                publishers: Arc::new(StubPublisherFactory {
                    publisher: self.publisher.clone(),
                    bound,
                }),
                resolver: Arc::new(FixedResolver),
                bot_username: "testbot".to_string(),
            }
        }

        async fn cursor_of(&self, id: domain::elements::poster::PosterId) -> u64 {
            self.posters.find_by_id(id).await.unwrap().unwrap().cursor
        }
    }

    #[tokio::test]
    async fn publishes_match_records_publication_and_advances_cursor() {
        let fx = Fixture::new();
        let id = fx.poster().await;
        let deps = fx.deps(
            FeedPick {
                post: Some(make_post(100, 7)),
                advance_to: 7,
            },
            true,
        );

        run_tick(at_minute(5), &deps).await.unwrap();

        assert_eq!(fx.publisher.count.load(Ordering::SeqCst), 1);
        assert_eq!(fx.cursor_of(id).await, 7);
        assert_eq!(*deps.posts.marked.lock().unwrap(), vec![PostId::from(100)]);
        use domain::elements::publisher::PublicationRepository as _;
        let recorded = deps.publications.list_for(PostId::from(100)).await.unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].chat_id, -100);
        // The publish item carried the post id (for the Report button).
        let item = fx.publisher.last_item.lock().unwrap().clone().unwrap();
        assert_eq!(item.post_id, PostId::from(100));
    }

    #[tokio::test]
    async fn does_not_fire_off_interval() {
        let fx = Fixture::new();
        let id = fx.poster().await;
        let deps = fx.deps(
            FeedPick {
                post: Some(make_post(100, 7)),
                advance_to: 7,
            },
            true,
        );

        run_tick(at_minute(7), &deps).await.unwrap();

        assert_eq!(fx.publisher.count.load(Ordering::SeqCst), 0);
        assert_eq!(fx.cursor_of(id).await, 0);
    }

    #[tokio::test]
    async fn unbound_poster_is_skipped_quietly() {
        let fx = Fixture::new();
        let id = fx.poster().await;
        let deps = fx.deps(
            FeedPick {
                post: Some(make_post(100, 7)),
                advance_to: 7,
            },
            false, // no channel binding
        );

        run_tick(at_minute(5), &deps).await.unwrap();

        assert_eq!(fx.publisher.count.load(Ordering::SeqCst), 0);
        assert_eq!(fx.cursor_of(id).await, 0); // cursor untouched
    }

    #[tokio::test]
    async fn empty_scan_advances_cursor_without_publish() {
        let fx = Fixture::new();
        let id = fx.poster().await;
        let deps = fx.deps(
            FeedPick {
                post: None,
                advance_to: 9,
            },
            true,
        );

        run_tick(at_minute(5), &deps).await.unwrap();

        assert_eq!(fx.publisher.count.load(Ordering::SeqCst), 0);
        assert_eq!(fx.cursor_of(id).await, 9);
    }

    #[tokio::test]
    async fn publish_failure_keeps_cursor_for_retry() {
        let fx = Fixture::new();
        let id = fx.poster().await;
        let mut deps = fx.deps(
            FeedPick {
                post: Some(make_post(100, 7)),
                advance_to: 7,
            },
            true,
        );
        deps.publishers = Arc::new(StubPublisherFactory {
            publisher: CountingPublisher {
                count: fx.publisher.count.clone(),
                fail: true,
                last_item: fx.publisher.last_item.clone(),
            },
            bound: true,
        });

        run_tick(at_minute(5), &deps).await.unwrap();

        assert_eq!(fx.publisher.count.load(Ordering::SeqCst), 0);
        assert_eq!(fx.cursor_of(id).await, 0); // retry next tick
    }

    #[tokio::test]
    async fn new_poster_is_picked_up_without_restart() {
        let fx = Fixture::new();
        let deps = fx.deps(
            FeedPick {
                post: Some(make_post(100, 7)),
                advance_to: 7,
            },
            true,
        );

        // No posters yet: nothing happens.
        run_tick(at_minute(5), &deps).await.unwrap();
        assert_eq!(fx.publisher.count.load(Ordering::SeqCst), 0);

        // Poster created between ticks (e.g. /newposter): next tick fires it.
        let id = fx.poster().await;
        run_tick(at_minute(10), &deps).await.unwrap();
        assert_eq!(fx.publisher.count.load(Ordering::SeqCst), 1);
        assert_eq!(fx.cursor_of(id).await, 7);
    }

    #[tokio::test]
    async fn caption_has_code_attribution_forward_credit_and_links() {
        use domain::elements::poster::PosterId;
        use domain::elements::user::{Role, TelegramId, UserRepository as _};

        let users = InMemoryUserRepository::new();
        let submitter = users
            .create(
                TelegramId::from(42),
                Role::User,
                None,
                Some("Ziel <3".to_string()),
            )
            .await
            .unwrap();

        let mut post = make_post(100, 1);
        post.source = Source::try_from(Url::parse("https://t.me/somechannel/42").unwrap()).unwrap();
        post.submitted_by = Some(submitter.id);

        let caption = build_caption(
            &post,
            PosterId::from(1),
            &users,
            "testbot",
            &ResolvedMedia::Photo(Url::parse("https://x/p.png").unwrap()),
        )
        .await;
        let lines: Vec<&str> = caption.lines().collect();
        // Fixed-size code header.
        assert!(lines[0].starts_with("<code>#"), "caption: {caption}");
        assert_eq!(lines[0].len(), "<code>#XXXXXXXX</code>".len());
        // HTML-escaped attribution.
        assert_eq!(lines[1], "Submitted by Ziel &lt;3");
        assert_eq!(lines[2], "Forwarded from channel: @somechannel");
        // Source + Report links.
        assert!(lines[3].contains("<a href=\"https://t.me/somechannel/42\">Source</a>"));
        assert!(lines[3].contains("https://t.me/testbot?start=report_100\">Report</a>"));
    }

    #[test]
    fn spoiler_rule_matches_listed_and_cw_tags() {
        let listed = vec![Tag::from("watersports"), Tag::from("questionable_consent")];
        assert!(needs_spoiler(
            &[Tag::from("wolf"), Tag::from("watersports")],
            &listed
        ));
        assert!(needs_spoiler(&[Tag::from("cw")], &[]));
        assert!(needs_spoiler(&[Tag::from("cw_blood")], &[]));
        assert!(needs_spoiler(&[Tag::from("CW:knife")], &[]));
        assert!(!needs_spoiler(
            &[Tag::from("wolf"), Tag::from("male")],
            &listed
        ));
        // 'cwhatever' must not trip the prefix rule.
        assert!(!needs_spoiler(&[Tag::from("cwhatever")], &[]));
    }

    #[tokio::test]
    async fn admin_added_post_has_no_attribution_line() {
        use domain::elements::poster::PosterId;
        let users = InMemoryUserRepository::new();
        let caption = build_caption(
            &make_post(100, 1),
            PosterId::from(1),
            &users,
            "testbot",
            &ResolvedMedia::Photo(Url::parse("https://x/p.png").unwrap()),
        )
        .await;
        assert!(!caption.contains("Submitted by"), "caption: {caption}");
        assert!(caption.contains("e621.net"), "caption: {caption}");
    }

    #[tokio::test]
    async fn video_media_gets_the_video_hashtag_and_artists_are_credited() {
        use domain::elements::poster::PosterId;
        let users = InMemoryUserRepository::new();
        let mut post = make_post(100, 1);
        post.artists = vec![
            domain::elements::tag::Tag::from("coolwolf"),
            domain::elements::tag::Tag::from("otherfox"),
        ];
        let caption = build_caption(
            &post,
            PosterId::from(1),
            &users,
            "testbot",
            &ResolvedMedia::Video(Url::parse("https://x/v.webm").unwrap()),
        )
        .await;
        let lines: Vec<&str> = caption.lines().collect();
        assert!(lines[0].ends_with(" #video"), "caption: {caption}");
        assert_eq!(lines[1], "By coolwolf, otherfox");

        // Photos don't get the hashtag.
        let caption = build_caption(
            &post,
            PosterId::from(1),
            &users,
            "testbot",
            &ResolvedMedia::Photo(Url::parse("https://x/p.png").unwrap()),
        )
        .await;
        assert!(!caption.contains("#video"), "caption: {caption}");
    }

    #[test]
    fn publish_code_is_fixed_size_and_deterministic() {
        use domain::elements::poster::PosterId;
        let post = make_post(1, 1);
        let a = publish_code(&post, PosterId::from(1));
        let b = publish_code(&post, PosterId::from(1));
        let other_poster = publish_code(&post, PosterId::from(2));
        assert_eq!(a, b);
        assert_eq!(a.len(), 8);
        assert_ne!(
            a, other_poster,
            "same source, different consumer → different code"
        );
    }
}

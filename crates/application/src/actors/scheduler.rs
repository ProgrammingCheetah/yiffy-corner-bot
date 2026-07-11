use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use domain::elements::{
    cadence::{PublishBlock, PublishBlockError},
    media::MediaResolver,
    post::{Post, PostRepository, PostSelectorStrategy},
    poster::{Poster, PosterRepository},
    publisher::{
        Publication, PublicationRepository, PublishItem, PublishReceipt, Publisher, PublisherError,
    },
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

// Manual impl: every field is an Arc (or String), so the deps clone cheaply
// regardless of whether the repositories themselves are Clone.
impl<R, U, PR, PB, ST> Clone for SchedulerDeps<R, U, PR, PB, ST>
where
    R: PostRepository + Send + Sync,
    U: UserRepository,
    PR: PosterRepository,
    PB: PublicationRepository,
    ST: SpoilerTagRepository,
{
    fn clone(&self) -> Self {
        Self {
            posts: self.posts.clone(),
            users: self.users.clone(),
            posters: self.posters.clone(),
            publications: self.publications.clone(),
            spoilers: self.spoilers.clone(),
            selectors: self.selectors.clone(),
            publishers: self.publishers.clone(),
            resolver: self.resolver.clone(),
            bot_username: self.bot_username.clone(),
        }
    }
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

/// The human labels behind the blur, for the caption: `cw_*`/`cw:*` keep
/// only the subject, listed spoiler tags appear as themselves; underscores
/// read as spaces. A bare `cw` blurs but names nothing, so it contributes
/// no label.
pub fn content_warnings(tags: &[Tag], spoiler_tags: &[Tag]) -> Vec<String> {
    let mut labels = Vec::new();
    for tag in tags {
        let name = tag.as_ref().to_ascii_lowercase();
        let label = match name.strip_prefix("cw_").or_else(|| name.strip_prefix("cw:")) {
            Some(subject) if !subject.is_empty() => subject.replace('_', " "),
            Some(_) => continue,
            None if spoiler_tags.contains(tag) => name.replace('_', " "),
            None => continue,
        };
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels
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
///
/// `content_warnings` (see [`content_warnings`]) names what's behind a
/// spoiler blur right under the header.
pub async fn build_caption<U: UserRepository>(
    post: &Post,
    poster_id: domain::elements::poster::PosterId,
    users: &U,
    bot_username: &str,
    media: &domain::elements::media::ResolvedMedia,
    content_warnings: &[String],
) -> String {
    use domain::elements::media::ResolvedMedia;

    let mut header = format!("<code>#{}</code>", publish_code(post, poster_id));
    if matches!(media, ResolvedMedia::Video(_) | ResolvedMedia::Animation(_)) {
        header.push_str(" #video");
    }
    let mut lines = vec![header];

    if !content_warnings.is_empty() {
        let labels = content_warnings
            .iter()
            .map(|label| escape_html(label))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("⚠️ CW: {labels}"));
    }

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
        "<a href=\"{0}\">Source</a> · \
         <a href=\"https://t.me/{bot_username}?start=more_{1}\">More like this</a> · \
         <a href=\"https://t.me/{bot_username}?start=report_{1}\">Report</a>",
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

/// Publish one already-resolved entry through `publisher` on behalf of
/// `poster`: caption → spoiler policy → send → record publication →
/// mark_posted. The publish tail shared by the scheduler fire and the
/// out-of-band pool batch (`actors::pool_batch`). Post-send bookkeeping
/// failures are logged but never returned — the message is already out.
pub async fn publish_resolved<R, U, PR, PB, ST>(
    post: &Post,
    media: domain::elements::media::ResolvedMedia,
    poster: &Poster,
    publisher: &dyn Publisher,
    deps: &SchedulerDeps<R, U, PR, PB, ST>,
    now: DateTime<Utc>,
) -> Result<PublishReceipt, PublisherError>
where
    R: PostRepository + Send + Sync,
    R::Err: std::fmt::Display,
    U: UserRepository,
    PR: PosterRepository,
    PB: PublicationRepository,
    PB::Err: std::fmt::Display,
    ST: SpoilerTagRepository,
    ST::Err: std::fmt::Display,
{
    let (spoiler, warnings) = match deps.spoilers.list_all().await {
        Ok(spoiler_tags) => (
            needs_spoiler(&post.tags, &spoiler_tags),
            content_warnings(&post.tags, &spoiler_tags),
        ),
        Err(e) => {
            tracing::warn!(post_id = %post.id, error = %e, "spoiler list read failed; publishing unspoilered");
            (false, Vec::new())
        }
    };
    let caption = build_caption(
        post,
        poster.id,
        &*deps.users,
        &deps.bot_username,
        &media,
        &warnings,
    )
    .await;
    let item = PublishItem {
        post_id: post.id,
        media,
        caption: Some(caption),
        spoiler,
    };
    let receipt = publisher.publish(&item).await?;
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
    Ok(receipt)
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

    // A dead entry must not wedge the consumer: when resolution says the
    // upstream is GONE (NotFound — not a transient failure), the entry is
    // marked MediaGone (dropping it from every consumer's scan) and the
    // scan repeats for the next candidate in this same fire. Bounded, so
    // one fire can't churn through an unbounded graveyard — the leftovers
    // wait one tick. Transient errors keep the retry-same-entry behavior.
    const MAX_DEAD_SKIPS: usize = 5;
    let mut dead_skips = 0;
    let (post, media, advance_to) = loop {
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
        match deps.resolver.resolve(&post.source).await {
            Ok(media) => break (post, media, pick.advance_to),
            Err(domain::elements::media::MediaResolveError::NotFound(_))
                if dead_skips < MAX_DEAD_SKIPS =>
            {
                dead_skips += 1;
                tracing::warn!(
                    event = %Event::DeadMediaFound, post_id = %post.id,
                    source = %post.source.as_ref(),
                    "upstream gone at fire time → MediaGone; taking the next entry"
                );
                if let Err(e) = deps
                    .posts
                    .set_status_to(post.id, domain::elements::post::PostStatus::MediaGone)
                    .await
                {
                    tracing::error!(
                        event = %Event::MediaResolveFailed, post_id = %post.id, error = %e,
                        "MediaGone write failed; cursor kept for retry"
                    );
                    return;
                }
                continue;
            }
            Err(e) => {
                tracing::error!(
                    event = %Event::MediaResolveFailed, post_id = %post.id,
                    source = %post.source.as_ref(), error = %e,
                    "media resolution failed; cursor kept for retry"
                );
                return;
            }
        }
    };
    tracing::debug!(event = %Event::MediaResolved, post_id = %post.id, media = ?media, "media resolved");
    let receipt = match publish_resolved(&post, media, &poster, &*publisher, deps, now).await {
        Ok(receipt) => receipt,
        Err(e) => {
            tracing::error!(
                event = %Event::PublishFailed, post_id = %post.id, error = %e,
                "publish failed; cursor kept for retry"
            );
            return;
        }
    };
    if let Err(e) = deps.posters.set_cursor(poster.id, advance_to).await {
        // Publish succeeded but the cursor didn't move: the entry may repeat.
        tracing::error!(
            event = %Event::MarkPostedFailed, post_id = %post.id, error = %e,
            "cursor write failed AFTER publish — entry may repeat"
        );
        return;
    }
    tracing::info!(
        event = %Event::Published, post_id = %post.id,
        chat_id = receipt.chat_id, cursor = advance_to,
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
            source: Source::try_from(Url::parse(&format!("https://e621.net/posts/{id}")).unwrap())
                .unwrap(),
            status: PostStatus::Accepted,
            tags: vec![],
            artists: vec![],
            feed_position: Some(position),
            last_posted: None,
            submitted_by: None,
            submitted_at: Utc::now(),
            moderated_by: None,
            moderated_at: None,
            phash: None,
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

    /// Factory replaying a scripted sequence of picks, shared across fires.
    struct SequenceSelectorFactory(Arc<Mutex<std::collections::VecDeque<FeedPick>>>);
    impl SelectorFactory for SequenceSelectorFactory {
        fn for_poster(&self, _poster: Poster) -> Box<dyn PostSelectorStrategy> {
            Box::new(SequenceSelector(self.0.clone()))
        }
    }
    struct SequenceSelector(Arc<Mutex<std::collections::VecDeque<FeedPick>>>);
    #[async_trait]
    impl PostSelectorStrategy for SequenceSelector {
        async fn next_post(&self, cursor: u64) -> Result<FeedPick, SelectorError> {
            Ok(self.0.lock().unwrap().pop_front().unwrap_or(FeedPick {
                post: None,
                advance_to: cursor,
            }))
        }
    }

    /// Resolver where /posts/100 is gone for good; everything else is a photo.
    struct DeadHundredResolver;
    #[async_trait]
    impl MediaResolver for DeadHundredResolver {
        async fn resolve(&self, source: &Source) -> Result<ResolvedMedia, MediaResolveError> {
            let url: &Url = source.as_ref();
            if url.path().ends_with("/100") {
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

    /// PostRepository stub recording mark_posted and status writes.
    #[derive(Default)]
    struct RecordingPostRepository {
        marked: Mutex<Vec<PostId>>,
        status_set: Mutex<Vec<(PostId, PostStatus)>>,
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
        async fn set_status_to(&self, id: PostId, status: PostStatus) -> Result<(), Self::Err> {
            self.status_set.lock().unwrap().push((id, status));
            Ok(())
        }
        async fn set_tags(
            &self,
            _id: PostId,
            _tags: Vec<domain::elements::tag::Tag>,
        ) -> Result<Post, Self::Err> {
            unimplemented!()
        }
        async fn resubmit(
            &self,
            _id: PostId,
            _tags: Vec<domain::elements::tag::Tag>,
            _artists: Vec<domain::elements::tag::Tag>,
            _submitted_at: DateTime<Utc>,
            _status: PostStatus,
        ) -> Result<Post, Self::Err> {
            unimplemented!()
        }
        async fn set_phash(&self, _id: PostId, _phash: Option<u64>) -> Result<(), Self::Err> {
            unimplemented!()
        }
        async fn top_submitters(
            &self,
            _limit: usize,
        ) -> Result<Vec<(domain::elements::user::UserId, u64)>, Self::Err> {
            unimplemented!()
        }
        async fn list_phashes(&self) -> Result<Vec<(PostId, u64)>, Self::Err> {
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
        async fn feed_after_paged(
            &self,
            _cursor: u64,
            _up_to: u64,
            _limit: u32,
        ) -> Result<Vec<Post>, Self::Err> {
            unimplemented!()
        }
        async fn list_by_submitter(
            &self,
            _submitter: domain::elements::user::UserId,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<Post>, Self::Err> {
            unimplemented!()
        }
        async fn count_by_submitter(
            &self,
            _submitter: domain::elements::user::UserId,
        ) -> Result<Vec<(PostStatus, u64)>, Self::Err> {
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
                .create(vec![], vec![], PostInterval::new(5).unwrap(), 0)
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
    async fn dead_entry_is_shelved_and_the_next_one_publishes() {
        use std::collections::VecDeque;

        let fx = Fixture::new();
        let id = fx.poster().await;
        let mut deps = fx.deps(
            FeedPick {
                post: None,
                advance_to: 0,
            },
            true,
        );
        // First candidate (post 100) is dead upstream; second (101) is fine.
        deps.selectors = Arc::new(SequenceSelectorFactory(Arc::new(Mutex::new(
            VecDeque::from([
                FeedPick {
                    post: Some(make_post(100, 7)),
                    advance_to: 7,
                },
                FeedPick {
                    post: Some(make_post(101, 8)),
                    advance_to: 8,
                },
            ]),
        ))));
        deps.resolver = Arc::new(DeadHundredResolver);

        run_tick(at_minute(5), &deps).await.unwrap();

        // The dead entry was shelved, the living one published, cursor on it.
        assert_eq!(
            *deps.posts.status_set.lock().unwrap(),
            vec![(PostId::from(100), PostStatus::MediaGone)]
        );
        assert_eq!(fx.publisher.count.load(Ordering::SeqCst), 1);
        assert_eq!(*deps.posts.marked.lock().unwrap(), vec![PostId::from(101)]);
        assert_eq!(fx.cursor_of(id).await, 8);
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
            &[],
        )
        .await;
        let lines: Vec<&str> = caption.lines().collect();
        // Fixed-size code header.
        assert!(lines[0].starts_with("<code>#"), "caption: {caption}");
        assert_eq!(lines[0].len(), "<code>#XXXXXXXX</code>".len());
        // HTML-escaped attribution.
        assert_eq!(lines[1], "Submitted by Ziel &lt;3");
        assert_eq!(lines[2], "Forwarded from channel: @somechannel");
        // Source + More + Report links.
        assert!(lines[3].contains("<a href=\"https://t.me/somechannel/42\">Source</a>"));
        assert!(lines[3].contains("https://t.me/testbot?start=more_100\">More like this</a>"));
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

    #[test]
    fn content_warnings_strip_the_cw_prefix_and_name_listed_tags() {
        let listed = vec![Tag::from("questionable_consent")];
        let labels = content_warnings(
            &[
                Tag::from("wolf"),
                Tag::from("cw_blood_and_gore"),
                Tag::from("CW:knife"),
                Tag::from("questionable_consent"),
                Tag::from("cw"),           // blurs, but names nothing
                Tag::from("cw_blood_and_gore"), // duplicate label collapses
            ],
            &listed,
        );
        assert_eq!(
            labels,
            vec!["blood and gore", "knife", "questionable consent"]
        );
        assert!(content_warnings(&[Tag::from("wolf")], &listed).is_empty());
    }

    #[tokio::test]
    async fn caption_names_the_content_warnings() {
        use domain::elements::poster::PosterId;
        let users = InMemoryUserRepository::new();
        let caption = build_caption(
            &make_post(100, 1),
            PosterId::from(1),
            &users,
            "testbot",
            &ResolvedMedia::Photo(Url::parse("https://x/p.png").unwrap()),
            &["blood and gore".to_string(), "knife <3".to_string()],
        )
        .await;
        let lines: Vec<&str> = caption.lines().collect();
        assert_eq!(lines[1], "⚠️ CW: blood and gore, knife &lt;3");
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
            &[],
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
            &[],
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
            &[],
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

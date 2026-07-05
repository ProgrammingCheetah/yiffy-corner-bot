use chrono::{DateTime, Utc};
use url::Url;

use crate::elements::tag::Tag;
use crate::elements::user::UserId;

/// The internal ID for a Post. Program-managed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostId(u64);

impl From<u64> for PostId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl AsRef<u64> for PostId {
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

impl std::fmt::Display for PostId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A URL pointing to externally-hosted media, typed by platform.
///
/// Sources are value objects: two `Source`s with the same URL compare equal.
/// The bot never re-hosts media; sources always reference the original
/// platform. The variant is derived from the URL's host at construction time
/// via [`TryFrom<Url>`]; URLs that don't match a known host are rejected with
/// [`SourceError::UnknownHost`].
///
/// Per `design/domain.md`, **only `E621` sources can be re-posted**. The other
/// variants exist so the system can record and reason about cross-platform
/// references without re-posting from them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    E621(Url),
    Twitter(Url),
    BlueSky(Url),
    Telegram(Url),
    FurAffinity(Url),
    DeviantArt(Url),
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("URL host not recognized as a known source: {0}")]
    UnknownHost(Url),
    #[error("URL has no host: {0}")]
    NoHost(Url),
}

impl TryFrom<Url> for Source {
    type Error = SourceError;
    fn try_from(url: Url) -> Result<Self, Self::Error> {
        let host = url
            .host_str()
            .ok_or_else(|| SourceError::NoHost(url.clone()))?;
        Ok(match host {
            "e621.net" | "e926.net" => Source::E621(url),
            "twitter.com" | "x.com" => Source::Twitter(url),
            "bsky.app" => Source::BlueSky(url),
            "t.me" => Source::Telegram(url),
            "furaffinity.net" | "www.furaffinity.net" => Source::FurAffinity(url),
            "deviantart.com" | "www.deviantart.com" => Source::DeviantArt(url),
            _ => return Err(SourceError::UnknownHost(url)),
        })
    }
}

impl AsRef<Url> for Source {
    fn as_ref(&self) -> &Url {
        match self {
            Source::E621(u)
            | Source::Twitter(u)
            | Source::BlueSky(u)
            | Source::Telegram(u)
            | Source::FurAffinity(u)
            | Source::DeviantArt(u) => u,
        }
    }
}

impl Source {
    /// The public channel handle (without `@`) of a `t.me/<channel>/<msg>`
    /// source. `None` for other variants and for non-channel t.me paths.
    pub fn telegram_channel(&self) -> Option<&str> {
        let Source::Telegram(url) = self else {
            return None;
        };
        let mut segments = url.path_segments()?;
        let channel = segments.next()?;
        // t.me/c/<internal>/<msg> is a private-channel link — no handle.
        if channel.is_empty() || channel == "c" {
            return None;
        }
        Some(channel)
    }
}

/// The status of a [`Post`] in the local workflow.
///
/// **Important**: this is a cached prior verdict for `Banned`, not a permanent
/// decision. The Selector re-validates against fresh e621 data on each
/// selection — a `Banned` post can flip back to `Accepted` if its tags no
/// longer contain anything in the global forbidden list. `Rejected` and
/// `Deleted` are explicit human decisions and never re-evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostStatus {
    /// Submitted but not yet moderated.
    AwaitingModeration,
    /// Approved by a moderator (or auto-Accepted via `/browse`).
    Accepted,
    /// A moderator explicitly rejected this post.
    Rejected,
    /// External takedown (DMCA etc.). Soft-delete; row retained for audit.
    Deleted,
    /// Owns at least one globally forbidden tag. May flip back to `Accepted`
    /// on re-validation if the offending tag is removed from the forbidden
    /// list or from the post on e621.
    Banned,
}

impl std::fmt::Display for PostStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PostStatus::AwaitingModeration => "awaiting_moderation",
            PostStatus::Accepted => "accepted",
            PostStatus::Rejected => "rejected",
            PostStatus::Deleted => "deleted",
            PostStatus::Banned => "banned",
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown post status: {0}")]
pub struct PostStatusParseError(String);

impl std::str::FromStr for PostStatus {
    type Err = PostStatusParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "awaiting_moderation" => Ok(PostStatus::AwaitingModeration),
            "accepted" => Ok(PostStatus::Accepted),
            "rejected" => Ok(PostStatus::Rejected),
            "deleted" => Ok(PostStatus::Deleted),
            "banned" => Ok(PostStatus::Banned),
            other => Err(PostStatusParseError(other.to_string())),
        }
    }
}

/// A piece of media curated by the bot — an entry in (or headed for) THE FEED.
///
/// Feed model (2026-07-05): all curated Posts live in one ordered feed;
/// consumers (Posters) walk it with per-consumer cursors. `feed_position` is
/// the monotonic ordering key, assigned once when the Post is accepted into
/// the feed (approval or admin save) — `None` means "not in the feed"
/// (awaiting moderation, rejected, or auto-banned at submission).
///
/// Every Post carries curated `tags`: fetched from e621 for e621 sources,
/// supplied by the submitter for everything else. e621 tags are still
/// re-validated fresh at consume time; curated tags are the fallback truth
/// for every other platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    pub id: PostId,
    /// The canonical reference URL.
    pub source: Source,
    pub status: PostStatus,
    /// Curated tags (e621: API tags at submission; other sources: submitter's).
    pub tags: Vec<Tag>,
    /// Position in the feed; `None` until accepted into it.
    pub feed_position: Option<u64>,
    /// When this Post was most recently published by a Poster. `None` if never.
    /// Audit only — feed consumption is cursor-driven.
    pub last_posted: Option<DateTime<Utc>>,
    /// The User who submitted it via `/suggest`. `None` for admin-added posts
    /// from `/browse` (which enter the feed directly, bypassing moderation).
    pub submitted_by: Option<UserId>,
    pub submitted_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum PostRepositoryError {
    #[error("Post could not be created: {0}")]
    NotCreated(String),
    #[error("Post not found: {0}")]
    NotFound(PostId),
}

/// Persistence port for [`Post`]s.
#[async_trait::async_trait]
pub trait PostRepository: Send + Sync {
    type Err;
    /// Create a Post with the given source, curated tags, submitter (if any),
    /// submission time, and initial status. Caller decides the status:
    /// `AwaitingModeration` for `/suggest`, `Banned` if the post owns a
    /// forbidden tag at submission. Creation never assigns a feed position —
    /// use [`Self::accept_into_feed`].
    async fn create(
        &self,
        source: Source,
        tags: Vec<Tag>,
        submitted_by: Option<UserId>,
        submitted_at: DateTime<Utc>,
        status: PostStatus,
    ) -> Result<Post, Self::Err>;

    async fn find_by_id(&self, id: PostId) -> Result<Option<Post>, Self::Err>;
    /// Lookup by source URL. Used by `/suggest` to detect a duplicate
    /// submission and by `/getpostinfo` to show local workflow status
    /// alongside the e621 fetch.
    async fn find_by_source(&self, source: &Source) -> Result<Option<Post>, Self::Err>;
    /// Soft-delete: sets status to [`PostStatus::Deleted`]. The row is retained
    /// for audit; selection skips Deleted posts.
    async fn remove(&self, id: PostId) -> Result<(), Self::Err>;
    async fn set_status_to(&self, post_id: PostId, status: PostStatus) -> Result<(), Self::Err>;
    /// Record that `id` was just published at `at`. Updates `last_posted`.
    async fn mark_posted(&self, id: PostId, at: DateTime<Utc>) -> Result<(), Self::Err>;
    /// All Posts currently in `status`, ordered oldest-submitted first.
    /// `AwaitingModeration` ordering IS the moderation queue.
    async fn list_by_status(&self, status: PostStatus) -> Result<Vec<Post>, Self::Err>;

    // --- feed operations -------------------------------------------------

    /// Accept a Post into the feed: status → `Accepted` and the next
    /// monotonic `feed_position` is assigned (idempotent for Posts already
    /// holding a position — only the status flips back). Used by moderator
    /// approval and admin `/browse` saves.
    async fn accept_into_feed(&self, id: PostId) -> Result<Post, Self::Err>;
    /// The current end of the feed (highest assigned position; 0 if empty).
    /// Consumers snapshot this BEFORE scanning so entries appended mid-scan
    /// are never skipped.
    async fn feed_end(&self) -> Result<u64, Self::Err>;
    /// Feed entries with `cursor < position <= up_to`, in feed order.
    /// Includes `Accepted` and `Banned` entries — Banned is a cached verdict
    /// the consumer re-validates (and may lift) at consume time.
    async fn feed_after(&self, cursor: u64, up_to: u64) -> Result<Vec<Post>, Self::Err>;
}

#[derive(Debug, thiserror::Error)]
pub enum SelectorError {
    #[error("repository error during selection: {0}")]
    Repository(String),
    #[error("upstream fetch error during selection: {0}")]
    Fetch(String),
}

/// The outcome of one feed scan.
///
/// `advance_to` is where the consumer's cursor should land — the matched
/// entry's position on a hit, or the pre-scan feed-end snapshot on a miss —
/// and the caller persists it only once the entry is safely published (so a
/// failed publish retries the same entry next tick).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedPick {
    pub post: Option<Post>,
    pub advance_to: u64,
}

/// Strategy for walking the feed on behalf of one consumer.
///
/// `+ Send + Sync` so the scheduler can hold one instance per Poster across an
/// async task boundary. Async because real selection hits the repository and
/// re-validates e621 entries against fresh data.
#[async_trait::async_trait]
pub trait PostSelectorStrategy: Send + Sync {
    /// Scan the feed from `cursor` (exclusive) to the pre-scan end snapshot,
    /// returning the first entry matching this consumer's tag criteria.
    /// May flip entry statuses as a side effect (Banned → Accepted on
    /// re-validation, Accepted → Banned on a fresh forbidden hit).
    async fn next_post(&self, cursor: u64) -> Result<FeedPick, SelectorError>;
}

#[cfg(test)]
mod source_tests {
    use super::*;

    fn parse(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn e621_hosts() {
        assert!(matches!(
            Source::try_from(parse("https://e621.net/posts/1")).unwrap(),
            Source::E621(_)
        ));
        assert!(matches!(
            Source::try_from(parse("https://e926.net/posts/1")).unwrap(),
            Source::E621(_)
        ));
    }

    #[test]
    fn twitter_hosts() {
        for host in ["twitter.com", "x.com"] {
            let url = parse(&format!("https://{host}/user/status/1"));
            assert!(matches!(Source::try_from(url).unwrap(), Source::Twitter(_)));
        }
    }

    #[test]
    fn bsky_host() {
        assert!(matches!(
            Source::try_from(parse("https://bsky.app/profile/x/post/1")).unwrap(),
            Source::BlueSky(_)
        ));
    }

    #[test]
    fn telegram_host() {
        assert!(matches!(
            Source::try_from(parse("https://t.me/channel/1")).unwrap(),
            Source::Telegram(_)
        ));
    }

    #[test]
    fn furaffinity_hosts() {
        for host in ["furaffinity.net", "www.furaffinity.net"] {
            let url = parse(&format!("https://{host}/view/1"));
            assert!(matches!(
                Source::try_from(url).unwrap(),
                Source::FurAffinity(_)
            ));
        }
    }

    #[test]
    fn deviantart_hosts() {
        for host in ["deviantart.com", "www.deviantart.com"] {
            let url = parse(&format!("https://{host}/x/art/y"));
            assert!(matches!(
                Source::try_from(url).unwrap(),
                Source::DeviantArt(_)
            ));
        }
    }

    #[test]
    fn unknown_host_rejected() {
        let err = Source::try_from(parse("https://example.com/p/1")).unwrap_err();
        assert!(matches!(err, SourceError::UnknownHost(_)));
    }

    #[test]
    fn as_ref_returns_inner_url() {
        let url = parse("https://e621.net/posts/1");
        let source = Source::try_from(url.clone()).unwrap();
        assert_eq!(source.as_ref(), &url);
    }

    #[test]
    fn telegram_channel_extracts_public_handle() {
        let source = Source::try_from(parse("https://t.me/somechannel/42")).unwrap();
        assert_eq!(source.telegram_channel(), Some("somechannel"));
    }

    #[test]
    fn telegram_channel_none_for_private_links_and_other_sources() {
        let private = Source::try_from(parse("https://t.me/c/123456/42")).unwrap();
        assert_eq!(private.telegram_channel(), None);
        let e621 = Source::try_from(parse("https://e621.net/posts/1")).unwrap();
        assert_eq!(e621.telegram_channel(), None);
    }
}

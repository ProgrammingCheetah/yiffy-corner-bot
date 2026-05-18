use chrono::{DateTime, Utc};
use url::Url;

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

/// A piece of media curated by the bot.
///
/// **Lean by design**: the bot is an indexer over e621, not a content store.
/// Tags, mime type, and other content metadata are always fetched fresh from
/// e621 — they're never persisted locally. This struct carries only the
/// identity (`source`) and the local workflow state (`status`, `last_posted`,
/// `submitted_by`, `submitted_at`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    pub id: PostId,
    /// The canonical reference URL (e621 post URL for re-postable Posts).
    pub source: Source,
    pub status: PostStatus,
    /// When this Post was most recently published by a Poster. `None` if never.
    pub last_posted: Option<DateTime<Utc>>,
    /// The User who submitted it via `/suggest`. `None` for admin-added posts
    /// from `/browse` (which auto-Accept, bypassing submission).
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
    /// Create a Post with the given source, submitter (if any), submission
    /// time, and initial status. Caller decides the status: `AwaitingModeration`
    /// for `/suggest`, `Banned` if the post owns a forbidden tag at submission,
    /// `Accepted` for admin-added Posts via `/browse`.
    async fn create(
        &self,
        source: Source,
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
    async fn set_status_to(
        &self,
        post_id: PostId,
        status: PostStatus,
    ) -> Result<(), Self::Err>;
    /// Record that `id` was just published at `at`. Updates `last_posted`.
    async fn mark_posted(&self, id: PostId, at: DateTime<Utc>) -> Result<(), Self::Err>;
}

#[derive(Debug, thiserror::Error)]
pub enum SelectorError {
    #[error("no post matched the Poster's tag criteria")]
    NoMatch,
    #[error("repository error during selection: {0}")]
    Repository(String),
    #[error("upstream fetch error during selection: {0}")]
    Fetch(String),
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
}

/// Strategy for selecting which [`Post`] a Poster fires next.
///
/// Different implementations (uniform, weighted, etc.) live behind this trait
/// so the selection policy can evolve without changing the use case.
///
/// `+ Send + Sync` so the scheduler can hold one instance per Poster across an
/// async task boundary.
pub trait PostSelectorStrategy: Send + Sync {
    /// Try the moderation queue first: if its head matches this Poster's tag
    /// criteria, return it; otherwise `Ok(None)` so the caller can fall back
    /// to [`Self::find_post`].
    fn find_due_post(&self) -> Result<Option<Post>, SelectorError>;
    /// Pick a Post from the saved pool (Accepted ∪ Banned). The strategy
    /// validates tags against fresh e621 data and may mutate Post.status as a
    /// side effect (Banned → Accepted on un-ban, Accepted → Banned on policy
    /// hit).
    fn find_post(&self) -> Result<Post, SelectorError>;
}

use chrono::{DateTime, Utc};
use url::Url;

use crate::elements::tag::Tag;

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

/// Image media subtypes the bot accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImgMimeSubtype {
    Jpeg,
    Png,
    Gif,
    Webp,
}

/// Video media subtypes the bot accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoMimeSubtype {
    Mp4,
    Webm,
}

/// The kind of media a [`Post`] carries.
///
/// Coarse split (`Image` vs `Video`) drives any branching where the two behave differently
/// (e.g. Telegram `send_photo` vs `send_video`); subtypes carry the precise format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimeType {
    Image(ImgMimeSubtype),
    Video(VideoMimeSubtype),
}

/// A URL pointing to externally-hosted media.
///
/// Sources are value objects: two `Source`s with the same URL compare equal.
/// The bot never re-hosts media; sources always reference the original platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source(Url);

impl AsRef<Url> for Source {
    fn as_ref(&self) -> &Url {
        &self.0
    }
}

impl From<Url> for Source {
    fn from(value: Url) -> Self {
        Self(value)
    }
}

/// The status of the Post
/// Why not queued? Infrastructure problem
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostStatus {
    /// When a post has been submitted but not moderated
    AwaitingModeration,
    /// When a post has been accepted and can be selected
    Accepted,
    /// When a post has been rejected for any reason of quality or purpose
    Rejected,
    /// When a post has been soft-deleted (removed from selection but retained for audit)
    Deleted,
    /// When a post has been banned outright, out of content
    Banned,
}

/// A perceptual hash of a Post's media.
///
/// Used for near-duplicate detection across submissions: visually similar images
/// produce numerically close hashes, even if their bytes differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerceptualHash(u64);

impl AsRef<u64> for PerceptualHash {
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

impl From<u64> for PerceptualHash {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

/// A piece of media curated by the bot.
///
/// - Has one or more sources.
/// - Is MEDIA: has a type such as png, mp4, or otherwise.
/// - Only Posts with an e621 source can be re-posted.
/// - Is described by zero or more tags. Non-e621 posts have zero tags.
/// - Has a last-posted date.
/// - Has a pHash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    pub id: PostId,
    pub media_type: MimeType,
    pub sources: Vec<Source>,
    pub tags: Vec<Tag>,
    pub status: PostStatus,
    pub last_posted: Option<DateTime<Utc>>,
    pub p_hash: PerceptualHash,
}

/// Persistence port for [`Post`]s.
#[async_trait::async_trait]
pub trait PostRepository: Send + Sync {
    type Err;
    async fn create(
        &self,
        media_type: MimeType,
        sources: Vec<Source>,
        tags: Vec<Tag>,
        p_hash: PerceptualHash,
    ) -> Result<Post, Self::Err>;

    async fn find_by_id(&self, id: PostId) -> Result<Option<Post>, Self::Err>;
    /// Soft-delete: sets status to [`PostStatus::Deleted`]. The row is retained
    /// for audit; selection skips Deleted posts. For content bans, use
    /// [`set_status_to`](Self::set_status_to) with [`PostStatus::Banned`].
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
pub enum PostRepositoryError {
    #[error("Post could not be created: {0}")]
    NotCreated(String),
    #[error("Post not found: {0}")]
    NotFound(PostId),
}

#[derive(Debug, thiserror::Error)]
pub enum SelectorError {
    #[error("no post matched the Poster's tag criteria")]
    NoMatch,
    #[error("repository error during selection: {0}")]
    Repository(String),
}

/// Strategy for selecting which [`Post`] a Poster fires next.
///
/// Different implementations (random, FIFO, weighted by tag-match, etc.) live behind
/// this trait so the selection policy can evolve without changing the use case.
///
/// `+ Send + Sync` so the scheduler can hold one instance per Poster across an
/// async task boundary.
pub trait PostSelectorStrategy: Send + Sync {
    /// Try the queue first: if its head matches this Poster's tag criteria,
    /// return it; otherwise `Ok(None)` so the caller can fall back to [`Self::find_post`].
    fn find_due_post(&self) -> Result<Option<Post>, SelectorError>;
    /// Pick a Post from the saved pool. Implementations decide the policy
    /// (random, FIFO, weighted, etc.).
    fn find_post(&self) -> Result<Post, SelectorError>;
}

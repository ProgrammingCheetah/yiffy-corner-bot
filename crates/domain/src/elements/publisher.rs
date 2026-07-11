use chrono::{DateTime, Utc};

use crate::elements::media::ResolvedMedia;
use crate::elements::post::PostId;

#[derive(Debug, thiserror::Error)]
pub enum PublisherError {
    #[error("publisher send failed: {0}")]
    Send(String),
}

/// A fully-prepared message for a Publisher to deliver: the resolved media
/// plus the caption the application layer built (attribution like
/// "Submitted by <name>", the source link). The Publisher is dumb transport —
/// it decides *how* to send based on the media variant, never *what*.
///
/// `post_id` rides along so the Publisher can attach post-scoped controls
/// (the ⚠️ Report button) to the delivered message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishItem {
    pub post_id: PostId,
    pub media: ResolvedMedia,
    pub caption: Option<String>,
    /// Send the media behind Telegram's spoiler blur (content-warning tags).
    /// Best-effort: link embeds and message copies cannot be spoilered.
    pub spoiler: bool,
}

/// Where a publish landed. Recorded per delivery so moderation can take a
/// post down from every chat it reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishReceipt {
    pub chat_id: i64,
    pub message_id: i32,
}

/// Delivers a [`PublishItem`] to its destination (e.g. a Telegram channel).
///
/// Cadence is not a Publisher concern — the scheduler decides which Poster
/// fires on which tick and then calls `publish` on the matching Publisher.
#[async_trait::async_trait]
pub trait Publisher: Send + Sync {
    async fn publish(&self, item: &PublishItem) -> Result<PublishReceipt, PublisherError>;
}

/// One delivery of a Post to a chat (the audit trail behind takedowns).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publication {
    pub post_id: PostId,
    pub chat_id: i64,
    pub message_id: i32,
    pub published_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum PublicationRepositoryError {
    #[error("publication repository error: {0}")]
    Storage(String),
}

/// Persistence port for [`Publication`]s.
#[async_trait::async_trait]
pub trait PublicationRepository: Send + Sync {
    type Err;
    async fn record(&self, publication: Publication) -> Result<(), Self::Err>;
    async fn list_for(&self, post_id: PostId) -> Result<Vec<Publication>, Self::Err>;
    /// Everything ever published to one chat — the channel-scoreboard corpus.
    async fn list_for_chat(&self, chat_id: i64) -> Result<Vec<Publication>, Self::Err>;
    /// (publication count, most recent published_at) for one chat — the
    /// poster-profile stats, without loading the whole corpus.
    async fn chat_stats(
        &self,
        chat_id: i64,
    ) -> Result<(u64, Option<chrono::DateTime<chrono::Utc>>), Self::Err>;
}

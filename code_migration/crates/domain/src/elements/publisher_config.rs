use std::path::PathBuf;

use crate::elements::poster::PosterId;

/// The delivery configuration for a Poster's Publisher.
///
/// 1:1 with [`Poster`](crate::elements::poster::Poster). Created by
/// `/setchannel` and consumed at boot to construct the matching Publisher.
/// The bot token itself is not stored in this struct — it lives on disk at
/// `token_path` (typically under `config/vault/<env>/posters/<id>/token.txt`)
/// so secrets stay out of the main DB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublisherConfig {
    pub poster_id: PosterId,
    /// Telegram chat ID (the numeric form — `@handle`s are resolved to a
    /// numeric ID at `/setchannel` time).
    pub chat_id: i64,
    /// Filesystem path to the per-Poster bot token.
    pub token_path: PathBuf,
    /// Whether this chat receives announcement broadcasts. Muted chats
    /// still appear in the directory published to other channels.
    pub receive_announcements: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PublisherConfigRepositoryError {
    #[error("publisher config repository error: {0}")]
    Storage(String),
    #[error("publisher config not found for poster {0}")]
    NotFound(PosterId),
}

/// Persistence port for [`PublisherConfig`]s.
#[async_trait::async_trait]
pub trait PublisherConfigRepository: Send + Sync {
    type Err;
    /// Create or replace the PublisherConfig for `config.poster_id`. The 1:1
    /// invariant with Poster means re-running `/setchannel` on the same
    /// Poster should swap the destination/token rather than error.
    async fn upsert(&self, config: PublisherConfig) -> Result<(), Self::Err>;
    async fn find_by_poster(
        &self,
        poster_id: PosterId,
    ) -> Result<Option<PublisherConfig>, Self::Err>;
    /// Mute/unmute announcement delivery for every binding onto `chat_id`.
    /// Returns how many bindings were affected.
    async fn set_receive_announcements(
        &self,
        chat_id: i64,
        receive: bool,
    ) -> Result<u64, Self::Err>;
    /// Drop a Poster's binding (no-op when none exists). Part of poster
    /// deletion — the config row must go before the poster row (FK).
    async fn remove(&self, poster_id: PosterId) -> Result<(), Self::Err>;
    async fn list_all(&self) -> Result<Vec<PublisherConfig>, Self::Err>;
}

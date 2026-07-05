//! Outbound port for talking to Telegram's user-resolution API.
//!
//! Used by `/setrole` and `/setowner` to convert `@username` → `TelegramId`.
//! The infra impl (in the `telegram_bot` crate) wraps `bot.get_chat("@…")`.

use crate::elements::user::TelegramId;

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("telegram resolve failed: {0}")]
    Telegram(String),
}

/// Resolves Telegram `@username` handles to their numeric [`TelegramId`].
///
/// Returns `Ok(None)` when the username doesn't correspond to a public
/// Telegram account (e.g., the user has no public handle, or the handle
/// doesn't exist). Returns `Err` only for transport/API failures.
#[async_trait::async_trait]
pub trait TelegramUserResolver: Send + Sync {
    async fn resolve_username(&self, username: &str) -> Result<Option<TelegramId>, ResolveError>;
}

/// Copy coordinates for a channel post that was forwarded into the bot as a
/// submission.
///
/// The bot cannot read arbitrary channel messages, but it *did* see the
/// forwarded copy in the submitter's private chat — that message is what gets
/// re-copied (content, no forward header) to reviewers and, on publish, to
/// the destination channel. Keyed by the Post's canonical `t.me` source URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramCopyRef {
    /// The Post's source URL (`https://t.me/<channel>/<msg>`), as a string key.
    pub source_url: String,
    /// The private chat where the bot received the forward.
    pub origin_chat_id: i64,
    /// The forwarded message's id in that private chat.
    pub origin_message_id: i32,
    /// The originating channel's public handle (without `@`).
    pub channel_username: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TelegramCopyRepositoryError {
    #[error("telegram copy repository error: {0}")]
    Storage(String),
}

/// Persistence port for [`TelegramCopyRef`]s.
#[async_trait::async_trait]
pub trait TelegramCopyRepository: Send + Sync {
    type Err;
    /// Idempotent per source URL (re-submission of the same channel post
    /// refreshes the coordinates).
    async fn upsert(&self, copy_ref: TelegramCopyRef) -> Result<(), Self::Err>;
    async fn find_by_source_url(
        &self,
        source_url: &str,
    ) -> Result<Option<TelegramCopyRef>, Self::Err>;
}

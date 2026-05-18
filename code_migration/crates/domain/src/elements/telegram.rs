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
    async fn resolve_username(
        &self,
        username: &str,
    ) -> Result<Option<TelegramId>, ResolveError>;
}

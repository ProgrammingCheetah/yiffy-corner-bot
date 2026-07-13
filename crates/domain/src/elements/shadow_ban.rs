//! Shadowbans: the silent sibling of the visible submission ban.
//!
//! A shadowbanned Telegram id walks through the report, more-like-this,
//! and submission flows exactly like anyone else — same prompts, same
//! thank-yous — but nothing is stored and no moderator is notified. Keyed
//! by raw Telegram id because reporters don't need to be registered Users.
//! Reversible: lifting the ban restores normal behavior instantly.

use chrono::{DateTime, Utc};

use crate::elements::user::TelegramId;

#[derive(Debug, thiserror::Error)]
pub enum ShadowBanRepositoryError {
    #[error("shadow ban repository error: {0}")]
    Storage(String),
}

/// Persistence port for shadowbans.
#[async_trait::async_trait]
pub trait ShadowBanRepository: Send + Sync {
    type Err;
    /// Shadowban an id. Idempotent.
    async fn set(
        &self,
        who: TelegramId,
        by: TelegramId,
        at: DateTime<Utc>,
    ) -> Result<(), Self::Err>;
    /// Lift a shadowban. Idempotent.
    async fn lift(&self, who: TelegramId) -> Result<(), Self::Err>;
    async fn contains(&self, who: TelegramId) -> Result<bool, Self::Err>;
}

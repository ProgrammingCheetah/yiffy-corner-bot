//! The "fulfilling request" toggle: a curator answering a viewer's wish.
//!
//! While a curator's toggle is ON (armed with the request text), every post
//! they save from browse is stamped with that text, and its publication
//! caption reads `Fulfilling request <text>`. The toggle stays ON across
//! saves — and restarts — until explicitly turned OFF. Per-curator, so two
//! moderators can fulfill different requests at the same time.

use chrono::{DateTime, Utc};

use crate::elements::user::TelegramId;

#[derive(Debug, thiserror::Error)]
pub enum FulfillingRequestRepositoryError {
    #[error("fulfilling request repository error: {0}")]
    Storage(String),
}

/// Persistence port for the per-curator "fulfilling request" toggle.
#[async_trait::async_trait]
pub trait FulfillingRequestRepository: Send + Sync {
    type Err;
    /// Turn the toggle ON with the request text (replacing any previous).
    async fn set(
        &self,
        curator: TelegramId,
        request: &str,
        at: DateTime<Utc>,
    ) -> Result<(), Self::Err>;
    /// Turn the toggle OFF. Idempotent.
    async fn clear(&self, curator: TelegramId) -> Result<(), Self::Err>;
    /// The active request text, if the curator's toggle is ON.
    async fn active(&self, curator: TelegramId) -> Result<Option<String>, Self::Err>;
}

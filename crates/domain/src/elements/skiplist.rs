//! The browse skiplist: sources a moderator explicitly waved off.
//!
//! Source dedupe hides what's already curated, and pHash catches image
//! re-uploads — but a video re-upload has neither, so it would resurface
//! in browse forever. "Skip" records the verdict: this source is not for
//! us, never show it in browse again. (Suggestions are unaffected — the
//! skiplist only filters browse results.)

use chrono::{DateTime, Utc};

use crate::elements::post::Source;
use crate::elements::user::TelegramId;

#[derive(Debug, thiserror::Error)]
pub enum SkipListRepositoryError {
    #[error("skiplist repository error: {0}")]
    Storage(String),
}

/// Persistence port for the browse skiplist.
#[async_trait::async_trait]
pub trait SkipListRepository: Send + Sync {
    type Err;
    /// Remember this source as skipped. Idempotent.
    async fn add(
        &self,
        source: &Source,
        by: TelegramId,
        at: DateTime<Utc>,
    ) -> Result<(), Self::Err>;
    async fn contains(&self, source: &Source) -> Result<bool, Self::Err>;
}

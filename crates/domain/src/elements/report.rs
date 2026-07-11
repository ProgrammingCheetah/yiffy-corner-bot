//! The report system (design/domain.md: reports + abuse prevention).
//!
//! Any Telegram user who can see a published post can report it via the
//! ⚠️ button; filing asks for a reason so moderators know why.
//! Abuse prevention (MVP): one report per (post, reporter) —
//! duplicates are acknowledged but not re-recorded and never re-notify
//! moderators. Moderators resolve a report by taking the post down
//! (deleting its published messages + soft-deleting the Post) or dismissing
//! it (clearing the post's reports so a fresh wave re-notifies).

use chrono::{DateTime, Utc};

use crate::elements::post::PostId;
use crate::elements::user::TelegramId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub post_id: PostId,
    /// Raw Telegram id — reporters don't need to be registered Users.
    pub reporter: TelegramId,
    pub reported_at: DateTime<Utc>,
    /// Why the reporter flagged the post. `None` only on paths that cannot
    /// collect one (legacy report buttons when the reporter's DMs are closed).
    pub reason: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReportRepositoryError {
    #[error("report repository error: {0}")]
    Storage(String),
}

/// Persistence port for [`Report`]s.
#[async_trait::async_trait]
pub trait ReportRepository: Send + Sync {
    type Err;
    /// Record a report. Returns `false` when this reporter already reported
    /// this post (nothing new is stored).
    async fn add(&self, report: Report) -> Result<bool, Self::Err>;
    async fn count_for(&self, post_id: PostId) -> Result<u64, Self::Err>;
    /// Clear a post's reports (moderator dismissal).
    async fn clear_for(&self, post_id: PostId) -> Result<(), Self::Err>;
}

//! Announcements: the recurring cross-promo directory.
//!
//! On a configurable cadence the bot publishes, to every consuming channel,
//! a directory of all channels consuming the feed — each by name,
//! hyperlinked, alphabetical. One channel may be the "Spotlight": it appears
//! at the top of the list (placement only, no other privilege).

use chrono::{DateTime, Utc};

/// Singleton configuration + state for the announcement cycle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AnnouncementSettings {
    /// Recurrence in hours; 0 = announcements disabled.
    pub interval_hours: u32,
    /// The chat pinned to the top of the directory, if any.
    pub spotlight_chat_id: Option<i64>,
    /// When the last round went out (drift-based recurrence anchor).
    pub last_announced_at: Option<DateTime<Utc>>,
}

impl AnnouncementSettings {
    /// Whether a new round is due at `now`.
    pub fn due(&self, now: DateTime<Utc>) -> bool {
        if self.interval_hours == 0 {
            return false;
        }
        match self.last_announced_at {
            None => true,
            Some(last) => now - last >= chrono::Duration::hours(self.interval_hours as i64),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AnnouncementRepositoryError {
    #[error("announcement repository error: {0}")]
    Storage(String),
}

/// Persistence port for the singleton [`AnnouncementSettings`].
#[async_trait::async_trait]
pub trait AnnouncementRepository: Send + Sync {
    type Err;
    async fn get(&self) -> Result<AnnouncementSettings, Self::Err>;
    async fn set_interval_hours(&self, hours: u32) -> Result<(), Self::Err>;
    async fn set_spotlight(&self, chat_id: Option<i64>) -> Result<(), Self::Err>;
    async fn mark_announced(&self, at: chrono::DateTime<Utc>) -> Result<(), Self::Err>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_logic() {
        let now = Utc::now();
        let mut settings = AnnouncementSettings::default();
        assert!(!settings.due(now)); // disabled

        settings.interval_hours = 24;
        assert!(settings.due(now)); // never announced

        settings.last_announced_at = Some(now - chrono::Duration::hours(23));
        assert!(!settings.due(now));
        settings.last_announced_at = Some(now - chrono::Duration::hours(25));
        assert!(settings.due(now));
    }
}

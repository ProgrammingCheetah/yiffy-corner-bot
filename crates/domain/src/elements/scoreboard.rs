//! Scoreboards: the recurring per-channel community leaderboard.
//!
//! On a configurable cadence the bot posts, into every consuming channel,
//! that channel's own leaderboard — the community members whose submissions
//! were published THERE (so each board reflects the channel's tag taste).
//! Staff never rank: curators grading their own homework isn't a highscore.

use chrono::{DateTime, Utc};

/// Singleton configuration + state for the scoreboard cycle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScoreboardSettings {
    /// Recurrence in hours; 0 = scoreboards disabled.
    pub interval_hours: u32,
    /// When the last round went out (drift-based recurrence anchor).
    pub last_posted_at: Option<DateTime<Utc>>,
}

impl ScoreboardSettings {
    /// Whether a new round is due at `now`.
    pub fn due(&self, now: DateTime<Utc>) -> bool {
        if self.interval_hours == 0 {
            return false;
        }
        match self.last_posted_at {
            None => true,
            Some(last) => now - last >= chrono::Duration::hours(self.interval_hours as i64),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScoreboardRepositoryError {
    #[error("scoreboard repository error: {0}")]
    Storage(String),
}

/// Persistence port for the singleton [`ScoreboardSettings`].
#[async_trait::async_trait]
pub trait ScoreboardRepository: Send + Sync {
    type Err;
    async fn get(&self) -> Result<ScoreboardSettings, Self::Err>;
    async fn set_interval_hours(&self, hours: u32) -> Result<(), Self::Err>;
    async fn mark_posted(&self, at: DateTime<Utc>) -> Result<(), Self::Err>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_logic() {
        let now = Utc::now();
        let mut settings = ScoreboardSettings::default();
        assert!(!settings.due(now)); // disabled

        settings.interval_hours = 168;
        assert!(settings.due(now)); // never posted

        settings.last_posted_at = Some(now - chrono::Duration::hours(167));
        assert!(!settings.due(now));
        settings.last_posted_at = Some(now - chrono::Duration::hours(169));
        assert!(settings.due(now));
    }
}

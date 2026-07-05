use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::scoreboard::{
    ScoreboardRepository, ScoreboardRepositoryError, ScoreboardSettings,
};
use sqlx::{Row, sqlite::SqlitePool};

#[derive(Clone)]
pub struct SqliteScoreboardRepository {
    pool: SqlitePool,
}

impl SqliteScoreboardRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ScoreboardRepository for SqliteScoreboardRepository {
    type Err = ScoreboardRepositoryError;

    async fn get(&self) -> Result<ScoreboardSettings, Self::Err> {
        let row = sqlx::query("SELECT * FROM scoreboard_settings WHERE id = 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ScoreboardRepositoryError::Storage(e.to_string()))?;
        Ok(ScoreboardSettings {
            interval_hours: row.get::<i64, _>("interval_hours") as u32,
            last_posted_at: row.get::<Option<DateTime<Utc>>, _>("last_posted_at"),
        })
    }

    async fn set_interval_hours(&self, hours: u32) -> Result<(), Self::Err> {
        sqlx::query("UPDATE scoreboard_settings SET interval_hours = ? WHERE id = 1")
            .bind(hours as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| ScoreboardRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn mark_posted(&self, at: DateTime<Utc>) -> Result<(), Self::Err> {
        sqlx::query("UPDATE scoreboard_settings SET last_posted_at = ? WHERE id = 1")
            .bind(at)
            .execute(&self.pool)
            .await
            .map_err(|e| ScoreboardRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_pool;

    #[tokio::test]
    async fn singleton_roundtrip() {
        let repo = SqliteScoreboardRepository::new(test_pool().await);
        let settings = repo.get().await.unwrap();
        assert_eq!(settings.interval_hours, 0);
        assert!(settings.last_posted_at.is_none());

        repo.set_interval_hours(168).await.unwrap();
        repo.mark_posted(Utc::now()).await.unwrap();

        let settings = repo.get().await.unwrap();
        assert_eq!(settings.interval_hours, 168);
        assert!(settings.last_posted_at.is_some());
    }
}

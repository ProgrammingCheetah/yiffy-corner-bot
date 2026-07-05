use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::announcement::{
    AnnouncementRepository, AnnouncementRepositoryError, AnnouncementSettings,
};
use sqlx::{Row, sqlite::SqlitePool};

#[derive(Clone)]
pub struct SqliteAnnouncementRepository {
    pool: SqlitePool,
}

impl SqliteAnnouncementRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AnnouncementRepository for SqliteAnnouncementRepository {
    type Err = AnnouncementRepositoryError;

    async fn get(&self) -> Result<AnnouncementSettings, Self::Err> {
        let row = sqlx::query("SELECT * FROM announcement_settings WHERE id = 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| AnnouncementRepositoryError::Storage(e.to_string()))?;
        Ok(AnnouncementSettings {
            interval_hours: row.get::<i64, _>("interval_hours") as u32,
            spotlight_chat_id: row.get("spotlight_chat_id"),
            last_announced_at: row.get::<Option<DateTime<Utc>>, _>("last_announced_at"),
        })
    }

    async fn set_interval_hours(&self, hours: u32) -> Result<(), Self::Err> {
        sqlx::query("UPDATE announcement_settings SET interval_hours = ? WHERE id = 1")
            .bind(hours as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| AnnouncementRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn set_spotlight(&self, chat_id: Option<i64>) -> Result<(), Self::Err> {
        sqlx::query("UPDATE announcement_settings SET spotlight_chat_id = ? WHERE id = 1")
            .bind(chat_id)
            .execute(&self.pool)
            .await
            .map_err(|e| AnnouncementRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn mark_announced(&self, at: DateTime<Utc>) -> Result<(), Self::Err> {
        sqlx::query("UPDATE announcement_settings SET last_announced_at = ? WHERE id = 1")
            .bind(at)
            .execute(&self.pool)
            .await
            .map_err(|e| AnnouncementRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_pool;

    #[tokio::test]
    async fn singleton_roundtrip() {
        let repo = SqliteAnnouncementRepository::new(test_pool().await);
        let settings = repo.get().await.unwrap();
        assert_eq!(settings.interval_hours, 0);
        assert!(settings.spotlight_chat_id.is_none());

        repo.set_interval_hours(24).await.unwrap();
        repo.set_spotlight(Some(-100123)).await.unwrap();
        let at = Utc::now();
        repo.mark_announced(at).await.unwrap();

        let settings = repo.get().await.unwrap();
        assert_eq!(settings.interval_hours, 24);
        assert_eq!(settings.spotlight_chat_id, Some(-100123));
        assert!(settings.last_announced_at.is_some());
    }
}

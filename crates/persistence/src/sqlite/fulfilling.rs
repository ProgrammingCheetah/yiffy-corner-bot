use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::{
    fulfilling::{FulfillingRequestRepository, FulfillingRequestRepositoryError},
    user::TelegramId,
};
use sqlx::{Row, sqlite::SqlitePool};

#[derive(Clone)]
pub struct SqliteFulfillingRequestRepository {
    pool: SqlitePool,
}

impl SqliteFulfillingRequestRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FulfillingRequestRepository for SqliteFulfillingRequestRepository {
    type Err = FulfillingRequestRepositoryError;

    async fn set(
        &self,
        curator: TelegramId,
        request: &str,
        at: DateTime<Utc>,
    ) -> Result<(), Self::Err> {
        sqlx::query(
            "INSERT INTO fulfilling_requests (telegram_id, request, since) VALUES (?, ?, ?)
             ON CONFLICT (telegram_id) DO UPDATE SET request = excluded.request,
                                                     since = excluded.since",
        )
        .bind(*curator.as_ref())
        .bind(request)
        .bind(at)
        .execute(&self.pool)
        .await
        .map_err(|e| FulfillingRequestRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn clear(&self, curator: TelegramId) -> Result<(), Self::Err> {
        sqlx::query("DELETE FROM fulfilling_requests WHERE telegram_id = ?")
            .bind(*curator.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| FulfillingRequestRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn active(&self, curator: TelegramId) -> Result<Option<String>, Self::Err> {
        let row = sqlx::query("SELECT request FROM fulfilling_requests WHERE telegram_id = ?")
            .bind(*curator.as_ref())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| FulfillingRequestRepositoryError::Storage(e.to_string()))?;
        Ok(row.map(|r| r.get::<String, _>("request")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_pool;

    #[tokio::test]
    async fn set_replace_clear_roundtrip() {
        let repo = SqliteFulfillingRequestRepository::new(test_pool().await);
        let curator = TelegramId::from(7);
        assert_eq!(repo.active(curator).await.unwrap(), None);
        repo.set(curator, "more wolves", Utc::now()).await.unwrap();
        assert_eq!(
            repo.active(curator).await.unwrap().as_deref(),
            Some("more wolves")
        );
        // Setting again replaces the text — the toggle stays ON.
        repo.set(curator, "sergals in space", Utc::now())
            .await
            .unwrap();
        assert_eq!(
            repo.active(curator).await.unwrap().as_deref(),
            Some("sergals in space")
        );
        // Another curator's toggle is independent.
        assert_eq!(repo.active(TelegramId::from(8)).await.unwrap(), None);
        repo.clear(curator).await.unwrap();
        repo.clear(curator).await.unwrap(); // idempotent
        assert_eq!(repo.active(curator).await.unwrap(), None);
    }
}

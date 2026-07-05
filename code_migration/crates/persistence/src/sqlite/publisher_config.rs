use std::path::PathBuf;

use async_trait::async_trait;
use domain::elements::{
    poster::PosterId,
    publisher_config::{
        PublisherConfig, PublisherConfigRepository, PublisherConfigRepositoryError,
    },
};
use sqlx::{Row, sqlite::SqlitePool};

#[derive(Clone)]
pub struct SqlitePublisherConfigRepository {
    pool: SqlitePool,
}

impl SqlitePublisherConfigRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn row_to_config(row: &sqlx::sqlite::SqliteRow) -> PublisherConfig {
    PublisherConfig {
        poster_id: PosterId::from(row.get::<i64, _>("poster_id") as u64),
        chat_id: row.get("chat_id"),
        token_path: PathBuf::from(row.get::<String, _>("token_path")),
    }
}

#[async_trait]
impl PublisherConfigRepository for SqlitePublisherConfigRepository {
    type Err = PublisherConfigRepositoryError;

    async fn upsert(&self, config: PublisherConfig) -> Result<(), Self::Err> {
        sqlx::query(
            "INSERT INTO publisher_configs (poster_id, chat_id, token_path)
             VALUES (?, ?, ?)
             ON CONFLICT (poster_id) DO UPDATE SET
                 chat_id = excluded.chat_id,
                 token_path = excluded.token_path",
        )
        .bind(*config.poster_id.as_ref() as i64)
        .bind(config.chat_id)
        .bind(config.token_path.to_string_lossy().into_owned())
        .execute(&self.pool)
        .await
        .map_err(|e| PublisherConfigRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn find_by_poster(
        &self,
        poster_id: PosterId,
    ) -> Result<Option<PublisherConfig>, Self::Err> {
        let row = sqlx::query("SELECT * FROM publisher_configs WHERE poster_id = ?")
            .bind(*poster_id.as_ref() as i64)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| PublisherConfigRepositoryError::Storage(e.to_string()))?;
        Ok(row.as_ref().map(row_to_config))
    }

    async fn remove(&self, poster_id: PosterId) -> Result<(), Self::Err> {
        sqlx::query("DELETE FROM publisher_configs WHERE poster_id = ?")
            .bind(*poster_id.as_ref() as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| PublisherConfigRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<PublisherConfig>, Self::Err> {
        let rows = sqlx::query("SELECT * FROM publisher_configs ORDER BY poster_id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| PublisherConfigRepositoryError::Storage(e.to_string()))?;
        Ok(rows.iter().map(row_to_config).collect())
    }
}

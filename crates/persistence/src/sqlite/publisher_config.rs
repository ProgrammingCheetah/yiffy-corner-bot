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
        receive_announcements: row.get::<i64, _>("receive_announcements") != 0,
    }
}

#[async_trait]
impl PublisherConfigRepository for SqlitePublisherConfigRepository {
    type Err = PublisherConfigRepositoryError;

    async fn upsert(&self, config: PublisherConfig) -> Result<(), Self::Err> {
        // Re-binding preserves the announcement mute (it is chat policy,
        // not part of the destination).
        sqlx::query(
            "INSERT INTO publisher_configs
                 (poster_id, chat_id, token_path, receive_announcements)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (poster_id) DO UPDATE SET
                 chat_id = excluded.chat_id,
                 token_path = excluded.token_path",
        )
        .bind(*config.poster_id.as_ref() as i64)
        .bind(config.chat_id)
        .bind(config.token_path.to_string_lossy().into_owned())
        .bind(config.receive_announcements as i64)
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

    async fn set_receive_announcements(
        &self,
        chat_id: i64,
        receive: bool,
    ) -> Result<u64, Self::Err> {
        let result =
            sqlx::query("UPDATE publisher_configs SET receive_announcements = ? WHERE chat_id = ?")
                .bind(receive as i64)
                .bind(chat_id)
                .execute(&self.pool)
                .await
                .map_err(|e| PublisherConfigRepositoryError::Storage(e.to_string()))?;
        Ok(result.rows_affected())
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

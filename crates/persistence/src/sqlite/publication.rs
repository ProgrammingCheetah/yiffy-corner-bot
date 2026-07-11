use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::{
    post::PostId,
    publisher::{Publication, PublicationRepository, PublicationRepositoryError},
};
use sqlx::{Row, sqlite::SqlitePool};

#[derive(Clone)]
pub struct SqlitePublicationRepository {
    pool: SqlitePool,
}

impl SqlitePublicationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PublicationRepository for SqlitePublicationRepository {
    type Err = PublicationRepositoryError;

    async fn record(&self, publication: Publication) -> Result<(), Self::Err> {
        sqlx::query(
            "INSERT INTO publications (post_id, chat_id, message_id, published_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(*publication.post_id.as_ref() as i64)
        .bind(publication.chat_id)
        .bind(publication.message_id as i64)
        .bind(publication.published_at)
        .execute(&self.pool)
        .await
        .map_err(|e| PublicationRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn list_for(&self, post_id: PostId) -> Result<Vec<Publication>, Self::Err> {
        let rows = sqlx::query("SELECT * FROM publications WHERE post_id = ? ORDER BY id")
            .bind(*post_id.as_ref() as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| PublicationRepositoryError::Storage(e.to_string()))?;
        Ok(rows.iter().map(row_to_publication).collect())
    }

    async fn list_for_chat(&self, chat_id: i64) -> Result<Vec<Publication>, Self::Err> {
        let rows = sqlx::query("SELECT * FROM publications WHERE chat_id = ? ORDER BY id")
            .bind(chat_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| PublicationRepositoryError::Storage(e.to_string()))?;
        Ok(rows.iter().map(row_to_publication).collect())
    }

    async fn chat_stats(
        &self,
        chat_id: i64,
    ) -> Result<(u64, Option<chrono::DateTime<chrono::Utc>>), Self::Err> {
        let row = sqlx::query(
            "SELECT COUNT(*) AS n, MAX(published_at) AS last
             FROM publications WHERE chat_id = ?",
        )
        .bind(chat_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| PublicationRepositoryError::Storage(e.to_string()))?;
        Ok((
            row.get::<i64, _>("n") as u64,
            row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last"),
        ))
    }
}

fn row_to_publication(row: &sqlx::sqlite::SqliteRow) -> Publication {
    Publication {
        post_id: PostId::from(row.get::<i64, _>("post_id") as u64),
        chat_id: row.get("chat_id"),
        message_id: row.get::<i64, _>("message_id") as i32,
        published_at: row.get::<DateTime<Utc>, _>("published_at"),
    }
}

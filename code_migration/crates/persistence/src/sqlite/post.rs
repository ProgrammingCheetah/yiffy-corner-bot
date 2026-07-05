use std::str::FromStr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::{
    post::{Post, PostId, PostRepository, PostRepositoryError, PostStatus, Source},
    user::UserId,
};
use sqlx::{Row, sqlite::SqlitePool};
use url::Url;

#[derive(Clone)]
pub struct SqlitePostRepository {
    pool: SqlitePool,
}

impl SqlitePostRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn row_to_post(row: &sqlx::sqlite::SqliteRow) -> Result<Post, PostRepositoryError> {
    let source_url: String = row.get("source_url");
    let url =
        Url::parse(&source_url).map_err(|e| PostRepositoryError::NotCreated(e.to_string()))?;
    let source =
        Source::try_from(url).map_err(|e| PostRepositoryError::NotCreated(e.to_string()))?;
    let status: String = row.get("status");
    Ok(Post {
        id: PostId::from(row.get::<i64, _>("id") as u64),
        source,
        status: PostStatus::from_str(&status)
            .map_err(|e| PostRepositoryError::NotCreated(e.to_string()))?,
        last_posted: row.get::<Option<DateTime<Utc>>, _>("last_posted"),
        submitted_by: row
            .get::<Option<i64>, _>("submitted_by")
            .map(|id| UserId::from(id as u64)),
        submitted_at: row.get::<DateTime<Utc>, _>("submitted_at"),
    })
}

#[async_trait]
impl PostRepository for SqlitePostRepository {
    type Err = PostRepositoryError;

    async fn create(
        &self,
        source: Source,
        submitted_by: Option<UserId>,
        submitted_at: DateTime<Utc>,
        status: PostStatus,
    ) -> Result<Post, Self::Err> {
        let row = sqlx::query(
            "INSERT INTO posts (source_url, status, submitted_by, submitted_at)
             VALUES (?, ?, ?, ?) RETURNING *",
        )
        .bind(source.as_ref().as_str())
        .bind(status.to_string())
        .bind(submitted_by.map(|id| *id.as_ref() as i64))
        .bind(submitted_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| PostRepositoryError::NotCreated(e.to_string()))?;
        row_to_post(&row)
    }

    async fn find_by_id(&self, id: PostId) -> Result<Option<Post>, Self::Err> {
        let row = sqlx::query("SELECT * FROM posts WHERE id = ?")
            .bind(*id.as_ref() as i64)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| PostRepositoryError::NotCreated(e.to_string()))?;
        row.as_ref().map(row_to_post).transpose()
    }

    async fn find_by_source(&self, source: &Source) -> Result<Option<Post>, Self::Err> {
        let row = sqlx::query("SELECT * FROM posts WHERE source_url = ?")
            .bind(source.as_ref().as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| PostRepositoryError::NotCreated(e.to_string()))?;
        row.as_ref().map(row_to_post).transpose()
    }

    async fn remove(&self, id: PostId) -> Result<(), Self::Err> {
        self.set_status_to(id, PostStatus::Deleted).await
    }

    async fn set_status_to(&self, post_id: PostId, status: PostStatus) -> Result<(), Self::Err> {
        let result = sqlx::query("UPDATE posts SET status = ? WHERE id = ?")
            .bind(status.to_string())
            .bind(*post_id.as_ref() as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| PostRepositoryError::NotCreated(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(PostRepositoryError::NotFound(post_id));
        }
        Ok(())
    }

    async fn mark_posted(&self, id: PostId, at: DateTime<Utc>) -> Result<(), Self::Err> {
        let result = sqlx::query("UPDATE posts SET last_posted = ? WHERE id = ?")
            .bind(at)
            .bind(*id.as_ref() as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| PostRepositoryError::NotCreated(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(PostRepositoryError::NotFound(id));
        }
        Ok(())
    }

    async fn list_by_status(&self, status: PostStatus) -> Result<Vec<Post>, Self::Err> {
        let rows = sqlx::query("SELECT * FROM posts WHERE status = ? ORDER BY submitted_at, id")
            .bind(status.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| PostRepositoryError::NotCreated(e.to_string()))?;
        rows.iter().map(row_to_post).collect()
    }
}

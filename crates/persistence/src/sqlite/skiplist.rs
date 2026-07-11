use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::{
    post::Source,
    skiplist::{SkipListRepository, SkipListRepositoryError},
    user::TelegramId,
};
use sqlx::{Row, sqlite::SqlitePool};

#[derive(Clone)]
pub struct SqliteSkipListRepository {
    pool: SqlitePool,
}

impl SqliteSkipListRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SkipListRepository for SqliteSkipListRepository {
    type Err = SkipListRepositoryError;

    async fn add(&self, source: &Source, by: TelegramId, at: DateTime<Utc>) -> Result<(), Self::Err> {
        sqlx::query(
            "INSERT INTO browse_skips (source, skipped_by, skipped_at) VALUES (?, ?, ?)
             ON CONFLICT (source) DO NOTHING",
        )
        .bind(source.as_ref().as_str())
        .bind(*by.as_ref())
        .bind(at)
        .execute(&self.pool)
        .await
        .map_err(|e| SkipListRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn contains(&self, source: &Source) -> Result<bool, Self::Err> {
        let row = sqlx::query("SELECT COUNT(*) AS n FROM browse_skips WHERE source = ?")
            .bind(source.as_ref().as_str())
            .fetch_one(&self.pool)
            .await
            .map_err(|e| SkipListRepositoryError::Storage(e.to_string()))?;
        Ok(row.get::<i64, _>("n") > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_pool;
    use url::Url;

    #[tokio::test]
    async fn add_is_idempotent_and_contains_finds_it() {
        let repo = SqliteSkipListRepository::new(test_pool().await);
        let source =
            Source::try_from(Url::parse("https://e621.net/posts/9").unwrap()).unwrap();
        assert!(!repo.contains(&source).await.unwrap());
        repo.add(&source, TelegramId::from(1), Utc::now())
            .await
            .unwrap();
        repo.add(&source, TelegramId::from(2), Utc::now())
            .await
            .unwrap();
        assert!(repo.contains(&source).await.unwrap());
    }
}

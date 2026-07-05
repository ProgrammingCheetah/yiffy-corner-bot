use async_trait::async_trait;
use domain::elements::telegram::{
    TelegramCopyRef, TelegramCopyRepository, TelegramCopyRepositoryError,
};
use sqlx::{Row, sqlite::SqlitePool};

#[derive(Clone)]
pub struct SqliteTelegramCopyRepository {
    pool: SqlitePool,
}

impl SqliteTelegramCopyRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TelegramCopyRepository for SqliteTelegramCopyRepository {
    type Err = TelegramCopyRepositoryError;

    async fn upsert(&self, copy_ref: TelegramCopyRef) -> Result<(), Self::Err> {
        sqlx::query(
            "INSERT INTO telegram_copies
                 (source_url, origin_chat_id, origin_message_id, channel_username)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (source_url) DO UPDATE SET
                 origin_chat_id = excluded.origin_chat_id,
                 origin_message_id = excluded.origin_message_id,
                 channel_username = excluded.channel_username",
        )
        .bind(&copy_ref.source_url)
        .bind(copy_ref.origin_chat_id)
        .bind(copy_ref.origin_message_id as i64)
        .bind(&copy_ref.channel_username)
        .execute(&self.pool)
        .await
        .map_err(|e| TelegramCopyRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn find_by_source_url(
        &self,
        source_url: &str,
    ) -> Result<Option<TelegramCopyRef>, Self::Err> {
        let row = sqlx::query("SELECT * FROM telegram_copies WHERE source_url = ?")
            .bind(source_url)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| TelegramCopyRepositoryError::Storage(e.to_string()))?;
        Ok(row.map(|row| TelegramCopyRef {
            source_url: row.get("source_url"),
            origin_chat_id: row.get("origin_chat_id"),
            origin_message_id: row.get::<i64, _>("origin_message_id") as i32,
            channel_username: row.get("channel_username"),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_pool;

    fn copy_ref(msg: i32) -> TelegramCopyRef {
        TelegramCopyRef {
            source_url: "https://t.me/somechannel/42".to_string(),
            origin_chat_id: 1402476143,
            origin_message_id: msg,
            channel_username: "somechannel".to_string(),
        }
    }

    #[tokio::test]
    async fn upsert_roundtrip_and_replace() {
        let repo = SqliteTelegramCopyRepository::new(test_pool().await);
        repo.upsert(copy_ref(7)).await.unwrap();
        // Re-submission refreshes the coordinates rather than erroring.
        repo.upsert(copy_ref(9)).await.unwrap();

        let found = repo
            .find_by_source_url("https://t.me/somechannel/42")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.origin_message_id, 9);
        assert_eq!(found.channel_username, "somechannel");
    }

    #[tokio::test]
    async fn unknown_source_returns_none() {
        let repo = SqliteTelegramCopyRepository::new(test_pool().await);
        assert!(
            repo.find_by_source_url("https://t.me/x/1")
                .await
                .unwrap()
                .is_none()
        );
    }
}

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::{
    shadow_ban::{ShadowBanRepository, ShadowBanRepositoryError},
    user::TelegramId,
};
use sqlx::sqlite::SqlitePool;

#[derive(Clone)]
pub struct SqliteShadowBanRepository {
    pool: SqlitePool,
}

impl SqliteShadowBanRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ShadowBanRepository for SqliteShadowBanRepository {
    type Err = ShadowBanRepositoryError;

    async fn set(
        &self,
        who: TelegramId,
        by: TelegramId,
        at: DateTime<Utc>,
    ) -> Result<(), Self::Err> {
        sqlx::query(
            "INSERT INTO shadow_bans (telegram_id, banned_by, banned_at) VALUES (?, ?, ?)
             ON CONFLICT (telegram_id) DO NOTHING",
        )
        .bind(*who.as_ref())
        .bind(*by.as_ref())
        .bind(at)
        .execute(&self.pool)
        .await
        .map_err(|e| ShadowBanRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn lift(&self, who: TelegramId) -> Result<(), Self::Err> {
        sqlx::query("DELETE FROM shadow_bans WHERE telegram_id = ?")
            .bind(*who.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| ShadowBanRepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn contains(&self, who: TelegramId) -> Result<bool, Self::Err> {
        let row = sqlx::query("SELECT 1 FROM shadow_bans WHERE telegram_id = ? LIMIT 1")
            .bind(*who.as_ref())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| ShadowBanRepositoryError::Storage(e.to_string()))?;
        Ok(row.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::test_pool;

    #[tokio::test]
    async fn set_lift_contains_roundtrip() {
        let repo = SqliteShadowBanRepository::new(test_pool().await);
        let who = TelegramId::from(99);
        assert!(!repo.contains(who).await.unwrap());
        repo.set(who, TelegramId::from(1), Utc::now()).await.unwrap();
        repo.set(who, TelegramId::from(2), Utc::now()).await.unwrap(); // idempotent
        assert!(repo.contains(who).await.unwrap());
        repo.lift(who).await.unwrap();
        assert!(!repo.contains(who).await.unwrap());
    }
}

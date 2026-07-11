use std::collections::HashSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::{
    shadow_ban::{ShadowBanRepository, ShadowBanRepositoryError},
    user::TelegramId,
};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct InMemoryShadowBanRepository {
    banned: RwLock<HashSet<i64>>,
}

impl InMemoryShadowBanRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ShadowBanRepository for InMemoryShadowBanRepository {
    type Err = ShadowBanRepositoryError;

    async fn set(
        &self,
        who: TelegramId,
        _by: TelegramId,
        _at: DateTime<Utc>,
    ) -> Result<(), Self::Err> {
        self.banned.write().await.insert(*who.as_ref());
        Ok(())
    }

    async fn lift(&self, who: TelegramId) -> Result<(), Self::Err> {
        self.banned.write().await.remove(who.as_ref());
        Ok(())
    }

    async fn contains(&self, who: TelegramId) -> Result<bool, Self::Err> {
        Ok(self.banned.read().await.contains(who.as_ref()))
    }
}

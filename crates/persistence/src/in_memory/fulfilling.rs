use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::{
    fulfilling::{FulfillingRequestRepository, FulfillingRequestRepositoryError},
    user::TelegramId,
};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct InMemoryFulfillingRequestRepository {
    requests: RwLock<HashMap<i64, String>>,
}

impl InMemoryFulfillingRequestRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl FulfillingRequestRepository for InMemoryFulfillingRequestRepository {
    type Err = FulfillingRequestRepositoryError;

    async fn set(
        &self,
        curator: TelegramId,
        request: &str,
        _at: DateTime<Utc>,
    ) -> Result<(), Self::Err> {
        self.requests
            .write()
            .await
            .insert(*curator.as_ref(), request.to_string());
        Ok(())
    }

    async fn clear(&self, curator: TelegramId) -> Result<(), Self::Err> {
        self.requests.write().await.remove(curator.as_ref());
        Ok(())
    }

    async fn active(&self, curator: TelegramId) -> Result<Option<String>, Self::Err> {
        Ok(self.requests.read().await.get(curator.as_ref()).cloned())
    }
}

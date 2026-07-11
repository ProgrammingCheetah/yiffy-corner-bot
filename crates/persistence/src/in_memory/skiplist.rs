use std::collections::HashSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::{
    post::Source,
    skiplist::{SkipListRepository, SkipListRepositoryError},
    user::TelegramId,
};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct InMemorySkipListRepository {
    sources: RwLock<HashSet<String>>,
}

impl InMemorySkipListRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SkipListRepository for InMemorySkipListRepository {
    type Err = SkipListRepositoryError;

    async fn add(
        &self,
        source: &Source,
        _by: TelegramId,
        _at: DateTime<Utc>,
    ) -> Result<(), Self::Err> {
        self.sources
            .write()
            .await
            .insert(source.as_ref().as_str().to_string());
        Ok(())
    }

    async fn contains(&self, source: &Source) -> Result<bool, Self::Err> {
        Ok(self
            .sources
            .read()
            .await
            .contains(source.as_ref().as_str()))
    }
}

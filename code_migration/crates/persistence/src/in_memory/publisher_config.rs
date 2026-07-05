use std::collections::HashMap;

use async_trait::async_trait;
use domain::elements::{
    poster::PosterId,
    publisher_config::{
        PublisherConfig, PublisherConfigRepository, PublisherConfigRepositoryError,
    },
};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct InMemoryPublisherConfigRepository {
    configs: RwLock<HashMap<u64, PublisherConfig>>,
}

impl InMemoryPublisherConfigRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PublisherConfigRepository for InMemoryPublisherConfigRepository {
    type Err = PublisherConfigRepositoryError;

    async fn upsert(&self, config: PublisherConfig) -> Result<(), Self::Err> {
        self.configs
            .write()
            .await
            .insert(*config.poster_id.as_ref(), config);
        Ok(())
    }

    async fn find_by_poster(
        &self,
        poster_id: PosterId,
    ) -> Result<Option<PublisherConfig>, Self::Err> {
        Ok(self.configs.read().await.get(poster_id.as_ref()).cloned())
    }

    async fn remove(&self, poster_id: PosterId) -> Result<(), Self::Err> {
        self.configs.write().await.remove(poster_id.as_ref());
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<PublisherConfig>, Self::Err> {
        Ok(self.configs.read().await.values().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(poster_id: u64, chat: i64) -> PublisherConfig {
        PublisherConfig {
            poster_id: PosterId::from(poster_id),
            chat_id: chat,
            token_path: PathBuf::from(format!("/vault/{poster_id}/token.txt")),
        }
    }

    #[tokio::test]
    async fn upsert_then_find_by_poster() {
        let repo = InMemoryPublisherConfigRepository::new();
        let cfg = fixture(1, 100);
        repo.upsert(cfg.clone()).await.unwrap();
        let found = repo.find_by_poster(PosterId::from(1)).await.unwrap();
        assert_eq!(found, Some(cfg));
    }

    #[tokio::test]
    async fn upsert_replaces_existing_for_same_poster() {
        let repo = InMemoryPublisherConfigRepository::new();
        repo.upsert(fixture(1, 100)).await.unwrap();
        repo.upsert(fixture(1, 200)).await.unwrap();
        let found = repo
            .find_by_poster(PosterId::from(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.chat_id, 200);
        assert_eq!(repo.list_all().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn find_by_poster_unknown_returns_none() {
        let repo = InMemoryPublisherConfigRepository::new();
        assert!(
            repo.find_by_poster(PosterId::from(99))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn list_all_returns_every_upserted() {
        let repo = InMemoryPublisherConfigRepository::new();
        repo.upsert(fixture(1, 100)).await.unwrap();
        repo.upsert(fixture(2, 200)).await.unwrap();
        repo.upsert(fixture(3, 300)).await.unwrap();
        assert_eq!(repo.list_all().await.unwrap().len(), 3);
    }
}

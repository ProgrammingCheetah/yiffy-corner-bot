use async_trait::async_trait;
use domain::elements::{
    post::PostId,
    publisher::{Publication, PublicationRepository, PublicationRepositoryError},
};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct InMemoryPublicationRepository {
    publications: RwLock<Vec<Publication>>,
}

impl InMemoryPublicationRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PublicationRepository for InMemoryPublicationRepository {
    type Err = PublicationRepositoryError;

    async fn record(&self, publication: Publication) -> Result<(), Self::Err> {
        self.publications.write().await.push(publication);
        Ok(())
    }

    async fn list_for(&self, post_id: PostId) -> Result<Vec<Publication>, Self::Err> {
        Ok(self
            .publications
            .read()
            .await
            .iter()
            .filter(|p| p.post_id == post_id)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn record_and_list_by_post() {
        let repo = InMemoryPublicationRepository::new();
        for (post, msg) in [(1u64, 10), (1, 11), (2, 12)] {
            repo.record(Publication {
                post_id: PostId::from(post),
                chat_id: -100,
                message_id: msg,
                published_at: Utc::now(),
            })
            .await
            .unwrap();
        }
        assert_eq!(repo.list_for(PostId::from(1)).await.unwrap().len(), 2);
        assert_eq!(repo.list_for(PostId::from(3)).await.unwrap().len(), 0);
    }
}

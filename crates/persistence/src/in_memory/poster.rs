use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use domain::elements::{
    cadence::PostInterval,
    poster::{Poster, PosterId, PosterRepository, PosterRepositoryError},
    tag::Tag,
    tag_rule::TagRule,
    tag_rule::TagTerm,
};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct InMemoryPosterRepository {
    posters: RwLock<HashMap<u64, Poster>>,
    next_id: AtomicU64,
}

impl InMemoryPosterRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl PosterRepository for InMemoryPosterRepository {
    type Err = PosterRepositoryError;

    async fn create(
        &self,
        subscribed_tags: Vec<TagTerm>,
        forbidden_tags: Vec<Tag>,
        time_interval: PostInterval,
    ) -> Result<Poster, Self::Err> {
        let mut posters = self.posters.write().await;
        let raw_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let poster = Poster {
            id: PosterId::from(raw_id),
            subscribed_tags,
            forbidden_tags,
            time_interval,
            cursor: 0,
            rules: Vec::new(),
        };
        posters.insert(raw_id, poster.clone());
        Ok(poster)
    }

    async fn find_by_id(&self, id: PosterId) -> Result<Option<Poster>, Self::Err> {
        Ok(self.posters.read().await.get(id.as_ref()).cloned())
    }

    async fn set_tags(
        &self,
        id: PosterId,
        subscribed_tags: Vec<TagTerm>,
        forbidden_tags: Vec<Tag>,
    ) -> Result<Poster, Self::Err> {
        let mut posters = self.posters.write().await;
        let poster = posters
            .get_mut(id.as_ref())
            .ok_or(PosterRepositoryError::NotFound(id))?;
        poster.subscribed_tags = subscribed_tags;
        poster.forbidden_tags = forbidden_tags;
        Ok(poster.clone())
    }

    async fn set_rules(&self, id: PosterId, rules: Vec<TagRule>) -> Result<Poster, Self::Err> {
        let mut posters = self.posters.write().await;
        let poster = posters
            .get_mut(id.as_ref())
            .ok_or(PosterRepositoryError::NotFound(id))?;
        poster.rules = rules;
        Ok(poster.clone())
    }

    async fn set_interval(
        &self,
        id: PosterId,
        interval: PostInterval,
    ) -> Result<Poster, Self::Err> {
        let mut posters = self.posters.write().await;
        let poster = posters
            .get_mut(id.as_ref())
            .ok_or(PosterRepositoryError::NotFound(id))?;
        poster.time_interval = interval;
        Ok(poster.clone())
    }

    async fn delete(&self, id: PosterId) -> Result<(), Self::Err> {
        self.posters
            .write()
            .await
            .remove(id.as_ref())
            .map(|_| ())
            .ok_or(PosterRepositoryError::NotFound(id))
    }

    async fn set_cursor(&self, id: PosterId, cursor: u64) -> Result<(), Self::Err> {
        let mut posters = self.posters.write().await;
        let poster = posters
            .get_mut(id.as_ref())
            .ok_or(PosterRepositoryError::NotFound(id))?;
        poster.cursor = cursor;
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<Poster>, Self::Err> {
        Ok(self.posters.read().await.values().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_interval() -> PostInterval {
        PostInterval::new(5).unwrap()
    }

    #[tokio::test]
    async fn create_then_find_by_id_roundtrip() {
        let repo = InMemoryPosterRepository::new();
        let poster = repo
            .create(vec![], vec![], fixture_interval())
            .await
            .unwrap();
        let found = repo.find_by_id(poster.id).await.unwrap();
        assert_eq!(found.map(|p| p.id), Some(poster.id));
    }

    #[tokio::test]
    async fn create_assigns_unique_ids() {
        let repo = InMemoryPosterRepository::new();
        let a = repo
            .create(vec![], vec![], fixture_interval())
            .await
            .unwrap();
        let b = repo
            .create(vec![], vec![], fixture_interval())
            .await
            .unwrap();
        assert_ne!(a.id, b.id);
    }

    #[tokio::test]
    async fn find_by_id_unknown_returns_none() {
        let repo = InMemoryPosterRepository::new();
        assert!(repo.find_by_id(PosterId::from(42)).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_all_returns_every_created_poster() {
        let repo = InMemoryPosterRepository::new();
        for _ in 0..3 {
            repo.create(vec![], vec![], fixture_interval())
                .await
                .unwrap();
        }
        assert_eq!(repo.list_all().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn set_tags_replaces_subscription_only() {
        let repo = InMemoryPosterRepository::new();
        let poster = repo
            .create(vec![Tag::from("fox").into()], vec![], fixture_interval())
            .await
            .unwrap();
        let updated = repo
            .set_tags(
                poster.id,
                vec![Tag::from("wolf").into()],
                vec![Tag::from("gore")],
            )
            .await
            .unwrap();
        assert_eq!(updated.subscribed_tags, vec![Tag::from("wolf").into()]);
        assert_eq!(updated.forbidden_tags, vec![Tag::from("gore")]);
        assert_eq!(updated.time_interval, fixture_interval());

        let err = repo
            .set_tags(PosterId::from(99), vec![], vec![])
            .await
            .unwrap_err();
        assert!(matches!(err, PosterRepositoryError::NotFound(_)));
    }

    #[tokio::test]
    async fn set_cursor_persists() {
        let repo = InMemoryPosterRepository::new();
        let poster = repo
            .create(vec![], vec![], fixture_interval())
            .await
            .unwrap();
        assert_eq!(poster.cursor, 0);
        repo.set_cursor(poster.id, 17).await.unwrap();
        assert_eq!(
            repo.find_by_id(poster.id).await.unwrap().unwrap().cursor,
            17
        );
        assert!(matches!(
            repo.set_cursor(PosterId::from(99), 1).await.unwrap_err(),
            PosterRepositoryError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn create_persists_tag_subscription_and_interval() {
        let repo = InMemoryPosterRepository::new();
        let subscribed = vec![TagTerm::from(Tag::from("fox"))];
        let forbidden = vec![Tag::from("snake")];
        let interval = PostInterval::new(15).unwrap();
        let poster = repo
            .create(subscribed.clone(), forbidden.clone(), interval)
            .await
            .unwrap();
        let found = repo.find_by_id(poster.id).await.unwrap().unwrap();
        assert_eq!(found.subscribed_tags, subscribed);
        assert_eq!(found.forbidden_tags, forbidden);
        assert_eq!(found.time_interval, interval);
    }
}

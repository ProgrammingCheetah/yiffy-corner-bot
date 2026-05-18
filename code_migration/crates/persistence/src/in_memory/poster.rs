use std::{
    collections::HashMap,
    sync::{
        RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

use domain::elements::{
    cadence::PostInterval,
    poster::{Poster, PosterId, PosterRepository, PosterRepositoryError},
    tag::Tag,
};

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

impl PosterRepository for InMemoryPosterRepository {
    type Err = PosterRepositoryError;

    fn create(
        &self,
        subscribed_tags: Vec<Tag>,
        forbidden_tags: Vec<Tag>,
        time_interval: PostInterval,
    ) -> Result<Poster, Self::Err> {
        let mut posters = self.posters.write().expect("posters RwLock poisoned");
        let raw_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let poster = Poster {
            id: PosterId::from(raw_id),
            subscribed_tags,
            forbidden_tags,
            time_interval,
        };
        posters.insert(raw_id, poster.clone());
        Ok(poster)
    }

    fn find_by_id(&self, id: PosterId) -> Result<Option<Poster>, Self::Err> {
        Ok(self
            .posters
            .read()
            .expect("posters RwLock poisoned")
            .get(id.as_ref())
            .cloned())
    }

    fn list_all(&self) -> Result<Vec<Poster>, Self::Err> {
        Ok(self
            .posters
            .read()
            .expect("posters RwLock poisoned")
            .values()
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_interval() -> PostInterval {
        PostInterval::new(5).unwrap()
    }

    #[test]
    fn create_then_find_by_id_roundtrip() {
        let repo = InMemoryPosterRepository::new();
        let poster = repo.create(vec![], vec![], fixture_interval()).unwrap();
        let found = repo.find_by_id(poster.id).unwrap();
        assert_eq!(found.map(|p| p.id), Some(poster.id));
    }

    #[test]
    fn create_assigns_unique_ids() {
        let repo = InMemoryPosterRepository::new();
        let a = repo.create(vec![], vec![], fixture_interval()).unwrap();
        let b = repo.create(vec![], vec![], fixture_interval()).unwrap();
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn find_by_id_unknown_returns_none() {
        let repo = InMemoryPosterRepository::new();
        assert!(repo.find_by_id(PosterId::from(42)).unwrap().is_none());
    }

    #[test]
    fn list_all_returns_every_created_poster() {
        let repo = InMemoryPosterRepository::new();
        for _ in 0..3 {
            repo.create(vec![], vec![], fixture_interval()).unwrap();
        }
        assert_eq!(repo.list_all().unwrap().len(), 3);
    }

    #[test]
    fn create_persists_tag_subscription_and_interval() {
        let repo = InMemoryPosterRepository::new();
        let subscribed = vec![Tag::from("fox")];
        let forbidden = vec![Tag::from("snake")];
        let interval = PostInterval::new(15).unwrap();
        let poster = repo
            .create(subscribed.clone(), forbidden.clone(), interval)
            .unwrap();
        let found = repo.find_by_id(poster.id).unwrap().unwrap();
        assert_eq!(found.subscribed_tags, subscribed);
        assert_eq!(found.forbidden_tags, forbidden);
        assert_eq!(found.time_interval, interval);
    }
}

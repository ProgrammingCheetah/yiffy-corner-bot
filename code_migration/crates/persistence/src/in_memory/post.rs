use std::{
    collections::HashMap,
    sync::{
        RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

use chrono::{DateTime, Utc};
use domain::elements::{
    post::{
        MimeType, PerceptualHash, Post, PostId, PostRepository, PostRepositoryError, PostStatus,
        Source,
    },
    tag::Tag,
};

#[derive(Debug, Default)]
pub struct InMemoryPostRepository {
    posts: RwLock<HashMap<u64, Post>>,
    next_id: AtomicU64,
}

impl InMemoryPostRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PostRepository for InMemoryPostRepository {
    type Err = PostRepositoryError;

    fn create(
        &self,
        media_type: MimeType,
        sources: Vec<Source>,
        tags: Vec<Tag>,
        p_hash: PerceptualHash,
    ) -> Result<Post, Self::Err> {
        let mut posts = self.posts.write().expect("posts RwLock poisoned");
        let raw_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let post = Post {
            id: PostId::from(raw_id),
            media_type,
            sources,
            tags,
            status: PostStatus::AwaitingModeration,
            last_posted: None,
            p_hash,
        };
        posts.insert(raw_id, post.clone());
        Ok(post)
    }

    fn find_by_id(&self, id: PostId) -> Result<Option<Post>, Self::Err> {
        Ok(self
            .posts
            .read()
            .expect("posts RwLock poisoned")
            .get(id.as_ref())
            .cloned())
    }

    fn remove(&self, id: PostId) -> Result<(), Self::Err> {
        self.set_status_to(id, PostStatus::Deleted)
    }

    fn set_status_to(&self, post_id: PostId, status: PostStatus) -> Result<(), Self::Err> {
        let mut posts = self.posts.write().expect("posts RwLock poisoned");
        let post = posts
            .get_mut(post_id.as_ref())
            .ok_or(PostRepositoryError::NotFound(post_id))?;
        post.status = status;
        Ok(())
    }

    fn mark_posted(&self, id: PostId, at: DateTime<Utc>) -> Result<(), Self::Err> {
        let mut posts = self.posts.write().expect("posts RwLock poisoned");
        let post = posts
            .get_mut(id.as_ref())
            .ok_or(PostRepositoryError::NotFound(id))?;
        post.last_posted = Some(at);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn fixture_hash() -> PerceptualHash {
        PerceptualHash::from(0xdeadbeef)
    }

    fn fixture_source() -> Source {
        Source::from(Url::parse("https://e621.net/posts/1").unwrap())
    }

    #[test]
    fn create_then_find_by_id_roundtrip() {
        let repo = InMemoryPostRepository::new();
        let post = repo
            .create(
                MimeType::Image(domain::elements::post::ImgMimeSubtype::Png),
                vec![fixture_source()],
                vec![],
                fixture_hash(),
            )
            .unwrap();
        let found = repo.find_by_id(post.id).unwrap();
        assert_eq!(found.map(|p| p.id), Some(post.id));
    }

    #[test]
    fn create_assigns_unique_ids() {
        let repo = InMemoryPostRepository::new();
        let a = repo
            .create(
                MimeType::Image(domain::elements::post::ImgMimeSubtype::Png),
                vec![fixture_source()],
                vec![],
                fixture_hash(),
            )
            .unwrap();
        let b = repo
            .create(
                MimeType::Image(domain::elements::post::ImgMimeSubtype::Png),
                vec![fixture_source()],
                vec![],
                fixture_hash(),
            )
            .unwrap();
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn newly_created_post_is_awaiting_moderation_with_no_last_posted() {
        let repo = InMemoryPostRepository::new();
        let post = repo
            .create(
                MimeType::Image(domain::elements::post::ImgMimeSubtype::Png),
                vec![fixture_source()],
                vec![],
                fixture_hash(),
            )
            .unwrap();
        assert_eq!(post.status, PostStatus::AwaitingModeration);
        assert!(post.last_posted.is_none());
    }

    #[test]
    fn remove_sets_status_to_deleted() {
        let repo = InMemoryPostRepository::new();
        let post = repo
            .create(
                MimeType::Image(domain::elements::post::ImgMimeSubtype::Png),
                vec![fixture_source()],
                vec![],
                fixture_hash(),
            )
            .unwrap();
        repo.remove(post.id).unwrap();
        let found = repo.find_by_id(post.id).unwrap().unwrap();
        assert_eq!(found.status, PostStatus::Deleted);
    }

    #[test]
    fn set_status_to_changes_status() {
        let repo = InMemoryPostRepository::new();
        let post = repo
            .create(
                MimeType::Image(domain::elements::post::ImgMimeSubtype::Png),
                vec![fixture_source()],
                vec![],
                fixture_hash(),
            )
            .unwrap();
        repo.set_status_to(post.id, PostStatus::Accepted).unwrap();
        let found = repo.find_by_id(post.id).unwrap().unwrap();
        assert_eq!(found.status, PostStatus::Accepted);
    }

    #[test]
    fn mark_posted_updates_timestamp() {
        let repo = InMemoryPostRepository::new();
        let post = repo
            .create(
                MimeType::Image(domain::elements::post::ImgMimeSubtype::Png),
                vec![fixture_source()],
                vec![],
                fixture_hash(),
            )
            .unwrap();
        let when = Utc::now();
        repo.mark_posted(post.id, when).unwrap();
        let found = repo.find_by_id(post.id).unwrap().unwrap();
        assert_eq!(found.last_posted, Some(when));
    }

    #[test]
    fn mark_posted_unknown_id_returns_not_found() {
        let repo = InMemoryPostRepository::new();
        let err = repo.mark_posted(PostId::from(42), Utc::now()).unwrap_err();
        assert!(matches!(err, PostRepositoryError::NotFound(_)));
    }

    #[test]
    fn set_status_to_unknown_id_returns_not_found() {
        let repo = InMemoryPostRepository::new();
        let err = repo
            .set_status_to(PostId::from(42), PostStatus::Accepted)
            .unwrap_err();
        assert!(matches!(err, PostRepositoryError::NotFound(_)));
    }
}

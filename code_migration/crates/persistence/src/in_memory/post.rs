use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::{
    post::{
        MimeType, PerceptualHash, Post, PostId, PostRepository, PostRepositoryError, PostStatus,
        Source,
    },
    tag::Tag,
};
use tokio::sync::RwLock;

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

#[async_trait]
impl PostRepository for InMemoryPostRepository {
    type Err = PostRepositoryError;

    async fn create(
        &self,
        media_type: MimeType,
        sources: Vec<Source>,
        tags: Vec<Tag>,
        p_hash: PerceptualHash,
    ) -> Result<Post, Self::Err> {
        let mut posts = self.posts.write().await;
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

    async fn find_by_id(&self, id: PostId) -> Result<Option<Post>, Self::Err> {
        Ok(self.posts.read().await.get(id.as_ref()).cloned())
    }

    async fn remove(&self, id: PostId) -> Result<(), Self::Err> {
        self.set_status_to(id, PostStatus::Deleted).await
    }

    async fn set_status_to(
        &self,
        post_id: PostId,
        status: PostStatus,
    ) -> Result<(), Self::Err> {
        let mut posts = self.posts.write().await;
        let post = posts
            .get_mut(post_id.as_ref())
            .ok_or(PostRepositoryError::NotFound(post_id))?;
        post.status = status;
        Ok(())
    }

    async fn mark_posted(&self, id: PostId, at: DateTime<Utc>) -> Result<(), Self::Err> {
        let mut posts = self.posts.write().await;
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
    use domain::elements::post::ImgMimeSubtype;
    use url::Url;

    fn fixture_hash() -> PerceptualHash {
        PerceptualHash::from(0xdeadbeef)
    }

    fn fixture_source() -> Source {
        Source::from(Url::parse("https://e621.net/posts/1").unwrap())
    }

    fn fixture_create_args() -> (MimeType, Vec<Source>, Vec<Tag>, PerceptualHash) {
        (
            MimeType::Image(ImgMimeSubtype::Png),
            vec![fixture_source()],
            vec![],
            fixture_hash(),
        )
    }

    #[tokio::test]
    async fn create_then_find_by_id_roundtrip() {
        let repo = InMemoryPostRepository::new();
        let (mt, s, t, h) = fixture_create_args();
        let post = repo.create(mt, s, t, h).await.unwrap();
        let found = repo.find_by_id(post.id).await.unwrap();
        assert_eq!(found.map(|p| p.id), Some(post.id));
    }

    #[tokio::test]
    async fn create_assigns_unique_ids() {
        let repo = InMemoryPostRepository::new();
        let (mt, s, t, h) = fixture_create_args();
        let a = repo.create(mt, s.clone(), t.clone(), h).await.unwrap();
        let b = repo.create(mt, s, t, h).await.unwrap();
        assert_ne!(a.id, b.id);
    }

    #[tokio::test]
    async fn newly_created_post_is_awaiting_moderation_with_no_last_posted() {
        let repo = InMemoryPostRepository::new();
        let (mt, s, t, h) = fixture_create_args();
        let post = repo.create(mt, s, t, h).await.unwrap();
        assert_eq!(post.status, PostStatus::AwaitingModeration);
        assert!(post.last_posted.is_none());
    }

    #[tokio::test]
    async fn remove_sets_status_to_deleted() {
        let repo = InMemoryPostRepository::new();
        let (mt, s, t, h) = fixture_create_args();
        let post = repo.create(mt, s, t, h).await.unwrap();
        repo.remove(post.id).await.unwrap();
        let found = repo.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(found.status, PostStatus::Deleted);
    }

    #[tokio::test]
    async fn set_status_to_changes_status() {
        let repo = InMemoryPostRepository::new();
        let (mt, s, t, h) = fixture_create_args();
        let post = repo.create(mt, s, t, h).await.unwrap();
        repo.set_status_to(post.id, PostStatus::Accepted)
            .await
            .unwrap();
        let found = repo.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(found.status, PostStatus::Accepted);
    }

    #[tokio::test]
    async fn mark_posted_updates_timestamp() {
        let repo = InMemoryPostRepository::new();
        let (mt, s, t, h) = fixture_create_args();
        let post = repo.create(mt, s, t, h).await.unwrap();
        let when = Utc::now();
        repo.mark_posted(post.id, when).await.unwrap();
        let found = repo.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(found.last_posted, Some(when));
    }

    #[tokio::test]
    async fn mark_posted_unknown_id_returns_not_found() {
        let repo = InMemoryPostRepository::new();
        let err = repo
            .mark_posted(PostId::from(42), Utc::now())
            .await
            .unwrap_err();
        assert!(matches!(err, PostRepositoryError::NotFound(_)));
    }

    #[tokio::test]
    async fn set_status_to_unknown_id_returns_not_found() {
        let repo = InMemoryPostRepository::new();
        let err = repo
            .set_status_to(PostId::from(42), PostStatus::Accepted)
            .await
            .unwrap_err();
        assert!(matches!(err, PostRepositoryError::NotFound(_)));
    }
}

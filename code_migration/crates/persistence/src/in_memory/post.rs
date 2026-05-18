use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::{
    post::{Post, PostId, PostRepository, PostRepositoryError, PostStatus, Source},
    user::UserId,
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
        source: Source,
        submitted_by: Option<UserId>,
        submitted_at: DateTime<Utc>,
        status: PostStatus,
    ) -> Result<Post, Self::Err> {
        let mut posts = self.posts.write().await;
        let raw_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let post = Post {
            id: PostId::from(raw_id),
            source,
            status,
            last_posted: None,
            submitted_by,
            submitted_at,
        };
        posts.insert(raw_id, post.clone());
        Ok(post)
    }

    async fn find_by_id(&self, id: PostId) -> Result<Option<Post>, Self::Err> {
        Ok(self.posts.read().await.get(id.as_ref()).cloned())
    }

    async fn find_by_source(&self, source: &Source) -> Result<Option<Post>, Self::Err> {
        Ok(self
            .posts
            .read()
            .await
            .values()
            .find(|p| &p.source == source)
            .cloned())
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
    use url::Url;

    fn fixture_source() -> Source {
        Source::try_from(Url::parse("https://e621.net/posts/1").unwrap()).unwrap()
    }

    async fn create_default(repo: &InMemoryPostRepository) -> Post {
        repo.create(
            fixture_source(),
            None,
            Utc::now(),
            PostStatus::AwaitingModeration,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn create_then_find_by_id_roundtrip() {
        let repo = InMemoryPostRepository::new();
        let post = create_default(&repo).await;
        let found = repo.find_by_id(post.id).await.unwrap();
        assert_eq!(found.map(|p| p.id), Some(post.id));
    }

    #[tokio::test]
    async fn create_assigns_unique_ids() {
        let repo = InMemoryPostRepository::new();
        let a = create_default(&repo).await;
        let b = create_default(&repo).await;
        assert_ne!(a.id, b.id);
    }

    #[tokio::test]
    async fn create_persists_submitter_and_status() {
        let repo = InMemoryPostRepository::new();
        let when = Utc::now();
        let post = repo
            .create(
                fixture_source(),
                Some(UserId::from(42)),
                when,
                PostStatus::Banned,
            )
            .await
            .unwrap();
        assert_eq!(post.submitted_by, Some(UserId::from(42)));
        assert_eq!(post.submitted_at, when);
        assert_eq!(post.status, PostStatus::Banned);
        assert!(post.last_posted.is_none());
    }

    #[tokio::test]
    async fn remove_sets_status_to_deleted() {
        let repo = InMemoryPostRepository::new();
        let post = create_default(&repo).await;
        repo.remove(post.id).await.unwrap();
        let found = repo.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(found.status, PostStatus::Deleted);
    }

    #[tokio::test]
    async fn set_status_to_changes_status() {
        let repo = InMemoryPostRepository::new();
        let post = create_default(&repo).await;
        repo.set_status_to(post.id, PostStatus::Accepted)
            .await
            .unwrap();
        let found = repo.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(found.status, PostStatus::Accepted);
    }

    #[tokio::test]
    async fn mark_posted_updates_timestamp() {
        let repo = InMemoryPostRepository::new();
        let post = create_default(&repo).await;
        let when = Utc::now();
        repo.mark_posted(post.id, when).await.unwrap();
        let found = repo.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(found.last_posted, Some(when));
    }

    #[tokio::test]
    async fn find_by_source_locates_existing_post() {
        let repo = InMemoryPostRepository::new();
        let post = create_default(&repo).await;
        let found = repo.find_by_source(&post.source).await.unwrap();
        assert_eq!(found.map(|p| p.id), Some(post.id));
    }

    #[tokio::test]
    async fn find_by_source_unknown_returns_none() {
        let repo = InMemoryPostRepository::new();
        let other = Source::try_from(Url::parse("https://e621.net/posts/999").unwrap()).unwrap();
        assert!(repo.find_by_source(&other).await.unwrap().is_none());
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

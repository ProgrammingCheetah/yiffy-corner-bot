use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::elements::{
    post::{Post, PostId, PostRepository, PostRepositoryError, PostStatus, Source},
    tag::Tag,
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
        tags: Vec<Tag>,
        artists: Vec<Tag>,
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
            tags,
            artists,
            feed_position: None,
            last_posted: None,
            submitted_by,
            submitted_at,
            moderated_by: None,
            moderated_at: None,
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

    async fn set_status_to(&self, post_id: PostId, status: PostStatus) -> Result<(), Self::Err> {
        let mut posts = self.posts.write().await;
        let post = posts
            .get_mut(post_id.as_ref())
            .ok_or(PostRepositoryError::NotFound(post_id))?;
        post.status = status;
        Ok(())
    }

    async fn record_moderation(
        &self,
        id: PostId,
        by: UserId,
        at: DateTime<Utc>,
    ) -> Result<(), Self::Err> {
        let mut posts = self.posts.write().await;
        let post = posts
            .get_mut(id.as_ref())
            .ok_or(PostRepositoryError::NotFound(id))?;
        post.moderated_by = Some(by);
        post.moderated_at = Some(at);
        Ok(())
    }

    async fn set_tags(&self, id: PostId, tags: Vec<Tag>) -> Result<Post, Self::Err> {
        let mut posts = self.posts.write().await;
        let post = posts
            .get_mut(id.as_ref())
            .ok_or(PostRepositoryError::NotFound(id))?;
        post.tags = tags;
        Ok(post.clone())
    }

    async fn mark_posted(&self, id: PostId, at: DateTime<Utc>) -> Result<(), Self::Err> {
        let mut posts = self.posts.write().await;
        let post = posts
            .get_mut(id.as_ref())
            .ok_or(PostRepositoryError::NotFound(id))?;
        post.last_posted = Some(at);
        Ok(())
    }

    async fn list_by_status(&self, status: PostStatus) -> Result<Vec<Post>, Self::Err> {
        let mut matching: Vec<Post> = self
            .posts
            .read()
            .await
            .values()
            .filter(|p| p.status == status)
            .cloned()
            .collect();
        matching.sort_by_key(|p| (p.submitted_at, *p.id.as_ref()));
        Ok(matching)
    }

    async fn accept_into_feed(&self, id: PostId) -> Result<Post, Self::Err> {
        let mut posts = self.posts.write().await;
        let next_position = posts
            .values()
            .filter_map(|p| p.feed_position)
            .max()
            .unwrap_or(0)
            + 1;
        let post = posts
            .get_mut(id.as_ref())
            .ok_or(PostRepositoryError::NotFound(id))?;
        post.status = PostStatus::Accepted;
        if post.feed_position.is_none() {
            post.feed_position = Some(next_position);
        }
        Ok(post.clone())
    }

    async fn feed_end(&self) -> Result<u64, Self::Err> {
        Ok(self
            .posts
            .read()
            .await
            .values()
            .filter_map(|p| p.feed_position)
            .max()
            .unwrap_or(0))
    }

    async fn feed_after(&self, cursor: u64, up_to: u64) -> Result<Vec<Post>, Self::Err> {
        let mut entries: Vec<Post> = self
            .posts
            .read()
            .await
            .values()
            .filter(|p| {
                matches!(p.status, PostStatus::Accepted | PostStatus::Banned)
                    && p.feed_position
                        .is_some_and(|pos| pos > cursor && pos <= up_to)
            })
            .cloned()
            .collect();
        entries.sort_by_key(|p| p.feed_position);
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn source(id: u64) -> Source {
        Source::try_from(Url::parse(&format!("https://e621.net/posts/{id}")).unwrap()).unwrap()
    }

    async fn create_default(repo: &InMemoryPostRepository) -> Post {
        repo.create(
            source(1),
            vec![Tag::from("wolf")],
            vec![],
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
        let found = repo.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(found.id, post.id);
        assert_eq!(found.tags, vec![Tag::from("wolf")]);
        assert!(found.feed_position.is_none());
    }

    #[tokio::test]
    async fn create_assigns_unique_ids() {
        let repo = InMemoryPostRepository::new();
        let a = create_default(&repo).await;
        let b = repo
            .create(
                source(2),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::Accepted,
            )
            .await
            .unwrap();
        assert_ne!(a.id, b.id);
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
        assert!(repo.find_by_source(&source(99)).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn unknown_id_updates_return_not_found() {
        let repo = InMemoryPostRepository::new();
        assert!(matches!(
            repo.mark_posted(PostId::from(42), Utc::now())
                .await
                .unwrap_err(),
            PostRepositoryError::NotFound(_)
        ));
        assert!(matches!(
            repo.accept_into_feed(PostId::from(42)).await.unwrap_err(),
            PostRepositoryError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn accept_into_feed_assigns_monotonic_positions() {
        let repo = InMemoryPostRepository::new();
        let a = create_default(&repo).await;
        let b = repo
            .create(
                source(2),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::AwaitingModeration,
            )
            .await
            .unwrap();

        let a = repo.accept_into_feed(a.id).await.unwrap();
        let b = repo.accept_into_feed(b.id).await.unwrap();
        assert_eq!(a.feed_position, Some(1));
        assert_eq!(b.feed_position, Some(2));
        assert_eq!(a.status, PostStatus::Accepted);
        assert_eq!(repo.feed_end().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn accept_into_feed_is_idempotent_on_position() {
        let repo = InMemoryPostRepository::new();
        let post = create_default(&repo).await;
        let first = repo.accept_into_feed(post.id).await.unwrap();
        // A Banned entry re-accepted keeps its original slot.
        repo.set_status_to(post.id, PostStatus::Banned)
            .await
            .unwrap();
        let again = repo.accept_into_feed(post.id).await.unwrap();
        assert_eq!(first.feed_position, again.feed_position);
        assert_eq!(again.status, PostStatus::Accepted);
    }

    #[tokio::test]
    async fn feed_after_windows_and_orders() {
        let repo = InMemoryPostRepository::new();
        let mut accepted = Vec::new();
        for i in 1..=4u64 {
            let p = repo
                .create(
                    source(i),
                    vec![],
                    vec![],
                    None,
                    Utc::now(),
                    PostStatus::AwaitingModeration,
                )
                .await
                .unwrap();
            accepted.push(repo.accept_into_feed(p.id).await.unwrap());
        }
        // Banned entries stay visible (cached verdict, re-validated later)…
        repo.set_status_to(accepted[2].id, PostStatus::Banned)
            .await
            .unwrap();
        // …but Deleted ones drop out.
        repo.remove(accepted[3].id).await.unwrap();

        let window = repo.feed_after(1, 4).await.unwrap();
        let positions: Vec<u64> = window.iter().filter_map(|p| p.feed_position).collect();
        assert_eq!(positions, vec![2, 3]);
    }

    #[tokio::test]
    async fn feed_end_is_zero_when_empty() {
        let repo = InMemoryPostRepository::new();
        create_default(&repo).await; // not accepted → not in feed
        assert_eq!(repo.feed_end().await.unwrap(), 0);
        assert!(repo.feed_after(0, 10).await.unwrap().is_empty());
    }
}

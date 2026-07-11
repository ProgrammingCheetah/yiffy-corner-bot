//! Whole-pool submission — the "this is page 3 of a comic, take all of it"
//! path.
//!
//! When a reviewed e621 post belongs to one or more pools, a moderator can
//! [`inspect`] them (name, series-vs-collection category, size) and choose
//! one to [`stage`]: every pool page becomes a local Post with status
//! `Accepted` but **no feed position**. Positionless entries are invisible
//! to every consumer's cursor scan by construction, so the batch publishes
//! out-of-band (see `actors::pool_batch`) without ever double-posting
//! through the timer — the feed model's invariants stay untouched.
//!
//! Staged pages carry no submitter attribution (they're staff-curated, like
//! `/browse` saves); only the originally reviewed post keeps its submitter.
//! Staging is idempotent: pages already known locally are skipped, so
//! re-choosing a pool after a crash resumes instead of duplicating.

use std::collections::HashSet;

use chrono::Utc;
use domain::elements::{
    e621::{E621Fetcher, E621Pool},
    post::{Post, PostId, PostRepository, PostStatus, Source},
    tag::Tag,
    tag_policy::ForbiddenTagRepository,
    user::{Role, TelegramId, UserRepository},
};

use crate::commands::auth::require_role;
use crate::traits::handler_response::{HandlerError, HandlerResult};
use telemetry::{Event, SkipReason};

/// The pools a reviewed e621 post belongs to, fresh from the API, for the
/// moderator to inspect and choose from. The post's stored metadata is not
/// trusted — pool membership changes upstream.
pub async fn inspect<P, E>(
    actor: TelegramId,
    post_id: PostId,
    users: &impl UserRepository,
    posts: &P,
    e621: &E,
) -> HandlerResult<(Post, Vec<E621Pool>)>
where
    P: PostRepository,
    E: E621Fetcher,
{
    require_role(users, actor, Role::Moderator).await?;
    let post = posts
        .find_by_id(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or(HandlerError::PostNotFound(post_id))?;
    if !matches!(post.source, Source::E621(_)) {
        return Err(HandlerError::InvalidState(format!(
            "post {post_id} is not an e621 source"
        )));
    }
    let metadata = e621
        .fetch(&post.source)
        .await
        .map_err(|e| HandlerError::Fetch(e.to_string()))?;
    let pools = e621
        .pools(&metadata.pools)
        .await
        .map_err(|e| HandlerError::Fetch(e.to_string()))?;
    tracing::info!(
        event = %Event::PoolInspected, post_id = %post.id,
        pools = pools.len(), "pool list fetched for inspection"
    );
    Ok((post, pools))
}

/// A pool staged for out-of-band publication.
#[derive(Debug)]
pub struct StagedPool {
    pub pool: E621Pool,
    /// The reviewed post that triggered the pool submission. Its tags decide
    /// which posters receive the batch (the pool goes wherever this post
    /// would have gone).
    pub trigger: Post,
    /// Every page to publish, in pool order — `Accepted`, never positioned.
    pub posts: Vec<Post>,
    /// Pages already known locally (curated or queued elsewhere) — left as
    /// they are and NOT re-published.
    pub already_curated: usize,
    /// Pages refused for owning a globally forbidden tag.
    pub forbidden: usize,
    /// Pages e621 no longer serves (deleted or login-restricted).
    pub missing_upstream: usize,
}

/// Stage `pool_id` for the moderator `actor`: fetch the pool, walk its pages
/// in order, and adopt each one — creating positionless `Accepted` Posts,
/// folding in the triggering post if it's a member still awaiting
/// moderation, and skipping pages that are already curated, globally
/// forbidden, or gone upstream.
pub async fn stage<P, E, F>(
    actor: TelegramId,
    post_id: PostId,
    pool_id: u64,
    users: &impl UserRepository,
    posts: &P,
    e621: &E,
    forbidden: &F,
) -> HandlerResult<StagedPool>
where
    P: PostRepository,
    E: E621Fetcher,
    F: ForbiddenTagRepository,
{
    let moderator = require_role(users, actor, Role::Moderator).await?;
    let trigger = posts
        .find_by_id(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or(HandlerError::PostNotFound(post_id))?;
    let pool = e621
        .pools(&[pool_id])
        .await
        .map_err(|e| HandlerError::Fetch(e.to_string()))?
        .into_iter()
        .next()
        .ok_or_else(|| HandlerError::Fetch(format!("e621 pool {pool_id} not found")))?;
    let pages = e621
        .pool_posts(&pool)
        .await
        .map_err(|e| HandlerError::Fetch(e.to_string()))?;
    let missing_upstream = pool.post_ids.len().saturating_sub(pages.len());

    let global_forbidden: HashSet<Tag> = forbidden
        .list_all()
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .into_iter()
        .collect();

    let mut staged = Vec::with_capacity(pages.len());
    let mut already_curated = 0usize;
    let mut refused = 0usize;
    for page in pages {
        let existing = posts
            .find_by_source(&page.source)
            .await
            .map_err(|_| HandlerError::RepositoryError)?;
        match existing {
            // The reviewed post itself is a member: choosing the pool IS its
            // approval — fold it into the batch instead of the feed.
            Some(known)
                if known.id == trigger.id && known.status == PostStatus::AwaitingModeration =>
            {
                posts
                    .set_status_to(known.id, PostStatus::Accepted)
                    .await
                    .map_err(|_| HandlerError::RepositoryError)?;
                posts
                    .record_moderation(known.id, moderator.id, Utc::now())
                    .await
                    .map_err(|_| HandlerError::RepositoryError)?;
                staged.push(Post {
                    status: PostStatus::Accepted,
                    ..known
                });
            }
            Some(known) => {
                already_curated += 1;
                tracing::debug!(
                    event = %Event::PoolPostSkipped, reason = %SkipReason::AlreadyCurated,
                    pool_id, post_id = %known.id, source = %page.source.as_ref(),
                    "pool page already known locally"
                );
            }
            None => {
                if let Some(hit) = page.tags.iter().find(|t| global_forbidden.contains(*t)) {
                    refused += 1;
                    tracing::info!(
                        event = %Event::PoolPostSkipped, reason = %SkipReason::GlobalForbiddenTag,
                        pool_id, source = %page.source.as_ref(), tag = %hit,
                        "pool page owns a globally forbidden tag"
                    );
                    continue;
                }
                let created = posts
                    .create(
                        page.source,
                        page.tags,
                        page.artists,
                        None,
                        Utc::now(),
                        PostStatus::Accepted,
                    )
                    .await
                    .map_err(|_| HandlerError::RepositoryError)?;
                posts
                    .record_moderation(created.id, moderator.id, Utc::now())
                    .await
                    .map_err(|_| HandlerError::RepositoryError)?;
                staged.push(created);
            }
        }
    }

    tracing::info!(
        event = %Event::PoolSubmitted, pool_id, trigger = %trigger.id,
        moderator = %moderator.id, pages = staged.len(),
        already_curated, forbidden = refused, missing_upstream,
        "pool staged for out-of-band publication"
    );
    Ok(StagedPool {
        pool,
        trigger,
        posts: staged,
        already_curated,
        forbidden: refused,
        missing_upstream,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use domain::elements::e621::{E621PoolCategory, E621PostMetadata, FetchError};
    use persistence::in_memory::{
        post::InMemoryPostRepository, tag_policy::InMemoryForbiddenTagRepository,
        user::InMemoryUserRepository,
    };
    use url::Url;

    fn metadata(id: u64, tags: &[&str]) -> E621PostMetadata {
        E621PostMetadata {
            source: Source::try_from(Url::parse(&format!("https://e621.net/posts/{id}")).unwrap())
                .unwrap(),
            tags: tags.iter().map(|t| Tag::from(*t)).collect(),
            artists: vec![],
            file_url: Url::parse("https://static1.e621.net/data/full.png").unwrap(),
            mp4_url: None,
            preview_url: Url::parse("https://static1.e621.net/data/preview.png").unwrap(),
            artist_sources: vec![],
            pools: vec![7],
        }
    }

    /// One pool (id 7) whose pages are the stub's canned metadata.
    struct PoolFetcher {
        pool: E621Pool,
        pages: Vec<E621PostMetadata>,
    }
    #[async_trait]
    impl E621Fetcher for PoolFetcher {
        async fn fetch(&self, source: &Source) -> Result<E621PostMetadata, FetchError> {
            self.pages
                .iter()
                .find(|m| &m.source == source)
                .cloned()
                .ok_or_else(|| FetchError::NotFound(source.clone()))
        }
        async fn search(
            &self,
            _tags: &[Tag],
            _page: u32,
        ) -> Result<Vec<E621PostMetadata>, FetchError> {
            unimplemented!("not needed by pool tests")
        }
        async fn pools(&self, ids: &[u64]) -> Result<Vec<E621Pool>, FetchError> {
            Ok(ids
                .iter()
                .filter(|id| **id == self.pool.id)
                .map(|_| self.pool.clone())
                .collect())
        }
        async fn pool_posts(&self, _pool: &E621Pool) -> Result<Vec<E621PostMetadata>, FetchError> {
            Ok(self.pages.clone())
        }
    }

    struct Fixture {
        users: InMemoryUserRepository,
        posts: InMemoryPostRepository,
        forbidden: InMemoryForbiddenTagRepository,
        fetcher: PoolFetcher,
    }

    impl Fixture {
        fn new(page_ids: &[u64]) -> Self {
            Self {
                users: InMemoryUserRepository::new(),
                posts: InMemoryPostRepository::new(),
                forbidden: InMemoryForbiddenTagRepository::new(),
                fetcher: PoolFetcher {
                    pool: E621Pool {
                        id: 7,
                        name: "Cool_Comic".to_string(),
                        category: E621PoolCategory::Series,
                        post_ids: page_ids.to_vec(),
                        is_active: true,
                    },
                    pages: page_ids.iter().map(|id| metadata(*id, &["wolf"])).collect(),
                },
            }
        }

        async fn moderator(&self) -> TelegramId {
            let id = TelegramId::from(1);
            self.users
                .create(id, Role::Moderator, None, None)
                .await
                .unwrap();
            id
        }

        /// A pending submission for one pool page, as /suggest would leave it.
        async fn pending_page(&self, e621_id: u64) -> Post {
            self.posts
                .create(
                    Source::try_from(
                        Url::parse(&format!("https://e621.net/posts/{e621_id}")).unwrap(),
                    )
                    .unwrap(),
                    vec![Tag::from("wolf")],
                    vec![],
                    Some(domain::elements::user::UserId::from(42)),
                    Utc::now(),
                    PostStatus::AwaitingModeration,
                )
                .await
                .unwrap()
        }
    }

    #[tokio::test]
    async fn stages_every_page_in_pool_order_without_feed_positions() {
        let fx = Fixture::new(&[30, 10, 20]); // pool order ≠ id order
        let actor = fx.moderator().await;
        let trigger = fx.pending_page(10).await;

        let staged = stage(
            actor,
            trigger.id,
            7,
            &fx.users,
            &fx.posts,
            &fx.fetcher,
            &fx.forbidden,
        )
        .await
        .unwrap();

        let urls: Vec<String> = staged
            .posts
            .iter()
            .map(|p| p.source.as_ref().to_string())
            .collect();
        assert_eq!(
            urls,
            vec![
                "https://e621.net/posts/30",
                "https://e621.net/posts/10",
                "https://e621.net/posts/20",
            ]
        );
        for post in &staged.posts {
            assert_eq!(post.status, PostStatus::Accepted);
            assert_eq!(
                post.feed_position, None,
                "pool pages must stay off the feed"
            );
        }
        assert_eq!(staged.already_curated, 0);
    }

    #[tokio::test]
    async fn trigger_page_is_adopted_and_keeps_its_submitter() {
        let fx = Fixture::new(&[10, 20]);
        let actor = fx.moderator().await;
        let trigger = fx.pending_page(10).await;

        let staged = stage(
            actor,
            trigger.id,
            7,
            &fx.users,
            &fx.posts,
            &fx.fetcher,
            &fx.forbidden,
        )
        .await
        .unwrap();

        let adopted = staged.posts.iter().find(|p| p.id == trigger.id).unwrap();
        assert_eq!(adopted.submitted_by, trigger.submitted_by);
        let stored = fx.posts.find_by_id(trigger.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::Accepted);
        assert_eq!(stored.feed_position, None);
        // The sibling page is staff-curated: no attribution.
        let sibling = staged.posts.iter().find(|p| p.id != trigger.id).unwrap();
        assert_eq!(sibling.submitted_by, None);
    }

    #[tokio::test]
    async fn already_curated_and_forbidden_pages_are_skipped() {
        let mut fx = Fixture::new(&[10, 20, 30]);
        fx.fetcher.pages[2] = metadata(30, &["wolf", "gore"]);
        fx.forbidden.add(Tag::from("gore")).await.unwrap();
        let actor = fx.moderator().await;
        let trigger = fx.pending_page(10).await;
        // Page 20 was curated some other way already.
        let other = fx.pending_page(20).await;
        fx.posts.accept_into_feed(other.id).await.unwrap();

        let staged = stage(
            actor,
            trigger.id,
            7,
            &fx.users,
            &fx.posts,
            &fx.fetcher,
            &fx.forbidden,
        )
        .await
        .unwrap();

        assert_eq!(staged.posts.len(), 1); // just the trigger
        assert_eq!(staged.already_curated, 1);
        assert_eq!(staged.forbidden, 1);
        // Idempotence: a second stage finds everything known and creates nothing.
        let again = stage(
            actor,
            trigger.id,
            7,
            &fx.users,
            &fx.posts,
            &fx.fetcher,
            &fx.forbidden,
        )
        .await
        .unwrap();
        assert!(again.posts.is_empty());
        assert_eq!(again.already_curated, 2);
    }

    #[tokio::test]
    async fn missing_upstream_pages_are_counted() {
        let mut fx = Fixture::new(&[10, 20]);
        fx.fetcher.pages.pop(); // page 20 deleted upstream
        let actor = fx.moderator().await;
        let trigger = fx.pending_page(10).await;

        let staged = stage(
            actor,
            trigger.id,
            7,
            &fx.users,
            &fx.posts,
            &fx.fetcher,
            &fx.forbidden,
        )
        .await
        .unwrap();
        assert_eq!(staged.missing_upstream, 1);
        assert_eq!(staged.posts.len(), 1);
    }

    #[tokio::test]
    async fn plain_users_cannot_stage_pools() {
        let fx = Fixture::new(&[10]);
        let user = TelegramId::from(9);
        fx.users.create(user, Role::User, None, None).await.unwrap();
        let trigger = fx.pending_page(10).await;

        let err = stage(
            user,
            trigger.id,
            7,
            &fx.users,
            &fx.posts,
            &fx.fetcher,
            &fx.forbidden,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::NotAuthorized(_)));
    }

    #[tokio::test]
    async fn inspect_returns_fresh_pools_for_e621_posts_only() {
        let fx = Fixture::new(&[10]);
        let actor = fx.moderator().await;
        let trigger = fx.pending_page(10).await;

        let (post, pools) = inspect(actor, trigger.id, &fx.users, &fx.posts, &fx.fetcher)
            .await
            .unwrap();
        assert_eq!(post.id, trigger.id);
        assert_eq!(pools.len(), 1);
        assert_eq!(pools[0].display_name(), "Cool Comic");

        let tweet = fx
            .posts
            .create(
                Source::try_from(Url::parse("https://x.com/a/status/1").unwrap()).unwrap(),
                vec![Tag::from("wolf")],
                vec![],
                None,
                Utc::now(),
                PostStatus::AwaitingModeration,
            )
            .await
            .unwrap();
        let err = inspect(actor, tweet.id, &fx.users, &fx.posts, &fx.fetcher)
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidState(_)));
    }
}

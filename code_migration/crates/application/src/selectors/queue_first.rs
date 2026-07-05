//! The MVP selection policy from `design/domain.md`.
//!
//! Two disjoint pools (design Q6):
//!
//! - **Queue** (`find_due_post`): user-submitted Accepted Posts that have
//!   never been posted, oldest first. The queue is *peeked* — if the head
//!   matches this Poster's tag criteria it fires; otherwise the head stays
//!   put (another Poster may match it) and the caller falls back to the pool.
//! - **Saved pool** (`find_post`): admin-added (`/browse`) e621 Posts,
//!   Accepted ∪ Banned, picked at random. Only e621 Posts can be re-posted,
//!   and a `repost_cooldown` keeps the same Post from repeating too soon.
//!
//! Every e621 candidate is re-validated against *fresh* e621 tags at
//! selection time (tags are never persisted):
//!
//! - a globally forbidden tag flips the Post to `Banned` and skips it;
//! - a `Banned` Post whose fresh tags are clean flips back to `Accepted`;
//! - a Poster-forbidden tag skips the Post *for this Poster only* (no status
//!   change — other Posters may still use it);
//! - the Poster's `subscribed_tags` must all be present.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use domain::elements::{
    e621::E621Fetcher,
    post::{Post, PostRepository, PostSelectorStrategy, PostStatus, SelectorError, Source},
    poster::Poster,
    tag::Tag,
    tag_policy::ForbiddenTagRepository,
};
use rand::{RngExt, SeedableRng, rngs::SmallRng};

pub struct QueueFirstSelector<P, E, F> {
    poster: Poster,
    posts: Arc<P>,
    e621: Arc<E>,
    forbidden: Arc<F>,
    /// Minimum time between two publications of the same Post.
    repost_cooldown: Duration,
    rng: Mutex<SmallRng>,
}

impl<P, E, F> QueueFirstSelector<P, E, F> {
    pub fn new(
        poster: Poster,
        posts: Arc<P>,
        e621: Arc<E>,
        forbidden: Arc<F>,
        repost_cooldown: Duration,
    ) -> Self {
        Self {
            poster,
            posts,
            e621,
            forbidden,
            repost_cooldown,
            rng: Mutex::new(SmallRng::from_rng(&mut rand::rng())),
        }
    }

    /// Deterministic RNG for tests.
    pub fn with_rng_seed(mut self, seed: u64) -> Self {
        self.rng = Mutex::new(SmallRng::seed_from_u64(seed));
        self
    }
}

impl<P, E, F> QueueFirstSelector<P, E, F>
where
    P: PostRepository + Send + Sync,
    P::Err: std::fmt::Display,
    E: E621Fetcher,
    F: ForbiddenTagRepository,
    F::Err: std::fmt::Display,
{
    async fn globally_forbidden(&self) -> Result<HashSet<Tag>, SelectorError> {
        Ok(self
            .forbidden
            .list_all()
            .await
            .map_err(|e| SelectorError::Repository(e.to_string()))?
            .into_iter()
            .collect())
    }

    /// Tag verdict for one candidate against this Poster + the global policy.
    ///
    /// Fetches fresh e621 tags, applies the status flips described in the
    /// module docs, and returns whether the candidate is usable by this
    /// Poster right now.
    async fn validate_e621(
        &self,
        post: &Post,
        global_forbidden: &HashSet<Tag>,
    ) -> Result<bool, SelectorError> {
        let metadata = self
            .e621
            .fetch(&post.source)
            .await
            .map_err(|e| SelectorError::Fetch(e.to_string()))?;
        let tags: HashSet<Tag> = metadata.tags.into_iter().collect();

        if tags.iter().any(|t| global_forbidden.contains(t)) {
            if post.status != PostStatus::Banned {
                self.posts
                    .set_status_to(post.id, PostStatus::Banned)
                    .await
                    .map_err(|e| SelectorError::Repository(e.to_string()))?;
            }
            return Ok(false);
        }
        if post.status == PostStatus::Banned {
            // Fresh tags are clean again: the cached Banned verdict expires.
            self.posts
                .set_status_to(post.id, PostStatus::Accepted)
                .await
                .map_err(|e| SelectorError::Repository(e.to_string()))?;
        }

        if self
            .poster
            .forbidden_tags
            .iter()
            .any(|t| tags.contains(t))
        {
            return Ok(false);
        }
        Ok(self
            .poster
            .subscribed_tags
            .iter()
            .all(|t| tags.contains(t)))
    }

    fn off_cooldown(&self, post: &Post) -> bool {
        match post.last_posted {
            None => true,
            Some(at) => Utc::now() - at >= self.repost_cooldown,
        }
    }
}

#[async_trait::async_trait]
impl<P, E, F> PostSelectorStrategy for QueueFirstSelector<P, E, F>
where
    P: PostRepository + Send + Sync,
    P::Err: std::fmt::Display,
    E: E621Fetcher,
    F: ForbiddenTagRepository,
    F::Err: std::fmt::Display,
{
    async fn find_due_post(&self) -> Result<Option<Post>, SelectorError> {
        let accepted = self
            .posts
            .list_by_status(PostStatus::Accepted)
            .await
            .map_err(|e| SelectorError::Repository(e.to_string()))?;
        // The queue: user submissions that have never been posted, oldest
        // first (list_by_status guarantees the ordering). Peek the head only.
        let Some(head) = accepted
            .iter()
            .find(|p| p.submitted_by.is_some() && p.last_posted.is_none())
        else {
            return Ok(None);
        };

        let matches = match &head.source {
            Source::E621(_) => {
                let global = self.globally_forbidden().await?;
                self.validate_e621(head, &global).await?
            }
            // Non-e621 posts have zero tags (design), so they only satisfy a
            // Poster with no subscription filter.
            _ => self.poster.subscribed_tags.is_empty(),
        };
        Ok(matches.then(|| head.clone()))
    }

    async fn find_post(&self) -> Result<Post, SelectorError> {
        let mut pool: Vec<Post> = Vec::new();
        for status in [PostStatus::Accepted, PostStatus::Banned] {
            pool.extend(
                self.posts
                    .list_by_status(status)
                    .await
                    .map_err(|e| SelectorError::Repository(e.to_string()))?,
            );
        }
        // Saved pool: admin-added e621 only (design Q6 — submissions never
        // enter tag-based selection), and off repost cooldown.
        pool.retain(|p| {
            matches!(p.source, Source::E621(_))
                && p.submitted_by.is_none()
                && self.off_cooldown(p)
        });

        let global = self.globally_forbidden().await?;
        // Random order without replacement: validate candidates until one
        // passes or the pool is exhausted.
        while !pool.is_empty() {
            let idx = self.rng.lock().unwrap().random_range(0..pool.len());
            let candidate = pool.swap_remove(idx);
            if self.validate_e621(&candidate, &global).await? {
                return Ok(candidate);
            }
        }
        Err(SelectorError::NoMatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use async_trait::async_trait;
    use domain::elements::{
        cadence::PostInterval,
        e621::{E621Order, E621PostMetadata, FetchError},
        poster::PosterId,
        user::UserId,
    };
    use persistence::in_memory::{
        post::InMemoryPostRepository, tag_policy::InMemoryForbiddenTagRepository,
    };
    use url::Url;

    /// E621Fetcher stub: a fixed URL → tags map.
    struct StubFetcher(HashMap<Url, Vec<Tag>>);
    #[async_trait]
    impl E621Fetcher for StubFetcher {
        async fn fetch(&self, source: &Source) -> Result<E621PostMetadata, FetchError> {
            let url: &Url = source.as_ref();
            let tags = self
                .0
                .get(url)
                .cloned()
                .ok_or_else(|| FetchError::NotFound(source.clone()))?;
            Ok(E621PostMetadata {
                source: source.clone(),
                tags,
                file_url: Url::parse("https://static1.e621.net/data/full.png").unwrap(),
                preview_url: Url::parse("https://static1.e621.net/data/preview.png").unwrap(),
            })
        }
        async fn search(
            &self,
            _tags: &[Tag],
            _order: E621Order,
            _page: u32,
        ) -> Result<Vec<E621PostMetadata>, FetchError> {
            unimplemented!("not needed by selector tests")
        }
    }

    fn e621_url(id: u64) -> Url {
        Url::parse(&format!("https://e621.net/posts/{id}")).unwrap()
    }

    fn poster(subscribed: &[&str], forbidden: &[&str]) -> Poster {
        Poster {
            id: PosterId::from(1),
            subscribed_tags: subscribed.iter().map(|s| Tag::from(*s)).collect(),
            forbidden_tags: forbidden.iter().map(|s| Tag::from(*s)).collect(),
            time_interval: PostInterval::new(5).unwrap(),
        }
    }

    struct Fixture {
        posts: Arc<InMemoryPostRepository>,
        forbidden: Arc<InMemoryForbiddenTagRepository>,
        tags_by_url: HashMap<Url, Vec<Tag>>,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                posts: Arc::new(InMemoryPostRepository::new()),
                forbidden: Arc::new(InMemoryForbiddenTagRepository::new()),
                tags_by_url: HashMap::new(),
            }
        }

        async fn add_post(
            &mut self,
            url: Url,
            tags: &[&str],
            submitted_by: Option<u64>,
            status: PostStatus,
        ) -> Post {
            self.tags_by_url
                .insert(url.clone(), tags.iter().map(|s| Tag::from(*s)).collect());
            self.posts
                .create(
                    Source::try_from(url).unwrap(),
                    submitted_by.map(UserId::from),
                    Utc::now(),
                    status,
                )
                .await
                .unwrap()
        }

        fn selector(
            &self,
            poster: Poster,
        ) -> QueueFirstSelector<
            InMemoryPostRepository,
            StubFetcher,
            InMemoryForbiddenTagRepository,
        > {
            QueueFirstSelector::new(
                poster,
                self.posts.clone(),
                Arc::new(StubFetcher(self.tags_by_url.clone())),
                self.forbidden.clone(),
                Duration::days(7),
            )
            .with_rng_seed(0)
        }
    }

    #[tokio::test]
    async fn due_post_returns_matching_queue_head() {
        let mut fx = Fixture::new();
        let head = fx
            .add_post(e621_url(1), &["wolf", "male"], Some(42), PostStatus::Accepted)
            .await;
        let selector = fx.selector(poster(&["wolf"], &[]));
        let found = selector.find_due_post().await.unwrap();
        assert_eq!(found.map(|p| p.id), Some(head.id));
    }

    #[tokio::test]
    async fn due_post_none_when_head_lacks_subscribed_tag() {
        let mut fx = Fixture::new();
        fx.add_post(e621_url(1), &["cat"], Some(42), PostStatus::Accepted)
            .await;
        let selector = fx.selector(poster(&["wolf"], &[]));
        assert!(selector.find_due_post().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn due_post_none_when_queue_empty() {
        let fx = Fixture::new();
        let selector = fx.selector(poster(&[], &[]));
        assert!(selector.find_due_post().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn due_post_skips_admin_added_posts() {
        let mut fx = Fixture::new();
        // Admin-added (no submitter): not part of the queue.
        fx.add_post(e621_url(1), &["wolf"], None, PostStatus::Accepted)
            .await;
        let selector = fx.selector(poster(&["wolf"], &[]));
        assert!(selector.find_due_post().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn non_e621_head_matches_only_unfiltered_poster() {
        let mut fx = Fixture::new();
        let url = Url::parse("https://x.com/artist/status/1").unwrap();
        let head = fx.add_post(url, &[], Some(42), PostStatus::Accepted).await;

        let unfiltered = fx.selector(poster(&[], &[]));
        assert_eq!(
            unfiltered.find_due_post().await.unwrap().map(|p| p.id),
            Some(head.id)
        );

        let filtered = fx.selector(poster(&["wolf"], &[]));
        assert!(filtered.find_due_post().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn find_post_picks_matching_admin_e621_post() {
        let mut fx = Fixture::new();
        let saved = fx
            .add_post(e621_url(1), &["wolf", "male"], None, PostStatus::Accepted)
            .await;
        let selector = fx.selector(poster(&["wolf"], &[]));
        let found = selector.find_post().await.unwrap();
        assert_eq!(found.id, saved.id);
    }

    #[tokio::test]
    async fn find_post_excludes_user_submissions() {
        let mut fx = Fixture::new();
        // Matching tags, but user-submitted → pools are disjoint (design Q6).
        fx.add_post(e621_url(1), &["wolf"], Some(42), PostStatus::Accepted)
            .await;
        let selector = fx.selector(poster(&["wolf"], &[]));
        assert!(matches!(
            selector.find_post().await.unwrap_err(),
            SelectorError::NoMatch
        ));
    }

    #[tokio::test]
    async fn find_post_unbans_clean_banned_post() {
        let mut fx = Fixture::new();
        let banned = fx
            .add_post(e621_url(1), &["wolf"], None, PostStatus::Banned)
            .await;
        let selector = fx.selector(poster(&["wolf"], &[]));
        let found = selector.find_post().await.unwrap();
        assert_eq!(found.id, banned.id);
        let stored = fx.posts.find_by_id(banned.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::Accepted);
    }

    #[tokio::test]
    async fn find_post_bans_post_with_globally_forbidden_tag() {
        let mut fx = Fixture::new();
        let dirty = fx
            .add_post(e621_url(1), &["wolf", "gore"], None, PostStatus::Accepted)
            .await;
        fx.forbidden.add(Tag::from("gore")).await.unwrap();

        let selector = fx.selector(poster(&["wolf"], &[]));
        assert!(matches!(
            selector.find_post().await.unwrap_err(),
            SelectorError::NoMatch
        ));
        let stored = fx.posts.find_by_id(dirty.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::Banned);
    }

    #[tokio::test]
    async fn poster_forbidden_tag_skips_without_status_change() {
        let mut fx = Fixture::new();
        let skipped = fx
            .add_post(e621_url(1), &["wolf", "vore"], None, PostStatus::Accepted)
            .await;
        let selector = fx.selector(poster(&["wolf"], &["vore"]));
        assert!(matches!(
            selector.find_post().await.unwrap_err(),
            SelectorError::NoMatch
        ));
        // Poster-local exclusion: the Post stays Accepted for other Posters.
        let stored = fx.posts.find_by_id(skipped.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::Accepted);
    }

    #[tokio::test]
    async fn find_post_respects_repost_cooldown() {
        let mut fx = Fixture::new();
        let recent = fx
            .add_post(e621_url(1), &["wolf"], None, PostStatus::Accepted)
            .await;
        fx.posts
            .mark_posted(recent.id, Utc::now() - Duration::days(1))
            .await
            .unwrap();
        let selector = fx.selector(poster(&["wolf"], &[]));
        assert!(matches!(
            selector.find_post().await.unwrap_err(),
            SelectorError::NoMatch
        ));

        // Same post, but posted longer ago than the cooldown → eligible again.
        fx.posts
            .mark_posted(recent.id, Utc::now() - Duration::days(30))
            .await
            .unwrap();
        let selector = fx.selector(poster(&["wolf"], &[]));
        assert_eq!(selector.find_post().await.unwrap().id, recent.id);
    }

    #[tokio::test]
    async fn find_post_falls_through_to_a_valid_candidate() {
        let mut fx = Fixture::new();
        fx.add_post(e621_url(1), &["cat"], None, PostStatus::Accepted)
            .await;
        let matching = fx
            .add_post(e621_url(2), &["wolf"], None, PostStatus::Accepted)
            .await;
        let selector = fx.selector(poster(&["wolf"], &[]));
        assert_eq!(selector.find_post().await.unwrap().id, matching.id);
    }
}

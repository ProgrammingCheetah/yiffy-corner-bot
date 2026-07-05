//! `/browse` — Moderator+ curation from e621 into the saved pool.
//!
//! `search` pulls a page of random e621 posts for the given tags (with the
//! global REQUIRED tags injected and globally forbidden posts filtered out)
//! so the admin can pick. `save` then stores a chosen source as an
//! admin-added Post: auto-`Accepted`, no submitter — this is the pool
//! Posters draw tag-based picks from (disjoint from user submissions).

use chrono::Utc;
use domain::elements::{
    e621::{E621Fetcher, E621Order, E621PostMetadata},
    post::{Post, PostRepository, PostStatus, Source},
    tag::Tag,
    tag_policy::{ForbiddenTagRepository, RequiredTagRepository},
    user::{Role, TelegramId, UserRepository},
};
use url::Url;

use crate::commands::auth::require_role;
use crate::traits::handler_response::{HandlerError, HandlerResult};
use telemetry::Event;

#[derive(Debug)]
pub struct BrowseCommand {
    pub actor: TelegramId,
    pub tags: Vec<Tag>,
    /// 1-indexed e621 result page.
    pub page: u32,
}

pub async fn search<E, F, R>(
    cmd: BrowseCommand,
    users: &impl UserRepository,
    e621: &E,
    forbidden: &F,
    required: &R,
) -> HandlerResult<Vec<E621PostMetadata>>
where
    E: E621Fetcher,
    F: ForbiddenTagRepository,
    R: RequiredTagRepository,
{
    require_role(users, cmd.actor, Role::Moderator).await?;

    // Global REQUIRED tags apply to every e621 query (design).
    let mut query = cmd.tags;
    for tag in required
        .list_all()
        .await
        .map_err(|_| HandlerError::RepositoryError)?
    {
        if !query.contains(&tag) {
            query.push(tag);
        }
    }

    tracing::debug!(event = %Event::BrowseQueried, query = ?query, page = cmd.page, "browse: querying e621");
    let results = e621
        .search(&query, E621Order::Random, cmd.page)
        .await
        .map_err(|e| HandlerError::Fetch(e.to_string()))?;

    // Never show posts that own a globally forbidden tag.
    let mut clean = Vec::with_capacity(results.len());
    for metadata in results {
        let mut hit = false;
        for tag in &metadata.tags {
            if forbidden
                .contains(tag)
                .await
                .map_err(|_| HandlerError::RepositoryError)?
            {
                hit = true;
                break;
            }
        }
        if !hit {
            clean.push(metadata);
        }
    }
    tracing::info!(event = %Event::BrowseResults, results = clean.len(), "browse results (after forbidden filter)");
    Ok(clean)
}

#[derive(Debug)]
pub struct SaveCommand {
    pub actor: TelegramId,
    pub url: Url,
}

/// Save a browsed e621 post straight into the feed: admin-added, tags from
/// the e621 API, no moderation step.
pub async fn save<P, E>(
    cmd: SaveCommand,
    users: &impl UserRepository,
    posts: &P,
    e621: &E,
) -> HandlerResult<Post>
where
    P: PostRepository,
    E: E621Fetcher,
{
    require_role(users, cmd.actor, Role::Moderator).await?;
    let source =
        Source::try_from(cmd.url).map_err(|e| HandlerError::InvalidSource(e.to_string()))?;
    if !matches!(source, Source::E621(_)) {
        return Err(HandlerError::InvalidSource(
            "only e621 posts can be saved via /browse (use /suggest for other sources)".to_string(),
        ));
    }
    if let Some(existing) = posts
        .find_by_source(&source)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
    {
        return Err(HandlerError::DuplicateSubmission(existing.id));
    }
    let metadata = e621
        .fetch(&source)
        .await
        .map_err(|e| HandlerError::Fetch(e.to_string()))?;
    let post = posts
        .create(
            source,
            metadata.tags,
            metadata.artists,
            None,
            Utc::now(),
            PostStatus::Accepted,
        )
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    let post = posts
        .accept_into_feed(post.id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    tracing::info!(
        event = %Event::AcceptedIntoFeed, post_id = %post.id,
        source = %post.source.as_ref(), position = post.feed_position,
        "browse: saved into the feed"
    );
    Ok(post)
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use domain::elements::e621::FetchError;
    use persistence::in_memory::{
        post::InMemoryPostRepository,
        tag_policy::{InMemoryForbiddenTagRepository, InMemoryRequiredTagRepository},
        user::InMemoryUserRepository,
    };

    /// Records the query it was given and returns canned results.
    struct RecordingFetcher {
        results: Vec<E621PostMetadata>,
        seen_query: std::sync::Mutex<Vec<Tag>>,
    }
    #[async_trait]
    impl E621Fetcher for RecordingFetcher {
        async fn fetch(&self, _source: &Source) -> Result<E621PostMetadata, FetchError> {
            Ok(self.results.first().cloned().expect("stub has one result"))
        }
        async fn search(
            &self,
            tags: &[Tag],
            _order: E621Order,
            _page: u32,
        ) -> Result<Vec<E621PostMetadata>, FetchError> {
            *self.seen_query.lock().unwrap() = tags.to_vec();
            Ok(self.results.clone())
        }
    }

    fn metadata(id: u64, tags: &[&str]) -> E621PostMetadata {
        E621PostMetadata {
            source: Source::try_from(Url::parse(&format!("https://e621.net/posts/{id}")).unwrap())
                .unwrap(),
            tags: tags.iter().map(|t| Tag::from(*t)).collect(),
            artists: vec![Tag::from("coolwolf")],
            mp4_url: None,
            file_url: Url::parse("https://static1.e621.net/data/full.png").unwrap(),
            preview_url: Url::parse("https://static1.e621.net/data/preview.png").unwrap(),
            artist_sources: vec![],
        }
    }

    struct Fixture {
        users: InMemoryUserRepository,
        posts: InMemoryPostRepository,
        forbidden: InMemoryForbiddenTagRepository,
        required: InMemoryRequiredTagRepository,
    }

    async fn fixture() -> Fixture {
        let users = InMemoryUserRepository::new();
        users
            .create(TelegramId::from(1), Role::Moderator, None, None)
            .await
            .unwrap();
        users
            .create(TelegramId::from(2), Role::User, None, None)
            .await
            .unwrap();
        Fixture {
            users,
            posts: InMemoryPostRepository::new(),
            forbidden: InMemoryForbiddenTagRepository::new(),
            required: InMemoryRequiredTagRepository::new(),
        }
    }

    #[tokio::test]
    async fn search_injects_required_tags_and_filters_forbidden_results() {
        use domain::elements::tag_policy::{
            ForbiddenTagRepository as _, RequiredTagRepository as _,
        };
        let fx = fixture().await;
        fx.required.add(Tag::from("furry")).await.unwrap();
        fx.forbidden.add(Tag::from("gore")).await.unwrap();

        let fetcher = RecordingFetcher {
            results: vec![metadata(1, &["wolf"]), metadata(2, &["wolf", "gore"])],
            seen_query: std::sync::Mutex::new(vec![]),
        };
        let results = search(
            BrowseCommand {
                actor: TelegramId::from(1),
                tags: vec![Tag::from("wolf")],
                page: 1,
            },
            &fx.users,
            &fetcher,
            &fx.forbidden,
            &fx.required,
        )
        .await
        .unwrap();

        // The forbidden-tagged result is dropped.
        assert_eq!(results.len(), 1);
        // The REQUIRED tag was injected into the outgoing query.
        let query = fetcher.seen_query.lock().unwrap().clone();
        assert!(query.contains(&Tag::from("furry")));
        assert!(query.contains(&Tag::from("wolf")));
    }

    #[tokio::test]
    async fn plain_user_cannot_browse() {
        let fx = fixture().await;
        let fetcher = RecordingFetcher {
            results: vec![],
            seen_query: std::sync::Mutex::new(vec![]),
        };
        let err = search(
            BrowseCommand {
                actor: TelegramId::from(2),
                tags: vec![],
                page: 1,
            },
            &fx.users,
            &fetcher,
            &fx.forbidden,
            &fx.required,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::NotAuthorized(_)));
    }

    fn stub_fetcher() -> RecordingFetcher {
        RecordingFetcher {
            results: vec![metadata(1, &["wolf", "male"])],
            seen_query: std::sync::Mutex::new(vec![]),
        }
    }

    #[tokio::test]
    async fn save_enters_feed_with_e621_tags() {
        let fx = fixture().await;
        let post = save(
            SaveCommand {
                actor: TelegramId::from(1),
                url: Url::parse("https://e621.net/posts/1").unwrap(),
            },
            &fx.users,
            &fx.posts,
            &stub_fetcher(),
        )
        .await
        .unwrap();
        assert_eq!(post.status, PostStatus::Accepted);
        assert!(post.submitted_by.is_none());
        assert_eq!(post.feed_position, Some(1));
        assert_eq!(post.tags, vec![Tag::from("wolf"), Tag::from("male")]);
    }

    #[tokio::test]
    async fn save_rejects_non_e621_sources() {
        let fx = fixture().await;
        let err = save(
            SaveCommand {
                actor: TelegramId::from(1),
                url: Url::parse("https://x.com/a/status/1").unwrap(),
            },
            &fx.users,
            &fx.posts,
            &stub_fetcher(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidSource(_)));
    }

    #[tokio::test]
    async fn save_rejects_duplicates() {
        let fx = fixture().await;
        let cmd = || SaveCommand {
            actor: TelegramId::from(1),
            url: Url::parse("https://e621.net/posts/1").unwrap(),
        };
        save(cmd(), &fx.users, &fx.posts, &stub_fetcher())
            .await
            .unwrap();
        let err = save(cmd(), &fx.users, &fx.posts, &stub_fetcher())
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::DuplicateSubmission(_)));
    }
}

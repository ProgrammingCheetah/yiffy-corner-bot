//! `/browse` — Moderator+ curation from e621 into the saved pool.
//!
//! `search` pulls a page of e621 posts for the given query (with the
//! global REQUIRED tags injected and globally forbidden posts filtered out)
//! so the admin can pick. Ordering is the admin's choice: `order:…`
//! modifiers travel in the query like any tag; none means newest first. `save` then stores a chosen source as an
//! admin-added Post: auto-`Accepted`, no submitter — this is the pool
//! Posters draw tag-based picks from (disjoint from user submissions).

use chrono::Utc;
use domain::elements::{
    e621::{E621Fetcher, E621PostMetadata},
    fulfilling::FulfillingRequestRepository,
    post::{Post, PostRepository, PostStatus, Source},
    skiplist::SkipListRepository,
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

pub async fn search<E, F, R, P, S>(
    cmd: BrowseCommand,
    users: &impl UserRepository,
    e621: &E,
    forbidden: &F,
    required: &R,
    posts: &P,
    skips: &S,
) -> HandlerResult<Vec<E621PostMetadata>>
where
    E: E621Fetcher,
    F: ForbiddenTagRepository,
    R: RequiredTagRepository,
    P: PostRepository,
    S: SkipListRepository,
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
        .search(&query, cmd.page)
        .await
        .map_err(|e| HandlerError::Fetch(e.to_string()))?;

    // Never show posts that own a globally forbidden tag — and hide what
    // the system already knows: curated/queued sources (dedupe) and
    // explicitly skipped ones (the skiplist verdict).
    let mut clean = Vec::with_capacity(results.len());
    let mut dropped = 0usize;
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
        if !hit
            && posts
                .find_by_source(&metadata.source)
                .await
                .map_err(|_| HandlerError::RepositoryError)?
                .is_some()
        {
            hit = true;
            dropped += 1;
        }
        if !hit
            && skips
                .contains(&metadata.source)
                .await
                .map_err(|_| HandlerError::RepositoryError)?
        {
            hit = true;
            dropped += 1;
        }
        if !hit {
            clean.push(metadata);
        }
    }
    tracing::info!(
        event = %Event::BrowseResults, results = clean.len(), already_known = dropped,
        "browse results (after forbidden/dedupe/skiplist filters)"
    );
    Ok(clean)
}

#[derive(Debug)]
pub struct SaveCommand {
    pub actor: TelegramId,
    pub url: Url,
    /// Curated tags for non-e621 sources (merged extras for e621).
    pub tags: Vec<Tag>,
}

/// What the bot layer needs to react to a direct add.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum SaveOutcome {
    /// In the feed, admin-added, moderation audit recorded.
    Added(Post),
    /// Non-e621 source with no tags: nothing created — ask and retry.
    TagsNeeded,
}

/// Add a post from ANY source straight into the feed: admin-added, no
/// moderation round-trip. e621 sources auto-tag from the API (submitter
/// extras merged); every other source needs tags (`TagsNeeded` otherwise).
/// Content owning a globally forbidden tag is refused outright — a direct
/// add should fail loudly, not silently auto-ban.
///
/// While the curator's "fulfilling request" toggle is ON (see
/// [`set_fulfilling`]), the saved post is stamped with the request text and
/// its publication caption will read `Fulfilling request <text>`.
pub async fn save<P, E, F, FR>(
    cmd: SaveCommand,
    users: &impl UserRepository,
    posts: &P,
    e621: &E,
    forbidden: &F,
    fulfilling: &FR,
) -> HandlerResult<SaveOutcome>
where
    P: PostRepository,
    E: E621Fetcher,
    F: ForbiddenTagRepository,
    FR: FulfillingRequestRepository,
{
    let curator = require_role(users, cmd.actor, Role::Moderator).await?;
    let source =
        Source::try_from(cmd.url).map_err(|e| HandlerError::InvalidSource(e.to_string()))?;
    if let Some(existing) = posts
        .find_by_source(&source)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
    {
        return Err(HandlerError::DuplicateSubmission(existing.id));
    }
    let (extra_tags, extra_artists) = domain::elements::tag::split_artist_tags(cmd.tags);
    let (tags, artists) = match &source {
        Source::E621(_) => {
            let metadata = e621
                .fetch(&source)
                .await
                .map_err(|e| HandlerError::Fetch(e.to_string()))?;
            let mut tags = metadata.tags;
            for extra in extra_tags {
                if !tags.contains(&extra) {
                    tags.push(extra);
                }
            }
            let mut artists = metadata.artists;
            for artist in extra_artists {
                if !artists.contains(&artist) {
                    artists.push(artist);
                }
            }
            (tags, artists)
        }
        // Attribution alone doesn't tag an entry — content tags are still
        // required for the feed.
        _ if extra_tags.is_empty() => return Ok(SaveOutcome::TagsNeeded),
        _ => (extra_tags, extra_artists),
    };
    for tag in &tags {
        if forbidden
            .contains(tag)
            .await
            .map_err(|_| HandlerError::RepositoryError)?
        {
            let reason = forbidden
                .reason_for(tag)
                .await
                .map_err(|_| HandlerError::RepositoryError)?;
            return Err(HandlerError::InvalidState(match reason {
                Some(reason) => {
                    format!("refused: owns globally forbidden tag '{tag}' ({reason})")
                }
                None => format!("refused: owns globally forbidden tag '{tag}'"),
            }));
        }
    }
    let post = posts
        .create(
            source,
            tags,
            artists,
            None,
            Utc::now(),
            PostStatus::Accepted,
        )
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    let mut post = posts
        .accept_into_feed(post.id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    posts
        .record_moderation(post.id, curator.id, Utc::now())
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    if let Some(request) = fulfilling
        .active(cmd.actor)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
    {
        posts
            .set_fulfills(post.id, Some(&request))
            .await
            .map_err(|_| HandlerError::RepositoryError)?;
        post.fulfills = Some(request);
    }
    tracing::info!(
        event = %Event::AcceptedIntoFeed, post_id = %post.id,
        source = %post.source.as_ref(), position = post.feed_position,
        fulfills = post.fulfills.as_deref().unwrap_or(""),
        "saved directly into the feed"
    );
    Ok(SaveOutcome::Added(post))
}

/// Turn the curator's "fulfilling request" toggle ON with the request text
/// (replacing any previous one). Every browse save from here on is stamped
/// with the text until [`clear_fulfilling`] turns the toggle OFF.
pub async fn set_fulfilling<FR>(
    actor: TelegramId,
    request: &str,
    users: &impl UserRepository,
    fulfilling: &FR,
) -> HandlerResult<()>
where
    FR: FulfillingRequestRepository,
{
    require_role(users, actor, Role::Moderator).await?;
    fulfilling
        .set(actor, request, Utc::now())
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    tracing::info!(
        event = %Event::FulfillingChanged, by = actor.as_ref(), request,
        "fulfilling-request toggle ON"
    );
    Ok(())
}

/// Turn the curator's "fulfilling request" toggle OFF. Idempotent.
pub async fn clear_fulfilling<FR>(
    actor: TelegramId,
    users: &impl UserRepository,
    fulfilling: &FR,
) -> HandlerResult<()>
where
    FR: FulfillingRequestRepository,
{
    require_role(users, actor, Role::Moderator).await?;
    fulfilling
        .clear(actor)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    tracing::info!(
        event = %Event::FulfillingChanged, by = actor.as_ref(),
        "fulfilling-request toggle OFF"
    );
    Ok(())
}

/// The curator's active request text, if their toggle is ON.
pub async fn fulfilling_status<FR>(
    actor: TelegramId,
    users: &impl UserRepository,
    fulfilling: &FR,
) -> HandlerResult<Option<String>>
where
    FR: FulfillingRequestRepository,
{
    require_role(users, actor, Role::Moderator).await?;
    fulfilling
        .active(actor)
        .await
        .map_err(|_| HandlerError::RepositoryError)
}

/// Remember a browse result as skipped: this source is not for us, and
/// browse must never show it again. The verdict a human takes where dedupe
/// can't (video re-uploads have no pHash).
pub async fn skip<S>(
    actor: TelegramId,
    url: Url,
    users: &impl UserRepository,
    skips: &S,
) -> HandlerResult<Source>
where
    S: SkipListRepository,
{
    require_role(users, actor, Role::Moderator).await?;
    let source = Source::try_from(url).map_err(|e| HandlerError::InvalidSource(e.to_string()))?;
    skips
        .add(&source, actor, Utc::now())
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    tracing::info!(
        event = %Event::BrowseSkipped, source = %source.as_ref(),
        by = actor.as_ref(), "browse result skipped for good"
    );
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use domain::elements::e621::FetchError;
    use persistence::in_memory::{
        fulfilling::InMemoryFulfillingRequestRepository,
        post::InMemoryPostRepository,
        skiplist::InMemorySkipListRepository,
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
            _page: u32,
        ) -> Result<Vec<E621PostMetadata>, FetchError> {
            *self.seen_query.lock().unwrap() = tags.to_vec();
            Ok(self.results.clone())
        }
        async fn pools(
            &self,
            _ids: &[u64],
        ) -> Result<Vec<domain::elements::e621::E621Pool>, FetchError> {
            unimplemented!("not needed by browse tests")
        }
        async fn pool_posts(
            &self,
            _pool: &domain::elements::e621::E621Pool,
        ) -> Result<Vec<E621PostMetadata>, FetchError> {
            unimplemented!("not needed by browse tests")
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
            pools: vec![],
        }
    }

    struct Fixture {
        users: InMemoryUserRepository,
        posts: InMemoryPostRepository,
        forbidden: InMemoryForbiddenTagRepository,
        required: InMemoryRequiredTagRepository,
        skips: InMemorySkipListRepository,
        fulfilling: InMemoryFulfillingRequestRepository,
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
            skips: InMemorySkipListRepository::new(),
            fulfilling: InMemoryFulfillingRequestRepository::new(),
        }
    }

    #[tokio::test]
    async fn search_injects_required_tags_and_filters_forbidden_results() {
        use domain::elements::tag_policy::{
            ForbiddenTagRepository as _, RequiredTagRepository as _,
        };
        let fx = fixture().await;
        fx.required.add(Tag::from("furry")).await.unwrap();
        fx.forbidden.add(Tag::from("gore"), None).await.unwrap();

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
            &fx.posts,
            &fx.skips,
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
            &fx.posts,
            &fx.skips,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::NotAuthorized(_)));
    }

    #[tokio::test]
    async fn search_hides_known_and_skipped_sources() {
        use domain::elements::skiplist::SkipListRepository as _;

        let fx = fixture().await;
        // Post 1 is already curated; post 2 was skipped for good.
        fx.posts
            .create(
                Source::try_from(Url::parse("https://e621.net/posts/1").unwrap()).unwrap(),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::Accepted,
            )
            .await
            .unwrap();
        fx.skips
            .add(
                &Source::try_from(Url::parse("https://e621.net/posts/2").unwrap()).unwrap(),
                TelegramId::from(1),
                Utc::now(),
            )
            .await
            .unwrap();

        let fetcher = RecordingFetcher {
            results: vec![
                metadata(1, &["wolf"]),
                metadata(2, &["wolf"]),
                metadata(3, &["wolf"]),
            ],
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
            &fx.posts,
            &fx.skips,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].source.as_ref().as_str(),
            "https://e621.net/posts/3"
        );
    }

    #[tokio::test]
    async fn skip_records_the_verdict_and_needs_moderator() {
        use domain::elements::skiplist::SkipListRepository as _;

        let fx = fixture().await;
        let url = Url::parse("https://e621.net/posts/5").unwrap();
        let source = skip(TelegramId::from(1), url.clone(), &fx.users, &fx.skips)
            .await
            .unwrap();
        assert!(fx.skips.contains(&source).await.unwrap());

        assert!(matches!(
            skip(TelegramId::from(2), url, &fx.users, &fx.skips)
                .await
                .unwrap_err(),
            HandlerError::NotAuthorized(_)
        ));
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
        let SaveOutcome::Added(post) = save(
            SaveCommand {
                actor: TelegramId::from(1),
                url: Url::parse("https://e621.net/posts/1").unwrap(),
                tags: vec![],
            },
            &fx.users,
            &fx.posts,
            &stub_fetcher(),
            &fx.forbidden,
            &fx.fulfilling,
        )
        .await
        .unwrap() else {
            panic!("expected Added");
        };
        assert_eq!(post.status, PostStatus::Accepted);
        assert!(post.submitted_by.is_none());
        assert_eq!(post.feed_position, Some(1));
        assert_eq!(post.tags, vec![Tag::from("wolf"), Tag::from("male")]);
        assert_eq!(post.fulfills, None, "toggle OFF → no stamp");
    }

    #[tokio::test]
    async fn save_stamps_the_active_fulfilling_request_until_cleared() {
        let fx = fixture().await;
        let actor = TelegramId::from(1);
        set_fulfilling(actor, "more wolves", &fx.users, &fx.fulfilling)
            .await
            .unwrap();
        assert_eq!(
            fulfilling_status(actor, &fx.users, &fx.fulfilling)
                .await
                .unwrap()
                .as_deref(),
            Some("more wolves")
        );

        // Every save while the toggle is ON carries the stamp…
        for id in 1..=2u64 {
            let SaveOutcome::Added(post) = save(
                SaveCommand {
                    actor,
                    url: Url::parse(&format!("https://e621.net/posts/{id}")).unwrap(),
                    tags: vec![],
                },
                &fx.users,
                &fx.posts,
                &stub_fetcher(),
                &fx.forbidden,
                &fx.fulfilling,
            )
            .await
            .unwrap() else {
                panic!("expected Added");
            };
            assert_eq!(post.fulfills.as_deref(), Some("more wolves"));
            let stored = fx.posts.find_by_id(post.id).await.unwrap().unwrap();
            assert_eq!(stored.fulfills.as_deref(), Some("more wolves"));
        }

        // …and none after it's turned OFF.
        clear_fulfilling(actor, &fx.users, &fx.fulfilling)
            .await
            .unwrap();
        let SaveOutcome::Added(post) = save(
            SaveCommand {
                actor,
                url: Url::parse("https://e621.net/posts/3").unwrap(),
                tags: vec![],
            },
            &fx.users,
            &fx.posts,
            &stub_fetcher(),
            &fx.forbidden,
            &fx.fulfilling,
        )
        .await
        .unwrap() else {
            panic!("expected Added");
        };
        assert_eq!(post.fulfills, None);
    }

    #[tokio::test]
    async fn fulfilling_toggle_needs_moderator() {
        let fx = fixture().await;
        assert!(matches!(
            set_fulfilling(TelegramId::from(2), "wish", &fx.users, &fx.fulfilling)
                .await
                .unwrap_err(),
            HandlerError::NotAuthorized(_)
        ));
        assert!(matches!(
            clear_fulfilling(TelegramId::from(2), &fx.users, &fx.fulfilling)
                .await
                .unwrap_err(),
            HandlerError::NotAuthorized(_)
        ));
    }

    #[tokio::test]
    async fn save_non_e621_needs_tags_then_adds_directly() {
        let fx = fixture().await;
        let outcome = save(
            SaveCommand {
                actor: TelegramId::from(1),
                url: Url::parse("https://x.com/a/status/1").unwrap(),
                tags: vec![],
            },
            &fx.users,
            &fx.posts,
            &stub_fetcher(),
            &fx.forbidden,
            &fx.fulfilling,
        )
        .await
        .unwrap();
        assert!(matches!(outcome, SaveOutcome::TagsNeeded));

        let SaveOutcome::Added(post) = save(
            SaveCommand {
                actor: TelegramId::from(1),
                url: Url::parse("https://x.com/a/status/1").unwrap(),
                tags: vec![Tag::from("wolf")],
            },
            &fx.users,
            &fx.posts,
            &stub_fetcher(),
            &fx.forbidden,
            &fx.fulfilling,
        )
        .await
        .unwrap() else {
            panic!("expected Added");
        };
        assert_eq!(post.tags, vec![Tag::from("wolf")]);
        assert!(post.feed_position.is_some());
    }

    #[tokio::test]
    async fn save_refuses_globally_forbidden_content() {
        use domain::elements::tag_policy::ForbiddenTagRepository as _;
        let fx = fixture().await;
        fx.forbidden.add(Tag::from("gore"), None).await.unwrap();
        let err = save(
            SaveCommand {
                actor: TelegramId::from(1),
                url: Url::parse("https://x.com/a/status/1").unwrap(),
                tags: vec![Tag::from("gore")],
            },
            &fx.users,
            &fx.posts,
            &stub_fetcher(),
            &fx.forbidden,
            &fx.fulfilling,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidState(_)));
    }

    #[tokio::test]
    async fn save_rejects_duplicates() {
        let fx = fixture().await;
        let cmd = || SaveCommand {
            actor: TelegramId::from(1),
            url: Url::parse("https://e621.net/posts/1").unwrap(),
            tags: vec![],
        };
        save(
            cmd(),
            &fx.users,
            &fx.posts,
            &stub_fetcher(),
            &fx.forbidden,
            &fx.fulfilling,
        )
        .await
        .unwrap();
        let err = save(
            cmd(),
            &fx.users,
            &fx.posts,
            &stub_fetcher(),
            &fx.forbidden,
            &fx.fulfilling,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::DuplicateSubmission(_)));
    }
}

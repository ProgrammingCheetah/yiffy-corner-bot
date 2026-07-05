//! `/suggest <url>` — the plain-User submission workflow.
//!
//! Any non-banned User (auto-registered on first contact) can submit art by
//! source URL. The URL must parse into a known [`Source`]; duplicates are
//! rejected by source lookup. e621 submissions are validated against the
//! global forbidden-tag policy immediately — a hit auto-Bans instead of
//! entering the queue. Everything else lands in `AwaitingModeration`, and the
//! caller receives the Moderator + Owner list to fan the review DM out to.

use chrono::Utc;
use domain::elements::{
    e621::E621Fetcher,
    post::{Post, PostRepository, PostStatus, Source},
    tag_policy::ForbiddenTagRepository,
    user::{Role, TelegramId, User, UserRepository},
};
use url::Url;

use crate::traits::handler_response::{HandlerError, HandlerResult};
use telemetry::{Event, RejectReason};

#[derive(Debug)]
pub struct SuggestCommand {
    pub submitter: TelegramId,
    /// Telegram display name at the moment of submission; cached for the
    /// "Submitted by <name>" attribution when the Post is eventually published.
    pub display_name: Option<String>,
    pub url: Url,
}

/// What the bot layer needs to react to a submission.
#[derive(Debug)]
pub enum SuggestOutcome {
    /// Submission entered the moderation queue; DM these reviewers.
    Queued { post: Post, reviewers: Vec<User> },
    /// Submission owned a globally forbidden tag and was auto-Banned.
    AutoBanned { post: Post },
}

pub async fn handle<P, E, F>(
    cmd: SuggestCommand,
    users: &impl UserRepository,
    posts: &P,
    e621: &E,
    forbidden: &F,
) -> HandlerResult<SuggestOutcome>
where
    P: PostRepository,
    E: E621Fetcher,
    F: ForbiddenTagRepository,
{
    // Auto-register unknown submitters; refresh the cached display name for
    // known ones (it feeds published attribution).
    let submitter = match users
        .find_by_telegram_id(cmd.submitter)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
    {
        Some(user) => {
            if user.display_name != cmd.display_name {
                users
                    .set_display_name(user.id, cmd.display_name.clone())
                    .await
                    .map_err(|_| HandlerError::RepositoryError)?;
            }
            user
        }
        None => users
            .create(cmd.submitter, Role::User, None, cmd.display_name.clone())
            .await
            .map_err(|_| HandlerError::RepositoryError)?,
    };

    if submitter.is_banned {
        tracing::warn!(event = %Event::SubmissionRejected, reason = %RejectReason::SubmitterBanned, user_id = %submitter.id, "submission rejected: user is banned");
        return Err(HandlerError::SubmitterBanned);
    }

    let source = Source::try_from(cmd.url).map_err(|e| {
        tracing::info!(event = %Event::SubmissionRejected, reason = %RejectReason::InvalidSource, user_id = %submitter.id, error = %e, "submission rejected: bad source URL");
        HandlerError::InvalidSource(e.to_string())
    })?;

    if let Some(existing) = posts
        .find_by_source(&source)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
    {
        tracing::info!(
            event = %Event::SubmissionRejected, reason = %RejectReason::DuplicateSource,
            user_id = %submitter.id, existing_post = %existing.id, source = %source.as_ref(),
            "submission rejected: duplicate source"
        );
        return Err(HandlerError::DuplicateSubmission(existing.id));
    }

    // e621 submissions are tag-checked at the door: a globally forbidden tag
    // auto-Bans (cached verdict, re-validated at selection). Non-e621 posts
    // have zero tags (design) — nothing to check.
    let status = match &source {
        Source::E621(_) => {
            let metadata = e621
                .fetch(&source)
                .await
                .map_err(|e| HandlerError::Fetch(e.to_string()))?;
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
            if hit {
                PostStatus::Banned
            } else {
                PostStatus::AwaitingModeration
            }
        }
        _ => PostStatus::AwaitingModeration,
    };

    let post = posts
        .create(source, Some(submitter.id), Utc::now(), status.clone())
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    tracing::info!(
        event = %Event::SubmissionCreated,
        post_id = %post.id, user_id = %submitter.id,
        source = %post.source.as_ref(), status = %post.status,
        "submission created"
    );

    match status {
        PostStatus::Banned => {
            tracing::warn!(
                event = %Event::SubmissionAutoBanned,
                post_id = %post.id, user_id = %submitter.id,
                "submission owned a globally forbidden tag"
            );
            Ok(SuggestOutcome::AutoBanned { post })
        }
        _ => {
            let mut reviewers = users
                .list_by_role(Role::Moderator)
                .await
                .map_err(|_| HandlerError::RepositoryError)?;
            reviewers.extend(
                users
                    .list_by_role(Role::Owner)
                    .await
                    .map_err(|_| HandlerError::RepositoryError)?,
            );
            Ok(SuggestOutcome::Queued { post, reviewers })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use async_trait::async_trait;
    use domain::elements::{
        e621::{E621Order, E621PostMetadata, FetchError},
        tag::Tag,
    };
    use persistence::in_memory::{
        post::InMemoryPostRepository, tag_policy::InMemoryForbiddenTagRepository,
        user::InMemoryUserRepository,
    };

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
                artist_sources: vec![],
            })
        }
        async fn search(
            &self,
            _tags: &[Tag],
            _order: E621Order,
            _page: u32,
        ) -> Result<Vec<E621PostMetadata>, FetchError> {
            unimplemented!("not needed by suggest tests")
        }
    }

    struct Fixture {
        users: InMemoryUserRepository,
        posts: InMemoryPostRepository,
        forbidden: InMemoryForbiddenTagRepository,
        fetcher: StubFetcher,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                users: InMemoryUserRepository::new(),
                posts: InMemoryPostRepository::new(),
                forbidden: InMemoryForbiddenTagRepository::new(),
                fetcher: StubFetcher(HashMap::new()),
            }
        }

        fn with_e621_post(mut self, url: &str, tags: &[&str]) -> Self {
            self.fetcher.0.insert(
                Url::parse(url).unwrap(),
                tags.iter().map(|t| Tag::from(*t)).collect(),
            );
            self
        }

        async fn suggest(&self, telegram_id: i64, url: &str) -> HandlerResult<SuggestOutcome> {
            handle(
                SuggestCommand {
                    submitter: TelegramId::from(telegram_id),
                    display_name: Some("Tester".to_string()),
                    url: Url::parse(url).unwrap(),
                },
                &self.users,
                &self.posts,
                &self.fetcher,
                &self.forbidden,
            )
            .await
        }
    }

    #[tokio::test]
    async fn clean_e621_submission_queues_and_lists_reviewers() {
        let fx = Fixture::new().with_e621_post("https://e621.net/posts/1", &["wolf"]);
        fx.users
            .create(TelegramId::from(1), Role::Owner, None, None)
            .await
            .unwrap();
        fx.users
            .create(TelegramId::from(2), Role::Moderator, None, None)
            .await
            .unwrap();

        let outcome = fx.suggest(42, "https://e621.net/posts/1").await.unwrap();
        let SuggestOutcome::Queued { post, reviewers } = outcome else {
            panic!("expected Queued");
        };
        assert_eq!(post.status, PostStatus::AwaitingModeration);
        assert!(post.submitted_by.is_some());
        assert_eq!(reviewers.len(), 2); // the Moderator + the Owner
    }

    #[tokio::test]
    async fn unknown_submitter_is_auto_registered_with_display_name() {
        let fx = Fixture::new().with_e621_post("https://e621.net/posts/1", &["wolf"]);
        fx.suggest(42, "https://e621.net/posts/1").await.unwrap();
        let user = fx
            .users
            .find_by_telegram_id(TelegramId::from(42))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(user.role, Role::User);
        assert_eq!(user.display_name.as_deref(), Some("Tester"));
    }

    #[tokio::test]
    async fn forbidden_tag_auto_bans() {
        let fx = Fixture::new().with_e621_post("https://e621.net/posts/1", &["wolf", "gore"]);
        fx.forbidden.add(Tag::from("gore")).await.unwrap();

        let outcome = fx.suggest(42, "https://e621.net/posts/1").await.unwrap();
        let SuggestOutcome::AutoBanned { post } = outcome else {
            panic!("expected AutoBanned");
        };
        assert_eq!(post.status, PostStatus::Banned);
    }

    #[tokio::test]
    async fn banned_user_cannot_submit() {
        let fx = Fixture::new().with_e621_post("https://e621.net/posts/1", &["wolf"]);
        let user = fx
            .users
            .create(TelegramId::from(42), Role::User, None, None)
            .await
            .unwrap();
        fx.users.set_banned(user.id, true).await.unwrap();

        let err = fx
            .suggest(42, "https://e621.net/posts/1")
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::SubmitterBanned));
    }

    #[tokio::test]
    async fn duplicate_source_is_rejected() {
        let fx = Fixture::new().with_e621_post("https://e621.net/posts/1", &["wolf"]);
        fx.suggest(42, "https://e621.net/posts/1").await.unwrap();
        let err = fx
            .suggest(43, "https://e621.net/posts/1")
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::DuplicateSubmission(_)));
    }

    #[tokio::test]
    async fn unknown_host_is_rejected() {
        let fx = Fixture::new();
        let err = fx
            .suggest(42, "https://example.com/a.png")
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidSource(_)));
    }

    #[tokio::test]
    async fn non_e621_source_queues_without_tag_check() {
        let fx = Fixture::new(); // fetcher knows no URLs — must not be called
        let outcome = fx
            .suggest(42, "https://x.com/artist/status/1")
            .await
            .unwrap();
        assert!(matches!(outcome, SuggestOutcome::Queued { .. }));
    }
}

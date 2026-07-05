//! Moderator queue operations: approve, reject, delete, and list.
//!
//! Approve/Reject act on `AwaitingModeration` Posts only — `Rejected` and
//! `Deleted` are permanent human decisions (design), so double-moderation is
//! an `InvalidState` error rather than a silent overwrite. Delete is the
//! soft-delete used for takedowns and queue cleanup; it works from any status.

use domain::elements::{
    post::{Post, PostId, PostRepository, PostStatus},
    user::{Role, TelegramId, UserRepository},
};

use crate::commands::auth::require_role;
use crate::traits::handler_response::{HandlerError, HandlerResult};
use telemetry::Event;

#[derive(Debug)]
pub struct ModerateCommand {
    pub actor: TelegramId,
    pub post_id: PostId,
}

async fn transition<P: PostRepository>(
    cmd: ModerateCommand,
    users: &impl UserRepository,
    posts: &P,
    to: PostStatus,
) -> HandlerResult<Post> {
    let moderator = require_role(users, cmd.actor, Role::Moderator).await?;
    tracing::debug!(event = %Event::ModerationRequested, moderator_id = %moderator.id, post_id = %cmd.post_id, "moderation requested");
    let post = posts
        .find_by_id(cmd.post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or(HandlerError::PostNotFound(cmd.post_id))?;
    if post.status != PostStatus::AwaitingModeration {
        tracing::warn!(
            event = %Event::ModerationInvalidState,
            post_id = %post.id, status = %post.status,
            "moderation rejected: post not awaiting moderation"
        );
        return Err(HandlerError::InvalidState(format!(
            "post {} is {:?}, not awaiting moderation",
            post.id, post.status
        )));
    }
    posts
        .set_status_to(post.id, to.clone())
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    tracing::info!(event = %Event::ModerationApplied, post_id = %post.id, decision = %to, "moderation decision applied");
    Ok(Post { status: to, ..post })
}

pub async fn approve<P: PostRepository>(
    cmd: ModerateCommand,
    users: &impl UserRepository,
    posts: &P,
) -> HandlerResult<Post> {
    transition(cmd, users, posts, PostStatus::Accepted).await
}

pub async fn reject<P: PostRepository>(
    cmd: ModerateCommand,
    users: &impl UserRepository,
    posts: &P,
) -> HandlerResult<Post> {
    transition(cmd, users, posts, PostStatus::Rejected).await
}

/// Soft-delete from any status (takedowns, queue cleanup). Moderator+.
pub async fn delete<P: PostRepository>(
    cmd: ModerateCommand,
    users: &impl UserRepository,
    posts: &P,
) -> HandlerResult<()> {
    require_role(users, cmd.actor, Role::Moderator).await?;
    posts
        .find_by_id(cmd.post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or(HandlerError::PostNotFound(cmd.post_id))?;
    tracing::info!(event = %Event::PostDeleted, post_id = %cmd.post_id, "post soft-deleted");
    posts
        .remove(cmd.post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)
}

/// The moderation queue, oldest first. Moderator+.
pub async fn queue<P: PostRepository>(
    actor: TelegramId,
    users: &impl UserRepository,
    posts: &P,
) -> HandlerResult<Vec<Post>> {
    require_role(users, actor, Role::Moderator).await?;
    posts
        .list_by_status(PostStatus::AwaitingModeration)
        .await
        .map_err(|_| HandlerError::RepositoryError)
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;
    use domain::elements::post::Source;
    use persistence::in_memory::{post::InMemoryPostRepository, user::InMemoryUserRepository};
    use url::Url;

    struct Fixture {
        users: InMemoryUserRepository,
        posts: InMemoryPostRepository,
    }

    impl Fixture {
        async fn new() -> Self {
            let users = InMemoryUserRepository::new();
            users
                .create(TelegramId::from(1), Role::Moderator, None, None)
                .await
                .unwrap();
            users
                .create(TelegramId::from(2), Role::User, None, None)
                .await
                .unwrap();
            Self {
                users,
                posts: InMemoryPostRepository::new(),
            }
        }

        async fn awaiting_post(&self, id: u64) -> Post {
            self.posts
                .create(
                    Source::try_from(Url::parse(&format!("https://e621.net/posts/{id}")).unwrap())
                        .unwrap(),
                    None,
                    Utc::now(),
                    PostStatus::AwaitingModeration,
                )
                .await
                .unwrap()
        }
    }

    fn cmd(actor: i64, post_id: PostId) -> ModerateCommand {
        ModerateCommand {
            actor: TelegramId::from(actor),
            post_id,
        }
    }

    #[tokio::test]
    async fn moderator_can_approve() {
        let fx = Fixture::new().await;
        let post = fx.awaiting_post(1).await;
        let approved = approve(cmd(1, post.id), &fx.users, &fx.posts)
            .await
            .unwrap();
        assert_eq!(approved.status, PostStatus::Accepted);
        let stored = fx.posts.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::Accepted);
    }

    #[tokio::test]
    async fn moderator_can_reject() {
        let fx = Fixture::new().await;
        let post = fx.awaiting_post(1).await;
        reject(cmd(1, post.id), &fx.users, &fx.posts).await.unwrap();
        let stored = fx.posts.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::Rejected);
    }

    #[tokio::test]
    async fn plain_user_cannot_moderate() {
        let fx = Fixture::new().await;
        let post = fx.awaiting_post(1).await;
        let err = approve(cmd(2, post.id), &fx.users, &fx.posts)
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::NotAuthorized(_)));
    }

    #[tokio::test]
    async fn unknown_actor_cannot_moderate() {
        let fx = Fixture::new().await;
        let post = fx.awaiting_post(1).await;
        let err = approve(cmd(99, post.id), &fx.users, &fx.posts)
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::UnknownActor));
    }

    #[tokio::test]
    async fn double_moderation_is_invalid_state() {
        let fx = Fixture::new().await;
        let post = fx.awaiting_post(1).await;
        approve(cmd(1, post.id), &fx.users, &fx.posts)
            .await
            .unwrap();
        let err = reject(cmd(1, post.id), &fx.users, &fx.posts)
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidState(_)));
    }

    #[tokio::test]
    async fn delete_soft_deletes_from_any_status() {
        let fx = Fixture::new().await;
        let post = fx.awaiting_post(1).await;
        approve(cmd(1, post.id), &fx.users, &fx.posts)
            .await
            .unwrap();
        delete(cmd(1, post.id), &fx.users, &fx.posts).await.unwrap();
        let stored = fx.posts.find_by_id(post.id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::Deleted);
    }

    #[tokio::test]
    async fn queue_lists_awaiting_posts_only() {
        let fx = Fixture::new().await;
        let a = fx.awaiting_post(1).await;
        let b = fx.awaiting_post(2).await;
        approve(cmd(1, a.id), &fx.users, &fx.posts).await.unwrap();

        let queue = queue(TelegramId::from(1), &fx.users, &fx.posts)
            .await
            .unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].id, b.id);
    }

    #[tokio::test]
    async fn moderating_missing_post_is_not_found() {
        let fx = Fixture::new().await;
        let err = approve(cmd(1, PostId::from(999)), &fx.users, &fx.posts)
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::PostNotFound(_)));
    }
}

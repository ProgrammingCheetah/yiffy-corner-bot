//! "Request more of this": a viewer on a published post tells the
//! moderators what they'd like more of. A relay, not a moderation object —
//! nothing is stored; the wish goes straight to Moderators + Owners with
//! the post attached, so they can curate accordingly.

use domain::elements::{
    post::{Post, PostId, PostRepository},
    shadow_ban::ShadowBanRepository,
    user::{Role, TelegramId, User, UserRepository},
};
use telemetry::Event;

use crate::traits::handler_response::{HandlerError, HandlerResult};

/// What the bot layer needs to relay the wish.
#[derive(Debug)]
pub struct MoreRequest {
    pub post: Post,
    pub reviewers: Vec<User>,
}

/// A viewer (any Telegram user — registration not required) asks for more
/// like `post_id`. `wish` is their answer to "what would you like more of?".
/// A shadowbanned requester gets the same flow back with an empty reviewer
/// list — the wish quietly goes nowhere.
pub async fn request_more<P, SB>(
    requester: TelegramId,
    post_id: PostId,
    wish: &str,
    posts: &P,
    users: &impl UserRepository,
    shadow: &SB,
) -> HandlerResult<MoreRequest>
where
    P: PostRepository,
    SB: ShadowBanRepository,
{
    let post = posts
        .find_by_id(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or(HandlerError::PostNotFound(post_id))?;

    if shadow
        .contains(requester)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
    {
        tracing::info!(
            event = %Event::ShadowDropped, post_id = %post_id,
            who = requester.as_ref(), flow = "more", "shadowbanned wish dropped"
        );
        return Ok(MoreRequest {
            post,
            reviewers: Vec::new(),
        });
    }

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
    tracing::info!(
        event = %Event::MoreRequested, post_id = %post_id,
        requester = requester.as_ref(), wish, "more-of request relayed"
    );
    Ok(MoreRequest { post, reviewers })
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;
    use domain::elements::post::{PostStatus, Source};
    use persistence::in_memory::{
        post::InMemoryPostRepository, shadow_ban::InMemoryShadowBanRepository,
        user::InMemoryUserRepository,
    };
    use url::Url;

    #[tokio::test]
    async fn relays_to_moderators_and_owners_and_checks_the_post() {
        let users = InMemoryUserRepository::new();
        users
            .create(TelegramId::from(1), Role::Moderator, None, None)
            .await
            .unwrap();
        users
            .create(TelegramId::from(2), Role::Owner, None, None)
            .await
            .unwrap();
        let posts = InMemoryPostRepository::new();
        let shadow = InMemoryShadowBanRepository::new();
        let post = posts
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

        let relay = request_more(TelegramId::from(99), post.id, "more wolves", &posts, &users, &shadow)
            .await
            .unwrap();
        assert_eq!(relay.post.id, post.id);
        assert_eq!(relay.reviewers.len(), 2);

        assert!(matches!(
            request_more(TelegramId::from(99), PostId::from(777), "?", &posts, &users, &shadow)
                .await
                .unwrap_err(),
            HandlerError::PostNotFound(_)
        ));
    }
}

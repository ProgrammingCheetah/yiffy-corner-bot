//! `/postinfo <id>` — the moderator's x-ray view of a Post: workflow state,
//! provenance, moderation audit, publish history, and report pressure.

use domain::elements::{
    post::{Post, PostId, PostRepository},
    publisher::{Publication, PublicationRepository},
    report::ReportRepository,
    user::{Role, TelegramId, User, UserRepository},
};

use crate::commands::auth::require_role;
use crate::traits::handler_response::{HandlerError, HandlerResult};

/// Everything `/postinfo` shows, pre-joined.
#[derive(Debug)]
pub struct PostInfo {
    pub post: Post,
    pub submitter: Option<User>,
    pub moderator: Option<User>,
    pub publications: Vec<Publication>,
    pub report_count: u64,
}

pub async fn post_info<P, PB, RR>(
    actor: TelegramId,
    post_id: PostId,
    users: &impl UserRepository,
    posts: &P,
    publications: &PB,
    reports: &RR,
) -> HandlerResult<PostInfo>
where
    P: PostRepository,
    PB: PublicationRepository,
    RR: ReportRepository,
{
    require_role(users, actor, Role::Moderator).await?;
    let post = posts
        .find_by_id(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or(HandlerError::PostNotFound(post_id))?;

    let lookup = |id| async move {
        match id {
            None => Ok::<_, HandlerError>(None),
            Some(user_id) => users
                .find_by_id(user_id)
                .await
                .map_err(|_| HandlerError::RepositoryError),
        }
    };
    let submitter = lookup(post.submitted_by).await?;
    let moderator = lookup(post.moderated_by).await?;
    let publications = publications
        .list_for(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    let report_count = reports
        .count_for(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    Ok(PostInfo {
        post,
        submitter,
        moderator,
        publications,
        report_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;
    use domain::elements::post::{PostStatus, Source};
    use persistence::in_memory::{
        post::InMemoryPostRepository, publication::InMemoryPublicationRepository,
        report::InMemoryReportRepository, user::InMemoryUserRepository,
    };
    use url::Url;

    #[tokio::test]
    async fn gathers_the_full_picture_for_moderators_only() {
        let users = InMemoryUserRepository::new();
        let moderator = users
            .create(
                TelegramId::from(1),
                Role::Moderator,
                None,
                Some("Mod".into()),
            )
            .await
            .unwrap();
        users
            .create(TelegramId::from(2), Role::User, None, Some("Subby".into()))
            .await
            .unwrap();
        let posts = InMemoryPostRepository::new();
        let publications = InMemoryPublicationRepository::new();
        let reports = InMemoryReportRepository::new();

        let submitter = users
            .find_by_telegram_id(TelegramId::from(2))
            .await
            .unwrap()
            .unwrap();
        let post = posts
            .create(
                Source::try_from(Url::parse("https://e621.net/posts/1").unwrap()).unwrap(),
                vec![],
                vec![],
                Some(submitter.id),
                Utc::now(),
                PostStatus::AwaitingModeration,
            )
            .await
            .unwrap();
        posts.accept_into_feed(post.id).await.unwrap();
        posts
            .record_moderation(post.id, moderator.id, Utc::now())
            .await
            .unwrap();

        let info = post_info(
            TelegramId::from(1),
            post.id,
            &users,
            &posts,
            &publications,
            &reports,
        )
        .await
        .unwrap();
        assert_eq!(
            info.submitter.unwrap().display_name.as_deref(),
            Some("Subby")
        );
        assert_eq!(info.moderator.unwrap().display_name.as_deref(), Some("Mod"));
        assert_eq!(info.post.feed_position, Some(1));

        // Plain users are denied.
        let err = post_info(
            TelegramId::from(2),
            post.id,
            &users,
            &posts,
            &publications,
            &reports,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::NotAuthorized(_)));
    }
}

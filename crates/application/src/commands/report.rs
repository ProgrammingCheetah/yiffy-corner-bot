//! The report loop: viewers press ⚠️ on a published post; moderators take
//! the post down or dismiss the reports.

use chrono::Utc;
use domain::elements::{
    post::{Post, PostId, PostRepository},
    publisher::{Publication, PublicationRepository},
    report::{Report, ReportRepository},
    user::{Role, TelegramId, User, UserRepository},
};
use telemetry::Event;

use crate::commands::auth::require_role;
use crate::traits::handler_response::{HandlerError, HandlerResult};

/// What the bot layer needs to react to a viewer report.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // Duplicate is the rare arm; boxing New buys nothing
pub enum ReportOutcome {
    /// First report from this viewer: notify these reviewers.
    New {
        post: Post,
        reviewers: Vec<User>,
        total_reports: u64,
        /// The reporter's (trimmed, non-empty) reason, for the reviewer DM.
        reason: Option<String>,
    },
    /// This viewer already reported this post (abuse dedupe) — acknowledge
    /// quietly, no re-notification.
    Duplicate,
}

/// A viewer (any Telegram user — registration not required) reports a post.
/// `reason` is what they answered to "why?" — `None` only on paths that
/// cannot collect one (legacy buttons with the reporter's DMs closed).
pub async fn report<P, RR>(
    reporter: TelegramId,
    post_id: PostId,
    reason: Option<String>,
    posts: &P,
    reports: &RR,
    users: &impl UserRepository,
) -> HandlerResult<ReportOutcome>
where
    P: PostRepository,
    RR: ReportRepository,
{
    let post = posts
        .find_by_id(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or(HandlerError::PostNotFound(post_id))?;

    let reason = reason
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());
    let fresh = reports
        .add(Report {
            post_id,
            reporter,
            reported_at: Utc::now(),
            reason: reason.clone(),
        })
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    if !fresh {
        tracing::debug!(
            event = %Event::ReportDuplicate, post_id = %post_id,
            reporter = reporter.as_ref(), "repeat report ignored"
        );
        return Ok(ReportOutcome::Duplicate);
    }

    let total_reports = reports
        .count_for(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    tracing::info!(
        event = %Event::PostReported, post_id = %post_id,
        reporter = reporter.as_ref(), total_reports,
        reason = reason.as_deref().unwrap_or("(none)"), "post reported"
    );

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
    Ok(ReportOutcome::New {
        post,
        reviewers,
        total_reports,
        reason,
    })
}

/// One reported post with all its open reports, newest report first.
#[derive(Debug)]
pub struct ReportedPost {
    pub post: Post,
    pub reports: Vec<Report>,
}

/// The moderation overview: every not-yet-deleted post that has open
/// reports, ordered by most recent report. Taken-down posts drop out
/// (soft-deleted), dismissed ones too (their reports are cleared).
pub async fn overview<P, RR>(
    actor: TelegramId,
    users: &impl UserRepository,
    posts: &P,
    reports: &RR,
) -> HandlerResult<Vec<ReportedPost>>
where
    P: PostRepository,
    RR: ReportRepository,
{
    use std::collections::HashMap;

    use domain::elements::post::PostStatus;

    require_role(users, actor, Role::Moderator).await?;
    let all = reports
        .list_all()
        .await
        .map_err(|_| HandlerError::RepositoryError)?;

    // Group per post, keeping the newest-first order of first appearance.
    let mut order: Vec<PostId> = Vec::new();
    let mut grouped: HashMap<PostId, Vec<Report>> = HashMap::new();
    for report in all {
        if !grouped.contains_key(&report.post_id) {
            order.push(report.post_id);
        }
        grouped.entry(report.post_id).or_default().push(report);
    }

    let mut out = Vec::with_capacity(order.len());
    for post_id in order {
        let Some(post) = posts
            .find_by_id(post_id)
            .await
            .map_err(|_| HandlerError::RepositoryError)?
        else {
            continue;
        };
        if post.status == PostStatus::Deleted {
            continue; // already taken down — nothing left to act on
        }
        let reports = grouped.remove(&post_id).unwrap_or_default();
        out.push(ReportedPost { post, reports });
    }
    Ok(out)
}

/// Moderator takedown: soft-delete the Post and hand back every recorded
/// delivery so the bot layer can delete the channel messages.
pub async fn take_down<P, PB>(
    actor: TelegramId,
    post_id: PostId,
    users: &impl UserRepository,
    posts: &P,
    publications: &PB,
) -> HandlerResult<Vec<Publication>>
where
    P: PostRepository,
    PB: PublicationRepository,
{
    let moderator = require_role(users, actor, Role::Moderator).await?;
    posts
        .find_by_id(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or(HandlerError::PostNotFound(post_id))?;
    posts
        .remove(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    posts
        .record_moderation(post_id, moderator.id, Utc::now())
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    let deliveries = publications
        .list_for(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    tracing::info!(
        event = %Event::PostTakenDown, post_id = %post_id,
        deliveries = deliveries.len(), "post taken down"
    );
    Ok(deliveries)
}

/// Moderator dismissal: clear the post's reports (a fresh wave re-notifies).
pub async fn dismiss<RR>(
    actor: TelegramId,
    post_id: PostId,
    users: &impl UserRepository,
    reports: &RR,
) -> HandlerResult<()>
where
    RR: ReportRepository,
{
    require_role(users, actor, Role::Moderator).await?;
    reports
        .clear_for(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    tracing::info!(event = %Event::ReportsDismissed, post_id = %post_id, "reports dismissed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use domain::elements::post::{PostStatus, Source};
    use persistence::in_memory::{
        post::InMemoryPostRepository, publication::InMemoryPublicationRepository,
        report::InMemoryReportRepository, user::InMemoryUserRepository,
    };
    use url::Url;

    struct Fixture {
        users: InMemoryUserRepository,
        posts: InMemoryPostRepository,
        reports: InMemoryReportRepository,
        publications: InMemoryPublicationRepository,
    }

    async fn fixture() -> (Fixture, PostId) {
        let users = InMemoryUserRepository::new();
        users
            .create(TelegramId::from(1), Role::Moderator, None, None)
            .await
            .unwrap();
        users
            .create(TelegramId::from(2), Role::User, None, None)
            .await
            .unwrap();
        let posts = InMemoryPostRepository::new();
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
        (
            Fixture {
                users,
                posts,
                reports: InMemoryReportRepository::new(),
                publications: InMemoryPublicationRepository::new(),
            },
            post.id,
        )
    }

    #[tokio::test]
    async fn first_report_notifies_repeat_is_duplicate() {
        let (fx, post_id) = fixture().await;
        let outcome = report(
            TelegramId::from(99),
            post_id,
            Some("  untagged gore  ".to_string()),
            &fx.posts,
            &fx.reports,
            &fx.users,
        )
        .await
        .unwrap();
        let ReportOutcome::New {
            reviewers,
            total_reports,
            reason,
            ..
        } = outcome
        else {
            panic!("expected New");
        };
        assert_eq!(reviewers.len(), 1); // the moderator
        assert_eq!(total_reports, 1);
        assert_eq!(reason.as_deref(), Some("untagged gore")); // trimmed

        let again = report(
            TelegramId::from(99),
            post_id,
            Some("still gore".to_string()),
            &fx.posts,
            &fx.reports,
            &fx.users,
        )
        .await
        .unwrap();
        assert!(matches!(again, ReportOutcome::Duplicate));
    }

    #[tokio::test]
    async fn reporting_unknown_post_fails() {
        let (fx, _) = fixture().await;
        let err = report(
            TelegramId::from(99),
            PostId::from(777),
            Some("untagged gore".to_string()),
            &fx.posts,
            &fx.reports,
            &fx.users,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::PostNotFound(_)));
    }

    #[tokio::test]
    async fn take_down_soft_deletes_and_returns_deliveries() {
        let (fx, post_id) = fixture().await;
        fx.publications
            .record(Publication {
                post_id,
                chat_id: -100,
                message_id: 7,
                published_at: Utc::now(),
            })
            .await
            .unwrap();

        let deliveries = take_down(
            TelegramId::from(1),
            post_id,
            &fx.users,
            &fx.posts,
            &fx.publications,
        )
        .await
        .unwrap();
        assert_eq!(deliveries.len(), 1);
        let stored = fx.posts.find_by_id(post_id).await.unwrap().unwrap();
        assert_eq!(stored.status, PostStatus::Deleted);
    }

    #[tokio::test]
    async fn plain_user_cannot_take_down_or_dismiss() {
        let (fx, post_id) = fixture().await;
        assert!(matches!(
            take_down(
                TelegramId::from(2),
                post_id,
                &fx.users,
                &fx.posts,
                &fx.publications
            )
            .await
            .unwrap_err(),
            HandlerError::NotAuthorized(_)
        ));
        assert!(matches!(
            dismiss(TelegramId::from(2), post_id, &fx.users, &fx.reports)
                .await
                .unwrap_err(),
            HandlerError::NotAuthorized(_)
        ));
    }

    #[tokio::test]
    async fn overview_groups_reports_and_drops_taken_down_posts() {
        let (fx, post_id) = fixture().await;
        let second = fx
            .posts
            .create(
                Source::try_from(Url::parse("https://e621.net/posts/2").unwrap()).unwrap(),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::Accepted,
            )
            .await
            .unwrap();
        for (reporter, post, why) in [
            (99, post_id, "gore"),
            (98, post_id, "untagged"),
            (99, second.id, "off-topic"),
        ] {
            report(
                TelegramId::from(reporter),
                post,
                Some(why.to_string()),
                &fx.posts,
                &fx.reports,
                &fx.users,
            )
            .await
            .unwrap();
        }

        let all = overview(TelegramId::from(1), &fx.users, &fx.posts, &fx.reports)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        let first = all.iter().find(|r| r.post.id == post_id).unwrap();
        assert_eq!(first.reports.len(), 2);

        // A takedown removes the post from the overview.
        take_down(
            TelegramId::from(1),
            second.id,
            &fx.users,
            &fx.posts,
            &fx.publications,
        )
        .await
        .unwrap();
        let all = overview(TelegramId::from(1), &fx.users, &fx.posts, &fx.reports)
            .await
            .unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].post.id, post_id);

        // Plain users don't get the overview.
        assert!(matches!(
            overview(TelegramId::from(2), &fx.users, &fx.posts, &fx.reports)
                .await
                .unwrap_err(),
            HandlerError::NotAuthorized(_)
        ));
    }

    #[tokio::test]
    async fn dismiss_clears_so_fresh_reports_renotify() {
        let (fx, post_id) = fixture().await;
        report(
            TelegramId::from(99),
            post_id,
            Some("untagged gore".to_string()),
            &fx.posts,
            &fx.reports,
            &fx.users,
        )
        .await
        .unwrap();
        dismiss(TelegramId::from(1), post_id, &fx.users, &fx.reports)
            .await
            .unwrap();
        let outcome = report(
            TelegramId::from(99),
            post_id,
            Some("untagged gore".to_string()),
            &fx.posts,
            &fx.reports,
            &fx.users,
        )
        .await
        .unwrap();
        assert!(matches!(outcome, ReportOutcome::New { .. }));
    }
}

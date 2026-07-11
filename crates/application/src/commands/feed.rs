//! Feed inspection: what's still ahead of a given post in the feed.
//!
//! The feed model (design/domain.md): consumers walk `(cursor, end]` in
//! feed order. Here a Post's own `feed_position` is resolved as the cursor,
//! so a moderator can see everything that comes after that post — the
//! to-be-posted backlog from that point, before any per-Poster tag filter.

use domain::elements::{
    post::{Post, PostId, PostRepository},
    poster::{Poster, PosterId, PosterRepository},
    user::{Role, TelegramId, UserRepository},
};

use crate::commands::auth::require_role;
use crate::selectors::feed::refusal_for;
use crate::traits::handler_response::{HandlerError, HandlerResult};

/// The feed from one post onward.
#[derive(Debug)]
pub struct FeedSlice {
    /// The post the cursor was resolved from.
    pub anchor: Post,
    /// Everything after the anchor in feed order. Accepted and Banned
    /// entries (Banned re-validates at consume time); `media_gone` entries
    /// are shelved and don't appear.
    pub entries: Vec<Post>,
    /// The feed end at the time of the scan.
    pub feed_end: u64,
}

/// Everything that comes after `post_id` in the feed.
pub async fn after_post<P>(
    actor: TelegramId,
    post_id: PostId,
    users: &impl UserRepository,
    posts: &P,
) -> HandlerResult<FeedSlice>
where
    P: PostRepository,
{
    require_role(users, actor, Role::Moderator).await?;
    let anchor = posts
        .find_by_id(post_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or(HandlerError::PostNotFound(post_id))?;
    let Some(cursor) = anchor.feed_position else {
        return Err(HandlerError::InvalidState(format!(
            "post #{post_id} has no feed position — it is not in the feed"
        )));
    };
    let feed_end = posts
        .feed_end()
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    let entries = posts
        .feed_after(cursor, feed_end)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    Ok(FeedSlice {
        anchor,
        entries,
        feed_end,
    })
}

/// One entry of a poster's upcoming queue, with that poster's verdict.
#[derive(Debug)]
pub struct PosterQueueEntry {
    pub post: Post,
    /// Why this poster would pass it over; `None` = it would post this
    /// (modulo the consume-time tag re-validation for e621 entries).
    pub refusal: Option<String>,
}

/// One poster's view of the feed ahead of its cursor.
#[derive(Debug)]
pub struct PosterQueue {
    pub poster: Poster,
    pub feed_end: u64,
    pub entries: Vec<PosterQueueEntry>,
}

/// What one poster still has ahead of it: every feed entry in
/// `(cursor, end]` with the poster's own eligibility verdict attached.
pub async fn poster_queue<P, PR>(
    actor: TelegramId,
    poster_id: PosterId,
    users: &impl UserRepository,
    posters: &PR,
    posts: &P,
) -> HandlerResult<PosterQueue>
where
    P: PostRepository,
    PR: PosterRepository,
{
    use std::collections::HashSet;

    use domain::elements::tag::Tag;

    require_role(users, actor, Role::Moderator).await?;
    let poster = posters
        .find_by_id(poster_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or_else(|| HandlerError::InvalidState(format!("no poster #{poster_id}")))?;
    let feed_end = posts
        .feed_end()
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    let entries = posts
        .feed_after(poster.cursor, feed_end)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .into_iter()
        .map(|post| {
            let tags: HashSet<Tag> = post.tags.iter().cloned().collect();
            let refusal = refusal_for(&poster, &tags).map(|r| r.to_string());
            PosterQueueEntry { post, refusal }
        })
        .collect();
    Ok(PosterQueue {
        poster,
        feed_end,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;
    use domain::elements::post::{PostStatus, Source};
    use persistence::in_memory::{post::InMemoryPostRepository, user::InMemoryUserRepository};
    use url::Url;

    async fn fixture() -> (InMemoryUserRepository, InMemoryPostRepository) {
        let users = InMemoryUserRepository::new();
        users
            .create(TelegramId::from(1), Role::Moderator, None, None)
            .await
            .unwrap();
        users
            .create(TelegramId::from(2), Role::User, None, None)
            .await
            .unwrap();
        (users, InMemoryPostRepository::new())
    }

    async fn feed_post(posts: &InMemoryPostRepository, n: u64) -> Post {
        let post = posts
            .create(
                Source::try_from(Url::parse(&format!("https://e621.net/posts/{n}")).unwrap())
                    .unwrap(),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::Accepted,
            )
            .await
            .unwrap();
        posts.accept_into_feed(post.id).await.unwrap()
    }

    #[tokio::test]
    async fn slice_lists_everything_after_the_anchor_in_order() {
        let (users, posts) = fixture().await;
        let first = feed_post(&posts, 1).await;
        let second = feed_post(&posts, 2).await;
        let third = feed_post(&posts, 3).await;

        let slice = after_post(TelegramId::from(1), first.id, &users, &posts)
            .await
            .unwrap();
        assert_eq!(slice.anchor.id, first.id);
        assert_eq!(slice.feed_end, third.feed_position.unwrap());
        assert_eq!(
            slice.entries.iter().map(|p| p.id).collect::<Vec<_>>(),
            vec![second.id, third.id]
        );

        // From the last post the slice is empty — nothing left to post.
        let tail = after_post(TelegramId::from(1), third.id, &users, &posts)
            .await
            .unwrap();
        assert!(tail.entries.is_empty());
    }

    #[tokio::test]
    async fn poster_queue_attaches_the_posters_verdicts() {
        use domain::elements::cadence::PostInterval;
        use domain::elements::tag::Tag;
        use domain::elements::tag_rule::{TagLiteral, TagTerm};
        use persistence::in_memory::poster::InMemoryPosterRepository;

        let (users, posts) = fixture().await;
        let anchor = feed_post(&posts, 1).await;
        // Wolf-tagged entry matches; the bare one misses the subscription.
        let wolf = posts
            .create(
                Source::try_from(Url::parse("https://e621.net/posts/2").unwrap()).unwrap(),
                vec![Tag::from("wolf")],
                vec![],
                None,
                Utc::now(),
                PostStatus::Accepted,
            )
            .await
            .unwrap();
        posts.accept_into_feed(wolf.id).await.unwrap();
        feed_post(&posts, 3).await;

        let posters = InMemoryPosterRepository::new();
        let poster = posters
            .create(
                vec![TagTerm(vec![TagLiteral::Has(Tag::from("wolf"))])],
                vec![],
                PostInterval::try_from(30).unwrap(),
                anchor.feed_position.unwrap(),
            )
            .await
            .unwrap();

        let queue = poster_queue(TelegramId::from(1), poster.id, &users, &posters, &posts)
            .await
            .unwrap();
        assert_eq!(queue.entries.len(), 2);
        assert!(queue.entries[0].refusal.is_none()); // the wolf entry
        assert!(
            queue.entries[1]
                .refusal
                .as_deref()
                .is_some_and(|r| r.contains("missing"))
        );

        assert!(matches!(
            poster_queue(
                TelegramId::from(1),
                domain::elements::poster::PosterId::from(99),
                &users,
                &posters,
                &posts
            )
            .await
            .unwrap_err(),
            HandlerError::InvalidState(_)
        ));
    }

    #[tokio::test]
    async fn positionless_post_is_rejected() {
        let (users, posts) = fixture().await;
        // Created but never accepted into the feed: no position.
        let outside = posts
            .create(
                Source::try_from(Url::parse("https://e621.net/posts/9").unwrap()).unwrap(),
                vec![],
                vec![],
                None,
                Utc::now(),
                PostStatus::Accepted,
            )
            .await
            .unwrap();
        let err = after_post(TelegramId::from(1), outside.id, &users, &posts)
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidState(_)));
    }

    #[tokio::test]
    async fn unknown_post_and_plain_user_are_rejected() {
        let (users, posts) = fixture().await;
        let anchor = feed_post(&posts, 1).await;
        assert!(matches!(
            after_post(TelegramId::from(1), PostId::from(777), &users, &posts)
                .await
                .unwrap_err(),
            HandlerError::PostNotFound(_)
        ));
        assert!(matches!(
            after_post(TelegramId::from(2), anchor.id, &users, &posts)
                .await
                .unwrap_err(),
            HandlerError::NotAuthorized(_)
        ));
    }
}

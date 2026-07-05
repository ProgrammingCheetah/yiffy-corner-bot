//! Leaderboard scoring — community members only.
//!
//! Staff (Moderator+) never rank: curators grading their own submissions
//! isn't a highscore, so both the global `/highscore` and the per-channel
//! scoreboards count plain Users exclusively.
//!
//! The per-channel board is scored on PUBLICATIONS, not submissions: what
//! actually went out in that channel. Since a channel only publishes what
//! matches its tag subscription, each board automatically reflects the
//! channel's taste.

use std::collections::HashMap;

use domain::elements::{
    post::{PostId, PostRepository},
    publisher::PublicationRepository,
    user::{Role, User, UserId, UserRepository},
};

use crate::traits::handler_response::{HandlerError, HandlerResult};

/// Resolve `(user id, score)` rows into ranked `(User, score)` rows, keeping
/// only community members (Role::User), at most `limit`.
async fn keep_community<U: UserRepository>(
    ranked: Vec<(UserId, u64)>,
    users: &U,
    limit: usize,
) -> HandlerResult<Vec<(User, u64)>> {
    let mut board = Vec::new();
    for (user_id, score) in ranked {
        if board.len() == limit {
            break;
        }
        let Some(user) = users
            .find_by_id(user_id)
            .await
            .map_err(|_| HandlerError::RepositoryError)?
        else {
            continue;
        };
        if user.role == Role::User {
            board.push((user, score));
        }
    }
    Ok(board)
}

/// The global leaderboard: community members by Posts accepted into the feed.
pub async fn global_board<P, U>(
    posts: &P,
    users: &U,
    limit: usize,
) -> HandlerResult<Vec<(User, u64)>>
where
    P: PostRepository,
    U: UserRepository,
{
    // Over-fetch so staff filtered out below can't shrink the board:
    // curation scale keeps this cheap.
    let ranked = posts
        .top_submitters(limit + 50)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    keep_community(ranked, users, limit).await
}

/// One channel's leaderboard: community members by how many of their Posts
/// were PUBLISHED to `chat_id` (distinct posts — a repeat delivery of the
/// same post is not a second point).
pub async fn channel_board<PB, P, U>(
    chat_id: i64,
    publications: &PB,
    posts: &P,
    users: &U,
    limit: usize,
) -> HandlerResult<Vec<(User, u64)>>
where
    PB: PublicationRepository,
    P: PostRepository,
    U: UserRepository,
{
    let published = publications
        .list_for_chat(chat_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    let distinct_posts: std::collections::HashSet<u64> = published
        .iter()
        .map(|publication| *publication.post_id.as_ref())
        .collect();

    let mut scores: HashMap<u64, u64> = HashMap::new();
    for post_id in distinct_posts {
        let Some(post) = posts
            .find_by_id(PostId::from(post_id))
            .await
            .map_err(|_| HandlerError::RepositoryError)?
        else {
            continue;
        };
        if let Some(submitter) = post.submitted_by {
            *scores.entry(*submitter.as_ref()).or_default() += 1;
        }
    }
    let mut ranked: Vec<(UserId, u64)> = scores
        .into_iter()
        .map(|(user, score)| (UserId::from(user), score))
        .collect();
    ranked.sort_by_key(|(user, score)| (std::cmp::Reverse(*score), *user.as_ref()));
    keep_community(ranked, users, limit).await
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;
    use domain::elements::post::{PostStatus, Source};
    use domain::elements::publisher::Publication;
    use domain::elements::user::TelegramId;
    use persistence::in_memory::{
        post::InMemoryPostRepository, publication::InMemoryPublicationRepository,
        user::InMemoryUserRepository,
    };
    use url::Url;

    struct Fixture {
        posts: InMemoryPostRepository,
        publications: InMemoryPublicationRepository,
        users: InMemoryUserRepository,
    }

    impl Fixture {
        async fn new() -> Self {
            let users = InMemoryUserRepository::new();
            users
                .create(TelegramId::from(1), Role::Owner, None, Some("Zuri".into()))
                .await
                .unwrap();
            users
                .create(TelegramId::from(2), Role::User, None, Some("Wolf".into()))
                .await
                .unwrap();
            users
                .create(TelegramId::from(3), Role::User, None, Some("Fox".into()))
                .await
                .unwrap();
            Self {
                posts: InMemoryPostRepository::new(),
                publications: InMemoryPublicationRepository::new(),
                users,
            }
        }

        async fn user_id(&self, telegram: i64) -> UserId {
            self.users
                .find_by_telegram_id(TelegramId::from(telegram))
                .await
                .unwrap()
                .unwrap()
                .id
        }

        /// An accepted post by `telegram`, published to each chat in `chats`.
        async fn published_post(&self, e621_id: u64, telegram: i64, chats: &[i64]) {
            let submitter = self.user_id(telegram).await;
            let post = self
                .posts
                .create(
                    Source::try_from(
                        Url::parse(&format!("https://e621.net/posts/{e621_id}")).unwrap(),
                    )
                    .unwrap(),
                    vec![],
                    vec![],
                    Some(submitter),
                    Utc::now(),
                    PostStatus::AwaitingModeration,
                )
                .await
                .unwrap();
            self.posts.accept_into_feed(post.id).await.unwrap();
            for chat in chats {
                self.publications
                    .record(Publication {
                        post_id: post.id,
                        chat_id: *chat,
                        message_id: 1,
                        published_at: Utc::now(),
                    })
                    .await
                    .unwrap();
            }
        }
    }

    #[tokio::test]
    async fn channel_board_counts_publications_in_that_chat_only() {
        let fx = Fixture::new().await;
        fx.published_post(1, 2, &[-100]).await; // Wolf → chat -100
        fx.published_post(2, 2, &[-100]).await; // Wolf → chat -100
        fx.published_post(3, 3, &[-100]).await; // Fox → chat -100
        fx.published_post(4, 3, &[-200]).await; // Fox → the OTHER chat

        let board = channel_board(-100, &fx.publications, &fx.posts, &fx.users, 10)
            .await
            .unwrap();
        let names: Vec<(String, u64)> = board
            .iter()
            .map(|(u, s)| (u.display_name.clone().unwrap(), *s))
            .collect();
        assert_eq!(names, vec![("Wolf".to_string(), 2), ("Fox".to_string(), 1)]);
    }

    #[tokio::test]
    async fn staff_never_rank() {
        let fx = Fixture::new().await;
        fx.published_post(1, 1, &[-100]).await; // the Owner
        fx.published_post(2, 1, &[-100]).await;
        fx.published_post(3, 2, &[-100]).await; // Wolf

        let board = channel_board(-100, &fx.publications, &fx.posts, &fx.users, 10)
            .await
            .unwrap();
        assert_eq!(board.len(), 1);
        assert_eq!(board[0].0.display_name.as_deref(), Some("Wolf"));

        // Same rule on the global board.
        let board = global_board(&fx.posts, &fx.users, 10).await.unwrap();
        assert_eq!(board.len(), 1);
        assert_eq!(board[0].0.display_name.as_deref(), Some("Wolf"));
    }

    #[tokio::test]
    async fn repeat_delivery_of_one_post_scores_once() {
        let fx = Fixture::new().await;
        fx.published_post(1, 2, &[-100, -100]).await; // double delivery

        let board = channel_board(-100, &fx.publications, &fx.posts, &fx.users, 10)
            .await
            .unwrap();
        assert_eq!(board[0].1, 1);
    }

    #[tokio::test]
    async fn empty_chat_has_an_empty_board() {
        let fx = Fixture::new().await;
        let board = channel_board(-999, &fx.publications, &fx.posts, &fx.users, 10)
            .await
            .unwrap();
        assert!(board.is_empty());
    }
}

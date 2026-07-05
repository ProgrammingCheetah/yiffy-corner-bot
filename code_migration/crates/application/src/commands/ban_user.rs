//! `/ban` `/unban` — block a User from submitting art.
//!
//! Moderator+ capability (design). The actor must strictly outrank the
//! target, so Moderators can't ban each other (or the Owner), while the Owner
//! can ban anyone below them. Banning affects *submission only* — the User
//! keeps existing, and their already-published Posts keep their attribution.

use domain::elements::user::{Role, TelegramId, UserRepository};

use crate::commands::auth::require_role;
use crate::traits::handler_response::{HandlerError, HandlerResult};

#[derive(Debug)]
pub struct BanCommand {
    pub actor: TelegramId,
    pub target: TelegramId,
    pub banned: bool,
}

pub async fn handle(cmd: BanCommand, users: &impl UserRepository) -> HandlerResult<()> {
    let actor = require_role(users, cmd.actor, Role::Moderator).await?;
    let target = users
        .find_by_telegram_id(cmd.target)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or(HandlerError::UnknownActor)?;
    if target.role >= actor.role {
        return Err(HandlerError::NotAuthorized(actor.id));
    }
    users
        .set_banned(target.id, cmd.banned)
        .await
        .map_err(|_| HandlerError::RepositoryError)
}

#[cfg(test)]
mod tests {
    use super::*;

    use persistence::in_memory::user::InMemoryUserRepository;

    async fn fixture() -> InMemoryUserRepository {
        let users = InMemoryUserRepository::new();
        users
            .create(TelegramId::from(1), Role::Owner, None, None)
            .await
            .unwrap();
        users
            .create(TelegramId::from(2), Role::Moderator, None, None)
            .await
            .unwrap();
        users
            .create(TelegramId::from(3), Role::Moderator, None, None)
            .await
            .unwrap();
        users
            .create(TelegramId::from(4), Role::User, None, None)
            .await
            .unwrap();
        users
    }

    fn cmd(actor: i64, target: i64, banned: bool) -> BanCommand {
        BanCommand {
            actor: TelegramId::from(actor),
            target: TelegramId::from(target),
            banned,
        }
    }

    #[tokio::test]
    async fn moderator_bans_and_unbans_plain_user() {
        let users = fixture().await;
        handle(cmd(2, 4, true), &users).await.unwrap();
        let target = users
            .find_by_telegram_id(TelegramId::from(4))
            .await
            .unwrap()
            .unwrap();
        assert!(target.is_banned);

        handle(cmd(2, 4, false), &users).await.unwrap();
        let target = users
            .find_by_telegram_id(TelegramId::from(4))
            .await
            .unwrap()
            .unwrap();
        assert!(!target.is_banned);
    }

    #[tokio::test]
    async fn moderator_cannot_ban_peer_moderator() {
        let users = fixture().await;
        let err = handle(cmd(2, 3, true), &users).await.unwrap_err();
        assert!(matches!(err, HandlerError::NotAuthorized(_)));
    }

    #[tokio::test]
    async fn moderator_cannot_ban_owner() {
        let users = fixture().await;
        let err = handle(cmd(2, 1, true), &users).await.unwrap_err();
        assert!(matches!(err, HandlerError::NotAuthorized(_)));
    }

    #[tokio::test]
    async fn owner_can_ban_moderator() {
        let users = fixture().await;
        handle(cmd(1, 2, true), &users).await.unwrap();
        let target = users
            .find_by_telegram_id(TelegramId::from(2))
            .await
            .unwrap()
            .unwrap();
        assert!(target.is_banned);
    }

    #[tokio::test]
    async fn plain_user_cannot_ban() {
        let users = fixture().await;
        let err = handle(cmd(4, 3, true), &users).await.unwrap_err();
        assert!(matches!(err, HandlerError::NotAuthorized(_)));
    }
}

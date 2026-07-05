//! `/setrole @username <role>` — Owner-only promotion/demotion.
//!
//! The target is addressed by Telegram `@username`, resolved to a numeric ID
//! through the [`TelegramUserResolver`] port. A resolved target who has never
//! talked to the bot is registered on the spot (so the Owner can promote
//! moderators before they ever `/start`). The Owner role itself is never
//! assignable — it is the singleton seed identity.

use domain::elements::{
    telegram::TelegramUserResolver,
    user::{Role, TelegramId, User, UserRepository},
};

use crate::commands::auth::require_role;
use crate::traits::handler_response::{HandlerError, HandlerResult};
use telemetry::Event;

#[derive(Debug)]
pub struct SetUserRole {
    pub actor: TelegramId,
    /// Target `@username`, without the leading `@`.
    pub target_username: String,
    pub new_role: Role,
}

pub async fn handle(
    cmd: SetUserRole,
    users: &impl UserRepository,
    resolver: &impl TelegramUserResolver,
) -> HandlerResult<User> {
    let actor = require_role(users, cmd.actor, Role::Owner).await?;

    if cmd.new_role == Role::Owner {
        return Err(HandlerError::InvalidState(
            "the Owner role is not assignable".to_string(),
        ));
    }

    let target_id = resolver
        .resolve_username(&cmd.target_username)
        .await
        .map_err(|e| HandlerError::ResolveFailed(e.to_string()))?
        .ok_or_else(|| HandlerError::UnknownUsername(cmd.target_username.clone()))?;

    let target = match users
        .find_by_telegram_id(target_id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
    {
        Some(user) => user,
        None => users
            .create(target_id, Role::User, Some(actor.id), None)
            .await
            .map_err(|_| HandlerError::RepositoryError)?,
    };

    if target.role == Role::Owner {
        return Err(HandlerError::InvalidState(
            "the Owner cannot be demoted".to_string(),
        ));
    }
    if target.role == cmd.new_role {
        tracing::debug!(event = %Event::RoleChanged, target_id = %target.id, role = %target.role, changed = false, "role unchanged");
        return Ok(target);
    }
    tracing::info!(
        event = %Event::RoleChanged,
        actor_id = %actor.id, target_id = %target.id,
        from = %target.role, to = %cmd.new_role,
        "role changed"
    );
    users
        .change_role(target.id, cmd.new_role)
        .await
        .map_err(|_| HandlerError::RepositoryError)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use async_trait::async_trait;
    use domain::elements::telegram::ResolveError;
    use persistence::in_memory::user::InMemoryUserRepository;

    struct StubResolver(HashMap<String, i64>);
    #[async_trait]
    impl TelegramUserResolver for StubResolver {
        async fn resolve_username(
            &self,
            username: &str,
        ) -> Result<Option<TelegramId>, ResolveError> {
            Ok(self.0.get(username).copied().map(TelegramId::from))
        }
    }

    struct Fixture {
        users: InMemoryUserRepository,
        resolver: StubResolver,
    }

    async fn fixture() -> Fixture {
        let users = InMemoryUserRepository::new();
        users
            .create(TelegramId::from(1), Role::Owner, None, None)
            .await
            .unwrap();
        users
            .create(TelegramId::from(2), Role::User, None, None)
            .await
            .unwrap();
        let resolver = StubResolver(HashMap::from([
            ("knownuser".to_string(), 2),
            ("stranger".to_string(), 99),
            ("thezuri".to_string(), 1),
        ]));
        Fixture { users, resolver }
    }

    fn cmd(actor: i64, username: &str, role: Role) -> SetUserRole {
        SetUserRole {
            actor: TelegramId::from(actor),
            target_username: username.to_string(),
            new_role: role,
        }
    }

    #[tokio::test]
    async fn owner_promotes_known_user_to_moderator() {
        let fx = fixture().await;
        let updated = handle(
            cmd(1, "knownuser", Role::Moderator),
            &fx.users,
            &fx.resolver,
        )
        .await
        .unwrap();
        assert_eq!(updated.role, Role::Moderator);
    }

    #[tokio::test]
    async fn owner_promotes_stranger_registering_them() {
        let fx = fixture().await;
        let updated = handle(cmd(1, "stranger", Role::Moderator), &fx.users, &fx.resolver)
            .await
            .unwrap();
        assert_eq!(updated.role, Role::Moderator);
        assert_eq!(updated.telegram_id, TelegramId::from(99));
        // Registered with the Owner recorded as promoter.
        assert!(updated.added_by.is_some());
    }

    #[tokio::test]
    async fn non_owner_cannot_set_roles() {
        let fx = fixture().await;
        let err = handle(
            cmd(2, "knownuser", Role::Moderator),
            &fx.users,
            &fx.resolver,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HandlerError::NotAuthorized(_)));
    }

    #[tokio::test]
    async fn owner_role_is_not_assignable() {
        let fx = fixture().await;
        let err = handle(cmd(1, "knownuser", Role::Owner), &fx.users, &fx.resolver)
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidState(_)));
    }

    #[tokio::test]
    async fn owner_cannot_be_demoted() {
        let fx = fixture().await;
        let err = handle(cmd(1, "thezuri", Role::User), &fx.users, &fx.resolver)
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::InvalidState(_)));
    }

    #[tokio::test]
    async fn unknown_username_is_reported() {
        let fx = fixture().await;
        let err = handle(cmd(1, "nobody", Role::Moderator), &fx.users, &fx.resolver)
            .await
            .unwrap_err();
        assert!(matches!(err, HandlerError::UnknownUsername(_)));
    }
}

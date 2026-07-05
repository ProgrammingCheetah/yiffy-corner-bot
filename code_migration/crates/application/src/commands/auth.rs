//! Shared permission gate for role-restricted commands.

use domain::elements::user::{Role, TelegramId, User, UserRepository};

use crate::traits::handler_response::{HandlerError, HandlerResult};

/// Look up the acting User and require at least `min` role.
///
/// Unknown actors are rejected outright — privileged commands never
/// auto-register (only `/start` and `/suggest` do).
pub async fn require_role(
    users: &impl UserRepository,
    actor: TelegramId,
    min: Role,
) -> HandlerResult<User> {
    let user = users
        .find_by_telegram_id(actor)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or(HandlerError::UnknownActor)?;
    if user.role >= min {
        Ok(user)
    } else {
        Err(HandlerError::NotAuthorized(user.id))
    }
}

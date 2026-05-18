use domain::elements::user::{Role, UserId, UserRepository};

use crate::traits::handler_response::{HandlerError, HandlerResult};

pub struct SetUserRole {
    pub id: UserId,
    pub new_role: Role,
}

pub async fn handle(cmd: SetUserRole, user_repository: &impl UserRepository) -> HandlerResult<()> {
    let user = user_repository
        .find_by_id(cmd.id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
        .ok_or(HandlerError::NotFound(cmd.id))?;

    if user.role != cmd.new_role {
        user_repository
            .change_role(cmd.id, cmd.new_role)
            .await
            .map_err(|_| HandlerError::RepositoryError)?;
    }
    Ok(())
}

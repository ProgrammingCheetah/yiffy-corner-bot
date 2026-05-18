use domain::elements::user::{Role, TelegramId, UserRepository};

use crate::traits::handler_response::{HandlerError, HandlerResult};

#[derive(Debug)]
pub struct StartCommand {
    pub id: TelegramId,
}

pub async fn handle(cmd: StartCommand, user_repository: &impl UserRepository) -> HandlerResult<()> {
    user_repository
        .create(cmd.id, Role::User, None)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;

    Ok(())
}

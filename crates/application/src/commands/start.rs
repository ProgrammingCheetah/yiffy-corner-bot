use domain::elements::user::{Role, TelegramId, UserRepository};

use crate::traits::handler_response::{HandlerError, HandlerResult};
use telemetry::Event;

#[derive(Debug)]
pub struct StartCommand {
    pub id: TelegramId,
    /// The Telegram display name at the moment of contact; cached on the User
    /// so published Posts can credit "Submitted by <name>" without a live
    /// Telegram lookup.
    pub display_name: Option<String>,
}

pub async fn handle(cmd: StartCommand, user_repository: &impl UserRepository) -> HandlerResult<()> {
    // Re-/start is idempotent: an existing User just gets their cached
    // display name refreshed.
    if let Some(existing) = user_repository
        .find_by_telegram_id(cmd.id)
        .await
        .map_err(|_| HandlerError::RepositoryError)?
    {
        if existing.display_name != cmd.display_name {
            tracing::info!(event = %Event::DisplayNameRefreshed, user_id = %existing.id, "display name refreshed on /start");
            user_repository
                .set_display_name(existing.id, cmd.display_name)
                .await
                .map_err(|_| HandlerError::RepositoryError)?;
        }
        return Ok(());
    }

    let user = user_repository
        .create(cmd.id, Role::User, None, cmd.display_name)
        .await
        .map_err(|_| HandlerError::RepositoryError)?;
    tracing::info!(event = %Event::UserRegistered, user_id = %user.id, telegram_id = cmd.id.as_ref(), "new user registered via /start");

    Ok(())
}

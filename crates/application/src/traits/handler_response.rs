use domain::elements::post::PostId;
use domain::elements::user::UserId;

pub type HandlerResult<T> = Result<T, HandlerError>;

#[derive(Debug, thiserror::Error)]
pub enum HandlerError {
    #[error("Not Authorized: {0}")]
    NotAuthorized(UserId),
    #[error("Actor is not a registered user")]
    UnknownActor,
    #[error("Repository Error")]
    RepositoryError,
    #[error("Not Found: {0}")]
    NotFound(UserId),
    #[error("Post Not Found: {0}")]
    PostNotFound(PostId),
    #[error("Not a supported source URL: {0}")]
    InvalidSource(String),
    #[error("This source was already submitted (post {0})")]
    DuplicateSubmission(PostId),
    #[error("Submitter is banned from submitting")]
    SubmitterBanned,
    #[error("Upstream fetch failed: {0}")]
    Fetch(String),
    #[error("Invalid state: {0}")]
    InvalidState(String),
    #[error("Unknown Telegram username: @{0}")]
    UnknownUsername(String),
    #[error("Telegram resolve failed: {0}")]
    ResolveFailed(String),
}

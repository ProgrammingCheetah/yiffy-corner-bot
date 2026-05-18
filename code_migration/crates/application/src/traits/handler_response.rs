use domain::elements::user::UserId;

pub type HandlerResult<T> = Result<T, HandlerError>;

#[derive(Debug, thiserror::Error)]
pub enum HandlerError {
    #[error("Not Authorized: {0}")]
    NotAuthorized(UserId),
    #[error("Repository Error")]
    RepositoryError,
    #[error("Not Found: {0}")]
    NotFound(UserId),
}

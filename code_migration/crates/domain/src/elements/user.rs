use std::fmt::Display;

/// A User's permission level within the system.
///
/// Hierarchy (higher includes lower): `Owner > Moderator > User`.
/// Per `design/domain.md`, Owner is the singleton role held by Zuri.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Owner,
    Moderator,
    User,
}

/// The internal ID for the user. Program-managed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserId(u64);

impl Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for UserId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl AsRef<u64> for UserId {
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

/// A Telegram numeric user ID. Platform-native identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TelegramId(i64);

impl From<i64> for TelegramId {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl AsRef<i64> for TelegramId {
    fn as_ref(&self) -> &i64 {
        &self.0
    }
}

/// Someone who interacts with the system.
///
/// - Has exactly one [`Role`].
/// - Identified externally by their [`TelegramId`].
/// - May know the User who promoted them (`added_by`); the seed Owner has none.
#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub telegram_id: TelegramId,
    pub role: Role,
    pub added_by: Option<UserId>,
}

#[derive(Debug, thiserror::Error)]
pub enum UserRepositoryError {
    #[error("User could not be created: {0}")]
    NotCreated(String),
    #[error("Not changed: {0}")]
    NotChanged(String),
}

/// Persistence port for [`User`]s.
#[async_trait::async_trait]
pub trait UserRepository: Send + Sync {
    async fn create(
        &self,
        telegram_id: TelegramId,
        role: Role,
        added_by: Option<UserId>,
    ) -> Result<User, UserRepositoryError>;
    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, UserRepositoryError>;
    async fn find_by_telegram_id(
        &self,
        telegram_id: TelegramId,
    ) -> Result<Option<User>, UserRepositoryError>;
    async fn change_role(&self, id: UserId, new_role: Role) -> Result<User, UserRepositoryError>;
}

use std::fmt::Display;

/// A User's permission level within the system.
///
/// Hierarchy (higher includes lower): `Owner > Moderator > User`.
/// Per `design/domain.md`, Owner is the singleton role held by Zuri.
///
/// Variant order is load-bearing: `derive(Ord)` gives `User < Moderator <
/// Owner`, so permission checks read as `actor.role >= Role::Moderator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    User,
    Moderator,
    Owner,
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
/// - `display_name` is captured from Telegram at registration time and
///   refreshed on contact — it feeds the "Submitted by <name>" attribution on
///   published Posts, so publishing never needs a live Telegram lookup.
/// - `is_banned` blocks submissions only; a banned User still exists (their
///   prior Posts keep their attribution).
#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub telegram_id: TelegramId,
    pub role: Role,
    pub added_by: Option<UserId>,
    pub display_name: Option<String>,
    pub is_banned: bool,
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
        display_name: Option<String>,
    ) -> Result<User, UserRepositoryError>;
    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, UserRepositoryError>;
    async fn find_by_telegram_id(
        &self,
        telegram_id: TelegramId,
    ) -> Result<Option<User>, UserRepositoryError>;
    async fn change_role(&self, id: UserId, new_role: Role) -> Result<User, UserRepositoryError>;
    /// Refresh the cached Telegram display name (users rename themselves;
    /// we re-capture on every contact).
    async fn set_display_name(
        &self,
        id: UserId,
        display_name: Option<String>,
    ) -> Result<(), UserRepositoryError>;
    /// Ban/unban a User from submitting. Moderator+ capability.
    async fn set_banned(&self, id: UserId, banned: bool) -> Result<(), UserRepositoryError>;
    /// All users with the given role. Used by `/suggest` to fan out the
    /// moderation DM to every Moderator + Owner.
    async fn list_by_role(&self, role: Role) -> Result<Vec<User>, UserRepositoryError>;
}

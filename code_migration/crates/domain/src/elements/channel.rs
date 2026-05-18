use crate::elements::{poster::PosterId, user::UserId};

/// The internal ID for a Channel. Program-managed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelId(u64);

impl From<u64> for ChannelId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl AsRef<u64> for ChannelId {
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

/// A destination where a Poster places media.
///
/// - Owned by exactly one [`User`](crate::elements::user::User).
/// - Loaded at cold start (Zuri-configured; not created at runtime).
/// - Topology assumes 1:1 with [`Poster`](crate::elements::poster::Poster). Not enforced at runtime.
#[derive(Debug, Clone)]
pub struct Channel {
    pub id: ChannelId,
    pub owner_id: UserId,
}

/// Persistence port for [`Channel`]s.
pub trait ChannelRepository: Send + Sync {
    type Err;
    fn create(&self, owner_id: UserId, poster_id: PosterId) -> Result<Channel, Self::Err>;
    fn find_by_id(&self, id: ChannelId) -> Result<Option<Channel>, Self::Err>;
}

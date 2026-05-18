use crate::elements::{cadence::PostInterval, channel::ChannelId, tag::Tag};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PosterId(u64);

impl AsRef<u64> for PosterId {
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

impl From<u64> for PosterId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for PosterId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A configured posting agent attached to a Channel.
///
/// A Poster is pure configuration — *what* to post, *to where*, and *how often*.
/// The act of actually firing the post on the cadence belongs to a Scheduler
/// (application/infra), not to the Poster itself.
///
/// Per `design/domain.md`:
/// - Owned by Zuri (only Zuri creates Posters for the MVP).
/// - Has exactly one Channel.
/// - Tag subscription is configured by Zuri at creation time.
#[derive(Debug, Clone)]
pub struct Poster {
    pub id: PosterId,
    /// A Poster's post always has to have these tags
    pub subscribed_tags: Vec<Tag>,
    /// A Poster's post can't have any of these tags
    pub forbidden_tags: Vec<Tag>,
    /// At what interval to post
    pub time_interval: PostInterval,
}

#[derive(Debug, thiserror::Error)]
pub enum PosterRepositoryError {
    #[error("Poster could not be created: {0}")]
    NotCreated(String),
    #[error("Poster not found: {0}")]
    NotFound(PosterId),
}

/// Persistence port for [`Poster`]s.
pub trait PosterRepository: Send + Sync {
    type Err;
    fn create(
        &self,
        for_channel: ChannelId,
        subscribed_tags: Vec<Tag>,
        forbidden_tags: Vec<Tag>,
        time_interval: PostInterval,
    ) -> Result<Poster, Self::Err>;
    fn find_by_id(&self, id: PosterId) -> Result<Option<Poster>, Self::Err>;
    fn list_all(&self) -> Result<Vec<Poster>, Self::Err>;
}

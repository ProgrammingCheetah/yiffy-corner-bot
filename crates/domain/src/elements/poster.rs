use crate::elements::{cadence::PostInterval, tag::Tag};

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

/// A configured posting agent.
///
/// A Poster is pure configuration — *what* to post and *how often*. Where it
/// posts is resolved at boot by looking up its [`PublisherConfig`] (see the
/// `publisher_config` module) and constructing a Publisher from it.
///
/// Per `design/domain.md`:
/// - Owned by Zuri (only Zuri creates Posters for the MVP).
/// - Bound to one delivery destination via PublisherConfig (1:1).
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
    /// The consumer's position in the feed: the highest `feed_position` this
    /// Poster has already consumed or scanned past. Starts at 0 (feed start).
    pub cursor: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum PosterRepositoryError {
    #[error("Poster could not be created: {0}")]
    NotCreated(String),
    #[error("Poster not found: {0}")]
    NotFound(PosterId),
}

/// Persistence port for [`Poster`]s.
#[async_trait::async_trait]
pub trait PosterRepository: Send + Sync {
    type Err;
    async fn create(
        &self,
        subscribed_tags: Vec<Tag>,
        forbidden_tags: Vec<Tag>,
        time_interval: PostInterval,
    ) -> Result<Poster, Self::Err>;
    async fn find_by_id(&self, id: PosterId) -> Result<Option<Poster>, Self::Err>;
    /// Replace the tag subscription. Cadence and channel binding stay put.
    async fn set_tags(
        &self,
        id: PosterId,
        subscribed_tags: Vec<Tag>,
        forbidden_tags: Vec<Tag>,
    ) -> Result<Poster, Self::Err>;
    /// Persist the consumer's feed cursor. Written after every successful
    /// consume (or empty scan); read fresh at every fire.
    async fn set_cursor(&self, id: PosterId, cursor: u64) -> Result<(), Self::Err>;
    /// Remove a Poster outright. Posters are pure config — deletion is hard;
    /// the database-first scheduler stops firing it on the next tick.
    async fn delete(&self, id: PosterId) -> Result<(), Self::Err>;
    async fn list_all(&self) -> Result<Vec<Poster>, Self::Err>;
}

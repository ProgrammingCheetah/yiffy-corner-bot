//! Global tag-policy ports.
//!
//! Two registries, one for each system-wide tag classification:
//!
//! - **REQUIRED**: tags added to every e621 query. Posts that don't match
//!   these never reach the system. Consumed by the (future) query-builder
//!   layer.
//! - **FORBIDDEN**: tags that disqualify a Post from being eligible.
//!   Consumed by submission validation (`/suggest` auto-bans if any
//!   forbidden tag is present) and by the Selector at re-validation time.
//!
//! Tags themselves are not persisted; only these two lists are. See
//! [[project-rust-architecture]] for the indexer-over-e621 framing.

use crate::elements::tag::Tag;

#[derive(Debug, thiserror::Error)]
pub enum ForbiddenTagRepositoryError {
    #[error("forbidden tag repository error: {0}")]
    Storage(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RequiredTagRepositoryError {
    #[error("required tag repository error: {0}")]
    Storage(String),
}

/// Globally forbidden tags. Any Post owning at least one of these is
/// ineligible (status flips to [`PostStatus::Banned`](crate::elements::post::PostStatus::Banned)).
#[async_trait::async_trait]
pub trait ForbiddenTagRepository: Send + Sync {
    type Err;
    async fn add(&self, tag: Tag) -> Result<(), Self::Err>;
    async fn remove(&self, tag: &Tag) -> Result<(), Self::Err>;
    async fn contains(&self, tag: &Tag) -> Result<bool, Self::Err>;
    async fn list_all(&self) -> Result<Vec<Tag>, Self::Err>;
}

/// Globally required tags. Added to every outgoing e621 query as a positive
/// filter so unrelated posts never reach the system in the first place.
#[async_trait::async_trait]
pub trait RequiredTagRepository: Send + Sync {
    type Err;
    async fn add(&self, tag: Tag) -> Result<(), Self::Err>;
    async fn remove(&self, tag: &Tag) -> Result<(), Self::Err>;
    async fn contains(&self, tag: &Tag) -> Result<bool, Self::Err>;
    async fn list_all(&self) -> Result<Vec<Tag>, Self::Err>;
}

#[derive(Debug, thiserror::Error)]
pub enum SpoilerTagRepositoryError {
    #[error("spoiler tag repository error: {0}")]
    Storage(String),
}

/// Content-warning tags (hard kinks etc. — watersports,
/// questionable_consent, …): a Post owning one still publishes, but its
/// media goes out behind Telegram's spoiler blur. Tags named `cw` or
/// prefixed `cw_`/`cw:` spoiler unconditionally, without being listed.
#[async_trait::async_trait]
pub trait SpoilerTagRepository: Send + Sync {
    type Err;
    async fn add(&self, tag: Tag) -> Result<(), Self::Err>;
    async fn remove(&self, tag: &Tag) -> Result<(), Self::Err>;
    async fn contains(&self, tag: &Tag) -> Result<bool, Self::Err>;
    async fn list_all(&self) -> Result<Vec<Tag>, Self::Err>;
}

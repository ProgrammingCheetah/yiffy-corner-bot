use crate::elements::post::Post;

#[derive(Debug, thiserror::Error)]
pub enum PublisherError {
    #[error("publisher send failed: {0}")]
    Send(String),
}

/// Delivers a [`Post`] to its destination (e.g. a Telegram channel).
///
/// Cadence is not a Publisher concern — the scheduler decides which Poster
/// fires on which tick and then calls `publish` on the matching Publisher.
#[async_trait::async_trait]
pub trait Publisher: Send + Sync {
    async fn publish(&self, post: &Post) -> Result<(), PublisherError>;
}

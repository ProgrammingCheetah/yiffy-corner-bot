use crate::elements::media::ResolvedMedia;

#[derive(Debug, thiserror::Error)]
pub enum PublisherError {
    #[error("publisher send failed: {0}")]
    Send(String),
}

/// A fully-prepared message for a Publisher to deliver: the resolved media
/// plus the caption the application layer built (attribution like
/// "Submitted by <name>", the source link). The Publisher is dumb transport —
/// it decides *how* to send based on the media variant, never *what*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishItem {
    pub media: ResolvedMedia,
    pub caption: Option<String>,
}

/// Delivers a [`PublishItem`] to its destination (e.g. a Telegram channel).
///
/// Cadence is not a Publisher concern — the scheduler decides which Poster
/// fires on which tick and then calls `publish` on the matching Publisher.
#[async_trait::async_trait]
pub trait Publisher: Send + Sync {
    async fn publish(&self, item: &PublishItem) -> Result<(), PublisherError>;
}

//! Outbound port for talking to e621.
//!
//! The bot is an indexer over e621, so this port is the only path through
//! which content metadata enters the system. The infra impl (`crates/infra-e621`)
//! holds the rate-limiter — 2 req/s shared across every consumer (Selector,
//! `/getpostinfo`, `/suggest`, `/browse`).

use url::Url;

use crate::elements::{post::Source, tag::Tag};

/// Metadata for a single e621 post.
///
/// Returned by [`E621Fetcher::fetch`] for a known source and by
/// [`E621Fetcher::search`] as elements of the result list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E621PostMetadata {
    pub source: Source,
    pub tags: Vec<Tag>,
    /// Actual artists only — e621's artist bucket also carries workflow
    /// markers (conditional_dnp, avoid_posting, unknown_artist, …) which the
    /// infra adapter filters out before they get here.
    pub artists: Vec<Tag>,
    /// The original-resolution media URL (what `/suggest` would re-post).
    pub file_url: Url,
    /// A Telegram-compatible MP4 rendition for webm posts (e621 provides
    /// h264 alternates). `None` for images/gifs or when e621 has none.
    pub mp4_url: Option<Url>,
    /// A smaller URL suitable for moderation/browse previews.
    pub preview_url: Url,
    /// The artist-declared off-site sources exactly as e621 reports them
    /// (free-form strings, not always URLs). Feeds the browse "Check src"
    /// button.
    pub artist_sources: Vec<String>,
}

/// Sort order for [`E621Fetcher::search`] results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum E621Order {
    /// Maps to e621's `order:random` query modifier — used by `/browse`.
    Random,
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("e621 post not found: {0:?}")]
    NotFound(Source),
    #[error("e621 post media is unavailable (deleted or login-restricted): {0:?}")]
    Unavailable(Source),
    #[error("e621 rate limit hit")]
    RateLimit,
    #[error("network error talking to e621: {0}")]
    Network(String),
    #[error("could not parse e621 response: {0}")]
    Parse(String),
}

/// Outbound port for e621.
#[async_trait::async_trait]
pub trait E621Fetcher: Send + Sync {
    /// Fetch metadata for a single known post.
    async fn fetch(&self, source: &Source) -> Result<E621PostMetadata, FetchError>;
    /// Search e621 for posts matching `tags` with the given order. `page` is
    /// 1-indexed; the caller paginates via incrementing `page`. The infra
    /// impl is responsible for injecting REQUIRED tags and excluding
    /// FORBIDDEN tags into the underlying query string.
    async fn search(
        &self,
        tags: &[Tag],
        order: E621Order,
        page: u32,
    ) -> Result<Vec<E621PostMetadata>, FetchError>;
}

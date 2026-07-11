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
    /// The e621 pools this post belongs to (ids only — resolve details via
    /// [`E621Fetcher::pools`]). A post can sit in several pools at once:
    /// its comic AND a themed collection, for example.
    pub pools: Vec<u64>,
}

/// How e621 groups the posts of a pool.
///
/// This is the comic-vs-grab-bag distinction: a `Series` is an ordered
/// sequential story (comic pages), while a `Collection` is a loose thematic
/// grouping that can hold hundreds of unrelated posts. The UI surfaces the
/// category so a curator can pick the actual comic and skip junk collections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum E621PoolCategory {
    Series,
    Collection,
}

impl std::fmt::Display for E621PoolCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            E621PoolCategory::Series => "series",
            E621PoolCategory::Collection => "collection",
        })
    }
}

impl std::str::FromStr for E621PoolCategory {
    type Err = std::convert::Infallible;
    /// Anything e621 doesn't call a `series` is treated as a collection —
    /// the only distinction the domain cares about is "ordered comic" vs
    /// "loose grouping", so an unknown future category degrades safely.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "series" => E621PoolCategory::Series,
            _ => E621PoolCategory::Collection,
        })
    }
}

/// One e621 pool: an ordered set of posts curated upstream (comic pages in
/// reading order for `Series`, arbitrary order for `Collection`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E621Pool {
    pub id: u64,
    /// Raw e621 name (words joined by underscores).
    pub name: String,
    pub category: E621PoolCategory,
    /// Every member post id, in pool order. This is the authoritative
    /// ordering — post fetches must be re-sorted against it.
    pub post_ids: Vec<u64>,
    /// Inactive pools are abandoned/locked upstream; still submittable,
    /// but the UI should say so.
    pub is_active: bool,
}

impl E621Pool {
    /// The pool's name with e621's underscores as spaces, for human display.
    pub fn display_name(&self) -> String {
        self.name.replace('_', " ")
    }

    /// The pool's page on e621, for inspection before choosing it.
    pub fn page_url(&self) -> Url {
        Url::parse(&format!("https://e621.net/pools/{}", self.id))
            .expect("static e621 pool URL shape is valid")
    }
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
    /// Search e621 for posts matching `tags`. Ordering belongs to the query
    /// itself — `order:…` modifiers ride along as ordinary tags, and without
    /// one e621's default (newest first) applies. `page` is 1-indexed; the
    /// caller paginates via incrementing `page`.
    async fn search(&self, tags: &[Tag], page: u32) -> Result<Vec<E621PostMetadata>, FetchError>;
    /// Metadata for the given pool ids. Unknown ids are simply absent from
    /// the result; the order follows e621's listing, not the input.
    async fn pools(&self, ids: &[u64]) -> Result<Vec<E621Pool>, FetchError>;
    /// Every available post of one pool, re-sorted into pool order. Posts
    /// e621 reports as deleted/login-restricted are silently absent — the
    /// caller compares against [`E621Pool::post_ids`] to count the gaps.
    async fn pool_posts(&self, pool: &E621Pool) -> Result<Vec<E621PostMetadata>, FetchError>;
}

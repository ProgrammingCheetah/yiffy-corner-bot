//! Rate-limited HTTP client for e621.
//!
//! Implements [`domain::elements::e621::E621Fetcher`] via reqwest, with a
//! shared 2 req/s rate limiter (e621's published cap). Every consumer should
//! hold the same `Arc<RateLimitedE621Client>` so the limiter is shared
//! across the whole process; otherwise per-consumer limiters multiply the
//! budget.

use std::sync::Arc;

use async_trait::async_trait;
use domain::elements::{
    e621::{E621Fetcher, E621Pool, E621PoolCategory, E621PostMetadata, FetchError},
    media::{MediaResolveError, MediaResolver, ResolvedMedia},
    post::Source,
    tag::Tag,
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use telemetry::{Event, Upstream};
use url::Url;

const E621_BASE: &str = "https://e621.net";

/// Minimal request pacer: at most one permit per `min_interval`, waiters
/// queue on the mutex in arrival order. Plain tokio time — no hardware
/// clock calibration (replaced `governor`, whose quanta/TSC clock hung
/// `until_ready()` on this host's kernel).
struct Pacer {
    min_interval: std::time::Duration,
    next_slot: tokio::sync::Mutex<Option<tokio::time::Instant>>,
}

impl Pacer {
    fn new(min_interval: std::time::Duration) -> Self {
        Self {
            min_interval,
            next_slot: tokio::sync::Mutex::new(None),
        }
    }

    async fn until_ready(&self) {
        let mut next_slot = self.next_slot.lock().await;
        let now = tokio::time::Instant::now();
        let slot = next_slot.unwrap_or(now).max(now);
        *next_slot = Some(slot + self.min_interval);
        drop(next_slot);
        tokio::time::sleep_until(slot).await;
    }
}

/// e621 API credentials (profile → API key). Sent as HTTP Basic auth;
/// unlocks content hidden from anonymous users and applies the account's
/// own blacklist instead of the anonymous defaults.
#[derive(Debug, Clone)]
pub struct E621Credentials {
    pub login: String,
    pub api_key: String,
}

pub struct RateLimitedE621Client {
    http: Client,
    limiter: Arc<Pacer>,
    user_agent: String,
    credentials: Option<E621Credentials>,
}

impl RateLimitedE621Client {
    /// Build a new client. `user_agent` should identify the bot per e621's
    /// API policy (e.g. `"yiffy-corner-bot/0.1 by ZielAnima"`).
    pub fn new(user_agent: impl Into<String>) -> Result<Self, FetchError> {
        // No default timeout in reqwest — without these a stalled
        // connection hangs a handler forever.
        let http = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| FetchError::Network(e.to_string()))?;
        // Hard cap is 2 req/s, but the API docs ask for a best effort of
        // ≤1 req/s sustained — pace accordingly.
        let limiter = Arc::new(Pacer::new(std::time::Duration::from_secs(1)));
        Ok(Self {
            http,
            limiter,
            user_agent: user_agent.into(),
            credentials: None,
        })
    }

    /// Authenticate all requests (Basic auth, per the e621 API docs).
    pub fn with_credentials(mut self, credentials: E621Credentials) -> Self {
        tracing::info!(
            event = %Event::UpstreamAuthenticated, upstream = %Upstream::E621,
            login = %credentials.login, "e621 requests will be authenticated"
        );
        self.credentials = Some(credentials);
        self
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: Url) -> Result<T, FetchError> {
        self.limiter.until_ready().await;
        tracing::debug!(event = %Event::UpstreamRequest, upstream = %Upstream::E621, url = %url, "GET");
        let mut request = self
            .http
            .get(url)
            .header(reqwest::header::USER_AGENT, &self.user_agent);
        if let Some(credentials) = &self.credentials {
            request = request.basic_auth(&credentials.login, Some(&credentials.api_key));
        }
        let resp = request
            .send()
            .await
            .map_err(|e| FetchError::Network(e.to_string()))?;

        tracing::debug!(event = %Event::UpstreamStatus, upstream = %Upstream::E621, status = resp.status().as_u16(), "response");
        match resp.status() {
            StatusCode::OK => resp
                .json::<T>()
                .await
                .map_err(|e| FetchError::Parse(e.to_string())),
            StatusCode::NOT_FOUND => Err(FetchError::Network(format!(
                "HTTP {}",
                StatusCode::NOT_FOUND
            ))),
            StatusCode::TOO_MANY_REQUESTS => Err(FetchError::RateLimit),
            other => Err(FetchError::Network(format!("HTTP {other}"))),
        }
    }
}

#[async_trait]
impl E621Fetcher for RateLimitedE621Client {
    async fn fetch(&self, source: &Source) -> Result<E621PostMetadata, FetchError> {
        let post_id = extract_post_id(source).ok_or_else(|| {
            FetchError::Parse(format!("could not extract post id from {source:?}"))
        })?;
        let url = Url::parse(&format!("{E621_BASE}/posts/{post_id}.json"))
            .map_err(|e| FetchError::Parse(e.to_string()))?;

        let wrapper: SinglePostResponse = match self.get_json(url).await {
            Ok(w) => w,
            Err(FetchError::Network(msg)) if msg.contains("404") => {
                return Err(FetchError::NotFound(source.clone()));
            }
            Err(e) => return Err(e),
        };
        metadata_from_raw(wrapper.post)
    }

    async fn search(&self, tags: &[Tag], page: u32) -> Result<Vec<E621PostMetadata>, FetchError> {
        // Ordering is the caller's business: an `order:…` modifier arrives
        // as an ordinary tag, and none means e621's newest-first default.
        let tag_query = tags
            .iter()
            .map(|t| t.as_ref().to_string())
            .collect::<Vec<_>>()
            .join("+");
        let url = Url::parse(&format!(
            "{E621_BASE}/posts.json?tags={tag_query}&page={page}&limit=20"
        ))
        .map_err(|e| FetchError::Parse(e.to_string()))?;

        let wrapper: SearchResponse = self.get_json(url).await?;
        // One deleted/restricted post must not kill the whole page.
        let mut results = Vec::new();
        for raw in wrapper.posts {
            match metadata_from_raw(raw) {
                Ok(metadata) => results.push(metadata),
                Err(FetchError::Unavailable(source)) => {
                    tracing::debug!(
                        event = %Event::UpstreamStatus, upstream = %Upstream::E621,
                        source = %source.as_ref(), "skipping unavailable post in search results"
                    );
                }
                Err(e) => return Err(e),
            }
        }
        Ok(results)
    }

    async fn pools(&self, ids: &[u64]) -> Result<Vec<E621Pool>, FetchError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let csv = ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let url = Url::parse(&format!("{E621_BASE}/pools.json?search[id]={csv}"))
            .map_err(|e| FetchError::Parse(e.to_string()))?;
        let pools: Vec<RawPool> = self.get_json(url).await?;
        Ok(pools.into_iter().map(E621Pool::from).collect())
    }

    async fn pool_posts(&self, pool: &E621Pool) -> Result<Vec<E621PostMetadata>, FetchError> {
        // `tags=pool:<id>` returns up to 320 posts per request — one call
        // for almost every pool, instead of one /posts/<id> fetch per page.
        const PAGE_LIMIT: usize = 320;
        let mut by_id: std::collections::HashMap<u64, E621PostMetadata> =
            std::collections::HashMap::with_capacity(pool.post_ids.len());
        let mut page = 1u32;
        loop {
            let url = Url::parse(&format!(
                "{E621_BASE}/posts.json?tags=pool:{}&page={page}&limit={PAGE_LIMIT}",
                pool.id
            ))
            .map_err(|e| FetchError::Parse(e.to_string()))?;
            let wrapper: SearchResponse = self.get_json(url).await?;
            let fetched = wrapper.posts.len();
            for raw in wrapper.posts {
                let id = raw.id;
                match metadata_from_raw(raw) {
                    Ok(metadata) => {
                        by_id.insert(id, metadata);
                    }
                    // Deleted/restricted pages: absent from the result, the
                    // caller counts the gap against pool.post_ids.
                    Err(FetchError::Unavailable(source)) => {
                        tracing::debug!(
                            event = %Event::UpstreamStatus, upstream = %Upstream::E621,
                            source = %source.as_ref(), pool_id = pool.id,
                            "skipping unavailable post in pool"
                        );
                    }
                    Err(e) => return Err(e),
                }
            }
            if fetched < PAGE_LIMIT || by_id.len() >= pool.post_ids.len() {
                break;
            }
            page += 1;
        }
        // The search endpoint orders by id — re-sort into pool order.
        Ok(pool
            .post_ids
            .iter()
            .filter_map(|id| by_id.remove(id))
            .collect())
    }
}

#[async_trait]
impl MediaResolver for RateLimitedE621Client {
    async fn resolve(&self, source: &Source) -> Result<ResolvedMedia, MediaResolveError> {
        if !matches!(source, Source::E621(_)) {
            return Err(MediaResolveError::Unsupported(source.clone()));
        }
        let metadata = self.fetch(source).await.map_err(|e| match e {
            FetchError::NotFound(s) | FetchError::Unavailable(s) => MediaResolveError::NotFound(s),
            FetchError::RateLimit => MediaResolveError::Network("e621 rate limit".into()),
            FetchError::Network(msg) => MediaResolveError::Network(msg),
            FetchError::Parse(msg) => MediaResolveError::Parse(msg),
        })?;
        let media = ResolvedMedia::classify(metadata.file_url);
        // Telegram can't URL-fetch webm — substitute e621's h264 rendition
        // so videos post natively via sendVideo.
        if let ResolvedMedia::Video(original) = &media
            && original.path().to_ascii_lowercase().ends_with(".webm")
            && let Some(mp4) = metadata.mp4_url
        {
            return Ok(ResolvedMedia::Video(mp4));
        }
        Ok(media)
    }
}

fn extract_post_id(source: &Source) -> Option<u64> {
    let url = source.as_ref();
    // e621 URLs look like https://e621.net/posts/123 or .../posts/123/show
    let segments: Vec<_> = url.path_segments()?.collect();
    let idx = segments.iter().position(|s| *s == "posts")?;
    segments.get(idx + 1)?.parse().ok()
}

/// Entries e621 keeps in the artist bucket that are NOT artists.
const NON_ARTIST_TAGS: &[&str] = &[
    "conditional_dnp",
    "avoid_posting",
    "unknown_artist",
    "unknown_artist_signature",
    "anonymous_artist",
    "third-party_edit",
    "sound_warning",
    "epilepsy_warning",
];

fn metadata_from_raw(raw: RawPost) -> Result<E621PostMetadata, FetchError> {
    let source_url = Url::parse(&format!("{E621_BASE}/posts/{}", raw.id))
        .map_err(|e| FetchError::Parse(e.to_string()))?;
    let source = Source::try_from(source_url)
        .map_err(|e| FetchError::Parse(format!("source rejected: {e}")))?;
    // Deleted and DNP/login-restricted posts come back with file.url: null.
    let file_url = raw
        .file
        .url
        .ok_or_else(|| FetchError::Unavailable(source.clone()))?;
    // Prefer the ~850px sample over the 150px thumbnail — this URL feeds
    // browse albums and moderation previews, where a thumbnail is useless.
    let preview_url = raw
        .sample
        .as_ref()
        .and_then(|s| s.url.clone())
        .or_else(|| raw.preview.url.clone())
        .unwrap_or_else(|| file_url.clone());

    // Telegram fetches video URLs only as MP4 and only up to ~20MB — pick
    // the best e621 h264 rendition that fits.
    const TELEGRAM_URL_FETCH_CAP: u64 = 19_000_000;
    let mp4_url = raw.sample.as_ref().and_then(|sample| {
        let alternates = sample.alternates.as_ref()?;
        let mut candidates: Vec<&RawRendition> = Vec::new();
        if let Some(variants) = &alternates.variants {
            candidates.extend(variants.get("mp4"));
        }
        if let Some(samples) = &alternates.samples {
            candidates.extend(samples.get("720p"));
            candidates.extend(samples.get("480p"));
        }
        candidates
            .into_iter()
            .filter(|r| r.size.is_none_or(|size| size <= TELEGRAM_URL_FETCH_CAP))
            .find_map(|r| r.url.clone())
    });

    let artists: Vec<Tag> = raw
        .tags
        .artist
        .iter()
        .filter(|name| !NON_ARTIST_TAGS.contains(&name.to_ascii_lowercase().as_str()))
        .map(|name| Tag::from(name.as_str()))
        .collect();

    let mut tags: Vec<Tag> = Vec::new();
    for bucket in [
        &raw.tags.general,
        &raw.tags.species,
        &raw.tags.character,
        &raw.tags.copyright,
        &raw.tags.artist,
        &raw.tags.meta,
        &raw.tags.lore,
        &raw.tags.invalid,
    ] {
        for name in bucket {
            tags.push(Tag::from(name.as_str()));
        }
    }

    Ok(E621PostMetadata {
        source,
        tags,
        artists,
        file_url,
        mp4_url,
        preview_url,
        artist_sources: raw.sources,
        pools: raw.pools,
    })
}

// ---- raw response shapes ----------------------------------------------------

#[derive(Debug, Deserialize)]
struct SinglePostResponse {
    post: RawPost,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    posts: Vec<RawPost>,
}

#[derive(Debug, Deserialize)]
struct RawPost {
    id: u64,
    file: RawFile,
    preview: RawPreview,
    sample: Option<RawSample>,
    tags: RawTags,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    pools: Vec<u64>,
}

#[derive(Debug, Deserialize)]
struct RawPool {
    id: u64,
    name: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    post_ids: Vec<u64>,
    #[serde(default)]
    is_active: bool,
}

impl From<RawPool> for E621Pool {
    fn from(raw: RawPool) -> Self {
        E621Pool {
            id: raw.id,
            name: raw.name,
            category: raw
                .category
                .parse::<E621PoolCategory>()
                .unwrap_or(E621PoolCategory::Collection),
            post_ids: raw.post_ids,
            is_active: raw.is_active,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawFile {
    url: Option<Url>,
}

#[derive(Debug, Deserialize)]
struct RawPreview {
    url: Option<Url>,
}

#[derive(Debug, Deserialize)]
struct RawSample {
    url: Option<Url>,
    #[serde(default)]
    alternates: Option<RawAlternates>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAlternates {
    #[serde(default)]
    variants: Option<std::collections::HashMap<String, RawRendition>>,
    #[serde(default)]
    samples: Option<std::collections::HashMap<String, RawRendition>>,
}

#[derive(Debug, Deserialize)]
struct RawRendition {
    url: Option<Url>,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTags {
    #[serde(default)]
    general: Vec<String>,
    #[serde(default)]
    species: Vec<String>,
    #[serde(default)]
    character: Vec<String>,
    #[serde(default)]
    copyright: Vec<String>,
    #[serde(default)]
    artist: Vec<String>,
    #[serde(default)]
    meta: Vec<String>,
    #[serde(default)]
    lore: Vec<String>,
    #[serde(default)]
    invalid: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_post_id_from_standard_url() {
        let s = Source::try_from(Url::parse("https://e621.net/posts/12345").unwrap()).unwrap();
        assert_eq!(extract_post_id(&s), Some(12345));
    }

    #[test]
    fn extract_post_id_from_show_url() {
        let s = Source::try_from(Url::parse("https://e621.net/posts/12345/show").unwrap()).unwrap();
        assert_eq!(extract_post_id(&s), Some(12345));
    }

    #[test]
    fn extract_post_id_returns_none_for_non_post_url() {
        let s = Source::try_from(Url::parse("https://e621.net/").unwrap()).unwrap();
        assert_eq!(extract_post_id(&s), None);
    }

    #[test]
    fn null_file_url_is_unavailable_not_parse_error() {
        let raw: RawPost = serde_json::from_str(
            r#"{"id":1,"file":{"url":null},"preview":{"url":null},"sample":null,"tags":{},"sources":[]}"#,
        )
        .unwrap();
        assert!(matches!(
            metadata_from_raw(raw).unwrap_err(),
            FetchError::Unavailable(_)
        ));
    }

    #[test]
    fn artist_bucket_markers_are_not_artists() {
        let raw: RawPost = serde_json::from_str(
            r#"{"id":1,"file":{"url":"https://static1.e621.net/data/a.png"},
                "preview":{"url":null},"sample":null,
                "tags":{"artist":["coolwolf","conditional_dnp","Unknown_Artist"],"general":["wolf"]},
                "sources":[]}"#,
        )
        .unwrap();
        let metadata = metadata_from_raw(raw).unwrap();
        assert_eq!(metadata.artists, vec![Tag::from("coolwolf")]);
        // The full tag list still carries everything (policy checks need it).
        assert!(metadata.tags.contains(&Tag::from("conditional_dnp")));
    }

    #[test]
    fn webm_posts_expose_the_mp4_rendition() {
        let raw: RawPost = serde_json::from_str(
            r#"{"id":1,"file":{"url":"https://static1.e621.net/data/a.webm"},
                "preview":{"url":null},
                "sample":{"url":null,"alternates":{
                    "variants":{"mp4":{"url":"https://static1.e621.net/data/sample/a_alt.mp4","size":7000000}},
                    "samples":{"720p":{"url":"https://static1.e621.net/data/sample/a_720p.mp4","size":4000000}}}},
                "tags":{},"sources":[]}"#,
        )
        .unwrap();
        let metadata = metadata_from_raw(raw).unwrap();
        assert_eq!(
            metadata.mp4_url.unwrap().as_str(),
            "https://static1.e621.net/data/sample/a_alt.mp4"
        );
    }

    #[test]
    fn oversized_mp4_variant_falls_back_to_smaller_sample() {
        let raw: RawPost = serde_json::from_str(
            r#"{"id":1,"file":{"url":"https://static1.e621.net/data/a.webm"},
                "preview":{"url":null},
                "sample":{"url":null,"alternates":{
                    "variants":{"mp4":{"url":"https://static1.e621.net/data/sample/a_alt.mp4","size":25000000}},
                    "samples":{"720p":{"url":"https://static1.e621.net/data/sample/a_720p.mp4","size":4000000}}}},
                "tags":{},"sources":[]}"#,
        )
        .unwrap();
        let metadata = metadata_from_raw(raw).unwrap();
        assert_eq!(
            metadata.mp4_url.unwrap().as_str(),
            "https://static1.e621.net/data/sample/a_720p.mp4"
        );
    }

    #[test]
    fn media_kind_follows_file_extension() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(matches!(
            ResolvedMedia::classify(u("https://static1.e621.net/data/a.png")),
            ResolvedMedia::Photo(_)
        ));
        assert!(matches!(
            ResolvedMedia::classify(u("https://static1.e621.net/data/a.webm")),
            ResolvedMedia::Video(_)
        ));
        assert!(matches!(
            ResolvedMedia::classify(u("https://static1.e621.net/data/a.gif")),
            ResolvedMedia::Animation(_)
        ));
    }

    #[test]
    fn resolve_rejects_non_e621_sources() {
        let client = RateLimitedE621Client::new("test").unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let source = Source::try_from(Url::parse("https://x.com/a/status/1").unwrap()).unwrap();
        let err = rt.block_on(client.resolve(&source)).unwrap_err();
        assert!(matches!(err, MediaResolveError::Unsupported(_)));
    }
}

#[cfg(test)]
mod live_tests {
    //! Network tests against real e621. Run with `--ignored`.
    use super::*;

    const UA: &str = "yiffy-corner-bot/0.1 (by ZielAnima; test suite)";

    #[tokio::test]
    #[ignore = "hits live e621"]
    async fn live_search_and_resolve_roundtrip() {
        let client = RateLimitedE621Client::new(UA).unwrap();
        let results = client
            .search(
                &[
                    Tag::from("canine"),
                    Tag::from("rating:s"),
                    Tag::from("order:random"),
                ],
                1,
            )
            .await
            .unwrap();
        assert!(!results.is_empty(), "search returned no posts");
        let first = &results[0];
        assert!(!first.tags.is_empty());

        // Resolve the same post through the MediaResolver port.
        let media = client.resolve(&first.source).await.unwrap();
        assert!(media.url().is_some_and(|u| !u.as_str().is_empty()));
    }

    #[tokio::test]
    #[ignore = "hits live e621"]
    async fn live_pool_roundtrip() {
        let client = RateLimitedE621Client::new(UA).unwrap();
        // Pool 44687 is a long-running SFW comic series ("Panic!!
        // Mysterious Dungeons!" by virmir); 20810 is a collection.
        let pools = client.pools(&[44687, 20810]).await.unwrap();
        assert_eq!(pools.len(), 2);
        let series = pools.iter().find(|p| p.id == 44687).unwrap();
        assert_eq!(series.category, E621PoolCategory::Series);
        assert!(!series.post_ids.is_empty());
        let collection = pools.iter().find(|p| p.id == 20810).unwrap();
        assert_eq!(collection.category, E621PoolCategory::Collection);

        // Post membership shows up on a fetched post.
        let source = Source::try_from(
            Url::parse(&format!("https://e621.net/posts/{}", series.post_ids[0])).unwrap(),
        )
        .unwrap();
        let metadata = client.fetch(&source).await.unwrap();
        assert!(metadata.pools.contains(&series.id));

        // Pool pages come back in pool order.
        let pages = client.pool_posts(series).await.unwrap();
        assert!(!pages.is_empty());
        let expected: Vec<&u64> = series
            .post_ids
            .iter()
            .filter(|id| {
                pages
                    .iter()
                    .any(|m| m.source.as_ref().path().ends_with(&format!("/{id}")))
            })
            .collect();
        let got: Vec<String> = pages
            .iter()
            .map(|m| {
                m.source
                    .as_ref()
                    .path()
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            got,
            expected.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "pages must follow pool order"
        );
    }
}

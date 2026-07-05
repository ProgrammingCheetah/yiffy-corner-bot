//! Rate-limited HTTP client for e621.
//!
//! Implements [`domain::elements::e621::E621Fetcher`] via reqwest, with a
//! shared 2 req/s rate limiter (e621's published cap). Every consumer should
//! hold the same `Arc<RateLimitedE621Client>` so the limiter is shared
//! across the whole process; otherwise per-consumer limiters multiply the
//! budget.

use std::num::NonZeroU32;
use std::sync::Arc;

use async_trait::async_trait;
use domain::elements::{
    e621::{E621Fetcher, E621Order, E621PostMetadata, FetchError},
    media::{MediaResolveError, MediaResolver, ResolvedMedia},
    post::Source,
    tag::Tag,
};
use governor::{
    Quota, RateLimiter,
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use url::Url;

const E621_BASE: &str = "https://e621.net";

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>;

pub struct RateLimitedE621Client {
    http: Client,
    limiter: Arc<Limiter>,
    user_agent: String,
}

impl RateLimitedE621Client {
    /// Build a new client. `user_agent` should identify the bot per e621's
    /// API policy (e.g. `"yiffy-corner-bot/0.1 by ZielAnima"`).
    pub fn new(user_agent: impl Into<String>) -> Result<Self, FetchError> {
        let http = Client::builder()
            .build()
            .map_err(|e| FetchError::Network(e.to_string()))?;
        let quota = Quota::per_second(NonZeroU32::new(2).expect("2 is nonzero"));
        let limiter = Arc::new(RateLimiter::direct(quota));
        Ok(Self {
            http,
            limiter,
            user_agent: user_agent.into(),
        })
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: Url) -> Result<T, FetchError> {
        self.limiter.until_ready().await;
        let resp = self
            .http
            .get(url)
            .header(reqwest::header::USER_AGENT, &self.user_agent)
            .send()
            .await
            .map_err(|e| FetchError::Network(e.to_string()))?;

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

    async fn search(
        &self,
        tags: &[Tag],
        order: E621Order,
        page: u32,
    ) -> Result<Vec<E621PostMetadata>, FetchError> {
        let mut tag_query = tags
            .iter()
            .map(|t| t.as_ref().to_string())
            .collect::<Vec<_>>()
            .join("+");
        match order {
            E621Order::Random => {
                if !tag_query.is_empty() {
                    tag_query.push('+');
                }
                tag_query.push_str("order:random");
            }
        }
        let url = Url::parse(&format!(
            "{E621_BASE}/posts.json?tags={tag_query}&page={page}&limit=20"
        ))
        .map_err(|e| FetchError::Parse(e.to_string()))?;

        let wrapper: SearchResponse = self.get_json(url).await?;
        wrapper.posts.into_iter().map(metadata_from_raw).collect()
    }
}

#[async_trait]
impl MediaResolver for RateLimitedE621Client {
    async fn resolve(&self, source: &Source) -> Result<ResolvedMedia, MediaResolveError> {
        if !matches!(source, Source::E621(_)) {
            return Err(MediaResolveError::Unsupported(source.clone()));
        }
        let metadata = self.fetch(source).await.map_err(|e| match e {
            FetchError::NotFound(s) => MediaResolveError::NotFound(s),
            FetchError::RateLimit => MediaResolveError::Network("e621 rate limit".into()),
            FetchError::Network(msg) => MediaResolveError::Network(msg),
            FetchError::Parse(msg) => MediaResolveError::Parse(msg),
        })?;
        Ok(ResolvedMedia::classify(metadata.file_url))
    }
}

fn extract_post_id(source: &Source) -> Option<u64> {
    let url = source.as_ref();
    // e621 URLs look like https://e621.net/posts/123 or .../posts/123/show
    let segments: Vec<_> = url.path_segments()?.collect();
    let idx = segments.iter().position(|s| *s == "posts")?;
    segments.get(idx + 1)?.parse().ok()
}

fn metadata_from_raw(raw: RawPost) -> Result<E621PostMetadata, FetchError> {
    let file_url = raw
        .file
        .url
        .ok_or_else(|| FetchError::Parse("post has no file.url".into()))?;
    let preview_url = raw
        .preview
        .url
        .clone()
        .or_else(|| raw.sample.as_ref().and_then(|s| s.url.clone()))
        .unwrap_or_else(|| file_url.clone());

    let source_url = Url::parse(&format!("{E621_BASE}/posts/{}", raw.id))
        .map_err(|e| FetchError::Parse(e.to_string()))?;
    let source = Source::try_from(source_url)
        .map_err(|e| FetchError::Parse(format!("source rejected: {e}")))?;

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
        file_url,
        preview_url,
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
                &[Tag::from("canine"), Tag::from("rating:s")],
                E621Order::Random,
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
}

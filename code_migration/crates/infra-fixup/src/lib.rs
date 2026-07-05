//! [`MediaResolver`] adapter for the FixUp embed family.
//!
//! - **Twitter/X** → the FixupX JSON API (`api.fxtwitter.com`): direct media
//!   URLs with photo/video/gif discrimination, multi-photo posts collapsed to
//!   the pre-rendered mosaic.
//! - **BlueSky** → the fxbsky embed page (`fxbsky.app`): fxbsky exposes no
//!   JSON API, so we read the `og:video` / `og:image` meta tags exactly like
//!   the chat platforms it was built for (sent with an embed-crawler
//!   User-Agent).
//! - **DeviantArt** → no media fetch; the URL is rewritten to
//!   `fixdeviantart.com` and published as a link so Telegram renders the
//!   fixed embed.
//! - **Telegram** (`t.me`) → published as a link; Telegram previews its own
//!   URLs natively.
//!
//! Twitter/BlueSky posts with no direct media degrade to a `Link` with the
//! FixUp host swapped in (`fixupx.com` / `fxbsky.app`), which still gives
//! Telegram a rich embed.

use async_trait::async_trait;
use domain::elements::{
    media::{MediaResolveError, MediaResolver, ResolvedMedia},
    post::Source,
};
use reqwest::Client;
use serde::Deserialize;
use url::Url;

const FXTWITTER_API: &str = "https://api.fxtwitter.com";
const FXBSKY_HOST: &str = "fxbsky.app";
const FIXUPX_HOST: &str = "fixupx.com";
const FIXDEVIANTART_HOST: &str = "fixdeviantart.com";
/// fxbsky (like all FixTweet-family services) only serves embed meta tags to
/// crawlers it recognizes; Telegram's own crawler string is the natural fit.
const EMBED_CRAWLER_UA: &str = "TelegramBot (like TwitterBot)";

pub struct FixupResolver {
    http: Client,
    user_agent: String,
}

impl FixupResolver {
    pub fn new(user_agent: impl Into<String>) -> Result<Self, MediaResolveError> {
        let http = Client::builder()
            .build()
            .map_err(|e| MediaResolveError::Network(e.to_string()))?;
        Ok(Self {
            http,
            user_agent: user_agent.into(),
        })
    }

    async fn resolve_twitter(
        &self,
        source: &Source,
        url: &Url,
    ) -> Result<ResolvedMedia, MediaResolveError> {
        let (user, id) = twitter_status_parts(url)
            .ok_or_else(|| MediaResolveError::Parse(format!("not a status URL: {url}")))?;
        let api_url = format!("{FXTWITTER_API}/{user}/status/{id}");
        let response = self
            .http
            .get(&api_url)
            .header(reqwest::header::USER_AGENT, &self.user_agent)
            .send()
            .await
            .map_err(|e| MediaResolveError::Network(e.to_string()))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(MediaResolveError::NotFound(source.clone()));
        }
        let body: FxTwitterResponse = response
            .json()
            .await
            .map_err(|e| MediaResolveError::Parse(e.to_string()))?;
        match body.code {
            200 => {}
            404 => return Err(MediaResolveError::NotFound(source.clone())),
            other => {
                return Err(MediaResolveError::Network(format!(
                    "fxtwitter returned {other}: {}",
                    body.message
                )));
            }
        }
        let tweet = body
            .tweet
            .ok_or_else(|| MediaResolveError::Parse("fxtwitter 200 without tweet".into()))?;
        Ok(pick_twitter_media(tweet.media.as_ref())
            .unwrap_or_else(|| ResolvedMedia::Link(with_host(url, FIXUPX_HOST))))
    }

    async fn resolve_bsky(&self, url: &Url) -> Result<ResolvedMedia, MediaResolveError> {
        let embed_url = with_host(url, FXBSKY_HOST);
        let html = self
            .http
            .get(embed_url.clone())
            .header(reqwest::header::USER_AGENT, EMBED_CRAWLER_UA)
            .send()
            .await
            .map_err(|e| MediaResolveError::Network(e.to_string()))?
            .text()
            .await
            .map_err(|e| MediaResolveError::Network(e.to_string()))?;
        Ok(pick_og_media(&html).unwrap_or(ResolvedMedia::Link(embed_url)))
    }
}

#[async_trait]
impl MediaResolver for FixupResolver {
    async fn resolve(&self, source: &Source) -> Result<ResolvedMedia, MediaResolveError> {
        match source {
            Source::Twitter(url) => self.resolve_twitter(source, url).await,
            Source::BlueSky(url) => self.resolve_bsky(url).await,
            Source::DeviantArt(url) => Ok(ResolvedMedia::Link(with_host(url, FIXDEVIANTART_HOST))),
            Source::Telegram(url) => Ok(ResolvedMedia::Link(url.clone())),
            other => Err(MediaResolveError::Unsupported(other.clone())),
        }
    }
}

/// Swap a URL's host, keeping path/query. Infallible for the hosts we use.
fn with_host(url: &Url, host: &str) -> Url {
    let mut swapped = url.clone();
    swapped
        .set_host(Some(host))
        .expect("static replacement hosts are valid");
    swapped
}

/// Extract `(user, status_id)` from a twitter.com/x.com status URL.
fn twitter_status_parts(url: &Url) -> Option<(String, String)> {
    let mut segments = url.path_segments()?;
    let user = segments.next()?.to_string();
    if segments.next()? != "status" {
        return None;
    }
    // Trailing /photo/1 etc. is fine — we only need the numeric ID.
    let id: String = segments
        .next()?
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if user.is_empty() || id.is_empty() {
        return None;
    }
    Some((user, id))
}

// --- FixupX (api.fxtwitter.com) response subset ---

#[derive(Debug, Deserialize)]
struct FxTwitterResponse {
    code: u16,
    #[serde(default)]
    message: String,
    tweet: Option<FxTweet>,
}

#[derive(Debug, Deserialize)]
struct FxTweet {
    media: Option<FxMedia>,
}

#[derive(Debug, Deserialize)]
struct FxMedia {
    #[serde(default)]
    photos: Vec<FxPhoto>,
    #[serde(default)]
    videos: Vec<FxVideo>,
    mosaic: Option<FxMosaic>,
}

#[derive(Debug, Deserialize)]
struct FxPhoto {
    url: Url,
}

#[derive(Debug, Deserialize)]
struct FxVideo {
    url: Url,
    /// `"video"` or `"gif"`.
    #[serde(rename = "type", default)]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct FxMosaic {
    formats: FxMosaicFormats,
}

#[derive(Debug, Deserialize)]
struct FxMosaicFormats {
    jpeg: Option<Url>,
}

/// Priority: video (gif → Animation), then multi-photo mosaic, then photo.
fn pick_twitter_media(media: Option<&FxMedia>) -> Option<ResolvedMedia> {
    let media = media?;
    if let Some(video) = media.videos.first() {
        return Some(if video.kind == "gif" {
            ResolvedMedia::Animation(video.url.clone())
        } else {
            ResolvedMedia::Video(video.url.clone())
        });
    }
    if media.photos.len() > 1
        && let Some(jpeg) = media.mosaic.as_ref().and_then(|m| m.formats.jpeg.clone())
    {
        return Some(ResolvedMedia::Photo(jpeg));
    }
    media
        .photos
        .first()
        .map(|p| ResolvedMedia::Photo(p.url.clone()))
}

/// Pull `og:video` / `og:image` out of an embed page. Video wins.
fn pick_og_media(html: &str) -> Option<ResolvedMedia> {
    let video = og_content(html, "og:video").or_else(|| og_content(html, "og:video:url"));
    if let Some(url) = video {
        return Some(ResolvedMedia::Video(url));
    }
    og_content(html, "og:image").map(ResolvedMedia::Photo)
}

fn og_content(html: &str, property: &str) -> Option<Url> {
    // Attribute order varies across FixUp services; match both orders.
    let patterns = [
        format!(r#"<meta[^>]*property="{property}"[^>]*content="([^"]+)""#),
        format!(r#"<meta[^>]*content="([^"]+)"[^>]*property="{property}""#),
    ];
    for pattern in patterns {
        let re = regex::Regex::new(&pattern).expect("static pattern");
        if let Some(captures) = re.captures(html)
            && let Ok(url) = Url::parse(&captures[1])
        {
            return Some(url);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn twitter_status_parts_from_x_and_twitter_urls() {
        for host in ["twitter.com", "x.com"] {
            let parsed = twitter_status_parts(&url(&format!(
                "https://{host}/SomeArtist/status/1790000000000000000"
            )))
            .unwrap();
            assert_eq!(parsed.0, "SomeArtist");
            assert_eq!(parsed.1, "1790000000000000000");
        }
    }

    #[test]
    fn twitter_status_parts_tolerates_photo_suffix_and_query() {
        let parsed = twitter_status_parts(&url("https://x.com/a/status/123/photo/1?s=20")).unwrap();
        assert_eq!(parsed.1, "123");
        assert!(twitter_status_parts(&url("https://x.com/a/likes")).is_none());
    }

    #[test]
    fn with_host_swaps_host_only() {
        let swapped = with_host(&url("https://x.com/a/status/1?s=20"), FIXUPX_HOST);
        assert_eq!(swapped.as_str(), "https://fixupx.com/a/status/1?s=20");
    }

    #[test]
    fn fxtwitter_photo_response_parses() {
        let body: FxTwitterResponse = serde_json::from_str(
            r#"{"code":200,"message":"OK","tweet":{"media":{"photos":[{"url":"https://pbs.twimg.com/media/abc.jpg","width":1,"height":1}]}}}"#,
        )
        .unwrap();
        let media = pick_twitter_media(body.tweet.unwrap().media.as_ref()).unwrap();
        assert!(matches!(media, ResolvedMedia::Photo(_)));
    }

    #[test]
    fn fxtwitter_gif_beats_photos() {
        let body: FxTwitterResponse = serde_json::from_str(
            r#"{"code":200,"message":"OK","tweet":{"media":{
                "photos":[{"url":"https://pbs.twimg.com/media/a.jpg"}],
                "videos":[{"url":"https://video.twimg.com/tweet_video/a.mp4","type":"gif"}]}}}"#,
        )
        .unwrap();
        let media = pick_twitter_media(body.tweet.unwrap().media.as_ref()).unwrap();
        assert!(matches!(media, ResolvedMedia::Animation(_)));
    }

    #[test]
    fn fxtwitter_multi_photo_uses_mosaic() {
        let body: FxTwitterResponse = serde_json::from_str(
            r#"{"code":200,"message":"OK","tweet":{"media":{
                "photos":[{"url":"https://pbs.twimg.com/media/a.jpg"},{"url":"https://pbs.twimg.com/media/b.jpg"}],
                "mosaic":{"formats":{"jpeg":"https://mosaic.fxtwitter.com/jpeg/1/a/b","webp":"https://mosaic.fxtwitter.com/webp/1/a/b"}}}}}"#,
        )
        .unwrap();
        let media = pick_twitter_media(body.tweet.unwrap().media.as_ref()).unwrap();
        let ResolvedMedia::Photo(url) = media else {
            panic!("expected photo");
        };
        assert!(url.as_str().contains("mosaic"));
    }

    #[test]
    fn fxtwitter_textonly_yields_none() {
        let body: FxTwitterResponse =
            serde_json::from_str(r#"{"code":200,"message":"OK","tweet":{}}"#).unwrap();
        assert!(pick_twitter_media(body.tweet.unwrap().media.as_ref()).is_none());
    }

    #[test]
    fn og_image_parses_from_fxbsky_html() {
        let html = r#"<meta property="og:image" content="https://cdn.bsky.app/img/feed_fullsize/plain/did:plc:x/bafy"/>
<meta property="twitter:card" content="summary_large_image"/>"#;
        let media = pick_og_media(html).unwrap();
        assert!(matches!(media, ResolvedMedia::Photo(_)));
    }

    #[test]
    fn og_video_beats_og_image() {
        let html = r#"<meta property="og:image" content="https://cdn.bsky.app/img/x"/>
<meta property="og:video" content="https://video.bsky.app/x/playlist.m3u8"/>"#;
        let media = pick_og_media(html).unwrap();
        assert!(matches!(media, ResolvedMedia::Video(_)));
    }

    #[test]
    fn og_reversed_attribute_order_parses() {
        let html = r#"<meta content="https://cdn.bsky.app/img/x" property="og:image"/>"#;
        assert!(pick_og_media(html).is_some());
    }

    #[test]
    fn no_media_yields_none() {
        assert!(pick_og_media("<html><head></head></html>").is_none());
    }

    #[test]
    fn telegram_and_deviantart_resolve_to_links() {
        let resolver = FixupResolver::new("test").unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();

        let tg = Source::try_from(url("https://t.me/somechannel/42")).unwrap();
        let media = rt.block_on(resolver.resolve(&tg)).unwrap();
        assert_eq!(media, ResolvedMedia::Link(url("https://t.me/somechannel/42")));

        let da = Source::try_from(url("https://www.deviantart.com/x/art/y-1")).unwrap();
        let media = rt.block_on(resolver.resolve(&da)).unwrap();
        assert_eq!(
            media,
            ResolvedMedia::Link(url("https://fixdeviantart.com/x/art/y-1"))
        );
    }

    #[test]
    fn e621_is_unsupported_here() {
        let resolver = FixupResolver::new("test").unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let source = Source::try_from(url("https://e621.net/posts/1")).unwrap();
        let err = rt.block_on(resolver.resolve(&source)).unwrap_err();
        assert!(matches!(err, MediaResolveError::Unsupported(_)));
    }
}

#[cfg(test)]
mod live_tests {
    //! Network tests against the real FixUp services. Run with `--ignored`.
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[tokio::test]
    #[ignore = "hits live FixUp services"]
    async fn live_twitter_textonly_falls_back_to_fixupx_link() {
        let resolver = FixupResolver::new("yiffy-corner-bot/0.1 test").unwrap();
        // jack's first tweet: text-only → Link fallback with fixupx host.
        let source = Source::try_from(url("https://twitter.com/jack/status/20")).unwrap();
        let media = resolver.resolve(&source).await.unwrap();
        assert_eq!(
            media,
            ResolvedMedia::Link(url("https://fixupx.com/jack/status/20"))
        );
    }

    #[tokio::test]
    #[ignore = "hits live FixUp services"]
    async fn live_bsky_photo_resolves() {
        let resolver = FixupResolver::new("yiffy-corner-bot/0.1 test").unwrap();
        let source = Source::try_from(url(
            "https://bsky.app/profile/bsky.app/post/3mpok7nkjtc2o",
        ))
        .unwrap();
        let media = resolver.resolve(&source).await.unwrap();
        assert!(
            matches!(media, ResolvedMedia::Photo(_)),
            "expected photo, got {media:?}"
        );
    }
}

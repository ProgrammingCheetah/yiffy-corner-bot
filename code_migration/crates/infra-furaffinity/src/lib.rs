//! [`MediaResolver`] adapter for FurAffinity.
//!
//! FA has no public API; this adapter fetches the submission page
//! (`/view/<id>/`) and extracts the full-resolution media URL — preferring
//! the Download link (`d.furaffinity.net/art/...`), falling back to the
//! `og:image` meta tag.
//!
//! Anonymous fetches work for General-rated content. Mature/Adult submissions
//! sit behind the login wall, so the resolver accepts the FA session cookies
//! (`a` and `b`, the same pair the legacy bot was configured for) and sends
//! them when present. A page that resolves to no media while logged out is
//! reported as [`MediaResolveError::Auth`] so the failure mode is explicit.
//!
//! Politeness: requests share a 1 req/s limiter — FA is scraped, not queried.

use std::num::NonZeroU32;
use std::sync::Arc;

use async_trait::async_trait;
use domain::elements::{
    media::{MediaResolveError, MediaResolver, ResolvedMedia},
    post::Source,
};
use governor::{
    Quota, RateLimiter,
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use reqwest::Client;
use telemetry::{Event, Upstream};
use url::Url;

type Limiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>;

/// FA session cookie pair. Values of the `a` and `b` cookies of a logged-in
/// session (the legacy bot's `cookie_a.txt` / `cookie_b.txt`).
#[derive(Debug, Clone)]
pub struct FaCookies {
    pub a: String,
    pub b: String,
}

pub struct FuraffinityResolver {
    http: Client,
    limiter: Arc<Limiter>,
    user_agent: String,
    cookies: Option<FaCookies>,
}

impl FuraffinityResolver {
    /// `cookies: None` limits resolution to General-rated (public) content.
    pub fn new(
        user_agent: impl Into<String>,
        cookies: Option<FaCookies>,
    ) -> Result<Self, MediaResolveError> {
        let http = Client::builder()
            .build()
            .map_err(|e| MediaResolveError::Network(e.to_string()))?;
        let quota = Quota::per_second(NonZeroU32::new(1).expect("1 is nonzero"));
        Ok(Self {
            http,
            limiter: Arc::new(RateLimiter::direct(quota)),
            user_agent: user_agent.into(),
            cookies,
        })
    }
}

#[async_trait]
impl MediaResolver for FuraffinityResolver {
    async fn resolve(&self, source: &Source) -> Result<ResolvedMedia, MediaResolveError> {
        let Source::FurAffinity(url) = source else {
            return Err(MediaResolveError::Unsupported(source.clone()));
        };
        self.limiter.until_ready().await;
        tracing::debug!(
            event = %Event::UpstreamRequest, upstream = %Upstream::FurAffinity,
            url = %url, authenticated = self.cookies.is_some(), "GET"
        );

        let mut request = self
            .http
            .get(url.clone())
            .header(reqwest::header::USER_AGENT, &self.user_agent);
        if let Some(cookies) = &self.cookies {
            request = request.header(
                reqwest::header::COOKIE,
                format!("a={}; b={}", cookies.a, cookies.b),
            );
        }
        let response = request
            .send()
            .await
            .map_err(|e| MediaResolveError::Network(e.to_string()))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(MediaResolveError::NotFound(source.clone()));
        }
        let html = response
            .text()
            .await
            .map_err(|e| MediaResolveError::Network(e.to_string()))?;

        match extract_media_url(&html) {
            Some(media_url) => Ok(ResolvedMedia::classify(media_url)),
            None if page_is_login_walled(&html) && self.cookies.is_none() => {
                tracing::warn!(
                    event = %Event::FaLoginWall, url = %url, authenticated = false,
                    "rating-gated FA submission and no cookies configured"
                );
                Err(MediaResolveError::Auth(
                    "submission requires an FA login (Mature/Adult rating) and no cookies are configured".into(),
                ))
            }
            None if page_is_login_walled(&html) => {
                tracing::warn!(
                    event = %Event::FaLoginWall, url = %url, authenticated = true,
                    "rating-gated FA submission; configured cookies were rejected"
                );
                Err(MediaResolveError::Auth(
                    "submission requires an FA login and the configured cookies were rejected"
                        .into(),
                ))
            }
            None => Err(MediaResolveError::Parse(
                "no download link or og:image found on FA page".into(),
            )),
        }
    }
}

/// Prefer the Download anchor (full resolution), then `og:image`.
fn extract_media_url(html: &str) -> Option<Url> {
    let download =
        regex::Regex::new(r#"href="(//d\.furaffinity\.net/art/[^"]+)""#).expect("static pattern");
    if let Some(captures) = download.captures(html) {
        return Url::parse(&format!("https:{}", &captures[1])).ok();
    }
    let og_image = regex::Regex::new(r#"<meta[^>]*property="og:image"[^>]*content="([^"]+)""#)
        .expect("static pattern");
    if let Some(captures) = og_image.captures(html) {
        // FA's generic site banner is not submission media.
        let url = &captures[1];
        if url.contains("d.furaffinity.net/art/") {
            return Url::parse(url).ok();
        }
    }
    None
}

/// FA serves a notice page instead of the submission when it's rating-gated.
fn page_is_login_walled(html: &str) -> bool {
    html.contains("registered users only")
        || html.contains("You are not allowed to view this image")
        || html.contains("log in")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_link_wins_over_og_image() {
        let html = r#"
            <meta property="og:image" content="https://d.furaffinity.net/art/artist/123/123.thumb.jpg"/>
            <div class="download"><a href="//d.furaffinity.net/art/artist/123/123.artist_full.png">Download</a></div>
        "#;
        let url = extract_media_url(html).unwrap();
        assert_eq!(
            url.as_str(),
            "https://d.furaffinity.net/art/artist/123/123.artist_full.png"
        );
    }

    #[test]
    fn og_image_fallback_requires_fa_art_host() {
        let html =
            r#"<meta property="og:image" content="https://d.furaffinity.net/art/a/1/1.png"/>"#;
        assert!(extract_media_url(html).is_some());

        let banner = r#"<meta property="og:image" content="https://www.furaffinity.net/themes/beta/img/banners/fender.jpg"/>"#;
        assert!(extract_media_url(banner).is_none());
    }

    #[test]
    fn gif_and_webm_classify_correctly() {
        let html = r#"<a href="//d.furaffinity.net/art/a/1/1.a_anim.gif">Download</a>"#;
        let media = ResolvedMedia::classify(extract_media_url(html).unwrap());
        assert!(matches!(media, ResolvedMedia::Animation(_)));
    }

    #[test]
    fn login_wall_is_detected() {
        assert!(page_is_login_walled(
            "This submission contains Mature or Adult content, viewable by registered users only."
        ));
        assert!(!page_is_login_walled("<html>art page</html>"));
    }

    #[test]
    fn non_fa_source_is_unsupported() {
        let resolver = FuraffinityResolver::new("test", None).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let source = Source::try_from(Url::parse("https://e621.net/posts/1").unwrap()).unwrap();
        let err = rt.block_on(resolver.resolve(&source)).unwrap_err();
        assert!(matches!(err, MediaResolveError::Unsupported(_)));
    }
}

#[cfg(test)]
mod live_tests {
    //! Network test against real FA. Run with `--ignored`.
    use super::*;

    #[tokio::test]
    #[ignore = "hits live FurAffinity"]
    async fn live_public_submission_resolves_anonymously() {
        let resolver =
            FuraffinityResolver::new("yiffy-corner-bot/0.1 (by ZielAnima; test)", None).unwrap();

        // Pull a current /view/ id off the public browse page so the test
        // doesn't depend on one submission staying up forever.
        let browse = resolver
            .http
            .get("https://www.furaffinity.net/browse/")
            .header(reqwest::header::USER_AGENT, &resolver.user_agent)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let view = regex::Regex::new(r#"/view/(\d+)/"#).unwrap();
        let id = &view.captures(&browse).expect("no /view/ links on browse")[1];

        let source = Source::try_from(
            Url::parse(&format!("https://www.furaffinity.net/view/{id}/")).unwrap(),
        )
        .unwrap();
        let media = resolver.resolve(&source).await.unwrap();
        assert!(
            media
                .url()
                .is_some_and(|u| u.as_str().contains("d.furaffinity.net")),
            "unexpected media: {media:?}"
        );
    }
}

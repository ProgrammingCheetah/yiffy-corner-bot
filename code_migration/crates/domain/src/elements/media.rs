//! Outbound port for turning a [`Source`] into publishable media.
//!
//! Every source type is usable (design decision 2026-07-04): e621 resolves
//! natively through its API, FurAffinity through an authenticated page fetch,
//! Twitter and BlueSky through the FixUp embed API family (FixupX, fxbsky).
//! Sources with no direct-media path resolve to [`ResolvedMedia::Link`] so
//! the Publisher can fall back to the platform's own link preview.
//!
//! Tag semantics stay e621-exclusive — this port only produces *media*,
//! never tags.

use url::Url;

use crate::elements::post::Source;

/// Media resolved from a [`Source`], ready for a Publisher to deliver.
///
/// The variant tells the Publisher *how* to send (photo vs. video vs.
/// animation vs. plain link); the inner [`Url`] is always the direct,
/// full-resolution media URL except for `Link`, where it is the page to embed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedMedia {
    Photo(Url),
    Video(Url),
    /// GIF-like media. Telegram delivers these as "animations".
    Animation(Url),
    /// No direct media could (or should) be resolved; publish the URL itself
    /// and let the destination platform render its embed.
    Link(Url),
}

impl ResolvedMedia {
    /// Classify a *direct* media URL by its file extension. For URLs that are
    /// pages rather than files, use [`ResolvedMedia::Link`] directly instead.
    pub fn classify(file_url: Url) -> Self {
        let path = file_url.path().to_ascii_lowercase();
        if path.ends_with(".webm") || path.ends_with(".mp4") {
            ResolvedMedia::Video(file_url)
        } else if path.ends_with(".gif") {
            ResolvedMedia::Animation(file_url)
        } else {
            ResolvedMedia::Photo(file_url)
        }
    }
}

impl AsRef<Url> for ResolvedMedia {
    fn as_ref(&self) -> &Url {
        match self {
            ResolvedMedia::Photo(u)
            | ResolvedMedia::Video(u)
            | ResolvedMedia::Animation(u)
            | ResolvedMedia::Link(u) => u,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MediaResolveError {
    #[error("no resolver supports this source: {0:?}")]
    Unsupported(Source),
    #[error("source content not found: {0:?}")]
    NotFound(Source),
    #[error("authentication failed against the source platform: {0}")]
    Auth(String),
    #[error("network error resolving media: {0}")]
    Network(String),
    #[error("could not parse platform response: {0}")]
    Parse(String),
}

/// Outbound port: resolve a [`Source`] into publishable media.
///
/// Implementations are per-platform (`infra-e621`, `infra-fixup`,
/// `infra-furaffinity`); a composite resolver dispatches on the `Source`
/// variant so callers hold a single `dyn MediaResolver`.
#[async_trait::async_trait]
pub trait MediaResolver: Send + Sync {
    async fn resolve(&self, source: &Source) -> Result<ResolvedMedia, MediaResolveError>;
}

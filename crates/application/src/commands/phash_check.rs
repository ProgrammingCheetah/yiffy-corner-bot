//! The perceptual-hash duplicate check, run after a submission is queued.
//!
//! Source URLs already dedupe exact re-submissions at the door; this catches
//! the same *picture* under a different URL. Best-effort by design: hashing
//! failures log and return `None` — the check flags, it never blocks or
//! rejects a submission (crops and variants are a human call).

use domain::elements::{
    media::ResolvedMedia,
    phash::{NEAR_DUPLICATE_DISTANCE, PerceptualHasher, hamming},
    post::{PostId, PostRepository},
};
use telemetry::Event;

/// An existing post whose media reads as "the same picture".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimilarPost {
    pub post_id: PostId,
    /// Hamming distance between the dHashes (0 = pixel-identical structure).
    pub distance: u32,
}

/// Hash `media` for `post_id`, store the hash, and return the closest OTHER
/// post within [`NEAR_DUPLICATE_DISTANCE`], if any. Non-image media is
/// skipped (videos and links don't dHash).
pub async fn hash_and_check<P, H>(
    post_id: PostId,
    media: &ResolvedMedia,
    hasher: &H,
    posts: &P,
) -> Option<SimilarPost>
where
    P: PostRepository,
    H: PerceptualHasher + ?Sized,
{
    let ResolvedMedia::Photo(url) = media else {
        return None;
    };
    let hash = match hasher.hash_image(url).await {
        Ok(hash) => hash,
        Err(e) => {
            tracing::warn!(
                event = %Event::PhashFailed, post_id = %post_id, error = %e,
                "perceptual hash failed; submission continues unflagged"
            );
            return None;
        }
    };
    if let Err(_e) = posts.set_phash(post_id, Some(hash)).await {
        tracing::warn!(
            event = %Event::PhashFailed, post_id = %post_id,
            "computed hash could not be stored"
        );
    }
    tracing::debug!(
        event = %Event::PhashComputed, post_id = %post_id,
        phash = format!("{hash:016x}"), "perceptual hash stored"
    );

    let closest = posts
        .list_phashes()
        .await
        .ok()?
        .into_iter()
        .filter(|(id, _)| *id != post_id)
        .map(|(id, other)| SimilarPost {
            post_id: id,
            distance: hamming(hash, other),
        })
        .min_by_key(|s| s.distance)?;
    if closest.distance <= NEAR_DUPLICATE_DISTANCE {
        tracing::info!(
            event = %Event::DuplicateSuspected, post_id = %post_id,
            similar_to = %closest.post_id, distance = closest.distance,
            "submission media reads as a near-duplicate"
        );
        Some(closest)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use chrono::Utc;
    use domain::elements::phash::PHashError;
    use domain::elements::post::{PostStatus, Source};
    use persistence::in_memory::post::InMemoryPostRepository;
    use url::Url;

    /// Hashes are keyed by URL path so tests control every value.
    struct StubHasher(std::collections::HashMap<String, u64>);
    #[async_trait]
    impl PerceptualHasher for StubHasher {
        async fn hash_image(&self, url: &Url) -> Result<u64, PHashError> {
            self.0
                .get(url.path())
                .copied()
                .ok_or_else(|| PHashError::Fetch("unknown".into()))
        }
    }

    async fn post_with(repo: &InMemoryPostRepository, id: u64) -> PostId {
        repo.create(
            Source::try_from(Url::parse(&format!("https://e621.net/posts/{id}")).unwrap()).unwrap(),
            vec![],
            vec![],
            None,
            Utc::now(),
            PostStatus::AwaitingModeration,
        )
        .await
        .unwrap()
        .id
    }

    fn photo(path: &str) -> ResolvedMedia {
        ResolvedMedia::Photo(Url::parse(&format!("https://cdn.example{path}")).unwrap())
    }

    #[tokio::test]
    async fn near_duplicate_is_flagged_and_hash_stored() {
        let repo = InMemoryPostRepository::new();
        let original = post_with(&repo, 1).await;
        repo.set_phash(original, Some(0xFF00FF00FF00FF00))
            .await
            .unwrap();
        let incoming = post_with(&repo, 2).await;

        // 2 bits away from the original.
        let hasher = StubHasher(
            [("/b.png".to_string(), 0xFF00FF00FF00FF03_u64)]
                .into_iter()
                .collect(),
        );
        let hit = hash_and_check(incoming, &photo("/b.png"), &hasher, &repo)
            .await
            .unwrap();
        assert_eq!(hit.post_id, original);
        assert_eq!(hit.distance, 2);
        let stored = repo.find_by_id(incoming).await.unwrap().unwrap();
        assert_eq!(stored.phash, Some(0xFF00FF00FF00FF03));
    }

    #[tokio::test]
    async fn distant_media_is_not_flagged() {
        let repo = InMemoryPostRepository::new();
        let original = post_with(&repo, 1).await;
        repo.set_phash(original, Some(0)).await.unwrap();
        let incoming = post_with(&repo, 2).await;

        let hasher = StubHasher(
            [("/b.png".to_string(), u64::MAX)] // 64 bits apart
                .into_iter()
                .collect(),
        );
        assert!(
            hash_and_check(incoming, &photo("/b.png"), &hasher, &repo)
                .await
                .is_none()
        );
        // The hash still landed for future comparisons.
        let stored = repo.find_by_id(incoming).await.unwrap().unwrap();
        assert_eq!(stored.phash, Some(u64::MAX));
    }

    #[tokio::test]
    async fn non_photo_media_is_skipped() {
        let repo = InMemoryPostRepository::new();
        let incoming = post_with(&repo, 1).await;
        let hasher = StubHasher(Default::default());
        let video = ResolvedMedia::Video(Url::parse("https://cdn.example/v.mp4").unwrap());
        assert!(
            hash_and_check(incoming, &video, &hasher, &repo)
                .await
                .is_none()
        );
        let stored = repo.find_by_id(incoming).await.unwrap().unwrap();
        assert_eq!(stored.phash, None);
    }

    #[tokio::test]
    async fn hash_failure_never_blocks() {
        let repo = InMemoryPostRepository::new();
        let incoming = post_with(&repo, 1).await;
        let hasher = StubHasher(Default::default()); // every fetch fails
        assert!(
            hash_and_check(incoming, &photo("/nope.png"), &hasher, &repo)
                .await
                .is_none()
        );
    }
}

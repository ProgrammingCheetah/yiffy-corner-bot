//! Perceptual hashing — the "resist duplication of content" invariant.
//!
//! Source URLs dedupe exact re-submissions; the pHash catches the same
//! *picture* arriving under a different URL (re-uploads, cross-platform
//! mirrors). Hashes are 64-bit dHashes computed from the resolved media at
//! submission time and compared by Hamming distance — a near match flags
//! the submission to moderators, it never auto-rejects (variants and crops
//! are a human call).

use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum PHashError {
    #[error("media fetch failed: {0}")]
    Fetch(String),
    #[error("image decode failed: {0}")]
    Decode(String),
}

/// Outbound port: fetch a still image and produce its 64-bit dHash.
/// Only images hash — callers skip video/link/copy media.
#[async_trait::async_trait]
pub trait PerceptualHasher: Send + Sync {
    async fn hash_image(&self, url: &Url) -> Result<u64, PHashError>;
}

/// Hamming distance ceiling at or under which two hashes read as "the same
/// picture". dHash convention: 0 = identical, ≤10 = near-duplicate.
pub const NEAR_DUPLICATE_DISTANCE: u32 = 10;

/// Bits differing between two 64-bit hashes.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hamming_counts_differing_bits() {
        assert_eq!(hamming(0, 0), 0);
        assert_eq!(hamming(0, u64::MAX), 64);
        assert_eq!(hamming(0b1010, 0b0110), 2);
    }
}

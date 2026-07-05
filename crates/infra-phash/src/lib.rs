//! HTTP-fetching dHash implementation of the [`PerceptualHasher`] port.
//!
//! dHash (difference hash): grayscale → shrink to 9×8 → each bit is
//! "left pixel darker than its right neighbour". Robust to rescaling and
//! recompression, which is exactly the cross-platform re-upload case the
//! duplicate check exists for. Implemented by hand on the `image` crate —
//! ~15 lines beats another dependency.

use domain::elements::phash::{PHashError, PerceptualHasher};
use image::imageops::FilterType;
use url::Url;

/// Refuse to buffer media larger than this — dHash needs 72 pixels, not a
/// 200 MB source file. Covers every realistic still image.
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

pub struct HttpPerceptualHasher {
    client: reqwest::Client,
}

impl HttpPerceptualHasher {
    pub fn new(user_agent: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent(user_agent)
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("static client config is valid"),
        }
    }
}

/// The 64-bit dHash of an encoded image.
pub fn dhash(bytes: &[u8]) -> Result<u64, PHashError> {
    let img = image::load_from_memory(bytes).map_err(|e| PHashError::Decode(e.to_string()))?;
    let gray = image::imageops::resize(&img.to_luma8(), 9, 8, FilterType::Triangle);
    let mut hash = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            let left = gray.get_pixel(x, y).0[0];
            let right = gray.get_pixel(x + 1, y).0[0];
            hash = (hash << 1) | u64::from(left < right);
        }
    }
    Ok(hash)
}

#[async_trait::async_trait]
impl PerceptualHasher for HttpPerceptualHasher {
    async fn hash_image(&self, url: &Url) -> Result<u64, PHashError> {
        let response = self
            .client
            .get(url.clone())
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| PHashError::Fetch(e.to_string()))?;
        if let Some(length) = response.content_length() {
            if length as usize > MAX_IMAGE_BYTES {
                return Err(PHashError::Fetch(format!(
                    "media too large to hash: {length} bytes"
                )));
            }
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| PHashError::Fetch(e.to_string()))?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(PHashError::Fetch(format!(
                "media too large to hash: {} bytes",
                bytes.len()
            )));
        }
        dhash(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::elements::phash::hamming;
    use image::{DynamicImage, RgbImage};

    fn encode_png(img: &DynamicImage) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    /// A deterministic low-frequency test image (smooth radial gradient —
    /// the frequency profile of real art, which dHash is built for).
    fn test_image(width: u32, height: u32) -> DynamicImage {
        let (cx, cy) = (width as f32 * 0.3, height as f32 * 0.6);
        DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let max = (width.max(height)) as f32;
            let v = (255.0 * (1.0 - (d / max).min(1.0))) as u8;
            image::Rgb([v, (x as f32 / width as f32 * 255.0) as u8, 200 - v / 2])
        }))
    }

    #[test]
    fn same_image_hashes_identically() {
        let bytes = encode_png(&test_image(200, 150));
        assert_eq!(dhash(&bytes).unwrap(), dhash(&bytes).unwrap());
    }

    #[test]
    fn rescaled_image_stays_within_near_duplicate_distance() {
        use domain::elements::phash::NEAR_DUPLICATE_DISTANCE;

        let original = test_image(400, 300);
        let scaled = original.resize_exact(200, 150, FilterType::Triangle);
        let a = dhash(&encode_png(&original)).unwrap();
        let b = dhash(&encode_png(&scaled)).unwrap();
        assert!(
            hamming(a, b) <= NEAR_DUPLICATE_DISTANCE,
            "rescale drifted {} bits",
            hamming(a, b)
        );
    }

    #[test]
    fn different_images_are_far_apart() {
        let a = dhash(&encode_png(&test_image(200, 150))).unwrap();
        // Opposite-corner gradient: structurally different content.
        let other = DynamicImage::ImageRgb8(RgbImage::from_fn(200, 150, |x, y| {
            let v = (x as f32 / 200.0 * 255.0) as u8;
            image::Rgb([255 - v, (y as f32 / 150.0 * 255.0) as u8, v])
        }));
        let b = dhash(&encode_png(&other)).unwrap();
        assert!(hamming(a, b) > 10, "only {} bits apart", hamming(a, b));
    }

    #[test]
    fn garbage_bytes_fail_to_decode() {
        assert!(matches!(
            dhash(b"not an image at all"),
            Err(PHashError::Decode(_))
        ));
    }
}

use super::*;
use crate::decode;

// Original 4x3 solid-color fixtures generated with libwebp (via Pillow), not
// image-webp's encoder. Reference RGBA comes from independent libwebp decoding.
const LOSSY: &[u8] = &[
    82, 73, 70, 70, 62, 0, 0, 0, 87, 69, 66, 80, 86, 80, 56, 32, 50, 0, 0, 0, 208, 1, 0, 157, 1,
    42, 4, 0, 3, 0, 0, 192, 18, 37, 160, 2, 116, 186, 1, 248, 0, 3, 176, 0, 254, 245, 137, 31, 255,
    50, 243, 231, 17, 126, 101, 231, 252, 203, 207, 254, 221, 159, 215, 103, 245, 217, 255, 111,
    192, 0,
];
const ALPHA: &[u8] = &[
    82, 73, 70, 70, 98, 0, 0, 0, 87, 69, 66, 80, 86, 80, 56, 88, 10, 0, 0, 0, 16, 0, 0, 0, 3, 0, 0,
    2, 0, 0, 65, 76, 80, 72, 10, 0, 0, 0, 1, 7, 80, 192, 136, 8, 68, 68, 255, 3, 86, 80, 56, 32,
    50, 0, 0, 0, 208, 1, 0, 157, 1, 42, 4, 0, 3, 0, 0, 192, 18, 37, 160, 2, 116, 186, 1, 248, 0, 3,
    176, 0, 254, 245, 137, 31, 255, 50, 243, 231, 17, 126, 101, 231, 252, 203, 207, 254, 221, 159,
    215, 103, 245, 217, 255, 111, 192, 0,
];
const LOSSLESS: &[u8] = &[
    82, 73, 70, 70, 30, 0, 0, 0, 87, 69, 66, 80, 86, 80, 56, 76, 17, 0, 0, 0, 47, 3, 128, 0, 16, 7,
    80, 178, 162, 20, 185, 128, 129, 136, 232, 127, 0, 0,
];
const ANIM: &[u8] = &[
    82, 73, 70, 70, 136, 0, 0, 0, 87, 69, 66, 80, 86, 80, 56, 88, 10, 0, 0, 0, 18, 0, 0, 0, 3, 0,
    0, 2, 0, 0, 65, 78, 73, 77, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 65, 78, 77, 70, 42, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 3, 0, 0, 2, 0, 0, 100, 0, 0, 2, 86, 80, 56, 76, 17, 0, 0, 0, 47, 3, 128, 0, 16, 7, 80,
    178, 162, 20, 185, 128, 129, 136, 232, 127, 0, 0, 65, 78, 77, 70, 42, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 3, 0, 0, 2, 0, 0, 100, 0, 0, 0, 86, 80, 56, 76, 17, 0, 0, 0, 47, 3, 128, 0, 0, 7, 80, 178,
    34, 23, 165, 255, 129, 136, 232, 127, 0, 0,
];

#[test]
fn lossy_lossless_and_alpha_use_signature_dispatch() {
    for (bytes, expected, tolerance) in [
        (LOSSY, [39, 100, 200, 255], 2),
        (ALPHA, [39, 100, 200, 128], 2),
        (LOSSLESS, [40, 100, 200, 128], 0),
    ] {
        let info = probe(&bytes[..33], bytes.len(), ImageLimits::default()).unwrap();
        assert_eq!(
            info,
            ImageInfo {
                width: 4,
                height: 3,
                format: ImageFormat::WebP
            }
        );
        let image = decode(bytes, ImageLimits::default()).unwrap();
        assert_eq!((image.width(), image.height()), (4, 3));
        for pixel in image.pixels().chunks_exact(4) {
            for c in 0..3 {
                assert!(pixel[c].abs_diff(expected[c]) <= tolerance, "{pixel:?}");
            }
            assert_eq!(pixel[3], expected[3]);
        }
    }
}
#[test]
fn animated_webp_presents_first_frame_without_playback() {
    let image = decode(ANIM, ImageLimits::default()).unwrap();
    assert!(
        image
            .pixels()
            .chunks_exact(4)
            .all(|p| p == [40, 100, 200, 128])
    );
}
#[test]
fn truncations_size_limits_and_internal_geometry_fail() {
    for bytes in [LOSSY, ALPHA, LOSSLESS, ANIM] {
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end], ImageLimits::default()).is_err());
        }
        assert!(matches!(
            decode(
                bytes,
                ImageLimits {
                    max_pixels: 11,
                    ..ImageLimits::default()
                }
            ),
            Err(ImageError::Budget)
        ));
        assert!(matches!(
            decode(
                bytes,
                ImageLimits {
                    max_file_bytes: bytes.len() - 1,
                    ..ImageLimits::default()
                }
            ),
            Err(ImageError::Budget)
        ));
        assert!(matches!(
            decode(
                bytes,
                ImageLimits {
                    max_decoded_bytes: 47,
                    ..ImageLimits::default()
                }
            ),
            Err(ImageError::Budget)
        ));
    }
    let mut bad = ALPHA.to_vec();
    bad[24] = 4; // Canvas 5px vs actual 4px.
    assert!(decode(&bad, ImageLimits::default()).is_err());
    let mut bad = ANIM.to_vec();
    bad[52] = 9; // Frame x outside its canvas.
    assert!(decode(&bad, ImageLimits::default()).is_err());
    assert!(decode(b"not an image", ImageLimits::default()).is_err());
}
#[test]
fn payload_failure_is_not_confused_with_header_probe_success() {
    let mut bad = LOSSLESS[..25].to_vec();
    let riff = (bad.len() - 8 + 1) as u32;
    bad[4..8].copy_from_slice(&riff.to_le_bytes());
    bad[16..20].copy_from_slice(&5_u32.to_le_bytes());
    bad.push(0);
    assert!(probe(&bad, bad.len(), ImageLimits::default()).is_ok());
    assert!(decode(&bad, ImageLimits::default()).is_err());
}

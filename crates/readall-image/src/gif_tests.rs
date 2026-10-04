use crate::{
    ImageError, ImageFormat, ImageLimits, decode, decode_gif, decode_svg_with_resource_loader,
    decode_svg_with_resources, probe,
};
#[path = "../../../apps/readall/tests/support/gif.rs"]
mod fixture;

#[test]
fn gif87a_and_89a_headers_dispatch_without_extension_or_pixel_decode() {
    let bytes = fixture::solid();
    for version in [b"GIF87a", b"GIF89a"] {
        let mut data = bytes.clone();
        data[..6].copy_from_slice(version);
        let info = probe(&data[..13], data.len(), ImageLimits::default()).unwrap();
        assert_eq!(
            (info.width, info.height, info.format),
            (8, 8, ImageFormat::Gif)
        );
        let image = decode(&data, ImageLimits::default()).unwrap();
        assert!(
            image
                .pixels()
                .chunks_exact(4)
                .all(|p| p == [231, 43, 71, 255])
        );
    }
    assert!(probe(&bytes[..12], bytes.len(), ImageLimits::default()).is_err());
}
#[test]
fn gif_global_local_palettes_interlacing_transparency_and_canvas_offsets() {
    let pixels: Vec<_> = (0..13).flat_map(|y| [0, 1 + (y % 3) as u8, 2]).collect();
    for local in [false, true] {
        for interlaced in [false, true] {
            let bytes = fixture::image(7, 17, (2, 1, 3, 13), &pixels, interlaced, local, Some(0));
            let image = decode_gif(&bytes, ImageLimits::default()).unwrap();
            assert_eq!((image.width(), image.height()), (7, 17));
            for y in 0..17 {
                for x in 0..7 {
                    let pixel = &image.pixels()[(y * 7 + x) * 4..(y * 7 + x + 1) * 4];
                    let expected = if (1..14).contains(&y) && (2..5).contains(&x) {
                        match pixels[(y - 1) * 3 + x - 2] {
                            0 => [0, 0, 0, 0],
                            1 => [231, 43, 71, 255],
                            2 => [19, 167, 83, 255],
                            _ => [37, 89, 211, 255],
                        }
                    } else {
                        [0, 0, 0, 0]
                    };
                    assert_eq!(
                        pixel, expected,
                        "local={local} interlaced={interlaced} pixel=({x},{y})"
                    );
                }
            }
        }
    }
}
#[test]
fn independent_encoder_gif_and_embedded_data_uri_render() {
    // Original solid-color image encoded independently with Pillow, using a growing LZW dictionary.
    const DATA: &[u8] = &[
        71, 73, 70, 56, 55, 97, 8, 0, 8, 0, 129, 0, 0, 231, 43, 71, 0, 0, 0, 0, 0, 0, 0, 0, 0, 44,
        0, 0, 0, 0, 8, 0, 8, 0, 0, 8, 15, 0, 1, 8, 28, 72, 176, 160, 193, 131, 8, 19, 42, 76, 24,
        16, 0, 59,
    ];
    assert!(
        decode_gif(DATA, ImageLimits::default())
            .unwrap()
            .pixels()
            .chunks_exact(4)
            .all(|p| p == [231, 43, 71, 255])
    );
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><image href="data:image/gif;base64,R0lGODdhCAAIAIEAAOcrRwAAAAAAAAAAACwAAAAACAAIAAAIDwABCBxIsKDBgwgTKkwYEAA7" width="8" height="8"/></svg>"#;
    let image = decode_svg_with_resources(svg, ImageLimits::default(), &[], &|_| {
        panic!("data URI must not use external loader")
    })
    .unwrap();
    assert!(
        image
            .pixels()
            .chunks_exact(4)
            .all(|p| p == [231, 43, 71, 255])
    );
}

#[test]
fn gif_animation_only_renders_first_frame() {
    let mut bytes = fixture::solid();
    bytes.pop();
    fixture::append_frame(&mut bytes, (0, 0, 8, 8), &[2; 64], false, true);
    bytes.push(0x3b);
    let image = decode_gif(&bytes, ImageLimits::default()).unwrap();
    assert!(
        image
            .pixels()
            .chunks_exact(4)
            .all(|p| p == [231, 43, 71, 255])
    );
}
#[test]
fn gif_corruption_truncation_bounds_and_budgets_fail() {
    let bytes = fixture::solid();
    for end in 0..bytes.len() {
        assert!(
            decode_gif(&bytes[..end], ImageLimits::default()).is_err(),
            "accepted truncation {end}"
        );
    }
    for limits in [
        ImageLimits {
            max_pixels: 63,
            ..Default::default()
        },
        ImageLimits {
            max_file_bytes: bytes.len() - 1,
            ..Default::default()
        },
        ImageLimits {
            max_decoded_bytes: 255,
            ..Default::default()
        },
    ] {
        assert!(decode_gif(&bytes, limits).is_err());
    }
    for at in [6, 8] {
        let mut bad = bytes.clone();
        bad[at..at + 2].fill(255);
        assert!(decode_gif(&bad, ImageLimits::default()).is_err());
    }
    let mut bad = bytes.clone();
    bad[26..28].copy_from_slice(&8_u16.to_le_bytes());
    assert!(decode_gif(&bad, ImageLimits::default()).is_err());
    // Two-entry palette, but index 3 in data.
    let mut indexed = fixture::image(1, 1, (0, 0, 1, 1), &[3], false, false, None);
    indexed[10] = 0x80;
    indexed.drain(19..25);
    assert!(
        decode_gif(&indexed, ImageLimits::default())
            .unwrap_err()
            .to_string()
            .contains("palette index")
    );
    // Bounded mutation smoke test for container and codec error handling.
    for i in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[i] ^= 255;
        let _ = decode_gif(&bad, ImageLimits::default());
    }
}
#[test]
fn gif_inside_svg_is_rendered_through_href_and_xlink() {
    for attribute in ["href", "xlink:href"] {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="16" height="16"><image {attribute}="../Images/P1.gif" width="16" height="16"/></svg>"#
        );
        let image =
            decode_svg_with_resources(svg.as_bytes(), ImageLimits::default(), &[], &|href| {
                assert_eq!(href, "../Images/P1.gif");
                Some(fixture::solid())
            })
            .unwrap();
        assert!(
            image
                .pixels()
                .chunks_exact(4)
                .any(|p| p == [231, 43, 71, 255])
        );
    }
}
#[test]
fn svg_child_errors_preserve_reference_and_reason_without_unsafe_fallback() {
    let svg=br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><image href="../Images/P1.gif" width="8" height="8"/></svg>"#;
    let error = decode_svg_with_resource_loader(svg, ImageLimits::default(), &[], &|_| {
        Err("missing from ZIP".into())
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("../Images/P1.gif") && error.contains("missing from ZIP"));
    let error = decode_svg_with_resources(svg, ImageLimits::default(), &[], &|_| {
        Some(b"GIF89a".to_vec())
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("../Images/P1.gif") && error.contains("GIF"));
    for href in [
        "/etc/passwd",
        "https://example.invalid/a.gif",
        "file:///tmp/a.gif",
        "../bad\\a.gif",
    ] {
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><image href="{href}" width="8" height="8"/></svg>"#
        );
        let error = decode_svg_with_resources(svg.as_bytes(), ImageLimits::default(), &[], &|_| {
            panic!("unsafe resource loader called")
        })
        .unwrap_err();
        assert!(matches!(error, ImageError::SvgResource { .. }));
        assert!(error.to_string().contains("disallowed"));
    }
}
#[test]
fn svg_child_count_budget_stops_before_loading_additional_resources() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = AtomicUsize::new(0);
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">{}</svg>"#,
        (0..40)
            .map(|i| format!(r#"<image href="{i}.gif" width="8" height="8"/>"#))
            .collect::<String>()
    );
    let error = decode_svg_with_resources(svg.as_bytes(), ImageLimits::default(), &[], &|_| {
        calls.fetch_add(1, Ordering::Relaxed);
        Some(fixture::solid())
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("count exceeds 32"));
    assert_eq!(calls.load(Ordering::Relaxed), 32);
}

//! Sandboxed static SVG adapter. Resource callbacks never fall back to the filesystem.
//! Raster children are validated by ReadAll and normalized to bounded PNG buffers.
use crate::{ImageError, ImageFormat, ImageInfo, ImageLimits, Result, RgbaImage, decode, probe};
use resvg::{tiny_skia, usvg};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
fn source(bytes: &[u8], full_len: usize, limits: ImageLimits) -> Result<&str> {
    if full_len > limits.max_file_bytes.min(4 * 1024 * 1024) {
        return Err(ImageError::Budget);
    }
    if bytes.len() < full_len {
        return Err(ImageError::IncompleteHeader);
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ImageError::Invalid("SVG must be UTF-8"))?
        .trim_start_matches('\u{feff}');
    let doc = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 25_000,
            ..roxmltree::ParsingOptions::default()
        },
    )
    .map_err(|_| ImageError::Invalid("invalid or oversized SVG XML"))?;
    if doc.root_element().tag_name().name() != "svg" {
        return Err(ImageError::Invalid("image XML root is not SVG"));
    }
    for node in doc.descendants().filter(|n| n.is_element()) {
        if node.ancestors().take(98).count() > 96 {
            return Err(ImageError::Budget);
        }
        if matches!(
            node.tag_name().name(),
            "filter" | "mask" | "pattern" | "foreignObject"
        ) {
            return Err(ImageError::Unsupported(
                "SVG filters, masks, patterns and foreignObject are not enabled",
            ));
        }
    }
    Ok(text)
}
fn metadata_options() -> usvg::Options<'static> {
    usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    }
}
pub(crate) fn header(bytes: &[u8], full_len: usize, limits: ImageLimits) -> Result<ImageInfo> {
    let text = source(bytes, full_len, limits)?;
    let tree = usvg::Tree::from_str(text, &metadata_options())
        .map_err(|_| ImageError::Invalid("invalid SVG tree"))?;
    let size = tree.size().to_int_size();
    ImageInfo {
        width: size.width(),
        height: size.height(),
        format: ImageFormat::Svg,
    }
    .checked(limits)
}
fn check_group(
    group: &usvg::Group,
    depth: usize,
    count: &mut usize,
    limits: ImageLimits,
) -> Result<()> {
    if depth > 96 {
        return Err(ImageError::Budget);
    }
    for node in group.children() {
        *count += 1;
        if *count > 25_000 {
            return Err(ImageError::Budget);
        }
        if let Some(rect) = node.abs_layer_bounding_box() {
            let area = f64::from(rect.width()) * f64::from(rect.height());
            if !area.is_finite() || area > limits.max_pixels as f64 * 4.0 {
                return Err(ImageError::Budget);
            }
        }
        if let usvg::Node::Group(group) = node {
            check_group(group, depth + 1, count, limits)?;
        }
        let mut result = Ok(());
        node.subroots(|subroot| {
            if result.is_ok() {
                result = check_group(subroot, depth + 1, count, limits);
            }
        });
        result?;
    }
    Ok(())
}
/// Font bytes and resources are supplied by the publication owner. Empty callbacks
/// mean no fonts/resources, not permission to inspect local files or use the network.
pub fn decode_svg_with_resources(
    bytes: &[u8],
    limits: ImageLimits,
    fonts: &[&[u8]],
    resource: &(dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync),
) -> Result<RgbaImage> {
    decode_svg_with_resource_loader(bytes, limits, fonts, &|href| {
        resource(href).ok_or_else(|| "resource loader returned no data".into())
    })
}

/// Checked loader variant: preserve the publication's missing-resource, path-policy,
/// CRC and size errors rather than turning them all into an ambiguous boolean.
pub fn decode_svg_with_resource_loader(
    bytes: &[u8],
    limits: ImageLimits,
    fonts: &[&[u8]],
    resource: &(dyn Fn(&str) -> std::result::Result<Vec<u8>, String> + Send + Sync),
) -> Result<RgbaImage> {
    let info = header(bytes, bytes.len(), limits)?;
    let text = source(bytes, bytes.len(), limits)?;
    let budget = AtomicUsize::new(0);
    let input_bytes = AtomicUsize::new(0);
    let resources = AtomicUsize::new(0);
    let failure: Mutex<Option<ImageError>> = Mutex::new(None);
    let fail = |reference: &str, reason: String| {
        let mut error = failure.lock().unwrap_or_else(|e| e.into_inner());
        if error.is_none() {
            *error = Some(ImageError::SvgResource {
                reference: reference.chars().take(512).collect(),
                reason: reason
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(1024)
                    .collect(),
            });
        }
    };
    let claim = |reference: &str| {
        if failure.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
            return false;
        }
        if resources.fetch_add(1, Ordering::Relaxed) >= 32 {
            fail(reference, "SVG child resource count exceeds 32".into());
            return false;
        }
        true
    };
    let normalize = |reference: &str, data: &[u8]| -> Option<usvg::ImageKind> {
        let normalized = (|| -> Result<Vec<u8>> {
            if !(data.starts_with(b"\x89PNG\r\n\x1a\n")
                || data.starts_with(&[255, 216])
                || data.starts_with(b"RIFF")
                || data.starts_with(b"GIF87a")
                || data.starts_with(b"GIF89a"))
            {
                return Err(ImageError::Unsupported(
                    "nested SVG resources must be PNG, JPEG, WebP or GIF",
                ));
            }
            let previous = input_bytes.fetch_add(data.len(), Ordering::Relaxed);
            if previous.saturating_add(data.len()) > 64 * 1024 * 1024 {
                return Err(ImageError::Budget);
            }
            let image_info = probe(data, data.len(), limits)?;
            let size = image_info.rgba_bytes()?;
            let previous = budget.fetch_add(size, Ordering::Relaxed);
            if previous.saturating_add(size) > limits.max_decoded_bytes.min(64 * 1024 * 1024) {
                return Err(ImageError::Budget);
            }
            encode_png(&decode(data, limits)?)
        })();
        match normalized {
            Ok(data) => Some(usvg::ImageKind::PNG(Arc::new(data))),
            Err(error) => {
                fail(reference, error.to_string());
                None
            }
        }
    };
    let mut options = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, data, _| {
                // Never print full data URIs (possibly huge or sensitive) in diagnostics.
                let reference = "<embedded data image>";
                claim(reference)
                    .then(|| normalize(reference, &data))
                    .flatten()
            }),
            resolve_string: Box::new(|href, _| {
                if !claim(href) {
                    return None;
                }
                if href.len() > 4096
                    || href.starts_with('/')
                    || href.contains(':')
                    || href.contains('\\')
                {
                    fail(
                        href,
                        "non-local or invalid SVG resource URI is disallowed".into(),
                    );
                    return None;
                }
                match resource(href) {
                    Ok(data) => normalize(href, &data),
                    Err(error) => {
                        fail(href, error);
                        None
                    }
                }
            }),
        },
        ..usvg::Options::default()
    };
    let mut font_bytes = 0_usize;
    for font in fonts.iter().take(12) {
        font_bytes = font_bytes.saturating_add(font.len());
        if font_bytes > 96 * 1024 * 1024 {
            return Err(ImageError::Budget);
        }
        options.fontdb_mut().load_font_data(font.to_vec());
    }
    let family = options
        .fontdb
        .faces()
        .next()
        .and_then(|face| face.families.first())
        .map(|family| family.0.clone());
    if let Some(family) = family {
        options.font_family = family.clone();
        let db = options.fontdb_mut();
        db.set_serif_family(&family);
        db.set_sans_serif_family(&family);
        db.set_monospace_family(&family);
    }
    let tree = usvg::Tree::from_str(text, &options)
        .map_err(|_| ImageError::Invalid("invalid SVG tree"))?;
    if let Some(error) = failure.lock().unwrap_or_else(|e| e.into_inner()).take() {
        return Err(error);
    }
    if tree.size().to_int_size().width() != info.width
        || tree.size().to_int_size().height() != info.height
    {
        return Err(ImageError::Invalid("SVG dimensions disagree"));
    }
    check_group(tree.root(), 0, &mut 0, limits)?;
    let mut pixmap =
        tiny_skia::Pixmap::new(info.width, info.height).ok_or(ImageError::Allocation)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    let mut pixels = pixmap.take();
    for rgba in pixels.chunks_exact_mut(4) {
        let a = u16::from(rgba[3]);
        for c in &mut rgba[..3] {
            *c = (u16::from(*c) * 255 + a / 2)
                .checked_div(a)
                .unwrap_or(0)
                .min(255) as u8;
        }
    }
    Ok(RgbaImage {
        width: info.width,
        height: info.height,
        pixels,
    })
}
/// A simple bounded PNG encoder for already-decoded raster children; stored DEFLATE.
fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&readall_archive::crc32(&out[start..]).to_be_bytes());
    }
    let mut raw = Vec::new();
    let size = (image.width as usize * 4 + 1) * image.height as usize;
    raw.try_reserve_exact(size)
        .map_err(|_| ImageError::Allocation)?;
    for row in image.pixels.chunks_exact(image.width as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = Vec::new();
    z.try_reserve_exact(size + size / 65535 * 5 + 16)
        .map_err(|_| ImageError::Allocation)?;
    z.extend_from_slice(&[0x78, 0x01]);
    let total = raw.chunks(65535).len();
    for (index, part) in raw.chunks(65535).enumerate() {
        let n = part.len() as u16;
        z.push(u8::from(index + 1 == total));
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(part);
    }
    let (mut a, mut b) = (1_u32, 0_u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&(b << 16 | a).to_be_bytes());
    let mut output = Vec::new();
    output
        .try_reserve_exact(z.len() + 80)
        .map_err(|_| ImageError::Allocation)?;
    output.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut header = Vec::new();
    header.extend_from_slice(&image.width.to_be_bytes());
    header.extend_from_slice(&image.height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut output, b"IHDR", &header);
    chunk(&mut output, b"IDAT", &z);
    chunk(&mut output, b"IEND", &[]);
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vectors_transform_clip_and_alpha_are_rendered() {
        let bytes=br#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="24"><g transform="translate(4,3)"><rect width="12" height="10" fill="blue" fill-opacity="0.5"/></g></svg>"#;
        let image =
            decode_svg_with_resources(bytes, ImageLimits::default(), &[], &|_| None).unwrap();
        assert_eq!((image.width(), image.height()), (32, 24));
        let p = &image.pixels[(5 * 32 + 6) * 4..(5 * 32 + 6) * 4 + 4];
        assert_eq!(p, [0, 0, 255, 128]);
    }
    #[test]
    fn filesystem_network_dtd_and_nested_svg_are_not_loaded() {
        for href in [
            "/etc/passwd",
            "https://example.invalid/a.png",
            "file:///tmp/a.png",
        ] {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><image href="{href}" width="8" height="8"/></svg>"#
            );
            assert!(
                decode_svg_with_resources(
                    svg.as_bytes(),
                    ImageLimits::default(),
                    &[],
                    &|_| panic!("external loader called")
                )
                .is_err()
            );
        }
        assert!(header(b"<!DOCTYPE svg><svg/>", 20, ImageLimits::default()).is_err());
    }
    #[test]
    fn local_raster_callback_is_validated_and_rendered() {
        let raster = RgbaImage {
            width: 2,
            height: 2,
            pixels: [240, 20, 40, 255].repeat(4),
        };
        let png = encode_png(&raster).unwrap();
        let svg=br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><image href="a.png" width="8" height="8"/></svg>"#;
        let image = decode_svg_with_resources(svg, ImageLimits::default(), &[], &|href| {
            (href == "a.png").then(|| png.clone())
        })
        .unwrap();
        assert!(
            image
                .pixels
                .chunks_exact(4)
                .any(|pixel| pixel == [240, 20, 40, 255])
        );
        assert!(
            decode_svg_with_resources(svg, ImageLimits::default(), &[], &|_| Some(
                b"not PNG".to_vec()
            ))
            .is_err()
        );
    }
}

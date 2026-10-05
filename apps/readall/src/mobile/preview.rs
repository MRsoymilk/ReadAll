//! Metadata and bounded cover thumbnails; no reader worker, font loading, or body pagination.
use readall_core::read_bounded;
use readall_epub::{EpubBook, EpubLimits};
use readall_image::{ImageLimits, RgbaImage};
use readall_mobi::{MobiBook, MobiLimits};
use readall_platform::LocalFileSource;
use std::{error::Error, path::Path};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
pub const PREVIEW_BYTES: usize = 384 * 512 * 4;
pub struct BookPreview {
    pub title: String,
    pub author: String,
    pub format: &'static str,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
pub fn book_preview(path: &Path) -> Result<BookPreview> {
    if !path.is_absolute() {
        return Err("preview needs an absolute local path".into());
    }
    let mut source = LocalFileSource::open(path)?;
    let bytes = read_bounded(&mut source, 128 * 1024 * 1024)?;
    preview_bytes(&bytes)
}
fn preview_bytes(bytes: &[u8]) -> Result<BookPreview> {
    let (title, author, format, cover) = if readall_mobi::is_mobi(bytes) {
        let book = MobiBook::parse(bytes, MobiLimits::default())?;
        let meta = book.metadata();
        (
            meta.title.clone(),
            meta.author.clone().unwrap_or_default(),
            if meta.version == 8 { "AZW3" } else { "MOBI" },
            book.cover_image().ok().flatten().map(ToOwned::to_owned),
        )
    } else {
        let book = EpubBook::parse(bytes, EpubLimits::default())?;
        (
            book.title().unwrap_or("").to_owned(),
            book.creator().unwrap_or("").to_owned(),
            "EPUB",
            book.cover_resource(8 * 1024 * 1024).ok().flatten(),
        )
    };
    let mut result = BookPreview {
        title: title.chars().take(512).collect(),
        author: author.chars().take(512).collect(),
        format,
        width: 0,
        height: 0,
        rgba: Vec::new(),
    };
    if let Some(encoded) = cover {
        let limits = ImageLimits {
            max_file_bytes: 8 * 1024 * 1024,
            max_pixels: 8 * 1024 * 1024,
            max_decoded_bytes: 40 * 1024 * 1024,
        };
        if let Ok(image) = readall_image::decode(&encoded, limits) {
            let (w, h, pixels) = thumbnail(&image);
            result.width = w;
            result.height = h;
            result.rgba = pixels;
        }
    }
    Ok(result)
}
fn thumbnail(image: &RgbaImage) -> (u32, u32, Vec<u8>) {
    let scale = (384.0_f64 / f64::from(image.width()))
        .min(512.0 / f64::from(image.height()))
        .min(1.0);
    let width = (f64::from(image.width()) * scale).round().max(1.0) as u32;
    let height = (f64::from(image.height()) * scale).round().max(1.0) as u32;
    let mut out = vec![0_u8; (width * height * 4) as usize];
    // Bilinear resampling, flattened on paper: no premultiplication mismatch at JNI.
    for y in 0..height {
        let sy =
            ((f64::from(y) + 0.5) * f64::from(image.height()) / f64::from(height) - 0.5).max(0.0);
        let y0 = sy.floor() as u32;
        let y1 = (y0 + 1).min(image.height() - 1);
        let fy = sy.fract();
        for x in 0..width {
            let sx =
                ((f64::from(x) + 0.5) * f64::from(image.width()) / f64::from(width) - 0.5).max(0.0);
            let x0 = sx.floor() as u32;
            let x1 = (x0 + 1).min(image.width() - 1);
            let fx = sx.fract();
            let at = ((y * width + x) * 4) as usize;
            for c in 0..3 {
                let pixel = |xx: u32, yy: u32| {
                    let i = ((yy * image.width() + xx) * 4) as usize;
                    let a = f64::from(image.pixels()[i + 3]) / 255.0;
                    f64::from(image.pixels()[i + c]) * a + 255.0 * (1.0 - a)
                };
                out[at + c] = ((pixel(x0, y0) * (1.0 - fx) + pixel(x1, y0) * fx) * (1.0 - fy)
                    + (pixel(x0, y1) * (1.0 - fx) + pixel(x1, y1) * fx) * fy)
                    .round() as u8;
            }
            out[at + 3] = 255;
        }
    }
    (width, height, out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shelf_previews_do_not_require_body_layout_and_covers_are_bounded() {
        let source = crate::test_epub::make_epub_with_resources(
            &["<html><body>AAAA</body></html>"],
            vec![(
                "cover.png",
                "image/png",
                crate::test_epub::make_png(800, 1000, [10, 90, 180, 255]),
            )],
        );
        let p = preview_bytes(&source).unwrap();
        assert_eq!(p.format, "EPUB");
        assert_eq!((p.width, p.height), (384, 480));
        assert!(p.rgba.len() <= PREVIEW_BYTES);
        assert_eq!(&p.rgba[..4], &[10, 90, 180, 255]);
        let mobi = crate::test_mobi::make_mobi("<html><body>AAAA</body></html>");
        assert_eq!(preview_bytes(&mobi).unwrap().format, "MOBI");
        assert!(preview_bytes(b"not a book").is_err());
        assert!(book_preview(Path::new("relative.epub")).is_err());
    }
    #[test]
    fn mobi_preview_reads_cover_record_without_decompressing_body() {
        let png = crate::test_epub::make_png(2, 3, [30, 80, 160, 255]);
        let bytes = crate::test_mobi::build(
            b"\xff\xff",
            crate::test_mobi::Options {
                images: vec![png],
                cover: Some(0),
                ..Default::default()
            },
        );
        let book = MobiBook::parse(&bytes, Default::default()).unwrap();
        assert!(book.cover_image().unwrap().is_some());
        assert!(book.to_epub().is_err());
        let p = preview_bytes(&bytes).unwrap();
        assert_eq!((p.format, p.width, p.height), ("MOBI", 2, 3));
        assert_eq!(&p.rgba[..4], &[30, 80, 160, 255]);
    }
    #[test]
    fn shelf_reads_dedicated_xhtml_and_svg_cover_wrappers_only() {
        for (name, mime, document) in [
            (
                "cover.xhtml",
                "application/xhtml+xml",
                "<html><body><svg xmlns:xlink='http://www.w3.org/1999/xlink'><image xlink:href='images/front.png'/></svg></body></html>",
            ),
            (
                "cover.svg",
                "image/svg+xml",
                "<svg><image href='images/front.png'/></svg>",
            ),
        ] {
            let bytes = crate::test_epub::make_epub_with_resources(
                &["<html><body>AAAA</body></html>"],
                vec![
                    (name, mime, document.as_bytes().to_vec()),
                    (
                        "images/front.png",
                        "image/png",
                        crate::test_epub::make_png(3, 5, [8, 88, 188, 255]),
                    ),
                ],
            );
            let p = preview_bytes(&bytes).unwrap();
            assert_eq!((p.width, p.height), (3, 5));
            assert_eq!(&p.rgba[..4], &[8, 88, 188, 255]);
        }
        for source in [
            "https://example.invalid/front.png",
            "file:///tmp/front.png",
            "../../outside.png",
            "absent.png",
        ] {
            let bytes = crate::test_epub::make_epub_with_resources(
                &["<html><body>AAAA</body></html>"],
                vec![(
                    "cover.xhtml",
                    "application/xhtml+xml",
                    format!("<html><body><img src='{source}'/></body></html>").into_bytes(),
                )],
            );
            assert_eq!(preview_bytes(&bytes).unwrap().width, 0);
        }
    }
    #[test]
    fn cover_byte_budget_is_enforced_before_archive_extraction() {
        let encoded = crate::test_epub::make_png(8, 8, [8, 88, 188, 255]);
        let count = encoded.len();
        let bytes = crate::test_epub::make_epub_with_resources(
            &["<html><body>AAAA</body></html>"],
            vec![("cover.png", "image/png", encoded)],
        );
        let book = EpubBook::parse(&bytes, Default::default()).unwrap();
        assert!(book.cover_resource(count - 1).is_err());
        assert_eq!(book.cover_resource(count).unwrap().unwrap().len(), count);
    }
    #[test]
    fn missing_or_invalid_cover_does_not_hide_a_book() {
        let p = preview_bytes(&crate::test_epub::make_epub()).unwrap();
        assert_eq!(p.width, 0);
        assert!(p.rgba.is_empty());
        let bad = crate::test_epub::make_epub_with_resources(
            &["<html><body>AAAA</body></html>"],
            vec![("cover.png", "image/png", b"broken image".to_vec())],
        );
        assert_eq!(preview_bytes(&bad).unwrap().height, 0);
    }
}

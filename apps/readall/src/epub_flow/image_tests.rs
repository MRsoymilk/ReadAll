use super::*;
use crate::{test_epub, test_font};
use readall_epub::EpubLimits;
use readall_font::FontLimits;

// Original 4x3 RGBA [40,100,200,128] lossless fixture encoded by libwebp.
const WEBP: &[u8] = &[
    82, 73, 70, 70, 30, 0, 0, 0, 87, 69, 66, 80, 86, 80, 56, 76, 17, 0, 0, 0, 47, 3, 128, 0, 16, 7,
    80, 178, 162, 20, 185, 128, 129, 136, 232, 127, 0, 0,
];
fn options() -> Options {
    Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "200",
            "--height",
            "128",
            "--margin",
            "16",
            "--font-size",
            "16",
        ]
        .map(Into::into),
    )
    .unwrap()
}

#[test]
fn hundreds_of_webp_images_render_past_64_and_reload_on_back_navigation() {
    let names: Vec<_> = (0..280).map(|n| format!("media/file{n}.webp")).collect();
    let body = format!(
        "<html><body>{}</body></html>",
        names
            .iter()
            .map(|name| format!("<img src='{name}'/><p>A</p>"))
            .collect::<String>()
    );
    let resources = names
        .iter()
        .map(|name| (name.as_str(), "image/webp", WEBP.to_vec()))
        .collect();
    let bytes = test_epub::make_epub_with_resources(&[&body], resources);
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    assert_eq!(chapter.images.borrow().stats(), (0, 0, 0, 0, 0));
    let font_bytes = test_font::make_font();
    let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let mut opts = options();
    let first = renderer.render(&chapter, &opts).unwrap();
    let (headers, decodes, _, _, warnings) = chapter.images.borrow().stats();
    assert_eq!(headers, 280);
    assert!(
        decodes > 0 && decodes < 280,
        "only current-page pixels should decode"
    );
    assert_eq!(warnings, 0);
    renderer.render(&chapter, &opts).unwrap();
    assert_eq!(chapter.images.borrow().stats().1, decodes);
    let expected = Color::rgba(40, 100, 200, 128).over(Color::WHITE);
    for page in 0..first.pages {
        opts.page = Some(page);
        let frame = renderer.render(&chapter, &opts).unwrap();
        // The final page can contain only trailing text; all image-bearing pages
        // must contain the actual WebP color rather than a missing-image box.
        let has_image = renderer.layout.as_ref().unwrap().pages[page]
            .items
            .iter()
            .any(|item| matches!(item, Item::Image { .. }));
        if has_image {
            assert!(
                frame.surface.pixels().contains(&expected),
                "image on page {page} missing"
            );
        }
        let (_, _, entries, resident, errors) = chapter.images.borrow().stats();
        assert!(entries <= 64);
        assert!(resident <= 64 * 1024 * 1024);
        assert_eq!(errors, 0);
    }
    let before = chapter.images.borrow().stats().1;
    opts.page = Some(0);
    let first_again = renderer.render(&chapter, &opts).unwrap();
    assert!(
        chapter.images.borrow().stats().1 > before,
        "evicted image should reload"
    );
    assert_eq!(first_again.surface.pixels(), first.surface.pixels());
    assert!(chapter.images.borrow_mut().get(279).is_some());
}

#[test]
fn many_failed_images_do_not_consume_success_cache_slots_or_repeat_warnings() {
    let body = format!(
        "<html><body>{}<img src='ok.png'/></body></html>",
        (0..70)
            .map(|n| format!("<img src='missing{n}.webp'/>"))
            .collect::<String>()
    );
    // Deliberately misleading suffix/MIME: dispatch is by signature.
    let bytes =
        test_epub::make_epub_with_resources(&[&body], vec![("ok.png", "image/png", WEBP.to_vec())]);
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let mut store = chapter.images.borrow_mut();
    for _ in 0..2 {
        for index in 0..70 {
            assert!(store.get(index).is_none());
        }
    }
    assert_eq!(store.stats(), (70, 0, 0, 0, 70));
    assert!(store.get(70).is_some());
    assert_eq!(store.stats(), (71, 1, 1, 48, 70));
}

#[test]
fn byte_budget_is_resident_not_lifetime_and_repeated_sources_share_cache() {
    let bytes = test_epub::make_epub_with_resources(
        &[
            "<html><body><img src='0.webp'/><img src='1.webp'/><img src='2.webp'/><img src='0.webp'/></body></html>",
        ],
        (0..3)
            .map(|n| {
                (
                    match n {
                        0 => "0.webp",
                        1 => "1.webp",
                        _ => "2.webp",
                    },
                    "image/webp",
                    WEBP.to_vec(),
                )
            })
            .collect(),
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let mut store = chapter.images.borrow_mut();
    store.set_cache_bytes(96);
    assert!(store.get(0).is_some());
    assert!(store.get(3).is_some());
    assert_eq!(store.stats().1, 1);
    for _ in 0..10 {
        for index in 0..3 {
            assert!(store.get(index).is_some());
            assert!(store.stats().3 <= 96);
        }
    }
    assert!(store.stats().1 > 3);
    assert_eq!(store.stats().4, 0);
    // Even an image larger than the cache cap may be drawn without retention.
    store.set_cache_bytes(1);
    assert!(store.get(0).is_some());
    assert_eq!(store.stats().3, 0);
}

#[test]
fn prefix_is_not_a_crc_bypass_and_payload_failure_is_local() {
    let mut bytes = test_epub::make_epub_with_resources(
        &["<html><body><img src='bad.webp'/><img src='good.webp'/></body></html>"],
        vec![
            ("bad.webp", "image/webp", WEBP.to_vec()),
            ("good.webp", "image/webp", WEBP.to_vec()),
        ],
    );
    let at = bytes
        .windows(WEBP.len())
        .position(|part| part == WEBP)
        .unwrap();
    bytes[at + 30] ^= 1; // Beyond the geometry header; leave ZIP CRC unchanged.
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let mut store = chapter.images.borrow_mut();
    assert!(store.info(0).is_some());
    assert_eq!(store.stats().1, 0);
    assert!(store.get(0).is_none());
    assert!(store.get(0).is_none());
    assert_eq!(store.stats().1, 1);
    assert_eq!(store.stats().4, 1);
    assert!(store.get(1).is_some());
}

use super::*;
use crate::{test_epub, test_font};
use readall_epub::EpubLimits;
use readall_font::FontLimits;

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
fn css_colors_alignment_and_indent_reach_rendered_pixels() {
    let bytes = test_epub::make_epub_with_resources(&["<html><head><link rel='stylesheet' href='styles/main.css'/></head><body><h1>A</h1><p class='lead'>WW</p></body></html>"], vec![("styles/main.css", "text/css", b"h1 { color:red; text-align:center; font-size:150% } .lead { color:blue; text-indent:2em }".to_vec())]);
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    assert_eq!(chapter.text(), book.read_spine_text(0).unwrap());
    let font_bytes = test_font::make_font();
    let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let frame = renderer.render(&chapter, &options()).unwrap();
    assert!(
        frame
            .surface
            .pixels()
            .iter()
            .any(|p| p.r > p.b && p.r > p.g)
    );
    assert!(
        frame
            .surface
            .pixels()
            .iter()
            .any(|p| p.b > p.r && p.b > p.g)
    );
    let lines: Vec<_> = renderer.layout.as_ref().unwrap().pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Text(line) => Some(line),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].x > 40.0);
    assert_eq!(lines[1].x, 32.0);
}

#[test]
fn text_image_text_paginates_without_dropping_text_and_reflows() {
    let png = test_epub::make_png(100, 80, [0, 0, 255, 255]);
    let bytes = test_epub::make_epub_with_resources(
        &["<html><body><p>AAAA</p><img src='images/a.png'/><p>WWWW</p></body></html>"],
        vec![("images/a.png", "image/png", png)],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let font_bytes = test_font::make_font();
    let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let mut opts = options();
    let first = renderer.render(&chapter, &opts).unwrap();
    assert!(first.pages >= 2);
    let layout = renderer.layout.as_ref().unwrap();
    let rendered_text: String = layout
        .pages
        .iter()
        .flat_map(|p| &p.items)
        .filter_map(|item| match item {
            Item::Text(line) => Some(&chapter.text()[line.range.clone()]),
            _ => None,
        })
        .collect();
    assert_eq!(rendered_text, "AAAAWWWW");
    assert_eq!(
        layout
            .pages
            .iter()
            .flat_map(|p| &p.items)
            .filter(|item| matches!(item, Item::Image { .. }))
            .count(),
        1
    );
    let mut blue = false;
    for page in 0..first.pages {
        opts.page = Some(page);
        let frame = renderer.render(&chapter, &opts).unwrap();
        blue |= frame
            .surface
            .pixels()
            .iter()
            .any(|p| *p == Color::rgba(0, 0, 255, 255));
        assert!(
            book.locator(0, chapter.restore(&frame.locator).unwrap())
                .is_ok()
        );
    }
    assert!(blue);
    let offset = chapter.text().find("WWWW").unwrap();
    opts.page = None;
    opts.at = Some(chapter.locator(offset).unwrap());
    opts.width = 260;
    opts.height = 240;
    let frame = renderer.render(&chapter, &opts).unwrap();
    assert!(frame.pages < first.pages);
    assert_eq!(
        book.restore(&book.locator(0, offset).unwrap()).unwrap(),
        (0, offset)
    );
}

#[test]
fn inline_image_keeps_both_adjacent_text_ranges() {
    let bytes = test_epub::make_epub_with_resources(
        &["<html><body>AAAA<img src='a.png'/>WWWW</body></html>"],
        vec![(
            "a.png",
            "image/png",
            test_epub::make_png(8, 8, [10, 20, 30, 128]),
        )],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let font_bytes = test_font::make_font();
    let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    renderer.render(&chapter, &options()).unwrap();
    let text: String = renderer
        .layout
        .as_ref()
        .unwrap()
        .pages
        .iter()
        .flat_map(|p| &p.items)
        .filter_map(|item| match item {
            Item::Text(line) => Some(&chapter.text()[line.range.clone()]),
            _ => None,
        })
        .collect();
    assert_eq!(text, "AAAAWWWW");
}

#[test]
fn image_only_and_missing_images_are_readable_and_bounded() {
    let bytes = test_epub::make_epub_with_resources(
        &[
            "<html><body><img src='a.png'/></body></html>",
            "<html><body><img src='https://example.invalid/no.png' alt='unavailable'/></body></html>",
        ],
        vec![(
            "a.png",
            "image/png",
            test_epub::make_png(400, 300, [20, 40, 60, 255]),
        )],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let font_bytes = test_font::make_font();
    let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    for spine in 0..2 {
        let chapter = Chapter::load(&book, spine).unwrap();
        assert!(chapter.is_readable());
        assert!(chapter.text().is_empty());
        let frame = renderer.render(&chapter, &options()).unwrap();
        assert_eq!(frame.pages, 1);
        assert_eq!(chapter.restore(&frame.locator).unwrap(), 0);
        assert!(frame.surface.pixels().iter().any(|p| *p != Color::WHITE));
        if spine == 0 {
            assert!(chapter.images.borrow_mut().get(0).is_some());
        } else {
            assert!(chapter.images.borrow_mut().get(0).is_none());
            assert_eq!(chapter.images.borrow().stats().4, 1);
        }
    }
    for reference in [
        "../../etc/passwd",
        "/etc/passwd",
        "file:///etc/passwd",
        "https://example.invalid/a.png",
        "%2fetc/passwd",
    ] {
        assert!(book.read_spine_resource(0, reference, 1024).is_err());
        assert!(
            book.read_spine_resource_prefix(0, reference, 1024, 33)
                .is_err()
        );
    }
    assert!(book.read_spine_resource(0, "a.png", 1).is_err());
}

#[test]
fn bad_css_and_missing_image_do_not_prevent_valid_text_rendering() {
    let bytes = test_epub::make_epub_with_resources(
        &[
            "<html><head><link rel='stylesheet' href='https://example.invalid/a.css'/><style>p:hover {color:red} @media print {p{color:red}}</style></head><body><p>AAAA</p><img src='missing.png'/><p>WWWW</p></body></html>",
        ],
        vec![],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    assert!(!chapter.content.warnings.is_empty());
    assert_eq!(chapter.images.borrow().stats(), (0, 0, 0, 0, 0));
    let font_bytes = test_font::make_font();
    let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let first = renderer.render(&chapter, &options()).unwrap();
    for page in 0..first.pages {
        let mut opts = options();
        opts.page = Some(page);
        assert!(renderer.render(&chapter, &opts).is_ok());
    }
}

use super::*;
use crate::{test_epub, test_font};
use readall_epub::EpubLimits;
use readall_font::FontLimits;
fn options(width: u32, height: u32) -> Options {
    Options::parse(&[
        "--font".into(),
        "fixture.ttf".into(),
        "--width".into(),
        width.to_string().into(),
        "--height".into(),
        height.to_string().into(),
        "--margin".into(),
        "16".into(),
        "--font-size".into(),
        "16".into(),
    ])
    .unwrap()
}
#[test]
fn nested_boxes_change_width_and_paint_background_and_borders() {
    let bytes = test_epub::make_epub_with_resources(
        &[
            "<html><body><section style='width:160px;margin:0 auto;padding:8px;border:2px solid blue;background:#ffeedd'><p style='padding:4px;border-left:3px solid red;background:#aaffaa'>AAAA WWWW</p></section></body></html>",
        ],
        vec![],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    assert_eq!(chapter.text(), book.read_spine_text(0).unwrap());
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let page = renderer.render(&chapter, &options(320, 256)).unwrap();
    for color in [
        Color::rgba(0, 0, 255, 255),
        Color::rgba(255, 0, 0, 255),
        Color::rgba(255, 238, 221, 255),
        Color::rgba(170, 255, 170, 255),
    ] {
        assert!(page.surface.pixels().contains(&color), "{color:?}");
    }
    assert!(!page.hits.is_empty());
    assert!(
        page.hits
            .iter()
            .all(|hit| hit.rect.x >= 80 && hit.rect.x + hit.rect.width as i32 <= 240)
    );
    assert_eq!(
        renderer.layout.as_ref().unwrap().pages[0].decorations.len(),
        2
    );
}
#[test]
fn multi_page_box_keeps_side_borders_and_all_text() {
    let text = "AAAA WWWW ".repeat(80);
    let body = format!(
        "<html><body><section style='padding:4px;border:2px solid blue;background:#eee'>{text}</section></body></html>"
    );
    let data = test_epub::make_epub_with_resources(&[&body], vec![]);
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let mut opts = options(200, 128);
    let first = renderer.render(&chapter, &opts).unwrap();
    assert!(first.pages > 2);
    let joined: String = renderer
        .layout
        .as_ref()
        .unwrap()
        .pages
        .iter()
        .flat_map(|p| &p.items)
        .filter_map(|item| {
            if let Item::Text(line) = item {
                Some(&chapter.text()[line.range.clone()])
            } else {
                None
            }
        })
        .collect();
    assert_eq!(joined, chapter.text());
    for page in 0..first.pages {
        opts.page = Some(page);
        let frame = renderer.render(&chapter, &opts).unwrap();
        assert!(
            frame
                .surface
                .pixels()
                .contains(&Color::rgba(0, 0, 255, 255))
        );
        assert!(
            frame
                .surface
                .pixels()
                .contains(&Color::rgba(238, 238, 238, 255))
        );
        assert!(
            frame
                .hits
                .iter()
                .all(|hit| hit.rect.x >= 22 && hit.rect.x + hit.rect.width as i32 <= 178)
        );
    }
}
#[test]
fn explicit_page_breaks_do_not_append_blank_pages() {
    let data = test_epub::make_epub_with_resources(
        &[
            "<html><body><p style='break-before:page;break-after:page'>AAAA</p><p style='page-break-before:always;page-break-after:always'>WWWW</p></body></html>",
        ],
        vec![],
    );
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let frame = renderer.render(&chapter, &options(320, 256)).unwrap();
    assert_eq!(frame.pages, 2);
    assert!(
        renderer
            .layout
            .as_ref()
            .unwrap()
            .pages
            .iter()
            .all(|page| !page.items.is_empty())
    );
}
#[test]
fn boxes_preserve_image_order_and_image_page_identity() {
    let data = test_epub::make_epub_with_resources(
        &[
            "<html><body><section style='background:red;padding:4px'><img src='a.png'/></section><section style='background:blue;padding:4px;break-before:page'><img src='a.png'/></section></body></html>",
        ],
        vec![(
            "a.png",
            "image/png",
            test_epub::make_png(100, 80, [20, 40, 60, 255]),
        )],
    );
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    assert!(chapter.text().is_empty());
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let opts = options(200, 128);
    let first = renderer.render(&chapter, &opts).unwrap();
    assert_eq!(first.image_index, Some(0));
    assert!(
        first
            .surface
            .pixels()
            .contains(&Color::rgba(255, 0, 0, 255))
    );
    let second = renderer
        .render_with_image(&chapter, &opts, Some(1))
        .unwrap();
    assert_eq!(second.image_index, Some(1));
    assert!(
        second
            .surface
            .pixels()
            .contains(&Color::rgba(0, 0, 255, 255))
    );
    assert_eq!(first.pages, 2);
}
#[test]
fn author_background_preserves_text_contrast_in_dark_theme() {
    let data = test_epub::make_epub_with_resources(
        &[
            "<html><body><p style='padding:4px;background:white;color:black'>AAAA</p><p>WWWW</p></body></html>",
        ],
        vec![],
    );
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let frame = EpubRenderer::new(&font, 16, false)
        .unwrap()
        .with_preferences(Theme::Dark, 1.0)
        .render(&chapter, &options(320, 256))
        .unwrap();
    assert!(frame.surface.pixels().contains(&Color::rgba(0, 0, 0, 255)));
    assert!(frame.surface.pixels().contains(&Color::WHITE));
    assert!(frame.surface.pixels().contains(&Theme::Dark.colors().1));
}
#[test]
fn self_closing_hidden_elements_do_not_suppress_following_content() {
    let data = test_epub::make_epub_with_resources(
        &["<html><body><script/><p>A</p><style/><p>W</p></body></html>"],
        vec![],
    );
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    assert_eq!(book.read_spine_text(0).unwrap(), "A\n\nW");
}

use super::*;
use crate::{
    epub_session::{Action, EpubSession, Start},
    test_epub, test_font,
};
use readall_epub::EpubLimits;
use readall_font::FontLimits;
fn options() -> Options {
    Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "320",
            "--height",
            "240",
            "--margin",
            "20",
            "--font-size",
            "16",
        ]
        .map(Into::into),
    )
    .unwrap()
}
#[test]
fn inline_svg_preserves_canonical_text_but_renders_as_a_single_image() {
    let xhtml = r#"<html><body><p>AAAA</p><svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"><rect width="80" height="60" fill="blue"/><text x="2" y="20">WWWW</text></svg><p>AAAA</p></body></html>"#;
    let data = test_epub::make_epub_with_resources(&[xhtml], vec![]);
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let content = book.read_spine_content(0).unwrap();
    assert_eq!(content.text, book.read_spine_text(0).unwrap());
    assert_eq!(content.images.len(), 1);
    assert!(
        content.images[0]
            .inline_svg
            .as_ref()
            .unwrap()
            .contains("<rect")
    );
    let offset = content.text.find("WWWW").unwrap();
    assert!(
        content
            .runs
            .iter()
            .any(|run| run.range.contains(&offset) && run.style.hidden)
    );
    let chapter = Chapter::load(&book, 0).unwrap();
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let frame = renderer.render(&chapter, &options()).unwrap();
    assert!(
        frame
            .surface
            .pixels()
            .contains(&Color::rgba(0, 0, 255, 255))
    );
    assert_eq!(frame.image_hits.len(), 1);
    assert_eq!(chapter.images.borrow().stats().4, 0);
}
#[test]
fn referenced_svg_reads_images_relative_to_svg_not_chapter() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"><image href="../media/a.png" width="80" height="60"/></svg>"#;
    let png = test_epub::make_png(8, 6, [20, 80, 160, 255]);
    let data = test_epub::make_epub_with_resources(
        &["<html><body><img src='illustrations/a.svg'/></body></html>"],
        vec![
            (
                "illustrations/a.svg",
                "image/svg+xml",
                svg.as_bytes().to_vec(),
            ),
            ("media/a.png", "image/png", png),
        ],
    );
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let frame = renderer.render(&chapter, &options()).unwrap();
    assert!(
        frame
            .surface
            .pixels()
            .contains(&Color::rgba(20, 80, 160, 255))
    );
    assert_eq!(chapter.images.borrow().stats().4, 0);
    assert!(
        book.read_spine_nested_resource(0, "illustrations/a.svg", "../../etc/passwd", 4096)
            .is_err()
    );
}
#[test]
fn standalone_svg_spine_can_be_read_bookmarked_and_restored() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="120"><rect width="160" height="120" fill="red"/></svg>"#;
    let data = test_epub::make_epub_with_svg_spine(svg, vec![]);
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    assert_eq!(book.read_spine_text(0).unwrap(), "");
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let mut session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
    assert!(
        session
            .frame()
            .surface
            .pixels()
            .contains(&Color::rgba(255, 0, 0, 255))
    );
    let locator = session.anchor().clone();
    assert_eq!(locator.image_index(), Some(0));
    assert_eq!(book.restore(&locator).unwrap(), (0, 0));
    assert!(!session.action(Action::Next).unwrap());
    assert_eq!(session.toc_entries().unwrap().len(), 1);
    let resumed = EpubSession::new(&book, &font, options(), Start::Locator(locator)).unwrap();
    assert_eq!(resumed.frame().image_index, Some(0));
}

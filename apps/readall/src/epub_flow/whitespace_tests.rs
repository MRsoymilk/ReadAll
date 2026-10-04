use super::*;
use crate::{test_epub, test_font};
use readall_epub::EpubLimits;
use readall_font::FontLimits;
fn options(width: u32) -> Options {
    Options::parse(&[
        "--font".into(),
        "fixture.ttf".into(),
        "--width".into(),
        width.to_string().into(),
        "--height".into(),
        "240".into(),
        "--margin".into(),
        "16".into(),
        "--font-size".into(),
        "16".into(),
    ])
    .unwrap()
}
#[test]
fn code_tabs_and_spaces_are_positioned_and_blank_lines_use_full_line_height() {
    let code = "A\n\tW\n\n    A";
    let source = format!("<html><body><pre><code>{code}</code></pre></body></html>");
    let data = test_epub::make_epub_with_resources(&[&source], vec![]);
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    assert_eq!(chapter.text(), code);
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let frame = renderer.render(&chapter, &options(300)).unwrap();
    assert_eq!(frame.pages, 1);
    let lines: Vec<_> = renderer.layout.as_ref().unwrap().pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Text(line) => Some(line),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(&chapter.text()[lines[1].range.clone()], "\tW");
    assert!(lines[1].glyphs[0].glyph.is_none());
    assert!(lines[1].glyphs[0].advance > 0.0);
    assert!(lines[1].glyphs[1].x >= lines[1].glyphs[0].advance);
    assert!(lines[2].glyphs.is_empty());
    let step = lines[1].baseline - lines[0].baseline;
    assert!((lines[3].baseline - lines[1].baseline - 2.0 * step).abs() < 0.01);
    let first_ink = lines[3]
        .glyphs
        .iter()
        .find(|g| &chapter.text()[g.source.clone()] == "A")
        .unwrap();
    assert!(first_ink.x > 0.0);
}
#[test]
fn overlong_code_soft_wraps_without_losing_indentation_or_copy_source() {
    let code = format!("    {}", "AAAA WWWW ".repeat(40));
    let source = format!("<html><body><pre style='white-space:pre'>{code}</pre></body></html>");
    let data = test_epub::make_epub_with_resources(&[&source], vec![]);
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let frame = renderer.render(&chapter, &options(160)).unwrap();
    assert!(frame.pages > 1);
    let copied: String = renderer
        .layout
        .as_ref()
        .unwrap()
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            Item::Text(line) => Some(&chapter.text()[line.range.clone()]),
            _ => None,
        })
        .collect();
    assert_eq!(copied, code);
    assert_eq!(book.read_spine_text(0).unwrap(), code);
    for page in 0..frame.pages {
        let mut opts = options(160);
        opts.page = Some(page);
        let rendered = renderer.render(&chapter, &opts).unwrap();
        let offset = chapter.restore(&rendered.locator).unwrap();
        let locator = book.locator(0, offset).unwrap();
        assert!(locator.to_string().starts_with("epub-v3:"));
        assert_eq!(book.restore(&locator).unwrap(), (0, offset));
    }
}
#[test]
fn old_code_position_is_upgraded_before_selection_settings_or_link_history_use_it() {
    use crate::epub_session::{EpubSession, Start};
    let data = test_epub::make_epub_with_resources(
        &["<html><body><pre>A\n\tW\n\n    A</pre></body></html>"],
        vec![],
    );
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let old: readall_epub::EpubLocator = format!("epub-v1:{}:0:2", book.id()).parse().unwrap();
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let mut session =
        EpubSession::new(&book, &font, options(300), Start::Locator(old.clone())).unwrap();
    assert_eq!(session.anchor().utf8_offset(), 3);
    assert!(session.anchor().to_string().starts_with("epub-v3:"));
    assert_eq!(session.text_at(0..11), "A\n\tW\n\n    A");
    let normalized = session.anchor().clone();
    session.resize(260, 300).unwrap();
    assert_eq!(session.anchor(), &normalized);
    session.jump_to_locator(old).unwrap();
    assert_eq!(session.anchor(), &normalized);
}

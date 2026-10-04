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
fn gsub_ligatures_are_drawn_by_glyph_id_and_keep_multi_character_source_ranges() {
    let bytes = test_font::make_layout_font(*b"latn", *b"liga", true, &[]);
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let data = test_epub::make_epub_with_resources(&["<html><body>AAAA A</body></html>"], vec![]);
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let frame = renderer.render(&chapter, &options()).unwrap();
    let line = renderer.layout.as_ref().unwrap().pages[0]
        .items
        .iter()
        .find_map(|item| {
            if let Item::Text(line) = item {
                Some(line)
            } else {
                None
            }
        })
        .unwrap();
    let ids: Vec<_> = line.glyphs.iter().filter_map(|glyph| glyph.glyph).collect();
    assert_eq!(ids, vec![2, 2, 3, 1]);
    assert_eq!(line.glyphs[0].source, 0..2);
    assert_eq!(line.glyphs[1].source, 2..4);
    assert!(frame.hits.iter().any(|hit| hit.start == 0 && hit.end == 2));
    assert!(
        frame
            .surface
            .pixels()
            .iter()
            .any(|pixel| *pixel != Color::WHITE)
    );
}
#[test]
fn rtl_visual_order_and_fallback_do_not_change_logical_offsets() {
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let fallback_bytes = test_font::make_layout_font(
        *b"latn",
        *b"liga",
        true,
        &[(0x5d0, 1), (0x5d1, 2), (0x5d2, 1)],
    );
    let fallback = Font::parse(&fallback_bytes, 0, FontLimits::default()).unwrap();
    let data = test_epub::make_epub_with_resources(&["<html><body>A אבג A</body></html>"], vec![]);
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false)
        .unwrap()
        .with_fallbacks(&[&fallback]);
    let frame = renderer.render(&chapter, &options()).unwrap();
    assert!(frame.missing.is_empty());
    let glyphs: Vec<_> = renderer.layout.as_ref().unwrap().pages[0]
        .items
        .iter()
        .flat_map(|item| {
            if let Item::Text(line) = item {
                line.glyphs.iter().collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        })
        .collect();
    let hebrew: Vec<_> = glyphs
        .iter()
        .filter(|glyph| glyph.face == 1)
        .map(|glyph| glyph.source.start)
        .collect();
    assert_eq!(hebrew, vec![6, 4, 2]);
    assert_eq!(chapter.text(), "A אבג A");
    for hit in &frame.hits {
        assert!(book.locator(0, hit.start).is_ok());
        assert!(book.locator(0, hit.end).is_ok());
    }
}
#[test]
fn arabic_joining_changes_forms_and_reshapes_at_line_boundaries() {
    let bytes = test_font::make_layout_font(*b"arab", *b"init", false, &[(0x628, 1)]);
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let data = test_epub::make_epub_with_resources(&["<html><body>بب</body></html>"], vec![]);
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let masked = bidi_text(&chapter, 0..4);
    let paragraph = Paragraph::new(&chapter, 0..4, &masked);
    let mut fonts = Fonts::new(&font, 16, false);
    let mut work = 0;
    let joined = paragraph
        .line(&chapter, 0..4, &mut fonts, &mut work)
        .unwrap();
    let ids: Vec<_> = joined
        .glyphs
        .iter()
        .filter_map(|glyph| glyph.glyph)
        .collect();
    assert_eq!(ids, vec![1, 2]);
    let isolated = paragraph
        .line(&chapter, 0..2, &mut fonts, &mut work)
        .unwrap();
    assert_eq!(isolated.glyphs[0].glyph, Some(1));
}
#[test]
fn combining_sequences_and_hidden_rtl_text_keep_stable_cluster_boundaries() {
    let bytes = test_font::make_layout_font(*b"latn", *b"liga", true, &[(0x301, 3)]);
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let data = test_epub::make_epub_with_resources(
        &["<html><body><span style='display:none'>אבג</span>Á W</body></html>"],
        vec![],
    );
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let frame = renderer.render(&chapter, &options()).unwrap();
    assert!(frame.missing.is_empty());
    let start = chapter.text().find('A').unwrap();
    let accents: Vec<_> = frame.hits.iter().filter(|hit| hit.start == start).collect();
    assert!(!accents.is_empty());
    assert!(accents.iter().all(|hit| hit.end == start + 3));
    assert!(frame.hits.iter().all(|hit| hit.start >= start));
}

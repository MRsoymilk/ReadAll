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
            "400",
            "--height",
            "300",
            "--margin",
            "24",
            "--font-size",
            "24",
        ]
        .map(Into::into),
    )
    .unwrap()
}
fn font_width(width: u16) -> Vec<u8> {
    let mut bytes = test_font::make_font();
    let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    for table in 0..count {
        let at = 12 + table * 16;
        if &bytes[at..at + 4] == b"hmtx" {
            let start = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            bytes[start + 4..start + 6].copy_from_slice(&width.to_be_bytes());
        }
    }
    bytes
}
fn text_glyphs<'a>(renderer: &'a EpubRenderer<'_, '_>) -> Vec<&'a PositionedGlyph> {
    renderer
        .layout
        .as_ref()
        .unwrap()
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            Item::Text(line) => Some(&line.glyphs),
            _ => None,
        })
        .flatten()
        .collect()
}
#[test]
fn author_font_changes_real_advances_and_reader_default_stays_separate() {
    let xml = "<html><head><link rel='stylesheet' href='styles/main.css'/></head><body><p>AAAA</p><p style='font-family:serif'>AAAA</p></body></html>";
    let css=b"@font-face {font-family:'Author Serif';src:url('../Fonts/Book.ttf') format('truetype')} p {font-family:'Author Serif'}";
    let bytes = test_epub::make_epub_with_resources(
        &[xml],
        vec![
            ("styles/main.css", "text/css", css.to_vec()),
            ("Fonts/Book.ttf", "font/ttf", font_width(1000)),
        ],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    assert_eq!(chapter.embedded.faces.len(), 1);
    assert_eq!(chapter.embedded.attempts, 1);
    assert_eq!(chapter.text(), book.read_spine_text(0).unwrap());
    let fb = test_font::make_font();
    let font = Font::parse(&fb, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 24, false).unwrap();
    let frame = renderer.render(&chapter, &options()).unwrap();
    let glyphs = text_glyphs(&renderer);
    assert!(
        glyphs[..4]
            .iter()
            .all(|g| g.face == 1 && (g.advance - 24.0).abs() < 0.01)
    );
    assert!(
        glyphs[4..]
            .iter()
            .all(|g| g.face == 0 && (g.advance - 14.4).abs() < 0.01)
    );
    assert!(
        renderer.fonts.caches.contains_key(&(1, 24))
            && renderer.fonts.caches.contains_key(&(0, 24))
    );
    let again = renderer.render(&chapter, &options()).unwrap();
    assert_eq!(frame.surface.pixels(), again.surface.pixels());
    assert_eq!(chapter.embedded.attempts, 1);
}
#[test]
fn descriptors_choose_bold_face_without_inventing_double_bold() {
    let xml = "<html><head><style>@font-face{font-family:X;src:url(regular.ttf)} @font-face{font-family:X;src:url(strong.ttf);font-weight:700;font-style:italic} body{font-family:X}</style></head><body><p>A<strong><em>A</em></strong></p></body></html>";
    let bytes = test_epub::make_epub_with_resources(
        &[xml],
        vec![
            ("regular.ttf", "font/ttf", font_width(800)),
            ("strong.ttf", "font/ttf", font_width(1100)),
        ],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    let fb = test_font::make_font();
    let font = Font::parse(&fb, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 24, false).unwrap();
    renderer.render(&chapter, &options()).unwrap();
    let glyphs = text_glyphs(&renderer);
    assert_eq!(glyphs.len(), 2);
    assert_eq!(glyphs[0].face, 1);
    assert_eq!(glyphs[1].face, 2);
    assert_eq!(glyphs[1].bold, 0);
    assert!(!glyphs[1].italic);
    assert!((glyphs[1].advance - 26.4).abs() < 0.01);
}
#[test]
fn fallback_sources_unused_faces_and_unicode_ranges_are_respected() {
    let xml = "<html><head><style>@font-face{font-family:Unused;src:url(missing.ttf)} @font-face{font-family:X;src:url(broken.ttf),url(good.ttf);unicode-range:U+41} p{font-family:Absent, X, serif}</style></head><body><p>AW</p></body></html>";
    let bytes = test_epub::make_epub_with_resources(
        &[xml],
        vec![
            ("broken.ttf", "font/ttf", b"not a font".to_vec()),
            ("good.ttf", "application/x-font-ttf", font_width(1000)),
        ],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    assert_eq!(chapter.embedded.attempts, 2);
    assert_eq!(chapter.embedded.faces.len(), 1);
    assert_eq!(chapter.content.warnings.len(), 1);
    assert!(chapter.content.warnings[0].contains("broken.ttf"));
    let fb = test_font::make_font();
    let font = Font::parse(&fb, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 24, false).unwrap();
    renderer.render(&chapter, &options()).unwrap();
    let glyphs = text_glyphs(&renderer);
    assert_eq!(glyphs[0].face, 1);
    assert_eq!(glyphs[1].face, 0);
    for path in [
        "../good.ttf",
        "/etc/passwd",
        "https://example.invalid/font.ttf",
        "OEBPS/chapter0.xhtml",
    ] {
        assert!(book.read_font_resource(path, 1024).is_err());
    }
    assert!(book.read_font_resource("OEBPS/good.ttf", 1).is_err());
}
#[test]
fn same_family_in_different_chapters_never_reuses_wrong_glyph_cache() {
    let a = "<html><head><style>@font-face{font-family:X;src:url(a.ttf)}p{font-family:X}</style></head><body><p>AA</p></body></html>";
    let b = "<html><head><style>@font-face{font-family:X;src:url(b.ttf)}p{font-family:X}</style></head><body><p>AA</p></body></html>";
    let bytes = test_epub::make_epub_with_resources(
        &[a, b],
        vec![
            ("a.ttf", "font/ttf", font_width(700)),
            ("b.ttf", "font/ttf", font_width(1300)),
        ],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let c0 = Chapter::load(&book, 0).unwrap();
    let c1 = Chapter::load(&book, 1).unwrap();
    let fb = test_font::make_font();
    let font = Font::parse(&fb, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 24, false).unwrap();
    let f0 = renderer.render(&c0, &options()).unwrap();
    assert!((text_glyphs(&renderer)[0].advance - 16.8).abs() < 0.01);
    renderer.render(&c1, &options()).unwrap();
    assert!((text_glyphs(&renderer)[0].advance - 31.2).abs() < 0.01);
    let back = renderer.render(&c0, &options()).unwrap();
    assert_eq!(f0.surface.pixels(), back.surface.pixels());
    let anchor = book.locator(0, 1).unwrap();
    let mut session = crate::epub_session::EpubSession::new(
        &book,
        &font,
        options(),
        crate::epub_session::Start::Locator(anchor.clone()),
    )
    .unwrap();
    session.resize(300, 240).unwrap();
    session.action(crate::epub_session::Action::Larger).unwrap();
    assert_eq!(session.anchor(), &anchor);
    session.action(crate::epub_session::Action::Next).unwrap();
    assert_eq!(session.current_spine(), 1);
    session
        .action(crate::epub_session::Action::Previous)
        .unwrap();
    assert_eq!(session.current_spine(), 0);
}
#[test]
fn multiple_aliases_share_the_font_allocation_and_failed_sources_do_not_reload() {
    let xml = "<html><head><style>@font-face{font-family:X;src:url(bad.ttf),url(font.ttf)}@font-face{font-family:Y;src:url(bad.ttf),url(font.ttf)}</style></head><body><p style='font-family:X'>A</p><p style='font-family:Y'>W</p></body></html>";
    let font_data = font_width(900);
    let len = font_data.len();
    let bytes = test_epub::make_epub_with_resources(
        &[xml],
        vec![
            ("font.ttf", "font/ttf", font_data),
            ("bad.ttf", "font/ttf", b"broken".to_vec()),
        ],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    assert_eq!(chapter.embedded.attempts, 2);
    assert_eq!(chapter.embedded.loaded_bytes, len + 6);
    assert_eq!(chapter.embedded.faces.len(), 2);
    assert!(std::ptr::eq(
        chapter.embedded.faces[0].font.data(),
        chapter.embedded.faces[1].font.data()
    ));
    assert_eq!(chapter.content.warnings.len(), 1);
}
#[test]
fn untrusted_font_file_count_is_bounded_but_valid_text_still_renders() {
    let names: Vec<_> = (0..17).map(|i| format!("f{i}.ttf")).collect();
    let css = names
        .iter()
        .enumerate()
        .map(|(i, name)| format!("@font-face{{font-family:F{i};src:url({name})}}"))
        .collect::<String>();
    let body = (0..17)
        .map(|i| format!("<span style='font-family:F{i}'>A</span>"))
        .collect::<String>();
    let xml = format!("<html><head><style>{css}</style></head><body>{body}</body></html>");
    let bytes = test_epub::make_epub_with_resources(
        &[&xml],
        names
            .iter()
            .map(|name| (name.as_str(), "font/ttf", font_width(600)))
            .collect(),
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&book, 0).unwrap();
    assert_eq!(chapter.embedded.faces.len(), 16);
    assert_eq!(chapter.embedded.attempts, 16);
    assert_eq!(chapter.content.warnings.len(), 1);
    let fb = test_font::make_font();
    let font = Font::parse(&fb, 0, FontLimits::default()).unwrap();
    let mut renderer = EpubRenderer::new(&font, 24, false).unwrap();
    renderer.render(&chapter, &options()).unwrap();
    let glyphs = text_glyphs(&renderer);
    assert_eq!(glyphs.len(), 17);
    assert_eq!(glyphs.last().unwrap().face, 0);
}

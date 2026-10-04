use super::*;
use crate::{
    reader_data::{Settings, Theme},
    test_epub, test_font,
};
fn options() -> Options {
    Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "640",
            "--height",
            "480",
            "--margin",
            "40",
        ]
        .map(Into::into),
    )
    .unwrap()
}
#[test]
fn first_reader_frame_does_not_scan_unopened_chapters_for_toc() {
    let bytes = test_epub::make_epub_with_resources(
        &[
            "<html><body><p>AAAA WWWW</p></body></html>",
            "<html><body><p>broken",
        ],
        vec![],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let font_bytes = test_font::make_font();
    let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
    let session = EpubSession::new_with_preferences(
        &book,
        &font,
        options(),
        Start::Beginning,
        &[],
        Settings::default(),
    )
    .unwrap();
    let ui = UiFont::from_bytes(font_bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
    let mut reader = ReaderWindow::new_lazy(session, None, ui).unwrap();
    assert!(!reader.toc_loaded);
    assert!(reader.toc.is_empty());
    assert_eq!(reader.session.current_spine(), 0);
    let before = reader.session.anchor().clone();
    // An unreadable later chapter cannot prevent this first frame from existing.
    assert!(reader.ensure_toc().is_err());
    assert_eq!(reader.session.anchor(), &before);
}
#[test]
fn initial_render_uses_saved_preferences_without_second_layout() {
    let bytes = test_epub::make_epub();
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let font_bytes = test_font::make_font();
    let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
    let mut options = options();
    options.size = 28;
    options.margin = 44;
    let settings = Settings {
        theme: Theme::Dark,
        size: 28,
        margin: 44,
        line_spacing: 1.4,
    };
    let session =
        EpubSession::new_with_preferences(&book, &font, options, Start::Beginning, &[], settings)
            .unwrap();
    assert_eq!(
        session.frame().surface.pixel(0, 0),
        Some(Theme::Dark.colors().0)
    );
    assert_eq!(session.settings(), settings);
}
#[test]
fn loading_benchmark_keeps_unopened_chapters_off_first_frame_path() {
    let body = format!(
        "<html><body><p>{}</p></body></html>",
        "AAAA WWWW ".repeat(120)
    );
    let chapters: Vec<_> = (0..120).map(|_| body.as_str()).collect();
    let bytes = test_epub::make_epub_with_resources(&chapters, vec![]);
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let font_bytes = test_font::make_font();
    let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
    let session = EpubSession::new_with_preferences(
        &book,
        &font,
        options(),
        Start::Beginning,
        &[],
        Settings::default(),
    )
    .unwrap();
    let ui = UiFont::from_bytes(font_bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
    let start = std::time::Instant::now();
    let mut reader = ReaderWindow::new_lazy(session, None, ui).unwrap();
    let lazy = start.elapsed();
    assert!(!reader.toc_loaded);
    let start = std::time::Instant::now();
    reader.ensure_toc().unwrap();
    let toc = start.elapsed();
    assert_eq!(reader.toc.len(), 120);
    println!(
        "LOADING_BENCH: 120 chapters; first UI assembly {:?}; deferred TOC {:?}",
        lazy, toc
    );
    let first = reader.toc.len();
    reader.ensure_toc().unwrap();
    assert_eq!(reader.toc.len(), first);
}

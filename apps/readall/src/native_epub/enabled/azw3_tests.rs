//! The reconstructed KF8 book uses the real reader session, including gestures.
use super::*;
use crate::{
    publication,
    reader_data::{PageMode, Settings},
    test_azw3, test_font,
};
#[test]
fn azw3_uses_page_modes_selection_links_return_and_reflow() {
    let source = test_azw3::build(
        &[
            test_azw3::Chapter {
                head: "<html><body>",
                fragments: vec![
                    "<p>AAAA WWWW</p><a href=\"kindle:pos:fid:0001:off:0000000000\">WWWW</a>",
                ],
                tail: "</body></html>",
            },
            test_azw3::Chapter {
                head: "<html><body>",
                fragments: vec!["<h1>AAAA</h1><p>WWWW AAAA</p>"],
                tail: "</body></html>",
            },
        ],
        Default::default(),
    );
    let prepared = publication::prepare(source, Path::new("book.azw3")).unwrap();
    assert_eq!(prepared.format, publication::Format::Azw3);
    let book = EpubBook::parse(&prepared.bytes, EpubLimits::default()).unwrap();
    let font_bytes = test_font::make_font();
    let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
    for mode in [PageMode::Slide, PageMode::Book, PageMode::Scroll] {
        let options = Options::parse(
            &[
                "--font",
                "fixture.ttf",
                "--width",
                "640",
                "--height",
                "480",
                "--margin",
                "40",
                "--font-size",
                "16",
            ]
            .map(Into::into),
        )
        .unwrap();
        let session = EpubSession::new_with_preferences(
            &book,
            &font,
            options,
            Start::Beginning,
            &[],
            Settings {
                page_mode: mode,
                ..Default::default()
            },
        )
        .unwrap();
        let ui = UiFont::from_bytes(font_bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
        let mut reader = ReaderWindow::new_lazy(session, None, ui).unwrap();
        reader.toolbar = ToolbarMode::Collapsed;
        reader.prefetch_page().unwrap();
        let hit = reader.session.frame().hits.first().unwrap().clone();
        let (x, y) = (hit.rect.x + 1, hit.rect.y + 1);
        reader.action(Action::Click { x, y }).unwrap();
        reader.action(Action::PointerRelease { x, y }).unwrap();
        assert!(reader.tools.selection.is_some());
        assert_eq!(reader.session.current_spine(), 0);
        reader.action(Action::Close).unwrap();
        let origin = reader.session.anchor().clone();
        let rect = reader.session.link_regions().next().unwrap().0;
        let (x, y) = (rect.x + 1, rect.y + 1);
        reader.action(Action::Click { x, y }).unwrap();
        reader.action(Action::PointerRelease { x, y }).unwrap();
        assert_eq!(reader.session.current_spine(), 1);
        reader.action(Action::Back).unwrap();
        assert_eq!(reader.session.anchor(), &origin);
        assert_eq!(reader.session.settings().page_mode, mode);
        reader.resize(720, 520).unwrap();
        assert_eq!(reader.session.anchor(), &origin);
        assert!(!reader.close_requested());
    }
}

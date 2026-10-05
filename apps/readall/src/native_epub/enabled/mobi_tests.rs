//! Exercise a MOBI adapter through the actual reader state, not a mock converter.
use super::*;
use crate::{
    publication,
    reader_data::{PageMode, Settings},
    test_font, test_mobi,
};
#[test]
fn mobi_uses_all_three_modes_text_selection_and_book_link_return() {
    let mut html = "<html><body><p>AAAA WWWW</p><a filepos=0000000000>WWWW</a><mbp:pagebreak/><h1>AAAA</h1><p>WWWW AAAA</p></body></html>".to_owned();
    let target = html.find("<h1>").unwrap();
    html = html.replace("0000000000", &format!("{target:010}"));
    let source = test_mobi::make_mobi(&html);
    let prepared = publication::prepare(source, Path::new("book.mobi")).unwrap();
    let book = EpubBook::parse(&prepared.bytes, EpubLimits::default()).unwrap();
    let font_bytes = test_font::make_font();
    let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
    for mode in [PageMode::Slide, PageMode::Book, PageMode::Scroll] {
        let opts = Options::parse(
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
            opts,
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
        assert!(
            reader.tools.selection.is_some(),
            "MOBI text must remain selectable"
        );
        assert_eq!(reader.session.current_spine(), 0);
        reader.action(Action::Close).unwrap(); // clear selection, do not close book
        let origin = reader.session.anchor().clone();
        let rect = reader.session.link_regions().next().unwrap().0;
        let (x, y) = (rect.x + 1, rect.y + 1);
        reader.action(Action::Click { x, y }).unwrap();
        reader.action(Action::PointerRelease { x, y }).unwrap();
        assert_eq!(reader.session.current_spine(), 1);
        reader.action(Action::Back).unwrap();
        assert_eq!(reader.session.current_spine(), 0);
        assert_eq!(reader.session.anchor(), &origin);
        assert_eq!(reader.session.settings().page_mode, mode);
        assert!(!reader.close_requested());
    }
}

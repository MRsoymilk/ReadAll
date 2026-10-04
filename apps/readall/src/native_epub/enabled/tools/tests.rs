use super::*;
use crate::{reader_data::Theme, test_epub, test_font};
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn temp() -> Temp {
    Temp(std::env::temp_dir().join(format!(
        "readall-native-tools-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )))
}
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
fn native_search_selection_highlight_note_bookmark_and_settings_work_together() {
    let dir = temp();
    let data = test_epub::make_epub();
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
    let ui = UiFont::from_bytes(bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
    let mut reader = ReaderWindow::new(session, None, ui).unwrap();
    reader
        .attach_tools(Store::new(dir.0.clone()), true, true)
        .unwrap();
    reader.action(Action::Command(ReaderCommand::Find)).unwrap();
    assert!(reader.text_input_active());
    for ch in "WWWW".chars() {
        reader.action(Action::Text(ch)).unwrap();
    }
    reader.action(Action::Activate).unwrap();
    assert!(!reader.tools.hits.is_empty());
    let target = reader.tools.hits[0].locator.clone();
    reader.action(Action::Activate).unwrap();
    assert_eq!(reader.session.anchor(), &target);
    assert!(reader.tools.selection.is_some());
    reader
        .action(Action::Command(ReaderCommand::Bookmark))
        .unwrap();
    reader
        .action(Action::Command(ReaderCommand::Highlight))
        .unwrap();
    reader.action(Action::Command(ReaderCommand::Note)).unwrap();
    for ch in "测试笔记".chars() {
        reader.action(Action::Text(ch)).unwrap();
    }
    reader.action(Action::Activate).unwrap();
    let rows = Store::new(dir.0.clone()).annotations(&book).unwrap();
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .any(|row| row.kind == Kind::Note && row.text == "测试笔记")
    );
    reader
        .action(Action::Command(ReaderCommand::Bookmarks))
        .unwrap();
    reader
        .action(Action::Command(ReaderCommand::Delete))
        .unwrap();
    assert_eq!(
        Store::new(dir.0.clone()).annotations(&book).unwrap().len(),
        2
    );
    assert!(reader.action(Action::Close).unwrap());
    assert_eq!(reader.tools.mode, Mode::None);
    assert!(!reader.close_requested());
    reader
        .action(Action::Command(ReaderCommand::Theme))
        .unwrap();
    reader
        .action(Action::Command(ReaderCommand::Theme))
        .unwrap();
    assert_eq!(reader.session.settings().theme, Theme::Dark);
    assert_eq!(
        Store::new(dir.0.clone()).settings().unwrap().theme,
        Theme::Dark
    );
    reader
        .action(Action::Command(ReaderCommand::Select))
        .unwrap();
    let first = reader.session.frame().hits[0].clone();
    let last = reader.session.frame().hits[2].clone();
    reader
        .action(Action::Click {
            x: first.rect.x + 1,
            y: first.rect.y + 1,
        })
        .unwrap();
    reader
        .action(Action::PointerMove {
            x: last.rect.x + 1,
            y: last.rect.y + 1,
        })
        .unwrap();
    reader
        .action(Action::PointerRelease {
            x: last.rect.x + 1,
            y: last.rect.y + 1,
        })
        .unwrap();
    assert!(reader.tools.selection.is_some());
    assert!(reader.tools.drag.is_none());
    reader.action(Action::Next).unwrap();
    assert!(reader.tools.selection.is_none());
}
#[test]
fn image_overlay_zooms_and_escape_returns_without_closing_reader() {
    let data = test_epub::make_epub_with_resources(
        &["<html><body><img src='a.png'/></body></html>"],
        vec![(
            "a.png",
            "image/png",
            test_epub::make_png(128, 96, [20, 80, 160, 255]),
        )],
    );
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
    let ui = UiFont::from_bytes(bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
    let mut reader = ReaderWindow::new(session, None, ui).unwrap();
    let rect = reader.session.frame().image_hits[0].0;
    reader
        .action(Action::Click {
            x: rect.x + 5,
            y: rect.y + 5,
        })
        .unwrap();
    assert_eq!(
        reader.tools.mode,
        Mode::None,
        "press must not open the image"
    );
    reader
        .action(Action::PointerRelease {
            x: rect.x + 5,
            y: rect.y + 5,
        })
        .unwrap();
    assert_eq!(reader.tools.mode, Mode::Zoom);
    reader.action(Action::Larger).unwrap();
    assert!(reader.tools.factor > 1.0);
    assert!(reader.action(Action::Close).unwrap());
    assert_eq!(reader.tools.mode, Mode::None);
    assert!(!reader.close_requested());
}
#[test]
fn clipboard_completion_only_inserts_into_an_open_editor() {
    let data = test_epub::make_epub();
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
    let ui = UiFont::from_bytes(bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
    let mut reader = ReaderWindow::new(session, None, ui).unwrap();
    reader.tool_command(ReaderCommand::Find).unwrap();
    let (tx, rx) = mpsc::channel();
    reader.tools.clipboard = Some(rx);
    tx.send(ClipboardResult::Pasted("中文查询".into())).unwrap();
    assert!(reader.poll_clipboard());
    assert_eq!(reader.tools.query, "中文查询");
    let (tx, rx) = mpsc::channel();
    reader.tools.clipboard = Some(rx);
    reader.tools.mode = Mode::None;
    tx.send(ClipboardResult::Pasted("not inserted".into()))
        .unwrap();
    reader.poll_clipboard();
    assert_eq!(reader.tools.query, "中文查询");
}

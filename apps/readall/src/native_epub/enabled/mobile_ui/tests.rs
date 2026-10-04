use super::*;
use crate::{test_epub, test_font, text_page::Options};
use readall_epub::{EpubBook, EpubLimits};
use readall_font::{Font, FontLimits};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "readall-parity-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn store(&self) -> Store {
        Store::new(self.0.clone())
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn options() -> Options {
    Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "400",
            "--height",
            "640",
            "--font-size",
            "16",
            "--margin",
            "24",
        ]
        .map(Into::into),
    )
    .unwrap()
}
fn center(rect: Rect) -> (i32, i32) {
    (
        rect.x + rect.width as i32 / 2,
        rect.y + rect.height as i32 / 2,
    )
}
fn tap(ui: &mut Presentation<'_, '_, '_, '_>, rect: Rect) {
    let (x, y) = center(rect);
    ui.touch(0, x, y).unwrap();
    ui.touch(1, x, y).unwrap();
}

#[test]
fn android_and_linux_chrome_toc_and_settings_are_pixel_identical() {
    let bytes = test_epub::make_epub();
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let data = test_font::make_font();
    let font = Font::parse(&data, 0, FontLimits::default()).unwrap();
    let session = || EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
    let ui_font = || UiFont::from_bytes(data.clone(), "fixture.ttf".into()).unwrap();
    let temp = Temp::new();
    let mut linux = ReaderWindow::new_lazy(session(), None, ui_font()).unwrap();
    linux.tools.store = Some(temp.store());
    let mut mobile = Presentation::new(session(), None, ui_font(), temp.store()).unwrap();
    assert_eq!(linux.surface().pixels(), mobile.surface().pixels());
    for index in [5, 1, 1] {
        let rect = if linux.toolbar == ToolbarMode::Collapsed {
            linux.collapsed_rect()
        } else {
            linux.toolbar_button_rect(index)
        };
        let (x, y) = center(rect);
        for action in [
            Action::PointerMove { x, y },
            Action::Click { x, y },
            Action::PointerRelease { x, y },
            Action::PointerLeave,
        ] {
            linux.action(action).unwrap();
            mobile.action(action).unwrap();
        }
        assert_eq!(linux.surface().pixels(), mobile.surface().pixels());
    }
    for action in [
        Action::Command(ReaderCommand::Settings),
        Action::Next,
        Action::Next,
        Action::Next,
        Action::Next,
        Action::Larger,
        Action::Larger,
        Action::Larger,
    ] {
        linux.action(action).unwrap();
        mobile.action(action).unwrap();
        assert_eq!(linux.surface().pixels(), mobile.surface().pixels());
        assert_eq!(linux.session.settings(), mobile.session().settings());
    }
}
#[test]
fn touch_toolbar_expands_collapses_and_toc_drag_never_turns_book_pages() {
    let chapters: Vec<_> = (0..20)
        .map(|_| "<html><body><p>AAAA WWWW</p></body></html>")
        .collect();
    let bytes = test_epub::make_epub_with_resources(&chapters, vec![]);
    let book = EpubBook::parse(&bytes, Default::default()).unwrap();
    let data = test_font::make_font();
    let font = Font::parse(&data, 0, FontLimits::default()).unwrap();
    let temp = Temp::new();
    let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
    let mut ui = Presentation::new(
        session,
        None,
        UiFont::from_bytes(data.clone(), "fixture.ttf".into()).unwrap(),
        temp.store(),
    )
    .unwrap();
    let anchor = ui.session().anchor().clone();
    let rect = ui.window.toolbar_button_rect(5);
    tap(&mut ui, rect);
    assert_eq!(ui.ui_state().mode, "collapsed");
    let rect = ui.window.collapsed_rect();
    tap(&mut ui, rect);
    assert_eq!(ui.ui_state().mode, "expanded");
    let rect = ui.window.toolbar_button_rect(1);
    tap(&mut ui, rect);
    assert_eq!(ui.ui_state().mode, "toc");
    let (x, y) = center(ui.window.toc_panel_rect());
    ui.touch(0, x, y).unwrap();
    ui.touch(2, x, y).unwrap();
    ui.touch(3, x, y - 180).unwrap();
    ui.touch(4, x, y - 180).unwrap();
    ui.touch(9, 0, 600).unwrap();
    assert_eq!(ui.session().anchor(), &anchor);
    assert_eq!(ui.ui_state().mode, "toc");
    assert!(ui.window.toc_selected > 0);
    ui.back().unwrap();
    assert!(!ui.closed());
    assert_eq!(ui.ui_state().mode, "expanded");
}
#[test]
fn all_page_modes_are_shared_persistent_and_background_freezes_motion() {
    let temp = Temp::new();
    let body = format!(
        "<html><body>{}</body></html>",
        "<p>AAAA WWWW AAAA</p>".repeat(100)
    );
    let bytes = test_epub::make_epub_with_resources(&[&body], vec![]);
    let book = EpubBook::parse(&bytes, Default::default()).unwrap();
    let data = test_font::make_font();
    let font = Font::parse(&data, 0, FontLimits::default()).unwrap();
    let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
    let mut ui = Presentation::new(
        session,
        None,
        UiFont::from_bytes(data.clone(), "fixture.ttf".into()).unwrap(),
        temp.store(),
    )
    .unwrap();
    let first = ui.session().anchor().clone();
    ui.touch(0, 100, 100).unwrap();
    ui.touch(1, 100, 100).unwrap();
    assert_eq!(ui.session().anchor(), &first);
    ui.action(Action::Command(ReaderCommand::Settings)).unwrap();
    for _ in 0..4 {
        ui.action(Action::Next).unwrap();
    }
    ui.action(Action::Larger).unwrap();
    ui.back().unwrap();
    assert_eq!(ui.ui_state().page_mode, "book");
    assert_eq!(temp.store().settings().unwrap().page_mode, PageMode::Book);
    ui.touch(0, 330, 200).unwrap();
    ui.touch(2, 330, 200).unwrap();
    ui.touch(3, 40, 200).unwrap();
    ui.touch(4, 40, 200).unwrap();
    assert_ne!(ui.session().anchor(), &first);
    assert!(ui.ui_state().animating);
    ui.pause().unwrap();
    assert!(!ui.ui_state().animating);
    ui.action(Action::Command(ReaderCommand::Settings)).unwrap();
    for _ in 0..4 {
        ui.action(Action::Next).unwrap();
    }
    ui.action(Action::Larger).unwrap();
    ui.back().unwrap();
    assert_eq!(ui.ui_state().page_mode, "scroll");
    assert_eq!(temp.store().settings().unwrap().page_mode, PageMode::Scroll);
    ui.touch(0, 200, 260).unwrap();
    ui.touch(2, 200, 260).unwrap();
    ui.touch(3, 200, 140).unwrap();
    ui.touch(4, 200, 140).unwrap();
    assert!(ui.ui_state().animating);
    std::thread::sleep(Duration::from_millis(20));
    ui.tick().unwrap();
    ui.pause().unwrap();
    assert!(!ui.ui_state().animating);
}
#[test]
fn short_viewports_keep_toc_clear_of_controls_and_rows_do_not_click_through() {
    let bytes = test_epub::make_epub_with_resources(
        &[
            "<html><body>AAAA</body></html>",
            "<html><body>WWWW</body></html>",
            "<html><body>AAAA WWWW</body></html>",
        ],
        vec![],
    );
    let book = EpubBook::parse(&bytes, Default::default()).unwrap();
    let data = test_font::make_font();
    let font = Font::parse(&data, 0, FontLimits::default()).unwrap();
    for (width, height) in [(320, 256), (640, 360), (400, 400)] {
        let temp = Temp::new();
        let mut opts = options();
        opts.width = width;
        opts.height = height;
        let session = EpubSession::new(&book, &font, opts, Start::Beginning).unwrap();
        let mut ui = Presentation::new(
            session,
            None,
            UiFont::from_bytes(data.clone(), "fixture.ttf".into()).unwrap(),
            temp.store(),
        )
        .unwrap();
        ui.contents().unwrap();
        let panel = ui.window.toc_panel_rect();
        let handle = ui.window.collapsed_rect();
        assert!(panel.y >= 32 && panel.y + (panel.height as i32) < handle.y);
        let selected = ui.window.toc_selected;
        let anchor = ui.session().anchor().clone();
        // This coordinate is an old, now hidden toolbar button inside the taller TOC.
        let (x, y) = center(ui.window.toolbar_button_rect(4));
        assert!(!matches!(
            ui.window.hover_target_at(x, y),
            ReaderHover::Toolbar(_)
        ));
        ui.touch(2, panel.x + 20, panel.y + 55).unwrap();
        ui.touch(3, panel.x + 20, panel.y - 35).unwrap();
        ui.touch(4, panel.x + 20, panel.y - 35).unwrap();
        assert_eq!(ui.session().anchor(), &anchor);
        assert!(ui.window.toc_selected >= selected);
        tap(&mut ui, handle);
        assert_eq!(ui.ui_state().mode, "expanded");
    }
}
#[test]
fn mobile_long_press_selection_emits_copy_without_turning_page() {
    let temp = Temp::new();
    let bytes = test_epub::make_epub_with_resources(
        &["<html><body><p>AAAA WWWW AAAA</p></body></html>"],
        vec![],
    );
    let book = EpubBook::parse(&bytes, Default::default()).unwrap();
    let data = test_font::make_font();
    let font = Font::parse(&data, 0, FontLimits::default()).unwrap();
    let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
    let mut ui = Presentation::new(
        session,
        None,
        UiFont::from_bytes(data.clone(), "fixture.ttf".into()).unwrap(),
        temp.store(),
    )
    .unwrap();
    let anchor = ui.session().anchor().clone();
    let hits = &ui.session().frame().hits;
    let (x, y) = center(hits[0].rect);
    let (end_x, end_y) = center(hits.last().unwrap().rect);
    for (kind, x, y) in [(0, x, y), (5, x, y), (6, end_x, end_y), (7, end_x, end_y)] {
        ui.touch(kind, x, y).unwrap();
    }
    assert_eq!(ui.session().anchor(), &anchor);
    assert!(ui.window.tools.selection.is_some());
    ui.action(Action::Command(ReaderCommand::Copy)).unwrap();
    assert!(
        matches!(ui.effects().as_slice(),[crate::mobile::Effect::Copy(s)] if s.contains("AAAA")&&s.contains("WWWW"))
    );
    ui.back().unwrap();
    assert!(!ui.closed());
    assert!(ui.window.tools.selection.is_none());
}
#[test]
fn shared_search_editing_and_clipboard_browser_requests_use_mobile_host() {
    let temp = Temp::new();
    let bytes = test_epub::make_epub_with_resources(
        &[
            "<html><body><p>AAAA</p><a href='https://example.invalid/read?q=1'>WWWW</a></body></html>",
        ],
        vec![],
    );
    let book = EpubBook::parse(&bytes, Default::default()).unwrap();
    let data = test_font::make_font();
    let font = Font::parse(&data, 0, FontLimits::default()).unwrap();
    let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
    let mut ui = Presentation::new(
        session,
        None,
        UiFont::from_bytes(data.clone(), "fixture.ttf".into()).unwrap(),
        temp.store(),
    )
    .unwrap();
    ui.action(Action::Command(ReaderCommand::Find)).unwrap();
    assert!(ui.ui_state().editing);
    ui.input("search", "中文😀".into()).unwrap();
    assert_eq!(ui.ui_state().input, "中文😀");
    assert!(!ui.input("note", "stale".into()).unwrap());
    ui.action(Action::Command(ReaderCommand::Paste)).unwrap();
    assert!(matches!(
        ui.effects().as_slice(),
        [crate::mobile::Effect::Paste]
    ));
    assert!(ui.effects().is_empty());
    ui.host_reply(2, " WWWW".into()).unwrap();
    assert_eq!(ui.ui_state().input, "中文😀 WWWW");
    ui.back().unwrap();
    assert!(!ui.ui_state().editing);
    let rect = ui
        .session()
        .frame()
        .hits
        .iter()
        .map(|h| h.rect)
        .find(|r| {
            let (x, y) = center(*r);
            ui.session().link_at(x, y).is_some()
        })
        .unwrap();
    tap(&mut ui, rect);
    assert_eq!(ui.ui_state().mode, "external");
    assert!(ui.effects().is_empty());
    ui.action(Action::Activate).unwrap();
    assert!(
        matches!(ui.effects().as_slice(),[crate::mobile::Effect::OpenUrl(url)] if url == "https://example.invalid/read?q=1")
    );
}

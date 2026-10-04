use super::super::link_tests::{book, with_reader};
use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "readall-selection-actions-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn select(reader: &mut ReaderWindow<'_, '_, '_, '_>) {
    let hits = reader.session.frame().hits.clone();
    let first = hits[0].rect;
    let last = hits[3].rect;
    reader
        .action(Action::Click {
            x: first.x + 1,
            y: first.y + 1,
        })
        .unwrap();
    assert!(
        reader.selection_actions_rect().is_none(),
        "do not cover the pointer during a drag"
    );
    reader
        .action(Action::PointerMove {
            x: last.x + 1,
            y: last.y + 1,
        })
        .unwrap();
    reader
        .action(Action::PointerRelease {
            x: last.x + 1,
            y: last.y + 1,
        })
        .unwrap();
    assert!(reader.selection_actions_rect().is_some());
}
fn button(reader: &mut ReaderWindow<'_, '_, '_, '_>, index: usize) {
    let bar = reader.selection_actions_rect().unwrap();
    let cell = ReaderWindow::selection_button(bar, index);
    reader
        .action(Action::Click {
            x: cell.x + cell.width as i32 / 2,
            y: cell.y + 12,
        })
        .unwrap();
    reader
        .action(Action::PointerRelease {
            x: cell.x + cell.width as i32 / 2,
            y: cell.y + 12,
        })
        .unwrap();
}
#[test]
fn selected_text_can_be_highlighted_noted_and_cleared_using_mouse_actions() {
    with_reader(
        book(&["<html><body><p>AAAA WWWW</p><p>AAAA</p></body></html>"]),
        |reader| {
            let temp = Temp::new();
            reader.tools.store = Some(Store::new(temp.0.clone()));
            let origin = reader.session.anchor().clone();
            select(reader);
            let range = reader.tools.selection.clone().unwrap();
            assert_eq!(reader.session.text_at(range.clone()), "AAAA");
            button(reader, 1);
            let rows = reader
                .tools
                .store
                .as_ref()
                .unwrap()
                .annotations(reader.session.book())
                .unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].kind, Kind::Highlight);
            assert_eq!(rows[0].end, Some(range.end));
            assert!(reader.tools.status.contains("高亮已保存"));
            assert_eq!(reader.tools.selection, Some(range.clone()));
            button(reader, 2);
            assert_eq!(reader.tools.mode, Mode::Note);
            assert!(reader.selection_actions_rect().is_none());
            for ch in "测试笔记".chars() {
                reader.action(Action::Text(ch)).unwrap();
            }
            reader.action(Action::Activate).unwrap();
            let rows = reader
                .tools
                .store
                .as_ref()
                .unwrap()
                .annotations(reader.session.book())
                .unwrap();
            assert_eq!(rows.len(), 2);
            assert!(rows.iter().any(|row| row.kind == Kind::Note
                && row.text == "测试笔记"
                && row.end == Some(range.end)));
            assert!(reader.selection_actions_rect().is_some());
            button(reader, 3);
            assert!(reader.tools.selection.is_none());
            assert!(reader.selection_actions_rect().is_none());
            assert_eq!(reader.session.anchor(), &origin);
            assert!(!reader.close_requested());
        },
    );
}
#[test]
fn copy_feedback_and_unavailable_storage_keep_selection_and_never_click_through() {
    with_reader(book(&["<html><body>AAAA WWWW</body></html>"]), |reader| {
        select(reader);
        let range = reader.tools.selection.clone();
        let origin = reader.session.anchor().clone();
        // Keep a simulated clipboard request in flight: the Copy button must use
        // the guarded clipboard path, never touch the user's actual clipboard.
        let (tx, rx) = mpsc::channel();
        reader.tools.clipboard = Some(rx);
        button(reader, 0);
        assert!(reader.tools.status.contains("剪贴板操作尚未结束"));
        assert_eq!(reader.tools.selection, range);
        tx.send(ClipboardResult::Copied).unwrap();
        reader.animation_tick().unwrap();
        assert!(reader.tools.status.contains("已复制"));
        assert!(reader.selection_actions_rect().is_some());
        button(reader, 1);
        assert!(reader.tools.status.contains("存储目录"));
        assert_eq!(reader.tools.selection, range);
        assert_eq!(reader.session.anchor(), &origin);
        assert!(reader.tools.drag.is_none());
    });
}
#[test]
fn selection_action_bar_fits_small_windows_and_is_hidden_behind_reader_panels() {
    with_reader(book(&["<html><body>AAAA WWWW</body></html>"]), |reader| {
        for (w, h) in [(256, 256), (640, 480), (1200, 700)] {
            reader.resize(w, h).unwrap();
            reader.toolbar = ToolbarMode::Collapsed;
            select(reader);
            let bar = reader.selection_actions_rect().unwrap();
            assert_eq!(bar.intersection(Rect::new(0, 0, w, h)), bar);
            for index in 0..4 {
                let button = ReaderWindow::selection_button(bar, index);
                assert_eq!(button.intersection(bar), button);
            }
            assert_eq!(
                reader.surface.pixel((bar.x + 2) as u32, (bar.y + 2) as u32),
                Some(Color::rgba(35, 42, 52, 255))
            );
            reader.tools.mode = Mode::Settings;
            assert!(reader.selection_actions_rect().is_none());
            reader.tools.mode = Mode::None;
            reader.toolbar = ToolbarMode::Toc;
            assert!(reader.selection_actions_rect().is_none());
            reader.toolbar = ToolbarMode::Collapsed;
            reader.tools.clear_selection();
        }
    });
}

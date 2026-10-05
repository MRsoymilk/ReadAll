use super::*;
use crate::{test_epub, test_font};
use std::sync::atomic::{AtomicUsize, Ordering};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "readall-desktop-ops-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn font() -> UiFont {
    UiFont::from_bytes(test_font::make_font(), "fixture.ttf".into()).unwrap()
}
#[test]
fn recent_keyboard_navigation_reaches_hidden_books_and_uses_the_visible_path() {
    let font = font();
    let mut h = Home::new(1040, 700, &font).unwrap();
    h.recent = (0..20)
        .map(|i| PathBuf::from(format!("/fixture/book{i:02}.epub")))
        .collect();
    h.paint().unwrap();
    assert!(h.visible_recent_count() > 3);
    h.action(Action::Last).unwrap();
    assert_eq!(h.recent_selected, Some(19));
    assert!(h.recent_scroll > 0);
    let last = h.recent.len() - 1 - h.recent_scroll;
    assert_eq!(h.recent_rows[last].path, h.recent[19]);
    h.action(Action::Activate).unwrap();
    assert_eq!(h.selected_book, Some(PathBuf::from("/fixture/book19.epub")));
}
#[test]
fn scrolling_recent_history_cancels_a_pressed_delete_without_removing_a_shifted_book() {
    let temp = Temp::new();
    let font = font();
    let mut h = Home::new(1040, 700, &font).unwrap();
    h.recent = (0..20)
        .map(|i| temp.0.join(format!("book{i}.epub")))
        .collect();
    let before = h.recent.clone();
    h.recent_store = None;
    h.paint().unwrap();
    let button = h.recent_delete_rect(0);
    h.action(Action::Click {
        x: button.x + 10,
        y: button.y + 10,
    })
    .unwrap();
    assert!(h.pending_recent_delete.is_some());
    h.action(Action::Scroll {
        dx: 0,
        dy: RECENT_ROW_HEIGHT * 256 * 3,
    })
    .unwrap();
    assert!(h.pending_recent_delete.is_none());
    assert_eq!(h.recent_scroll, 3);
    h.action(Action::PointerRelease {
        x: button.x + 10,
        y: button.y + 10,
    })
    .unwrap();
    assert_eq!(h.recent, before);
    assert!(!h.close_requested);
    let row = h.recent_row_rect(1);
    h.action(Action::Click {
        x: row.x + 20,
        y: row.y + 10,
    })
    .unwrap();
    assert_eq!(h.selected_book, Some(before[4].clone()));
}
#[test]
fn sidebar_recent_is_clickable_and_escape_returns_from_the_browser() {
    let temp = Temp::new();
    let font = font();
    let mut h = Home::new(1040, 700, &font).unwrap();
    h.recent = vec![temp.0.join("sample.epub")];
    h.mode = Mode::Browser(Browser::load(temp.0.clone()).unwrap());
    assert!(h.action(Action::Close).unwrap());
    assert!(matches!(h.mode, Mode::Library));
    assert!(!h.close_requested);
    let nav = h.sidebar_rect(2);
    h.action(Action::Click {
        x: nav.x + 20,
        y: nav.y + 12,
    })
    .unwrap();
    assert_eq!(h.recent_selected, Some(0));
    let nav = h.sidebar_rect(0);
    h.action(Action::Click {
        x: nav.x + 20,
        y: nav.y + 12,
    })
    .unwrap();
    assert_eq!(h.recent_selected, None);
}
#[test]
fn browser_scroll_moves_the_viewport_and_directory_failures_do_not_close_the_window() {
    let temp = Temp::new();
    for i in 0..30 {
        fs::create_dir(temp.0.join(format!("folder{i:02}"))).unwrap();
    }
    let font = font();
    let mut h = Home::new(1040, 700, &font).unwrap();
    h.mode = Mode::Browser(Browser::load(temp.0.clone()).unwrap());
    h.paint().unwrap();
    h.action(Action::Scroll {
        dx: 0,
        dy: ROW_HEIGHT * 256 * 4,
    })
    .unwrap();
    let Mode::Browser(b) = &h.mode else { panic!() };
    assert_eq!(b.scroll, 4);
    let folder = b.selected().unwrap().path.clone();
    fs::remove_dir(&folder).unwrap();
    assert!(h.action(Action::Activate).unwrap());
    assert!(!h.close_requested);
    assert!(h.status.starts_with("无法访问"));
    let now = Instant::now();
    h.observe_home_notice(now);
    assert!(h.expire_home_notice(now + Duration::from_secs(7)));
    assert!(h.status.is_empty());
}
#[test]
fn background_preview_debounces_and_never_applies_a_previous_rows_metadata() {
    let temp = Temp::new();
    let first = temp.0.join("first.epub");
    let second = temp.0.join("second.epub");
    fs::write(&first, test_epub::make_epub()).unwrap();
    fs::write(&second, b"invalid book").unwrap();
    let mut worker = preview::Preview::new().unwrap();
    let now = Instant::now();
    worker.select(Some(first), now);
    assert!(worker.poll(now).is_none());
    worker.select(Some(second), now);
    assert!(worker.poll(now + Duration::from_millis(50)).is_none());
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut result = None;
    while Instant::now() < deadline {
        result = worker.poll(Instant::now() + Duration::from_millis(200));
        if result.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(result.unwrap().contains("不可用"));
    worker.select(None, Instant::now());
    assert!(worker.poll(Instant::now()).is_none());
}
#[test]
#[ignore = "manual desktop-library screenshots using original fixtures and the built-in font"]
fn capture_linux_library() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/linux-ui-validation/screens");
    fs::create_dir_all(&root).unwrap();
    let data = crate::ui::builtin_font_bytes();
    let font = UiFont::from_bytes(data.to_vec(), "builtin.ttf".into()).unwrap();
    let mut h = Home::new(1040, 700, &font).unwrap();
    h.recent_store = None;
    h.recent = [
        "嵌入式 Linux 驱动开发指南.epub",
        "深入理解计算机系统.azw3",
        "Rust 程序设计语言.mobi",
        "山与海的故事.epub",
        "一个很长的书名：原生桌面阅读与工程实践.epub",
        "设计与生活.epub",
        "算法导论.epub",
        "现代软件工程.epub",
        "第九本电子书.epub",
    ]
    .iter()
    .map(|name| PathBuf::from("/fixture").join(name))
    .collect();
    for (name, theme) in [("light", Theme::Paper), ("dark", Theme::Dark)] {
        h.theme = theme;
        h.paint().unwrap();
        let mut bytes = format!(
            "P6\n{} {}\n255\n",
            h.surface.pixel_width(),
            h.surface.pixel_height()
        )
        .into_bytes();
        for p in h.surface.pixels() {
            bytes.extend_from_slice(&[p.r, p.g, p.b]);
        }
        fs::write(root.join(format!("{name}-library.ppm")), bytes).unwrap();
    }
    println!("library captures: {}", root.display());
}

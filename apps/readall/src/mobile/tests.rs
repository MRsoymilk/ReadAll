use super::*;
use crate::{test_azw3, test_epub, test_font, test_mobi};
use std::{fs, time::Instant};
static TEST_LOCK: Mutex<()> = Mutex::new(());
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "readall-mobile-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn config(&self, name: &str, bytes: &[u8]) -> Config {
        fs::write(self.0.join(name), bytes).unwrap();
        fs::write(self.0.join("font.ttf"), test_font::make_font()).unwrap();
        Config {
            book: self.0.join(name),
            font: self.0.join("font.ttf"),
            state_dir: self.0.join("state"),
            width: 400,
            height: 520,
            font_size: 16,
            margin: 24,
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(reader: &Reader, predicate: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let s = reader.snapshot();
        if predicate(&s) {
            return s;
        }
        assert!(
            Instant::now() < deadline,
            "mobile worker timed out: {:?}",
            s
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn stop(reader: &Reader) {
    reader.cancel();
    wait(reader, |s| s.closed);
}

#[test]
fn shared_worker_opens_all_publication_formats_without_wayland() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for (name, bytes) in [
        ("book.epub", test_epub::make_epub()),
        (
            "book.mobi",
            test_mobi::make_mobi("<html><body><p>AAAA WWWW</p></body></html>"),
        ),
        ("book.azw3", test_azw3::make_azw3("<p>AAAA WWWW</p>")),
    ] {
        let temp = Temp::new();
        let config = temp.config(name, &bytes);
        let reader = Reader::open(config.clone()).unwrap();
        let ready = wait(&reader, |s| s.frame.is_some() || s.closed);
        assert!(!ready.closed, "{name}: {}", ready.notice);
        assert!(!ready.locator.is_empty());
        let frame = ready.frame.unwrap();
        assert_eq!((frame.surface.width(), frame.surface.height()), (400, 520));
        let mut rgba = vec![0; frame.byte_len()];
        frame.write_rgba(&mut rgba).unwrap();
        assert!(rgba.chunks_exact(4).any(|p| p != [255, 255, 255, 255]));
        assert!(frame.write_rgba(&mut [0; 4]).is_err());
        stop(&reader);
        assert_eq!(fs::read(config.book).unwrap(), bytes);
    }
}
#[test]
fn navigation_toc_theme_resize_and_restart_keep_a_content_anchor() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp = Temp::new();
    let body = format!(
        "<html><body>{}</body></html>",
        "<p>AAAA WWWW AAAA WWWW</p>".repeat(150)
    );
    let bytes = test_epub::make_epub_with_resources(
        &[&body, "<html><body><h1>WWWW</h1><p>AAAA</p></body></html>"],
        vec![],
    );
    let config = temp.config("book.epub", &bytes);
    let reader = Reader::open(config.clone()).unwrap();
    let first = wait(&reader, |s| s.frame.is_some());
    reader.command(Command::Next).unwrap();
    let second = wait(&reader, |s| !s.busy && s.locator != first.locator);
    assert_ne!(first.locator, second.locator);
    reader
        .command(Command::Resize {
            width: 500,
            height: 600,
        })
        .unwrap();
    let resized = wait(&reader, |s| {
        s.frame.as_ref().is_some_and(|f| f.surface.width() == 500)
    });
    assert_eq!(resized.locator, second.locator);
    reader.command(Command::Contents).unwrap();
    let toc = wait(&reader, |s| !s.contents.is_empty());
    assert_eq!(toc.contents.len(), 2);
    reader
        .command(Command::Jump {
            spine: 1,
            offset: 0,
        })
        .unwrap();
    let jumped = wait(&reader, |s| s.position.contains("第 2/2 章"));
    reader.command(Command::CycleTheme).unwrap();
    let themed = wait(&reader, |s| {
        s.frame
            .as_ref()
            .is_some_and(|f| f.serial > jumped.frame.as_ref().unwrap().serial)
    });
    assert_eq!(themed.locator, jumped.locator);
    reader.command(Command::Bookmark).unwrap();
    wait(&reader, |s| s.notice.contains("书签已保存"));
    stop(&reader);
    drop(reader);
    let resumed = Reader::open(config).unwrap();
    let ready = wait(&resumed, |s| s.frame.is_some());
    assert_eq!(ready.locator, jumped.locator);
    stop(&resumed);
}
#[test]
fn errors_and_cancellation_preserve_page_content_while_reporting_status() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp = Temp::new();
    let config = temp.config("book.epub", &test_epub::make_epub());
    let reader = Reader::open(config).unwrap();
    let first = wait(&reader, |s| s.frame.is_some());
    reader
        .command(Command::Jump {
            spine: usize::MAX,
            offset: 0,
        })
        .unwrap();
    let error = wait(&reader, |s| !s.notice.is_empty());
    assert_eq!(error.locator, first.locator);
    let before = first.frame.unwrap();
    let after = error.frame.unwrap();
    assert_eq!(
        &before.surface.pixels()[100 * 400..200 * 400],
        &after.surface.pixels()[100 * 400..200 * 400]
    );
    assert!(
        reader
            .command(Command::Resize {
                width: 1,
                height: 1
            })
            .is_err()
    );
    stop(&reader);
    assert!(reader.command(Command::Next).is_err());
    let invalid = Reader::open(temp.config("bad.azw3", b"not a book")).unwrap();
    let state = wait(&invalid, |s| s.closed);
    assert!(state.frame.is_none());
    assert!(!state.notice.is_empty());
}
#[test]
fn mobile_commands_drive_shared_settings_touch_and_pause_without_reopening_book() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let temp = Temp::new();
    let html = format!(
        "<html><body>{}</body></html>",
        "<p>AAAA WWWW AAAA</p>".repeat(150)
    );
    let bytes = test_epub::make_epub_with_resources(&[&html], vec![]);
    let config = temp.config("touch.epub", &bytes);
    let reader = Reader::open(config.clone()).unwrap();
    let first = wait(&reader, |s| s.frame.is_some());
    assert_eq!(first.ui_mode, "expanded");
    reader
        .command(Command::Ui(UiAction::Command(UiCommand::Settings)))
        .unwrap();
    wait(&reader, |s| s.ui_mode == "settings");
    for _ in 0..4 {
        reader.command(Command::Next).unwrap();
    }
    reader.command(Command::Larger).unwrap();
    wait(&reader, |s| s.page_mode == "book" && !s.busy);
    reader.command(Command::Back).unwrap();
    wait(&reader, |s| s.ui_mode == "expanded" && !s.busy);
    for (kind, x, y) in [(0, 350, 170), (2, 350, 170), (3, 40, 170), (4, 40, 170)] {
        reader.command(Command::Touch { kind, x, y }).unwrap();
    }
    let turned = wait(&reader, |s| s.locator != first.locator);
    reader.command(Command::Pause(true)).unwrap();
    wait(&reader, |s| {
        !s.animating && !s.busy && s.revision > turned.revision
    });
    reader.command(Command::Pause(false)).unwrap();
    stop(&reader);
    drop(reader);
    let resumed = Reader::open(config).unwrap();
    let ready = wait(&resumed, |s| s.frame.is_some());
    assert_eq!(ready.page_mode, "book");
    assert_eq!(ready.locator, turned.locator);
    stop(&resumed);
}
#[test]
fn validated_mobile_paths_and_dimensions_precede_thread_creation() {
    let temp = Temp::new();
    let mut config = temp.config("book.epub", &test_epub::make_epub());
    config.book = PathBuf::from("relative.epub");
    assert!(Reader::open(config.clone()).is_err());
    config.book = temp.0.join("book.epub");
    config.width = 4096;
    config.height = 4096;
    assert!(Reader::open(config).is_err());
}

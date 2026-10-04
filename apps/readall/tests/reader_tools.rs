//! Exercise the actual executable, without relying on a compositor or user data.
// Shared fixture module includes constructors exercised by the other integration suites.
#[allow(dead_code)]
#[path = "support/epub.rs"]
mod epub;
#[path = "support/font.rs"]
mod font;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "readall-tools-cli-{}-{}",
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
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_readall"))
        .args(args)
        .output()
        .unwrap()
}
fn ok(args: &[&str]) -> String {
    let result = run(args);
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}
#[test]
fn cli_roundtrips_v2_image_progress_annotations_and_settings() {
    let dir = Temp::new();
    let book = dir.0.join("images.epub");
    let face = dir.0.join("test.ttf");
    let data = dir.0.join("state");
    fs::write(&book,epub::make_epub_with_resources(&["<html><body><section style='background:#ffeecc'><img src='a.png'/><img src='a.png'/></section></body></html>"],vec![("a.png","image/png",epub::make_png(100,90,[20,40,60,255]))])).unwrap();
    fs::write(&face, font::make_font()).unwrap();
    let path = book.to_str().unwrap();
    let face = face.to_str().unwrap();
    let data = data.to_str().unwrap();
    let out = dir.0.join("second.ppm");
    let report = ok(&[
        "render-epub",
        path,
        out.to_str().unwrap(),
        "--font",
        face,
        "--width",
        "200",
        "--height",
        "128",
        "--margin",
        "16",
        "--font-size",
        "16",
        "--page",
        "2",
    ]);
    let locator = report
        .lines()
        .find_map(|line| line.strip_prefix("EPUB locator: "))
        .unwrap();
    assert!(locator.starts_with("epub-v2:"));
    assert!(locator.ends_with(":0:1"));
    let restored = dir.0.join("restored.ppm");
    let result = ok(&[
        "render-epub",
        path,
        restored.to_str().unwrap(),
        "--font",
        face,
        "--width",
        "240",
        "--height",
        "144",
        "--margin",
        "16",
        "--font-size",
        "18",
        "--at",
        locator,
    ]);
    assert!(result.contains("Page: 2/2"));
    assert!(result.contains(locator));
    ok(&["bookmark", path, locator, "图片书签", "--data-dir", data]);
    ok(&["note", path, locator, "图片笔记", "--data-dir", data]);
    let rows = ok(&["annotations", path, "--data-dir", data]);
    assert!(rows.contains("图片书签"));
    assert!(rows.contains("图片笔记"));
    assert_eq!(rows.matches(locator).count(), 2);
    ok(&["settings", "theme", "dark", "--data-dir", data]);
    assert!(ok(&["settings", "--data-dir", data]).contains("theme=dark"));
    assert!(
        !run(&["settings", "size", "0", "--data-dir", data])
            .status
            .success()
    );
    assert!(ok(&["settings", "--data-dir", data]).contains("theme=dark"));
    assert!(ok(&["annotation-remove", path, "1", "--data-dir", data]).contains("Removed: true"));
    assert!(!ok(&["annotations", path, "--data-dir", data]).contains("图片书签"));
}
#[test]
fn cli_search_and_help_match_the_shipped_capabilities() {
    let dir = Temp::new();
    let book = dir.0.join("search.epub");
    fs::write(&book,epub::make_epub_with_resources(&["<html><body><p style='padding:8px;border:1px solid red'>Hello 中文</p><p style='display:none'>hello</p></body></html>","<html><body>HELLO 中文</body></html>"],vec![])).unwrap();
    let report = ok(&["search", book.to_str().unwrap(), "hello"]);
    assert!(report.contains("Matches: 2;"));
    let sensitive = ok(&[
        "search",
        book.to_str().unwrap(),
        "hello",
        "--case-sensitive",
    ]);
    assert!(sensitive.contains("Matches: 0;"));
    let help = ok(&["--help"]);
    assert!(help.contains("PNG/JPEG/WebP/GIF/SVG"));
    assert!(help.contains("shaping/bidi"));
    assert!(help.contains("F9 text selection"));
    assert!(!help.contains("font shaping and PDF reading are not implemented"));
}

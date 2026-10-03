#[path = "support/font.rs"]
mod font;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture {
    root: PathBuf,
    book: PathBuf,
    font: PathBuf,
}
impl Fixture {
    fn new(text: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "readall-render-test-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let book = root.join("book.txt");
        let font = root.join("fixture.ttf");
        fs::write(&book, text).unwrap();
        fs::write(&font, font::make_font()).unwrap();
        Self { root, book, font }
    }
    fn run(&self, output: &Path, extra: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_readall"))
            .arg("render-text")
            .arg(&self.book)
            .arg(output)
            .arg("--font")
            .arg(&self.font)
            .args(["--width", "200", "--height", "128", "--margin", "16"])
            .args(extra)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn info(result: &Output) -> String {
    assert!(result.status.success(), "{result:?}");
    String::from_utf8(result.stdout.clone()).unwrap()
}

#[test]
fn real_binary_renders_ink_and_antialiasing_and_never_overwrites() {
    let fixture = Fixture::new("AAAA WWWW\nAAA W\n");
    let output = fixture.root.join("page.ppm");
    let result = fixture.run(&output, &[]);
    let report = info(&result);
    assert!(report.contains("Missing characters: 0"));
    assert!(report.contains("Start locator: txt-v1:"));
    let bytes = fs::read(&output).unwrap();
    let header = b"P6\n200 128\n255\n";
    assert!(bytes.starts_with(header));
    assert_eq!(bytes.len(), header.len() + 200 * 128 * 3);
    let pixels = &bytes[header.len()..];
    assert!(pixels.iter().filter(|p| **p < 255).count() > 100);
    assert!(pixels.iter().any(|p| *p > 24 && *p < 255));
    assert!(!fixture.run(&output, &[]).status.success());
    assert_eq!(fs::read(&output).unwrap(), bytes);
    let book_before = fs::read(&fixture.book).unwrap();
    assert!(!fixture.run(&fixture.book, &[]).status.success());
    assert_eq!(fs::read(&fixture.book).unwrap(), book_before);
}
#[test]
fn measured_pages_resume_after_font_size_changes() {
    let fixture = Fixture::new(&"AAAA WWWW AAAA\n".repeat(12));
    let first = fixture.run(
        &fixture.root.join("small.ppm"),
        &["--font-size", "24", "--page", "2"],
    );
    let report = info(&first);
    assert!(report.contains("Page: 2/"));
    let locator = report
        .lines()
        .find_map(|line| line.strip_prefix("Start locator: "))
        .unwrap();
    let resumed = fixture.run(
        &fixture.root.join("resumed.ppm"),
        &["--font-size", "16", "--at", locator],
    );
    assert!(info(&resumed).contains("Rendered TXT page"));
    let document = readall_core::TextDocument::from_bytes(
        &fs::read(&fixture.book).unwrap(),
        readall_core::Limits::default(),
    )
    .unwrap();
    let offset = document.restore(&locator.parse().unwrap()).unwrap();
    assert!(offset > 0);
    let invalid = fixture.run(&fixture.root.join("invalid.ppm"), &["--page", "999999"]);
    assert!(!invalid.status.success());
    assert!(!fixture.root.join("invalid.ppm").exists());
}
#[test]
fn missing_and_corrupt_fonts_do_not_silently_produce_pages() {
    let fixture = Fixture::new("A中W");
    let output = fixture.root.join("strict.ppm");
    let failed = fixture.run(&output, &[]);
    assert!(!failed.status.success());
    assert!(String::from_utf8(failed.stderr).unwrap().contains("U+4E2D"));
    assert!(!output.exists());
    let replacement = fixture.run(
        &fixture.root.join("replacement.ppm"),
        &["--missing", "replacement"],
    );
    let report = info(&replacement);
    assert!(report.contains("Missing characters: 1"));
    assert!(report.contains("U+4E2D"));
    fs::write(&fixture.font, b"broken font").unwrap();
    let corrupt = fixture.run(&output, &[]);
    assert!(!corrupt.status.success());
    assert!(!output.exists());
}

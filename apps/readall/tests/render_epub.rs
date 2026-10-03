#[path = "support/epub.rs"]
mod epub;
#[path = "support/font.rs"]
mod font;

use std::{
    fs,
    path::PathBuf,
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
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "readall-epub-render-test-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let book = root.join("book.epub");
        let font = root.join("fixture.ttf");
        fs::write(&book, epub::make_epub()).unwrap();
        fs::write(&font, font::make_font()).unwrap();
        Self { root, book, font }
    }
    fn run(&self, output: &str, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_readall"));
        command
            .arg("render-epub")
            .arg(&self.book)
            .arg(self.root.join(output))
            .arg("--font")
            .arg(&self.font)
            .args(["--width", "200", "--height", "128", "--margin", "16"]);
        command.args(args).output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn report(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn epub_locator_restores_second_spine_after_reflow() {
    let fixture = Fixture::new();
    let first = report(fixture.run("second.ppm", &["--spine", "2", "--page", "2"]));
    assert!(first.contains("Spine: 2/2"));
    assert!(first.contains("Page: 2/"));
    let locator = first
        .lines()
        .find_map(|line| line.strip_prefix("EPUB locator: "))
        .unwrap();
    assert!(locator.starts_with("epub-v1:"));

    let mut command = Command::new(env!("CARGO_BIN_EXE_readall"));
    let resumed = command
        .arg("render-epub")
        .arg(&fixture.book)
        .arg(fixture.root.join("resumed.ppm"))
        .arg("--font")
        .arg(&fixture.font)
        .args(["--width", "260", "--height", "160", "--margin", "16"])
        .arg("--at")
        .arg(locator)
        .output()
        .unwrap();
    let resumed = report(resumed);
    assert!(resumed.contains("Spine: 2/2"));
    assert!(resumed.contains("EPUB locator: epub-v1:"));
}

#[test]
fn locator_cannot_be_mixed_with_spine_or_page() {
    let fixture = Fixture::new();
    let first = report(fixture.run("first.ppm", &["--spine", "1"]));
    let locator = first
        .lines()
        .find_map(|line| line.strip_prefix("EPUB locator: "))
        .unwrap();

    for conflicting in [
        vec!["--at", locator, "--spine", "1"],
        vec!["--at", locator, "--page", "1"],
    ] {
        let output = fixture.run("conflict.ppm", &conflicting);
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("cannot be combined")
        );
    }
}

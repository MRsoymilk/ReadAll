//! Actual CLI ingestion, image rendering and persisted data; isolated test state.
#[path = "support/epub.rs"]
mod epub;
#[path = "support/font.rs"]
mod font;
#[path = "support/mobi.rs"]
mod mobi;
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
            "readall-mobi-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_readall"))
            .args(args)
            .current_dir(&self.0)
            .env("HOME", &self.0)
            .env("XDG_STATE_HOME", self.0.join("state"))
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn mobi_cli_renders_images_preserves_code_searches_and_restores_annotations() {
    let temp = Temp::new();
    let html = "<html><body><h1>AAAA</h1><p>AAAA WWWW</p><img recindex='00001'/><pre class='language-c'>int f() {\n\treturn 42;\n}</pre><mbp:pagebreak/><h2>WWWW</h2><p>target 中文</p></body></html>";
    let bytes = mobi::build(
        html.as_bytes(),
        mobi::Options {
            images: vec![epub::make_png(40, 20, [11, 77, 199, 255])],
            ..Default::default()
        },
    );
    fs::write(temp.0.join("book.MOBI"), &bytes).unwrap();
    fs::write(temp.0.join("font.ttf"), font::make_font()).unwrap();
    let meta = temp.ok(&["mobi-info", "book.MOBI"]);
    assert!(meta.contains("Format: MOBI6") && meta.contains("ReadAll MOBI 中文"));
    let text = temp.ok(&["mobi-text", "book.MOBI"]);
    assert!(text.contains("int f() {\n\treturn 42;\n}"));
    let later = temp.ok(&["mobi-text", "book.MOBI", "--spine", "2"]);
    assert!(later.contains("target 中文"));
    let rendered = temp.ok(&[
        "render-mobi",
        "book.MOBI",
        "page.ppm",
        "--font",
        "font.ttf",
        "--width",
        "400",
        "--height",
        "520",
        "--margin",
        "32",
        "--font-size",
        "16",
        "--missing",
        "replacement",
    ]);
    assert!(rendered.contains("Rendered MOBI"));
    let image = fs::read(temp.0.join("page.ppm")).unwrap();
    let start = image
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'\n')
        .nth(2)
        .unwrap()
        .0
        + 1;
    assert!(
        image[start..].chunks_exact(3).any(|p| p == [11, 77, 199]),
        "embedded PNG should be visible"
    );
    assert!(
        !temp
            .run(&["render-mobi", "book.MOBI", "page.ppm", "--font", "font.ttf"])
            .status
            .success(),
        "do not overwrite output"
    );
    let found = temp.ok(&["search", "book.MOBI", "target", "--data-dir", "data"]);
    let locator = found
        .lines()
        .find_map(|line| line.split('\t').nth(1))
        .unwrap();
    assert!(locator.starts_with("epub-v"));
    temp.ok(&[
        "bookmark",
        "book.MOBI",
        locator,
        "saved target",
        "--data-dir",
        "data",
    ]);
    assert!(
        temp.ok(&["annotations", "book.MOBI", "--data-dir", "data"])
            .contains("saved target")
    );
    let restored = temp.ok(&[
        "render-mobi",
        "book.MOBI",
        "restored.ppm",
        "--font",
        "font.ttf",
        "--at",
        locator,
        "--missing",
        "replacement",
    ]);
    assert!(restored.contains("Spine: 2/2"));
    assert_eq!(fs::read(temp.0.join("book.MOBI")).unwrap(), bytes);
    assert!(
        fs::read_dir(&temp.0).unwrap().all(|e| e
            .unwrap()
            .path()
            .extension()
            .is_none_or(|e| e != "epub")),
        "adapter never writes temporary EPUBs"
    );
}
#[test]
fn encrypted_and_fake_files_fail_before_output_creation() {
    let temp = Temp::new();
    let mut bytes = mobi::make_mobi("<html><body>AAAA</body></html>");
    let at = u32::from_be_bytes(bytes[78..82].try_into().unwrap()) as usize;
    bytes[at + 12..at + 14].copy_from_slice(&2_u16.to_be_bytes());
    fs::write(temp.0.join("drm.mobi"), &bytes).unwrap();
    fs::write(temp.0.join("font.ttf"), font::make_font()).unwrap();
    let out = temp.run(&[
        "render-mobi",
        "drm.mobi",
        "not-created.ppm",
        "--font",
        "font.ttf",
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("DRM"));
    assert!(!temp.0.join("not-created.ppm").exists());
    assert_eq!(fs::read(temp.0.join("drm.mobi")).unwrap(), bytes);
    fs::write(temp.0.join("fake.mobi"), b"not a book").unwrap();
    assert!(!temp.run(&["mobi-text", "fake.mobi"]).status.success());
}
#[test]
fn epub_and_mobi_share_tools_without_changing_source_identity() {
    let temp = Temp::new();
    let epub_bytes = epub::make_epub();
    let mobi_bytes = mobi::make_mobi("<html><body>AAAA WWWW</body></html>");
    fs::write(temp.0.join("book.epub"), &epub_bytes).unwrap();
    fs::write(temp.0.join("book.mobi"), &mobi_bytes).unwrap();
    let original = temp.ok(&["epub-info", "book.epub"]);
    assert!(original.contains("Format: EPUB"));
    assert!(
        temp.ok(&["epub-info", "book.mobi"])
            .contains("Format: MOBI")
    );
    let first = temp.ok(&["search", "book.epub", "AAAA"]);
    let second = temp.ok(&["search", "book.mobi", "AAAA"]);
    let locator = |s: &str| {
        s.lines()
            .find_map(|line| line.split('\t').nth(1))
            .unwrap()
            .to_owned()
    };
    assert_ne!(locator(&first), locator(&second));
    assert_eq!(temp.ok(&["epub-info", "book.epub"]), original);
    assert_eq!(fs::read(temp.0.join("book.epub")).unwrap(), epub_bytes);
    assert_eq!(fs::read(temp.0.join("book.mobi")).unwrap(), mobi_bytes);
}

#[test]
fn mobi_signature_is_not_sent_to_txt_decoding() {
    let temp = Temp::new();
    fs::write(
        temp.0.join("binary.txt"),
        mobi::make_mobi("<html><body>AAAA</body></html>"),
    )
    .unwrap();
    let out = temp.run(&["inspect", "binary.txt"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("MOBI signature"));
}

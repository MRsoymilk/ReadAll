//! Real CLI paths, isolated state, and deterministic KF8 fixtures with images/CSS.
#[path = "support/azw3.rs"]
mod azw3;
#[path = "support/epub.rs"]
mod epub;
#[path = "support/font.rs"]
mod font;
#[path = "support/mobi.rs"]
mod test_mobi;
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
        let p = std::env::temp_dir().join(format!(
            "readall-azw3-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
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
fn book() -> Vec<u8> {
    const HEAD: &str = "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>AZW3 Test</title><link rel=\"stylesheet\" href=\"kindle:flow:0001?mime=text/css\"/></head><body>";
    azw3::build(
        &[
            azw3::Chapter {
                head: HEAD,
                fragments: vec![
                    "<p>AAAA WWWW</p><img src=\"kindle:embed:0001?mime=image/png\"/><pre class=\"language-c\">#define SIZE 4096\n\treturn 42;\n</pre>",
                ],
                tail: "</body></html>",
            },
            azw3::Chapter {
                head: HEAD,
                fragments: vec!["<h1>WWWW</h1><p>target 中文</p>"],
                tail: "</body></html>",
            },
        ],
        azw3::Options {
            flows: vec![b"p { color:#123456 }".to_vec()],
            resources: vec![epub::make_png(40, 20, [11, 77, 199, 255])],
            navigation: vec![
                azw3::Nav {
                    label: "One",
                    fid: 0,
                    offset: 0,
                    parent: None,
                },
                azw3::Nav {
                    label: "Two",
                    fid: 1,
                    offset: 0,
                    parent: None,
                },
            ],
            ..Default::default()
        },
    )
}
#[test]
fn azw3_cli_renders_images_searches_and_restores_progress_without_intermediate_files() {
    let temp = Temp::new();
    let bytes = book();
    fs::write(temp.0.join("book.AZW3"), &bytes).unwrap();
    fs::write(temp.0.join("font.ttf"), font::make_font()).unwrap();
    let meta = temp.ok(&["azw3-info", "book.AZW3"]);
    assert!(meta.contains("Format: AZW3/KF8"));
    let info = temp.ok(&["epub-info", "book.AZW3"]);
    assert!(info.contains("Spine items: 2") && info.contains("Navigation entries: 2"));
    let text = temp.ok(&["azw3-text", "book.AZW3"]);
    assert!(text.contains("#define SIZE 4096\n\treturn 42;"));
    let frame = temp.ok(&[
        "render-azw3",
        "book.AZW3",
        "first.ppm",
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
    assert!(frame.contains("Rendered AZW3/KF8"));
    let ppm = fs::read(temp.0.join("first.ppm")).unwrap();
    let at = ppm
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'\n')
        .nth(2)
        .unwrap()
        .0
        + 1;
    assert!(ppm[at..].chunks_exact(3).any(|p| p == [11, 77, 199]));
    let hits = temp.ok(&["search", "book.AZW3", "target"]);
    let locator = hits.lines().find_map(|l| l.split('\t').nth(1)).unwrap();
    temp.ok(&[
        "bookmark",
        "book.AZW3",
        locator,
        "KF8 marker",
        "--data-dir",
        "notes",
    ]);
    assert!(
        temp.ok(&["annotations", "book.AZW3", "--data-dir", "notes"])
            .contains("KF8 marker")
    );
    let frame = temp.ok(&[
        "render-azw3",
        "book.AZW3",
        "restored.ppm",
        "--font",
        "font.ttf",
        "--at",
        locator,
        "--missing",
        "replacement",
    ]);
    assert!(frame.contains("Spine: 2/2"));
    assert_eq!(fs::read(temp.0.join("book.AZW3")).unwrap(), bytes);
    assert!(
        fs::read_dir(&temp.0).unwrap().all(|e| e
            .unwrap()
            .path()
            .extension()
            .is_none_or(|e| e != "epub"))
    );
}
#[test]
fn azw3_content_detection_does_not_break_epub_or_accept_fake_and_encrypted_books() {
    let temp = Temp::new();
    let original = epub::make_epub();
    fs::write(temp.0.join("existing.epub"), &original).unwrap();
    assert!(
        temp.ok(&["epub-info", "existing.epub"])
            .contains("Format: EPUB")
    );
    let bytes = book();
    fs::write(temp.0.join("disguised.mobi"), &bytes).unwrap();
    assert!(
        temp.ok(&["epub-info", "disguised.mobi"])
            .contains("Format: AZW3/KF8")
    );
    let mut encrypted = azw3::records(&bytes);
    encrypted[0][12..14].copy_from_slice(&2_u16.to_be_bytes());
    let encrypted = azw3::assemble(encrypted);
    fs::write(temp.0.join("drm.azw3"), &encrypted).unwrap();
    let out = temp.run(&["azw3-text", "drm.azw3"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("DRM"));
    assert_eq!(fs::read(temp.0.join("drm.azw3")).unwrap(), encrypted);
    fs::write(temp.0.join("fake.azw3"), b"not KF8").unwrap();
    assert!(!temp.run(&["azw3-text", "fake.azw3"]).status.success());
    assert_eq!(fs::read(temp.0.join("existing.epub")).unwrap(), original);
}
#[cfg(all(target_os = "linux", feature = "wayland"))]
#[test]
fn azw3_gui_alias_keeps_loading_window_and_missing_display_error() {
    let temp = Temp::new();
    let bytes = book();
    fs::write(temp.0.join("book.azw3"), &bytes).unwrap();
    fs::write(temp.0.join("font.ttf"), font::make_font()).unwrap();
    let out = temp.run(&[
        "open-azw3",
        "book.azw3",
        "--font",
        "font.ttf",
        "--display",
        "/not-existing-readall-kf8-display",
        "--frames",
        "1",
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot connect to Wayland"));
    assert_eq!(fs::read(temp.0.join("book.azw3")).unwrap(), bytes);
}

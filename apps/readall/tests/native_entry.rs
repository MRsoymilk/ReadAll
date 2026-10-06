//! The actual CLI is tested without opening or manipulating the user's desktop.
use std::process::Command;
#[cfg(not(all(target_os = "linux", feature = "wayland")))]
#[test]
fn disabled_native_backend_has_an_actionable_error() {
    for args in [["open", "not-opened.txt"], ["open-epub", "not-opened.epub"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_readall"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("--features wayland")
        );
    }
}
#[cfg(all(target_os = "linux", feature = "wayland"))]
#[path = "support/epub.rs"]
mod epub;
#[cfg(all(target_os = "linux", feature = "wayland"))]
#[path = "support/font.rs"]
mod font;
#[cfg(all(target_os = "linux", feature = "wayland"))]
#[test]
fn malformed_window_options_fail_before_loading_a_book() {
    let output = Command::new(env!("CARGO_BIN_EXE_readall"))
        .args(["open", "not-opened.txt", "--frames", "2"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("one-frame")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_readall"))
        .args([
            "open-epub",
            "not-opened.epub",
            "--progress",
            "off",
            "--state-dir",
            "/tmp/unused",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("--state-dir cannot be used")
    );
}
#[cfg(all(target_os = "linux", feature = "wayland"))]
#[test]
fn missing_display_is_reported_by_the_real_binary() {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = std::env::temp_dir().join(format!(
        "readall-native-cli-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    fs::write(root.join("book.txt"), b"AAAA WWWW").unwrap();
    fs::write(root.join("font.ttf"), font::make_font()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_readall"))
        .arg("open")
        .arg(root.join("book.txt"))
        .arg("--font")
        .arg(root.join("font.ttf"))
        .arg("--display")
        .arg(root.join("does-not-exist"))
        .args(["--frames", "1"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("cannot connect to Wayland")
    );
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("Closed.")
    );
    assert_eq!(fs::read(root.join("book.txt")).unwrap(), b"AAAA WWWW");
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
fn minimal_pdf() -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for object in [
        b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".as_slice(),
        b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".as_slice(),
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Resources << >> /Contents 4 0 R >>\nendobj\n".as_slice(),
        b"4 0 obj\n<< /Length 32 >>\nstream\n0.1 0.4 0.8 rg 0 0 200 300 re f\nendstream\nendobj\n".as_slice(),
    ] {
        offsets.push(out.len());
        out.extend_from_slice(object);
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
#[test]
fn pdf_dispatch_reaches_wayland_without_modifying_the_book() {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = std::env::temp_dir().join(format!(
        "readall-native-pdf-cli-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let book = minimal_pdf();
    let path = root.join("book.pdf");
    fs::write(&path, &book).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_readall"))
        .arg(&path)
        .env("XDG_RUNTIME_DIR", &root)
        .env("WAYLAND_DISPLAY", "does-not-exist")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("cannot connect to Wayland")
    );
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("Input format: PDF")
    );
    assert_eq!(fs::read(path).unwrap(), book);
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
#[test]
fn missing_display_is_reported_for_epub_without_modifying_the_book() {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = std::env::temp_dir().join(format!(
        "readall-native-epub-cli-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let book = epub::make_epub();
    fs::write(root.join("book.epub"), &book).unwrap();
    fs::write(root.join("font.ttf"), font::make_font()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_readall"))
        .arg("open-epub")
        .arg(root.join("book.epub"))
        .arg("--font")
        .arg(root.join("font.ttf"))
        .arg("--spine")
        .arg("2")
        .arg("--display")
        .arg(root.join("does-not-exist"))
        .args(["--frames", "1"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("cannot connect to Wayland")
    );
    assert_eq!(fs::read(root.join("book.epub")).unwrap(), book);
}

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

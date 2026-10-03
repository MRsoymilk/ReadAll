use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct TempDirectory(PathBuf);
impl TempDirectory {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "readall-cli-test-{}-{nonce}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn cli(command: &str, path: &Path, options: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_readall"))
        .arg(command)
        .arg(path)
        .args(options)
        .output()
        .unwrap()
}
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sample.txt")
}

#[test]
fn inspect_reads_the_actual_fixture_and_reports_its_content_hash() {
    let result = cli("inspect", &fixture(), &[]);
    assert!(result.status.success(), "{:?}", result);
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("Format: TXT"));
    assert!(text.contains("Encoding: Utf8"));
    assert!(text.contains("ef72ad21eb4f96c29cad379969e306b66e5be397719749865a0d5d5d93f2733c"));
}

#[test]
fn read_and_resume_work_through_the_real_binary() {
    let result = cli(
        "read",
        &fixture(),
        &["--columns", "40", "--rows", "4", "--page", "2"],
    );
    assert!(result.status.success(), "{:?}", result);
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("Page: 2/"));
    let locator = text
        .lines()
        .find_map(|line| line.strip_prefix("Start locator: "))
        .unwrap();
    let resumed = cli(
        "read",
        &fixture(),
        &["--columns", "60", "--rows", "6", "--at", locator],
    );
    assert!(resumed.status.success(), "{:?}", resumed);
    assert!(
        String::from_utf8(resumed.stdout)
            .unwrap()
            .contains("Diagnostic preview")
    );
}

#[test]
fn unsupported_formats_exit_with_an_explicit_error() {
    let directory = TempDirectory::new();
    for (name, bytes, message) in [
        (
            "sample.pdf",
            b"%PDF-1.7".as_slice(),
            "PDF engine is not implemented",
        ),
        (
            "sample.epub",
            b"PK\x03\x04".as_slice(),
            "EPUB engine is not implemented",
        ),
    ] {
        let path = directory.0.join(name);
        fs::write(&path, bytes).unwrap();
        let output = cli("read", &path, &[]);
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr).unwrap().contains(message));
    }
}

#[test]
fn graphics_export_is_valid_and_never_overwrites_existing_files() {
    let directory = TempDirectory::new();
    let path = directory.0.join("calibration.ppm");
    let output = cli("render-demo", &path, &[]);
    assert!(output.status.success(), "{:?}", output);
    let bytes = fs::read(&path).unwrap();
    let header = b"P6\n640 360\n255\n";
    assert!(bytes.starts_with(header));
    assert_eq!(bytes.len(), header.len() + 640 * 360 * 3);
    let repeated = cli("render-demo", &path, &[]);
    assert!(!repeated.status.success());
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

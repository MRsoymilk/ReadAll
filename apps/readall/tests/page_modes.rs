//! Persistent mode settings are tested in isolated directories, never the user's state.
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
            "readall-page-modes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_readall"))
            .arg("settings")
            .args(args)
            .arg("--data-dir")
            .arg(&self.0)
            .output()
            .unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn old_settings_default_to_slide_and_explicit_changes_round_trip() {
    let temp = Temp::new();
    let path = temp.0.join("settings.conf");
    let legacy = "readall-settings-v1\ntheme=sepia\nsize=28\nmargin=48\nline-spacing=1.2\n";
    fs::write(&path, legacy).unwrap();
    let output = temp.run(&[]);
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("page-mode=slide")
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), legacy);
    for mode in ["book", "scroll", "slide"] {
        let output = temp.run(&["page-mode", mode]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = temp.run(&[]);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains(&format!("page-mode={mode}")));
        // Legacy sepia is readable without rewriting on load; explicit edits
        // use the canonical light/dark names and preserve non-theme fields.
        assert!(text.contains("theme=light"));
        assert!(text.contains("size=28"));
        assert!(text.contains("margin=48"));
        assert!(text.contains("line-spacing=1.2"));
    }
    let before = fs::read(&path).unwrap();
    let output = temp.run(&["page-mode", "invalid"]);
    assert!(!output.status.success());
    assert_eq!(fs::read(&path).unwrap(), before);
}

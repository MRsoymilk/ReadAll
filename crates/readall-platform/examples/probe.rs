//! Read-only host diagnostic; does not launch a shell or change desktop settings.
use std::{env, fs, path::Path};
fn main() {
    for key in ["WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "DISPLAY"] {
        println!("{key}={:?}", env::var_os(key));
    }
    for path in [
        "/run/user/1000",
        "/usr/lib64",
        "/usr/lib",
        "/usr/bin",
        "/usr/share/wayland-protocols/stable/xdg-shell",
    ] {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.take(16384).flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("wayland-")
                    || name.starts_with("libwayland-client.so")
                    || name == "weston"
                    || name == "xdg-shell.xml"
                    || name == "hyprctl"
                {
                    println!("visible: {}", entry.path().display());
                }
            }
        }
    }
    fonts(Path::new("/usr/share/fonts"), 0, &mut 0);
}
fn fonts(path: &Path, depth: usize, count: &mut usize) {
    if depth > 4 || *count >= 20 {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.take(2048).flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            fonts(&entry.path(), depth + 1, count);
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if kind.is_file()
            && *count < 20
            && ["cjk", "wqy", "droid"]
                .iter()
                .any(|part| name.contains(part))
        {
            println!("font: {}", entry.path().display());
            *count += 1;
        }
    }
}

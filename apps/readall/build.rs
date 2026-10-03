use readall_font::{Font, FontLimits};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const FONT_NAME: &str = "LXGWWenKaiLite-Regular.ttf";
const FONT_SIZE: u64 = 13_872_424;
const FONT_COMMIT: &str = "4cddacbe244b0a24b10076369105f0495e5ec898";
const FONT_URLS: &[&str] = &[
    "https://raw.githubusercontent.com/lxgw/LxgwWenKai-Lite/4cddacbe244b0a24b10076369105f0495e5ec898/fonts/TTF/LXGWWenKaiLite-Regular.ttf",
    "https://cdn.jsdelivr.net/gh/lxgw/LxgwWenKai-Lite@4cddacbe244b0a24b10076369105f0495e5ec898/fonts/TTF/LXGWWenKaiLite-Regular.ttf",
];
const REQUIRED_GLYPHS: &str =
    "ReadAll阅读器书库打开图书返回上一级当前章节页码字号目录最近设置中文简体繁體Aa0123";

fn main() {
    println!("cargo:rerun-if-env-changed=READALL_BUILTIN_FONT_SOURCE");
    println!("cargo:rerun-if-env-changed=READALL_SKIP_BUILTIN_FONT");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=READALL_BUILTIN_FONT_NAME={FONT_NAME}");
    println!("cargo:rustc-env=READALL_BUILTIN_FONT_COMMIT={FONT_COMMIT}");

    if env::var_os("CARGO_FEATURE_WAYLAND").is_none() {
        return;
    }

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join(FONT_NAME);
    if out.is_file() && validate(&out).is_ok() {
        return;
    }

    if env::var_os("READALL_SKIP_BUILTIN_FONT").is_some() {
        // Only for hermetic developer/CI builds that do not execute the GUI.
        // The runtime include still needs a file to compile.
        let fallback = env::var_os("READALL_BUILTIN_FONT_SOURCE")
            .map(PathBuf::from)
            .expect("READALL_SKIP_BUILTIN_FONT requires READALL_BUILTIN_FONT_SOURCE");
        fs::copy(&fallback, &out).expect("copy READALL_BUILTIN_FONT_SOURCE");
        return;
    }

    if let Some(source) = env::var_os("READALL_BUILTIN_FONT_SOURCE").map(PathBuf::from) {
        fs::copy(&source, &out).expect("copy READALL_BUILTIN_FONT_SOURCE");
        validate(&out).unwrap_or_else(|error| {
            panic!("READALL_BUILTIN_FONT_SOURCE is not a compatible Chinese TrueType font: {error}")
        });
        return;
    }

    let temp = out.with_extension("download");
    let _ = fs::remove_file(&temp);
    for url in FONT_URLS {
        if download(url, &temp) {
            match validate(&temp) {
                Ok(()) => {
                    fs::rename(&temp, &out).expect("install downloaded built-in font");
                    return;
                }
                Err(error) => {
                    println!("cargo:warning=discarding invalid built-in font download: {error}");
                    let _ = fs::remove_file(&temp);
                }
            }
        }
    }

    let _ = fs::remove_file(&temp);
    panic!(
        "cannot obtain the built-in Chinese font automatically. Network access is needed only when building from source after a clean; end users of the compiled ReadAll binary do not download fonts. Packagers may set READALL_BUILTIN_FONT_SOURCE to a local {FONT_NAME}."
    );
}

fn validate(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("not a regular file".into());
    }
    if metadata.len() != FONT_SIZE {
        return Err(format!(
            "unexpected byte size: got {}, expected {FONT_SIZE}",
            metadata.len()
        ));
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let font = Font::parse(&bytes, 0, FontLimits::default()).map_err(|error| error.to_string())?;
    for ch in REQUIRED_GLYPHS.chars().filter(|ch| !ch.is_whitespace()) {
        let glyph = font.glyph_index(ch).map_err(|error| error.to_string())?;
        if glyph == 0 {
            return Err(format!("required glyph U+{:04X} is missing", ch as u32));
        }
        font.glyph(glyph).map_err(|error| {
            format!(
                "required glyph U+{:04X} cannot be decoded by ReadAll: {error}",
                ch as u32
            )
        })?;
    }
    Ok(())
}

fn download(url: &str, output: &Path) -> bool {
    if run(
        "curl",
        &[
            "-L",
            "--fail",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "15",
            "--max-time",
            "180",
            "--output",
            output.to_string_lossy().as_ref(),
            url,
        ],
    ) {
        return true;
    }
    let _ = fs::remove_file(output);

    if run(
        "wget",
        &[
            "-q",
            "--timeout=30",
            "--tries=2",
            "-O",
            output.to_string_lossy().as_ref(),
            url,
        ],
    ) {
        return true;
    }
    let _ = fs::remove_file(output);

    let script = r#"
import sys, urllib.request
url, path = sys.argv[1], sys.argv[2]
with urllib.request.urlopen(url, timeout=60) as response, open(path, "wb") as output:
    while True:
        chunk = response.read(1024 * 1024)
        if not chunk:
            break
        output.write(chunk)
"#;
    run(
        "python3",
        &["-c", script, url, output.to_string_lossy().as_ref()],
    )
}

fn run(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .status()
        .is_ok_and(|status| status.success())
}

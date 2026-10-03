//! Developer diagnostic: reads fonts but never copies them into the repository.
use readall_font::{Font, FontLimits};
use std::{
    error::Error,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

fn inspect(path: &Path) -> Result<(), Box<dyn Error>> {
    let limits = FontLimits::default();
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err("expected a regular font file".into());
    }
    let mut bytes = Vec::new();
    file.take(limits.max_file_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    let font = Font::parse(&bytes, 0, limits)?;
    println!(
        "Font: {path:?}; glyphs={}; metrics={:?}",
        font.glyph_count(),
        font.metrics()
    );
    for ch in "AaéΩ中文阅读器😀".chars() {
        let index = font.glyph_index(ch)?;
        let glyph = font.glyph(index)?;
        println!(
            "  {ch:?}: glyph={index}, advance={}, contours={}, points={}{}",
            glyph.metrics.advance_width,
            glyph.outline.contour_count(),
            glyph.outline.points().len(),
            if index == 0 { " (MISSING)" } else { "" }
        );
    }
    Ok(())
}
fn discover(cjk_only: bool) {
    let mut pending: Vec<(PathBuf, usize)> = [
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "/system/fonts",
        "C:/Windows/Fonts",
    ]
    .into_iter()
    .map(|root| (root.into(), 0))
    .collect();
    let (mut visited, mut found) = (0, 0);
    while let Some((directory, depth)) = pending.pop() {
        visited += 1;
        if visited > 2048 || found >= 512 {
            break;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.take(2048).flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() && depth < 8 {
                pending.push((path, depth + 1));
            } else if kind.is_file()
                && (!cjk_only
                    || path.to_string_lossy().to_ascii_lowercase().contains("cjk")
                    || path.to_string_lossy().to_ascii_lowercase().contains("wqy")
                    || path
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains("wenquanyi")
                    || path
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains("sarasa")
                    || path
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains("sourcehan")
                    || path
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains("source-han")
                    || path
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains("notosanssc")
                    || path
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains("notoserifsc"))
                && path.extension().is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("ttf")
                        || ext.eq_ignore_ascii_case("ttc")
                        || ext.eq_ignore_ascii_case("otf")
                })
            {
                println!("{}", path.display());
                found += 1;
                if found >= 512 {
                    break;
                }
            }
        }
    }
    println!("Visible font files: {found} (bounded diagnostic, not a complete inventory)");
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        return Err("usage: cargo run -p readall-font --example inspect --offline -- <font.ttf | --discover | --discover-cjk>".into());
    }
    if args[0] == "--discover" || args[0] == "--discover-cjk" {
        discover(args[0] == "--discover-cjk");
        Ok(())
    } else {
        inspect(Path::new(&args[0]))
    }
}

//! Bounded fallback-font loading; bytes live at the reader entry point, not in self-referential caches.
use readall_core::read_bounded;
use readall_font::{Font, FontLimits};
use readall_platform::LocalFileSource;
use std::{
    collections::{BTreeSet, VecDeque},
    path::{Path, PathBuf},
};

pub(crate) struct FontBytes {
    pub bytes: Vec<u8>,
    pub face: u32,
}
pub(crate) fn load_fallbacks(explicit: &[PathBuf], system: bool) -> Vec<FontBytes> {
    if crate::loading::stage("查找回退字体").is_err() {
        return Vec::new();
    }
    let mut paths = explicit.to_vec();
    if system {
        if let Some(value) = std::env::var_os("READALL_FALLBACK_FONTS") {
            paths.extend(std::env::split_paths(&value));
        }
        paths.extend(system_candidates());
    }
    let mut seen = BTreeSet::new();
    let (mut output, mut total) = (Vec::new(), 0_usize);
    let path_count = paths.len().min(128);
    for (index, path) in paths.into_iter().take(128).enumerate() {
        if crate::loading::step("加载回退字体", index, path_count).is_err() {
            break;
        }
        if output.len() >= 12 {
            break;
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        let loaded = (|| -> Result<FontBytes, Box<dyn std::error::Error>> {
            let limit = (96 * 1024 * 1024_usize)
                .saturating_sub(total)
                .min(FontLimits::default().max_file_bytes);
            let bytes = read_bounded(&mut LocalFileSource::open(&path)?, limit)?;
            let font = Font::parse(&bytes, 0, FontLimits::default())?;
            if font.metrics().ascender <= 0 || font.metrics().descender > 0 {
                return Err("unsupported fallback metrics".into());
            }
            Ok(FontBytes { bytes, face: 0 })
        })();
        match loaded {
            Ok(font) => {
                total += font.bytes.len();
                output.push(font);
            }
            Err(error) if explicit.contains(&path) => {
                eprintln!("ReadAll: fallback font {:?} ignored: {error}", path)
            }
            Err(_) => {}
        }
    }
    output
}
fn system_candidates() -> Vec<PathBuf> {
    let mut pending = VecDeque::from([
        (PathBuf::from("/usr/share/fonts"), 0),
        (PathBuf::from("/usr/local/share/fonts"), 0),
    ]);
    if let Some(home) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        pending.push_back((home.join(".local/share/fonts"), 0));
    }
    let (mut output, mut entries, mut directories) = (Vec::new(), 0_usize, 0_usize);
    while let Some((directory, depth)) = pending.pop_front() {
        if crate::loading::check().is_err() {
            break;
        }
        directories += 1;
        if directories > 128 {
            break;
        }
        let Ok(reader) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in reader.flatten() {
            entries += 1;
            if entries > 8192 {
                break;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() && depth < 4 {
                pending.push_back((entry.path(), depth + 1));
            } else if kind.is_file() && candidate(&entry.path()) {
                output.push(entry.path());
            }
        }
        if entries > 8192 {
            break;
        }
    }
    output.sort_by_key(|path| {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        let family = if name.starts_with("dejavusans") {
            0
        } else if name.contains("symbols") {
            1
        } else if name.contains("cjk") {
            2
        } else {
            3
        };
        (family, name)
    });
    output
}
fn candidate(path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    (name.ends_with(".ttf") || name.ends_with(".ttc"))
        && (name.starts_with("dejavusans")
            || name.starts_with("notosans")
            || name.starts_with("notoserifcjk")
            || name.starts_with("symbola")
            || name.starts_with("liberationsans"))
        && !name.contains("condensed")
        && !name.contains("mono")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_candidates_are_restricted_to_font_files() {
        assert!(candidate(Path::new("DejaVuSans.ttf")));
        assert!(candidate(Path::new("NotoSansSymbols2-Regular.ttf")));
        assert!(!candidate(Path::new("secrets.txt")));
        assert!(!candidate(Path::new("DejaVuSansMono.ttf")));
    }
}

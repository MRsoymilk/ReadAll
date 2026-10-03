//! Bounded system-font selection for GUI-opened books.
use readall_core::read_bounded;
use readall_font::{Font, FontLimits};
use readall_platform::LocalFileSource;
use std::{
    collections::HashSet,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub(crate) fn find_for_text(text: &str) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("READALL_UI_FONT").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
        return Err("READALL_UI_FONT does not point to a regular font file".into());
    }
    let mut required = Vec::new();
    let mut seen = HashSet::new();
    for ch in text.chars().filter(|ch| !ch.is_whitespace()) {
        if seen.insert(ch) {
            required.push(ch);
            if required.len() >= 96 {
                break;
            }
        }
    }
    if seen.insert('A') {
        required.push('A');
    }

    let mut candidates = Vec::new();
    let mut paths_seen = HashSet::new();
    for path in [
        "/usr/share/fonts/wenquanyi/wqy-zenhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/sarasa-gothic/SarasaGothicSC-Regular.ttf",
        "/usr/share/fonts/sarasa/SarasaGothicSC-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/liberation-fonts/LiberationSans-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ] {
        push_candidate(PathBuf::from(path), &mut candidates, &mut paths_seen);
    }

    let mut roots = vec![
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
    ];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join(".local/share/fonts"));
        roots.push(home.join(".fonts"));
    }
    discover_fonts(roots, &mut candidates, &mut paths_seen);
    candidates.sort_by(|a, b| {
        font_priority(a)
            .cmp(&font_priority(b))
            .then_with(|| a.cmp(b))
    });

    let requires_han = required.iter().copied().any(is_han);
    let limits = FontLimits::default();
    let mut best: Option<(usize, usize, PathBuf)> = None;
    for path in candidates.into_iter().take(384) {
        let mut source = match LocalFileSource::open(&path) {
            Ok(source) => source,
            Err(_) => continue,
        };
        let bytes = match read_bounded(&mut source, limits.max_file_bytes) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let font = match Font::parse(&bytes, 0, limits) {
            Ok(font) => font,
            Err(_) => continue,
        };
        let (mut score, mut matched, mut han_matched) = (0_usize, 0_usize, 0_usize);
        for &ch in &required {
            if font.glyph_index(ch).is_ok_and(|glyph| glyph != 0) {
                matched += 1;
                if is_han(ch) {
                    han_matched += 1;
                    score += 10_000;
                } else if ch.is_ascii() {
                    score += 1;
                } else {
                    score += 100;
                }
            }
        }
        if requires_han && han_matched == 0 {
            continue;
        }
        if score > best.as_ref().map_or(0, |(score, _, _)| *score) {
            let complete = matched == required.len();
            best = Some((score, matched, path));
            if complete {
                break;
            }
        }
    }

    best.map(|(_, _, path)| path).ok_or_else(|| {
        if requires_han {
            "no supported system TrueType/TTC font contains Chinese glyphs; install a Chinese TrueType font such as WenQuanYi Zen Hei or Sarasa Gothic, or set READALL_UI_FONT"
                .into()
        } else {
            "no supported TrueType/TTC system font was found; install a static TrueType font".into()
        }
    })
}

fn push_candidate(path: PathBuf, out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    if path.is_file() && seen.insert(path.clone()) {
        out.push(path);
    }
}

fn discover_fonts(roots: Vec<PathBuf>, out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    let mut pending: Vec<_> = roots.into_iter().map(|root| (root, 0_usize)).collect();
    let mut visited = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        visited += 1;
        if visited > 2048 || out.len() >= 1024 {
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
                continue;
            }
            if kind.is_file() && supported_extension(&path) {
                push_candidate(path, out, seen);
                if out.len() >= 1024 {
                    return;
                }
            }
        }
    }
}

fn supported_extension(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("ttf") || extension.eq_ignore_ascii_case("ttc")
    })
}

fn is_han(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2EBEF
            | 0x30000..=0x323AF
    )
}

fn font_priority(path: &Path) -> u8 {
    let name = path.to_string_lossy().to_ascii_lowercase();
    if [
        "cjk",
        "sourcehan",
        "source-han",
        "wenquanyi",
        "wqy",
        "sarasa",
        "lxgw",
        "droidsansfallback",
        "uming",
        "ukai",
        "hanazono",
        "unifont",
    ]
    .iter()
    .any(|needle| name.contains(needle))
    {
        0
    } else if name.contains("noto") {
        1
    } else if name.contains("dejavu") || name.contains("liberation") {
        2
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_intentionally_narrow() {
        assert!(supported_extension(Path::new("a.ttf")));
        assert!(supported_extension(Path::new("a.TTC")));
        assert!(!supported_extension(Path::new("a.otf")));
        assert!(!supported_extension(Path::new("a.woff2")));
        assert!(
            font_priority(Path::new("/fonts/SarasaGothicSC-Regular.ttf"))
                < font_priority(Path::new("/fonts/DejaVuSans.ttf"))
        );
        assert!(
            font_priority(Path::new("/fonts/NotoSansCJK.ttc"))
                < font_priority(Path::new("/fonts/Plain.ttf"))
        );
        assert!(is_han('中'));
        assert!(is_han('阅'));
        assert!(!is_han('A'));
        assert!(!is_han('あ'));
    }
}

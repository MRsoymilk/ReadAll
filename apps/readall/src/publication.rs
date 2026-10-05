//! Format dispatch before the shared EPUB reading pipeline. MOBI normalization
//! stays on the existing reader worker, with progress/cancellation; no disk copies.
use readall_mobi::{MobiBook, MobiLimits, Stage};
use std::{error::Error, path::Path};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Epub,
    Mobi,
    Azw3,
}
impl Format {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Epub => "EPUB",
            Self::Mobi => "MOBI",
            Self::Azw3 => "AZW3/KF8",
        }
    }
}
pub(crate) fn path_format(path: &Path) -> Option<Format> {
    let ext = path.extension()?.to_str()?;
    if ext.eq_ignore_ascii_case("epub") {
        Some(Format::Epub)
    } else if ext.eq_ignore_ascii_case("azw3") {
        Some(Format::Azw3)
    } else if ["mobi", "azw", "prc"]
        .iter()
        .any(|kind| ext.eq_ignore_ascii_case(kind))
    {
        Some(Format::Mobi)
    } else {
        None
    }
}
pub(crate) struct Prepared {
    pub bytes: Vec<u8>,
    pub format: Format,
}
pub(crate) fn prepare(bytes: Vec<u8>, path: &Path) -> Result<Prepared> {
    if readall_mobi::is_mobi(&bytes) {
        crate::loading::stage("校验 MOBI 文件结构")?;
        let book = MobiBook::parse(&bytes, MobiLimits::default())?;
        let format = if book.metadata().version == 8 {
            Format::Azw3
        } else {
            Format::Mobi
        };
        let converted = book.to_epub_with_progress(|p| {
            let phase = match (format, p.stage) {
                (Format::Azw3, Stage::Decompress) => "解压 AZW3 正文",
                (_, Stage::Index) => "解析 KF8 章节与目录索引",
                (Format::Azw3, Stage::Markup) => "重建 AZW3 章节与链接",
                (Format::Azw3, Stage::Package) => "准备 AZW3 样式、图片与字体",
                (_, Stage::Decompress) => "解压 MOBI 正文",
                (_, Stage::Markup) => "整理 MOBI 章节与链接",
                (_, Stage::Package) => "准备 MOBI 图片资源",
            };
            crate::loading::step(phase, p.done, p.total).is_ok()
        })?;
        for warning in &converted.warnings {
            eprintln!("ReadAll: {warning}");
        }
        return Ok(Prepared {
            bytes: converted.epub,
            format,
        });
    }
    // Do not parse a random Palm database or disguised binary as ordinary text.
    if matches!(path_format(path), Some(Format::Mobi | Format::Azw3)) {
        return Err("invalid MOBI: expected PalmDB BOOKMOBI signature (not every PRC/AZW file is a MOBI book)".into());
    }
    Ok(Prepared {
        bytes,
        format: Format::Epub,
    })
}
pub(crate) fn open(args: &[std::ffi::OsString], output: &mut impl std::io::Write) -> Result<()> {
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    {
        if args.len() == 1 {
            return crate::native_epub::open_path(Path::new(&args[0]), output);
        }
        let mut options = args.to_vec();
        if !options.is_empty() && !options.iter().any(|s| s == "--font") {
            options.extend(["--font".into(), crate::ui::UiFont::builtin_label().into()]);
        }
        crate::native_epub::run(&options, output)
    }
    #[cfg(not(all(target_os = "linux", feature = "wayland")))]
    crate::native_epub::run(args, output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signatures_win_over_extensions_and_extensions_do_not_fake_support() {
        for name in ["book.mobi", "book.MOBI", "book.azw", "book.PRC"] {
            assert_eq!(path_format(Path::new(name)), Some(Format::Mobi));
        }
        assert_eq!(path_format(Path::new("book.azw3")), Some(Format::Azw3));
        assert_eq!(path_format(Path::new("book.AZW3")), Some(Format::Azw3));
        assert!(prepare(b"not KF8".to_vec(), Path::new("fake.azw3")).is_err());
        assert_eq!(path_format(Path::new("book.txt")), None);
        let source = crate::test_mobi::make_mobi("<html><body>AAAA</body></html>");
        let prepared = prepare(source, Path::new("wrong.epub")).unwrap();
        assert_eq!(prepared.format, Format::Mobi);
        let book = readall_epub::EpubBook::parse(&prepared.bytes, Default::default()).unwrap();
        assert_eq!(book.read_spine_text(0).unwrap(), "AAAA");
        assert!(prepare(b"not mobi".to_vec(), Path::new("book.mobi")).is_err());
    }
    #[test]
    fn normal_epub_bytes_and_identity_remain_unchanged() {
        let bytes = crate::test_epub::make_epub();
        let prepared = prepare(bytes.clone(), Path::new("book.epub")).unwrap();
        assert_eq!(prepared.format, Format::Epub);
        assert_eq!(prepared.bytes, bytes);
    }
}

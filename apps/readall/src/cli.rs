use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::PathBuf,
};

use readall_core::{
    Limits, TextDocument, TextLocator,
    preview::{PreviewConfig, PreviewLayout, diagnostic_cell_width},
    read_bounded,
};
use readall_epub::{EpubBook, EpubLimits};
use readall_platform::LocalFileSource;
use readall_render::{Color, DrawCommand, Rect, RenderLimits, Surface};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub fn run(args: Vec<OsString>, output: &mut impl Write) -> Result<()> {
    if args.is_empty() {
        return crate::home::run(output);
    }
    if matches!(args[0].to_str(), Some("--help" | "-h")) {
        writeln!(
            output,
            "ReadAll {} — native Rust reader foundations\n\nCommands:\n  readall inspect <book.txt>\n  readall epub-info <book.epub>\n  readall epub-text <book.epub> [--spine N]\n  readall render-epub <book.epub> <new-output.ppm> --font <font.ttf> [--spine N | --at EPUB_LOCATOR] [page options]\n  readall read <book.txt> [--columns N] [--rows N] [--page N | --at LOCATOR]\n  readall render-demo <new-output.ppm>\n  readall render-text <book.txt> <new-output.ppm> --font <font.ttf> [--face N] [--width N] [--height N] [--font-size N] [--margin N] [--page N | --at LOCATOR] [--missing error|replacement]\n  readall licenses\n  readall diagnostics\n\nPages are 1-based. Columns: 4..4096. Rows: 1..1024.\nNative Linux TXT window: readall open <book.txt> --font <font.ttf> [page options] [--progress on|off] [--state-dir DIR] [--display SOCKET] [--frames 1].\nNative Linux EPUB window: readall open-epub <book.epub> --font <font.ttf> [--spine N | --at EPUB_LOCATOR] [page options] [--progress on|off] [--state-dir DIR] [--data-dir DIR] [--display SOCKET] [--frames 1] (build with --features wayland).\nEPUB supports CSS text/block properties, PNG/JPEG/WebP/SVG, shaping/bidi, font fallback, static TrueType @font-face selection and epub-v1/v2/v3 progress. Code blocks preserve line breaks, spaces and tabs; legacy positions are migrated. Automatic syntax highlighting supports C/C++, Rust, Python, Shell, JavaScript/TypeScript and JSON; author multicolor styles remain unchanged. Full EPUB styling, WOFF/obfuscated fonts and PDF reading remain unimplemented; Windows/Android windows remain unimplemented.\nrender-demo writes a graphics calibration image, not an ebook page, and never overwrites an existing file.",
            env!("CARGO_PKG_VERSION")
        )?;
        writeln!(
            output,
            "\nReader tools:\n  readall search <book.epub> <query> [--case-sensitive]\n  readall annotations <book.epub> [--data-dir DIR]\n  readall bookmark <book.epub> <EPUB_LOCATOR> [label]\n  readall note <book.epub> <EPUB_LOCATOR> [text]\n  readall highlight <book.epub> <EPUB_LOCATOR> <end-utf8-offset> [text]\n  readall annotation-remove <book.epub> <ID>\n  readall settings [theme|size|margin|line-spacing|page-mode VALUE] [--data-dir DIR]\nAll annotation commands accept --data-dir DIR. EPUB render/open accept repeated --fallback-font FILE.\nReader shortcuts: F2 search, F3 annotations, F4 bookmark, F5 settings, F6 theme, F7 note, F8 highlight, F9 text selection priority; Ctrl+C/Ctrl+V clipboard. EPUB body text is draggable by default, without F9; blank clicks do not turn pages. Releasing a selection shows Copy/Highlight/Note/Cancel actions. Links and images activate on release only when not dragging. F5 Page mode: slide (horizontal), book (2D paper curl), scroll (continuous vertical); settings page-mode slide|book|scroll persists it. Use wheel, touchpad, PageUp/PageDown, arrows or toolbar buttons; right-button drag navigates, left-button drag still selects. Esc stops motion, clears a selection or dismisses the active tool first. Native text input uses the compositor XKB keymap; IME composition is not integrated yet."
        )?;
        writeln!(
            output,
            "\nMOBI reading (unencrypted MOBI6/7):\n  readall <book.mobi>\n  readall open-mobi <book.mobi> [reader options]\n  readall mobi-info <book.mobi>\n  readall mobi-text <book.mobi> [--spine N]\n  readall render-mobi <book.mobi> <new-output.ppm> --font <font.ttf> [page options]\nSearch, annotations and all three page modes also accept MOBI. UTF-8/Windows-1252 and uncompressed/PalmDOC/HUFF-CDIC are supported. Dual files use their legacy MOBI section; standalone reflowable AZW3/KF8 is supported; KFX, fixed-layout KF8 and DRM remain unsupported.\nAZW3 commands: readall <book.azw3>, open-azw3, azw3-info, azw3-text and render-azw3 (same options as MOBI)."
        )?;
        return Ok(());
    }
    if args.len() == 1 && crate::publication::path_format(std::path::Path::new(&args[0])).is_some()
    {
        return crate::publication::open(&args, output);
    }
    let command = args[0].to_str().ok_or("command must be UTF-8")?;
    if matches!(command, "mobi-info" | "azw3-info") {
        return mobi_info(&args[1..], output);
    }
    if matches!(
        command,
        "search"
            | "annotations"
            | "bookmark"
            | "note"
            | "highlight"
            | "annotation-remove"
            | "settings"
    ) {
        return crate::reader_cli::run(command, &args[1..], output);
    }
    if command == "licenses" {
        if args.len() != 1 {
            return Err("licenses takes no arguments".into());
        }
        writeln!(
            output,
            "ReadAll bundled font: LXGW WenKai Lite Regular\nSource commit: {}\n\n{}",
            option_env!("READALL_BUILTIN_FONT_COMMIT").unwrap_or("not bundled in this build"),
            include_str!("../../../licenses/LXGW_WenKai_Lite_OFL.txt")
        )?;
        writeln!(
            output,
            "\n{}",
            include_str!("../../../licenses/foliate-js-MIT.txt")
        )?;
        return Ok(());
    }
    if command == "diagnostics" {
        if args.len() != 1 {
            return Err("diagnostics takes no arguments".into());
        }
        let path = crate::diagnostics::diagnostic_path();
        writeln!(output, "ReadAll error log: {}", path.display())?;
        match fs::read_to_string(&path) {
            Ok(log) => write!(output, "{log}")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                writeln!(output, "No diagnostic log has been written yet.")?;
            }
            Err(error) => return Err(error.into()),
        }
        return Ok(());
    }
    if command == "open" {
        if args.get(1).is_some_and(|path| {
            crate::publication::path_format(std::path::Path::new(path)).is_some()
        }) {
            return crate::publication::open(&args[1..], output);
        }
        return crate::native::run(&args[1..], output);
    }
    if matches!(command, "open-epub" | "open-mobi" | "open-azw3") {
        return crate::publication::open(&args[1..], output);
    }
    if command == "render-text" {
        return crate::text_page::run(&args[1..], output);
    }
    if matches!(command, "render-epub" | "render-mobi" | "render-azw3") {
        return crate::epub_page::run(&args[1..], output);
    }
    if command == "epub-info" {
        return epub_info(&args[1..], output);
    }
    if matches!(command, "epub-text" | "mobi-text" | "azw3-text") {
        return epub_text(&args[1..], output);
    }
    if !matches!(command, "inspect" | "read" | "render-demo") {
        return Err("unknown command; use --help".into());
    }
    if args.len() < 2 {
        return Err("missing input/output path; use --help".into());
    }
    let path = PathBuf::from(&args[1]);
    if command == "render-demo" {
        if args.len() != 2 {
            return Err("render-demo expects exactly one output path".into());
        }
        render_demo(path, output)?;
        return Ok(());
    }
    if command == "inspect" && args.len() != 2 {
        return Err("inspect expects exactly one document path".into());
    }
    // Validate options before opening an input file.
    let options = if command == "read" {
        Some(ReadOptions::parse(&args[2..])?)
    } else {
        None
    };
    let mut source = LocalFileSource::open(path)?;
    let document = TextDocument::open(&mut source, Limits::default())?;
    if let Some(options) = options {
        read_page(&document, options, output)?;
    } else {
        writeln!(
            output,
            "Format: TXT\nEncoding: {:?}\nDocument: {}\nUTF-8 bytes: {}\nUnicode scalars: {}\nStart locator: {}",
            document.encoding(),
            document.id(),
            document.text().len(),
            document.text().chars().count(),
            document.locator(0)?
        )?;
    }
    Ok(())
}

#[derive(Debug)]
struct ReadOptions {
    config: PreviewConfig,
    page: Option<usize>,
    at: Option<TextLocator>,
}

impl ReadOptions {
    fn parse(args: &[OsString]) -> Result<Self> {
        if !args.len().is_multiple_of(2) {
            return Err("each reading option requires a value".into());
        }
        let (mut columns, mut rows, mut page, mut at) = (None, None, None, None);
        for pair in args.chunks_exact(2) {
            match pair[0].to_str() {
                Some("--columns") if columns.is_none() => columns = Some(number(&pair[1])?),
                Some("--rows") if rows.is_none() => rows = Some(number(&pair[1])?),
                Some("--page") if page.is_none() => {
                    page = Some(
                        number(&pair[1])?
                            .checked_sub(1)
                            .ok_or("page must be at least 1")?,
                    )
                }
                Some("--at") if at.is_none() => {
                    at = Some(pair[1].to_str().ok_or("locator must be UTF-8")?.parse()?)
                }
                _ => return Err("unknown or duplicate reading option".into()),
            }
        }
        if page.is_some() && at.is_some() {
            return Err("--page and --at cannot be used together".into());
        }
        let defaults = PreviewConfig::default();
        let config = PreviewConfig {
            columns: columns.unwrap_or(defaults.columns),
            rows: rows.unwrap_or(defaults.rows),
            ..defaults
        };
        if !(4..=4096).contains(&config.columns) || !(1..=1024).contains(&config.rows) {
            return Err("columns must be 4..4096 and rows 1..1024".into());
        }
        Ok(Self { config, page, at })
    }
}

fn number(value: &OsStr) -> Result<usize> {
    let text = value.to_str().ok_or("expected an unsigned integer")?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("expected an unsigned integer".into());
    }
    Ok(text.parse()?)
}

fn read_page(document: &TextDocument, options: ReadOptions, output: &mut impl Write) -> Result<()> {
    let layout = PreviewLayout::build(document, options.config)?;
    let page = if let Some(locator) = options.at {
        layout.page_for_locator(document, &locator)?
    } else {
        options.page.unwrap_or(0)
    };
    let lines = layout.page(page).ok_or("page is outside this document")?;
    writeln!(
        output,
        "Diagnostic preview (approximate cell widths; no font shaping)\nPage: {}/{}\nStart locator: {}\n",
        page + 1,
        layout.page_count(),
        document.locator(lines[0].text_range.start)?
    )?;
    for line in lines {
        let mut column = 0;
        for ch in document.text()[line.text_range.clone()].chars() {
            let advance = diagnostic_cell_width(ch, column);
            if ch == '\t' {
                for _ in 0..advance {
                    output.write_all(b" ")?;
                }
            } else {
                write!(output, "{ch}")?;
            }
            column += advance;
        }
        writeln!(output)?;
    }
    Ok(())
}

fn mobi_info(args: &[OsString], output: &mut impl Write) -> Result<()> {
    if args.len() != 1 {
        return Err("mobi-info expects exactly one MOBI path".into());
    }
    let limits = readall_mobi::MobiLimits::default();
    let bytes = read_bounded(
        &mut LocalFileSource::open(PathBuf::from(&args[0]))?,
        limits.max_file_bytes,
    )?;
    let book = readall_mobi::MobiBook::parse(&bytes, limits)?;
    let meta = book.metadata();
    writeln!(
        output,
        "Format: {}\nTitle: {}\nCreator: {}\nLanguage: {}\nEncoding: {}\nCompression: {}\nText records: {}\nText bytes: {}\nDual MOBI/KF8: {}",
        if meta.version == 8 {
            "AZW3/KF8".to_owned()
        } else {
            format!("MOBI{}", meta.version)
        },
        meta.title,
        meta.author.as_deref().unwrap_or("(unknown)"),
        meta.language.as_deref().unwrap_or("(unknown)"),
        meta.encoding,
        meta.compression,
        meta.text_records,
        meta.text_bytes,
        meta.dual_format
    )?;
    Ok(())
}

fn epub_info(args: &[OsString], output: &mut impl Write) -> Result<()> {
    if args.len() != 1 {
        return Err("epub-info expects exactly one EPUB path".into());
    }
    let limits = EpubLimits::default();
    let mut source = LocalFileSource::open(PathBuf::from(&args[0]))?;
    let bytes = read_bounded(&mut source, limits.zip.max_archive_bytes)?;
    let prepared = crate::publication::prepare(bytes, std::path::Path::new(&args[0]))?;
    let book = EpubBook::parse(&prepared.bytes, limits)?;
    write_epub_info(&book, prepared.format, output)
}

fn write_epub_info(
    book: &EpubBook<'_>,
    format: crate::publication::Format,
    output: &mut impl Write,
) -> Result<()> {
    writeln!(
        output,
        "Format: {}\nTitle: {}\nCreator: {}\nLanguage: {}\nPackage: {}\nManifest items: {}\nSpine items: {}",
        format.label(),
        book.title().unwrap_or("(untitled)"),
        book.creator().unwrap_or("(unknown)"),
        book.language().unwrap_or("(unknown)"),
        book.package_path(),
        book.manifest().len(),
        book.spine().len()
    )?;
    for (index, spine) in book.spine().iter().take(32).enumerate() {
        let item = book
            .manifest()
            .get(spine.manifest_index())
            .ok_or("invalid resolved EPUB spine")?;
        writeln!(
            output,
            "Spine {}: {} [{}]{}",
            index + 1,
            item.path(),
            item.media_type(),
            if spine.linear() { "" } else { " (non-linear)" }
        )?;
    }
    if book.spine().len() > 32 {
        writeln!(
            output,
            "... {} additional spine items omitted from diagnostic output",
            book.spine().len() - 32
        )?;
    }
    match book.navigation() {
        Ok(navigation) if navigation.is_empty() => {
            writeln!(output, "Navigation: none")?;
        }
        Ok(navigation) => {
            writeln!(output, "Navigation entries: {}", navigation.len())?;
            for entry in navigation.iter().take(32) {
                let indent = "  ".repeat(entry.depth().min(8));
                let fragment = entry
                    .fragment()
                    .map(|fragment| format!("#{fragment}"))
                    .unwrap_or_default();
                writeln!(
                    output,
                    "Nav: {indent}{} -> spine {}{}",
                    entry.label(),
                    entry.spine_index() + 1,
                    fragment
                )?;
            }
            if navigation.len() > 32 {
                writeln!(
                    output,
                    "... {} additional navigation entries omitted from diagnostic output",
                    navigation.len() - 32
                )?;
            }
        }
        Err(error) => {
            writeln!(output, "Navigation: unreadable ({error})")?;
        }
    }
    Ok(())
}

fn epub_text(args: &[OsString], output: &mut impl Write) -> Result<()> {
    if args.is_empty() || args.len() > 3 {
        return Err("epub-text expects <book.epub> [--spine N]".into());
    }
    let spine = if args.len() == 1 {
        0
    } else if args.len() == 3 && args[1] == "--spine" {
        number(&args[2])?
            .checked_sub(1)
            .ok_or("spine number must be at least 1")?
    } else {
        return Err("epub-text expects <book.epub> [--spine N]".into());
    };
    let limits = EpubLimits::default();
    let mut source = LocalFileSource::open(PathBuf::from(&args[0]))?;
    let bytes = read_bounded(&mut source, limits.zip.max_archive_bytes)?;
    let prepared = crate::publication::prepare(bytes, std::path::Path::new(&args[0]))?;
    let book = EpubBook::parse(&prepared.bytes, limits)?;
    let item = book
        .spine_item(spine)
        .ok_or("spine number is outside this EPUB")?;
    let text = book.read_spine_text(spine)?;
    writeln!(
        output,
        "{} spine text (XHTML subset; CSS/images not rendered)\nTitle: {}\nSpine: {}/{}\nResource: {}\n\n{}",
        prepared.format.label(),
        book.title().unwrap_or("(untitled)"),
        spine + 1,
        book.spine().len(),
        item.path(),
        text
    )?;
    Ok(())
}

fn render_demo(path: PathBuf, output: &mut impl Write) -> Result<()> {
    let mut image = Surface::new(640, 360, RenderLimits::default())?;
    let fill = |rect, color| DrawCommand::FillRect { rect, color };
    image.draw(&[
        fill(Rect::new(0, 0, 640, 360), Color::rgba(238, 238, 238, 255)),
        fill(Rect::new(24, 24, 280, 312), Color::WHITE),
        DrawCommand::PushClip(Rect::new(40, 40, 248, 280)),
        fill(Rect::new(-20, 64, 330, 80), Color::rgba(28, 76, 120, 255)),
        fill(Rect::new(80, 100, 240, 160), Color::rgba(240, 130, 40, 160)),
        DrawCommand::PopClip,
        fill(Rect::new(336, 24, 280, 312), Color::WHITE),
        fill(Rect::new(356, 64, 160, 160), Color::rgba(220, 40, 40, 180)),
        fill(Rect::new(416, 120, 160, 160), Color::rgba(40, 80, 220, 150)),
    ])?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let mut writer = BufWriter::new(file);
    image.write_ppm(&mut writer)?;
    writer.flush()?;
    writeln!(
        output,
        "Wrote {}x{} graphics calibration image: {:?}",
        image.width(),
        image.height(),
        path
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_epub;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn help_does_not_claim_gui_or_pdf_support() {
        let mut output = Vec::new();
        run(vec![OsString::from("--help")], &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("inspect"));
        assert!(text.contains("Full EPUB styling"));
    }

    #[test]
    fn bundled_font_license_is_user_visible() {
        let mut output = Vec::new();
        run(args(&["licenses"]), &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("LXGW WenKai Lite Regular"));
        assert!(text.contains("SIL OPEN FONT LICENSE Version 1.1"));
        assert!(run(args(&["licenses", "extra"]), &mut Vec::new()).is_err());
    }

    #[test]
    fn diagnostics_command_reports_log_path_even_before_a_failure() {
        let mut output = Vec::new();
        run(args(&["diagnostics"]), &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("ReadAll error log:"));
        assert!(text.contains("readall-error.log"));
        assert!(run(args(&["diagnostics", "extra"]), &mut Vec::new()).is_err());
    }

    #[test]
    fn epub_info_reports_epub3_navigation_targets() {
        let bytes = test_epub::make_epub_with_navigation();
        let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
        let mut output = Vec::new();
        write_epub_info(&book, crate::publication::Format::Epub, &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("Navigation entries: 3"));
        assert!(text.contains("正式目录第一章 -> spine 1#intro"));
        assert!(text.contains("  第一章详细部分 -> spine 1#details"));
        assert!(text.contains("正式目录第二章 -> spine 2#deep"));
    }

    #[test]
    fn invalid_arguments_and_unknown_commands_are_rejected() {
        assert!(run(args(&["inspect"]), &mut Vec::new()).is_err());
        assert!(run(args(&["unknown", "book.txt"]), &mut Vec::new()).is_err());
    }

    #[test]
    fn reading_options_reject_duplicates_missing_values_and_overflows() {
        for values in [
            vec!["--page", "0"],
            vec!["--page", "1", "--page", "2"],
            vec!["--rows"],
            vec!["--rows", "-1"],
            vec!["--rows", "0"],
            vec!["--columns", "3"],
            vec!["--columns", "18446744073709551616"],
            vec!["--unknown", "2"],
        ] {
            assert!(ReadOptions::parse(&args(&values)).is_err(), "{values:?}");
        }
    }

    #[test]
    fn page_and_locator_are_mutually_exclusive() {
        let document = TextDocument::from_bytes(b"abc", Limits::default()).unwrap();
        assert!(
            ReadOptions::parse(&args(&[
                "--page",
                "1",
                "--at",
                &document.locator(0).unwrap().to_string()
            ]))
            .is_err()
        );
    }

    #[test]
    fn reading_emits_text_and_a_restorable_locator() {
        let document = TextDocument::from_bytes(b"abcdefgh", Limits::default()).unwrap();
        let mut output = Vec::new();
        read_page(
            &document,
            ReadOptions::parse(&args(&["--columns", "4", "--rows", "1", "--page", "2"])).unwrap(),
            &mut output,
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("Page: 2/2"));
        assert!(text.ends_with("efgh\n"));
        let locator = text
            .lines()
            .find_map(|line| line.strip_prefix("Start locator: "))
            .unwrap();
        assert_eq!(document.restore(&locator.parse().unwrap()).unwrap(), 4);
        let mut wide = Vec::new();
        read_page(
            &document,
            ReadOptions::parse(&args(&["--columns", "8", "--rows", "1", "--at", locator])).unwrap(),
            &mut wide,
        )
        .unwrap();
        assert!(String::from_utf8(wide).unwrap().ends_with("abcdefgh\n"));
    }

    #[test]
    fn out_of_range_pages_fail_instead_of_silently_clamping() {
        let document = TextDocument::from_bytes(b"abc", Limits::default()).unwrap();
        assert!(
            read_page(
                &document,
                ReadOptions::parse(&args(&["--page", "99"])).unwrap(),
                &mut Vec::new()
            )
            .is_err()
        );
    }
}

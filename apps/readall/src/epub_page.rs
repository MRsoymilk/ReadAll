//! Diagnostic EPUB spine renderer: XHTML subset -> canonical text -> existing TXT page engine.
use crate::text_page::{Options, render};
use readall_core::{Limits, TextDocument, read_bounded};
use readall_epub::{EpubBook, EpubLimits};
use readall_font::{Font, FontLimits};
use readall_platform::LocalFileSource;
use std::{
    error::Error,
    ffi::OsString,
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::PathBuf,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub(crate) fn run(args: &[OsString], output: &mut impl Write) -> Result<()> {
    if args.len() < 2 {
        return Err(
            "render-epub expects <book.epub> <new-output.ppm> --font <font.ttf> [--spine N] [page options]"
                .into(),
        );
    }
    if (args.len() - 2) % 2 != 0 {
        return Err("each render-epub option requires a value".into());
    }

    let mut spine = None;
    let mut page_args = Vec::new();
    for pair in args[2..].chunks_exact(2) {
        if pair[0] == "--spine" {
            if spine.is_some() {
                return Err("duplicate --spine".into());
            }
            spine = Some(
                number(&pair[1])?
                    .checked_sub(1)
                    .ok_or("spine number must be at least 1")?,
            );
        } else {
            page_args.extend_from_slice(pair);
        }
    }
    let spine = spine.unwrap_or(0);
    let options = Options::parse(&page_args)?;

    let epub_limits = EpubLimits::default();
    let mut source = LocalFileSource::open(PathBuf::from(&args[0]))?;
    let bytes = read_bounded(&mut source, epub_limits.zip.max_archive_bytes)?;
    let book = EpubBook::parse(&bytes, epub_limits)?;
    let item = book
        .spine_item(spine)
        .ok_or("spine number is outside this EPUB")?;
    let text = book.read_spine_text(spine)?;
    if text.is_empty() {
        return Err(
            "selected EPUB spine contains no readable text in the current XHTML subset".into(),
        );
    }
    let document = TextDocument::from_bytes(text.as_bytes(), Limits::default())?;

    let font_limits = FontLimits::default();
    let font_bytes = read_bounded(
        &mut LocalFileSource::open(&options.font)?,
        font_limits.max_file_bytes,
    )?;
    let font = Font::parse(&font_bytes, options.face, font_limits)?;
    let rendered = render(&document, &font, &options)?;

    let path = PathBuf::from(&args[1]);
    let mut writer = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?,
    );
    rendered.surface.write_ppm(&mut writer)?;
    writer.flush()?;

    writeln!(
        output,
        "Rendered EPUB spine page (XHTML text subset; CSS/images not rendered)\nTitle: {}\nSpine: {}/{}\nResource: {}\nPage: {}/{}\nChapter locator: {}\nImage: {}x{}\nMissing characters: {}\nOutput: {:?}",
        book.title().unwrap_or("(untitled)"),
        spine + 1,
        book.spine().len(),
        item.path(),
        rendered.page + 1,
        rendered.pages,
        rendered.locator,
        rendered.surface.width(),
        rendered.surface.height(),
        rendered.missing.len(),
        path
    )?;
    Ok(())
}

fn number(value: &OsString) -> Result<usize> {
    let text = value.to_str().ok_or("expected an unsigned integer")?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("expected an unsigned integer".into());
    }
    Ok(text.parse()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_spine_options_fail_before_opening_files() {
        for values in [
            vec!["book.epub", "out.ppm", "--spine", "0"],
            vec!["book.epub", "out.ppm", "--spine", "1", "--spine", "2"],
            vec!["book.epub", "out.ppm", "--spine"],
        ] {
            let args: Vec<OsString> = values.into_iter().map(Into::into).collect();
            assert!(run(&args, &mut Vec::new()).is_err());
        }
    }
}

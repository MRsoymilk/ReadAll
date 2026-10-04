//! Diagnostic EPUB renderer using the same styled pagination as the native reader.
use crate::{
    epub_flow::{Chapter, EpubRenderer},
    text_page::Options,
};
use readall_core::read_bounded;
use readall_epub::{EpubBook, EpubLimits, EpubLocator};
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
    if !(args.len() - 2).is_multiple_of(2) {
        return Err("each render-epub option requires a value".into());
    }

    let mut spine = None;
    let mut fallback_paths = Vec::new();
    let mut epub_at: Option<EpubLocator> = None;
    let mut page_args = Vec::new();
    for pair in args[2..].chunks_exact(2) {
        if pair[0] == "--fallback-font" {
            if fallback_paths.len() >= 12 {
                return Err("at most 12 fallback fonts may be supplied".into());
            }
            fallback_paths.push(PathBuf::from(&pair[1]));
        } else if pair[0] == "--spine" {
            if spine.is_some() {
                return Err("duplicate --spine".into());
            }
            spine = Some(
                number(&pair[1])?
                    .checked_sub(1)
                    .ok_or("spine number must be at least 1")?,
            );
        } else if pair[0] == "--at" {
            if epub_at.is_some() {
                return Err("duplicate --at".into());
            }
            epub_at = Some(pair[1].to_str().ok_or("locator must be UTF-8")?.parse()?);
        } else {
            page_args.extend_from_slice(pair);
        }
    }
    let mut options = Options::parse(&page_args)?;
    if epub_at.is_some() && (spine.is_some() || options.page.is_some()) {
        return Err("EPUB --at cannot be combined with --spine or --page".into());
    }

    let epub_limits = EpubLimits::default();
    let mut source = LocalFileSource::open(PathBuf::from(&args[0]))?;
    let bytes = read_bounded(&mut source, epub_limits.zip.max_archive_bytes)?;
    let prepared = crate::publication::prepare(bytes, std::path::Path::new(&args[0]))?;
    let book = EpubBook::parse(&prepared.bytes, epub_limits)?;
    let (spine, restored_offset) = if let Some(locator) = &epub_at {
        let (spine, offset) = book.restore(locator)?;
        (spine, Some(offset))
    } else {
        (spine.unwrap_or(0), None)
    };
    let item = book
        .spine_item(spine)
        .ok_or("spine number is outside this EPUB")?;
    let document = Chapter::load(&book, spine)?;
    if !document.is_readable() {
        return Err("selected EPUB spine contains no readable text or images".into());
    }
    if let Some(offset) = restored_offset {
        options.at = Some(document.locator(offset)?);
    }

    let font_limits = FontLimits::default();
    let font_bytes = read_bounded(
        &mut LocalFileSource::open(&options.font)?,
        font_limits.max_file_bytes,
    )?;
    let font = Font::parse(&font_bytes, options.face, font_limits)?;
    let fallback_sources = crate::fonts::load_fallbacks(&fallback_paths, false);
    let fallback_faces: Vec<_> = fallback_sources
        .iter()
        .filter_map(|source| Font::parse(&source.bytes, source.face, FontLimits::default()).ok())
        .collect();
    let fallback_refs: Vec<_> = fallback_faces.iter().collect();
    let rendered = EpubRenderer::new(&font, options.size, options.allow_missing)?
        .with_fallbacks(&fallback_refs)
        .render_with_image(
            &document,
            &options,
            epub_at.as_ref().and_then(EpubLocator::image_index),
        )?;
    let chapter_offset = document.restore(&rendered.locator)?;
    let epub_locator = match rendered.image_index {
        Some(image) => book.image_locator(spine, image)?,
        None => book.locator(spine, chapter_offset)?,
    };

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
        "Rendered {} spine page (CSS text/block subset + PNG/JPEG/WebP/SVG; shaping/bidi/font fallback)\nTitle: {}\nSpine: {}/{}\nResource: {}\nPage: {}/{}\nEPUB locator: {}\nChapter locator: {}\nImage: {}x{}\nMissing characters: {}\nOutput: {:?}",
        prepared.format.label(),
        book.title().unwrap_or("(untitled)"),
        spine + 1,
        book.spine().len(),
        item.path(),
        rendered.page + 1,
        rendered.pages,
        epub_locator,
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
            vec!["book.epub", "out.ppm", "--at", "not-a-locator"],
        ] {
            let args: Vec<OsString> = values.into_iter().map(Into::into).collect();
            assert!(run(&args, &mut Vec::new()).is_err());
        }
    }
}

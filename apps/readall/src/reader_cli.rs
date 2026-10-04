//! Search and persistent annotations share the native reader's data model.
use crate::reader_data::{Kind, PageMode, Store, Theme};
use readall_core::read_bounded;
use readall_epub::{EpubBook, EpubLimits, SearchLimits};
use readall_platform::LocalFileSource;
use std::{ffi::OsString, io::Write, path::PathBuf};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub(crate) fn run(command: &str, args: &[OsString], output: &mut impl Write) -> Result<()> {
    let mut values = Vec::new();
    let mut root = None;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--data-dir" {
            if root.is_some() || index + 1 >= args.len() {
                return Err("--data-dir requires one unique path".into());
            }
            root = Some(PathBuf::from(&args[index + 1]));
            index += 2;
        } else {
            values.push(args[index].clone());
            index += 1;
        }
    }
    let value = |index: usize| -> Result<&str> {
        values
            .get(index)
            .and_then(|v| v.to_str())
            .ok_or_else(|| "missing UTF-8 argument; see --help".into())
    };
    let store = || -> Result<Store> {
        Ok(match &root {
            Some(path) => Store::new(path.clone()),
            None => Store::from_environment()?,
        })
    };
    if command == "settings" {
        if !values.is_empty() && values.len() != 2 {
            return Err(
                "settings [theme|size|margin|line-spacing|page-mode VALUE] [--data-dir DIR]".into(),
            );
        }
        let store = store()?;
        let mut settings = store.settings()?;
        if values.len() == 2 {
            match value(0)? {
                "theme" => settings.theme = Theme::parse(value(1)?)?,
                "size" => settings.size = value(1)?.parse()?,
                "margin" => settings.margin = value(1)?.parse()?,
                "line-spacing" => settings.line_spacing = value(1)?.parse()?,
                "page-mode" => settings.page_mode = PageMode::parse(value(1)?)?,
                _ => return Err("unknown reader setting".into()),
            }
            store.save_settings(settings)?;
        }
        writeln!(
            output,
            "theme={}\nsize={}\nmargin={}\nline-spacing={}\npage-mode={}\ndata={}",
            settings.theme.name(),
            settings.size,
            settings.margin,
            settings.line_spacing,
            settings.page_mode.name(),
            store.root().display()
        )?;
        return Ok(());
    }
    let expected = match command {
        "search" => 2..=3,
        "annotations" => 1..=1,
        "bookmark" | "note" => 2..=3,
        "highlight" => 3..=4,
        "annotation-remove" => 2..=2,
        _ => return Err("unknown reader tool".into()),
    };
    if !expected.contains(&values.len()) {
        return Err("invalid reader tool arguments; use --help".into());
    }
    let limits = EpubLimits::default();
    let bytes = read_bounded(
        &mut LocalFileSource::open(PathBuf::from(&values[0]))?,
        limits.zip.max_archive_bytes,
    )?;
    let book = EpubBook::parse(&bytes, limits)?;
    match command {
        "search" => {
            let case = values.len() == 3;
            if case && value(2)? != "--case-sensitive" {
                return Err("search optional argument is --case-sensitive".into());
            }
            let report = book.search(value(1)?, case, SearchLimits::default())?;
            for (index, hit) in report.hits.iter().enumerate() {
                writeln!(
                    output,
                    "{}\t{}\t{}\t{}",
                    index + 1,
                    hit.locator,
                    hit.end_offset,
                    hit.excerpt
                )?;
            }
            writeln!(
                output,
                "Matches: {}; chapters scanned: {}; skipped: {}; truncated: {}",
                report.hits.len(),
                report.chapters_scanned,
                report.skipped_chapters,
                report.truncated
            )?;
        }
        "annotations" => {
            for row in store()?.annotations(&book)? {
                writeln!(
                    output,
                    "{}\t{}\t{}\t{}\t{}",
                    row.id,
                    row.kind.name(),
                    row.locator,
                    row.end.map_or_else(|| "-".into(), |n| n.to_string()),
                    row.text.replace('\n', "\\n")
                )?;
            }
        }
        "annotation-remove" => {
            let removed = store()?.remove(&book, value(1)?.parse()?)?;
            writeln!(output, "Removed: {removed}")?;
        }
        _ => {
            let kind = Kind::parse(command)?;
            let locator = value(1)?.parse()?;
            let (end, label) = if kind == Kind::Highlight {
                (
                    Some(value(2)?.parse()?),
                    if values.len() > 3 { value(3)? } else { "" },
                )
            } else {
                (None, if values.len() > 2 { value(2)? } else { "" })
            };
            let id = store()?.add(&book, kind, locator, end, label.to_owned())?;
            writeln!(output, "Saved {} ID: {id}", kind.name())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_epub;
    #[test]
    fn search_keeps_utf8_offsets_ignores_hidden_text_and_reports_limits() {
        let bytes = test_epub::make_epub_with_resources(
            &[
                "<html><body><p>Hello 中文 hello</p><p style='display:none'>hello</p></body></html>",
                "<html><body>HELLO 中文</body></html>",
            ],
            vec![],
        );
        let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
        let report = book
            .search("hello", false, SearchLimits::default())
            .unwrap();
        assert_eq!(report.hits.len(), 3);
        for hit in &report.hits {
            book.restore(&hit.locator).unwrap();
        }
        assert_eq!(
            book.search("中文", true, SearchLimits::default())
                .unwrap()
                .hits
                .len(),
            2
        );
        assert_eq!(
            book.search("Hello", true, SearchLimits::default())
                .unwrap()
                .hits
                .len(),
            1
        );
        let report = book
            .search(
                "hello",
                false,
                SearchLimits {
                    max_hits: 1,
                    ..SearchLimits::default()
                },
            )
            .unwrap();
        assert!(report.truncated);
        assert_eq!(report.hits.len(), 1);
        assert!(book.search("", false, SearchLimits::default()).is_err());
    }
}

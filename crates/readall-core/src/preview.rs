//! Diagnostic cell-based pagination, NOT the production font/shaping engine.
//! ASCII takes one cell, non-ASCII scalars two, tabs advance to four-cell stops.
//! This intentionally does not claim grapheme, bidi, word-breaking, or UAX conformance.

use std::ops::Range;

use crate::{DocumentId, Error, TextDocument, TextLocator};

#[derive(Debug, Clone, Copy)]
pub struct PreviewConfig {
    pub columns: usize,
    pub rows: usize,
    pub max_lines: usize,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            columns: 80,
            rows: 24,
            max_lines: 200_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewLine {
    pub text_range: Range<usize>,
    /// Includes the LF consumed after a hard line break, if any.
    pub next_offset: usize,
}

#[derive(Debug)]
pub struct PreviewLayout {
    document_id: DocumentId,
    lines: Vec<PreviewLine>,
    rows: usize,
}

pub fn diagnostic_cell_width(ch: char, column: usize) -> usize {
    if ch == '\t' {
        4 - column % 4
    } else if ch.is_ascii() {
        1
    } else {
        2
    }
}

impl PreviewLayout {
    pub fn build(document: &TextDocument, config: PreviewConfig) -> Result<Self, Error> {
        if !(4..=4096).contains(&config.columns)
            || !(1..=1024).contains(&config.rows)
            || !(1..=1_000_000).contains(&config.max_lines)
        {
            return Err(Error::InvalidLayout(
                "columns must be 4..4096, rows 1..1024, max_lines 1..1000000",
            ));
        }
        let mut lines = Vec::new();
        let mut start = 0;
        let mut column = 0;
        for (offset, ch) in document.text().char_indices() {
            if ch == '\n' {
                push_line(&mut lines, start..offset, offset + 1, config.max_lines)?;
                start = offset + 1;
                column = 0;
                continue;
            }
            let mut advance = diagnostic_cell_width(ch, column);
            if column != 0 && column + advance > config.columns {
                push_line(&mut lines, start..offset, offset, config.max_lines)?;
                start = offset;
                column = 0;
                advance = diagnostic_cell_width(ch, column);
            }
            column += advance;
        }
        // A final LF terminates its line; it does not create a phantom extra page.
        if start < document.text().len() || lines.is_empty() {
            push_line(
                &mut lines,
                start..document.text().len(),
                document.text().len(),
                config.max_lines,
            )?;
        }
        Ok(Self {
            document_id: document.id(),
            lines,
            rows: config.rows,
        })
    }

    pub fn page_count(&self) -> usize {
        self.lines.len().div_ceil(self.rows)
    }

    /// Zero-based page index; invalid and overflowing indices return None.
    pub fn page(&self, index: usize) -> Option<&[PreviewLine]> {
        if index >= self.page_count() {
            return None;
        }
        let start = index.checked_mul(self.rows)?;
        self.lines
            .get(start..start.saturating_add(self.rows).min(self.lines.len()))
    }

    pub fn page_for_locator(
        &self,
        document: &TextDocument,
        locator: &TextLocator,
    ) -> Result<usize, Error> {
        if self.document_id != document.id() {
            return Err(Error::InvalidLocator(
                "layout belongs to a different document",
            ));
        }
        let offset = document.restore(locator)?;
        let line = self
            .lines
            .partition_point(|line| line.text_range.start <= offset)
            .saturating_sub(1);
        Ok(line / self.rows)
    }
}

fn push_line(
    lines: &mut Vec<PreviewLine>,
    text_range: Range<usize>,
    next_offset: usize,
    limit: usize,
) -> Result<(), Error> {
    if lines.len() >= limit {
        return Err(Error::InvalidLayout("diagnostic line budget exceeded"));
    }
    lines.try_reserve(1).map_err(|_| Error::AllocationFailed)?;
    lines.push(PreviewLine {
        text_range,
        next_offset,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Limits;

    fn doc(text: &str) -> TextDocument {
        TextDocument::from_bytes(text.as_bytes(), Limits::default()).unwrap()
    }
    fn config(columns: usize, rows: usize) -> PreviewConfig {
        PreviewConfig {
            columns,
            rows,
            ..PreviewConfig::default()
        }
    }

    #[test]
    fn ascii_and_cjk_wrap_on_utf8_boundaries() {
        let document = doc("ab中文cd");
        let layout = PreviewLayout::build(&document, config(4, 1)).unwrap();
        let text: Vec<_> = (0..layout.page_count())
            .map(|page| &document.text()[layout.page(page).unwrap()[0].text_range.clone()])
            .collect();
        assert_eq!(text, ["ab中", "文cd"]);
        assert!(layout.page(2).is_none());
        assert!(layout.page(usize::MAX).is_none());
    }

    #[test]
    fn newlines_and_empty_document_have_no_phantom_pages() {
        for (text, expected) in [("", 1), ("abcd\n", 1), ("a\n\n", 2), ("\n\n", 2)] {
            let layout = PreviewLayout::build(&doc(text), config(4, 1)).unwrap();
            assert_eq!(layout.page_count(), expected, "{text:?}");
        }
    }

    #[test]
    fn every_source_byte_belongs_to_exactly_one_line() {
        let document = doc("a\t中文😀\n\nabcdefg e\u{301}\nend\n");
        for columns in 4..17 {
            for rows in 1..5 {
                let layout = PreviewLayout::build(&document, config(columns, rows)).unwrap();
                let mut reconstructed = String::new();
                let mut offset = 0;
                for page in 0..layout.page_count() {
                    for line in layout.page(page).unwrap() {
                        assert_eq!(line.text_range.start, offset);
                        reconstructed.push_str(&document.text()[offset..line.next_offset]);
                        offset = line.next_offset;
                    }
                }
                assert_eq!(reconstructed, document.text());
            }
        }
    }

    #[test]
    fn locator_restores_content_after_reflow() {
        let document = doc("abcdefghijklmnop");
        let locator = document.locator(8).unwrap();
        let narrow = PreviewLayout::build(&document, config(4, 1)).unwrap();
        let wide = PreviewLayout::build(&document, config(8, 1)).unwrap();
        assert_eq!(narrow.page_for_locator(&document, &locator).unwrap(), 2);
        assert_eq!(wide.page_for_locator(&document, &locator).unwrap(), 1);
        assert!(
            narrow
                .page_for_locator(&doc("different"), &locator)
                .is_err()
        );
        assert_eq!(
            narrow
                .page_for_locator(&document, &document.locator(16).unwrap())
                .unwrap(),
            3
        );
    }

    #[test]
    fn newline_offsets_and_tabs_are_deterministic() {
        let document = doc("a\tb\nnext");
        let layout = PreviewLayout::build(&document, config(4, 1)).unwrap();
        assert_eq!(
            &document.text()[layout.page(0).unwrap()[0].text_range.clone()],
            "a\t"
        );
        assert_eq!(
            layout
                .page_for_locator(&document, &document.locator(3).unwrap())
                .unwrap(),
            1
        );
        assert_eq!(diagnostic_cell_width('\t', 1), 3);
    }

    #[test]
    fn invalid_dimensions_and_line_budget_are_rejected() {
        let document = doc("abcdefgh");
        for config in [
            config(0, 1),
            config(4, 0),
            config(usize::MAX, 1),
            PreviewConfig {
                max_lines: 1,
                ..config(4, 1)
            },
        ] {
            assert!(PreviewLayout::build(&document, config).is_err());
        }
    }
}

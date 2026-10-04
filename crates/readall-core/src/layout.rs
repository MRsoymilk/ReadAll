//! Measured Unicode line wrapping with source locators and grapheme-safe emergency breaks.
//! Segmentation and break opportunities are separate from shaping and bidi.
use crate::{DocumentId, Error, TextDocument, TextLocator};
use std::{fmt, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

/// Extended grapheme clusters with UAX #14 legal break opportunities.
/// The pinned linebreak table and segmentation versions are documented in Cargo.lock.
pub fn clusters(text: &str) -> impl Iterator<Item = (Range<usize>, bool)> + '_ {
    let mut breaks = unicode_linebreak::linebreaks(text).peekable();
    text.grapheme_indices(true).map(move |(start, cluster)| {
        let end = start + cluster.len();
        let mut allowed = false;
        while breaks.peek().is_some_and(|(offset, _)| *offset <= end) {
            allowed |= breaks.next().is_some_and(|(offset, _)| offset == end);
        }
        (start..end, allowed)
    })
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutConfig {
    pub width: f32,
    pub rows: usize,
    pub tab_width: f32,
    pub max_lines: usize,
    pub max_scalars: usize,
}
impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            width: 720.0,
            rows: 24,
            tab_width: 64.0,
            max_lines: 200_000,
            max_scalars: 1_000_000,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct MeasuredLine {
    pub text_range: Range<usize>,
    pub next_offset: usize,
    pub width: f32,
}
#[derive(Debug)]
pub struct MeasuredLayout {
    document_id: DocumentId,
    lines: Vec<MeasuredLine>,
    rows: usize,
}
#[derive(Debug)]
pub enum LayoutError<E> {
    Document(Error),
    Measurement(E),
}
impl<E> From<Error> for LayoutError<E> {
    fn from(value: Error) -> Self {
        Self::Document(value)
    }
}
impl<E: fmt::Display> fmt::Display for LayoutError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Document(error) => error.fmt(f),
            Self::Measurement(error) => write!(f, "text measurement failed: {error}"),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for LayoutError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Document(error) => error,
            Self::Measurement(error) => error,
        })
    }
}

impl MeasuredLayout {
    pub fn build<E>(
        document: &TextDocument,
        config: LayoutConfig,
        mut measure: impl FnMut(char) -> Result<f32, E>,
    ) -> Result<Self, LayoutError<E>> {
        if !config.width.is_finite()
            || !(1.0..=16384.0).contains(&config.width)
            || !config.tab_width.is_finite()
            || config.tab_width <= 0.0
            || config.tab_width > 16384.0
            || !(1..=1024).contains(&config.rows)
            || !(1..=1_000_000).contains(&config.max_lines)
            || !(1..=4_000_000).contains(&config.max_scalars)
        {
            return Err(Error::InvalidLayout("invalid measured layout configuration").into());
        }
        if document.text().chars().count() > config.max_scalars {
            return Err(Error::InvalidLayout("measured scalar budget exceeded").into());
        }
        let mut lines = Vec::new();
        let (mut base, mut work) = (0_usize, 0_usize);
        for part in document.text().split_inclusive('\n') {
            let body = part.strip_suffix('\n').unwrap_or(part);
            let units: Vec<_> = clusters(body).collect();
            if units.is_empty() {
                push(
                    &mut lines,
                    base..base,
                    base + part.len(),
                    0.0,
                    config.max_lines,
                )?;
            }
            let mut start = 0;
            while start < units.len() {
                let (mut end, mut width, mut opportunity) = (start, 0.0_f32, None);
                while end < units.len() {
                    let mut advance = 0.0;
                    for ch in body[units[end].0.clone()].chars() {
                        work += 1;
                        if work > config.max_scalars.saturating_mul(3) {
                            return Err(
                                Error::InvalidLayout("measured work budget exceeded").into()
                            );
                        }
                        let amount = if ch == '\t' {
                            tab_advance(width + advance, config.tab_width)
                        } else {
                            measure(ch).map_err(LayoutError::Measurement)?
                        };
                        if !amount.is_finite() || amount < 0.0 {
                            return Err(Error::InvalidLayout("invalid glyph advance").into());
                        }
                        advance += amount;
                    }
                    if advance > config.width {
                        return Err(Error::InvalidLayout(
                            "grapheme or tab is wider than the content area",
                        )
                        .into());
                    }
                    if end > start && width + advance > config.width {
                        if let Some((at, w)) = opportunity {
                            end = at;
                            width = w;
                        }
                        break;
                    }
                    width += advance;
                    end += 1;
                    if units[end - 1].1 {
                        opportunity = Some((end, width));
                    }
                }
                let first = base + units[start].0.start;
                let last = base + units[end - 1].0.end;
                let next = if end == units.len() {
                    base + part.len()
                } else {
                    last
                };
                push(&mut lines, first..last, next, width, config.max_lines)?;
                start = end;
            }
            base += part.len();
        }
        if lines.is_empty() {
            push(&mut lines, 0..0, 0, 0.0, config.max_lines)?;
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
    pub fn page(&self, index: usize) -> Option<&[MeasuredLine]> {
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
        if document.id() != self.document_id {
            return Err(Error::InvalidLocator("layout belongs to another document"));
        }
        let offset = document.restore(locator)?;
        Ok(self
            .lines
            .partition_point(|line| line.text_range.start <= offset)
            .saturating_sub(1)
            / self.rows)
    }
}
pub fn tab_advance(x: f32, tab_width: f32) -> f32 {
    tab_width - x % tab_width
}
fn push(
    lines: &mut Vec<MeasuredLine>,
    text_range: Range<usize>,
    next_offset: usize,
    width: f32,
    limit: usize,
) -> Result<(), Error> {
    if lines.len() >= limit {
        return Err(Error::InvalidLayout("measured line budget exceeded"));
    }
    lines.try_reserve(1).map_err(|_| Error::AllocationFailed)?;
    lines.push(MeasuredLine {
        text_range,
        next_offset,
        width,
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
    fn measure(ch: char) -> Result<f32, std::convert::Infallible> {
        Ok(match ch {
            'W' => 8.0,
            'i' => 2.0,
            '中' => 10.0,
            _ => 4.0,
        })
    }
    fn config(width: f32) -> LayoutConfig {
        LayoutConfig {
            width,
            rows: 1,
            tab_width: 8.0,
            ..LayoutConfig::default()
        }
    }
    #[test]
    fn word_boundaries_and_graphemes_are_preserved() {
        let document = doc("Hello world");
        let layout = MeasuredLayout::build(&document, config(28.0), measure).unwrap();
        assert_eq!(
            &document.text()[layout.page(0).unwrap()[0].text_range.clone()],
            "Hello "
        );
        assert_eq!(
            &document.text()[layout.page(1).unwrap()[0].text_range.clone()],
            "world"
        );
        let ranges: Vec<_> = clusters("a\u{301}👩‍💻中，文").collect();
        assert_eq!(ranges.len(), 5);
        assert_eq!(ranges[0].0, 0..3);
        assert_eq!(ranges[1].0, 3..14);
        assert!(!ranges[2].1, "do not break before a closing CJK comma");
        let document = doc("a\u{301}b\u{301}");
        let layout = MeasuredLayout::build(&document, config(8.0), measure).unwrap();
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0].text_range, 0..3);
    }
    #[test]
    fn real_advances_drive_wrapping_and_reflow_locations() {
        let document = doc("Wi中i");
        let narrow = MeasuredLayout::build(&document, config(12.0), measure).unwrap();
        assert_eq!(narrow.page_count(), 2);
        assert_eq!(narrow.page(0).unwrap()[0].width, 10.0);
        assert_eq!(narrow.page(1).unwrap()[0].text_range, 2..6);
        assert!(narrow.page(usize::MAX).is_none());
        let locator = document.locator(2).unwrap();
        assert_eq!(narrow.page_for_locator(&document, &locator).unwrap(), 1);
        let wide = MeasuredLayout::build(&document, config(24.0), measure).unwrap();
        assert_eq!(wide.page_for_locator(&document, &locator).unwrap(), 0);
        assert!(wide.page_for_locator(&doc("other"), &locator).is_err());
    }
    #[test]
    fn all_source_bytes_survive_newlines_tabs_and_wrapping() {
        let document = doc("Wi\t中i\n\nWiWi\n");
        for width in [10.0, 12.0, 24.0] {
            let layout = MeasuredLayout::build(&document, config(width), measure).unwrap();
            let mut next = 0;
            for line in &layout.lines {
                assert_eq!(line.text_range.start, next);
                assert!(line.width <= width);
                next = line.next_offset;
            }
            assert_eq!(next, document.text().len());
            assert_eq!(
                layout
                    .page_for_locator(&document, &document.locator(next).unwrap())
                    .unwrap(),
                layout.page_count() - 1
            );
        }
    }
    #[test]
    fn empty_documents_and_final_newlines_do_not_add_phantom_pages() {
        for text in ["", "Wi\n"] {
            assert_eq!(
                MeasuredLayout::build(&doc(text), config(12.0), measure)
                    .unwrap()
                    .page_count(),
                1
            );
        }
    }
    #[test]
    fn invalid_measurements_and_work_limits_fail() {
        for width in [0.0, f32::NAN, f32::INFINITY] {
            assert!(MeasuredLayout::build(&doc("W"), config(width), measure).is_err());
        }
        for advance in [-1.0, f32::NAN, f32::INFINITY, 30.0] {
            assert!(
                MeasuredLayout::build(&doc("a"), config(12.0), |_| Ok::<_, Error>(advance))
                    .is_err()
            );
        }
        for config in [
            LayoutConfig {
                max_scalars: 1,
                ..config(12.0)
            },
            LayoutConfig {
                max_lines: 1,
                ..config(4.0)
            },
        ] {
            assert!(MeasuredLayout::build(&doc("aaaa"), config, measure).is_err());
        }
        assert!(matches!(
            MeasuredLayout::build(&doc("a"), config(12.0), |_| Err::<f32, _>(
                "test measurement failure"
            )),
            Err(LayoutError::Measurement(_))
        ));
    }
}

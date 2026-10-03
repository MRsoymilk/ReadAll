//! Transactional EPUB reading state over the current XHTML text subset.
use crate::text_page::{Options, PageRenderer, RenderedPage};
use readall_core::{Limits, TextDocument};
use readall_epub::{EpubBook, EpubError, EpubLocator};
use readall_font::Font;
use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Action {
    Next,
    Previous,
    First,
    Last,
    Larger,
    Smaller,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TocEntry {
    pub(crate) spine: usize,
    pub(crate) title: String,
    pub(crate) current: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum Start {
    Beginning,
    Spine(usize),
    Locator(EpubLocator),
}

pub(crate) struct EpubSession<'book, 'archive, 'font, 'font_bytes> {
    book: &'book EpubBook<'archive>,
    font: &'font Font<'font_bytes>,
    options: Options,
    renderer: PageRenderer<'font, 'font_bytes>,
    chapter: TextDocument,
    spine: usize,
    frame: RenderedPage,
    anchor: EpubLocator,
}

impl<'book, 'archive, 'font, 'font_bytes> EpubSession<'book, 'archive, 'font, 'font_bytes> {
    pub(crate) fn new(
        book: &'book EpubBook<'archive>,
        font: &'font Font<'font_bytes>,
        mut options: Options,
        start: Start,
    ) -> Result<Self> {
        if options.at.is_some() {
            return Err("EPUB session requires EpubLocator rather than TextLocator".into());
        }
        let (spine, chapter, offset) = match start {
            Start::Beginning => {
                let (spine, chapter) = first_readable(book)?
                    .ok_or("EPUB has no readable text chapters in the current XHTML subset")?;
                (spine, chapter, None)
            }
            Start::Spine(spine) => {
                if book.spine_item(spine).is_none() {
                    return Err("EPUB spine index is outside this book".into());
                }
                (spine, chapter_document(book, spine)?, None)
            }
            Start::Locator(locator) => {
                if options.page.is_some() {
                    return Err("EPUB locator cannot be combined with a page number".into());
                }
                let (spine, offset) = book.restore(&locator)?;
                (
                    spine,
                    chapter_document(book, spine)?,
                    Some((offset, locator)),
                )
            }
        };
        let exact_anchor = if let Some((offset, locator)) = offset {
            options.page = None;
            options.at = Some(chapter.locator(offset)?);
            Some(locator)
        } else {
            None
        };
        let mut renderer = PageRenderer::new(font, options.size, options.allow_missing)?;
        let frame = renderer.render(&chapter, &options)?;
        let anchor = if let Some(locator) = exact_anchor {
            locator
        } else {
            page_anchor(book, spine, &chapter, &frame)?
        };

        Ok(Self {
            book,
            font,
            options,
            renderer,
            chapter,
            spine,
            frame,
            anchor,
        })
    }

    pub(crate) fn frame(&self) -> &RenderedPage {
        &self.frame
    }

    pub(crate) fn anchor(&self) -> &EpubLocator {
        &self.anchor
    }

    pub(crate) fn book_title(&self) -> &str {
        self.book.title().unwrap_or("(untitled)")
    }

    pub(crate) fn chapter_position(&self) -> (usize, usize) {
        (self.spine + 1, self.book.spine().len())
    }

    pub(crate) fn page_position(&self) -> (usize, usize) {
        (self.frame.page + 1, self.frame.pages)
    }

    pub(crate) fn font_size(&self) -> u32 {
        self.options.size
    }

    pub(crate) fn overall_progress(&self) -> f32 {
        let chapters = self.book.spine().len().max(1) as f32;
        let chapter_fraction = (self.frame.page + 1) as f32 / self.frame.pages.max(1) as f32;
        ((self.spine as f32 + chapter_fraction) / chapters).clamp(0.0, 1.0)
    }

    pub(crate) fn toc_entries(&self) -> Result<Vec<TocEntry>> {
        let mut entries = Vec::new();
        for spine in 0..self.book.spine().len() {
            let Some(item) = self.book.spine().get(spine) else {
                continue;
            };
            if !item.linear() {
                continue;
            }
            let text = match self.book.read_spine_text(spine) {
                Ok(text) => text,
                Err(EpubError::Unsupported(_)) => continue,
                Err(error) => return Err(error.into()),
            };
            if text.trim().is_empty() {
                continue;
            }
            let title = chapter_title(&text, entries.len() + 1);
            entries.push(TocEntry {
                spine,
                title,
                current: spine == self.spine,
            });
            if entries.len() >= 512 {
                break;
            }
        }
        Ok(entries)
    }

    pub(crate) fn jump_to_spine(&mut self, spine: usize) -> Result<bool> {
        if spine == self.spine && self.frame.page == 0 {
            return Ok(false);
        }
        let chapter = readable_chapter(self.book, spine)?
            .ok_or("selected EPUB chapter has no readable text")?;
        self.switch_chapter(spine, chapter, PageTarget::First)
    }

    pub(crate) fn title(&self) -> String {
        format!(
            "ReadAll — {} — chapter {}/{} — page {}/{} — {} px{}",
            self.book.title().unwrap_or("(untitled)"),
            self.spine + 1,
            self.book.spine().len(),
            self.frame.page + 1,
            self.frame.pages,
            self.options.size,
            if self.frame.missing.is_empty() {
                ""
            } else {
                " — missing glyph replacements"
            }
        )
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) -> Result<bool> {
        if (width, height) == (self.options.width, self.options.height) {
            return Ok(false);
        }
        let mut options = self.options.clone();
        options.width = width;
        options.height = height;
        self.reflow(options)
    }

    fn reflow(&mut self, mut options: Options) -> Result<bool> {
        let (spine, offset) = self.book.restore(&self.anchor)?;
        if spine != self.spine {
            return Err("EPUB session anchor and chapter disagree".into());
        }
        options.page = None;
        options.at = Some(self.chapter.locator(offset)?);

        if options.size != self.options.size || options.allow_missing != self.options.allow_missing
        {
            let mut renderer = PageRenderer::new(self.font, options.size, options.allow_missing)?;
            let frame = renderer.render(&self.chapter, &options)?;
            self.renderer = renderer;
            self.options = options;
            self.frame = frame;
            return Ok(true);
        }

        let frame = self.renderer.render(&self.chapter, &options)?;
        self.options = options;
        self.frame = frame;
        Ok(true)
    }

    pub(crate) fn action(&mut self, action: Action) -> Result<bool> {
        match action {
            Action::Larger | Action::Smaller => {
                let mut options = self.options.clone();
                let size = if matches!(action, Action::Larger) {
                    options.size.saturating_add(2).min(256)
                } else {
                    options.size.saturating_sub(2).max(8)
                };
                if size == options.size {
                    return Ok(false);
                }
                options.size = size;
                self.reflow(options)
            }
            Action::Next => self.next(),
            Action::Previous => self.previous(),
            Action::First => self.first(),
            Action::Last => self.last(),
        }
    }

    fn next(&mut self) -> Result<bool> {
        if self.frame.page + 1 < self.frame.pages {
            return self.render_current_page(self.frame.page + 1);
        }
        let Some((spine, chapter)) = next_readable(self.book, self.spine)? else {
            return Ok(false);
        };
        self.switch_chapter(spine, chapter, PageTarget::First)
    }

    fn previous(&mut self) -> Result<bool> {
        if self.frame.page > 0 {
            return self.render_current_page(self.frame.page - 1);
        }
        let Some((spine, chapter)) = previous_readable(self.book, self.spine)? else {
            return Ok(false);
        };
        self.switch_chapter(spine, chapter, PageTarget::Last)
    }

    fn first(&mut self) -> Result<bool> {
        let (spine, chapter) = first_readable(self.book)?
            .ok_or("EPUB has no readable text chapters in the current XHTML subset")?;
        if spine == self.spine && self.frame.page == 0 {
            return Ok(false);
        }
        self.switch_chapter(spine, chapter, PageTarget::First)
    }

    fn last(&mut self) -> Result<bool> {
        let (spine, chapter) = last_readable(self.book)?
            .ok_or("EPUB has no readable text chapters in the current XHTML subset")?;
        if spine == self.spine && self.frame.page + 1 == self.frame.pages {
            return Ok(false);
        }
        self.switch_chapter(spine, chapter, PageTarget::Last)
    }

    fn render_current_page(&mut self, page: usize) -> Result<bool> {
        let mut options = self.options.clone();
        options.at = None;
        options.page = Some(page);
        let frame = self.renderer.render(&self.chapter, &options)?;
        let anchor = page_anchor(self.book, self.spine, &self.chapter, &frame)?;
        self.options = options;
        self.frame = frame;
        self.anchor = anchor;
        Ok(true)
    }

    fn switch_chapter(
        &mut self,
        spine: usize,
        chapter: TextDocument,
        target: PageTarget,
    ) -> Result<bool> {
        let mut options = self.options.clone();
        options.at = None;
        options.page = Some(0);
        let mut frame = self.renderer.render(&chapter, &options)?;
        if matches!(target, PageTarget::Last) && frame.pages > 1 {
            options.page = Some(frame.pages - 1);
            frame = self.renderer.render(&chapter, &options)?;
        }
        let anchor = page_anchor(self.book, spine, &chapter, &frame)?;

        self.chapter = chapter;
        self.spine = spine;
        self.options = options;
        self.frame = frame;
        self.anchor = anchor;
        Ok(true)
    }
}

#[derive(Debug, Clone, Copy)]
enum PageTarget {
    First,
    Last,
}

fn chapter_title(text: &str, ordinal: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if line.is_empty() {
        return format!("第 {ordinal} 章");
    }
    let mut title: String = line.chars().take(48).collect();
    if line.chars().count() > 48 {
        title.push('…');
    }
    title
}

fn chapter_document(book: &EpubBook<'_>, spine: usize) -> Result<TextDocument> {
    let text = book.read_spine_text(spine)?;
    if text.trim().is_empty() {
        return Err("EPUB spine contains no readable text in the current XHTML subset".into());
    }
    Ok(TextDocument::from_bytes(
        text.as_bytes(),
        Limits::default(),
    )?)
}

fn readable_chapter(book: &EpubBook<'_>, spine: usize) -> Result<Option<TextDocument>> {
    let Some(spine_item) = book.spine().get(spine) else {
        return Ok(None);
    };
    if !spine_item.linear() {
        return Ok(None);
    }
    let text = match book.read_spine_text(spine) {
        Ok(text) => text,
        Err(EpubError::Unsupported(_)) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if text.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(TextDocument::from_bytes(
        text.as_bytes(),
        Limits::default(),
    )?))
}

fn page_anchor(
    book: &EpubBook<'_>,
    spine: usize,
    chapter: &TextDocument,
    frame: &RenderedPage,
) -> Result<EpubLocator> {
    let offset = chapter.restore(&frame.locator)?;
    Ok(book.locator(spine, offset)?)
}

fn first_readable(book: &EpubBook<'_>) -> Result<Option<(usize, TextDocument)>> {
    for index in 0..book.spine().len() {
        if let Some(chapter) = readable_chapter(book, index)? {
            return Ok(Some((index, chapter)));
        }
    }
    Ok(None)
}

fn last_readable(book: &EpubBook<'_>) -> Result<Option<(usize, TextDocument)>> {
    for index in (0..book.spine().len()).rev() {
        if let Some(chapter) = readable_chapter(book, index)? {
            return Ok(Some((index, chapter)));
        }
    }
    Ok(None)
}

fn next_readable(book: &EpubBook<'_>, current: usize) -> Result<Option<(usize, TextDocument)>> {
    for index in current.saturating_add(1)..book.spine().len() {
        if let Some(chapter) = readable_chapter(book, index)? {
            return Ok(Some((index, chapter)));
        }
    }
    Ok(None)
}

fn previous_readable(book: &EpubBook<'_>, current: usize) -> Result<Option<(usize, TextDocument)>> {
    for index in (0..current.min(book.spine().len())).rev() {
        if let Some(chapter) = readable_chapter(book, index)? {
            return Ok(Some((index, chapter)));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_epub, test_font};
    use readall_epub::EpubLimits;
    use readall_font::FontLimits;

    fn options() -> Options {
        Options::parse(
            &[
                "--font",
                "fixture.ttf",
                "--width",
                "200",
                "--height",
                "128",
                "--margin",
                "16",
            ]
            .map(Into::into),
        )
        .unwrap()
    }

    #[test]
    fn next_and_previous_cross_spine_boundaries() {
        let epub_bytes = test_epub::make_epub();
        let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
        let font_bytes = test_font::make_font();
        let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
        let mut session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();

        assert_eq!(session.spine, 0);
        assert!(session.title().contains("ReadAll Test"));
        for _ in 0..100 {
            if session.spine == 1 {
                break;
            }
            assert!(session.action(Action::Next).unwrap());
        }
        assert_eq!(session.spine, 1);
        assert_eq!(session.frame().page, 0);
        assert_eq!(session.anchor().spine_index(), 1);

        assert!(session.action(Action::Previous).unwrap());
        assert_eq!(session.spine, 0);
        assert_eq!(session.frame().page + 1, session.frame().pages);
        assert_eq!(session.anchor().spine_index(), 0);
    }

    #[test]
    fn first_last_and_reflow_preserve_epub_identity() {
        let epub_bytes = test_epub::make_epub();
        let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
        let font_bytes = test_font::make_font();
        let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
        let locator = book.locator(1, 11).unwrap();
        let mut session =
            EpubSession::new(&book, &font, options(), Start::Locator(locator.clone())).unwrap();

        assert_eq!(session.spine, 1);
        assert_eq!(session.anchor(), &locator);
        session.resize(260, 160).unwrap();
        session.action(Action::Larger).unwrap();
        session.action(Action::Smaller).unwrap();
        assert_eq!(session.anchor(), &locator);

        assert!(session.action(Action::First).unwrap());
        assert_eq!(session.spine, 0);
        assert_eq!(session.frame().page, 0);
        assert!(session.action(Action::Last).unwrap());
        assert_eq!(session.spine, 1);
        assert_eq!(session.frame().page + 1, session.frame().pages);
    }

    #[test]
    fn empty_cover_and_non_text_spines_are_skipped_during_navigation() {
        let epub_bytes = test_epub::make_epub_with_empty_spines();
        let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
        let font_bytes = test_font::make_font();
        let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
        let mut session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();

        // spine 0 is SVG cover, so reading begins at the first text chapter.
        assert_eq!(session.spine, 1);

        // Advance until the next readable chapter. spine 2 is blank and must be skipped.
        for _ in 0..100 {
            if session.spine == 3 {
                break;
            }
            assert!(session.action(Action::Next).unwrap());
        }
        assert_eq!(session.spine, 3);
        assert_eq!(session.frame().page, 0);

        // Going back crosses over the blank spine and returns to chapter one.
        assert!(session.action(Action::Previous).unwrap());
        assert_eq!(session.spine, 1);
        assert_eq!(session.frame().page + 1, session.frame().pages);

        // First/Last ignore the non-readable cover and trailing image-only page.
        assert!(session.action(Action::Last).unwrap());
        assert_eq!(session.spine, 3);
        assert_eq!(session.frame().page + 1, session.frame().pages);
        assert!(!session.action(Action::Next).unwrap());
        assert!(session.action(Action::First).unwrap());
        assert_eq!(session.spine, 1);
        assert_eq!(session.frame().page, 0);
        assert!(!session.action(Action::Previous).unwrap());
    }

    #[test]
    fn toc_lists_only_readable_chapters_and_jumps_to_them() {
        let epub_bytes = test_epub::make_epub_with_empty_spines();
        let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
        let font_bytes = test_font::make_font();
        let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
        let mut session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();

        let toc = session.toc_entries().unwrap();
        assert_eq!(toc.len(), 2);
        assert_eq!(toc[0].spine, 1);
        assert_eq!(toc[1].spine, 3);
        assert!(toc[0].current);
        assert!(!toc[1].current);
        assert!(toc[0].title.starts_with("AAAA"));

        assert!(session.jump_to_spine(toc[1].spine).unwrap());
        assert_eq!(session.spine, 3);
        assert_eq!(session.frame().page, 0);
        assert!(!session.jump_to_spine(3).unwrap());
        assert!(session.jump_to_spine(0).is_err());
    }

    #[test]
    fn explicit_spine_start_is_validated() {
        let epub_bytes = test_epub::make_epub();
        let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
        let font_bytes = test_font::make_font();
        let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();

        assert!(EpubSession::new(&book, &font, options(), Start::Spine(99)).is_err());
        let session = EpubSession::new(&book, &font, options(), Start::Spine(1)).unwrap();
        assert_eq!(session.spine, 1);
    }
}

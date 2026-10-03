//! Transactional EPUB reading state over the current XHTML text subset.
use crate::text_page::{Options, PageRenderer, RenderedPage};
use readall_core::{Limits, TextDocument};
use readall_epub::{EpubBook, EpubLocator};
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
        let (spine, offset) = match start {
            Start::Beginning => (
                first_linear(book).ok_or("EPUB has no linear spine items")?,
                None,
            ),
            Start::Spine(spine) => {
                if book.spine_item(spine).is_none() {
                    return Err("EPUB spine index is outside this book".into());
                }
                (spine, None)
            }
            Start::Locator(locator) => {
                if options.page.is_some() {
                    return Err("EPUB locator cannot be combined with a page number".into());
                }
                let (spine, offset) = book.restore(&locator)?;
                (spine, Some((offset, locator)))
            }
        };

        let chapter = chapter_document(book, spine)?;
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
        let Some(spine) = next_linear(self.book, self.spine) else {
            return Ok(false);
        };
        self.switch_spine(spine, PageTarget::First)
    }

    fn previous(&mut self) -> Result<bool> {
        if self.frame.page > 0 {
            return self.render_current_page(self.frame.page - 1);
        }
        let Some(spine) = previous_linear(self.book, self.spine) else {
            return Ok(false);
        };
        self.switch_spine(spine, PageTarget::Last)
    }

    fn first(&mut self) -> Result<bool> {
        let spine = first_linear(self.book).ok_or("EPUB has no linear spine items")?;
        if spine == self.spine && self.frame.page == 0 {
            return Ok(false);
        }
        self.switch_spine(spine, PageTarget::First)
    }

    fn last(&mut self) -> Result<bool> {
        let spine = last_linear(self.book).ok_or("EPUB has no linear spine items")?;
        if spine == self.spine && self.frame.page + 1 == self.frame.pages {
            return Ok(false);
        }
        self.switch_spine(spine, PageTarget::Last)
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

    fn switch_spine(&mut self, spine: usize, target: PageTarget) -> Result<bool> {
        let chapter = chapter_document(self.book, spine)?;
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

fn chapter_document(book: &EpubBook<'_>, spine: usize) -> Result<TextDocument> {
    let text = book.read_spine_text(spine)?;
    if text.is_empty() {
        return Err("EPUB spine contains no readable text in the current XHTML subset".into());
    }
    Ok(TextDocument::from_bytes(
        text.as_bytes(),
        Limits::default(),
    )?)
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

fn first_linear(book: &EpubBook<'_>) -> Option<usize> {
    book.spine().iter().position(|item| item.linear())
}

fn last_linear(book: &EpubBook<'_>) -> Option<usize> {
    book.spine().iter().rposition(|item| item.linear())
}

fn next_linear(book: &EpubBook<'_>, current: usize) -> Option<usize> {
    book.spine()
        .iter()
        .enumerate()
        .skip(current.saturating_add(1))
        .find_map(|(index, item)| item.linear().then_some(index))
}

fn previous_linear(book: &EpubBook<'_>, current: usize) -> Option<usize> {
    book.spine()
        .iter()
        .enumerate()
        .take(current)
        .rev()
        .find_map(|(index, item)| item.linear().then_some(index))
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

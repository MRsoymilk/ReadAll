//! Transactional EPUB reading state over the current XHTML text subset.
use crate::epub_flow::{Chapter, EpubRenderer as PageRenderer};
use crate::reader_data::Settings;
mod paging;
mod tools;
use crate::text_page::{Options, RenderedPage};
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
    pub(crate) offset: usize,
    pub(crate) title: String,
    pub(crate) depth: usize,
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
    fallbacks: Vec<&'font Font<'font_bytes>>,
    options: Options,
    renderer: PageRenderer<'font, 'font_bytes>,
    chapter: Chapter<'book, 'archive>,
    spine: usize,
    frame: RenderedPage,
    anchor: EpubLocator,
    preferences: Settings,
    paging: paging::Paging<'book, 'archive, 'font, 'font_bytes>,
}

impl<'book, 'archive, 'font, 'font_bytes> EpubSession<'book, 'archive, 'font, 'font_bytes> {
    #[cfg(test)]
    pub(crate) fn new(
        book: &'book EpubBook<'archive>,
        font: &'font Font<'font_bytes>,
        options: Options,
        start: Start,
    ) -> Result<Self> {
        Self::new_with_fallbacks(book, font, options, start, &[])
    }
    #[cfg(test)]
    pub(crate) fn new_with_fallbacks(
        book: &'book EpubBook<'archive>,
        font: &'font Font<'font_bytes>,
        options: Options,
        start: Start,
        fallbacks: &[&'font Font<'font_bytes>],
    ) -> Result<Self> {
        Self::new_with_preferences(book, font, options, start, fallbacks, Settings::default())
    }
    pub(crate) fn new_with_preferences(
        book: &'book EpubBook<'archive>,
        font: &'font Font<'font_bytes>,
        mut options: Options,
        start: Start,
        fallbacks: &[&'font Font<'font_bytes>],
        preferences: Settings,
    ) -> Result<Self> {
        crate::loading::stage("读取当前章节")?;
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
                let locator = book.normalize_locator(&locator)?;
                let (spine, offset) = (locator.spine_index(), locator.utf8_offset() as usize);
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
        let mut renderer = PageRenderer::new(font, options.size, options.allow_missing)?
            .with_fallbacks(fallbacks)
            .with_preferences(preferences.theme, preferences.line_spacing);
        let frame = renderer.render_with_image(
            &chapter,
            &options,
            exact_anchor.as_ref().and_then(EpubLocator::image_index),
        )?;
        let anchor = if let Some(locator) = exact_anchor {
            locator
        } else {
            page_anchor(book, spine, &chapter, &frame)?
        };

        Ok(Self {
            book,
            font,
            fallbacks: fallbacks.iter().copied().take(12).collect(),
            options,
            renderer,
            chapter,
            spine,
            frame,
            anchor,
            preferences,
            paging: paging::Paging::default(),
        })
    }

    pub(crate) fn frame(&self) -> &RenderedPage {
        self.paging.view.as_ref().unwrap_or(&self.frame)
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

    #[cfg(test)]
    pub(crate) fn font_size(&self) -> u32 {
        self.options.size
    }

    pub(crate) fn overall_progress(&self) -> f32 {
        if let Some(progress) = self.scroll_progress() {
            return progress;
        }
        let chapters = self.book.spine().len().max(1) as f32;
        let chapter_fraction = (self.frame.page + 1) as f32 / self.frame.pages.max(1) as f32;
        ((self.spine as f32 + chapter_fraction) / chapters).clamp(0.0, 1.0)
    }

    pub(crate) fn toc_entries(&self) -> Result<Vec<TocEntry>> {
        if let Ok(navigation) = self.book.navigation() {
            let mut entries = Vec::new();
            let navigation_len = navigation.len();
            for (position, item) in navigation.into_iter().enumerate() {
                crate::loading::step("生成目录", position, navigation_len)?;
                let spine = item.spine_index();
                let Some(spine_item) = self.book.spine().get(spine) else {
                    continue;
                };
                if !spine_item.linear() {
                    continue;
                }
                let content = match self.book.read_spine_content(spine) {
                    Ok(content) => content,
                    Err(EpubError::Unsupported(_)) => continue,
                    Err(_) => continue,
                };
                if content.text.trim().is_empty() && content.images.is_empty() {
                    continue;
                }
                let offset = match item.fragment() {
                    Some(fragment) => self
                        .book
                        .locator_for_fragment(spine, fragment)
                        .ok()
                        .flatten()
                        .and_then(|locator| usize::try_from(locator.utf8_offset()).ok())
                        .unwrap_or(0),
                    None => 0,
                };
                entries.push(TocEntry {
                    spine,
                    offset,
                    title: item.label().to_owned(),
                    depth: item.depth(),
                });
                if entries.len() >= 512 {
                    break;
                }
            }
            if !entries.is_empty() {
                return Ok(entries);
            }
        }

        let mut entries = Vec::new();
        for spine in 0..self.book.spine().len() {
            crate::loading::step("生成目录", spine, self.book.spine().len())?;
            let Some(item) = self.book.spine().get(spine) else {
                continue;
            };
            if !item.linear() {
                continue;
            }
            let content = match self.book.read_spine_content(spine) {
                Ok(content) => content,
                Err(EpubError::Unsupported(_)) => continue,
                Err(error) => return Err(error.into()),
            };
            if content.text.trim().is_empty() && content.images.is_empty() {
                continue;
            }
            let title = chapter_title(&content.text, entries.len() + 1);
            entries.push(TocEntry {
                spine,
                offset: 0,
                title,
                depth: 0,
            });
            if entries.len() >= 512 {
                break;
            }
        }
        Ok(entries)
    }

    #[cfg(test)]
    pub(crate) fn jump_to_spine(&mut self, spine: usize) -> Result<bool> {
        self.jump_to_toc_target(spine, 0)
    }

    pub(crate) fn jump_to_toc_target(&mut self, spine: usize, offset: usize) -> Result<bool> {
        let chapter = readable_chapter(self.book, spine)?
            .ok_or("selected EPUB chapter has no readable text")?;
        let anchor = self.book.locator(spine, offset)?;
        if self.spine == spine && self.anchor == anchor {
            return Ok(false);
        }
        let mut options = self.options.clone();
        options.page = None;
        options.at = Some(chapter.locator(offset)?);
        let frame = self.renderer.render(&chapter, &options)?;
        self.chapter = chapter;
        self.spine = spine;
        self.options = options;
        self.clear_paging();
        self.frame = frame;
        self.anchor = anchor;
        Ok(true)
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
            let mut renderer = PageRenderer::new(self.font, options.size, options.allow_missing)?
                .with_fallbacks(&self.fallbacks)
                .with_preferences(self.preferences.theme, self.preferences.line_spacing);
            let frame =
                renderer.render_with_image(&self.chapter, &options, self.anchor.image_index())?;
            self.renderer = renderer;
            self.options = options;
            self.clear_paging();
            self.frame = frame;
            return Ok(true);
        }

        let frame =
            self.renderer
                .render_with_image(&self.chapter, &options, self.anchor.image_index())?;
        self.options = options;
        self.clear_paging();
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
        self.clear_paging();
        self.frame = frame;
        self.anchor = anchor;
        Ok(true)
    }

    fn switch_chapter(
        &mut self,
        spine: usize,
        chapter: Chapter<'book, 'archive>,
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
        self.clear_paging();
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

fn chapter_document<'book, 'archive>(
    book: &'book EpubBook<'archive>,
    spine: usize,
) -> Result<Chapter<'book, 'archive>> {
    let chapter = Chapter::load(book, spine)?;
    if !chapter.is_readable() {
        return Err("EPUB spine contains no readable text or images".into());
    }
    Ok(chapter)
}

fn readable_chapter<'book, 'archive>(
    book: &'book EpubBook<'archive>,
    spine: usize,
) -> Result<Option<Chapter<'book, 'archive>>> {
    let Some(spine_item) = book.spine().get(spine) else {
        return Ok(None);
    };
    if !spine_item.linear() {
        return Ok(None);
    }
    match Chapter::load(book, spine) {
        Ok(chapter) => Ok(chapter.is_readable().then_some(chapter)),
        Err(error)
            if matches!(
                error.downcast_ref::<EpubError>(),
                Some(EpubError::Unsupported(_))
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn page_anchor(
    _book: &EpubBook<'_>,
    _spine: usize,
    chapter: &Chapter<'_, '_>,
    frame: &RenderedPage,
) -> Result<EpubLocator> {
    let offset = chapter.restore(&frame.locator)?;
    chapter.epub_locator(offset, frame.image_index)
}

fn first_readable<'book, 'archive>(
    book: &'book EpubBook<'archive>,
) -> Result<Option<(usize, Chapter<'book, 'archive>)>> {
    for index in 0..book.spine().len() {
        if let Some(chapter) = readable_chapter(book, index)? {
            return Ok(Some((index, chapter)));
        }
    }
    Ok(None)
}

fn last_readable<'book, 'archive>(
    book: &'book EpubBook<'archive>,
) -> Result<Option<(usize, Chapter<'book, 'archive>)>> {
    for index in (0..book.spine().len()).rev() {
        if let Some(chapter) = readable_chapter(book, index)? {
            return Ok(Some((index, chapter)));
        }
    }
    Ok(None)
}

fn next_readable<'book, 'archive>(
    book: &'book EpubBook<'archive>,
    current: usize,
) -> Result<Option<(usize, Chapter<'book, 'archive>)>> {
    for index in current.saturating_add(1)..book.spine().len() {
        if let Some(chapter) = readable_chapter(book, index)? {
            return Ok(Some((index, chapter)));
        }
    }
    Ok(None)
}

fn previous_readable<'book, 'archive>(
    book: &'book EpubBook<'archive>,
    current: usize,
) -> Result<Option<(usize, Chapter<'book, 'archive>)>> {
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
        assert_eq!(toc[0].offset, 0);
        assert_eq!(toc[1].offset, 0);
        assert_eq!(toc[0].depth, 0);
        assert_eq!(toc[1].depth, 0);
        assert!(toc[0].title.starts_with("AAAA"));

        assert!(session.jump_to_spine(toc[1].spine).unwrap());
        assert_eq!(session.spine, 3);
        assert_eq!(session.frame().page, 0);
        assert!(!session.jump_to_spine(3).unwrap());
        assert!(session.jump_to_spine(0).is_err());
    }

    #[test]
    fn toc_prefers_epub3_navigation_labels_and_preserves_hierarchy() {
        let epub_bytes = test_epub::make_epub_with_navigation();
        let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
        let font_bytes = test_font::make_font();
        let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
        let mut session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();

        let toc = session.toc_entries().unwrap();
        assert_eq!(toc.len(), 3);
        assert_eq!(toc[0].title, "正式目录第一章");
        assert_eq!(toc[0].spine, 0);
        assert_eq!(toc[0].offset, 0);
        assert_eq!(toc[0].depth, 0);
        assert_eq!(toc[1].title, "第一章详细部分");
        assert_eq!(toc[1].spine, 0);
        assert!(toc[1].offset > 0);
        assert_eq!(toc[1].depth, 1);
        assert_eq!(toc[2].title, "正式目录第二章");
        assert_eq!(toc[2].spine, 1);
        assert_eq!(toc[2].offset, 0);
        assert_eq!(toc[2].depth, 0);

        let target = toc[1].offset;
        assert!(session.jump_to_toc_target(toc[1].spine, target).unwrap());
        assert_eq!(session.anchor().spine_index(), 0);
        assert_eq!(session.anchor().utf8_offset(), target as u64);
        let exact = session.anchor().clone();
        session.resize(260, 160).unwrap();
        session.action(Action::Larger).unwrap();
        assert_eq!(session.anchor(), &exact);
    }

    #[test]
    fn toc_uses_epub2_ncx_when_epub3_navigation_is_absent() {
        let epub_bytes = test_epub::make_epub_with_ncx_navigation();
        let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
        let font_bytes = test_font::make_font();
        let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
        let mut session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();

        let toc = session.toc_entries().unwrap();
        assert_eq!(toc.len(), 3);
        assert_eq!(toc[0].title, "旧目录第一章");
        assert_eq!(toc[0].spine, 0);
        assert_eq!(toc[0].offset, 0);
        assert_eq!(toc[0].depth, 0);
        assert_eq!(toc[1].title, "第一章子节");
        assert_eq!(toc[1].spine, 0);
        assert!(toc[1].offset > 0);
        assert_eq!(toc[1].depth, 1);
        assert_eq!(toc[2].title, "旧目录第二章");
        assert_eq!(toc[2].spine, 1);
        assert_eq!(toc[2].offset, 0);

        assert!(
            session
                .jump_to_toc_target(toc[1].spine, toc[1].offset)
                .unwrap()
        );
        assert_eq!(session.anchor().utf8_offset(), toc[1].offset as u64);
    }

    #[test]
    fn image_only_spines_participate_in_navigation_toc_and_progress() {
        let epub_bytes = test_epub::make_epub_with_resources(
            &[
                "<html><body><img src='a.png'/></body></html>",
                "<html><body><p>AAAA</p></body></html>",
            ],
            vec![(
                "a.png",
                "image/png",
                test_epub::make_png(64, 32, [0, 0, 255, 255]),
            )],
        );
        let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
        let font_bytes = test_font::make_font();
        let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
        let mut session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
        assert_eq!(session.spine, 0);
        assert_eq!(session.toc_entries().unwrap().len(), 2);
        let anchor = session.anchor().clone();
        assert!(session.resize(1, 1).is_err());
        assert_eq!(session.anchor(), &anchor);
        assert!(session.action(Action::Next).unwrap());
        assert_eq!(session.spine, 1);
        assert!(session.action(Action::Previous).unwrap());
        assert_eq!(session.spine, 0);
        assert_eq!(session.anchor(), &anchor);
        let restored = EpubSession::new(&book, &font, options(), Start::Locator(anchor)).unwrap();
        assert_eq!(restored.spine, 0);
        assert_eq!(restored.frame().page, 0);
    }

    #[test]
    fn adjacent_image_pages_resume_exactly_across_resize_and_restart() {
        let source =
            "<html><body><img src='a.png'/><img src='a.png'/><img src='a.png'/></body></html>";
        let bytes = test_epub::make_epub_with_resources(
            &[source],
            vec![(
                "a.png",
                "image/png",
                test_epub::make_png(160, 96, [20, 40, 60, 255]),
            )],
        );
        let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
        let font_bytes = test_font::make_font();
        let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
        let mut session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
        assert_eq!(session.frame().pages, 3);
        session.action(Action::Next).unwrap();
        let anchor = session.anchor().clone();
        assert_eq!(anchor.image_index(), Some(1));
        assert_eq!(anchor.utf8_offset(), 0);
        assert!(anchor.to_string().starts_with("epub-v2:"));
        assert_eq!(anchor.to_string().parse::<EpubLocator>().unwrap(), anchor);
        let mut resumed =
            EpubSession::new(&book, &font, options(), Start::Locator(anchor.clone())).unwrap();
        assert_eq!(resumed.frame().page, 1);
        resumed.resize(260, 240).unwrap();
        resumed.resize(200, 128).unwrap();
        assert_eq!(resumed.frame().page, 1);
        assert_eq!(resumed.anchor(), &anchor);
        assert!(
            book.restore(&format!("epub-v2:{}:0:0:99", book.id()).parse().unwrap())
                .is_err()
        );
        let legacy = book.locator(0, 0).unwrap();
        assert!(legacy.to_string().starts_with("epub-v1:"));
        assert_eq!(
            EpubSession::new(&book, &font, options(), Start::Locator(legacy))
                .unwrap()
                .frame()
                .page,
            0
        );
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

use super::*;
use readall_epub::{SearchLimits, SearchReport};
use readall_image::RgbaImage;
use std::{ops::Range, sync::Arc};
impl<'book, 'archive, 'font, 'data> EpubSession<'book, 'archive, 'font, 'data> {
    pub(crate) fn book(&self) -> &EpubBook<'archive> {
        self.book
    }
    pub(crate) fn current_spine(&self) -> usize {
        self.spine
    }
    pub(crate) fn settings(&self) -> Settings {
        Settings {
            size: self.options.size,
            margin: self.options.margin,
            ..self.preferences
        }
    }
    pub(crate) fn apply_settings(&mut self, settings: Settings) -> Result<bool> {
        settings.validate()?;
        let mut options = self.options.clone();
        options.size = settings.size;
        options.margin = settings.margin;
        options.page = None;
        let (_, offset) = self.book.restore(&self.anchor)?;
        options.at = Some(self.chapter.locator(offset)?);
        let mut renderer = PageRenderer::new(self.font, options.size, options.allow_missing)?
            .with_fallbacks(&self.fallbacks)
            .with_preferences(settings.theme, settings.line_spacing);
        let frame =
            renderer.render_with_image(&self.chapter, &options, self.anchor.image_index())?;
        self.options = options;
        self.renderer = renderer;
        self.frame = frame;
        self.preferences = settings;
        Ok(true)
    }
    pub(crate) fn search(&self, query: &str) -> Result<SearchReport> {
        Ok(self.book.search(query, false, SearchLimits::default())?)
    }
    pub(crate) fn jump_to_locator(&mut self, locator: EpubLocator) -> Result<bool> {
        let (spine, offset) = self.book.restore(&locator)?;
        let mut options = self.options.clone();
        options.page = None;
        if spine == self.spine {
            options.at = Some(self.chapter.locator(offset)?);
            let frame =
                self.renderer
                    .render_with_image(&self.chapter, &options, locator.image_index())?;
            self.options = options;
            self.frame = frame;
            self.anchor = locator;
            return Ok(true);
        }
        let chapter = chapter_document(self.book, spine)?;
        options.at = Some(chapter.locator(offset)?);
        let frame = self
            .renderer
            .render_with_image(&chapter, &options, locator.image_index())?;
        self.chapter = chapter;
        self.spine = spine;
        self.options = options;
        self.frame = frame;
        self.anchor = locator;
        Ok(true)
    }
    pub(crate) fn link_regions(
        &self,
    ) -> impl Iterator<Item = (readall_render::Rect, &readall_epub::ContentLink)> {
        self.frame
            .hits
            .iter()
            .filter_map(|hit| {
                self.chapter
                    .text_link(hit.start..hit.end)
                    .map(|link| (hit.rect, link))
            })
            .chain(self.frame.image_hits.iter().filter_map(|(rect, index)| {
                self.chapter.image_link(*index).map(|link| (*rect, link))
            }))
    }
    pub(crate) fn link_at(&self, x: i32, y: i32) -> Option<readall_epub::ContentLink> {
        self.link_regions()
            .find(|(rect, _)| {
                i64::from(x) >= i64::from(rect.x)
                    && i64::from(x) < i64::from(rect.x) + i64::from(rect.width)
                    && i64::from(y) >= i64::from(rect.y)
                    && i64::from(y) < i64::from(rect.y) + i64::from(rect.height)
            })
            .map(|(_, link)| link.clone())
    }
    pub(crate) fn image(&self, index: usize) -> Option<Arc<RgbaImage>> {
        let fonts: Vec<_> = std::iter::once(self.font.data())
            .chain(self.fallbacks.iter().map(|font| font.data()))
            .take(12)
            .collect();
        self.chapter.image(index, &fonts)
    }
    pub(crate) fn text_at(&self, range: Range<usize>) -> String {
        let Some(text) = self.chapter.text().get(range.clone()) else {
            return String::new();
        };
        text.char_indices()
            .filter_map(|(offset, ch)| {
                (!self.chapter.style(range.start + offset).hidden).then_some(ch)
            })
            .collect()
    }
    pub(crate) fn selected_locator(&self, offset: usize) -> Result<EpubLocator> {
        Ok(self.book.locator(self.spine, offset)?)
    }
    pub(crate) fn grapheme_range(&self, range: Range<usize>) -> Range<usize> {
        let mut start = range.start;
        let mut end = range.end;
        for (cluster, _) in readall_core::layout::clusters(self.chapter.text()) {
            if cluster.start <= range.start && range.start < cluster.end {
                start = cluster.start;
            }
            if cluster.start < range.end && range.end <= cluster.end {
                end = cluster.end;
                break;
            }
        }
        start..end
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{reader_data::Theme, test_epub, test_font};
    use readall_epub::EpubLimits;
    use readall_font::FontLimits;
    #[test]
    fn settings_and_search_jump_keep_content_anchor() {
        let bytes = test_epub::make_epub();
        let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
        let fb = test_font::make_font();
        let font = Font::parse(&fb, 0, FontLimits::default()).unwrap();
        let opts = Options::parse(
            &[
                "--font",
                "fixture.ttf",
                "--width",
                "400",
                "--height",
                "300",
                "--margin",
                "24",
            ]
            .map(Into::into),
        )
        .unwrap();
        let mut session = EpubSession::new(&book, &font, opts, Start::Beginning).unwrap();
        let hit = session.search("WWWW").unwrap().hits[0].locator.clone();
        session.jump_to_locator(hit.clone()).unwrap();
        let settings = Settings {
            theme: Theme::Dark,
            size: 26,
            margin: 28,
            line_spacing: 1.25,
        };
        session.apply_settings(settings).unwrap();
        assert_eq!(session.anchor(), &hit);
        assert_eq!(
            session.frame().surface.pixel(0, 0),
            Some(Theme::Dark.colors().0)
        );
        let before = session.anchor().clone();
        assert!(
            session
                .apply_settings(Settings {
                    margin: 200,
                    ..settings
                })
                .is_err()
        );
        assert_eq!(session.anchor(), &before);
        assert_eq!(session.text_at(0..4), "AAAA");
    }
}

//! Styled EPUB pagination. Canonical text offsets remain independent of presentation.
#[cfg(test)]
mod image_tests;
mod images;
#[cfg(test)]
mod svg_tests;
#[cfg(test)]
mod tests;
use crate::reader_data::Theme;
use crate::text_page::{GlyphCache, Options, RenderedPage, TextHit};
mod box_layout;
#[cfg(test)]
mod box_tests;
mod wrapping;
use box_layout::{BoxPaint, OpenBox};
use wrapping::build;
mod embedded;
#[cfg(test)]
mod embedded_tests;
mod font_cache;
mod shaping;
use font_cache::Fonts;
use images::ImageStore;
use readall_core::{DocumentId, Limits, TextDocument, layout::tab_advance};
use readall_epub::{
    ChapterContent, EpubBook,
    css::{TextAlign, TextStyle},
};
use readall_font::Font;
use readall_render::{Color, DrawCommand, Rect, RenderLimits, Surface};
use shaping::PositionedGlyph;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    error::Error,
    ops::{Deref, Range},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub(crate) struct Chapter<'book, 'archive> {
    document: TextDocument,
    content: ChapterContent,
    embedded: embedded::EmbeddedFonts,
    images: RefCell<ImageStore<'book, 'archive>>,
    identity: (DocumentId, usize),
}
impl Deref for Chapter<'_, '_> {
    type Target = TextDocument;
    fn deref(&self) -> &Self::Target {
        &self.document
    }
}
impl<'book, 'archive> Chapter<'book, 'archive> {
    pub(crate) fn load(book: &'book EpubBook<'archive>, spine: usize) -> Result<Self> {
        crate::loading::stage("解析当前章节与样式")?;
        let mut content = book.read_spine_content(spine)?;
        crate::loading::stage("加载章节内嵌字体")?;
        let embedded = embedded::EmbeddedFonts::load(book, &mut content);
        crate::loading::check()?;
        let document = TextDocument::from_bytes(content.text.as_bytes(), Limits::default())?;
        let images = RefCell::new(ImageStore::new(book, spine, &content.images));
        for warning in &content.warnings {
            eprintln!("ReadAll: EPUB chapter {}: {warning}", spine + 1);
        }
        Ok(Self {
            document,
            content,
            embedded,
            images,
            identity: (book.id(), spine),
        })
    }
    #[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
    pub(crate) fn image(
        &self,
        index: usize,
        fonts: &[&[u8]],
    ) -> Option<std::sync::Arc<readall_image::RgbaImage>> {
        self.images.borrow_mut().get_with_fonts(index, fonts)
    }
    #[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
    pub(crate) fn text_link(&self, range: Range<usize>) -> Option<&readall_epub::ContentLink> {
        let index = self
            .content
            .links
            .partition_point(|link| link.text.end <= range.start);
        self.content
            .links
            .get(index)
            .filter(|link| link.text.start < range.end && range.start < link.text.end)
    }
    #[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
    pub(crate) fn image_link(&self, image: usize) -> Option<&readall_epub::ContentLink> {
        let index = self
            .content
            .links
            .partition_point(|link| link.images.end <= image);
        self.content
            .links
            .get(index)
            .filter(|link| link.images.contains(&image))
    }
    pub(crate) fn is_readable(&self) -> bool {
        !self.text().trim().is_empty() || !self.content.images.is_empty()
    }
    pub(crate) fn style(&self, offset: usize) -> TextStyle {
        let index = self
            .content
            .runs
            .partition_point(|run| run.range.end <= offset);
        self.content
            .runs
            .get(index)
            .filter(|run| run.range.contains(&offset))
            .map_or(TextStyle::default(), |run| run.style)
    }
}

#[derive(Debug)]
struct TextLine {
    range: Range<usize>,
    glyphs: Vec<PositionedGlyph>,
    x: f32,
    baseline: f32,
}
#[derive(Debug)]
enum Item {
    Text(TextLine),
    Image { index: usize, rect: Rect },
}
#[derive(Debug)]
struct Page {
    start: usize,
    items: Vec<Item>,
    decorations: Vec<BoxPaint>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    book: DocumentId,
    spine: usize,
    width: u32,
    height: u32,
    margin: u32,
}
struct Layout {
    key: Key,
    pages: Vec<Page>,
}
struct Builder {
    pages: Vec<Page>,
    boxes: Vec<OpenBox>,
    pending_break: bool,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    items: usize,
}
impl Builder {
    fn new(width: u32, height: u32) -> Self {
        Self {
            pages: vec![Page {
                start: 0,
                items: Vec::new(),
                decorations: Vec::new(),
            }],
            boxes: Vec::new(),
            pending_break: false,
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
            items: 0,
        }
    }
    fn reserve(&mut self, offset: usize, height: f32) -> Result<()> {
        if !height.is_finite() || height > self.height {
            return Err("EPUB page is too short for the selected font size".into());
        }
        if self.pending_break || self.y + height > self.height {
            self.next_page(offset)?;
        }
        // Oversized leading decorations must not clip the first line/image.
        if self.y + height > self.height {
            self.y = (self.height - height).max(0.0);
        }
        self.items += 1;
        if self.items > 200_000 {
            return Err("EPUB layout item budget exceeded".into());
        }
        Ok(())
    }
    fn line(&mut self, line: &mut PendingLine) -> Result<()> {
        let Some(start) = line.start else {
            return Ok(());
        };
        self.reserve(start, line.height)?;
        let free = (self.width - line.indent - line.width).max(0.0);
        let x = self.x
            + line.indent
            + match line.align {
                TextAlign::Left => 0.0,
                TextAlign::Center => free / 2.0,
                TextAlign::Right => free,
            };
        self.pages
            .last_mut()
            .expect("initial page exists")
            .items
            .push(Item::Text(TextLine {
                range: start..line.end,
                glyphs: std::mem::take(&mut line.glyphs),
                x,
                baseline: self.y + line.ascent,
            }));
        self.y += line.height;
        *line = PendingLine::default();
        Ok(())
    }
    fn image(&mut self, chapter: &Chapter<'_, '_>, index: usize) -> Result<()> {
        let reference = &chapter.content.images[index];
        let dimensions = chapter.images.borrow_mut().info(index);
        let image_height = if self.pages.last().is_some_and(|page| page.items.is_empty()) {
            (self.height - self.y).max(1.0)
        } else {
            self.height
        };
        let (width, height) = if let Some(info) = dimensions {
            let width = reference.width.unwrap_or(info.width).min(self.width as u32) as f32;
            let height = reference
                .height
                .unwrap_or(info.height)
                .min(image_height as u32) as f32;
            let scale = (width / info.width as f32)
                .min(height / info.height as f32)
                .min(1.0);
            (
                (info.width as f32 * scale).floor().max(1.0),
                (info.height as f32 * scale).floor().max(1.0),
            )
        } else {
            (self.width.min(320.0), image_height.min(64.0))
        };
        self.reserve(reference.offset, height)?;
        let rect = Rect::new(
            (self.x + (self.width - width) / 2.0).round() as i32,
            self.y.round() as i32,
            width as u32,
            height as u32,
        );
        self.pages
            .last_mut()
            .expect("initial page exists")
            .items
            .push(Item::Image { index, rect });
        self.y = (self.y + height + 6.0).min(self.height);
        Ok(())
    }
    fn gap(&mut self, size: f32) {
        if self.pages.last().is_some_and(|page| !page.items.is_empty()) {
            self.y = (self.y + size).min(self.height);
        }
    }
}
#[derive(Default, Clone)]
struct PendingLine {
    glyphs: Vec<PositionedGlyph>,
    start: Option<usize>,
    end: usize,
    width: f32,
    indent: f32,
    ascent: f32,
    height: f32,
    align: TextAlign,
}

pub(crate) struct EpubRenderer<'f, 'd> {
    fonts: Fonts<'f, 'd>,
    layout: Option<Layout>,
    theme: Theme,
}
impl<'f, 'd> EpubRenderer<'f, 'd> {
    pub(crate) fn new(font: &'f Font<'d>, size: u32, allow_missing: bool) -> Result<Self> {
        if !(8..=256).contains(&size)
            || font.metrics().ascender <= 0
            || font.metrics().descender > 0
        {
            return Err("unsupported EPUB font size or horizontal metrics".into());
        }
        Ok(Self {
            fonts: Fonts::new(font, size, allow_missing),
            layout: None,
            theme: Theme::Paper,
        })
    }
    pub(crate) fn with_preferences(mut self, theme: Theme, line_spacing: f32) -> Self {
        self.theme = theme;
        self.fonts.line_spacing = line_spacing.clamp(0.8, 2.0);
        self.layout = None;
        self
    }
    pub(crate) fn with_fallbacks(mut self, fallbacks: &[&'f Font<'d>]) -> Self {
        self.fonts.fallbacks = fallbacks.iter().copied().take(12).collect();
        self
    }
    pub(crate) fn render(
        &mut self,
        chapter: &Chapter<'_, '_>,
        options: &Options,
    ) -> Result<RenderedPage> {
        self.render_with_image(chapter, options, None)
    }
    pub(crate) fn render_with_image(
        &mut self,
        chapter: &Chapter<'_, '_>,
        options: &Options,
        image_target: Option<usize>,
    ) -> Result<RenderedPage> {
        if !(128..=4096).contains(&options.width)
            || !(128..=4096).contains(&options.height)
            || options.margin >= options.width / 2
            || options.margin >= options.height / 2
            || options.size != self.fonts.base
            || options.allow_missing != self.fonts.allow_missing
            || (options.page.is_some() && options.at.is_some())
        {
            return Err("invalid EPUB page geometry or renderer options".into());
        }
        self.fonts.use_chapter(chapter);
        for cache in self.fonts.caches.values_mut() {
            cache.begin_page();
        }
        let key = Key {
            book: chapter.identity.0,
            spine: chapter.identity.1,
            width: options.width,
            height: options.height,
            margin: options.margin,
        };
        if self.layout.as_ref().is_none_or(|layout| layout.key != key) {
            self.layout = Some(build(chapter, options, &mut self.fonts, key)?);
        }
        let layout = self
            .layout
            .as_ref()
            .ok_or("EPUB layout was not initialized")?;
        let page = if let Some(image) = image_target {
            layout
                .pages
                .iter()
                .position(|page| {
                    page.items
                        .iter()
                        .any(|item| matches!(item, Item::Image { index, .. } if *index == image))
                })
                .ok_or("image target is outside this chapter layout")?
        } else if let Some(locator) = &options.at {
            let offset = chapter.restore(locator)?;
            let next = layout.pages.partition_point(|page| page.start < offset);
            if layout
                .pages
                .get(next)
                .is_some_and(|page| page.start == offset)
            {
                next
            } else {
                next.saturating_sub(1)
            }
        } else {
            options.page.unwrap_or(0)
        };
        let current = layout
            .pages
            .get(page)
            .ok_or("page is outside this EPUB chapter")?;
        let mut surface = Surface::new(options.width, options.height, RenderLimits::default())?;
        surface.draw(&[DrawCommand::FillRect {
            rect: Rect::new(0, 0, options.width, options.height),
            color: self.theme.colors().0,
        }])?;
        let margin = options.margin as i32;
        let clip = Rect::new(
            margin,
            margin,
            options.width - options.margin * 2,
            options.height - options.margin * 2,
        );
        let mut decorations = Vec::new();
        for painted in &current.decorations {
            painted.commands(margin, clip, &mut decorations);
        }
        surface.draw(&decorations)?;
        let (mut raster_work, mut blend_work, mut glyphs) = (0_u64, 0_u64, 0_usize);
        let mut hits = Vec::new();
        let mut image_hits = Vec::new();
        for (item_index, item) in current.items.iter().enumerate() {
            crate::loading::step("绘制页面文字与图片", item_index, current.items.len())?;
            match item {
                Item::Text(line) => {
                    for glyph in &line.glyphs {
                        if glyph.source.start < line.range.start
                            || glyph.source.end > line.range.end
                        {
                            return Err("shaped cluster outside source line".into());
                        }
                        let x = line.x + glyph.x;
                        let rect = Rect::new(
                            margin + x.round() as i32,
                            margin + (line.baseline - glyph.ascent).floor() as i32,
                            glyph.advance.ceil().max(1.0) as u32,
                            glyph.height.ceil().max(1.0) as u32,
                        )
                        .intersection(clip);
                        hits.push(TextHit {
                            rect,
                            start: glyph.source.start,
                            end: glyph.source.end,
                        });
                        if let Some(index) = glyph.glyph {
                            glyphs += 1;
                            if glyphs > 200_000 {
                                return Err("EPUB visible glyph budget exceeded".into());
                            }
                            let cache = self.fonts.cache_index(glyph.face, glyph.size);
                            let before = cache.raster_work;
                            let mask = cache.mask_index(index)?;
                            blend_work = blend_work.saturating_add(
                                u64::from(mask.width())
                                    * u64::from(mask.height())
                                    * u64::from(glyph.bold + 1),
                            );
                            if blend_work > 64 * 1024 * 1024 {
                                return Err("EPUB glyph blending budget exceeded".into());
                            }
                            surface.draw_glyph_emphasis(
                                mask,
                                (
                                    margin + x.round() as i32,
                                    margin + (line.baseline + glyph.y).round() as i32,
                                ),
                                if glyph.style.backdrop.is_some() {
                                    let [r, g, b] = glyph.style.color;
                                    Color::rgba(r, g, b, 255)
                                } else {
                                    self.theme.text_color(glyph.style.color)
                                },
                                clip,
                                glyph.bold,
                                glyph.italic,
                            )?;
                            raster_work += cache.raster_work - before;
                            if raster_work > 128 * 1024 * 1024 {
                                return Err("EPUB aggregate raster work budget exceeded".into());
                            }
                        }
                    }
                }
                Item::Image { index, rect } => {
                    let rect = Rect::new(rect.x + margin, rect.y + margin, rect.width, rect.height);
                    image_hits.push((rect, *index));
                    let svg_fonts: Vec<_> = std::iter::once(self.fonts.font.data())
                        .chain(self.fonts.fallbacks.iter().map(|font| font.data()))
                        .take(12)
                        .collect();
                    if let Some(image) = chapter
                        .images
                        .borrow_mut()
                        .get_with_fonts(*index, &svg_fonts)
                    {
                        surface.draw_rgba(
                            image.pixels(),
                            (image.width(), image.height()),
                            rect,
                            clip,
                        )?;
                    } else {
                        surface.draw(&[DrawCommand::FillRect {
                            rect: rect.intersection(clip),
                            color: Color::rgba(232, 234, 238, 255),
                        }])?;
                        // Missing optional images must not make text navigation fail, even
                        // when their alt text cannot be represented by the selected font.
                        let label = format!("[Image] {}", chapter.content.images[*index].alt);
                        let mut x = rect.x + 8;
                        for ch in label.chars().take(80) {
                            if self.fonts.font.glyph_index(ch)? == 0 {
                                continue;
                            }
                            let cache = self.fonts.cache(14);
                            let advance = cache.advance(ch)?;
                            if x as f32 + advance > (rect.x + rect.width as i32 - 8) as f32 {
                                break;
                            }
                            surface.draw_glyph(
                                cache.mask(ch)?,
                                (x, rect.y + 24),
                                Color::rgba(96, 100, 110, 255),
                                rect.intersection(clip),
                            )?;
                            x += advance.round() as i32;
                        }
                    }
                }
            }
        }
        crate::loading::step(
            "绘制页面文字与图片",
            current.items.len(),
            current.items.len(),
        )?;
        let mut missing: Vec<_> = self
            .fonts
            .caches
            .values()
            .flat_map(|cache| cache.missing().iter().copied())
            .collect();
        missing.sort_unstable();
        missing.dedup();
        Ok(RenderedPage {
            surface,
            page,
            pages: layout.pages.len(),
            locator: chapter.locator(current.start)?,
            image_index: match current.items.first() {
                Some(Item::Image { index, .. }) => Some(*index),
                _ => None,
            },
            hits,
            image_hits,
            missing,
            cached_masks: self
                .fonts
                .caches
                .values()
                .map(GlyphCache::cached_masks)
                .sum(),
        })
    }
}

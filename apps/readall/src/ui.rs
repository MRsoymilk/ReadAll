//! Dependency-free native UI text drawn through ReadAll's own TrueType rasterizer.
use crate::font_select;
use readall_core::read_bounded;
use readall_font::{Font, FontLimits};
use readall_platform::LocalFileSource;
use readall_render::{
    Color, Rect, Surface,
    glyph::{GlyphMask, RasterLimits, rasterize},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    error::Error,
    path::{Path, PathBuf},
};

pub(crate) type UiResult<T> = Result<T, Box<dyn Error>>;

const UI_SAMPLE: &str =
    "ReadAll 阅读器 书库 打开图书 返回 上一级 当前阅读 章节 页码 字号 目录 最近阅读 设置 EPUB TXT";

struct CachedGlyph {
    advance: f32,
    mask: GlyphMask,
}

pub(crate) struct UiFont {
    bytes: Vec<u8>,
    path: PathBuf,
    face: u32,
    cache: RefCell<HashMap<(u32, char), CachedGlyph>>,
}

impl UiFont {
    pub(crate) fn system() -> UiResult<Self> {
        let path = font_select::find_for_text(UI_SAMPLE)?;
        Self::load(path)
    }

    pub(crate) fn load(path: PathBuf) -> UiResult<Self> {
        let limits = FontLimits::default();
        let mut source = LocalFileSource::open(&path)?;
        let bytes = read_bounded(&mut source, limits.max_file_bytes)?;
        Self::from_bytes_face(bytes, path, 0)
    }

    #[cfg(test)]
    pub(crate) fn from_bytes(bytes: Vec<u8>, path: PathBuf) -> UiResult<Self> {
        Self::from_bytes_face(bytes, path, 0)
    }

    pub(crate) fn from_bytes_face(bytes: Vec<u8>, path: PathBuf, face: u32) -> UiResult<Self> {
        Font::parse(&bytes, face, FontLimits::default())?;
        Ok(Self {
            bytes,
            path,
            face,
            cache: RefCell::new(HashMap::new()),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

pub(crate) struct UiPainter<'font, 'surface> {
    surface: &'surface mut Surface,
    source: &'font UiFont,
    font: Font<'font>,
}

impl<'font, 'surface> UiPainter<'font, 'surface> {
    pub(crate) fn new(font: &'font UiFont, surface: &'surface mut Surface) -> UiResult<Self> {
        Ok(Self {
            surface,
            source: font,
            font: Font::parse(&font.bytes, font.face, FontLimits::default())?,
        })
    }

    pub(crate) fn draw(
        &mut self,
        x: i32,
        y: i32,
        size: u32,
        text: &str,
        color: Color,
    ) -> UiResult<u32> {
        if !(8..=96).contains(&size) {
            return Err("UI font size must be 8..96".into());
        }
        let scale = size as f32 / f32::from(self.font.metrics().units_per_em);
        let ascender = f32::from(self.font.metrics().ascender) * scale;
        let baseline_y = (y as f32 + ascender).round() as i32;
        let clip = Rect::new(0, 0, self.surface.width(), self.surface.height());
        let mut cursor = x as f32;

        for ch in text.chars() {
            if ch == '\n' {
                continue;
            }
            let key = (size, ch);
            if !self.source.cache.borrow().contains_key(&key) {
                let glyph_index = self.font.glyph_index(ch)?;
                let glyph = self.font.glyph(glyph_index)?;
                let advance = f32::from(glyph.metrics.advance_width) * scale;
                let mask = rasterize(&glyph.outline, scale, RasterLimits::default())?;
                self.source
                    .cache
                    .borrow_mut()
                    .insert(key, CachedGlyph { advance, mask });
            }
            let cache = self.source.cache.borrow();
            let glyph = cache.get(&key).ok_or("UI glyph cache failure")?;
            self.surface.draw_glyph(
                &glyph.mask,
                (cursor.round() as i32, baseline_y),
                color,
                clip,
            )?;
            cursor += glyph.advance;
        }
        Ok((cursor - x as f32).max(0.0).ceil() as u32)
    }

    pub(crate) fn measure(&self, size: u32, text: &str) -> UiResult<u32> {
        if !(8..=96).contains(&size) {
            return Err("UI font size must be 8..96".into());
        }
        let scale = size as f32 / f32::from(self.font.metrics().units_per_em);
        let mut width = 0.0_f32;
        for ch in text.chars() {
            if ch == '\n' {
                continue;
            }
            let glyph_index = self.font.glyph_index(ch)?;
            width += f32::from(self.font.horizontal_metrics(glyph_index)?.advance_width) * scale;
        }
        Ok(width.max(0.0).ceil() as u32)
    }

    pub(crate) fn fit(&self, size: u32, text: &str, max_width: u32) -> UiResult<String> {
        if self.measure(size, text)? <= max_width {
            return Ok(text.to_owned());
        }
        let suffix = "...";
        let suffix_width = self.measure(size, suffix)?;
        if suffix_width >= max_width {
            return Ok(suffix.into());
        }
        let scale = size as f32 / f32::from(self.font.metrics().units_per_em);
        let mut width = 0.0_f32;
        let mut result = String::new();
        for ch in text.chars() {
            let glyph_index = self.font.glyph_index(ch)?;
            let advance =
                f32::from(self.font.horizontal_metrics(glyph_index)?.advance_width) * scale;
            if width + advance + suffix_width as f32 > max_width as f32 {
                break;
            }
            width += advance;
            result.push(ch);
        }
        result.push_str(suffix);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_font;
    use readall_render::RenderLimits;

    fn fixture_font() -> UiFont {
        UiFont::from_bytes(test_font::make_font(), PathBuf::from("fixture.ttf")).unwrap()
    }

    #[test]
    fn true_type_ui_text_draws_antialiased_pixels() {
        let font = fixture_font();
        let mut surface = Surface::new(240, 80, RenderLimits::default()).unwrap();
        let mut painter = UiPainter::new(&font, &mut surface).unwrap();
        let width = painter
            .draw(8, 8, 24, "AAAA", Color::rgba(30, 30, 30, 255))
            .unwrap();
        assert!(width > 0);
        assert!(surface.pixels().iter().any(|pixel| pixel.a != 0));
    }

    #[test]
    fn fitting_preserves_unicode_instead_of_question_mark_substitution() {
        let font = fixture_font();
        let mut surface = Surface::new(240, 80, RenderLimits::default()).unwrap();
        let painter = UiPainter::new(&font, &mut surface).unwrap();
        let fitted = painter.fit(16, "AéAAAA", 45).unwrap();
        assert!(!fitted.contains('?'));
        assert!(fitted.ends_with("..."));
    }
}

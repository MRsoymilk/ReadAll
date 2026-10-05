//! Dependency-free native UI text drawn through ReadAll's own TrueType rasterizer.
use readall_core::read_bounded;
use readall_font::{Font, FontLimits};
use readall_platform::LocalFileSource;
use readall_render::{
    Color, Rect, Surface,
    glyph::{GlyphMask, RasterLimits, rasterize_scaled},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    error::Error,
    path::{Path, PathBuf},
};

pub(crate) type UiResult<T> = Result<T, Box<dyn Error>>;
mod shapes;
pub(crate) use shapes::rounded;

const BUILTIN_FONT_FILE: &str = "LXGWWenKaiLite-Regular.ttf";
const BUILTIN_FONT_LABEL: &str = "<built-in>/LXGWWenKaiLite-Regular.ttf";

#[cfg(feature = "wayland")]
pub(crate) fn builtin_font_bytes() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/LXGWWenKaiLite-Regular.ttf"))
}

struct CachedGlyph {
    advance: f32,
    mask: GlyphMask,
}

pub(crate) struct UiFont {
    font: Font<'static>,
    path: PathBuf,
    cache: RefCell<HashMap<(u32, char, u32, u32), CachedGlyph>>,
}

impl UiFont {
    pub(crate) fn system() -> UiResult<Self> {
        if let Some(path) = std::env::var_os("READALL_UI_FONT").map(PathBuf::from) {
            return Self::load(path);
        }
        #[cfg(feature = "wayland")]
        {
            Self::from_bytes_face(
                builtin_font_bytes().to_vec(),
                PathBuf::from(BUILTIN_FONT_LABEL),
                0,
            )
        }
        #[cfg(not(feature = "wayland"))]
        Err("built-in GUI font is available only in GUI builds".into())
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
        let font = Font::from_shared(bytes.into(), face, FontLimits::default())?;
        Ok(Self {
            font,
            path,
            cache: RefCell::new(HashMap::new()),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn builtin_label() -> &'static str {
        BUILTIN_FONT_LABEL
    }

    pub(crate) fn builtin_file_name() -> &'static str {
        BUILTIN_FONT_FILE
    }
}

pub(crate) struct UiPainter<'font, 'surface> {
    surface: &'surface mut Surface,
    source: &'font UiFont,
    font: &'font Font<'static>,
}

impl<'font, 'surface> UiPainter<'font, 'surface> {
    pub(crate) fn new(font: &'font UiFont, surface: &'surface mut Surface) -> UiResult<Self> {
        Ok(Self {
            surface,
            source: font,
            font: &font.font,
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
        self.draw_clipped(
            x,
            y,
            size,
            text,
            color,
            Rect::new(0, 0, self.surface.width(), self.surface.height()),
        )
    }

    pub(crate) fn draw_clipped(
        &mut self,
        x: i32,
        y: i32,
        size: u32,
        text: &str,
        color: Color,
        clip: Rect,
    ) -> UiResult<u32> {
        if !(8..=96).contains(&size) {
            return Err("UI font size must be 8..96".into());
        }
        let scale = size as f32 / f32::from(self.font.metrics().units_per_em);
        let ascender = f32::from(self.font.metrics().ascender) * scale;
        let baseline_y = (y as f32 + ascender).round() as i32;
        let clip = clip.intersection(Rect::new(0, 0, self.surface.width(), self.surface.height()));
        let mut cursor = x as f32;

        for ch in text.chars() {
            if ch == '\n' {
                continue;
            }
            let density = self.surface.pixel_scale();
            let key = (size, ch, density.0.to_bits(), density.1.to_bits());
            if !self.source.cache.borrow().contains_key(&key) {
                let glyph_index = self.font.glyph_index(ch)?;
                let glyph = self.font.glyph(glyph_index)?;
                let advance = f32::from(glyph.metrics.advance_width) * scale;
                let mask = rasterize_scaled(
                    &glyph.outline,
                    (scale * density.0, scale * density.1),
                    RasterLimits::default(),
                )?;
                if self.source.cache.borrow().len() >= 4096 {
                    self.source.cache.borrow_mut().clear();
                }
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

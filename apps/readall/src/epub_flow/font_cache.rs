//! Face-aware glyph caches: author-family selection precedes reader font fallback.
use super::embedded::EmbeddedFace;
use super::*;
use readall_epub::css::FontFamilies;
use std::collections::HashMap;

pub(super) struct Fonts<'f, 'd> {
    pub(super) font: &'f Font<'d>,
    pub(super) base: u32,
    pub(super) line_spacing: f32,
    pub(super) allow_missing: bool,
    raster_scale: (f32, f32),
    pub(super) caches: BTreeMap<(usize, u32), GlyphCache<'f, 'd>>,
    pub(super) fallbacks: Vec<&'f Font<'d>>,
    embedded: Vec<EmbeddedFace>,
    families: FontFamilies,
    chapter: Option<(DocumentId, usize)>,
    choices: HashMap<(char, bool, bool, u16), usize>,
}
impl<'f, 'd> Fonts<'f, 'd> {
    pub(super) fn new(font: &'f Font<'d>, base: u32, allow_missing: bool) -> Self {
        Self {
            font,
            base,
            line_spacing: 1.0,
            allow_missing,
            raster_scale: (1.0, 1.0),
            caches: BTreeMap::new(),
            fallbacks: Vec::new(),
            embedded: Vec::new(),
            families: FontFamilies::default(),
            chapter: None,
            choices: HashMap::new(),
        }
    }
    pub(super) fn set_raster_scale(&mut self, scale: (f32, f32)) {
        self.raster_scale = scale;
        for cache in self.caches.values_mut() {
            cache.set_raster_scale(scale);
        }
    }
    pub(super) fn use_chapter(&mut self, chapter: &Chapter<'_, '_>) {
        if self.chapter == Some(chapter.identity) {
            return;
        }
        self.caches.clear();
        self.choices.clear();
        self.embedded = chapter.embedded.faces.clone();
        self.families = chapter.content.font_families.clone();
        self.chapter = Some(chapter.identity);
    }
    pub(super) fn size(&self, style: TextStyle) -> u32 {
        (self.base as f32 * style.font_scale)
            .round()
            .clamp(8.0, 256.0) as u32
    }
    pub(super) fn face(&self, index: usize) -> &Font<'d> {
        if index == 0 {
            self.font
        } else if index <= self.fallbacks.len() {
            self.fallbacks[index - 1]
        } else {
            &self.embedded[index - self.fallbacks.len() - 1].font
        }
    }
    /// Author descriptors, not internal font names/style bits, control synthetic style.
    pub(super) fn face_style(&self, index: usize) -> (bool, bool) {
        if index > self.fallbacks.len() {
            let face = &self.embedded[index - self.fallbacks.len() - 1];
            (face.declaration.bold, face.declaration.italic)
        } else {
            let font = self.face(index);
            (font.is_bold(), font.is_italic())
        }
    }
    fn reader_face(&self, text: &str, style: TextStyle) -> Option<usize> {
        std::iter::once(self.font)
            .chain(self.fallbacks.iter().copied())
            .enumerate()
            .filter(|(_, font)| {
                text.chars()
                    .filter(|ch| !super::shaping::ignorable(*ch))
                    .all(|ch| font.glyph_index(ch).is_ok_and(|glyph| glyph != 0))
            })
            .min_by_key(|(index, font)| {
                (
                    usize::from(font.is_italic() != style.italic) * 2
                        + usize::from(font.is_bold() != style.bold),
                    *index,
                )
            })
            .map(|(index, _)| index)
    }
    fn select(&self, text: &str, style: TextStyle) -> Option<usize> {
        for family in self.families.get(style.families) {
            if family.starts_with("<generic:") {
                if let Some(index) = self.reader_face(text, style) {
                    return Some(index);
                }
                continue;
            }
            // Within a family, style match wins; later declarations win ties.
            let matched = self
                .embedded
                .iter()
                .enumerate()
                .filter(|(_, face)| {
                    face.declaration.family == *family
                        && text
                            .chars()
                            .filter(|ch| !super::shaping::ignorable(*ch))
                            .all(|ch| {
                                face.declaration.covers(ch)
                                    && face.font.glyph_index(ch).is_ok_and(|glyph| glyph != 0)
                            })
                })
                .min_by_key(|(index, face)| {
                    (
                        usize::from(face.declaration.italic != style.italic) * 2
                            + usize::from(face.declaration.bold != style.bold),
                        std::cmp::Reverse(*index),
                    )
                });
            if let Some((index, _)) = matched {
                return Some(1 + self.fallbacks.len() + index);
            }
        }
        self.reader_face(text, style)
    }
    pub(super) fn choose(&mut self, ch: char, style: TextStyle) -> usize {
        let key = (ch, style.bold, style.italic, style.families);
        if let Some(index) = self.choices.get(&key) {
            return *index;
        }
        let mut bytes = [0; 4];
        let index = self.select(ch.encode_utf8(&mut bytes), style).unwrap_or(0);
        if self.choices.len() >= 8192 {
            self.choices.clear();
        }
        self.choices.insert(key, index);
        index
    }
    pub(super) fn choose_cluster(&mut self, text: &str, style: TextStyle) -> usize {
        let mut chars = text.chars().filter(|ch| !super::shaping::ignorable(*ch));
        let Some(first) = chars.next() else {
            return 0;
        };
        if chars.next().is_none() {
            return self.choose(first, style);
        }
        self.select(text, style)
            .unwrap_or_else(|| self.choose(first, style))
    }
    pub(super) fn synthetic(&mut self, ch: char, style: TextStyle) -> (bool, bool) {
        let index = self.choose(ch, style);
        let (bold, italic) = self.face_style(index);
        (style.bold && !bold, style.italic && !italic)
    }
    pub(super) fn cache(&mut self, size: u32) -> &mut GlyphCache<'f, 'd> {
        self.cache_index(0, size)
    }
    pub(super) fn cache_styled(
        &mut self,
        ch: char,
        size: u32,
        style: TextStyle,
    ) -> &mut GlyphCache<'f, 'd> {
        let index = self.choose(ch, style);
        self.cache_index(index, size)
    }
    pub(super) fn cache_index(&mut self, index: usize, size: u32) -> &mut GlyphCache<'f, 'd> {
        let key = (index, size);
        if !self.caches.contains_key(&key) {
            if self.caches.len() >= 16 {
                self.caches.pop_first();
            }
            // A clone retains shared author bytes or the original reader-font borrow.
            let mut cache = GlyphCache::owned(self.face(index).clone(), size, self.allow_missing);
            cache.set_mask_limit(2 * 1024 * 1024);
            cache.set_raster_scale(self.raster_scale);
            self.caches.insert(key, cache);
        }
        self.caches.get_mut(&key).expect("font cache just inserted")
    }
    pub(super) fn advance(&mut self, ch: char, size: u32, x: f32, style: TextStyle) -> Result<f32> {
        if ch == '\t' {
            let tab = (self.cache_styled(' ', size, style).advance(' ')? * 4.0).max(size as f32);
            return Ok(tab_advance(x, tab));
        }
        let synth = self.synthetic(ch, style).0;
        Ok(self.cache_styled(ch, size, style).advance(ch)?
            + if synth {
                (size as f32 / 24.0).round().clamp(1.0, 8.0)
            } else {
                0.0
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_font;
    use readall_font::FontLimits;
    #[test]
    fn style_selection_prefers_real_variant_and_keeps_cache_identity() {
        let regular = test_font::make_font();
        let mut bold = regular.clone();
        let count = u16::from_be_bytes([bold[4], bold[5]]) as usize;
        for table in 0..count {
            let at = 12 + table * 16;
            if &bold[at..at + 4] == b"head" {
                let start = u32::from_be_bytes(bold[at + 8..at + 12].try_into().unwrap()) as usize;
                bold[start + 44..start + 46].copy_from_slice(&1_u16.to_be_bytes());
            }
        }
        let regular = Font::parse(&regular, 0, FontLimits::default()).unwrap();
        let bold = Font::parse(&bold, 0, FontLimits::default()).unwrap();
        let mut fonts = Fonts::new(&regular, 16, false);
        fonts.fallbacks.push(&bold);
        let normal = TextStyle::default();
        let strong = TextStyle {
            bold: true,
            ..normal
        };
        assert_eq!(fonts.choose('A', normal), 0);
        assert_eq!(fonts.choose('A', strong), 1);
        fonts.cache_styled('A', 16, normal).mask('A').unwrap();
        fonts.cache_styled('A', 16, strong).mask('A').unwrap();
        assert_eq!(fonts.caches.len(), 2);
        assert_eq!(fonts.synthetic('A', strong), (false, false));
    }
}

//! Borrowed or shared immutable font tables. Shared views contain offsets, never
//! self-references, so publication fonts and glyph caches can own their bytes safely.
use crate::{Font, FontError, FontLimits, Result};
use std::{
    ops::{Deref, Range},
    sync::Arc,
};

#[derive(Debug, Clone)]
pub(crate) enum FontData<'a> {
    Borrowed(&'a [u8]),
    Shared {
        bytes: Arc<[u8]>,
        range: Range<usize>,
    },
}
impl Deref for FontData<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Borrowed(bytes) => bytes,
            Self::Shared { bytes, range } => &bytes[range.clone()],
        }
    }
}
impl FontData<'_> {
    pub(crate) fn shared(&self, root: &[u8], bytes: &Arc<[u8]>) -> Result<FontData<'static>> {
        let start = (self.as_ptr() as usize)
            .checked_sub(root.as_ptr() as usize)
            .ok_or(FontError::Invalid("font table outside shared storage"))?;
        let end = start
            .checked_add(self.len())
            .filter(|end| *end <= root.len() && *end <= bytes.len())
            .ok_or(FontError::Invalid("font table outside shared storage"))?;
        Ok(FontData::Shared {
            bytes: Arc::clone(bytes),
            range: start..end,
        })
    }
}
impl Font<'_> {
    /// Validate once, then retain immutable table views over one shared allocation.
    /// Cloning this font shares bytes, rather than copying or leaking a font file.
    pub fn from_shared(
        bytes: Arc<[u8]>,
        face_index: u32,
        limits: FontLimits,
    ) -> Result<Font<'static>> {
        let parsed = Font::parse(&bytes, face_index, limits)?;
        Ok(Font {
            bytes: parsed.bytes.shared(&bytes, &bytes)?,
            face_index: parsed.face_index,
            mac_style: parsed.mac_style,
            glyph_count: parsed.glyph_count,
            metrics_count: parsed.metrics_count,
            metrics: parsed.metrics,
            hmtx: parsed.hmtx.shared(&bytes, &bytes)?,
            loca: parsed.loca.shared(&bytes, &bytes)?,
            glyf: parsed.glyf.shared(&bytes, &bytes)?,
            long_loca: parsed.long_loca,
            cmap: parsed.cmap.shared(&bytes, &bytes)?,
            limits: parsed.limits,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    // Use the existing TrueType test data instead of bundling any third-party font.
    #[test]
    fn owned_font_views_survive_owner_and_clone_drops() {
        let bytes = crate::tests::fixture();
        let borrowed = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
        let expected: Vec<_> = ['A', 'W', ' ']
            .map(|ch| (borrowed.glyph_index(ch).unwrap(), borrowed.metrics()))
            .into();
        let shared: Arc<[u8]> = bytes.into();
        let owned = Font::from_shared(Arc::clone(&shared), 0, FontLimits::default()).unwrap();
        let cloned = owned.clone();
        drop(owned);
        drop(shared);
        for (ch, (index, metrics)) in ['A', 'W', ' '].into_iter().zip(expected) {
            assert_eq!(cloned.glyph_index(ch).unwrap(), index);
            assert_eq!(cloned.metrics(), metrics);
            cloned.glyph(index).unwrap();
        }
        let reparsed = Font::parse(cloned.data(), 0, FontLimits::default()).unwrap();
        assert_eq!(reparsed.glyph_count(), cloned.glyph_count());
    }
}

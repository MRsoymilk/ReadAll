//! Bounded, dependency-free TrueType outline reader. No shaping or hinting VM.
//! Format references: https://learn.microsoft.com/en-us/typography/opentype/spec/
mod binary;
mod cmap;
mod outline;
mod storage;
use storage::FontData;

use binary::{i16_at, offset_at, reserve, slice, u16_at, u32_at};
use cmap::Cmap;
pub use outline::{Glyph, Outline, Point};
use std::fmt;

type Result<T> = std::result::Result<T, FontError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontError {
    Invalid(&'static str),
    Unsupported(&'static str),
    LimitExceeded(&'static str),
    AllocationFailed,
}
impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(f, "invalid font: {reason}"),
            Self::Unsupported(reason) => write!(f, "unsupported font feature: {reason}"),
            Self::LimitExceeded(reason) => write!(f, "font budget exceeded: {reason}"),
            Self::AllocationFailed => f.write_str("cannot allocate font data"),
        }
    }
}
impl std::error::Error for FontError {}

#[derive(Debug, Clone, Copy)]
pub struct FontLimits {
    pub max_file_bytes: usize,
    pub max_points: usize,
    pub max_contours: usize,
    pub max_components: usize,
    pub max_depth: usize,
}
impl Default for FontLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 64 * 1024 * 1024,
            max_points: 65_536,
            max_contours: 16_384,
            max_components: 1024,
            max_depth: 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontMetrics {
    pub units_per_em: u16,
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HorizontalMetrics {
    pub advance_width: u16,
    pub left_side_bearing: i16,
}

#[derive(Debug, Clone)]
pub struct Font<'a> {
    bytes: FontData<'a>,
    face_index: u32,
    mac_style: u16,
    glyph_count: u16,
    metrics_count: u16,
    metrics: FontMetrics,
    hmtx: FontData<'a>,
    loca: FontData<'a>,
    glyf: FontData<'a>,
    long_loca: bool,
    cmap: Cmap<'a>,
    limits: FontLimits,
}
impl<'a> Font<'a> {
    /// Parses a standalone sfnt or one face of a TTC. Table offsets remain file-relative.
    /// Checks bounds/structure, not checksums, licensing, or authenticity.
    pub fn parse(bytes: &'a [u8], face_index: u32, limits: FontLimits) -> Result<Self> {
        if bytes.len() > limits.max_file_bytes {
            return Err(FontError::LimitExceeded("font file size"));
        }
        if limits.max_depth == 0
            || limits.max_depth > 32
            || limits.max_points > 1_000_000
            || limits.max_contours > 100_000
            || limits.max_components > 65_536
        {
            return Err(FontError::Invalid("unsafe font limit configuration"));
        }
        let offset = if slice(bytes, 0, 4)? == b"ttcf" {
            if !matches!(u32_at(bytes, 4)?, 0x00010000 | 0x00020000) {
                return Err(FontError::Unsupported("TTC version"));
            }
            let count = u32_at(bytes, 8)?;
            if count == 0 || count > 1024 {
                return Err(FontError::LimitExceeded("TTC face count"));
            }
            slice(bytes, 12, count as usize * 4)?;
            if face_index >= count {
                return Err(FontError::Invalid("TTC face index out of range"));
            }
            offset_at(bytes, 12 + face_index as usize * 4)?
        } else {
            if face_index != 0 {
                return Err(FontError::Invalid("standalone font has only face zero"));
            }
            0
        };
        let directory = bytes
            .get(offset..)
            .ok_or(FontError::Invalid("font directory offset"))?;
        match slice(directory, 0, 4)? {
            b"\x00\x01\x00\x00" | b"true" => {}
            b"OTTO" => return Err(FontError::Unsupported("CFF/CFF2 outlines")),
            b"wOFF" | b"wOF2" => return Err(FontError::Unsupported("WOFF/WOFF2 containers")),
            _ => return Err(FontError::Invalid("sfnt signature")),
        }
        let count = usize::from(u16_at(directory, 4)?);
        if count == 0 || count > 256 {
            return Err(FontError::LimitExceeded("table count"));
        }
        slice(directory, 12, count * 16)?;
        let mut tables: Vec<([u8; 4], &'a [u8])> = Vec::new();
        reserve(&mut tables, count)?;
        for i in 0..count {
            let at = 12 + i * 16;
            let tag: [u8; 4] = slice(directory, at, 4)?
                .try_into()
                .map_err(|_| FontError::Invalid("table tag"))?;
            if tables.iter().any(|(existing, _)| *existing == tag) {
                return Err(FontError::Invalid("duplicate table"));
            }
            if tag == *b"fvar" {
                return Err(FontError::Unsupported("variable fonts"));
            }
            let data = slice(
                bytes,
                offset_at(directory, at + 8)?,
                offset_at(directory, at + 12)?,
            )?;
            tables.push((tag, data));
        }
        let table = |tag: &[u8; 4]| {
            tables
                .iter()
                .find(|(name, _)| name == tag)
                .map(|(_, data)| *data)
                .ok_or(FontError::Invalid("required font table missing"))
        };
        let head = table(b"head")?;
        slice(head, 0, 54)?;
        if u32_at(head, 12)? != 0x5f0f3cf5 {
            return Err(FontError::Invalid("head magic"));
        }
        let units_per_em = u16_at(head, 18)?;
        if !(16..=16384).contains(&units_per_em) {
            return Err(FontError::Invalid("units per em"));
        }
        let long_loca = match i16_at(head, 50)? {
            0 => false,
            1 => true,
            _ => return Err(FontError::Invalid("loca format")),
        };
        let maxp = table(b"maxp")?;
        slice(maxp, 0, 32)?;
        if u32_at(maxp, 0)? != 0x00010000 {
            return Err(FontError::Invalid("TrueType maxp version"));
        }
        let glyph_count = u16_at(maxp, 4)?;
        if glyph_count == 0 {
            return Err(FontError::Invalid("empty glyph set"));
        }
        let hhea = table(b"hhea")?;
        slice(hhea, 0, 36)?;
        if u32_at(hhea, 0)? != 0x00010000 || i16_at(hhea, 32)? != 0 {
            return Err(FontError::Invalid("hhea version or metric format"));
        }
        let metrics_count = u16_at(hhea, 34)?;
        if metrics_count == 0 || metrics_count > glyph_count {
            return Err(FontError::Invalid("horizontal metric count"));
        }
        let metrics = FontMetrics {
            units_per_em,
            ascender: i16_at(hhea, 4)?,
            descender: i16_at(hhea, 6)?,
            line_gap: i16_at(hhea, 8)?,
        };
        let hmtx = table(b"hmtx")?;
        slice(
            hmtx,
            0,
            usize::from(metrics_count) * 4 + usize::from(glyph_count - metrics_count) * 2,
        )?;
        let loca = table(b"loca")?;
        slice(
            loca,
            0,
            (usize::from(glyph_count) + 1) * if long_loca { 4 } else { 2 },
        )?;
        let glyf = table(b"glyf")?;
        let cmap = Cmap::parse(table(b"cmap")?, glyph_count)?;
        let font = Self {
            bytes: FontData::Borrowed(bytes),
            face_index,
            mac_style: u16_at(head, 44)?,
            glyph_count,
            metrics_count,
            metrics,
            hmtx: FontData::Borrowed(hmtx),
            loca: FontData::Borrowed(loca),
            glyf: FontData::Borrowed(glyf),
            long_loca,
            cmap,
            limits,
        };
        let mut previous = 0;
        for i in 0..=usize::from(glyph_count) {
            let position = font.glyph_offset(i)?;
            if position < previous || position > glyf.len() {
                return Err(FontError::Invalid("nonmonotonic or out-of-range loca"));
            }
            previous = position;
        }
        Ok(font)
    }
    pub fn data(&self) -> &[u8] {
        &self.bytes
    }
    pub fn face_index(&self) -> u32 {
        self.face_index
    }
    pub fn is_bold(&self) -> bool {
        self.mac_style & 1 != 0
    }
    pub fn is_italic(&self) -> bool {
        self.mac_style & 2 != 0
    }
    pub fn glyph_count(&self) -> u16 {
        self.glyph_count
    }
    pub fn metrics(&self) -> FontMetrics {
        self.metrics
    }
    pub fn glyph_index(&self, ch: char) -> Result<u16> {
        let glyph = self.cmap.glyph_index(ch)?;
        if glyph >= self.glyph_count {
            return Err(FontError::Invalid("cmap references absent glyph"));
        }
        Ok(glyph)
    }
    /// Raw hmtx values. Glyph::metrics additionally resolves USE_MY_METRICS on composites.
    pub fn horizontal_metrics(&self, glyph: u16) -> Result<HorizontalMetrics> {
        if glyph >= self.glyph_count {
            return Err(FontError::Invalid("glyph index out of range"));
        }
        let index = usize::from(glyph);
        let count = usize::from(self.metrics_count);
        let advance_width = u16_at(&self.hmtx, index.min(count - 1) * 4)?;
        let bearing_at = if index < count {
            index * 4 + 2
        } else {
            count * 4 + (index - count) * 2
        };
        Ok(HorizontalMetrics {
            advance_width,
            left_side_bearing: i16_at(&self.hmtx, bearing_at)?,
        })
    }
    pub fn glyph(&self, glyph: u16) -> Result<Glyph> {
        outline::decode(self, glyph)
    }
    fn glyph_offset(&self, index: usize) -> Result<usize> {
        if self.long_loca {
            offset_at(&self.loca, index * 4)
        } else {
            Ok(usize::from(u16_at(&self.loca, index * 2)?) * 2)
        }
    }
    fn glyph_bytes(&self, glyph: u16) -> Result<&[u8]> {
        if glyph >= self.glyph_count {
            return Err(FontError::Invalid("glyph index out of range"));
        }
        let start = self.glyph_offset(usize::from(glyph))?;
        let end = self.glyph_offset(usize::from(glyph) + 1)?;
        slice(&self.glyf, start, end - start)
    }
}

#[cfg(test)]
mod tests;

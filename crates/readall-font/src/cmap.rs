use crate::{
    FontError, Result,
    binary::{offset_at, slice, u16_at, u32_at},
    storage::FontData,
};

#[derive(Debug, Clone)]
pub(crate) enum Cmap<'a> {
    Segments { bytes: FontData<'a>, count: usize },
    Groups { bytes: FontData<'a>, count: usize },
}
impl<'a> Cmap<'a> {
    pub(crate) fn parse(table: &'a [u8], glyph_count: u16) -> Result<Self> {
        if u16_at(table, 0)? != 0 {
            return Err(FontError::Invalid("cmap version"));
        }
        let records = usize::from(u16_at(table, 2)?);
        slice(table, 4, records * 8)?;
        let mut selected = None;
        // Select ONE Unicode subtable; a full-repertoire map takes priority over BMP.
        for i in 0..records {
            let record = 4 + i * 8;
            let platform = u16_at(table, record)?;
            let encoding = u16_at(table, record + 2)?;
            if !(platform == 0 && encoding != 5 || platform == 3 && matches!(encoding, 1 | 10)) {
                continue;
            }
            let offset = offset_at(table, record + 4)?;
            let format = u16_at(table, offset)?;
            let score = match format {
                12 => 2,
                4 => 1,
                _ => continue,
            };
            if selected.is_none_or(|(old, _, _)| score > old) {
                selected = Some((score, offset, format));
            }
        }
        let (_, offset, format) = selected.ok_or(FontError::Unsupported(
            "Unicode cmap format 4 or 12 required",
        ))?;
        let tail = table
            .get(offset..)
            .ok_or(FontError::Invalid("cmap offset"))?;
        if format == 4 {
            let bytes = slice(tail, 0, usize::from(u16_at(tail, 2)?))?;
            let count_x2 = usize::from(u16_at(bytes, 6)?);
            if count_x2 == 0 || count_x2 % 2 != 0 {
                return Err(FontError::Invalid("cmap segment count"));
            }
            let count = count_x2 / 2;
            let arrays_end = 16 + count * 8;
            slice(bytes, 0, arrays_end)?;
            if u16_at(bytes, 14 + count * 2)? != 0 {
                return Err(FontError::Invalid("cmap reserved padding"));
            }
            let mut previous_end = None;
            for i in 0..count {
                let end = u16_at(bytes, 14 + i * 2)?;
                let start = u16_at(bytes, 16 + count * 2 + i * 2)?;
                if start > end || previous_end.is_some_and(|previous| start <= previous) {
                    return Err(FontError::Invalid("overlapping or unsorted cmap segments"));
                }
                previous_end = Some(end);
                let field = 16 + count * 6 + i * 2;
                let range = usize::from(u16_at(bytes, field)?);
                if range != 0 {
                    let first = field + range;
                    if range % 2 != 0 || first < arrays_end {
                        return Err(FontError::Invalid("cmap glyph array offset"));
                    }
                    slice(bytes, first, (usize::from(end - start) + 1) * 2)?;
                }
            }
            if u16_at(bytes, 14 + (count - 1) * 2)? != 0xffff
                || u16_at(bytes, 16 + count * 2 + (count - 1) * 2)? != 0xffff
            {
                return Err(FontError::Invalid("cmap sentinel is missing"));
            }
            Ok(Self::Segments {
                bytes: FontData::Borrowed(bytes),
                count,
            })
        } else {
            if u16_at(tail, 2)? != 0 {
                return Err(FontError::Invalid("cmap reserved field"));
            }
            let bytes = slice(tail, 0, offset_at(tail, 4)?)?;
            let count = offset_at(bytes, 12)?;
            let size = count
                .checked_mul(12)
                .ok_or(FontError::Invalid("cmap group size overflow"))?;
            slice(bytes, 16, size)?;
            let mut previous_end = None;
            for i in 0..count {
                let at = 16 + i * 12;
                let start = u32_at(bytes, at)?;
                let end = u32_at(bytes, at + 4)?;
                let glyph = u32_at(bytes, at + 8)?;
                if start > end
                    || end > 0x10ffff
                    || previous_end.is_some_and(|previous| start <= previous)
                {
                    return Err(FontError::Invalid("overlapping or invalid cmap groups"));
                }
                let last = glyph
                    .checked_add(end - start)
                    .ok_or(FontError::Invalid("cmap glyph overflow"))?;
                if last >= u32::from(glyph_count) {
                    return Err(FontError::Invalid("cmap references absent glyph"));
                }
                previous_end = Some(end);
            }
            Ok(Self::Groups {
                bytes: FontData::Borrowed(bytes),
                count,
            })
        }
    }

    pub(crate) fn shared(
        &self,
        root: &[u8],
        owner: &std::sync::Arc<[u8]>,
    ) -> Result<Cmap<'static>> {
        Ok(match self {
            Self::Segments { bytes, count } => Cmap::Segments {
                bytes: bytes.shared(root, owner)?,
                count: *count,
            },
            Self::Groups { bytes, count } => Cmap::Groups {
                bytes: bytes.shared(root, owner)?,
                count: *count,
            },
        })
    }

    pub(crate) fn glyph_index(&self, ch: char) -> Result<u16> {
        let code = u32::from(ch);
        match self {
            Self::Segments { bytes, count } => {
                let count = *count;
                if code > 0xffff {
                    return Ok(0);
                }
                let code = code as u16;
                let (mut lo, mut hi) = (0, count);
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    if u16_at(bytes, 14 + mid * 2)? < code {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                if lo == count {
                    return Ok(0);
                }
                let start = u16_at(bytes, 16 + count * 2 + lo * 2)?;
                if code < start {
                    return Ok(0);
                }
                let delta = u16_at(bytes, 16 + count * 4 + lo * 2)?;
                let field = 16 + count * 6 + lo * 2;
                let range = usize::from(u16_at(bytes, field)?);
                if range == 0 {
                    return Ok(code.wrapping_add(delta));
                }
                let glyph = u16_at(bytes, field + range + usize::from(code - start) * 2)?;
                Ok(if glyph == 0 {
                    0
                } else {
                    glyph.wrapping_add(delta)
                })
            }
            Self::Groups { bytes, count } => {
                let count = *count;
                let (mut lo, mut hi) = (0, count);
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    if u32_at(bytes, 20 + mid * 12)? < code {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                if lo == count {
                    return Ok(0);
                }
                let start = u32_at(bytes, 16 + lo * 12)?;
                if code < start {
                    return Ok(0);
                }
                let glyph = u32_at(bytes, 24 + lo * 12)? + (code - start);
                u16::try_from(glyph).map_err(|_| FontError::Invalid("cmap glyph overflow"))
            }
        }
    }
}

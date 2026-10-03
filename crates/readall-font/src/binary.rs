use crate::{FontError, Result};

pub(crate) fn slice(bytes: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    let end = offset
        .checked_add(length)
        .ok_or(FontError::Invalid("offset overflow"))?;
    bytes
        .get(offset..end)
        .ok_or(FontError::Invalid("truncated font data"))
}
pub(crate) fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    let p = slice(bytes, offset, 2)?;
    Ok(u16::from_be_bytes([p[0], p[1]]))
}
pub(crate) fn i16_at(bytes: &[u8], offset: usize) -> Result<i16> {
    Ok(u16_at(bytes, offset)? as i16)
}
pub(crate) fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    let p = slice(bytes, offset, 4)?;
    Ok(u32::from_be_bytes([p[0], p[1], p[2], p[3]]))
}
pub(crate) fn offset_at(bytes: &[u8], offset: usize) -> Result<usize> {
    usize::try_from(u32_at(bytes, offset)?)
        .map_err(|_| FontError::Invalid("offset cannot fit this platform"))
}
pub(crate) struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    pub(crate) fn new(bytes: &'a [u8], offset: usize) -> Self {
        Self { bytes, offset }
    }
    pub(crate) fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let result = slice(self.bytes, self.offset, count)?;
        self.offset += count;
        Ok(result)
    }
    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub(crate) fn u16(&mut self) -> Result<u16> {
        u16_at(self.take(2)?, 0)
    }
    pub(crate) fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    pub(crate) fn f2dot14(&mut self) -> Result<f32> {
        Ok(f32::from(self.i16()?) / 16384.0)
    }
}

pub(crate) fn reserve<T>(values: &mut Vec<T>, additional: usize) -> Result<()> {
    values
        .try_reserve(additional)
        .map_err(|_| FontError::AllocationFailed)
}

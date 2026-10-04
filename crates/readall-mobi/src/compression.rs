//! PalmDOC and HUFF/CDIC decompression with output, work, recursion and cache caps.
//! Format/algorithm cross-checked against foliate-js (MIT, John Factotum), see
//! licenses/foliate-js-MIT.txt. This implementation uses checked Rust slices.
use crate::{MobiBook, MobiError, Result, slice, u16be, u32be};
use std::sync::Arc;

pub(crate) fn strip_trailing(mut bytes: &[u8], flags: u16) -> Result<&[u8]> {
    for _ in 0..(flags >> 1).count_ones() {
        let mut length = 0_usize;
        let mut marker = false;
        let mut encoded = 0;
        for (i, &b) in bytes.iter().rev().take(4).enumerate() {
            length |= usize::from(b & 127) << (7 * i);
            encoded += 1;
            if b & 128 != 0 {
                marker = true;
                break;
            }
        }
        if !marker || length < encoded || length > bytes.len() {
            return Err(MobiError::Invalid("malformed trailing text-record entry"));
        }
        bytes = &bytes[..bytes.len() - length];
    }
    if flags & 1 != 0 {
        let n = usize::from(
            bytes
                .last()
                .ok_or(MobiError::Invalid("missing multibyte trailer"))?
                & 3,
        ) + 1;
        bytes = bytes
            .get(
                ..bytes
                    .len()
                    .checked_sub(n)
                    .ok_or(MobiError::Invalid("multibyte trailer length"))?,
            )
            .ok_or(MobiError::Invalid("multibyte trailer"))?;
    }
    Ok(bytes)
}
pub(crate) fn palmdoc(bytes: &[u8], limit: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(bytes.len().min(limit));
    let mut at = 0;
    while at < bytes.len() {
        let code = bytes[at];
        at += 1;
        match code {
            1..=8 => {
                let literal = slice(bytes, at, usize::from(code))?;
                append(&mut out, literal, limit)?;
                at += usize::from(code);
            }
            0 | 9..=127 => append(&mut out, &[code], limit)?,
            128..=191 => {
                let pair = u16::from(code) << 8
                    | u16::from(
                        *bytes
                            .get(at)
                            .ok_or(MobiError::Invalid("truncated PalmDOC back-reference"))?,
                    );
                at += 1;
                let distance = usize::from((pair & 0x3fff) >> 3);
                let length = usize::from(pair & 7) + 3;
                if distance == 0 || distance > out.len() {
                    return Err(MobiError::Invalid("PalmDOC back-reference distance"));
                }
                if out.len().saturating_add(length) > limit {
                    return Err(MobiError::Limit("PalmDOC output"));
                }
                for _ in 0..length {
                    out.push(out[out.len() - distance]);
                }
            }
            _ => append(&mut out, &[b' ', code ^ 128], limit)?,
        }
    }
    Ok(out)
}
fn append(out: &mut Vec<u8>, bytes: &[u8], limit: usize) -> Result<()> {
    if out.len().saturating_add(bytes.len()) > limit {
        return Err(MobiError::Limit("decompressed record bytes"));
    }
    out.try_reserve(bytes.len())
        .map_err(|_| MobiError::Limit("decompression allocation"))?;
    out.extend_from_slice(bytes);
    Ok(())
}
struct Phrase<'a> {
    bytes: &'a [u8],
    literal: bool,
    active: bool,
    cached: Option<Arc<[u8]>>,
}
pub(crate) struct Huff<'a> {
    quick: [u32; 256],
    ranges: [(u32, u32); 33],
    phrases: Vec<Phrase<'a>>,
    cached: usize,
    work: usize,
}
impl<'a> Huff<'a> {
    pub fn new(book: &MobiBook<'a>, start: usize, count: usize) -> Result<Self> {
        let header = book.record(start)?;
        if slice(header, 0, 8)? != b"HUFF\0\0\0\x18" {
            return Err(MobiError::Invalid("HUFF header"));
        }
        let first = u32be(header, 8)? as usize;
        let second = u32be(header, 12)? as usize;
        if first < 24 || second < 24 {
            return Err(MobiError::Invalid("HUFF table offsets"));
        }
        slice(header, first, 1024)?;
        slice(header, second, 256)?;
        let mut quick = [0; 256];
        for (i, slot) in quick.iter_mut().enumerate() {
            *slot = u32be(header, first + i * 4)?;
        }
        let mut ranges = [(0, 0); 33];
        for (i, slot) in ranges.iter_mut().enumerate().skip(1) {
            *slot = (
                u32be(header, second + (i - 1) * 8)?,
                u32be(header, second + (i - 1) * 8 + 4)?,
            );
        }
        let mut phrases = Vec::new();
        let mut total = None;
        for index in start + 1..start + count {
            let data = book.record(index)?;
            if slice(data, 0, 4)? != b"CDIC" {
                return Err(MobiError::Invalid("CDIC header"));
            }
            let base = u32be(data, 4)? as usize;
            let entries = u32be(data, 8)? as usize;
            let bits = u32be(data, 12)?;
            if base < 16 || bits > 16 || entries == 0 || entries > 65536 {
                return Err(MobiError::Limit("HUFF/CDIC phrase table"));
            }
            if total.is_some_and(|n| n != entries) || phrases.len() >= entries {
                return Err(MobiError::Invalid("inconsistent CDIC dictionary sizes"));
            }
            total = Some(entries);
            let n = (1_usize << bits).min(entries - phrases.len());
            let table = slice(
                data,
                base,
                data.len()
                    .checked_sub(base)
                    .ok_or(MobiError::Invalid("CDIC data offset"))?,
            )?;
            slice(table, 0, n * 2)?;
            for i in 0..n {
                let at = usize::from(u16be(table, i * 2)?);
                if at < n * 2 {
                    return Err(MobiError::Invalid("CDIC phrase overlaps offsets"));
                }
                let size = u16be(table, at)?;
                let bytes = slice(table, at + 2, usize::from(size & 0x7fff))?;
                phrases.push(Phrase {
                    bytes,
                    literal: size & 0x8000 != 0,
                    active: false,
                    cached: None,
                });
            }
        }
        if total != Some(phrases.len()) {
            return Err(MobiError::Invalid("incomplete CDIC dictionary"));
        }
        Ok(Self {
            quick,
            ranges,
            phrases,
            cached: 0,
            work: 0,
        })
    }
    pub fn decode(&mut self, bytes: &[u8], limit: usize) -> Result<Vec<u8>> {
        self.expand(bytes, limit, 0)
    }
    fn expand(&mut self, bytes: &[u8], limit: usize, depth: usize) -> Result<Vec<u8>> {
        if depth > 32 {
            return Err(MobiError::Limit("HUFF/CDIC recursion"));
        }
        let mut out = Vec::new();
        let bit_length = bytes
            .len()
            .checked_mul(8)
            .ok_or(MobiError::Limit("HUFF input bits"))?;
        let mut bit = 0;
        while bit < bit_length {
            self.work += 1;
            if self.work > 128 * 1024 * 1024 {
                return Err(MobiError::Limit("HUFF/CDIC symbol work"));
            }
            let start = bit / 8;
            let shift = bit % 8;
            let mut window = 0_u64;
            for i in 0..5 {
                window = window << 8 | u64::from(bytes.get(start + i).copied().unwrap_or(0));
            }
            let look = (window >> (8 - shift)) as u32;
            let quick = self.quick[(look >> 24) as usize];
            let mut len = (quick & 31) as usize;
            if len == 0 {
                return Err(MobiError::Invalid("zero Huffman code length"));
            }
            let mut maximum = quick >> 8;
            if quick & 128 == 0 {
                while len <= 32 && (look >> (32 - len)) < self.ranges[len].0 {
                    len += 1;
                }
                if len > 32 {
                    return Err(MobiError::Invalid("Huffman code outside ranges"));
                }
                maximum = self.ranges[len].1;
            }
            if bit + len > bit_length {
                break;
            } // An incomplete final code is bit padding.
            bit += len;
            let symbol = maximum
                .checked_sub(look >> (32 - len))
                .ok_or(MobiError::Invalid("invalid Huffman symbol"))?
                as usize;
            let phrase = self
                .phrases
                .get(symbol)
                .ok_or(MobiError::Invalid("HUFF phrase index outside CDIC"))?;
            if phrase.active {
                return Err(MobiError::Invalid("cyclic HUFF/CDIC phrase"));
            }
            if phrase.literal {
                append(&mut out, phrase.bytes, limit)?;
                continue;
            }
            if let Some(cache) = &phrase.cached {
                append(&mut out, cache, limit)?;
                continue;
            }
            let compressed = phrase.bytes;
            self.phrases[symbol].active = true;
            let expanded = self.expand(compressed, 65536.min(limit), depth + 1);
            self.phrases[symbol].active = false;
            let expanded = expanded?;
            append(&mut out, &expanded, limit)?;
            if self.cached.saturating_add(expanded.len()) <= 16 * 1024 * 1024 {
                self.cached += expanded.len();
                self.phrases[symbol].cached = Some(expanded.into());
            }
        }
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palmdoc_literal_shortcuts_overlaps_and_malformed_input() {
        assert_eq!(palmdoc(b"abc\x80\x1b\xc1", 20).unwrap(), b"abcabcabc A");
        assert_eq!(palmdoc(b"\x03\xff\x00\x80", 3).unwrap(), [255, 0, 128]);
        for data in [b"\x08a".as_slice(), b"\x80", b"\x80\0", b"\x80\x08"] {
            assert!(palmdoc(data, 20).is_err());
        }
        assert!(palmdoc(b"abc\x80\x1b", 8).is_err());
    }
    #[test]
    fn trailing_entries_are_stripped_without_underflow() {
        assert_eq!(strip_trailing(b"abcXYZ\x84", 2).unwrap(), b"abc");
        assert_eq!(strip_trailing(b"abc\0XYZ\x84", 3).unwrap(), b"abc");
        assert!(strip_trailing(b"x\xff", 2).is_err());
        assert!(strip_trailing(b"abc\x00", 2).is_err());
        assert!(strip_trailing(b"", 1).is_err());
    }
}

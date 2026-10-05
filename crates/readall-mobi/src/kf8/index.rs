//! Bounded KF8 INDX/TAGX/IDXT/CNCX decoding. Format reference: foliate-js
//! mobi.js (MIT, John Factotum); see licenses/foliate-js-MIT.txt.
use crate::{
    MobiBook, MobiError, Progress, Result, Stage, decode_text, progress, slice, u16be, u32be,
};
use std::collections::BTreeMap;

#[derive(Debug)]
pub(super) struct Entry {
    pub name: String,
    pub tags: BTreeMap<u8, Vec<u32>>,
}
impl Entry {
    pub fn value(&self, tag: u8, at: usize) -> Result<usize> {
        self.tags
            .get(&tag)
            .and_then(|v| v.get(at))
            .copied()
            .map(|v| v as usize)
            .ok_or(MobiError::Invalid("missing KF8 index value"))
    }
    pub fn optional(&self, tag: u8) -> Option<usize> {
        self.tags.get(&tag)?.first().map(|v| *v as usize)
    }
}
pub(super) struct Index {
    pub entries: Vec<Entry>,
    pub strings: BTreeMap<usize, String>,
}

fn header(data: &[u8]) -> Result<usize> {
    if slice(data, 0, 4)? != b"INDX" {
        return Err(MobiError::Invalid("KF8 INDX signature"));
    }
    let length = u32be(data, 4)? as usize;
    if length < 56 {
        return Err(MobiError::Invalid("KF8 INDX header length"));
    }
    slice(data, 0, length)?;
    Ok(length)
}
pub(super) fn varint(data: &[u8], at: &mut usize) -> Result<u32> {
    let mut value = 0_u32;
    for _ in 0..5 {
        let byte = *data
            .get(*at)
            .ok_or(MobiError::Invalid("truncated KF8 variable integer"))?;
        *at += 1;
        value = value
            .checked_mul(128)
            .and_then(|n| n.checked_add(u32::from(byte & 127)))
            .ok_or(MobiError::Invalid("KF8 variable integer overflow"))?;
        if byte & 128 != 0 {
            return Ok(value);
        }
    }
    Err(MobiError::Invalid("unterminated KF8 variable integer"))
}

pub(super) fn read(
    book: &MobiBook<'_>,
    record: usize,
    observer: &mut dyn FnMut(Progress) -> bool,
) -> Result<Index> {
    let main = book.record(record)?;
    let length = header(main)?;
    let count = u32be(main, 24)? as usize;
    let strings_count = u32be(main, 52)? as usize;
    if count > 1024
        || strings_count > 64
        || record
            .checked_add(count)
            .and_then(|n| n.checked_add(strings_count))
            .is_none_or(|end| end >= book.records.len())
    {
        return Err(MobiError::Limit("KF8 index records"));
    }
    let encoding = match u32be(main, 28)? {
        65001 | u32::MAX => 65001,
        1252 => 1252,
        _ => return Err(MobiError::Unsupported("KF8 index encoding")),
    };
    let tagx = main
        .get(length..)
        .ok_or(MobiError::Invalid("missing TAGX"))?;
    if slice(tagx, 0, 4)? != b"TAGX" {
        return Err(MobiError::Invalid("KF8 TAGX signature"));
    }
    let tag_length = u32be(tagx, 4)? as usize;
    let controls = u32be(tagx, 8)? as usize;
    if tag_length < 12
        || !(tag_length - 12).is_multiple_of(4)
        || tag_length > 1036
        || !(1..=32).contains(&controls)
    {
        return Err(MobiError::Invalid("KF8 TAGX dimensions"));
    }
    let descriptors = slice(tagx, 12, tag_length - 12)?;
    let mut result = Index {
        entries: Vec::new(),
        strings: BTreeMap::new(),
    };
    let mut string_bytes = 0;
    for i in 0..strings_count {
        progress(observer, Stage::Index, i, strings_count + count)?;
        let data = book.record(record + count + 1 + i)?;
        if data.len() > 65536 {
            return Err(MobiError::Limit("KF8 CNCX record bytes"));
        }
        let mut at = 0;
        while at < data.len() {
            // CNCX records from real writers may end with 0..3 alignment bytes.
            // Only accept zero padding to a four-byte boundary, not arbitrary junk.
            if data.len().is_multiple_of(4)
                && data.len() - at <= 3
                && data[at..].iter().all(|b| *b == 0)
            {
                break;
            }
            let offset = at;
            let n = varint(data, &mut at)? as usize;
            let value = decode_text(slice(data, at, n)?, encoding)?.into_owned();
            at += n;
            string_bytes += value.len();
            if string_bytes > 4 * 1024 * 1024 || result.strings.len() >= 100_000 {
                return Err(MobiError::Limit("KF8 index strings"));
            }
            result.strings.insert(i * 65536 + offset, value);
        }
    }
    let mut values_work = 0_usize;
    for i in 0..count {
        progress(
            observer,
            Stage::Index,
            strings_count + i,
            strings_count + count,
        )?;
        let data = book.record(record + 1 + i)?;
        let data_header = header(data)?;
        let idxt = u32be(data, 20)? as usize;
        let entries = u32be(data, 24)? as usize;
        if entries > 65536 || result.entries.len().saturating_add(entries) > 100_000 {
            return Err(MobiError::Limit("KF8 index entries"));
        }
        if idxt < data_header || slice(data, idxt, 4)? != b"IDXT" {
            return Err(MobiError::Invalid("KF8 IDXT signature/offset"));
        }
        slice(data, idxt + 4, entries * 2)?;
        for row in 0..entries {
            if row % 256 == 0 {
                progress(
                    observer,
                    Stage::Index,
                    strings_count + i,
                    strings_count + count,
                )?;
            }
            let start = usize::from(u16be(data, idxt + 4 + row * 2)?);
            let end = if row + 1 < entries {
                usize::from(u16be(data, idxt + 6 + row * 2)?)
            } else {
                idxt
            };
            if start < data_header || start >= end || end > idxt {
                return Err(MobiError::Invalid("KF8 index row offsets"));
            }
            let data = &data[start..end];
            let name_length = usize::from(data[0]);
            let name = decode_text(slice(data, 1, name_length)?, encoding)?.into_owned();
            let control = slice(data, 1 + name_length, controls)?;
            let mut at = 1 + name_length + controls;
            let mut control_at = 0;
            let mut pending = Vec::new();
            for description in descriptors.chunks_exact(4) {
                let [tag, number, mask, end] = [
                    description[0],
                    description[1],
                    description[2],
                    description[3],
                ];
                if end & 1 != 0 {
                    control_at += 1;
                    continue;
                }
                if mask == 0 || number == 0 {
                    return Err(MobiError::Invalid("KF8 TAGX mask/value count"));
                }
                let value = control
                    .get(control_at)
                    .ok_or(MobiError::Invalid("KF8 TAGX control overrun"))?
                    & mask;
                let bytes = if value == mask && mask.count_ones() > 1 {
                    Some(varint(data, &mut at)? as usize)
                } else {
                    None
                };
                let repeats = if value == mask {
                    1
                } else {
                    usize::from(value >> mask.trailing_zeros())
                };
                pending.push((tag, repeats * usize::from(number), bytes));
            }
            let mut tags = BTreeMap::new();
            for (tag, count, encoded_bytes) in pending {
                let mut values = Vec::new();
                if let Some(n) = encoded_bytes {
                    let end = at
                        .checked_add(n)
                        .filter(|end| *end <= data.len())
                        .ok_or(MobiError::Invalid("KF8 tag byte count"))?;
                    while at < end {
                        if values.len() >= 4096 {
                            return Err(MobiError::Limit("KF8 tag values"));
                        }
                        values.push(varint(&data[..end], &mut at)?);
                    }
                } else {
                    for _ in 0..count {
                        values.push(varint(data, &mut at)?);
                    }
                }
                values_work += values.len();
                if values_work > 1_000_000 {
                    return Err(MobiError::Limit("KF8 index value work"));
                }
                if !values.is_empty() && tags.insert(tag, values).is_some() {
                    return Err(MobiError::Invalid("duplicate KF8 index tag"));
                }
            }
            result.entries.push(Entry { name, tags });
        }
    }
    Ok(result)
}

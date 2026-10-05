//! Bounded, read-only MOBI6/7 and standalone AZW3/KF8 ingestion.
//! Existing dual-format MOBI uses its legacy part to preserve stored locations.
//! Produces a deterministic in-memory EPUB adapter, never changes the source.
mod compression;
mod html;
mod kf8;
mod package;
#[cfg(test)]
#[path = "../../../apps/readall/tests/support/mobi.rs"]
mod test_mobi;
#[cfg(test)]
mod tests;
use std::{borrow::Cow, error::Error, fmt, ops::Range};

#[derive(Debug, Clone, Copy)]
pub struct MobiLimits {
    pub max_file_bytes: usize,
    pub max_text_bytes: usize,
    pub max_package_bytes: usize,
    pub max_image_bytes: usize,
    pub max_sections: usize,
    pub max_tokens: usize,
}
impl Default for MobiLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 128 * 1024 * 1024,
            max_text_bytes: 32 * 1024 * 1024,
            max_package_bytes: 192 * 1024 * 1024,
            max_image_bytes: 16 * 1024 * 1024,
            max_sections: 1024,
            max_tokens: 500_000,
        }
    }
}
#[derive(Debug)]
pub enum MobiError {
    Invalid(&'static str),
    Unsupported(&'static str),
    Limit(&'static str),
    Encrypted,
    Cancelled,
}
impl fmt::Display for MobiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(s) => write!(f, "invalid MOBI: {s}"),
            Self::Unsupported(s) => write!(f, "unsupported MOBI: {s}"),
            Self::Limit(s) => write!(f, "MOBI resource budget exceeded: {s}"),
            Self::Encrypted => f.write_str(
                "MOBI is encrypted/DRM-protected; ReadAll does not decrypt protected books",
            ),
            Self::Cancelled => f.write_str("MOBI loading cancelled"),
        }
    }
}
impl Error for MobiError {}
pub type Result<T> = std::result::Result<T, MobiError>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Decompress,
    Index,
    Markup,
    Package,
}
#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub stage: Stage,
    pub done: usize,
    pub total: usize,
}
pub(crate) fn progress(
    observer: &mut dyn FnMut(Progress) -> bool,
    stage: Stage,
    done: usize,
    total: usize,
) -> Result<()> {
    if observer(Progress { stage, done, total }) {
        Ok(())
    } else {
        Err(MobiError::Cancelled)
    }
}
#[derive(Debug, Clone)]
pub struct Metadata {
    pub title: String,
    pub author: Option<String>,
    pub language: Option<String>,
    pub version: u32,
    pub compression: u16,
    pub text_records: usize,
    pub text_bytes: usize,
    pub encoding: u32,
    pub dual_format: bool,
}
#[derive(Debug)]
pub struct ConvertedBook {
    pub epub: Vec<u8>,
    pub metadata: Metadata,
    pub warnings: Vec<String>,
}
#[derive(Debug)]
pub struct MobiBook<'a> {
    data: &'a [u8],
    records: Vec<Range<usize>>,
    metadata: Metadata,
    limits: MobiLimits,
    image_start: Option<usize>,
    cover: Option<usize>,
    huff: Option<(usize, usize)>,
    trailing: u16,
    kf8: Option<kf8::Header>,
}
pub fn is_mobi(data: &[u8]) -> bool {
    data.get(60..68) == Some(b"BOOKMOBI")
}
pub(crate) fn slice(data: &[u8], at: usize, count: usize) -> Result<&[u8]> {
    data.get(
        at..at
            .checked_add(count)
            .ok_or(MobiError::Invalid("offset overflow"))?,
    )
    .ok_or(MobiError::Invalid("truncated record or invalid offset"))
}
pub(crate) fn u16be(data: &[u8], at: usize) -> Result<u16> {
    let b = slice(data, at, 2)?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}
pub(crate) fn u32be(data: &[u8], at: usize) -> Result<u32> {
    let b = slice(data, at, 4)?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

impl<'a> MobiBook<'a> {
    /// Metadata-only parsing does not decompress text or rasterize resources.
    pub fn parse(data: &'a [u8], limits: MobiLimits) -> Result<Self> {
        if data.len() > limits.max_file_bytes {
            return Err(MobiError::Limit("file bytes"));
        }
        if !is_mobi(data) {
            return Err(MobiError::Invalid("expected PalmDB BOOKMOBI signature"));
        }
        if u32be(data, 72)? != 0 {
            return Err(MobiError::Unsupported("chained PalmDB record lists"));
        }
        let count = usize::from(u16be(data, 76)?);
        if count < 2 {
            return Err(MobiError::Invalid("missing text records"));
        }
        let table_end = 78 + count * 8;
        slice(data, 0, table_end)?;
        let mut starts = Vec::with_capacity(count + 1);
        for i in 0..count {
            let offset = u32be(data, 78 + i * 8)? as usize;
            if offset < table_end
                || offset > data.len()
                || starts.last().is_some_and(|old| *old > offset)
            {
                return Err(MobiError::Invalid(
                    "PalmDB record offsets are not ordered/in bounds",
                ));
            }
            starts.push(offset);
        }
        starts.push(data.len());
        let records: Vec<_> = starts.windows(2).map(|p| p[0]..p[1]).collect();
        let header = &data[records[0].clone()];
        // Check encryption before interpreting any HTML or compressed text.
        if u16be(header, 12)? != 0 {
            return Err(MobiError::Encrypted);
        }
        if slice(header, 16, 4)? != b"MOBI" {
            return Err(MobiError::Invalid("missing MOBI header"));
        }
        let header_length = u32be(header, 20)? as usize;
        if header_length < 116 {
            return Err(MobiError::Unsupported("MOBI header shorter than 116 bytes"));
        }
        slice(header, 16, header_length)?;
        let field = |at: usize| -> Result<u32> {
            if at + 4 > 16 + header_length {
                return Ok(u32::MAX);
            }
            u32be(header, at)
        };
        if header_length >= 156 {
            let drm_count = field(168)?;
            if drm_count != 0 && drm_count != u32::MAX {
                return Err(MobiError::Encrypted);
            }
        }
        let version = field(36)?;
        if version > 8 {
            return Err(MobiError::Unsupported("MOBI version newer than KF8"));
        }
        let kf8 = if version == 8 {
            Some(kf8::Header::parse(header, header_length, count)?)
        } else {
            None
        };
        let compression = u16be(header, 0)?;
        if !matches!(compression, 1 | 2 | 17480) {
            return Err(MobiError::Unsupported("text compression method"));
        }
        let encoding = field(28)?;
        if !matches!(encoding, 65001 | 1252) {
            return Err(MobiError::Unsupported(
                "text encoding (supported: UTF-8 and Windows-1252)",
            ));
        }
        let text_bytes = u32be(header, 4)? as usize;
        let text_records = usize::from(u16be(header, 8)?);
        if text_bytes == 0 || text_records == 0 || text_records >= count {
            return Err(MobiError::Invalid("invalid text length/record count"));
        }
        if text_bytes > limits.max_text_bytes {
            return Err(MobiError::Limit("decompressed text bytes"));
        }
        let size = u16be(header, 10)?;
        if size == 0 {
            return Err(MobiError::Invalid("zero text record size"));
        }
        let name_offset = field(84)? as usize;
        let name_length = field(88)? as usize;
        let raw_title = if name_length == 0 || name_length == u32::MAX as usize {
            &data[..32]
        } else {
            slice(header, name_offset, name_length)?
        };
        let mut title = metadata_text(raw_title, encoding)?;
        let mut author = None;
        let language_code = field(92)? & 0xff;
        let mut language = match language_code {
            4 => Some("zh".into()),
            9 => Some("en".into()),
            17 => Some("ja".into()),
            18 => Some("ko".into()),
            7 => Some("de".into()),
            12 => Some("fr".into()),
            _ => None,
        };
        let mut cover = None;
        let mut dual_format = false;
        if field(128)? & 0x40 != 0 {
            let start = 16 + header_length;
            if slice(header, start, 4)? != b"EXTH" {
                return Err(MobiError::Invalid("missing EXTH metadata"));
            }
            let len = u32be(header, start + 4)? as usize;
            if len < 12 {
                return Err(MobiError::Invalid("EXTH length"));
            }
            let exth = slice(header, start, len)?;
            let n = u32be(exth, 8)? as usize;
            if n > 4096 {
                return Err(MobiError::Limit("EXTH metadata records"));
            }
            let mut at = 12;
            for _ in 0..n {
                let kind = u32be(exth, at)?;
                let len = u32be(exth, at + 4)? as usize;
                if len < 8 {
                    return Err(MobiError::Invalid("EXTH record length"));
                }
                let value = &slice(exth, at, len)?[8..];
                match kind {
                    100 if author.is_none() => author = Some(metadata_text(value, encoding)?),
                    503 => {
                        let s = metadata_text(value, encoding)?;
                        if !s.is_empty() {
                            title = s;
                        }
                    }
                    524 => language = Some(metadata_text(value, encoding)?),
                    122 if version == 8 && value.eq_ignore_ascii_case(b"true") => {
                        return Err(MobiError::Unsupported(
                            "fixed-layout KF8; reflowable AZW3 is supported",
                        ));
                    }
                    201 if value.len() == 4 => {
                        let n = u32be(value, 0)?;
                        if n != u32::MAX {
                            cover = Some(n as usize);
                        }
                    }
                    121 if value.len() == 4 => dual_format = u32be(value, 0)? != u32::MAX,
                    _ => {}
                }
                at += len;
            }
        }
        if title.is_empty() {
            title = "Untitled MOBI".into();
        }
        let image_start = match field(108)? {
            u32::MAX => None,
            n => Some(n as usize),
        };
        if image_start.is_some_and(|n| n <= text_records || n > count) {
            return Err(MobiError::Invalid(
                "image record range overlaps text or exceeds PalmDB",
            ));
        }
        let huff = if compression == 17480 {
            let (start, n) = (field(112)? as usize, field(116)? as usize);
            if !(2..=1024).contains(&n)
                || start <= text_records
                || start.checked_add(n).is_none_or(|end| end > count)
            {
                return Err(MobiError::Invalid("HUFF/CDIC record range"));
            }
            Some((start, n))
        } else {
            None
        };
        let trailing = if header_length >= 228 && version >= 5 {
            u16be(header, 242)?
        } else {
            0
        };
        Ok(Self {
            data,
            records,
            metadata: Metadata {
                title,
                author,
                language,
                version,
                compression,
                text_records,
                text_bytes,
                encoding,
                dual_format,
            },
            limits,
            image_start,
            cover,
            huff,
            trailing,
            kf8,
        })
    }
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    /// Encoded cover resource without decompressing MOBI/KF8 text or rebuilding EPUB.
    pub fn cover_image(&self) -> Result<Option<&'a [u8]>> {
        self.cover.map(|index| self.image(index)).transpose()
    }
    pub(crate) fn record(&self, index: usize) -> Result<&'a [u8]> {
        let range = self
            .records
            .get(index)
            .ok_or(MobiError::Invalid("record index outside PalmDB"))?;
        Ok(&self.data[range.clone()])
    }
    pub(crate) fn image(&self, index: usize) -> Result<&'a [u8]> {
        let start = self
            .image_start
            .ok_or(MobiError::Invalid("book has no image records"))?;
        let record = self.record(
            start
                .checked_add(index)
                .ok_or(MobiError::Invalid("image index overflow"))?,
        )?;
        if record.len() > self.limits.max_image_bytes {
            return Err(MobiError::Limit("single image bytes"));
        }
        Ok(record)
    }
    pub fn to_epub(&self) -> Result<ConvertedBook> {
        self.to_epub_with_progress(|_| true)
    }
    pub fn to_epub_with_progress(
        &self,
        mut observer: impl FnMut(Progress) -> bool,
    ) -> Result<ConvertedBook> {
        let text = self.text(&mut observer)?;
        if let Some(header) = &self.kf8 {
            return kf8::convert(self, header, &text, &mut observer);
        }
        let mut content =
            html::normalize(&text, self.metadata.encoding, self.limits, &mut observer)?;
        if self.metadata.dual_format {
            content.warnings.push("Dual MOBI/KF8: using the MOBI6/7 compatibility section; KF8-specific styling is not imported".into());
        }
        let epub = package::build(self, &mut content, &mut observer)?;
        Ok(ConvertedBook {
            epub,
            metadata: self.metadata.clone(),
            warnings: content.warnings,
        })
    }
    fn text(&self, observer: &mut dyn FnMut(Progress) -> bool) -> Result<Vec<u8>> {
        let mut result = Vec::new();
        result
            .try_reserve(self.metadata.text_bytes)
            .map_err(|_| MobiError::Limit("text allocation"))?;
        let mut huff = self
            .huff
            .map(|(at, n)| compression::Huff::new(self, at, n))
            .transpose()?;
        progress(observer, Stage::Decompress, 0, self.metadata.text_records)?;
        for i in 1..=self.metadata.text_records {
            let bytes = compression::strip_trailing(self.record(i)?, self.trailing)?;
            let allowance = (self.limits.max_text_bytes - result.len()).min(65536);
            let decoded = match self.metadata.compression {
                1 => {
                    if bytes.len() > allowance {
                        return Err(MobiError::Limit("text record size"));
                    }
                    Cow::Borrowed(bytes)
                }
                2 => Cow::Owned(compression::palmdoc(bytes, allowance)?),
                17480 => Cow::Owned(
                    huff.as_mut()
                        .ok_or(MobiError::Invalid("missing Huffman decoder"))?
                        .decode(bytes, allowance)?,
                ),
                _ => return Err(MobiError::Unsupported("compression")),
            };
            result.extend_from_slice(&decoded);
            progress(observer, Stage::Decompress, i, self.metadata.text_records)?;
        }
        if result.len() < self.metadata.text_bytes {
            return Err(MobiError::Invalid(
                "decompressed text is shorter than the declared length",
            ));
        }
        if result.len() - self.metadata.text_bytes > 65536 {
            return Err(MobiError::Invalid("excessive trailing decompressed text"));
        }
        result.truncate(self.metadata.text_bytes);
        // Validate the complete stream after joining records: UTF-8 characters may
        // straddle record boundaries and must not be decoded independently.
        if self.kf8.is_none()
            && self.metadata.encoding == 65001
            && std::str::from_utf8(&result).is_err()
        {
            return Err(MobiError::Invalid("text is not valid UTF-8"));
        }
        Ok(result)
    }
}
pub(crate) fn decode_text(bytes: &[u8], encoding: u32) -> Result<Cow<'_, str>> {
    if encoding == 65001 {
        return std::str::from_utf8(bytes)
            .map(Cow::Borrowed)
            .map_err(|_| MobiError::Invalid("invalid UTF-8"));
    }
    const CP1252: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    Ok(Cow::Owned(
        bytes
            .iter()
            .map(|&b| {
                if (0x80..0xa0).contains(&b) {
                    CP1252[(b - 0x80) as usize]
                } else {
                    char::from(b)
                }
            })
            .collect(),
    ))
}
fn metadata_text(bytes: &[u8], encoding: u32) -> Result<String> {
    if bytes.len() > 64 * 1024 {
        return Err(MobiError::Limit("metadata text bytes"));
    }
    let text = decode_text(bytes, encoding)?;
    Ok(html_escape::decode_html_entities(text.trim_matches('\0'))
        .chars()
        .filter(|ch| !ch.is_control())
        .take(4096)
        .collect::<String>()
        .trim()
        .to_owned())
}

//! Bounded ZIP reader for ReadAll document containers.
//! Supports stored and raw-DEFLATE entries without extracting paths to the filesystem.
mod deflate;

use std::{collections::HashSet, fmt, ops::Range};

type Result<T> = std::result::Result<T, ArchiveError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveError {
    Invalid(&'static str),
    Unsupported(&'static str),
    UnsupportedFlags(u16),
    LimitExceeded(&'static str),
    AllocationFailed,
    CrcMismatch,
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(f, "invalid ZIP archive: {reason}"),
            Self::Unsupported(reason) => write!(f, "unsupported ZIP feature: {reason}"),
            Self::UnsupportedFlags(flags) => write!(
                f,
                "unsupported ZIP feature: general-purpose flags 0x{flags:04x}"
            ),
            Self::LimitExceeded(what) => write!(f, "ZIP budget exceeded: {what}"),
            Self::AllocationFailed => f.write_str("cannot allocate ZIP data"),
            Self::CrcMismatch => f.write_str("ZIP entry CRC-32 mismatch"),
        }
    }
}
impl std::error::Error for ArchiveError {}

#[derive(Debug, Clone, Copy)]
pub struct ZipLimits {
    pub max_archive_bytes: usize,
    pub max_entries: usize,
    pub max_name_bytes: usize,
    pub max_entry_uncompressed: usize,
    pub max_total_uncompressed: usize,
}

impl Default for ZipLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 256 * 1024 * 1024,
            max_entries: 8192,
            max_name_bytes: 4096,
            max_entry_uncompressed: 128 * 1024 * 1024,
            max_total_uncompressed: 512 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ZipEntry {
    name: String,
    method: u16,
    compressed_size: usize,
    uncompressed_size: usize,
    crc32: u32,
    data: Range<usize>,
    local_offset: usize,
}

impl ZipEntry {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn method(&self) -> u16 {
        self.method
    }
    pub fn compressed_size(&self) -> usize {
        self.compressed_size
    }
    pub fn uncompressed_size(&self) -> usize {
        self.uncompressed_size
    }
    pub fn local_offset(&self) -> usize {
        self.local_offset
    }
    pub fn data_offset(&self) -> usize {
        self.data.start
    }
    pub fn is_directory(&self) -> bool {
        self.name.ends_with('/')
    }
}

#[derive(Debug)]
pub struct ZipArchive<'a> {
    bytes: &'a [u8],
    entries: Vec<ZipEntry>,
}

impl<'a> ZipArchive<'a> {
    pub fn parse(bytes: &'a [u8], limits: ZipLimits) -> Result<Self> {
        validate_limits(limits)?;
        if bytes.len() > limits.max_archive_bytes {
            return Err(ArchiveError::LimitExceeded("archive bytes"));
        }
        let eocd = find_eocd(bytes)?;
        let disk = le16(bytes, eocd + 4)?;
        let central_disk = le16(bytes, eocd + 6)?;
        let entries_on_disk = le16(bytes, eocd + 8)?;
        let entry_count = le16(bytes, eocd + 10)?;
        let central_size = le32(bytes, eocd + 12)?;
        let central_offset = le32(bytes, eocd + 16)?;
        if disk != 0 || central_disk != 0 || entries_on_disk != entry_count {
            return Err(ArchiveError::Unsupported("multi-disk archives"));
        }
        if entry_count == u16::MAX || central_size == u32::MAX || central_offset == u32::MAX {
            return Err(ArchiveError::Unsupported("ZIP64"));
        }

        let entry_count = usize::from(entry_count);
        if entry_count > limits.max_entries {
            return Err(ArchiveError::LimitExceeded("entry count"));
        }
        let central_offset = usize::try_from(central_offset)
            .map_err(|_| ArchiveError::Invalid("central directory offset overflow"))?;
        let central_size = usize::try_from(central_size)
            .map_err(|_| ArchiveError::Invalid("central directory size overflow"))?;
        let central_end = central_offset
            .checked_add(central_size)
            .ok_or(ArchiveError::Invalid("central directory range overflow"))?;
        if central_end != eocd {
            return Err(ArchiveError::Invalid(
                "central directory must immediately precede the end record",
            ));
        }

        let mut entries = Vec::new();
        reserve(&mut entries, entry_count)?;
        let mut names = HashSet::new();
        names
            .try_reserve(entry_count)
            .map_err(|_| ArchiveError::AllocationFailed)?;
        let mut spans = Vec::new();
        reserve(&mut spans, entry_count)?;
        let mut cursor = central_offset;
        let mut total_uncompressed = 0_usize;

        for _ in 0..entry_count {
            let header = slice(bytes, cursor, 46)?;
            if le32(header, 0)? != 0x0201_4b50 {
                return Err(ArchiveError::Invalid("central directory signature"));
            }
            let flags = le16(header, 8)?;
            let method = le16(header, 10)?;
            validate_flags(flags)?;
            if !matches!(method, 0 | 8) {
                return Err(ArchiveError::Unsupported("compression method"));
            }
            let crc32 = le32(header, 16)?;
            let compressed_size = le32(header, 20)?;
            let uncompressed_size = le32(header, 24)?;
            let name_length = usize::from(le16(header, 28)?);
            let extra_length = usize::from(le16(header, 30)?);
            let comment_length = usize::from(le16(header, 32)?);
            let start_disk = le16(header, 34)?;
            let local_offset = le32(header, 42)?;
            if compressed_size == u32::MAX
                || uncompressed_size == u32::MAX
                || local_offset == u32::MAX
                || start_disk == u16::MAX
            {
                return Err(ArchiveError::Unsupported("ZIP64"));
            }
            if start_disk != 0 {
                return Err(ArchiveError::Unsupported("multi-disk entry"));
            }
            if name_length == 0 || name_length > limits.max_name_bytes {
                return Err(ArchiveError::LimitExceeded("entry name"));
            }
            let record_size = 46_usize
                .checked_add(name_length)
                .and_then(|n| n.checked_add(extra_length))
                .and_then(|n| n.checked_add(comment_length))
                .ok_or(ArchiveError::Invalid("central entry size overflow"))?;
            let record = slice(bytes, cursor, record_size)?;
            if cursor
                .checked_add(record_size)
                .is_none_or(|end| end > central_end)
            {
                return Err(ArchiveError::Invalid("central entry exceeds directory"));
            }
            let raw_name = &record[46..46 + name_length];
            let name = std::str::from_utf8(raw_name)
                .map_err(|_| ArchiveError::Unsupported("non-UTF-8 entry names"))?;
            validate_name(name)?;
            if !names.insert(name.to_owned()) {
                return Err(ArchiveError::Invalid("duplicate entry name"));
            }

            let compressed_size = usize::try_from(compressed_size)
                .map_err(|_| ArchiveError::LimitExceeded("compressed entry size"))?;
            let uncompressed_size = usize::try_from(uncompressed_size)
                .map_err(|_| ArchiveError::LimitExceeded("uncompressed entry size"))?;
            if uncompressed_size > limits.max_entry_uncompressed {
                return Err(ArchiveError::LimitExceeded("uncompressed entry size"));
            }
            if method == 0 && compressed_size != uncompressed_size {
                return Err(ArchiveError::Invalid("stored entry size mismatch"));
            }
            total_uncompressed = total_uncompressed
                .checked_add(uncompressed_size)
                .filter(|total| *total <= limits.max_total_uncompressed)
                .ok_or(ArchiveError::LimitExceeded("total uncompressed size"))?;

            let local_offset = usize::try_from(local_offset)
                .map_err(|_| ArchiveError::Invalid("local header offset overflow"))?;
            let data = local_data_range(
                bytes,
                local_offset,
                raw_name,
                flags,
                method,
                crc32,
                compressed_size,
                uncompressed_size,
                central_offset,
            )?;
            spans.push((local_offset, data.end));
            entries.push(ZipEntry {
                name: name.to_owned(),
                method,
                compressed_size,
                uncompressed_size,
                crc32,
                data,
                local_offset,
            });
            cursor += record_size;
        }

        if cursor != central_end {
            return Err(ArchiveError::Invalid(
                "central directory size does not match entries",
            ));
        }
        spans.sort_unstable_by_key(|span| span.0);
        for pair in spans.windows(2) {
            if pair[1].0 < pair[0].1 {
                return Err(ArchiveError::Invalid("overlapping ZIP local entries"));
            }
        }
        Ok(Self { bytes, entries })
    }

    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    pub fn entry(&self, name: &str) -> Option<&ZipEntry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let entry = self
            .entry(name)
            .ok_or(ArchiveError::Invalid("requested ZIP entry is absent"))?;
        self.read_entry(entry)
    }

    pub fn read_entry(&self, entry: &ZipEntry) -> Result<Vec<u8>> {
        let compressed = slice(self.bytes, entry.data.start, entry.compressed_size)?;
        let output = match entry.method {
            0 => {
                let mut output = Vec::new();
                output
                    .try_reserve_exact(entry.uncompressed_size)
                    .map_err(|_| ArchiveError::AllocationFailed)?;
                output.extend_from_slice(compressed);
                output
            }
            8 => deflate::decode(compressed, entry.uncompressed_size)?,
            _ => return Err(ArchiveError::Unsupported("compression method")),
        };
        if output.len() != entry.uncompressed_size {
            return Err(ArchiveError::Invalid("entry output size mismatch"));
        }
        if crc32(&output) != entry.crc32 {
            return Err(ArchiveError::CrcMismatch);
        }
        Ok(output)
    }
}

fn validate_limits(limits: ZipLimits) -> Result<()> {
    if limits.max_archive_bytes < 22
        || limits.max_entries > 1_000_000
        || limits.max_name_bytes == 0
        || limits.max_name_bytes > 65_535
        || limits.max_entry_uncompressed > 2 * 1024 * 1024 * 1024_usize
        || limits.max_total_uncompressed < limits.max_entry_uncompressed
    {
        return Err(ArchiveError::Invalid("unsafe ZIP limit configuration"));
    }
    Ok(())
}

fn find_eocd(bytes: &[u8]) -> Result<usize> {
    if bytes.len() < 22 {
        return Err(ArchiveError::Invalid("end of central directory is absent"));
    }
    let start = bytes.len().saturating_sub(22 + 65_535);
    for offset in (start..=bytes.len() - 22).rev() {
        if bytes.get(offset..offset + 4) != Some(&[0x50, 0x4b, 0x05, 0x06]) {
            continue;
        }
        let comment_length = usize::from(le16(bytes, offset + 20)?);
        if offset
            .checked_add(22)
            .and_then(|end| end.checked_add(comment_length))
            == Some(bytes.len())
        {
            return Ok(offset);
        }
    }
    Err(ArchiveError::Invalid("end of central directory is absent"))
}

#[allow(clippy::too_many_arguments)]
fn local_data_range(
    bytes: &[u8],
    local_offset: usize,
    central_name: &[u8],
    flags: u16,
    method: u16,
    crc32: u32,
    compressed_size: usize,
    uncompressed_size: usize,
    central_offset: usize,
) -> Result<Range<usize>> {
    if local_offset >= central_offset {
        return Err(ArchiveError::Invalid(
            "local header overlaps central directory",
        ));
    }
    let header = slice(bytes, local_offset, 30)?;
    if le32(header, 0)? != 0x0403_4b50 {
        return Err(ArchiveError::Invalid("local file header signature"));
    }
    let local_flags = le16(header, 6)?;
    let local_method = le16(header, 8)?;
    if local_flags != flags || local_method != method {
        return Err(ArchiveError::Invalid(
            "local and central ZIP metadata disagree",
        ));
    }
    let name_length = usize::from(le16(header, 26)?);
    let extra_length = usize::from(le16(header, 28)?);
    if name_length != central_name.len() {
        return Err(ArchiveError::Invalid("local entry name length mismatch"));
    }
    let variable = slice(bytes, local_offset + 30, name_length + extra_length)?;
    if &variable[..name_length] != central_name {
        return Err(ArchiveError::Invalid(
            "local and central entry names disagree",
        ));
    }

    if flags & 0x0008 == 0
        && (le32(header, 14)? != crc32
            || usize::try_from(le32(header, 18)?).ok() != Some(compressed_size)
            || usize::try_from(le32(header, 22)?).ok() != Some(uncompressed_size))
    {
        return Err(ArchiveError::Invalid(
            "local and central ZIP sizes or CRC disagree",
        ));
    }
    let start = local_offset
        .checked_add(30)
        .and_then(|n| n.checked_add(name_length))
        .and_then(|n| n.checked_add(extra_length))
        .ok_or(ArchiveError::Invalid("local data offset overflow"))?;
    let end = start
        .checked_add(compressed_size)
        .ok_or(ArchiveError::Invalid("compressed data range overflow"))?;
    if end > central_offset {
        return Err(ArchiveError::Invalid(
            "compressed data overlaps central directory",
        ));
    }
    slice(bytes, start, compressed_size)?;
    Ok(start..end)
}

fn validate_flags(flags: u16) -> Result<()> {
    if flags & 0x0001 != 0 {
        return Err(ArchiveError::Unsupported("encrypted entries"));
    }
    // Bits 1/2 are compression-option metadata for methods that define them;
    // bit 4 is reserved for enhanced DEFLATE. Real-world EPUB producers may
    // leave these bits set even on STORE entries. The compression-method field
    // is authoritative for decoding, so these bits are safe to ignore for the
    // two methods ReadAll supports (STORE=0, DEFLATE=8). Bit 3 selects a data
    // descriptor and bit 11 declares UTF-8 names. Patched data, strong
    // encryption, masked headers/central-directory encryption, and reserved
    // bits stay rejected.
    const ALLOWED: u16 = 0x081e;
    if flags & !ALLOWED != 0 {
        return Err(ArchiveError::UnsupportedFlags(flags));
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('/') || name.contains('\\') || name.contains('\0') {
        return Err(ArchiveError::Invalid("unsafe ZIP entry name"));
    }
    let path = name.strip_suffix('/').unwrap_or(name);
    if path.is_empty()
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(ArchiveError::Invalid("unsafe ZIP entry path"));
    }
    Ok(())
}

fn slice(bytes: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    let end = offset
        .checked_add(length)
        .ok_or(ArchiveError::Invalid("ZIP offset overflow"))?;
    bytes
        .get(offset..end)
        .ok_or(ArchiveError::Invalid("truncated ZIP data"))
}

fn le16(bytes: &[u8], offset: usize) -> Result<u16> {
    let value = slice(bytes, offset, 2)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn le32(bytes: &[u8], offset: usize) -> Result<u32> {
    let value = slice(bytes, offset, 4)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

pub(crate) fn reserve<T>(values: &mut Vec<T>, additional: usize) -> Result<()> {
    values
        .try_reserve(additional)
        .map_err(|_| ArchiveError::AllocationFailed)
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Entry<'a> {
        name: &'a str,
        method: u16,
        compressed: &'a [u8],
        plain: &'a [u8],
    }

    fn push16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn push32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn zip(entries: &[Entry<'_>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut records = Vec::new();
        for entry in entries {
            let offset = bytes.len() as u32;
            let crc = crc32(entry.plain);
            push32(&mut bytes, 0x0403_4b50);
            push16(&mut bytes, 20);
            push16(&mut bytes, 0x0800);
            push16(&mut bytes, entry.method);
            push16(&mut bytes, 0);
            push16(&mut bytes, 0);
            push32(&mut bytes, crc);
            push32(&mut bytes, entry.compressed.len() as u32);
            push32(&mut bytes, entry.plain.len() as u32);
            push16(&mut bytes, entry.name.len() as u16);
            push16(&mut bytes, 0);
            bytes.extend_from_slice(entry.name.as_bytes());
            bytes.extend_from_slice(entry.compressed);
            records.push((entry, offset, crc));
        }
        let central_offset = bytes.len() as u32;
        for (entry, offset, crc) in &records {
            push32(&mut bytes, 0x0201_4b50);
            push16(&mut bytes, 20);
            push16(&mut bytes, 20);
            push16(&mut bytes, 0x0800);
            push16(&mut bytes, entry.method);
            push16(&mut bytes, 0);
            push16(&mut bytes, 0);
            push32(&mut bytes, *crc);
            push32(&mut bytes, entry.compressed.len() as u32);
            push32(&mut bytes, entry.plain.len() as u32);
            push16(&mut bytes, entry.name.len() as u16);
            push16(&mut bytes, 0);
            push16(&mut bytes, 0);
            push16(&mut bytes, 0);
            push16(&mut bytes, 0);
            push32(&mut bytes, 0);
            push32(&mut bytes, *offset);
            bytes.extend_from_slice(entry.name.as_bytes());
        }
        let central_size = bytes.len() as u32 - central_offset;
        push32(&mut bytes, 0x0605_4b50);
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        push16(&mut bytes, entries.len() as u16);
        push16(&mut bytes, entries.len() as u16);
        push32(&mut bytes, central_size);
        push32(&mut bytes, central_offset);
        push16(&mut bytes, 0);
        bytes
    }

    #[test]
    fn stored_and_deflated_entries_are_read_with_crc_validation() {
        let plain = b"hello hello hello";
        let fixed = [0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x90, 0x00];
        let bytes = zip(&[
            Entry {
                name: "mimetype",
                method: 0,
                compressed: b"application/epub+zip",
                plain: b"application/epub+zip",
            },
            Entry {
                name: "OPS/chapter.txt",
                method: 8,
                compressed: &fixed,
                plain,
            },
        ]);
        let archive = ZipArchive::parse(&bytes, ZipLimits::default()).unwrap();
        assert_eq!(archive.entries().len(), 2);
        assert_eq!(archive.entries()[0].name(), "mimetype");
        assert_eq!(archive.entries()[0].local_offset(), 0);
        assert_eq!(archive.read("mimetype").unwrap(), b"application/epub+zip");
        assert_eq!(archive.read("OPS/chapter.txt").unwrap(), plain);
        assert!(archive.read("missing").is_err());
    }

    #[test]
    fn unsafe_duplicate_and_truncated_archives_are_rejected() {
        for name in ["../x", "/x", "a//b", "a/./b", "a/../b", "a\\b"] {
            let bytes = zip(&[Entry {
                name,
                method: 0,
                compressed: b"x",
                plain: b"x",
            }]);
            assert!(ZipArchive::parse(&bytes, ZipLimits::default()).is_err());
        }
        let duplicate = zip(&[
            Entry {
                name: "a",
                method: 0,
                compressed: b"x",
                plain: b"x",
            },
            Entry {
                name: "a",
                method: 0,
                compressed: b"y",
                plain: b"y",
            },
        ]);
        assert!(ZipArchive::parse(&duplicate, ZipLimits::default()).is_err());

        let valid = zip(&[Entry {
            name: "a",
            method: 0,
            compressed: b"hello",
            plain: b"hello",
        }]);
        for length in 0..valid.len() {
            assert!(ZipArchive::parse(&valid[..length], ZipLimits::default()).is_err());
        }
    }

    fn set_single_entry_flags(bytes: &mut [u8], flags: u16) {
        bytes[6..8].copy_from_slice(&flags.to_le_bytes());
        let central = bytes
            .windows(4)
            .position(|window| window == [0x50, 0x4b, 0x01, 0x02])
            .unwrap();
        bytes[central + 8..central + 10].copy_from_slice(&flags.to_le_bytes());
    }

    #[test]
    fn deflate_metadata_flags_are_accepted_but_unsafe_flags_stay_rejected() {
        let plain = b"hello hello hello";
        let fixed = [0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x90, 0x00];
        let mut enhanced = zip(&[Entry {
            name: "OPS/chapter.xhtml",
            method: 8,
            compressed: &fixed,
            plain,
        }]);
        set_single_entry_flags(&mut enhanced, 0x0810);
        let archive = ZipArchive::parse(&enhanced, ZipLimits::default()).unwrap();
        assert_eq!(archive.read("OPS/chapter.xhtml").unwrap(), plain);

        let mut stored_with_compat_flags = zip(&[Entry {
            name: "mimetype",
            method: 0,
            compressed: b"application/epub+zip",
            plain: b"application/epub+zip",
        }]);
        set_single_entry_flags(&mut stored_with_compat_flags, 0x0816);
        let archive = ZipArchive::parse(&stored_with_compat_flags, ZipLimits::default()).unwrap();
        assert_eq!(archive.read("mimetype").unwrap(), b"application/epub+zip");

        for unsafe_flags in [0x0820_u16, 0x0840, 0x2800, 0x1800] {
            let mut bytes = zip(&[Entry {
                name: "OPS/chapter.xhtml",
                method: 8,
                compressed: &fixed,
                plain,
            }]);
            set_single_entry_flags(&mut bytes, unsafe_flags);
            assert!(ZipArchive::parse(&bytes, ZipLimits::default()).is_err());
        }
    }

    #[test]
    fn crc_and_resource_limits_fail_explicitly() {
        let mut bytes = zip(&[Entry {
            name: "a",
            method: 0,
            compressed: b"hello",
            plain: b"hello",
        }]);
        let data = bytes
            .windows(5)
            .position(|window| window == b"hello")
            .unwrap();
        bytes[data] ^= 1;
        let archive = ZipArchive::parse(&bytes, ZipLimits::default()).unwrap();
        assert_eq!(archive.read("a"), Err(ArchiveError::CrcMismatch));

        let good = zip(&[Entry {
            name: "a",
            method: 0,
            compressed: b"hello",
            plain: b"hello",
        }]);
        assert!(
            ZipArchive::parse(
                &good,
                ZipLimits {
                    max_entry_uncompressed: 4,
                    max_total_uncompressed: 4,
                    ..ZipLimits::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn crc32_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
}

//! Referenced KF8 flows/resources only. All paths are generated, all bytes come
//! from the supplied book. Cycles are collected once rather than recursively expanded.
use super::markup;
use crate::{MobiBook, MobiError, Progress, Result, Stage, decode_text, progress, slice, u32be};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Flow(usize),
    Embed(usize),
}
impl Key {
    fn path(self) -> String {
        match self {
            Self::Flow(n) => format!("flow{n}"),
            Self::Embed(n) => format!("embed{n}"),
        }
    }
}
pub(super) struct Resource {
    pub path: String,
    pub mime: &'static str,
    pub data: Vec<u8>,
}
pub(super) struct Resources<'b, 'a> {
    book: &'b MobiBook<'a>,
    text: &'b [u8],
    flows: &'b [Range<usize>],
    requested: BTreeMap<Key, Option<String>>,
    done: BTreeSet<Key>,
    pub resolved: Vec<Resource>,
    pub warnings: Vec<String>,
    bytes: usize,
    references: usize,
    pub cover_referenced: bool,
}
impl<'b, 'a> Resources<'b, 'a> {
    pub fn new(book: &'b MobiBook<'a>, text: &'b [u8], flows: &'b [Range<usize>]) -> Self {
        let mut resources = Self {
            book,
            text,
            flows,
            requested: BTreeMap::new(),
            done: BTreeSet::new(),
            resolved: Vec::new(),
            warnings: Vec::new(),
            bytes: 0,
            references: 0,
            cover_referenced: false,
        };
        if let Some(cover) = book.cover {
            resources.requested.insert(Key::Embed(cover), None);
        }
        resources
    }
    fn warn(&mut self, message: String) {
        if self.warnings.len() < 32 {
            self.warnings.push(message);
        }
    }
    pub fn cover(&self) -> Option<&str> {
        let key = Key::Embed(self.book.cover?).path();
        self.resolved
            .iter()
            .find(|r| r.path == key && r.mime.starts_with("image/"))
            .map(|r| r.path.as_str())
    }
    pub fn rewrite(&mut self, text: &str) -> Result<String> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("kindle:") {
            out.push_str(&rest[..start]);
            rest = &rest[start..];
            let kind = if rest.starts_with("kindle:flow:") {
                Some(true)
            } else if rest.starts_with("kindle:embed:") {
                Some(false)
            } else {
                None
            };
            let Some(flow) = kind else {
                out.push_str("kindle:");
                rest = &rest[7..];
                continue;
            };
            let end = rest
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || b":?=/+._-".contains(b))
                .count();
            let uri = &rest[..end];
            let prefix = if flow { 12 } else { 13 };
            // Prefix lengths include the trailing colon (kindle:flow: = 12).
            let value = uri.get(prefix..).unwrap_or("");
            let (number, mime) = value
                .split_once("?mime=")
                .map_or((value, None), |(n, m)| (n, Some(m)));
            let parsed = markup::base32(number).and_then(|id| {
                if flow {
                    (id > 0 && id < self.flows.len()).then_some(Key::Flow(id))
                } else {
                    id.checked_sub(1).map(Key::Embed)
                }
            });
            self.references += 1;
            if self.references > 100_000 {
                return Err(MobiError::Limit("KF8 resource references"));
            }
            if let Some(key) = parsed {
                if self.book.cover.map(Key::Embed) == Some(key) {
                    self.cover_referenced = true;
                }
                if self.requested.len() >= 8192 && !self.requested.contains_key(&key) {
                    return Err(MobiError::Limit("KF8 referenced resources"));
                }
                self.requested
                    .entry(key)
                    .or_insert_with(|| mime.map(str::to_owned));
                out.push_str(&key.path());
            } else {
                self.warn(format!(
                    "KF8 resource reference ignored: {}",
                    uri.chars().take(160).collect::<String>()
                ));
                out.push_str("missing-kf8-resource");
            }
            rest = &rest[end..];
        }
        out.push_str(rest);
        Ok(out)
    }
    pub fn prepare(&mut self, observer: &mut dyn FnMut(Progress) -> bool) -> Result<()> {
        while let Some((key, mime)) = self
            .requested
            .iter()
            .find(|(k, _)| !self.done.contains(k))
            .map(|(k, m)| (*k, m.clone()))
        {
            progress(
                observer,
                Stage::Package,
                self.done.len(),
                self.requested.len(),
            )?;
            self.done.insert(key);
            match self.load(key, mime.as_deref()) {
                Ok(resource) => {
                    self.bytes = self
                        .bytes
                        .checked_add(resource.data.len())
                        .filter(|n| *n <= self.book.limits.max_package_bytes)
                        .ok_or(MobiError::Limit("KF8 resource package bytes"))?;
                    self.resolved.push(resource);
                }
                Err(error) => {
                    self.warn(format!("KF8 resource {} unavailable: {error}", key.path()))
                }
            }
        }
        self.resolved.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(())
    }
    fn load(&mut self, key: Key, hint: Option<&str>) -> Result<Resource> {
        let (mime, data) = match key {
            Key::Flow(index) => {
                let range = self
                    .flows
                    .get(index)
                    .ok_or(MobiError::Invalid("KF8 flow index"))?;
                if range.len() > 4 * 1024 * 1024 {
                    return Err(MobiError::Limit("KF8 flow bytes"));
                }
                let text = decode_text(&self.text[range.clone()], self.book.metadata.encoding)?
                    .into_owned();
                let svg = hint == Some("image/svg+xml") || text.trim_start().starts_with("<svg");
                if svg {
                    let data = markup::rewrite(&text, self.book.limits, &BTreeMap::new(), self)?;
                    ("image/svg+xml", data.into_bytes())
                } else if hint.is_none() || hint == Some("text/css") {
                    ("text/css", self.rewrite(&text)?.into_bytes())
                } else {
                    return Err(MobiError::Unsupported("KF8 flow media type"));
                }
            }
            Key::Embed(index) => {
                let raw = self.book.image(index)?;
                if raw.starts_with(b"FONT") {
                    let data = font(raw, self.book.limits.max_image_bytes)?;
                    let mime = font_type(&data)
                        .ok_or(MobiError::Unsupported("KF8 font data signature"))?;
                    (mime, data)
                } else if let Some(mime) =
                    crate::package::image_type(raw).or_else(|| font_type(raw))
                {
                    (mime, raw.to_vec())
                } else {
                    return Err(MobiError::Unsupported("KF8 embedded resource signature"));
                }
            }
        };
        Ok(Resource {
            path: key.path(),
            mime,
            data,
        })
    }
}
fn font_type(bytes: &[u8]) -> Option<&'static str> {
    match bytes.get(..4)? {
        b"\0\x01\0\0" | b"true" => Some("font/ttf"),
        b"OTTO" => Some("font/otf"),
        b"ttcf" => Some("font/collection"),
        b"wOFF" => Some("font/woff"),
        b"wOF2" => Some("font/woff2"),
        _ => None,
    }
}
/// FONT's optional key is in the record itself: format obfuscation, not book DRM.
/// Encrypted books are rejected by MobiBook before any resource is inspected.
fn font(raw: &[u8], limit: usize) -> Result<Vec<u8>> {
    let size = u32be(raw, 4)? as usize;
    let flags = u32be(raw, 8)?;
    let start = u32be(raw, 12)? as usize;
    if size == 0 || size > limit || raw.len() > limit || flags & !3 != 0 || start < 24 {
        return Err(MobiError::Limit("KF8 FONT header/output size"));
    }
    let mut data = raw
        .get(start..)
        .ok_or(MobiError::Invalid("KF8 FONT data offset"))?
        .to_vec();
    if flags & 2 != 0 {
        let key_length = u32be(raw, 16)? as usize;
        let key_start = u32be(raw, 20)? as usize;
        if key_length == 0
            || key_length > 1024
            || key_start < 24
            || key_start
                .checked_add(key_length)
                .is_none_or(|end| end > start)
        {
            return Err(MobiError::Invalid("KF8 FONT obfuscation key"));
        }
        let key = slice(raw, key_start, key_length)?;
        for (i, byte) in data
            .iter_mut()
            .take(if key_length == 16 { 1024 } else { 1040 })
            .enumerate()
        {
            *byte ^= key[i % key_length];
        }
    }
    if flags & 1 != 0 {
        data = readall_archive::zlib::decode(&data, size, limit)
            .map_err(|_| MobiError::Invalid("KF8 FONT zlib stream"))?;
    } else if data.len() != size {
        return Err(MobiError::Invalid("KF8 FONT uncompressed length"));
    }
    Ok(data)
}

//! Bounded legacy-HTML lexer. Byte ranges refer to the original MOBI encoding,
//! before entity expansion, so filepos targets do not drift in Chinese text.
use crate::{MobiError, Result, decode_text};
use std::{collections::BTreeMap, ops::Range};
#[derive(Debug)]
pub(super) struct Tag {
    pub name: String,
    pub closing: bool,
    pub empty: bool,
    pub attrs: BTreeMap<String, String>,
}
impl Tag {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }
}
#[derive(Debug)]
pub(super) enum Kind {
    Text,
    Tag(Tag),
    Skip,
}
#[derive(Debug)]
pub(super) struct Token {
    pub range: Range<usize>,
    pub kind: Kind,
}
fn name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-_:".contains(&b)
}
pub(super) fn tokens(data: &[u8], encoding: u32, limit: usize) -> Result<Vec<Token>> {
    let mut out = Vec::new();
    let mut at = 0;
    let mut raw: Option<String> = None;
    while at < data.len() {
        if out.len() >= limit {
            return Err(MobiError::Limit("HTML tokens"));
        }
        if let Some(name) = raw.take() {
            let closing = format!("</{name}");
            let next = data[at..]
                .windows(closing.len())
                .position(|p| p.eq_ignore_ascii_case(closing.as_bytes()))
                .map_or(data.len(), |i| at + i);
            if next > at {
                out.push(Token {
                    range: at..next,
                    kind: Kind::Text,
                });
                at = next;
                continue;
            }
        }
        let start = at;
        if data[at..].starts_with(b"<!--") {
            at = data[at + 4..]
                .windows(3)
                .position(|s| s == b"-->")
                .map_or(data.len(), |n| at + 4 + n + 3);
            out.push(Token {
                range: start..at,
                kind: Kind::Skip,
            });
            continue;
        }
        if data[at] != b'<'
            || !data
                .get(at + 1)
                .is_some_and(|b| b.is_ascii_alphabetic() || b"/!?".contains(b))
        {
            at += 1;
            while at < data.len() && data[at] != b'<' && at - start < 16384 {
                at += 1;
            }
            if encoding == 65001 {
                while data.get(at).is_some_and(|b| b & 0xc0 == 0x80) {
                    at += 1;
                }
            }
            // Do not split an entity just because a large text node was chunked.
            if at < data.len() && data[at] != b'<' {
                for i in (start..at).rev().take(64) {
                    if data[i] == b'&' {
                        at = i.max(start + 1);
                        break;
                    }
                    if data[i].is_ascii_whitespace() || data[i] == b';' {
                        break;
                    }
                }
            }
            out.push(Token {
                range: start..at,
                kind: Kind::Text,
            });
            continue;
        }
        let mut quote = None;
        at += 1;
        while at < data.len() {
            let b = data[at];
            if let Some(q) = quote {
                if b == q {
                    quote = None;
                }
            } else if b == b'\'' || b == b'"' {
                quote = Some(b);
            } else if b == b'>' {
                break;
            }
            at += 1;
            if at - start > 65536 {
                return Err(MobiError::Limit("single HTML tag bytes"));
            }
        }
        if at == data.len() {
            out.push(Token {
                range: start..at,
                kind: Kind::Text,
            });
            break;
        }
        at += 1;
        if matches!(data[start + 1], b'!' | b'?') {
            out.push(Token {
                range: start..at,
                kind: Kind::Skip,
            });
            continue;
        }
        let tag = tag(&data[start + 1..at - 1], encoding)?;
        if !tag.closing
            && !tag.empty
            && matches!(tag.name.as_str(), "script" | "style" | "title" | "textarea")
        {
            raw = Some(tag.name.clone());
        }
        out.push(Token {
            range: start..at,
            kind: Kind::Tag(tag),
        });
    }
    Ok(out)
}
fn tag(data: &[u8], encoding: u32) -> Result<Tag> {
    let closing = data.first() == Some(&b'/');
    let mut at = usize::from(closing);
    while data.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    let start = at;
    while data.get(at).is_some_and(|b| name_byte(*b)) {
        at += 1;
    }
    if at == start || at - start > 128 {
        return Err(MobiError::Invalid("HTML tag name"));
    }
    let name = String::from_utf8_lossy(&data[start..at]).to_ascii_lowercase();
    let empty = data.last() == Some(&b'/');
    let mut attrs = BTreeMap::new();
    while at < data.len() && !closing {
        while data
            .get(at)
            .is_some_and(|b| b.is_ascii_whitespace() || *b == b'/')
        {
            at += 1;
        }
        let start = at;
        while data.get(at).is_some_and(|b| name_byte(*b)) {
            at += 1;
        }
        if start == at {
            at += 1;
            continue;
        }
        if at - start > 128 || attrs.len() >= 64 {
            return Err(MobiError::Limit("HTML attributes"));
        }
        let key = String::from_utf8_lossy(&data[start..at]).to_ascii_lowercase();
        while data.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let mut value = &b""[..];
        if data.get(at) == Some(&b'=') {
            at += 1;
            while data.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            if let Some(q @ (b'\'' | b'"')) = data.get(at).copied() {
                at += 1;
                let start = at;
                while data.get(at).is_some_and(|b| *b != q) {
                    at += 1;
                }
                value = &data[start..at];
                if at < data.len() {
                    at += 1;
                }
            } else {
                let start = at;
                while data.get(at).is_some_and(|b| !b.is_ascii_whitespace()) {
                    at += 1;
                }
                value = &data[start..at];
            }
        }
        if value.len() > 16384 {
            return Err(MobiError::Limit("HTML attribute value"));
        }
        let decoded = decode_text(value, encoding)?;
        attrs
            .entry(key)
            .or_insert_with(|| html_escape::decode_html_entities(&decoded).into_owned());
    }
    Ok(Tag {
        name,
        closing,
        empty,
        attrs,
    })
}

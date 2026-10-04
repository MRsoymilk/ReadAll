//! Preserve reconstructed XHTML/SVG, changing only URI attributes, stylesheet
//! contents and empty position anchors. Visible prose/code is never regex-rewritten.
use super::resources::Resources;
use crate::{
    MobiError, MobiLimits, Result,
    html::{
        escape,
        tokenizer::{self, Kind},
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};
type Target = (usize, usize);
pub(super) struct Edit {
    pub range: Range<usize>,
    pub value: String,
}
struct Attribute<'a> {
    name: &'a str,
    value: &'a str,
    range: Range<usize>,
    quoted: bool,
}

pub(super) fn base32(text: &str) -> Option<usize> {
    if text.is_empty() || text.len() > 13 {
        return None;
    }
    text.bytes().try_fold(0_usize, |n, byte| {
        let value = match byte.to_ascii_uppercase() {
            b'0'..=b'9' => byte - b'0',
            b'A'..=b'V' => byte.to_ascii_uppercase() - b'A' + 10,
            _ => return None,
        };
        n.checked_mul(32)?.checked_add(usize::from(value))
    })
}
pub(super) fn position(text: &str) -> Option<Target> {
    let rest = text.trim().strip_prefix("kindle:pos:fid:")?;
    let (fid, off) = rest.split_once(":off:")?;
    Some((base32(fid)?, base32(off)?))
}
pub(super) fn fragment(id: &str) -> String {
    let mut out = String::new();
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
fn attributes(raw: &str) -> Result<Vec<Attribute<'_>>> {
    let bytes = raw.as_bytes();
    let mut at = 1;
    while bytes
        .get(at)
        .is_some_and(|b| !b.is_ascii_whitespace() && !b"/>".contains(b))
    {
        at += 1;
    }
    let mut result = Vec::new();
    while at < bytes.len() {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if bytes.get(at).is_none_or(|b| b"/>".contains(b)) {
            break;
        }
        let start = at;
        while bytes
            .get(at)
            .is_some_and(|b| !b.is_ascii_whitespace() && !b"=/>".contains(b))
        {
            at += 1;
        }
        if start == at {
            return Err(MobiError::Invalid("KF8 attribute name"));
        }
        let name = &raw[start..at];
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if bytes.get(at) != Some(&b'=') {
            continue;
        }
        at += 1;
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        let quote = bytes.get(at).copied().filter(|q| matches!(q, b'\'' | b'"'));
        if quote.is_some() {
            at += 1;
        }
        let start = at;
        while let Some(&b) = bytes.get(at) {
            if quote.map_or(b.is_ascii_whitespace() || b == b'>', |q| b == q) {
                break;
            }
            at += 1;
        }
        if at == bytes.len() {
            return Err(MobiError::Invalid("unterminated KF8 attribute"));
        }
        result.push(Attribute {
            name,
            value: &raw[start..at],
            range: start..at,
            quoted: quote.is_some(),
        });
        if quote.is_some() {
            at += 1;
        }
    }
    Ok(result)
}
pub(super) fn collect_targets(
    text: &str,
    limits: MobiLimits,
    out: &mut BTreeSet<Target>,
) -> Result<()> {
    for token in tokenizer::tokens(text.as_bytes(), 65001, limits.max_tokens)? {
        if let Kind::Tag(tag) = token.kind
            && !tag.closing
            && let Some(target) = tag.attr("href").and_then(position)
        {
            if out.len() >= 100_000 {
                return Err(MobiError::Limit("KF8 link targets"));
            }
            out.insert(target);
        }
    }
    Ok(())
}

pub(super) fn anchors(
    text: &str,
    targets: &[(Target, usize)],
    limits: MobiLimits,
) -> Result<(Vec<Edit>, BTreeMap<Target, String>)> {
    let tokens = tokenizer::tokens(text.as_bytes(), 65001, limits.max_tokens)?;
    let mut used = BTreeSet::new();
    let mut visible = Vec::with_capacity(tokens.len());
    let mut head = false;
    let mut raw: Option<String> = None;
    let mut body_start = None;
    for token in &tokens {
        if let Kind::Tag(tag) = &token.kind {
            for key in ["id", "xml:id", "name"] {
                if let Some(id) = tag.attr(key) {
                    used.insert(id.to_owned());
                }
            }
            if tag.name == "head" {
                head = !tag.closing;
            }
            if tag.name == "body" && !tag.closing {
                body_start = Some(token.range.end);
            }
            if tag.closing && raw.as_ref() == Some(&tag.name) {
                raw = None;
            } else if !tag.closing
                && !tag.empty
                && matches!(tag.name.as_str(), "style" | "script" | "title" | "textarea")
            {
                raw = Some(tag.name.clone());
            }
        }
        visible.push(!head && raw.is_none());
    }
    let mut edits = Vec::new();
    let mut anchors = BTreeMap::new();
    for &((fid, off), at) in targets {
        if at > text.len() {
            continue;
        }
        let index = tokens.partition_point(|token| token.range.end <= at);
        let mut where_ = at;
        if let Some(token) = tokens.get(index) {
            if !visible[index] {
                where_ = body_start.unwrap_or(token.range.start);
            } else if matches!(token.kind, Kind::Text) {
                while !text.is_char_boundary(where_) {
                    where_ -= 1;
                }
                // Never split a named/numeric entity with an inserted anchor.
                for p in (token.range.start..where_).rev().take(64) {
                    let b = text.as_bytes()[p];
                    if b == b'&' {
                        where_ = p;
                        break;
                    }
                    if b.is_ascii_whitespace() || b == b';' {
                        break;
                    }
                }
            } else {
                where_ = token.range.start;
            }
        } else if let Some(body_close) = text.rfind("</body") {
            where_ = body_close;
        }
        let mut id = format!("readall-kf8-{fid}-{off}");
        while !used.insert(id.clone()) {
            id.push('_');
        }
        edits.push(Edit {
            range: where_..where_,
            value: format!("<a id=\"{id}\"></a>"),
        });
        anchors.insert((fid, off), id);
    }
    Ok((edits, anchors))
}

pub(super) fn apply(text: &str, mut edits: Vec<Edit>) -> Result<String> {
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    let extra = edits
        .iter()
        .try_fold(0_usize, |n, e| n.checked_add(e.value.len()))
        .ok_or(MobiError::Limit("KF8 markup edits"))?;
    if text.len().saturating_add(extra) > 8 * 1024 * 1024 {
        return Err(MobiError::Limit("KF8 rewritten markup bytes"));
    }
    let mut out = String::with_capacity(text.len() + extra);
    let mut at = 0;
    for edit in edits {
        if edit.range.start < at
            || !text.is_char_boundary(edit.range.start)
            || !text.is_char_boundary(edit.range.end)
        {
            return Err(MobiError::Invalid("overlapping KF8 markup edits"));
        }
        out.push_str(
            text.get(at..edit.range.start)
                .ok_or(MobiError::Invalid("KF8 edit outside text"))?,
        );
        out.push_str(&edit.value);
        at = edit.range.end;
    }
    out.push_str(text.get(at..).ok_or(MobiError::Invalid("KF8 edit end"))?);
    Ok(out)
}

pub(super) fn rewrite(
    text: &str,
    limits: MobiLimits,
    links: &BTreeMap<Target, String>,
    resources: &mut Resources<'_, '_>,
) -> Result<String> {
    let tokens = tokenizer::tokens(text.as_bytes(), 65001, limits.max_tokens)?;
    let mut edits = Vec::new();
    let mut style = false;
    for token in tokens {
        match token.kind {
            Kind::Tag(tag) => {
                if tag.name == "style" {
                    style = !tag.closing;
                }
                if tag.closing {
                    continue;
                }
                let raw = &text[token.range.clone()];
                for attr in attributes(raw)? {
                    let name = attr.name.to_ascii_lowercase();
                    if !matches!(
                        name.as_str(),
                        "href" | "src" | "xlink:href" | "poster" | "style"
                    ) {
                        continue;
                    }
                    let value = html_escape::decode_html_entities(attr.value);
                    let new = if value.starts_with("kindle:pos:") {
                        position(&value)
                            .and_then(|p| links.get(&p))
                            .cloned()
                            .unwrap_or_else(|| "missing-kf8-target.xhtml".into())
                    } else {
                        resources.rewrite(&value)?
                    };
                    if new != value {
                        let value = if attr.quoted {
                            escape(&new)
                        } else {
                            format!("\"{}\"", escape(&new))
                        };
                        edits.push(Edit {
                            range: token.range.start + attr.range.start
                                ..token.range.start + attr.range.end,
                            value,
                        });
                    }
                }
            }
            Kind::Text if style => {
                let raw = &text[token.range.clone()];
                let replaced = resources.rewrite(raw)?;
                if replaced != raw {
                    edits.push(Edit {
                        range: token.range,
                        value: replaced,
                    });
                }
            }
            _ => {}
        }
    }
    apply(text, edits)
}
pub(super) fn title(text: &str, limits: MobiLimits) -> Result<Option<String>> {
    let mut collecting = false;
    let mut title = String::new();
    for token in tokenizer::tokens(text.as_bytes(), 65001, limits.max_tokens)? {
        match token.kind {
            Kind::Tag(tag) if matches!(tag.name.as_str(), "title" | "h1" | "h2") => {
                if tag.closing && !title.trim().is_empty() {
                    break;
                }
                collecting = !tag.closing;
            }
            Kind::Text if collecting => {
                title.extend(
                    html_escape::decode_html_entities(&text[token.range])
                        .chars()
                        .take(128),
                );
                if title.len() >= 512 {
                    break;
                }
            }
            _ => {}
        }
    }
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    Ok((!title.is_empty()).then_some(title))
}

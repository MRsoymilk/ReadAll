//! Static @font-face and font-family subset, with archive-local source resolution.
//! No local() system lookup, data/network URLs, @import, variable ranges or font shorthand.
use super::{declaration_parts, delimiter, identifier, without_comments};
use crate::{EpubError, Result, resolve_path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFace {
    pub family: String,
    /// Archive-root paths, already resolved relative to the declaring stylesheet.
    pub sources: Vec<String>,
    pub bold: bool,
    pub italic: bool,
    pub unicode_ranges: Vec<(u32, u32)>,
}
impl FontFace {
    pub fn covers(&self, ch: char) -> bool {
        self.unicode_ranges.is_empty()
            || self
                .unicode_ranges
                .iter()
                .any(|&(lo, hi)| (lo..=hi).contains(&u32::from(ch)))
    }
}
#[derive(Debug, Clone, Default)]
pub struct FontFamilies {
    lists: Vec<Vec<String>>,
}
impl FontFamilies {
    /// Zero means the reader default. IDs are scoped to one ChapterContent.
    pub fn get(&self, id: u16) -> &[String] {
        id.checked_sub(1)
            .and_then(|index| self.lists.get(usize::from(index)))
            .map_or(&[], Vec::as_slice)
    }
    pub(super) fn intern(&mut self, value: &str) -> Option<u16> {
        let list = family_list(value)?;
        if let Some(index) = self.lists.iter().position(|item| item == &list) {
            return Some((index + 1) as u16);
        }
        if self.lists.len() >= 256 {
            return None;
        }
        self.lists.push(list);
        Some(self.lists.len() as u16)
    }
}
fn split_values(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = delimiter(rest, b",").unwrap_or(rest.len());
        let part = rest[..end].trim();
        rest = rest.get(end + 1..).unwrap_or("");
        Some(part)
    })
}
fn css_string(text: &str) -> Option<(String, &str)> {
    let quote = text.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let mut value = String::new();
    let mut it = text[1..].char_indices().peekable();
    while let Some((offset, ch)) = it.next() {
        if ch == quote {
            return Some((value, &text[offset + 2..]));
        }
        if ch == '\n' || ch == '\r' || ch == '\0' {
            return None;
        }
        if ch != '\\' {
            value.push(ch);
            continue;
        }
        let (_, escaped) = it.next()?;
        if matches!(escaped, '\n' | '\r' | '\0') {
            return None;
        }
        if escaped.is_ascii_hexdigit() {
            let mut hex = String::from(escaped);
            for _ in 0..5 {
                if it.peek().is_some_and(|(_, ch)| ch.is_ascii_hexdigit()) {
                    hex.push(it.next()?.1);
                } else {
                    break;
                }
            }
            if it.peek().is_some_and(|(_, ch)| ch.is_ascii_whitespace()) {
                it.next();
            }
            let scalar = char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?;
            if scalar.is_control() {
                return None;
            }
            value.push(scalar);
        } else {
            value.push(escaped);
        }
        if value.len() > 4096 {
            return None;
        }
    }
    None
}
fn generic(name: &str) -> bool {
    matches!(
        name,
        "serif"
            | "sans-serif"
            | "monospace"
            | "cursive"
            | "fantasy"
            | "system-ui"
            | "ui-serif"
            | "ui-sans-serif"
            | "ui-monospace"
            | "emoji"
            | "math"
            | "fangsong"
    )
}
fn family_name(text: &str) -> Option<String> {
    let value = if text.starts_with(['\'', '"']) {
        let (name, rest) = css_string(text)?;
        if !rest.trim().is_empty() {
            return None;
        }
        name
    } else {
        let words: Vec<_> = text.split_ascii_whitespace().collect();
        if words.is_empty()
            || !words
                .iter()
                .all(|word| identifier(word) && !word.starts_with(|ch: char| ch.is_ascii_digit()))
        {
            return None;
        }
        words.join(" ")
    };
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return None;
    }
    let name = value.to_lowercase();
    if matches!(
        name.as_str(),
        "inherit" | "initial" | "unset" | "revert" | "revert-layer"
    ) {
        return None;
    }
    // Reserve generic family markers so an author-defined face cannot replace them.
    Some(if !text.starts_with(['\'', '"']) && generic(&name) {
        format!("<generic:{name}>")
    } else {
        name
    })
}
fn family_list(text: &str) -> Option<Vec<String>> {
    if text.trim().ends_with(',') {
        return None;
    }
    let list: Vec<_> = split_values(text).map(family_name).collect::<Option<_>>()?;
    (!list.is_empty() && list.len() <= 8).then_some(list)
}
fn function<'a>(text: &'a str, name: &str) -> Option<(String, &'a str)> {
    let open = text.find('(')?;
    if !text[..open].trim().eq_ignore_ascii_case(name) {
        return None;
    }
    let inner = text[open + 1..].trim_start();
    if inner.starts_with(['\'', '"']) {
        let (value, rest) = css_string(inner)?;
        return Some((value, rest.trim_start().strip_prefix(')')?.trim_start()));
    }
    let end = inner.find(')')?;
    let value = inner[..end].trim();
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '(' | '\'' | '"' | '\\'))
    {
        return None;
    }
    Some((value.to_owned(), inner[end + 1..].trim_start()))
}
fn sources(text: &str, base: &str) -> Vec<String> {
    split_values(text)
        .take(8)
        .filter_map(|value| {
            // local() is intentionally skipped; later package URLs can still succeed.
            let (url, rest) = function(value, "url")?;
            if url.len() > 4096 || url.chars().any(char::is_control) {
                return None;
            }
            if !rest.is_empty() {
                let (hint, rest) = function(rest, "format")?;
                if !rest.is_empty()
                    || !matches!(
                        hint.to_ascii_lowercase().as_str(),
                        "truetype" | "opentype" | "collection"
                    )
                {
                    return None;
                }
            }
            resolve_path(base, &url).ok()
        })
        .collect()
}
fn ranges(text: &str) -> Option<Vec<(u32, u32)>> {
    let mut output = Vec::new();
    for part in split_values(text) {
        if output.len() >= 32 {
            return None;
        }
        let part = part
            .strip_prefix("U+")
            .or_else(|| part.strip_prefix("u+"))?;
        let (low, high) = if let Some((lo, hi)) = part.split_once('-') {
            (
                u32::from_str_radix(lo, 16).ok()?,
                u32::from_str_radix(hi, 16).ok()?,
            )
        } else if part.contains('?') {
            if part.len() > 6 || part.is_empty() {
                return None;
            }
            let first = part.find('?')?;
            if !part[first..].chars().all(|ch| ch == '?') {
                return None;
            }
            (
                u32::from_str_radix(&part.replace('?', "0"), 16).ok()?,
                u32::from_str_radix(&part.replace('?', "F"), 16).ok()?,
            )
        } else {
            let n = u32::from_str_radix(part, 16).ok()?;
            (n, n)
        };
        if low > high || high > 0x10ffff {
            return None;
        }
        output.push((low, high));
    }
    (!output.is_empty()).then_some(output)
}
pub(super) fn parse_face(text: &str, base: &str) -> Option<FontFace> {
    let text = without_comments(text);
    let (mut family, mut src) = (None, None);
    let (mut bold, mut italic) = (false, false);
    let mut unicode_ranges = Vec::new();
    for descriptor in declaration_parts(&text).take(256) {
        let Some((name, value)) = descriptor.split_once(':') else {
            continue;
        };
        let value = value.trim();
        // !important is not valid in a descriptor; ignore that descriptor only.
        if delimiter(value, b"!").is_some() {
            continue;
        }
        match name.trim().to_ascii_lowercase().as_str() {
            "font-family" => {
                if let Some(name) = family_name(value).filter(|name| !name.starts_with("<generic:"))
                {
                    family = Some(name);
                }
            }
            "src" => src = Some(sources(value, base)),
            "font-weight" => {
                bold = match value.to_ascii_lowercase().as_str() {
                    "normal" => false,
                    "bold" => true,
                    _ => {
                        let n = value.parse::<u16>().ok()?;
                        if !(100..=900).contains(&n) || n % 100 != 0 {
                            return None;
                        }
                        n >= 600
                    }
                }
            }
            "font-style" => {
                italic = match value.to_ascii_lowercase().as_str() {
                    "normal" => false,
                    "italic" | "oblique" => true,
                    _ => return None,
                }
            }
            "font-stretch" if !value.eq_ignore_ascii_case("normal") => return None,
            "unicode-range" => unicode_ranges = ranges(value)?,
            _ => {}
        }
    }
    let sources = src?;
    if sources.is_empty() {
        return None;
    }
    Some(FontFace {
        family: family?,
        sources,
        bold,
        italic,
        unicode_ranges,
    })
}
pub(super) fn add_face(faces: &mut Vec<FontFace>, body: &str, base: &str) -> Result<()> {
    if let Some(face) = parse_face(body, base) {
        if faces.len() >= 32 {
            return Err(EpubError::LimitExceeded("CSS font faces"));
        }
        faces.push(face);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn family_cascade_inheritance_and_reset_keep_correct_chapter_ids() {
        use super::super::{StyleSheet, TextStyle};
        use crate::xml::Element;
        let mut sheet = StyleSheet::default();
        sheet
            .append("p {font-family:First} #lead {font-family:'Book Serif', serif !important}")
            .unwrap();
        let element = |name: &str, attrs: &[(&str, &str)]| Element {
            name: name.into(),
            attributes: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            empty: false,
        };
        let parent = sheet
            .compute(
                &element("p", &[("id", "lead"), ("style", "font-family:Other")]),
                TextStyle::default(),
            )
            .unwrap();
        assert_eq!(
            sheet.families.get(parent.families),
            ["book serif", "<generic:serif>"]
        );
        let inherited = sheet.compute(&element("span", &[]), parent).unwrap();
        assert_eq!(inherited.families, parent.families);
        let reset = sheet
            .compute(
                &element("span", &[("style", "font-family:initial")]),
                parent,
            )
            .unwrap();
        assert_eq!(reset.families, 0);
        let inline = sheet
            .compute(
                &element(
                    "p",
                    &[("id", "lead"), ("style", "font-family:Other !important")],
                ),
                parent,
            )
            .unwrap();
        assert_eq!(sheet.families.get(inline.families), ["other"]);
    }
    #[test]
    fn font_rules_in_unsupported_at_rules_never_escape_their_scope() {
        use super::super::StyleSheet;
        let mut sheet = StyleSheet::default();
        sheet.append_at("@media print { @font-face {font-family:X;src:url(x.ttf)} } @font-face{font-family:Y;src:url(y.ttf)}", "OPS/chapter.xhtml").unwrap();
        assert_eq!(sheet.font_faces.len(), 1);
        assert_eq!(sheet.font_faces[0].family, "y");
        assert!(
            parse_face(
                "font-family:X;src:url(file:///etc/font.ttf)",
                "OPS/chapter.xhtml"
            )
            .is_none()
        );
        assert!(
            parse_face(
                "font-family:X;src:url(data:font/ttf;base64,AAAA)",
                "OPS/chapter.xhtml"
            )
            .is_none()
        );
        assert!(parse_face("font-family:X;src:url(a.ttc#face)", "OPS/chapter.xhtml").is_none());
    }
    #[test]
    fn source_order_case_and_stylesheet_relative_paths() {
        let face=parse_face(r#"font-family: 'Book Serif'; src: local('Absent'), url('../Fonts/Broken.ttf'), url('../Fonts/Good.ttf') format('truetype'); font-weight:700; font-style:italic;"#,"OPS/styles/main.css").unwrap();
        assert_eq!(face.family, "book serif");
        assert_eq!(face.sources, ["OPS/Fonts/Broken.ttf", "OPS/Fonts/Good.ttf"]);
        assert!(face.bold && face.italic);
        assert!(
            parse_face(
                "font-family:X; src:url(https://example.invalid/x.ttf)",
                "OPS/main.css"
            )
            .is_none()
        );
        assert!(parse_face("font-family:X; src:url(../../outside.ttf)", "OPS/main.css").is_none());
    }
    #[test]
    fn families_are_interned_with_quotes_escapes_and_order() {
        let mut table = FontFamilies::default();
        let a = table
            .intern(r#"'Book\20 Serif', "备用字体", serif"#)
            .unwrap();
        assert_eq!(table.get(a), ["book serif", "备用字体", "<generic:serif>"]);
        assert_eq!(table.intern("book serif, '备用字体', serif"), Some(a));
        assert!(table.intern("Name,").is_none());
        assert!(table.intern("initial").is_none());
        assert!(table.intern("'bad").is_none());
    }
    #[test]
    fn unsupported_hints_and_unicode_ranges_are_bounded() {
        let face=parse_face("font-family:X; src:url(a.woff2) format('woff2'), url(a.ttf); unicode-range:U+4E??,U+20-7F;","OPS/c.xhtml").unwrap();
        assert_eq!(face.sources, ["OPS/a.ttf"]);
        assert!(face.covers('中'));
        assert!(face.covers('A'));
        assert!(!face.covers('Ж'));
        assert!(
            parse_face(
                "font-family:X; src:url(a.ttf); unicode-range:U+11FFFF",
                "OPS/c.xhtml"
            )
            .is_none()
        );
        assert!(parse_face("font-family:serif; src:url(a.ttf)", "OPS/c.xhtml").is_none());
    }
}

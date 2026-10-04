//! Bounded author-CSS subset for the native reader, not a browser CSS engine.
//! Unsupported selectors/properties and at-rules are ignored as whole units.
use crate::{
    EpubError, Result,
    xml::{Element, local_name},
};

mod boxes;
mod fonts;
pub use boxes::{BoxLength, BoxStyle};
pub use fonts::{FontFace, FontFamilies};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Computed size relative to the reader's base size (CSS 16px == 1em).
    pub font_scale: f32,
    /// Chapter-local computed font-family list; zero keeps the reader default.
    pub families: u16,
    pub color: [u8; 3],
    pub align: TextAlign,
    /// Computed first-line indentation in reader-base em units.
    pub indent: f32,
    pub line_height: f32,
    pub hidden: bool,
    pub bold: bool,
    pub italic: bool,
    /// Effective author-painted block background for theme contrast handling.
    pub backdrop: Option<[u8; 3]>,
}
impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_scale: 1.0,
            families: 0,
            color: [24, 24, 24],
            align: TextAlign::Left,
            indent: 0.0,
            line_height: 1.2,
            hidden: false,
            bold: false,
            italic: false,
            backdrop: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Length {
    Em(f32),
    Px(f32),
    Percent(f32),
}
impl Length {
    fn resolve(self, parent: f32) -> f32 {
        match self {
            Self::Em(n) => n * parent,
            Self::Px(n) => n / 16.0,
            Self::Percent(n) => parent * n / 100.0,
        }
    }
}
#[derive(Debug, Clone, Copy)]
enum Value {
    Color([u8; 3]),
    Length(Length),
    Align(TextAlign),
    Number(f32),
    Hidden(bool),
    Inherit,
    Auto,
    Families(u16),
}
#[derive(Debug, Clone)]
struct Declaration {
    property: usize,
    value: Value,
    important: bool,
}
#[derive(Debug, Clone)]
struct Selector {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
}
impl Selector {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() || text.len() > 256 {
            return None;
        }
        let mut selector = Self {
            tag: None,
            id: None,
            classes: Vec::new(),
        };
        let mut rest = text;
        if let Some(after) = rest.strip_prefix('*') {
            rest = after;
        } else if !rest.starts_with(['.', '#']) {
            let end = rest.find(['.', '#']).unwrap_or(rest.len());
            let tag = &rest[..end];
            if !identifier(tag) {
                return None;
            }
            selector.tag = Some(tag.to_ascii_lowercase());
            rest = &rest[end..];
        }
        while !rest.is_empty() {
            let kind = rest.as_bytes()[0];
            if kind != b'.' && kind != b'#' {
                return None;
            }
            rest = &rest[1..];
            let end = rest.find(['.', '#']).unwrap_or(rest.len());
            let name = &rest[..end];
            if !identifier(name) {
                return None;
            }
            if kind == b'#' {
                if selector.id.is_some() {
                    return None;
                }
                selector.id = Some(name.to_owned());
            } else {
                selector.classes.push(name.to_owned());
            }
            rest = &rest[end..];
        }
        Some(selector)
    }
    fn matches(&self, element: &Element) -> bool {
        self.tag
            .as_ref()
            .is_none_or(|tag| tag == local_name(&element.name))
            && self
                .id
                .as_ref()
                .is_none_or(|id| element.attribute("id") == Some(id.as_str()))
            && self.classes.iter().all(|class| {
                element.attribute("class").is_some_and(|classes| {
                    classes.split_ascii_whitespace().any(|word| word == class)
                })
            })
    }
    fn specificity(&self) -> (bool, usize, usize, usize) {
        (
            false,
            usize::from(self.id.is_some()),
            self.classes.len(),
            usize::from(self.tag.is_some()),
        )
    }
}
fn identifier(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_')
}

#[derive(Debug, Default)]
pub(crate) struct StyleSheet {
    pub(crate) families: FontFamilies,
    pub(crate) font_faces: Vec<FontFace>,
    rules: Vec<(Selector, Vec<Declaration>)>,
    bytes: usize,
    work: usize,
}
impl StyleSheet {
    #[cfg(test)]
    pub(crate) fn append(&mut self, text: &str) -> Result<()> {
        self.append_at(text, "")
    }
    pub(crate) fn append_at(&mut self, text: &str, stylesheet: &str) -> Result<()> {
        self.bytes = self
            .bytes
            .checked_add(text.len())
            .filter(|n| *n <= 1024 * 1024)
            .ok_or(EpubError::LimitExceeded("CSS source bytes"))?;
        let text = without_comments(text);
        let mut rest = text.as_str();
        while !rest.trim().is_empty() {
            rest = rest.trim_start();
            let Some(end) = delimiter(rest, b"{;") else {
                break;
            };
            if rest.as_bytes()[end] == b';' {
                rest = &rest[end + 1..];
                continue;
            }
            let selector_text = rest[..end].trim();
            let Some(close) = closing_brace(rest, end) else {
                break;
            };
            let body = &rest[end + 1..close];
            if selector_text.eq_ignore_ascii_case("@font-face") && delimiter(body, b"{}").is_none()
            {
                fonts::add_face(&mut self.font_faces, body, stylesheet)?;
            } else if !selector_text.starts_with('@') && delimiter(body, b"{}").is_none() {
                let declarations = declarations_with_families(body, &mut self.families);
                for text in selector_text.split(',') {
                    if let Some(selector) = Selector::parse(text) {
                        if self.rules.len() >= 1024 {
                            return Err(EpubError::LimitExceeded("CSS rules"));
                        }
                        self.rules.push((selector, declarations.clone()));
                    }
                }
            }
            rest = &rest[close + 1..];
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn compute(&mut self, element: &Element, parent: TextStyle) -> Result<TextStyle> {
        self.compute_with_box(element, parent)
            .map(|(style, _)| style)
    }
    pub(crate) fn compute_with_box(
        &mut self,
        element: &Element,
        parent: TextStyle,
    ) -> Result<(TextStyle, BoxStyle)> {
        self.work = self.work.saturating_add(self.rules.len() + 1);
        if self.work > 2_000_000 {
            return Err(EpubError::LimitExceeded("CSS selector work"));
        }
        type Priority = (bool, (bool, usize, usize, usize), usize, usize);
        let mut selected: [Option<(Priority, Value)>; boxes::PROPERTIES] =
            [None; boxes::PROPERTIES];
        let mut offer = |declaration: &Declaration, specificity, order, index| {
            let priority = (declaration.important, specificity, order, index);
            let slot = &mut selected[declaration.property];
            if slot.is_none_or(|(old, _)| priority >= old) {
                *slot = Some((priority, declaration.value));
            }
        };
        for (order, (selector, values)) in self.rules.iter().enumerate() {
            if selector.matches(element) {
                for (index, declaration) in values.iter().enumerate() {
                    offer(declaration, selector.specificity(), order, index);
                }
            }
        }
        if let Some(inline) = element.attribute("style") {
            for (index, declaration) in declarations_with_families(inline, &mut self.families)
                .iter()
                .enumerate()
            {
                offer(declaration, (true, 0, 0, 0), self.rules.len(), index);
            }
        }
        let mut style = parent;
        if matches!(
            local_name(&element.name),
            "b" | "strong" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        ) {
            style.bold = true;
        }
        if matches!(local_name(&element.name), "i" | "em") {
            style.italic = true;
        }
        style.font_scale *= match local_name(&element.name) {
            "h1" => 2.0,
            "h2" => 1.5,
            "h3" => 1.17,
            "h5" | "small" => 0.83,
            "h6" => 0.67,
            _ => 1.0,
        };
        if let Some((_, value)) = selected[1] {
            style.font_scale = match value {
                Value::Length(length) => length.resolve(parent.font_scale),
                Value::Inherit => parent.font_scale,
                _ => style.font_scale,
            };
        }
        style.font_scale = style.font_scale.clamp(0.25, 8.0);
        for (property, slot) in selected.iter().enumerate() {
            let Some((_, value)) = slot else {
                continue;
            };
            match (property, *value) {
                (0, Value::Color(color)) => style.color = color,
                (2, Value::Align(align)) => style.align = align,
                (3, Value::Length(length)) => {
                    style.indent = length.resolve(style.font_scale).clamp(-32.0, 32.0)
                }
                (4, Value::Number(number)) => style.line_height = number,
                (5, Value::Hidden(hidden)) => style.hidden = parent.hidden || hidden,
                (6, Value::Hidden(bold)) => style.bold = bold,
                (7, Value::Hidden(italic)) => style.italic = italic,
                (33, Value::Families(families)) => style.families = families,
                _ => {}
            }
        }
        if element.attribute("hidden").is_some() {
            style.hidden = true;
        }
        let values = selected.map(|entry| entry.map(|(_, value)| value));
        let mut layout = boxes::compute(&values, style);
        if crate::xhtml::is_block(local_name(&element.name))
            || matches!(local_name(&element.name), "body" | "html")
        {
            if layout.background.is_some() {
                style.backdrop = layout.background;
            }
            if local_name(&element.name) == "body" && layout.background.is_none() {
                layout.background = style.backdrop;
            }
        }
        Ok((style, layout))
    }
}

#[cfg(test)]
fn declarations(text: &str) -> Vec<Declaration> {
    declarations_with_families(text, &mut FontFamilies::default())
}
fn declarations_with_families(text: &str, families: &mut FontFamilies) -> Vec<Declaration> {
    let mut values = Vec::new();
    let text = without_comments(text);
    for declaration in declaration_parts(&text).take(256) {
        let Some((property, raw)) = declaration.split_once(':') else {
            continue;
        };
        let value = raw.trim().to_ascii_lowercase();
        let (value, important) = value
            .strip_suffix("!important")
            .map_or((value.as_str(), false), |v| (v.trim_end(), true));
        let name = property.trim().to_ascii_lowercase();
        if let Some(expanded) = boxes::declarations(&name, value, important) {
            values.extend(expanded);
            continue;
        }
        let property = match name.as_str() {
            "color" => 0,
            "font-size" => 1,
            "text-align" => 2,
            "text-indent" => 3,
            "line-height" => 4,
            "display" => 5,
            "font-weight" => 6,
            "font-style" => 7,
            "font-family" => 33,
            _ => continue,
        };
        let parsed = if value == "inherit" || property == 33 && value == "unset" {
            Some(Value::Inherit)
        } else {
            match property {
                0 => color(value).map(Value::Color),
                1 => length(value)
                    .filter(|length| length.resolve(1.0) > 0.0)
                    .map(Value::Length),
                2 => match value {
                    "left" | "start" => Some(Value::Align(TextAlign::Left)),
                    "center" => Some(Value::Align(TextAlign::Center)),
                    "right" | "end" => Some(Value::Align(TextAlign::Right)),
                    _ => None,
                },
                3 => length(value)
                    .filter(|n| !matches!(n, Length::Percent(_)))
                    .map(Value::Length),
                4 => {
                    if value == "normal" {
                        Some(Value::Number(1.2))
                    } else {
                        finite(value)
                            .filter(|n| (0.5..=4.0).contains(n))
                            .map(Value::Number)
                    }
                }
                5 => match value {
                    "none" => Some(Value::Hidden(true)),
                    "block" | "inline" => Some(Value::Hidden(false)),
                    _ => None,
                },
                6 => match value {
                    "bold" | "bolder" => Some(Value::Hidden(true)),
                    "normal" | "lighter" => Some(Value::Hidden(false)),
                    _ => finite(value)
                        .filter(|n| (100.0..=900.0).contains(n))
                        .map(|n| Value::Hidden(n >= 600.0)),
                },
                7 => match value {
                    "italic" | "oblique" => Some(Value::Hidden(true)),
                    "normal" => Some(Value::Hidden(false)),
                    _ => None,
                },
                33 => {
                    if value == "initial" {
                        Some(Value::Families(0))
                    } else {
                        families.intern(value).map(Value::Families)
                    }
                }
                _ => None,
            }
        };
        if let Some(value) = parsed {
            values.push(Declaration {
                property,
                value,
                important,
            });
        }
    }
    values
}
fn finite(text: &str) -> Option<f32> {
    text.parse::<f32>()
        .ok()
        .filter(|n| n.is_finite() && n.abs() <= 4096.0)
}
fn length(text: &str) -> Option<Length> {
    if text == "0" {
        return Some(Length::Px(0.0));
    }
    if let Some(n) = text.strip_suffix("em") {
        finite(n).map(Length::Em)
    } else if let Some(n) = text.strip_suffix("px") {
        finite(n).map(Length::Px)
    } else if let Some(n) = text.strip_suffix('%') {
        finite(n).map(Length::Percent)
    } else {
        None
    }
}
fn color(text: &str) -> Option<[u8; 3]> {
    if let Some(hex) = text.strip_prefix('#') {
        if !hex.is_ascii() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        return match hex.len() {
            3 => Some([
                u8::from_str_radix(&hex[0..1], 16).ok()? * 17,
                u8::from_str_radix(&hex[1..2], 16).ok()? * 17,
                u8::from_str_radix(&hex[2..3], 16).ok()? * 17,
            ]),
            6 => Some([
                u8::from_str_radix(&hex[0..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..6], 16).ok()?,
            ]),
            _ => None,
        };
    }
    if let Some(rgb) = text.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let mut parts = rgb.split(',').map(str::trim);
        let result = [
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ];
        return parts.next().is_none().then_some(result);
    }
    Some(match text {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "gray" | "grey" => [128, 128, 128],
        "navy" => [0, 0, 128],
        "maroon" => [128, 0, 0],
        "purple" => [128, 0, 128],
        "teal" => [0, 128, 128],
        "yellow" => [255, 255, 0],
        _ => return None,
    })
}
fn without_comments(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut quote = None;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            result.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
            result.push(ch);
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(ch) = chars.next() {
                if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    result
}
fn declaration_parts(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = delimiter(rest, b";").unwrap_or(rest.len());
        let part = &rest[..end];
        rest = rest.get(end + 1..).unwrap_or("");
        Some(part)
    })
}
fn delimiter(text: &str, needles: &[u8]) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut parentheses = 0_usize;
    for (i, b) in text.bytes().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if b == b'\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
        } else if b == b'\'' || b == b'"' {
            quote = Some(b);
        } else if b == b'(' {
            parentheses = parentheses.saturating_add(1);
        } else if b == b')' {
            parentheses = parentheses.saturating_sub(1);
        } else if parentheses == 0 && needles.contains(&b) {
            return Some(i);
        }
    }
    None
}
fn closing_brace(text: &str, open: usize) -> Option<usize> {
    let mut depth = 1;
    let mut at = open + 1;
    while let Some(next) = delimiter(&text[at..], b"{}") {
        at += next;
        if text.as_bytes()[at] == b'{' {
            depth += 1;
        } else {
            depth -= 1;
        }
        if depth == 0 {
            return Some(at);
        }
        at += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn element(name: &str, attrs: &[(&str, &str)]) -> Element {
        Element {
            name: name.into(),
            attributes: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            empty: false,
        }
    }
    #[test]
    fn cascade_is_property_wise_and_important_beats_inline_normal() {
        let mut sheet = StyleSheet::default();
        sheet.append("p { color: blue; text-align: center } .lead { color: green } #a { color: red !important } p.lead { text-indent: 2em }").unwrap();
        let style = sheet
            .compute(
                &element(
                    "p",
                    &[
                        ("id", "a"),
                        ("class", "lead other"),
                        ("style", "color: #abc; font-size: 150%"),
                    ],
                ),
                TextStyle::default(),
            )
            .unwrap();
        assert_eq!(style.color, [255, 0, 0]);
        assert_eq!(style.align, TextAlign::Center);
        assert_eq!(style.font_scale, 1.5);
        assert_eq!(style.indent, 3.0);
    }
    #[test]
    fn inheritance_and_inline_important_are_preserved() {
        let mut sheet = StyleSheet::default();
        sheet
            .append("#a { color: red !important; font-size: 200% }")
            .unwrap();
        let parent = sheet
            .compute(
                &element("h1", &[("id", "a"), ("style", "color: blue !important")]),
                TextStyle::default(),
            )
            .unwrap();
        let child = sheet.compute(&element("span", &[]), parent).unwrap();
        assert_eq!(child.color, [0, 0, 255]);
        assert_eq!(child.font_scale, 2.0);
    }
    #[test]
    fn unsupported_selectors_and_at_rules_never_become_global_rules() {
        let mut sheet = StyleSheet::default();
        sheet.append("@import url(https://invalid/x); @media screen { p { color:red } } div p { color:red } p:hover { color:red } [id=a] { color:red } p { color: #123; color:invalid; font-size:NaNem }").unwrap();
        let style = sheet
            .compute(&element("p", &[]), TextStyle::default())
            .unwrap();
        assert_eq!(style.color, [17, 34, 51]);
        assert_eq!(style.font_scale, 1.0);
    }
    #[test]
    fn quoted_and_function_values_cannot_inject_supported_declarations() {
        let mut sheet = StyleSheet::default();
        sheet.append(r#"p { content: \"/*; color:red; */\"; background:url(data:foo;color:red); color:blue }"#).unwrap();
        let style = sheet
            .compute(
                &element(
                    "p",
                    &[(
                        "style",
                        r#"content: "; color:red;"; color: green /* valid comment */"#,
                    )],
                ),
                TextStyle::default(),
            )
            .unwrap();
        assert_eq!(style.color, [0, 128, 0]);
        let values = declarations(r#"content: ";color:red;"; background:url(data:foo;color:red)"#);
        assert!(values.is_empty());
    }

    #[test]
    fn stylesheet_budgets_and_invalid_colors_are_checked() {
        assert!(
            StyleSheet::default()
                .append(&"x".repeat(1024 * 1024 + 1))
                .is_err()
        );
        assert_eq!(color("#汉字"), None);
        assert_eq!(color("rgb(1, 2, 3)"), Some([1, 2, 3]));
    }
}

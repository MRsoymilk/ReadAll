//! Positive-length block box subset. No floats, positioning or margin collapsing.
use super::{Declaration, Length, TextStyle, Value, color, length};
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoxLength {
    Em(f32),
    Percent(f32),
    Auto,
}
impl Default for BoxLength {
    fn default() -> Self {
        Self::Em(0.0)
    }
}
impl BoxLength {
    pub fn resolve(self, base: f32, parent: f32) -> f32 {
        match self {
            Self::Em(n) => n * base,
            Self::Percent(n) => parent * n / 100.0,
            Self::Auto => 0.0,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxStyle {
    /// Top, right, bottom, left. Values are relative to the reader's base em.
    pub margin: [BoxLength; 4],
    pub padding: [BoxLength; 4],
    pub border: [BoxLength; 4],
    pub border_color: [[u8; 3]; 4],
    pub solid: [bool; 4],
    pub background: Option<[u8; 3]>,
    pub width: BoxLength,
    pub max_width: BoxLength,
    pub break_before: bool,
    pub break_after: bool,
}
impl Default for BoxStyle {
    fn default() -> Self {
        Self {
            margin: [BoxLength::default(); 4],
            padding: [BoxLength::default(); 4],
            border: [BoxLength::Em(3.0 / 16.0); 4],
            border_color: [[24, 24, 24]; 4],
            solid: [false; 4],
            background: None,
            width: BoxLength::Auto,
            max_width: BoxLength::Auto,
            break_before: false,
            break_after: false,
        }
    }
}
impl BoxStyle {
    pub fn active(self) -> bool {
        self.margin
            .iter()
            .chain(self.padding.iter())
            .any(|n| *n != BoxLength::default())
            || self.solid.iter().any(|v| *v)
            || self.background.is_some()
            || self.width != BoxLength::Auto
            || self.max_width != BoxLength::Auto
            || self.break_before
            || self.break_after
    }
}
pub(super) const PROPERTIES: usize = 34;
fn computed(value: Option<Value>, style: TextStyle, default: BoxLength) -> BoxLength {
    match value {
        Some(Value::Length(Length::Em(n))) => BoxLength::Em(n * style.font_scale),
        Some(Value::Length(Length::Px(n))) => BoxLength::Em(n / 16.0),
        Some(Value::Length(Length::Percent(n))) => BoxLength::Percent(n),
        Some(Value::Auto) => BoxLength::Auto,
        _ => default,
    }
}
pub(super) fn compute(values: &[Option<Value>; PROPERTIES], style: TextStyle) -> BoxStyle {
    let mut result = BoxStyle {
        border_color: [style.color; 4],
        ..BoxStyle::default()
    };
    for i in 0..4 {
        result.margin[i] = computed(values[8 + i], style, result.margin[i]);
        result.padding[i] = computed(values[12 + i], style, result.padding[i]);
        result.border[i] = computed(values[16 + i], style, result.border[i]);
        if let Some(Value::Color(c)) = values[20 + i] {
            result.border_color[i] = c;
        }
        if let Some(Value::Hidden(v)) = values[24 + i] {
            result.solid[i] = v;
        }
    }
    if let Some(Value::Color(c)) = values[28] {
        result.background = Some(c);
    }
    result.width = computed(values[29], style, BoxLength::Auto);
    result.max_width = computed(values[30], style, BoxLength::Auto);
    if let Some(Value::Hidden(v)) = values[31] {
        result.break_before = v;
    }
    if let Some(Value::Hidden(v)) = values[32] {
        result.break_after = v;
    }
    result
}
fn positive(value: &str, auto: bool, percent: bool) -> Option<Value> {
    if auto && value == "auto" {
        return Some(Value::Auto);
    }
    length(value)
        .filter(|length| {
            length.resolve(1.0) >= 0.0 && (percent || !matches!(length, Length::Percent(_)))
        })
        .map(Value::Length)
}
fn four(values: Vec<Value>) -> Option<[Value; 4]> {
    Some(match values.as_slice() {
        [a] => [*a, *a, *a, *a],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => return None,
    })
}
fn border_width(value: &str) -> Option<Value> {
    match value {
        "thin" => Some(Value::Length(Length::Px(1.0))),
        "medium" => Some(Value::Length(Length::Px(3.0))),
        "thick" => Some(Value::Length(Length::Px(5.0))),
        _ => positive(value, false, false),
    }
}
fn solid(value: &str) -> Option<Value> {
    match value {
        "solid" => Some(Value::Hidden(true)),
        "none" | "hidden" => Some(Value::Hidden(false)),
        _ => None,
    }
}
fn border_color(value: &str) -> Option<Value> {
    if value == "currentcolor" {
        Some(Value::Auto)
    } else {
        color(value).map(Value::Color)
    }
}
pub(super) fn declarations(
    property: &str,
    value: &str,
    important: bool,
) -> Option<Vec<Declaration>> {
    let mut out = Vec::new();
    let mut push = |property, value| {
        out.push(Declaration {
            property,
            value,
            important,
        })
    };
    match property {
        "margin" | "padding" | "border-width" | "border-color" | "border-style" => {
            let base = match property {
                "margin" => 8,
                "padding" => 12,
                "border-width" => 16,
                "border-color" => 20,
                _ => 24,
            };
            let values: Option<Vec<_>> = value
                .split_ascii_whitespace()
                .map(|v| match base {
                    8 => positive(v, true, true),
                    12 => positive(v, false, true),
                    16 => border_width(v),
                    20 => border_color(v),
                    _ => solid(v),
                })
                .collect();
            for (i, value) in four(values?)?.into_iter().enumerate() {
                push(base + i, value);
            }
        }
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            let mut width = None;
            let mut style = None;
            let mut color = None;
            for part in value.split_ascii_whitespace() {
                if let Some(v) = border_width(part) {
                    if width.replace(v).is_some() {
                        return None;
                    }
                } else if let Some(v) = solid(part) {
                    if style.replace(v).is_some() {
                        return None;
                    }
                } else {
                    let v = border_color(part)?;
                    if color.replace(v).is_some() {
                        return None;
                    }
                }
            }
            if width.is_none() && style.is_none() && color.is_none() {
                return None;
            }
            let side = match property {
                "border-top" => Some(0),
                "border-right" => Some(1),
                "border-bottom" => Some(2),
                "border-left" => Some(3),
                _ => None,
            };
            for i in 0..4 {
                if side.is_none_or(|side| side == i) {
                    push(16 + i, width.unwrap_or(Value::Length(Length::Px(3.0))));
                    push(20 + i, color.unwrap_or(Value::Auto));
                    push(24 + i, style.unwrap_or(Value::Hidden(false)));
                }
            }
        }
        "background" | "background-color" => push(
            28,
            if value == "none" || value == "transparent" {
                Value::Auto
            } else {
                Value::Color(color(value)?)
            },
        ),
        "width" | "max-width" => push(
            if property == "width" { 29 } else { 30 },
            if value == "none" {
                Value::Auto
            } else {
                positive(value, true, true)?
            },
        ),
        "break-before" | "page-break-before" | "break-after" | "page-break-after" => {
            let page = match value {
                "page" | "always" => true,
                "auto" => false,
                _ => return None,
            };
            push(
                if property.ends_with("before") { 31 } else { 32 },
                Value::Hidden(page),
            );
        }
        _ => {
            let (prefix, rest) = property.split_once('-')?;
            let (side, suffix) = rest.split_once('-').map_or((rest, ""), |(a, b)| (a, b));
            let i = match side {
                "top" => 0,
                "right" => 1,
                "bottom" => 2,
                "left" => 3,
                _ => return None,
            };
            let (slot, parsed) = match (prefix, suffix) {
                ("margin", "") => (8 + i, positive(value, i == 1 || i == 3, true)?),
                ("padding", "") => (12 + i, positive(value, false, true)?),
                ("border", "width") => (16 + i, border_width(value)?),
                ("border", "color") => (20 + i, border_color(value)?),
                ("border", "style") => (24 + i, solid(value)?),
                _ => return None,
            };
            push(slot, parsed);
        }
    }
    Some(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::StyleSheet;
    use crate::xml::Element;
    #[test]
    fn shorthand_priority_and_individual_sides_cascade_independently() {
        let mut sheet = StyleSheet::default();
        sheet.append("p {padding: 1em 2em; padding-left:3px !important; border:2px solid red; background:#abc; width:80%; max-width:30em; page-break-before:always}").unwrap();
        let element = Element {
            name: "p".into(),
            attributes: vec![("style".into(), "padding:4px; border-left-color:blue".into())],
            empty: false,
        };
        let (_, style) = sheet
            .compute_with_box(&element, TextStyle::default())
            .unwrap();
        assert_eq!(style.padding[3], BoxLength::Em(3.0 / 16.0));
        assert_eq!(style.padding[0], BoxLength::Em(4.0 / 16.0));
        assert_eq!(style.border_color[3], [0, 0, 255]);
        assert_eq!(style.border_color[0], [255, 0, 0]);
        assert_eq!(style.width, BoxLength::Percent(80.0));
        assert!(style.break_before);
        assert_eq!(style.background, Some([170, 187, 204]));
    }
    #[test]
    fn unsupported_negative_and_unsafe_lengths_are_ignored() {
        assert!(declarations("padding", "-2px", false).is_none());
        assert!(declarations("width", "NaNem", false).is_none());
        assert!(declarations("border", "2px dotted red", false).is_none());
        assert!(declarations("background", "url(file:///tmp/x)", false).is_none());
    }
}

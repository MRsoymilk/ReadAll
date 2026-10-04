//! Inherited white-space modes. The paged reader may emergency-wrap overlong
//! pre/nowrap lines to keep content accessible without horizontal scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhiteSpace {
    #[default]
    Normal,
    NoWrap,
    Pre,
    PreWrap,
    PreLine,
    BreakSpaces,
}
impl WhiteSpace {
    pub fn preserves_spaces(self) -> bool {
        matches!(self, Self::Pre | Self::PreWrap | Self::BreakSpaces)
    }
    pub fn preserves_breaks(self) -> bool {
        self.preserves_spaces() || self == Self::PreLine
    }
    pub(crate) fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "normal" | "initial" => Self::Normal,
            "nowrap" => Self::NoWrap,
            "pre" => Self::Pre,
            "pre-wrap" => Self::PreWrap,
            "pre-line" => Self::PreLine,
            "break-spaces" => Self::BreakSpaces,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        css::{StyleSheet, TextStyle},
        xml::Element,
    };
    fn element(name: &str, style: &str) -> Element {
        Element {
            name: name.into(),
            attributes: vec![("style".into(), style.into())],
            empty: false,
        }
    }
    #[test]
    fn pre_defaults_and_author_cascade_inherit_across_syntax_spans() {
        let mut sheet = StyleSheet::default();
        let pre = sheet
            .compute(&element("pre", ""), TextStyle::default())
            .unwrap();
        assert_eq!(pre.white_space, WhiteSpace::PreWrap);
        assert_eq!(
            sheet
                .compute(&element("code", ""), pre)
                .unwrap()
                .white_space,
            WhiteSpace::PreWrap
        );
        sheet
            .append("pre {white-space: pre !important} span {white-space:pre-line}")
            .unwrap();
        let pre = sheet
            .compute(&element("pre", "white-space:normal"), TextStyle::default())
            .unwrap();
        assert_eq!(pre.white_space, WhiteSpace::Pre);
        assert_eq!(
            sheet
                .compute(&element("span", "white-space:inherit"), pre)
                .unwrap()
                .white_space,
            WhiteSpace::Pre
        );
        assert_eq!(
            sheet
                .compute(&element("span", "white-space:initial"), pre)
                .unwrap()
                .white_space,
            WhiteSpace::Normal
        );
        assert_eq!(
            sheet
                .compute(&element("span", "white-space:unset"), pre)
                .unwrap()
                .white_space,
            WhiteSpace::Pre
        );
        assert_eq!(
            sheet
                .compute(&element("code", ""), TextStyle::default())
                .unwrap()
                .white_space,
            WhiteSpace::Normal
        );
    }
    #[test]
    fn supported_modes_and_invalid_values_do_not_destroy_valid_declarations() {
        let mut sheet = StyleSheet::default();
        for (value, expected) in [
            ("normal", WhiteSpace::Normal),
            ("nowrap", WhiteSpace::NoWrap),
            ("pre", WhiteSpace::Pre),
            ("pre-wrap", WhiteSpace::PreWrap),
            ("pre-line", WhiteSpace::PreLine),
            ("break-spaces", WhiteSpace::BreakSpaces),
        ] {
            let style = sheet
                .compute(
                    &element("div", &format!("white-space:{value};white-space:unknown")),
                    TextStyle::default(),
                )
                .unwrap();
            assert_eq!(style.white_space, expected);
        }
    }
}

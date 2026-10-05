//! The single light/dark palette for desktop chrome and Android host surfaces.
//! Legacy paper/sepia settings remain readable; new choices are light and dark.
use readall_render::Color;
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Theme {
    #[default]
    Paper,
    Dark,
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub canvas: Color,
    pub panel: Color,
    pub ink: Color,
    pub muted: Color,
    pub border: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub button: Color,
    pub hover: Color,
    pub selected: Color,
    pub pressed: Color,
    pub shadow: Color,
    pub scrim: Color,
    pub selection: Color,
    pub highlight: Color,
    pub danger: Color,
    pub danger_hover: Color,
    pub danger_pressed: Color,
}
const fn rgb(hex: u32) -> Color {
    Color::rgba((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255)
}
impl Theme {
    pub(crate) fn colors(self) -> (Color, Color) {
        match self {
            Self::Paper => (Color::WHITE, rgb(0x181818)),
            Self::Dark => (rgb(0x1c1f24), rgb(0xdee1e6)),
        }
    }
    pub(crate) fn palette(self) -> Palette {
        match self {
            Self::Paper => Palette {
                canvas: rgb(0xf4f6f9),
                panel: rgb(0xfafbfd),
                ink: rgb(0x222831),
                muted: rgb(0x566273),
                border: rgb(0xcbd3df),
                accent: rgb(0x3467be),
                on_accent: Color::WHITE,
                button: rgb(0xe7eefb),
                hover: rgb(0xdce8f9),
                selected: rgb(0xdce8f9),
                pressed: rgb(0xd2e2f8),
                shadow: Color::rgba(0, 0, 0, 32),
                scrim: Color::rgba(0, 0, 0, 90),
                selection: Color::rgba(55, 116, 216, 70),
                highlight: Color::rgba(250, 196, 40, 70),
                danger: rgb(0xa7373a),
                danger_hover: rgb(0xfdebea),
                danger_pressed: rgb(0xf5d5d4),
            },
            Self::Dark => Palette {
                canvas: rgb(0x15181e),
                panel: rgb(0x252a33),
                ink: rgb(0xdee1e6),
                muted: rgb(0xb4bfce),
                border: rgb(0x454f60),
                accent: rgb(0x8dbaff),
                on_accent: rgb(0x111c2e),
                button: rgb(0x2b3a50),
                hover: rgb(0x34465f),
                selected: rgb(0x34465f),
                pressed: rgb(0x405776),
                shadow: Color::rgba(0, 0, 0, 90),
                scrim: Color::rgba(0, 0, 0, 145),
                selection: Color::rgba(96, 158, 255, 95),
                highlight: Color::rgba(250, 196, 40, 80),
                danger: rgb(0xffa3a6),
                danger_hover: rgb(0x482d35),
                danger_pressed: rgb(0x623440),
            },
        }
    }
    pub(crate) fn text_color(self, [r, g, b]: [u8; 3]) -> Color {
        if self == Self::Paper {
            return Color::rgba(r, g, b, 255);
        }
        if [r, g, b].iter().all(|c| *c < 80) {
            return self.colors().1;
        }
        Color::rgba(r.max(80), g.max(80), b.max(80), 255)
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Paper => "light",
            Self::Dark => "dark",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Paper => "亮色",
            Self::Dark => "暗色",
        }
    }
    pub(crate) fn parse(value: &str) -> io::Result<Self> {
        match value {
            "light" | "paper" | "sepia" => Ok(Self::Paper),
            "dark" => Ok(Self::Dark),
            _ => Err(super::invalid(
                "theme expects light or dark (legacy paper/sepia are accepted)",
            )),
        }
    }
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Paper => Self::Dark,
            Self::Dark => Self::Paper,
        }
    }
    /// Stable host order: canvas, page, panel, ink, muted, border, accent,
    /// on-accent, button, selected, hover. Android does not duplicate RGB values.
    #[cfg(feature = "mobile")]
    pub(crate) fn host_colors(self) -> [u32; 11] {
        let p = self.palette();
        [
            p.canvas,
            self.colors().0,
            p.panel,
            p.ink,
            p.muted,
            p.border,
            p.accent,
            p.on_accent,
            p.button,
            p.selected,
            p.hover,
        ]
        .map(|c| {
            (u32::from(c.a) << 24) | (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn luminance(c: Color) -> f64 {
        let linear = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
    }
    fn contrast(a: Color, b: Color) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    #[test]
    fn exactly_two_choices_and_legacy_preferences_migrate() {
        assert_eq!(Theme::Paper.next(), Theme::Dark);
        assert_eq!(Theme::Dark.next(), Theme::Paper);
        for legacy in ["light", "paper", "sepia"] {
            assert_eq!(Theme::parse(legacy).unwrap(), Theme::Paper);
        }
        for t in [Theme::Paper, Theme::Dark] {
            assert_eq!(Theme::parse(t.name()).unwrap(), t);
        }
        assert!(Theme::parse("unknown").is_err());
    }
    #[test]
    fn palettes_keep_controls_readable_on_both_backgrounds() {
        for t in [Theme::Paper, Theme::Dark] {
            let p = t.palette();
            for bg in [p.canvas, p.panel, p.button, p.hover, p.selected] {
                assert!(contrast(p.ink, bg) >= 4.5, "{:?}: ink", t);
                assert!(contrast(p.muted, bg) >= 4.5, "{:?}: muted", t);
            }
            assert!(contrast(p.on_accent, p.accent) >= 4.5);
            assert!(contrast(p.danger, p.danger_hover) >= 4.5);
            assert!(contrast(t.colors().1, t.colors().0) >= 7.0);
        }
    }
}

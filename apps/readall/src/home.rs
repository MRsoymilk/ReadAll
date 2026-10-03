//! Minimal native application home screen. It deliberately does not depend on an external font.
//! ASCII labels are rendered with a tiny built-in 5x7 bitmap so zero-argument startup is self-contained.
use std::io::Write;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(not(all(target_os = "linux", feature = "wayland")))]
pub(crate) fn run(_: &mut impl Write) -> Result<()> {
    Err("ReadAll GUI is unavailable in this build: on Linux rebuild with --features wayland; Windows/Android GUI backends are not implemented yet".into())
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod enabled {
    use super::*;
    use readall_platform::window::{
        self, Action, WindowHandler, WindowOptions, WindowReport, WindowResult,
    };
    use readall_render::{Color, DrawCommand, Rect, RenderLimits, Surface};

    const BG: Color = Color::rgba(238, 241, 245, 255);
    const PANEL: Color = Color::rgba(255, 255, 255, 255);
    const INK: Color = Color::rgba(32, 38, 46, 255);
    const MUTED: Color = Color::rgba(100, 110, 124, 255);
    const ACCENT: Color = Color::rgba(53, 105, 190, 255);
    const BORDER: Color = Color::rgba(204, 211, 220, 255);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Choice {
        Epub,
        Text,
    }

    struct Home {
        surface: Surface,
        choice: Choice,
    }

    impl Home {
        fn new(width: u32, height: u32) -> WindowResult<Self> {
            let mut home = Self {
                surface: Surface::new(width, height, RenderLimits::default())?,
                choice: Choice::Epub,
            };
            home.paint()?;
            Ok(home)
        }

        fn paint(&mut self) -> WindowResult<()> {
            let w = self.surface.width();
            let h = self.surface.height();
            let mut commands = vec![DrawCommand::FillRect {
                rect: Rect::new(0, 0, w, h),
                color: BG,
            }];
            let panel_w = w.saturating_sub(80).min(960);
            let panel_h = h.saturating_sub(80).min(620);
            let px = ((w - panel_w) / 2) as i32;
            let py = ((h - panel_h) / 2) as i32;
            commands.push(DrawCommand::FillRect {
                rect: Rect::new(px, py, panel_w, panel_h),
                color: PANEL,
            });

            let card_gap = 24_u32;
            let card_w = panel_w.saturating_sub(72 + card_gap) / 2;
            let card_h = 180_u32.min(panel_h.saturating_sub(210));
            let card_y = py + 120;
            let left_x = px + 24;
            let right_x = left_x + card_w as i32 + card_gap as i32;
            draw_card(
                &mut commands,
                Rect::new(left_x, card_y, card_w, card_h),
                self.choice == Choice::Epub,
            );
            draw_card(
                &mut commands,
                Rect::new(right_x, card_y, card_w, card_h),
                self.choice == Choice::Text,
            );
            self.surface.draw(&commands)?;

            draw_text(&mut self.surface, px + 28, py + 28, 4, "READALL", INK)?;
            draw_text(
                &mut self.surface,
                px + 30,
                py + 72,
                2,
                "NATIVE RUST READER",
                MUTED,
            )?;
            draw_text(
                &mut self.surface,
                left_x + 22,
                card_y + 28,
                3,
                "OPEN EPUB",
                INK,
            )?;
            draw_text(
                &mut self.surface,
                left_x + 22,
                card_y + 82,
                2,
                "EPUB XHTML TEXT READER",
                MUTED,
            )?;
            draw_text(
                &mut self.surface,
                right_x + 22,
                card_y + 28,
                3,
                "OPEN TXT",
                INK,
            )?;
            draw_text(
                &mut self.surface,
                right_x + 22,
                card_y + 82,
                2,
                "PLAIN TEXT READER",
                MUTED,
            )?;
            let hint_y = card_y + card_h as i32 + 36;
            draw_text(
                &mut self.surface,
                px + 30,
                hint_y,
                2,
                "LEFT RIGHT OR CLICK TO SELECT",
                MUTED,
            )?;
            draw_text(
                &mut self.surface,
                px + 30,
                hint_y + 32,
                2,
                "FILE PICKER IS THE NEXT GUI STEP",
                MUTED,
            )?;
            draw_text(
                &mut self.surface,
                px + 30,
                hint_y + 64,
                2,
                "ESC CLOSES THE WINDOW",
                MUTED,
            )?;
            Ok(())
        }

        fn select(&mut self, choice: Choice) -> WindowResult<bool> {
            if self.choice == choice {
                return Ok(false);
            }
            self.choice = choice;
            self.paint()?;
            Ok(true)
        }
    }

    impl WindowHandler for Home {
        fn resize(&mut self, width: u32, height: u32) -> WindowResult<bool> {
            if (width, height) == (self.surface.width(), self.surface.height()) {
                return Ok(false);
            }
            if width < 420 || height < 320 {
                return Err("ReadAll home window requires at least 420x320".into());
            }
            self.surface = Surface::new(width, height, RenderLimits::default())?;
            self.paint()?;
            Ok(true)
        }

        fn action(&mut self, action: Action) -> WindowResult<bool> {
            match action {
                Action::Previous | Action::First | Action::Smaller => self.select(Choice::Epub),
                Action::Next | Action::Last | Action::Larger => self.select(Choice::Text),
                Action::Close => Ok(false),
            }
        }

        fn surface(&self) -> &Surface {
            &self.surface
        }

        fn title(&self) -> String {
            "ReadAll".into()
        }
    }

    fn draw_card(commands: &mut Vec<DrawCommand>, rect: Rect, selected: bool) {
        let border = if selected { ACCENT } else { BORDER };
        commands.push(DrawCommand::FillRect {
            rect,
            color: border,
        });
        let inner = Rect::new(
            rect.x + 3,
            rect.y + 3,
            rect.width.saturating_sub(6),
            rect.height.saturating_sub(6),
        );
        commands.push(DrawCommand::FillRect {
            rect: inner,
            color: PANEL,
        });
        if selected {
            commands.push(DrawCommand::FillRect {
                rect: Rect::new(inner.x, inner.y, 7, inner.height),
                color: ACCENT,
            });
        }
    }

    fn draw_text(
        surface: &mut Surface,
        x: i32,
        y: i32,
        scale: u32,
        text: &str,
        color: Color,
    ) -> WindowResult<()> {
        let mut commands = Vec::new();
        let mut cursor = x;
        for ch in text.chars() {
            if ch == ' ' {
                cursor += (6 * scale) as i32;
                continue;
            }
            let glyph = glyph(ch).unwrap_or([0; 7]);
            for (row, bits) in glyph.into_iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) != 0 {
                        commands.push(DrawCommand::FillRect {
                            rect: Rect::new(
                                cursor + (col * scale) as i32,
                                y + (row as u32 * scale) as i32,
                                scale,
                                scale,
                            ),
                            color,
                        });
                    }
                }
            }
            cursor += (6 * scale) as i32;
        }
        surface.draw(&commands)?;
        Ok(())
    }

    fn glyph(ch: char) -> Option<[u8; 7]> {
        Some(match ch.to_ascii_uppercase() {
            'A' => [14, 17, 17, 31, 17, 17, 17],
            'B' => [30, 17, 17, 30, 17, 17, 30],
            'C' => [14, 17, 16, 16, 16, 17, 14],
            'D' => [30, 17, 17, 17, 17, 17, 30],
            'E' => [31, 16, 16, 30, 16, 16, 31],
            'F' => [31, 16, 16, 30, 16, 16, 16],
            'G' => [14, 17, 16, 23, 17, 17, 15],
            'H' => [17, 17, 17, 31, 17, 17, 17],
            'I' => [31, 4, 4, 4, 4, 4, 31],
            'J' => [7, 2, 2, 2, 18, 18, 12],
            'K' => [17, 18, 20, 24, 20, 18, 17],
            'L' => [16, 16, 16, 16, 16, 16, 31],
            'M' => [17, 27, 21, 21, 17, 17, 17],
            'N' => [17, 25, 21, 19, 17, 17, 17],
            'O' => [14, 17, 17, 17, 17, 17, 14],
            'P' => [30, 17, 17, 30, 16, 16, 16],
            'Q' => [14, 17, 17, 17, 21, 18, 13],
            'R' => [30, 17, 17, 30, 20, 18, 17],
            'S' => [15, 16, 16, 14, 1, 1, 30],
            'T' => [31, 4, 4, 4, 4, 4, 4],
            'U' => [17, 17, 17, 17, 17, 17, 14],
            'V' => [17, 17, 17, 17, 17, 10, 4],
            'W' => [17, 17, 17, 21, 21, 21, 10],
            'X' => [17, 17, 10, 4, 10, 17, 17],
            'Y' => [17, 17, 10, 4, 4, 4, 4],
            'Z' => [31, 1, 2, 4, 8, 16, 31],
            '0' => [14, 17, 19, 21, 25, 17, 14],
            '1' => [4, 12, 4, 4, 4, 4, 14],
            '2' => [14, 17, 1, 2, 4, 8, 31],
            '3' => [30, 1, 1, 14, 1, 1, 30],
            '4' => [2, 6, 10, 18, 31, 2, 2],
            '5' => [31, 16, 16, 30, 1, 1, 30],
            '6' => [14, 16, 16, 30, 17, 17, 14],
            '7' => [31, 1, 2, 4, 8, 8, 8],
            '8' => [14, 17, 17, 14, 17, 17, 14],
            '9' => [14, 17, 17, 15, 1, 1, 14],
            '-' => [0, 0, 0, 31, 0, 0, 0],
            _ => return None,
        })
    }

    pub(super) fn start(output: &mut impl Write) -> Result<()> {
        writeln!(
            output,
            "Starting ReadAll GUI. Use --help for command-line tools."
        )?;
        output.flush()?;
        let mut home = Home::new(900, 600)?;
        let report: WindowReport = window::run(&mut home, WindowOptions::default())?;
        writeln!(
            output,
            "ReadAll GUI closed. Buffer commits: {}; last size: {}x{}",
            report.committed_frames, report.width, report.height
        )?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn home_draws_nonempty_ui_and_reacts_to_selection() {
            let mut home = Home::new(900, 600).unwrap();
            let initial = home.surface.pixels().to_vec();
            assert!(home.action(Action::Next).unwrap());
            assert_ne!(home.surface.pixels(), initial);
            assert!(!home.action(Action::Next).unwrap());
            assert!(home.action(Action::Previous).unwrap());
        }

        #[test]
        fn bitmap_font_covers_all_home_labels() {
            for ch in "READALLNATIVE RUST READEROPEN EPUBXHTML TEXTPLAINFILE PICKER IS THE NEXT GUI STEPESC CLOSES WINDOWLEFT RIGHT OR CLICK TO SELECT".chars() {
                if ch != ' ' {
                    assert!(glyph(ch).is_some(), "missing {ch}");
                }
            }
        }
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) fn run(output: &mut impl Write) -> Result<()> {
    enabled::start(output)
}

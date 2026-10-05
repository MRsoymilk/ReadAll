//! Desktop wheel routing and bounded control hover detection.
use super::*;

impl ReaderWindow<'_, '_, '_, '_> {
    pub(in crate::native_epub::enabled) fn desktop_tool_scroll(
        &mut self,
        dx: i32,
        dy: i32,
    ) -> Option<bool> {
        if matches!(self.tools.mode, Mode::None | Mode::External) {
            return None;
        }
        if self
            .pointer
            .is_some_and(|(x, y)| !point_in(self.tool_panel(), x, y))
        {
            return Some(false);
        }
        if self.tools.mode == Mode::Zoom {
            let before = self.tools.pan;
            self.tools.pan.0 = self
                .tools
                .pan
                .0
                .saturating_sub(dx / 256)
                .clamp(-32000, 32000);
            self.tools.pan.1 = self
                .tools
                .pan
                .1
                .saturating_sub(dy / 256)
                .clamp(-32000, 32000);
            return Some(before != self.tools.pan);
        }
        if !matches!(
            self.tools.mode,
            Mode::Settings | Mode::Search | Mode::Annotations
        ) {
            return Some(false);
        }
        let delta = f64::from(dy) / 256.0;
        if delta == 0.0 {
            return Some(false);
        }
        if self.tools.desktop_wheel.signum() != delta.signum() {
            self.tools.desktop_wheel = 0.0;
        }
        self.tools.desktop_wheel = (self.tools.desktop_wheel + delta).clamp(-4096.0, 4096.0);
        let steps = (self.tools.desktop_wheel / 42.0).trunc() as isize;
        if steps == 0 {
            return Some(false);
        }
        self.tools.desktop_wheel -= steps as f64 * 42.0;
        let previous = self.tools.scroll;
        self.tools.scroll = self
            .tools
            .scroll
            .saturating_add_signed(steps)
            .min(self.tool_count().saturating_sub(self.tool_rows()));
        if previous == self.tools.scroll {
            self.tools.desktop_wheel = 0.0;
            return Some(false);
        }
        self.tools.selected = self.tools.selected.clamp(
            self.tools.scroll,
            (self.tools.scroll + self.tool_rows() - 1).min(self.tool_count().saturating_sub(1)),
        );
        self.pointer = None;
        Some(true)
    }
    pub(super) fn menu_hover_control(&self) -> Option<Rect> {
        if self.mobile_chrome() {
            return None;
        }
        let (x, y) = self.pointer?;
        let panel = self.tool_panel();
        if self.tools.mode == Mode::None {
            return None;
        }
        let close = Rect::new(panel.x + panel.width as i32 - 40, panel.y + 5, 30, 28);
        if point_in(close, x, y) {
            return Some(close);
        }
        if self.tools.mode == Mode::Zoom {
            let (minus, plus) = self.mobile_zoom_buttons();
            return [minus, plus].into_iter().find(|r| point_in(*r, x, y));
        }
        if self.tools.editing() {
            let submit = Rect::new(panel.x + panel.width as i32 - 78, panel.y + 40, 66, 38);
            if point_in(submit, x, y) {
                return Some(submit);
            }
        }
        if self.tools.mode == Mode::Settings {
            let index = self.tool_row_at(x, y)?;
            let y0 = panel.y + 88 + (index - self.tools.scroll) as i32 * 42;
            let (minus, plus) = self.mobile_setting_buttons(y0);
            return [minus, plus].into_iter().find(|r| point_in(*r, x, y));
        }
        None
    }
}

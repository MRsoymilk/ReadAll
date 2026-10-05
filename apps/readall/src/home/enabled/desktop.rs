//! Desktop library navigation, non-blocking previews and short-lived feedback.
use super::*;
use std::time::{Duration, Instant};
#[cfg(test)]
mod tests;

impl Home<'_> {
    pub(super) fn sidebar_rect(&self, index: usize) -> Rect {
        Rect::new(16, 112 + index as i32 * 40, 184, 36)
    }
    pub(super) fn recent_capacity(&self) -> usize {
        ((self.surface.height() as i32 - 54 - RECENT_TOP + 4).max(0) as usize
            / RECENT_ROW_HEIGHT as usize)
            .max(1)
    }
    pub(super) fn paint_sidebar(&mut self) -> WindowResult<()> {
        let p = self.theme.palette();
        let active = if matches!(self.mode, Mode::Browser(_)) {
            1
        } else if self.recent_selected.is_some() {
            2
        } else {
            0
        };
        for (i, label) in ["书库", "打开图书", "最近阅读"].into_iter().enumerate() {
            let rect = self.sidebar_rect(i);
            let hovered = self.hover_target() == HoverTarget::Sidebar(i);
            if i == active || hovered {
                crate::ui::rounded(
                    &mut self.surface,
                    rect,
                    10,
                    if i == active { p.selected } else { p.hover },
                )?;
            }
            let mut text = UiPainter::new(self.ui_font, &mut self.surface)?;
            text.draw_clipped(
                rect.x + 14,
                rect.y + 10,
                14,
                label,
                if i == active { p.accent } else { p.ink },
                rect,
            )?;
        }
        Ok(())
    }
    fn focus_recent(&mut self, selected: Option<usize>) {
        self.pending_recent_delete = None;
        self.pointer = None;
        self.recent_selected = selected.filter(|&i| i < self.recent.len());
        if let Some(i) = self.recent_selected {
            let cap = self.recent_capacity();
            if i < self.recent_scroll {
                self.recent_scroll = i;
            } else if i >= self.recent_scroll + cap {
                self.recent_scroll = i + 1 - cap;
            }
        } else {
            self.recent_scroll = 0;
        }
    }
    pub(super) fn desktop_home_action(&mut self, action: Action) -> WindowResult<Option<bool>> {
        if let Action::Click { x, y } = action {
            for i in 0..3 {
                if point_in(self.sidebar_rect(i), x, y) {
                    self.pending_recent_delete = None;
                    if i == 1 {
                        return match self.open_browser() {
                            Ok(changed) => Ok(Some(changed)),
                            Err(error) => {
                                self.status = format!("无法访问：{error}");
                                self.paint()?;
                                Ok(Some(true))
                            }
                        };
                    }
                    self.mode = Mode::Library;
                    self.status.clear();
                    self.focus_recent(if i == 2 && !self.recent.is_empty() {
                        Some(0)
                    } else {
                        None
                    });
                    self.paint()?;
                    return Ok(Some(true));
                }
            }
        }
        if action == Action::Close && matches!(self.mode, Mode::Browser(_)) {
            self.mode = Mode::Library;
            self.pointer = None;
            self.pending_recent_delete = None;
            self.status.clear();
            self.paint()?;
            return Ok(Some(true));
        }
        if let Action::Scroll { dy, .. } = action {
            let allowed = match &self.mode {
                Mode::Browser(_) => self.pointer.is_none_or(|(x, y)| {
                    x >= 250
                        && x < (self.surface.width() as i32 - 36)
                        && y >= LIST_TOP
                        && y < self.surface.height() as i32 - LIST_BOTTOM_MARGIN
                }),
                Mode::Library => self.pointer.is_none_or(|(x, y)| {
                    x >= 270
                        && x < self.recent_row_rect(0).x + self.recent_row_rect(0).width as i32
                        && y >= RECENT_TOP
                        && y < self.surface.height() as i32 - 54
                }),
            };
            if !allowed || dy == 0 {
                return Ok(Some(false));
            }
            let delta = f64::from(dy) / 256.0;
            if self.wheel_remainder.signum() != delta.signum() {
                self.wheel_remainder = 0.0;
            }
            let unit = if matches!(self.mode, Mode::Library) {
                RECENT_ROW_HEIGHT
            } else {
                ROW_HEIGHT
            } as f64;
            self.wheel_remainder = (self.wheel_remainder + delta).clamp(-8192.0, 8192.0);
            let steps = (self.wheel_remainder / unit).trunc() as isize;
            if steps == 0 {
                return Ok(Some(false));
            }
            self.wheel_remainder -= steps as f64 * unit;
            let visible = self.visible_rows();
            let cap = self.recent_capacity();
            let changed = match &mut self.mode {
                Mode::Browser(b) => {
                    let old = b.scroll;
                    b.scroll = b
                        .scroll
                        .saturating_add_signed(steps)
                        .min(b.entries.len().saturating_sub(visible));
                    let before = b.selected;
                    b.selected = b.selected.clamp(
                        b.scroll,
                        (b.scroll + visible - 1).min(b.entries.len().saturating_sub(1)),
                    );
                    if b.selected != before {
                        b.refresh_preview();
                    }
                    old != b.scroll
                }
                Mode::Library => {
                    let old = self.recent_scroll;
                    self.recent_scroll = self
                        .recent_scroll
                        .saturating_add_signed(steps)
                        .min(self.recent.len().saturating_sub(cap));
                    if !self.recent.is_empty() {
                        self.recent_selected =
                            Some(self.recent_selected.unwrap_or(self.recent_scroll).clamp(
                                self.recent_scroll,
                                (self.recent_scroll + cap - 1).min(self.recent.len() - 1),
                            ));
                    }
                    old != self.recent_scroll
                }
            };
            if changed {
                self.pointer = None;
                self.pending_recent_delete = None;
                self.paint()?;
            } else {
                self.wheel_remainder = 0.0;
            }
            return Ok(Some(changed));
        }
        if matches!(self.mode, Mode::Library) {
            let selected = match action {
                Action::Next => {
                    if self.recent.is_empty() {
                        None
                    } else {
                        Some(
                            self.recent_selected
                                .map_or(0, |i| (i + 1).min(self.recent.len() - 1)),
                        )
                    }
                }
                Action::Previous => self.recent_selected.and_then(|i| i.checked_sub(1)),
                Action::First => None,
                Action::Last => self.recent.len().checked_sub(1),
                _ => return Ok(None),
            };
            let before = (self.recent_selected, self.recent_scroll);
            self.focus_recent(selected);
            if before == (self.recent_selected, self.recent_scroll) {
                return Ok(Some(false));
            }
            self.paint()?;
            return Ok(Some(true));
        }
        Ok(None)
    }
    pub(super) fn queue_preview(&mut self, now: Instant) {
        let path = match &self.mode {
            Mode::Browser(b) => b
                .selected()
                .filter(|e| !e.directory)
                .map(|e| e.path.clone()),
            Mode::Library => None,
        };
        self.preview_worker.select(path, now);
        self.poll_preview(now);
    }
    pub(super) fn poll_preview(&mut self, now: Instant) -> bool {
        if let Some(value) = self.preview_worker.poll(now)
            && let Mode::Browser(b) = &mut self.mode
            && b.preview.as_ref() != Some(&value)
        {
            b.preview = Some(value);
            return true;
        }
        false
    }
    pub(super) fn observe_home_notice(&mut self, now: Instant) {
        if self.status.is_empty()
            || self.status.starts_with("支持 EPUB")
            || self.status.starts_with("选择 EPUB")
        {
            self.status_notice = None;
        } else if self
            .status_notice
            .as_ref()
            .is_none_or(|(text, _)| text != &self.status)
        {
            self.status_notice = Some((self.status.clone(), now + Duration::from_secs(6)));
        }
    }
    pub(super) fn expire_home_notice(&mut self, now: Instant) -> bool {
        if self
            .status_notice
            .as_ref()
            .is_some_and(|(text, deadline)| text == &self.status && now >= *deadline)
        {
            self.status.clear();
            self.status_notice = None;
            return true;
        }
        false
    }
}

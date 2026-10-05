//! Desktop-only interaction policy; Android gesture routing is left unchanged.
use super::*;
use std::time::Instant;
#[cfg(test)]
mod tests;

impl ReaderWindow<'_, '_, '_, '_> {
    pub(super) fn desktop_notice_close(&self) -> Rect {
        let right = if self.tools.link_history.is_empty() {
            self.surface.width() as i32 - 12
        } else {
            self.link_back_rect().x - 4
        };
        Rect::new(right - 24, 35, 24, 24)
    }
    pub(super) fn observe_desktop_notice(&mut self, now: Instant) {
        if self.mobile_chrome()
            || self.tools.mode != tools::Mode::None
            || self.tools.pending()
            || self.tools.status.is_empty()
        {
            self.desktop_notice = None;
        } else if self
            .desktop_notice
            .as_ref()
            .is_none_or(|(text, _)| text != &self.tools.status)
        {
            self.desktop_notice = Some((self.tools.status.clone(), now + Duration::from_secs(6)));
        }
    }
    pub(super) fn expire_desktop_notice(&mut self, now: Instant) -> bool {
        if self.mobile_chrome() || self.tools.mode != tools::Mode::None || self.tools.pending() {
            return false;
        }
        if self
            .desktop_notice
            .as_ref()
            .is_some_and(|(text, deadline)| text == &self.tools.status && now >= *deadline)
        {
            self.desktop_notice = None;
            self.tools.status.clear();
            return true;
        }
        false
    }
    pub(super) fn desktop_action(&mut self, action: Action) -> WindowResult<Option<bool>> {
        if self.mobile_chrome() {
            return Ok(None);
        }
        if matches!(action, Action::Command(_)) {
            self.desktop_notice = None;
        }
        if let Action::Scroll { dx, dy } = action
            && let Some(changed) = self.desktop_tool_scroll(dx, dy)
        {
            if changed {
                self.refresh_surface()?;
            }
            return Ok(Some(changed));
        }
        if let Action::Click { x, y } = action
            && self.tools.mode == tools::Mode::None
            && !self.tools.status.is_empty()
            && !self.compact_toc()
            && !self.tools.selecting
            && self.tools.selection.is_none()
            && !self.tools.dragging()
            && point_in(self.desktop_notice_close(), x, y)
        {
            self.tools.status.clear();
            self.desktop_notice = None;
            self.refresh_surface()?;
            return Ok(Some(true));
        }
        // Esc first lets an active turn/selection/input finish. Then close the TOC,
        // then the toolbar; only a subsequent Esc can close the reading window.
        if action == Action::Close
            && !self.motion.active()
            && !self.motion.panning()
            && self.tools.mode == tools::Mode::None
            && !self.tools.selecting
            && self.tools.selection.is_none()
            && !self.tools.dragging()
        {
            match self.toolbar {
                ToolbarMode::Toc => self.toolbar = ToolbarMode::Expanded,
                ToolbarMode::Expanded => self.toolbar = ToolbarMode::Collapsed,
                ToolbarMode::Collapsed => return Ok(None),
            }
            self.refresh_surface()?;
            return Ok(Some(true));
        }
        Ok(None)
    }
}

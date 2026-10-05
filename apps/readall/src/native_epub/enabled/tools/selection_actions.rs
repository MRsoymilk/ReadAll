//! Context actions for an existing selection. Do not cover the pointer during a
//! drag or pass action clicks through to text, links, images or page controls.
use super::*;
const BUTTON_HEIGHT: u32 = 32;
const BAR_HEIGHT: u32 = 54;

impl ReaderWindow<'_, '_, '_, '_> {
    fn selection_actions_rect(&self) -> Option<Rect> {
        if self.tools.mode != Mode::None
            || self.tools.drag.is_some()
            || self.toolbar == ToolbarMode::Toc
        {
            return None;
        }
        let range = self.tools.selection.as_ref()?;
        let mut hits = self.session.frame().hits.iter().filter(|hit| {
            hit.start < range.end
                && hit.end > range.start
                && hit.rect.width > 0
                && hit.rect.height > 0
        });
        let first = hits.next()?;
        let last = hits.next_back().unwrap_or(first);
        let width = self.surface.width().saturating_sub(16).min(248);
        let bottom = if self.toolbar == ToolbarMode::Collapsed {
            self.collapsed_rect().y - 8
        } else {
            self.toolbar_rect().y - 8
        };
        if width < 160 || bottom < 34 + BAR_HEIGHT as i32 {
            return None;
        }
        let x = (last.rect.x + last.rect.width as i32 / 2 - width as i32 / 2)
            .clamp(8, self.surface.width() as i32 - width as i32 - 8);
        let below = last.rect.y + last.rect.height as i32 + 8;
        let y = if below + BAR_HEIGHT as i32 <= bottom {
            below
        } else {
            first.rect.y - BAR_HEIGHT as i32 - 8
        }
        .clamp(34, bottom - BAR_HEIGHT as i32);
        Some(Rect::new(x, y, width, BAR_HEIGHT))
    }
    fn selection_button(bar: Rect, index: usize) -> Rect {
        let start = bar.width * index as u32 / 4;
        let end = bar.width * (index as u32 + 1) / 4;
        Rect::new(bar.x + start as i32, bar.y, end - start, BUTTON_HEIGHT)
    }
    pub(super) fn selection_action_click(&mut self, x: i32, y: i32) -> WindowResult<Option<bool>> {
        let Some(bar) = self.selection_actions_rect() else {
            return Ok(None);
        };
        if !point_in(bar, x, y) {
            return Ok(None);
        }
        for (index, command) in [
            Some(ReaderCommand::Copy),
            Some(ReaderCommand::Highlight),
            Some(ReaderCommand::Note),
            None,
        ]
        .into_iter()
        .enumerate()
        {
            if point_in(Self::selection_button(bar, index), x, y) {
                if let Some(command) = command {
                    self.tool_command(command)?;
                } else {
                    self.tools.clear_selection();
                    self.tools.status.clear();
                }
                return Ok(Some(true));
            }
        }
        // Feedback belongs to the selection bar too, not the document below it.
        Ok(Some(false))
    }
    pub(super) fn draw_selection_actions(&mut self) -> WindowResult<()> {
        let palette = self.session.settings().theme.palette();
        let Some(bar) = self.selection_actions_rect() else {
            return Ok(());
        };
        self.menu_fill(bar, palette.panel, 13)?;
        for (index, label) in ["复制", "高亮", "笔记", "取消"].into_iter().enumerate() {
            let cell = Self::selection_button(bar, index);
            if index == 0 {
                self.menu_fill(
                    Rect::new(
                        cell.x + 4,
                        cell.y + 4,
                        cell.width.saturating_sub(8),
                        cell.height.saturating_sub(4),
                    ),
                    palette.selected,
                    9,
                )?;
            }
            self.menu_label(
                cell,
                label,
                13,
                if index == 0 {
                    palette.accent
                } else {
                    palette.ink
                },
            )?;
        }
        let hint = if self.tools.status.is_empty() {
            if self.mobile_chrome() {
                "选中文字"
            } else {
                "Ctrl+C 复制 · F8 高亮 · F7 笔记"
            }
        } else {
            &self.tools.status
        };
        let mut text = UiPainter::new(&self.ui_font, &mut self.surface)?;
        let hint = text.fit(10, hint, bar.width.saturating_sub(24))?;
        text.draw_clipped(bar.x + 12, bar.y + 37, 10, &hint, palette.muted, bar)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

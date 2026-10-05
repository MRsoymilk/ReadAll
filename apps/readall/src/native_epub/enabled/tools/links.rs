//! Clickable body links and a bounded return stack, separate from persisted progress.
use super::*;

impl ReaderWindow<'_, '_, '_, '_> {
    pub(in crate::native_epub::enabled) fn link_back_rect(&self) -> Rect {
        Rect::new(self.surface.width().saturating_sub(88) as i32, 34, 80, 26)
    }
    pub(super) fn follow_body_link(
        &mut self,
        link: readall_epub::ContentLink,
    ) -> WindowResult<bool> {
        if readall_platform::web_link::WebLink::is_web_reference(&link.href) {
            return self.offer_external_link(&link.href);
        }
        let origin = self.session.anchor().clone();
        let result = self
            .session
            .book()
            .link_locator(self.session.current_spine(), &link.href)
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
            .and_then(|locator| self.session.jump_to_locator(locator));
        match result {
            Ok(_) => {
                if self.session.anchor() != &origin {
                    if self.tools.link_history.len() >= 64 {
                        self.tools.link_history.remove(0);
                    }
                    self.tools.link_history.push(origin);
                }
                self.tools.clear_selection();
                self.sync_toc_selection();
                self.save_progress();
                self.tools.status = if link.noteref {
                    "已跳转到注释；Backspace 或“返回”回到原处"
                } else {
                    "已打开书内链接；Backspace 或“返回”回到原处"
                }
                .into();
            }
            Err(error) => self.tools.status = format!("链接未打开：{error}"),
        }
        // A failed link is handled, not reinterpreted as a page-turn click.
        Ok(true)
    }
    pub(super) fn return_from_link(&mut self) -> WindowResult<bool> {
        let Some(origin) = self.tools.link_history.last().cloned() else {
            return Ok(false);
        };
        match self.session.jump_to_locator(origin) {
            Ok(_) => {
                self.tools.link_history.pop();
                self.tools.clear_selection();
                self.sync_toc_selection();
                self.save_progress();
                self.tools.status = if self.tools.link_history.is_empty() {
                    String::new()
                } else {
                    "已返回；Backspace 可继续返回上一处".into()
                };
            }
            Err(error) => self.tools.status = format!("返回失败：{error}"),
        }
        Ok(true)
    }
    pub(in super::super) fn draw_link_marks(&mut self) -> WindowResult<()> {
        // Reuse clipped shaping hit regions, including bidi text and image links.
        // Author text colors remain unchanged; only an underline marks interaction.
        for (rect, _) in self.session.link_regions() {
            if rect.width == 0 || rect.height == 0 {
                continue;
            }
            self.surface.draw(&[DrawCommand::FillRect {
                rect: Rect::new(
                    rect.x,
                    rect.y.saturating_add(rect.height as i32 - 1),
                    rect.width,
                    1,
                ),
                color: self.session.settings().theme.palette().accent,
            }])?;
        }
        Ok(())
    }
}

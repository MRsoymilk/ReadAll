//! Shared modern reader menus; desktop hover/keyboard hints do not change document layout.
use super::*;
use readall_render::Color;

pub(super) const CLOSE: &str = r#"<svg viewBox="0 0 24 24"><line x1="6" y1="6" x2="18" y2="18"/><line x1="18" y1="6" x2="6" y2="18"/></svg>"#;

pub(super) use crate::ui::rounded;

impl ReaderWindow<'_, '_, '_, '_> {
    pub(super) fn mobile_chrome(&self) -> bool {
        self.host_effects.is_some()
    }
    pub(super) fn menu_fill(&mut self, rect: Rect, color: Color, radius: u32) -> WindowResult<()> {
        rounded(&mut self.surface, rect, radius, color)
    }
    pub(super) fn menu_button(&mut self, rect: Rect, fill: Color, radius: u32) -> WindowResult<()> {
        let p = self.session.settings().theme.palette();
        let hover =
            !self.mobile_chrome() && self.pointer.is_some_and(|(x, y)| point_in(rect, x, y));
        self.menu_fill(
            rect,
            if hover && fill != p.accent {
                p.hover
            } else {
                fill
            },
            radius,
        )
    }
    pub(super) fn menu_label(
        &mut self,
        rect: Rect,
        label: &str,
        size: u32,
        color: Color,
    ) -> WindowResult<()> {
        let mut text = UiPainter::new(&self.ui_font, &mut self.surface)?;
        let label = text.fit(size, label, rect.width.saturating_sub(8))?;
        let width = text.measure(size, &label)?;
        text.draw_clipped(
            rect.x + rect.width.saturating_sub(width) as i32 / 2,
            rect.y + rect.height.saturating_sub(size) as i32 / 2,
            size,
            &label,
            color,
            rect,
        )?;
        Ok(())
    }
    pub(super) fn draw_modern_collapsed(&mut self) -> WindowResult<()> {
        let p = self.session.settings().theme.palette();
        let r = self.collapsed_rect();
        self.menu_fill(
            r,
            if self.hover_target() == ReaderHover::Collapsed {
                p.hover
            } else {
                p.panel
            },
            15,
        )?;
        svg_icon::draw(
            &mut self.surface,
            CHEVRON_UP,
            Rect::new(r.x + r.width as i32 / 2 - 9, r.y + 6, 18, 18),
            p.muted,
        )?;
        Ok(())
    }
    pub(super) fn draw_modern_toolbar(&mut self) -> WindowResult<()> {
        let p = self.session.settings().theme.palette();
        let r = self.toolbar_rect();
        let hover = self.hover_target();
        if !self.mobile_chrome() {
            self.menu_fill(
                Rect::new(r.x, r.y + 1, r.width, r.height),
                Color {
                    a: p.shadow.a.min(16),
                    ..p.shadow
                },
                20,
            )?;
        }
        self.menu_fill(r, p.panel, 20)?;
        if self.mobile_chrome() {
            self.menu_fill(
                Rect::new(r.x + r.width as i32 / 2 - 16, r.y + 12, 32, 3),
                p.border,
                2,
            )?;
        } else {
            let handle = self.toolbar_button_rect(5);
            if hover == ReaderHover::Toolbar(5) {
                self.menu_fill(
                    Rect::new(
                        handle.x + 8,
                        handle.y + 3,
                        handle.width.saturating_sub(16),
                        22,
                    ),
                    p.hover,
                    10,
                )?;
            }
            svg_icon::draw(
                &mut self.surface,
                CHEVRON_DOWN,
                Rect::new(r.x + r.width as i32 / 2 - 9, r.y + 5, 18, 18),
                p.muted,
            )?;
        }
        for (i, source, label) in [(0, CHEVRON_LEFT, "上一页"), (4, CHEVRON_RIGHT, "下一页")]
        {
            let b = self.toolbar_button_rect(i);
            if hover == ReaderHover::Toolbar(i) {
                self.menu_fill(
                    Rect::new(
                        b.x + 3,
                        b.y + 4,
                        b.width.saturating_sub(6),
                        b.height.saturating_sub(8),
                    ),
                    p.hover,
                    12,
                )?;
            }
            svg_icon::draw(
                &mut self.surface,
                source,
                Rect::new(b.x + b.width as i32 / 2 - 12, b.y + 30, 24, 24),
                p.ink,
            )?;
            self.menu_label(Rect::new(b.x, b.y + 69, b.width, 24), label, 12, p.muted)?;
        }
        for (i, label) in [(1, "目录"), (2, "A−"), (3, "A+")] {
            let b = self.toolbar_button_rect(i);
            let active = i == 1 && self.toolbar == ToolbarMode::Toc;
            self.menu_fill(
                b,
                if active {
                    p.selected
                } else if hover == ReaderHover::Toolbar(i) {
                    p.hover
                } else {
                    p.button
                },
                10,
            )?;
            self.menu_label(
                b,
                label,
                if i == 1 { 14 } else { 16 },
                if active { p.accent } else { p.ink },
            )?;
        }
        Ok(())
    }
    pub(super) fn draw_modern_toc(&mut self) -> WindowResult<()> {
        let p = self.session.settings().theme.palette();
        let r = self.toc_panel_rect();
        self.menu_fill(r, p.panel, 18)?;
        let mut text = UiPainter::new(&self.ui_font, &mut self.surface)?;
        text.draw_clipped(r.x + 18, r.y + 14, 18, "目录", p.ink, r)?;
        let count = format!("{} 节", self.toc.len());
        let width = text.measure(12, &count)?;
        text.draw_clipped(
            r.x + r.width.saturating_sub(width + 18) as i32,
            r.y + 17,
            12,
            &count,
            p.muted,
            r,
        )?;
        self.draw_toc_rows()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rounded_menu_corners_are_antialiased_at_device_resolution_and_snapshots_stay_immutable() {
        for pixels in [(100, 100), (275, 275)] {
            let mut s = Surface::new_scaled((100, 100), pixels, Default::default()).unwrap();
            let before = s.clone();
            let ink = Color::rgba(20, 30, 40, 255);
            rounded(&mut s, Rect::new(10, 10, 80, 70), 20, ink).unwrap();
            let r = s.pixel_rect(Rect::new(10, 10, 80, 70));
            assert_eq!(s.pixel(r.x as u32, r.y as u32).unwrap().a, 0);
            assert_eq!(
                s.pixel(
                    (r.x + r.width as i32 / 2) as u32,
                    (r.y + r.height as i32 / 2) as u32
                ),
                Some(ink)
            );
            assert!(s.pixels().iter().any(|p| p.a > 0 && p.a < 255));
            assert!(before.pixels().iter().all(|p| p.a == 0));
            rounded(&mut s, Rect::new(-20, -20, 28, 28), 12, ink).unwrap();
        }
    }
}

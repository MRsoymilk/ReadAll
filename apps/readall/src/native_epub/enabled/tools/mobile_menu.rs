//! Compact rounded Android menus; same actions, anchors, page cache and input coordinates.
use super::*;
use crate::native_epub::enabled::menu_style;

impl ReaderWindow<'_, '_, '_, '_> {
    /// Explicit controls share geometry between drawing and touch. Labels only select a setting.
    pub(super) fn mobile_setting_buttons(&self, y: i32) -> (Rect, Rect) {
        let p = self.tool_panel();
        let right = p.x + p.width as i32;
        (
            Rect::new(right - 108, y + 3, 40, 32),
            Rect::new(right - 60, y + 3, 40, 32),
        )
    }
    pub(super) fn mobile_zoom_buttons(&self) -> (Rect, Rect) {
        let p = self.tool_panel();
        let right = p.x + p.width as i32;
        (
            Rect::new(right - 136, p.y + 4, 36, 30),
            Rect::new(right - 92, p.y + 4, 36, 30),
        )
    }
    pub(super) fn draw_mobile_tools(&mut self) -> WindowResult<()> {
        let p = self.session.settings().theme.palette();
        if self.tools.mode == Mode::External {
            return self.draw_external_link();
        }
        if self.tools.mode == Mode::None {
            if self.compact_toc() {
                return Ok(());
            }
            if self.toolbar != ToolbarMode::Collapsed {
                for (index, label) in ["查找", "标注", "设置"].into_iter().enumerate() {
                    let r = self.tool_dock(index);
                    self.menu_fill(r, p.button, 9)?;
                    self.menu_label(r, label, 12, p.ink)?;
                }
            }
            if !self.tools.selecting
                && self.tools.selection.is_none()
                && self.tools.drag.is_none()
                && (!self.tools.status.is_empty() || !self.tools.link_history.is_empty())
            {
                let r = Rect::new(8, 34, self.surface.width().saturating_sub(16), 26);
                self.menu_fill(r, p.panel, 10)?;
                let back = self.link_back_rect();
                let has_back = !self.tools.link_history.is_empty();
                if has_back {
                    self.menu_fill(back, p.selected, 9)?;
                    self.menu_label(back, "返回", 12, p.accent)?;
                }
                let mut text = UiPainter::new(&self.ui_font, &mut self.surface)?;
                let hint = text.fit(
                    12,
                    &self.tools.status,
                    r.width.saturating_sub(if has_back { 100 } else { 16 }),
                )?;
                text.draw_clipped(r.x + 8, r.y + 6, 12, &hint, p.muted, r)?;
            }
            return self.draw_selection_actions();
        }
        let panel = self.tool_panel();
        let mode = self.tools.mode;
        self.surface.draw(&[DrawCommand::FillRect {
            rect: Rect::new(
                0,
                32,
                self.surface.width(),
                self.surface.height().saturating_sub(32),
            ),
            color: p.scrim,
        }])?;
        self.menu_fill(panel, p.panel, 20)?;
        let close = Rect::new(panel.x + panel.width as i32 - 40, panel.y + 5, 30, 28);
        self.menu_fill(close, p.button, 10)?;
        svg_icon::draw(
            &mut self.surface,
            menu_style::CLOSE,
            Rect::new(close.x + 6, close.y + 5, 18, 18),
            p.muted,
        )?;
        let title = match mode {
            Mode::Search => "全文搜索",
            Mode::Annotations => "书签与标注",
            Mode::Settings => "阅读设置",
            Mode::Note => "添加笔记",
            Mode::Zoom => "图片查看",
            _ => "",
        };
        {
            let mut text = UiPainter::new(&self.ui_font, &mut self.surface)?;
            let title = text.fit(
                18,
                title,
                panel
                    .width
                    .saturating_sub(if mode == Mode::Zoom { 160 } else { 64 }),
            )?;
            text.draw_clipped(panel.x + 18, panel.y + 12, 18, &title, p.ink, panel)?;
        }
        if mode == Mode::Zoom {
            for (r, label) in [
                (self.mobile_zoom_buttons().0, "−"),
                (self.mobile_zoom_buttons().1, "+"),
            ] {
                self.menu_fill(r, p.button, 9)?;
                self.menu_label(r, label, 18, p.ink)?;
            }
            if let Some(image) = &self.tools.zoom {
                let clip = Rect::new(
                    panel.x + 8,
                    panel.y + 44,
                    panel.width.saturating_sub(16),
                    panel.height.saturating_sub(94),
                );
                let scale = (clip.width as f32 / image.width() as f32)
                    .min(clip.height as f32 / image.height() as f32)
                    * self.tools.factor;
                let w = (image.width() as f32 * scale).round().clamp(1.0, 32768.0) as u32;
                let h = (image.height() as f32 * scale).round().clamp(1.0, 32768.0) as u32;
                let r = Rect::new(
                    clip.x + (clip.width as i32 - w as i32) / 2 + self.tools.pan.0,
                    clip.y + (clip.height as i32 - h as i32) / 2 + self.tools.pan.1,
                    w,
                    h,
                );
                self.surface
                    .draw_rgba(image.pixels(), (image.width(), image.height()), r, clip)?;
            }
        }
        if self.tools.editing() {
            let r = Rect::new(
                panel.x + 12,
                panel.y + 40,
                panel.width.saturating_sub(24),
                38,
            );
            let submit = Rect::new(panel.x + panel.width as i32 - 78, r.y, 66, r.height);
            self.menu_fill(r, p.button, 11)?;
            self.menu_fill(submit, p.accent, 11)?;
            self.menu_label(
                submit,
                if mode == Mode::Search {
                    "搜索"
                } else {
                    "保存"
                },
                14,
                p.on_accent,
            )?;
            let mut text = UiPainter::new(&self.ui_font, &mut self.surface)?;
            let value = if self.tools.query.is_empty() {
                if mode == Mode::Search {
                    "输入关键词".into()
                } else {
                    "写下你的想法".into()
                }
            } else {
                format!("{}▏", self.tools.query.replace('\n', " "))
            };
            let value = text.fit(14, &value, r.width.saturating_sub(84))?;
            text.draw_clipped(
                r.x + 12,
                r.y + 11,
                14,
                &value,
                if self.tools.query.is_empty() {
                    p.muted
                } else {
                    p.ink
                },
                r,
            )?;
        }
        let visible = self.tool_rows();
        let rows: Vec<(usize, String, String)> = match mode {
            Mode::Settings => {
                let s = self.session.settings();
                [
                    ("主题", s.theme.label().to_owned()),
                    ("字号", format!("{} px", s.size)),
                    ("页边距", format!("{} px", s.margin)),
                    ("行距", format!("{:.1} 倍", s.line_spacing)),
                    ("翻页方式", s.page_mode.label().to_owned()),
                ]
                .into_iter()
                .enumerate()
                .skip(self.tools.scroll)
                .take(visible)
                .map(|(i, (name, value))| (i, name.into(), value))
                .collect()
            }
            Mode::Search => self
                .tools
                .hits
                .iter()
                .enumerate()
                .skip(self.tools.scroll)
                .take(visible)
                .map(|(i, hit)| {
                    (
                        i,
                        hit.excerpt.replace('\n', " "),
                        format!("第 {} 章", hit.locator.spine_index() + 1),
                    )
                })
                .collect(),
            Mode::Annotations => self
                .tools
                .annotations
                .iter()
                .enumerate()
                .skip(self.tools.scroll)
                .take(visible)
                .map(|(i, a)| {
                    (
                        i,
                        a.text.replace('\n', " "),
                        format!(
                            "{} · 第 {} 章",
                            match a.kind {
                                Kind::Bookmark => "书签",
                                Kind::Highlight => "高亮",
                                Kind::Note => "笔记",
                            },
                            a.locator.spine_index() + 1
                        ),
                    )
                })
                .collect(),
            _ => Vec::new(),
        };
        for (row, (index, primary, secondary)) in rows.iter().enumerate() {
            let r = Rect::new(
                panel.x + 8,
                panel.y + 88 + row as i32 * 42,
                panel.width.saturating_sub(16),
                38,
            );
            if *index == self.tools.selected {
                self.menu_fill(r, p.selected, 9)?;
            }
            if mode == Mode::Settings {
                for (button, label) in [
                    (self.mobile_setting_buttons(r.y).0, "−"),
                    (self.mobile_setting_buttons(r.y).1, "+"),
                ] {
                    self.menu_fill(button, p.button, 9)?;
                    self.menu_label(button, label, 18, p.ink)?;
                }
            }
            let width = r
                .width
                .saturating_sub(if mode == Mode::Settings { 120 } else { 24 });
            let mut text = UiPainter::new(&self.ui_font, &mut self.surface)?;
            let primary = text.fit(if mode == Mode::Settings { 11 } else { 13 }, primary, width)?;
            let secondary = text.fit(
                if mode == Mode::Settings { 14 } else { 11 },
                secondary,
                width,
            )?;
            text.draw_clipped(
                r.x + 10,
                r.y + 3,
                if mode == Mode::Settings { 11 } else { 13 },
                &primary,
                if mode == Mode::Settings {
                    p.muted
                } else {
                    p.ink
                },
                r,
            )?;
            text.draw_clipped(
                r.x + 10,
                r.y + 20,
                if mode == Mode::Settings { 14 } else { 11 },
                &secondary,
                if mode == Mode::Settings {
                    p.ink
                } else {
                    p.muted
                },
                r,
            )?;
        }
        if rows.is_empty() && matches!(mode, Mode::Search | Mode::Annotations) {
            let hint = if mode == Mode::Annotations {
                "暂无书签或标注"
            } else if self.tools.dirty {
                "输入关键词，搜索书内内容"
            } else {
                "没有找到匹配内容"
            };
            self.menu_label(
                Rect::new(
                    panel.x + 12,
                    panel.y + 106,
                    panel.width.saturating_sub(24),
                    32,
                ),
                hint,
                13,
                p.muted,
            )?;
        }
        let hint = if mode == Mode::Zoom {
            "轻点加减缩放图片"
        } else {
            &self.tools.status
        };
        let mut text = UiPainter::new(&self.ui_font, &mut self.surface)?;
        let hint = text.fit(11, hint, panel.width.saturating_sub(36))?;
        text.draw_clipped(
            panel.x + 18,
            panel.y + panel.height as i32 - 29,
            11,
            &hint,
            p.muted,
            panel,
        )?;
        Ok(())
    }
}

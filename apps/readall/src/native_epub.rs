//! Native EPUB reading entry for the current XHTML text subset.
use std::{ffi::OsString, io::Write};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(not(all(target_os = "linux", feature = "wayland")))]
pub(crate) fn run(_: &[OsString], _: &mut impl Write) -> Result<()> {
    Err("native EPUB window unavailable: on Linux build with --features wayland; Windows/Android windows are not implemented".into())
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod enabled {
    mod async_reader;
    #[cfg(test)]
    mod loading_tests;
    #[cfg(test)]
    mod selection_tests;
    mod tools;
    use super::*;
    use crate::{
        diagnostics::{ResultContext, boxed_stage},
        epub_session::{Action as ReaderAction, EpubSession, Start, TocEntry},
        progress::EpubProgressStore,
        svg_icon::{
            self, CHEVRON_DOWN, CHEVRON_LEFT, CHEVRON_RIGHT, CHEVRON_UP, LIST, MINUS, PLUS,
        },
        text_page::Options,
        ui::{UiFont, UiPainter, builtin_font_bytes},
    };
    use readall_core::read_bounded;
    use readall_epub::{EpubBook, EpubLimits, EpubLocator};
    use readall_font::{Font, FontLimits};
    use readall_platform::{
        LocalFileSource,
        window::{Action, WindowHandler, WindowOptions, WindowResult},
    };
    use readall_render::{Color, DrawCommand, Rect, Surface};
    use std::{
        path::{Path, PathBuf},
        time::Duration,
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ToolbarMode {
        Expanded,
        Collapsed,
        Toc,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ReaderHover {
        None,
        Collapsed,
        Toolbar(usize),
        TocRow(usize),
    }

    struct ReaderWindow<'book, 'archive, 'font, 'font_bytes> {
        session: EpubSession<'book, 'archive, 'font, 'font_bytes>,
        progress: Option<EpubProgressStore>,
        surface: Surface,
        ui_font: UiFont,
        toc: Vec<TocEntry>,
        toc_loaded: bool,
        toc_selected: usize,
        toc_scroll: usize,
        toolbar: ToolbarMode,
        pointer: Option<(i32, i32)>,
        title_scroll: u32,
        title_marquee_span: u32,
        close_requested: bool,
        tools: tools::Tools,
    }

    fn point_in(rect: Rect, x: i32, y: i32) -> bool {
        let right = i64::from(rect.x) + i64::from(rect.width);
        let bottom = i64::from(rect.y) + i64::from(rect.height);
        i64::from(x) >= i64::from(rect.x)
            && i64::from(x) < right
            && i64::from(y) >= i64::from(rect.y)
            && i64::from(y) < bottom
    }

    impl<'book, 'archive, 'font, 'font_bytes> ReaderWindow<'book, 'archive, 'font, 'font_bytes> {
        #[cfg(test)]
        fn new(
            session: EpubSession<'book, 'archive, 'font, 'font_bytes>,
            progress: Option<EpubProgressStore>,
            ui_font: UiFont,
        ) -> WindowResult<Self> {
            let mut reader = Self::new_lazy(session, progress, ui_font)?;
            reader.ensure_toc()?;
            Ok(reader)
        }
        fn ensure_toc(&mut self) -> WindowResult<()> {
            if !self.toc_loaded {
                let entries = self.session.toc_entries()?;
                self.toc = entries;
                self.toc_loaded = true;
                self.sync_toc_selection();
            }
            Ok(())
        }
        fn new_lazy(
            session: EpubSession<'book, 'archive, 'font, 'font_bytes>,
            progress: Option<EpubProgressStore>,
            ui_font: UiFont,
        ) -> WindowResult<Self> {
            let surface = session.frame().surface.clone();
            let toc = Vec::new();
            let mut reader = ReaderWindow {
                session,
                progress,
                surface,
                ui_font,
                toc,
                toc_loaded: false,
                toc_selected: 0,
                toc_scroll: 0,
                toolbar: ToolbarMode::Expanded,
                pointer: None,
                title_scroll: 0,
                title_marquee_span: 0,
                close_requested: false,
                tools: tools::Tools::default(),
            };
            reader.sync_toc_selection();
            reader.refresh_surface()?;
            Ok(reader)
        }

        fn toolbar_rect(&self) -> Rect {
            let width = 520_u32.min(self.surface.width().saturating_sub(24));
            Rect::new(
                ((self.surface.width().saturating_sub(width)) / 2) as i32,
                self.surface.height().saturating_sub(190) as i32,
                width,
                174,
            )
        }

        fn collapsed_rect(&self) -> Rect {
            Rect::new(
                self.surface.width().saturating_sub(88) as i32 / 2,
                self.surface.height().saturating_sub(42) as i32,
                88,
                30,
            )
        }

        fn toolbar_button_rect(&self, index: usize) -> Rect {
            let bar = self.toolbar_rect();
            let content_y = bar.y + 32;
            let content_h = bar.height.saturating_sub(40);
            let side_w = 96_u32.min(bar.width / 4);
            let right_x = bar.x + bar.width as i32 - side_w as i32 - 8;
            let center_left = bar.x + side_w as i32 + 20;
            let center_right = right_x - 12;
            let center_width = (center_right - center_left).max(0) as u32;
            let pair_gap = 8_u32.min(center_width);
            let pair_button = center_width
                .saturating_sub(pair_gap)
                .checked_div(2)
                .unwrap_or(0)
                .min(82);
            let pair_total = pair_button.saturating_mul(2).saturating_add(pair_gap);
            let pair_start = center_left + center_width.saturating_sub(pair_total) as i32 / 2;
            match index {
                0 => Rect::new(bar.x + 8, content_y, side_w, content_h),
                1 => Rect::new(
                    center_left + center_width.saturating_sub(112) as i32 / 2,
                    content_y + 44,
                    112_u32.min(center_width),
                    38,
                ),
                2 => Rect::new(pair_start, content_y + 88, pair_button, 38),
                3 => Rect::new(
                    pair_start + pair_button as i32 + pair_gap as i32,
                    content_y + 88,
                    pair_button,
                    38,
                ),
                4 => Rect::new(right_x, content_y, side_w, content_h),
                5 => Rect::new(bar.x, bar.y, bar.width, 28),
                _ => Rect::new(0, 0, 0, 0),
            }
        }

        fn toc_panel_rect(&self) -> Rect {
            let toolbar = self.toolbar_rect();
            let width = 520_u32.min(self.surface.width().saturating_sub(32));
            let max_height = self.surface.height().saturating_sub(290).min(390);
            let height = max_height.max(160);
            Rect::new(
                ((self.surface.width().saturating_sub(width)) / 2) as i32,
                toolbar.y.saturating_sub(height as i32 + 12).max(44),
                width,
                height,
            )
        }

        fn visible_toc_rows(&self) -> usize {
            ((self.toc_panel_rect().height.saturating_sub(52)) / 38).max(1) as usize
        }

        fn keep_toc_selected_visible(&mut self) {
            let visible = self.visible_toc_rows();
            if self.toc_selected < self.toc_scroll {
                self.toc_scroll = self.toc_selected;
            } else if self.toc_selected >= self.toc_scroll.saturating_add(visible) {
                self.toc_scroll = self.toc_selected + 1 - visible;
            }
        }

        fn hover_target(&self) -> ReaderHover {
            self.pointer
                .map_or(ReaderHover::None, |(x, y)| self.hover_target_at(x, y))
        }

        fn pointer_changed(&mut self, pointer: Option<(i32, i32)>) -> WindowResult<bool> {
            let before = self.hover_target();
            self.pointer = pointer;
            let after = self.hover_target();
            // One active row, controlled by the most recent navigation or pointer
            // event. Hovering never changes the actual reading position.
            let selection_changed = if let ReaderHover::TocRow(index) = after
                && self.toc_selected != index
            {
                self.toc_selected = index;
                true
            } else {
                false
            };
            if before == after && !selection_changed {
                return Ok(false);
            }
            self.refresh_surface()?;
            Ok(true)
        }

        fn refresh_surface(&mut self) -> WindowResult<()> {
            self.surface = self.session.frame().surface.clone();
            self.draw_link_marks()?;
            self.draw_marks()?;
            let width = self.surface.width();
            let height = self.surface.height();
            let (background, theme_ink) = self.session.settings().theme.colors();
            let header = Color {
                a: 245,
                ..background
            };
            let border = Color::rgba(224, 228, 234, 255);
            let ink = theme_ink;
            let muted = Color::rgba(112, 121, 133, 255);
            let accent = Color::rgba(55, 104, 190, 255);
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(0, 0, width, 32),
                    color: header,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(0, 31, width, 1),
                    color: border,
                },
            ])?;
            let (chapter, chapters) = self.session.chapter_position();
            let (page, pages) = self.session.page_position();
            let status = format!(
                "第 {chapter}/{chapters} 章 · 第 {page}/{pages} 页 · {} px",
                self.session.font_size()
            );
            {
                let mut text = UiPainter::new(&self.ui_font, &mut self.surface)?;
                let status_width = text.measure(12, &status)?;
                let status_x = width.saturating_sub(status_width.saturating_add(16)) as i32;
                text.draw(status_x, 9, 12, &status, muted)?;

                let title_clip_end = status_x.saturating_sub(14).max(16);
                let title_clip = Rect::new(16, 4, (title_clip_end - 16).max(0) as u32, 24);
                let title = self.session.book_title();
                let measured = text.measure(14, title)?;
                if title_clip.width == 0 || measured <= title_clip.width {
                    self.title_scroll = 0;
                    self.title_marquee_span = 0;
                    if title_clip.width != 0 {
                        text.draw_clipped(16, 8, 14, title, ink, title_clip)?;
                    }
                } else {
                    let span = measured.saturating_add(48).max(1);
                    self.title_marquee_span = span;
                    self.title_scroll %= span;
                    let first_x = 16_i32.saturating_sub(self.title_scroll as i32);
                    text.draw_clipped(first_x, 8, 14, title, ink, title_clip)?;
                    let second_x = first_x.saturating_add(span as i32);
                    if second_x < title_clip.x.saturating_add(title_clip.width as i32) {
                        text.draw_clipped(second_x, 8, 14, title, ink, title_clip)?;
                    }
                }
            }

            let track_w = width.saturating_sub(32);
            let filled = (track_w as f32 * self.session.overall_progress()).round() as u32;
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(16, height.saturating_sub(3) as i32, track_w, 2),
                    color: Color::rgba(200, 206, 214, 150),
                },
                DrawCommand::FillRect {
                    rect: Rect::new(16, height.saturating_sub(3) as i32, filled.min(track_w), 2),
                    color: accent,
                },
            ])?;

            if self.toolbar == ToolbarMode::Toc {
                self.draw_toc()?;
            }
            match self.toolbar {
                ToolbarMode::Collapsed => self.draw_collapsed_control()?,
                ToolbarMode::Expanded | ToolbarMode::Toc => self.draw_toolbar()?,
            }
            self.draw_tools()?;
            Ok(())
        }

        fn draw_collapsed_control(&mut self) -> WindowResult<()> {
            let rect = self.collapsed_rect();
            let hovered = self.hover_target() == ReaderHover::Collapsed;
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(rect.x + 2, rect.y + 2, rect.width, rect.height),
                    color: Color::rgba(0, 0, 0, 28),
                },
                DrawCommand::FillRect {
                    rect,
                    color: if hovered {
                        Color::rgba(31, 37, 47, 232)
                    } else {
                        Color::rgba(31, 37, 47, 185)
                    },
                },
            ])?;
            svg_icon::draw(
                &mut self.surface,
                CHEVRON_UP,
                Rect::new(rect.x + rect.width as i32 / 2 - 11, rect.y + 4, 22, 22),
                Color::rgba(245, 248, 252, if hovered { 255 } else { 220 }),
            )?;
            Ok(())
        }

        fn draw_toolbar(&mut self) -> WindowResult<()> {
            let rect = self.toolbar_rect();
            let hover = self.hover_target();
            let handle = self.toolbar_button_rect(5);
            let prev = self.toolbar_button_rect(0);
            let toc = self.toolbar_button_rect(1);
            let smaller = self.toolbar_button_rect(2);
            let larger = self.toolbar_button_rect(3);
            let next = self.toolbar_button_rect(4);
            let line = Color::rgba(255, 255, 255, 28);
            let panel = Color::rgba(31, 37, 47, 225);
            let hover_fill = Color::rgba(255, 255, 255, 34);
            let active_fill = Color::rgba(70, 117, 195, 225);
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(rect.x + 3, rect.y + 4, rect.width, rect.height),
                    color: Color::rgba(0, 0, 0, 34),
                },
                DrawCommand::FillRect { rect, color: panel },
                DrawCommand::FillRect {
                    rect: Rect::new(rect.x + 10, rect.y + 28, rect.width.saturating_sub(20), 1),
                    color: line,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(
                        prev.x + prev.width as i32 + 10,
                        prev.y + 43,
                        next.x.saturating_sub(prev.x + prev.width as i32 + 20) as u32,
                        1,
                    ),
                    color: line,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(
                        prev.x + prev.width as i32 + 10,
                        prev.y + 87,
                        next.x.saturating_sub(prev.x + prev.width as i32 + 20) as u32,
                        1,
                    ),
                    color: line,
                },
            ])?;

            for (index, button) in [
                (0, prev),
                (1, toc),
                (2, smaller),
                (3, larger),
                (4, next),
                (5, handle),
            ] {
                let hovered = hover == ReaderHover::Toolbar(index);
                let active = index == 1 && self.toolbar == ToolbarMode::Toc;
                if hovered || active {
                    self.surface.draw(&[DrawCommand::FillRect {
                        rect: if index == 5 {
                            Rect::new(
                                button.x + 8,
                                button.y + 3,
                                button.width.saturating_sub(16),
                                22,
                            )
                        } else {
                            Rect::new(
                                button.x + 3,
                                button.y + 3,
                                button.width.saturating_sub(6),
                                button.height.saturating_sub(6),
                            )
                        },
                        color: if active { active_fill } else { hover_fill },
                    }])?;
                }
            }

            let icon = Color::rgba(244, 247, 252, 235);
            svg_icon::draw(
                &mut self.surface,
                CHEVRON_DOWN,
                Rect::new(rect.x + rect.width as i32 / 2 - 11, rect.y + 3, 22, 22),
                icon,
            )?;
            svg_icon::draw(
                &mut self.surface,
                CHEVRON_LEFT,
                Rect::new(prev.x + prev.width as i32 / 2 - 14, prev.y + 30, 28, 28),
                icon,
            )?;
            svg_icon::draw(
                &mut self.surface,
                CHEVRON_RIGHT,
                Rect::new(next.x + next.width as i32 / 2 - 14, next.y + 30, 28, 28),
                icon,
            )?;
            svg_icon::draw(
                &mut self.surface,
                LIST,
                Rect::new(toc.x + 13, toc.y + 7, 24, 24),
                icon,
            )?;
            svg_icon::draw(
                &mut self.surface,
                MINUS,
                Rect::new(smaller.x + 10, smaller.y + 7, 24, 24),
                icon,
            )?;
            svg_icon::draw(
                &mut self.surface,
                PLUS,
                Rect::new(larger.x + 10, larger.y + 7, 24, 24),
                icon,
            )?;

            let mut text = UiPainter::new(&self.ui_font, &mut self.surface)?;
            let label = Color::rgba(239, 243, 249, 235);
            text.draw(prev.x + 25, prev.y + 74, 13, "上一页", label)?;
            text.draw(next.x + 25, next.y + 74, 13, "下一页", label)?;
            text.draw(toc.x + 45, toc.y + 9, 14, "目录", label)?;
            text.draw(smaller.x + 36, smaller.y + 9, 13, "字体", label)?;
            text.draw(larger.x + 36, larger.y + 9, 13, "字体", label)?;
            Ok(())
        }

        fn draw_toc(&mut self) -> WindowResult<()> {
            let panel = self.toc_panel_rect();
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(panel.x + 3, panel.y + 4, panel.width, panel.height),
                    color: Color::rgba(0, 0, 0, 35),
                },
                DrawCommand::FillRect {
                    rect: panel,
                    color: Color::rgba(250, 251, 253, 246),
                },
                DrawCommand::FillRect {
                    rect: Rect::new(panel.x, panel.y + 46, panel.width, 1),
                    color: Color::rgba(221, 226, 233, 255),
                },
            ])?;
            let visible = self.visible_toc_rows();
            for (row, _entry) in self
                .toc
                .iter()
                .skip(self.toc_scroll)
                .take(visible)
                .enumerate()
            {
                let index = self.toc_scroll + row;
                let y = panel.y + 48 + row as i32 * 38;
                if index == self.toc_selected {
                    self.surface.draw(&[DrawCommand::FillRect {
                        rect: Rect::new(panel.x + 8, y, panel.width.saturating_sub(16), 34),
                        color: Color::rgba(220, 232, 249, 245),
                    }])?;
                }
            }
            let mut text = UiPainter::new(&self.ui_font, &mut self.surface)?;
            text.draw(
                panel.x + 18,
                panel.y + 13,
                17,
                "目录",
                Color::rgba(42, 48, 57, 255),
            )?;
            for (row, entry) in self
                .toc
                .iter()
                .skip(self.toc_scroll)
                .take(visible)
                .enumerate()
            {
                let y = panel.y + 57 + row as i32 * 38;
                let indent = entry.depth.min(6) as i32 * 16;
                let text_width = panel
                    .width
                    .saturating_sub(56_u32.saturating_add(indent as u32));
                let title = text.fit(14, &entry.title, text_width)?;
                text.draw(
                    panel.x + 20 + indent,
                    y,
                    14,
                    &title,
                    Color::rgba(60, 67, 78, 255),
                )?;
            }
            Ok(())
        }

        fn current_toc_index(&self) -> Option<usize> {
            let anchor = self.session.anchor();
            let current_spine = anchor.spine_index();
            let current_offset = usize::try_from(anchor.utf8_offset()).unwrap_or(usize::MAX);
            self.toc
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.spine == current_spine && entry.offset <= current_offset)
                .max_by_key(|(_, entry)| entry.offset)
                .map(|(index, _)| index)
                .or_else(|| {
                    self.toc
                        .iter()
                        .position(|entry| entry.spine == current_spine)
                })
        }

        fn sync_toc_selection(&mut self) {
            if let Some(index) = self.current_toc_index() {
                self.toc_selected = index;
                self.keep_toc_selected_visible();
            }
        }

        fn jump_to_toc(&mut self, index: usize) -> WindowResult<bool> {
            let Some((spine, offset)) =
                self.toc.get(index).map(|entry| (entry.spine, entry.offset))
            else {
                return Ok(false);
            };
            let changed = self.session.jump_to_toc_target(spine, offset)?;
            self.tools.clear_selection();
            self.toc_selected = index;
            self.keep_toc_selected_visible();
            self.toolbar = ToolbarMode::Expanded;
            if changed {
                self.save_progress();
            }
            self.refresh_surface()?;
            Ok(true)
        }

        fn move_toc_selection(&mut self, delta: isize) -> WindowResult<bool> {
            if self.toc.is_empty() {
                return Ok(false);
            }
            self.toc_selected = if delta < 0 {
                self.toc_selected.saturating_sub(delta.unsigned_abs())
            } else {
                self.toc_selected
                    .saturating_add(delta as usize)
                    .min(self.toc.len() - 1)
            };
            self.keep_toc_selected_visible();
            self.refresh_surface()?;
            Ok(true)
        }

        fn perform_reader_action(&mut self, action: ReaderAction) -> WindowResult<bool> {
            match EpubSession::action(&mut self.session, action) {
                Ok(changed) => {
                    if changed {
                        self.tools.clear_selection();
                        self.save_progress();
                        self.sync_toc_selection();
                    }
                    self.refresh_surface()?;
                    Ok(changed)
                }
                Err(error) => {
                    eprintln!("ReadAll: keeping current EPUB page: {error}");
                    self.tools.status = format!("页面未切换：{error}");
                    self.refresh_surface()?;
                    Ok(true)
                }
            }
        }

        fn handle_toolbar_button(&mut self, index: usize) -> WindowResult<bool> {
            self.tools.cancel_gesture();
            match index {
                0 => {
                    self.toolbar = ToolbarMode::Expanded;
                    self.perform_reader_action(ReaderAction::Previous)
                }
                1 => {
                    self.toolbar = if self.toolbar == ToolbarMode::Toc {
                        ToolbarMode::Expanded
                    } else {
                        self.ensure_toc()?;
                        self.sync_toc_selection();
                        ToolbarMode::Toc
                    };
                    self.refresh_surface()?;
                    Ok(true)
                }
                2 => {
                    self.toolbar = ToolbarMode::Expanded;
                    self.perform_reader_action(ReaderAction::Smaller)
                }
                3 => {
                    self.toolbar = ToolbarMode::Expanded;
                    self.perform_reader_action(ReaderAction::Larger)
                }
                4 => {
                    self.toolbar = ToolbarMode::Expanded;
                    self.perform_reader_action(ReaderAction::Next)
                }
                5 => {
                    self.toolbar = ToolbarMode::Collapsed;
                    self.refresh_surface()?;
                    Ok(true)
                }
                _ => Ok(false),
            }
        }

        fn handle_click(&mut self, x: i32, y: i32) -> WindowResult<bool> {
            if self.toolbar == ToolbarMode::Collapsed {
                if point_in(self.collapsed_rect(), x, y) {
                    self.toolbar = ToolbarMode::Expanded;
                    self.refresh_surface()?;
                    return Ok(true);
                }
                return Ok(false);
            }

            if self.toolbar == ToolbarMode::Toc
                && let ReaderHover::TocRow(index) = self.hover_target_at(x, y)
            {
                return self.jump_to_toc(index);
            }
            for index in 0..6 {
                if point_in(self.toolbar_button_rect(index), x, y) {
                    return self.handle_toolbar_button(index);
                }
            }
            if self.toolbar == ToolbarMode::Toc {
                self.toolbar = ToolbarMode::Expanded;
                self.refresh_surface()?;
                return Ok(true);
            }
            // Body clicks are handled by selection/link gestures, never by page side.
            Ok(false)
        }

        fn hover_target_at(&self, x: i32, y: i32) -> ReaderHover {
            match self.toolbar {
                ToolbarMode::Collapsed => {
                    if point_in(self.collapsed_rect(), x, y) {
                        ReaderHover::Collapsed
                    } else {
                        ReaderHover::None
                    }
                }
                ToolbarMode::Expanded | ToolbarMode::Toc => {
                    if self.toolbar == ToolbarMode::Toc {
                        let panel = self.toc_panel_rect();
                        let row_area_y = panel.y + 48;
                        if x >= panel.x
                            && x < panel.x + panel.width as i32
                            && y >= row_area_y
                            && y < panel.y + panel.height as i32
                        {
                            let row = ((y - row_area_y) / 38) as usize;
                            let index = self.toc_scroll.saturating_add(row);
                            let rect = Rect::new(
                                panel.x + 8,
                                row_area_y + row as i32 * 38,
                                panel.width.saturating_sub(16),
                                34,
                            );
                            if row < self.visible_toc_rows()
                                && index < self.toc.len()
                                && point_in(rect, x, y)
                            {
                                return ReaderHover::TocRow(index);
                            }
                        }
                    }
                    for index in 0..6 {
                        if point_in(self.toolbar_button_rect(index), x, y) {
                            return ReaderHover::Toolbar(index);
                        }
                    }
                    ReaderHover::None
                }
            }
        }

        fn save_progress(&self) {
            let Some(store) = &self.progress else {
                return;
            };
            if let Err(error) = store.save(self.session.anchor()) {
                eprintln!("ReadAll: cannot save EPUB reading progress: {error}");
            }
        }
    }

    impl WindowHandler for ReaderWindow<'_, '_, '_, '_> {
        fn resize(&mut self, width: u32, height: u32) -> WindowResult<bool> {
            if (width, height) != (self.surface.width(), self.surface.height()) {
                self.tools.cancel_gesture();
            }
            let changed = EpubSession::resize(&mut self.session, width, height)?;
            if changed {
                self.refresh_surface()?;
            }
            Ok(changed)
        }

        fn text_input_active(&self) -> bool {
            self.tools.editing()
        }
        fn action(&mut self, action: Action) -> WindowResult<bool> {
            match self.tool_action(action) {
                Ok(Some(changed)) => {
                    if changed {
                        self.refresh_surface()?;
                    }
                    return Ok(changed);
                }
                Err(error) => {
                    self.tools.status = error.to_string();
                    self.refresh_surface()?;
                    return Ok(true);
                }
                Ok(None) => {}
            }
            match action {
                Action::PointerMove { x, y } => self.pointer_changed(Some((x, y))),
                Action::PointerLeave => self.pointer_changed(None),
                Action::Click { x, y } => self.handle_click(x, y),
                Action::Activate if self.toolbar == ToolbarMode::Toc => {
                    self.jump_to_toc(self.toc_selected)
                }
                Action::Activate => Ok(false),
                Action::Back if self.toolbar == ToolbarMode::Toc => {
                    self.toolbar = ToolbarMode::Expanded;
                    self.refresh_surface()?;
                    Ok(true)
                }
                Action::Back => {
                    self.close_requested = true;
                    Ok(false)
                }
                Action::Next if self.toolbar == ToolbarMode::Toc => self.move_toc_selection(1),
                Action::Previous if self.toolbar == ToolbarMode::Toc => self.move_toc_selection(-1),
                Action::First if self.toolbar == ToolbarMode::Toc => {
                    if self.toc.is_empty() {
                        return Ok(false);
                    }
                    self.toc_selected = 0;
                    self.keep_toc_selected_visible();
                    self.refresh_surface()?;
                    Ok(true)
                }
                Action::Last if self.toolbar == ToolbarMode::Toc => {
                    if self.toc.is_empty() {
                        return Ok(false);
                    }
                    self.toc_selected = self.toc.len() - 1;
                    self.keep_toc_selected_visible();
                    self.refresh_surface()?;
                    Ok(true)
                }
                Action::Next => self.perform_reader_action(ReaderAction::Next),
                Action::Previous => self.perform_reader_action(ReaderAction::Previous),
                Action::First => self.perform_reader_action(ReaderAction::First),
                Action::Last => self.perform_reader_action(ReaderAction::Last),
                Action::Larger => self.perform_reader_action(ReaderAction::Larger),
                Action::Smaller => self.perform_reader_action(ReaderAction::Smaller),
                Action::Close
                | Action::Text(_)
                | Action::Command(_)
                | Action::PointerRelease { .. } => Ok(false),
            }
        }

        fn surface(&self) -> &Surface {
            &self.surface
        }

        fn title(&self) -> String {
            self.session.title()
        }

        fn animation_interval(&self) -> Option<Duration> {
            (self.title_marquee_span != 0 || self.tools.pending())
                .then_some(Duration::from_millis(40))
        }

        fn animation_tick(&mut self) -> WindowResult<bool> {
            let clipboard_changed = self.poll_clipboard() | self.poll_external_link();
            if self.title_marquee_span == 0 {
                if clipboard_changed {
                    self.refresh_surface()?;
                }
                return Ok(clipboard_changed);
            }
            self.title_scroll = (self.title_scroll + 1) % self.title_marquee_span;
            self.refresh_surface()?;
            Ok(true)
        }

        fn close_requested(&self) -> bool {
            self.close_requested
        }
    }

    pub(super) fn open_path(path: &Path, output: &mut impl Write) -> Result<()> {
        writeln!(output, "使用内置字体: {}", UiFont::builtin_file_name())?;
        let args = vec![
            path.as_os_str().to_owned(),
            OsString::from("--font"),
            OsString::from(UiFont::builtin_label()),
            OsString::from("--missing"),
            OsString::from("replacement"),
        ];
        start(&args, output)
    }

    pub(super) fn start(args: &[OsString], output: &mut impl Write) -> Result<()> {
        if args.is_empty() {
            return Err("open-epub expects <book.epub> --font <font.ttf>".into());
        }
        if !(args.len() - 1).is_multiple_of(2) {
            return Err("each EPUB window option needs a value".into());
        }

        let mut window = WindowOptions::default();
        let mut page_args = Vec::new();
        let mut fallback_paths = Vec::new();
        let mut spine = None;
        let mut locator: Option<EpubLocator> = None;
        let mut state_dir = None;
        let mut data_dir = None;
        let mut progress_enabled = true;
        let mut progress_seen = false;
        for pair in args[1..].chunks_exact(2) {
            match pair[0].to_str() {
                Some("--fallback-font") => {
                    if fallback_paths.len() >= 12 {
                        return Err("at most 12 fallback fonts may be supplied".into());
                    }
                    fallback_paths.push(PathBuf::from(&pair[1]));
                }
                Some("--display") => {
                    if window.display.is_some() {
                        return Err("duplicate --display".into());
                    }
                    window.display = Some(PathBuf::from(&pair[1]));
                }
                Some("--frames") => {
                    if window.close_after_frames.is_some() || pair[1] != "1" {
                        return Err(
                            "--frames supports only 1 for a one-frame protocol smoke check".into(),
                        );
                    }
                    window.close_after_frames = Some(1);
                }
                Some("--spine") => {
                    if spine.is_some() {
                        return Err("duplicate --spine".into());
                    }
                    spine = Some(
                        number(&pair[1])?
                            .checked_sub(1)
                            .ok_or("spine number must be at least 1")?,
                    );
                }
                Some("--at") => {
                    if locator.is_some() {
                        return Err("duplicate --at".into());
                    }
                    locator = Some(pair[1].to_str().ok_or("locator must be UTF-8")?.parse()?);
                }
                Some("--data-dir") => {
                    if data_dir.is_some() {
                        return Err("duplicate --data-dir".into());
                    }
                    data_dir = Some(PathBuf::from(&pair[1]));
                }
                Some("--state-dir") => {
                    if state_dir.is_some() {
                        return Err("duplicate --state-dir".into());
                    }
                    state_dir = Some(PathBuf::from(&pair[1]));
                }
                Some("--progress") => {
                    if progress_seen {
                        return Err("duplicate --progress".into());
                    }
                    progress_seen = true;
                    progress_enabled = match pair[1].to_str() {
                        Some("on") => true,
                        Some("off") => false,
                        _ => return Err("--progress expects on or off".into()),
                    };
                }
                _ => page_args.extend_from_slice(pair),
            }
        }
        if spine.is_some() && locator.is_some() {
            return Err("--spine and EPUB --at cannot be combined".into());
        }
        if !progress_enabled && state_dir.is_some() {
            return Err("--state-dir cannot be used with --progress off".into());
        }

        let options = Options::parse(&page_args)
            .map_err(|error| boxed_stage("parse reader options", error))?;
        if locator.is_some() && options.page.is_some() {
            return Err("EPUB --at and --page cannot be combined".into());
        }

        let epub_path = PathBuf::from(&args[0]);
        let dimensions = (options.width, options.height);
        let (report, log) = async_reader::run(
            epub_path.clone(),
            dimensions,
            window,
            move |bridge| {
                let mut options = options.clone();
                let mut locator = locator.clone();
                let state_dir = state_dir.clone();
                let data_dir = data_dir.clone();
                let mut log = Vec::new();
                let output = &mut log;
                let began = std::time::Instant::now();
                crate::loading::stage("打开文档文件")?;
                let epub_limits = EpubLimits::default();
                let mut source = LocalFileSource::open(&epub_path).epub_stage("open EPUB file")?;
                let epub_bytes = crate::loading::read(
                    &mut source,
                    epub_limits.zip.max_archive_bytes,
                    "读取文档字节",
                )
                .map_err(|error| boxed_stage("read EPUB bytes", error))?;
                crate::loading::stage("校验文档与解析目录结构")?;
                let book = EpubBook::parse(&epub_bytes, epub_limits)
                    .epub_stage("parse EPUB ZIP/container/OPF")?;
                crate::loading::stage("恢复阅读位置")?;
                let explicit_position =
                    spine.is_some() || locator.is_some() || options.page.is_some();
                let progress = if progress_enabled {
                    match state_dir
                        .map(EpubProgressStore::new)
                        .map(Ok)
                        .unwrap_or_else(EpubProgressStore::from_environment)
                    {
                        Ok(store) => Some(store),
                        Err(error) => {
                            writeln!(
                                output,
                                "EPUB reading progress disabled for this session: {error}"
                            )?;
                            None
                        }
                    }
                } else {
                    None
                };
                if !explicit_position && let Some(store) = &progress {
                    match store.load(&book) {
                        Ok(Some(saved)) => {
                            writeln!(output, "Restored EPUB locator: {saved}")?;
                            locator = Some(saved);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            writeln!(
                                output,
                                "Ignoring invalid EPUB reading progress and starting normally: {error}"
                            )?;
                        }
                    }
                }

                crate::loading::stage("准备界面与正文字体")?;
                let font_limits = FontLimits::default();
                let font_bytes = if options.font.as_path() == Path::new(UiFont::builtin_label()) {
                    builtin_font_bytes().to_vec()
                } else {
                    let mut source =
                        LocalFileSource::open(&options.font).epub_stage("open reader font")?;
                    read_bounded(&mut source, font_limits.max_file_bytes)
                        .epub_stage("read reader font")?
                };
                let ui_font =
                    UiFont::from_bytes_face(font_bytes.clone(), options.font.clone(), options.face)
                        .map_err(|error| boxed_stage("prepare UI font", error))?;
                let font = Font::parse(&font_bytes, options.face, font_limits)
                    .epub_stage("parse reader font")?;

                let start = if let Some(locator) = locator {
                    Start::Locator(locator)
                } else if let Some(spine) = spine {
                    Start::Spine(spine)
                } else {
                    Start::Beginning
                };
                let fallback_sources = crate::fonts::load_fallbacks(
                    &fallback_paths,
                    options.font.as_path() == Path::new(UiFont::builtin_label()),
                );
                let fallback_faces: Vec<_> = fallback_sources
                    .iter()
                    .filter_map(|source| {
                        Font::parse(&source.bytes, source.face, FontLimits::default()).ok()
                    })
                    .collect();
                let fallback_refs: Vec<_> = fallback_faces.iter().collect();
                bridge.checkpoint()?;
                (options.width, options.height) = bridge.size();
                crate::loading::stage("读取阅读设置")?;
                let store = data_dir
                    .map(crate::reader_data::Store::new)
                    .map(Ok)
                    .unwrap_or_else(crate::reader_data::Store::from_environment);
                let mut preferences = crate::reader_data::Settings::default();
                let mut setting_error = None;
                if let Ok(store) = &store {
                    match store.settings() {
                        Ok(mut saved) => {
                            if page_args.iter().any(|arg| arg == "--font-size") {
                                saved.size = options.size;
                            }
                            if page_args.iter().any(|arg| arg == "--margin")
                                || saved.margin * 2 >= options.width.min(options.height)
                            {
                                saved.margin = options.margin;
                            }
                            options.size = saved.size;
                            options.margin = saved.margin;
                            preferences = saved;
                        }
                        Err(error) => setting_error = Some(format!("读取设置失败：{error}")),
                    }
                }
                let session = EpubSession::new_with_preferences(
                    &book,
                    &font,
                    options,
                    start,
                    &fallback_refs,
                    preferences,
                )
                .map_err(|error| boxed_stage("restore/select readable EPUB chapter", error))?;
                crate::loading::stage("准备阅读界面与标注")?;
                writeln!(output, "Fallback font faces: {}", fallback_refs.len())?;
                let mut reader = ReaderWindow::new_lazy(session, progress, ui_font)
                    .map_err(|error| boxed_stage("build EPUB reader UI", error))?;
                bridge.preview(&reader);
                crate::loading::stage("载入标注与阅读工具")?;
                match store {
                    Ok(store) => {
                        match store.annotations(reader.session.book()) {
                            Ok(rows) => reader.tools.annotations = rows,
                            Err(error) => reader.tools.status = format!("读取标注失败：{error}"),
                        }
                        reader.tools.store = Some(store);
                    }
                    Err(error) => reader.tools.status = format!("阅读设置存储不可用：{error}"),
                }
                if let Some(error) = setting_error {
                    reader.tools.status = error;
                }
                reader.refresh_surface()?;

                writeln!(
                    output,
                    "Native Wayland EPUB reader (CSS text/block subset + PNG/JPEG/WebP/SVG)\nKeys: PageUp/PageDown, arrows, Space, Home/End, +/-; Esc dismisses an open panel before closing the reader.\nPage navigation crosses linear spine boundaries. Progress supports epub-v1/v2 and whitespace-aware epub-v3 locators; legacy code positions are migrated when storage is available.\nCode blocks preserve source line breaks, indentation, tabs and blank lines; long lines soft-wrap for the viewport. Automatic bounded syntax colors support C/C++, Rust, Python, Shell, JavaScript/TypeScript and JSON; existing multicolor author code is preserved. PNG includes Adam7. Images decode on demand with a bounded LRU cache; animated WebP shows its first frame. EPUB text uses shaping, bidi, grapheme-safe wrapping and bounded font fallback. Static TrueType @font-face resources are selected by chapter-local font-family lists.\nF2 search; F3 annotations; F4 bookmark; F5 settings; F6 theme; F7 note; F8 highlight; F9 text selection priority; Ctrl+C/Ctrl+V clipboard. Drag body text to select without F9; blank clicks do not turn pages. Link/image clicks activate on release, not while dragging. Click unlinked images to inspect them. Book-local body links and footnotes are clickable (id and legacy name anchors); Backspace or the return button restores the previous reading position. HTTP/HTTPS links show their target for confirmation, then open in the default browser; Esc cancels without leaving the reader.\nFull CSS, WOFF/WOFF2, CFF/variable/obfuscated fonts, MathML, PDF and Windows/Android windows remain unimplemented."
                )?;
                output.flush()?;

                eprintln!(
                    "ReadAll: 页面准备完成，用时 {:.3}s；目录按需生成",
                    began.elapsed().as_secs_f64()
                );
                bridge
                    .serve(&mut reader)
                    .map_err(|error| boxed_stage("reader worker", error))?;
                reader.save_progress();
                writeln!(output, "EPUB locator: {}", reader.session.anchor())?;
                Ok(log)
            },
        )?;
        output.write_all(&log)?;
        writeln!(
            output,
            "Closed. Buffer commits: {}; last size: {}x{}",
            report.committed_frames, report.width, report.height
        )?;
        Ok(())
    }

    fn number(value: &OsString) -> Result<usize> {
        let text = value.to_str().ok_or("expected an unsigned integer")?;
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("expected an unsigned integer".into());
        }
        Ok(text.parse()?)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{test_epub, test_font};

        fn options() -> Options {
            Options::parse(
                &[
                    "--font",
                    "fixture.ttf",
                    "--width",
                    "420",
                    "--height",
                    "520",
                    "--margin",
                    "20",
                ]
                .map(Into::into),
            )
            .unwrap()
        }

        #[test]
        fn toolbar_collapses_to_chevron_and_reopens_on_click() {
            let epub_bytes = test_epub::make_epub();
            let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
            let font_bytes = test_font::make_font();
            let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
            let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
            let ui_font =
                UiFont::from_bytes(font_bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
            let mut reader = ReaderWindow::new(session, None, ui_font).unwrap();

            assert_eq!(reader.toolbar, ToolbarMode::Expanded);
            assert!(reader.handle_toolbar_button(5).unwrap());
            assert_eq!(reader.toolbar, ToolbarMode::Collapsed);
            let rect = reader.collapsed_rect();
            assert!(
                reader
                    .handle_click(
                        rect.x + rect.width as i32 / 2,
                        rect.y + rect.height as i32 / 2,
                    )
                    .unwrap()
            );
            assert_eq!(reader.toolbar, ToolbarMode::Expanded);
        }

        #[test]
        fn toolbar_uses_three_row_layout_with_full_height_page_controls() {
            let epub_bytes = test_epub::make_epub();
            let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
            let font_bytes = test_font::make_font();
            let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
            let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
            let ui_font =
                UiFont::from_bytes(font_bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
            let reader = ReaderWindow::new(session, None, ui_font).unwrap();

            let bar = reader.toolbar_rect();
            let previous = reader.toolbar_button_rect(0);
            let toc = reader.toolbar_button_rect(1);
            let smaller = reader.toolbar_button_rect(2);
            let larger = reader.toolbar_button_rect(3);
            let next = reader.toolbar_button_rect(4);
            let toggle = reader.toolbar_button_rect(5);
            assert_eq!(toggle.x, bar.x);
            assert_eq!(toggle.width, bar.width);
            assert_eq!(previous.y, next.y);
            assert_eq!(previous.height, next.height);
            assert!(previous.height > toc.height * 2);
            assert!(toc.y < smaller.y);
            assert_eq!(smaller.y, larger.y);
            assert!(smaller.x + smaller.width as i32 <= larger.x);
            assert!(previous.x + (previous.width as i32) < smaller.x);
            assert!(larger.x + (larger.width as i32) < next.x);
        }

        #[test]
        fn long_title_enables_marquee_and_animation_ticks_move_it() {
            let epub_bytes = test_epub::make_epub_with_long_title();
            let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
            let font_bytes = test_font::make_font();
            let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
            let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
            let ui_font =
                UiFont::from_bytes(font_bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
            let mut reader = ReaderWindow::new(session, None, ui_font).unwrap();

            assert!(reader.title_marquee_span > 0);
            assert_eq!(reader.animation_interval(), Some(Duration::from_millis(40)));
            let before = reader.title_scroll;
            assert!(reader.animation_tick().unwrap());
            assert_eq!(
                reader.title_scroll,
                (before + 1) % reader.title_marquee_span
            );
        }

        #[test]
        fn toc_popup_lists_chapters_and_click_jumps_to_selected_chapter() {
            let epub_bytes = test_epub::make_epub();
            let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
            let font_bytes = test_font::make_font();
            let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
            let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
            let ui_font =
                UiFont::from_bytes(font_bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
            let mut reader = ReaderWindow::new(session, None, ui_font).unwrap();

            assert_eq!(reader.toc.len(), 2);
            assert!(reader.handle_toolbar_button(1).unwrap());
            assert_eq!(reader.toolbar, ToolbarMode::Toc);
            let panel = reader.toc_panel_rect();
            assert!(
                reader
                    .handle_click(panel.x + 30, panel.y + 48 + 38 + 12)
                    .unwrap()
            );
            assert_eq!(reader.session.anchor().spine_index(), 1);
            assert_eq!(reader.toolbar, ToolbarMode::Expanded);
        }

        #[test]
        fn toc_fragment_jump_selects_the_exact_entry_within_one_spine() {
            let epub_bytes = test_epub::make_epub_with_navigation();
            let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
            let font_bytes = test_font::make_font();
            let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
            let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
            let ui_font =
                UiFont::from_bytes(font_bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
            let mut reader = ReaderWindow::new(session, None, ui_font).unwrap();

            assert_eq!(reader.toc.len(), 3);
            assert_eq!(reader.current_toc_index(), Some(0));
            let target_offset = reader.toc[1].offset;
            assert!(target_offset > 0);
            assert!(reader.jump_to_toc(1).unwrap());
            assert_eq!(reader.session.anchor().spine_index(), 0);
            assert_eq!(reader.session.anchor().utf8_offset(), target_offset as u64);
            assert_eq!(reader.current_toc_index(), Some(1));
            assert_eq!(reader.toc_selected, 1);
        }

        #[test]
        fn toc_keyboard_navigation_moves_selection_without_turning_page() {
            let epub_bytes = test_epub::make_epub();
            let book = EpubBook::parse(&epub_bytes, EpubLimits::default()).unwrap();
            let font_bytes = test_font::make_font();
            let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
            let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
            let ui_font =
                UiFont::from_bytes(font_bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
            let mut reader = ReaderWindow::new(session, None, ui_font).unwrap();

            reader.handle_toolbar_button(1).unwrap();
            assert_eq!(reader.toc_selected, 0);
            assert!(reader.action(Action::Next).unwrap());
            assert_eq!(reader.toc_selected, 1);
            assert_eq!(reader.session.anchor().spine_index(), 0);
            assert!(reader.action(Action::Activate).unwrap());
            assert_eq!(reader.session.anchor().spine_index(), 1);
        }
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) fn run(args: &[OsString], output: &mut impl Write) -> Result<()> {
    let result = enabled::start(args, output);
    if let Err(error) = &result
        && let Some(path) = args.first().map(std::path::PathBuf::from)
    {
        match crate::diagnostics::log_epub_failure(&path, error.as_ref()) {
            Ok(log) => {
                let _ = writeln!(output, "错误日志: {}", log.display());
            }
            Err(log_error) => {
                let _ = writeln!(
                    output,
                    "错误日志写入失败: {log_error}; 目标路径: {}",
                    crate::diagnostics::diagnostic_path().display()
                );
            }
        }
    }
    result
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) fn open_path(path: &std::path::Path, output: &mut impl Write) -> Result<()> {
    enabled::open_path(path, output)
}

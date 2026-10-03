//! Native EPUB reading entry for the current XHTML text subset.
use std::{ffi::OsString, io::Write};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(not(all(target_os = "linux", feature = "wayland")))]
pub(crate) fn run(_: &[OsString], _: &mut impl Write) -> Result<()> {
    Err("native EPUB window unavailable: on Linux build with --features wayland; Windows/Android windows are not implemented".into())
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod enabled {
    use super::*;
    use crate::{
        epub_session::{Action as ReaderAction, EpubSession, Start, TocEntry},
        progress::EpubProgressStore,
        text_page::Options,
        ui::{UiFont, UiPainter, builtin_font_bytes},
    };
    use readall_core::read_bounded;
    use readall_epub::{EpubBook, EpubLimits, EpubLocator};
    use readall_font::{Font, FontLimits};
    use readall_platform::{
        LocalFileSource,
        window::{self, Action, WindowHandler, WindowOptions, WindowReport, WindowResult},
    };
    use readall_render::{Color, DrawCommand, Rect, Surface};
    use std::path::{Path, PathBuf};

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
        toc_selected: usize,
        toc_scroll: usize,
        toolbar: ToolbarMode,
        pointer: Option<(i32, i32)>,
        close_requested: bool,
    }

    fn point_in(rect: Rect, x: i32, y: i32) -> bool {
        let right = i64::from(rect.x) + i64::from(rect.width);
        let bottom = i64::from(rect.y) + i64::from(rect.height);
        i64::from(x) >= i64::from(rect.x)
            && i64::from(x) < right
            && i64::from(y) >= i64::from(rect.y)
            && i64::from(y) < bottom
    }

    fn draw_down_chevron(surface: &mut Surface, x: i32, y: i32, color: Color) -> WindowResult<()> {
        surface.draw(&[
            DrawCommand::FillRect {
                rect: Rect::new(x, y, 4, 4),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x + 4, y + 4, 4, 4),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x + 8, y + 8, 4, 4),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x + 12, y + 8, 4, 4),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x + 16, y + 4, 4, 4),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x + 20, y, 4, 4),
                color,
            },
        ])?;
        Ok(())
    }

    fn draw_left_chevron(surface: &mut Surface, x: i32, y: i32, color: Color) -> WindowResult<()> {
        surface.draw(&[
            DrawCommand::FillRect {
                rect: Rect::new(x + 8, y, 4, 4),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x + 4, y + 4, 4, 4),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x, y + 8, 4, 8),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x + 4, y + 16, 4, 4),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x + 8, y + 20, 4, 4),
                color,
            },
        ])?;
        Ok(())
    }

    fn draw_right_chevron(surface: &mut Surface, x: i32, y: i32, color: Color) -> WindowResult<()> {
        surface.draw(&[
            DrawCommand::FillRect {
                rect: Rect::new(x, y, 4, 4),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x + 4, y + 4, 4, 4),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x + 8, y + 8, 4, 8),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x + 4, y + 16, 4, 4),
                color,
            },
            DrawCommand::FillRect {
                rect: Rect::new(x, y + 20, 4, 4),
                color,
            },
        ])?;
        Ok(())
    }

    fn draw_toolbar_icon(
        surface: &mut Surface,
        index: usize,
        rect: Rect,
        color: Color,
    ) -> WindowResult<()> {
        let cx = rect.x + rect.width as i32 / 2;
        let cy = rect.y + rect.height as i32 / 2;
        match index {
            0 => draw_left_chevron(surface, cx - 6, cy - 12, color)?,
            1 => {
                surface.draw(&[
                    DrawCommand::FillRect {
                        rect: Rect::new(cx - 10, cy - 10, 4, 4),
                        color,
                    },
                    DrawCommand::FillRect {
                        rect: Rect::new(cx - 3, cy - 10, 15, 3),
                        color,
                    },
                    DrawCommand::FillRect {
                        rect: Rect::new(cx - 10, cy - 2, 4, 4),
                        color,
                    },
                    DrawCommand::FillRect {
                        rect: Rect::new(cx - 3, cy - 2, 15, 3),
                        color,
                    },
                    DrawCommand::FillRect {
                        rect: Rect::new(cx - 10, cy + 6, 4, 4),
                        color,
                    },
                    DrawCommand::FillRect {
                        rect: Rect::new(cx - 3, cy + 6, 15, 3),
                        color,
                    },
                ])?;
            }
            2 => surface.draw(&[DrawCommand::FillRect {
                rect: Rect::new(cx - 10, cy - 2, 20, 4),
                color,
            }])?,
            3 => surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(cx - 10, cy - 2, 20, 4),
                    color,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(cx - 2, cy - 10, 4, 20),
                    color,
                },
            ])?,
            4 => draw_right_chevron(surface, cx - 6, cy - 12, color)?,
            5 => draw_down_chevron(surface, cx - 12, cy - 6, color)?,
            _ => {}
        }
        Ok(())
    }

    impl<'book, 'archive, 'font, 'font_bytes> ReaderWindow<'book, 'archive, 'font, 'font_bytes> {
        fn new(
            session: EpubSession<'book, 'archive, 'font, 'font_bytes>,
            progress: Option<EpubProgressStore>,
            ui_font: UiFont,
        ) -> WindowResult<Self> {
            let surface = session.frame().surface.clone();
            let toc = session.toc_entries()?;
            let toc_selected = toc.iter().position(|entry| entry.current).unwrap_or(0);
            let mut reader = ReaderWindow {
                session,
                progress,
                surface,
                ui_font,
                toc,
                toc_selected,
                toc_scroll: 0,
                toolbar: ToolbarMode::Expanded,
                pointer: None,
                close_requested: false,
            };
            reader.keep_toc_selected_visible();
            reader.refresh_surface()?;
            Ok(reader)
        }

        fn toolbar_rect(&self) -> Rect {
            let width = 324_u32.min(self.surface.width().saturating_sub(24));
            Rect::new(
                ((self.surface.width().saturating_sub(width)) / 2) as i32,
                self.surface.height().saturating_sub(78) as i32,
                width,
                54,
            )
        }

        fn collapsed_rect(&self) -> Rect {
            Rect::new(
                self.surface.width().saturating_sub(56) as i32 / 2,
                self.surface.height().saturating_sub(38) as i32,
                56,
                28,
            )
        }

        fn toolbar_button_rect(&self, index: usize) -> Rect {
            let bar = self.toolbar_rect();
            let button = 48_i32;
            let total = 6 * button;
            let start = bar.x + ((bar.width as i32 - total) / 2);
            Rect::new(start + index as i32 * button, bar.y + 3, 48, 48)
        }

        fn toc_panel_rect(&self) -> Rect {
            let toolbar = self.toolbar_rect();
            let width = 520_u32.min(self.surface.width().saturating_sub(32));
            let max_height = self.surface.height().saturating_sub(170).min(390);
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
            if before == after {
                return Ok(false);
            }
            self.refresh_surface()?;
            Ok(true)
        }

        fn refresh_surface(&mut self) -> WindowResult<()> {
            self.surface = self.session.frame().surface.clone();
            let width = self.surface.width();
            let height = self.surface.height();
            let header = Color::rgba(248, 249, 251, 235);
            let border = Color::rgba(224, 228, 234, 255);
            let ink = Color::rgba(48, 54, 64, 255);
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
                let title = text.fit(14, self.session.book_title(), width.saturating_sub(320))?;
                text.draw(16, 8, 14, &title, ink)?;
                let status_width = text.measure(12, &status)?;
                let status_x = width.saturating_sub(status_width.saturating_add(16)) as i32;
                text.draw(status_x, 9, 12, &status, muted)?;
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
            Ok(())
        }

        fn draw_collapsed_control(&mut self) -> WindowResult<()> {
            let rect = self.collapsed_rect();
            let hovered = self.hover_target() == ReaderHover::Collapsed;
            let color = if hovered {
                Color::rgba(50, 58, 70, 210)
            } else {
                Color::rgba(70, 78, 90, 105)
            };
            draw_down_chevron(&mut self.surface, rect.x + 18, rect.y + 9, color)?;
            Ok(())
        }

        fn draw_toolbar(&mut self) -> WindowResult<()> {
            let rect = self.toolbar_rect();
            let hover = self.hover_target();
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(rect.x + 2, rect.y + 3, rect.width, rect.height),
                    color: Color::rgba(0, 0, 0, 38),
                },
                DrawCommand::FillRect {
                    rect,
                    color: Color::rgba(31, 37, 47, 225),
                },
            ])?;
            for index in 0..6 {
                let button = self.toolbar_button_rect(index);
                let hovered = hover == ReaderHover::Toolbar(index);
                let active = index == 1 && self.toolbar == ToolbarMode::Toc;
                if hovered || active {
                    self.surface.draw(&[DrawCommand::FillRect {
                        rect: Rect::new(button.x + 4, button.y + 4, 40, 40),
                        color: if active {
                            Color::rgba(70, 117, 195, 230)
                        } else {
                            Color::rgba(255, 255, 255, 36)
                        },
                    }])?;
                }
                let color = Color::rgba(240, 244, 250, if hovered { 255 } else { 220 });
                draw_toolbar_icon(&mut self.surface, index, button, color)?;
            }
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
            let hover = self.hover_target();
            let current_spine = self.session.anchor().spine_index();
            let visible = self.visible_toc_rows();
            for (row, entry) in self
                .toc
                .iter()
                .skip(self.toc_scroll)
                .take(visible)
                .enumerate()
            {
                let index = self.toc_scroll + row;
                let y = panel.y + 48 + row as i32 * 38;
                let selected = index == self.toc_selected;
                let current = entry.spine == current_spine;
                let hovered = hover == ReaderHover::TocRow(index);
                if selected || current || hovered {
                    self.surface.draw(&[DrawCommand::FillRect {
                        rect: Rect::new(panel.x + 8, y, panel.width.saturating_sub(16), 34),
                        color: if hovered {
                            Color::rgba(220, 232, 249, 245)
                        } else if current {
                            Color::rgba(231, 238, 251, 245)
                        } else {
                            Color::rgba(238, 241, 246, 245)
                        },
                    }])?;
                }
            }
            let text_width = panel.width.saturating_sub(56);
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
                let title = text.fit(14, &entry.title, text_width)?;
                text.draw(panel.x + 20, y, 14, &title, Color::rgba(60, 67, 78, 255))?;
            }
            Ok(())
        }

        fn sync_toc_selection(&mut self) {
            let current = self.session.anchor().spine_index();
            if let Some(index) = self.toc.iter().position(|entry| entry.spine == current) {
                self.toc_selected = index;
                self.keep_toc_selected_visible();
            }
        }

        fn jump_to_toc(&mut self, index: usize) -> WindowResult<bool> {
            let Some(spine) = self.toc.get(index).map(|entry| entry.spine) else {
                return Ok(false);
            };
            let changed = self.session.jump_to_spine(spine)?;
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
                        self.save_progress();
                        self.sync_toc_selection();
                    }
                    self.refresh_surface()?;
                    Ok(changed)
                }
                Err(error) => {
                    eprintln!("ReadAll: keeping current EPUB page: {error}");
                    Ok(false)
                }
            }
        }

        fn handle_toolbar_button(&mut self, index: usize) -> WindowResult<bool> {
            match index {
                0 => {
                    self.toolbar = ToolbarMode::Expanded;
                    self.perform_reader_action(ReaderAction::Previous)
                }
                1 => {
                    self.toolbar = if self.toolbar == ToolbarMode::Toc {
                        ToolbarMode::Expanded
                    } else {
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
                return self.perform_reader_action(if x < self.surface.width() as i32 / 2 {
                    ReaderAction::Previous
                } else {
                    ReaderAction::Next
                });
            }

            if self.toolbar == ToolbarMode::Toc {
                if let ReaderHover::TocRow(index) = self.hover_target_at(x, y) {
                    return self.jump_to_toc(index);
                }
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
            self.perform_reader_action(if x < self.surface.width() as i32 / 2 {
                ReaderAction::Previous
            } else {
                ReaderAction::Next
            })
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
                            if row < self.visible_toc_rows() && index < self.toc.len() {
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
            let changed = EpubSession::resize(&mut self.session, width, height)?;
            if changed {
                self.refresh_surface()?;
            }
            Ok(changed)
        }

        fn action(&mut self, action: Action) -> WindowResult<bool> {
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
                Action::Close => Ok(false),
            }
        }

        fn surface(&self) -> &Surface {
            &self.surface
        }

        fn title(&self) -> String {
            self.session.title()
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
        if (args.len() - 1) % 2 != 0 {
            return Err("each EPUB window option needs a value".into());
        }

        let mut window = WindowOptions::default();
        let mut page_args = Vec::new();
        let mut spine = None;
        let mut locator: Option<EpubLocator> = None;
        let mut state_dir = None;
        let mut progress_enabled = true;
        let mut progress_seen = false;
        for pair in args[1..].chunks_exact(2) {
            match pair[0].to_str() {
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

        let options = Options::parse(&page_args)?;
        if locator.is_some() && options.page.is_some() {
            return Err("EPUB --at and --page cannot be combined".into());
        }

        let epub_limits = EpubLimits::default();
        let epub_bytes = read_bounded(
            &mut LocalFileSource::open(PathBuf::from(&args[0]))?,
            epub_limits.zip.max_archive_bytes,
        )?;
        let book = EpubBook::parse(&epub_bytes, epub_limits)?;
        let explicit_position = spine.is_some() || locator.is_some() || options.page.is_some();
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
        if !explicit_position {
            if let Some(store) = &progress {
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
        }

        let font_limits = FontLimits::default();
        let font_bytes = if options.font == PathBuf::from(UiFont::builtin_label()) {
            builtin_font_bytes().to_vec()
        } else {
            read_bounded(
                &mut LocalFileSource::open(&options.font)?,
                font_limits.max_file_bytes,
            )?
        };
        let ui_font =
            UiFont::from_bytes_face(font_bytes.clone(), options.font.clone(), options.face)?;
        let font = Font::parse(&font_bytes, options.face, font_limits)?;

        let start = if let Some(locator) = locator {
            Start::Locator(locator)
        } else if let Some(spine) = spine {
            Start::Spine(spine)
        } else {
            Start::Beginning
        };
        let session = EpubSession::new(&book, &font, options, start)?;
        let mut reader = ReaderWindow::new(session, progress, ui_font)?;

        writeln!(
            output,
            "Native Wayland EPUB reader (XHTML text subset)\nKeys: PageUp/PageDown, arrows, Space, Home/End, +/-; Esc closes.\nPage navigation crosses linear spine boundaries. Reading position is saved with epub-v1 locator when progress storage is available.\nCSS, images, shaping and font fallback are not rendered yet."
        )?;
        output.flush()?;

        let report: WindowReport = window::run(&mut reader, window)?;
        reader.save_progress();
        writeln!(
            output,
            "Closed. Buffer commits: {}; last size: {}x{}\nEPUB locator: {}",
            report.committed_frames,
            report.width,
            report.height,
            reader.session.anchor()
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
    enabled::start(args, output)
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) fn open_path(path: &std::path::Path, output: &mut impl Write) -> Result<()> {
    enabled::open_path(path, output)
}

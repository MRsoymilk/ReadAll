//! Recent-history actions and clipped, time-based filename scrolling.
//! Nothing in this UI deletes a book, annotation, or reading-position file.
use super::*;
use std::time::{Duration, Instant};

const REMOVE_WIDTH: u32 = 48;
const REMOVE_INK: Color = Color::rgba(167, 55, 58, 255);
const REMOVE_HOVER: Color = Color::rgba(253, 235, 234, 255);
const REMOVE_PRESSED: Color = Color::rgba(245, 213, 212, 255);
const SCROLL_SPEED: f64 = 28.0;
const END_PAUSE: f64 = 1.0;

pub(super) struct RecentRowView {
    pub(super) path: PathBuf,
    pub(super) name: String,
    pub(super) clip: Rect,
    pub(super) overflow: u32,
    pub(super) started: Instant,
    pub(super) offset: u32,
}

/// Ping-pong at a fixed logical-pixel speed, pausing at both ends. This never
/// slices UTF-8: clipping is applied to the pixels, not the filename string.
pub(super) fn marquee_offset(overflow: u32, elapsed: Duration) -> u32 {
    if overflow == 0 {
        return 0;
    }
    let travel = f64::from(overflow) / SCROLL_SPEED;
    let phase = elapsed.as_secs_f64() % (2.0 * (END_PAUSE + travel));
    let distance = if phase < END_PAUSE {
        0.0
    } else if phase < END_PAUSE + travel {
        (phase - END_PAUSE) * SCROLL_SPEED
    } else if phase < 2.0 * END_PAUSE + travel {
        f64::from(overflow)
    } else {
        f64::from(overflow) - (phase - 2.0 * END_PAUSE - travel) * SCROLL_SPEED
    };
    distance.round().clamp(0.0, f64::from(overflow)) as u32
}

impl Home<'_> {
    pub(super) fn visible_recent_count(&self) -> usize {
        // Keep every visible row/button above the status line, including at the
        // minimum supported window height. Never leave invisible click targets.
        let space = (self.surface.height() as i32 - 54 - RECENT_TOP + 4).max(0) as usize;
        (space / RECENT_ROW_HEIGHT as usize)
            .min(RECENT_VISIBLE)
            .min(self.recent.len())
    }

    pub(super) fn recent_delete_rect(&self, index: usize) -> Rect {
        let row = self.recent_row_rect(index);
        Rect::new(
            row.x + row.width as i32 - REMOVE_WIDTH as i32 - 4,
            row.y + 2,
            REMOVE_WIDTH,
            row.height.saturating_sub(4),
        )
    }

    fn recent_name_clip(&self, index: usize) -> Rect {
        let row = self.recent_row_rect(index);
        let button = self.recent_delete_rect(index);
        let left = row.x + 16;
        Rect::new(
            left,
            row.y + 2,
            (button.x - 8 - left).max(0) as u32,
            row.height.saturating_sub(4),
        )
    }

    fn refresh_recent_rows(&mut self, now: Instant) -> WindowResult<()> {
        let count = self.visible_recent_count();
        self.recent_rows.truncate(count);
        for index in 0..count {
            let path = &self.recent[index];
            let clip = self.recent_name_clip(index);
            if self
                .recent_rows
                .get(index)
                .is_some_and(|row| row.path == *path && row.clip == clip)
            {
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned();
            let width = UiPainter::new(self.ui_font, &mut self.surface)?.measure(14, &name)?;
            let row = RecentRowView {
                path: path.clone(),
                name,
                clip,
                overflow: width.saturating_sub(clip.width),
                started: now,
                offset: 0,
            };
            if let Some(slot) = self.recent_rows.get_mut(index) {
                *slot = row;
            } else {
                self.recent_rows.push(row);
            }
        }
        Ok(())
    }

    pub(super) fn paint_recent_rows(&mut self, now: Instant) -> WindowResult<()> {
        self.refresh_recent_rows(now)?;
        for index in 0..self.recent_rows.len() {
            self.paint_recent_row(index)?;
        }
        Ok(())
    }

    fn paint_recent_row(&mut self, index: usize) -> WindowResult<()> {
        let row = self.recent_row_rect(index);
        let button = self.recent_delete_rect(index);
        let hover = self.hover_target();
        let hovered =
            matches!(hover, HoverTarget::RecentRow(i) | HoverTarget::RecentDelete(i) if i == index);
        let delete_hover = hover == HoverTarget::RecentDelete(index);
        let view = &self.recent_rows[index];
        let pressed = delete_hover && self.pending_recent_delete.as_ref() == Some(&view.path);
        self.surface.draw(&[
            DrawCommand::FillRect {
                rect: row,
                color: if hovered { HOVER_SOFT } else { PANEL },
            },
            DrawCommand::FillRect {
                rect: Rect::new(row.x, row.y, if hovered { 4 } else { 2 }, row.height),
                color: if hovered { ACCENT } else { BORDER },
            },
            DrawCommand::FillRect {
                rect: button,
                color: if delete_hover { REMOVE_INK } else { BORDER },
            },
            DrawCommand::FillRect {
                rect: Rect::new(
                    button.x + 1,
                    button.y + 1,
                    button.width - 2,
                    button.height - 2,
                ),
                color: if pressed {
                    REMOVE_PRESSED
                } else if delete_hover {
                    REMOVE_HOVER
                } else {
                    PANEL
                },
            },
        ])?;
        let mut text = UiPainter::new(self.ui_font, &mut self.surface)?;
        text.draw_clipped(
            view.clip.x.saturating_sub(view.offset as i32),
            row.y + 7,
            14,
            &view.name,
            INK,
            view.clip,
        )?;
        let width = text.measure(12, "删除")?;
        text.draw_clipped(
            button.x + (button.width.saturating_sub(width) / 2) as i32,
            button.y + 6,
            12,
            "删除",
            if delete_hover { REMOVE_INK } else { MUTED },
            button,
        )?;
        Ok(())
    }

    pub(super) fn recent_animation_interval(&self) -> Option<Duration> {
        (matches!(self.mode, Mode::Library)
            && !self.close_requested
            && self.recent_rows.iter().any(|row| row.overflow > 0))
        .then_some(Duration::from_millis(33))
    }

    pub(super) fn tick_recent_rows(&mut self, now: Instant) -> WindowResult<bool> {
        if self.recent_animation_interval().is_none() {
            return Ok(false);
        }
        let mut changed = false;
        for index in 0..self.recent_rows.len() {
            let row = &mut self.recent_rows[index];
            let offset = marquee_offset(row.overflow, now.saturating_duration_since(row.started));
            if offset == row.offset {
                continue;
            }
            row.offset = offset;
            // Repaint only the changed row; no metadata I/O, measurement, parsing
            // or whole-window repaint is performed for an animation frame.
            self.paint_recent_row(index)?;
            changed = true;
        }
        Ok(changed)
    }

    pub(super) fn release_recent_delete(&mut self, x: i32, y: i32) -> WindowResult<bool> {
        let pressed = self.pending_recent_delete.take();
        self.pointer = Some((x, y));
        if let (Some(path), HoverTarget::RecentDelete(index)) = (&pressed, self.hover_target())
            && self.recent.get(index) == Some(path)
        {
            return self.remove_recent(index);
        }
        if pressed.is_some() {
            self.paint_recent_rows(Instant::now())?;
            return Ok(true);
        }
        Ok(false)
    }

    fn remove_recent(&mut self, index: usize) -> WindowResult<bool> {
        let Some(path) = self.recent.get(index).cloned() else {
            return Ok(false);
        };
        let result = self
            .recent_store
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "最近阅读存储不可用"))
            .and_then(|store| store.remove(&path));
        match result {
            Ok(recent) => {
                self.recent = recent;
                self.recent_rows.clear();
                // The next item may shift under the pointer: do not keep a stale
                // pressed/highlighted row or accidentally activate that next book.
                self.pointer = None;
                self.status = "已移出最近阅读；原书、阅读进度和标注均保留".into();
            }
            Err(error) => {
                self.status = format!("移除最近阅读失败：{error}");
            }
        }
        self.paint()?;
        Ok(true)
    }
}

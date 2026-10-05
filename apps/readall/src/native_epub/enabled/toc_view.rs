//! Fractional TOC offset and a bounded raster cache for the visible rows plus one.
//! The same clipped coordinate transform is used for painting and picking rows.
use super::*;
use crate::reader_data::Theme;
pub(super) const ROW_HEIGHT: f64 = 38.0;
#[derive(Default)]
pub(super) struct TocView {
    pub fraction: f64,
    cache: Option<(Key, Surface)>,
}
#[derive(PartialEq)]
struct Key {
    first: usize,
    selected: usize,
    count: usize,
    panel: Rect,
    logical: (u32, u32),
    pixels: (u32, u32),
    theme: Theme,
}
impl ReaderWindow<'_, '_, '_, '_> {
    pub(super) fn toc_rows_rect(&self) -> Rect {
        let p = self.toc_panel_rect();
        Rect::new(p.x, p.y + 48, p.width, p.height.saturating_sub(52))
    }
    pub(super) fn toc_offset(&self) -> f64 {
        self.toc_scroll as f64 * ROW_HEIGHT + self.toc_view.fraction
    }
    pub(super) fn toc_max_offset(&self) -> f64 {
        (self.toc.len() as f64 * ROW_HEIGHT - f64::from(self.toc_rows_rect().height)).max(0.0)
    }
    pub(super) fn scroll_toc_pixels(&mut self, delta: f64) -> bool {
        if !delta.is_finite()
            || self.toolbar != ToolbarMode::Toc
            || self.tools.mode != tools::Mode::None
        {
            return false;
        }
        let before = self.toc_offset();
        let next = (before + delta).clamp(0.0, self.toc_max_offset());
        self.toc_scroll = (next / ROW_HEIGHT).floor() as usize;
        self.toc_view.fraction = next - self.toc_scroll as f64 * ROW_HEIGHT;
        if (next - before).abs() < f64::EPSILON {
            return false;
        }
        self.pointer = None;
        let last =
            ((next + f64::from(self.toc_rows_rect().height) - 1.0) / ROW_HEIGHT).floor() as usize;
        self.toc_selected = self
            .toc_selected
            .clamp(self.toc_scroll, last.min(self.toc.len().saturating_sub(1)));
        true
    }
    fn toc_row_at(&self, x: i32, y: i32) -> Option<usize> {
        let rows = self.toc_rows_rect();
        if !point_in(rows, x, y) || x < rows.x + 8 || x >= rows.x + rows.width as i32 - 8 {
            return None;
        }
        // Use the identical device-pixel rounding used by the row-cache blit.
        let scale = f64::from(self.surface.pixel_scale().1);
        let shift = (self.toc_view.fraction * scale).round() / scale;
        let local = f64::from(y - rows.y) + shift;
        let row = (local / ROW_HEIGHT).floor() as usize;
        let index = self.toc_scroll.saturating_add(row);
        (local - row as f64 * ROW_HEIGHT < 34.0 && index < self.toc.len()).then_some(index)
    }
    pub(super) fn toc_hover_at(&self, x: i32, y: i32) -> Option<ReaderHover> {
        self.toc_row_at(x, y).map(ReaderHover::TocRow)
    }
    pub(super) fn draw_toc_rows(&mut self) -> WindowResult<()> {
        let rows = self.toc_rows_rect();
        let theme = self.session.settings().theme;
        let p = theme.palette();
        let key = Key {
            first: self.toc_scroll,
            selected: self.toc_selected,
            count: self.toc.len(),
            panel: self.toc_panel_rect(),
            theme,
            logical: (self.surface.width(), self.surface.height()),
            pixels: (self.surface.pixel_width(), self.surface.pixel_height()),
        };
        if self
            .toc_view
            .cache
            .as_ref()
            .is_none_or(|(old, _)| old != &key)
        {
            let mut surface = Surface::new_scaled(key.logical, key.pixels, Default::default())?;
            // The extra row is off-panel in this scratch surface, never on the reading page.
            let band = Rect::new(rows.x, rows.y, rows.width, rows.height + 38);
            surface.draw(&[DrawCommand::FillRect {
                rect: band,
                color: p.panel,
            }])?;
            let count = rows.height.div_ceil(38) as usize + 1;
            for (row, entry) in self
                .toc
                .iter()
                .skip(self.toc_scroll)
                .take(count)
                .enumerate()
            {
                let y = rows.y + row as i32 * 38;
                if self.toc_scroll + row == self.toc_selected {
                    let selected = Rect::new(rows.x + 8, y, rows.width.saturating_sub(16), 34);
                    super::menu_style::rounded(&mut surface, selected, 9, p.selected)?;
                }
                let indent = entry.depth.min(6) as i32 * 16;
                let mut text = UiPainter::new(&self.ui_font, &mut surface)?;
                let title = text.fit(
                    14,
                    &entry.title,
                    rows.width.saturating_sub(56 + indent as u32),
                )?;
                let ink = if self.toc_scroll + row == self.toc_selected {
                    p.accent
                } else {
                    p.ink
                };
                text.draw_clipped(rows.x + 20 + indent, y + 9, 14, &title, ink, band)?;
            }
            self.toc_view.cache = Some((key, surface));
        }
        let (_, cached) = self.toc_view.cache.as_ref().unwrap();
        let destination = self.surface.pixel_rect(rows);
        let clip = self.surface.pixel_rect(Rect::new(
            rows.x + 8,
            rows.y,
            rows.width.saturating_sub(16),
            rows.height,
        ));
        let band = cached.pixel_rect(Rect::new(rows.x, rows.y, rows.width, rows.height + 38));
        let shift =
            (self.toc_view.fraction * f64::from(self.surface.pixel_scale().1)).round() as i32;
        self.surface.copy_region_pixels(
            cached,
            band,
            (destination.x, destination.y - shift),
            clip,
        )?;
        // A narrow position indicator does not consume or change row hit targets.
        let extent = self.toc_max_offset();
        if extent > 0.0 {
            let height = ((f64::from(rows.height).powi(2) / (extent + f64::from(rows.height)))
                .round() as u32)
                .clamp(16, rows.height.max(16));
            let y = rows.y
                + ((rows.height.saturating_sub(height)) as f64
                    * (self.toc_offset() / extent).clamp(0.0, 1.0))
                .round() as i32;
            self.surface.draw(&[DrawCommand::FillRect {
                rect: Rect::new(rows.x + rows.width as i32 - 10, y, 2, height).intersection(rows),
                color: p.border,
            }])?;
        }
        Ok(())
    }
}

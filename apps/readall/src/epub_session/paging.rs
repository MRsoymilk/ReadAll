//! Three-page strip cache. Neighbour preparation never advances reading progress;
//! promotion is transactional and swaps the parsed chapter/renderer at boundaries.
use super::*;
use crate::text_page::TextHit;
use readall_render::{DrawCommand, Rect, Surface};
struct Neighbour<'b, 'a, 'f, 'd> {
    frame: RenderedPage,
    anchor: EpubLocator,
    spine: usize,
    owned: Option<(Chapter<'b, 'a>, PageRenderer<'f, 'd>)>,
}
pub(super) struct Paging<'b, 'a, 'f, 'd> {
    neighbours: [Option<Neighbour<'b, 'a, 'f, 'd>>; 2],
    checked: [bool; 2],
    pub(super) view: Option<RenderedPage>,
    offset: f64,
}
impl Default for Paging<'_, '_, '_, '_> {
    fn default() -> Self {
        Self {
            neighbours: [None, None],
            checked: [false; 2],
            view: None,
            offset: 0.0,
        }
    }
}
fn side(direction: i32) -> usize {
    usize::from(direction > 0)
}
impl<'b, 'a, 'f, 'd> EpubSession<'b, 'a, 'f, 'd> {
    pub(crate) fn clear_paging(&mut self) {
        self.paging = Paging::default();
    }
    pub(crate) fn raw_surface(&self) -> &Surface {
        &self.frame.surface
    }
    pub(crate) fn clear_scroll_view(&mut self) {
        self.paging.view = None;
    }
    pub(crate) fn neighbour_surface(&self, direction: i32) -> Option<&Surface> {
        self.paging.neighbours[side(direction)]
            .as_ref()
            .map(|n| &n.frame.surface)
    }
    pub(super) fn scroll_progress(&self) -> Option<f32> {
        self.paging.view.as_ref()?;
        let fraction = self.paging.offset / self.scroll_stride();
        let (spine, page, pages, portion) = if fraction > 0.0 {
            self.paging.neighbours[1].as_ref().map_or(
                (self.spine, self.frame.page, self.frame.pages, 1.0),
                |next| (next.spine, next.frame.page, next.frame.pages, fraction),
            )
        } else {
            (
                self.spine,
                self.frame.page,
                self.frame.pages,
                1.0 + fraction,
            )
        };
        Some(
            ((spine as f64 + (page as f64 + portion.clamp(0.0, 1.0)) / pages.max(1) as f64)
                / self.book.spine().len().max(1) as f64)
                .clamp(0.0, 1.0) as f32,
        )
    }
    pub(crate) fn scroll_stride(&self) -> f64 {
        self.options.height.saturating_sub(36).max(1) as f64
    }
    pub(crate) fn neighbour_ready(&self, direction: i32) -> bool {
        self.paging.neighbours[side(direction)].is_some()
    }
    pub(crate) fn neighbour_checked(&self, direction: i32) -> bool {
        self.paging.checked[side(direction)]
    }
    pub(crate) fn prepare_neighbour(&mut self, direction: i32) -> Result<bool> {
        let side = side(direction);
        if self.paging.checked[side] {
            return Ok(self.paging.neighbours[side].is_some());
        }
        let next = if direction > 0 {
            self.frame
                .page
                .checked_add(1)
                .filter(|p| *p < self.frame.pages)
        } else {
            self.frame.page.checked_sub(1)
        };
        let mut options = self.options.clone();
        options.at = None;
        let neighbour = if let Some(page) = next {
            options.page = Some(page);
            let frame = self.renderer.render(&self.chapter, &options)?;
            let anchor = page_anchor(self.book, self.spine, &self.chapter, &frame)?;
            Some(Neighbour {
                frame,
                anchor,
                spine: self.spine,
                owned: None,
            })
        } else {
            let found = if direction > 0 {
                next_readable(self.book, self.spine)?
            } else {
                previous_readable(self.book, self.spine)?
            };
            if let Some((spine, chapter)) = found {
                let mut renderer =
                    PageRenderer::new(self.font, options.size, options.allow_missing)?
                        .with_fallbacks(&self.fallbacks)
                        .with_preferences(self.preferences.theme, self.preferences.line_spacing);
                options.page = Some(0);
                let mut frame = renderer.render(&chapter, &options)?;
                if direction < 0 && frame.pages > 1 {
                    options.page = Some(frame.pages - 1);
                    frame = renderer.render(&chapter, &options)?;
                }
                let anchor = page_anchor(self.book, spine, &chapter, &frame)?;
                Some(Neighbour {
                    frame,
                    anchor,
                    spine,
                    owned: Some((chapter, renderer)),
                })
            } else {
                None
            }
        };
        self.paging.neighbours[side] = neighbour;
        self.paging.checked[side] = true;
        Ok(self.paging.neighbours[side].is_some())
    }
    /// Failed speculative work is not retried on every animation tick. An explicit
    /// navigation can report it once; unrelated current-page interactions still work.
    pub(crate) fn skip_prefetch(&mut self, direction: i32) {
        self.paging.checked[side(direction)] = true;
    }
    pub(crate) fn turn_cached(&mut self, direction: i32) -> Result<bool> {
        if !self.prepare_neighbour(direction)? {
            return Ok(false);
        }
        self.promote_neighbour(direction)
    }
    fn promote_neighbour(&mut self, direction: i32) -> Result<bool> {
        let side = side(direction);
        let Some(mut next) = self.paging.neighbours[side].take() else {
            return Ok(false);
        };
        let old_anchor = page_anchor(self.book, self.spine, &self.chapter, &self.frame)?;
        let owned = next.owned.take().map(|(chapter, renderer)| {
            (
                std::mem::replace(&mut self.chapter, chapter),
                std::mem::replace(&mut self.renderer, renderer),
            )
        });
        let previous = Neighbour {
            frame: std::mem::replace(&mut self.frame, next.frame),
            anchor: old_anchor,
            spine: self.spine,
            owned,
        };
        self.spine = next.spine;
        self.anchor = next.anchor;
        self.options.at = None;
        self.options.page = Some(self.frame.page);
        self.paging.neighbours = [None, None];
        self.paging.neighbours[1 - side] = Some(previous);
        self.paging.checked = [false; 2];
        self.paging.checked[1 - side] = true;
        self.paging.view = None;
        Ok(true)
    }
    /// Keep the leading page as the strip origin. Each promotion leaves pixels at
    /// the same screen coordinate; the target delta receives the identical shift.
    pub(crate) fn normalize_scroll(&mut self, offset: &mut f64, target: &mut f64) -> Result<()> {
        let stride = self.scroll_stride();
        for _ in 0..4 {
            let direction = if *offset >= stride {
                1
            } else if *offset < 0.0 {
                -1
            } else {
                break;
            };
            if self.turn_cached(direction)? {
                let shift = f64::from(direction) * stride;
                *offset -= shift;
                *target -= shift;
            } else {
                *offset = 0.0;
                *target = 0.0;
                break;
            }
        }
        if !self.neighbour_ready(1) && self.neighbour_checked(1) {
            *offset = offset.min(0.0);
            *target = target.min(0.0);
        }
        if !self.neighbour_ready(-1) && self.neighbour_checked(-1) {
            *offset = offset.max(0.0);
            *target = target.max(0.0);
        }
        Ok(())
    }
    pub(crate) fn scroll_offset_for_anchor(&self) -> f64 {
        let y = if let Some(image) = self.anchor.image_index() {
            self.frame
                .image_hits
                .iter()
                .find(|(_, index)| *index == image)
                .map(|(rect, _)| rect.y)
        } else {
            let at = self.anchor.utf8_offset() as usize;
            self.frame
                .hits
                .iter()
                .filter(|h| h.end > at)
                .min_by_key(|h| (h.start, h.rect.y))
                .map(|h| h.rect.y)
        };
        y.map_or(0.0, |y| f64::from((y - 32).max(0)))
            .min(self.scroll_stride() - 1.0)
    }
    pub(crate) fn compose_scroll(&mut self, offset: f64) -> Result<()> {
        let offset = if offset.is_finite() { offset } else { 0.0 };
        let (w, h) = (self.options.width, self.options.height);
        let stride = self.scroll_stride() as i32;
        let source_top = 32;
        let top = 32;
        let clip = Rect::new(0, 32, w, h.saturating_sub(36));
        let band = Rect::new(0, source_top, w, stride as u32);
        let mut view = self
            .paging
            .view
            .take()
            .unwrap_or_else(|| self.frame.clone());
        view.surface.draw(&[DrawCommand::FillRect {
            rect: Rect::new(0, 0, w, h),
            color: self.preferences.theme.colors().0,
        }])?;
        view.hits.clear();
        view.image_hits.clear();
        view.page = self.frame.page;
        view.pages = self.frame.pages;
        view.locator = self.frame.locator.clone();
        view.image_index = self.frame.image_index;
        for index in [-1, 0, 1] {
            let tile = if index == 0 {
                Some((&self.frame, self.spine))
            } else {
                self.paging.neighbours[side(index)]
                    .as_ref()
                    .map(|n| (&n.frame, n.spine))
            };
            let Some((frame, spine)) = tile else { continue };
            let y = top + index * stride - offset.round() as i32;
            view.surface
                .copy_region(&frame.surface, band, (0, y), clip)?;
            // Hits use the active chapter's logical offsets. Other-chapter slices
            // are activated before a press, so offsets can never alias accidentally.
            if spine != self.spine {
                continue;
            }
            let shift = y - source_top;
            for hit in &frame.hits {
                let r = Rect::new(
                    hit.rect.x,
                    hit.rect.y + shift,
                    hit.rect.width,
                    hit.rect.height,
                )
                .intersection(clip);
                if r.width != 0 && r.height != 0 {
                    view.hits.push(TextHit {
                        rect: r,
                        start: hit.start,
                        end: hit.end,
                    });
                }
            }
            for (rect, image) in &frame.image_hits {
                let r =
                    Rect::new(rect.x, rect.y + shift, rect.width, rect.height).intersection(clip);
                if r.width != 0 && r.height != 0 {
                    view.image_hits.push((r, *image));
                }
            }
        }
        view.hits.sort_by_key(|h| (h.rect.y, h.start));
        self.paging.offset = offset;
        self.paging.view = Some(view);
        Ok(())
    }
    /// Activate a visible next/previous chapter without moving the strip. Text
    /// selection remains within one chapter; same-chapter page seams are selectable.
    pub(crate) fn activate_scroll_slice(&mut self, y: i32) -> Result<f64> {
        let stride = self.scroll_stride();
        let top = 32.0;
        let index = ((f64::from(y) - top + self.paging.offset) / stride).floor() as i32;
        if matches!(index, -1 | 1)
            && self.paging.neighbours[side(index)]
                .as_ref()
                .is_some_and(|n| n.spine != self.spine)
        {
            let offset = self.paging.offset - f64::from(index) * stride;
            if self.promote_neighbour(index)? {
                self.compose_scroll(offset)?;
                return Ok(f64::from(index) * stride);
            }
        }
        Ok(0.0)
    }
    pub(crate) fn remember_scroll_position(&mut self) -> Result<()> {
        let Some(view) = self.paging.view.as_ref() else {
            return Ok(());
        };
        let text = view
            .hits
            .iter()
            .filter(|h| h.rect.height > 0)
            .min_by_key(|h| (h.rect.y, h.start));
        let image = view.image_hits.iter().min_by_key(|(r, _)| r.y);
        if let Some((rect, index)) = image
            && text.is_none_or(|hit| rect.y <= hit.rect.y)
        {
            self.anchor = self.chapter.epub_locator(0, Some(*index))?;
        } else if let Some(hit) = text {
            self.anchor = self.chapter.epub_locator(hit.start, None)?;
        }
        Ok(())
    }
}

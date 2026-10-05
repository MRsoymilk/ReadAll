//! Paint cached page surfaces without rasterizing text again.
use crate::{Color, Rect, RenderError, Surface};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageEffect {
    Slide,
    Book,
}
impl Surface {
    /// Copy already-composited pixels. Source/destination clipping preserves the
    /// coordinate mapping, including negative origins. This is not alpha blending.
    pub fn copy_region(
        &mut self,
        source: &Surface,
        region: Rect,
        at: (i32, i32),
        clip: Rect,
    ) -> Result<(), RenderError> {
        if self.pixel_scale() != source.pixel_scale() {
            return Err(RenderError::InvalidGeometry(
                "page copy device scale mismatch",
            ));
        }
        self.copy_region_pixels(
            source,
            source.pixel_rect(region),
            self.pixel_point((f64::from(at.0), f64::from(at.1))),
            self.pixel_rect(clip),
        )
    }
    /// Device-pixel copy for strip compositors; avoids accumulating fractional-density rounding.
    pub fn copy_region_pixels(
        &mut self,
        source: &Surface,
        region: Rect,
        at: (i32, i32),
        clip: Rect,
    ) -> Result<(), RenderError> {
        let region_in_source = region.intersection(Rect::new(0, 0, source.width, source.height));
        let dx = i64::from(at.0) - i64::from(region.x);
        let dy = i64::from(at.1) - i64::from(region.y);
        let x0 = (i64::from(region_in_source.x) + dx)
            .max(0)
            .max(i64::from(clip.x));
        let y0 = (i64::from(region_in_source.y) + dy)
            .max(0)
            .max(i64::from(clip.y));
        let x1 = (i64::from(region_in_source.x) + dx + i64::from(region_in_source.width))
            .min(i64::from(self.width))
            .min(i64::from(clip.x) + i64::from(clip.width));
        let y1 = (i64::from(region_in_source.y) + dy + i64::from(region_in_source.height))
            .min(i64::from(self.height))
            .min(i64::from(clip.y) + i64::from(clip.height));
        if x1 <= x0 || y1 <= y0 {
            return Ok(());
        }
        if ((x1 - x0) as u64).saturating_mul((y1 - y0) as u64) > self.limits.max_blended_pixels {
            return Err(RenderError::BudgetExceeded("page copy pixels"));
        }
        let count = (x1 - x0) as usize;
        for y in y0..y1 {
            let from = (y - dy) as usize * source.width as usize + (x0 - dx) as usize;
            let to = y as usize * self.width as usize + x0 as usize;
            self.pixels[to..to + count].copy_from_slice(&source.pixels[from..from + count]);
        }
        Ok(())
    }
    /// A bounded 2D paper-curl approximation: curved fold, mirrored/tinted back
    /// face and moving shadow. Endpoint frames are exactly the source/target.
    pub fn page_transition(
        &mut self,
        pages: (&Surface, &Surface),
        clip: Rect,
        progress: f32,
        backwards: bool,
        effect: PageEffect,
        paper: Color,
    ) -> Result<(), RenderError> {
        let (from, to) = pages;
        if !progress.is_finite()
            || (self.width, self.height) != (from.width, from.height)
            || (self.width, self.height) != (to.width, to.height)
            || self.logical != from.logical
            || self.logical != to.logical
        {
            return Err(RenderError::InvalidGeometry("page transition geometry"));
        }
        let clip = self
            .pixel_rect(clip)
            .intersection(Rect::new(0, 0, self.width, self.height));
        if clip.area().saturating_mul(3) > self.limits.max_blended_pixels {
            return Err(RenderError::BudgetExceeded("page transition pixels"));
        }
        let p = progress.clamp(0.0, 1.0);
        if p <= 0.0 {
            return self.copy_region_pixels(from, clip, (clip.x, clip.y), clip);
        }
        if p >= 1.0 {
            return self.copy_region_pixels(to, clip, (clip.x, clip.y), clip);
        }
        self.copy_region_pixels(to, clip, (clip.x, clip.y), clip)?;
        let w = clip.width as i32;
        if effect == PageEffect::Slide {
            let shift = (p * w as f32).round() as i32;
            let direction = if backwards { 1 } else { -1 };
            self.copy_region_pixels(to, clip, (clip.x - direction * (w - shift), clip.y), clip)?;
            return self.copy_region_pixels(from, clip, (clip.x + direction * shift, clip.y), clip);
        }
        let curl = (p * std::f32::consts::PI).sin();
        let roll = (clip.width as f32 * 0.17).min(140.0 * self.pixel_scale().0) * curl;
        for row in 0..clip.height as usize {
            let bend = ((row as f32 / clip.height.max(1) as f32) - 0.5) * roll * 0.35;
            let fold = ((1.0 - p) * w as f32 + bend).clamp(0.0, w as f32);
            let front = fold.floor() as i32;
            let base = (clip.y as usize + row) * self.width as usize + clip.x as usize;
            if front > 0 {
                let (left, right) = if backwards {
                    ((w - front) as usize, w as usize)
                } else {
                    (0, front as usize)
                };
                self.pixels[base + left..base + right]
                    .copy_from_slice(&from.pixels[base + left..base + right]);
            }
            let end = (fold + roll + 20.0 * self.pixel_scale().0 * curl)
                .ceil()
                .min(w as f32) as i32;
            for local in front..end {
                let x = if backwards { w - 1 - local } else { local };
                let at = base + x as usize;
                let distance = local as f32 - fold;
                if roll > 0.01 && distance < roll {
                    let q = (distance / roll).clamp(0.0, 1.0);
                    let reflected = (fold - distance * 0.9)
                        .floor()
                        .clamp(0.0, (w - 1).max(0) as f32)
                        as i32;
                    let source_x = if backwards {
                        w - 1 - reflected
                    } else {
                        reflected
                    };
                    let ink = from.pixels[base + source_x as usize];
                    let shade = 1.0 - 0.22 * (1.0 - q) * (1.0 - q);
                    let channel = |bg: u8, text: u8| {
                        ((0.93 * f32::from(bg) + 0.07 * f32::from(text)) * shade).round() as u8
                    };
                    self.pixels[at] = Color::rgba(
                        channel(paper.r, ink.r),
                        channel(paper.g, ink.g),
                        channel(paper.b, ink.b),
                        255,
                    );
                } else {
                    let shadow = (1.0
                        - (distance - roll) / (20.0 * self.pixel_scale().0 * curl).max(0.01))
                    .clamp(0.0, 1.0);
                    self.pixels[at] =
                        Color::rgba(0, 0, 0, (shadow * 45.0) as u8).over(self.pixels[at]);
                }
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DrawCommand, RenderLimits};
    fn page(color: Color) -> Surface {
        let mut s = Surface::new(80, 60, RenderLimits::default()).unwrap();
        s.draw(&[DrawCommand::FillRect {
            rect: Rect::new(0, 0, 80, 60),
            color,
        }])
        .unwrap();
        s
    }
    #[test]
    fn effects_have_exact_endpoints_and_leave_chrome_unchanged() {
        let from = page(Color::rgba(80, 20, 10, 255));
        let to = page(Color::rgba(10, 80, 20, 255));
        let clip = Rect::new(0, 8, 80, 44);
        for effect in [PageEffect::Slide, PageEffect::Book] {
            for reverse in [false, true] {
                for p in [0.0, 0.2, 0.5, 0.8, 1.0] {
                    let mut out = to.clone();
                    out.page_transition((&from, &to), clip, p, reverse, effect, Color::WHITE)
                        .unwrap();
                    assert_eq!(out.pixel(1, 2), to.pixel(1, 2));
                    assert_eq!(out.pixel(1, 58), to.pixel(1, 58));
                    if p == 0.0 {
                        assert_eq!(out.pixel(10, 10), from.pixel(10, 10));
                    }
                    if p == 1.0 {
                        assert_eq!(out.pixels(), to.pixels());
                    }
                }
            }
        }
    }
    #[test]
    fn page_copy_clips_negative_offsets_and_rejects_bad_transitions() {
        let source = page(Color::rgba(1, 2, 3, 255));
        let mut out = page(Color::WHITE);
        out.copy_region(
            &source,
            Rect::new(5, 6, 30, 20),
            (-10, -4),
            Rect::new(0, 0, 80, 60),
        )
        .unwrap();
        assert_eq!(out.pixel(0, 0), source.pixel(15, 10));
        assert_eq!(out.pixel(20, 20), Some(Color::WHITE));
        let before = out.pixels().to_vec();
        assert!(
            out.page_transition(
                (&source, &source),
                Rect::new(0, 0, 80, 60),
                f32::NAN,
                false,
                PageEffect::Book,
                Color::WHITE
            )
            .is_err()
        );
        assert_eq!(before, out.pixels());
    }
}

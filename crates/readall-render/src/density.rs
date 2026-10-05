//! Explicit logical layout versus device-pixel storage. Scaling is applied to
//! vector geometry before rasterization, never to an already rasterized text page.
use crate::{Rect, RenderError, RenderLimits, Surface};
impl Surface {
    pub fn new_scaled(
        logical: (u32, u32),
        pixels: (u32, u32),
        limits: RenderLimits,
    ) -> Result<Self, RenderError> {
        if logical.0 == 0 || logical.1 == 0 || logical.0 > 16384 || logical.1 > 16384 {
            return Err(RenderError::InvalidDimensions);
        }
        let sx = f64::from(pixels.0) / f64::from(logical.0);
        let sy = f64::from(pixels.1) / f64::from(logical.1);
        if !(0.25..=8.0).contains(&sx) || !(0.25..=8.0).contains(&sy) {
            return Err(RenderError::InvalidGeometry("device scale must be 0.25..8"));
        }
        let mut surface = Self::new(pixels.0, pixels.1, limits)?;
        surface.logical = logical;
        Ok(surface)
    }
    pub fn pixel_width(&self) -> u32 {
        self.width
    }
    pub fn pixel_height(&self) -> u32 {
        self.height
    }
    pub fn pixel_scale(&self) -> (f32, f32) {
        (
            self.width as f32 / self.logical.0 as f32,
            self.height as f32 / self.logical.1 as f32,
        )
    }
    pub fn pixel_point(&self, point: (f64, f64)) -> (i32, i32) {
        (
            (point.0 * f64::from(self.width) / f64::from(self.logical.0)).round() as i32,
            (point.1 * f64::from(self.height) / f64::from(self.logical.1)).round() as i32,
        )
    }
    pub fn pixel_rect(&self, rect: Rect) -> Rect {
        if self.logical == (self.width, self.height) {
            return rect;
        }
        let (x, y) = self.pixel_point((f64::from(rect.x), f64::from(rect.y)));
        let (right, bottom) = self.pixel_point((
            f64::from(rect.x) + f64::from(rect.width),
            f64::from(rect.y) + f64::from(rect.height),
        ));
        Rect::new(
            x,
            y,
            (i64::from(right) - i64::from(x)).max(0) as u32,
            (i64::from(bottom) - i64::from(y)).max(0) as u32,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, DrawCommand, PageEffect};
    #[test]
    fn native_pixels_keep_layout_size_and_clip_in_device_coordinates() {
        let mut s = Surface::new_scaled((40, 60), (110, 165), RenderLimits::default()).unwrap();
        assert_eq!((s.width(), s.height()), (40, 60));
        assert_eq!(
            (s.pixel_width(), s.pixel_height(), s.pixels().len()),
            (110, 165, 18150)
        );
        let r = Rect::new(2, 3, 10, 11);
        s.draw(&[
            DrawCommand::PushClip(r),
            DrawCommand::FillRect {
                rect: Rect::new(-20, -20, 100, 100),
                color: Color::WHITE,
            },
            DrawCommand::PopClip,
        ])
        .unwrap();
        let p = s.pixel_rect(r);
        assert_eq!(
            s.pixels().iter().filter(|c| c.a == 255).count(),
            p.width as usize * p.height as usize
        );
        assert!(Surface::new_scaled((40, 60), (400, 600), RenderLimits::default()).is_err());
        assert!(
            Surface::new_scaled(
                (40, 60),
                (110, 165),
                RenderLimits {
                    max_pixels: 100,
                    ..RenderLimits::default()
                }
            )
            .is_err()
        );
    }
    #[test]
    fn dense_page_copy_and_effect_endpoints_use_exact_pixels() {
        let mut a = Surface::new_scaled((40, 60), (110, 165), RenderLimits::default()).unwrap();
        a.draw(&[DrawCommand::FillRect {
            rect: Rect::new(0, 0, 40, 60),
            color: Color::WHITE,
        }])
        .unwrap();
        let b = Surface::new_scaled((40, 60), (110, 165), RenderLimits::default()).unwrap();
        for effect in [PageEffect::Slide, PageEffect::Book] {
            let mut out = b.clone();
            out.page_transition(
                (&a, &b),
                Rect::new(0, 0, 40, 60),
                0.0,
                false,
                effect,
                Color::WHITE,
            )
            .unwrap();
            assert_eq!(out.pixels(), a.pixels());
            out.page_transition(
                (&a, &b),
                Rect::new(0, 0, 40, 60),
                1.0,
                false,
                effect,
                Color::WHITE,
            )
            .unwrap();
            assert_eq!(out.pixels(), b.pixels());
        }
    }
}

//! Clipped nearest-neighbor RGBA image presentation with straight-alpha compositing.
use crate::{Color, Rect, RenderError, Surface};
impl Surface {
    /// Validate source and work budgets before touching the destination.
    pub fn draw_rgba(
        &mut self,
        pixels: &[u8],
        source: (u32, u32),
        destination: Rect,
        clip: Rect,
    ) -> Result<(), RenderError> {
        let destination = self.pixel_rect(destination);
        let clip = self.pixel_rect(clip);
        let (width, height) = source;
        let count = u64::from(width) * u64::from(height);
        if width == 0
            || height == 0
            || width > 16384
            || height > 16384
            || destination.width == 0
            || destination.height == 0
        {
            return Err(RenderError::InvalidDimensions);
        }
        if count > self.limits.max_pixels as u64 || count * 4 != pixels.len() as u64 {
            return Err(RenderError::InvalidGeometry("RGBA image buffer size"));
        }
        let visible =
            destination
                .intersection(clip)
                .intersection(Rect::new(0, 0, self.width, self.height));
        if visible.area() > self.limits.max_blended_pixels {
            return Err(RenderError::BudgetExceeded("image pixel work"));
        }
        for y in visible.y..visible.y + visible.height as i32 {
            let sy = ((i64::from(y) - i64::from(destination.y)) as u64 * u64::from(height)
                / u64::from(destination.height)) as usize;
            for x in visible.x..visible.x + visible.width as i32 {
                let sx = ((i64::from(x) - i64::from(destination.x)) as u64 * u64::from(width)
                    / u64::from(destination.width)) as usize;
                let at = (sy * width as usize + sx) * 4;
                let source =
                    Color::rgba(pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]);
                let destination = &mut self.pixels[y as usize * self.width as usize + x as usize];
                *destination = source.over(*destination);
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderLimits;
    #[test]
    fn scaled_image_clips_and_preserves_alpha() {
        let mut surface = Surface::new(4, 2, RenderLimits::default()).unwrap();
        let pixels = [255, 0, 0, 128, 0, 0, 255, 255];
        surface
            .draw_rgba(
                &pixels,
                (2, 1),
                Rect::new(-2, 0, 8, 2),
                Rect::new(0, 0, 3, 2),
            )
            .unwrap();
        assert_eq!(surface.pixel(0, 0), Some(Color::rgba(255, 0, 0, 128)));
        assert_eq!(surface.pixel(2, 0), Some(Color::rgba(0, 0, 255, 255)));
        assert_eq!(surface.pixel(3, 0), Some(Color::default()));
    }
    #[test]
    fn bad_source_is_transactional() {
        let mut surface = Surface::new(2, 2, RenderLimits::default()).unwrap();
        assert!(
            surface
                .draw_rgba(
                    &[255, 0, 0],
                    (1, 1),
                    Rect::new(0, 0, 2, 2),
                    Rect::new(0, 0, 2, 2)
                )
                .is_err()
        );
        assert!(
            surface
                .pixels()
                .iter()
                .all(|pixel| *pixel == Color::default())
        );
    }
}

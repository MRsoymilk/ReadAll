//! Dependency-free CPU drawing and unhinted TrueType glyph rasterization; no GPU or window backend.
//! Channels use straight-alpha, byte-space source-over compositing (not linear-light color management).

mod density;
pub mod glyph;
mod image;
mod page_motion;
pub use page_motion::PageEffect;

use std::{
    fmt,
    io::{self, Write},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    pub const WHITE: Self = Self::rgba(255, 255, 255, 255);

    pub fn over(self, destination: Self) -> Self {
        let source_alpha = u32::from(self.a);
        let remaining = 255 - source_alpha;
        let destination_alpha = u32::from(destination.a);
        let alpha = source_alpha * 255 + destination_alpha * remaining;
        if alpha == 0 {
            return Self::default();
        }
        let blend = |source: u8, dest: u8| -> u8 {
            ((u32::from(source) * source_alpha * 255
                + u32::from(dest) * destination_alpha * remaining
                + alpha / 2)
                / alpha) as u8
        };
        Self::rgba(
            blend(self.r, destination.r),
            blend(self.g, destination.g),
            blend(self.b, destination.b),
            ((alpha + 127) / 255) as u8,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn intersection(self, other: Self) -> Self {
        let x = i64::from(self.x).max(i64::from(other.x));
        let y = i64::from(self.y).max(i64::from(other.y));
        let end_x = (i64::from(self.x) + i64::from(self.width))
            .min(i64::from(other.x) + i64::from(other.width));
        let end_y = (i64::from(self.y) + i64::from(self.height))
            .min(i64::from(other.y) + i64::from(other.height));
        Self::new(
            x as i32,
            y as i32,
            (end_x - x).max(0) as u32,
            (end_y - y).max(0) as u32,
        )
    }

    fn area(self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DrawCommand {
    FillRect { rect: Rect, color: Color },
    PushClip(Rect),
    PopClip,
}

#[derive(Debug, Clone, Copy)]
pub struct RenderLimits {
    pub max_pixels: usize,
    pub max_commands: usize,
    pub max_clip_depth: usize,
    pub max_blended_pixels: u64,
}

impl Default for RenderLimits {
    fn default() -> Self {
        Self {
            max_pixels: 16 * 1024 * 1024,
            max_commands: 100_000,
            max_clip_depth: 64,
            max_blended_pixels: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug)]
pub enum RenderError {
    InvalidDimensions,
    InvalidGeometry(&'static str),
    BudgetExceeded(&'static str),
    InvalidClipStack,
    AllocationFailed,
    Io(io::Error),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions => f.write_str("surface dimensions must be 1..16384"),
            Self::InvalidGeometry(reason) => write!(f, "invalid geometry: {reason}"),
            Self::BudgetExceeded(what) => write!(f, "render budget exceeded: {what}"),
            Self::InvalidClipStack => f.write_str("unbalanced clip stack"),
            Self::AllocationFailed => f.write_str("cannot allocate rendering buffer"),
            Self::Io(error) => write!(f, "image output error: {error}"),
        }
    }
}
impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(error) = self {
            Some(error)
        } else {
            None
        }
    }
}
impl From<io::Error> for RenderError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct Surface {
    width: u32,
    height: u32,
    // Published frames and page-cache snapshots share immutable pixels. A painter
    // obtains unique storage once per operation, never once per destination pixel.
    pixels: std::sync::Arc<Vec<Color>>,
    limits: RenderLimits,
    logical: (u32, u32),
}

impl Surface {
    pub fn new(width: u32, height: u32, limits: RenderLimits) -> Result<Self, RenderError> {
        if !(1..=16384).contains(&width) || !(1..=16384).contains(&height) {
            return Err(RenderError::InvalidDimensions);
        }
        let count = usize::try_from(u64::from(width) * u64::from(height))
            .ok()
            .filter(|count| *count <= limits.max_pixels)
            .ok_or(RenderError::BudgetExceeded("pixels"))?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(count)
            .map_err(|_| RenderError::AllocationFailed)?;
        pixels.resize(count, Color::default());
        Ok(Self {
            width,
            height,
            pixels: std::sync::Arc::new(pixels),
            limits,
            logical: (width, height),
        })
    }

    /// Layout dimensions; `pixel_width/height` describe the raster backing store.
    pub fn width(&self) -> u32 {
        self.logical.0
    }
    pub fn height(&self) -> u32 {
        self.logical.1
    }
    pub fn pixels(&self) -> &[Color] {
        &self.pixels
    }
    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.pixels
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }

    /// Validates budgets and clipping before modifying any pixel.
    pub fn draw(&mut self, commands: &[DrawCommand]) -> Result<(), RenderError> {
        if self.logical == (self.width, self.height) {
            return self.draw_pixels(commands);
        }
        if commands.len() > self.limits.max_commands {
            return Err(RenderError::BudgetExceeded("commands"));
        }
        let mapped: Vec<_> = commands
            .iter()
            .map(|cmd| match *cmd {
                DrawCommand::FillRect { rect, color } => DrawCommand::FillRect {
                    rect: self.pixel_rect(rect),
                    color,
                },
                DrawCommand::PushClip(rect) => DrawCommand::PushClip(self.pixel_rect(rect)),
                DrawCommand::PopClip => DrawCommand::PopClip,
            })
            .collect();
        self.draw_pixels(&mapped)
    }

    /// Device-coordinate drawing for already rasterized vector primitives.
    pub fn draw_pixels(&mut self, commands: &[DrawCommand]) -> Result<(), RenderError> {
        if commands.len() > self.limits.max_commands {
            return Err(RenderError::BudgetExceeded("commands"));
        }
        let bounds = Rect::new(0, 0, self.width, self.height);
        let mut clips = Vec::new();
        clips
            .try_reserve(1)
            .map_err(|_| RenderError::AllocationFailed)?;
        clips.push(bounds);
        let mut draws = Vec::new();
        let mut work = 0_u64;
        for command in commands {
            match *command {
                DrawCommand::PushClip(rect) => {
                    if clips.len() > self.limits.max_clip_depth {
                        return Err(RenderError::BudgetExceeded("clip depth"));
                    }
                    let clipped = clips[clips.len() - 1].intersection(rect);
                    clips
                        .try_reserve(1)
                        .map_err(|_| RenderError::AllocationFailed)?;
                    clips.push(clipped);
                }
                DrawCommand::PopClip => {
                    if clips.len() == 1 {
                        return Err(RenderError::InvalidClipStack);
                    }
                    clips.pop();
                }
                DrawCommand::FillRect { rect, color } => {
                    let clipped = rect.intersection(clips[clips.len() - 1]);
                    work = work
                        .checked_add(clipped.area())
                        .filter(|work| *work <= self.limits.max_blended_pixels)
                        .ok_or(RenderError::BudgetExceeded("pixel work"))?;
                    if clipped.area() != 0 {
                        draws
                            .try_reserve(1)
                            .map_err(|_| RenderError::AllocationFailed)?;
                        draws.push((clipped, color));
                    }
                }
            }
        }
        if clips.len() != 1 {
            return Err(RenderError::InvalidClipStack);
        }
        for (rect, color) in draws {
            self.fill_clipped(rect, color);
        }
        Ok(())
    }

    fn fill_clipped(&mut self, rect: Rect, color: Color) {
        // Preflight guarantees nonempty rectangles are inside this surface.
        let x = rect.x as usize;
        let y = rect.y as usize;
        let pixels = std::sync::Arc::make_mut(&mut self.pixels);
        for row in y..y + rect.height as usize {
            let start = row * self.width as usize + x;
            let row = &mut pixels[start..start + rect.width as usize];
            if color.a == 255 {
                row.fill(color);
            } else {
                for destination in row {
                    *destination = color.over(*destination);
                }
            }
        }
    }

    /// Debug image export, flattening transparency onto white. Not a document renderer.
    pub fn write_ppm(&self, output: &mut impl Write) -> Result<(), RenderError> {
        write!(output, "P6\n{} {}\n255\n", self.width, self.height)?;
        for pixel in self.pixels.iter() {
            let pixel = pixel.over(Color::WHITE);
            output.write_all(&[pixel.r, pixel.g, pixel.b])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = Color::rgba(255, 0, 0, 255);
    const BLUE: Color = Color::rgba(0, 0, 255, 255);
    fn fill(rect: Rect, color: Color) -> DrawCommand {
        DrawCommand::FillRect { rect, color }
    }
    fn surface() -> Surface {
        Surface::new(4, 4, RenderLimits::default()).unwrap()
    }

    #[test]
    fn frame_snapshots_share_pixels_until_a_painter_writes() {
        let mut original = surface();
        original.draw(&[fill(Rect::new(0, 0, 4, 4), RED)]).unwrap();
        let snapshot = original.clone();
        assert!(std::sync::Arc::ptr_eq(&original.pixels, &snapshot.pixels));
        original.draw(&[fill(Rect::new(0, 0, 1, 1), BLUE)]).unwrap();
        assert!(!std::sync::Arc::ptr_eq(&original.pixels, &snapshot.pixels));
        assert_eq!(snapshot.pixel(0, 0), Some(RED));
        assert_eq!(original.pixel(0, 0), Some(BLUE));
        let copy = original.clone();
        original
            .copy_region_pixels(
                &snapshot,
                Rect::new(0, 0, 4, 4),
                (0, 0),
                Rect::new(0, 0, 4, 4),
            )
            .unwrap();
        assert_eq!(copy.pixel(0, 0), Some(BLUE));
        assert_eq!(original.pixel(0, 0), Some(RED));
    }
    #[test]
    fn straight_alpha_handles_transparent_and_opaque_destinations() {
        let half_red = Color::rgba(255, 0, 0, 128);
        assert_eq!(half_red.over(Color::default()), half_red);
        assert_eq!(half_red.over(BLUE), Color::rgba(128, 0, 127, 255));
        assert_eq!(
            Color::rgba(0, 0, 255, 128).over(half_red),
            Color::rgba(85, 0, 170, 192)
        );
        assert_eq!(Color::default().over(RED), RED);
    }

    #[test]
    fn clips_negative_and_extreme_coordinates() {
        let mut image = surface();
        image
            .draw(&[
                fill(Rect::new(-2, -2, 4, 4), RED),
                fill(Rect::new(i32::MAX, i32::MAX, u32::MAX, u32::MAX), BLUE),
            ])
            .unwrap();
        assert_eq!(image.pixel(0, 0), Some(RED));
        assert_eq!(image.pixel(1, 1), Some(RED));
        assert_eq!(image.pixel(2, 2), Some(Color::default()));
        assert_eq!(image.pixel(u32::MAX, 0), None);
        image
            .draw(&[fill(
                Rect::new(i32::MIN, i32::MIN, u32::MAX, u32::MAX),
                BLUE,
            )])
            .unwrap();
        assert!(image.pixels().iter().all(|color| *color == BLUE));
    }

    #[test]
    fn nested_clip_and_restore() {
        let mut image = surface();
        image
            .draw(&[
                DrawCommand::PushClip(Rect::new(1, 1, 2, 2)),
                fill(Rect::new(0, 0, 4, 4), RED),
                DrawCommand::PushClip(Rect::new(2, 2, 1, 1)),
                fill(Rect::new(0, 0, 4, 4), BLUE),
                DrawCommand::PopClip,
                DrawCommand::PopClip,
                fill(Rect::new(0, 0, 1, 1), Color::WHITE),
            ])
            .unwrap();
        assert_eq!(image.pixel(0, 0), Some(Color::WHITE));
        assert_eq!(image.pixel(1, 1), Some(RED));
        assert_eq!(image.pixel(2, 2), Some(BLUE));
        assert_eq!(image.pixel(3, 3), Some(Color::default()));
    }

    #[test]
    fn empty_intersections_remain_empty_under_nested_clips() {
        let mut image = surface();
        image
            .draw(&[
                DrawCommand::PushClip(Rect::new(100, 100, 2, 2)),
                DrawCommand::PushClip(Rect::new(0, 0, 4, 4)),
                fill(Rect::new(0, 0, 4, 4), RED),
                DrawCommand::PopClip,
                DrawCommand::PopClip,
            ])
            .unwrap();
        assert!(
            image
                .pixels()
                .iter()
                .all(|pixel| *pixel == Color::default())
        );
    }

    #[test]
    fn malformed_clip_lists_do_not_partially_draw() {
        for suffix in [
            DrawCommand::PopClip,
            DrawCommand::PushClip(Rect::new(0, 0, 1, 1)),
        ] {
            let mut image = surface();
            assert!(matches!(
                image.draw(&[fill(Rect::new(0, 0, 4, 4), RED), suffix]),
                Err(RenderError::InvalidClipStack)
            ));
            assert!(
                image
                    .pixels()
                    .iter()
                    .all(|pixel| *pixel == Color::default())
            );
        }
    }

    #[test]
    fn dimensions_allocations_and_work_are_bounded() {
        assert!(Surface::new(0, 1, RenderLimits::default()).is_err());
        assert!(Surface::new(u32::MAX, 1, RenderLimits::default()).is_err());
        assert!(
            Surface::new(
                4,
                4,
                RenderLimits {
                    max_pixels: 15,
                    ..RenderLimits::default()
                }
            )
            .is_err()
        );
        for limits in [
            RenderLimits {
                max_commands: 0,
                ..RenderLimits::default()
            },
            RenderLimits {
                max_blended_pixels: 15,
                ..RenderLimits::default()
            },
        ] {
            let mut image = Surface::new(4, 4, limits).unwrap();
            assert!(matches!(
                image.draw(&[fill(Rect::new(0, 0, 4, 4), RED)]),
                Err(RenderError::BudgetExceeded(_))
            ));
            assert!(
                image
                    .pixels()
                    .iter()
                    .all(|pixel| *pixel == Color::default())
            );
        }
        let mut image = Surface::new(
            4,
            4,
            RenderLimits {
                max_clip_depth: 0,
                ..RenderLimits::default()
            },
        )
        .unwrap();
        assert!(
            image
                .draw(&[
                    DrawCommand::PushClip(Rect::new(0, 0, 1, 1)),
                    DrawCommand::PopClip
                ])
                .is_err()
        );
    }

    #[test]
    fn ppm_header_size_and_alpha_flattening() {
        let mut image = Surface::new(2, 1, RenderLimits::default()).unwrap();
        image.draw(&[fill(Rect::new(0, 0, 1, 1), RED)]).unwrap();
        let mut bytes = Vec::new();
        image.write_ppm(&mut bytes).unwrap();
        assert_eq!(bytes, b"P6\n2 1\n255\n\xff\x00\x00\xff\xff\xff");
    }
}

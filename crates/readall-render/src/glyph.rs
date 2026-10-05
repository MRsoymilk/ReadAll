//! Unhinted quadratic-outline rasterization: adaptive flattening, nonzero winding,
//! and 4x4 grayscale supersampling. No platform font or graphics engine is called.
use crate::{Color, Rect, RenderError, Surface};
use readall_font::{Outline, Point};

type Result<T> = std::result::Result<T, RenderError>;
const SAMPLES: i32 = 4;

#[derive(Debug, Clone, Copy)]
pub struct RasterLimits {
    pub max_points: usize,
    pub max_edges: usize,
    pub max_pixels: usize,
    pub max_work: u64,
}
impl Default for RasterLimits {
    fn default() -> Self {
        Self {
            max_points: 65_536,
            max_edges: 65_536,
            max_pixels: 1024 * 1024,
            max_work: 32 * 1024 * 1024,
        }
    }
}
#[derive(Debug)]
pub struct GlyphMask {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    coverage: Vec<u8>,
    work: u64,
}
impl GlyphMask {
    pub fn left(&self) -> i32 {
        self.left
    }
    pub fn top(&self) -> i32 {
        self.top
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn coverage(&self) -> &[u8] {
        &self.coverage
    }
    pub fn work(&self) -> u64 {
        self.work
    }
    fn empty() -> Self {
        Self {
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            coverage: Vec::new(),
            work: 0,
        }
    }
}
#[derive(Debug, Clone, Copy)]
struct Vec2 {
    x: f32,
    y: f32,
}
impl Vec2 {
    fn midpoint(self, other: Self) -> Self {
        Self {
            x: (self.x + other.x) * 0.5,
            y: (self.y + other.y) * 0.5,
        }
    }
}
#[derive(Debug, Clone, Copy)]
struct Edge {
    from: Vec2,
    to: Vec2,
}
fn reserve<T>(values: &mut Vec<T>, count: usize) -> Result<()> {
    values
        .try_reserve(count)
        .map_err(|_| RenderError::AllocationFailed)
}
fn edge(edges: &mut Vec<Edge>, from: Vec2, to: Vec2, limit: usize) -> Result<()> {
    if from.x == to.x && from.y == to.y {
        return Ok(());
    }
    if edges.len() >= limit {
        return Err(RenderError::BudgetExceeded("flattened edges"));
    }
    reserve(edges, 1)?;
    edges.push(Edge { from, to });
    Ok(())
}
fn quadratic(
    edges: &mut Vec<Edge>,
    a: Vec2,
    b: Vec2,
    c: Vec2,
    depth: usize,
    limit: usize,
) -> Result<()> {
    let (dx, dy) = (a.x - 2.0 * b.x + c.x, a.y - 2.0 * b.y + c.y);
    if dx * dx + dy * dy <= 0.25 {
        return edge(edges, a, c, limit);
    }
    if depth >= 16 {
        return Err(RenderError::BudgetExceeded("quadratic subdivision depth"));
    }
    let ab = a.midpoint(b);
    let bc = b.midpoint(c);
    let mid = ab.midpoint(bc);
    quadratic(edges, a, ab, mid, depth + 1, limit)?;
    quadratic(edges, mid, bc, c, depth + 1, limit)
}

pub fn rasterize(outline: &Outline, scale: f32, limits: RasterLimits) -> Result<GlyphMask> {
    rasterize_contours(outline.contours(), scale, limits)
}
/// Rasterize outlines at the device scale without changing logical advances.
pub fn rasterize_scaled(
    outline: &Outline,
    scale: (f32, f32),
    limits: RasterLimits,
) -> Result<GlyphMask> {
    rasterize_contours_scaled(outline.contours(), scale, limits)
}
fn rasterize_contours<'a>(
    contours: impl Iterator<Item = &'a [Point]>,
    scale: f32,
    limits: RasterLimits,
) -> Result<GlyphMask> {
    rasterize_contours_scaled(contours, (scale, scale), limits)
}
fn rasterize_contours_scaled<'a>(
    contours: impl Iterator<Item = &'a [Point]>,
    scale: (f32, f32),
    limits: RasterLimits,
) -> Result<GlyphMask> {
    if [scale.0, scale.1]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0 || *v > 16.0)
    {
        return Err(RenderError::InvalidGeometry(
            "glyph scale must be finite and in (0,16]",
        ));
    }
    let mut edges = Vec::new();
    let (mut point_count, mut contour_count) = (0_usize, 0_usize);
    let mut min = Vec2 {
        x: f32::INFINITY,
        y: f32::INFINITY,
    };
    let mut max = Vec2 {
        x: f32::NEG_INFINITY,
        y: f32::NEG_INFINITY,
    };
    for points in contours {
        contour_count += 1;
        if contour_count > limits.max_points {
            return Err(RenderError::BudgetExceeded("contours"));
        }
        point_count = point_count
            .checked_add(points.len())
            .filter(|count| *count <= limits.max_points)
            .ok_or(RenderError::BudgetExceeded("outline points"))?;
        if points.is_empty() {
            continue;
        }
        let convert = |p: Point| Vec2 {
            x: p.x * scale.0,
            y: -p.y * scale.1,
        };
        for &p in points {
            let p = convert(p);
            if !p.x.is_finite()
                || !p.y.is_finite()
                || p.x.abs() > 16_777_216.0
                || p.y.abs() > 16_777_216.0
            {
                return Err(RenderError::InvalidGeometry(
                    "nonfinite or excessive glyph coordinate",
                ));
            }
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
        if max.x.ceil() - min.x.floor() > 4096.0 || max.y.ceil() - min.y.floor() > 4096.0 {
            return Err(RenderError::BudgetExceeded("glyph dimensions"));
        }
        let first = points[0];
        let last = points[points.len() - 1];
        let start = if first.on_curve {
            convert(first)
        } else if last.on_curve {
            convert(last)
        } else {
            convert(last).midpoint(convert(first))
        };
        let mut current = start;
        let mut i = usize::from(first.on_curve);
        while i < points.len() {
            let p = points[i];
            if p.on_curve {
                let next = convert(p);
                edge(&mut edges, current, next, limits.max_edges)?;
                current = next;
                i += 1;
            } else {
                let (next, on_curve) = if let Some(p) = points.get(i + 1) {
                    (convert(*p), p.on_curve)
                } else {
                    (start, true)
                };
                let end = if on_curve {
                    next
                } else {
                    convert(p).midpoint(next)
                };
                quadratic(&mut edges, current, convert(p), end, 0, limits.max_edges)?;
                current = end;
                i += if on_curve { 2 } else { 1 };
            }
        }
        edge(&mut edges, current, start, limits.max_edges)?;
    }
    if edges.is_empty() {
        return Ok(GlyphMask::empty());
    }
    let (left, top) = (min.x.floor() as i32, min.y.floor() as i32);
    let width = (max.x.ceil() - min.x.floor()) as u32;
    let height = (max.y.ceil() - min.y.floor()) as u32;
    if width == 0 || height == 0 {
        return Ok(GlyphMask::empty());
    }
    if width > 4096 || height > 4096 {
        return Err(RenderError::BudgetExceeded("glyph dimensions"));
    }
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .filter(|count| *count <= limits.max_pixels)
        .ok_or(RenderError::BudgetExceeded("glyph mask pixels"))?;
    // Include a conservative sorting factor as well as edge scans and coverage samples.
    let sort_factor = u64::from(edges.len().ilog2()) + 2;
    let work = (height as u64)
        .checked_mul(SAMPLES as u64)
        .and_then(|rows| rows.checked_mul(edges.len() as u64))
        .and_then(|scan| scan.checked_mul(sort_factor))
        .and_then(|scan| scan.checked_add(pixels as u64 * (SAMPLES * SAMPLES) as u64))
        .filter(|work| *work <= limits.max_work)
        .ok_or(RenderError::BudgetExceeded("glyph raster work"))?;
    let mut coverage = Vec::new();
    reserve(&mut coverage, pixels)?;
    coverage.resize(pixels, 0_u8);
    let mut intersections: Vec<(f32, i32)> = Vec::new();
    reserve(&mut intersections, edges.len())?;
    for sy in 0..height as i32 * SAMPLES {
        let y = top as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
        intersections.clear();
        for e in &edges {
            if y >= e.from.y.min(e.to.y) && y < e.from.y.max(e.to.y) {
                let t = (y - e.from.y) / (e.to.y - e.from.y);
                let x = e.from.x + t * (e.to.x - e.from.x);
                intersections.push((x, if e.to.y > e.from.y { 1 } else { -1 }));
            }
        }
        intersections.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let (mut winding, mut previous) = (0_i32, 0.0_f32);
        let row = (sy / SAMPLES) as usize * width as usize;
        for &(x, direction) in &intersections {
            if winding != 0 {
                let begin = (((previous - left as f32) * SAMPLES as f32 - 0.5).ceil() as i32)
                    .clamp(0, width as i32 * SAMPLES);
                let end = (((x - left as f32) * SAMPLES as f32 - 0.5).ceil() as i32)
                    .clamp(0, width as i32 * SAMPLES);
                for sx in begin..end {
                    coverage[row + (sx / SAMPLES) as usize] += 1;
                }
            }
            winding += direction;
            previous = x;
        }
    }
    for value in &mut coverage {
        *value = ((u16::from(*value) * 255 + 8) / 16) as u8;
    }
    Ok(GlyphMask {
        left,
        top,
        width,
        height,
        coverage,
        work,
    })
}

impl Surface {
    /// Synthetic emphasis is used only when the requested real font variant is absent.
    /// Pixel work and clipping remain bounded; this does not claim typographic hinting.
    pub fn draw_glyph_emphasis(
        &mut self,
        mask: &GlyphMask,
        baseline: (i32, i32),
        color: Color,
        clip: Rect,
        bold: u32,
        italic: bool,
    ) -> Result<()> {
        if bold == 0 && !italic {
            return self.draw_glyph(mask, baseline, color, clip);
        }
        if bold > 8 {
            return Err(RenderError::InvalidGeometry("synthetic bold width"));
        }
        let baseline = self.pixel_point((f64::from(baseline.0), f64::from(baseline.1)));
        let clip = self.pixel_rect(clip);
        let bold = (bold as f32 * self.pixel_scale().0)
            .round()
            .clamp(0.0, 64.0) as u32;
        let work = u64::from(mask.width) * u64::from(mask.height) * u64::from(bold + 1);
        if work > self.limits.max_blended_pixels {
            return Err(RenderError::BudgetExceeded("synthetic glyph work"));
        }
        let clip = clip.intersection(Rect::new(0, 0, self.width, self.height));
        for row in 0..mask.height {
            let y = i64::from(baseline.1) + i64::from(mask.top) + i64::from(row);
            if y < i64::from(clip.y) || y >= i64::from(clip.y) + i64::from(clip.height) {
                continue;
            }
            let skew = if italic {
                ((i64::from(baseline.1) - y) as f32 * 0.2).round() as i64
            } else {
                0
            };
            for column in 0..mask.width {
                let alpha = ((u16::from(color.a)
                    * u16::from(mask.coverage[(row * mask.width + column) as usize])
                    + 127)
                    / 255) as u8;
                if alpha == 0 {
                    continue;
                }
                for extra in 0..=bold {
                    let x = i64::from(baseline.0)
                        + i64::from(mask.left)
                        + i64::from(column)
                        + i64::from(extra)
                        + skew;
                    if x < i64::from(clip.x) || x >= i64::from(clip.x) + i64::from(clip.width) {
                        continue;
                    }
                    let destination =
                        &mut self.pixels[y as usize * self.width as usize + x as usize];
                    *destination = Color { a: alpha, ..color }.over(*destination);
                }
            }
        }
        Ok(())
    }

    /// Blits a validated mask at an integer baseline. Clips both to `clip` and surface bounds.
    /// All checks happen before pixels are modified. Placement currently snaps to whole pixels.
    pub fn draw_glyph(
        &mut self,
        mask: &GlyphMask,
        baseline: (i32, i32),
        color: Color,
        clip: Rect,
    ) -> Result<()> {
        if mask.width == 0 || mask.height == 0 {
            return Ok(());
        }
        let baseline = self.pixel_point((f64::from(baseline.0), f64::from(baseline.1)));
        let clip = self.pixel_rect(clip);
        let x = baseline
            .0
            .checked_add(mask.left)
            .ok_or(RenderError::InvalidGeometry("glyph x overflow"))?;
        let y = baseline
            .1
            .checked_add(mask.top)
            .ok_or(RenderError::InvalidGeometry("glyph y overflow"))?;
        let rect = Rect::new(x, y, mask.width, mask.height)
            .intersection(clip)
            .intersection(Rect::new(0, 0, self.width, self.height));
        if rect.area() == 0 {
            return Ok(());
        }
        if rect.area() > self.limits.max_blended_pixels {
            return Err(RenderError::BudgetExceeded("glyph blending work"));
        }
        for row in 0..rect.height as usize {
            let target = (rect.y as usize + row) * self.width as usize + rect.x as usize;
            let source = (i64::from(rect.y) - i64::from(y) + row as i64) as usize
                * mask.width as usize
                + (i64::from(rect.x) - i64::from(x)) as usize;
            for column in 0..rect.width as usize {
                let alpha = ((u16::from(color.a) * u16::from(mask.coverage[source + column]) + 127)
                    / 255) as u8;
                self.pixels[target + column] =
                    Color { a: alpha, ..color }.over(self.pixels[target + column]);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderLimits;
    fn point(x: f32, y: f32, on_curve: bool) -> Point {
        Point { x, y, on_curve }
    }
    fn mask(contours: &[Vec<Point>]) -> GlyphMask {
        rasterize_contours(
            contours.iter().map(Vec::as_slice),
            1.0,
            RasterLimits::default(),
        )
        .unwrap()
    }
    fn rectangle(x: f32, y: f32, w: f32, h: f32) -> Vec<Point> {
        vec![
            point(x, y, true),
            point(x + w, y, true),
            point(x + w, y + h, true),
            point(x, y + h, true),
        ]
    }
    #[test]
    fn synthetic_emphasis_is_clipped_and_bounded() {
        let mask = mask(&[rectangle(0.0, 0.0, 3.0, 4.0)]);
        let mut surface = Surface::new(16, 16, RenderLimits::default()).unwrap();
        surface
            .draw_glyph_emphasis(&mask, (2, 8), Color::WHITE, Rect::new(3, 3, 3, 5), 2, true)
            .unwrap();
        for y in 0..16 {
            for x in 0..16 {
                if !(3..6).contains(&x) || !(3..8).contains(&y) {
                    assert_eq!(surface.pixel(x, y), Some(Color::default()));
                }
            }
        }
        assert!(
            surface
                .draw_glyph_emphasis(
                    &mask,
                    (0, 0),
                    Color::WHITE,
                    Rect::new(0, 0, 16, 16),
                    9,
                    true
                )
                .is_err()
        );
    }
    #[test]
    fn solid_rectangle_preserves_baseline_and_negative_bearing() {
        let result = mask(&[rectangle(-2.0, 0.0, 4.0, 3.0)]);
        assert_eq!(
            (result.left(), result.top(), result.width(), result.height()),
            (-2, -3, 4, 3)
        );
        assert!(result.coverage().iter().all(|value| *value == 255));
    }
    #[test]
    fn nonzero_winding_preserves_holes_and_overlapping_fills() {
        let outer = rectangle(0.0, 0.0, 4.0, 4.0);
        let mut inner = rectangle(1.0, 1.0, 2.0, 2.0);
        inner.reverse();
        let hollow = mask(&[outer.clone(), inner]);
        assert_eq!(hollow.coverage()[5], 0);
        assert_eq!(hollow.coverage()[0], 255);
        let solid = mask(&[outer, rectangle(1.0, 1.0, 2.0, 2.0)]);
        assert!(solid.coverage().iter().all(|value| *value == 255));
    }
    #[test]
    fn quadratic_curves_and_implicit_on_curve_points_are_antialiased() {
        let points = vec![
            point(0.0, 2.0, false),
            point(2.0, 4.0, false),
            point(4.0, 2.0, false),
            point(2.0, 0.0, false),
        ];
        let result = mask(&[points]);
        assert!(result.coverage().iter().any(|a| *a > 0 && *a < 255));
        assert!(result.coverage().contains(&255));
        let curve = mask(&[vec![
            point(0.0, 0.0, true),
            point(4.0, 8.0, false),
            point(8.0, 0.0, true),
        ]]);
        assert_eq!((curve.width(), curve.height()), (8, 8));
        assert!(curve.coverage().iter().any(|a| *a > 0));
    }
    #[test]
    fn invalid_geometry_and_budgets_fail_before_blitting() {
        let points = rectangle(0.0, 0.0, 8.0, 8.0);
        for limits in [
            RasterLimits {
                max_pixels: 1,
                ..RasterLimits::default()
            },
            RasterLimits {
                max_edges: 1,
                ..RasterLimits::default()
            },
            RasterLimits {
                max_work: 1,
                ..RasterLimits::default()
            },
            RasterLimits {
                max_points: 1,
                ..RasterLimits::default()
            },
        ] {
            assert!(rasterize_contours([points.as_slice()].into_iter(), 1.0, limits).is_err());
        }
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                rasterize_contours(
                    [points.as_slice()].into_iter(),
                    scale,
                    RasterLimits::default()
                )
                .is_err()
            );
        }
        let bad = [point(f32::NAN, 0.0, true)];
        assert!(
            rasterize_contours([bad.as_slice()].into_iter(), 1.0, RasterLimits::default()).is_err()
        );
    }
    #[test]
    fn masks_clip_and_apply_color_alpha() {
        let result = mask(&[rectangle(0.0, 0.0, 4.0, 4.0)]);
        let mut surface = Surface::new(4, 4, RenderLimits::default()).unwrap();
        surface
            .draw_glyph(
                &result,
                (-1, 3),
                Color::rgba(255, 0, 0, 128),
                Rect::new(1, 1, 2, 2),
            )
            .unwrap();
        assert_eq!(surface.pixel(1, 1), Some(Color::rgba(255, 0, 0, 128)));
        assert_eq!(surface.pixel(0, 0), Some(Color::default()));
        let before = surface.pixels().to_vec();
        assert!(
            surface
                .draw_glyph(&result, (0, i32::MIN), Color::WHITE, Rect::new(0, 0, 4, 4))
                .is_err()
        );
        assert_eq!(surface.pixels(), before);
    }
    #[test]
    fn empty_and_degenerate_outlines_have_empty_masks() {
        assert!(mask(&[]).coverage().is_empty());
        assert!(mask(&[vec![point(1.0, 1.0, true)]]).coverage().is_empty());
    }
}

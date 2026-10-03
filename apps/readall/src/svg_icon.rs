//! Tiny renderer for trusted Feather SVG icons made from <line> and <polyline>.
use readall_render::{Color, DrawCommand, Rect, Surface};
use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub(crate) const CHEVRON_LEFT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/icons/reader/chevron-left.svg"
));
pub(crate) const CHEVRON_RIGHT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/icons/reader/chevron-right.svg"
));
pub(crate) const CHEVRON_UP: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/icons/reader/chevron-up.svg"
));
pub(crate) const CHEVRON_DOWN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/icons/reader/chevron-down.svg"
));
pub(crate) const LIST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/icons/reader/list.svg"
));
pub(crate) const MINUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/icons/reader/minus.svg"
));
pub(crate) const PLUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../res/icons/reader/plus.svg"
));

#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    x: f32,
    y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Segment {
    a: Point,
    b: Point,
}

pub(crate) fn draw(surface: &mut Surface, svg: &str, rect: Rect, color: Color) -> Result<()> {
    if rect.width == 0 || rect.height == 0 || rect.width > 96 || rect.height > 96 {
        return Err("SVG icon dimensions must be 1..96".into());
    }
    let segments = parse_segments(svg)?;
    if segments.is_empty() {
        return Err("SVG icon has no supported line geometry".into());
    }
    if segments.len() > 64 {
        return Err("SVG icon segment budget exceeded".into());
    }

    let scale_x = rect.width as f32 / 24.0;
    let scale_y = rect.height as f32 / 24.0;
    let stroke = 2.0 * scale_x.min(scale_y);
    let radius = stroke * 0.5;
    let mut commands = Vec::new();

    for segment in segments {
        let a = Point {
            x: rect.x as f32 + segment.a.x * scale_x,
            y: rect.y as f32 + segment.a.y * scale_y,
        };
        let b = Point {
            x: rect.x as f32 + segment.b.x * scale_x,
            y: rect.y as f32 + segment.b.y * scale_y,
        };
        let min_x = (a.x.min(b.x) - radius - 1.0).floor() as i32;
        let max_x = (a.x.max(b.x) + radius + 1.0).ceil() as i32;
        let min_y = (a.y.min(b.y) - radius - 1.0).floor() as i32;
        let max_y = (a.y.max(b.y) + radius + 1.0).ceil() as i32;

        for y in min_y..max_y {
            for x in min_x..max_x {
                let mut inside = 0_u32;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let p = Point {
                            x: x as f32 + (sx as f32 + 0.5) / 4.0,
                            y: y as f32 + (sy as f32 + 0.5) / 4.0,
                        };
                        if point_segment_distance(p, a, b) <= radius {
                            inside += 1;
                        }
                    }
                }
                if inside == 0 {
                    continue;
                }
                let alpha = (u32::from(color.a) * inside + 8) / 16;
                commands.push(DrawCommand::FillRect {
                    rect: Rect::new(x, y, 1, 1),
                    color: Color::rgba(color.r, color.g, color.b, alpha as u8),
                });
            }
        }
    }
    surface.draw(&commands)?;
    Ok(())
}

fn parse_segments(svg: &str) -> Result<Vec<Segment>> {
    if !svg.contains("viewBox=\"0 0 24 24\"") {
        return Err("unsupported SVG viewBox".into());
    }
    let mut segments = Vec::new();

    let mut rest = svg;
    while let Some(start) = rest.find("<line") {
        rest = &rest[start..];
        let end = rest.find('>').ok_or("unterminated SVG line")?;
        let tag = &rest[..=end];
        let a = Point {
            x: attr(tag, "x1")?,
            y: attr(tag, "y1")?,
        };
        let b = Point {
            x: attr(tag, "x2")?,
            y: attr(tag, "y2")?,
        };
        segments.push(Segment { a, b });
        rest = &rest[end + 1..];
    }

    let mut rest = svg;
    while let Some(start) = rest.find("<polyline") {
        rest = &rest[start..];
        let end = rest.find('>').ok_or("unterminated SVG polyline")?;
        let tag = &rest[..=end];
        let points = attr_text(tag, "points")?;
        let normalized = points.replace(',', " ");
        let values: Vec<f32> = normalized
            .split_whitespace()
            .map(str::parse)
            .collect::<std::result::Result<_, _>>()?;
        if values.len() < 4 || values.len() % 2 != 0 {
            return Err("invalid SVG polyline coordinate list".into());
        }
        let mut parsed = Vec::new();
        for pair in values.chunks_exact(2) {
            parsed.push(Point {
                x: pair[0],
                y: pair[1],
            });
        }
        if parsed.len() < 2 {
            return Err("SVG polyline requires at least two points".into());
        }
        for pair in parsed.windows(2) {
            segments.push(Segment {
                a: pair[0],
                b: pair[1],
            });
        }
        rest = &rest[end + 1..];
    }

    Ok(segments)
}

fn attr(tag: &str, name: &str) -> Result<f32> {
    Ok(attr_text(tag, name)?.parse()?)
}

fn attr_text<'a>(tag: &'a str, name: &str) -> Result<&'a str> {
    let needle = format!("{name}=\"");
    let start = tag.find(&needle).ok_or("missing SVG attribute")? + needle.len();
    let tail = &tag[start..];
    let end = tail.find('"').ok_or("unterminated SVG attribute")?;
    Ok(&tail[..end])
}

fn point_segment_distance(p: Point, a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let length_sq = dx * dx + dy * dy;
    if length_sq <= f32::EPSILON {
        return ((p.x - a.x).powi(2) + (p.y - a.y).powi(2)).sqrt();
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / length_sq).clamp(0.0, 1.0);
    let x = a.x + t * dx;
    let y = a.y + t * dy;
    ((p.x - x).powi(2) + (p.y - y).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use readall_render::RenderLimits;

    #[test]
    fn feather_assets_parse_into_expected_segments() {
        assert_eq!(parse_segments(CHEVRON_LEFT).unwrap().len(), 2);
        assert_eq!(parse_segments(CHEVRON_RIGHT).unwrap().len(), 2);
        assert_eq!(parse_segments(CHEVRON_UP).unwrap().len(), 2);
        assert_eq!(parse_segments(CHEVRON_DOWN).unwrap().len(), 2);
        assert_eq!(parse_segments(LIST).unwrap().len(), 6);
        assert_eq!(parse_segments(MINUS).unwrap().len(), 1);
        assert_eq!(parse_segments(PLUS).unwrap().len(), 2);
    }

    #[test]
    fn svg_lines_are_antialiased_onto_surface() {
        let mut surface = Surface::new(48, 48, RenderLimits::default()).unwrap();
        draw(
            &mut surface,
            CHEVRON_LEFT,
            Rect::new(12, 12, 24, 24),
            Color::rgba(20, 30, 40, 255),
        )
        .unwrap();
        assert!(surface.pixels().iter().any(|pixel| pixel.a == 255));
        assert!(
            surface
                .pixels()
                .iter()
                .any(|pixel| pixel.a > 0 && pixel.a < 255)
        );
    }
}

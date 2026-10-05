//! Shared high-density UI shapes for the desktop library and both reader hosts.
use super::UiResult;
use readall_render::{Color, DrawCommand, Rect, Surface};

/// Bounded scanline rounded fill; edge coverage is applied once, without full-size scratch images.
pub(crate) fn rounded(
    surface: &mut Surface,
    rect: Rect,
    radius: u32,
    color: Color,
) -> UiResult<()> {
    let bounds = surface.pixel_rect(rect);
    if bounds.width == 0 || bounds.height == 0 {
        return Ok(());
    }
    let scale = surface.pixel_scale();
    let rx = (radius.min(24) as f64 * f64::from(scale.0)).min(f64::from(bounds.width) / 2.0);
    let ry = (radius.min(24) as f64 * f64::from(scale.1)).min(f64::from(bounds.height) / 2.0);
    if rx < 1.0 || ry < 1.0 {
        surface.draw_pixels(&[DrawCommand::FillRect {
            rect: bounds,
            color,
        }])?;
        return Ok(());
    }
    let rows = ry.ceil() as u32;
    let mut commands = Vec::with_capacity((rows * 6 + 1) as usize);
    let center = bounds.height.saturating_sub(rows * 2);
    if center > 0 {
        commands.push(DrawCommand::FillRect {
            rect: Rect::new(bounds.x, bounds.y + rows as i32, bounds.width, center),
            color,
        });
    }
    for row in 0..rows.min(bounds.height.div_ceil(2)) {
        let y = (ry - f64::from(row) - 0.5).max(0.0) / ry;
        let inset =
            (rx * (1.0 - (1.0 - y * y).max(0.0).sqrt())).clamp(0.0, f64::from(bounds.width) / 2.0);
        let edge = inset.floor() as u32;
        let alpha = (f64::from(color.a) * (1.0 - inset.fract())).round() as u8;
        for (side, yy) in [
            bounds.y + row as i32,
            bounds.y + bounds.height as i32 - 1 - row as i32,
        ]
        .into_iter()
        .enumerate()
        {
            if side == 1 && row * 2 + 1 == bounds.height {
                continue;
            }
            let left = bounds.x + edge as i32;
            let right = bounds.x + bounds.width as i32 - 1 - edge as i32;
            if right < left {
                continue;
            }
            commands.push(DrawCommand::FillRect {
                rect: Rect::new(left, yy, 1, 1),
                color: Color { a: alpha, ..color },
            });
            if right > left {
                commands.push(DrawCommand::FillRect {
                    rect: Rect::new(right, yy, 1, 1),
                    color: Color { a: alpha, ..color },
                });
            }
            if right > left + 1 {
                commands.push(DrawCommand::FillRect {
                    rect: Rect::new(left + 1, yy, (right - left - 1) as u32, 1),
                    color,
                });
            }
        }
    }
    surface.draw_pixels(&commands)?;
    Ok(())
}

use crate::{
    Font, FontError, HorizontalMetrics, Result,
    binary::{Cursor, i16_at, reserve, slice},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub on_curve: bool,
}
#[derive(Debug, Default)]
pub struct Outline {
    points: Vec<Point>,
    ends: Vec<usize>,
}
impl Outline {
    pub fn points(&self) -> &[Point] {
        &self.points
    }
    pub fn contour_count(&self) -> usize {
        self.ends.len()
    }
    pub fn contours(&self) -> impl Iterator<Item = &[Point]> {
        let mut start = 0;
        self.ends.iter().map(move |&end| {
            let result = &self.points[start..=end];
            start = end + 1;
            result
        })
    }
}
#[derive(Debug)]
pub struct Glyph {
    pub outline: Outline,
    pub metrics: HorizontalMetrics,
}
struct Budget {
    points: usize,
    contours: usize,
    nodes: usize,
}
fn charge(remaining: &mut usize, count: usize, what: &'static str) -> Result<()> {
    *remaining = remaining
        .checked_sub(count)
        .ok_or(FontError::LimitExceeded(what))?;
    Ok(())
}

pub(crate) fn decode(font: &Font<'_>, glyph: u16) -> Result<Glyph> {
    let mut budget = Budget {
        points: font.limits.max_points,
        contours: font.limits.max_contours,
        nodes: font.limits.max_components,
    };
    let mut stack = Vec::new();
    reserve(&mut stack, font.limits.max_depth)?;
    recurse(font, glyph, &mut budget, &mut stack)
}
fn recurse(
    font: &Font<'_>,
    glyph: u16,
    budget: &mut Budget,
    stack: &mut Vec<u16>,
) -> Result<Glyph> {
    if stack.contains(&glyph) {
        return Err(FontError::Invalid("composite glyph cycle"));
    }
    if stack.len() >= font.limits.max_depth {
        return Err(FontError::LimitExceeded("composite depth"));
    }
    charge(&mut budget.nodes, 1, "component visits")?;
    stack.push(glyph);
    let result = (|| {
        let metrics = font.horizontal_metrics(glyph)?;
        let bytes = font.glyph_bytes(glyph)?;
        if bytes.is_empty() {
            return Ok(Glyph {
                outline: Outline::default(),
                metrics,
            });
        }
        slice(bytes, 0, 10)?;
        let contours = i16_at(bytes, 0)?;
        if contours >= 0 {
            Ok(Glyph {
                outline: simple(bytes, contours as usize, budget)?,
                metrics,
            })
        } else {
            composite(font, bytes, metrics, budget, stack)
        }
    })();
    stack.pop();
    result
}

fn simple(bytes: &[u8], contours: usize, budget: &mut Budget) -> Result<Outline> {
    charge(&mut budget.contours, contours, "contours")?;
    let mut outline = Outline::default();
    if contours == 0 {
        // A zero-contour glyph may end at the header, or contain hinting bytecode.
        if bytes.len() > 10 {
            let mut cursor = Cursor::new(bytes, 10);
            let length = usize::from(cursor.u16()?);
            cursor.take(length)?;
        }
        return Ok(outline);
    }
    let mut cursor = Cursor::new(bytes, 10);
    reserve(&mut outline.ends, contours)?;
    for _ in 0..contours {
        let end = usize::from(cursor.u16()?);
        if outline.ends.last().is_some_and(|previous| end <= *previous) {
            return Err(FontError::Invalid("unordered contour endpoints"));
        }
        outline.ends.push(end);
    }
    let count = outline.ends[contours - 1] + 1;
    charge(&mut budget.points, count, "outline points")?;
    let instruction_length = usize::from(cursor.u16()?);
    cursor.take(instruction_length)?; // Intentionally no TrueType bytecode interpreter.
    let mut flags = Vec::new();
    reserve(&mut flags, count)?;
    while flags.len() < count {
        let flag = cursor.u8()?;
        if flag & 0x80 != 0 {
            return Err(FontError::Invalid("reserved simple glyph flag"));
        }
        let repeats = if flag & 8 != 0 {
            usize::from(cursor.u8()?) + 1
        } else {
            1
        };
        if repeats > count - flags.len() {
            return Err(FontError::Invalid("glyph flag repeat exceeds point count"));
        }
        flags.resize(flags.len() + repeats, flag);
    }
    reserve(&mut outline.points, count)?;
    let mut x = 0_i32;
    for &flag in &flags {
        x = x
            .checked_add(delta(&mut cursor, flag, 2, 16)?)
            .ok_or(FontError::Invalid("glyph x overflow"))?;
        if x.abs() > 1_048_576 {
            return Err(FontError::LimitExceeded("glyph coordinates"));
        }
        outline.points.push(Point {
            x: x as f32,
            y: 0.0,
            on_curve: flag & 1 != 0,
        });
    }
    let mut y = 0_i32;
    for (point, &flag) in outline.points.iter_mut().zip(&flags) {
        y = y
            .checked_add(delta(&mut cursor, flag, 4, 32)?)
            .ok_or(FontError::Invalid("glyph y overflow"))?;
        if y.abs() > 1_048_576 {
            return Err(FontError::LimitExceeded("glyph coordinates"));
        }
        point.y = y as f32;
    }
    Ok(outline)
}
fn delta(cursor: &mut Cursor<'_>, flag: u8, short: u8, same: u8) -> Result<i32> {
    if flag & short != 0 {
        let value = i32::from(cursor.u8()?);
        Ok(if flag & same != 0 { value } else { -value })
    } else if flag & same != 0 {
        Ok(0)
    } else {
        Ok(i32::from(cursor.i16()?))
    }
}

fn composite(
    font: &Font<'_>,
    bytes: &[u8],
    mut metrics: HorizontalMetrics,
    budget: &mut Budget,
    stack: &mut Vec<u16>,
) -> Result<Glyph> {
    let mut cursor = Cursor::new(bytes, 10);
    let mut outline = Outline::default();
    let mut instructions = false;
    let mut metrics_selected = false;
    loop {
        let flags = cursor.u16()?;
        let child_index = cursor.u16()?;
        if flags & 0xe010 != 0 || (flags & 0xc8).count_ones() > 1 || flags & 0x1800 == 0x1800 {
            return Err(FontError::Invalid(
                "conflicting or reserved composite flags",
            ));
        }
        instructions |= flags & 0x100 != 0;
        let words = flags & 1 != 0;
        let xy = flags & 2 != 0;
        let mut arg = || -> Result<i32> {
            if words {
                let value = cursor.u16()?;
                Ok(if xy {
                    i32::from(value as i16)
                } else {
                    i32::from(value)
                })
            } else {
                let value = cursor.u8()?;
                Ok(if xy {
                    i32::from(value as i8)
                } else {
                    i32::from(value)
                })
            }
        };
        let (arg1, arg2) = (arg()?, arg()?);
        let (mut a, mut b, mut c, mut d) = (1.0, 0.0, 0.0, 1.0);
        if flags & 8 != 0 {
            a = cursor.f2dot14()?;
            d = a;
        } else if flags & 0x40 != 0 {
            a = cursor.f2dot14()?;
            d = cursor.f2dot14()?;
        } else if flags & 0x80 != 0 {
            a = cursor.f2dot14()?;
            b = cursor.f2dot14()?;
            c = cursor.f2dot14()?;
            d = cursor.f2dot14()?;
        }
        let mut child = recurse(font, child_index, budget, stack)?;
        if flags & 0x200 != 0 {
            if metrics_selected {
                return Err(FontError::Invalid("multiple USE_MY_METRICS components"));
            }
            if a != 1.0 || b != 0.0 || c != 0.0 || d != 1.0 {
                return Err(FontError::Unsupported("transformed USE_MY_METRICS"));
            }
            metrics = child.metrics;
            metrics_selected = true;
        }
        for point in &mut child.outline.points {
            let (x, y) = (point.x, point.y);
            point.x = a * x + c * y;
            point.y = b * x + d * y;
        }
        let (dx, dy) = if xy {
            let (x, y) = (arg1 as f32, arg2 as f32);
            if flags & 0x800 != 0 {
                (a * x + c * y, b * x + d * y)
            } else {
                (x, y)
            }
        } else {
            let parent_point = outline
                .points
                .get(arg1 as usize)
                .ok_or(FontError::Unsupported(
                    "absent or phantom parent attachment point",
                ))?;
            let child_point =
                child
                    .outline
                    .points
                    .get(arg2 as usize)
                    .ok_or(FontError::Unsupported(
                        "absent or phantom child attachment point",
                    ))?;
            (
                parent_point.x - child_point.x,
                parent_point.y - child_point.y,
            )
        };
        // ROUND_XY_TO_GRID belongs to hinting, deliberately disabled in this unhinted reader.
        for point in &mut child.outline.points {
            point.x += dx;
            point.y += dy;
            if !point.x.is_finite()
                || !point.y.is_finite()
                || point.x.abs() > 1_048_576.0
                || point.y.abs() > 1_048_576.0
            {
                return Err(FontError::LimitExceeded("transformed coordinates"));
            }
        }
        let base = outline.points.len();
        reserve(&mut outline.points, child.outline.points.len())?;
        reserve(&mut outline.ends, child.outline.ends.len())?;
        outline
            .ends
            .extend(child.outline.ends.iter().map(|end| base + end));
        outline.points.extend(child.outline.points);
        if flags & 0x20 == 0 {
            break;
        }
    }
    if instructions {
        let length = usize::from(cursor.u16()?);
        cursor.take(length)?;
    }
    Ok(Glyph { outline, metrics })
}

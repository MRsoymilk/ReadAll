//! Positive CSS block boxes with nested content widths and sliced page decoration.
//! No floats, positioning, margin collapsing or height-constrained overflow.
use super::*;
use readall_epub::css::{BoxLength, BoxStyle};

#[derive(Debug, Clone)]
pub(super) struct BoxPaint {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    border: [f32; 4],
    colors: [[u8; 3]; 4],
    background: Option<[u8; 3]>,
}
#[derive(Debug)]
pub(super) struct OpenBox {
    parent_x: f32,
    parent_width: f32,
    paint: BoxPaint,
    decoration: usize,
    bottom: f32,
    margin_bottom: f32,
    break_after: bool,
}
fn bounded(value: f32, maximum: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, maximum.max(0.0))
    } else {
        0.0
    }
}
impl Builder {
    fn charge_box(&mut self) -> Result<()> {
        self.items += 1;
        if self.items > 200_000 {
            return Err("EPUB layout item budget exceeded".into());
        }
        Ok(())
    }
    pub(super) fn next_page(&mut self, offset: usize) -> Result<()> {
        self.pending_break = false;
        if self.pages.last().is_none_or(|page| page.items.is_empty()) {
            return Ok(());
        }
        if self.pages.len() >= 100_000 {
            return Err("EPUB page count budget exceeded".into());
        }
        let page = self.pages.last_mut().ok_or("missing current page")?;
        for frame in &self.boxes {
            let paint = &mut page.decorations[frame.decoration];
            paint.height = (self.height - paint.y).max(0.0);
            paint.border[2] = 0.0;
        }
        let mut decorations = Vec::new();
        for frame in &mut self.boxes {
            let mut paint = frame.paint.clone();
            paint.y = 0.0;
            paint.height = 0.0;
            paint.border[0] = 0.0;
            paint.border[2] = 0.0;
            frame.decoration = decorations.len();
            decorations.push(paint);
        }
        self.items += decorations.len();
        if self.items > 200_000 {
            return Err("EPUB layout item budget exceeded".into());
        }
        self.pages.push(Page {
            start: offset,
            items: Vec::new(),
            decorations,
        });
        self.y = 0.0;
        Ok(())
    }
    pub(super) fn open_box(&mut self, style: BoxStyle, offset: usize, base: f32) -> Result<()> {
        if self.boxes.len() >= 256 {
            return Err("EPUB block nesting budget exceeded".into());
        }
        if self.pending_break || style.break_before {
            self.next_page(offset)?;
        }
        let parent = self.width;
        let mut margin = style
            .margin
            .map(|n| bounded(n.resolve(base, parent), parent));
        let mut padding = style
            .padding
            .map(|n| bounded(n.resolve(base, parent), parent));
        let mut border = [0.0; 4];
        for (i, n) in border.iter_mut().enumerate() {
            if style.solid[i] {
                *n = bounded(style.border[i].resolve(base, parent), parent);
            }
        }
        // Keep a usable content area even with hostile decoration lengths.
        let minimum = parent.min(base.max(8.0));
        let horizontal = margin[1] + margin[3] + padding[1] + padding[3] + border[1] + border[3];
        let ratio = if horizontal > parent - minimum && horizontal > 0.0 {
            (parent - minimum) / horizontal
        } else {
            1.0
        };
        for i in [1, 3] {
            margin[i] *= ratio;
            padding[i] *= ratio;
            border[i] *= ratio;
        }
        for i in [0, 2] {
            margin[i] = margin[i].min(self.height / 3.0);
            padding[i] = padding[i].min(self.height / 4.0);
            border[i] = border[i].min(self.height / 8.0);
        }
        let available =
            (parent - margin[1] - margin[3] - padding[1] - padding[3] - border[1] - border[3])
                .max(1.0);
        let requested = match style.width {
            BoxLength::Auto => available,
            n => bounded(n.resolve(base, parent), available).max(1.0),
        };
        let maximum = match style.max_width {
            BoxLength::Auto => available,
            n => bounded(n.resolve(base, parent), available).max(1.0),
        };
        let content_width = requested.min(maximum).min(available);
        let remaining = (available - content_width).max(0.0);
        let auto_left = style.margin[3] == BoxLength::Auto;
        let auto_right = style.margin[1] == BoxLength::Auto;
        if auto_left {
            margin[3] += remaining / if auto_right { 2.0 } else { 1.0 };
        }
        let top = margin[0] + border[0] + padding[0];
        if self.y + top >= self.height {
            self.next_page(offset)?;
        }
        self.y = (self.y + margin[0]).min(self.height);
        let paint = BoxPaint {
            x: self.x + margin[3],
            y: self.y,
            width: content_width + padding[1] + padding[3] + border[1] + border[3],
            height: 0.0,
            border,
            colors: style.border_color,
            background: style.background,
        };
        self.charge_box()?;
        let page = self.pages.last_mut().ok_or("missing current page")?;
        let decoration = page.decorations.len();
        page.decorations.push(paint.clone());
        self.boxes.push(OpenBox {
            parent_x: self.x,
            parent_width: parent,
            paint: paint.clone(),
            decoration,
            bottom: padding[2] + border[2],
            margin_bottom: margin[2],
            break_after: style.break_after,
        });
        self.x = paint.x + border[3] + padding[3];
        self.width = content_width;
        self.y = (self.y + border[0] + padding[0]).min(self.height);
        Ok(())
    }
    pub(super) fn close_box(&mut self) -> Result<()> {
        let frame = self.boxes.pop().ok_or("unbalanced EPUB block boxes")?;
        self.y = (self.y + frame.bottom).min(self.height);
        let paint = &mut self
            .pages
            .last_mut()
            .ok_or("missing current page")?
            .decorations[frame.decoration];
        paint.height = (self.y - paint.y).max(0.0);
        paint.border[2] = frame.paint.border[2];
        self.y = (self.y + frame.margin_bottom).min(self.height);
        self.x = frame.parent_x;
        self.width = frame.parent_width;
        self.pending_break |= frame.break_after;
        Ok(())
    }
}
impl BoxPaint {
    pub(super) fn commands(&self, margin: i32, clip: Rect, output: &mut Vec<DrawCommand>) {
        let rect = Rect::new(
            margin + self.x.round() as i32,
            margin + self.y.round() as i32,
            self.width.round().max(0.0) as u32,
            self.height.round().max(0.0) as u32,
        );
        let color = |[r, g, b]: [u8; 3]| Color::rgba(r, g, b, 255);
        if let Some(background) = self.background {
            output.push(DrawCommand::FillRect {
                rect: rect.intersection(clip),
                color: color(background),
            });
        }
        let widths = self.border.map(|width| width.round().max(0.0) as u32);
        let sides = [
            Rect::new(rect.x, rect.y, rect.width, widths[0].min(rect.height)),
            Rect::new(
                rect.x + rect.width.saturating_sub(widths[1]) as i32,
                rect.y,
                widths[1].min(rect.width),
                rect.height,
            ),
            Rect::new(
                rect.x,
                rect.y + rect.height.saturating_sub(widths[2]) as i32,
                rect.width,
                widths[2].min(rect.height),
            ),
            Rect::new(rect.x, rect.y, widths[3].min(rect.width), rect.height),
        ];
        for (i, side) in sides.into_iter().enumerate() {
            if side.width != 0 && side.height != 0 {
                output.push(DrawCommand::FillRect {
                    rect: side.intersection(clip),
                    color: color(self.colors[i]),
                });
            }
        }
    }
}

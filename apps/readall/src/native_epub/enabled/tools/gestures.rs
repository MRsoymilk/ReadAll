//! Body input is selection-first. A press is never a page turn or a link activation.
//! Link/image clicks are resolved on release; crossing the drag threshold cancels
//! activation even when the pointer subsequently returns to its original position.
use super::*;
use readall_platform::window::POINTER_DRAG_THRESHOLD;

pub(super) struct Gesture {
    origin: (i32, i32),
    anchor: Option<Range<usize>>,
    link: Option<readall_epub::ContentLink>,
    image: Option<usize>,
    moved: bool,
}
impl ReaderWindow<'_, '_, '_, '_> {
    pub(super) fn body_press(&mut self, x: i32, y: i32) -> bool {
        self.tools.clear_selection();
        self.tools.status.clear();
        self.pointer = Some((x, y));
        let anchor = self.hit_range(x, y, false);
        let link = (!self.tools.selecting)
            .then(|| self.session.link_at(x, y))
            .flatten();
        let image = (!self.tools.selecting && link.is_none())
            .then(|| {
                self.session
                    .frame()
                    .image_hits
                    .iter()
                    .find(|(rect, _)| point_in(*rect, x, y))
                    .map(|(_, index)| *index)
            })
            .flatten();
        // A normal text click selects its source cluster; links stay unselected
        // until a drag is observed, so an ordinary click remains a link click.
        if link.is_none() && image.is_none() {
            self.tools.selection = anchor.clone();
        }
        if anchor.is_some() || link.is_some() || image.is_some() {
            self.tools.drag = Some(Gesture {
                origin: (x, y),
                anchor,
                link,
                image,
                moved: false,
            });
        }
        true
    }

    pub(super) fn body_motion(&mut self, x: i32, y: i32, released: bool) -> WindowResult<bool> {
        let Some(mut gesture) = self.tools.drag.take() else {
            return Ok(false);
        };
        self.pointer = Some((x, y));
        gesture.moved |= (i64::from(x) - i64::from(gesture.origin.0)).abs()
            >= POINTER_DRAG_THRESHOLD
            || (i64::from(y) - i64::from(gesture.origin.1)).abs() >= POINTER_DRAG_THRESHOLD;
        let before = self.tools.selection.clone();
        if gesture.moved
            && let (Some(anchor), Some(target)) = (&gesture.anchor, self.hit_range(x, y, true))
        {
            self.tools.selection = Some(anchor.start.min(target.start)..anchor.end.max(target.end));
        }
        if !released {
            self.tools.drag = Some(gesture);
            return Ok(before != self.tools.selection);
        }
        // Also classify the final release coordinate: the compositor/mailbox may
        // coalesce motion events, but releasing far from the press is still a drag.
        if !gesture.moved && self.tools.mode == Mode::None {
            if let Some(link) = gesture.link {
                if self.session.link_at(x, y).is_some_and(|target| {
                    target.href == link.href
                        && target.text == link.text
                        && target.images == link.images
                }) {
                    return self.follow_body_link(link);
                }
            } else if let Some(index) = gesture.image
                && self
                    .session
                    .frame()
                    .image_hits
                    .iter()
                    .any(|(rect, i)| *i == index && point_in(*rect, x, y))
                && let Some(image) = self.session.image(index)
            {
                self.tools.zoom = Some(image);
                self.tools.factor = 1.0;
                self.tools.pan = (0, 0);
                self.tools.mode = Mode::Zoom;
                self.tools.status = "+/− 缩放；↑↓/滚轮垂直移动；Home/End 水平移动；Esc 返回".into();
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;

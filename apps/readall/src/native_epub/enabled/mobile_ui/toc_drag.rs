//! Direct manipulation of the TOC viewport, not keyboard selection navigation.
use super::{ReaderWindow, Rect, ToolbarMode, point_in, tools};

// Match the shared TOC row pitch; all coordinates are logical (not bitmap) pixels.
const ROW_HEIGHT: i64 = 38;

pub(super) struct TocDrag {
    last_y: i32,
    remainder: i64,
}
impl TocDrag {
    pub(super) fn start(window: &ReaderWindow<'_, '_, '_, '_>, x: i32, y: i32) -> Option<Self> {
        let panel = window.toc_panel_rect();
        let rows = Rect::new(
            panel.x,
            panel.y + 48,
            panel.width,
            panel.height.saturating_sub(52),
        );
        point_in(rows, x, y).then_some(Self {
            last_y: y,
            remainder: 0,
        })
    }
    pub(super) fn move_to(&mut self, window: &mut ReaderWindow<'_, '_, '_, '_>, y: i32) -> bool {
        if window.toolbar != ToolbarMode::Toc || window.tools.mode != tools::Mode::None {
            return false;
        }
        // Finger up -> larger first-visible index -> the existing rows move up.
        // Do not synthesize Next/Previous: those first walk the highlight down/up.
        self.remainder += i64::from(self.last_y) - i64::from(y);
        self.last_y = y;
        let steps = self.remainder / ROW_HEIGHT;
        self.remainder %= ROW_HEIGHT;
        let visible = window.visible_toc_rows();
        let last = window.toc.len().saturating_sub(visible);
        let before = window.toc_scroll;
        let next = (before as i64 + steps).clamp(0, last as i64) as usize;
        // Discard only outward motion at the ends. Reversing the finger must not
        // pay back overscroll, and a repeated/coalesced MOVE must not drain a backlog.
        if (next == 0 && self.remainder < 0) || (next == last && self.remainder > 0) {
            self.remainder = 0;
        }
        if next == before {
            return false;
        }
        window.toc_scroll = next;
        window.pointer = None;
        // Keep one visible active row without navigating the book or pinning its chapter.
        window.toc_selected = window.toc_selected.clamp(
            next,
            (next + visible).min(window.toc.len()).saturating_sub(1),
        );
        true
    }
}

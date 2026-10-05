//! Direct, continuous TOC manipulation in logical pixels. No row-sized dead zone.
use super::ReaderWindow;
pub(super) struct TocDrag {
    last_y: i32,
}
impl TocDrag {
    pub(super) fn start(window: &ReaderWindow<'_, '_, '_, '_>, x: i32, y: i32) -> Option<Self> {
        super::point_in(window.toc_rows_rect(), x, y).then_some(Self { last_y: y })
    }
    pub(super) fn move_to(&mut self, window: &mut ReaderWindow<'_, '_, '_, '_>, y: i32) -> bool {
        let delta = f64::from(self.last_y) - f64::from(y);
        self.last_y = y;
        window.scroll_toc_pixels(delta)
    }
}

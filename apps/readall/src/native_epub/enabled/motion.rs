//! Time-based page presentation. Left-button text selection is never a swipe.
//! Cached source pages are translated/composited; no text/image raster on a tick.
use super::scroll_physics::Inertia;
use super::*;
use crate::reader_data::PageMode;
use readall_render::PageEffect;
use std::time::Instant;
struct Turn {
    from: Surface,
    to: Surface,
    started: Instant,
    start: f32,
    end: f32,
    backwards: bool,
    effect: PageEffect,
}
impl Turn {
    fn progress(&self, now: Instant) -> f32 {
        let t = (now.saturating_duration_since(self.started).as_secs_f32() / 0.24).clamp(0.0, 1.0);
        self.start + (self.end - self.start) * (1.0 - (1.0 - t).powi(3))
    }
}
#[derive(Default)]
pub(super) struct Motion {
    turn: Option<Turn>,
    pan: Option<Pan>,
    offset: f64,
    target: f64,
    last_tick: Option<Instant>,
    wheel: f64,
    wheel_at: Option<Instant>,
    dirty_progress: bool,
    direct: bool,
    fling: Option<Inertia>,
}
struct Pan {
    start: (i32, i32),
    last: (i32, i32),
}
impl Motion {
    pub(super) fn panning(&self) -> bool {
        self.pan.is_some()
    }
    pub(super) fn active(&self) -> bool {
        self.turn.is_some() || self.fling.is_some() || (self.target - self.offset).abs() > 0.1
    }
    fn preview(&self) -> Option<(i32, f32)> {
        let pan = self.pan.as_ref()?;
        let delta = i64::from(pan.last.0) - i64::from(pan.start.0);
        (delta != 0).then_some((if delta < 0 { 1 } else { -1 }, delta.unsigned_abs() as f32))
    }
}
impl ReaderWindow<'_, '_, '_, '_> {
    pub(super) fn reset_motion(&mut self) -> WindowResult<()> {
        self.motion = Motion::default();
        if self.session.settings().page_mode == PageMode::Scroll {
            self.motion.offset = self.session.scroll_offset_for_anchor();
            self.motion.target = self.motion.offset;
            self.session.compose_scroll(self.motion.offset)?;
        } else {
            self.session.clear_scroll_view();
        }
        Ok(())
    }
    pub(super) fn freeze_motion(&mut self) -> WindowResult<()> {
        self.motion.turn = None;
        self.motion.pan = None;
        self.motion.fling = None;
        self.motion.direct = false;
        self.motion.target = self.motion.offset;
        if self.session.settings().page_mode == PageMode::Scroll {
            self.session.remember_scroll_position()?;
            if self.motion.dirty_progress {
                self.save_progress();
                self.motion.dirty_progress = false;
            }
        }
        Ok(())
    }
    pub(super) fn draw_page_motion(&mut self) -> WindowResult<()> {
        let clip = Rect::new(
            0,
            32,
            self.surface.width(),
            self.surface.height().saturating_sub(36),
        );
        let paper = self.session.settings().theme.colors().0;
        if let Some(turn) = &self.motion.turn {
            self.surface.page_transition(
                (&turn.from, &turn.to),
                clip,
                turn.progress(Instant::now()),
                turn.backwards,
                turn.effect,
                paper,
            )?;
        } else if self.session.settings().page_mode != PageMode::Scroll
            && let Some((direction, distance)) = self.motion.preview()
            && let Some(next) = self.session.neighbour_surface(direction)
        {
            let effect = if self.session.settings().page_mode == PageMode::Book {
                PageEffect::Book
            } else {
                PageEffect::Slide
            };
            self.surface.page_transition(
                (self.session.raw_surface(), next),
                clip,
                (distance / self.surface.width() as f32).min(0.98),
                direction < 0,
                effect,
                paper,
            )?;
        }
        Ok(())
    }
    pub(super) fn animated_page_action(&mut self, action: ReaderAction) -> WindowResult<bool> {
        let direction = match action {
            ReaderAction::Next => 1,
            ReaderAction::Previous => -1,
            _ => {
                self.freeze_motion()?;
                let changed = self.perform_reader_action_instant(action)?;
                self.reset_motion()?;
                self.refresh_surface()?;
                return Ok(changed);
            }
        };
        if self.session.settings().page_mode == PageMode::Scroll {
            return self.scroll_request(f64::from(direction) * self.session.scroll_stride() * 0.85);
        }
        self.turn_page(direction, 0.0)
    }
    fn turn_page(&mut self, direction: i32, start: f32) -> WindowResult<bool> {
        self.motion.turn = None;
        let from = self.session.raw_surface().clone();
        match self.session.turn_cached(direction) {
            Ok(true) => {
                self.tools.clear_selection();
                self.sync_toc_selection();
                self.save_progress();
                self.motion.turn = Some(Turn {
                    from,
                    to: self.session.raw_surface().clone(),
                    started: Instant::now(),
                    start,
                    end: 1.0,
                    backwards: direction < 0,
                    effect: if self.session.settings().page_mode == PageMode::Book {
                        PageEffect::Book
                    } else {
                        PageEffect::Slide
                    },
                });
                self.refresh_surface()?;
                Ok(true)
            }
            Ok(false) => {
                self.refresh_surface()?;
                Ok(false)
            }
            Err(error) => {
                self.tools.status = format!("页面未切换：{error}");
                self.refresh_surface()?;
                Ok(true)
            }
        }
    }
    #[cfg(feature = "mobile")]
    pub(super) fn start_touch_fling(&mut self, displacement: i32) -> bool {
        if self.session.settings().page_mode != PageMode::Scroll {
            return false;
        }
        self.motion.fling = Inertia::from_displacement(displacement, Instant::now());
        self.motion.direct = true;
        self.motion.fling.is_some()
    }
    fn scroll_request(&mut self, delta: f64) -> WindowResult<bool> {
        if !delta.is_finite() || delta.abs() < 0.001 || self.tools.dragging() {
            return Ok(false);
        }
        let direction = if delta > 0.0 { 1 } else { -1 };
        // Speculate one page, never the entire book; glyph/image work is reused
        // afterwards for every sub-page scroll step.
        if let Err(error) = self.session.prepare_neighbour(direction) {
            self.session.skip_prefetch(direction);
            self.tools.status = format!("相邻页无法加载：{error}");
        }
        self.tools.clear_selection();
        let stride = self.session.scroll_stride();
        self.motion.target = (self.motion.target + delta).clamp(
            self.motion.offset - stride * 3.0,
            self.motion.offset + stride * 3.0,
        );
        if !self.session.neighbour_ready(-1) && self.session.neighbour_checked(-1) {
            self.motion.target = self.motion.target.max(0.0);
        }
        if !self.session.neighbour_ready(1) && self.session.neighbour_checked(1) {
            self.motion.target = self.motion.target.min(0.0);
        }
        self.motion.last_tick.get_or_insert_with(Instant::now);
        self.motion.dirty_progress = true;
        Ok(true)
    }
    /// Return Some when input belongs to motion rather than overlays/text selection.
    pub(super) fn motion_action(&mut self, action: Action) -> WindowResult<Option<bool>> {
        let unobstructed = self.tools.mode == tools::Mode::None && self.toolbar != ToolbarMode::Toc;
        match action {
            Action::Close if self.motion.active() || self.motion.pan.is_some() => {
                self.freeze_motion()?;
                Ok(Some(true))
            }
            Action::Scroll { dx, dy } => {
                self.motion.fling = None;
                self.motion.direct = false;
                if self.tools.dragging() {
                    return Ok(Some(false));
                }
                if !self.mobile_chrome()
                    && self.toolbar == ToolbarMode::Toc
                    && self.tools.mode == tools::Mode::None
                {
                    if self
                        .pointer
                        .is_some_and(|(x, y)| !point_in(self.toc_panel_rect(), x, y))
                    {
                        return Ok(Some(false));
                    }
                    return Ok(Some(self.scroll_toc_pixels(f64::from(dy) / 256.0)));
                }
                if !unobstructed {
                    let delta = if dy != 0 { dy } else { dx };
                    if delta == 0 {
                        return Ok(Some(false));
                    }
                    return self
                        .action(if delta > 0 {
                            Action::Next
                        } else {
                            Action::Previous
                        })
                        .map(Some);
                }
                if self.session.settings().page_mode == PageMode::Scroll {
                    return self.scroll_request(f64::from(dy) / 256.0).map(Some);
                }
                let raw = if dx.unsigned_abs() > dy.unsigned_abs() {
                    dx
                } else {
                    dy
                };
                let delta = f64::from(raw) / 256.0;
                let now = Instant::now();
                if self
                    .motion
                    .wheel_at
                    .is_some_and(|at| now.duration_since(at) > Duration::from_millis(350))
                    || self.motion.wheel.signum() != delta.signum()
                {
                    self.motion.wheel = 0.0;
                }
                self.motion.wheel += delta;
                if self.motion.wheel.abs() < 48.0 {
                    return Ok(Some(false));
                }
                if self
                    .motion
                    .wheel_at
                    .is_some_and(|at| now.duration_since(at) < Duration::from_millis(180))
                {
                    self.motion.wheel = 0.0;
                    return Ok(Some(false));
                }
                let direction = if self.motion.wheel > 0.0 { 1 } else { -1 };
                self.motion.wheel = 0.0;
                self.motion.wheel_at = Some(now);
                self.turn_page(direction, 0.0).map(Some)
            }
            Action::PanStart { x, y } => {
                if !unobstructed
                    || y < 32
                    || (self.toolbar != ToolbarMode::Collapsed
                        && point_in(self.toolbar_rect(), x, y))
                    || point_in(self.collapsed_rect(), x, y)
                {
                    return Ok(Some(false));
                }
                self.freeze_motion()?;
                self.tools.clear_selection();
                self.motion.direct = true;
                self.motion.pan = Some(Pan {
                    start: (x, y),
                    last: (x, y),
                });
                Ok(Some(true))
            }
            Action::PointerMove { x, y } if self.motion.pan.is_some() => {
                let pan = self.motion.pan.as_mut().unwrap();
                let old = pan.last;
                pan.last = (x, y);
                if self.session.settings().page_mode == PageMode::Scroll {
                    self.scroll_request(f64::from(old.1) - f64::from(y))?;
                } else {
                    let direction = if x < pan.start.0 { 1 } else { -1 };
                    if let Err(error) = self.session.prepare_neighbour(direction) {
                        self.tools.status = format!("相邻页无法加载：{error}");
                        self.session.skip_prefetch(direction);
                    }
                }
                Ok(Some(true))
            }
            Action::PanEnd { x, y } => {
                let Some(pan) = self.motion.pan.take() else {
                    return Ok(Some(false));
                };
                if self.session.settings().page_mode == PageMode::Scroll {
                    self.scroll_request(f64::from(pan.last.1) - f64::from(y))?;
                    return Ok(Some(true));
                }
                let dx = i64::from(x) - i64::from(pan.start.0);
                let direction = if dx < 0 { 1 } else { -1 };
                let distance = dx.unsigned_abs() as f32;
                let phase = (distance / self.surface.width() as f32).min(0.98);
                if distance >= 48.0 {
                    return self.turn_page(direction, phase).map(Some);
                }
                if let Some(to) = self.session.neighbour_surface(direction) {
                    self.motion.turn = Some(Turn {
                        from: self.session.raw_surface().clone(),
                        to: to.clone(),
                        started: Instant::now(),
                        start: phase,
                        end: 0.0,
                        backwards: direction < 0,
                        effect: if self.session.settings().page_mode == PageMode::Book {
                            PageEffect::Book
                        } else {
                            PageEffect::Slide
                        },
                    });
                }
                Ok(Some(true))
            }
            Action::PointerLeave if self.motion.pan.is_some() => {
                self.motion.pan = None;
                self.freeze_motion()?;
                Ok(Some(true))
            }
            _ => {
                if matches!(
                    action,
                    Action::Click { .. } | Action::Command(_) | Action::Back | Action::Close
                ) {
                    self.freeze_motion()?;
                }
                if let Action::Click { x, y } = action
                    && unobstructed
                    && self.session.settings().page_mode == PageMode::Scroll
                    && y >= 32
                    && !(self.toolbar != ToolbarMode::Collapsed
                        && point_in(self.toolbar_rect(), x, y))
                    && !point_in(self.collapsed_rect(), x, y)
                {
                    let shift = self.session.activate_scroll_slice(y)?;
                    self.motion.offset -= shift;
                    self.motion.target -= shift;
                }
                Ok(None)
            }
        }
    }
    pub(super) fn tick_page_motion(&mut self, now: Instant) -> WindowResult<bool> {
        let mut changed = false;
        if let Some(turn) = &self.motion.turn {
            if now.saturating_duration_since(turn.started) >= Duration::from_millis(240) {
                self.motion.turn = None;
            }
            changed = true;
        }
        if let Some(fling) = &mut self.motion.fling {
            let delta = fling.step(now);
            let active = fling.active();
            let before = self.motion.target;
            self.scroll_request(delta)?;
            if !active || (delta.abs() > 0.001 && (self.motion.target - before).abs() < 0.001) {
                self.motion.fling = None;
                changed = true; // Deliver the final idle state, including at book boundaries.
            }
        }
        if self.session.settings().page_mode == PageMode::Scroll
            && (self.motion.target - self.motion.offset).abs() > 0.1
        {
            let dt = self
                .motion
                .last_tick
                .map_or(0.016, |at| now.saturating_duration_since(at).as_secs_f64())
                .clamp(0.0, 0.05);
            // Direct manipulation must track the finger, not chase it through a
            // second easing filter. Wheel/key scrolling keeps its existing smoothing.
            let next = if self.motion.direct {
                self.motion.target
            } else {
                self.motion.offset
                    + (self.motion.target - self.motion.offset) * (1.0 - (-dt / 0.065).exp())
            };
            self.motion.offset = if (self.motion.target - next).abs() < 0.1 {
                self.motion.target
            } else {
                next
            };
            self.session
                .normalize_scroll(&mut self.motion.offset, &mut self.motion.target)?;
            // A new origin requires at most one visible-neighbour render. Otherwise
            // the entire tick is a row copy of the already-rendered strip.
            for direction in [-1, 1] {
                if !self.session.neighbour_checked(direction)
                    && let Err(error) = self.session.prepare_neighbour(direction)
                {
                    self.session.skip_prefetch(direction);
                    self.tools.status = format!("相邻页无法加载：{error}");
                }
            }
            self.session.compose_scroll(self.motion.offset)?;
            self.sync_toc_selection();
            changed = true;
        }
        self.motion.last_tick = Some(now);
        if self.motion.pan.is_none()
            && self.motion.fling.is_none()
            && (self.motion.target - self.motion.offset).abs() <= 0.1
        {
            self.motion.direct = false;
        }
        if self.motion.dirty_progress && !self.motion.active() && self.motion.pan.is_none() {
            self.session.remember_scroll_position()?;
            self.save_progress();
            self.motion.dirty_progress = false;
        }
        Ok(changed)
    }
    pub(super) fn prefetch_page(&mut self) -> WindowResult<bool> {
        if self.motion.active()
            || self.motion.pan.is_some()
            || self.tools.dragging()
            || self.tools.mode != tools::Mode::None
            || self.toolbar == ToolbarMode::Toc
        {
            return Ok(false);
        }
        let Some(direction) = [1, -1]
            .into_iter()
            .find(|d| !self.session.neighbour_checked(*d))
        else {
            return Ok(false);
        };
        if let Err(error) = self.session.prepare_neighbour(direction) {
            if crate::loading::preempted(error.as_ref()) {
                return Ok(false);
            }
            self.session.skip_prefetch(direction);
            eprintln!("ReadAll: adjacent-page prefetch skipped: {error}");
            return Ok(false);
        }
        if self.session.settings().page_mode == PageMode::Scroll {
            self.session.compose_scroll(self.motion.offset)?;
            self.refresh_surface()?;
            return Ok(true);
        }
        Ok(false)
    }
}
#[cfg(test)]
mod benchmark;
#[cfg(test)]
mod tests;

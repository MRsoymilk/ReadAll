//! Coalesce motion without erasing evidence that a held pointer became a drag.
use super::Action;
pub const POINTER_DRAG_THRESHOLD: i64 = 4;

#[derive(Default)]
pub struct MotionCoalescer {
    origin: Option<(i32, i32)>,
    crossed: bool,
    protect_tail: bool,
}
impl MotionCoalescer {
    pub fn pressed(&self) -> bool {
        self.origin.is_some()
    }
    pub fn may_replace(&self, previous: Option<Action>, next: Action) -> bool {
        !self.protect_tail
            && matches!(previous, Some(Action::PointerMove { .. }))
            && matches!(next, Action::PointerMove { .. })
    }
    /// Call only after accepting the event into the ordered queue.
    pub fn accepted(&mut self, action: Action) {
        self.protect_tail = false;
        match action {
            Action::Click { x, y } => {
                self.origin = Some((x, y));
                self.crossed = false;
            }
            Action::PointerRelease { .. } | Action::PointerLeave => {
                self.origin = None;
                self.crossed = false;
            }
            Action::PointerMove { x, y } if !self.crossed => {
                if let Some((ox, oy)) = self.origin
                    && ((i64::from(x) - i64::from(ox)).abs() >= POINTER_DRAG_THRESHOLD
                        || (i64::from(y) - i64::from(oy)).abs() >= POINTER_DRAG_THRESHOLD)
                {
                    self.crossed = true;
                    self.protect_tail = true;
                }
            }
            _ => {}
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excursion_and_return_survive_both_native_and_worker_coalescing() {
        fn queued(events: Vec<Action>) -> Vec<Action> {
            let mut state = MotionCoalescer::default();
            let mut queue = Vec::new();
            for event in events {
                if state.may_replace(queue.last().copied(), event) {
                    queue.pop();
                }
                queue.push(event);
                state.accepted(event);
            }
            queue
        }
        let mut events = vec![Action::Click { x: 0, y: 0 }];
        for x in 1..50 {
            events.push(Action::PointerMove { x, y: 0 });
        }
        for x in (0..50).rev() {
            events.push(Action::PointerMove { x, y: 0 });
        }
        events.push(Action::PointerRelease { x: 0, y: 0 });
        let result = queued(queued(events));
        assert_eq!(result.len(), 4);
        assert!(matches!(result[1], Action::PointerMove { x: 4, y: 0 }));
        assert!(matches!(result[2], Action::PointerMove { x: 0, y: 0 }));
        assert!(matches!(result[3], Action::PointerRelease { .. }));
    }
    #[test]
    fn hover_stays_compact_and_leave_is_a_boundary() {
        let mut state = MotionCoalescer::default();
        let motion = Action::PointerMove { x: 30, y: 40 };
        state.accepted(motion);
        assert!(state.may_replace(Some(motion), motion));
        state.accepted(Action::Click { x: 0, y: 0 });
        assert!(state.pressed());
        assert!(!state.may_replace(Some(motion), Action::PointerLeave));
        state.accepted(Action::PointerLeave);
        assert!(!state.pressed());
    }
}

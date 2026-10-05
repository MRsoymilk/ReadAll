//! Coalesce dirty input into a bounded display cadence, not one raster per MOVE.
use std::time::{Duration, Instant};
pub(super) const FRAME_INTERVAL: Duration = Duration::from_millis(16);
pub(super) struct FrameClock {
    last: Instant,
    dirty: bool,
}
impl FrameClock {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            last: now,
            dirty: false,
        }
    }
    pub(super) fn mark(&mut self, changed: bool) {
        self.dirty |= changed;
    }
    pub(super) fn pending(&self) -> bool {
        self.dirty
    }
    pub(super) fn delay(&self, now: Instant) -> Option<Duration> {
        self.dirty
            .then(|| FRAME_INTERVAL.saturating_sub(now.saturating_duration_since(self.last)))
    }
    pub(super) fn due(&self, now: Instant, urgent: bool) -> bool {
        self.dirty && (urgent || now.saturating_duration_since(self.last) >= FRAME_INTERVAL)
    }
    pub(super) fn presented(&mut self, frame_started: Instant) {
        self.last = frame_started;
        self.dirty = false;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_bursts_share_one_frame_without_losing_the_last_update() {
        let at = Instant::now();
        let mut clock = FrameClock::new(at);
        for _ in 0..240 {
            clock.mark(true);
            assert!(!clock.due(at + Duration::from_millis(8), false));
        }
        assert_eq!(
            clock.delay(at + Duration::from_millis(8)),
            Some(Duration::from_millis(8))
        );
        assert!(clock.due(at + FRAME_INTERVAL, false));
        clock.presented(at + FRAME_INTERVAL);
        assert!(!clock.pending());
        assert_eq!(clock.delay(at + FRAME_INTERVAL), None);
        clock.mark(false);
        assert!(!clock.pending());
        clock.mark(true);
        assert!(clock.due(at + FRAME_INTERVAL, true));
        // Five ms spent painting are part of the next interval, not extra latency.
        assert_eq!(
            clock.delay(at + Duration::from_millis(21)),
            Some(Duration::from_millis(11))
        );
        assert!(clock.due(at + Duration::from_millis(32), false));
    }
}

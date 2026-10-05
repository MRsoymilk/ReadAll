//! Time-based, bounded inertial displacement. No frame-count-dependent animation.
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;
const DECAY_SECONDS: f64 = 0.22;
#[derive(Debug)]
pub(super) struct Inertia {
    velocity: f64,
    last: Instant,
}
impl Inertia {
    /// Android currently supplies velocity * 0.15 in logical units. Keep this wire
    /// contract, but integrate velocity over time instead of jumping to an endpoint.
    pub(super) fn from_displacement(displacement: i32, now: Instant) -> Option<Self> {
        let velocity = (f64::from(displacement.clamp(-2048, 2048)) / 0.15).clamp(-6000.0, 6000.0);
        (velocity.abs() >= 40.0).then_some(Self {
            velocity,
            last: now,
        })
    }
    pub(super) fn step(&mut self, now: Instant) -> f64 {
        let dt = now
            .saturating_duration_since(self.last)
            .as_secs_f64()
            .min(0.05);
        self.last = now;
        let decay = (-dt / DECAY_SECONDS).exp();
        let delta = self.velocity * DECAY_SECONDS * (1.0 - decay);
        self.velocity *= decay;
        delta
    }
    pub(super) fn active(&self) -> bool {
        self.velocity.abs() >= 5.0
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inertia_is_monotonic_bounded_and_independent_of_frame_rate() {
        let start = Instant::now();
        let run = |step: u64, backwards: bool| {
            let mut motion =
                Inertia::from_displacement(if backwards { -150 } else { 150 }, start).unwrap();
            let mut distance = 0.0;
            for ms in (step..=2000).step_by(step as usize) {
                let delta = motion.step(start + Duration::from_millis(ms));
                assert_eq!(delta.is_sign_negative(), backwards);
                distance += delta;
            }
            assert!(!motion.active());
            assert!(distance.abs() <= 220.0);
            distance
        };
        assert!((run(10, false) - run(20, false)).abs() < 0.001);
        assert!((run(10, false) + run(10, true)).abs() < 0.001);
        assert!(Inertia::from_displacement(0, start).is_none());
    }
    #[test]
    fn a_stalled_frame_cannot_teleport_an_entire_fling() {
        let start = Instant::now();
        let mut motion = Inertia::from_displacement(150, start).unwrap();
        assert_eq!(motion.step(start), 0.0);
        assert!(motion.step(start + Duration::from_secs(10)) < 50.0);
    }
}

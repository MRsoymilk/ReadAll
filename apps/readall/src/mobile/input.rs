//! Bounded ordered input with coalescing only between adjacent compatible events.
//! Gesture ends/cancels have reserved space; an overloaded UI must never leave a drag held.
use super::*;
use std::{collections::VecDeque, sync::Condvar};
#[derive(Default)]
pub(super) struct Inbox {
    queue: Mutex<VecDeque<Command>>,
    wake: Condvar,
}
fn same_direction(before: i32, current: i32, next: i32) -> bool {
    let first = i64::from(current) - i64::from(before);
    let second = i64::from(next) - i64::from(current);
    first == 0 || second == 0 || first.signum() == second.signum()
}
fn replaceable(queue: &VecDeque<Command>, new: &Command) -> bool {
    let Some(old) = queue.back() else {
        return false;
    };
    match (old, new) {
        (Command::Resize { .. }, Command::Resize { .. })
        | (Command::Viewport { .. }, Command::Viewport { .. }) => true,
        (Command::Input { mode: a, .. }, Command::Input { mode: b, .. }) => a == b,
        (
            Command::Touch { kind: 3, x, y },
            Command::Touch {
                kind: 3,
                x: nx,
                y: ny,
            },
        ) => {
            // Keep turning points: clamped list dragging is path-dependent at its
            // ends. Dropping an outward excursion also drops the subsequent reversal.
            // If the worker consumed our predecessor, keep this sample conservatively.
            queue.iter().rev().nth(1).is_some_and(|previous| {
                matches!(previous, Command::Touch { kind: 2 | 3, x: px, y: py }
                    if same_direction(*px, *x, *nx) && same_direction(*py, *y, *ny))
            })
        }
        (Command::Touch { kind: 6, .. }, Command::Touch { kind: 6, .. }) => true,
        _ => false,
    }
}
impl Inbox {
    pub(super) fn push(&self, command: Command) -> io::Result<()> {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if replaceable(&queue, &command) {
            *queue.back_mut().unwrap() = command;
        } else {
            let terminal = matches!(
                command,
                Command::Touch {
                    kind: 4 | 7 | 8,
                    ..
                } | Command::Pause(_)
                    | Command::Back
            );
            if !terminal && queue.len() >= 60 {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "reader input queue is full",
                ));
            }
            if terminal && queue.len() >= 64 {
                queue.retain(|old| {
                    !matches!(
                        old,
                        Command::Touch { .. } | Command::Pause(_) | Command::Back
                    )
                });
            }
            queue.push_back(command);
        }
        self.wake.notify_one();
        Ok(())
    }
    pub(super) fn receive(&self, timeout: Duration) -> Option<Command> {
        let queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let mut queue = if queue.is_empty() {
            self.wake
                .wait_timeout(queue, timeout)
                .unwrap_or_else(|e| e.into_inner())
                .0
        } else {
            queue
        };
        queue.pop_front()
    }
    pub(super) fn interrupt_idle(&self, flag: &std::sync::atomic::AtomicBool) -> bool {
        let queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if !queue.is_empty() {
            return false;
        }
        flag.store(false, Ordering::Release);
        true
    }
    pub(super) fn wake(&self) {
        self.wake.notify_one();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moves_merge_but_starts_ends_and_cancel_keep_order() {
        let inbox = Inbox::default();
        inbox
            .push(Command::Touch {
                kind: 2,
                x: 0,
                y: 0,
            })
            .unwrap();
        for n in 0..1000 {
            inbox
                .push(Command::Touch {
                    kind: 3,
                    x: n,
                    y: n,
                })
                .unwrap();
        }
        inbox
            .push(Command::Touch {
                kind: 4,
                x: 999,
                y: 999,
            })
            .unwrap();
        let q = inbox.queue.lock().unwrap();
        assert_eq!(q.len(), 3);
        assert!(matches!(q[0], Command::Touch { kind: 2, .. }));
        assert!(matches!(
            q[1],
            Command::Touch {
                kind: 3,
                x: 999,
                ..
            }
        ));
        assert!(matches!(q[2], Command::Touch { kind: 4, .. }));
    }
    #[test]
    fn coalesced_pan_retains_reversals_with_and_without_a_queued_start() {
        for consume_start in [false, true] {
            let inbox = Inbox::default();
            inbox
                .push(Command::Touch {
                    kind: 2,
                    x: 200,
                    y: 260,
                })
                .unwrap();
            if consume_start {
                assert!(matches!(
                    inbox.receive(Duration::ZERO),
                    Some(Command::Touch { kind: 2, .. })
                ));
            }
            for y in [360, 460, 440, 422, 422] {
                inbox.push(Command::Touch { kind: 3, x: 200, y }).unwrap();
            }
            inbox
                .push(Command::Touch {
                    kind: 4,
                    x: 200,
                    y: 422,
                })
                .unwrap();
            let q = inbox.queue.lock().unwrap();
            let positions: Vec<_> = q
                .iter()
                .filter_map(|c| match c {
                    Command::Touch { kind: 3, y, .. } => Some(*y),
                    _ => None,
                })
                .collect();
            assert!(
                positions.windows(2).any(|p| p == [460, 422]),
                "turning point lost: {positions:?}"
            );
            assert!(
                positions.len() <= 3,
                "same-direction moves must still coalesce"
            );
            assert!(matches!(q.back(), Some(Command::Touch { kind: 4, .. })));
        }
    }
    #[test]
    fn saturated_input_always_accepts_gesture_cleanup_without_unbounded_growth() {
        let inbox = Inbox::default();
        for _ in 0..60 {
            inbox.push(Command::Next).unwrap();
        }
        assert!(inbox.push(Command::Next).is_err());
        for _ in 0..100 {
            inbox
                .push(Command::Touch {
                    kind: 8,
                    x: 0,
                    y: 0,
                })
                .unwrap();
        }
        let q = inbox.queue.lock().unwrap();
        assert!(q.len() <= 64);
        assert!(matches!(q.back(), Some(Command::Touch { kind: 8, .. })));
    }
}

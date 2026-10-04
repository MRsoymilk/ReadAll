//! Bounded ordered input with coalescing only between adjacent compatible events.
//! Gesture ends/cancels have reserved space; an overloaded UI must never leave a drag held.
use super::*;
use std::{collections::VecDeque, sync::Condvar};
#[derive(Default)]
pub(super) struct Inbox {
    queue: Mutex<VecDeque<Command>>,
    wake: Condvar,
}
fn replaceable(old: &Command, new: &Command) -> bool {
    match (old, new) {
        (Command::Resize { .. }, Command::Resize { .. }) => true,
        (Command::Input { mode: a, .. }, Command::Input { mode: b, .. }) => a == b,
        (Command::Touch { kind: a, .. }, Command::Touch { kind: b, .. }) => {
            a == b && matches!(a, 3 | 6)
        }
        _ => false,
    }
}
impl Inbox {
    pub(super) fn push(&self, command: Command) -> io::Result<()> {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if queue.back().is_some_and(|old| replaceable(old, &command)) {
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

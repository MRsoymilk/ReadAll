//! Per-worker progress and cooperative cancellation. No global book state and no
//! timer-generated percentages: fractions describe work in the current phase.
use std::{
    cell::RefCell,
    error::Error,
    fmt, io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[derive(Debug, Clone)]
pub(crate) struct Progress {
    pub phase: &'static str,
    pub done: usize,
    pub total: usize,
    pub started: Instant,
}
#[derive(Clone)]
pub(crate) struct Tracker {
    progress: Arc<Mutex<Progress>>,
    cancelled: Arc<AtomicBool>,
}
impl Default for Tracker {
    fn default() -> Self {
        Self {
            progress: Arc::new(Mutex::new(Progress {
                phase: "准备打开",
                done: 0,
                total: 0,
                started: Instant::now(),
            })),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl Tracker {
    pub fn snapshot(&self) -> Progress {
        self.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn install(&self) -> Guard {
        let old = CURRENT.with(|slot| slot.replace(Some(self.clone())));
        Guard(old)
    }
}
pub(crate) struct Guard(Option<Tracker>);
impl Drop for Guard {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}
thread_local! { static CURRENT: RefCell<Option<Tracker>> = const { RefCell::new(None) }; }
#[derive(Debug)]
struct Cancelled;
impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("加载已取消")
    }
}
impl Error for Cancelled {}
pub(crate) fn check() -> Result<()> {
    CURRENT.with(|slot| {
        if slot.borrow().as_ref().is_some_and(Tracker::is_cancelled) {
            Err(Box::new(Cancelled) as Box<dyn Error>)
        } else {
            Ok(())
        }
    })
}
pub(crate) fn step(phase: &'static str, done: usize, total: usize) -> Result<()> {
    check()?;
    CURRENT.with(|slot| {
        if let Some(tracker) = slot.borrow().as_ref() {
            let mut state = tracker.progress.lock().unwrap_or_else(|e| e.into_inner());
            if state.phase != phase || done < state.done {
                state.started = Instant::now();
            }
            state.phase = phase;
            state.done = done.min(total);
            state.total = total;
        }
    });
    Ok(())
}
pub(crate) fn stage(phase: &'static str) -> Result<()> {
    step(phase, 0, 0)
}

/// Keep the existing bounded source reader's truncation/growth checks intact.
pub(crate) fn read(
    source: &mut impl readall_core::DocumentSource,
    limit: usize,
    phase: &'static str,
) -> Result<Vec<u8>> {
    struct Observed<'a, S> {
        source: &'a mut S,
        total: usize,
        phase: &'static str,
    }
    impl<S: readall_core::DocumentSource> readall_core::DocumentSource for Observed<'_, S> {
        fn length(&self) -> io::Result<u64> {
            self.source.length()
        }
        fn read_at(&mut self, offset: u64, destination: &mut [u8]) -> io::Result<usize> {
            // Do not use Interrupted: read_bounded retries that error by design.
            check().map_err(|e| io::Error::other(e.to_string()))?;
            let n = self.source.read_at(offset, destination)?;
            step(self.phase, (offset as usize).saturating_add(n), self.total)
                .map_err(|e| io::Error::other(e.to_string()))?;
            Ok(n)
        }
    }
    let total = usize::try_from(source.length()?).unwrap_or(usize::MAX);
    step(phase, 0, total)?;
    Ok(readall_core::read_bounded(
        &mut Observed {
            source,
            total,
            phase,
        },
        limit,
    )?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_is_phase_local_and_cancellation_does_not_leak_between_workers() {
        let tracker = Tracker::default();
        {
            let _guard = tracker.install();
            step("读取", 20, 100).unwrap();
            assert_eq!(tracker.snapshot().done, 20);
            stage("解析").unwrap();
            assert_eq!(tracker.snapshot().total, 0);
            tracker.cancel();
            assert!(check().is_err());
        }
        assert!(check().is_ok());
        let other = Tracker::default();
        let _guard = other.install();
        assert!(check().is_ok());
    }
    #[test]
    fn observed_read_keeps_bounds_and_cancel_semantics() {
        let t = Tracker::default();
        let _guard = t.install();
        let mut source = readall_core::BytesSource(b"abc");
        assert_eq!(read(&mut source, 3, "读取").unwrap(), b"abc");
        assert_eq!(t.snapshot().done, 3);
        assert!(read(&mut source, 2, "读取").is_err());
        t.cancel();
        assert!(read(&mut source, 3, "读取").is_err());
    }
}

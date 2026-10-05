//! One bounded metadata worker. Pointer movement and painting never open a book.
use super::*;
use std::{
    collections::VecDeque,
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    time::{Duration, Instant},
};

pub(super) struct Preview {
    sender: SyncSender<PathBuf>,
    results: Receiver<(PathBuf, String)>,
    selected: Option<PathBuf>,
    due: Option<Instant>,
    running: bool,
    cache: VecDeque<(PathBuf, String)>,
}
impl Preview {
    pub fn new() -> WindowResult<Self> {
        let (sender, work) = mpsc::sync_channel::<PathBuf>(1);
        let (done, results) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("readall-library-preview".into())
            .spawn(move || {
                while let Ok(path) = work.recv() {
                    let text = epub_preview(&path);
                    if done.send((path, text)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender,
            results,
            selected: None,
            due: None,
            running: false,
            cache: VecDeque::new(),
        })
    }
    pub fn select(&mut self, path: Option<PathBuf>, now: Instant) {
        if self.selected == path {
            return;
        }
        self.selected = path;
        self.due = self
            .selected
            .as_ref()
            .map(|_| now + Duration::from_millis(140));
    }
    pub fn pending(&self) -> bool {
        self.running || self.due.is_some()
    }
    pub fn poll(&mut self, now: Instant) -> Option<String> {
        match self.results.try_recv() {
            Ok(row) => {
                self.running = false;
                self.cache.retain(|(path, _)| path != &row.0);
                self.cache.push_back(row);
                while self.cache.len() > 16 {
                    self.cache.pop_front();
                }
            }
            Err(TryRecvError::Disconnected) => {
                self.running = false;
                self.due = None;
                return self
                    .selected
                    .as_ref()
                    .map(|_| "元数据预览不可用，仍可尝试打开".into());
            }
            Err(TryRecvError::Empty) => {}
        }
        let path = self.selected.as_ref()?;
        if let Some((_, text)) = self.cache.iter().find(|(p, _)| p == path) {
            self.due = None;
            return Some(text.clone());
        }
        if !self.running && self.due.is_some_and(|deadline| now >= deadline) {
            if self.sender.try_send(path.clone()).is_ok() {
                self.running = true;
                self.due = None;
            } else {
                self.due = None;
                return Some("元数据预览不可用，仍可尝试打开".into());
            }
        }
        None
    }
}

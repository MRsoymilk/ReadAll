//! Platform-neutral, single-owner reader worker used by the Android JNI host.
//! Owns all borrowing engine objects on one Rust thread; only immutable snapshots
//! cross the boundary. No JVM objects, environment changes or platform UI here.
#[cfg(test)]
mod tests;
use crate::{
    epub_session::{Action, EpubSession, Start},
    loading,
    progress::EpubProgressStore,
    publication,
    reader_data::{Kind, Settings, Store},
    text_page::Options,
};
use readall_epub::{EpubBook, EpubLimits};
use readall_font::{Font, FontLimits};
use readall_platform::LocalFileSource;
use readall_render::Surface;
use std::{
    error::Error,
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::Duration,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
static WORKERS: AtomicUsize = AtomicUsize::new(0);

/// Paths are explicitly supplied by the host. A SAF URI must first be imported
/// by Android into a bounded app-private file; it is never treated as a Unix path.
#[derive(Debug, Clone)]
pub struct Config {
    pub book: PathBuf,
    pub font: PathBuf,
    pub state_dir: PathBuf,
    pub width: u32,
    pub height: u32,
    pub font_size: u32,
    pub margin: u32,
}
impl Config {
    fn validate(&self) -> io::Result<()> {
        geometry(self.width, self.height)?;
        if !self.book.is_absolute() || !self.font.is_absolute() || !self.state_dir.is_absolute() {
            return Err(invalid("mobile reader requires absolute app-private paths"));
        }
        if !(8..=96).contains(&self.font_size)
            || self.margin > 160
            || self.margin * 2 + self.font_size * 2 >= self.width.min(self.height)
        {
            return Err(invalid("invalid mobile font size or margins"));
        }
        Ok(())
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn geometry(width: u32, height: u32) -> io::Result<()> {
    if !(256..=4096).contains(&width)
        || !(256..=4096).contains(&height)
        || u64::from(width) * u64::from(height) > 4 * 1024 * 1024
    {
        Err(invalid(
            "mobile viewport must be 256..4096 pixels, at most 4M pixels",
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum Command {
    Next,
    Previous,
    First,
    Last,
    Larger,
    Smaller,
    Resize { width: u32, height: u32 },
    Contents,
    Jump { spine: usize, offset: usize },
    CycleTheme,
    Bookmark,
    Save,
}
#[derive(Debug, Clone)]
pub struct ContentsItem {
    pub title: String,
    pub depth: usize,
    pub spine: usize,
    pub offset: usize,
}
#[derive(Debug, Clone)]
pub struct Frame {
    pub serial: u64,
    pub surface: Surface,
}
impl Frame {
    pub fn byte_len(&self) -> usize {
        self.surface.width() as usize * self.surface.height() as usize * 4
    }
    /// Android RGBA_8888 storage; premultiplied channels, native Java direct buffer.
    pub fn write_rgba(&self, output: &mut [u8]) -> io::Result<()> {
        if output.len() != self.byte_len() {
            return Err(invalid("pixel buffer length does not match frame"));
        }
        for (out, color) in output.chunks_exact_mut(4).zip(self.surface.pixels()) {
            let a = u16::from(color.a);
            out.copy_from_slice(&[
                ((u16::from(color.r) * a + 127) / 255) as u8,
                ((u16::from(color.g) * a + 127) / 255) as u8,
                ((u16::from(color.b) * a + 127) / 255) as u8,
                color.a,
            ]);
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub revision: u64,
    pub busy: bool,
    pub closed: bool,
    pub title: String,
    pub position: String,
    pub locator: String,
    pub progress: f32,
    pub frame: Option<Arc<Frame>>,
    pub contents: Arc<Vec<ContentsItem>>,
    pub notice: String,
    pub phase: &'static str,
    pub done: usize,
    pub total: usize,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            busy: true,
            closed: false,
            title: "ReadAll".into(),
            position: String::new(),
            locator: String::new(),
            progress: 0.0,
            frame: None,
            contents: Arc::new(Vec::new()),
            notice: String::new(),
            phase: "准备打开",
            done: 0,
            total: 0,
        }
    }
}
struct Shared {
    snapshot: Mutex<Snapshot>,
    resize: Mutex<Option<(u32, u32)>>,
    tracker: loading::Tracker,
}
impl Shared {
    fn update(&self, edit: impl FnOnce(&mut Snapshot)) {
        let mut snapshot = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        edit(&mut snapshot);
        snapshot.revision = snapshot.revision.wrapping_add(1);
    }
}
/// Dropping or cancelling does not block Android's main thread. The worker observes
/// cancellation at existing decode/layout checks and releases its owned state.
pub struct Reader {
    shared: Arc<Shared>,
    sender: SyncSender<Command>,
}
impl Reader {
    pub fn open(config: Config) -> io::Result<Self> {
        config.validate()?;
        let permit = Permit::acquire()?;
        let (sender, receiver) = mpsc::sync_channel(16);
        let shared = Arc::new(Shared {
            snapshot: Mutex::new(Snapshot::default()),
            resize: Mutex::new(None),
            tracker: loading::Tracker::default(),
        });
        let owner = Arc::clone(&shared);
        thread::Builder::new()
            .name("readall-mobile".into())
            .spawn(move || {
                let _permit = permit;
                let _guard = owner.tracker.install();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker(config, &owner, receiver)
                }));
                owner.update(|snapshot| {
                    snapshot.busy = false;
                    snapshot.closed = true;
                    if !owner.tracker.is_cancelled() {
                        match result {
                            Ok(Err(error)) => snapshot.notice = error.to_string(),
                            Err(_) => {
                                snapshot.notice =
                                    "阅读工作线程发生异常；请关闭并重新打开图书".into()
                            }
                            _ => {}
                        }
                    }
                });
            })?;
        Ok(Self { shared, sender })
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut snapshot = self
            .shared
            .snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let progress = self.shared.tracker.snapshot();
        snapshot.phase = progress.phase;
        snapshot.done = progress.done;
        snapshot.total = progress.total;
        snapshot
    }
    pub fn command(&self, command: Command) -> io::Result<()> {
        if self.shared.tracker.is_cancelled() || self.snapshot().closed {
            return Err(invalid("reader is closed"));
        }
        if let Command::Resize { width, height } = command {
            geometry(width, height)?;
            *self.shared.resize.lock().unwrap_or_else(|e| e.into_inner()) = Some((width, height));
            return Ok(());
        }
        match self.sender.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "reader input queue is full",
            )),
            Err(TrySendError::Disconnected(_)) => Err(invalid("reader worker has stopped")),
        }
    }
    pub fn cancel(&self) {
        self.shared.tracker.cancel();
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        self.cancel();
    }
}
struct Permit;
impl Permit {
    fn acquire() -> io::Result<Self> {
        // Includes cancelled-but-not-yet-finished workers: rapidly opening books
        // cannot leave unlimited decoders alive behind retired JNI handles.
        WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 4).then_some(n + 1)
            })
            .map(|_| Self)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "previous reader workers are still closing",
                )
            })
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}

fn publish(shared: &Shared, session: &EpubSession<'_, '_, '_, '_>, replace_frame: bool) {
    shared.update(|snapshot| {
        snapshot.busy = false;
        snapshot.title = session.book_title().to_owned();
        let (chapter, chapters) = session.chapter_position();
        let (page, pages) = session.page_position();
        snapshot.position = format!("第 {chapter}/{chapters} 章 · 第 {page}/{pages} 页");
        snapshot.progress = session.overall_progress();
        snapshot.locator = session.anchor().to_string();
        if replace_frame {
            let serial = snapshot.frame.as_ref().map_or(1, |f| f.serial + 1);
            snapshot.frame = Some(Arc::new(Frame {
                serial,
                surface: session.frame().surface.clone(),
            }));
        }
    });
}
fn save(shared: &Shared, store: Option<&EpubProgressStore>, session: &EpubSession<'_, '_, '_, '_>) {
    if let Some(store) = store
        && let Err(error) = store.save(session.anchor())
    {
        shared.update(|snapshot| snapshot.notice = format!("阅读进度保存失败：{error}"));
    }
}
fn worker(config: Config, shared: &Shared, receiver: Receiver<Command>) -> Result<()> {
    let mut source = LocalFileSource::open(&config.book)?;
    let bytes = loading::read(&mut source, 128 * 1024 * 1024, "读取图书")?;
    let prepared = publication::prepare(bytes, &config.book)?;
    loading::stage("解析图书结构")?;
    let book = EpubBook::parse(&prepared.bytes, EpubLimits::default())?;
    let mut source = LocalFileSource::open(&config.font)?;
    let bytes = loading::read(
        &mut source,
        FontLimits::default().max_file_bytes,
        "准备中文字体",
    )?;
    let font = Font::parse(&bytes, 0, FontLimits::default())?;
    let store = Store::new(config.state_dir.join("library-v1"));
    let mut settings = if store.root().join("settings.conf").is_file() {
        store.settings()?
    } else {
        Settings {
            size: config.font_size,
            margin: config.margin,
            ..Settings::default()
        }
    };
    settings.margin = settings
        .margin
        .min((config.width.min(config.height) / 4).saturating_sub(1));
    let mut progress = Some(EpubProgressStore::new(config.state_dir.join("progress-v1")));
    let start = match progress.as_ref().expect("created store").load(&book) {
        Ok(Some(locator)) => Start::Locator(locator),
        Ok(None) => Start::Beginning,
        Err(error) => {
            shared.update(|snapshot| {
                snapshot.notice = format!("旧进度不可用，已从开头打开；原记录未覆盖：{error}")
            });
            progress = None;
            Start::Beginning
        }
    };
    let (width, height) = shared
        .resize
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .unwrap_or((config.width, config.height));
    let options = Options {
        font: config.font.clone(),
        face: 0,
        width,
        height,
        size: settings.size,
        margin: settings.margin,
        page: None,
        at: None,
        allow_missing: true,
    };
    let mut session =
        EpubSession::new_with_preferences(&book, &font, options, start, &[], settings)?;
    publish(shared, &session, true);
    save(shared, progress.as_ref(), &session);
    while !shared.tracker.is_cancelled() {
        let resize = shared
            .resize
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let command = if let Some((width, height)) = resize {
            Command::Resize { width, height }
        } else {
            match receiver.recv_timeout(Duration::from_millis(60)) {
                Ok(command) => command,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        };
        shared.update(|snapshot| {
            snapshot.busy = true;
            snapshot.notice.clear();
        });
        let result: Result<bool> = (|| {
            loading::stage("更新阅读页面")?;
            match command {
                Command::Next => session.action(Action::Next),
                Command::Previous => session.action(Action::Previous),
                Command::First => session.action(Action::First),
                Command::Last => session.action(Action::Last),
                Command::Resize { width, height } => session.resize(width, height),
                Command::Larger | Command::Smaller => {
                    let changed = session.action(if matches!(command, Command::Larger) {
                        Action::Larger
                    } else {
                        Action::Smaller
                    })?;
                    if let Err(error) = store.save_settings(session.settings()) {
                        shared.update(|snapshot| {
                            snapshot.notice = format!("设置保存失败，当前页面仍使用新设置：{error}")
                        });
                    }
                    Ok(changed)
                }
                Command::Contents => {
                    loading::stage("生成章节目录")?;
                    let contents = session
                        .toc_entries()?
                        .into_iter()
                        .map(|e| ContentsItem {
                            title: e.title,
                            depth: e.depth,
                            spine: e.spine,
                            offset: e.offset,
                        })
                        .collect();
                    shared.update(|snapshot| snapshot.contents = Arc::new(contents));
                    Ok(false)
                }
                Command::Jump { spine, offset } => session.jump_to_toc_target(spine, offset),
                Command::CycleTheme => {
                    let mut preferences = session.settings();
                    preferences.theme = preferences.theme.next();
                    let changed = session.apply_settings(preferences)?;
                    if let Err(error) = store.save_settings(preferences) {
                        shared.update(|snapshot| {
                            snapshot.notice = format!("设置保存失败，当前页面仍使用新设置：{error}")
                        });
                    }
                    Ok(changed)
                }
                Command::Bookmark => {
                    store.add(
                        &book,
                        Kind::Bookmark,
                        session.anchor().clone(),
                        None,
                        session.title(),
                    )?;
                    shared.update(|snapshot| snapshot.notice = "已保存书签".into());
                    Ok(false)
                }
                Command::Save => {
                    save(shared, progress.as_ref(), &session);
                    Ok(false)
                }
            }
        })();
        match result {
            Ok(changed) => {
                publish(shared, &session, changed);
                save(shared, progress.as_ref(), &session);
            }
            Err(error) if !shared.tracker.is_cancelled() => {
                shared.update(|snapshot| {
                    snapshot.busy = false;
                    snapshot.notice = error.to_string();
                });
            }
            Err(_) => break,
        }
    }
    save(shared, progress.as_ref(), &session);
    Ok(())
}

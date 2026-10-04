//! Mobile worker for the SAME reader presenter used by the Linux window.
//! Only immutable frames/state cross JNI. Parsing, interaction and animation live
//! on this owner thread; Android supplies touch, IME, clipboard and browser services.
mod input;
#[cfg(test)]
mod tests;
use crate::{
    epub_session::{EpubSession, Start},
    loading,
    native_epub::Presentation,
    progress::EpubProgressStore,
    publication,
    reader_data::{Settings, Store},
    text_page::Options,
    ui::UiFont,
};
use input::Inbox;
use readall_epub::{EpubBook, EpubLimits};
use readall_font::{Font, FontLimits};
use readall_platform::LocalFileSource;
pub use readall_platform::window::{Action as UiAction, ReaderCommand as UiCommand};
use readall_render::Surface;
use std::{
    error::Error,
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
static WORKERS: AtomicUsize = AtomicUsize::new(0);
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
    Ui(UiAction),
    Touch { kind: u32, x: i32, y: i32 },
    Back,
    Pause(bool),
    Input { mode: String, text: String },
    HostReply { kind: u32, text: String },
}
#[derive(Debug, Clone)]
pub enum Effect {
    Copy(String),
    Paste,
    OpenUrl(String),
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
    pub ui_mode: &'static str,
    pub page_mode: &'static str,
    pub animating: bool,
    pub editing: bool,
    pub input: String,
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
            ui_mode: "expanded",
            page_mode: "slide",
            animating: false,
            editing: false,
            input: String::new(),
        }
    }
}
struct Shared {
    snapshot: Mutex<Snapshot>,
    inbox: Inbox,
    tracker: loading::Tracker,
    interrupt: Arc<AtomicBool>,
    effects: Mutex<Vec<Effect>>,
}
impl Shared {
    fn update(&self, edit: impl FnOnce(&mut Snapshot)) {
        let mut snapshot = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        edit(&mut snapshot);
        snapshot.revision = snapshot.revision.wrapping_add(1);
    }
}
pub struct Reader {
    shared: Arc<Shared>,
}
impl Reader {
    pub fn open(config: Config) -> io::Result<Self> {
        config.validate()?;
        let permit = Permit::acquire()?;
        let shared = Arc::new(Shared {
            snapshot: Mutex::new(Snapshot::default()),
            inbox: Inbox::default(),
            tracker: loading::Tracker::default(),
            interrupt: Arc::new(AtomicBool::new(false)),
            effects: Mutex::new(Vec::new()),
        });
        let owner = Arc::clone(&shared);
        thread::Builder::new()
            .name("readall-mobile".into())
            .spawn(move || {
                let _permit = permit;
                let _guard = owner.tracker.install();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker(config, &owner)
                }));
                owner.update(|snapshot| {
                    snapshot.busy = false;
                    snapshot.closed = true;
                    snapshot.animating = false;
                    if !owner.tracker.is_cancelled() {
                        match result {
                            Ok(Err(error)) => snapshot.notice = error.to_string(),
                            Err(_) => {
                                snapshot.notice =
                                    "阅读工作线程发生异常；请关闭并重新打开图书".into()
                            }
                            Ok(Ok(())) => snapshot.notice.clear(),
                        }
                    }
                });
            })?;
        Ok(Self { shared })
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut snapshot = self
            .shared
            .snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let p = self.shared.tracker.snapshot();
        snapshot.phase = p.phase;
        snapshot.done = p.done;
        snapshot.total = p.total;
        snapshot
    }
    pub fn command(&self, command: Command) -> io::Result<()> {
        if self.shared.tracker.is_cancelled() || self.snapshot().closed {
            return Err(invalid("reader is closed"));
        }
        match &command {
            Command::Resize { width, height } => geometry(*width, *height)?,
            Command::Touch { kind, x, y }
                if *kind > 9 || x.unsigned_abs() > 65536 || y.unsigned_abs() > 65536 =>
            {
                return Err(invalid("invalid touch event"));
            }
            Command::Input { mode, text }
                if !matches!(mode.as_str(), "search" | "note")
                    || text.len() > 8192
                    || text.contains('\0') =>
            {
                return Err(invalid("invalid editor input"));
            }
            Command::HostReply { text, .. } if text.len() > 128 * 1024 => {
                return Err(invalid("host reply too large"));
            }
            _ => {}
        }
        self.shared.interrupt.store(true, Ordering::Release);
        self.shared.inbox.push(command)
    }
    pub fn take_effects(&self) -> Vec<Effect> {
        std::mem::take(
            &mut *self
                .shared
                .effects
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        )
    }
    pub fn cancel(&self) {
        self.shared.tracker.cancel();
        self.shared.interrupt.store(true, Ordering::Release);
        self.shared.inbox.wake();
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
fn publish(shared: &Shared, ui: &mut Presentation<'_, '_, '_, '_>, changed: bool) {
    let state = ui.ui_state();
    shared.update(|snapshot| {
        let session = ui.session();
        snapshot.busy = false;
        snapshot.title = session.book_title().to_owned();
        let (chapter, chapters) = session.chapter_position();
        let (page, pages) = session.page_position();
        snapshot.position = format!("第 {chapter}/{chapters} 章 · 第 {page}/{pages} 页");
        snapshot.progress = session.overall_progress();
        snapshot.locator = session.anchor().to_string();
        snapshot.ui_mode = state.mode;
        snapshot.page_mode = state.page_mode;
        snapshot.animating = state.animating;
        snapshot.editing = state.editing;
        snapshot.input = state.input;
        snapshot.notice = state.notice;
        if changed {
            let serial = snapshot.frame.as_ref().map_or(1, |frame| frame.serial + 1);
            snapshot.frame = Some(Arc::new(Frame {
                serial,
                surface: ui.surface().clone(),
            }));
        }
    });
    let mut effects = shared.effects.lock().unwrap_or_else(|e| e.into_inner());
    let free = 8_usize.saturating_sub(effects.len());
    effects.extend(ui.effects().into_iter().take(free));
}
fn worker(config: Config, shared: &Shared) -> Result<()> {
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
    let ui_font = UiFont::from_bytes_face(bytes.clone(), config.font.clone(), 0)?;
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
    let mut notice = None;
    let start = match progress.as_ref().unwrap().load(&book) {
        Ok(Some(locator)) => Start::Locator(locator),
        Ok(None) => Start::Beginning,
        Err(error) => {
            notice = Some(format!("旧进度不可用，已从开头打开；原记录未覆盖：{error}"));
            progress = None;
            Start::Beginning
        }
    };
    let options = Options {
        font: config.font.clone(),
        face: 0,
        width: config.width,
        height: config.height,
        size: settings.size,
        margin: settings.margin,
        page: None,
        at: None,
        allow_missing: true,
    };
    let session = EpubSession::new_with_preferences(&book, &font, options, start, &[], settings)?;
    let mut ui = Presentation::new(session, progress.clone(), ui_font, store)?;
    if let Some(message) = notice {
        ui.notice(message)?;
    }
    publish(shared, &mut ui, true);
    if ui.load_annotations()? {
        publish(shared, &mut ui, true);
    }
    let mut paused = false;
    let mut last_tick = Instant::now();
    while !shared.tracker.is_cancelled() && !ui.closed() {
        let interval = if paused { None } else { ui.interval() };
        let delay = interval.map_or(Duration::from_millis(60), |dt| {
            dt.saturating_sub(last_tick.elapsed())
        });
        if let Some(command) = shared.inbox.receive(delay) {
            shared.update(|s| s.busy = true);
            let result: Result<bool> = (|| match command {
                Command::Resize { width, height } => ui.resize(width, height),
                Command::Contents => {
                    let contents = ui
                        .contents()?
                        .into_iter()
                        .map(|e| ContentsItem {
                            title: e.title,
                            depth: e.depth,
                            spine: e.spine,
                            offset: e.offset,
                        })
                        .collect();
                    shared.update(|s| s.contents = Arc::new(contents));
                    Ok(true)
                }
                Command::Jump { spine, offset } => ui.jump(spine, offset),
                Command::Ui(action) => ui.action(action),
                Command::Touch { kind, x, y } => ui.touch(kind, x, y),
                Command::Back => ui.back(),
                Command::Pause(value) => {
                    paused = value;
                    ui.pause()
                }
                Command::Input { mode, text } => ui.input(&mode, text),
                Command::HostReply { kind, text } => ui.host_reply(kind, text),
                Command::Save => {
                    if let Some(store) = &progress {
                        store.save(ui.session().anchor())?;
                    }
                    Ok(false)
                }
                command => ui.legacy(&command),
            })();
            match result {
                Ok(changed) => publish(shared, &mut ui, changed),
                Err(error) if !shared.tracker.is_cancelled() => {
                    ui.notice(error.to_string())?;
                    publish(shared, &mut ui, true);
                }
                Err(_) => break,
            }
        } else if !paused && shared.inbox.interrupt_idle(&shared.interrupt) {
            let result = {
                let _guard = loading::speculate(Arc::clone(&shared.interrupt));
                ui.idle()
            };
            if let Ok(true) = result {
                publish(shared, &mut ui, true);
            }
        }
        if !paused && ui.interval().is_some_and(|dt| last_tick.elapsed() >= dt) {
            last_tick = Instant::now();
            match ui.tick() {
                Ok(true) => publish(shared, &mut ui, true),
                Ok(false) => {}
                Err(error) if !shared.tracker.is_cancelled() => {
                    ui.notice(error.to_string())?;
                    publish(shared, &mut ui, true);
                }
                Err(_) => break,
            }
        }
    }
    // Preserve the content anchor on app background/close, not every animation frame.
    let _ = ui.pause();
    if let Some(store) = progress {
        store.save(ui.session().anchor())?;
    }
    Ok(())
}

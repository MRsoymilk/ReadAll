//! Wayland stays on the presentation thread. Book parsing, layout, glyph/image
//! work and reader actions live together on one worker (no self-referential moves).
use crate::{
    loading::{self, Tracker},
    ui::{UiFont, UiPainter},
};
use readall_platform::window::{
    self, Action, WindowHandler, WindowOptions, WindowReport, WindowResult,
};
use readall_render::{Color, DrawCommand, Rect, RenderLimits, Surface};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

struct Frame {
    surface: Surface,
    title: String,
    editing: bool,
    close: bool,
}
#[derive(Default)]
struct Pending {
    size: Option<(u32, u32)>,
    actions: VecDeque<Action>,
}
struct Shared {
    pending: Mutex<Pending>,
    wake: Condvar,
    frame: Mutex<Option<Frame>>,
    result: Mutex<Option<Result<Vec<u8>, String>>>,
    size: Mutex<(u32, u32)>,
    busy: AtomicBool,
    tracker: Tracker,
}
impl Shared {
    fn new(size: (u32, u32)) -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(Pending::default()),
            wake: Condvar::new(),
            frame: Mutex::new(None),
            result: Mutex::new(None),
            size: Mutex::new(size),
            busy: AtomicBool::new(true),
            tracker: Tracker::default(),
        })
    }
    fn push(&self, action: Action) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        // Only adjacent motion events coalesce; never reorder click/drag boundaries.
        if matches!(action, Action::PointerMove { .. } | Action::PointerLeave)
            && pending
                .actions
                .back()
                .is_some_and(|a| matches!(a, Action::PointerMove { .. } | Action::PointerLeave))
        {
            pending.actions.pop_back();
        }
        if pending.actions.len() >= 128 {
            return false;
        }
        pending.actions.push_back(action);
        self.wake.notify_one();
        true
    }
    fn resize(&self, width: u32, height: u32) {
        *self.size.lock().unwrap_or_else(|e| e.into_inner()) = (width, height);
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).size = Some((width, height));
        self.wake.notify_one();
    }
    fn cancel(&self) {
        self.tracker.cancel();
        self.wake.notify_all();
    }
}
#[derive(Clone)]
pub(super) struct Bridge {
    shared: Arc<Shared>,
}
impl Bridge {
    pub(super) fn size(&self) -> (u32, u32) {
        *self.shared.size.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub(super) fn checkpoint(&self) -> WindowResult<()> {
        loading::check()
    }
    /// The handler never crosses the thread boundary; only immutable pixel snapshots do.
    pub(super) fn serve(&self, handler: &mut impl WindowHandler) -> WindowResult<()> {
        self.checkpoint()?;
        let (w, h) = self.size();
        handler.resize(w, h)?;
        self.publish(handler);
        self.shared.busy.store(false, Ordering::Release);
        let mut tick = Instant::now();
        loop {
            if self.shared.tracker.is_cancelled() {
                return Ok(());
            }
            let (size, action) = {
                let mut p = self
                    .shared
                    .pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if p.size.is_none() && p.actions.is_empty() {
                    let wait = handler
                        .animation_interval()
                        .unwrap_or(Duration::from_millis(250))
                        .min(Duration::from_millis(250));
                    p = self
                        .shared
                        .wake
                        .wait_timeout(p, wait)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
                (p.size.take(), p.actions.pop_front())
            };
            if self.shared.tracker.is_cancelled() {
                return Ok(());
            }
            let mut changed = false;
            if let Some((w, h)) = size {
                self.shared.busy.store(true, Ordering::Release);
                loading::stage("调整窗口排版")?;
                changed |= handler.resize(w, h)?;
            }
            if let Some(action) = action {
                let motion = matches!(
                    action,
                    Action::PointerMove { .. }
                        | Action::PointerLeave
                        | Action::PointerRelease { .. }
                );
                if !motion {
                    self.shared.busy.store(true, Ordering::Release);
                    loading::stage("处理阅读操作")?;
                }
                let handled = handler.action(action)?;
                changed |= handled;
                if action == Action::Close && !handled {
                    self.publish_close(handler);
                    return Ok(());
                }
            }
            if handler
                .animation_interval()
                .is_some_and(|interval| tick.elapsed() >= interval)
            {
                changed |= handler.animation_tick()?;
                tick = Instant::now();
            }
            if handler.close_requested() {
                self.publish_close(handler);
                return Ok(());
            }
            if changed {
                self.publish(handler);
            }
            self.shared.busy.store(false, Ordering::Release);
        }
    }
    pub(super) fn preview(&self, handler: &impl WindowHandler) {
        self.publish(handler);
    }
    fn publish(&self, handler: &impl WindowHandler) {
        let frame = Frame {
            surface: handler.surface().clone(),
            title: handler.title(),
            editing: handler.text_input_active(),
            close: false,
        };
        // Latest-only mailbox bounds both frame memory and latency on rapid input.
        *self.shared.frame.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame);
    }
    fn publish_close(&self, handler: &impl WindowHandler) {
        self.publish(handler);
        if let Some(frame) = self
            .shared
            .frame
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            frame.close = true;
        }
    }
}
type Factory = Arc<dyn Fn(Bridge) -> WindowResult<Vec<u8>> + Send + Sync>;
struct AsyncWindow {
    path: PathBuf,
    surface: Surface,
    page: Option<Surface>,
    ui: UiFont,
    shared: Arc<Shared>,
    factory: Factory,
    worker: Option<JoinHandle<()>>,
    started: bool,
    presented: bool,
    ready: bool,
    close: bool,
    cancelling: bool,
    editing: bool,
    title: String,
    error: Option<String>,
    busy_since: Option<Instant>,
    pulse: u32,
}
impl AsyncWindow {
    fn new(path: PathBuf, size: (u32, u32), ui: UiFont, factory: Factory) -> WindowResult<Self> {
        let mut result = Self {
            title: "ReadAll — 正在加载".into(),
            path,
            surface: Surface::new(size.0, size.1, RenderLimits::default())?,
            page: None,
            ui,
            shared: Shared::new(size),
            factory,
            worker: None,
            started: false,
            presented: false,
            ready: false,
            close: false,
            cancelling: false,
            editing: false,
            error: None,
            busy_since: None,
            pulse: 0,
        };
        result.paint_status()?;
        Ok(result)
    }
    fn start(&mut self) -> WindowResult<()> {
        let shared = Arc::clone(&self.shared);
        let factory = Arc::clone(&self.factory);
        self.worker = Some(thread::Builder::new().name("readall-book".into()).spawn(
            move || {
                let _guard = shared.tracker.install();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    factory(Bridge {
                        shared: Arc::clone(&shared),
                    })
                }));
                let result = match result {
                    Ok(result) => result.map_err(|e| e.to_string()),
                    Err(_) => Err("阅读工作线程异常终止；文档未被修改".into()),
                };
                *shared.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
                shared.busy.store(false, Ordering::Release);
            },
        )?);
        self.started = true;
        Ok(())
    }
    fn cancel(&mut self) {
        self.cancelling = true;
        self.shared.cancel();
    }
    fn button(&self, retry: bool) -> Rect {
        let width = self.surface.width();
        let height = self.surface.height();
        let button_width = if self.error.is_some() {
            width.saturating_sub(48) / 2
        } else {
            width.saturating_sub(32)
        }
        .min(132);
        let span = if self.error.is_some() {
            button_width * 2 + 12
        } else {
            button_width
        };
        let left = width.saturating_sub(span) / 2;
        let x = left
            + if self.error.is_some() && !retry {
                button_width + 12
            } else {
                0
            };
        Rect::new(x as i32, (height / 2 + 84) as i32, button_width, 34)
    }
    fn paint_status(&mut self) -> WindowResult<()> {
        let (w, h) = (self.surface.width(), self.surface.height());
        if let Some(page) = &self.page {
            self.surface = page.clone();
        } else {
            self.surface.draw(&[DrawCommand::FillRect {
                rect: Rect::new(0, 0, w, h),
                color: Color::rgba(22, 27, 35, 255),
            }])?;
        }
        let progress = self.shared.tracker.snapshot();
        let failed = self.error.is_some();
        let busy_overlay = self.page.is_some() && !failed && !self.cancelling;
        let left = if busy_overlay {
            16
        } else {
            w.saturating_sub(560.min(w.saturating_sub(32))) / 2
        };
        let top = if busy_overlay {
            36
        } else {
            (h / 2).saturating_sub(92)
        };
        let width = if busy_overlay {
            w.saturating_sub(32)
        } else {
            560.min(w.saturating_sub(32))
        };
        let panel = Rect::new(
            left as i32,
            top as i32,
            width,
            if busy_overlay { 78 } else { 160 },
        );
        self.surface.draw(&[DrawCommand::FillRect {
            rect: panel,
            color: Color::rgba(30, 38, 49, 248),
        }])?;
        let title = if failed {
            "无法打开文档"
        } else if self.cancelling {
            "正在取消加载…"
        } else if busy_overlay {
            "正在更新页面"
        } else {
            "正在打开文档"
        };
        let label = if let Some(error) = &self.error {
            error.clone()
        } else {
            format!(
                "{}   本阶段 {:.1} 秒",
                progress.phase,
                progress.started.elapsed().as_secs_f32()
            )
        };
        {
            let mut text = UiPainter::new(&self.ui, &mut self.surface)?;
            text.draw_clipped(panel.x + 16, panel.y + 10, 16, title, Color::WHITE, panel)?;
            let label = text.fit(13, &label, width.saturating_sub(32))?;
            text.draw_clipped(
                panel.x + 16,
                panel.y + 35,
                13,
                &label,
                Color::rgba(191, 202, 217, 255),
                panel,
            )?;
            if !busy_overlay {
                let name = self.path.file_name().unwrap_or_default().to_string_lossy();
                let name = text.fit(14, &name, width.saturating_sub(32))?;
                text.draw_clipped(
                    panel.x + 16,
                    panel.y + 66,
                    14,
                    &name,
                    Color::rgba(206, 216, 230, 255),
                    panel,
                )?;
            }
        }
        if !failed {
            let track = Rect::new(
                panel.x + 16,
                panel.y + if busy_overlay { 62 } else { 100 },
                width.saturating_sub(32),
                6,
            );
            let (x, fill) = if progress.total > 0 {
                (
                    track.x,
                    ((track.width as u64 * progress.done as u64) / progress.total as u64) as u32,
                )
            } else {
                let fill = (track.width / 5).max(1);
                (
                    track.x + (self.pulse % (track.width.saturating_sub(fill).max(1))) as i32,
                    fill,
                )
            };
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: track,
                    color: Color::rgba(64, 75, 91, 255),
                },
                DrawCommand::FillRect {
                    rect: Rect::new(x, track.y, fill, 6),
                    color: Color::rgba(94, 174, 246, 255),
                },
            ])?;
            if !busy_overlay {
                let note = if progress.total > 0 {
                    format!(
                        "当前阶段 {}/{}（{}%）",
                        progress.done,
                        progress.total,
                        100_u64 * progress.done as u64 / progress.total as u64
                    )
                } else {
                    "当前阶段处理中；Esc 取消，不会修改原文档".into()
                };
                let mut text = UiPainter::new(&self.ui, &mut self.surface)?;
                let note = text.fit(12, &note, width.saturating_sub(32))?;
                text.draw_clipped(
                    panel.x + 16,
                    panel.y + 120,
                    12,
                    &note,
                    Color::rgba(191, 202, 217, 255),
                    panel,
                )?;
            }
        }
        if !busy_overlay {
            for retry in if failed {
                vec![true, false]
            } else {
                vec![false]
            } {
                let rect = self.button(retry);
                self.surface.draw(&[DrawCommand::FillRect {
                    rect,
                    color: Color::rgba(53, 76, 105, 255),
                }])?;
                let mut text = UiPainter::new(&self.ui, &mut self.surface)?;
                text.draw_clipped(
                    rect.x + 18,
                    rect.y + 8,
                    14,
                    if retry {
                        "重试"
                    } else if failed {
                        "返回 / 关闭"
                    } else {
                        "取消加载"
                    },
                    Color::WHITE,
                    rect,
                )?;
            }
        }
        Ok(())
    }
    fn retry(&mut self) -> WindowResult<()> {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.shared = Shared::new((self.surface.width(), self.surface.height()));
        self.error = None;
        self.ready = false;
        self.page = None;
        self.started = false;
        self.presented = false;
        self.cancelling = false;
        self.busy_since = None;
        self.paint_status()
    }
}
fn contains(rect: Rect, x: i32, y: i32) -> bool {
    i64::from(x) >= i64::from(rect.x)
        && i64::from(y) >= i64::from(rect.y)
        && i64::from(x) < i64::from(rect.x) + i64::from(rect.width)
        && i64::from(y) < i64::from(rect.y) + i64::from(rect.height)
}
impl WindowHandler for AsyncWindow {
    fn resize(&mut self, w: u32, h: u32) -> WindowResult<bool> {
        if (w, h) == (self.surface.width(), self.surface.height()) {
            return Ok(false);
        }
        // Never do reader reflow in a configure callback.
        self.surface = Surface::new(w, h, RenderLimits::default())?;
        self.page = None;
        self.shared.resize(w, h);
        self.paint_status()?;
        Ok(true)
    }
    fn action(&mut self, action: Action) -> WindowResult<bool> {
        if self.error.is_some() {
            if action == Action::Activate
                || matches!(action,Action::Click{x,y} if contains(self.button(true),x,y))
            {
                self.retry()?;
                return Ok(true);
            }
            if matches!(action, Action::Close | Action::Back)
                || matches!(action,Action::Click{x,y} if contains(self.button(false),x,y))
            {
                self.close = true;
                return Ok(true);
            }
            return Ok(false);
        }
        if !self.ready || self.page.is_none() || self.cancelling {
            if matches!(action, Action::Close | Action::Back)
                || matches!(action,Action::Click{x,y} if contains(self.button(false),x,y))
            {
                self.cancel();
                self.paint_status()?;
                return Ok(true);
            }
            return Ok(false);
        }
        if self.shared.busy.load(Ordering::Acquire)
            && self
                .busy_since
                .is_some_and(|at| at.elapsed() > Duration::from_millis(200))
        {
            if action == Action::Close {
                self.cancel();
                self.paint_status()?;
                return Ok(true);
            }
            // Do not apply pointer coordinates from a stale layout after a resize.
            if matches!(
                action,
                Action::Click { .. } | Action::PointerMove { .. } | Action::PointerRelease { .. }
            ) {
                return Ok(false);
            }
        }
        if matches!(
            action,
            Action::Command(
                readall_platform::window::ReaderCommand::Find
                    | readall_platform::window::ReaderCommand::Note
            )
        ) {
            self.editing = true;
        }
        self.shared.push(action);
        // Close is resolved by the worker, not interpreted as immediate window exit.
        Ok(action == Action::Close)
    }
    fn surface(&self) -> &Surface {
        &self.surface
    }
    fn title(&self) -> String {
        self.title.clone()
    }
    fn text_input_active(&self) -> bool {
        self.ready && self.editing && !self.cancelling
    }
    fn animation_interval(&self) -> Option<Duration> {
        Some(Duration::from_millis(40))
    }
    fn animation_tick(&mut self) -> WindowResult<bool> {
        // A slow compositor handshake must not start book work before a buffer commit.
        if !self.started {
            if !self.presented {
                return Ok(false);
            }
            self.start()?;
        }
        let mut changed = false;
        let frame = self
            .shared
            .frame
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(frame) = frame {
            self.title = frame.title;
            self.editing = frame.editing;
            self.close |= frame.close;
            self.ready = true;
            if (frame.surface.width(), frame.surface.height())
                == (self.surface.width(), self.surface.height())
            {
                self.page = Some(frame.surface.clone());
                self.surface = frame.surface;
                changed = true;
            }
        }
        let terminal = self
            .shared
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|r| r.as_ref().map(|_| ()).map_err(Clone::clone));
        if let Some(result) = terminal {
            if self.cancelling || result.is_ok() {
                self.close = true;
            } else if self.error.is_none() {
                self.error = result.err();
                self.title = "ReadAll — 加载失败".into();
                self.paint_status()?;
                changed = true;
            }
        }
        let busy = self.shared.busy.load(Ordering::Acquire);
        if busy {
            self.busy_since.get_or_insert_with(Instant::now);
        } else if self.busy_since.take().is_some()
            && let Some(page) = &self.page
        {
            self.surface = page.clone();
            changed = true;
        }
        if self.error.is_none()
            && (!self.ready
                || self.page.is_none()
                || self.cancelling
                || busy
                    && self
                        .busy_since
                        .is_some_and(|at| at.elapsed() > Duration::from_millis(160)))
        {
            self.pulse = self.pulse.wrapping_add(9);
            self.paint_status()?;
            changed = true;
        }
        Ok(changed)
    }
    fn frame_presented(&mut self) {
        self.presented = true;
    }
    fn close_requested(&self) -> bool {
        self.close
    }
}
impl Drop for AsyncWindow {
    fn drop(&mut self) {
        self.shared.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
pub(super) fn run(
    path: PathBuf,
    size: (u32, u32),
    options: WindowOptions,
    factory: impl Fn(Bridge) -> WindowResult<Vec<u8>> + Send + Sync + 'static,
) -> WindowResult<(WindowReport, Vec<u8>)> {
    let ui = UiFont::system()?;
    let mut window = AsyncWindow::new(path, size, ui, Arc::new(factory))?;
    let report = window::run(&mut window, options)?;
    window.shared.cancel();
    if let Some(worker) = window.worker.take() {
        let _ = worker.join();
    }
    let result = window
        .shared
        .result
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    match result {
        Some(Ok(log)) => Ok((report, log)),
        Some(Err(error)) if !window.cancelling => Err(error.into()),
        _ => Ok((report, Vec::new())),
    }
}
#[cfg(test)]
mod tests;

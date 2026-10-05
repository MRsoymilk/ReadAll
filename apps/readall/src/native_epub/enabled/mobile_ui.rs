//! Android uses the desktop ReaderWindow, not a second toolbar/TOC renderer.
//! This adapter translates touch and host services; layout, panels, hit testing,
//! selection, page effects and settings stay in the shared implementation.
use super::scroll_physics::Inertia;
use super::*;
use crate::reader_data::{PageMode, Store};
use readall_platform::window::ReaderCommand;
use std::{collections::VecDeque, time::Instant};
mod toc_drag;
use toc_drag::TocDrag;

#[cfg(test)]
mod tests;

#[derive(Default)]
enum Drag {
    #[default]
    None,
    Page,
    Toc(TocDrag),
    List {
        last_y: i32,
        remainder: i64,
    },
    Selection,
}

pub(crate) struct Presentation<'b, 'a, 'f, 'd> {
    window: ReaderWindow<'b, 'a, 'f, 'd>,
    drag: Drag,
    allow_fling: bool,
    allow_toc_fling: bool,
    toc_fling: Option<Inertia>,
}
#[derive(Debug, Clone)]
pub(crate) struct UiState {
    pub mode: &'static str,
    pub page_mode: &'static str,
    pub animating: bool,
    pub editing: bool,
    pub input: String,
    pub notice: String,
}
impl<'b, 'a, 'f, 'd> Presentation<'b, 'a, 'f, 'd> {
    pub(crate) fn new(
        session: EpubSession<'b, 'a, 'f, 'd>,
        progress: Option<EpubProgressStore>,
        font: UiFont,
        store: Store,
    ) -> WindowResult<Self> {
        let mut window = ReaderWindow::new_lazy(session, progress, font)?;
        window.host_effects = Some(VecDeque::new());
        window.tools.store = Some(store);
        Ok(Self {
            window,
            drag: Drag::None,
            allow_fling: false,
            allow_toc_fling: false,
            toc_fling: None,
        })
    }
    pub(crate) fn load_annotations(&mut self) -> WindowResult<bool> {
        let mut changed = false;
        if let Some(store) = &self.window.tools.store {
            match store.annotations(self.window.session.book()) {
                Ok(rows) => {
                    changed = !rows.is_empty();
                    self.window.tools.annotations = rows;
                }
                Err(error) => {
                    changed = true;
                    self.window.tools.status = format!("读取标注失败：{error}");
                }
            }
        }
        if changed {
            self.window.refresh_surface()?;
        }
        Ok(changed)
    }
    pub(crate) fn session(&self) -> &EpubSession<'b, 'a, 'f, 'd> {
        &self.window.session
    }
    pub(crate) fn surface(&self) -> &Surface {
        self.window.surface()
    }
    pub(crate) fn closed(&self) -> bool {
        self.window.close_requested()
    }
    /// The mobile actor enables frame-batched painting; direct presenter tests stay immediate.
    pub(crate) fn defer_frames(&mut self) {
        self.window.defer_paint = true;
    }
    pub(crate) fn flush_frame(&mut self) -> WindowResult<()> {
        if self.window.paint_pending {
            let deferred = self.window.defer_paint;
            self.window.defer_paint = false;
            let result = self.window.refresh_surface();
            self.window.defer_paint = deferred;
            result?;
        }
        Ok(())
    }
    pub(crate) fn interval(&self) -> Option<Duration> {
        if self.toc_fling.is_some() {
            Some(Duration::from_millis(16))
        } else {
            self.window.animation_interval()
        }
    }
    pub(crate) fn tick(&mut self) -> WindowResult<bool> {
        let toc_changed = self.tick_toc(Instant::now())?;
        self.window
            .animation_tick()
            .map(|changed| changed || toc_changed || self.window.paint_pending)
    }
    fn tick_toc(&mut self, now: Instant) -> WindowResult<bool> {
        let Some(fling) = &mut self.toc_fling else {
            return Ok(false);
        };
        let delta = fling.step(now);
        let active = fling.active();
        let changed = self.window.scroll_toc_pixels(delta);
        let stopped = !active || (!changed && delta.abs() > 0.0001);
        if stopped {
            self.toc_fling = None;
        }
        // Publish the final idle state even if an edge prevented pixel movement.
        // Otherwise Android keeps polling the preceding animating=true snapshot.
        if changed || stopped {
            self.window.refresh_surface()?;
        }
        Ok(changed || stopped)
    }
    pub(crate) fn idle(&mut self) -> WindowResult<bool> {
        if self.window.motion.active() || !matches!(self.drag, Drag::None) {
            return Ok(false);
        }
        self.window.idle_tick()
    }
    pub(crate) fn ui_state(&self) -> UiState {
        let mode = match self.window.tools.mode {
            tools::Mode::Search => "search",
            tools::Mode::Note => "note",
            tools::Mode::Settings => "settings",
            tools::Mode::Annotations => "annotations",
            tools::Mode::Zoom => "image",
            tools::Mode::External => "external",
            tools::Mode::None => match self.window.toolbar {
                ToolbarMode::Expanded => "expanded",
                ToolbarMode::Collapsed => "collapsed",
                ToolbarMode::Toc => "toc",
            },
        };
        UiState {
            mode,
            page_mode: self.window.session.settings().page_mode.name(),
            animating: self.window.motion.active() || self.toc_fling.is_some(),
            editing: self.window.text_input_active(),
            input: self.window.tools.query.clone(),
            notice: self.window.tools.status.clone(),
        }
    }
    pub(crate) fn notice(&mut self, text: String) -> WindowResult<()> {
        self.window.tools.status = text;
        self.window.refresh_surface()
    }
    pub(crate) fn effects(&mut self) -> Vec<crate::mobile::Effect> {
        self.window
            .host_effects
            .as_mut()
            .unwrap()
            .drain(..)
            .map(|effect| match effect {
                HostEffect::Copy(text) => crate::mobile::Effect::Copy(text),
                HostEffect::Paste => crate::mobile::Effect::Paste,
                HostEffect::OpenUrl(url) => crate::mobile::Effect::OpenUrl(url),
            })
            .collect()
    }
    pub(crate) fn input(&mut self, mode: &str, value: String) -> WindowResult<bool> {
        if mode != self.ui_state().mode || !self.window.tools.editing() {
            return Ok(false);
        }
        let limit = if mode == "search" { 1024 } else { 8192 };
        if value.len() > limit || value.contains('\0') {
            return Err("输入内容超过上限或包含 NUL".into());
        }
        self.window.tools.query = value;
        self.window.tools.dirty = true;
        self.window.refresh_surface()?;
        Ok(true)
    }
    pub(crate) fn host_reply(&mut self, kind: u32, text: String) -> WindowResult<bool> {
        if text.len() > 128 * 1024 {
            return Err("系统返回文本过长".into());
        }
        match kind {
            1 => self.window.tools.status = "已复制选中文字".into(),
            2 => {
                if self.window.tools.editing() {
                    let mode = self.ui_state().mode;
                    let limit = if mode == "search" { 1024 } else { 8192 };
                    let mut value = self.window.tools.query.clone();
                    for ch in text
                        .chars()
                        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                    {
                        if value.len() + ch.len_utf8() > limit {
                            break;
                        }
                        value.push(ch);
                    }
                    self.input(mode, value)?;
                    self.window.tools.status = "已粘贴".into();
                }
            }
            3 => self.window.tools.status = "网页已交给系统浏览器；阅读位置保持不变".into(),
            _ => {
                self.window.tools.status = format!(
                    "系统操作失败：{}",
                    text.chars().take(1024).collect::<String>()
                )
            }
        }
        self.window.refresh_surface()?;
        Ok(true)
    }
    pub(crate) fn action(&mut self, action: Action) -> WindowResult<bool> {
        self.toc_fling = None;
        self.allow_toc_fling = false;
        self.window.action(action)
    }
    pub(crate) fn contents(&mut self) -> WindowResult<Vec<TocEntry>> {
        self.cancel_touch()?;
        self.window.ensure_toc()?;
        self.window.toolbar = ToolbarMode::Toc;
        self.window.sync_toc_selection();
        self.window.scroll_toc_pixels(0.0);
        self.window.refresh_surface()?;
        Ok(self.window.toc.clone())
    }
    pub(crate) fn jump(&mut self, spine: usize, offset: usize) -> WindowResult<bool> {
        self.cancel_touch()?;
        let changed = self.window.session.jump_to_toc_target(spine, offset)?;
        self.window.toolbar = ToolbarMode::Expanded;
        self.window.tools.mode = tools::Mode::None;
        self.window.tools.clear_selection();
        self.window.reset_motion()?;
        self.window.sync_toc_selection();
        self.window.save_progress();
        self.window.refresh_surface()?;
        Ok(changed)
    }
    pub(crate) fn resize(&mut self, width: u32, height: u32) -> WindowResult<bool> {
        self.cancel_touch()?;
        self.window.resize(width, height)
    }
    pub(crate) fn resize_output(
        &mut self,
        width: u32,
        height: u32,
        pixel_width: u32,
        pixel_height: u32,
    ) -> WindowResult<bool> {
        self.cancel_touch()?;
        let changed =
            self.window
                .session
                .resize_output(width, height, pixel_width, pixel_height)?;
        if changed {
            self.window.reset_motion()?;
            self.window.refresh_surface()?;
        }
        Ok(changed)
    }
    pub(crate) fn pause(&mut self) -> WindowResult<bool> {
        self.cancel_touch()?;
        self.window.freeze_motion()?;
        self.window.save_progress();
        self.window.refresh_surface()?;
        Ok(true)
    }
    pub(crate) fn back(&mut self) -> WindowResult<bool> {
        if self.toc_fling.take().is_some() {
            return Ok(true);
        }
        if self.window.motion.active()
            || self.window.tools.mode != tools::Mode::None
            || self.window.tools.selection.is_some()
            || self.window.tools.selecting
        {
            self.window.action(Action::Close)
        } else {
            self.window.action(Action::Back)
        }
    }
    fn body(&self, x: i32, y: i32) -> bool {
        self.window.tools.mode == tools::Mode::None
            && self.window.toolbar != ToolbarMode::Toc
            && y >= 32
            && !(self.window.toolbar != ToolbarMode::Collapsed
                && point_in(self.window.toolbar_rect(), x, y))
            && !point_in(self.window.collapsed_rect(), x, y)
            && !(self.window.tools.selection.is_some() && y < 84)
    }
    fn cancel_touch(&mut self) -> WindowResult<bool> {
        if matches!(self.drag, Drag::Selection) {
            self.window.tools.selecting = false;
        }
        self.drag = Drag::None;
        self.allow_fling = false;
        self.allow_toc_fling = false;
        self.toc_fling = None;
        self.window.action(Action::PointerLeave)?;
        self.window.freeze_motion()?;
        Ok(true)
    }
    /// 0 down; 1 tap; 2/3/4 pan start/move/end; 5/6/7 long-press selection;
    /// 8 cancel; 9 fling displacement. No click is generated before a finger lifts.
    pub(crate) fn touch(&mut self, kind: u32, x: i32, y: i32) -> WindowResult<bool> {
        let changed = match kind {
            0 => {
                self.cancel_touch()?;
                true
            }
            1 => {
                self.window.action(Action::PointerMove { x, y })?;
                self.window.action(Action::Click { x, y })?;
                self.window.action(Action::PointerRelease { x, y })?;
                self.window.action(Action::PointerLeave)?;
                self.window.scroll_toc_pixels(0.0);
                true
            }
            2 => {
                self.allow_fling = false;
                self.allow_toc_fling = false;
                self.toc_fling = None;
                if self.window.tools.mode != tools::Mode::None {
                    self.drag = Drag::List {
                        last_y: y,
                        remainder: 0,
                    };
                } else if self.window.toolbar == ToolbarMode::Toc {
                    self.window.scroll_toc_pixels(0.0);
                    self.drag = TocDrag::start(&self.window, x, y).map_or(Drag::None, Drag::Toc);
                } else if self.body(x, y) {
                    self.window.action(Action::PanStart { x, y })?;
                    self.drag = Drag::Page;
                }
                true
            }
            3 => match &mut self.drag {
                Drag::Page => self.window.action(Action::PointerMove { x, y })?,
                Drag::Toc(drag) => drag.move_to(&mut self.window, y),
                Drag::List { last_y, remainder } => {
                    *remainder += i64::from(*last_y) - i64::from(y);
                    *last_y = y;
                    let steps = (*remainder / 30).clamp(-8, 8);
                    *remainder -= steps * 30;
                    for _ in 0..steps.unsigned_abs() {
                        self.window.action(if steps > 0 {
                            Action::Next
                        } else {
                            Action::Previous
                        })?;
                    }
                    steps != 0
                }
                _ => false,
            },
            4 => {
                let was_page = matches!(self.drag, Drag::Page);
                self.allow_toc_fling = matches!(self.drag, Drag::Toc(_));
                if let Drag::Toc(drag) = &mut self.drag {
                    drag.move_to(&mut self.window, y);
                }
                self.drag = Drag::None;
                self.allow_fling =
                    was_page && self.window.session.settings().page_mode == PageMode::Scroll;
                if was_page {
                    self.window.action(Action::PanEnd { x, y })?;
                }
                self.window.action(Action::PointerLeave)?;
                true
            }
            5 => {
                if self.body(x, y) {
                    self.window.tools.selecting = true;
                    self.window.action(Action::Click { x, y })?;
                    self.drag = Drag::Selection;
                }
                true
            }
            6 if matches!(self.drag, Drag::Selection) => {
                self.window.action(Action::PointerMove { x, y })?
            }
            7 if matches!(self.drag, Drag::Selection) => {
                self.window.action(Action::PointerRelease { x, y })?;
                self.window.tools.selecting = false;
                self.drag = Drag::None;
                true
            }
            6 | 7 => false,
            8 => self.cancel_touch()?,
            9 if self.allow_toc_fling => {
                self.allow_toc_fling = false;
                self.toc_fling = Inertia::from_displacement(y, Instant::now());
                self.toc_fling.is_some()
            }
            9 if self.allow_fling => {
                self.allow_fling = false;
                self.window.start_touch_fling(y)
            }
            9 => false,
            _ => return Err("unknown mobile touch event".into()),
        };
        if changed {
            self.window.refresh_surface()?;
        }
        Ok(changed)
    }
    pub(crate) fn legacy(&mut self, command: &crate::mobile::Command) -> WindowResult<bool> {
        use crate::mobile::Command;
        self.action(match command {
            Command::Next => Action::Next,
            Command::Previous => Action::Previous,
            Command::First => Action::First,
            Command::Last => Action::Last,
            Command::Larger => Action::Larger,
            Command::Smaller => Action::Smaller,
            Command::CycleTheme => Action::Command(ReaderCommand::Theme),
            Command::Bookmark => Action::Command(ReaderCommand::Bookmark),
            _ => return Err("not a shared UI command".into()),
        })
    }
}

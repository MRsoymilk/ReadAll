//! Native search, annotations, settings, selection and image inspection overlays.
mod desktop;
mod external;
mod gestures;
#[cfg(test)]
mod link_tests;
mod links;
mod mobile_menu;
mod selection_actions;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
use super::*;
use crate::reader_data::{Annotation, Kind, Settings, Store};
use readall_epub::SearchHit;
use readall_image::RgbaImage;
use readall_platform::window::ReaderCommand;
use std::{
    ops::Range,
    sync::{Arc, mpsc},
    time::Instant,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Mode {
    #[default]
    None,
    Search,
    Annotations,
    Note,
    Settings,
    Zoom,
    External,
}
pub(super) enum ClipboardResult {
    Copied,
    Pasted(String),
    Failed(String),
}
#[derive(Default)]
pub(super) struct Tools {
    pub(super) store: Option<Store>,
    pub(super) annotations: Vec<Annotation>,
    pub(super) mode: Mode,
    pub(super) query: String,
    pub(super) dirty: bool,
    hits: Vec<SearchHit>,
    selected: usize,
    scroll: usize,
    desktop_wheel: f64,
    pub(super) status: String,
    pub(super) link_history: Vec<readall_epub::EpubLocator>,
    external: external::ExternalLink,
    pub(super) selecting: bool,
    drag: Option<gestures::Gesture>,
    pub(super) selection: Option<Range<usize>>,
    zoom: Option<Arc<RgbaImage>>,
    factor: f32,
    pan: (i32, i32),
    clipboard: Option<mpsc::Receiver<ClipboardResult>>,
    started: Option<Instant>,
}
impl Tools {
    pub(super) fn editing(&self) -> bool {
        matches!(self.mode, Mode::Search | Mode::Note)
    }
    pub(super) fn pending(&self) -> bool {
        self.clipboard.is_some() || self.external.pending()
    }
    pub(super) fn cancel_gesture(&mut self) {
        self.drag = None;
    }
    pub(super) fn dragging(&self) -> bool {
        self.drag.is_some()
    }
    pub(super) fn clear_selection(&mut self) {
        self.selection = None;
        self.drag = None;
    }
}
impl<'book, 'archive, 'font, 'data> ReaderWindow<'book, 'archive, 'font, 'data> {
    #[cfg(test)]
    pub(super) fn attach_tools(
        &mut self,
        store: Store,
        load_size: bool,
        load_margin: bool,
    ) -> WindowResult<()> {
        let result = (|| -> WindowResult<()> {
            let mut settings = store.settings()?;
            if !load_size {
                settings.size = self.session.font_size();
            }
            if !load_margin {
                settings.margin = self.session.settings().margin;
            }
            if settings.margin * 2 >= self.surface.width().min(self.surface.height()) {
                settings.margin = self.session.settings().margin;
            }
            self.session.apply_settings(settings)?;
            self.tools.annotations = store.annotations(self.session.book())?;
            Ok(())
        })();
        if let Err(error) = result {
            self.tools.status = format!("读取设置/标注失败：{error}");
        }
        self.tools.store = Some(store);
        self.refresh_surface()
    }
    pub(super) fn tool_panel(&self) -> Rect {
        let w = self.surface.width().saturating_sub(24).min(640);
        Rect::new(
            (self.surface.width() - w) as i32 / 2,
            40,
            w,
            self.surface.height().saturating_sub(54),
        )
    }
    pub(super) fn tool_dock(&self, index: usize) -> Rect {
        let prev = self.toolbar_button_rect(0);
        let next = self.toolbar_button_rect(4);
        let left = prev.x + prev.width as i32 + 20;
        let width = next.x.saturating_sub(left + 12).max(0) as u32;
        let cell = width / 3;
        Rect::new(
            left + index as i32 * cell as i32,
            prev.y + 5,
            cell.saturating_sub(4),
            30,
        )
    }
    fn tool_rows(&self) -> usize {
        (self.tool_panel().height.saturating_sub(136) / 42).max(1) as usize
    }
    fn tool_row_at(&self, x: i32, y: i32) -> Option<usize> {
        let panel = self.tool_panel();
        let start = panel.y + 88;
        if y < start || y >= panel.y + panel.height as i32 - 46 {
            return None;
        }
        let row = ((y - start) / 42) as usize;
        let index = self.tools.scroll.saturating_add(row);
        let rect = Rect::new(
            panel.x + 8,
            start + row as i32 * 42,
            panel.width.saturating_sub(16),
            38,
        );
        (row < self.tool_rows() && index < self.tool_count() && point_in(rect, x, y))
            .then_some(index)
    }
    fn tool_keep_visible(&mut self) {
        let visible = self.tool_rows();
        if self.tools.selected < self.tools.scroll {
            self.tools.scroll = self.tools.selected;
        } else if self.tools.selected >= self.tools.scroll + visible {
            self.tools.scroll = self.tools.selected + 1 - visible;
        }
    }
    fn tool_count(&self) -> usize {
        match self.tools.mode {
            Mode::Search => self.tools.hits.len(),
            Mode::Annotations => self.tools.annotations.len(),
            Mode::Settings => 5,
            _ => 0,
        }
    }
    fn reload_annotations(&mut self) -> WindowResult<()> {
        if let Some(store) = &self.tools.store {
            self.tools.annotations = store.annotations(self.session.book())?;
        }
        Ok(())
    }
    fn tool_store(&self) -> WindowResult<&Store> {
        self.tools
            .store
            .as_ref()
            .ok_or_else(|| "当前会话没有可用的标注存储目录".into())
    }
    fn tool_bookmark(&mut self) -> WindowResult<()> {
        let locator = self.session.anchor().clone();
        self.tool_store()?.add(
            self.session.book(),
            Kind::Bookmark,
            locator,
            None,
            self.session.book_title().to_owned(),
        )?;
        self.reload_annotations()?;
        self.tools.status = if self.mobile_chrome() {
            "书签已保存，可在标注中查看"
        } else {
            "书签已保存；F3 查看"
        }
        .into();
        Ok(())
    }
    fn tool_highlight(&mut self) -> WindowResult<()> {
        let range = self.tools.selection.clone().ok_or("请先拖动选择文字")?;
        let locator = self.session.selected_locator(range.start)?;
        self.tool_store()?.add(
            self.session.book(),
            Kind::Highlight,
            locator,
            Some(range.end),
            self.session.text_at(range).chars().take(100).collect(),
        )?;
        self.reload_annotations()?;
        self.tools.status = "高亮已保存".into();
        Ok(())
    }
    fn tool_note(&mut self) -> WindowResult<()> {
        let (locator, end) = match &self.tools.selection {
            Some(range) => (self.session.selected_locator(range.start)?, Some(range.end)),
            None => (self.session.anchor().clone(), None),
        };
        self.tool_store()?.add(
            self.session.book(),
            Kind::Note,
            locator,
            end,
            self.tools.query.clone(),
        )?;
        self.reload_annotations()?;
        self.tools.mode = Mode::None;
        self.tools.status = "笔记已保存".into();
        Ok(())
    }
    fn run_search(&mut self) -> WindowResult<()> {
        let report = self.session.search(&self.tools.query)?;
        self.tools.status = format!(
            "{} 个结果 · 扫描 {} 章 · 跳过 {} 章{}",
            report.hits.len(),
            report.chapters_scanned,
            report.skipped_chapters,
            if report.truncated {
                " · 达到搜索上限"
            } else {
                ""
            }
        );
        self.tools.hits = report.hits;
        self.tools.selected = 0;
        self.tools.scroll = 0;
        self.tools.dirty = false;
        Ok(())
    }
    fn tool_activate(&mut self) -> WindowResult<()> {
        match self.tools.mode {
            Mode::Search if self.tools.dirty => self.run_search()?,
            Mode::Search => {
                if let Some(hit) = self.tools.hits.get(self.tools.selected).cloned() {
                    self.session.jump_to_locator(hit.locator.clone())?;
                    self.tools.selection = Some(hit.locator.utf8_offset() as usize..hit.end_offset);
                    self.tools.mode = Mode::None;
                    self.sync_toc_selection();
                    self.save_progress();
                }
            }
            Mode::Annotations => {
                if let Some(row) = self.tools.annotations.get(self.tools.selected).cloned() {
                    self.session.jump_to_locator(row.locator.clone())?;
                    self.tools.selection =
                        row.end.map(|end| row.locator.utf8_offset() as usize..end);
                    self.tools.status = row.text;
                    self.tools.mode = Mode::None;
                    self.sync_toc_selection();
                    self.save_progress();
                }
            }
            Mode::Note => self.tool_note()?,
            Mode::Settings => self.change_setting(1)?,
            _ => {}
        }
        Ok(())
    }
    fn change_setting(&mut self, direction: i32) -> WindowResult<()> {
        let mut settings = self.session.settings();
        match self.tools.selected {
            0 => settings.theme = settings.theme.next(),
            1 => settings.size = (settings.size as i32 + direction * 2).clamp(8, 96) as u32,
            2 => settings.margin = (settings.margin as i32 + direction * 4).clamp(0, 160) as u32,
            3 => {
                settings.line_spacing =
                    (settings.line_spacing + direction as f32 * 0.1).clamp(0.8, 2.0)
            }
            4 => settings.page_mode = settings.page_mode.step(direction),
            _ => {}
        }
        self.apply_tool_settings(settings)
    }
    fn apply_tool_settings(&mut self, settings: Settings) -> WindowResult<()> {
        self.freeze_motion()?;
        self.session.apply_settings(settings)?;
        self.reset_motion()?;
        self.tools.clear_selection();
        self.save_progress();
        if let Some(store) = &self.tools.store {
            store.save_settings(settings)?;
            self.tools.status = "阅读设置已保存".into();
        } else {
            self.tools.status = "设置已应用，仅当前会话生效".into();
        }
        Ok(())
    }
    fn tool_command(&mut self, command: ReaderCommand) -> WindowResult<()> {
        self.tools.cancel_gesture();
        match command {
            ReaderCommand::Find => {
                self.tools.mode = Mode::Search;
                self.tools.query.clear();
                self.tools.hits.clear();
                self.tools.dirty = true;
                self.tools.selected = 0;
                self.tools.scroll = 0;
                self.tools.status = if self.mobile_chrome() {
                    "输入关键词后点搜索，轻点结果跳转"
                } else {
                    "输入查询，Enter 搜索；↑↓ 选择，Enter 跳转；Ctrl+V 粘贴"
                }
                .into();
            }
            ReaderCommand::Bookmarks => {
                self.reload_annotations()?;
                self.tools.mode = Mode::Annotations;
                self.tools.selected = 0;
                self.tools.scroll = 0;
                self.tools.status = if self.mobile_chrome() {
                    "轻点书签或标注，返回对应位置"
                } else {
                    "Enter/点击跳转；Delete 删除选中的标注"
                }
                .into();
            }
            ReaderCommand::Bookmark => self.tool_bookmark()?,
            ReaderCommand::Highlight => self.tool_highlight()?,
            ReaderCommand::Note => {
                self.tools.mode = Mode::Note;
                self.tools.query.clear();
                self.tools.status = if self.mobile_chrome() {
                    "输入笔记后点保存，关闭可取消"
                } else {
                    "输入笔记，Enter 保存；Esc 取消；Ctrl+V 可粘贴中文"
                }
                .into();
            }
            ReaderCommand::Settings => {
                self.tools.mode = Mode::Settings;
                self.tools.selected = 0;
                self.tools.scroll = 0;
                self.tools.status = if self.mobile_chrome() {
                    "轻点右侧加减按钮，设置自动保存"
                } else {
                    "↑↓ 选择；+/− 修改；点击右侧加减按钮"
                }
                .into();
            }
            ReaderCommand::Theme => {
                let mut settings = self.session.settings();
                settings.theme = settings.theme.next();
                self.apply_tool_settings(settings)?;
            }
            ReaderCommand::Select => {
                self.tools.mode = Mode::None;
                self.tools.selecting = !self.tools.selecting;
                self.tools.clear_selection();
                self.tools.status = if self.mobile_chrome() {
                    if self.tools.selecting {
                        "长按拖动选择文字，松手后选择操作"
                    } else {
                        "长按选择文字，轻点链接跳转"
                    }
                } else if self.tools.selecting {
                    "文字选择优先：链接也可选字；Ctrl+C 复制；F8 高亮；F7 笔记；F9 返回普通阅读"
                } else {
                    "普通阅读：直接拖选文字，单击链接跳转；Ctrl+C 复制，F8 高亮，F7 笔记"
                }
                .into();
            }
            ReaderCommand::Copy => {
                let range = self
                    .tools
                    .selection
                    .clone()
                    .ok_or("没有选中的文字；请直接拖动选择")?;
                self.start_clipboard(Some(self.session.text_at(range)))?;
            }
            ReaderCommand::Paste => {
                if self.tools.editing() {
                    self.start_clipboard(None)?;
                } else {
                    self.tools.status = "先打开搜索或笔记输入框再粘贴".into();
                }
            }
            ReaderCommand::Delete => {
                if self.tools.mode == Mode::Annotations {
                    if let Some(row) = self.tools.annotations.get(self.tools.selected) {
                        self.tool_store()?.remove(self.session.book(), row.id)?;
                        self.reload_annotations()?;
                        self.tools.selected = self
                            .tools
                            .selected
                            .min(self.tools.annotations.len().saturating_sub(1));
                        self.tool_keep_visible();
                    }
                } else if self.tools.editing() {
                    self.tools.query.clear();
                    self.tools.dirty = true;
                }
            }
        }
        Ok(())
    }
    fn hit_range(&self, x: i32, y: i32, nearest: bool) -> Option<Range<usize>> {
        let hits = &self.session.frame().hits;
        let visible = || {
            hits.iter()
                .filter(|hit| hit.rect.width > 0 && hit.rect.height > 0)
        };
        let hit = visible().find(|hit| point_in(hit.rect, x, y)).or_else(|| {
            nearest
                .then(|| {
                    visible().min_by_key(|hit| {
                        let r = hit.rect;
                        let (x, y) = (i64::from(x), i64::from(y));
                        let dx = (x - x
                            .clamp(i64::from(r.x), i64::from(r.x) + i64::from(r.width) - 1))
                        .abs();
                        let dy = (y - y
                            .clamp(i64::from(r.y), i64::from(r.y) + i64::from(r.height) - 1))
                        .abs();
                        dy * 10000 + dx
                    })
                })
                .flatten()
        })?;
        Some(self.session.grapheme_range(hit.start..hit.end))
    }
    fn tool_click(&mut self, x: i32, y: i32) -> WindowResult<Option<bool>> {
        self.tools.cancel_gesture();
        if self.tools.mode != Mode::None {
            let panel = self.tool_panel();
            if !point_in(panel, x, y) || y < panel.y + 34 && x > panel.x + panel.width as i32 - 44 {
                self.tools.mode = Mode::None;
                return Ok(Some(true));
            }
            if self.tools.mode == Mode::Zoom {
                let (minus, plus) = self.mobile_zoom_buttons();
                if point_in(minus, x, y) {
                    self.tools.factor = (self.tools.factor / 1.25).max(0.25);
                } else if point_in(plus, x, y) {
                    self.tools.factor = (self.tools.factor * 1.25).min(8.0);
                }
                return Ok(Some(true));
            }
            if self.tools.editing() && y >= panel.y + 38 && y < panel.y + 82 {
                if x > panel.x + panel.width as i32 - 80 {
                    if self.tools.mode == Mode::Search {
                        self.run_search()?;
                    } else {
                        self.tool_note()?;
                    }
                }
                return Ok(Some(true));
            }
            if let Some(index) = self.tool_row_at(x, y) {
                self.tools.selected = index;
                if self.tools.mode == Mode::Settings {
                    let row_y = panel.y + 88 + (index - self.tools.scroll) as i32 * 42;
                    let (minus, plus) = self.mobile_setting_buttons(row_y);
                    if point_in(minus, x, y) {
                        self.change_setting(-1)?;
                    } else if point_in(plus, x, y) {
                        self.change_setting(1)?;
                    }
                } else {
                    self.tool_activate()?;
                }
            }
            return Ok(Some(true));
        }
        if self.compact_toc() {
            return Ok(None);
        }
        if let Some(changed) = self.selection_action_click(x, y)? {
            return Ok(Some(changed));
        }
        if y < 32 {
            return Ok(Some(false));
        }
        if !self.tools.selecting
            && self.tools.selection.is_none()
            && !self.tools.link_history.is_empty()
            && point_in(self.link_back_rect(), x, y)
        {
            return Ok(Some(self.return_from_link()?));
        }
        if self.toolbar != ToolbarMode::Collapsed {
            for (index, cmd) in [
                ReaderCommand::Find,
                ReaderCommand::Bookmarks,
                ReaderCommand::Settings,
            ]
            .into_iter()
            .enumerate()
            {
                if point_in(self.tool_dock(index), x, y) {
                    self.tool_command(cmd)?;
                    return Ok(Some(true));
                }
            }
        }
        if self.toolbar == ToolbarMode::Toc
            || point_in(self.toolbar_rect(), x, y) && self.toolbar != ToolbarMode::Collapsed
            || self.toolbar == ToolbarMode::Collapsed && point_in(self.collapsed_rect(), x, y)
        {
            return Ok(None);
        }
        Ok(Some(self.body_press(x, y)))
    }
    pub(super) fn tool_action(&mut self, action: Action) -> WindowResult<Option<bool>> {
        if self.tools.mode == Mode::External {
            return self.external_action(action).map(Some);
        }
        match action {
            // Wheel/keyboard page changes while the button is held would invalidate
            // selection coordinates. Finish or cancel the gesture before navigating.
            Action::Next
            | Action::Previous
            | Action::First
            | Action::Last
            | Action::Larger
            | Action::Smaller
                if self.tools.drag.is_some() && self.tools.mode == Mode::None =>
            {
                Ok(Some(false))
            }
            Action::Command(command) => {
                self.tool_command(command)?;
                Ok(Some(true))
            }
            Action::Text(ch) if self.tools.editing() => {
                if self.tools.query.len() + ch.len_utf8()
                    <= if self.tools.mode == Mode::Search {
                        1024
                    } else {
                        8192
                    }
                {
                    self.tools.query.push(ch);
                    self.tools.dirty = true;
                }
                Ok(Some(true))
            }
            Action::Close
                if self.tools.mode != Mode::None
                    || self.tools.selecting
                    || self.tools.selection.is_some()
                    || self.tools.drag.is_some() =>
            {
                self.tools.mode = Mode::None;
                self.tools.selecting = false;
                self.tools.clear_selection();
                Ok(Some(true))
            }
            Action::Back
                if self.tools.mode == Mode::None
                    && !self.tools.selecting
                    && !self.tools.link_history.is_empty() =>
            {
                self.tools.cancel_gesture();
                Ok(Some(self.return_from_link()?))
            }
            Action::Back if self.tools.editing() => {
                self.tools.query.pop();
                self.tools.dirty = true;
                Ok(Some(true))
            }
            Action::Back if self.tools.mode != Mode::None => {
                self.tools.mode = Mode::None;
                Ok(Some(true))
            }
            Action::Activate if self.tools.mode != Mode::None => {
                self.tool_activate()?;
                Ok(Some(true))
            }
            Action::Next | Action::Previous if self.tools.mode != Mode::None => {
                let delta = if action == Action::Next { 1_i32 } else { -1 };
                if self.tools.mode == Mode::Zoom {
                    self.tools.pan.1 = self
                        .tools
                        .pan
                        .1
                        .saturating_sub(delta * 48)
                        .clamp(-32000, 32000);
                } else {
                    self.tools.selected = if delta > 0 {
                        self.tools
                            .selected
                            .saturating_add(1)
                            .min(self.tool_count().saturating_sub(1))
                    } else {
                        self.tools.selected.saturating_sub(1)
                    };
                    self.tool_keep_visible();
                }
                Ok(Some(true))
            }
            Action::First | Action::Last if self.tools.mode != Mode::None => {
                if self.tools.mode == Mode::Zoom {
                    self.tools.pan.0 = self
                        .tools
                        .pan
                        .0
                        .saturating_add(if action == Action::First { 48 } else { -48 })
                        .clamp(-32000, 32000);
                } else {
                    self.tools.selected = if action == Action::First {
                        0
                    } else {
                        self.tool_count().saturating_sub(1)
                    };
                    self.tool_keep_visible();
                }
                Ok(Some(true))
            }
            Action::Larger | Action::Smaller
                if self.tools.mode == Mode::Settings || self.tools.mode == Mode::Zoom =>
            {
                if self.tools.mode == Mode::Zoom {
                    self.tools.factor = if action == Action::Larger {
                        (self.tools.factor * 1.25).min(8.0)
                    } else {
                        (self.tools.factor / 1.25).max(0.25)
                    };
                } else {
                    self.change_setting(if action == Action::Larger { 1 } else { -1 })?;
                }
                Ok(Some(true))
            }
            Action::Click { x, y } => self.tool_click(x, y),
            Action::PointerMove { x, y } | Action::PointerRelease { x, y }
                if self.tools.drag.is_some() =>
            {
                self.body_motion(x, y, matches!(action, Action::PointerRelease { .. }))
                    .map(Some)
            }
            Action::PointerLeave if self.tools.drag.is_some() => {
                self.tools.cancel_gesture();
                self.pointer = None;
                Ok(Some(true))
            }
            Action::PointerMove { x, y } if self.tools.mode != Mode::None => {
                let before = self.menu_hover_control();
                self.pointer = Some((x, y));
                let hover_changed = before != self.menu_hover_control();
                if let Some(index) = self.tool_row_at(x, y)
                    && self.tools.selected != index
                {
                    self.tools.selected = index;
                    return Ok(Some(true));
                }
                Ok(Some(hover_changed))
            }
            Action::PointerLeave if self.tools.mode != Mode::None => {
                let hovered = self.menu_hover_control().is_some();
                self.pointer = None;
                Ok(Some(hovered))
            }
            _ => Ok(None),
        }
    }
    pub(super) fn draw_marks(&mut self) -> WindowResult<()> {
        let palette = self.session.settings().theme.palette();
        let mut commands = Vec::new();
        let mut ranges: Vec<_> = self
            .tools
            .annotations
            .iter()
            .filter(|row| {
                matches!(row.kind, Kind::Highlight | Kind::Note)
                    && row.locator.spine_index() == self.session.current_spine()
            })
            .filter_map(|row| row.end.map(|end| (row.locator.utf8_offset() as usize, end)))
            .collect();
        ranges.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (start, end) in ranges {
            if let Some(last) = merged.last_mut().filter(|last| start <= last.1) {
                last.1 = last.1.max(end);
            } else {
                merged.push((start, end));
            }
        }
        for hit in &self.session.frame().hits {
            let at = merged.partition_point(|(_, end)| *end <= hit.start);
            if merged.get(at).is_some_and(|(start, _)| *start < hit.end) {
                commands.push(DrawCommand::FillRect {
                    rect: hit.rect,
                    color: palette.highlight,
                });
            }
        }
        if let Some(range) = &self.tools.selection {
            for hit in &self.session.frame().hits {
                if hit.start < range.end && hit.end > range.start {
                    commands.push(DrawCommand::FillRect {
                        rect: hit.rect,
                        color: palette.selection,
                    });
                }
            }
        }
        for batch in commands.chunks(8192) {
            self.surface.draw(batch)?;
        }
        Ok(())
    }
    pub(super) fn draw_tools(&mut self) -> WindowResult<()> {
        self.draw_modern_tools()
    }
    fn start_clipboard(&mut self, text: Option<String>) -> WindowResult<()> {
        if let Some(effects) = &mut self.host_effects {
            if text.as_ref().is_some_and(|s| s.len() > 128 * 1024) {
                return Err("selection exceeds clipboard limit".into());
            }
            if effects.len() >= 8 {
                return Err("系统操作队列已满".into());
            }
            effects.push_back(match text {
                Some(text) => HostEffect::Copy(text),
                None => HostEffect::Paste,
            });
            self.tools.status = "正在访问系统剪贴板…".into();
            return Ok(());
        }
        #[cfg(not(all(target_os = "linux", feature = "wayland")))]
        {
            let _ = text;
            Err("clipboard host is not attached".into())
        }
        #[cfg(all(target_os = "linux", feature = "wayland"))]
        {
            if self.tools.clipboard.is_some() {
                return Err("剪贴板操作尚未结束".into());
            }
            let (sender, receiver) = mpsc::channel();
            std::thread::spawn(move || {
                let result = (|| -> std::result::Result<ClipboardResult, String> {
                    if let Some(text) = text {
                        if text.len() > 128 * 1024 {
                            return Err("selection exceeds clipboard limit".into());
                        }
                        let opts = wl_clipboard_rs::copy::Options::new();
                        opts.copy(
                            wl_clipboard_rs::copy::Source::Bytes(text.into_bytes().into()),
                            wl_clipboard_rs::copy::MimeType::Specific(
                                "text/plain;charset=utf-8".into(),
                            ),
                        )
                        .map_err(|e| e.to_string())?;
                        Ok(ClipboardResult::Copied)
                    } else {
                        use std::io::Read;
                        let (pipe, _) = wl_clipboard_rs::paste::get_contents(
                            wl_clipboard_rs::paste::ClipboardType::Regular,
                            wl_clipboard_rs::paste::Seat::Unspecified,
                            wl_clipboard_rs::paste::MimeType::Text,
                        )
                        .map_err(|e| e.to_string())?;
                        let mut data = Vec::new();
                        pipe.take(128 * 1024 + 1)
                            .read_to_end(&mut data)
                            .map_err(|e| e.to_string())?;
                        if data.len() > 128 * 1024 {
                            return Err("clipboard exceeds text limit".into());
                        }
                        Ok(ClipboardResult::Pasted(
                            String::from_utf8(data).map_err(|e| e.to_string())?,
                        ))
                    }
                })();
                let _ = sender.send(result.unwrap_or_else(ClipboardResult::Failed));
            });
            self.tools.clipboard = Some(receiver);
            self.tools.started = Some(Instant::now());
            self.tools.status = "正在访问剪贴板…".into();
            Ok(())
        }
    }
    pub(super) fn poll_clipboard(&mut self) -> bool {
        let Some(receiver) = &self.tools.clipboard else {
            return false;
        };
        match receiver.try_recv() {
            Ok(result) => {
                match result {
                    ClipboardResult::Copied => self.tools.status = "已复制选中文字".into(),
                    ClipboardResult::Failed(error) => {
                        self.tools.status = format!("剪贴板不可用：{error}")
                    }
                    ClipboardResult::Pasted(text) => {
                        if self.tools.editing() {
                            let limit = if self.tools.mode == Mode::Search {
                                1024
                            } else {
                                8192
                            };
                            for ch in text
                                .chars()
                                .filter(|ch| !ch.is_control() || *ch == '\n' || *ch == '\t')
                            {
                                if self.tools.query.len() + ch.len_utf8() > limit {
                                    break;
                                }
                                self.tools.query.push(ch);
                            }
                            self.tools.dirty = true;
                            self.tools.status = "已粘贴；Enter 提交".into();
                        } else {
                            self.tools.status = "输入框已关闭，未插入剪贴板内容".into();
                        }
                    }
                }
                self.tools.clipboard = None;
                self.tools.started = None;
                true
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.tools.clipboard = None;
                self.tools.status = "剪贴板请求已结束".into();
                true
            }
            Err(mpsc::TryRecvError::Empty) => {
                if self
                    .tools
                    .started
                    .is_some_and(|start| start.elapsed() > Duration::from_secs(5))
                {
                    self.tools.started = None;
                    self.tools.status = "剪贴板响应缓慢；阅读仍可继续".into();
                    return true;
                }
                false
            }
        }
    }
}

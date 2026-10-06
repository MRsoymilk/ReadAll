//! Fixed-page PDF worker using the same mobile frame/state/JNI contract as EPUB.
use super::{Appearance, Command, Config, ContentsItem, Frame, Shared};
use crate::{
    loading,
    progress::PdfProgressStore,
    reader_data::{Settings, Store},
    ui::{UiFont, UiPainter},
};
use readall_font::FontLimits;
use readall_pdf::{Document, Limits};
use readall_platform::{
    LocalFileSource,
    window::{Action as UiAction, ReaderCommand as UiCommand},
};
use readall_render::{DrawCommand, Rect, RenderLimits, Surface};
use std::{error::Error, sync::Arc, time::Duration};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct PdfPresentation {
    document: Document,
    title: String,
    font: UiFont,
    settings: Settings,
    settings_store: Store,
    progress: Option<PdfProgressStore>,
    page: usize,
    logical: (u32, u32),
    pixels: (u32, u32),
    expanded: bool,
    notice: String,
    touch_start: Option<(i32, i32)>,
    surface: Surface,
}

impl PdfPresentation {
    fn new(
        document: Document,
        title: String,
        font: UiFont,
        settings: Settings,
        settings_store: Store,
        progress: Option<PdfProgressStore>,
        page: usize,
        logical: (u32, u32),
        pixels: (u32, u32),
    ) -> Result<Self> {
        let surface = Surface::new_scaled(logical, pixels, RenderLimits::default())?;
        let mut this = Self {
            document,
            title,
            font,
            settings,
            settings_store,
            progress,
            page,
            logical,
            pixels,
            expanded: true,
            notice: String::new(),
            touch_start: None,
            surface,
        };
        this.render()?;
        Ok(this)
    }

    fn save(&self) {
        if let Some(progress) = &self.progress {
            let _ = progress.save(self.document.id(), self.page);
        }
    }

    fn page_count(&self) -> usize {
        self.document.page_count()
    }

    fn progress(&self) -> f32 {
        if self.page_count() <= 1 {
            1.0
        } else {
            self.page as f32 / (self.page_count() - 1) as f32
        }
    }

    fn locator(&self) -> String {
        format!("pdf:{}:page-{}", self.document.id(), self.page)
    }

    fn move_to(&mut self, page: usize) -> Result<bool> {
        let page = page.min(self.page_count().saturating_sub(1));
        if page == self.page {
            return Ok(false);
        }
        self.page = page;
        self.notice.clear();
        self.render()?;
        self.save();
        Ok(true)
    }

    fn resize(&mut self, logical: (u32, u32), pixels: (u32, u32)) -> Result<bool> {
        if self.logical == logical && self.pixels == pixels {
            return Ok(false);
        }
        self.logical = logical;
        self.pixels = pixels;
        self.render()?;
        Ok(true)
    }

    fn toggle_theme(&mut self) -> Result<bool> {
        self.settings.theme = self.settings.theme.next();
        self.settings_store.save_settings(self.settings)?;
        self.render()?;
        Ok(true)
    }

    fn unsupported(&mut self, label: &str) -> Result<bool> {
        self.notice = format!("PDF 固定页阅读暂不支持{label}");
        self.render()?;
        Ok(true)
    }

    fn touch(&mut self, kind: u32, x: i32, y: i32) -> Result<bool> {
        match kind {
            0 => {
                self.touch_start = Some((x, y));
                Ok(false)
            }
            1 => {
                self.touch_start = None;
                let width = self.logical.0 as i32;
                if self.expanded && y >= self.logical.1 as i32 - 64 {
                    if x < width / 2 {
                        self.move_to(self.page.saturating_sub(1))
                    } else {
                        self.move_to(self.page.saturating_add(1))
                    }
                } else if x < width / 4 {
                    self.move_to(self.page.saturating_sub(1))
                } else if x > width * 3 / 4 {
                    self.move_to(self.page.saturating_add(1))
                } else {
                    self.expanded = !self.expanded;
                    self.render()?;
                    Ok(true)
                }
            }
            4 => {
                let start = self.touch_start.take();
                if let Some((sx, sy)) = start {
                    let dx = x - sx;
                    let dy = y - sy;
                    if dx.abs() >= 48 && dx.abs() > dy.abs() {
                        if dx < 0 {
                            return self.move_to(self.page.saturating_add(1));
                        }
                        return self.move_to(self.page.saturating_sub(1));
                    }
                }
                Ok(false)
            }
            8 => {
                self.touch_start = None;
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn render(&mut self) -> Result<()> {
        let palette = self.settings.theme.palette();
        let (width, height) = self.logical;
        let mut surface = Surface::new_scaled(self.logical, self.pixels, RenderLimits::default())?;
        surface.draw(&[DrawCommand::FillRect {
            rect: Rect::new(0, 0, width, height),
            color: palette.canvas,
        }])?;

        let top = if self.expanded { 54 } else { 16 };
        let bottom = if self.expanded { 58 } else { 18 };
        if self.expanded {
            surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(0, 0, width, 46),
                    color: palette.panel,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(0, height as i32 - 50, width, 50),
                    color: palette.panel,
                },
            ])?;
        }

        let logical_content = Rect::new(
            12,
            top,
            width.saturating_sub(24),
            height.saturating_sub(top as u32 + bottom as u32),
        );
        let physical = surface.pixel_rect(logical_content);
        let page =
            self.document
                .render_fit(self.page, physical.width.max(1), physical.height.max(1))?;
        let px = physical.x.max(0) as u32 + (physical.width - page.width) / 2;
        let py = physical.y.max(0) as u32 + (physical.height - page.height) / 2;
        let (sx, sy) = surface.pixel_scale();
        let border = Rect::new(
            ((px as f32 / sx).floor() as i32).saturating_sub(1),
            ((py as f32 / sy).floor() as i32).saturating_sub(1),
            ((page.width as f32 / sx).ceil() as u32).saturating_add(2),
            ((page.height as f32 / sy).ceil() as u32).saturating_add(2),
        );
        surface.draw(&[DrawCommand::FillRect {
            rect: border,
            color: palette.border,
        }])?;
        surface.write_rgba_pixels(px, py, page.width, page.height, &page.rgba)?;

        if self.expanded {
            let mut ui = UiPainter::new(&self.font, &mut surface)?;
            let position = format!("第 {}/{} 页", self.page + 1, self.page_count());
            let position_width = ui.measure(13, &position)?;
            let title_width = width.saturating_sub(position_width + 56);
            let title = ui.fit(16, &self.title, title_width)?;
            ui.draw(16, 13, 16, &title, palette.ink)?;
            ui.draw(
                width.saturating_sub(position_width + 16) as i32,
                15,
                13,
                &position,
                palette.muted,
            )?;
            let footer = "‹ 上一页            下一页 ›";
            let footer_width = ui.measure(14, footer)?;
            ui.draw(
                ((width.saturating_sub(footer_width)) / 2) as i32,
                height as i32 - 35,
                14,
                footer,
                palette.ink,
            )?;
            if !self.notice.is_empty() {
                let message = ui.fit(12, &self.notice, width.saturating_sub(32))?;
                ui.draw(16, height as i32 - 76, 12, &message, palette.muted)?;
            }
        }
        self.surface = surface;
        Ok(())
    }
}

pub(super) fn worker(config: Config, shared: &Shared, bytes: Vec<u8>) -> Result<()> {
    loading::stage("解析 PDF 页面结构")?;
    let document = Document::parse(bytes, Limits::default())?;
    let title = {
        let title = document.metadata().title.trim();
        if title.is_empty() {
            config
                .book
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("PDF")
                .to_owned()
        } else {
            title.replace(['\n', '\r', '\0'], " ")
        }
    };

    loading::stage("准备 PDF 阅读界面")?;
    let mut source = LocalFileSource::open(&config.font)?;
    let font_bytes = loading::read(
        &mut source,
        FontLimits::default().max_file_bytes,
        "准备中文字体",
    )?;
    let ui_font = UiFont::from_bytes_face(font_bytes, config.font.clone(), 0)?;
    let settings_store = Store::new(config.state_dir.join("library-v1"));
    let settings = if settings_store.root().join("settings.conf").is_file() {
        settings_store.settings()?
    } else {
        Settings {
            size: config.font_size,
            margin: config.margin,
            ..Settings::default()
        }
    };
    let candidate_progress = PdfProgressStore::new(config.state_dir.join("progress-v1"));
    let (progress, page, progress_notice) =
        match candidate_progress.load(document.id(), document.page_count()) {
            Ok(saved) => (Some(candidate_progress), saved.unwrap_or(0), None),
            Err(error) => (
                None,
                0,
                Some(format!(
                    "旧 PDF 进度不可用，已从第一页打开；原记录未覆盖：{error}"
                )),
            ),
        };
    let pixels = config.raster_size.unwrap_or((config.width, config.height));
    let mut ui = PdfPresentation::new(
        document,
        title,
        ui_font,
        settings,
        settings_store,
        progress,
        page,
        (config.width, config.height),
        pixels,
    )?;
    if let Some(message) = progress_notice {
        ui.notice = message;
        ui.render()?;
    }

    publish(shared, &ui, true);
    let mut closed = false;
    while !shared.tracker.is_cancelled() && !closed {
        let Some(command) = shared.inbox.receive(Duration::from_millis(80)) else {
            shared.update(|snapshot| snapshot.busy = false);
            continue;
        };
        shared.update(|snapshot| snapshot.busy = true);
        let changed = match command {
            Command::Next => ui.move_to(ui.page.saturating_add(1))?,
            Command::Previous => ui.move_to(ui.page.saturating_sub(1))?,
            Command::First => ui.move_to(0)?,
            Command::Last => ui.move_to(ui.page_count().saturating_sub(1))?,
            Command::Resize { width, height } => ui.resize((width, height), ui.pixels)?,
            Command::Viewport {
                width,
                height,
                pixel_width,
                pixel_height,
            } => ui.resize((width, height), (pixel_width, pixel_height))?,
            Command::CycleTheme | Command::Ui(UiAction::Command(UiCommand::Theme)) => {
                ui.toggle_theme()?
            }
            Command::Contents => {
                let items = (0..ui.page_count().min(20_000))
                    .map(|page| ContentsItem {
                        title: format!("第 {} 页", page + 1),
                        depth: 0,
                        spine: page,
                        offset: 0,
                    })
                    .collect();
                shared.update(|snapshot| snapshot.contents = Arc::new(items));
                false
            }
            Command::Jump { spine, .. } => ui.move_to(spine)?,
            Command::Touch { kind, x, y } => ui.touch(kind, x, y)?,
            Command::Save | Command::Pause(true) => {
                ui.save();
                false
            }
            Command::Pause(false) => false,
            Command::Back | Command::Ui(UiAction::Back | UiAction::Close) => {
                ui.save();
                closed = true;
                false
            }
            Command::Ui(UiAction::Command(UiCommand::Find)) => ui.unsupported("全文搜索")?,
            Command::Bookmark
            | Command::Ui(UiAction::Command(UiCommand::Bookmarks | UiCommand::Bookmark)) => {
                ui.unsupported("书签")?
            }
            Command::Ui(UiAction::Command(
                UiCommand::Note | UiCommand::Highlight | UiCommand::Select | UiCommand::Delete,
            )) => ui.unsupported("标注")?,
            Command::Ui(UiAction::Command(UiCommand::Settings)) => {
                ui.notice = "PDF 固定页模式仅使用全局亮暗主题；字号与边距不改变原始页面".into();
                ui.render()?;
                true
            }
            Command::Ui(UiAction::Command(UiCommand::Copy | UiCommand::Paste))
            | Command::Ui(UiAction::Activate)
            | Command::Input { .. }
            | Command::HostReply { .. } => ui.unsupported("文本编辑操作")?,
            Command::Larger | Command::Smaller => {
                ui.notice = "PDF 保持原始页面比例；当前未启用缩放".into();
                ui.render()?;
                true
            }
            Command::Ui(_) => false,
        };
        if changed {
            publish(shared, &ui, true);
        } else {
            publish(shared, &ui, false);
        }
    }
    ui.save();
    Ok(())
}

fn publish(shared: &Shared, ui: &PdfPresentation, changed: bool) {
    let surface = changed.then(|| ui.surface.clone());
    shared.update(|snapshot| {
        snapshot.busy = false;
        snapshot.title = ui.title.clone();
        snapshot.position = format!("第 {}/{} 页", ui.page + 1, ui.page_count());
        snapshot.locator = ui.locator();
        snapshot.progress = ui.progress();
        snapshot.ui_mode = if ui.expanded { "expanded" } else { "collapsed" };
        snapshot.page_mode = "pdf";
        snapshot.appearance = Appearance::from_theme(ui.settings.theme);
        snapshot.animating = false;
        snapshot.editing = false;
        snapshot.input.clear();
        snapshot.notice = ui.notice.clone();
        if let Some(surface) = surface {
            let serial = snapshot.frame.as_ref().map_or(1, |frame| frame.serial + 1);
            snapshot.frame = Some(Arc::new(Frame { serial, surface }));
        }
    });
}

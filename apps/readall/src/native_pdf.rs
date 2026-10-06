//! Fixed-page PDF window for the Linux Wayland frontend.
use crate::{
    progress::PdfProgressStore,
    reader_data::{Settings, Store, Theme},
    ui::{UiFont, UiPainter},
};
use readall_core::read_bounded;
use readall_pdf::{Document, Limits};
use readall_platform::{
    LocalFileSource,
    window::{self, Action, ReaderCommand, WindowHandler, WindowOptions, WindowResult},
};
use readall_render::{DrawCommand, Rect, RenderLimits, Surface};
use std::{error::Error, io::Write, path::Path};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub(crate) fn open_path(path: &Path, output: &mut impl Write) -> Result<()> {
    crate::loading::stage("读取 PDF 文件")?;
    let mut source = LocalFileSource::open(path)?;
    let bytes = read_bounded(&mut source, Limits::default().max_file_bytes)?;
    crate::loading::stage("解析 PDF 页面结构")?;
    let document = Document::parse(bytes, Limits::default())?;
    let title = if document.metadata().title.trim().is_empty() {
        path.file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("PDF")
            .to_owned()
    } else {
        document
            .metadata()
            .title
            .trim()
            .replace(['\n', '\r', '\0'], " ")
    };
    let store = Store::from_environment().ok();
    let settings = store
        .as_ref()
        .and_then(|store| store.settings().ok())
        .unwrap_or_default();
    let mut progress = PdfProgressStore::from_environment().ok();
    let restored = progress
        .as_ref()
        .map(|store| store.load(document.id(), document.page_count()));
    let start = match restored {
        Some(Ok(Some(page))) => page,
        Some(Ok(None)) | None => 0,
        Some(Err(error)) => {
            writeln!(
                output,
                "PDF reading progress disabled for this session; original state preserved: {error}"
            )?;
            progress = None;
            0
        }
    };
    let ui_font = UiFont::system()?;
    let mut reader = PdfWindow::new(document, title, ui_font, settings, store, progress, start)?;
    writeln!(output, "Input format: PDF")?;
    let report = window::run(&mut reader, WindowOptions::default())?;
    reader.save_progress();
    writeln!(
        output,
        "PDF reader closed after {} frames at {}x{}",
        report.committed_frames, report.width, report.height
    )?;
    Ok(())
}

struct PdfWindow {
    document: Document,
    title: String,
    ui_font: UiFont,
    settings: Settings,
    settings_store: Option<Store>,
    progress: Option<PdfProgressStore>,
    page: usize,
    surface: Surface,
    closed: bool,
}

impl PdfWindow {
    fn new(
        document: Document,
        title: String,
        ui_font: UiFont,
        settings: Settings,
        settings_store: Option<Store>,
        progress: Option<PdfProgressStore>,
        page: usize,
    ) -> Result<Self> {
        let surface = Surface::new(900, 700, RenderLimits::default())?;
        let mut this = Self {
            document,
            title,
            ui_font,
            settings,
            settings_store,
            progress,
            page,
            surface,
            closed: false,
        };
        this.render()?;
        Ok(this)
    }

    fn save_progress(&self) {
        if let Some(store) = &self.progress {
            let _ = store.save(self.document.id(), self.page);
        }
    }

    fn move_to(&mut self, page: usize) -> WindowResult<bool> {
        let target = page.min(self.document.page_count().saturating_sub(1));
        if target == self.page {
            return Ok(false);
        }
        self.page = target;
        self.render()?;
        self.save_progress();
        Ok(true)
    }

    fn cycle_theme(&mut self) -> WindowResult<bool> {
        self.settings.theme = self.settings.theme.next();
        if let Some(store) = &self.settings_store {
            store.save_settings(self.settings)?;
        }
        self.render()?;
        Ok(true)
    }

    fn render(&mut self) -> WindowResult<()> {
        let (width, height) = (self.surface.width(), self.surface.height());
        let palette = self.settings.theme.palette();
        let mut surface = Surface::new(width, height, RenderLimits::default())?;
        surface.draw(&[
            DrawCommand::FillRect {
                rect: Rect::new(0, 0, width, height),
                color: palette.canvas,
            },
            DrawCommand::FillRect {
                rect: Rect::new(0, 0, width, 54),
                color: palette.panel,
            },
            DrawCommand::FillRect {
                rect: Rect::new(0, height as i32 - 54, width, 54),
                color: palette.panel,
            },
        ])?;

        let content_width = width.saturating_sub(32).max(1);
        let content_height = height.saturating_sub(132).max(1);
        let page = self
            .document
            .render_fit(self.page, content_width, content_height)?;
        let left = (width - page.width) / 2;
        let top = 66 + (content_height - page.height) / 2;
        // Subtle border/shadow is drawn before the opaque PDF page.
        surface.draw(&[DrawCommand::FillRect {
            rect: Rect::new(
                left.saturating_sub(2) as i32,
                top.saturating_sub(2) as i32,
                page.width.saturating_add(4),
                page.height.saturating_add(4),
            ),
            color: palette.border,
        }])?;
        surface.write_rgba_pixels(left, top, page.width, page.height, &page.rgba)?;

        let mut ui = UiPainter::new(&self.ui_font, &mut surface)?;
        let available = width.saturating_sub(190);
        let title = ui.fit(18, &self.title, available)?;
        ui.draw(18, 15, 18, &title, palette.ink)?;
        let position = format!("第 {}/{} 页", self.page + 1, self.document.page_count());
        let position_width = ui.measure(14, &position)?;
        ui.draw(
            width.saturating_sub(position_width + 18) as i32,
            18,
            14,
            &position,
            palette.muted,
        )?;
        let footer = "← 上一页          下一页 →";
        let footer_width = ui.measure(15, footer)?;
        ui.draw(
            ((width.saturating_sub(footer_width)) / 2) as i32,
            height as i32 - 38,
            15,
            footer,
            palette.ink,
        )?;
        ui.draw(18, height as i32 - 37, 12, "PDF · 固定页面", palette.muted)?;
        self.surface = surface;
        Ok(())
    }
}

impl WindowHandler for PdfWindow {
    fn resize(&mut self, width: u32, height: u32) -> WindowResult<bool> {
        if width < 256 || height < 256 || width > 4096 || height > 4096 {
            return Ok(false);
        }
        if (self.surface.width(), self.surface.height()) == (width, height) {
            return Ok(false);
        }
        self.surface = Surface::new(width, height, RenderLimits::default())?;
        self.render()?;
        Ok(true)
    }

    fn minimum_size(&self) -> (u32, u32) {
        (320, 360)
    }

    fn action(&mut self, action: Action) -> WindowResult<bool> {
        match action {
            Action::Next => self.move_to(self.page.saturating_add(1)),
            Action::Previous => self.move_to(self.page.saturating_sub(1)),
            Action::First => self.move_to(0),
            Action::Last => self.move_to(self.document.page_count().saturating_sub(1)),
            Action::Scroll { dy, .. } if dy > 0 => self.move_to(self.page.saturating_add(1)),
            Action::Scroll { dy, .. } if dy < 0 => self.move_to(self.page.saturating_sub(1)),
            Action::Command(ReaderCommand::Theme) => self.cycle_theme(),
            Action::Click { x, y } | Action::PointerRelease { x, y }
                if y >= self.surface.height() as i32 - 60 =>
            {
                if x < self.surface.width() as i32 / 2 {
                    self.move_to(self.page.saturating_sub(1))
                } else {
                    self.move_to(self.page.saturating_add(1))
                }
            }
            Action::Close | Action::Back => {
                self.closed = true;
                self.save_progress();
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn surface(&self) -> &Surface {
        &self.surface
    }

    fn title(&self) -> String {
        format!("{} — ReadAll", self.title)
    }

    fn dark_theme(&self) -> bool {
        self.settings.theme == Theme::Dark
    }

    fn close_requested(&self) -> bool {
        self.closed
    }
}

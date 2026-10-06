//! Native ReadAll library and book file browser.
use std::io::Write;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(not(all(target_os = "linux", feature = "wayland")))]
pub(crate) fn run(_: &mut impl Write) -> Result<()> {
    Err("ReadAll GUI is unavailable in this build; use --no-default-features only for headless CLI builds".into())
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod enabled {
    mod desktop;
    mod preview;
    mod recent_rows;
    #[cfg(test)]
    mod recent_tests;
    use super::*;
    use crate::{
        reader_data::{Store, Theme},
        recent::RecentStore,
        ui::{UiFont, UiPainter},
    };
    use readall_core::read_bounded;
    use readall_epub::{EpubBook, EpubLimits};
    use readall_platform::{
        LocalFileSource,
        window::{self, Action, WindowHandler, WindowOptions, WindowReport, WindowResult},
    };
    use readall_render::{DrawCommand, Rect, RenderLimits, Surface};
    use recent_rows::RecentRowView;
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    const ROW_HEIGHT: i32 = 48;
    const LIST_TOP: i32 = 150;
    const LIST_BOTTOM_MARGIN: i32 = 96;
    const PREVIEW_MAX_BOOK_BYTES: u64 = 16 * 1024 * 1024;
    const RECENT_TOP: i32 = 366;
    const RECENT_ROW_HEIGHT: i32 = 34;

    #[derive(Debug, Clone)]
    struct FileEntry {
        name: String,
        path: PathBuf,
        directory: bool,
    }

    #[derive(Debug, Clone)]
    struct Browser {
        directory: PathBuf,
        entries: Vec<FileEntry>,
        selected: usize,
        scroll: usize,
        preview: Option<String>,
    }

    impl Browser {
        fn load(directory: PathBuf) -> WindowResult<Self> {
            let mut entries = Vec::new();
            for item in fs::read_dir(&directory)? {
                if entries.len() >= 512 {
                    break;
                }
                let item = match item {
                    Ok(item) => item,
                    Err(_) => continue,
                };
                let name = item.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let kind = match item.file_type() {
                    Ok(kind) => kind,
                    Err(_) => continue,
                };
                let directory = kind.is_dir();
                let book_file =
                    kind.is_file() && crate::publication::path_format(&item.path()).is_some();
                if directory || book_file {
                    entries.push(FileEntry {
                        name,
                        path: item.path(),
                        directory,
                    });
                }
            }
            entries.sort_by(|a, b| {
                b.directory
                    .cmp(&a.directory)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            let mut browser = Self {
                directory,
                entries,
                selected: 0,
                scroll: 0,
                preview: None,
            };
            browser.refresh_preview();
            Ok(browser)
        }

        fn selected(&self) -> Option<&FileEntry> {
            self.entries.get(self.selected)
        }

        fn move_by(&mut self, delta: isize, visible: usize) {
            if self.entries.is_empty() {
                return;
            }
            self.selected = if delta < 0 {
                self.selected.saturating_sub(delta.unsigned_abs())
            } else {
                self.selected
                    .saturating_add(delta as usize)
                    .min(self.entries.len() - 1)
            };
            self.keep_visible(visible);
            self.refresh_preview();
        }

        fn first(&mut self) {
            self.selected = 0;
            self.scroll = 0;
            self.refresh_preview();
        }

        fn last(&mut self, visible: usize) {
            if self.entries.is_empty() {
                return;
            }
            self.selected = self.entries.len() - 1;
            self.keep_visible(visible);
            self.refresh_preview();
        }

        fn keep_visible(&mut self, visible: usize) {
            let visible = visible.max(1);
            if self.selected < self.scroll {
                self.scroll = self.selected;
            } else if self.selected >= self.scroll.saturating_add(visible) {
                self.scroll = self.selected + 1 - visible;
            }
        }

        fn refresh_preview(&mut self) {
            let Some(entry) = self.selected().cloned() else {
                self.preview = None;
                return;
            };
            self.preview = if entry.directory {
                None
            } else {
                Some("正在读取元数据预览…".into())
            };
        }
    }

    fn book_preview(path: &Path) -> String {
        let size = match fs::metadata(path) {
            Ok(metadata) => metadata.len(),
            Err(_) => return "元数据预览不可用，仍可尝试打开".into(),
        };
        if size > PREVIEW_MAX_BOOK_BYTES {
            return format!(
                "{} · {:.1} MiB · 元数据预览已跳过（预览上限 16 MiB）",
                crate::publication::path_format(path).map_or("图书", |f| f.label()),
                size as f64 / (1024.0 * 1024.0)
            );
        }
        let mut limits = EpubLimits::default();
        limits.zip.max_archive_bytes = PREVIEW_MAX_BOOK_BYTES as usize;
        let parsed = (|| -> Result<String> {
            let mut source = LocalFileSource::open(path)?;
            let bytes = read_bounded(&mut source, limits.zip.max_archive_bytes)?;
            if readall_pdf::is_pdf(&bytes) {
                let document = readall_pdf::Document::parse(bytes, readall_pdf::Limits::default())?;
                let meta = document.metadata();
                let title = if meta.title.trim().is_empty() {
                    "(未命名)"
                } else {
                    meta.title.trim()
                };
                let author = if meta.author.trim().is_empty() {
                    "作者未知"
                } else {
                    meta.author.trim()
                };
                return Ok(format!(
                    "《{title}》 · {author} · {} 页 · PDF",
                    document.page_count()
                ));
            }
            if readall_mobi::is_mobi(&bytes) {
                let book =
                    readall_mobi::MobiBook::parse(&bytes, readall_mobi::MobiLimits::default())?;
                let meta = book.metadata();
                return Ok(format!(
                    "《{}》 · {} · {} · {}",
                    meta.title,
                    meta.author.as_deref().unwrap_or("作者未知"),
                    meta.language.as_deref().unwrap_or("语言未知"),
                    if meta.version == 8 { "AZW3" } else { "MOBI" }
                ));
            }
            let book = EpubBook::parse(&bytes, limits)?;
            let title = book.title().unwrap_or("(未命名)");
            let creator = book.creator().unwrap_or("作者未知");
            let language = book.language().unwrap_or("语言未知");
            Ok(format!("《{title}》 · {creator} · {language}"))
        })();
        parsed.unwrap_or_else(|_| "元数据预览不可用，仍可尝试打开".into())
    }

    enum Mode {
        Library,
        Browser(Browser),
    }

    fn point_in(rect: Rect, x: i32, y: i32) -> bool {
        let right = i64::from(rect.x) + i64::from(rect.width);
        let bottom = i64::from(rect.y) + i64::from(rect.height);
        i64::from(x) >= i64::from(rect.x)
            && i64::from(x) < right
            && i64::from(y) >= i64::from(rect.y)
            && i64::from(y) < bottom
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum HoverTarget {
        None,
        OpenCard,
        RecentRow(usize),
        RecentDelete(usize),
        BrowserBack,
        BrowserRow(usize),
        Sidebar(usize),
        Theme,
    }

    struct Home<'font> {
        theme: Theme,
        theme_store: Option<Store>,
        surface: Surface,
        ui_font: &'font UiFont,
        mode: Mode,
        recent: Vec<PathBuf>,
        recent_store: Option<RecentStore>,
        recent_rows: Vec<RecentRowView>,
        recent_scroll: usize,
        recent_selected: Option<usize>,
        wheel_remainder: f64,
        preview_worker: preview::Preview,
        status_notice: Option<(String, std::time::Instant)>,
        pending_recent_delete: Option<PathBuf>,
        selected_book: Option<PathBuf>,
        close_requested: bool,
        pointer: Option<(i32, i32)>,
        status: String,
    }

    impl<'font> Home<'font> {
        fn new(width: u32, height: u32, ui_font: &'font UiFont) -> WindowResult<Self> {
            let recent_store = RecentStore::from_environment().ok();
            let recent = recent_store
                .as_ref()
                .and_then(|store| store.load().ok())
                .unwrap_or_default();
            let mut home = Self {
                theme: Theme::Paper,
                theme_store: None,
                surface: Surface::new(width, height, RenderLimits::default())?,
                ui_font,
                mode: Mode::Library,
                recent,
                recent_store,
                recent_rows: Vec::new(),
                recent_scroll: 0,
                recent_selected: None,
                wheel_remainder: 0.0,
                preview_worker: preview::Preview::new()?,
                status_notice: None,
                pending_recent_delete: None,
                selected_book: None,
                close_requested: false,
                pointer: None,
                status: "支持 EPUB / MOBI / AZW3 / PDF，点击“打开图书”开始".into(),
            };
            home.paint()?;
            Ok(home)
        }

        fn theme_rect(&self) -> Rect {
            Rect::new(24, self.surface.height().saturating_sub(82) as i32, 166, 42)
        }
        fn toggle_theme(&mut self) -> WindowResult<bool> {
            let theme = self.theme.next();
            if let Some(store) = &self.theme_store
                && let Err(error) = store.save_theme(theme)
            {
                self.status = format!("主题保存失败：{error}");
                self.paint()?;
                return Ok(true);
            }
            self.theme = theme;
            self.status = format!("已切换为{}主题", theme.label());
            self.paint()?;
            Ok(true)
        }
        fn default_directory() -> PathBuf {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute() && path.is_dir())
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("/"))
        }

        fn open_browser(&mut self) -> WindowResult<bool> {
            self.pending_recent_delete = None;
            self.recent_rows.clear();
            self.mode = Mode::Browser(Browser::load(Self::default_directory())?);
            self.status = "选择 EPUB / MOBI / AZW3 / PDF 图书".into();
            self.paint()?;
            Ok(true)
        }

        fn visible_rows(&self) -> usize {
            ((self.surface.height() as i32 - LIST_TOP - LIST_BOTTOM_MARGIN) / ROW_HEIGHT).max(1)
                as usize
        }

        fn open_card_rect(&self) -> Rect {
            Rect::new(
                250,
                120,
                self.surface.width().saturating_sub(280).min(650),
                174,
            )
        }

        fn recent_row_rect(&self, index: usize) -> Rect {
            Rect::new(
                270,
                RECENT_TOP + index as i32 * RECENT_ROW_HEIGHT,
                self.surface.width().saturating_sub(320).min(610),
                (RECENT_ROW_HEIGHT - 4) as u32,
            )
        }

        fn activate_recent(&mut self, index: usize) -> WindowResult<bool> {
            let Some(path) = self
                .recent
                .get(self.recent_scroll.saturating_add(index))
                .cloned()
            else {
                return Ok(false);
            };
            self.selected_book = Some(path);
            self.close_requested = true;
            Ok(false)
        }

        fn hover_target(&self) -> HoverTarget {
            self.pointer
                .map_or(HoverTarget::None, |(x, y)| self.hover_target_at(x, y))
        }

        fn hover_target_at(&self, x: i32, y: i32) -> HoverTarget {
            if point_in(self.theme_rect(), x, y) {
                return HoverTarget::Theme;
            }
            for index in 0..3 {
                if point_in(self.sidebar_rect(index), x, y) {
                    return HoverTarget::Sidebar(index);
                }
            }
            match &self.mode {
                Mode::Library => {
                    let card = self.open_card_rect();
                    if point_in(card, x, y) {
                        return HoverTarget::OpenCard;
                    }
                    for index in 0..self.visible_recent_count() {
                        if point_in(self.recent_delete_rect(index), x, y) {
                            return HoverTarget::RecentDelete(index);
                        }
                        if point_in(self.recent_row_rect(index), x, y) {
                            return HoverTarget::RecentRow(index);
                        }
                    }
                    HoverTarget::None
                }
                Mode::Browser(browser) => {
                    if (250..=326).contains(&x) && (102..=142).contains(&y) {
                        return HoverTarget::BrowserBack;
                    }
                    let row_width = self.surface.width().saturating_sub(286);
                    let right = 250_i32.saturating_add(row_width as i32);
                    if x < 250 || x >= right || y < LIST_TOP {
                        return HoverTarget::None;
                    }
                    let row = ((y - LIST_TOP) / ROW_HEIGHT) as usize;
                    if row >= self.visible_rows() || (y - LIST_TOP) % ROW_HEIGHT >= ROW_HEIGHT - 5 {
                        return HoverTarget::None;
                    }
                    let index = browser.scroll.saturating_add(row);
                    if index < browser.entries.len() {
                        HoverTarget::BrowserRow(index)
                    } else {
                        HoverTarget::None
                    }
                }
            }
        }

        fn pointer_changed(&mut self, pointer: Option<(i32, i32)>) -> WindowResult<bool> {
            let before = self.hover_target();
            self.pointer = pointer;
            let after = self.hover_target();
            let delete_cancelled = self.pending_recent_delete.as_ref().is_some_and(|path| {
                !matches!(after, HoverTarget::RecentDelete(index) if self.recent.get(self.recent_scroll.saturating_add(index)) == Some(path))
            });
            if delete_cancelled {
                self.pending_recent_delete = None;
            }
            // Pointer and keyboard/wheel navigation share one active row. Do this
            // even within the same hover target: navigation may have moved it.
            let selection_changed = if let (HoverTarget::BrowserRow(index), Mode::Browser(browser)) =
                (after, &mut self.mode)
                && browser.selected != index
            {
                browser.selected = index;
                browser.refresh_preview();
                true
            } else if let HoverTarget::RecentRow(index) | HoverTarget::RecentDelete(index) = after {
                let selected = Some(self.recent_scroll.saturating_add(index));
                let changed = selected != self.recent_selected;
                self.recent_selected = selected;
                changed
            } else {
                false
            };
            if before == after && !selection_changed && !delete_cancelled {
                return Ok(false);
            }
            self.paint()?;
            Ok(true)
        }

        fn activate_browser(&mut self) -> WindowResult<bool> {
            let selected = match &self.mode {
                Mode::Browser(browser) => browser.selected().cloned(),
                Mode::Library => None,
            };
            let Some(selected) = selected else {
                self.status = "当前目录没有可打开的 EPUB / MOBI / AZW3 / PDF 图书".into();
                self.paint()?;
                return Ok(true);
            };
            if selected.directory {
                self.mode = Mode::Browser(Browser::load(selected.path)?);
                self.status = "选择 EPUB / MOBI / AZW3 / PDF 图书".into();
                self.paint()?;
                return Ok(true);
            }
            self.selected_book = Some(selected.path);
            self.close_requested = true;
            Ok(false)
        }

        fn back(&mut self) -> WindowResult<bool> {
            let parent = match &self.mode {
                Mode::Browser(browser) => browser.directory.parent().map(Path::to_path_buf),
                Mode::Library => None,
            };
            match parent {
                Some(parent) => {
                    self.mode = Mode::Browser(Browser::load(parent)?);
                    self.status = "选择 EPUB / MOBI / AZW3 / PDF 图书".into();
                }
                None => {
                    self.mode = Mode::Library;
                    self.status = "支持 EPUB / MOBI / AZW3 / PDF，点击“打开图书”开始".into();
                }
            }
            self.paint()?;
            Ok(true)
        }

        fn paint(&mut self) -> WindowResult<()> {
            self.recent_scroll = self
                .recent_scroll
                .min(self.recent.len().saturating_sub(self.recent_capacity()));
            self.observe_home_notice(std::time::Instant::now());
            self.queue_preview(std::time::Instant::now());
            let palette = self.theme.palette();
            self.surface.draw(&[DrawCommand::FillRect {
                rect: Rect::new(0, 0, self.surface.width(), self.surface.height()),
                color: palette.canvas,
            }])?;
            match &self.mode {
                Mode::Library => self.paint_library(),
                Mode::Browser(browser) => {
                    let browser = browser.clone();
                    self.paint_browser(&browser)
                }
            }
        }

        fn paint_shell(&mut self, section: &str) -> WindowResult<()> {
            let palette = self.theme.palette();
            let h = self.surface.height();
            let w = self.surface.width();
            let theme_button = self.theme_rect();
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(0, 0, 218, h),
                    color: palette.panel,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(218, 0, w.saturating_sub(218), 86),
                    color: palette.panel,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(218, 85, w.saturating_sub(218), 1),
                    color: palette.border,
                },
            ])?;

            let theme_hovered = self.hover_target() == HoverTarget::Theme;
            crate::ui::rounded(
                &mut self.surface,
                theme_button,
                12,
                if theme_hovered {
                    palette.hover
                } else {
                    palette.button
                },
            )?;
            self.paint_sidebar()?;
            let mut text = UiPainter::new(self.ui_font, &mut self.surface)?;
            text.draw(
                theme_button.x + 12,
                theme_button.y + 13,
                13,
                &format!("{}主题 · F6 切换", self.theme.label()),
                palette.ink,
            )?;
            text.draw(28, 24, 28, "ReadAll", palette.ink)?;
            text.draw(30, 62, 13, "本地图书 · 专注阅读", palette.muted)?;
            text.draw(250, 28, 24, section, palette.ink)?;
            Ok(())
        }

        fn paint_library(&mut self) -> WindowResult<()> {
            let palette = self.theme.palette();
            self.paint_shell("书库")?;
            let content_w = self.surface.width().saturating_sub(280);
            let card = self.open_card_rect();
            let hovered = self.hover_target() == HoverTarget::OpenCard;
            crate::ui::rounded(
                &mut self.surface,
                card,
                18,
                if hovered {
                    palette.hover
                } else {
                    palette.panel
                },
            )?;
            crate::ui::rounded(
                &mut self.surface,
                Rect::new(card.x + 18, card.y + 27, 3, 36),
                2,
                palette.accent,
            )?;

            let info_y = 328;
            let recent_panel = Rect::new(
                250,
                info_y,
                content_w.min(650),
                self.surface.height().saturating_sub(info_y as u32 + 54),
            );
            crate::ui::rounded(&mut self.surface, recent_panel, 16, palette.panel)?;
            let footer_y = self.surface.height() as i32 - 42;
            let mut text = UiPainter::new(self.ui_font, &mut self.surface)?;
            text.draw(282, 148, 24, "打开电子书", palette.ink)?;
            let subtitle = text.fit(
                15,
                "支持 EPUB / MOBI / AZW3 / PDF，选择后阅读",
                card.width.saturating_sub(58),
            )?;
            text.draw_clipped(282, 190, 15, &subtitle, palette.muted, card)?;
            text.draw(282, 230, 14, "点击打开 · ↑↓ 选择最近阅读", palette.accent)?;
            text.draw(282, info_y + 20, 17, "最近阅读", palette.ink)?;
            if self.recent.is_empty() {
                text.draw(282, info_y + 62, 14, "暂无最近阅读", palette.muted)?;
                if info_y + 112 < footer_y - 8 {
                    text.draw(
                        282,
                        info_y + 96,
                        13,
                        "成功关闭图书后会自动记录",
                        palette.muted,
                    )?;
                }
            }
            let status = text.fit(14, &self.status, content_w.saturating_sub(24))?;
            text.draw(250, footer_y, 14, &status, palette.muted)?;
            self.paint_recent_rows(std::time::Instant::now())?;
            Ok(())
        }

        fn paint_browser(&mut self, browser: &Browser) -> WindowResult<()> {
            let palette = self.theme.palette();
            self.paint_shell("打开图书")?;
            let top_w = self.surface.width().saturating_sub(278);
            let hover = self.hover_target();
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(242, 102, top_w, 40),
                    color: palette.panel,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(250, 108, 76, 28),
                    color: if hover == HoverTarget::BrowserBack {
                        palette.pressed
                    } else {
                        palette.panel
                    },
                },
            ])?;

            let visible = self.visible_rows();
            let row_width = self.surface.width().saturating_sub(286);
            for (row, _entry) in browser
                .entries
                .iter()
                .skip(browser.scroll)
                .take(visible)
                .enumerate()
            {
                let index = browser.scroll + row;
                let y = LIST_TOP + row as i32 * ROW_HEIGHT;
                let selected = index == browser.selected;
                let row_color = if selected {
                    palette.button
                } else {
                    palette.panel
                };
                crate::ui::rounded(
                    &mut self.surface,
                    Rect::new(250, y, row_width, (ROW_HEIGHT - 5) as u32),
                    8,
                    row_color,
                )?;
                self.surface.draw(&[DrawCommand::FillRect {
                    rect: Rect::new(
                        250,
                        y,
                        if selected { 5 } else { 1 },
                        (ROW_HEIGHT - 5) as u32,
                    ),
                    color: if selected {
                        palette.accent
                    } else {
                        palette.border
                    },
                }])?;
            }

            let footer_y = self.surface.height() as i32 - 42;
            let preview_y = footer_y - 30;
            let path_width = self.surface.width().saturating_sub(360);
            let footer_width = self.surface.width().saturating_sub(280);
            let mut text = UiPainter::new(self.ui_font, &mut self.surface)?;
            text.draw(260, 114, 14, "‹ 返回", palette.accent)?;
            let path = text.fit(13, &browser.directory.display().to_string(), path_width)?;
            text.draw(350, 115, 13, &path, palette.muted)?;

            for (row, entry) in browser
                .entries
                .iter()
                .skip(browser.scroll)
                .take(visible)
                .enumerate()
            {
                let y = LIST_TOP + row as i32 * ROW_HEIGHT;
                text.draw(
                    270,
                    y + 13,
                    13,
                    if entry.directory {
                        "文件夹"
                    } else {
                        crate::publication::path_format(&entry.path).map_or("图书", |f| f.label())
                    },
                    if entry.directory {
                        palette.muted
                    } else {
                        palette.accent
                    },
                )?;
                let name = text.fit(15, &entry.name, row_width.saturating_sub(120))?;
                text.draw(350, y + 11, 15, &name, palette.ink)?;
            }

            if browser.entries.is_empty() {
                text.draw(
                    272,
                    LIST_TOP + 30,
                    15,
                    "当前目录没有 EPUB / MOBI / AZW3 / PDF 文件",
                    palette.muted,
                )?;
            }
            if let Some(preview) = &browser.preview {
                let preview = text.fit(13, preview, footer_width)?;
                text.draw(250, preview_y, 13, &preview, palette.accent)?;
            }
            let footer = if self.status.starts_with("无法访问") {
                self.status.clone()
            } else {
                format!(
                    "{} 项 · Enter 打开 · Backspace 上一级 · Esc 返回书库",
                    browser.entries.len()
                )
            };
            let footer = text.fit(13, &footer, footer_width)?;
            text.draw(250, footer_y, 13, &footer, palette.muted)?;
            Ok(())
        }

        fn browser_action(&mut self, action: Action) -> WindowResult<bool> {
            let visible = self.visible_rows();
            match action {
                Action::Previous | Action::Smaller => {
                    if let Mode::Browser(browser) = &mut self.mode {
                        browser.move_by(-1, visible);
                    }
                    self.paint()?;
                    Ok(true)
                }
                Action::Next | Action::Larger => {
                    if let Mode::Browser(browser) = &mut self.mode {
                        browser.move_by(1, visible);
                    }
                    self.paint()?;
                    Ok(true)
                }
                Action::First => {
                    if let Mode::Browser(browser) = &mut self.mode {
                        browser.first();
                    }
                    self.paint()?;
                    Ok(true)
                }
                Action::Last => {
                    if let Mode::Browser(browser) = &mut self.mode {
                        browser.last(visible);
                    }
                    self.paint()?;
                    Ok(true)
                }
                Action::Activate => self.activate_browser(),
                Action::Back => self.back(),
                Action::PointerMove { x, y } => self.pointer_changed(Some((x, y))),
                Action::PointerLeave => self.pointer_changed(None),
                Action::Click { x, y } => {
                    self.pointer = Some((x, y));
                    match self.hover_target() {
                        HoverTarget::BrowserBack => self.back(),
                        HoverTarget::BrowserRow(target) => {
                            if let Mode::Browser(browser) = &mut self.mode {
                                if browser.selected != target {
                                    browser.selected = target;
                                    browser.refresh_preview();
                                }
                                browser.keep_visible(visible);
                            }
                            self.paint()?;
                            self.activate_browser()
                        }
                        _ => Ok(false),
                    }
                }
                Action::Close
                | Action::Text(_)
                | Action::Command(_)
                | Action::PointerRelease { .. }
                | Action::Scroll { .. }
                | Action::PanStart { .. }
                | Action::PanEnd { .. } => Ok(false),
            }
        }
    }

    impl WindowHandler for Home<'_> {
        fn minimum_size(&self) -> (u32, u32) {
            (680, 460)
        }

        fn resize(&mut self, width: u32, height: u32) -> WindowResult<bool> {
            if (width, height) == (self.surface.width(), self.surface.height()) {
                return Ok(false);
            }
            if width < 680 || height < 460 {
                return Err("ReadAll main window requires at least 680x460".into());
            }
            self.surface = Surface::new(width, height, RenderLimits::default())?;
            self.pending_recent_delete = None;
            self.paint()?;
            Ok(true)
        }

        fn action(&mut self, action: Action) -> WindowResult<bool> {
            if let Some(changed) = self.desktop_home_action(action)? {
                return Ok(changed);
            }
            if action == Action::Command(readall_platform::window::ReaderCommand::Theme)
                || matches!(action, Action::Click { x, y } if point_in(self.theme_rect(), x, y))
            {
                self.pending_recent_delete = None;
                return self.toggle_theme();
            }
            if matches!(self.mode, Mode::Browser(_)) {
                return match self.browser_action(action) {
                    Ok(changed) => Ok(changed),
                    Err(error) => {
                        self.status = format!("无法访问：{error}");
                        self.paint()?;
                        Ok(true)
                    }
                };
            }
            match action {
                Action::Activate => {
                    if let Some(index) = self.recent_selected {
                        self.activate_recent(index.saturating_sub(self.recent_scroll))
                    } else {
                        self.open_browser()
                    }
                }
                Action::Click { x, y } => {
                    self.pending_recent_delete = None;
                    self.pointer = Some((x, y));
                    match self.hover_target() {
                        HoverTarget::OpenCard => self.open_browser(),
                        HoverTarget::RecentDelete(index) => {
                            self.pending_recent_delete = self
                                .recent
                                .get(self.recent_scroll.saturating_add(index))
                                .cloned();
                            self.paint_recent_rows(std::time::Instant::now())?;
                            Ok(true)
                        }
                        HoverTarget::RecentRow(index) => self.activate_recent(index),
                        _ => Ok(false),
                    }
                }
                Action::PointerRelease { x, y } => self.release_recent_delete(x, y),
                Action::Back => Ok(false),
                Action::PointerMove { x, y } => self.pointer_changed(Some((x, y))),
                Action::PointerLeave => self.pointer_changed(None),
                Action::Previous
                | Action::Next
                | Action::First
                | Action::Last
                | Action::Larger
                | Action::Smaller
                | Action::Close
                | Action::Text(_)
                | Action::Command(_)
                | Action::Scroll { .. }
                | Action::PanStart { .. }
                | Action::PanEnd { .. } => Ok(false),
            }
        }

        fn surface(&self) -> &Surface {
            &self.surface
        }

        fn title(&self) -> String {
            match &self.mode {
                Mode::Library => "ReadAll — 书库".into(),
                Mode::Browser(browser) => {
                    format!("ReadAll — 打开图书 — {}", browser.directory.display())
                }
            }
        }

        fn precise_scroll(&self) -> bool {
            true
        }

        fn animation_interval(&self) -> Option<std::time::Duration> {
            self.recent_animation_interval().or_else(|| {
                (self.preview_worker.pending() || self.status_notice.is_some())
                    .then_some(std::time::Duration::from_millis(40))
            })
        }

        fn animation_tick(&mut self) -> WindowResult<bool> {
            let now = std::time::Instant::now();
            if self.poll_preview(now) | self.expire_home_notice(now) {
                self.paint()?;
                Ok(true)
            } else {
                self.tick_recent_rows(now)
            }
        }

        fn close_requested(&self) -> bool {
            self.close_requested
        }
    }

    pub(super) fn start(output: &mut impl Write) -> Result<()> {
        writeln!(
            output,
            "正在启动 ReadAll 图形界面。命令行帮助请使用 --help。"
        )?;
        output.flush()?;
        let ui_font = UiFont::system()?;
        writeln!(output, "GUI 字体: {:?}", ui_font.path())?;
        loop {
            let mut home = Home::new(1040, 700, &ui_font)?;
            if let Ok(store) = Store::from_environment() {
                match store.settings() {
                    Ok(settings) => home.theme = settings.theme,
                    Err(error) => home.status = format!("读取主题失败：{error}"),
                }
                home.theme_store = Some(store);
                home.paint()?;
            }
            let report: WindowReport = window::run(&mut home, WindowOptions::default())?;
            let Some(book) = home.selected_book.take() else {
                writeln!(
                    output,
                    "ReadAll GUI 已关闭。提交帧: {}; 最后尺寸: {}x{}",
                    report.committed_frames, report.width, report.height
                )?;
                return Ok(());
            };
            writeln!(output, "正在打开图书: {:?}", book)?;
            output.flush()?;
            let opened = if matches!(
                crate::publication::path_format(&book),
                Some(crate::publication::Format::Pdf)
            ) {
                crate::native_pdf::open_path(&book, output)
            } else {
                crate::native_epub::open_path(&book, output)
            };
            match opened {
                Ok(()) => {
                    if let Err(error) =
                        RecentStore::from_environment().and_then(|store| store.record(&book))
                    {
                        writeln!(output, "最近阅读记录写入失败: {error}")?;
                    }
                }
                Err(error) => {
                    writeln!(output, "无法打开图书: {error}")?;
                    match crate::diagnostics::log_publication_failure(&book, error.as_ref()) {
                        Ok(path) => writeln!(output, "错误日志: {}", path.display())?,
                        Err(log_error) => writeln!(
                            output,
                            "错误日志写入失败: {log_error}; 目标路径: {}",
                            crate::diagnostics::diagnostic_path().display()
                        )?,
                    }
                    output.flush()?;
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{test_epub, test_font, ui::UiFont};
        use std::{
            sync::atomic::{AtomicU64, Ordering},
            time::{SystemTime, UNIX_EPOCH},
        };

        struct Temp(PathBuf);
        impl Temp {
            fn new() -> Self {
                static NEXT: AtomicU64 = AtomicU64::new(0);
                let root = std::env::temp_dir().join(format!(
                    "readall-home-test-{}-{}-{}",
                    std::process::id(),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                fs::create_dir(&root).unwrap();
                Self(root)
            }
        }
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        fn font() -> UiFont {
            UiFont::from_bytes(test_font::make_font(), PathBuf::from("fixture.ttf")).unwrap()
        }

        #[test]
        fn browser_filters_and_sorts_directories_before_epubs() {
            let temp = Temp::new();
            fs::create_dir(temp.0.join("中文目录")).unwrap();
            fs::write(temp.0.join("中文图书.epub"), b"x").unwrap();
            fs::write(temp.0.join("a.txt"), b"x").unwrap();
            fs::write(temp.0.join("A.EPUB"), b"x").unwrap();
            fs::write(temp.0.join("B.MOBI"), b"x").unwrap();
            fs::write(temp.0.join("c.azw"), b"x").unwrap();
            fs::write(temp.0.join("d.azw3"), b"x").unwrap();
            let browser = Browser::load(temp.0.clone()).unwrap();
            let names: Vec<_> = browser
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert_eq!(
                names,
                [
                    "中文目录",
                    "A.EPUB",
                    "B.MOBI",
                    "c.azw",
                    "d.azw3",
                    "中文图书.epub"
                ]
            );
        }

        #[test]
        fn selected_epub_preview_uses_package_metadata_without_blocking_invalid_books() {
            let temp = Temp::new();
            let valid = temp.0.join("valid.epub");
            fs::write(&valid, test_epub::make_epub()).unwrap();
            let preview = book_preview(&valid);
            assert!(preview.contains("ReadAll"));
            assert!(preview.contains("作者未知"));

            let invalid = temp.0.join("invalid.epub");
            fs::write(&invalid, b"not an epub").unwrap();
            assert_eq!(book_preview(&invalid), "元数据预览不可用，仍可尝试打开");
        }

        #[test]
        fn mobi_preview_uses_metadata_without_decompressing_text() {
            let temp = Temp::new();
            let path = temp.0.join("book.MOBI");
            let mut bytes = crate::test_mobi::make_mobi("<html><body>AAAA</body></html>");
            // Break the text record only; metadata preview should still succeed.
            let start = u32::from_be_bytes(bytes[86..90].try_into().unwrap()) as usize;
            bytes[start] = 8;
            bytes.truncate(start + 1);
            fs::write(&path, &bytes).unwrap();
            let preview = book_preview(&path);
            assert!(preview.contains("ReadAll MOBI 中文") && preview.contains("ReadAll Tests"));
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }

        #[test]
        fn activating_epub_requests_window_close_with_selected_path() {
            let temp = Temp::new();
            let book = temp.0.join("book.epub");
            fs::write(&book, b"x").unwrap();
            let ui_font = font();
            let mut home = Home::new(1040, 700, &ui_font).unwrap();
            home.mode = Mode::Browser(Browser::load(temp.0.clone()).unwrap());
            assert!(!home.action(Action::Activate).unwrap());
            assert!(home.close_requested());
            assert_eq!(home.selected_book.as_deref(), Some(book.as_path()));
        }

        #[test]
        fn clicking_recent_book_requests_open_without_entering_browser() {
            let temp = Temp::new();
            let book = temp.0.join("recent.epub");
            fs::write(&book, test_epub::make_epub()).unwrap();
            let ui_font = font();
            let mut home = Home::new(1040, 700, &ui_font).unwrap();
            home.recent = vec![book.clone()];
            home.paint().unwrap();
            let row = home.recent_row_rect(0);
            assert!(home.hover_target() == HoverTarget::None);
            assert!(
                !home
                    .action(Action::Click {
                        x: row.x + 10,
                        y: row.y + 10,
                    })
                    .unwrap()
            );
            assert!(home.close_requested());
            assert_eq!(home.selected_book.as_deref(), Some(book.as_path()));
            assert!(matches!(home.mode, Mode::Library));
        }

        #[test]
        fn hover_changes_library_card_before_click_and_leave_restores_it() {
            let ui_font = font();
            let mut home = Home::new(1040, 700, &ui_font).unwrap();
            let initial = home.surface.pixels().to_vec();
            assert!(home.action(Action::PointerMove { x: 300, y: 180 }).unwrap());
            assert_eq!(home.hover_target(), HoverTarget::OpenCard);
            assert_ne!(home.surface.pixels(), initial);
            assert!(!home.action(Action::PointerMove { x: 320, y: 200 }).unwrap());
            assert!(home.action(Action::PointerLeave).unwrap());
            assert_eq!(home.hover_target(), HoverTarget::None);
            assert_eq!(home.surface.pixels(), initial);
        }

        fn assert_browser_highlight(home: &Home<'_>, selected: usize) {
            let Mode::Browser(browser) = &home.mode else {
                panic!("not a browser")
            };
            assert_eq!(browser.selected, selected);
            let visible = home
                .visible_rows()
                .min(browser.entries.len().saturating_sub(browser.scroll));
            let mut active = 0;
            for row in 0..visible {
                // Sample the interior, not the antialiased corner of the rounded row.
                let y = (LIST_TOP + row as i32 * ROW_HEIGHT + 20) as u32;
                let pixel = home.surface.pixel(home.surface.width() - 44, y).unwrap();
                let current = browser.scroll + row == selected;
                assert_eq!(
                    pixel,
                    if current {
                        Theme::Paper.palette().button
                    } else {
                        Theme::Paper.palette().panel
                    },
                    "row {}",
                    browser.scroll + row
                );
                assert_eq!(
                    home.surface.pixel(252, y),
                    Some(if current {
                        Theme::Paper.palette().accent
                    } else {
                        Theme::Paper.palette().panel
                    })
                );
                active += usize::from(pixel == Theme::Paper.palette().button);
            }
            assert_eq!(active, usize::from(!browser.entries.is_empty()));
        }

        #[test]
        fn browser_hover_and_navigation_share_one_highlight_and_activation_target() {
            let temp = Temp::new();
            fs::create_dir(temp.0.join("Folder")).unwrap();
            let book = temp.0.join("book.epub");
            fs::write(&book, test_epub::make_epub()).unwrap();
            fs::write(temp.0.join("z.epub"), b"x").unwrap();
            let ui_font = font();
            let mut home = Home::new(1040, 700, &ui_font).unwrap();
            home.mode = Mode::Browser(Browser::load(temp.0.clone()).unwrap());
            home.paint().unwrap();
            assert_browser_highlight(&home, 0);
            let motion = Action::PointerMove {
                x: 400,
                y: LIST_TOP + ROW_HEIGHT + 10,
            };
            assert!(home.action(motion).unwrap());
            assert_browser_highlight(&home, 1);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while std::time::Instant::now() < deadline {
                home.poll_preview(
                    std::time::Instant::now() + std::time::Duration::from_millis(200),
                );
                if matches!(&home.mode,Mode::Browser(b) if b.preview.as_ref().is_some_and(|text|text.contains("ReadAll")))
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let Mode::Browser(browser) = &home.mode else {
                unreachable!()
            };
            assert!(browser.preview.as_ref().unwrap().contains("ReadAll"));
            assert!(!home.action(motion).unwrap());
            home.action(Action::Next).unwrap();
            assert_browser_highlight(&home, 2);
            // Same hovered row, but a different active row after wheel/keys.
            assert!(home.action(motion).unwrap());
            assert_browser_highlight(&home, 1);
            home.action(Action::PointerLeave).unwrap();
            assert_browser_highlight(&home, 1);
            assert!(!home.action(Action::Activate).unwrap());
            assert_eq!(home.selected_book.as_deref(), Some(book.as_path()));
        }

        #[test]
        fn browser_scrolling_bounds_and_empty_or_outside_rows_do_not_add_highlights() {
            let temp = Temp::new();
            for index in 0..24 {
                fs::create_dir(temp.0.join(format!("folder{index:02}"))).unwrap();
            }
            let ui_font = font();
            let mut home = Home::new(1040, 700, &ui_font).unwrap();
            home.mode = Mode::Browser(Browser::load(temp.0.clone()).unwrap());
            home.paint().unwrap();
            home.action(Action::PointerMove {
                x: 400,
                y: LIST_TOP + 10,
            })
            .unwrap();
            for selected in 1..24 {
                home.action(Action::Next).unwrap();
                assert_browser_highlight(&home, selected);
            }
            home.action(Action::Next).unwrap();
            assert_browser_highlight(&home, 23);
            for selected in (0..23).rev() {
                home.action(Action::Previous).unwrap();
                assert_browser_highlight(&home, selected);
            }
            home.action(Action::Previous).unwrap();
            assert_browser_highlight(&home, 0);
            for (x, y) in [
                (220, LIST_TOP + ROW_HEIGHT + 10),
                (1035, LIST_TOP + 10),
                (400, LIST_TOP + ROW_HEIGHT - 1),
                (400, 680),
            ] {
                home.action(Action::PointerMove { x, y }).unwrap();
                assert_browser_highlight(&home, 0);
                assert!(!home.action(Action::Click { x, y }).unwrap());
                assert!(!home.close_requested());
            }
            home.action(Action::Last).unwrap();
            assert_browser_highlight(&home, 23);
            let Mode::Browser(browser) = &home.mode else {
                unreachable!()
            };
            let target = browser.scroll + 1;
            home.action(Action::PointerMove {
                x: 400,
                y: LIST_TOP + ROW_HEIGHT + 10,
            })
            .unwrap();
            assert_browser_highlight(&home, target);
            home.action(Action::First).unwrap();
            assert_browser_highlight(&home, 0);
            let empty = temp.0.join("empty");
            fs::create_dir(&empty).unwrap();
            home.mode = Mode::Browser(Browser::load(empty).unwrap());
            home.paint().unwrap();
            home.action(Action::PointerMove {
                x: 400,
                y: LIST_TOP + 10,
            })
            .unwrap();
            home.action(Action::Next).unwrap();
            assert_browser_highlight(&home, 0);
        }

        #[test]
        fn home_draws_with_true_type_font_and_click_enters_browser() {
            let ui_font = font();
            let mut home = Home::new(1040, 700, &ui_font).unwrap();
            let initial = home.surface.pixels().to_vec();
            assert!(home.action(Action::Click { x: 300, y: 180 }).unwrap());
            assert!(matches!(home.mode, Mode::Browser(_)));
            assert_ne!(home.surface.pixels(), initial);
        }
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) fn run(output: &mut impl Write) -> Result<()> {
    enabled::start(output)
}

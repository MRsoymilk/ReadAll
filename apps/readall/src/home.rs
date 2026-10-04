//! Native ReadAll library and EPUB file browser.
use std::io::Write;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(not(all(target_os = "linux", feature = "wayland")))]
pub(crate) fn run(_: &mut impl Write) -> Result<()> {
    Err("ReadAll GUI is unavailable in this build; use --no-default-features only for headless CLI builds".into())
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod enabled {
    use super::*;
    use crate::ui::{UiFont, UiPainter};
    use readall_core::read_bounded;
    use readall_epub::{EpubBook, EpubLimits};
    use readall_platform::{
        LocalFileSource,
        window::{self, Action, WindowHandler, WindowOptions, WindowReport, WindowResult},
    };
    use readall_render::{Color, DrawCommand, Rect, RenderLimits, Surface};
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    const BG: Color = Color::rgba(244, 246, 249, 255);
    const SIDEBAR: Color = Color::rgba(26, 31, 39, 255);
    const PANEL: Color = Color::rgba(255, 255, 255, 255);
    const INK: Color = Color::rgba(34, 40, 49, 255);
    const MUTED: Color = Color::rgba(105, 115, 128, 255);
    const ACCENT: Color = Color::rgba(52, 103, 190, 255);
    const ACCENT_SOFT: Color = Color::rgba(231, 238, 251, 255);
    const HOVER_SOFT: Color = Color::rgba(220, 232, 249, 255);
    const HOVER_STRONG: Color = Color::rgba(210, 226, 248, 255);
    const BORDER: Color = Color::rgba(224, 229, 236, 255);
    const SIDEBAR_TEXT: Color = Color::rgba(239, 243, 248, 255);
    const SIDEBAR_MUTED: Color = Color::rgba(152, 164, 181, 255);
    const ROW_HEIGHT: i32 = 48;
    const LIST_TOP: i32 = 150;
    const LIST_BOTTOM_MARGIN: i32 = 96;
    const PREVIEW_MAX_EPUB_BYTES: u64 = 16 * 1024 * 1024;

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
                let epub = kind.is_file()
                    && item
                        .path()
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"));
                if directory || epub {
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
                Some(epub_preview(&entry.path))
            };
        }
    }

    fn epub_preview(path: &Path) -> String {
        let size = match fs::metadata(path) {
            Ok(metadata) => metadata.len(),
            Err(_) => return "元数据预览不可用，仍可尝试打开".into(),
        };
        if size > PREVIEW_MAX_EPUB_BYTES {
            return format!(
                "EPUB · {:.1} MiB · 元数据预览已跳过（预览上限 16 MiB）",
                size as f64 / (1024.0 * 1024.0)
            );
        }
        let mut limits = EpubLimits::default();
        limits.zip.max_archive_bytes = PREVIEW_MAX_EPUB_BYTES as usize;
        let parsed = (|| -> Result<String> {
            let mut source = LocalFileSource::open(path)?;
            let bytes = read_bounded(&mut source, limits.zip.max_archive_bytes)?;
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
        BrowserBack,
        BrowserRow(usize),
    }

    struct Home<'font> {
        surface: Surface,
        ui_font: &'font UiFont,
        mode: Mode,
        selected_book: Option<PathBuf>,
        close_requested: bool,
        pointer: Option<(i32, i32)>,
        status: String,
    }

    impl<'font> Home<'font> {
        fn new(width: u32, height: u32, ui_font: &'font UiFont) -> WindowResult<Self> {
            let mut home = Self {
                surface: Surface::new(width, height, RenderLimits::default())?,
                ui_font,
                mode: Mode::Library,
                selected_book: None,
                close_requested: false,
                pointer: None,
                status: "已支持 EPUB 文本阅读，点击“打开图书”开始".into(),
            };
            home.paint()?;
            Ok(home)
        }

        fn default_directory() -> PathBuf {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute() && path.is_dir())
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("/"))
        }

        fn open_browser(&mut self) -> WindowResult<bool> {
            self.mode = Mode::Browser(Browser::load(Self::default_directory())?);
            self.status = "选择一个 EPUB 文件".into();
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

        fn hover_target(&self) -> HoverTarget {
            let Some((x, y)) = self.pointer else {
                return HoverTarget::None;
            };
            match &self.mode {
                Mode::Library => {
                    let card = self.open_card_rect();
                    if point_in(card, x, y) {
                        HoverTarget::OpenCard
                    } else {
                        HoverTarget::None
                    }
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
                    if row >= self.visible_rows() {
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
            if before == after {
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
                self.status = "当前目录没有可打开的 EPUB 文件".into();
                self.paint()?;
                return Ok(true);
            };
            if selected.directory {
                self.mode = Mode::Browser(Browser::load(selected.path)?);
                self.status = "选择一个 EPUB 文件".into();
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
                    self.status = "选择一个 EPUB 文件".into();
                }
                None => {
                    self.mode = Mode::Library;
                    self.status = "已支持 EPUB 文本阅读，点击“打开图书”开始".into();
                }
            }
            self.paint()?;
            Ok(true)
        }

        fn paint(&mut self) -> WindowResult<()> {
            self.surface.draw(&[DrawCommand::FillRect {
                rect: Rect::new(0, 0, self.surface.width(), self.surface.height()),
                color: BG,
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
            let h = self.surface.height();
            let w = self.surface.width();
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(0, 0, 218, h),
                    color: SIDEBAR,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(218, 0, w.saturating_sub(218), 86),
                    color: PANEL,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(218, 85, w.saturating_sub(218), 1),
                    color: BORDER,
                },
            ])?;

            let mut text = UiPainter::new(self.ui_font, &mut self.surface)?;
            text.draw(28, 24, 28, "ReadAll", SIDEBAR_TEXT)?;
            text.draw(30, 62, 13, "原生 Rust 阅读器", SIDEBAR_MUTED)?;
            text.draw(30, 126, 14, "书库", Color::rgba(126, 174, 244, 255))?;
            text.draw(30, 166, 15, "打开图书", SIDEBAR_TEXT)?;
            text.draw(30, 206, 15, "最近阅读", SIDEBAR_MUTED)?;
            text.draw(250, 28, 24, section, INK)?;
            Ok(())
        }

        fn paint_library(&mut self) -> WindowResult<()> {
            self.paint_shell("书库")?;
            let content_w = self.surface.width().saturating_sub(280);
            let card = self.open_card_rect();
            let hovered = self.hover_target() == HoverTarget::OpenCard;
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: card,
                    color: ACCENT,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(card.x + 3, card.y + 3, card.width - 6, card.height - 6),
                    color: if hovered { HOVER_SOFT } else { PANEL },
                },
                DrawCommand::FillRect {
                    rect: Rect::new(card.x + 3, card.y + 3, 8, card.height - 6),
                    color: ACCENT,
                },
            ])?;

            let info_y = 328;
            self.surface.draw(&[DrawCommand::FillRect {
                rect: Rect::new(250, info_y, content_w.min(650), 158),
                color: PANEL,
            }])?;

            let footer_y = self.surface.height() as i32 - 42;
            let mut text = UiPainter::new(self.ui_font, &mut self.surface)?;
            text.draw(282, 148, 24, "打开 EPUB 图书", INK)?;
            text.draw(
                282,
                190,
                15,
                "浏览本地目录，选择 EPUB 后直接进入阅读",
                MUTED,
            )?;
            text.draw(282, 230, 14, "点击卡片或按 Enter", ACCENT)?;
            text.draw(282, info_y + 24, 17, "当前阅读能力", INK)?;
            text.draw(
                282,
                info_y + 62,
                14,
                "XHTML 文本 · 跨章节翻页 · 阅读进度保存",
                MUTED,
            )?;
            text.draw(
                282,
                info_y + 98,
                14,
                "CSS、图片和复杂版式仍在继续完善",
                MUTED,
            )?;
            let status = text.fit(14, &self.status, content_w.saturating_sub(24))?;
            text.draw(250, footer_y, 14, &status, MUTED)?;
            Ok(())
        }

        fn paint_browser(&mut self, browser: &Browser) -> WindowResult<()> {
            self.paint_shell("打开 EPUB")?;
            let top_w = self.surface.width().saturating_sub(278);
            let hover = self.hover_target();
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(242, 102, top_w, 40),
                    color: PANEL,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(250, 108, 76, 28),
                    color: if hover == HoverTarget::BrowserBack {
                        HOVER_STRONG
                    } else {
                        PANEL
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
                let hovered = hover == HoverTarget::BrowserRow(index);
                let row_color = match (selected, hovered) {
                    (true, true) => HOVER_STRONG,
                    (false, true) => HOVER_SOFT,
                    (true, false) => ACCENT_SOFT,
                    (false, false) => PANEL,
                };
                self.surface.draw(&[
                    DrawCommand::FillRect {
                        rect: Rect::new(250, y, row_width, (ROW_HEIGHT - 5) as u32),
                        color: row_color,
                    },
                    DrawCommand::FillRect {
                        rect: Rect::new(
                            250,
                            y,
                            if selected {
                                5
                            } else if hovered {
                                3
                            } else {
                                1
                            },
                            (ROW_HEIGHT - 5) as u32,
                        ),
                        color: if selected || hovered { ACCENT } else { BORDER },
                    },
                ])?;
            }

            let footer_y = self.surface.height() as i32 - 42;
            let preview_y = footer_y - 30;
            let path_width = self.surface.width().saturating_sub(360);
            let footer_width = self.surface.width().saturating_sub(280);
            let mut text = UiPainter::new(self.ui_font, &mut self.surface)?;
            text.draw(260, 114, 14, "‹ 返回", ACCENT)?;
            let path = text.fit(13, &browser.directory.display().to_string(), path_width)?;
            text.draw(350, 115, 13, &path, MUTED)?;

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
                    if entry.directory { "文件夹" } else { "EPUB" },
                    if entry.directory { MUTED } else { ACCENT },
                )?;
                let name = text.fit(15, &entry.name, row_width.saturating_sub(120))?;
                text.draw(350, y + 11, 15, &name, INK)?;
            }

            if browser.entries.is_empty() {
                text.draw(272, LIST_TOP + 30, 15, "当前目录没有 EPUB 文件", MUTED)?;
            }
            if let Some(preview) = &browser.preview {
                let preview = text.fit(13, preview, footer_width)?;
                text.draw(250, preview_y, 13, &preview, ACCENT)?;
            }
            let footer = format!(
                "{} 项 · Enter 打开 · Backspace 上一级 · Esc 退出",
                browser.entries.len()
            );
            let footer = text.fit(13, &footer, footer_width)?;
            text.draw(250, footer_y, 13, &footer, MUTED)?;
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
                    if (250..=326).contains(&x) && (102..=142).contains(&y) {
                        return self.back();
                    }
                    if y >= LIST_TOP {
                        let row = ((y - LIST_TOP) / ROW_HEIGHT) as usize;
                        let target = match &self.mode {
                            Mode::Browser(browser) => browser.scroll + row,
                            Mode::Library => return Ok(false),
                        };
                        if let Mode::Browser(browser) = &mut self.mode {
                            if target < browser.entries.len() {
                                browser.selected = target;
                                browser.keep_visible(visible);
                                browser.refresh_preview();
                            } else {
                                return Ok(false);
                            }
                        }
                        self.paint()?;
                        return self.activate_browser();
                    }
                    Ok(false)
                }
                Action::Close => Ok(false),
            }
        }
    }

    impl WindowHandler for Home<'_> {
        fn resize(&mut self, width: u32, height: u32) -> WindowResult<bool> {
            if (width, height) == (self.surface.width(), self.surface.height()) {
                return Ok(false);
            }
            if width < 680 || height < 460 {
                return Err("ReadAll main window requires at least 680x460".into());
            }
            self.surface = Surface::new(width, height, RenderLimits::default())?;
            self.paint()?;
            Ok(true)
        }

        fn action(&mut self, action: Action) -> WindowResult<bool> {
            if matches!(self.mode, Mode::Browser(_)) {
                return self.browser_action(action);
            }
            match action {
                Action::Activate => self.open_browser(),
                Action::Click { x, y } if (250..=900).contains(&x) && (120..=294).contains(&y) => {
                    self.open_browser()
                }
                Action::Back => Ok(false),
                Action::PointerMove { x, y } => self.pointer_changed(Some((x, y))),
                Action::PointerLeave => self.pointer_changed(None),
                Action::Previous
                | Action::Next
                | Action::First
                | Action::Last
                | Action::Larger
                | Action::Smaller
                | Action::Click { .. }
                | Action::Close => Ok(false),
            }
        }

        fn surface(&self) -> &Surface {
            &self.surface
        }

        fn title(&self) -> String {
            match &self.mode {
                Mode::Library => "ReadAll — 书库".into(),
                Mode::Browser(browser) => {
                    format!("ReadAll — 打开 EPUB — {}", browser.directory.display())
                }
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
            let report: WindowReport = window::run(&mut home, WindowOptions::default())?;
            let Some(book) = home.selected_book.take() else {
                writeln!(
                    output,
                    "ReadAll GUI 已关闭。提交帧: {}; 最后尺寸: {}x{}",
                    report.committed_frames, report.width, report.height
                )?;
                return Ok(());
            };
            writeln!(output, "正在打开 EPUB: {:?}", book)?;
            output.flush()?;
            if let Err(error) = crate::native_epub::open_path(&book, output) {
                writeln!(output, "无法打开 EPUB: {error}")?;
                match crate::diagnostics::log_epub_failure(&book, error.as_ref()) {
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
            let browser = Browser::load(temp.0.clone()).unwrap();
            let names: Vec<_> = browser
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert_eq!(names, ["中文目录", "A.EPUB", "中文图书.epub"]);
        }

        #[test]
        fn selected_epub_preview_uses_package_metadata_without_blocking_invalid_books() {
            let temp = Temp::new();
            let valid = temp.0.join("valid.epub");
            fs::write(&valid, test_epub::make_epub()).unwrap();
            let preview = epub_preview(&valid);
            assert!(preview.contains("ReadAll"));
            assert!(preview.contains("作者未知"));

            let invalid = temp.0.join("invalid.epub");
            fs::write(&invalid, b"not an epub").unwrap();
            assert_eq!(epub_preview(&invalid), "元数据预览不可用，仍可尝试打开");
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

        #[test]
        fn browser_hover_does_not_replace_keyboard_selection() {
            let temp = Temp::new();
            fs::create_dir(temp.0.join("Folder")).unwrap();
            fs::write(temp.0.join("book.epub"), b"x").unwrap();
            let ui_font = font();
            let mut home = Home::new(1040, 700, &ui_font).unwrap();
            home.mode = Mode::Browser(Browser::load(temp.0.clone()).unwrap());
            home.paint().unwrap();
            let selected = match &home.mode {
                Mode::Browser(browser) => browser.selected,
                Mode::Library => unreachable!(),
            };
            assert!(
                home.action(Action::PointerMove {
                    x: 400,
                    y: LIST_TOP + ROW_HEIGHT + 10,
                })
                .unwrap()
            );
            assert_eq!(home.hover_target(), HoverTarget::BrowserRow(1));
            let still_selected = match &home.mode {
                Mode::Browser(browser) => browser.selected,
                Mode::Library => unreachable!(),
            };
            assert_eq!(still_selected, selected);
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

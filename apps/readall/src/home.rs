//! Native ReadAll home screen and dependency-free EPUB file browser.
use std::io::Write;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(not(all(target_os = "linux", feature = "wayland")))]
pub(crate) fn run(_: &mut impl Write) -> Result<()> {
    Err("ReadAll GUI is unavailable in this build; use --no-default-features only for headless CLI builds".into())
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod enabled {
    use super::*;
    use crate::ui::{display_ascii, draw_text};
    use readall_platform::window::{
        self, Action, WindowHandler, WindowOptions, WindowReport, WindowResult,
    };
    use readall_render::{Color, DrawCommand, Rect, RenderLimits, Surface};
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    const BG: Color = Color::rgba(244, 246, 249, 255);
    const SIDEBAR: Color = Color::rgba(27, 32, 40, 255);
    const PANEL: Color = Color::rgba(255, 255, 255, 255);
    const INK: Color = Color::rgba(34, 40, 49, 255);
    const MUTED: Color = Color::rgba(107, 117, 130, 255);
    const ACCENT: Color = Color::rgba(55, 104, 190, 255);
    const ACCENT_SOFT: Color = Color::rgba(229, 237, 252, 255);
    const SIDEBAR_TEXT: Color = Color::rgba(238, 242, 247, 255);
    const ROW_HEIGHT: i32 = 42;
    const LIST_TOP: i32 = 142;
    const LIST_BOTTOM_MARGIN: i32 = 72;

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
            Ok(Self {
                directory,
                entries,
                selected: 0,
                scroll: 0,
            })
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
        }

        fn first(&mut self) {
            self.selected = 0;
            self.scroll = 0;
        }

        fn last(&mut self, visible: usize) {
            if self.entries.is_empty() {
                return;
            }
            self.selected = self.entries.len() - 1;
            self.keep_visible(visible);
        }

        fn keep_visible(&mut self, visible: usize) {
            let visible = visible.max(1);
            if self.selected < self.scroll {
                self.scroll = self.selected;
            } else if self.selected >= self.scroll.saturating_add(visible) {
                self.scroll = self.selected + 1 - visible;
            }
        }
    }

    enum Mode {
        Home,
        Browser(Browser),
    }

    struct Home {
        surface: Surface,
        mode: Mode,
        selected_book: Option<PathBuf>,
        close_requested: bool,
        status: String,
    }

    impl Home {
        fn new(width: u32, height: u32) -> WindowResult<Self> {
            let mut home = Self {
                surface: Surface::new(width, height, RenderLimits::default())?,
                mode: Mode::Home,
                selected_book: None,
                close_requested: false,
                status: "EPUB TEXT READING IS READY".into(),
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
            let browser = Browser::load(Self::default_directory())?;
            self.mode = Mode::Browser(browser);
            self.status = "SELECT AN EPUB FILE".into();
            self.paint()?;
            Ok(true)
        }

        fn visible_rows(&self) -> usize {
            ((self.surface.height() as i32 - LIST_TOP - LIST_BOTTOM_MARGIN) / ROW_HEIGHT).max(1)
                as usize
        }

        fn activate_browser(&mut self) -> WindowResult<bool> {
            let selected = match &self.mode {
                Mode::Browser(browser) => browser.selected().cloned(),
                Mode::Home => None,
            };
            let Some(selected) = selected else {
                self.status = "NO EPUB FILES IN THIS FOLDER".into();
                self.paint()?;
                return Ok(true);
            };
            if selected.directory {
                self.mode = Mode::Browser(Browser::load(selected.path)?);
                self.status = "SELECT AN EPUB FILE".into();
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
                Mode::Home => None,
            };
            match parent {
                Some(parent) => {
                    self.mode = Mode::Browser(Browser::load(parent)?);
                    self.status = "SELECT AN EPUB FILE".into();
                }
                None => {
                    self.mode = Mode::Home;
                    self.status = "EPUB TEXT READING IS READY".into();
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
                Mode::Home => self.paint_home(),
                Mode::Browser(browser) => {
                    let browser = browser.clone();
                    self.paint_browser(&browser)
                }
            }
        }

        fn paint_shell(&mut self, section: &str) -> WindowResult<()> {
            let h = self.surface.height();
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(0, 0, 214, h),
                    color: SIDEBAR,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(214, 0, self.surface.width().saturating_sub(214), 82),
                    color: PANEL,
                },
            ])?;
            draw_text(&mut self.surface, 28, 28, 4, "READALL", SIDEBAR_TEXT)?;
            draw_text(
                &mut self.surface,
                28,
                72,
                2,
                "RUST READER",
                Color::rgba(157, 169, 185, 255),
            )?;
            draw_text(
                &mut self.surface,
                30,
                138,
                2,
                "LIBRARY",
                Color::rgba(126, 174, 244, 255),
            )?;
            draw_text(&mut self.surface, 30, 176, 2, "OPEN EPUB", SIDEBAR_TEXT)?;
            draw_text(&mut self.surface, 246, 30, 3, section, INK)?;
            Ok(())
        }

        fn paint_home(&mut self) -> WindowResult<()> {
            self.paint_shell("LIBRARY")?;
            let w = self.surface.width();
            let content_w = w.saturating_sub(270);
            let card = Rect::new(246, 116, content_w.min(620), 164);
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: card,
                    color: ACCENT,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(card.x + 4, card.y + 4, card.width - 8, card.height - 8),
                    color: PANEL,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(card.x + 4, card.y + 4, 10, card.height - 8),
                    color: ACCENT,
                },
            ])?;
            draw_text(&mut self.surface, 276, 144, 3, "OPEN EPUB", INK)?;
            draw_text(
                &mut self.surface,
                276,
                194,
                2,
                "BROWSE FOLDERS AND START READING",
                MUTED,
            )?;
            draw_text(
                &mut self.surface,
                276,
                232,
                2,
                "CLICK THIS CARD OR PRESS ENTER",
                ACCENT,
            )?;

            let info_y = 326;
            self.surface.draw(&[DrawCommand::FillRect {
                rect: Rect::new(246, info_y, content_w.min(620), 150),
                color: PANEL,
            }])?;
            draw_text(
                &mut self.surface,
                276,
                info_y + 24,
                2,
                "CURRENT EPUB SUPPORT",
                INK,
            )?;
            draw_text(
                &mut self.surface,
                276,
                info_y + 60,
                2,
                "XHTML TEXT  MULTI CHAPTER  PROGRESS",
                MUTED,
            )?;
            draw_text(
                &mut self.surface,
                276,
                info_y + 94,
                2,
                "CSS AND IMAGES ARE NEXT",
                MUTED,
            )?;

            let footer_y = self.surface.height() as i32 - 38;
            draw_text(
                &mut self.surface,
                246,
                footer_y,
                2,
                &display_ascii(&self.status, 58),
                MUTED,
            )?;
            Ok(())
        }

        fn paint_browser(&mut self, browser: &Browser) -> WindowResult<()> {
            self.paint_shell("OPEN EPUB")?;
            self.surface.draw(&[
                DrawCommand::FillRect {
                    rect: Rect::new(238, 96, self.surface.width().saturating_sub(264), 38),
                    color: PANEL,
                },
                DrawCommand::FillRect {
                    rect: Rect::new(246, 103, 74, 24),
                    color: ACCENT_SOFT,
                },
            ])?;
            draw_text(&mut self.surface, 256, 108, 2, "< BACK", ACCENT)?;
            let path = display_ascii(&browser.directory.display().to_string(), 55);
            draw_text(&mut self.surface, 340, 108, 2, &path, MUTED)?;

            let visible = self.visible_rows();
            let width = self.surface.width().saturating_sub(276);
            for (row, entry) in browser
                .entries
                .iter()
                .skip(browser.scroll)
                .take(visible)
                .enumerate()
            {
                let index = browser.scroll + row;
                let y = LIST_TOP + row as i32 * ROW_HEIGHT;
                let selected = index == browser.selected;
                self.surface.draw(&[DrawCommand::FillRect {
                    rect: Rect::new(246, y, width, (ROW_HEIGHT - 4) as u32),
                    color: if selected { ACCENT_SOFT } else { PANEL },
                }])?;
                if selected {
                    self.surface.draw(&[DrawCommand::FillRect {
                        rect: Rect::new(246, y, 5, (ROW_HEIGHT - 4) as u32),
                        color: ACCENT,
                    }])?;
                }
                draw_text(
                    &mut self.surface,
                    264,
                    y + 10,
                    2,
                    if entry.directory { "DIR" } else { "EPUB" },
                    if entry.directory { MUTED } else { ACCENT },
                )?;
                let name = display_ascii(&entry.name, 52);
                draw_text(&mut self.surface, 330, y + 10, 2, &name, INK)?;
            }

            if browser.entries.is_empty() {
                draw_text(
                    &mut self.surface,
                    266,
                    LIST_TOP + 28,
                    2,
                    "NO EPUB FILES OR FOLDERS HERE",
                    MUTED,
                )?;
            }
            let footer = format!(
                "{} ITEMS  ENTER OPEN  BACKSPACE UP  ESC EXIT",
                browser.entries.len()
            );
            let footer_y = self.surface.height() as i32 - 38;
            draw_text(
                &mut self.surface,
                246,
                footer_y,
                2,
                &display_ascii(&footer, 62),
                MUTED,
            )?;
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
                Action::Click { x, y } => {
                    if (246..=320).contains(&x) && (96..=134).contains(&y) {
                        return self.back();
                    }
                    if y >= LIST_TOP {
                        let row = ((y - LIST_TOP) / ROW_HEIGHT) as usize;
                        let target = match &self.mode {
                            Mode::Browser(browser) => browser.scroll + row,
                            Mode::Home => return Ok(false),
                        };
                        if let Mode::Browser(browser) = &mut self.mode {
                            if target < browser.entries.len() {
                                browser.selected = target;
                                browser.keep_visible(visible);
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

    impl WindowHandler for Home {
        fn resize(&mut self, width: u32, height: u32) -> WindowResult<bool> {
            if (width, height) == (self.surface.width(), self.surface.height()) {
                return Ok(false);
            }
            if width < 640 || height < 420 {
                return Err("ReadAll main window requires at least 640x420".into());
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
                Action::Click { x, y } if (246..=866).contains(&x) && (116..=280).contains(&y) => {
                    self.open_browser()
                }
                Action::Back => Ok(false),
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
                Mode::Home => "ReadAll — Library".into(),
                Mode::Browser(browser) => format!(
                    "ReadAll — Open EPUB — {}",
                    display_ascii(&browser.directory.display().to_string(), 48)
                ),
            }
        }

        fn close_requested(&self) -> bool {
            self.close_requested
        }
    }

    pub(super) fn start(output: &mut impl Write) -> Result<()> {
        writeln!(
            output,
            "Starting ReadAll GUI. Use --help for command-line tools."
        )?;
        output.flush()?;
        loop {
            let mut home = Home::new(1000, 680)?;
            let report: WindowReport = window::run(&mut home, WindowOptions::default())?;
            let Some(book) = home.selected_book.take() else {
                writeln!(
                    output,
                    "ReadAll GUI closed. Buffer commits: {}; last size: {}x{}",
                    report.committed_frames, report.width, report.height
                )?;
                return Ok(());
            };
            writeln!(output, "Opening EPUB: {:?}", book)?;
            output.flush()?;
            if let Err(error) = crate::native_epub::open_path(&book, output) {
                writeln!(output, "Could not open EPUB: {error}")?;
                output.flush()?;
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
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

        #[test]
        fn browser_filters_and_sorts_directories_before_epubs() {
            let temp = Temp::new();
            fs::create_dir(temp.0.join("Folder")).unwrap();
            fs::write(temp.0.join("b.epub"), b"x").unwrap();
            fs::write(temp.0.join("a.txt"), b"x").unwrap();
            fs::write(temp.0.join("A.EPUB"), b"x").unwrap();
            let browser = Browser::load(temp.0.clone()).unwrap();
            let names: Vec<_> = browser
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect();
            assert_eq!(names, ["Folder", "A.EPUB", "b.epub"]);
        }

        #[test]
        fn activating_epub_requests_window_close_with_selected_path() {
            let temp = Temp::new();
            let book = temp.0.join("book.epub");
            fs::write(&book, b"x").unwrap();
            let mut home = Home::new(1000, 680).unwrap();
            home.mode = Mode::Browser(Browser::load(temp.0.clone()).unwrap());
            assert!(!home.action(Action::Activate).unwrap());
            assert!(home.close_requested());
            assert_eq!(home.selected_book.as_deref(), Some(book.as_path()));
        }

        #[test]
        fn home_draws_and_click_enters_browser() {
            let mut home = Home::new(1000, 680).unwrap();
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

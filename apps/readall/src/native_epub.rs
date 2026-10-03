//! Native EPUB reading entry for the current XHTML text subset.
use std::{ffi::OsString, io::Write};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(not(all(target_os = "linux", feature = "wayland")))]
pub(crate) fn run(_: &[OsString], _: &mut impl Write) -> Result<()> {
    Err("native EPUB window unavailable: on Linux build with --features wayland; Windows/Android windows are not implemented".into())
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod enabled {
    use super::*;
    use crate::{
        epub_session::{Action as ReaderAction, EpubSession, Start},
        progress::EpubProgressStore,
        text_page::Options,
    };
    use readall_core::read_bounded;
    use readall_epub::{EpubBook, EpubLimits, EpubLocator};
    use readall_font::{Font, FontLimits};
    use readall_platform::{
        LocalFileSource,
        window::{self, Action, WindowHandler, WindowOptions, WindowReport, WindowResult},
    };
    use readall_render::Surface;
    use std::path::PathBuf;

    struct ReaderWindow<'book, 'archive, 'font, 'font_bytes> {
        session: EpubSession<'book, 'archive, 'font, 'font_bytes>,
        progress: Option<EpubProgressStore>,
    }

    impl ReaderWindow<'_, '_, '_, '_> {
        fn save_progress(&self) {
            let Some(store) = &self.progress else {
                return;
            };
            if let Err(error) = store.save(self.session.anchor()) {
                eprintln!("ReadAll: cannot save EPUB reading progress: {error}");
            }
        }
    }

    impl WindowHandler for ReaderWindow<'_, '_, '_, '_> {
        fn resize(&mut self, width: u32, height: u32) -> WindowResult<bool> {
            EpubSession::resize(&mut self.session, width, height)
        }

        fn action(&mut self, action: Action) -> WindowResult<bool> {
            let action = match action {
                Action::Next => ReaderAction::Next,
                Action::Previous => ReaderAction::Previous,
                Action::First => ReaderAction::First,
                Action::Last => ReaderAction::Last,
                Action::Larger => ReaderAction::Larger,
                Action::Smaller => ReaderAction::Smaller,
                Action::Close => return Ok(false),
            };
            match EpubSession::action(&mut self.session, action) {
                Ok(changed) => {
                    if changed {
                        self.save_progress();
                    }
                    Ok(changed)
                }
                Err(error) => {
                    eprintln!("ReadAll: keeping current EPUB page: {error}");
                    Ok(false)
                }
            }
        }

        fn surface(&self) -> &Surface {
            &self.session.frame().surface
        }

        fn title(&self) -> String {
            self.session.title()
        }
    }

    pub(super) fn start(args: &[OsString], output: &mut impl Write) -> Result<()> {
        if args.is_empty() {
            return Err("open-epub expects <book.epub> --font <font.ttf>".into());
        }
        if (args.len() - 1) % 2 != 0 {
            return Err("each EPUB window option needs a value".into());
        }

        let mut window = WindowOptions::default();
        let mut page_args = Vec::new();
        let mut spine = None;
        let mut locator: Option<EpubLocator> = None;
        let mut state_dir = None;
        let mut progress_enabled = true;
        let mut progress_seen = false;
        for pair in args[1..].chunks_exact(2) {
            match pair[0].to_str() {
                Some("--display") => {
                    if window.display.is_some() {
                        return Err("duplicate --display".into());
                    }
                    window.display = Some(PathBuf::from(&pair[1]));
                }
                Some("--frames") => {
                    if window.close_after_frames.is_some() || pair[1] != "1" {
                        return Err(
                            "--frames supports only 1 for a one-frame protocol smoke check".into(),
                        );
                    }
                    window.close_after_frames = Some(1);
                }
                Some("--spine") => {
                    if spine.is_some() {
                        return Err("duplicate --spine".into());
                    }
                    spine = Some(
                        number(&pair[1])?
                            .checked_sub(1)
                            .ok_or("spine number must be at least 1")?,
                    );
                }
                Some("--at") => {
                    if locator.is_some() {
                        return Err("duplicate --at".into());
                    }
                    locator = Some(pair[1].to_str().ok_or("locator must be UTF-8")?.parse()?);
                }
                Some("--state-dir") => {
                    if state_dir.is_some() {
                        return Err("duplicate --state-dir".into());
                    }
                    state_dir = Some(PathBuf::from(&pair[1]));
                }
                Some("--progress") => {
                    if progress_seen {
                        return Err("duplicate --progress".into());
                    }
                    progress_seen = true;
                    progress_enabled = match pair[1].to_str() {
                        Some("on") => true,
                        Some("off") => false,
                        _ => return Err("--progress expects on or off".into()),
                    };
                }
                _ => page_args.extend_from_slice(pair),
            }
        }
        if spine.is_some() && locator.is_some() {
            return Err("--spine and EPUB --at cannot be combined".into());
        }
        if !progress_enabled && state_dir.is_some() {
            return Err("--state-dir cannot be used with --progress off".into());
        }

        let options = Options::parse(&page_args)?;
        if locator.is_some() && options.page.is_some() {
            return Err("EPUB --at and --page cannot be combined".into());
        }

        let epub_limits = EpubLimits::default();
        let epub_bytes = read_bounded(
            &mut LocalFileSource::open(PathBuf::from(&args[0]))?,
            epub_limits.zip.max_archive_bytes,
        )?;
        let book = EpubBook::parse(&epub_bytes, epub_limits)?;
        let explicit_position = spine.is_some() || locator.is_some() || options.page.is_some();
        let progress = if progress_enabled {
            match state_dir
                .map(EpubProgressStore::new)
                .map(Ok)
                .unwrap_or_else(EpubProgressStore::from_environment)
            {
                Ok(store) => Some(store),
                Err(error) => {
                    writeln!(
                        output,
                        "EPUB reading progress disabled for this session: {error}"
                    )?;
                    None
                }
            }
        } else {
            None
        };
        if !explicit_position {
            if let Some(store) = &progress {
                match store.load(&book) {
                    Ok(Some(saved)) => {
                        writeln!(output, "Restored EPUB locator: {saved}")?;
                        locator = Some(saved);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        writeln!(
                            output,
                            "Ignoring invalid EPUB reading progress and starting normally: {error}"
                        )?;
                    }
                }
            }
        }

        let font_limits = FontLimits::default();
        let font_bytes = read_bounded(
            &mut LocalFileSource::open(&options.font)?,
            font_limits.max_file_bytes,
        )?;
        let font = Font::parse(&font_bytes, options.face, font_limits)?;

        let start = if let Some(locator) = locator {
            Start::Locator(locator)
        } else if let Some(spine) = spine {
            Start::Spine(spine)
        } else {
            Start::Beginning
        };
        let session = EpubSession::new(&book, &font, options, start)?;
        let mut reader = ReaderWindow { session, progress };

        writeln!(
            output,
            "Native Wayland EPUB reader (XHTML text subset)\nKeys: PageUp/PageDown, arrows, Space, Home/End, +/-; Esc closes.\nPage navigation crosses linear spine boundaries. Reading position is saved with epub-v1 locator when progress storage is available.\nCSS, images, shaping and font fallback are not rendered yet."
        )?;
        output.flush()?;

        let report: WindowReport = window::run(&mut reader, window)?;
        reader.save_progress();
        writeln!(
            output,
            "Closed. Buffer commits: {}; last size: {}x{}\nEPUB locator: {}",
            report.committed_frames,
            report.width,
            report.height,
            reader.session.anchor()
        )?;
        Ok(())
    }

    fn number(value: &OsString) -> Result<usize> {
        let text = value.to_str().ok_or("expected an unsigned integer")?;
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("expected an unsigned integer".into());
        }
        Ok(text.parse()?)
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) fn run(args: &[OsString], output: &mut impl Write) -> Result<()> {
    enabled::start(args, output)
}

//! Native application entry; document bytes and font are loaded once per session.
use std::{ffi::OsString, io::Write};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(not(all(target_os = "linux", feature = "wayland")))]
pub(crate) fn run(_: &[OsString], _: &mut impl Write) -> Result<()> {
    Err("native window unavailable: on Linux run cargo run -p readall --features wayland --offline -- open <book.txt> --font <font.ttf>; Windows/Android windows are not implemented".into())
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
mod enabled {
    use super::*;
    use crate::{
        progress::ProgressStore,
        session::{Action as ReaderAction, Session},
        text_page::Options,
    };
    use readall_core::{Limits, TextDocument, read_bounded};
    use readall_font::{Font, FontLimits};
    use readall_platform::{
        LocalFileSource,
        window::{self, Action, WindowHandler, WindowOptions, WindowReport, WindowResult},
    };
    use readall_render::Surface;
    use std::path::PathBuf;

    struct ReaderWindow<'doc, 'font, 'bytes> {
        session: Session<'doc, 'font, 'bytes>,
        progress: Option<ProgressStore>,
    }

    impl ReaderWindow<'_, '_, '_> {
        fn save_progress(&self) {
            let Some(store) = &self.progress else {
                return;
            };
            if let Err(error) = store.save(self.session.anchor()) {
                eprintln!("ReadAll: cannot save reading progress: {error}");
            }
        }
    }

    impl WindowHandler for ReaderWindow<'_, '_, '_> {
        fn resize(&mut self, width: u32, height: u32) -> WindowResult<bool> {
            Session::resize(&mut self.session, width, height)
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
            match Session::action(&mut self.session, action) {
                Ok(changed) => {
                    if changed {
                        self.save_progress();
                    }
                    Ok(changed)
                }
                Err(error) => {
                    eprintln!("ReadAll: keeping current page: {error}");
                    Ok(false)
                }
            }
        }

        fn surface(&self) -> &Surface {
            &self.session.frame().surface
        }

        fn title(&self) -> String {
            Session::title(&self.session)
        }
    }

    pub(super) fn start(args: &[OsString], output: &mut impl Write) -> Result<()> {
        if args.is_empty() {
            return Err("open expects <book.txt> --font <font.ttf>".into());
        }
        if (args.len() - 1) % 2 != 0 {
            return Err("each window/page option needs a value".into());
        }

        let mut window = WindowOptions::default();
        let mut page_args = Vec::new();
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
        if !progress_enabled && state_dir.is_some() {
            return Err("--state-dir cannot be used with --progress off".into());
        }

        let mut options = Options::parse(&page_args)?;
        let explicit_position = options.page.is_some() || options.at.is_some();
        let document = TextDocument::open(
            &mut LocalFileSource::open(PathBuf::from(&args[0]))?,
            Limits::default(),
        )?;

        let progress = if progress_enabled {
            match state_dir
                .map(ProgressStore::new)
                .map(Ok)
                .unwrap_or_else(ProgressStore::from_environment)
            {
                Ok(store) => Some(store),
                Err(error) => {
                    writeln!(
                        output,
                        "Reading progress disabled for this session: {error}"
                    )?;
                    None
                }
            }
        } else {
            None
        };

        if !explicit_position {
            if let Some(store) = &progress {
                match store.load(&document) {
                    Ok(Some(locator)) => {
                        writeln!(output, "Restored reading locator: {locator}")?;
                        options.at = Some(locator);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        writeln!(
                            output,
                            "Ignoring invalid reading progress and starting normally: {error}"
                        )?;
                    }
                }
            }
        }

        let limits = FontLimits::default();
        let bytes = read_bounded(
            &mut LocalFileSource::open(&options.font)?,
            limits.max_file_bytes,
        )?;
        let font = Font::parse(&bytes, options.face, limits)?;
        let session = Session::new(&document, &font, options)?;
        let mut reader = ReaderWindow { session, progress };

        writeln!(
            output,
            "Native Wayland TXT reader\nKeys: PageUp/PageDown, arrows, Space, Home/End, +/-; Esc closes.\nClick left/right half to turn pages; vertical wheel turns pages.\nReading position is saved by content locator when progress storage is available.\nNo shaping or font fallback yet."
        )?;
        output.flush()?;

        let report: WindowReport = window::run(&mut reader, window)?;
        reader.save_progress();
        writeln!(
            output,
            "Closed. Buffer commits: {}; last size: {}x{}\nReading locator: {}",
            report.committed_frames,
            report.width,
            report.height,
            reader.session.anchor()
        )?;
        Ok(())
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) fn run(args: &[OsString], output: &mut impl Write) -> Result<()> {
    enabled::start(args, output)
}

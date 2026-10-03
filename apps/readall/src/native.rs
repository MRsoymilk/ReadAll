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
        session::{Action as ReaderAction, Session},
        text_page::Options,
    };
    use readall_core::{Limits, TextDocument, read_bounded};
    use readall_font::{Font, FontLimits};
    use readall_platform::{
        LocalFileSource,
        window::{self, Action, WindowHandler, WindowOptions, WindowResult},
    };
    use readall_render::Surface;
    use std::path::PathBuf;
    impl WindowHandler for Session<'_, '_, '_> {
        fn resize(&mut self, width: u32, height: u32) -> WindowResult<bool> {
            Session::resize(self, width, height)
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
            match Session::action(self, action) {
                Ok(changed) => Ok(changed),
                Err(error) => {
                    eprintln!("ReadAll: keeping current page: {error}");
                    Ok(false)
                }
            }
        }
        fn surface(&self) -> &Surface {
            &self.frame().surface
        }
        fn title(&self) -> String {
            Session::title(self)
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
                _ => page_args.extend_from_slice(pair),
            }
        }
        let options = Options::parse(&page_args)?;
        let document = TextDocument::open(
            &mut LocalFileSource::open(PathBuf::from(&args[0]))?,
            Limits::default(),
        )?;
        let limits = FontLimits::default();
        let bytes = read_bounded(
            &mut LocalFileSource::open(&options.font)?,
            limits.max_file_bytes,
        )?;
        let font = Font::parse(&bytes, options.face, limits)?;
        let mut session = Session::new(&document, &font, options)?;
        writeln!(
            output,
            "Native Wayland TXT reader\nKeys: PageUp/PageDown, arrows, Space, Home/End, +/-; Esc closes.\nClick left/right half to turn pages; vertical wheel turns pages.\nNo shaping, font fallback, or automatic progress saving yet."
        )?;
        output.flush()?;
        let report = window::run(&mut session, window)?;
        writeln!(
            output,
            "Closed. Buffer commits: {}; last size: {}x{}\nReading locator: {}",
            report.committed_frames,
            report.width,
            report.height,
            session.anchor()
        )?;
        Ok(())
    }
}
#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) fn run(args: &[OsString], output: &mut impl Write) -> Result<()> {
    enabled::start(args, output)
}

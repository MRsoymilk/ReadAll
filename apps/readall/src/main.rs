mod cli;
mod diagnostics;
mod epub_page;
#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
mod epub_session;
mod home;
mod native;
mod native_epub;
#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
mod progress;
#[cfg(all(target_os = "linux", any(test, feature = "wayland")))]
mod recent;
#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
mod session;
#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
mod svg_icon;
#[cfg(test)]
#[path = "../tests/support/epub.rs"]
mod test_epub;
#[cfg(test)]
#[path = "../tests/support/font.rs"]
mod test_font;
mod text_page;
#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
mod ui;

use std::{io, process::ExitCode};

fn main() -> ExitCode {
    match cli::run(
        std::env::args_os().skip(1).collect(),
        &mut io::stdout().lock(),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
            {
                return ExitCode::SUCCESS;
            }
            eprintln!("ReadAll: {error}");
            ExitCode::FAILURE
        }
    }
}

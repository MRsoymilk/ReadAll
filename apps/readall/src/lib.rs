//! Shared ReadAll application library. Desktop CLI/Wayland and mobile hosts use
//! the same publication, layout and reading-session modules; no duplicated engine.
mod cli;
mod diagnostics;
mod epub_flow;
mod epub_page;
#[cfg(any(
    test,
    feature = "mobile",
    all(target_os = "linux", feature = "wayland")
))]
mod epub_session;
mod fonts;
mod home;
mod loading;
#[cfg(feature = "mobile")]
pub mod mobile;
mod native;
mod native_epub;
#[cfg(all(target_os = "linux", feature = "wayland"))]
mod native_pdf;
#[cfg(any(
    test,
    feature = "mobile",
    all(target_os = "linux", feature = "wayland")
))]
mod progress;
mod publication;
mod reader_cli;
mod reader_data;
#[cfg(all(target_os = "linux", any(test, feature = "wayland")))]
mod recent;
#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
mod session;
#[cfg(any(
    test,
    feature = "mobile",
    all(target_os = "linux", feature = "wayland")
))]
mod svg_icon;
#[cfg(test)]
#[path = "../tests/support/azw3.rs"]
mod test_azw3;
#[cfg(test)]
#[path = "../tests/support/epub.rs"]
mod test_epub;
#[cfg(test)]
#[path = "../tests/support/font.rs"]
mod test_font;
#[cfg(test)]
#[path = "../tests/support/mobi.rs"]
mod test_mobi;
mod text_page;
#[cfg(any(
    test,
    feature = "mobile",
    all(target_os = "linux", feature = "wayland")
))]
mod ui;

/// Existing command-line entry; platform hosts should use the typed mobile API.
pub fn run(
    args: Vec<std::ffi::OsString>,
    output: &mut impl std::io::Write,
) -> Result<(), Box<dyn std::error::Error>> {
    cli::run(args, output)
}

//! Theme changes replace colors, not interaction geometry or reading identity.
use super::*;
use crate::{
    reader_data::{PageMode, Settings, Store, Theme},
    test_epub, test_font,
};
use readall_platform::window::ReaderCommand;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "readall-chrome-theme-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn pixel(reader: &ReaderWindow<'_, '_, '_, '_>, rect: Rect) -> Color {
    let at = reader.surface.pixel_point((
        f64::from(rect.x + rect.width as i32 / 2),
        f64::from(rect.y + 2),
    ));
    reader.surface.pixel(at.0 as u32, at.1 as u32).unwrap()
}
fn options(dense: bool) -> Options {
    let mut o = Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "400",
            "--height",
            "640",
            "--font-size",
            "16",
            "--margin",
            "24",
        ]
        .map(Into::into),
    )
    .unwrap();
    if dense {
        o.raster_size = Some((1080, 1728));
    }
    o
}
#[test]
fn light_and_dark_cover_toolbar_toc_settings_and_keep_geometry() {
    let data = test_epub::make_epub();
    let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let bytes = test_font::make_font();
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    for dense in [false, true] {
        let temp = Temp::new();
        let session = EpubSession::new(&book, &font, options(dense), Start::Beginning).unwrap();
        let mut r = ReaderWindow::new_lazy(
            session,
            None,
            UiFont::from_bytes(bytes.clone(), "fixture.ttf".into()).unwrap(),
        )
        .unwrap();
        r.tools.store = Some(Store::new(temp.0.clone()));
        let anchor = r.session.anchor().clone();
        let rects = (r.toolbar_rect(), r.collapsed_rect(), r.toc_panel_rect());
        let pages = r.session.page_position();
        for theme in [Theme::Dark, Theme::Paper] {
            r.action(Action::Command(ReaderCommand::Theme)).unwrap();
            let p = theme.palette();
            assert_eq!(r.session.settings().theme, theme);
            assert_eq!(
                r.tools.store.as_ref().unwrap().settings().unwrap().theme,
                theme
            );
            assert_eq!(r.session.anchor(), &anchor);
            assert_eq!(r.session.page_position(), pages);
            assert_eq!(
                (r.toolbar_rect(), r.collapsed_rect(), r.toc_panel_rect()),
                rects
            );
            assert_eq!(r.surface.pixel(0, 0), Some(theme.colors().0));
            assert_eq!(pixel(&r, r.toolbar_rect()), p.panel);
            let corner = r
                .surface
                .pixel_point((f64::from(r.toolbar_rect().x), f64::from(r.toolbar_rect().y)));
            assert_ne!(
                r.surface.pixel(corner.0 as u32, corner.1 as u32),
                Some(p.panel),
                "rounded menu must expose the page at the corner"
            );
            r.handle_toolbar_button(1).unwrap();
            assert_eq!(pixel(&r, r.toc_panel_rect()), p.panel);
            r.handle_toolbar_button(1).unwrap();
            r.action(Action::Command(ReaderCommand::Settings)).unwrap();
            assert_eq!(pixel(&r, r.tool_panel()), p.panel);
            r.action(Action::Close).unwrap();
            r.handle_toolbar_button(5).unwrap();
            assert_eq!(pixel(&r, r.collapsed_rect()), p.panel);
            let rect = r.collapsed_rect();
            r.action(Action::Click {
                x: rect.x + 2,
                y: rect.y + 2,
            })
            .unwrap();
        }
    }
}
#[test]
fn changing_theme_preserves_images_and_all_page_modes() {
    let temp = Temp::new();
    let bytes = test_epub::make_epub_with_resources(
        &["<html><body><img src='sample.png'/><p>AAAA WWWW</p></body></html>"],
        vec![(
            "sample.png",
            "image/png",
            test_epub::make_png(50, 50, [10, 90, 180, 255]),
        )],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let data = test_font::make_font();
    let font = Font::parse(&data, 0, FontLimits::default()).unwrap();
    for mode in [PageMode::Slide, PageMode::Book, PageMode::Scroll] {
        let preferences = Settings {
            page_mode: mode,
            ..Settings::default()
        };
        let session = EpubSession::new_with_preferences(
            &book,
            &font,
            options(false),
            Start::Beginning,
            &[],
            preferences,
        )
        .unwrap();
        let mut r = ReaderWindow::new_lazy(
            session,
            None,
            UiFont::from_bytes(data.clone(), "fixture.ttf".into()).unwrap(),
        )
        .unwrap();
        r.tools.store = Some(Store::new(temp.0.clone()));
        r.freeze_motion().unwrap();
        let anchor = r.session.anchor().clone();
        r.action(Action::Command(ReaderCommand::Theme)).unwrap();
        assert_eq!(r.session.anchor(), &anchor);
        assert_eq!(r.session.settings().page_mode, mode);
        assert!(
            r.session
                .raw_surface()
                .pixels()
                .contains(&Color::rgba(10, 90, 180, 255))
        );
    }
}

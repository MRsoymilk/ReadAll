//! Regression coverage for a single active TOC row, not separate current/hover rows.
use super::*;
use crate::{test_epub, test_font};

fn with_toc(count: usize, test: impl FnOnce(&mut ReaderWindow<'_, '_, '_, '_>)) {
    let chapters = vec!["<html><body><p>AAAA WWWW</p></body></html>"; count];
    let bytes = test_epub::make_epub_with_resources(&chapters, vec![]);
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let fb = test_font::make_font();
    let font = Font::parse(&fb, 0, FontLimits::default()).unwrap();
    let options = Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "640",
            "--height",
            "600",
            "--margin",
            "40",
        ]
        .map(Into::into),
    )
    .unwrap();
    let session = EpubSession::new(&book, &font, options, Start::Beginning).unwrap();
    let ui = UiFont::from_bytes(fb.clone(), PathBuf::from("fixture.ttf")).unwrap();
    let mut reader = ReaderWindow::new(session, None, ui).unwrap();
    reader.handle_toolbar_button(1).unwrap();
    test(&mut reader);
}

fn assert_highlight(reader: &ReaderWindow<'_, '_, '_, '_>, selected: usize) {
    assert_eq!(reader.toolbar, ToolbarMode::Toc);
    assert_eq!(reader.toc_selected, selected);
    let panel = reader.toc_panel_rect();
    let x = (panel.x + panel.width as i32 - 12) as u32;
    let visible = reader
        .visible_toc_rows()
        .min(reader.toc.len().saturating_sub(reader.toc_scroll));
    let mut active = 0;
    for row in 0..visible {
        let y = (panel.y + 48 + row as i32 * 38 + 2) as u32;
        // Sample outside text, reconstructing the unchanged panel background.
        let page = reader.session.frame().surface.pixel(x, y).unwrap();
        let background = Color::rgba(250, 251, 253, 246).over(Color::rgba(0, 0, 0, 35).over(page));
        let current = reader.toc_scroll + row == selected;
        let expected = if current {
            Color::rgba(220, 232, 249, 245).over(background)
        } else {
            background
        };
        let pixel = reader.surface.pixel(x, y).unwrap();
        assert_eq!(pixel, expected, "TOC row {}", reader.toc_scroll + row);
        active += usize::from(pixel != background);
    }
    assert_eq!(active, usize::from(!reader.toc.is_empty()));
}

#[test]
fn toc_hover_wheel_and_keyboard_have_one_active_row_without_moving_reading_progress() {
    with_toc(12, |reader| {
        let origin = reader.session.anchor().clone();
        assert_highlight(reader, 0);
        let panel = reader.toc_panel_rect();
        let motion = Action::PointerMove {
            x: panel.x + 24,
            y: panel.y + 48 + 38 + 12,
        };
        reader.action(motion).unwrap();
        assert_highlight(reader, 1);
        assert_eq!(reader.current_toc_index(), Some(0));
        reader.action(Action::Next).unwrap();
        assert_highlight(reader, 2);
        // Mouse takeover must work even though the hovered index itself did not change.
        assert!(reader.action(motion).unwrap());
        assert_highlight(reader, 1);
        assert!(!reader.action(motion).unwrap());
        reader.action(Action::PointerLeave).unwrap();
        assert_highlight(reader, 1);
        assert_eq!(reader.session.anchor(), &origin);
        reader.action(Action::Activate).unwrap();
        assert_eq!(reader.session.current_spine(), 1);
        assert_eq!(reader.toolbar, ToolbarMode::Expanded);
    });
}

#[test]
fn toc_scrolling_and_reentering_visible_rows_never_highlight_the_reading_chapter_twice() {
    with_toc(16, |reader| {
        let origin = reader.session.anchor().clone();
        let panel = reader.toc_panel_rect();
        let motion = Action::PointerMove {
            x: panel.x + 24,
            y: panel.y + 60,
        };
        reader.action(motion).unwrap();
        for selected in 1..16 {
            reader.action(Action::Next).unwrap();
            assert_highlight(reader, selected);
        }
        reader.action(Action::Next).unwrap();
        assert_highlight(reader, 15);
        let target = reader.toc_scroll;
        reader.action(motion).unwrap();
        assert_highlight(reader, target);
        reader.action(Action::Last).unwrap();
        assert_highlight(reader, 15);
        for selected in (0..15).rev() {
            reader.action(Action::Previous).unwrap();
            assert_highlight(reader, selected);
        }
        reader.action(Action::Previous).unwrap();
        assert_highlight(reader, 0);
        for (x, y) in [
            (panel.x - 1, panel.y + 60),
            (panel.x + 24, panel.y + 48 + 36),
            (panel.x + 24, panel.y + 20),
        ] {
            reader.action(Action::PointerMove { x, y }).unwrap();
            assert_highlight(reader, 0);
        }
        reader.action(Action::First).unwrap();
        assert_eq!(reader.session.anchor(), &origin);
        reader.toc.clear();
        reader.refresh_surface().unwrap();
        reader.action(motion).unwrap();
        reader.action(Action::Next).unwrap();
        assert_highlight(reader, 0);
    });
}

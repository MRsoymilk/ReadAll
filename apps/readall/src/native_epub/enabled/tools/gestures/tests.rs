use super::super::link_tests::{book, with_reader};
use super::*;
use crate::test_epub;

fn point(rect: Rect) -> (i32, i32) {
    (
        rect.x + (rect.width / 2) as i32,
        rect.y + (rect.height / 2) as i32,
    )
}
fn press(reader: &mut ReaderWindow<'_, '_, '_, '_>, at: (i32, i32)) {
    reader.action(Action::Click { x: at.0, y: at.1 }).unwrap();
}
fn motion(reader: &mut ReaderWindow<'_, '_, '_, '_>, at: (i32, i32)) {
    reader
        .action(Action::PointerMove { x: at.0, y: at.1 })
        .unwrap();
}
fn release(reader: &mut ReaderWindow<'_, '_, '_, '_>, at: (i32, i32)) {
    reader
        .action(Action::PointerRelease { x: at.0, y: at.1 })
        .unwrap();
}
fn source() -> String {
    format!(
        "<html><body>{}</body></html>",
        "<p>AAAA WWWW AAAA</p>".repeat(60)
    )
}

#[test]
fn body_clicks_never_turn_pages_with_expanded_or_collapsed_toolbar() {
    with_reader(book(&[&source()]), |reader| {
        assert!(!reader.tools.selecting);
        assert!(reader.session.frame().pages > 1);
        for toolbar in [ToolbarMode::Expanded, ToolbarMode::Collapsed] {
            reader.toolbar = toolbar;
            reader.refresh_surface().unwrap();
            let origin = reader.session.anchor().clone();
            let pixels = reader.session.frame().surface.pixels().to_vec();
            // Header, left/right page margin and content all keep the current page.
            for at in [
                (4, 140),
                (630, 140),
                (300, 8),
                point(reader.session.frame().hits[0].rect),
            ] {
                press(reader, at);
                assert_eq!(reader.session.anchor(), &origin);
                release(reader, at);
                assert_eq!(reader.session.anchor(), &origin);
                assert_eq!(reader.session.frame().surface.pixels(), pixels);
                assert!(!reader.close_requested());
            }
        }
        reader.action(Action::Next).unwrap();
        assert_eq!(reader.session.frame().page, 1);
        reader.action(Action::Previous).unwrap();
        assert_eq!(reader.session.frame().page, 0);
        reader.handle_toolbar_button(4).unwrap();
        assert_eq!(reader.session.frame().page, 1);
        reader.handle_toolbar_button(0).unwrap();
        assert_eq!(reader.session.frame().page, 0);
    });
}

#[test]
fn default_cross_line_selection_supports_reverse_drag_and_escape_without_exiting() {
    with_reader(book(&[&source()]), |reader| {
        let origin = reader.session.anchor().clone();
        let hits = reader.session.frame().hits.clone();
        let first = &hits[0];
        let last = hits
            .iter()
            .find(|hit| hit.rect.y > first.rect.y + first.rect.height as i32)
            .unwrap();
        let (a, b) = (point(first.rect), point(last.rect));
        for (from, to) in [(a, b), (b, a)] {
            press(reader, from);
            motion(reader, to);
            release(reader, to);
            assert_eq!(reader.tools.selection, Some(first.start..last.end));
            assert!(
                reader
                    .session
                    .text_at(reader.tools.selection.clone().unwrap())
                    .contains('\n')
            );
            assert!(reader.tools.drag.is_none());
            assert_eq!(reader.session.anchor(), &origin);
            reader.action(Action::Close).unwrap();
            assert!(reader.tools.selection.is_none());
            assert!(!reader.close_requested());
        }
        // Releasing elsewhere must classify a drag even without an intervening move event.
        press(reader, a);
        release(reader, b);
        assert_eq!(reader.tools.selection, Some(first.start..last.end));
        press(reader, (630, 140));
        release(reader, (630, 140));
        assert!(reader.tools.selection.is_none());
        assert_eq!(reader.session.anchor(), &origin);
    });
}

#[test]
fn dragging_link_text_selects_instead_of_activating_including_external_links() {
    for href in ["#note", "https://example.invalid/path"] {
        let source = format!(
            "<html><body><p>AAAA</p><p><a href='{href}'>AAAA WWWW</a></p><p id='note'>AAAA</p></body></html>"
        );
        with_reader(book(&[&source]), |reader| {
            let origin = reader.session.anchor().clone();
            let regions: Vec<_> = reader.session.link_regions().map(|(r, _)| r).collect();
            let (a, b) = (point(regions[0]), point(*regions.last().unwrap()));
            press(reader, a);
            assert_eq!(reader.session.anchor(), &origin);
            assert_eq!(reader.tools.mode, Mode::None);
            assert!(reader.tools.selection.is_none());
            motion(reader, b);
            release(reader, b);
            assert_eq!(reader.tools.mode, Mode::None);
            assert_eq!(reader.session.anchor(), &origin);
            assert_eq!(
                reader
                    .session
                    .text_at(reader.tools.selection.clone().unwrap()),
                "AAAA WWWW"
            );
            assert!(reader.tools.link_history.is_empty());
            assert!(!reader.tools.pending());
            // Moving out and back must not accidentally re-enable a link click.
            press(reader, a);
            motion(reader, b);
            motion(reader, a);
            release(reader, a);
            assert_eq!(reader.session.anchor(), &origin);
            assert_eq!(reader.tools.mode, Mode::None);
            press(reader, a);
            release(reader, a);
            if href.starts_with('#') {
                assert_ne!(reader.session.anchor(), &origin);
            } else {
                assert_eq!(reader.tools.mode, Mode::External);
                assert!(
                    !reader.tools.pending(),
                    "confirmation must not launch a browser"
                );
            }
        });
    }
}

#[test]
fn drag_blocks_accidental_wheel_page_changes_and_cancels_on_leave_or_reflow() {
    with_reader(book(&[&source()]), |reader| {
        let origin = reader.session.anchor().clone();
        let hits = reader.session.frame().hits.clone();
        let (a, b) = (point(hits[0].rect), point(hits[3].rect));
        press(reader, a);
        motion(reader, b);
        let selected = reader.tools.selection.clone();
        reader.action(Action::Next).unwrap();
        assert_eq!(reader.session.anchor(), &origin);
        assert_eq!(reader.tools.selection, selected);
        reader.action(Action::PointerLeave).unwrap();
        assert!(reader.tools.drag.is_none());
        motion(reader, a);
        release(reader, a);
        assert_eq!(reader.tools.selection, selected);
        press(reader, a);
        reader.resize(700, 500).unwrap();
        assert!(reader.tools.drag.is_none());
        release(reader, b);
        assert_eq!(reader.session.anchor(), &origin);
        reader.action(Action::Next).unwrap();
        assert_ne!(reader.session.anchor(), &origin);
    });
}

#[test]
fn image_press_drag_and_overlay_controls_do_not_click_through() {
    let bytes = test_epub::make_epub_with_resources(
        &["<html><body><img src='a.png'/></body></html>"],
        vec![(
            "a.png",
            "image/png",
            test_epub::make_png(128, 96, [20, 80, 160, 255]),
        )],
    );
    with_reader(bytes, |reader| {
        let origin = reader.session.anchor().clone();
        let a = point(reader.session.frame().image_hits[0].0);
        let b = (a.0 + 30, a.1 + 20);
        press(reader, a);
        motion(reader, b);
        release(reader, b);
        assert_eq!(reader.tools.mode, Mode::None);
        press(reader, a);
        release(reader, a);
        assert_eq!(reader.tools.mode, Mode::Zoom);
        reader.action(Action::Close).unwrap();
        reader.handle_toolbar_button(1).unwrap();
        assert_eq!(reader.toolbar, ToolbarMode::Toc);
        press(reader, (4, 180));
        release(reader, (4, 180));
        assert_eq!(reader.toolbar, ToolbarMode::Expanded);
        assert_eq!(reader.tools.mode, Mode::None);
        assert_eq!(reader.session.anchor(), &origin);
        reader.handle_toolbar_button(5).unwrap();
        let control = point(reader.collapsed_rect());
        press(reader, control);
        release(reader, control);
        assert_eq!(reader.toolbar, ToolbarMode::Expanded);
    });
}

#[test]
fn default_selection_keeps_combining_clusters_intact() {
    with_reader(
        book(&["<html><body><p>A<span style='display:none'>W</span>W</p></body></html>"]),
        |reader| {
            let hits = reader.session.frame().hits.clone();
            press(reader, point(hits[0].rect));
            release(reader, point(hits[1].rect));
            assert_eq!(
                reader
                    .session
                    .text_at(reader.tools.selection.clone().unwrap()),
                "AW"
            );
            assert_eq!(reader.tools.selection, Some(0..3));
        },
    );
    // Extend the fixture's character map for a combining sequence; keep its logical
    // byte range even when shaping produces more than one glyph for the cluster.
    let bytes = crate::test_font::make_layout_font(*b"latn", *b"liga", true, &[(0x301, 3)]);
    let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
    let data = book(&["<html><body>Á W</body></html>"]);
    let publication = EpubBook::parse(&data, EpubLimits::default()).unwrap();
    let opts = Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "640",
            "--height",
            "480",
            "--margin",
            "40",
        ]
        .map(Into::into),
    )
    .unwrap();
    let session = EpubSession::new(&publication, &font, opts, Start::Beginning).unwrap();
    let ui = UiFont::from_bytes(bytes.clone(), PathBuf::from("fixture.ttf")).unwrap();
    let mut reader = ReaderWindow::new(session, None, ui).unwrap();
    reader.toolbar = ToolbarMode::Collapsed;
    let at = point(reader.session.frame().hits[0].rect);
    press(&mut reader, at);
    release(&mut reader, at);
    assert_eq!(reader.tools.selection, Some(0..3));
    assert_eq!(reader.session.text_at(0..3), "Á");
}

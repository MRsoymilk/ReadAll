//! Tool lists use the same selected row for hover, navigation, paint and activation.
use super::link_tests::{book, with_reader};
use super::*;

fn assert_highlight(reader: &ReaderWindow<'_, '_, '_, '_>, selected: usize) {
    assert_eq!(reader.tools.selected, selected);
    let panel = reader.tool_panel();
    let palette = reader.session.settings().theme.palette();
    let background = palette.panel;
    let visible = reader
        .tool_rows()
        .min(reader.tool_count().saturating_sub(reader.tools.scroll));
    let mut active = 0;
    for row in 0..visible {
        let pixel = reader
            .surface
            .pixel(
                (panel.x + 12) as u32,
                (panel.y + 88 + row as i32 * 42 + 2) as u32,
            )
            .unwrap();
        let expected = if reader.tools.scroll + row == selected {
            palette.selected
        } else {
            background
        };
        assert_eq!(pixel, expected, "tool row {}", reader.tools.scroll + row);
        active += usize::from(pixel != background);
    }
    assert_eq!(active, usize::from(reader.tool_count() != 0));
}

#[test]
fn search_annotations_and_settings_hover_move_only_the_active_row() {
    let source = format!("<html><body>{}</body></html>", "<p>AAAA</p>".repeat(12));
    with_reader(book(&[&source]), |reader| {
        let origin = reader.session.anchor().clone();
        let settings = reader.session.settings();
        reader.tool_command(ReaderCommand::Find).unwrap();
        reader.tools.query = "AAAA".into();
        reader.run_search().unwrap();
        reader.tools.annotations = (0..4)
            .map(|i| Annotation {
                id: i + 1,
                kind: Kind::Bookmark,
                locator: origin.clone(),
                end: None,
                text: format!("row{i}"),
            })
            .collect();
        for mode in [Mode::Search, Mode::Annotations, Mode::Settings] {
            reader.tools.mode = mode;
            reader.tools.selected = 0;
            reader.tools.scroll = 0;
            reader.refresh_surface().unwrap();
            assert_highlight(reader, 0);
            let panel = reader.tool_panel();
            let motion = Action::PointerMove {
                x: panel.x + 24,
                y: panel.y + 88 + 42 + 12,
            };
            assert!(reader.action(motion).unwrap());
            assert_highlight(reader, 1);
            assert!(!reader.action(motion).unwrap());
            reader.action(Action::Next).unwrap();
            assert_highlight(reader, 2);
            assert!(reader.action(motion).unwrap());
            assert_highlight(reader, 1);
            for (x, y) in [
                (panel.x - 1, panel.y + 100),
                (panel.x + 24, panel.y + 88 + 39),
                (panel.x + 24, panel.y + 20),
                (panel.x + 24, panel.y + panel.height as i32 - 20),
            ] {
                reader.action(Action::PointerMove { x, y }).unwrap();
                assert_highlight(reader, 1);
            }
            reader.action(Action::PointerLeave).unwrap();
            assert_highlight(reader, 1);
            assert_eq!(reader.session.anchor(), &origin);
            assert_eq!(
                reader.session.settings(),
                settings,
                "hover must not modify a setting"
            );
        }
    });
}

#[test]
fn search_scroll_reentry_activation_and_empty_results_share_the_visible_row() {
    let source = format!("<html><body>{}</body></html>", "<p>AAAA</p>".repeat(16));
    with_reader(book(&[&source]), |reader| {
        reader.tool_command(ReaderCommand::Find).unwrap();
        reader.tools.query = "AAAA".into();
        reader.run_search().unwrap();
        reader.refresh_surface().unwrap();
        let panel = reader.tool_panel();
        let motion = Action::PointerMove {
            x: panel.x + 24,
            y: panel.y + 100,
        };
        reader.action(motion).unwrap();
        for selected in 1..16 {
            reader.action(Action::Next).unwrap();
            assert_highlight(reader, selected);
        }
        reader.action(Action::Next).unwrap();
        assert_highlight(reader, 15);
        let selected = reader.tools.scroll;
        reader.action(motion).unwrap();
        assert_highlight(reader, selected);
        let target = reader.tools.hits[selected].locator.clone();
        reader.action(Action::Activate).unwrap();
        assert_eq!(reader.tools.mode, Mode::None);
        assert_eq!(reader.session.anchor(), &target);
        reader.tool_command(ReaderCommand::Find).unwrap();
        reader.refresh_surface().unwrap();
        reader.action(motion).unwrap();
        reader.action(Action::Next).unwrap();
        assert_highlight(reader, 0);
    });
}

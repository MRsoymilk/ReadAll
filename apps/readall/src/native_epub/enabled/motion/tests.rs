use super::*;
use crate::{reader_data::Settings, test_epub, test_font};
fn with_reader(
    mode: PageMode,
    chapters: &[&str],
    test: impl FnOnce(&mut ReaderWindow<'_, '_, '_, '_>),
) {
    let bytes = test_epub::make_epub_with_resources(chapters, vec![]);
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let font_data = test_font::make_font();
    let font = Font::parse(&font_data, 0, FontLimits::default()).unwrap();
    let options = Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "640",
            "--height",
            "480",
            "--margin",
            "40",
            "--font-size",
            "16",
        ]
        .map(Into::into),
    )
    .unwrap();
    let session = EpubSession::new_with_preferences(
        &book,
        &font,
        options,
        Start::Beginning,
        &[],
        Settings {
            page_mode: mode,
            ..Settings::default()
        },
    )
    .unwrap();
    let ui = UiFont::from_bytes(font_data.clone(), PathBuf::from("fixture.ttf")).unwrap();
    let mut reader = ReaderWindow::new_lazy(session, None, ui).unwrap();
    reader.toolbar = ToolbarMode::Collapsed;
    reader.prefetch_page().unwrap();
    reader.prefetch_page().unwrap();
    reader.refresh_surface().unwrap();
    test(&mut reader);
}
fn long_chapter() -> String {
    format!(
        "<html><body>{}</body></html>",
        "<p>AAAA WWWW AAAA WWWW</p>".repeat(100)
    )
}
fn settle(reader: &mut ReaderWindow<'_, '_, '_, '_>) {
    let base = reader.motion.last_tick.unwrap_or_else(Instant::now);
    for step in 1..=120 {
        reader
            .tick_page_motion(base + Duration::from_millis(step * 16))
            .unwrap();
        if !reader.motion.active() {
            break;
        }
    }
    reader.refresh_surface().unwrap();
    assert!(!reader.motion.active());
}
#[test]
fn cached_neighbours_do_not_advance_progress_and_slide_book_have_distinct_effects() {
    for mode in [PageMode::Slide, PageMode::Book] {
        with_reader(mode, &[&long_chapter()], |r| {
            let original = r.session.anchor().clone();
            let original_page = r.session.page_position();
            assert!(r.session.neighbour_ready(1));
            assert_eq!(r.session.anchor(), &original);
            r.action(Action::Next).unwrap();
            assert_eq!(r.session.page_position().0, original_page.0 + 1);
            let turn = r.motion.turn.as_ref().unwrap();
            assert_eq!(
                turn.effect,
                if mode == PageMode::Book {
                    PageEffect::Book
                } else {
                    PageEffect::Slide
                }
            );
            assert_eq!(r.animation_interval(), Some(Duration::from_millis(16)));
            let until = turn.started + Duration::from_millis(260);
            r.tick_page_motion(until).unwrap();
            r.refresh_surface().unwrap();
            assert!(r.motion.turn.is_none());
            r.action(Action::Previous).unwrap();
            assert_eq!(r.session.anchor(), &original);
        });
    }
}
#[test]
fn scroll_moves_fractional_pixels_then_settles_without_forcing_a_page_turn() {
    with_reader(PageMode::Scroll, &[&long_chapter()], |r| {
        let initial = r.motion.offset;
        let page = r.session.page_position();
        let source = r
            .session
            .frame()
            .hits
            .iter()
            .find(|h| h.rect.y > 160)
            .unwrap()
            .clone();
        r.action(Action::Scroll {
            dx: 0,
            dy: 73 * 256,
        })
        .unwrap();
        let start = r.motion.last_tick.unwrap();
        r.tick_page_motion(start + Duration::from_millis(16))
            .unwrap();
        assert!(r.motion.offset > initial && r.motion.offset < initial + 73.0);
        settle(r);
        assert!((r.motion.offset - initial - 73.0).abs() < 0.2);
        assert_eq!(r.session.page_position(), page);
        let hit = r
            .session
            .frame()
            .hits
            .iter()
            .find(|h| h.start == source.start)
            .unwrap();
        assert!((source.rect.y - hit.rect.y - 73).abs() <= 1);
        let at = (hit.rect.x + 1, hit.rect.y + 1);
        r.action(Action::Click { x: at.0, y: at.1 }).unwrap();
        r.action(Action::PointerRelease { x: at.0, y: at.1 })
            .unwrap();
        assert!(
            r.tools
                .selection
                .as_ref()
                .is_some_and(|selected| selected.contains(&source.start))
        );
        assert_eq!(r.session.page_position(), page);
    });
}
#[test]
fn page_seams_and_chapter_seams_are_pixel_continuous() {
    for chapters in [
        vec![long_chapter()],
        vec![
            "<html><body>AAAA</body></html>".into(),
            "<html><body>WWWW</body></html>".into(),
        ],
    ] {
        let refs: Vec<_> = chapters.iter().map(String::as_str).collect();
        with_reader(PageMode::Scroll, &refs, |r| {
            let stride = r.session.scroll_stride();
            r.session.compose_scroll(stride).unwrap();
            let before = r.session.frame().surface.pixels().to_vec();
            assert!(r.session.turn_cached(1).unwrap());
            r.session.compose_scroll(0.0).unwrap();
            assert_eq!(before, r.session.frame().surface.pixels());
            let mut offset = -1.0;
            let mut target = -1.0;
            r.session
                .normalize_scroll(&mut offset, &mut target)
                .unwrap();
            assert!((offset - (stride - 1.0)).abs() < 0.01);
        });
    }
}
#[test]
fn visible_next_chapter_links_activate_with_correct_hit_coordinates() {
    with_reader(
        PageMode::Scroll,
        &[
            "<html><body>AAAA</body></html>",
            "<html><body><p><a href='chapter2.xhtml'>WWWW</a></p></body></html>",
            "<html><body>AAAA WWWW</body></html>",
        ],
        |r| {
            let stride = r.session.scroll_stride();
            r.motion.offset = stride - 120.0;
            r.motion.target = r.motion.offset;
            r.session.compose_scroll(r.motion.offset).unwrap();
            r.refresh_surface().unwrap();
            // Next chapter's first line starts at original y=40 plus a 120px strip shift.
            let (x, y) = (42, 162);
            let before = r.session.frame().surface.pixels().to_vec();
            r.action(Action::Click { x, y }).unwrap();
            assert_eq!(r.session.current_spine(), 1);
            assert_eq!(r.session.frame().surface.pixels(), before);
            r.action(Action::PointerRelease { x, y }).unwrap();
            assert_eq!(r.session.current_spine(), 2);
        },
    );
}
#[test]
fn right_drag_turns_pages_but_left_text_drag_does_not() {
    with_reader(PageMode::Slide, &[&long_chapter()], |r| {
        let origin = r.session.anchor().clone();
        let hits = r.session.frame().hits.clone();
        let (a, b) = (&hits[0], &hits[4]);
        r.action(Action::Click {
            x: a.rect.x + 1,
            y: a.rect.y + 1,
        })
        .unwrap();
        r.action(Action::PointerMove {
            x: b.rect.x + 1,
            y: b.rect.y + 1,
        })
        .unwrap();
        r.action(Action::PointerRelease {
            x: b.rect.x + 1,
            y: b.rect.y + 1,
        })
        .unwrap();
        assert_eq!(r.session.anchor(), &origin);
        assert!(r.tools.selection.is_some());
        r.action(Action::PanStart { x: 400, y: 180 }).unwrap();
        r.action(Action::PointerMove { x: 270, y: 180 }).unwrap();
        assert_eq!(r.session.anchor(), &origin);
        r.action(Action::PanEnd { x: 270, y: 180 }).unwrap();
        assert_ne!(r.session.anchor(), &origin);
        assert!(r.motion.turn.is_some());
    });
}
#[test]
fn reversing_scroll_stops_at_book_edges_and_resize_preserves_content_anchor() {
    with_reader(PageMode::Scroll, &[&long_chapter()], |r| {
        r.action(Action::Scroll { dx: 0, dy: -262144 }).unwrap();
        settle(r);
        assert_eq!(r.motion.offset, 0.0);
        assert_eq!(r.session.page_position().0, 1);
        r.action(Action::Scroll {
            dx: 0,
            dy: 180 * 256,
        })
        .unwrap();
        settle(r);
        let anchor = r.session.anchor().clone();
        r.resize(700, 520).unwrap();
        assert_eq!(r.session.anchor(), &anchor);
        assert!(!r.motion.active());
        r.action(Action::Last).unwrap();
        r.action(Action::Scroll { dx: 0, dy: 262144 }).unwrap();
        settle(r);
        assert_eq!(r.session.page_position().0, r.session.page_position().1);
        assert_eq!(r.motion.offset, 0.0);
    });
}
#[test]
fn preempted_prefetch_can_retry_and_does_not_change_the_page() {
    use std::sync::{Arc, atomic::AtomicBool};
    with_reader(PageMode::Slide, &[&long_chapter()], |r| {
        let origin = r.session.anchor().clone();
        r.session.clear_paging();
        {
            let _guard = crate::loading::speculate(Arc::new(AtomicBool::new(true)));
            assert!(!r.prefetch_page().unwrap());
            assert!(!r.session.neighbour_checked(1));
        }
        r.prefetch_page().unwrap();
        assert!(r.session.neighbour_ready(1));
        assert_eq!(r.session.anchor(), &origin);
    });
}
#[test]
fn escape_cancels_motion_without_closing_and_short_drag_returns_to_source() {
    with_reader(PageMode::Book, &[&long_chapter()], |r| {
        let origin = r.session.anchor().clone();
        r.action(Action::PanStart { x: 300, y: 200 }).unwrap();
        r.action(Action::PointerMove { x: 280, y: 200 }).unwrap();
        r.action(Action::PanEnd { x: 280, y: 200 }).unwrap();
        let end = r.motion.turn.as_ref().unwrap().started + Duration::from_millis(260);
        r.tick_page_motion(end).unwrap();
        assert_eq!(r.session.anchor(), &origin);
        r.action(Action::Next).unwrap();
        assert!(r.motion.active());
        assert!(r.action(Action::Close).unwrap());
        assert!(!r.motion.active());
        assert!(!r.close_requested());
    });
}
#[test]
fn progress_is_fractional_and_content_locator_can_resume_scrolled_text() {
    with_reader(PageMode::Scroll, &[&long_chapter()], |r| {
        let before = r.session.overall_progress();
        r.action(Action::Scroll {
            dx: 0,
            dy: 150 * 256,
        })
        .unwrap();
        settle(r);
        assert!(r.session.overall_progress() > before);
        let anchor = r.session.anchor().clone();
        r.session.jump_to_locator(anchor.clone()).unwrap();
        r.reset_motion().unwrap();
        assert_eq!(r.session.anchor(), &anchor);
        assert!(
            r.session
                .frame()
                .hits
                .iter()
                .any(|h| h.start <= anchor.utf8_offset() as usize
                    && h.end > anchor.utf8_offset() as usize)
        );
    });
}
#[test]
fn toc_jump_discards_old_scroll_offset_and_resumes_at_the_new_target() {
    with_reader(
        PageMode::Scroll,
        &[&long_chapter(), "<html><body>WWWW</body></html>"],
        |r| {
            r.action(Action::Scroll {
                dx: 0,
                dy: 180 * 256,
            })
            .unwrap();
            settle(r);
            assert!(r.motion.offset > 100.0);
            r.handle_toolbar_button(1).unwrap();
            assert_eq!(r.toolbar, ToolbarMode::Toc);
            r.action(Action::Last).unwrap();
            r.action(Action::Activate).unwrap();
            assert_eq!(r.session.current_spine(), 1);
            assert_eq!(r.session.anchor().utf8_offset(), 0);
            assert!(!r.motion.active());
            assert!(r.motion.offset < 32.0);
            let origin = r.session.anchor().clone();
            r.prefetch_page().unwrap();
            assert_eq!(r.session.anchor(), &origin);
            assert!(
                r.session
                    .frame()
                    .hits
                    .iter()
                    .any(|hit| hit.start == 0 && hit.rect.y >= 32)
            );
        },
    );
}
#[test]
fn five_setting_rows_include_page_mode_without_changing_font_or_locator() {
    with_reader(PageMode::Slide, &[&long_chapter()], |r| {
        let origin = r.session.anchor().clone();
        let old = r.session.settings();
        r.action(Action::Command(
            readall_platform::window::ReaderCommand::Settings,
        ))
        .unwrap();
        r.action(Action::Last).unwrap();
        r.action(Action::Larger).unwrap();
        assert_eq!(r.session.settings().page_mode, PageMode::Book);
        r.action(Action::Larger).unwrap();
        assert_eq!(r.session.settings().page_mode, PageMode::Scroll);
        assert_eq!(r.session.settings().size, old.size);
        assert_eq!(r.session.anchor(), &origin);
        r.action(Action::Smaller).unwrap();
        assert_eq!(r.session.settings().page_mode, PageMode::Book);
    });
}

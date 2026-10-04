use super::*;
use crate::{test_epub, test_font};

pub(super) fn with_reader(bytes: Vec<u8>, test: impl FnOnce(&mut ReaderWindow<'_, '_, '_, '_>)) {
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
            "480",
            "--margin",
            "40",
        ]
        .map(Into::into),
    )
    .unwrap();
    let session = EpubSession::new(&book, &font, options, Start::Beginning).unwrap();
    let ui = UiFont::from_bytes(fb.clone(), PathBuf::from("fixture.ttf")).unwrap();
    let mut reader = ReaderWindow::new(session, None, ui).unwrap();
    reader.toolbar = ToolbarMode::Collapsed;
    reader.refresh_surface().unwrap();
    test(&mut reader);
}
pub(super) fn click_link(reader: &mut ReaderWindow<'_, '_, '_, '_>, href: &str) {
    let rect = reader
        .session
        .link_regions()
        .find(|(_, link)| link.href == href)
        .unwrap()
        .0;
    assert!(
        reader
            .action(Action::Click {
                x: rect.x + (rect.width / 2) as i32,
                y: rect.y + (rect.height / 2) as i32
            })
            .unwrap()
    );
    reader
        .action(Action::PointerRelease {
            x: rect.x + (rect.width / 2) as i32,
            y: rect.y + (rect.height / 2) as i32,
        })
        .unwrap();
}
pub(super) fn book(chapters: &[&str]) -> Vec<u8> {
    test_epub::make_epub_with_resources(chapters, vec![])
}

#[test]
fn same_chapter_footnote_click_and_back_preserve_anchor() {
    with_reader(
        book(&[
            "<html><body><p>AAAA</p><p><a epub:type='noteref' href='#note'>W</a></p><p id='note'>WWWW</p></body></html>",
        ]),
        |reader| {
            let origin = reader.session.anchor().clone();
            let target = reader
                .session
                .book()
                .locator_for_fragment(0, "note")
                .unwrap()
                .unwrap();
            click_link(reader, "#note");
            assert_eq!(reader.session.anchor(), &target);
            assert_eq!(reader.tools.link_history.len(), 1);
            assert!(reader.tools.status.contains("注释"));
            reader.action(Action::Back).unwrap();
            assert_eq!(reader.session.anchor(), &origin);
            assert!(reader.tools.link_history.is_empty());
            assert!(!reader.close_requested);
        },
    );
}
#[test]
fn nested_cross_chapter_links_and_return_button_work() {
    with_reader(
        book(&[
            "<html><body><p>AAAA</p><p><a href='chapter1.xhtml#note'>W</a></p></body></html>",
            "<html><body><p id='note'>AAAA</p><p><a href='chapter2.xhtml#end'>W</a></p></body></html>",
            "<html><body><p id='end'>WWWW</p></body></html>",
        ]),
        |reader| {
            let origin = reader.session.anchor().clone();
            click_link(reader, "chapter1.xhtml#note");
            let middle = reader.session.anchor().clone();
            click_link(reader, "chapter2.xhtml#end");
            assert_eq!(reader.session.current_spine(), 2);
            assert_eq!(reader.tools.link_history.len(), 2);
            let back = reader.link_back_rect();
            reader
                .action(Action::Click {
                    x: back.x + 8,
                    y: back.y + 8,
                })
                .unwrap();
            assert_eq!(reader.session.anchor(), &middle);
            reader.action(Action::Back).unwrap();
            assert_eq!(reader.session.anchor(), &origin);
        },
    );
}
#[test]
fn external_unsafe_and_broken_links_do_not_turn_pages_or_change_progress() {
    for href in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "../../outside.xhtml",
        "#missing",
        "absent.xhtml",
        "chapter0.xhtml?query=1",
    ] {
        let source = format!("<html><body><p>AAAA</p><p><a href='{href}'>W</a></p></body></html>");
        with_reader(book(&[&source]), |reader| {
            let anchor = reader.session.anchor().clone();
            let pixels = reader.session.frame().surface.pixels().to_vec();
            click_link(reader, href);
            assert_eq!(reader.session.anchor(), &anchor, "{href}");
            assert_eq!(reader.session.frame().surface.pixels(), pixels);
            assert!(reader.tools.link_history.is_empty());
            assert!(reader.tools.status.contains("链接未打开"));
        });
    }
}
#[test]
fn selection_has_priority_over_link_activation() {
    with_reader(
        book(&[
            "<html><body><p>AAAA</p><p><a href='#note'>W</a></p><p id='note'>AAAA</p></body></html>",
        ]),
        |reader| {
            let anchor = reader.session.anchor().clone();
            reader.tools.selecting = true;
            click_link(reader, "#note");
            assert_eq!(reader.session.anchor(), &anchor);
            assert!(reader.tools.selection.is_some());
            assert!(reader.tools.link_history.is_empty());
        },
    );
}
#[test]
fn linked_image_uses_link_not_zoom_and_returns_to_exact_image_page() {
    let bytes = test_epub::make_epub_with_resources(
        &[
            "<html><body><img src='a.png'/><a href='chapter1.xhtml#note'><img src='b.png'/></a></body></html>",
            "<html><body><p id='note'>AAAA</p></body></html>",
        ],
        vec![
            (
                "a.png",
                "image/png",
                test_epub::make_png(400, 350, [0, 0, 255, 255]),
            ),
            (
                "b.png",
                "image/png",
                test_epub::make_png(400, 350, [255, 0, 0, 255]),
            ),
        ],
    );
    with_reader(bytes, |reader| {
        let origin = reader.session.book().image_locator(0, 1).unwrap();
        reader.session.jump_to_locator(origin.clone()).unwrap();
        reader.refresh_surface().unwrap();
        click_link(reader, "chapter1.xhtml#note");
        assert_eq!(reader.tools.mode, Mode::None);
        assert_eq!(reader.session.current_spine(), 1);
        reader.action(Action::Back).unwrap();
        assert_eq!(reader.session.anchor(), &origin);
        assert_eq!(reader.session.anchor().image_index(), Some(1));
    });
}
#[test]
fn legacy_named_targets_jump_across_chapters_and_ids_take_precedence() {
    with_reader(
        book(&[
            "<html><body><p>AAAA</p><p><a href='chapter1.xhtml#legacy'>W</a></p></body></html>",
            "<html><body><p>AAAA</p><a name='legacy'/><p>WWWW</p><a name='same'>A</a><p id='same'>WW</p><a name='注释'>A</a></body></html>",
        ]),
        |reader| {
            let origin = reader.session.anchor().clone();
            let book = reader.session.book();
            let canonical = book.read_spine_text(1).unwrap();
            let target = book.link_locator(0, "chapter1.xhtml#legacy").unwrap();
            assert_eq!(target.utf8_offset(), canonical.find("WWWW").unwrap() as u64);
            let same = book.link_locator(0, "chapter1.xhtml#same").unwrap();
            assert_eq!(same.utf8_offset(), canonical.rfind("WW").unwrap() as u64);
            assert!(
                book.link_locator(0, "chapter1.xhtml#%E6%B3%A8%E9%87%8A")
                    .is_ok()
            );
            click_link(reader, "chapter1.xhtml#legacy");
            assert_eq!(reader.session.anchor(), &target);
            reader.action(Action::Back).unwrap();
            assert_eq!(reader.session.anchor(), &origin);
        },
    );
}

#[test]
fn encoded_paths_and_utf8_fragments_resolve_to_canonical_offsets() {
    with_reader(
        book(&[
            "<html><body><p>AAAA</p></body></html>",
            "<html><body><p>AAAA</p><p id='注释'>WWWW</p></body></html>",
        ]),
        |reader| {
            let book = reader.session.book();
            let target = book
                .link_locator(0, "chapter%31.xhtml#%E6%B3%A8%E9%87%8A")
                .unwrap();
            assert_eq!(
                target,
                book.locator_for_fragment(1, "注释").unwrap().unwrap()
            );
            assert!(book.link_locator(99, "#note").is_err());
            assert!(book.link_locator(0, "#%GG").is_err());
        },
    );
}

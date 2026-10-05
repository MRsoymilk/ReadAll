use super::*;
use crate::{test_epub, test_font, text_page::Options};
use readall_platform::window::ReaderCommand;

fn options() -> Options {
    Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "1040",
            "--height",
            "700",
            "--font-size",
            "16",
            "--margin",
            "24",
        ]
        .map(Into::into),
    )
    .unwrap()
}
fn with_reader(test: impl FnOnce(&mut ReaderWindow<'_, '_, '_, '_>)) {
    let chapters = vec!["<html><body><p>AAAA WWWW AAAA WWWW</p></body></html>"; 32];
    let bytes = test_epub::make_epub_with_resources(&chapters, vec![]);
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let data = test_font::make_font();
    let font = Font::parse(&data, 0, FontLimits::default()).unwrap();
    let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
    let mut reader = ReaderWindow::new(
        session,
        None,
        UiFont::from_bytes(data.clone(), "fixture.ttf".into()).unwrap(),
    )
    .unwrap();
    test(&mut reader);
}
#[test]
fn desktop_toc_wheel_is_continuous_directional_and_never_turns_the_book() {
    with_reader(|r| {
        let anchor = r.session.anchor().clone();
        r.handle_toolbar_button(1).unwrap();
        let rows = r.toc_rows_rect();
        r.action(Action::PointerMove {
            x: rows.x + 30,
            y: rows.y + 14,
        })
        .unwrap();
        r.action(Action::Scroll { dx: 0, dy: 5 * 256 }).unwrap();
        assert_eq!(r.toc_offset(), 5.0);
        r.action(Action::Scroll {
            dx: 0,
            dy: 38 * 256,
        })
        .unwrap();
        assert_eq!(r.toc_offset(), 43.0);
        r.action(Action::Scroll {
            dx: 0,
            dy: -10 * 256,
        })
        .unwrap();
        assert_eq!(r.toc_offset(), 33.0);
        r.action(Action::Scroll {
            dx: 0,
            dy: i32::MAX,
        })
        .unwrap();
        assert_eq!(r.toc_offset(), r.toc_max_offset());
        r.action(Action::Scroll { dx: 0, dy: -256 }).unwrap();
        assert_eq!(r.toc_offset(), r.toc_max_offset() - 1.0);
        assert_eq!(r.session.anchor(), &anchor);
        r.action(Action::PointerMove { x: 1, y: 1 }).unwrap();
        let before = r.toc_offset();
        assert!(
            !r.action(Action::Scroll {
                dx: 0,
                dy: 48 * 256
            })
            .unwrap()
        );
        assert_eq!(r.toc_offset(), before);
        r.action(Action::First).unwrap();
        assert_eq!(r.toc_offset(), 0.0);
    });
}
#[test]
fn desktop_settings_labels_wheel_and_hover_are_not_mutations() {
    with_reader(|r| {
        r.action(Action::Command(ReaderCommand::Settings)).unwrap();
        let p = r.tool_panel();
        let y = p.y + 88 + 42 + 16;
        let before = r.session.settings();
        let anchor = r.session.anchor().clone();
        r.action(Action::Click { x: p.x + 24, y }).unwrap();
        assert_eq!(r.session.settings(), before);
        r.action(Action::Scroll {
            dx: 0,
            dy: 12 * 256,
        })
        .unwrap();
        assert_eq!(r.session.settings(), before);
        let x = p.x + p.width as i32 - 40;
        r.action(Action::PointerMove { x, y }).unwrap();
        let hovered = r.surface.clone();
        assert!(!r.action(Action::PointerMove { x: x + 1, y }).unwrap());
        assert!(r.action(Action::PointerLeave).unwrap());
        assert!(r.surface.pixels() != hovered.pixels());
        r.action(Action::Click { x, y }).unwrap();
        assert_eq!(r.session.settings().size, before.size + 2);
        r.action(Action::Click {
            x: p.x + p.width as i32 - 88,
            y,
        })
        .unwrap();
        assert_eq!(r.session.settings(), before);
        assert_eq!(r.session.anchor(), &anchor);
    });
}
#[test]
fn desktop_escape_closes_panels_before_exiting_and_preserves_keyboard_hints() {
    with_reader(|r| {
        r.action(Action::PanStart { x: 200, y: 100 }).unwrap();
        assert!(r.motion.panning());
        assert!(r.action(Action::Close).unwrap());
        assert!(!r.motion.panning());
        assert_eq!(
            r.toolbar,
            ToolbarMode::Expanded,
            "Esc must stop right-button dragging before hiding the toolbar"
        );
        r.action(Action::Command(ReaderCommand::Find)).unwrap();
        assert!(r.tools.status.contains("Enter"));
        assert!(r.action(Action::Close).unwrap());
        assert_eq!(r.tools.mode, tools::Mode::None);
        r.handle_toolbar_button(1).unwrap();
        assert!(r.action(Action::Close).unwrap());
        assert_eq!(r.toolbar, ToolbarMode::Expanded);
        assert!(r.action(Action::Close).unwrap());
        assert_eq!(r.toolbar, ToolbarMode::Collapsed);
        assert!(!r.action(Action::Close).unwrap());
    });
}
#[test]
fn desktop_feedback_expires_without_discarding_back_navigation_or_pending_work() {
    with_reader(|r| {
        let now = Instant::now();
        r.tools.status = "设置已保存".into();
        r.observe_desktop_notice(now);
        r.observe_desktop_notice(now + Duration::from_secs(5));
        assert!(!r.expire_desktop_notice(now + Duration::from_secs(5)));
        r.tools.link_history.push(r.session.anchor().clone());
        assert!(r.expire_desktop_notice(now + Duration::from_secs(6)));
        assert!(r.tools.status.is_empty());
        assert_eq!(r.tools.link_history.len(), 1);
        r.tools.status = "新提示".into();
        r.observe_desktop_notice(now);
        r.tools.mode = tools::Mode::Settings;
        r.observe_desktop_notice(now);
        assert!(!r.expire_desktop_notice(now + Duration::from_secs(30)));
        assert_eq!(r.tools.status, "新提示");
        r.tools.mode = tools::Mode::None;
        r.refresh_surface().unwrap();
        let close = r.desktop_notice_close();
        assert!(
            r.action(Action::Click {
                x: close.x + 10,
                y: close.y + 10
            })
            .unwrap()
        );
        assert!(r.tools.status.is_empty());
        assert_eq!(r.tools.link_history.len(), 1);
    });
}

#[test]
#[ignore = "manual screenshot export using the built-in font; no real books or user state"]
#[cfg(all(target_os = "linux", feature = "wayland"))]
fn capture_linux_modern_menus() {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/linux-ui-validation/screens");
    std::fs::create_dir_all(&root).unwrap();
    let content = format!(
        "<html><body><h1>原生阅读界面</h1><p>{}</p></body></html>",
        "简约的界面，让阅读回到内容本身。鼠标选择文字，滚轮浏览目录，键盘也能完成日常操作。"
            .repeat(40)
    );
    let chapters = vec![content.as_str(); 32];
    let bytes = test_epub::make_epub_with_resources(&chapters, vec![]);
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let data = crate::ui::builtin_font_bytes();
    let font = Font::parse(data, 0, FontLimits::default()).unwrap();
    let session = EpubSession::new(&book, &font, options(), Start::Beginning).unwrap();
    let mut r = ReaderWindow::new(
        session,
        None,
        UiFont::from_bytes(data.to_vec(), "builtin.ttf".into()).unwrap(),
    )
    .unwrap();
    fn save(r: &ReaderWindow<'_, '_, '_, '_>, path: &std::path::Path) {
        let mut bytes = format!(
            "P6\n{} {}\n255\n",
            r.surface.pixel_width(),
            r.surface.pixel_height()
        )
        .into_bytes();
        for p in r.surface.pixels() {
            bytes.extend_from_slice(&[p.r, p.g, p.b]);
        }
        std::fs::write(path, bytes).unwrap();
    }
    for name in ["light", "dark"] {
        if name == "dark" {
            r.action(Action::Command(ReaderCommand::Theme)).unwrap();
            r.tools.status.clear();
            r.refresh_surface().unwrap();
        }
        save(&r, &root.join(format!("{name}-toolbar.ppm")));
        r.handle_toolbar_button(1).unwrap();
        save(&r, &root.join(format!("{name}-toc.ppm")));
        r.handle_toolbar_button(1).unwrap();
        r.action(Action::Command(ReaderCommand::Settings)).unwrap();
        save(&r, &root.join(format!("{name}-settings.ppm")));
        r.action(Action::Close).unwrap();
    }
    println!("desktop captures: {}", root.display());
}

use super::*;
use crate::{loading, test_font};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Reader {
    surface: Surface,
    editing: bool,
    page: usize,
}
impl Reader {
    fn new(size: (u32, u32)) -> Self {
        Self {
            surface: Surface::new(size.0, size.1, RenderLimits::default()).unwrap(),
            editing: false,
            page: 0,
        }
    }
}
impl WindowHandler for Reader {
    fn resize(&mut self, w: u32, h: u32) -> WindowResult<bool> {
        if (w, h) == (self.surface.width(), self.surface.height()) {
            return Ok(false);
        }
        self.surface = Surface::new(w, h, RenderLimits::default())?;
        Ok(true)
    }
    fn surface(&self) -> &Surface {
        &self.surface
    }
    fn title(&self) -> String {
        format!("page {}", self.page)
    }
    fn text_input_active(&self) -> bool {
        self.editing
    }
    fn action(&mut self, action: Action) -> WindowResult<bool> {
        match action {
            Action::Command(_) => self.editing = true,
            Action::Next => self.page += 1,
            Action::Close if self.editing => self.editing = false,
            Action::Close => return Ok(false),
            _ => return Ok(false),
        }
        Ok(true)
    }
}
fn window(
    factory: impl Fn(Bridge) -> WindowResult<Vec<u8>> + Send + Sync + 'static,
) -> AsyncWindow {
    let ui = UiFont::from_bytes(test_font::make_font(), PathBuf::from("fixture.ttf")).unwrap();
    AsyncWindow::new(
        PathBuf::from("测试书.epub"),
        (640, 480),
        ui,
        Arc::new(factory),
    )
    .unwrap()
}
fn until(window: &mut AsyncWindow, condition: impl Fn(&AsyncWindow) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition(window) {
        assert!(
            Instant::now() < deadline,
            "worker did not reach expected state"
        );
        window.frame_presented();
        window.animation_tick().unwrap();
        thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn loading_surface_precedes_io_and_worker_uses_configured_size() {
    let called = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&called);
    let mut w = window(move |bridge| {
        seen.store(true, Ordering::Release);
        assert_eq!(bridge.size(), (720, 500));
        let mut r = Reader::new(bridge.size());
        bridge.serve(&mut r)?;
        Ok(vec![])
    });
    assert!(!called.load(Ordering::Acquire));
    assert!(!w.ready);
    assert!(w.surface.pixels().iter().any(|p| p.a != 0));
    w.resize(720, 500).unwrap();
    assert!(!called.load(Ordering::Acquire));
    w.animation_tick().unwrap();
    assert!(!called.load(Ordering::Acquire));
    until(&mut w, |w| w.ready);
    assert_eq!((w.surface.width(), w.surface.height()), (720, 500));
    w.action(Action::Next).unwrap();
    until(&mut w, |w| w.title == "page 1");
}
#[test]
fn cancel_has_a_live_status_and_worker_stops_at_checkpoint() {
    let mut w = window(|bridge| {
        loop {
            loading::step("测试慢速排版", 1, 10)?;
            bridge.checkpoint()?;
            thread::sleep(Duration::from_millis(2));
        }
    });
    w.frame_presented();
    w.animation_tick().unwrap();
    w.action(Action::Close).unwrap();
    assert!(w.cancelling);
    until(&mut w, |w| w.close);
    assert!(!w.ready);
}
#[test]
fn failure_remains_visible_and_retry_replaces_the_failed_worker() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&attempts);
    let mut w = window(move |bridge| {
        if count.fetch_add(1, Ordering::AcqRel) == 0 {
            return Err("损坏的测试文档".into());
        }
        let mut r = Reader::new(bridge.size());
        bridge.serve(&mut r)?;
        Ok(vec![])
    });
    until(&mut w, |w| w.error.is_some());
    assert!(!w.close);
    assert!(w.error.as_ref().unwrap().contains("损坏"));
    w.action(Action::Activate).unwrap();
    until(&mut w, |w| w.ready);
    assert_eq!(attempts.load(Ordering::Acquire), 2);
}
#[test]
fn close_dismisses_an_editor_before_closing_the_window() {
    let mut w = window(|bridge| {
        let mut r = Reader::new(bridge.size());
        bridge.serve(&mut r)?;
        Ok(vec![])
    });
    until(&mut w, |w| w.ready);
    w.action(Action::Command(
        readall_platform::window::ReaderCommand::Find,
    ))
    .unwrap();
    until(&mut w, |w| w.editing);
    assert!(w.action(Action::Close).unwrap());
    until(&mut w, |w| !w.editing);
    assert!(!w.close);
    w.action(Action::Close).unwrap();
    until(&mut w, |w| w.close);
}
#[test]
fn status_surface_supports_small_windows_and_optional_visual_capture() {
    let mut w = window(|_| Ok(vec![]));
    let _guard = w.shared.tracker.install();
    loading::step("章节文字排版", 37, 100).unwrap();
    for (width, height) in [(256, 256), (640, 480), (1920, 1080)] {
        w.resize(width, height).unwrap();
        w.paint_status().unwrap();
        assert_eq!(w.surface.pixels().len(), width as usize * height as usize);
        assert!(w.surface.pixels().iter().all(|p| p.a == 255));
    }
    if let Some(path) = std::env::var_os("READALL_LOADING_PREVIEW") {
        w.ui = UiFont::system().unwrap();
        w.resize(800, 600).unwrap();
        w.paint_status().unwrap();
        w.surface
            .write_ppm(&mut std::fs::File::create(path).unwrap())
            .unwrap();
    }
}

#[test]
fn real_reader_links_and_external_cancel_work_through_async_mailbox() {
    struct LinkedReader<'a, 'b, 'c, 'd>(super::super::ReaderWindow<'a, 'b, 'c, 'd>);
    impl WindowHandler for LinkedReader<'_, '_, '_, '_> {
        fn resize(&mut self, w: u32, h: u32) -> WindowResult<bool> {
            self.0.resize(w, h)
        }
        fn action(&mut self, action: Action) -> WindowResult<bool> {
            self.0.action(action)
        }
        fn surface(&self) -> &Surface {
            self.0.surface()
        }
        fn title(&self) -> String {
            format!("{}:{:?}", self.0.session.current_spine(), self.0.tools.mode)
        }
        fn close_requested(&self) -> bool {
            self.0.close_requested()
        }
        fn animation_interval(&self) -> Option<Duration> {
            self.0.animation_interval()
        }
        fn animation_tick(&mut self) -> WindowResult<bool> {
            self.0.animation_tick()
        }
    }
    let (sender, points) = std::sync::mpsc::channel();
    let mut w = window(move |bridge| {
        use super::super::*;
        let bytes = crate::test_epub::make_epub_with_resources(
            &[
                "<html><body><p>AAAA</p><p><a href='chapter1.xhtml#note'>W</a></p></body></html>",
                "<html><body><p id='note'>AAAA</p><p><a href='https://example.invalid/page'>W</a></p></body></html>",
            ],
            vec![],
        );
        let book = EpubBook::parse(&bytes, EpubLimits::default())?;
        let fb = test_font::make_font();
        let font = Font::parse(&fb, 0, FontLimits::default())?;
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
        )?;
        let session = EpubSession::new(&book, &font, options, Start::Beginning)?;
        let ui = UiFont::from_bytes(fb.clone(), PathBuf::from("fixture.ttf"))?;
        let mut reader = ReaderWindow::new(session, None, ui)?;
        reader.toolbar = ToolbarMode::Collapsed;
        let origin = reader.session.anchor().clone();
        let internal = reader.session.link_regions().next().unwrap().0;
        reader
            .session
            .jump_to_locator(book.link_locator(0, "chapter1.xhtml#note")?)?;
        let external = reader.session.link_regions().next().unwrap().0;
        reader.session.jump_to_locator(origin)?;
        reader.refresh_surface()?;
        sender.send((internal, external)).unwrap();
        bridge.serve(&mut LinkedReader(reader))?;
        Ok(Vec::new())
    });
    until(&mut w, |w| w.ready);
    let (internal, external) = points.recv_timeout(Duration::from_secs(1)).unwrap();
    let click = |rect: Rect| Action::Click {
        x: rect.x + rect.width as i32 / 2,
        y: rect.y + rect.height as i32 / 2,
    };
    w.action(click(internal)).unwrap();
    until(&mut w, |w| w.title == "1:None");
    w.action(click(external)).unwrap();
    until(&mut w, |w| w.title == "1:External");
    w.action(Action::Close).unwrap();
    until(&mut w, |w| w.title == "1:None");
    assert!(!w.close);
    assert!(!w.cancelling);
    w.action(Action::Back).unwrap();
    until(&mut w, |w| w.title == "0:None");
    assert!(!w.close);
}

#[test]
fn queue_and_frame_mailboxes_are_bounded_and_motion_keeps_click_order() {
    let shared = Shared::new((640, 480));
    for x in 0..1000 {
        shared.push(Action::PointerMove { x, y: 0 });
        shared.resize(x as u32 + 256, 480);
    }
    {
        let p = shared.pending.lock().unwrap();
        assert_eq!(p.actions.len(), 1);
        assert_eq!(p.size, Some((1255, 480)));
    }
    shared.push(Action::Click { x: 10, y: 20 });
    shared.push(Action::PointerMove { x: 30, y: 40 });
    {
        let p = shared.pending.lock().unwrap();
        assert_eq!(p.actions.len(), 3);
        assert!(matches!(p.actions[1], Action::Click { .. }));
    }
    for _ in 0..500 {
        shared.push(Action::Next);
    }
    assert_eq!(shared.pending.lock().unwrap().actions.len(), 128);
    let bridge = Bridge {
        shared: Arc::clone(&shared),
    };
    let mut r = Reader::new((640, 480));
    for i in 0..100 {
        r.page = i;
        bridge.publish(&r);
    }
    assert_eq!(
        shared.frame.lock().unwrap().as_ref().unwrap().title,
        "page 99"
    );
}

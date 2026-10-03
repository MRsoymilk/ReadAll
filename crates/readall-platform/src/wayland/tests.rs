//! Protocol smoke tests use a local mock compositor, not the user's desktop.
//! They validate wire ordering and lifetimes; they are NOT visual/compositor interoperability tests.
use super::*;
use crate::window::WindowOptions;
use readall_render::{Color, DrawCommand, Rect, RenderLimits, Surface};
use std::{
    collections::HashMap,
    io::Read,
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    thread,
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "readall-wayland-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|n| n.to_ne_bytes()).collect()
}
fn string(value: &str) -> Vec<u8> {
    let mut bytes = words(&[value.len() as u32 + 1]);
    bytes.extend_from_slice(value.as_bytes());
    bytes.push(0);
    while bytes.len() % 4 != 0 {
        bytes.push(0);
    }
    bytes
}
fn event_bytes(object: u32, opcode: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = words(&[object, ((body.len() as u32 + 8) << 16) | u32::from(opcode)]);
    bytes.extend_from_slice(body);
    bytes
}
fn event(stream: &mut UnixStream, object: u32, opcode: u16, body: &[u8]) {
    stream
        .write_all(&event_bytes(object, opcode, body))
        .unwrap();
}
#[derive(Clone, Copy, Debug)]
enum Kind {
    Display,
    Registry,
    Compositor,
    Shm,
    Wm,
    Seat,
    Surface,
    Xdg,
    Top,
    Keyboard,
    Pointer,
    Pool,
    Buffer(u32, u32),
}
#[derive(Default, Debug)]
struct Observed {
    frames: usize,
    releases: usize,
    pong: bool,
    sizes: Vec<(u32, u32)>,
}
fn serve(listener: UnixListener, scripted: bool) -> Observed {
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(12)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(12)))
        .unwrap();
    let mut objects = HashMap::from([(1, Kind::Display)]);
    let (mut xdg, mut top, mut keyboard, mut surface_id) = (0, 0, 0, 0);
    let (mut configured, mut acknowledged, mut serial) = (false, false, 10);
    let (mut attached, mut previous) = (None, None);
    let mut observed = Observed::default();
    loop {
        let mut header = [0; 8];
        match stream.read_exact(&mut header) {
            Ok(()) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
                ) =>
            {
                break;
            }
            Err(e) => panic!("mock read failed: {e}; {observed:?}"),
        }
        let object = word(&header, 0);
        let message = word(&header, 4);
        let opcode = message & 0xffff;
        let size = (message >> 16) as usize;
        assert!((8..=65535).contains(&size));
        let mut body = vec![0; size - 8];
        stream.read_exact(&mut body).unwrap();
        let kind = objects
            .get(&object)
            .copied()
            .unwrap_or_else(|| panic!("unknown mock object {object}"));
        match (kind, opcode) {
            (Kind::Display, 1) => {
                let id = word(&body, 0);
                objects.insert(id, Kind::Registry);
                for (name, interface, version) in [
                    (1, "wl_compositor", 4),
                    (2, "wl_shm", 1),
                    (3, "xdg_wm_base", 1),
                    (4, "wl_seat", 5),
                ] {
                    let mut data = words(&[name]);
                    data.extend(string(interface));
                    data.extend(words(&[version]));
                    event(&mut stream, id, 0, &data);
                }
            }
            (Kind::Display, 0) => {
                let id = word(&body, 0);
                // The client may disconnect immediately after callback.done. Send the
                // paired delete_id in the same write so test teardown cannot race the
                // second protocol event and masquerade as a client failure.
                let mut response = event_bytes(id, 0, &words(&[1]));
                response.extend(event_bytes(1, 1, &words(&[id])));
                stream.write_all(&response).unwrap();
            }
            (Kind::Registry, 0) => {
                let name = word(&body, 0);
                let len = word(&body, 4) as usize;
                let end = 8 + len.div_ceil(4) * 4;
                let version = word(&body, end);
                let id = word(&body, end + 4);
                let kind = match name {
                    1 => Kind::Compositor,
                    2 => Kind::Shm,
                    3 => Kind::Wm,
                    4 => Kind::Seat,
                    _ => panic!("unexpected bind"),
                };
                objects.insert(id, kind);
                if name == 2 {
                    assert_eq!(version, 1);
                    event(&mut stream, id, 0, &words(&[1]));
                }
                if name == 3 {
                    event(&mut stream, id, 0, &words(&[987]));
                }
                if name == 4 {
                    assert_eq!(version, 5);
                    event(&mut stream, id, 0, &words(&[3]));
                }
            }
            (Kind::Compositor, 0) => {
                surface_id = word(&body, 0);
                objects.insert(surface_id, Kind::Surface);
            }
            (Kind::Wm, 2) => {
                xdg = word(&body, 0);
                assert_eq!(word(&body, 4), surface_id);
                objects.insert(xdg, Kind::Xdg);
            }
            (Kind::Wm, 3) => {
                assert_eq!(word(&body, 0), 987);
                observed.pong = true;
            }
            (Kind::Xdg, 1) => {
                top = word(&body, 0);
                objects.insert(top, Kind::Top);
            }
            (Kind::Xdg, 4) => {
                assert_eq!(word(&body, 0), serial);
                acknowledged = true;
            }
            (Kind::Xdg, 3) | (Kind::Top, 2 | 3 | 7 | 8) => {}
            (Kind::Seat, 1) => {
                keyboard = word(&body, 0);
                objects.insert(keyboard, Kind::Keyboard);
                event(&mut stream, keyboard, 1, &words(&[1, surface_id, 0]));
            }
            (Kind::Seat, 0) => {
                objects.insert(word(&body, 0), Kind::Pointer);
            }
            (Kind::Shm, 0) => {
                assert_eq!(body.len(), 8);
                assert!(word(&body, 4) > 0);
                objects.insert(word(&body, 0), Kind::Pool);
            }
            (Kind::Pool, 0) => {
                let id = word(&body, 0);
                let (w, h) = (word(&body, 8), word(&body, 12));
                assert_eq!(word(&body, 4), 0);
                assert_eq!(word(&body, 16), w * 4);
                assert_eq!(word(&body, 20), 1);
                objects.insert(id, Kind::Buffer(w, h));
            }
            (Kind::Pool, 1) => {
                objects.remove(&object);
                event(&mut stream, 1, 1, &words(&[object]));
            }
            (Kind::Buffer(_, _), 0) => {
                observed.releases += 1;
                objects.remove(&object);
                event(&mut stream, 1, 1, &words(&[object]));
            }
            (Kind::Surface, 1) => {
                assert!(
                    acknowledged,
                    "buffer attached before configure acknowledgement"
                );
                attached = Some(word(&body, 0));
            }
            (Kind::Surface, 9) => {}
            (Kind::Surface, 6) => {
                if !configured {
                    assert!(attached.is_none());
                    configured = true;
                    event(&mut stream, top, 0, &words(&[320, 300, 0]));
                    event(&mut stream, xdg, 0, &words(&[serial]));
                    continue;
                }
                let id = attached.take().expect("commit without a new buffer");
                let Kind::Buffer(w, h) = objects[&id] else {
                    panic!("invalid attached buffer")
                };
                observed.frames += 1;
                observed.sizes.push((w, h));
                if let Some(id) = previous.replace(id) {
                    event(&mut stream, id, 0, &[]);
                }
                if scripted && observed.frames == 1 {
                    assert_ne!(keyboard, 0);
                    event(&mut stream, keyboard, 3, &words(&[2, 0, 109, 1]));
                }
                if scripted && observed.frames == 2 {
                    serial += 1;
                    acknowledged = false;
                    event(&mut stream, top, 0, &words(&[640, 480, 0]));
                    event(&mut stream, xdg, 0, &words(&[serial]));
                }
            }
            _ => panic!("unhandled mock request {kind:?} opcode {opcode}"),
        }
    }
    observed
}
struct Handler {
    surface: Surface,
    actions: usize,
    sizes: Vec<(u32, u32)>,
}
impl Handler {
    fn new() -> Self {
        Self {
            surface: Surface::new(320, 300, RenderLimits::default()).unwrap(),
            actions: 0,
            sizes: Vec::new(),
        }
    }
}
impl WindowHandler for Handler {
    fn resize(&mut self, w: u32, h: u32) -> WindowResult<bool> {
        if (w, h) == (self.surface.width(), self.surface.height()) {
            return Ok(false);
        }
        self.surface = Surface::new(w, h, RenderLimits::default())?;
        self.sizes.push((w, h));
        Ok(true)
    }
    fn action(&mut self, action: Action) -> WindowResult<bool> {
        assert_eq!(action, Action::Next);
        self.actions += 1;
        self.surface.draw(&[DrawCommand::FillRect {
            rect: Rect::new(0, 0, 20, 20),
            color: Color::WHITE,
        }])?;
        Ok(true)
    }
    fn surface(&self) -> &Surface {
        &self.surface
    }
    fn title(&self) -> String {
        "ReadAll protocol test".into()
    }
}
#[test]
fn real_client_library_completes_initial_configure_and_buffer_commit() {
    let temp = Temp::new();
    let socket = temp.0.join("display");
    let listener = UnixListener::bind(&socket).unwrap();
    let thread = thread::spawn(move || serve(listener, false));
    let mut handler = Handler::new();
    let report = run(
        &mut handler,
        WindowOptions {
            display: Some(socket),
            close_after_frames: Some(1),
        },
    )
    .unwrap();
    let observed = thread.join().unwrap();
    assert_eq!(report.committed_frames, 1);
    assert_eq!(observed.sizes, [(320, 300)]);
    assert!(observed.pong);
}
struct AnimatedHandler {
    inner: Handler,
    ticks: usize,
}
impl WindowHandler for AnimatedHandler {
    fn resize(&mut self, w: u32, h: u32) -> WindowResult<bool> {
        self.inner.resize(w, h)
    }
    fn action(&mut self, action: Action) -> WindowResult<bool> {
        self.inner.action(action)
    }
    fn surface(&self) -> &Surface {
        self.inner.surface()
    }
    fn title(&self) -> String {
        "ReadAll animation test".into()
    }
    fn animation_interval(&self) -> Option<Duration> {
        Some(Duration::from_millis(1))
    }
    fn animation_tick(&mut self) -> WindowResult<bool> {
        self.ticks += 1;
        self.inner.surface.draw(&[DrawCommand::FillRect {
            rect: Rect::new((self.ticks % 10) as i32, 0, 1, 1),
            color: Color::WHITE,
        }])?;
        Ok(true)
    }
}

#[test]
fn animation_ticks_produce_frames_without_input_events() {
    let temp = Temp::new();
    let socket = temp.0.join("display");
    let listener = UnixListener::bind(&socket).unwrap();
    let thread = thread::spawn(move || serve(listener, false));
    let mut handler = AnimatedHandler {
        inner: Handler::new(),
        ticks: 0,
    };
    let report = run(
        &mut handler,
        WindowOptions {
            display: Some(socket),
            close_after_frames: Some(2),
        },
    )
    .unwrap();
    let observed = thread.join().unwrap();
    assert_eq!(report.committed_frames, 2);
    assert!(handler.ticks >= 1);
    assert_eq!(observed.frames, 2);
}

#[test]
fn keyboard_resize_ack_and_buffer_release_work_on_the_wire() {
    let temp = Temp::new();
    let socket = temp.0.join("display");
    let listener = UnixListener::bind(&socket).unwrap();
    let thread = thread::spawn(move || serve(listener, true));
    let mut handler = Handler::new();
    let report = run(
        &mut handler,
        WindowOptions {
            display: Some(socket),
            close_after_frames: Some(3),
        },
    )
    .unwrap();
    let observed = thread.join().unwrap();
    assert_eq!(report.committed_frames, 3);
    assert_eq!(handler.actions, 1);
    assert_eq!(observed.sizes, [(320, 300), (320, 300), (640, 480)]);
    assert!(observed.releases >= 1);
    assert_eq!(handler.sizes, [(640, 480)]);
}
#[test]
fn absent_display_is_an_error_not_a_headless_success() {
    let temp = Temp::new();
    let mut handler = Handler::new();
    assert!(
        run(
            &mut handler,
            WindowOptions {
                display: Some(temp.0.join("missing")),
                close_after_frames: Some(1)
            }
        )
        .is_err()
    );
}
#[test]
fn pointer_click_preserves_surface_coordinates() {
    let mut state = State::new(320, 300);
    state.pointer_focus = true;
    state.pointer_x = 123 * 256;
    state.pointer_y = 77 * 256;
    // SAFETY: event argument shape matches wl_pointer.button v5.
    unsafe {
        dispatch(
            &mut state,
            POINTER,
            null_mut(),
            3,
            [Arg { u: 1 }, Arg { u: 0 }, Arg { u: 272 }, Arg { u: 1 }].as_mut_ptr(),
        );
    }
    assert_eq!(state.action_count, 1);
    assert_eq!(state.actions[0], Some(Action::Click { x: 123, y: 77 }));
}

#[test]
fn pointer_motion_is_coalesced_and_leave_is_delivered() {
    let mut state = State::new(320, 300);
    state.pointer_focus = true;
    // SAFETY: event argument shapes match wl_pointer.motion/leave v5.
    unsafe {
        dispatch(
            &mut state,
            POINTER,
            null_mut(),
            2,
            [Arg { u: 1 }, Arg { i: 12 * 256 }, Arg { i: 20 * 256 }].as_mut_ptr(),
        );
        dispatch(
            &mut state,
            POINTER,
            null_mut(),
            2,
            [Arg { u: 2 }, Arg { i: 18 * 256 }, Arg { i: 25 * 256 }].as_mut_ptr(),
        );
    }
    assert_eq!(state.action_count, 1);
    assert_eq!(state.actions[0], Some(Action::PointerMove { x: 18, y: 25 }));
    // SAFETY: leave branch does not dereference the event arguments.
    unsafe {
        dispatch(&mut state, POINTER, null_mut(), 1, null_mut());
    }
    assert_eq!(state.action_count, 2);
    assert_eq!(state.actions[1], Some(Action::PointerLeave));
    assert!(!state.pointer_focus);
}

#[test]
fn discrete_and_continuous_wheel_events_do_not_turn_twice() {
    let mut state = State::new(320, 300);
    state.pointer_focus = true;
    // SAFETY: event argument shapes match wl_pointer v5; these branches do not dereference target.
    unsafe {
        dispatch(
            &mut state,
            POINTER,
            null_mut(),
            4,
            [Arg { u: 0 }, Arg { u: 0 }, Arg { i: 3840 }].as_mut_ptr(),
        );
        dispatch(
            &mut state,
            POINTER,
            null_mut(),
            8,
            [Arg { u: 0 }, Arg { i: 1 }].as_mut_ptr(),
        );
        dispatch(&mut state, POINTER, null_mut(), 5, null_mut());
    }
    assert_eq!(state.action_count, 1);
    assert_eq!(state.actions[0], Some(Action::Next));
}

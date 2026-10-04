//! Single-threaded Wayland adapter. FFI is confined to this module and protocol.rs.
//! Callback state is a Box converted to a raw pointer, stays at one address, and
//! is freed only AFTER all proxies and the connection. No state reference is
//! held while dispatching. Callbacks only collect bounded events or marshal
//! protocol replies; they never invoke application code or unwind into C.
mod keyboard;
mod protocol;
use super::window::{Action, WindowHandler, WindowOptions, WindowReport, WindowResult, write_xrgb};
use protocol::*;
use std::{
    ffi::{CStr, CString, c_void},
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::{null, null_mut},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
const REGISTRY: usize = 1;
const WM: usize = 2;
const XDG: usize = 3;
const TOPLEVEL: usize = 4;
const SEAT: usize = 5;
const KEYBOARD: usize = 6;
const POINTER: usize = 7;
const BUFFER: usize = 8;
const SYNC: usize = 9;
const SHM: usize = 10;
const SURFACE: usize = 11;
#[derive(Clone, Copy, Default)]
struct Global {
    name: u32,
    version: u32,
}
struct State {
    globals: [Global; 4],
    registry: *mut Proxy,
    compositor: *mut Proxy,
    shm: *mut Proxy,
    wm: *mut Proxy,
    seat: *mut Proxy,
    keyboard: *mut Proxy,
    pointer: *mut Proxy,
    surface: *mut Proxy,
    xdg: *mut Proxy,
    top: *mut Proxy,
    sync: *mut Proxy,
    buffers: [*mut Proxy; 2],
    width: u32,
    height: u32,
    pending_width: i32,
    pending_height: i32,
    configured: bool,
    dirty: bool,
    closed: bool,
    synced: bool,
    xrgb: bool,
    capabilities: u32,
    keyboard_focus: bool,
    key_state: keyboard::Keyboard,
    text_input: bool,
    pointer_focus: bool,
    pointer_x: i32,
    pointer_y: i32,
    scroll: [i32; 2],
    discrete: [i32; 2],
    precise_scroll: bool,
    motion: super::window::MotionCoalescer,
    actions: [Option<Action>; 64],
    action_count: usize,
    fault: Option<&'static str>,
}
impl State {
    fn new(width: u32, height: u32) -> Self {
        Self {
            globals: [Global::default(); 4],
            registry: null_mut(),
            compositor: null_mut(),
            shm: null_mut(),
            wm: null_mut(),
            seat: null_mut(),
            keyboard: null_mut(),
            pointer: null_mut(),
            surface: null_mut(),
            xdg: null_mut(),
            top: null_mut(),
            sync: null_mut(),
            buffers: [null_mut(); 2],
            width,
            height,
            pending_width: 0,
            pending_height: 0,
            configured: false,
            dirty: false,
            closed: false,
            synced: false,
            xrgb: false,
            capabilities: 0,
            keyboard_focus: false,
            key_state: keyboard::Keyboard::default(),
            text_input: false,
            pointer_focus: false,
            pointer_x: 0,
            pointer_y: 0,
            scroll: [0; 2],
            discrete: [0; 2],
            precise_scroll: false,
            motion: super::window::MotionCoalescer::default(),
            actions: [None; 64],
            action_count: 0,
            fault: None,
        }
    }
    fn action(&mut self, action: Action) {
        if self.action_count > 0
            && self
                .motion
                .may_replace(self.actions[self.action_count - 1], action)
        {
            self.actions[self.action_count - 1] = Some(action);
            self.motion.accepted(action);
            return;
        }
        if self.action_count == self.actions.len() {
            self.fault = Some("native input queue budget exceeded");
            return;
        }
        self.actions[self.action_count] = Some(action);
        self.action_count += 1;
        self.motion.accepted(action);
    }
}
// SAFETY: callers supply a live proxy and exactly the argument signature for its opcode.
unsafe fn send(proxy: *mut Proxy, opcode: u32, args: &mut [Arg]) {
    unsafe {
        wl_proxy_marshal_array_flags(
            proxy,
            opcode,
            null(),
            wl_proxy_get_version(proxy),
            0,
            args.as_mut_ptr(),
        );
    }
}
unsafe fn destroy(proxy: *mut Proxy, opcode: u32) {
    unsafe {
        wl_proxy_marshal_array_flags(
            proxy,
            opcode,
            null(),
            wl_proxy_get_version(proxy),
            1,
            null_mut(),
        );
    }
}
unsafe fn construct(
    proxy: *mut Proxy,
    opcode: u32,
    interface: *const Interface,
    version: u32,
    args: &mut [Arg],
) -> WindowResult<*mut Proxy> {
    let result = unsafe {
        wl_proxy_marshal_array_flags(proxy, opcode, interface, version, 0, args.as_mut_ptr())
    };
    if result.is_null() {
        return Err(io::Error::last_os_error().into());
    }
    Ok(result)
}
unsafe fn listen(proxy: *mut Proxy, kind: usize, state: *mut State) -> WindowResult<()> {
    if unsafe { wl_proxy_add_dispatcher(proxy, dispatcher, kind as *const c_void, state.cast()) }
        != 0
    {
        return Err("cannot register Wayland dispatcher".into());
    }
    Ok(())
}
unsafe extern "C" fn dispatcher(
    kind: *const c_void,
    target: *mut c_void,
    opcode: u32,
    _: *const Message,
    args: *mut Arg,
) -> i32 {
    // SAFETY: libwayland validates signatures and provides the argument array for
    // this proxy. The boxed state is valid for the lifetime of every dispatcher.
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        let state = &mut *wl_proxy_get_user_data(target).cast::<State>();
        dispatch(state, kind as usize, target, opcode, args);
    }));
    if result.is_err() { -1 } else { 0 }
}
unsafe fn dispatch(s: &mut State, kind: usize, target: *mut Proxy, opcode: u32, args: *mut Arg) {
    // SAFETY: private, fixed interface versions determine every index and union field below.
    unsafe {
        match (kind, opcode) {
            (REGISTRY, 0) => {
                let name = CStr::from_ptr((*args.add(1)).s).to_bytes();
                let slot = match name {
                    b"wl_compositor" => Some(0),
                    b"wl_shm" => Some(1),
                    b"xdg_wm_base" => Some(2),
                    b"wl_seat" => Some(3),
                    _ => None,
                };
                if let Some(i) = slot
                    && s.globals[i].name == 0
                {
                    s.globals[i] = Global {
                        name: (*args).u,
                        version: (*args.add(2)).u,
                    };
                }
            }
            (REGISTRY, 1) => {
                for (i, global) in s.globals.iter_mut().enumerate() {
                    if global.name == (*args).u {
                        *global = Global::default();
                        if i == 3 {
                            s.capabilities = 0;
                        } else {
                            s.fault = Some("required Wayland global was removed");
                        }
                    }
                }
            }
            (WM, 0) => send(target, 3, &mut [Arg { u: (*args).u }]),
            (TOPLEVEL, 0) => {
                s.pending_width = (*args).i;
                s.pending_height = (*args.add(1)).i;
            }
            (TOPLEVEL, 1) => s.closed = true,
            (XDG, 0) => {
                send(target, 4, &mut [Arg { u: (*args).u }]);
                if s.pending_width < 0
                    || s.pending_height < 0
                    || s.pending_width > 4096
                    || s.pending_height > 4096
                {
                    s.fault = Some("configured window exceeds supported dimensions");
                    return;
                }
                if s.pending_width != 0 {
                    s.width = s.pending_width as u32;
                }
                if s.pending_height != 0 {
                    s.height = s.pending_height as u32;
                }
                s.pending_width = 0;
                s.pending_height = 0;
                s.configured = true;
                s.dirty = true;
            }
            (SEAT, 0) => s.capabilities = (*args).u,
            (KEYBOARD, 0) => {
                let fd = (*args.add(1)).h;
                if fd >= 0 {
                    let fd = OwnedFd::from_raw_fd(fd);
                    if (*args).u == 1
                        && s.key_state
                            .keymap(fs::File::from(fd), (*args.add(2)).u)
                            .is_err()
                    {
                        eprintln!(
                            "ReadAll: compositor keymap unavailable; physical navigation remains enabled"
                        );
                    }
                }
            }
            (KEYBOARD, 1) => s.keyboard_focus = (*args.add(1)).o == s.surface,
            (KEYBOARD, 2) => s.keyboard_focus = false,
            (KEYBOARD, 3) if s.keyboard_focus && (*args.add(3)).u == 1 => {
                if let Some(action) = s.key_state.action((*args.add(2)).u, s.text_input) {
                    s.action(action);
                }
            }
            (KEYBOARD, 4) => s.key_state.modifiers(
                (*args.add(1)).u,
                (*args.add(2)).u,
                (*args.add(3)).u,
                (*args.add(4)).u,
            ),
            (POINTER, 0) => {
                s.pointer_focus = (*args.add(1)).o == s.surface;
                s.pointer_x = (*args.add(2)).i;
                s.pointer_y = (*args.add(3)).i;
                if s.pointer_focus {
                    s.action(Action::PointerMove {
                        x: s.pointer_x / 256,
                        y: s.pointer_y / 256,
                    });
                }
            }
            (POINTER, 1) => {
                if s.pointer_focus {
                    s.action(Action::PointerLeave);
                }
                s.pointer_focus = false;
                s.scroll = [0; 2];
                s.discrete = [0; 2];
            }
            (POINTER, 2) => {
                s.pointer_x = (*args.add(1)).i;
                s.pointer_y = (*args.add(2)).i;
                if s.pointer_focus {
                    s.action(Action::PointerMove {
                        x: s.pointer_x / 256,
                        y: s.pointer_y / 256,
                    });
                }
            }
            (POINTER, 3) if s.pointer_focus && (*args.add(2)).u == 272 && (*args.add(3)).u == 1 => {
                s.action(Action::Click {
                    x: s.pointer_x / 256,
                    y: s.pointer_y / 256,
                });
            }
            (POINTER, 3) if s.pointer_focus && (*args.add(2)).u == 272 && (*args.add(3)).u == 0 => {
                s.action(Action::PointerRelease {
                    x: s.pointer_x / 256,
                    y: s.pointer_y / 256,
                });
            }
            (POINTER, 3) if s.pointer_focus && s.precise_scroll && (*args.add(2)).u == 273 => {
                let (x, y) = (s.pointer_x / 256, s.pointer_y / 256);
                s.action(if (*args.add(3)).u == 1 {
                    Action::PanStart { x, y }
                } else {
                    Action::PanEnd { x, y }
                });
            }
            (POINTER, 4) if s.pointer_focus && (*args.add(1)).u < 2 => {
                let axis = (*args.add(1)).u as usize;
                s.scroll[axis] = s.scroll[axis]
                    .saturating_add((*args.add(2)).i)
                    .clamp(-262144, 262144);
            }
            (POINTER, 8) if s.pointer_focus && (*args).u < 2 => {
                let axis = (*args).u as usize;
                s.discrete[axis] = s.discrete[axis]
                    .saturating_add((*args.add(1)).i)
                    .clamp(-16, 16);
            }
            (POINTER, 5) => {
                if s.precise_scroll {
                    let delta = std::array::from_fn::<_, 2, _>(|axis| {
                        if s.discrete[axis] != 0 {
                            s.discrete[axis] * 64 * 256
                        } else {
                            s.scroll[axis]
                        }
                    });
                    if delta != [0, 0] {
                        s.action(Action::Scroll {
                            dx: delta[1],
                            dy: delta[0],
                        });
                    }
                    s.scroll = [0; 2];
                } else {
                    let direction = if s.discrete[0] != 0 {
                        s.discrete[0].signum()
                    } else if s.scroll[0].abs() >= 2560 {
                        s.scroll[0].signum()
                    } else {
                        0
                    };
                    if direction != 0 {
                        s.action(if direction > 0 {
                            Action::Next
                        } else {
                            Action::Previous
                        });
                        s.scroll = [0; 2];
                    }
                }
                s.discrete = [0; 2];
            }
            (BUFFER, 0) => {
                if let Some(slot) = s.buffers.iter_mut().find(|slot| **slot == target) {
                    *slot = null_mut();
                    destroy(target, 0);
                } else {
                    s.fault = Some("unknown buffer release");
                }
            }
            (SYNC, 0) => {
                s.synced = true;
                s.sync = null_mut();
                wl_proxy_destroy(target);
            }
            (SHM, 0) => s.xrgb |= (*args).u == 1,
            _ => {}
        }
    }
}
struct Connection {
    display: *mut Proxy,
    state: *mut State,
}
impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: no dispatch occurs during teardown. All still-live proxies are
        // locally destroyed once; disconnect releases server objects and SHM fds.
        unsafe {
            let s = &*self.state;
            for p in [
                s.buffers[0],
                s.buffers[1],
                s.sync,
                s.keyboard,
                s.pointer,
                s.top,
                s.xdg,
                s.surface,
                s.seat,
                s.shm,
                s.compositor,
                s.wm,
                s.registry,
            ] {
                if !p.is_null() {
                    wl_proxy_destroy(p);
                }
            }
            wl_display_disconnect(self.display);
            drop(Box::from_raw(self.state));
        }
    }
}
impl Connection {
    fn connect(options: &WindowOptions, width: u32, height: u32) -> WindowResult<Self> {
        let name = options
            .display
            .as_ref()
            .map(|p| CString::new(p.as_os_str().as_bytes()))
            .transpose()?;
        // SAFETY: optional name stays alive for the call; display ownership moves into Connection.
        let display = unsafe { wl_display_connect(name.as_ref().map_or(null(), |s| s.as_ptr())) };
        if display.is_null() {
            return Err(format!("cannot connect to Wayland: {}; run from your desktop session with WAYLAND_DISPLAY and XDG_RUNTIME_DIR (no display socket is guessed)", io::Error::last_os_error()).into());
        }
        let connection = Self {
            display,
            state: Box::into_raw(Box::new(State::new(width, height))),
        };
        unsafe {
            let registry = construct(
                display,
                1,
                &raw const wl_registry_interface,
                1,
                &mut [Arg { o: null_mut() }],
            )?;
            (*connection.state).registry = registry;
            listen(registry, REGISTRY, connection.state)?;
        }
        Ok(connection)
    }
    fn sync(&mut self) -> WindowResult<()> {
        unsafe {
            (*self.state).synced = false;
            let callback = construct(
                self.display,
                0,
                &raw const wl_callback_interface,
                1,
                &mut [Arg { o: null_mut() }],
            )?;
            (*self.state).sync = callback;
            listen(callback, SYNC, self.state)?;
        }
        Ok(())
    }
    fn wait_sync(&mut self) -> WindowResult<()> {
        self.sync()?;
        let start = Instant::now();
        while !unsafe { (*self.state).synced } {
            if start.elapsed() > Duration::from_secs(10) {
                return Err("Wayland initialization timed out".into());
            }
            self.pump(1000)?;
        }
        Ok(())
    }
    fn bind(
        &mut self,
        index: usize,
        interface: *const Interface,
        version: u32,
        kind: usize,
    ) -> WindowResult<*mut Proxy> {
        // SAFETY: interface is one of the static descriptors; registry was created on this connection.
        unsafe {
            let global = (*self.state).globals[index];
            if global.name == 0 || global.version < version {
                return Err("required Wayland protocol/version not available".into());
            }
            let p = construct(
                (*self.state).registry,
                0,
                interface,
                version,
                &mut [
                    Arg { u: global.name },
                    Arg {
                        s: (*interface).name,
                    },
                    Arg { u: version },
                    Arg { o: null_mut() },
                ],
            )?;
            if kind != 0
                && let Err(e) = listen(p, kind, self.state)
            {
                wl_proxy_destroy(p);
                return Err(e);
            }
            Ok(p)
        }
    }
    fn initialize(&mut self, minimum_size: (u32, u32)) -> WindowResult<()> {
        self.wait_sync()?;
        unsafe {
            (*self.state).compositor = self.bind(0, &raw const wl_compositor_interface, 4, 0)?;
            (*self.state).shm = self.bind(1, &raw const wl_shm_interface, 1, SHM)?;
            (*self.state).wm = self.bind(2, &XDG_WM_BASE, 1, WM)?;
            if (*self.state).globals[3].name != 0 && (*self.state).globals[3].version >= 5 {
                (*self.state).seat = self.bind(3, &raw const wl_seat_interface, 5, SEAT)?;
            }
            let surface = construct(
                (*self.state).compositor,
                0,
                &raw const wl_surface_interface,
                4,
                &mut [Arg { o: null_mut() }],
            )?;
            (*self.state).surface = surface;
            listen(surface, SURFACE, self.state)?;
            let xdg = construct(
                (*self.state).wm,
                2,
                &XDG_SURFACE,
                1,
                &mut [Arg { o: null_mut() }, Arg { o: surface }],
            )?;
            (*self.state).xdg = xdg;
            listen(xdg, XDG, self.state)?;
            let top = construct(xdg, 1, &XDG_TOPLEVEL, 1, &mut [Arg { o: null_mut() }])?;
            (*self.state).top = top;
            listen(top, TOPLEVEL, self.state)?;
            send(
                top,
                3,
                &mut [Arg {
                    s: c"xin.soymilk.ReadAll".as_ptr(),
                }],
            );
            let min_width = i32::try_from(minimum_size.0)
                .map_err(|_| "minimum window width exceeds Wayland limits")?;
            let min_height = i32::try_from(minimum_size.1)
                .map_err(|_| "minimum window height exceeds Wayland limits")?;
            send(top, 8, &mut [Arg { i: min_width }, Arg { i: min_height }]);
            send(top, 7, &mut [Arg { i: 4096 }, Arg { i: 4096 }]);
            send(surface, 6, &mut []); // Initial empty commit; wait for xdg_surface.configure.
        }
        Ok(())
    }
    fn devices(&mut self) -> WindowResult<()> {
        unsafe {
            let s = &mut *self.state;
            for (bit, object, opcode, interface, kind, release) in [
                (
                    2,
                    &mut s.keyboard,
                    1,
                    &raw const wl_keyboard_interface,
                    KEYBOARD,
                    0,
                ),
                (
                    1,
                    &mut s.pointer,
                    0,
                    &raw const wl_pointer_interface,
                    POINTER,
                    1,
                ),
            ] {
                if s.capabilities & bit == 0 && !object.is_null() {
                    destroy(*object, release);
                    *object = null_mut();
                } else if s.capabilities & bit != 0 && object.is_null() && !s.seat.is_null() {
                    *object =
                        construct(s.seat, opcode, interface, 5, &mut [Arg { o: null_mut() }])?;
                    listen(*object, kind, self.state)?;
                }
            }
        }
        Ok(())
    }
    fn pump(&mut self, timeout_ms: i32) -> WindowResult<()> {
        // SAFETY: prepare/read/cancel are balanced on every path. Poll does not
        // dispatch callbacks. No Rust state borrow exists across dispatch_pending.
        unsafe {
            while wl_display_prepare_read(self.display) != 0 {
                if wl_display_dispatch_pending(self.display) < 0 {
                    return Err("Wayland dispatch failed".into());
                }
            }
            let mut flags = 1_i16;
            if wl_display_flush(self.display) < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::WouldBlock {
                    wl_display_cancel_read(self.display);
                    return Err(error.into());
                }
                flags |= 4;
            }
            let mut fd = PollFd {
                fd: wl_display_get_fd(self.display),
                events: flags,
                revents: 0,
            };
            let result = poll(&mut fd, 1, timeout_ms.clamp(0, 1000));
            if result < 0 {
                let error = io::Error::last_os_error();
                wl_display_cancel_read(self.display);
                if error.kind() == io::ErrorKind::Interrupted {
                    return Ok(());
                }
                return Err(error.into());
            }
            if fd.revents & (8 | 16 | 32) != 0 {
                wl_display_cancel_read(self.display);
                return Err("Wayland display disconnected".into());
            }
            if fd.revents & 1 != 0 {
                if wl_display_read_events(self.display) < 0 {
                    return Err("cannot read Wayland events".into());
                }
            } else {
                wl_display_cancel_read(self.display);
            }
            if wl_display_dispatch_pending(self.display) < 0 {
                return Err("Wayland protocol dispatch failed".into());
            }
            if let Some(error) = (*self.state).fault {
                return Err(error.into());
            }
        }
        Ok(())
    }
    fn present(&mut self, handler: &impl WindowHandler) -> WindowResult<bool> {
        let slot = unsafe { (*self.state).buffers.iter().position(|p| p.is_null()) };
        let Some(slot) = slot else {
            return Ok(false);
        };
        let surface = handler.surface();
        let size = u64::from(surface.width()) * u64::from(surface.height()) * 4;
        if size > 64 * 1024 * 1024 {
            return Err("native buffer budget exceeded".into());
        }
        let title = CString::new(handler.title())?;
        let mut file = anonymous_file()?;
        file.set_len(size)?;
        {
            let mut writer = BufWriter::new(&mut file);
            write_xrgb(surface, &mut writer)?;
            writer.flush()?;
        }
        unsafe {
            let s = &mut *self.state;
            if (surface.width(), surface.height()) != (s.width, s.height) {
                return Err("reader frame does not match configured window size".into());
            }
            let pool = construct(
                s.shm,
                0,
                &raw const wl_shm_pool_interface,
                1,
                &mut [
                    Arg { o: null_mut() },
                    Arg {
                        h: file.as_raw_fd(),
                    },
                    Arg { i: size as i32 },
                ],
            )?;
            let buffer = construct(
                pool,
                0,
                &raw const wl_buffer_interface,
                1,
                &mut [
                    Arg { o: null_mut() },
                    Arg { i: 0 },
                    Arg {
                        i: surface.width() as i32,
                    },
                    Arg {
                        i: surface.height() as i32,
                    },
                    Arg {
                        i: surface.width() as i32 * 4,
                    },
                    Arg { u: 1 },
                ],
            );
            destroy(pool, 1);
            let buffer = buffer?;
            s.buffers[slot] = buffer;
            listen(buffer, BUFFER, self.state)?;
            send(s.top, 2, &mut [Arg { s: title.as_ptr() }]);
            send(
                s.xdg,
                3,
                &mut [
                    Arg { i: 0 },
                    Arg { i: 0 },
                    Arg {
                        i: surface.width() as i32,
                    },
                    Arg {
                        i: surface.height() as i32,
                    },
                ],
            );
            send(
                s.surface,
                1,
                &mut [Arg { o: buffer }, Arg { i: 0 }, Arg { i: 0 }],
            );
            send(
                s.surface,
                9,
                &mut [
                    Arg { i: 0 },
                    Arg { i: 0 },
                    Arg {
                        i: surface.width() as i32,
                    },
                    Arg {
                        i: surface.height() as i32,
                    },
                ],
            );
            send(s.surface, 6, &mut []);
            s.dirty = false;
        }
        // libwayland duplicates the fd during request marshalling. The anonymous
        // inode is retained by the compositor until the buffer is released.
        Ok(true)
    }
}
fn anonymous_file() -> io::Result<fs::File> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..32 {
        let name = std::env::temp_dir().join(format!(
            "readall-shm-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&name)
        {
            Ok(file) => {
                fs::remove_file(&name)?;
                return Ok(file);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other(
        "cannot reserve a unique shared buffer file",
    ))
}
pub(super) fn run(
    handler: &mut impl WindowHandler,
    options: WindowOptions,
) -> WindowResult<WindowReport> {
    let mut connection = Connection::connect(
        &options,
        handler.surface().width(),
        handler.surface().height(),
    )?;
    connection.initialize(handler.minimum_size())?;
    let start = Instant::now();
    let mut report = WindowReport::default();
    let mut finishing = false;
    let mut last_animation = Instant::now();
    loop {
        // SAFETY: state is only borrowed between (not during) dispatch calls.
        let (closed, configured, dirty, width, height, synced, xrgb) = unsafe {
            let s = &*connection.state;
            (
                s.closed,
                s.configured,
                s.dirty,
                s.width,
                s.height,
                s.synced,
                s.xrgb,
            )
        };
        if closed || finishing && synced {
            return Ok(report);
        }
        if report.committed_frames == 0 && start.elapsed() > Duration::from_secs(10) {
            return Err("compositor did not configure the window within 10 seconds".into());
        }
        connection.devices()?;
        if configured && !finishing {
            if !xrgb {
                return Err("compositor did not advertise XRGB8888 shared buffers".into());
            }
            let resized = if dirty {
                handler.resize(width, height)?
            } else {
                false
            };
            let (actions, count) = unsafe {
                let s = &mut *connection.state;
                let values = s.actions;
                let count = s.action_count;
                s.action_count = 0;
                s.actions = [None; 64];
                (values, count)
            };
            let mut changed = resized;
            for action in actions.into_iter().take(count).flatten() {
                let handled = handler.action(action)?;
                if action == Action::Close && !handled {
                    return Ok(report);
                }
                changed |= handled;
                if handler.close_requested() {
                    return Ok(report);
                }
            }
            if let Some(interval) = handler.animation_interval()
                && last_animation.elapsed() >= interval
            {
                // Schedule start-to-start; frame preparation and SHM presentation
                // count towards the interval rather than being added after it.
                last_animation = Instant::now();
                changed |= handler.animation_tick()?;
            }
            // Worker completions and cancellation can arrive without input events.
            if handler.close_requested() {
                return Ok(report);
            }
            unsafe {
                (*connection.state).dirty |= changed;
                (*connection.state).text_input = handler.text_input_active();
                (*connection.state).precise_scroll = handler.precise_scroll();
            }
            if (dirty || changed) && connection.present(handler)? {
                handler.frame_presented();
                report.committed_frames = report.committed_frames.saturating_add(1);
                report.width = width;
                report.height = height;
                if options
                    .close_after_frames
                    .is_some_and(|limit| report.committed_frames >= limit)
                {
                    finishing = true;
                    connection.sync()?;
                }
            }
        }
        let timeout_ms = if configured && !finishing {
            handler.animation_interval().map_or(1000, |interval| {
                let remaining = interval.saturating_sub(last_animation.elapsed());
                remaining.as_millis().clamp(1, 1000) as i32
            })
        } else {
            1000
        };
        connection.pump(timeout_ms)?;
    }
}
#[cfg(test)]
mod tests;

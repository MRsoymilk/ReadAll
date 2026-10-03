//! Minimal Rust bindings to libwayland-client and stable xdg-shell v1.
//! Sources: wayland.freedesktop.org/docs/html/apb.html and stable/xdg-shell/xdg-shell.xml.
//! Only the private adapter calls these declarations; no raw pointer crosses its safe API.
use std::ffi::{c_char, c_int, c_void};
pub type Proxy = c_void;
#[repr(C)]
pub struct Message {
    pub name: *const c_char,
    pub signature: *const c_char,
    pub types: *const *const Interface,
}
#[repr(C)]
pub struct Interface {
    pub name: *const c_char,
    pub version: c_int,
    pub method_count: c_int,
    pub methods: *const Message,
    pub event_count: c_int,
    pub events: *const Message,
}
// SAFETY: every pointer in these immutable descriptors points at another static descriptor/string.
unsafe impl Sync for Message {}
unsafe impl Sync for Interface {}
struct Types<const N: usize>([*const Interface; N]);
// SAFETY: these arrays contain only immutable, process-lifetime interface pointers.
unsafe impl<const N: usize> Sync for Types<N> {}
#[repr(C)]
#[derive(Clone, Copy)]
pub union Arg {
    pub i: i32,
    pub u: u32,
    pub s: *const c_char,
    pub o: *mut Proxy,
    pub h: i32,
    pub a: *mut c_void,
}
pub type Dispatcher =
    unsafe extern "C" fn(*const c_void, *mut c_void, u32, *const Message, *mut Arg) -> c_int;
#[repr(C)]
pub struct PollFd {
    pub fd: c_int,
    pub events: i16,
    pub revents: i16,
}
#[link(name = "wayland-client")]
unsafe extern "C" {
    pub fn wl_display_connect(name: *const c_char) -> *mut Proxy;
    pub fn wl_display_disconnect(display: *mut Proxy);
    pub fn wl_display_get_fd(display: *mut Proxy) -> c_int;
    pub fn wl_display_dispatch_pending(display: *mut Proxy) -> c_int;
    pub fn wl_display_prepare_read(display: *mut Proxy) -> c_int;
    pub fn wl_display_cancel_read(display: *mut Proxy);
    pub fn wl_display_read_events(display: *mut Proxy) -> c_int;
    pub fn wl_display_flush(display: *mut Proxy) -> c_int;
    pub fn wl_proxy_marshal_array_flags(
        proxy: *mut Proxy,
        opcode: u32,
        interface: *const Interface,
        version: u32,
        flags: u32,
        args: *mut Arg,
    ) -> *mut Proxy;
    pub fn wl_proxy_destroy(proxy: *mut Proxy);
    pub fn wl_proxy_get_version(proxy: *mut Proxy) -> u32;
    pub fn wl_proxy_get_user_data(proxy: *mut Proxy) -> *mut c_void;
    pub fn wl_proxy_add_dispatcher(
        proxy: *mut Proxy,
        dispatcher: Dispatcher,
        implementation: *const c_void,
        data: *mut c_void,
    ) -> c_int;
    pub static wl_registry_interface: Interface;
    pub static wl_compositor_interface: Interface;
    pub static wl_shm_interface: Interface;
    pub static wl_shm_pool_interface: Interface;
    pub static wl_surface_interface: Interface;
    pub static wl_buffer_interface: Interface;
    pub static wl_seat_interface: Interface;
    pub static wl_keyboard_interface: Interface;
    pub static wl_pointer_interface: Interface;
    pub static wl_output_interface: Interface;
    pub static wl_callback_interface: Interface;
}
unsafe extern "C" {
    pub fn poll(fds: *mut PollFd, nfds: usize, timeout: c_int) -> c_int;
}
static EMPTY: Types<6> = Types([std::ptr::null(); 6]);
macro_rules! message {
    ($name:literal, $sig:literal) => {
        Message {
            name: $name.as_ptr(),
            signature: $sig.as_ptr(),
            types: EMPTY.0.as_ptr(),
        }
    };
    ($name:literal, $sig:literal, $types:ident) => {
        Message {
            name: $name.as_ptr(),
            signature: $sig.as_ptr(),
            types: $types.0.as_ptr(),
        }
    };
}
// Positioners/popups are never instantiated. Their descriptors only identify constructor argument types.
static POSITIONER: Interface = Interface {
    name: c"xdg_positioner".as_ptr(),
    version: 1,
    method_count: 0,
    methods: std::ptr::null(),
    event_count: 0,
    events: std::ptr::null(),
};
static POPUP: Interface = Interface {
    name: c"xdg_popup".as_ptr(),
    version: 1,
    method_count: 0,
    methods: std::ptr::null(),
    event_count: 0,
    events: std::ptr::null(),
};
static POSITIONER_NEW: Types<1> = Types([&POSITIONER]);
static SURFACE_NEW: Types<2> = Types([&XDG_SURFACE, &raw const wl_surface_interface]);
static TOPLEVEL_NEW: Types<1> = Types([&XDG_TOPLEVEL]);
static POPUP_NEW: Types<3> = Types([&POPUP, &XDG_SURFACE, &POSITIONER]);
static PARENT: Types<1> = Types([&XDG_TOPLEVEL]);
static SEAT: Types<4> = Types([
    &raw const wl_seat_interface,
    std::ptr::null(),
    std::ptr::null(),
    std::ptr::null(),
]);
static OUTPUT: Types<1> = Types([&raw const wl_output_interface]);
static WM_METHODS: [Message; 4] = [
    message!(c"destroy", c""),
    message!(c"create_positioner", c"n", POSITIONER_NEW),
    message!(c"get_xdg_surface", c"no", SURFACE_NEW),
    message!(c"pong", c"u"),
];
static WM_EVENTS: [Message; 1] = [message!(c"ping", c"u")];
pub static XDG_WM_BASE: Interface = Interface {
    name: c"xdg_wm_base".as_ptr(),
    version: 1,
    method_count: 4,
    methods: WM_METHODS.as_ptr(),
    event_count: 1,
    events: WM_EVENTS.as_ptr(),
};
static SURFACE_METHODS: [Message; 5] = [
    message!(c"destroy", c""),
    message!(c"get_toplevel", c"n", TOPLEVEL_NEW),
    message!(c"get_popup", c"n?oo", POPUP_NEW),
    message!(c"set_window_geometry", c"iiii"),
    message!(c"ack_configure", c"u"),
];
static SURFACE_EVENTS: [Message; 1] = [message!(c"configure", c"u")];
pub static XDG_SURFACE: Interface = Interface {
    name: c"xdg_surface".as_ptr(),
    version: 1,
    method_count: 5,
    methods: SURFACE_METHODS.as_ptr(),
    event_count: 1,
    events: SURFACE_EVENTS.as_ptr(),
};
static TOPLEVEL_METHODS: [Message; 14] = [
    message!(c"destroy", c""),
    message!(c"set_parent", c"?o", PARENT),
    message!(c"set_title", c"s"),
    message!(c"set_app_id", c"s"),
    message!(c"show_window_menu", c"ouii", SEAT),
    message!(c"move", c"ou", SEAT),
    message!(c"resize", c"ouu", SEAT),
    message!(c"set_max_size", c"ii"),
    message!(c"set_min_size", c"ii"),
    message!(c"set_maximized", c""),
    message!(c"unset_maximized", c""),
    message!(c"set_fullscreen", c"?o", OUTPUT),
    message!(c"unset_fullscreen", c""),
    message!(c"set_minimized", c""),
];
static TOPLEVEL_EVENTS: [Message; 2] = [message!(c"configure", c"iia"), message!(c"close", c"")];
pub static XDG_TOPLEVEL: Interface = Interface {
    name: c"xdg_toplevel".as_ptr(),
    version: 1,
    method_count: 14,
    methods: TOPLEVEL_METHODS.as_ptr(),
    event_count: 2,
    events: TOPLEVEL_EVENTS.as_ptr(),
};

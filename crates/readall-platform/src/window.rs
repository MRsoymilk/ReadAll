//! Small safe boundary between the reader and native presentation.
use readall_render::Surface;
use std::{error::Error, path::PathBuf, time::Duration};
#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
use {
    readall_render::Color,
    std::io::{self, Write},
};
pub type WindowResult<T> = Result<T, Box<dyn Error>>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Next,
    Previous,
    First,
    Last,
    Larger,
    Smaller,
    Activate,
    Back,
    PointerMove { x: i32, y: i32 },
    PointerLeave,
    Click { x: i32, y: i32 },
    Close,
}
#[derive(Debug, Clone, Default)]
pub struct WindowOptions {
    /// None uses the caller's WAYLAND_DISPLAY / XDG_RUNTIME_DIR, never guesses a user socket.
    pub display: Option<PathBuf>,
    /// Diagnostic: exit after this many buffer commits have been acknowledged by a sync callback.
    pub close_after_frames: Option<u32>,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowReport {
    pub committed_frames: u32,
    pub width: u32,
    pub height: u32,
}
pub trait WindowHandler {
    fn resize(&mut self, width: u32, height: u32) -> WindowResult<bool>;
    fn action(&mut self, action: Action) -> WindowResult<bool>;
    fn surface(&self) -> &Surface;
    fn title(&self) -> String;
    fn animation_interval(&self) -> Option<Duration> {
        None
    }
    fn animation_tick(&mut self) -> WindowResult<bool> {
        Ok(false)
    }
    fn close_requested(&self) -> bool {
        false
    }
}
pub fn run(handler: &mut impl WindowHandler, options: WindowOptions) -> WindowResult<WindowReport> {
    if options.close_after_frames == Some(0) {
        return Err("frame limit must be positive".into());
    }
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    {
        super::wayland::run(handler, options)
    }
    #[cfg(not(all(target_os = "linux", feature = "wayland")))]
    {
        let _ = handler;
        Err("native window backend unavailable: on Linux build with --features wayland; Windows/Android windows are not implemented".into())
    }
}
// XRGB8888 consists of native-endian 0x00RRGGBB words. Write a bounded row at a time.
#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
pub(crate) fn write_xrgb(surface: &Surface, output: &mut impl Write) -> io::Result<()> {
    let mut row = Vec::new();
    row.try_reserve_exact(surface.width() as usize * 4)
        .map_err(io::Error::other)?;
    for pixels in surface.pixels().chunks_exact(surface.width() as usize) {
        row.clear();
        for &pixel in pixels {
            let c = pixel.over(Color::WHITE);
            row.extend_from_slice(
                &((u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)).to_ne_bytes(),
            );
        }
        output.write_all(&row)?;
    }
    Ok(())
}
#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
pub(crate) fn physical_key(code: u32) -> Option<Action> {
    Some(match code {
        1 => Action::Close,
        28 => Action::Activate,
        14 => Action::Back,
        57 | 106 | 108 | 109 => Action::Next,
        103..=105 => Action::Previous,
        102 => Action::First,
        107 => Action::Last,
        13 | 78 => Action::Larger,
        12 | 74 => Action::Smaller,
        _ => return None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use readall_render::{DrawCommand, Rect, RenderLimits};
    #[test]
    fn native_xrgb_has_correct_channel_order_and_white_alpha_background() {
        let mut surface = Surface::new(2, 1, RenderLimits::default()).unwrap();
        surface
            .draw(&[DrawCommand::FillRect {
                rect: Rect::new(0, 0, 1, 1),
                color: Color::rgba(255, 0, 0, 128),
            }])
            .unwrap();
        let mut bytes = Vec::new();
        write_xrgb(&surface, &mut bytes).unwrap();
        assert_eq!(
            bytes,
            [0x00ff7f7f_u32.to_ne_bytes(), 0x00ffffff_u32.to_ne_bytes()].concat()
        );
    }
    #[test]
    fn only_documented_physical_keys_are_actions() {
        assert_eq!(physical_key(109), Some(Action::Next));
        assert_eq!(physical_key(104), Some(Action::Previous));
        assert_eq!(physical_key(13), Some(Action::Larger));
        assert_eq!(physical_key(28), Some(Action::Activate));
        assert_eq!(physical_key(14), Some(Action::Back));
        assert_eq!(physical_key(1), Some(Action::Close));
        assert_eq!(physical_key(30), None);
        assert_eq!(physical_key(u32::MAX), None);
    }
}

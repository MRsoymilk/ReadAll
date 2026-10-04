//! Wayland keyboard translation. Uses the compositor's serialized XKB state;
//! no hardcoded US text map and no environment/file includes in received keymaps.
use super::super::window::{Action, ReaderCommand, physical_key};
use std::{fs::File, io, os::unix::fs::FileExt};
use xkbcommon::xkb;
#[derive(Default)]
pub(super) struct Keyboard {
    state: Option<xkb::State>,
    mods: [u32; 4],
}
impl Keyboard {
    pub(super) fn keymap(&mut self, file: File, size: u32) -> io::Result<()> {
        self.state = None;
        if size == 0 || size > 2 * 1024 * 1024 {
            return Err(io::Error::other("keymap size limit"));
        }
        let mut bytes = vec![0; size as usize];
        file.read_exact_at(&mut bytes, 0)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(io::Error::other)?
            .trim_end_matches('\0');
        if text.contains('\0')
            || text
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == "include")
        {
            return Err(io::Error::other("keymap includes are disabled"));
        }
        let context =
            xkb::Context::new(xkb::CONTEXT_NO_DEFAULT_INCLUDES | xkb::CONTEXT_NO_ENVIRONMENT_NAMES);
        let keymap = xkb::Keymap::new_from_string(
            &context,
            text.to_owned(),
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or_else(|| io::Error::other("invalid keymap"))?;
        let mut state = xkb::State::new(&keymap);
        state.update_mask(self.mods[0], self.mods[1], self.mods[2], 0, 0, self.mods[3]);
        self.state = Some(state);
        Ok(())
    }
    pub(super) fn modifiers(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        self.mods = [depressed, latched, locked, group];
        if let Some(state) = &mut self.state {
            state.update_mask(depressed, latched, locked, 0, 0, group);
        }
    }
    pub(super) fn action(&self, code: u32, text_input: bool) -> Option<Action> {
        let control = self.state.as_ref().is_some_and(|state| {
            state.mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE)
        });
        if control {
            return match code {
                33 => Some(Action::Command(ReaderCommand::Find)),
                48 => Some(Action::Command(ReaderCommand::Bookmark)),
                46 => Some(Action::Command(ReaderCommand::Copy)),
                47 => Some(Action::Command(ReaderCommand::Paste)),
                _ => None,
            };
        }
        if text_input && !matches!(code,1|14|28|60..=67|102..=111) {
            let state = self.state.as_ref()?;
            let code = code.checked_add(8)?;
            let ch = char::from_u32(state.key_get_utf32(xkb::Keycode::new(code)))?;
            return (!ch.is_control()).then_some(Action::Text(ch));
        }
        physical_key(code)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_keymap_keeps_navigation_but_never_invents_text() {
        let keyboard = Keyboard::default();
        assert_eq!(keyboard.action(109, false), Some(Action::Next));
        assert_eq!(keyboard.action(30, true), None);
        assert_eq!(
            keyboard.action(60, false),
            Some(Action::Command(ReaderCommand::Find))
        );
    }
}

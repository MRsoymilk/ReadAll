//! Small host palette contract, with optional explicit app-private persistence.
use crate::reader_data::{Settings, Store, Theme};
use std::{io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Appearance {
    pub name: &'static str,
    /// ARGB: canvas/page/panel/ink/muted/border/accent/on-accent/button/selected/hover.
    pub colors: [u32; 11],
}
impl Default for Appearance {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}
impl Appearance {
    pub(crate) fn from_theme(theme: Theme) -> Self {
        Self {
            name: theme.name(),
            colors: theme.host_colors(),
        }
    }
}
/// No directory means a pure palette lookup (no disk access). With an absolute
/// state directory, read saved settings or change only the explicitly requested theme.
/// Hosts call the disk-backed form on their IO thread, never on every frame.
pub fn appearance(state_dir: Option<&Path>, requested: Option<&str>) -> io::Result<Appearance> {
    let requested = requested.map(Theme::parse).transpose()?;
    let theme = if let Some(root) = state_dir {
        if !root.is_absolute() {
            return Err(super::invalid(
                "appearance requires an absolute state directory",
            ));
        }
        let store = Store::new(root.join("library-v1"));
        match requested {
            Some(theme) => {
                store
                    .save_theme_with_defaults(
                        theme,
                        Settings {
                            size: 20,
                            margin: 16,
                            ..Settings::default()
                        },
                    )?
                    .theme
            }
            None => store.settings()?.theme,
        }
    } else {
        requested.unwrap_or_default()
    };
    Ok(Appearance::from_theme(theme))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader_data::{PageMode, Settings};
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    #[test]
    fn home_and_reader_share_theme_without_resetting_other_settings() {
        let root = std::env::temp_dir().join(format!(
            "readall-theme-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let store = Store::new(root.join("library-v1"));
        appearance(Some(&root), Some("dark")).unwrap();
        assert_eq!(store.settings().unwrap().size, 20);
        assert_eq!(store.settings().unwrap().margin, 16);
        let original = Settings {
            size: 30,
            margin: 12,
            page_mode: PageMode::Scroll,
            ..Settings::default()
        };
        store.save_settings(original).unwrap();
        let dark = appearance(Some(&root), Some("dark")).unwrap();
        assert_eq!(appearance(Some(&root), None).unwrap(), dark);
        assert_eq!(
            store.settings().unwrap(),
            Settings {
                theme: Theme::Dark,
                ..original
            }
        );
        assert_eq!(
            appearance(Some(&root), Some("light")).unwrap().name,
            "light"
        );
        let path = store.root().join("settings.conf");
        std::fs::write(&path, "foreign settings").unwrap();
        assert!(appearance(Some(&root), Some("dark")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "foreign settings");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn pure_palette_lookup_has_no_io_and_validates_names() {
        for name in ["light", "dark"] {
            let value = appearance(None, Some(name)).unwrap();
            assert_eq!(value.name, name);
            assert!(value.colors.iter().all(|c| c >> 24 == 255));
        }
        assert!(appearance(None, Some("broken")).is_err());
        assert!(appearance(Some(Path::new("relative")), Some("dark")).is_err());
    }
}

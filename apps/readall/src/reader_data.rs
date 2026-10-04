//! Bounded reader settings and annotations. Atomic writes, explicit corruption errors,
//! and a cooperative lock prevent two ReadAll windows from losing each other's edits.
use readall_epub::{EpubBook, EpubLocator};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Theme {
    #[default]
    Paper,
    Sepia,
    Dark,
}
impl Theme {
    pub(crate) fn colors(self) -> (readall_render::Color, readall_render::Color) {
        use readall_render::Color;
        match self {
            Self::Paper => (Color::WHITE, Color::rgba(24, 24, 24, 255)),
            Self::Sepia => (
                Color::rgba(244, 236, 216, 255),
                Color::rgba(68, 52, 37, 255),
            ),
            Self::Dark => (
                Color::rgba(28, 31, 36, 255),
                Color::rgba(222, 225, 230, 255),
            ),
        }
    }
    pub(crate) fn text_color(self, rgb: [u8; 3]) -> readall_render::Color {
        let (_, ink) = self.colors();
        if self == Self::Paper {
            let [r, g, b] = rgb;
            return readall_render::Color::rgba(r, g, b, 255);
        }
        if rgb.iter().all(|c| *c < 80) {
            return ink;
        }
        let [r, g, b] = rgb;
        if self == Self::Dark {
            readall_render::Color::rgba(r.max(80), g.max(80), b.max(80), 255)
        } else {
            readall_render::Color::rgba(r, g, b, 255)
        }
    }
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Sepia => "sepia",
            Self::Dark => "dark",
        }
    }
    pub(crate) fn parse(value: &str) -> io::Result<Self> {
        match value {
            "paper" => Ok(Self::Paper),
            "sepia" => Ok(Self::Sepia),
            "dark" => Ok(Self::Dark),
            _ => Err(invalid("theme expects paper, sepia or dark")),
        }
    }
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Paper => Self::Sepia,
            Self::Sepia => Self::Dark,
            Self::Dark => Self::Paper,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Settings {
    pub theme: Theme,
    pub size: u32,
    pub margin: u32,
    pub line_spacing: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Paper,
            size: 24,
            margin: 40,
            line_spacing: 1.0,
        }
    }
}
impl Settings {
    pub(crate) fn validate(self) -> io::Result<Self> {
        if !(8..=96).contains(&self.size)
            || self.margin > 160
            || !self.line_spacing.is_finite()
            || !(0.8..=2.0).contains(&self.line_spacing)
        {
            return Err(invalid("invalid reader settings"));
        }
        Ok(self)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Bookmark,
    Highlight,
    Note,
}
impl Kind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Bookmark => "bookmark",
            Self::Highlight => "highlight",
            Self::Note => "note",
        }
    }
    pub(crate) fn parse(value: &str) -> io::Result<Self> {
        match value {
            "bookmark" => Ok(Self::Bookmark),
            "highlight" => Ok(Self::Highlight),
            "note" => Ok(Self::Note),
            _ => Err(invalid("unknown annotation kind")),
        }
    }
}
#[derive(Debug, Clone)]
pub(crate) struct Annotation {
    pub id: u64,
    pub kind: Kind,
    pub locator: EpubLocator,
    pub end: Option<usize>,
    pub text: String,
}
#[derive(Debug, Clone)]
pub(crate) struct Store {
    root: PathBuf,
}
impl Store {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub(crate) fn from_environment() -> io::Result<Self> {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .map(|p| p.join(".local/state"))
            })
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "no absolute XDG_STATE_HOME or HOME",
                )
            })?;
        Ok(Self::new(base.join("readall/library-v1")))
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    fn annotations_path(&self, book: &EpubBook<'_>) -> PathBuf {
        self.root.join(format!("{}.annotations", book.id()))
    }
    pub(crate) fn settings(&self) -> io::Result<Settings> {
        parse_settings(read_text(&self.root.join("settings.conf"))?.as_deref())
    }
    pub(crate) fn save_settings(&self, settings: Settings) -> io::Result<()> {
        let settings = settings.validate()?;
        let _lock = Lock::acquire(&self.root)?;
        // Do not silently replace a corrupt/foreign file.
        self.settings()?;
        atomic(
            &self.root.join("settings.conf"),
            &format!(
                "readall-settings-v1\ntheme={}\nsize={}\nmargin={}\nline-spacing={}\n",
                settings.theme.name(),
                settings.size,
                settings.margin,
                settings.line_spacing
            ),
        )
    }
    pub(crate) fn annotations(&self, book: &EpubBook<'_>) -> io::Result<Vec<Annotation>> {
        parse_annotations(read_text(&self.annotations_path(book))?.as_deref(), book)
    }
    pub(crate) fn add(
        &self,
        book: &EpubBook<'_>,
        kind: Kind,
        locator: EpubLocator,
        end: Option<usize>,
        text: String,
    ) -> io::Result<u64> {
        let (spine, offset) = book
            .restore(&locator)
            .map_err(|e| invalid(&e.to_string()))?;
        if text.len() > 8192 || text.contains('\0') {
            return Err(invalid("annotation text too large or contains NUL"));
        }
        if let Some(end) = end {
            if end <= offset || locator.image_index().is_some() {
                return Err(invalid("invalid highlight range"));
            }
            book.locator(spine, end)
                .map_err(|e| invalid(&e.to_string()))?;
        }
        if kind == Kind::Highlight && end.is_none() {
            return Err(invalid("highlight requires an end offset"));
        }
        let _lock = Lock::acquire(&self.root)?;
        let mut rows = self.annotations(book)?;
        if kind == Kind::Bookmark
            && let Some(row) = rows.iter().find(|r| r.kind == kind && r.locator == locator)
        {
            return Ok(row.id);
        }
        if rows.len() >= 1024 {
            return Err(invalid("annotation count limit exceeded"));
        }
        let id = rows
            .iter()
            .map(|r| r.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| invalid("annotation ID overflow"))?;
        rows.push(Annotation {
            id,
            kind,
            locator,
            end,
            text,
        });
        self.write_annotations(book, &rows)?;
        Ok(id)
    }
    pub(crate) fn remove(&self, book: &EpubBook<'_>, id: u64) -> io::Result<bool> {
        let _lock = Lock::acquire(&self.root)?;
        let mut rows = self.annotations(book)?;
        let before = rows.len();
        rows.retain(|r| r.id != id);
        if before == rows.len() {
            return Ok(false);
        }
        self.write_annotations(book, &rows)?;
        Ok(true)
    }
    fn write_annotations(&self, book: &EpubBook<'_>, rows: &[Annotation]) -> io::Result<()> {
        let mut text = "readall-annotations-v1\n".to_owned();
        for row in rows {
            text.push_str(&format!(
                "{}|{}|{}|{}|{}\n",
                row.id,
                row.kind.name(),
                row.locator,
                row.end.map_or_else(|| "-".into(), |n| n.to_string()),
                hex(row.text.as_bytes())
            ));
        }
        if text.len() as u64 > MAX_BYTES {
            return Err(invalid("annotation byte limit exceeded"));
        }
        atomic(&self.annotations_path(book), &text)
    }
}
fn invalid(text: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, text.to_owned())
}
fn read_text(path: &Path) -> io::Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
        Ok(m) if !m.file_type().is_file() => {
            return Err(invalid("reader data must be a regular file"));
        }
        Ok(_) => {}
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("reader data byte limit exceeded"));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| invalid("reader data is not UTF-8"))
}
fn parse_settings(text: Option<&str>) -> io::Result<Settings> {
    let Some(text) = text else {
        return Ok(Settings::default());
    };
    let mut lines = text.lines();
    if lines.next() != Some("readall-settings-v1") {
        return Err(invalid("unknown reader settings version"));
    }
    let mut settings = Settings::default();
    let mut seen = std::collections::HashSet::new();
    for line in lines {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid("invalid settings line"))?;
        if !seen.insert(key) {
            return Err(invalid("duplicate setting"));
        }
        match key {
            "theme" => settings.theme = Theme::parse(value)?,
            "size" => settings.size = value.parse().map_err(|_| invalid("invalid font size"))?,
            "margin" => settings.margin = value.parse().map_err(|_| invalid("invalid margin"))?,
            "line-spacing" => {
                settings.line_spacing =
                    value.parse().map_err(|_| invalid("invalid line spacing"))?
            }
            _ => return Err(invalid("unknown setting")),
        }
    }
    settings.validate()
}
fn parse_annotations(text: Option<&str>, book: &EpubBook<'_>) -> io::Result<Vec<Annotation>> {
    let Some(text) = text else {
        return Ok(Vec::new());
    };
    let mut lines = text.lines();
    if lines.next() != Some("readall-annotations-v1") {
        return Err(invalid("unknown annotations version"));
    }
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in lines {
        if rows.len() >= 1024 {
            return Err(invalid("annotation count limit"));
        }
        let fields: Vec<_> = line.split('|').collect();
        if fields.len() != 5 {
            return Err(invalid("invalid annotation row"));
        }
        let id = fields[0]
            .parse::<u64>()
            .map_err(|_| invalid("invalid annotation ID"))?;
        if id == 0 || !seen.insert(id) {
            return Err(invalid("duplicate annotation ID"));
        }
        let kind = Kind::parse(fields[1])?;
        let locator: EpubLocator = fields[2]
            .parse()
            .map_err(|e: readall_epub::EpubError| invalid(&e.to_string()))?;
        if locator.book_id() != book.id() || locator.spine_index() >= book.spine().len() {
            return Err(invalid("annotation belongs to another book"));
        }
        let end = if fields[3] == "-" {
            None
        } else {
            Some(
                fields[3]
                    .parse::<usize>()
                    .map_err(|_| invalid("invalid range end"))?,
            )
        };
        if end.is_some_and(|end| end as u64 <= locator.utf8_offset())
            || kind == Kind::Highlight && end.is_none()
        {
            return Err(invalid("invalid annotation range"));
        }
        let text = unhex(fields[4])?;
        rows.push(Annotation {
            id,
            kind,
            locator,
            end,
            text,
        });
    }
    Ok(rows)
}
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}
fn unhex(text: &str) -> io::Result<String> {
    if text.len() > 16384 || !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err(invalid("invalid annotation text encoding"));
    }
    let bytes: Result<Vec<_>, _> = text
        .as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap_or(""), 16))
        .collect();
    let text = String::from_utf8(bytes.map_err(|_| invalid("invalid hexadecimal text"))?)
        .map_err(|_| invalid("annotation text not UTF-8"))?;
    if text.contains('\0') {
        return Err(invalid("annotation contains NUL"));
    }
    Ok(text)
}
struct Lock(PathBuf);
impl Lock {
    fn acquire(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        if !fs::symlink_metadata(root)?.file_type().is_dir() {
            return Err(invalid("reader data root must be a directory"));
        }
        let path = root.join(".write-lock");
        private_new(&path).map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "another reader is updating data; retry after it finishes",
                )
            } else {
                e
            }
        })?;
        Ok(Self(path))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn private_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
fn atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && !meta.file_type().is_file()
    {
        return Err(invalid("refusing to replace non-regular reader data"));
    }
    let root = path
        .parent()
        .ok_or_else(|| invalid("missing data directory"))?;
    let temp = root.join(format!(
        ".reader-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = private_new(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        #[cfg(unix)]
        File::open(root)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_epub;
    use readall_epub::EpubLimits;
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn temp() -> Temp {
        Temp(std::env::temp_dir().join(format!(
            "readall-data-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    #[test]
    fn persistence_roundtrips_unicode_annotations_settings_and_deletion() {
        let tmp = temp();
        let store = Store::new(tmp.0.clone());
        let data = test_epub::make_epub();
        let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
        let locator = book.locator(0, 0).unwrap();
        let id = store
            .add(
                &book,
                Kind::Bookmark,
                locator.clone(),
                None,
                "中文 | note\nline".into(),
            )
            .unwrap();
        assert_eq!(
            store
                .add(&book, Kind::Bookmark, locator.clone(), None, "again".into())
                .unwrap(),
            id
        );
        store
            .add(
                &book,
                Kind::Highlight,
                locator.clone(),
                Some(4),
                "mark".into(),
            )
            .unwrap();
        store
            .add(&book, Kind::Note, locator, None, "note".into())
            .unwrap();
        let rows = Store::new(tmp.0.clone()).annotations(&book).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].text, "中文 | note\nline");
        assert_eq!(rows[1].end, Some(4));
        assert!(store.remove(&book, id).unwrap());
        assert!(!store.remove(&book, id).unwrap());
        let settings = Settings {
            theme: Theme::Dark,
            size: 28,
            margin: 36,
            line_spacing: 1.25,
        };
        store.save_settings(settings).unwrap();
        assert_eq!(store.settings().unwrap(), settings);
    }
    #[test]
    fn corruption_and_conflicts_never_overwrite_existing_data() {
        let tmp = temp();
        let store = Store::new(tmp.0.clone());
        fs::create_dir_all(&tmp.0).unwrap();
        fs::write(tmp.0.join("settings.conf"), b"broken").unwrap();
        assert!(store.save_settings(Settings::default()).is_err());
        assert_eq!(fs::read(tmp.0.join("settings.conf")).unwrap(), b"broken");
        let lock = Lock::acquire(&tmp.0).unwrap();
        assert!(Lock::acquire(&tmp.0).is_err());
        drop(lock);
        assert!(Lock::acquire(&tmp.0).is_ok());
        assert!(parse_settings(Some("readall-settings-v1\nsize=NaN\n")).is_err());
        assert!(unhex("fff").is_err());
    }
    #[test]
    fn image_bookmarks_keep_v2_locator() {
        let tmp = temp();
        let store = Store::new(tmp.0.clone());
        let data = test_epub::make_epub_with_resources(
            &["<html><body><img src='a.png'/><img src='a.png'/></body></html>"],
            vec![],
        );
        let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
        let locator = book.image_locator(0, 1).unwrap();
        store
            .add(&book, Kind::Bookmark, locator.clone(), None, String::new())
            .unwrap();
        assert_eq!(store.annotations(&book).unwrap()[0].locator, locator);
    }
}

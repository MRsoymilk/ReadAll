//! Versioned, bounded recent-EPUB list for the native library.
#![cfg(target_os = "linux")]

use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const VERSION: &str = "readall-recent-linux-v1";
const MAX_RECENT: usize = 24;
const MAX_PATH_BYTES: usize = 4096;
const MAX_FILE_BYTES: u64 = 256 * 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub(crate) struct RecentStore {
    root: PathBuf,
}

impl RecentStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    #[cfg(all(target_os = "linux", feature = "wayland"))]
    pub(crate) fn from_environment() -> io::Result<Self> {
        Ok(Self::new(environment_root()?))
    }

    fn path(&self) -> PathBuf {
        self.root.join("recent-linux-v1.state")
    }

    pub(crate) fn load(&self) -> io::Result<Vec<PathBuf>> {
        let file = match File::open(self.path()) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "recent-books state is not a regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "recent-books state exceeds the size limit",
            ));
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "recent-books state is not UTF-8",
            )
        })?;
        let mut lines = text.lines();
        if lines.next() != Some(VERSION) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unknown recent-books state version",
            ));
        }

        let mut entries = Vec::new();
        for line in lines {
            if line.is_empty() {
                continue;
            }
            if entries.len() >= MAX_RECENT {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "recent-books state has too many entries",
                ));
            }
            let bytes = decode_hex(line)?;
            if bytes.is_empty() || bytes.len() > MAX_PATH_BYTES || bytes.contains(&0) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid recent EPUB path",
                ));
            }
            let path = PathBuf::from(OsString::from_vec(bytes));
            if !path.is_absolute() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "recent EPUB path is not absolute",
                ));
            }
            if entries.iter().any(|existing| existing == &path) {
                continue;
            }
            if path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"))
            {
                entries.push(path);
            }
        }
        Ok(entries)
    }

    pub(crate) fn record(&self, path: &Path) -> io::Result<()> {
        if !path.is_absolute() || !path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "recent EPUB path must be an existing absolute file",
            ));
        }
        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "recent EPUB path must have .epub extension",
            ));
        }
        let raw = path.as_os_str().as_bytes();
        if raw.is_empty() || raw.len() > MAX_PATH_BYTES || raw.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "recent EPUB path exceeds the supported path budget",
            ));
        }

        let mut entries = self.load()?;
        entries.retain(|existing| existing != path);
        entries.insert(0, path.to_path_buf());
        entries.truncate(MAX_RECENT);
        self.write(&entries)
    }

    fn write(&self, entries: &[PathBuf]) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        if !self.root.metadata()?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "recent-books root is not a directory",
            ));
        }

        let mut body = String::new();
        body.push_str(VERSION);
        body.push('\n');
        for path in entries.iter().take(MAX_RECENT) {
            let raw = path.as_os_str().as_bytes();
            if raw.is_empty() || raw.len() > MAX_PATH_BYTES || raw.contains(&0) {
                continue;
            }
            push_hex(&mut body, raw)?;
            body.push('\n');
        }
        if body.len() as u64 > MAX_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "recent-books state exceeds the size limit",
            ));
        }

        let temp = self.root.join(format!(
            ".recent.{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let target = self.path();
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(body.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temp, &target)?;
            File::open(&self.root)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

fn environment_root() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return Ok(path.join("readall"));
        }
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "neither absolute XDG_STATE_HOME nor HOME is available",
            )
        })?;
    Ok(home.join(".local/state/readall"))
}

fn push_hex(output: &mut String, bytes: &[u8]) -> io::Result<()> {
    output
        .try_reserve(bytes.len().saturating_mul(2))
        .map_err(|_| io::Error::other("cannot allocate recent-books state"))?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(())
}

fn decode_hex(text: &str) -> io::Result<Vec<u8>> {
    if text.len() % 2 != 0 || text.len() > MAX_PATH_BYTES.saturating_mul(2) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid recent-books path encoding",
        ));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(text.len() / 2)
        .map_err(|_| io::Error::other("cannot allocate recent-books path"))?;
    for pair in text.as_bytes().chunks_exact(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn hex_nibble(byte: u8) -> io::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid recent-books path encoding",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "readall-recent-test-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
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

    #[test]
    fn recent_books_roundtrip_exact_non_utf8_paths_and_move_existing_to_front() {
        let temp = Temp::new();
        let root = temp.0.join("state");
        let books = temp.0.join("books");
        fs::create_dir(&books).unwrap();
        let first = books.join("first.epub");
        fs::write(&first, b"x").unwrap();
        let second = books.join(OsString::from_vec(b"second-\xff.epub".to_vec()));
        fs::write(&second, b"y").unwrap();

        let store = RecentStore::new(root);
        store.record(&first).unwrap();
        store.record(&second).unwrap();
        assert_eq!(store.load().unwrap(), vec![second.clone(), first.clone()]);

        store.record(&first).unwrap();
        assert_eq!(store.load().unwrap(), vec![first, second]);
    }

    #[test]
    fn missing_files_are_pruned_and_corrupt_state_is_rejected() {
        let temp = Temp::new();
        let store = RecentStore::new(temp.0.join("state"));
        let book = temp.0.join("book.epub");
        fs::write(&book, b"x").unwrap();
        store.record(&book).unwrap();
        fs::remove_file(&book).unwrap();
        assert!(store.load().unwrap().is_empty());

        fs::write(store.path(), b"broken\n").unwrap();
        assert_eq!(store.load().unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}

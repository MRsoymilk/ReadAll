//! Versioned, dependency-free reading progress storage.
//! Progress is keyed by document content identity and stores a content locator, never a page number.
use readall_core::{DocumentId, TextDocument, TextLocator};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_PROGRESS_BYTES: u64 = 1024;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub(crate) struct ProgressStore {
    root: PathBuf,
}

impl ProgressStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn from_environment() -> io::Result<Self> {
        if let Some(path) = env::var_os("XDG_STATE_HOME") {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                return Ok(Self::new(path.join("readall/progress-v1")));
            }
        }
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "neither absolute XDG_STATE_HOME nor HOME is available",
                )
            })?;
        Ok(Self::new(home.join(".local/state/readall/progress-v1")))
    }

    fn path_for(&self, id: DocumentId) -> PathBuf {
        self.root.join(format!("{id}.state"))
    }

    pub(crate) fn load(&self, document: &TextDocument) -> io::Result<Option<TextLocator>> {
        let path = self.path_for(document.id());
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "reading progress is not a regular file",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_PROGRESS_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_PROGRESS_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "reading progress exceeds the size limit",
            ));
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "reading progress is not UTF-8")
        })?;
        let text = text.strip_suffix('\n').unwrap_or(text);
        let mut lines = text.lines();
        if lines.next() != Some("readall-progress-v1") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unknown reading progress version",
            ));
        }
        let locator = lines
            .next()
            .and_then(|line| line.strip_prefix("locator="))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing reading locator"))?;
        if lines.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected reading progress fields",
            ));
        }
        let locator: TextLocator = locator
            .parse()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        document
            .restore(&locator)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(Some(locator))
    }

    pub(crate) fn save(&self, locator: &TextLocator) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        if !self.root.metadata()?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reading progress root is not a directory",
            ));
        }
        let target = self.path_for(locator.document_id);
        let temp = self.root.join(format!(
            ".{}.{}.{}.tmp",
            locator.document_id,
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            write!(file, "readall-progress-v1\nlocator={locator}\n")?;
            file.sync_all()?;
            fs::rename(&temp, &target)?;
            sync_directory(&self.root)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use readall_core::Limits;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "readall-progress-test-{}-{}",
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
    fn save_load_and_replace_are_content_keyed() {
        let temp = Temp::new();
        let store = ProgressStore::new(temp.0.join("state"));
        let document = TextDocument::from_bytes("AéZ".as_bytes(), Limits::default()).unwrap();
        assert!(store.load(&document).unwrap().is_none());

        let first = document.locator(1).unwrap();
        store.save(&first).unwrap();
        assert_eq!(store.load(&document).unwrap(), Some(first));

        let second = document.locator(document.text().len()).unwrap();
        store.save(&second).unwrap();
        assert_eq!(store.load(&document).unwrap(), Some(second));
        assert_eq!(fs::read_dir(&store.root).unwrap().count(), 1);

        let other = TextDocument::from_bytes(b"different", Limits::default()).unwrap();
        assert!(store.load(&other).unwrap().is_none());
    }

    #[test]
    fn corrupt_or_mismatched_progress_is_rejected() {
        let temp = Temp::new();
        let store = ProgressStore::new(temp.0.join("state"));
        fs::create_dir_all(&store.root).unwrap();
        let document = TextDocument::from_bytes(b"abc", Limits::default()).unwrap();
        let path = store.path_for(document.id());

        fs::write(&path, b"broken\n").unwrap();
        assert_eq!(
            store.load(&document).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        let other = TextDocument::from_bytes(b"xyz", Limits::default()).unwrap();
        fs::write(
            &path,
            format!(
                "readall-progress-v1\nlocator={}\n",
                other.locator(0).unwrap()
            ),
        )
        .unwrap();
        assert_eq!(
            store.load(&document).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn oversized_progress_is_rejected() {
        let temp = Temp::new();
        let store = ProgressStore::new(temp.0.join("state"));
        fs::create_dir_all(&store.root).unwrap();
        let document = TextDocument::from_bytes(b"abc", Limits::default()).unwrap();
        fs::write(
            store.path_for(document.id()),
            vec![b'x'; MAX_PROGRESS_BYTES as usize + 1],
        )
        .unwrap();
        assert_eq!(
            store.load(&document).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}

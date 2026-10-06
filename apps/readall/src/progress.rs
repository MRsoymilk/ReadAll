//! Versioned, dependency-free reading progress storage.
//! Progress is keyed by document content identity and stores a content locator, never a page number.
use readall_core::{DocumentId, TextDocument, TextLocator};
use readall_epub::{EpubBook, EpubLocator};
use std::{
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

    #[cfg(all(target_os = "linux", feature = "wayland"))]
    pub(crate) fn from_environment() -> io::Result<Self> {
        Ok(Self::new(environment_root()?))
    }

    fn path_for(&self, id: DocumentId) -> PathBuf {
        self.root.join(format!("{id}.state"))
    }

    pub(crate) fn load(&self, document: &TextDocument) -> io::Result<Option<TextLocator>> {
        let Some(raw) = read_locator(&self.path_for(document.id()), "readall-progress-v1")? else {
            return Ok(None);
        };
        let locator: TextLocator = raw
            .parse()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        document
            .restore(&locator)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(Some(locator))
    }

    pub(crate) fn save(&self, locator: &TextLocator) -> io::Result<()> {
        let target = self.path_for(locator.document_id);
        write_locator(
            &self.root,
            &target,
            &locator.document_id.to_string(),
            "readall-progress-v1",
            &locator.to_string(),
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EpubProgressStore {
    root: PathBuf,
}

impl EpubProgressStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    #[cfg(all(target_os = "linux", feature = "wayland"))]
    pub(crate) fn from_environment() -> io::Result<Self> {
        Ok(Self::new(environment_root()?))
    }

    fn path_for(&self, id: DocumentId) -> PathBuf {
        self.root.join(format!("epub-{id}.state"))
    }

    pub(crate) fn load(&self, book: &EpubBook<'_>) -> io::Result<Option<EpubLocator>> {
        let Some(raw) = read_locator(&self.path_for(book.id()), "readall-epub-progress-v1")? else {
            return Ok(None);
        };
        let locator: EpubLocator = raw
            .parse()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        book.restore(&locator)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(Some(locator))
    }

    pub(crate) fn save(&self, locator: &EpubLocator) -> io::Result<()> {
        let target = self.path_for(locator.book_id());
        write_locator(
            &self.root,
            &target,
            &format!("epub-{}", locator.book_id()),
            "readall-epub-progress-v1",
            &locator.to_string(),
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PdfProgressStore {
    root: PathBuf,
}

impl PdfProgressStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    #[cfg(all(target_os = "linux", feature = "wayland"))]
    pub(crate) fn from_environment() -> io::Result<Self> {
        Ok(Self::new(environment_root()?))
    }

    fn path_for(&self, id: DocumentId) -> PathBuf {
        self.root.join(format!("pdf-{id}.state"))
    }

    pub(crate) fn load(&self, id: DocumentId, pages: usize) -> io::Result<Option<usize>> {
        let Some(raw) = read_locator(&self.path_for(id), "readall-pdf-progress-v1")? else {
            return Ok(None);
        };
        let page = raw
            .strip_prefix("page-")
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid PDF page locator"))?
            .parse::<usize>()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid PDF page number"))?;
        if page >= pages {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "saved PDF page is outside the document",
            ));
        }
        Ok(Some(page))
    }

    pub(crate) fn save(&self, id: DocumentId, page: usize) -> io::Result<()> {
        let target = self.path_for(id);
        write_locator(
            &self.root,
            &target,
            &format!("pdf-{id}"),
            "readall-pdf-progress-v1",
            &format!("page-{page}"),
        )
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
fn environment_root() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return Ok(path.join("readall/progress-v1"));
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
    Ok(home.join(".local/state/readall/progress-v1"))
}

fn read_locator(path: &Path, version: &str) -> io::Result<Option<String>> {
    let file = match File::open(path) {
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
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "reading progress is not UTF-8"))?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next() != Some(version) {
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
    Ok(Some(locator.to_owned()))
}

fn write_locator(
    root: &Path,
    target: &Path,
    temp_key: &str,
    version: &str,
    locator: &str,
) -> io::Result<()> {
    fs::create_dir_all(root)?;
    if !root.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reading progress root is not a directory",
        ));
    }
    let temp = root.join(format!(
        ".{temp_key}.{}.{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        write!(file, "{version}\nlocator={locator}\n")?;
        file.sync_all()?;
        fs::rename(&temp, target)?;
        sync_directory(root)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_epub;
    use readall_core::Limits;
    use readall_epub::EpubLimits;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
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
    fn epub_progress_roundtrips_stable_locator() {
        let temp = Temp::new();
        let store = EpubProgressStore::new(temp.0.join("state"));
        let bytes = test_epub::make_epub();
        let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
        assert!(store.load(&book).unwrap().is_none());
        let locator = book.locator(1, 11).unwrap();
        store.save(&locator).unwrap();
        assert_eq!(store.load(&book).unwrap(), Some(locator));
    }

    #[test]
    fn pdf_progress_is_content_keyed_and_page_bounded() {
        let temp = Temp::new();
        let store = PdfProgressStore::new(temp.0.join("state"));
        let first = DocumentId::of(b"first pdf");
        let second = DocumentId::of(b"second pdf");
        assert_eq!(store.load(first, 10).unwrap(), None);
        store.save(first, 4).unwrap();
        assert_eq!(store.load(first, 10).unwrap(), Some(4));
        assert_eq!(store.load(second, 10).unwrap(), None);
        assert_eq!(
            store.load(first, 4).unwrap_err().kind(),
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

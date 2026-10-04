//! Host adapters. Linux Wayland is opt-in; Android SAF and other window backends remain unimplemented.
#[cfg(all(target_os = "linux", feature = "wayland"))]
pub mod web_link;
pub mod window;

// Necessary native FFI is isolated; all other platform modules deny unsafe code.
#[cfg(all(target_os = "linux", feature = "wayland"))]
#[allow(unsafe_code)]
mod wayland;

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

use readall_core::DocumentSource;

#[derive(Debug)]
pub struct LocalFileSource {
    file: File,
}

impl LocalFileSource {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        // Avoid opening known directories, devices, and FIFOs as books.
        // A concurrent path replacement remains subject to ordinary host filesystem semantics.
        if !path.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "book source must be a regular file",
            ));
        }
        let file = File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "opened source is not a regular file",
            ));
        }
        Ok(Self { file })
    }
}

impl DocumentSource for LocalFileSource {
    fn length(&self) -> io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }
    fn read_at(&mut self, offset: u64, destination: &mut [u8]) -> io::Result<usize> {
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read(destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::OpenOptions,
        io::Write,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct TestFile(PathBuf);
    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn file_source_reads_offsets_and_normalized_document() {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "readall-file-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let temp = TestFile(path);
        file.write_all(b"abc\r\nxyz").unwrap();
        drop(file);
        let mut source = LocalFileSource::open(&temp.0).unwrap();
        assert_eq!(source.length().unwrap(), 8);
        let mut buffer = [0; 3];
        assert_eq!(source.read_at(5, &mut buffer).unwrap(), 3);
        assert_eq!(&buffer, b"xyz");
        assert_eq!(source.read_at(8, &mut buffer).unwrap(), 0);
        let document =
            readall_core::TextDocument::open(&mut source, readall_core::Limits::default()).unwrap();
        assert_eq!(document.text(), "abc\nxyz");
    }

    #[test]
    fn directories_are_not_document_sources() {
        assert_eq!(
            LocalFileSource::open(std::env::temp_dir())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

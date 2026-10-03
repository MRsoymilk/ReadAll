//! Persistent, dependency-free diagnostics for failures users can report.
use std::path::PathBuf;

#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
use std::{
    error::Error,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
const LOG_NAME: &str = "readall-error.log";

#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
#[derive(Debug)]
pub(crate) struct StageError {
    stage: &'static str,
    source: Box<dyn Error>,
}

#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EPUB stage '{}' failed: {}", self.stage, self.source)
    }
}

#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
impl Error for StageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
pub(crate) fn boxed_stage(stage: &'static str, source: Box<dyn Error>) -> Box<dyn Error> {
    Box::new(StageError { stage, source })
}

#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
pub(crate) trait ResultContext<T> {
    fn epub_stage(self, stage: &'static str) -> Result<T, Box<dyn Error>>;
}

#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
impl<T, E> ResultContext<T> for Result<T, E>
where
    E: Error + 'static,
{
    fn epub_stage(self, stage: &'static str) -> Result<T, Box<dyn Error>> {
        self.map_err(|source| {
            Box::new(StageError {
                stage,
                source: Box::new(source),
            }) as Box<dyn Error>
        })
    }
}

#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
pub(crate) fn log_epub_failure(path: &Path, error: &(dyn Error + 'static)) -> io::Result<PathBuf> {
    log_epub_failure_at(&diagnostic_root(), path, error)
}

#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
fn log_epub_failure_at(
    root: &Path,
    path: &Path,
    error: &(dyn Error + 'static),
) -> io::Result<PathBuf> {
    fs::create_dir_all(root)?;
    let target = root.join(LOG_NAME);
    rotate_if_needed(&target)?;

    let mut file = OpenOptions::new().create(true).append(true).open(&target)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    writeln!(file, "===== ReadAll EPUB failure =====")?;
    writeln!(
        file,
        "unix_time: {}.{:09}",
        now.as_secs(),
        now.subsec_nanos()
    )?;
    writeln!(file, "version: {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(
        file,
        "platform: {}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )?;
    writeln!(file, "pid: {}", std::process::id())?;
    if let Ok(cwd) = std::env::current_dir() {
        writeln!(file, "cwd: {}", cwd.display())?;
    }
    let argv = std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(file, "argv: {argv}")?;
    if let Some(display) = std::env::var_os("WAYLAND_DISPLAY") {
        writeln!(file, "wayland_display: {}", display.to_string_lossy())?;
    }
    writeln!(file, "book: {}", path.display())?;
    match fs::metadata(path) {
        Ok(metadata) => {
            writeln!(file, "book_size: {}", metadata.len())?;
            if let Ok(modified) = metadata.modified()
                && let Ok(value) = modified.duration_since(UNIX_EPOCH)
            {
                writeln!(file, "book_modified_unix: {}", value.as_secs())?;
            }
        }
        Err(metadata_error) => {
            writeln!(file, "book_metadata_error: {metadata_error}")?;
        }
    }

    writeln!(file, "error: {error}")?;
    writeln!(file, "error_debug: {error:?}")?;
    let mut depth = 0_usize;
    let mut source = error.source();
    while let Some(cause) = source {
        depth += 1;
        writeln!(file, "caused_by[{depth}]: {cause}")?;
        source = cause.source();
        if depth >= 32 {
            writeln!(file, "caused_by: truncated after 32 levels")?;
            break;
        }
    }
    writeln!(file)?;
    file.flush()?;
    Ok(target)
}

pub(crate) fn diagnostic_path() -> PathBuf {
    diagnostic_root().join(LOG_NAME)
}

fn diagnostic_root() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return path.join("readall/logs");
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if home.is_absolute() {
            return home.join(".local/state/readall/logs");
        }
    }
    std::env::temp_dir().join("readall/logs")
}

#[cfg(any(test, all(target_os = "linux", feature = "wayland")))]
fn rotate_if_needed(path: &Path) -> io::Result<()> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    if metadata.len() < MAX_LOG_BYTES {
        return Ok(());
    }
    let old = path.with_extension("log.old");
    let _ = fs::remove_file(&old);
    match fs::rename(path, &old) {
        Ok(()) => Ok(()),
        Err(_) => {
            File::create(path)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Debug)]
    struct Inner;
    impl fmt::Display for Inner {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("inner failure")
        }
    }
    impl Error for Inner {}

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "readall-diagnostics-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn stage_error_preserves_source_chain() {
        let error = Err::<(), _>(Inner).epub_stage("parse package").unwrap_err();
        assert!(error.to_string().contains("parse package"));
        assert_eq!(error.source().unwrap().to_string(), "inner failure");
    }

    #[test]
    fn failure_log_contains_file_metadata_stage_and_error_chain() {
        let root = temp_dir();
        let book = root.join("坏书.epub");
        fs::write(&book, b"epub bytes").unwrap();
        let error = boxed_stage("parse EPUB ZIP/container/OPF", Box::new(Inner));
        let path = log_epub_failure_at(&root, &book, error.as_ref()).unwrap();
        let log = fs::read_to_string(path).unwrap();
        assert!(log.contains("ReadAll EPUB failure"));
        assert!(log.contains("坏书.epub"));
        assert!(log.contains("book_size: 10"));
        assert!(log.contains("parse EPUB ZIP/container/OPF"));
        assert!(log.contains("caused_by[1]: inner failure"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rotation_keeps_log_bounded() {
        let root = temp_dir();
        let path = root.join(LOG_NAME);
        fs::write(&path, vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        rotate_if_needed(&path).unwrap();
        assert!(!path.exists());
        assert!(path.with_extension("log.old").exists());
        let _ = fs::remove_dir_all(root);
    }
}

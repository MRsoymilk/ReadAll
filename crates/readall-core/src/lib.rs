//! Platform-independent document loading and stable text locations.
//! No windowing, filesystem paths, or third-party dependencies belong here.

mod digest;
pub mod layout;
pub mod preview;
pub mod source;
pub mod text;

pub use digest::DocumentId;
pub use source::{BytesSource, DocumentSource, read_bounded};
pub use text::{TextDocument, TextEncoding, TextLocator};

use std::{fmt, io};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_source_bytes: usize,
    pub max_decoded_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_source_bytes: 32 * 1024 * 1024,
            max_decoded_bytes: 64 * 1024 * 1024,
        }
    }
}

/// A signature hint, not validation or a claim of format support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatHint {
    Pdf,
    ZipArchive,
    Mobi,
    TextCandidate,
}

pub fn format_hint(bytes: &[u8]) -> FormatHint {
    if bytes.get(60..68) == Some(b"BOOKMOBI") {
        FormatHint::Mobi
    } else if bytes.starts_with(b"%PDF-") {
        FormatHint::Pdf
    } else if [b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"]
        .iter()
        .any(|magic| bytes.starts_with(*magic))
    {
        FormatHint::ZipArchive
    } else {
        FormatHint::TextCandidate
    }
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    LimitExceeded { what: &'static str, limit: usize },
    AllocationFailed,
    SourceChanged,
    UnsupportedFormat(FormatHint),
    InvalidEncoding(&'static str),
    InvalidText(char),
    InvalidLocator(&'static str),
    InvalidLayout(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "document I/O error: {e}"),
            Self::LimitExceeded { what, limit } => {
                write!(f, "{what} exceeds the configured limit ({limit} bytes)")
            }
            Self::AllocationFailed => f.write_str("cannot allocate document buffer"),
            Self::SourceChanged => {
                f.write_str("source length changed while reading; reopen the document")
            }
            Self::UnsupportedFormat(FormatHint::Pdf) => {
                f.write_str("PDF signature detected; the PDF engine is not implemented yet")
            }
            Self::UnsupportedFormat(FormatHint::ZipArchive) => f.write_str(
                "ZIP archive detected (possibly EPUB); the EPUB engine is not implemented yet",
            ),
            Self::UnsupportedFormat(FormatHint::Mobi) => f.write_str(
                "MOBI signature detected; use open-mobi, mobi-text or render-mobi instead of the TXT reader",
            ),
            Self::UnsupportedFormat(FormatHint::TextCandidate) => {
                f.write_str("unsupported document format")
            }
            Self::InvalidEncoding(reason) => {
                write!(f, "invalid or unsupported text encoding: {reason}")
            }
            Self::InvalidText(ch) => {
                write!(f, "unsupported control character U+{:04X}", u32::from(*ch))
            }
            Self::InvalidLocator(reason) => write!(f, "invalid text locator: {reason}"),
            Self::InvalidLayout(reason) => write!(f, "invalid layout: {reason}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(error) = self {
            Some(error)
        } else {
            None
        }
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_are_hints_not_epub_claims() {
        assert_eq!(format_hint(b"%PDF-1.7"), FormatHint::Pdf);
        assert_eq!(format_hint(b"PK\x03\x04data"), FormatHint::ZipArchive);
        assert_eq!(format_hint(b"plain text"), FormatHint::TextCandidate);
        assert_eq!(format_hint(b""), FormatHint::TextCandidate);
    }
}

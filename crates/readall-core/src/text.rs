use std::{fmt, str::FromStr};

use crate::{DocumentId, DocumentSource, Error, FormatHint, Limits, format_hint, read_bounded};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

/// Offsets are bytes in canonical UTF-8 text, NOT bytes in the original file.
/// Canonicalization v1 strips an encoding BOM and maps CRLF / CR to LF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextLocator {
    pub document_id: DocumentId,
    pub utf8_offset: u64,
}

impl fmt::Display for TextLocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "txt-v1:{}:{}", self.document_id, self.utf8_offset)
    }
}

impl FromStr for TextLocator {
    type Err = Error;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.split(':');
        if parts.next() != Some("txt-v1") {
            return Err(Error::InvalidLocator("unknown locator version"));
        }
        let id = parts
            .next()
            .ok_or(Error::InvalidLocator("missing document ID"))?
            .parse()?;
        let offset = parts
            .next()
            .ok_or(Error::InvalidLocator("missing offset"))?;
        if offset.is_empty()
            || !offset.bytes().all(|byte| byte.is_ascii_digit())
            || parts.next().is_some()
        {
            return Err(Error::InvalidLocator("expected an unsigned byte offset"));
        }
        let utf8_offset = offset
            .parse()
            .map_err(|_| Error::InvalidLocator("offset overflow"))?;
        Ok(Self {
            document_id: id,
            utf8_offset,
        })
    }
}

#[derive(Debug)]
pub struct TextDocument {
    id: DocumentId,
    text: String,
    encoding: TextEncoding,
}

impl TextDocument {
    pub fn open(source: &mut impl DocumentSource, limits: Limits) -> Result<Self, Error> {
        Self::from_bytes(&read_bounded(source, limits.max_source_bytes)?, limits)
    }

    pub fn from_bytes(bytes: &[u8], limits: Limits) -> Result<Self, Error> {
        if bytes.len() > limits.max_source_bytes {
            return Err(Error::LimitExceeded {
                what: "source",
                limit: limits.max_source_bytes,
            });
        }
        let hint = format_hint(bytes);
        if hint != FormatHint::TextCandidate {
            return Err(Error::UnsupportedFormat(hint));
        }
        // UTF-32LE also starts with the UTF-16LE BOM: reject before UTF-16 detection.
        if bytes.starts_with(&[0xff, 0xfe, 0, 0]) || bytes.starts_with(&[0, 0, 0xfe, 0xff]) {
            return Err(Error::InvalidEncoding("UTF-32 is not supported"));
        }
        let (body, encoding) = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            (&bytes[3..], TextEncoding::Utf8Bom)
        } else if bytes.starts_with(&[0xff, 0xfe]) {
            (&bytes[2..], TextEncoding::Utf16Le)
        } else if bytes.starts_with(&[0xfe, 0xff]) {
            (&bytes[2..], TextEncoding::Utf16Be)
        } else {
            (bytes, TextEncoding::Utf8)
        };
        let text = match encoding {
            TextEncoding::Utf8 | TextEncoding::Utf8Bom => {
                let decoded = std::str::from_utf8(body)
                    .map_err(|_| Error::InvalidEncoding("expected UTF-8, or UTF-16 with a BOM"))?;
                normalize(decoded.chars().map(Ok), limits.max_decoded_bytes)?
            }
            TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
                if body.len() % 2 != 0 {
                    return Err(Error::InvalidEncoding("odd UTF-16 byte count"));
                }
                let words = body.chunks_exact(2).map(|pair| {
                    if encoding == TextEncoding::Utf16Le {
                        u16::from_le_bytes([pair[0], pair[1]])
                    } else {
                        u16::from_be_bytes([pair[0], pair[1]])
                    }
                });
                normalize(
                    char::decode_utf16(words).map(|item| {
                        item.map_err(|_| Error::InvalidEncoding("unpaired UTF-16 surrogate"))
                    }),
                    limits.max_decoded_bytes,
                )?
            }
        };
        Ok(Self {
            id: DocumentId::of(bytes),
            text,
            encoding,
        })
    }

    pub fn id(&self) -> DocumentId {
        self.id
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    pub fn locator(&self, utf8_offset: usize) -> Result<TextLocator, Error> {
        self.validate_offset(utf8_offset)?;
        Ok(TextLocator {
            document_id: self.id,
            utf8_offset: utf8_offset as u64,
        })
    }

    pub fn restore(&self, locator: &TextLocator) -> Result<usize, Error> {
        if locator.document_id != self.id {
            return Err(Error::InvalidLocator(
                "document content has changed or belongs to another book",
            ));
        }
        let offset = usize::try_from(locator.utf8_offset)
            .map_err(|_| Error::InvalidLocator("offset cannot fit on this platform"))?;
        self.validate_offset(offset)?;
        Ok(offset)
    }

    fn validate_offset(&self, offset: usize) -> Result<(), Error> {
        if !self.text.is_char_boundary(offset) {
            return Err(Error::InvalidLocator(
                "offset is outside the text or inside a UTF-8 character",
            ));
        }
        Ok(())
    }
}

fn normalize(
    chars: impl Iterator<Item = Result<char, Error>>,
    limit: usize,
) -> Result<String, Error> {
    let mut output = String::new();
    let mut previous_cr = false;
    for ch in chars {
        let ch = ch?;
        if ch == '\n' && previous_cr {
            previous_cr = false;
            continue;
        }
        previous_cr = ch == '\r';
        let ch = if previous_cr { '\n' } else { ch };
        // Prevent binary input and terminal escape sequences from reaching the diagnostic CLI.
        if ch.is_control() && ch != '\n' && ch != '\t' {
            return Err(Error::InvalidText(ch));
        }
        let length = output
            .len()
            .checked_add(ch.len_utf8())
            .filter(|length| *length <= limit)
            .ok_or(Error::LimitExceeded {
                what: "decoded text",
                limit,
            })?;
        output
            .try_reserve(length - output.len())
            .map_err(|_| Error::AllocationFailed)?;
        output.push(ch);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(bytes: &[u8]) -> TextDocument {
        TextDocument::from_bytes(bytes, Limits::default()).unwrap()
    }

    #[test]
    fn utf8_bom_and_line_endings() {
        let doc = document("\u{feff}第一章\r\nhello\rworld\n".as_bytes());
        assert_eq!(doc.encoding(), TextEncoding::Utf8Bom);
        assert_eq!(doc.text(), "第一章\nhello\nworld\n");
        assert_eq!(document(b"a\r\r\nb").text(), "a\n\nb");
    }

    #[test]
    fn utf16_both_endian_orders_and_surrogate_pairs() {
        for little in [true, false] {
            let mut bytes = if little {
                vec![0xff, 0xfe]
            } else {
                vec![0xfe, 0xff]
            };
            for word in "中文😀\r\n".encode_utf16() {
                bytes.extend_from_slice(&if little {
                    word.to_le_bytes()
                } else {
                    word.to_be_bytes()
                });
            }
            let doc = document(&bytes);
            assert_eq!(doc.text(), "中文😀\n");
            assert_eq!(
                doc.encoding(),
                if little {
                    TextEncoding::Utf16Le
                } else {
                    TextEncoding::Utf16Be
                }
            );
        }
    }

    #[test]
    fn invalid_encodings_and_binary_controls_are_rejected() {
        for bytes in [
            &[0xff][..],
            &[0xff, 0xfe, 1],
            &[0xff, 0xfe, 0, 0xd8],
            &[0xff, 0xfe, 0, 0],
            &[0, 0, 0xfe, 0xff],
        ] {
            assert!(matches!(
                TextDocument::from_bytes(bytes, Limits::default()),
                Err(Error::InvalidEncoding(_))
            ));
        }
        for bytes in [b"a\x00b".as_slice(), b"\x1b[31m"] {
            assert!(matches!(
                TextDocument::from_bytes(bytes, Limits::default()),
                Err(Error::InvalidText(_))
            ));
        }
    }

    #[test]
    fn source_and_decoded_limits_are_independent() {
        assert!(matches!(
            TextDocument::from_bytes(
                b"abcd",
                Limits {
                    max_source_bytes: 3,
                    max_decoded_bytes: 8
                }
            ),
            Err(Error::LimitExceeded { what: "source", .. })
        ));
        // Two UTF-16 CJK code units expand from four payload bytes to six UTF-8 bytes.
        assert!(matches!(
            TextDocument::from_bytes(
                &[0xff, 0xfe, 0x2d, 0x4e, 0x87, 0x65],
                Limits {
                    max_source_bytes: 6,
                    max_decoded_bytes: 5
                }
            ),
            Err(Error::LimitExceeded {
                what: "decoded text",
                ..
            })
        ));
    }

    #[test]
    fn locators_roundtrip_and_ignore_filesystem_paths() {
        let doc = document("a中文\n".as_bytes());
        let locator = doc.locator(4).unwrap();
        let restored: TextLocator = locator.to_string().parse().unwrap();
        assert_eq!(doc.restore(&restored).unwrap(), 4);
        assert_eq!(
            document("a中文\n".as_bytes()).restore(&restored).unwrap(),
            4
        );
        assert!(document(b"other").restore(&restored).is_err());
        assert!(doc.locator(2).is_err());
        assert!(doc.locator(doc.text().len() + 1).is_err());
        assert_eq!(
            doc.restore(&doc.locator(doc.text().len()).unwrap())
                .unwrap(),
            doc.text().len()
        );
    }

    #[test]
    fn locator_version_numbers_and_boundaries_are_checked() {
        let doc = document(b"abc");
        for text in [
            format!("txt-v2:{}:0", doc.id()),
            format!("txt-v1:{}:-1", doc.id()),
            format!("txt-v1:{}:+1", doc.id()),
            format!("txt-v1:{}:1:2", doc.id()),
            format!("txt-v1:{}:18446744073709551616", doc.id()),
        ] {
            assert!(text.parse::<TextLocator>().is_err());
        }
        let locator: TextLocator = format!("txt-v1:{}:99", doc.id()).parse().unwrap();
        assert!(doc.restore(&locator).is_err());
    }

    #[test]
    fn pdf_and_zip_are_explicitly_unsupported_not_lossy_text() {
        for bytes in [b"%PDF-1.7".as_slice(), b"PK\x03\x04"] {
            assert!(matches!(
                TextDocument::from_bytes(bytes, Limits::default()),
                Err(Error::UnsupportedFormat(_))
            ));
        }
    }

    #[test]
    fn empty_book_has_a_valid_start_locator() {
        let doc = document(b"");
        assert_eq!(doc.restore(&doc.locator(0).unwrap()).unwrap(), 0);
    }
}

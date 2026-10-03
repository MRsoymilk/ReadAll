use std::io;

use crate::Error;

/// Random access independent of paths, desktop files, or Android document URIs.
/// Implementations must not return a count greater than the destination length.
pub trait DocumentSource {
    fn length(&self) -> io::Result<u64>;
    fn read_at(&mut self, offset: u64, destination: &mut [u8]) -> io::Result<usize>;
}

#[derive(Debug)]
pub struct BytesSource<'a>(pub &'a [u8]);

impl DocumentSource for BytesSource<'_> {
    fn length(&self) -> io::Result<u64> {
        Ok(self.0.len() as u64)
    }

    fn read_at(&mut self, offset: u64, destination: &mut [u8]) -> io::Result<usize> {
        let Ok(offset) = usize::try_from(offset) else {
            return Ok(0);
        };
        let remaining = self.0.get(offset..).unwrap_or_default();
        let count = destination.len().min(remaining.len());
        destination[..count].copy_from_slice(&remaining[..count]);
        Ok(count)
    }
}

/// Reads at most `limit` bytes; never trusts an advertised length for allocation.
/// Detects truncation/growth, but does not promise an atomic filesystem snapshot.
pub fn read_bounded(source: &mut impl DocumentSource, limit: usize) -> Result<Vec<u8>, Error> {
    let advertised = source.length()?;
    let size = usize::try_from(advertised)
        .ok()
        .filter(|size| *size <= limit)
        .ok_or(Error::LimitExceeded {
            what: "source",
            limit,
        })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| Error::AllocationFailed)?;
    bytes.resize(size, 0);
    let mut offset = 0;
    while offset < size {
        let destination = &mut bytes[offset..size.min(offset.saturating_add(64 * 1024))];
        let count = match source.read_at(offset as u64, destination) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            return Err(Error::SourceChanged);
        }
        if count > destination.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "source returned an invalid read count",
            )
            .into());
        }
        offset += count;
    }
    let mut probe = [0];
    let extra = loop {
        match source.read_at(advertised, &mut probe) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => break result?,
        }
    };
    if extra != 0 || source.length()? != advertised {
        return Err(Error::SourceChanged);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_source_handles_offsets_and_eof() {
        let mut source = BytesSource(b"abc");
        let mut buffer = [0; 4];
        assert_eq!(source.read_at(1, &mut buffer).unwrap(), 2);
        assert_eq!(&buffer[..2], b"bc");
        assert_eq!(source.read_at(u64::MAX, &mut buffer).unwrap(), 0);
        assert_eq!(source.read_at(0, &mut []).unwrap(), 0);
    }

    #[test]
    fn exact_limit_and_empty_sources_are_allowed() {
        assert_eq!(read_bounded(&mut BytesSource(b"abc"), 3).unwrap(), b"abc");
        assert!(read_bounded(&mut BytesSource(b""), 0).unwrap().is_empty());
        assert!(matches!(
            read_bounded(&mut BytesSource(b"abc"), 2),
            Err(Error::LimitExceeded { .. })
        ));
    }

    struct Unreliable {
        data: &'static [u8],
        advertised: u64,
        interrupt: bool,
    }
    impl DocumentSource for Unreliable {
        fn length(&self) -> io::Result<u64> {
            Ok(self.advertised)
        }
        fn read_at(&mut self, offset: u64, destination: &mut [u8]) -> io::Result<usize> {
            if self.interrupt {
                self.interrupt = false;
                return Err(io::ErrorKind::Interrupted.into());
            }
            let length = destination.len().min(1);
            BytesSource(self.data).read_at(offset, &mut destination[..length])
        }
    }

    #[test]
    fn supports_short_reads_and_interrupted_reads() {
        let mut source = Unreliable {
            data: b"abc",
            advertised: 3,
            interrupt: true,
        };
        assert_eq!(read_bounded(&mut source, 3).unwrap(), b"abc");
    }

    #[test]
    fn detects_truncation_and_growth() {
        for advertised in [2, 4] {
            let mut source = Unreliable {
                data: b"abc",
                advertised,
                interrupt: false,
            };
            assert!(matches!(
                read_bounded(&mut source, 8),
                Err(Error::SourceChanged)
            ));
        }
    }

    #[test]
    fn rejects_huge_advertised_size_before_reading() {
        let mut source = Unreliable {
            data: b"",
            advertised: u64::MAX,
            interrupt: false,
        };
        assert!(matches!(
            read_bounded(&mut source, 8),
            Err(Error::LimitExceeded { .. })
        ));
    }
}

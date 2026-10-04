//! RFC 1950 wrapper over the existing bounded RFC 1951 decoder.
use crate::{ArchiveError, Result, deflate};

pub fn decode(input: &[u8], expected_size: usize, max_output: usize) -> Result<Vec<u8>> {
    if expected_size > max_output {
        return Err(ArchiveError::LimitExceeded("zlib output bytes"));
    }
    if input.len() < 6 {
        return Err(ArchiveError::Invalid("truncated zlib stream"));
    }
    let header = u16::from_be_bytes([input[0], input[1]]);
    if input[0] & 15 != 8 || input[0] >> 4 > 7 || !header.is_multiple_of(31) {
        return Err(ArchiveError::Invalid("invalid zlib header"));
    }
    if input[1] & 32 != 0 {
        return Err(ArchiveError::Unsupported("zlib preset dictionary"));
    }
    let end = input.len() - 4;
    let output = deflate::decode(&input[2..end], expected_size)?;
    let expected = u32::from_be_bytes(
        input[end..]
            .try_into()
            .map_err(|_| ArchiveError::Invalid("zlib checksum"))?,
    );
    if adler32(&output) != expected {
        return Err(ArchiveError::Invalid("zlib Adler-32 mismatch"));
    }
    Ok(output)
}
fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1_u32, 0_u32);
    for &byte in bytes {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    b << 16 | a
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stored_stream_checks_length_checksum_and_budget() {
        let mut bytes = vec![0x78, 0x01, 1, 3, 0, 0xfc, 0xff, b'a', b'b', b'c'];
        bytes.extend_from_slice(&adler32(b"abc").to_be_bytes());
        assert_eq!(decode(&bytes, 3, 3).unwrap(), b"abc");
        assert!(decode(&bytes, 3, 2).is_err());
        assert!(decode(&bytes, 4, 4).is_err());
        *bytes.last_mut().unwrap() ^= 1;
        assert!(decode(&bytes, 3, 3).is_err());
    }
    #[test]
    fn malformed_headers_and_truncated_input_fail() {
        for input in [
            b"".as_slice(),
            &[0, 0, 0, 0, 0, 0],
            &[0x78, 0x20, 0, 0, 0, 0],
        ] {
            assert!(decode(input, 0, 100).is_err());
        }
    }
}

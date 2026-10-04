use crate::{ArchiveError, Result, reserve};

struct Bits<'a> {
    bytes: &'a [u8],
    offset: usize,
    buffer: u64,
    count: u8,
}

impl<'a> Bits<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            offset: 0,
            buffer: 0,
            count: 0,
        }
    }

    fn read(&mut self, bits: u8) -> Result<u32> {
        if bits > 24 {
            return Err(ArchiveError::Invalid("excessive DEFLATE bit request"));
        }
        while self.count < bits {
            let byte = *self
                .bytes
                .get(self.offset)
                .ok_or(ArchiveError::Invalid("truncated DEFLATE stream"))?;
            self.buffer |= u64::from(byte) << self.count;
            self.offset += 1;
            self.count += 8;
        }
        let mask = if bits == 0 { 0 } else { (1_u64 << bits) - 1 };
        let value = (self.buffer & mask) as u32;
        self.buffer >>= bits;
        self.count -= bits;
        Ok(value)
    }

    fn align_byte(&mut self) {
        let discard = self.count % 8;
        self.buffer >>= discard;
        self.count -= discard;
    }

    fn loaded_bytes(&self) -> usize {
        self.offset
    }
}

#[derive(Debug)]
struct Huffman {
    by_len: Vec<Vec<(u16, u16)>>,
    max_len: usize,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self> {
        let mut count = [0_u16; 16];
        for &length in lengths {
            if length > 15 {
                return Err(ArchiveError::Invalid("DEFLATE Huffman code is too long"));
            }
            if length != 0 {
                count[usize::from(length)] = count[usize::from(length)]
                    .checked_add(1)
                    .ok_or(ArchiveError::Invalid("DEFLATE Huffman count overflow"))?;
            }
        }
        let max_len = (1..=15)
            .rev()
            .find(|&length| count[length] != 0)
            .ok_or(ArchiveError::Invalid("empty DEFLATE Huffman tree"))?;

        let mut left = 1_i32;
        for &codes in count.iter().skip(1) {
            left = (left << 1) - i32::from(codes);
            if left < 0 {
                return Err(ArchiveError::Invalid("oversubscribed DEFLATE Huffman tree"));
            }
        }

        let mut next = [0_u32; 16];
        let mut code = 0_u32;
        for bits in 1..=15 {
            code = (code + u32::from(count[bits - 1])) << 1;
            next[bits] = code;
        }

        let mut by_len = Vec::new();
        reserve(&mut by_len, 16)?;
        for _ in 0..16 {
            by_len.push(Vec::new());
        }
        for (symbol, &length) in lengths.iter().enumerate() {
            if length == 0 {
                continue;
            }
            let length = usize::from(length);
            let canonical = next[length];
            next[length] += 1;
            let reversed = reverse_bits(canonical, length as u8);
            let group = &mut by_len[length];
            reserve(group, 1)?;
            group.push((
                u16::try_from(reversed)
                    .map_err(|_| ArchiveError::Invalid("DEFLATE Huffman code overflow"))?,
                u16::try_from(symbol)
                    .map_err(|_| ArchiveError::Invalid("DEFLATE symbol overflow"))?,
            ));
        }
        for group in &mut by_len {
            group.sort_unstable_by_key(|pair| pair.0);
        }
        Ok(Self { by_len, max_len })
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16> {
        let mut code = 0_u16;
        for length in 1..=self.max_len {
            code |= (bits.read(1)? as u16) << (length - 1);
            let group = &self.by_len[length];
            if let Ok(index) = group.binary_search_by_key(&code, |pair| pair.0) {
                return Ok(group[index].1);
            }
        }
        Err(ArchiveError::Invalid("invalid DEFLATE Huffman code"))
    }
}

fn reverse_bits(mut code: u32, length: u8) -> u32 {
    let mut reversed = 0;
    for _ in 0..length {
        reversed = (reversed << 1) | (code & 1);
        code >>= 1;
    }
    reversed
}

pub(crate) fn decode(input: &[u8], expected_size: usize) -> Result<Vec<u8>> {
    decode_until(input, expected_size, None)
}

/// Unverified prefix for image header probes; full reads still verify size/CRC.
/// Limit compressed work as well as output so empty-block streams stay bounded.
pub(crate) fn decode_prefix(input: &[u8], expected_size: usize, prefix: usize) -> Result<Vec<u8>> {
    if prefix > 4096 {
        return Err(ArchiveError::LimitExceeded("DEFLATE probe bytes"));
    }
    decode_until(
        &input[..input.len().min(64 * 1024)],
        expected_size,
        Some(prefix.min(expected_size)),
    )
}

fn decode_until(input: &[u8], expected_size: usize, stop: Option<usize>) -> Result<Vec<u8>> {
    if stop == Some(0) {
        return Ok(Vec::new());
    }
    let mut bits = Bits::new(input);
    let mut output = Vec::new();
    output
        .try_reserve_exact(stop.unwrap_or(expected_size))
        .map_err(|_| ArchiveError::AllocationFailed)?;

    let mut blocks = 0;
    loop {
        blocks += 1;
        if stop.is_some() && blocks > 4096 {
            return Err(ArchiveError::LimitExceeded("DEFLATE probe blocks"));
        }
        let final_block = bits.read(1)? != 0;
        match bits.read(2)? {
            0 => stored(&mut bits, &mut output, expected_size, stop)?,
            1 => {
                let (literal, distance) = fixed_trees()?;
                compressed(
                    &mut bits,
                    &literal,
                    &distance,
                    &mut output,
                    expected_size,
                    stop,
                )?;
            }
            2 => {
                let (literal, distance) = dynamic_trees(&mut bits)?;
                compressed(
                    &mut bits,
                    &literal,
                    &distance,
                    &mut output,
                    expected_size,
                    stop,
                )?;
            }
            _ => return Err(ArchiveError::Invalid("reserved DEFLATE block type")),
        }
        if stop.is_some_and(|end| output.len() == end) {
            return Ok(output);
        }
        if final_block {
            break;
        }
    }

    if output.len() != expected_size {
        return Err(ArchiveError::Invalid(
            "DEFLATE output size does not match ZIP metadata",
        ));
    }
    if bits.loaded_bytes() != input.len() {
        return Err(ArchiveError::Invalid("trailing bytes after DEFLATE stream"));
    }
    Ok(output)
}

fn stored(
    bits: &mut Bits<'_>,
    output: &mut Vec<u8>,
    limit: usize,
    stop: Option<usize>,
) -> Result<()> {
    bits.align_byte();
    let length = bits.read(16)? as u16;
    let complement = bits.read(16)? as u16;
    if length ^ complement != 0xffff {
        return Err(ArchiveError::Invalid("invalid DEFLATE stored block length"));
    }
    let length = usize::from(length);
    let end = output
        .len()
        .checked_add(length)
        .filter(|end| *end <= limit)
        .ok_or(ArchiveError::LimitExceeded("DEFLATE output"))?;
    let end = end.min(stop.unwrap_or(end));
    output
        .try_reserve(end - output.len())
        .map_err(|_| ArchiveError::AllocationFailed)?;
    for _ in output.len()..end {
        output.push(bits.read(8)? as u8);
    }
    Ok(())
}

fn fixed_trees() -> Result<(Huffman, Huffman)> {
    let mut literal = vec![0_u8; 288];
    literal[..144].fill(8);
    literal[144..256].fill(9);
    literal[256..280].fill(7);
    literal[280..].fill(8);
    let distance = vec![5_u8; 32];
    Ok((Huffman::new(&literal)?, Huffman::new(&distance)?))
}

fn dynamic_trees(bits: &mut Bits<'_>) -> Result<(Huffman, Huffman)> {
    let literal_count = bits.read(5)? as usize + 257;
    let distance_count = bits.read(5)? as usize + 1;
    let code_count = bits.read(4)? as usize + 4;
    if literal_count > 286 || distance_count > 32 {
        return Err(ArchiveError::Invalid("invalid DEFLATE dynamic tree counts"));
    }

    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut code_lengths = [0_u8; 19];
    for &symbol in ORDER.iter().take(code_count) {
        code_lengths[symbol] = bits.read(3)? as u8;
    }
    let code_tree = Huffman::new(&code_lengths)?;

    let total = literal_count + distance_count;
    let mut lengths = Vec::new();
    reserve(&mut lengths, total)?;
    while lengths.len() < total {
        let symbol = code_tree.decode(bits)?;
        match symbol {
            0..=15 => lengths.push(symbol as u8),
            16 => {
                let previous = *lengths.last().ok_or(ArchiveError::Invalid(
                    "DEFLATE repeat without previous code",
                ))?;
                repeat_length(&mut lengths, previous, bits.read(2)? as usize + 3, total)?;
            }
            17 => repeat_length(&mut lengths, 0, bits.read(3)? as usize + 3, total)?,
            18 => repeat_length(&mut lengths, 0, bits.read(7)? as usize + 11, total)?,
            _ => return Err(ArchiveError::Invalid("invalid DEFLATE code-length symbol")),
        }
    }
    if lengths[256] == 0 {
        return Err(ArchiveError::Invalid(
            "DEFLATE literal tree has no end-of-block code",
        ));
    }
    let literal = Huffman::new(&lengths[..literal_count])?;
    let distance = Huffman::new(&lengths[literal_count..])?;
    Ok((literal, distance))
}

fn repeat_length(lengths: &mut Vec<u8>, value: u8, count: usize, total: usize) -> Result<()> {
    if count > total.saturating_sub(lengths.len()) {
        return Err(ArchiveError::Invalid(
            "DEFLATE code-length repeat exceeds tree size",
        ));
    }
    lengths
        .try_reserve(count)
        .map_err(|_| ArchiveError::AllocationFailed)?;
    lengths.resize(lengths.len() + count, value);
    Ok(())
}

fn compressed(
    bits: &mut Bits<'_>,
    literal: &Huffman,
    distance: &Huffman,
    output: &mut Vec<u8>,
    limit: usize,
    stop: Option<usize>,
) -> Result<()> {
    const LENGTH_BASE: [usize; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const LENGTH_EXTRA: [u8; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DISTANCE_BASE: [usize; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DISTANCE_EXTRA: [u8; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];

    loop {
        if stop.is_some_and(|end| output.len() == end) {
            return Ok(());
        }
        let symbol = literal.decode(bits)?;
        match symbol {
            0..=255 => {
                if output.len() >= limit {
                    return Err(ArchiveError::LimitExceeded("DEFLATE output"));
                }
                output.push(symbol as u8);
            }
            256 => return Ok(()),
            257..=285 => {
                let index = usize::from(symbol - 257);
                let length = LENGTH_BASE[index] + bits.read(LENGTH_EXTRA[index])? as usize;
                let distance_symbol = usize::from(distance.decode(bits)?);
                if distance_symbol >= DISTANCE_BASE.len() {
                    return Err(ArchiveError::Invalid("reserved DEFLATE distance code"));
                }
                let distance = DISTANCE_BASE[distance_symbol]
                    + bits.read(DISTANCE_EXTRA[distance_symbol])? as usize;
                if distance == 0 || distance > output.len() {
                    return Err(ArchiveError::Invalid(
                        "invalid DEFLATE back-reference distance",
                    ));
                }
                let end = output
                    .len()
                    .checked_add(length)
                    .filter(|end| *end <= limit)
                    .ok_or(ArchiveError::LimitExceeded("DEFLATE output"))?;
                let end = end.min(stop.unwrap_or(end));
                output
                    .try_reserve(end - output.len())
                    .map_err(|_| ArchiveError::AllocationFailed)?;
                while output.len() < end {
                    let byte = output[output.len() - distance];
                    output.push(byte);
                }
            }
            _ => {
                return Err(ArchiveError::Invalid(
                    "reserved DEFLATE literal/length code",
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_probes_stop_before_later_data_and_bound_work() {
        let stored = b"\x01\x05\x00\xfa\xffhello";
        let fixed = [0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x90, 0x00];
        for (data, expected) in [
            (stored.as_slice(), b"hello".as_slice()),
            (&fixed, b"hello hello hello"),
        ] {
            for length in 0..=expected.len() {
                assert_eq!(
                    decode_prefix(data, expected.len(), length).unwrap(),
                    &expected[..length]
                );
            }
        }
        assert_eq!(decode_prefix(&stored[..8], 5, 3).unwrap(), b"hel");
        assert!(decode(&stored[..8], 5).is_err());
        assert!(decode_prefix(stored, 5, 4097).is_err());
        let empty_blocks = b"\x00\x00\x00\xff\xff".repeat(4097);
        assert!(decode_prefix(&empty_blocks, 1, 1).is_err());
    }

    #[test]
    fn stored_and_fixed_streams_decode() {
        let stored = b"\x01\x05\x00\xfa\xffhello";
        assert_eq!(decode(stored, 5).unwrap(), b"hello");

        // Raw DEFLATE produced for "hello hello hello"; the stream uses fixed codes.
        let fixed = [0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x90, 0x00];
        assert_eq!(decode(&fixed, 17).unwrap(), b"hello hello hello");
    }

    #[test]
    fn dynamic_huffman_stream_decodes() {
        let compressed = [
            0xed, 0xc8, 0xd1, 0x15, 0x80, 0x10, 0x00, 0x00, 0xc0, 0x95, 0x10, 0xd1, 0x38, 0x88,
            0xf6, 0xdf, 0xc0, 0x1e, 0xbd, 0xbb, 0xcf, 0xeb, 0x63, 0xbe, 0x6b, 0x7f, 0x31, 0x5d,
            0xb9, 0xdc, 0xb5, 0x3d, 0xa1, 0x0b, 0x21, 0x84, 0x10, 0x42, 0x08, 0x21, 0x84, 0x10,
            0x42, 0x08, 0x21, 0x84, 0x10, 0x42, 0x08, 0x21, 0x84, 0x10, 0x42, 0x08, 0x21, 0x84,
            0x10, 0x42, 0x08, 0x21, 0x84, 0x10, 0x42, 0x08, 0x21, 0x84, 0x10, 0x42, 0x08, 0x21,
            0x84, 0x10, 0x42, 0x88, 0x3f, 0xc6, 0x01,
        ];
        let plain = b"abcdefg1234567890".repeat(1000);
        assert_eq!(decode(&compressed, plain.len()).unwrap(), plain);
        assert_eq!(
            decode_prefix(&compressed, plain.len(), 33).unwrap(),
            &plain[..33]
        );
    }

    #[test]
    fn malformed_streams_are_bounded_and_rejected() {
        for bytes in [
            b"".as_slice(),
            b"\x07",
            b"\x01\x01\x00\x00\x00A",
            b"\x01\xff\xff\x00\x00",
        ] {
            assert!(decode(bytes, 1).is_err());
        }
        assert!(decode(b"\x01\x05\x00\xfa\xffhello", 4).is_err());
        assert!(decode(b"\x01\x05\x00\xfa\xffhello\x00", 5).is_err());
    }
}

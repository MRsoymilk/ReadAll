//! Original GIF fixtures. The literal LZW stream is assembled independently of gif's encoder.
#![allow(dead_code)]
const COLORS: [u8; 12] = [0, 0, 0, 231, 43, 71, 19, 167, 83, 37, 89, 211];
pub fn solid() -> Vec<u8> {
    image(8, 8, (0, 0, 8, 8), &[1; 64], false, false, None)
}
pub fn image(
    width: u16,
    height: u16,
    frame: (u16, u16, u16, u16),
    pixels: &[u8],
    interlaced: bool,
    local: bool,
    transparent: Option<u8>,
) -> Vec<u8> {
    let mut bytes = b"GIF89a".to_vec();
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&[if local { 0 } else { 0x81 }, 0, 0]);
    if !local {
        bytes.extend_from_slice(&COLORS);
    }
    if let Some(index) = transparent {
        bytes.extend_from_slice(&[0x21, 0xf9, 4, 1, 0, 0, index, 0]);
    }
    append_frame(&mut bytes, frame, pixels, interlaced, local);
    bytes.push(0x3b);
    bytes
}
pub fn append_frame(
    bytes: &mut Vec<u8>,
    frame: (u16, u16, u16, u16),
    pixels: &[u8],
    interlaced: bool,
    local: bool,
) {
    let (left, top, width, height) = frame;
    assert_eq!(pixels.len(), usize::from(width) * usize::from(height));
    bytes.push(0x2c);
    for value in [left, top, width, height] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(if interlaced { 0x40 } else { 0 } | if local { 0x81 } else { 0 });
    if local {
        bytes.extend_from_slice(&COLORS);
    }
    let order: Vec<_> = if interlaced {
        [(0, 8), (4, 8), (2, 4), (1, 2)]
            .iter()
            .flat_map(|&(start, step)| (start..usize::from(height)).step_by(step))
            .collect()
    } else {
        (0..usize::from(height)).collect()
    };
    // Reset before each literal: code width remains three bits, with clear=4 and end=5.
    let mut codes = Vec::new();
    for y in order {
        for &pixel in &pixels[y * usize::from(width)..(y + 1) * usize::from(width)] {
            codes.extend_from_slice(&[4_u8, pixel]);
        }
    }
    codes.push(5);
    let mut compressed = vec![0_u8; (codes.len() * 3).div_ceil(8)];
    for (i, &code) in codes.iter().enumerate() {
        for bit in 0..3 {
            if code & (1 << bit) != 0 {
                let at = i * 3 + bit;
                compressed[at / 8] |= 1 << (at % 8);
            }
        }
    }
    bytes.push(2);
    for chunk in compressed.chunks(255) {
        bytes.push(chunk.len() as u8);
        bytes.extend_from_slice(chunk);
    }
    bytes.push(0);
}

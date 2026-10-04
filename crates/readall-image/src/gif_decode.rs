//! GIF87a/89a first-frame presentation on the logical canvas. No animation loop.
//! Probe dimensions before allocation; validate framing without decoding later frames.
use crate::{ImageError, ImageFormat, ImageInfo, ImageLimits, Result, RgbaImage};
use std::{io::Cursor, num::NonZeroU64};

pub(crate) fn header(bytes: &[u8], full_len: usize, limits: ImageLimits) -> Result<ImageInfo> {
    if full_len > limits.max_file_bytes {
        return Err(ImageError::Budget);
    }
    if !bytes.starts_with(b"GIF87a") && !bytes.starts_with(b"GIF89a") {
        return Err(ImageError::Unsupported(
            "expected a GIF87a or GIF89a signature",
        ));
    }
    let h = bytes
        .get(..13)
        .ok_or(ImageError::Invalid("truncated GIF screen descriptor"))?;
    ImageInfo {
        width: u32::from(le16(&h[6..8])),
        height: u32::from(le16(&h[8..10])),
        format: ImageFormat::Gif,
    }
    .checked(limits)
}
fn le16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}
fn take<'a>(bytes: &'a [u8], at: &mut usize, len: usize) -> Result<&'a [u8]> {
    let end = at
        .checked_add(len)
        .ok_or(ImageError::Invalid("GIF block offset overflow"))?;
    let part = bytes
        .get(*at..end)
        .ok_or(ImageError::Invalid("truncated GIF block"))?;
    *at = end;
    Ok(part)
}
fn palette_len(flags: u8) -> usize {
    if flags & 128 == 0 {
        0
    } else {
        3 * (1 << ((flags & 7) + 1))
    }
}
fn subblocks(bytes: &[u8], at: &mut usize, work: &mut usize) -> Result<()> {
    loop {
        *work += 1;
        if *work > 131_072 {
            return Err(ImageError::Budget);
        }
        let len = usize::from(take(bytes, at, 1)?[0]);
        if len == 0 {
            return Ok(());
        }
        take(bytes, at, len)?;
    }
}
// Structural validation is linear in encoded bytes and never expands later frames.
fn validate(bytes: &[u8], info: ImageInfo) -> Result<()> {
    let mut at = 13;
    let global = palette_len(bytes[10]);
    take(bytes, &mut at, global)?;
    let (mut blocks, mut frames, mut work) = (0, 0, 0);
    loop {
        blocks += 1;
        if blocks > 8192 {
            return Err(ImageError::Budget);
        }
        match take(bytes, &mut at, 1)?[0] {
            0x3b => {
                if frames == 0 {
                    return Err(ImageError::Invalid("GIF contains no image frame"));
                }
                // A few writers pad record resources with zero bytes. No second stream.
                if bytes[at..].len() > 3 || bytes[at..].iter().any(|b| *b != 0) {
                    return Err(ImageError::Invalid("data after GIF trailer"));
                }
                return Ok(());
            }
            0x21 => {
                let kind = take(bytes, &mut at, 1)?[0];
                if kind == 0xf9 {
                    let data = take(bytes, &mut at, 6)?;
                    if data[0] != 4 || data[5] != 0 {
                        return Err(ImageError::Invalid("GIF graphic control extension"));
                    }
                } else {
                    subblocks(bytes, &mut at, &mut work)?;
                }
            }
            0x2c => {
                frames += 1;
                if frames > 4096 {
                    return Err(ImageError::Budget);
                }
                let d = take(bytes, &mut at, 9)?;
                let (x, y, w, h) = (
                    u32::from(le16(&d[..2])),
                    u32::from(le16(&d[2..4])),
                    u32::from(le16(&d[4..6])),
                    u32::from(le16(&d[6..8])),
                );
                if w == 0 || h == 0 || x + w > info.width || y + h > info.height {
                    return Err(ImageError::Invalid("GIF frame outside logical canvas"));
                }
                let local = palette_len(d[8]);
                if global == 0 && local == 0 {
                    return Err(ImageError::Invalid("GIF frame has no color table"));
                }
                take(bytes, &mut at, local)?;
                if !(2..=8).contains(&take(bytes, &mut at, 1)?[0]) {
                    return Err(ImageError::Invalid("GIF LZW minimum code size"));
                }
                subblocks(bytes, &mut at, &mut work)?;
            }
            _ => return Err(ImageError::Invalid("unknown GIF block")),
        }
    }
}

/// Decode only the first image. Frame offsets and interlacing are honored; transparent
/// indices and uncovered canvas pixels stay transparent for the reader's paper theme.
pub fn decode_gif(bytes: &[u8], limits: ImageLimits) -> Result<RgbaImage> {
    let info = header(bytes, bytes.len(), limits)?;
    validate(bytes, info)?;
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    options.set_memory_limit(gif::MemoryLimit::Bytes(
        NonZeroU64::new(limits.max_decoded_bytes as u64).ok_or(ImageError::Budget)?,
    ));
    options.check_frame_consistency(true);
    options.check_lzw_end_code(true);
    let mut decoder = options
        .read_info(Cursor::new(bytes))
        .map_err(ImageError::Gif)?;
    if (u32::from(decoder.width()), u32::from(decoder.height())) != (info.width, info.height) {
        return Err(ImageError::Invalid(
            "GIF decoder/header dimensions disagree",
        ));
    }
    let global = decoder.global_palette().map(<[u8]>::to_vec);
    let frame = decoder
        .read_next_frame()
        .map_err(ImageError::Gif)?
        .ok_or(ImageError::Invalid("GIF contains no image frame"))?;
    let palette = frame
        .palette
        .as_deref()
        .or(global.as_deref())
        .ok_or(ImageError::Invalid("GIF color table missing"))?;
    let count = usize::from(frame.width) * usize::from(frame.height);
    if frame.buffer.len() != count {
        return Err(ImageError::Invalid("GIF decoded frame length"));
    }
    let size = info.rgba_bytes()?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(size)
        .map_err(|_| ImageError::Allocation)?;
    pixels.resize(size, 0);
    for (y, row) in frame
        .buffer
        .chunks_exact(usize::from(frame.width))
        .enumerate()
    {
        let start =
            ((y + usize::from(frame.top)) * info.width as usize + usize::from(frame.left)) * 4;
        for (x, &index) in row.iter().enumerate() {
            if frame.transparent == Some(index) {
                continue;
            }
            let p = usize::from(index) * 3;
            let rgb = palette
                .get(p..p + 3)
                .ok_or(ImageError::Invalid("GIF palette index out of range"))?;
            pixels[start + x * 4..start + x * 4 + 4]
                .copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    Ok(RgbaImage {
        width: info.width,
        height: info.height,
        pixels,
    })
}

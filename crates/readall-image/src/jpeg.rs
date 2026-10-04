//! JPEG header scanning and isolated pure-Rust pixel decoding.
use crate::{ImageError, ImageFormat, ImageInfo, ImageLimits, Result, RgbaImage};
use jpeg_decoder::{Decoder, PixelFormat};
use std::io::Cursor;
#[cfg(test)]
mod fixtures;

/// Scan bounded marker segments, not entropy-coded pixels. A short prefix can
/// request a larger read; corrupt complete resources never silently succeed.
pub(crate) fn header(
    bytes: &[u8],
    full_len: usize,
    limits: ImageLimits,
) -> Result<(ImageInfo, u8)> {
    if full_len > limits.max_file_bytes {
        return Err(ImageError::Budget);
    }
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(ImageError::Invalid("JPEG SOI"));
    }
    let incomplete = || {
        if bytes.len() < full_len {
            ImageError::IncompleteHeader
        } else {
            ImageError::Invalid("truncated JPEG header")
        }
    };
    let (mut at, mut orientation, mut dimensions) = (2_usize, 1_u8, None);
    for _ in 0..4096 {
        if bytes.get(at) != Some(&0xff) {
            return Err(if at >= bytes.len() {
                incomplete()
            } else {
                ImageError::Invalid("JPEG marker")
            });
        }
        while bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let marker = *bytes.get(at).ok_or_else(incomplete)?;
        at += 1;
        if marker == 0xda {
            // SOS: all pre-scan metadata is now available.
            let (width, height) =
                dimensions.ok_or(ImageError::Invalid("JPEG scan before frame"))?;
            let (width, height) = if orientation >= 5 {
                (height, width)
            } else {
                (width, height)
            };
            return Ok((
                ImageInfo {
                    width,
                    height,
                    format: ImageFormat::Jpeg,
                }
                .checked(limits)?,
                orientation,
            ));
        }
        if matches!(marker, 0xd8 | 0xd9 | 0x00) {
            return Err(ImageError::Invalid("unexpected JPEG marker"));
        }
        if matches!(marker, 0x01 | 0xd0..=0xd7) {
            continue;
        }
        let size = bytes.get(at..at + 2).ok_or_else(incomplete)?;
        let size = usize::from(u16::from_be_bytes([size[0], size[1]]));
        if size < 2 {
            return Err(ImageError::Invalid("JPEG segment length"));
        }
        let end = at
            .checked_add(size)
            .filter(|end| *end <= full_len)
            .ok_or(ImageError::Invalid("JPEG segment outside resource"))?;
        let data = bytes.get(at + 2..end).ok_or_else(incomplete)?;
        if matches!(marker,0xc0..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) {
            let h = data.get(..6).ok_or(ImageError::Invalid("JPEG SOF"))?;
            let height = u32::from(u16::from_be_bytes([h[1], h[2]]));
            let width = u32::from(u16::from_be_bytes([h[3], h[4]]));
            ImageInfo {
                width,
                height,
                format: ImageFormat::Jpeg,
            }
            .checked(limits)?;
            if dimensions.replace((width, height)).is_some() {
                return Err(ImageError::Invalid("multiple JPEG frames"));
            }
        }
        if marker == 0xe1 && data.starts_with(b"Exif\0\0") {
            orientation = exif_orientation(&data[6..]).unwrap_or(1);
        }
        at = end;
    }
    Err(ImageError::Budget)
}

fn exif_orientation(tiff: &[u8]) -> Option<u8> {
    let little = match tiff.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |at: usize| -> Option<u16> {
        let s: [u8; 2] = tiff.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(s)
        } else {
            u16::from_be_bytes(s)
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let s: [u8; 4] = tiff.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(s)
        } else {
            u32::from_be_bytes(s)
        })
    };
    if u16_at(2)? != 42 {
        return None;
    }
    let start = usize::try_from(u32_at(4)?).ok()?;
    let count = usize::from(u16_at(start)?).min(4096);
    for index in 0..count {
        let at = start.checked_add(2)?.checked_add(index * 12)?;
        if u16_at(at)? == 0x112 && u16_at(at + 2)? == 3 && u32_at(at + 4)? == 1 {
            let value = u16_at(at + 8)?;
            return (1..=8).contains(&value).then_some(value as u8);
        }
    }
    None
}

pub fn decode_jpeg(bytes: &[u8], limits: ImageLimits) -> Result<RgbaImage> {
    let (info, orientation) = header(bytes, bytes.len(), limits)?;
    let mut decoder = Decoder::new(Cursor::new(bytes));
    decoder.set_max_decoding_buffer_size(limits.max_decoded_bytes);
    decoder.read_info().map_err(ImageError::Jpeg)?;
    let meta = decoder
        .info()
        .ok_or(ImageError::Invalid("JPEG metadata missing"))?;
    let (w, h) = (u32::from(meta.width), u32::from(meta.height));
    let expected = if orientation >= 5 { (h, w) } else { (w, h) };
    if expected != (info.width, info.height) {
        return Err(ImageError::Invalid("JPEG dimensions disagree"));
    }
    let decoded = decoder.decode().map_err(ImageError::Jpeg)?;
    let channels = meta.pixel_format.pixel_bytes();
    if decoded.len() != w as usize * h as usize * channels {
        return Err(ImageError::Invalid("JPEG pixel length"));
    }
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(info.rgba_bytes()?)
        .map_err(|_| ImageError::Allocation)?;
    pixels.resize(info.rgba_bytes()?, 0);
    for y in 0..h {
        for x in 0..w {
            let at = (y as usize * w as usize + x as usize) * channels;
            let p = &decoded[at..at + channels];
            let color = match meta.pixel_format {
                PixelFormat::L8 => [p[0], p[0], p[0], 255],
                PixelFormat::L16 => {
                    let v = (u16::from_ne_bytes([p[0], p[1]]) >> 8) as u8;
                    [v, v, v, 255]
                }
                PixelFormat::RGB24 => [p[0], p[1], p[2], 255],
                PixelFormat::CMYK32 => {
                    let k = 255 - u16::from(p[3]);
                    let c = |v: u8| (((255 - u16::from(v)) * k + 127) / 255) as u8;
                    [c(p[0]), c(p[1]), c(p[2]), 255]
                }
            };
            let (dx, dy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (h - 1 - y, x),
                7 => (h - 1 - y, w - 1 - x),
                8 => (y, w - 1 - x),
                _ => (x, y),
            };
            let dst = (dy as usize * info.width as usize + dx as usize) * 4;
            pixels[dst..dst + 4].copy_from_slice(&color);
        }
    }
    Ok(RgbaImage {
        width: info.width,
        height: info.height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_rejects_invalid_dimensions_and_requests_larger_prefix() {
        let bytes = [
            255, 216, 255, 192, 0, 11, 8, 0, 3, 0, 4, 1, 1, 17, 0, 255, 218,
        ];
        let (info, _) = header(&bytes, bytes.len(), ImageLimits::default()).unwrap();
        assert_eq!((info.width, info.height), (4, 3));
        assert!(matches!(
            header(&bytes[..9], bytes.len(), ImageLimits::default()),
            Err(ImageError::IncompleteHeader)
        ));
        assert!(
            header(
                &bytes,
                bytes.len(),
                ImageLimits {
                    max_pixels: 11,
                    ..ImageLimits::default()
                }
            )
            .is_err()
        );
        for end in 0..bytes.len() {
            assert!(header(&bytes[..end], end, ImageLimits::default()).is_err());
        }
    }
    #[test]
    fn exif_is_bounded_and_reads_both_endian_orders() {
        let le = b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0";
        assert_eq!(exif_orientation(le), Some(6));
        assert_eq!(
            exif_orientation(b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x08\0\0"),
            Some(8)
        );
        for n in 0..20 {
            assert_eq!(exif_orientation(&le[..n]), None);
        }
    }
}

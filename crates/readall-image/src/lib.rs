//! Bounded image dispatch: native ReadAll PNG and an isolated pure-Rust WebP codec.
//! Header probes never decode pixels. No system image library or color management.
#[cfg(test)]
mod adam7_tests;
mod format;
mod gif_decode;
#[cfg(test)]
mod gif_tests;
pub use gif_decode::decode_gif;
mod jpeg;
mod svg;
pub use jpeg::decode_jpeg;
pub use svg::{decode_svg_with_resource_loader, decode_svg_with_resources};
mod webp;
pub use format::{ImageFormat, ImageInfo, decode, probe};
use readall_archive::{crc32, zlib};
use std::{error::Error, fmt};
pub use webp::decode_webp;

#[derive(Debug, Clone, Copy)]
pub struct ImageLimits {
    pub max_file_bytes: usize,
    pub max_pixels: usize,
    pub max_decoded_bytes: usize,
}
impl Default for ImageLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 16 * 1024 * 1024,
            max_pixels: 8 * 1024 * 1024,
            max_decoded_bytes: 64 * 1024 * 1024,
        }
    }
}
#[derive(Debug)]
pub enum ImageError {
    Invalid(&'static str),
    Unsupported(&'static str),
    Budget,
    Allocation,
    Deflate(readall_archive::ArchiveError),
    WebP(image_webp::DecodingError),
    Jpeg(jpeg_decoder::Error),
    Gif(gif::DecodingError),
    SvgResource { reference: String, reason: String },
    IncompleteHeader,
}
impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(s) => write!(f, "invalid image: {s}"),
            Self::Unsupported(s) => write!(f, "unsupported image: {s}"),
            Self::Budget => f.write_str("image resource budget exceeded"),
            Self::Allocation => f.write_str("image allocation failed"),
            Self::Deflate(e) => write!(f, "PNG zlib decode: {e}"),
            Self::WebP(e) => write!(f, "WebP decode: {e}"),
            Self::Jpeg(e) => write!(f, "JPEG decode: {e}"),
            Self::Gif(e) => write!(f, "GIF decode: {e}"),
            Self::SvgResource { reference, reason } => {
                write!(f, "SVG resource {reference:?}: {reason}")
            }
            Self::IncompleteHeader => f.write_str("image header requires a larger bounded read"),
        }
    }
}
impl Error for ImageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Deflate(error) => Some(error),
            Self::WebP(error) => Some(error),
            Self::Jpeg(error) => Some(error),
            Self::Gif(error) => Some(error),
            _ => None,
        }
    }
}
type Result<T> = std::result::Result<T, ImageError>;
#[derive(Debug, Clone)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
impl RgbaImage {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}
fn be32(bytes: &[u8]) -> Result<u32> {
    Ok(u32::from_be_bytes(
        bytes
            .try_into()
            .map_err(|_| ImageError::Invalid("truncated integer"))?,
    ))
}
fn extend(vec: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    vec.try_reserve(bytes.len())
        .map_err(|_| ImageError::Allocation)?;
    vec.extend_from_slice(bytes);
    Ok(())
}

pub fn decode_png(bytes: &[u8], limits: ImageLimits) -> Result<RgbaImage> {
    if bytes.len() > limits.max_file_bytes {
        return Err(ImageError::Budget);
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(ImageError::Unsupported("expected a PNG signature"));
    }
    let mut at = 8_usize;
    let mut header: Option<[u8; 13]> = None;
    let (mut palette, mut transparency, mut compressed) = (Vec::new(), Vec::new(), Vec::new());
    let (mut seen_palette, mut seen_transparency, mut seen_idat, mut idat_closed, mut ended) =
        (false, false, false, false, false);
    let mut chunks = 0;
    while at < bytes.len() {
        chunks += 1;
        if chunks > 8192 {
            return Err(ImageError::Budget);
        }
        let length = be32(
            bytes
                .get(at..at + 4)
                .ok_or(ImageError::Invalid("truncated chunk"))?,
        )? as usize;
        let end = at
            .checked_add(12)
            .and_then(|n| n.checked_add(length))
            .filter(|n| *n <= bytes.len())
            .ok_or(ImageError::Invalid("chunk length"))?;
        let kind = &bytes[at + 4..at + 8];
        let data = &bytes[at + 8..end - 4];
        if !kind.iter().all(u8::is_ascii_alphabetic) || kind[2] & 32 != 0 {
            return Err(ImageError::Invalid("chunk type"));
        }
        if crc32(&bytes[at + 4..end - 4]) != be32(&bytes[end - 4..end])? {
            return Err(ImageError::Invalid("chunk CRC-32 mismatch"));
        }
        if header.is_none() && kind != b"IHDR" {
            return Err(ImageError::Invalid("IHDR must be first"));
        }
        if seen_idat && kind != b"IDAT" {
            idat_closed = true;
        }
        match kind {
            b"IHDR" => {
                if header.is_some() || data.len() != 13 {
                    return Err(ImageError::Invalid("duplicate or malformed IHDR"));
                }
                let h: [u8; 13] = data.try_into().map_err(|_| ImageError::Invalid("IHDR"))?;
                let (width, height) = (be32(&h[..4])?, be32(&h[4..8])?);
                if width == 0 || height == 0 || width > 16384 || height > 16384 {
                    return Err(ImageError::Invalid("dimensions"));
                }
                if u64::from(width) * u64::from(height) > limits.max_pixels as u64 {
                    return Err(ImageError::Budget);
                }
                let valid = match h[9] {
                    0 => matches!(h[8], 1 | 2 | 4 | 8 | 16),
                    3 => matches!(h[8], 1 | 2 | 4 | 8),
                    2 | 4 | 6 => matches!(h[8], 8 | 16),
                    _ => false,
                };
                if !valid || h[10] != 0 || h[11] != 0 {
                    return Err(ImageError::Invalid(
                        "color type, bit depth or compression/filter method",
                    ));
                }
                if h[12] > 1 {
                    return Err(ImageError::Invalid("PNG interlace method"));
                }
                header = Some(h);
            }
            b"PLTE" => {
                let h = header.as_ref().ok_or(ImageError::Invalid("missing IHDR"))?;
                if seen_palette
                    || seen_idat
                    || data.is_empty()
                    || !data.len().is_multiple_of(3)
                    || data.len() > 768
                    || matches!(h[9], 0 | 4)
                    || (h[9] == 3 && data.len() / 3 > 1 << h[8])
                {
                    return Err(ImageError::Invalid("PLTE"));
                }
                seen_palette = true;
                extend(&mut palette, data)?;
            }
            b"tRNS" => {
                let h = header.as_ref().ok_or(ImageError::Invalid("missing IHDR"))?;
                let valid = match h[9] {
                    0 => data.len() == 2,
                    2 => data.len() == 6,
                    3 => seen_palette && !data.is_empty() && data.len() <= palette.len() / 3,
                    _ => false,
                };
                if seen_transparency || seen_idat || !valid {
                    return Err(ImageError::Invalid("tRNS"));
                }
                seen_transparency = true;
                extend(&mut transparency, data)?;
            }
            b"IDAT" => {
                if idat_closed {
                    return Err(ImageError::Invalid("nonconsecutive IDAT"));
                }
                seen_idat = true;
                extend(&mut compressed, data)?;
            }
            b"IEND" => {
                if !data.is_empty() || !seen_idat || end != bytes.len() {
                    return Err(ImageError::Invalid("IEND or trailing data"));
                }
                ended = true;
                break;
            }
            _ if kind[0] & 32 == 0 => {
                return Err(ImageError::Unsupported("unknown critical PNG chunk"));
            }
            _ => {}
        }
        at = end;
    }
    if !ended {
        return Err(ImageError::Invalid("missing IEND"));
    }
    let h = header.ok_or(ImageError::Invalid("missing IHDR"))?;
    let (width, height, depth, kind) = (be32(&h[..4])?, be32(&h[4..8])?, h[8] as usize, h[9]);
    if kind == 3 && !seen_palette {
        return Err(ImageError::Invalid("indexed image without PLTE"));
    }
    let channels = match kind {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err(ImageError::Invalid("color type")),
    };
    let patterns: &[(usize, usize, usize, usize)] = if h[12] == 0 {
        &[(0, 0, 1, 1)]
    } else {
        &[
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ]
    };
    let mut passes = Vec::new();
    let mut expected = 0_usize;
    for &(x0, y0, dx, dy) in patterns {
        let pw = (width as usize).saturating_sub(x0).div_ceil(dx);
        let ph = (height as usize).saturating_sub(y0).div_ceil(dy);
        if pw == 0 || ph == 0 {
            continue;
        }
        let stride = (pw * channels * depth).div_ceil(8) + 1;
        expected = expected
            .checked_add(stride.checked_mul(ph).ok_or(ImageError::Budget)?)
            .ok_or(ImageError::Budget)?;
        passes.push((x0, y0, dx, dy, pw, ph, stride));
    }
    let rgba_bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(ImageError::Budget)?;
    if expected > limits.max_decoded_bytes || rgba_bytes > limits.max_decoded_bytes {
        return Err(ImageError::Budget);
    }
    let mut raw = zlib::decode(&compressed, expected, limits.max_decoded_bytes)
        .map_err(ImageError::Deflate)?;
    let bpp = (channels * depth).div_ceil(8);
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(rgba_bytes)
        .map_err(|_| ImageError::Allocation)?;
    pixels.resize(rgba_bytes, 0);
    let maximum = (1_u32 << depth) - 1;
    let scale = |sample: u16| ((u32::from(sample) * 255 + maximum / 2) / maximum) as u8;
    let transparent_sample = |index: usize| -> Option<u16> {
        let value = transparency.get(index * 2..index * 2 + 2)?;
        Some(u16::from_be_bytes([value[0], value[1]]) & maximum as u16)
    };
    let mut offset = 0;
    for (x0, y0, dx, dy, pw, ph, stride) in passes {
        let data = &mut raw[offset..offset + stride * ph];
        unfilter(data, stride, ph, bpp)?;
        for y in 0..ph {
            let row = &data[y * stride + 1..(y + 1) * stride];
            for x in 0..pw {
                let sample = |channel| sample(row, x * channels + channel, depth);
                let pixel = match kind {
                    0 => {
                        let v = sample(0);
                        let a = if transparent_sample(0) == Some(v) {
                            0
                        } else {
                            255
                        };
                        [scale(v), scale(v), scale(v), a]
                    }
                    2 => {
                        let rgb = [sample(0), sample(1), sample(2)];
                        let alpha = if (0..3).all(|i| transparent_sample(i) == Some(rgb[i])) {
                            0
                        } else {
                            255
                        };
                        [scale(rgb[0]), scale(rgb[1]), scale(rgb[2]), alpha]
                    }
                    3 => {
                        let index = sample(0) as usize;
                        let rgb = palette
                            .get(index * 3..index * 3 + 3)
                            .ok_or(ImageError::Invalid("palette index"))?;
                        [
                            rgb[0],
                            rgb[1],
                            rgb[2],
                            transparency.get(index).copied().unwrap_or(255),
                        ]
                    }
                    4 => {
                        let v = scale(sample(0));
                        [v, v, v, scale(sample(1))]
                    }
                    6 => [
                        scale(sample(0)),
                        scale(sample(1)),
                        scale(sample(2)),
                        scale(sample(3)),
                    ],
                    _ => unreachable!(),
                };
                let target = ((y0 + y * dy) * width as usize + x0 + x * dx) * 4;
                pixels[target..target + 4].copy_from_slice(&pixel);
            }
        }
        offset += stride * ph;
    }
    Ok(RgbaImage {
        width,
        height,
        pixels,
    })
}
fn unfilter(raw: &mut [u8], stride: usize, height: usize, bpp: usize) -> Result<()> {
    for y in 0..height {
        let base = y * stride;
        let filter = raw[base];
        if filter > 4 {
            return Err(ImageError::Invalid("scanline filter"));
        }
        for x in 0..stride - 1 {
            let at = base + 1 + x;
            let left = if x >= bpp { raw[at - bpp] } else { 0 };
            let up = if y > 0 { raw[at - stride] } else { 0 };
            let upper_left = if y > 0 && x >= bpp {
                raw[at - stride - bpp]
            } else {
                0
            };
            let prediction = match filter {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                4 => paeth(left, up, upper_left),
                _ => unreachable!(),
            };
            raw[at] = raw[at].wrapping_add(prediction);
        }
    }
    Ok(())
}
fn sample(row: &[u8], index: usize, depth: usize) -> u16 {
    match depth {
        16 => u16::from_be_bytes([row[index * 2], row[index * 2 + 1]]),
        8 => u16::from(row[index]),
        _ => u16::from(
            (row[index * depth / 8] >> (8 - depth - index * depth % 8)) & ((1 << depth) - 1),
        ),
    }
}
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i32::from(a) + i32::from(b) - i32::from(c);
    let (pa, pb, pc) = (
        (p - i32::from(a)).abs(),
        (p - i32::from(b)).abs(),
        (p - i32::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc32(&out[start..]).to_be_bytes());
    }
    fn png(
        width: u32,
        height: u32,
        depth: u8,
        kind: u8,
        raw: &[u8],
        palette: &[u8],
        trns: &[u8],
    ) -> Vec<u8> {
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut h = Vec::new();
        h.extend_from_slice(&width.to_be_bytes());
        h.extend_from_slice(&height.to_be_bytes());
        h.extend_from_slice(&[depth, kind, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &h);
        if !palette.is_empty() {
            chunk(&mut out, b"PLTE", palette);
        }
        if !trns.is_empty() {
            chunk(&mut out, b"tRNS", trns);
        }
        let n = raw.len() as u16;
        let mut z = vec![0x78, 1, 1];
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(raw);
        let (mut a, mut b) = (1_u32, 0_u32);
        for &v in raw {
            a = (a + u32::from(v)) % 65521;
            b = (b + a) % 65521;
        }
        z.extend_from_slice(&(b << 16 | a).to_be_bytes());
        let mid = z.len() / 2;
        chunk(&mut out, b"IDAT", &z[..mid]);
        chunk(&mut out, b"IDAT", &z[mid..]);
        chunk(&mut out, b"IEND", &[]);
        out
    }
    #[test]
    fn rgba_and_all_five_filters_decode_split_idat() {
        let source = [
            12_u8, 23, 34, 255, 56, 67, 78, 128, 90, 101, 112, 255, 123, 134, 145, 0,
        ];
        for filter in 0..=4 {
            let mut raw = Vec::new();
            for y in 0..2 {
                raw.push(filter);
                for x in 0..8 {
                    let i = y * 8 + x;
                    let a = if x >= 4 { source[i - 4] } else { 0 };
                    let b = if y > 0 { source[i - 8] } else { 0 };
                    let c = if y > 0 && x >= 4 { source[i - 12] } else { 0 };
                    let p = match filter {
                        0 => 0,
                        1 => a,
                        2 => b,
                        3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                        _ => paeth(a, b, c),
                    };
                    raw.push(source[i].wrapping_sub(p));
                }
            }
            assert_eq!(
                decode_png(&png(2, 2, 8, 6, &raw, &[], &[]), ImageLimits::default())
                    .unwrap()
                    .pixels(),
                &source
            );
        }
    }
    #[test]
    fn packed_palette_grayscale_and_transparency() {
        let indexed = png(
            2,
            1,
            1,
            3,
            &[0, 0b0100_0000],
            &[255, 0, 0, 0, 0, 255],
            &[0, 128],
        );
        assert_eq!(
            decode_png(&indexed, ImageLimits::default())
                .unwrap()
                .pixels(),
            &[255, 0, 0, 0, 0, 0, 255, 128]
        );
        let gray = png(2, 1, 4, 0, &[0, 0x0f], &[], &[0, 0]);
        assert_eq!(
            decode_png(&gray, ImageLimits::default()).unwrap().pixels(),
            &[0, 0, 0, 0, 255, 255, 255, 255]
        );
        let gray16 = png(1, 1, 16, 4, &[0, 0xff, 0xff, 0x80, 0x80], &[], &[]);
        assert_eq!(
            decode_png(&gray16, ImageLimits::default())
                .unwrap()
                .pixels(),
            &[255, 255, 255, 128]
        );
    }
    #[test]
    fn corruption_truncation_and_limits_fail_without_panicking() {
        let bytes = png(1, 1, 8, 2, &[0, 10, 20, 30], &[], &[]);
        for end in 0..bytes.len() {
            assert!(decode_png(&bytes[..end], ImageLimits::default()).is_err());
        }
        let mut corrupt = bytes.clone();
        corrupt[29] ^= 1;
        assert!(decode_png(&corrupt, ImageLimits::default()).is_err());
        assert!(
            decode_png(
                &bytes,
                ImageLimits {
                    max_pixels: 0,
                    ..ImageLimits::default()
                }
            )
            .is_err()
        );
        assert!(
            decode_png(
                &png(1, 1, 8, 3, &[0, 2], &[255, 0, 0], &[]),
                ImageLimits::default()
            )
            .is_err()
        );
        assert!(
            decode_png(
                &png(1, 1, 8, 6, &[5, 0, 0, 0, 0], &[], &[]),
                ImageLimits::default()
            )
            .is_err()
        );
    }
}

//! Signature-based dispatch and small, untrusted geometry probes for EPUB pagination.
use crate::{ImageError, ImageLimits, Result, RgbaImage, be32, crc32, decode_png, decode_webp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    WebP,
    Jpeg,
    Svg,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
}
impl ImageInfo {
    pub fn rgba_bytes(self) -> Result<usize> {
        usize::try_from(u64::from(self.width) * u64::from(self.height) * 4)
            .map_err(|_| ImageError::Budget)
    }
    pub(crate) fn checked(self, limits: ImageLimits) -> Result<Self> {
        if self.width == 0 || self.height == 0 || self.width > 16384 || self.height > 16384 {
            return Err(ImageError::Invalid("image dimensions"));
        }
        if u64::from(self.width) * u64::from(self.height) > limits.max_pixels as u64
            || self.rgba_bytes()? > limits.max_decoded_bytes
        {
            return Err(ImageError::Budget);
        }
        Ok(self)
    }
}

/// Probe at most 33 bytes, with the full resource length from the ZIP directory.
/// Success validates geometry, not image payload/ZIP CRC. Decode validates those later.
pub fn probe(prefix: &[u8], resource_bytes: usize, limits: ImageLimits) -> Result<ImageInfo> {
    if resource_bytes > limits.max_file_bytes {
        return Err(ImageError::Budget);
    }
    if prefix.len() > resource_bytes {
        return Err(ImageError::Invalid("image prefix length"));
    }
    let xml_prefix = prefix.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(prefix);
    if xml_prefix.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'<') {
        return crate::svg::header(prefix, resource_bytes, limits);
    }
    if prefix.starts_with(&[0xff, 0xd8]) {
        return crate::jpeg::header(prefix, resource_bytes, limits).map(|(info, _)| info);
    }
    if prefix.starts_with(b"\x89PNG\r\n\x1a\n") {
        let header = prefix
            .get(..33)
            .ok_or(ImageError::Invalid("truncated PNG header"))?;
        if header[8..12] != 13_u32.to_be_bytes() || &header[12..16] != b"IHDR" {
            return Err(ImageError::Invalid("PNG IHDR must be first"));
        }
        if crc32(&header[12..29]) != be32(&header[29..33])? {
            return Err(ImageError::Invalid("PNG header CRC-32 mismatch"));
        }
        return ImageInfo {
            width: be32(&header[16..20])?,
            height: be32(&header[20..24])?,
            format: ImageFormat::Png,
        }
        .checked(limits);
    }
    if prefix.starts_with(b"RIFF") && prefix.get(8..12) == Some(b"WEBP") {
        let riff_length = le32(&prefix[4..8])? as u64 + 8;
        if riff_length != resource_bytes as u64 {
            return Err(ImageError::Invalid("WebP RIFF size mismatch"));
        }
        let chunk = prefix
            .get(12..20)
            .ok_or(ImageError::Invalid("truncated WebP chunk"))?;
        let length = le32(&chunk[4..8])? as u64;
        if 20 + length + (length & 1) > riff_length {
            return Err(ImageError::Invalid("WebP chunk exceeds RIFF"));
        }
        let payload = prefix
            .get(20..)
            .ok_or(ImageError::Invalid("truncated WebP header"))?;
        let (width, height) = match &chunk[..4] {
            b"VP8X" if length == 10 => {
                let h = payload
                    .get(..10)
                    .ok_or(ImageError::Invalid("truncated VP8X header"))?;
                (le24(&h[4..7]) + 1, le24(&h[7..10]) + 1)
            }
            b"VP8 " if length >= 10 => vp8_dimensions(payload)?,
            b"VP8L" if length >= 5 => vp8l_dimensions(payload)?,
            _ => {
                return Err(ImageError::Invalid(
                    "WebP first chunk must be VP8, VP8L or VP8X",
                ));
            }
        };
        return ImageInfo {
            width,
            height,
            format: ImageFormat::WebP,
        }
        .checked(limits);
    }
    Err(ImageError::Unsupported(
        "supported image signatures are PNG, WebP, JPEG and SVG",
    ))
}

/// Detect by content, not by extension or an untrusted manifest MIME type.
pub fn decode(bytes: &[u8], limits: ImageLimits) -> Result<RgbaImage> {
    let info = probe(bytes, bytes.len(), limits)?;
    match info.format {
        ImageFormat::Png => decode_png(bytes, limits),
        ImageFormat::WebP => decode_webp(bytes, limits),
        ImageFormat::Jpeg => crate::decode_jpeg(bytes, limits),
        ImageFormat::Svg => crate::decode_svg_with_resources(bytes, limits, &[], &|_| None),
    }
}
pub(crate) fn le32(bytes: &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes.try_into().map_err(|_| {
        ImageError::Invalid("truncated WebP integer")
    })?))
}
pub(crate) fn le24(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16)
}
pub(crate) fn vp8_dimensions(payload: &[u8]) -> Result<(u32, u32)> {
    let h = payload
        .get(..10)
        .ok_or(ImageError::Invalid("truncated VP8 frame"))?;
    if h[0] & 1 != 0 || &h[3..6] != b"\x9d\x01\x2a" {
        return Err(ImageError::Invalid("WebP VP8 key-frame signature"));
    }
    Ok((
        u32::from(u16::from_le_bytes([h[6], h[7]]) & 0x3fff),
        u32::from(u16::from_le_bytes([h[8], h[9]]) & 0x3fff),
    ))
}
pub(crate) fn vp8l_dimensions(payload: &[u8]) -> Result<(u32, u32)> {
    let h = payload
        .get(..5)
        .ok_or(ImageError::Invalid("truncated VP8L frame"))?;
    if h[0] != 0x2f {
        return Err(ImageError::Invalid("VP8L signature"));
    }
    let bits = le32(&h[1..5])?;
    if bits >> 29 != 0 {
        return Err(ImageError::Invalid("unsupported VP8L version"));
    }
    Ok(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
}

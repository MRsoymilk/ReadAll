//! WebP codec adapter. RIFF/frame geometry is checked before invoking image-webp.
//! Animated images are presented as their first composited frame, not played.
use crate::format::{le24, le32, vp8_dimensions, vp8l_dimensions};
use crate::{ImageError, ImageFormat, ImageInfo, ImageLimits, Result, RgbaImage, probe};
use std::io::Cursor;
#[cfg(test)]
mod tests;

pub fn decode_webp(bytes: &[u8], limits: ImageLimits) -> Result<RgbaImage> {
    let info = probe(&bytes[..bytes.len().min(33)], bytes.len(), limits)?;
    if info.format != ImageFormat::WebP {
        return Err(ImageError::Unsupported("expected a WebP signature"));
    }
    // Bound chunks and all frame dimensions, including subframes which a corrupt
    // extended container could otherwise describe as larger than its canvas.
    validate_chunks(
        &bytes[12..],
        (info.width, info.height),
        limits,
        false,
        &mut 0,
    )?;
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(ImageError::WebP)?;
    if decoder.dimensions() != (info.width, info.height) {
        return Err(ImageError::Invalid(
            "WebP decoder/header dimensions disagree",
        ));
    }
    // This is defense in depth: upstream documents that some internal allocations
    // do not yet honor this setting. File/pixel/output caps above remain enforced.
    decoder.set_memory_limit(limits.max_decoded_bytes);
    let output_size = decoder.output_buffer_size().ok_or(ImageError::Budget)?;
    let rgba_bytes = info.rgba_bytes()?;
    let expected = if decoder.has_alpha() {
        rgba_bytes
    } else {
        rgba_bytes / 4 * 3
    };
    if output_size != expected {
        return Err(ImageError::Invalid("WebP output size mismatch"));
    }
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(rgba_bytes)
        .map_err(|_| ImageError::Allocation)?;
    pixels.resize(rgba_bytes, 0);
    decoder
        .read_image(&mut pixels[..output_size])
        .map_err(ImageError::WebP)?;
    if !decoder.has_alpha() {
        // Expand backwards in the same allocation, avoiding an extra RGB buffer.
        for index in (0..rgba_bytes / 4).rev() {
            let [r, g, b] = [
                pixels[index * 3],
                pixels[index * 3 + 1],
                pixels[index * 3 + 2],
            ];
            pixels[index * 4..index * 4 + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
    Ok(RgbaImage {
        width: info.width,
        height: info.height,
        pixels,
    })
}

fn validate_chunks(
    bytes: &[u8],
    expected: (u32, u32),
    limits: ImageLimits,
    in_frame: bool,
    chunks: &mut usize,
) -> Result<()> {
    let mut at = 0_usize;
    while at < bytes.len() {
        *chunks += 1;
        if *chunks > 8192 {
            return Err(ImageError::Budget);
        }
        let header = bytes
            .get(at..at + 8)
            .ok_or(ImageError::Invalid("truncated WebP chunk"))?;
        let length = le32(&header[4..8])? as usize;
        let end = at
            .checked_add(8)
            .and_then(|n| n.checked_add(length))
            .filter(|n| *n <= bytes.len())
            .ok_or(ImageError::Invalid("WebP chunk length"))?;
        let next = end
            .checked_add(length & 1)
            .filter(|n| *n <= bytes.len())
            .ok_or(ImageError::Invalid("missing WebP chunk padding"))?;
        let payload = &bytes[at + 8..end];
        match &header[..4] {
            b"VP8 " | b"VP8L" => {
                let dimensions = if &header[..4] == b"VP8 " {
                    vp8_dimensions(payload)?
                } else {
                    vp8l_dimensions(payload)?
                };
                ImageInfo {
                    width: dimensions.0,
                    height: dimensions.1,
                    format: ImageFormat::WebP,
                }
                .checked(limits)?;
                if dimensions != expected {
                    return Err(ImageError::Invalid(
                        "WebP frame dimensions disagree with canvas",
                    ));
                }
            }
            b"ANMF" => {
                if in_frame {
                    return Err(ImageError::Invalid("nested WebP animation frame"));
                }
                let frame = payload
                    .get(..16)
                    .ok_or(ImageError::Invalid("truncated WebP animation frame"))?;
                let (x, y) = (le24(&frame[..3]) * 2, le24(&frame[3..6]) * 2);
                let (w, h) = (le24(&frame[6..9]) + 1, le24(&frame[9..12]) + 1);
                if u64::from(x) + u64::from(w) > u64::from(expected.0)
                    || u64::from(y) + u64::from(h) > u64::from(expected.1)
                {
                    return Err(ImageError::Invalid("WebP subframe outside canvas"));
                }
                validate_chunks(&payload[16..], (w, h), limits, true, chunks)?;
            }
            b"VP8X" if in_frame => return Err(ImageError::Invalid("WebP VP8X inside subframe")),
            _ => {}
        }
        at = next;
    }
    Ok(())
}

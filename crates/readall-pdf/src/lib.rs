//! Bounded fixed-page PDF parsing and CPU rasterization shared by Linux and Android.
//! PDF pages keep their authored geometry; this crate never reflows them as EPUB text.

use hayro::{RenderSettings, hayro_interpret::InterpreterSettings, hayro_syntax::Pdf, render};
use readall_core::DocumentId;
use std::{error::Error, fmt, sync::Arc};

pub const MAX_FILE_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_RENDER_PIXELS: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_file_bytes: usize,
    pub max_pages: usize,
    pub max_render_pixels: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file_bytes: MAX_FILE_BYTES,
            max_pages: 20_000,
            max_render_pixels: MAX_RENDER_PIXELS,
        }
    }
}

#[derive(Debug)]
pub enum PdfError {
    TooLarge,
    Invalid,
    TooManyPages,
    Empty,
    PageOutOfRange,
    InvalidViewport,
}
impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "PDF exceeds the 128 MiB input limit",
            Self::Invalid => "invalid or unsupported PDF",
            Self::TooManyPages => "PDF page count exceeds the supported limit",
            Self::Empty => "PDF contains no pages",
            Self::PageOutOfRange => "PDF page index is out of range",
            Self::InvalidViewport => "PDF render viewport is invalid or exceeds the pixel budget",
        })
    }
}
impl Error for PdfError {}

#[derive(Debug, Clone)]
pub struct Metadata {
    pub title: String,
    pub author: String,
}

#[derive(Debug, Clone)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct Document {
    pdf: Pdf,
    id: DocumentId,
    metadata: Metadata,
    limits: Limits,
}

impl Document {
    pub fn parse(bytes: Vec<u8>, limits: Limits) -> Result<Self, PdfError> {
        if bytes.len() > limits.max_file_bytes {
            return Err(PdfError::TooLarge);
        }
        if !is_pdf(&bytes) {
            return Err(PdfError::Invalid);
        }
        let id = DocumentId::of(&bytes);
        let data: hayro::hayro_syntax::PdfData = Arc::new(bytes);
        let pdf = Pdf::new(data).map_err(|_| PdfError::Invalid)?;
        let pages = pdf.pages().len();
        if pages == 0 {
            return Err(PdfError::Empty);
        }
        if pages > limits.max_pages {
            return Err(PdfError::TooManyPages);
        }
        let raw = pdf.metadata();
        let metadata = Metadata {
            title: decode_pdf_string(raw.title.as_deref()),
            author: decode_pdf_string(raw.author.as_deref()),
        };
        Ok(Self {
            pdf,
            id,
            metadata,
            limits,
        })
    }

    pub fn page_count(&self) -> usize {
        self.pdf.pages().len()
    }

    pub fn id(&self) -> DocumentId {
        self.id
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub fn render_fit(
        &self,
        page_index: usize,
        max_width: u32,
        max_height: u32,
    ) -> Result<RenderedPage, PdfError> {
        if max_width == 0
            || max_height == 0
            || max_width > u16::MAX as u32
            || max_height > u16::MAX as u32
        {
            return Err(PdfError::InvalidViewport);
        }
        let page = self
            .pdf
            .pages()
            .get(page_index)
            .ok_or(PdfError::PageOutOfRange)?;
        let (source_width, source_height) = page.render_dimensions();
        if !source_width.is_finite()
            || !source_height.is_finite()
            || source_width <= 0.0
            || source_height <= 0.0
        {
            return Err(PdfError::Invalid);
        }
        let mut scale = (max_width as f64 / source_width as f64)
            .min(max_height as f64 / source_height as f64)
            .max(0.000_001);
        let requested_pixels = source_width as f64 * source_height as f64 * scale * scale;
        if requested_pixels > self.limits.max_render_pixels as f64 {
            scale *= (self.limits.max_render_pixels as f64 / requested_pixels).sqrt();
        }
        let width = ((source_width as f64 * scale).floor() as u32).clamp(1, max_width);
        let height = ((source_height as f64 * scale).floor() as u32).clamp(1, max_height);
        if u64::from(width) * u64::from(height) > self.limits.max_render_pixels {
            return Err(PdfError::InvalidViewport);
        }
        let settings = RenderSettings {
            x_scale: scale as f32,
            y_scale: scale as f32,
            width: Some(width as u16),
            height: Some(height as u16),
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
        };
        let pixmap = render(page, &InterpreterSettings::default(), &settings);
        Ok(RenderedPage {
            width,
            height,
            rgba: pixmap.data_as_u8_slice().to_vec(),
        })
    }

    pub fn thumbnail(&self) -> Result<RenderedPage, PdfError> {
        self.render_fit(0, 384, 512)
    }
}

pub fn is_pdf(bytes: &[u8]) -> bool {
    let probe = &bytes[..bytes.len().min(1024)];
    probe.windows(5).any(|part| part == b"%PDF-")
}

fn decode_pdf_string(value: Option<&[u8]>) -> String {
    let Some(bytes) = value else {
        return String::new();
    };
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let units = rest
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]));
        return char::decode_utf16(units)
            .map(|item| item.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect::<String>()
            .chars()
            .take(512)
            .collect();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let units = rest
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
        return char::decode_utf16(units)
            .map(|item| item.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect::<String>()
            .chars()
            .take(512)
            .collect();
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.chars().take(512).collect(),
        Err(_) => bytes.iter().copied().map(char::from).take(512).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_pdf() -> Vec<u8> {
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0usize];
        for object in [
            b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".as_slice(),
            b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".as_slice(),
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Resources << >> /Contents 4 0 R >>\nendobj\n".as_slice(),
            b"4 0 obj\n<< /Length 32 >>\nstream\n0.1 0.4 0.8 rg 0 0 200 300 re f\nendstream\nendobj\n".as_slice(),
        ] {
            offsets.push(out.len());
            out.extend_from_slice(object);
        }
        let xref = out.len();
        out.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        out
    }

    #[test]
    fn signature_is_bounded_and_not_extension_based() {
        assert!(is_pdf(b"junk\n%PDF-1.7\n"));
        assert!(!is_pdf(b"PK\x03\x04"));
    }

    #[test]
    fn one_page_document_renders_with_bounded_geometry() {
        let document = Document::parse(minimal_pdf(), Limits::default()).unwrap();
        assert_eq!(document.page_count(), 1);
        let page = document.render_fit(0, 400, 400).unwrap();
        assert_eq!((page.width, page.height), (266, 400));
        assert_eq!(
            page.rgba.len(),
            page.width as usize * page.height as usize * 4
        );
        assert!(page.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn invalid_data_is_rejected_without_panicking() {
        assert!(Document::parse(b"%PDF-1.4\n%%EOF".to_vec(), Limits::default()).is_err());
        assert!(Document::parse(vec![0; MAX_FILE_BYTES + 1], Limits::default()).is_err());
    }
}

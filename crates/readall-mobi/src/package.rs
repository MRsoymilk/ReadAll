//! Deterministic stored ZIP adapter. No temporary EPUB files or subprocesses.
use crate::{
    MobiBook, MobiError, Progress, Result, Stage,
    html::{Content, escape},
    progress,
};
use readall_archive::crc32;
use readall_core::DocumentId;
use std::collections::BTreeSet;

pub(crate) fn build(
    book: &MobiBook<'_>,
    content: &mut Content,
    observer: &mut dyn FnMut(Progress) -> bool,
) -> Result<Vec<u8>> {
    let mut requested = content.images.clone();
    if let Some(index) = book.cover {
        requested.insert(index);
    }
    if requested.len() > 8192 {
        return Err(MobiError::Limit("image resources"));
    }
    let mut resources = Vec::new();
    let mut bytes = 0_usize;
    for (done, &index) in requested.iter().enumerate() {
        progress(
            observer,
            Stage::Package,
            done,
            requested.len() + content.sections.len(),
        )?;
        match book.image(index).and_then(|data| {
            image_type(data)
                .map(|kind| (data, kind))
                .ok_or(MobiError::Unsupported("embedded image signature"))
        }) {
            Ok((data, mime)) => {
                bytes = bytes.saturating_add(data.len());
                if bytes > book.limits.max_package_bytes {
                    return Err(MobiError::Limit("image package bytes"));
                }
                resources.push((index, data, mime));
            }
            Err(error) => {
                if content.warnings.len() < 32 {
                    content.warnings.push(format!(
                        "MOBI image recindex {} unavailable: {error}",
                        index.saturating_add(1)
                    ));
                }
            }
        }
    }
    let cover = book.cover.filter(|index| {
        !content.images.contains(index) && resources.iter().any(|(i, _, _)| i == index)
    });
    let id = DocumentId::of(book.data);
    let info = &book.metadata;
    let title = escape(&info.title);
    let mut opf = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><package version=\"3.0\" unique-identifier=\"source\" xmlns=\"http://www.idpf.org/2007/opf\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"source\">readall-mobi-v1:{id}</dc:identifier><dc:title>{title}</dc:title><dc:language>{}</dc:language><meta property=\"dcterms:modified\">2000-01-01T00:00:00Z</meta>",
        escape(info.language.as_deref().unwrap_or("und"))
    );
    if let Some(author) = &info.author {
        opf.push_str(&format!("<dc:creator>{}</dc:creator>", escape(author)));
    }
    opf.push_str("</metadata><manifest><item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>");
    if cover.is_some() {
        opf.push_str(
            "<item id=\"cover\" href=\"cover.xhtml\" media-type=\"application/xhtml+xml\"/>",
        );
    }
    for n in 0..content.sections.len() {
        opf.push_str(&format!(
            "<item id=\"s{n}\" href=\"section{n}.xhtml\" media-type=\"application/xhtml+xml\"/>"
        ));
    }
    for (index, _, mime) in &resources {
        opf.push_str(&format!(
            "<item id=\"i{index}\" href=\"media/image{index}\" media-type=\"{mime}\"{}/>",
            if Some(*index) == book.cover {
                " properties=\"cover-image\""
            } else {
                ""
            }
        ));
    }
    opf.push_str("</manifest><spine>");
    if cover.is_some() {
        opf.push_str("<itemref idref=\"cover\"/>");
    }
    for n in 0..content.sections.len() {
        opf.push_str(&format!("<itemref idref=\"s{n}\"/>"));
    }
    opf.push_str("</spine></package>");
    let mut nav = String::from(
        "<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>Contents</title></head><body><nav epub:type=\"toc\"><ol>",
    );
    let mut seen = BTreeSet::new();
    if cover.is_some() {
        nav.push_str("<li><a href=\"cover.xhtml\">Cover</a></li>");
    }
    for (label, href) in &content.navigation {
        if seen.insert(href) {
            nav.push_str(&format!(
                "<li><a href=\"{}\">{}</a></li>",
                escape(href),
                escape(label)
            ));
        }
    }
    nav.push_str("</ol></nav></body></html>");
    let mut zip = Zip::new(book.limits.max_package_bytes);
    zip.add("mimetype", b"application/epub+zip")?;
    zip.add("META-INF/container.xml", b"<container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\" version=\"1.0\"><rootfiles><rootfile full-path=\"OPS/book.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>")?;
    zip.add("OPS/book.opf", opf.as_bytes())?;
    zip.add("OPS/nav.xhtml", nav.as_bytes())?;
    if let Some(index) = cover {
        zip.add("OPS/cover.xhtml", format!("<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{title}</title></head><body><img src=\"media/image{index}\" alt=\"Cover\"/></body></html>").as_bytes())?;
    }
    for (n, section) in content.sections.iter().enumerate() {
        progress(
            observer,
            Stage::Package,
            requested.len() + n,
            requested.len() + content.sections.len(),
        )?;
        let document = format!(
            "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{}</title><style>blockquote {{ margin-left:1em; }} {}</style></head><body>{}</body></html>",
            escape(&section.label),
            escape(&content.css),
            section.body
        );
        if document.len() > 4 * 1024 * 1024 {
            return Err(MobiError::Limit("adapted chapter XML bytes"));
        }
        zip.add(&format!("OPS/section{n}.xhtml"), document.as_bytes())?;
    }
    for (index, bytes, _) in resources {
        zip.add(&format!("OPS/media/image{index}"), bytes)?;
    }
    progress(
        observer,
        Stage::Package,
        requested.len() + content.sections.len(),
        requested.len() + content.sections.len(),
    )?;
    zip.finish()
}
pub(crate) fn image_type(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}
pub(crate) struct Zip {
    bytes: Vec<u8>,
    central: Vec<u8>,
    count: u16,
    limit: usize,
}
impl Zip {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            central: Vec::new(),
            count: 0,
            limit,
        }
    }
    pub(crate) fn add(&mut self, name: &str, data: &[u8]) -> Result<()> {
        let name = name.as_bytes();
        let name_size = u16::try_from(name.len()).map_err(|_| MobiError::Limit("ZIP name"))?;
        let size = u32::try_from(data.len()).map_err(|_| MobiError::Limit("ZIP entry bytes"))?;
        let offset = u32::try_from(self.bytes.len()).map_err(|_| MobiError::Limit("ZIP offset"))?;
        let additional = 30 + 46 + name.len() * 2 + data.len();
        if self
            .bytes
            .len()
            .saturating_add(self.central.len())
            .saturating_add(additional)
            .saturating_add(22)
            > self.limit
        {
            return Err(MobiError::Limit("in-memory EPUB bytes"));
        }
        self.count = self
            .count
            .checked_add(1)
            .ok_or(MobiError::Limit("ZIP entry count"))?;
        self.bytes
            .try_reserve(additional)
            .map_err(|_| MobiError::Limit("ZIP allocation"))?;
        let crc = crc32(data);
        let mut header = Vec::new();
        for n in [20_u16, 0x800, 0, 0, 0] {
            header.extend_from_slice(&n.to_le_bytes());
        }
        for n in [crc, size, size] {
            header.extend_from_slice(&n.to_le_bytes());
        }
        header.extend_from_slice(&name_size.to_le_bytes());
        header.extend_from_slice(&0_u16.to_le_bytes());
        self.bytes.extend_from_slice(b"PK\x03\x04");
        self.bytes.extend_from_slice(&header);
        self.bytes.extend_from_slice(name);
        self.bytes.extend_from_slice(data);
        self.central.extend_from_slice(b"PK\x01\x02");
        self.central.extend_from_slice(&20_u16.to_le_bytes());
        self.central.extend_from_slice(&header);
        self.central.extend_from_slice(&[0; 10]);
        self.central.extend_from_slice(&offset.to_le_bytes());
        self.central.extend_from_slice(name);
        Ok(())
    }
    pub(crate) fn finish(mut self) -> Result<Vec<u8>> {
        let offset =
            u32::try_from(self.bytes.len()).map_err(|_| MobiError::Limit("ZIP central offset"))?;
        let size =
            u32::try_from(self.central.len()).map_err(|_| MobiError::Limit("ZIP central size"))?;
        self.bytes.extend_from_slice(&self.central);
        self.bytes.extend_from_slice(b"PK\x05\x06");
        self.bytes.extend_from_slice(&[0; 4]);
        for _ in 0..2 {
            self.bytes.extend_from_slice(&self.count.to_le_bytes());
        }
        self.bytes.extend_from_slice(&size.to_le_bytes());
        self.bytes.extend_from_slice(&offset.to_le_bytes());
        self.bytes.extend_from_slice(&[0; 2]);
        Ok(self.bytes)
    }
}

//! Local publication resources and presentation annotations over stable EPUB text.
use crate::{
    ChapterContent, EpubBook, EpubError, Result,
    css::StyleSheet,
    resolve_path, xhtml,
    xml::{self, Event, XmlLimits, local_name},
};

impl EpubBook<'_> {
    /// Read only a manifest-listed, archive-local resource relative to a spine XHTML.
    /// Check decompressed size before decoding ZIP data; never resolve network/file URIs.
    pub fn read_spine_resource(
        &self,
        spine: usize,
        reference: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>> {
        let (path, _) = self.spine_resource_location(spine, reference, max_bytes)?;
        Ok(self.archive.read(&path)?)
    }

    /// A bounded, unverified header plus the declared full resource length.
    /// Actual image reads still verify the ZIP CRC through read_spine_resource.
    pub fn read_spine_resource_prefix(
        &self,
        spine: usize,
        reference: &str,
        max_bytes: usize,
        prefix: usize,
    ) -> Result<(Vec<u8>, usize)> {
        let (path, length) = self.spine_resource_location(spine, reference, max_bytes)?;
        Ok((self.archive.read_prefix(&path, prefix)?, length))
    }

    fn spine_resource_location(
        &self,
        spine: usize,
        reference: &str,
        max_bytes: usize,
    ) -> Result<(String, usize)> {
        let base = self
            .spine_item(spine)
            .ok_or(EpubError::Invalid("spine index is out of range"))?;
        let path = resolve_path(base.path(), reference)?;
        if !self.manifest.iter().any(|item| item.path() == path) {
            return Err(EpubError::Invalid(
                "referenced resource is absent from manifest",
            ));
        }
        let entry = self
            .archive
            .entry(&path)
            .ok_or(EpubError::Invalid("referenced resource is absent from ZIP"))?;
        if entry.uncompressed_size() > max_bytes {
            return Err(EpubError::LimitExceeded("linked resource bytes"));
        }
        Ok((path, entry.uncompressed_size()))
    }

    /// Read a raster reference relative to an archive-local SVG resource, not to the XHTML.
    pub fn read_spine_nested_resource(
        &self,
        spine: usize,
        parent: &str,
        reference: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>> {
        let (base, _) = self.spine_resource_location(spine, parent, 16 * 1024 * 1024)?;
        let path = resolve_path(&base, reference)?;
        if !self.manifest.iter().any(|item| item.path() == path) {
            return Err(EpubError::Invalid("nested resource missing from manifest"));
        }
        let entry = self
            .archive
            .entry(&path)
            .ok_or(EpubError::Invalid("nested resource missing from ZIP"))?;
        if entry.uncompressed_size() > max_bytes {
            return Err(EpubError::LimitExceeded("nested resource bytes"));
        }
        Ok(self.archive.read(&path)?)
    }

    /// Read an already resolved @font-face path. The manifest and ZIP directory
    /// are checked before decompression; font validation is performed by the caller.
    pub fn read_font_resource(&self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        let item = self
            .manifest
            .iter()
            .find(|item| item.path() == path)
            .ok_or(EpubError::Invalid("font resource is absent from manifest"))?;
        if !matches!(
            item.media_type(),
            "font/ttf"
                | "font/otf"
                | "font/collection"
                | "application/font-sfnt"
                | "application/vnd.ms-opentype"
                | "application/x-font-ttf"
                | "application/x-font-truetype"
                | "application/x-font-opentype"
                | "application/octet-stream"
        ) {
            return Err(EpubError::Unsupported("embedded font manifest media type"));
        }
        let entry = self
            .archive
            .entry(path)
            .ok_or(EpubError::Invalid("font resource is absent from ZIP"))?;
        if entry.uncompressed_size() > max_bytes.min(16 * 1024 * 1024) {
            return Err(EpubError::LimitExceeded("embedded font bytes"));
        }
        Ok(self.archive.read(path)?)
    }

    /// Retain styles and image positions without changing `epub-v1` text offsets.
    pub fn read_spine_content(&self, index: usize) -> Result<ChapterContent> {
        let item = self
            .spine_item(index)
            .ok_or(EpubError::Invalid("spine index is out of range"))?;
        if item.media_type() == "image/svg+xml" {
            let bytes = self.read_spine(index)?;
            let events = xml::parse(
                &bytes,
                XmlLimits {
                    max_bytes: self.limits.max_xml_bytes,
                    ..XmlLimits::default()
                },
            )?;
            if !events
                .iter()
                .find_map(|event| {
                    if let Event::Start(element) = event {
                        Some(local_name(&element.name))
                    } else {
                        None
                    }
                })
                .is_some_and(|name| name == "svg")
            {
                return Err(EpubError::Invalid("SVG spine root"));
            }
            let svg = String::from_utf8(bytes)
                .map_err(|_| EpubError::Invalid("SVG spine is not UTF-8"))?;
            return Ok(ChapterContent {
                font_families: Default::default(),
                font_faces: Vec::new(),
                text: String::new(),
                runs: Vec::new(),
                blocks: Vec::new(),
                links: Vec::new(),
                images: vec![crate::ImageReference {
                    offset: 0,
                    source: "<spine-svg>".into(),
                    alt: "SVG page".into(),
                    width: None,
                    height: None,
                    inline_svg: Some(svg),
                }],
                warnings: Vec::new(),
            });
        }
        if item.media_type() != "application/xhtml+xml" {
            return Err(EpubError::Unsupported(
                "spine item is not application/xhtml+xml",
            ));
        }
        let bytes = self.read_spine(index)?;
        let sources = stylesheets(&bytes, self.limits.max_xml_bytes)?;
        let mut sheet = StyleSheet::default();
        let mut warnings = Vec::new();
        for source in sources {
            let loaded = match source {
                Source::Inline(text) => Ok((text.into_bytes(), item.path().to_owned())),
                Source::Linked(reference) => (|| {
                    let path = resolve_path(item.path(), &reference)?;
                    if !self
                        .manifest
                        .iter()
                        .any(|item| item.path() == path && item.media_type() == "text/css")
                    {
                        return Err(EpubError::Unsupported(
                            "stylesheet must be a local text/css manifest item",
                        ));
                    }
                    Ok((
                        self.read_spine_resource(index, &reference, 1024 * 1024)?,
                        path,
                    ))
                })(),
            };
            let applied = loaded.and_then(|(bytes, origin)| {
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| EpubError::Unsupported("CSS encoding must be UTF-8"))?;
                sheet.append_at(text.trim_start_matches('\u{feff}'), &origin)
            });
            if let Err(error) = applied {
                warnings.push(format!("stylesheet ignored: {error}"));
            }
        }
        let extracted =
            match xhtml::extract_styled(&bytes, self.limits.max_xml_bytes, Some(&mut sheet)) {
                Ok(extracted) => extracted,
                Err(EpubError::LimitExceeded("CSS selector work")) => {
                    warnings.push("CSS selector budget exceeded; using semantic defaults".into());
                    sheet = StyleSheet::default();
                    xhtml::extract_styled(&bytes, self.limits.max_xml_bytes, Some(&mut sheet))?
                }
                Err(error) => return Err(error),
            };
        Ok(ChapterContent {
            font_families: sheet.families,
            font_faces: sheet.font_faces,
            text: extracted.text,
            runs: extracted.runs,
            images: extracted.images,
            blocks: extracted.blocks,
            links: extracted.links,
            warnings,
        })
    }
}

enum Source {
    Inline(String),
    Linked(String),
}
fn stylesheets(bytes: &[u8], max_bytes: usize) -> Result<Vec<Source>> {
    let events = xml::parse(
        bytes,
        XmlLimits {
            max_bytes,
            ..XmlLimits::default()
        },
    )?;
    let mut sources = Vec::new();
    let mut inline: Option<String> = None;
    for event in events {
        match event {
            Event::Start(element) => {
                let media = element.attribute("media").unwrap_or("").trim();
                let applicable = matches!(media, "" | "all" | "screen")
                    && element
                        .attribute("type")
                        .is_none_or(|kind| kind == "text/css");
                match local_name(&element.name) {
                    "style" if applicable && !element.empty => inline = Some(String::new()),
                    "link" if applicable && element.attribute("disabled").is_none() => {
                        let rel = element.attribute("rel").unwrap_or("");
                        if rel
                            .split_ascii_whitespace()
                            .any(|v| v.eq_ignore_ascii_case("stylesheet"))
                            && !rel
                                .split_ascii_whitespace()
                                .any(|v| v.eq_ignore_ascii_case("alternate"))
                            && let Some(href) = element.attribute("href")
                        {
                            sources.push(Source::Linked(href.to_owned()));
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(text) => {
                if let Some(inline) = &mut inline {
                    inline.push_str(&text);
                }
            }
            Event::End(name) if local_name(&name) == "style" => {
                if let Some(text) = inline.take() {
                    sources.push(Source::Inline(text));
                }
            }
            _ => {}
        }
        if sources.len() > 32 {
            return Err(EpubError::LimitExceeded("chapter stylesheets"));
        }
    }
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::TextAlign;
    #[test]
    fn annotations_preserve_old_text_and_fragment_offsets() {
        let bytes = b"<html><head><style>p {color:red}</style></head><body><h1 id='top'>Title</h1><p>A <span style='color:blue'>B</span><img src='../images/a.png' alt='test'/> C</p></body></html>";
        let plain = xhtml::extract_with_anchors(bytes, 4096).unwrap();
        let mut sheet = StyleSheet::default();
        sheet.append("p {color:red; text-align:center}").unwrap();
        let rich = xhtml::extract_styled(bytes, 4096, Some(&mut sheet)).unwrap();
        assert_eq!(plain.text, rich.text);
        assert_eq!(plain.anchor("top"), rich.anchor("top"));
        assert_eq!(rich.runs[0].style.font_scale, 2.0);
        let b = rich
            .runs
            .iter()
            .find(|run| &rich.text[run.range.clone()] == " B")
            .unwrap();
        assert_eq!(b.style.color, [0, 0, 255]);
        assert_eq!(b.style.align, TextAlign::Center);
        assert_eq!(rich.images.len(), 1);
        assert!(rich.text.is_char_boundary(rich.images[0].offset));
    }
    #[test]
    fn image_only_documents_keep_empty_canonical_text() {
        let rich = xhtml::extract_styled(
            b"<html><body><img src='a.png'/></body></html>",
            1024,
            Some(&mut StyleSheet::default()),
        )
        .unwrap();
        assert!(rich.text.is_empty());
        assert_eq!(rich.images[0].offset, 0);
    }
    #[test]
    fn stylesheet_document_order_and_media_are_respected() {
        let sources = stylesheets(b"<html><head><link rel='stylesheet' href='a.css'/><style>p{color:red}</style><link rel='alternate stylesheet' href='b.css'/><style media='print'>p{color:blue}</style></head><body/></html>", 4096).unwrap();
        assert_eq!(sources.len(), 2);
        assert!(matches!(&sources[0], Source::Linked(path) if path == "a.css"));
        assert!(matches!(&sources[1], Source::Inline(_)));
    }
}

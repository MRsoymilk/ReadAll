//! Bounded cover discovery for bookshelves; never loads or lays out every chapter.
use super::*;
impl EpubBook<'_> {
    /// EPUB 3 cover-image, EPUB 2 metadata/guide, then conservative cover naming.
    /// Dedicated XHTML/SVG wrappers may reference one manifest image, never external data.
    pub fn cover_resource(&self, max_bytes: usize) -> Result<Option<Vec<u8>>> {
        let mut candidate = self.manifest.iter().find(|item| {
            item.media_type.starts_with("image/") && has_token(&item.properties, "cover-image")
        });
        if candidate.is_none() {
            let package = self.archive.read(&self.package_path)?;
            let events = xml::parse(&package, xml_limits(self.limits.max_xml_bytes))?;
            let id = events.iter().find_map(|event| match event {
                Event::Start(e)
                    if local_name(&e.name) == "meta" && e.attribute("name") == Some("cover") =>
                {
                    e.attribute("content")
                }
                _ => None,
            });
            candidate = id.and_then(|id| self.manifest.iter().find(|i| i.id == id));
            if candidate.is_none() {
                let href = events.iter().find_map(|event| match event {
                    Event::Start(e)
                        if local_name(&e.name) == "reference"
                            && e.attribute("type").is_some_and(|t| has_token(t, "cover")) =>
                    {
                        e.attribute("href")
                    }
                    _ => None,
                });
                if let Some(href) = href {
                    let (path, _) = resolve_navigation_href(&self.package_path, href)?;
                    candidate = self.manifest.iter().find(|i| i.path == path);
                }
            }
        }
        if candidate.is_none() {
            candidate = self.manifest.iter().find(|i| {
                let name = i.path.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
                i.media_type.starts_with("image/")
                    && (i.id.eq_ignore_ascii_case("cover")
                        || name.starts_with("cover.")
                        || name.starts_with("cover-image."))
            });
        }
        if candidate.is_none() {
            candidate = self.manifest.iter().find(|i| {
                let name = i.path.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
                i.media_type == "application/xhtml+xml"
                    && (i.id.eq_ignore_ascii_case("cover") || name.starts_with("cover."))
            });
        }
        let Some(item) = candidate else {
            return Ok(None);
        };
        self.cover_item(item, max_bytes, true)
    }
    fn cover_item(
        &self,
        item: &ManifestItem,
        max_bytes: usize,
        wrapper: bool,
    ) -> Result<Option<Vec<u8>>> {
        let image = item.media_type.starts_with("image/");
        if !image && item.media_type != "application/xhtml+xml" {
            return Ok(None);
        }
        if !image && !wrapper {
            return Ok(None);
        }
        let cap = if image {
            max_bytes
        } else {
            max_bytes.min(512 * 1024)
        };
        let entry = self
            .archive
            .entry(&item.path)
            .ok_or(EpubError::Invalid("cover resource missing"))?;
        if entry.uncompressed_size() > cap {
            return Err(EpubError::LimitExceeded("cover image bytes"));
        }
        let bytes = self.archive.read(&item.path)?;
        if wrapper && (item.media_type == "image/svg+xml" || !image) && bytes.len() <= 512 * 1024 {
            // A wrapper commonly contains <svg><image xlink:href="../Images/front.jpg"/>.
            // Only inspect this document; depth is one and there is no chapter/font layout.
            if let Ok(events) = xml::parse(&bytes, xml_limits(512 * 1024)) {
                for event in &events {
                    let Event::Start(e) = event else { continue };
                    let href = match local_name(&e.name) {
                        "img" => e.attribute("src"),
                        "image" => e.attribute("href").or_else(|| e.attribute("xlink:href")),
                        _ => None,
                    };
                    let Some(href) = href else { continue };
                    let Ok(path) = resolve_path(&item.path, href) else {
                        continue;
                    };
                    if let Some(resource) = self.manifest.iter().find(|i| {
                        i.path == path && i.media_type.starts_with("image/") && i.path != item.path
                    }) {
                        return self.cover_item(resource, max_bytes, false);
                    }
                }
            }
        }
        Ok(image.then_some(bytes))
    }
}

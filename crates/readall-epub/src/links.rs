//! Book-local body hyperlinks. No URL launching, file access or script execution.
use crate::{
    EpubBook, EpubError, EpubLocator, Result, resolve_navigation_href, spine_index_for_path,
    xml::Element,
};
use std::ops::Range;

/// Canonical text and source-ordered image ranges covered by one XHTML anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentLink {
    pub text: Range<usize>,
    pub images: Range<usize>,
    pub href: String,
    pub noteref: bool,
}

#[derive(Default)]
pub(crate) struct LinkCollector {
    open: Option<(usize, ContentLink)>,
    completed: Vec<ContentLink>,
}
impl LinkCollector {
    pub(crate) fn start(
        &mut self,
        element: &Element,
        depth: usize,
        offset: usize,
        images: usize,
    ) -> Result<()> {
        // Nested anchors are invalid HTML. Keep the outer anchor rather than
        // manufacturing overlapping hit regions from malformed publication data.
        if self.open.is_some() || element.empty {
            return Ok(());
        }
        let Some(href) = element
            .attribute("href")
            .map(str::trim)
            .filter(|href| !href.is_empty())
        else {
            return Ok(());
        };
        if href.len() > 4096 || self.completed.len() >= 16_384 {
            return Err(EpubError::LimitExceeded("XHTML hyperlinks"));
        }
        let noteref = element
            .attribute("type")
            .is_some_and(|v| v.split_ascii_whitespace().any(|v| v == "noteref"))
            || element
                .attribute("role")
                .is_some_and(|v| v.split_ascii_whitespace().any(|v| v == "doc-noteref"));
        self.open = Some((
            depth,
            ContentLink {
                text: offset..offset,
                images: images..images,
                href: href.to_owned(),
                noteref,
            },
        ));
        Ok(())
    }
    pub(crate) fn end(&mut self, depth: usize, offset: usize, images: usize) {
        if self.open.as_ref().is_some_and(|(level, _)| *level == depth)
            && let Some((_, mut link)) = self.open.take()
        {
            link.text.end = offset;
            link.images.end = images;
            self.completed.push(link);
        }
    }
    pub(crate) fn finish(mut self, text_len: usize) -> Vec<ContentLink> {
        for link in &mut self.completed {
            link.text.start = link.text.start.min(text_len);
            link.text.end = link.text.end.min(text_len);
        }
        self.completed
            .retain(|link| !link.text.is_empty() || !link.images.is_empty());
        self.completed
    }
}

impl EpubBook<'_> {
    /// Resolve an explicit click, including a non-linear note chapter. Missing
    /// fragments are errors here (unlike TOC fallback), so clicks never silently
    /// navigate to the wrong location. Targets must be in this publication spine.
    pub fn link_locator(&self, from_spine: usize, href: &str) -> Result<EpubLocator> {
        let href = href.trim();
        if href.len() > 4096 || href.chars().any(char::is_control) {
            return Err(EpubError::Invalid("invalid hyperlink reference"));
        }
        let from = self
            .spine_item(from_spine)
            .ok_or(EpubError::Invalid("source spine index is out of range"))?;
        let (path, fragment) = resolve_navigation_href(from.path(), href)?;
        let target = spine_index_for_path(&self.manifest, &self.spine, &path).ok_or(
            EpubError::Unsupported("hyperlink target is not in the publication spine"),
        )?;
        if let Some(fragment) = fragment {
            self.locator_for_fragment(target, &fragment)?
                .ok_or(EpubError::InvalidLocator("hyperlink fragment not found"))
        } else if self
            .spine_item(target)
            .is_some_and(|item| item.media_type() == "image/svg+xml")
        {
            self.image_locator(target, 0)
        } else {
            self.locator(target, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{css::StyleSheet, xhtml};
    #[test]
    fn links_preserve_canonical_offsets_and_cover_images() {
        let xml = br#"<html><body><p>A<a href='two.xhtml#note' epub:type='noteref'>W<span>A</span><img src='pic.png'/></a>W</p><a role='doc-noteref' href='#top'><img src='b.png'/></a></body></html>"#;
        let plain = xhtml::extract_with_anchors(xml, 4096).unwrap();
        let rich = xhtml::extract_styled(xml, 4096, Some(&mut StyleSheet::default())).unwrap();
        assert_eq!(plain.text, rich.text);
        assert_eq!(rich.links.len(), 2);
        assert_eq!(&rich.text[rich.links[0].text.clone()], "WA");
        assert_eq!(rich.links[0].images, 0..1);
        assert_eq!(rich.links[1].images, 1..2);
        assert!(rich.links[1].text.is_empty());
        assert!(rich.links.iter().all(|link| link.noteref));
    }
    #[test]
    fn hidden_empty_and_nested_links_do_not_create_overlapping_targets() {
        let xml = br#"<html><body><script><a href='#x'>A</a></script><a href='#x' style='display:none'>W</a><a id='x'/><a href='#empty'></a><a href='#outer'>A<a href='#inner'>W</a>A</a></body></html>"#;
        let rich = xhtml::extract_styled(xml, 4096, Some(&mut StyleSheet::default())).unwrap();
        assert_eq!(rich.links.len(), 1);
        assert_eq!(rich.links[0].href, "#outer");
        assert_eq!(&rich.text[rich.links[0].text.clone()], "AWA");
    }
}

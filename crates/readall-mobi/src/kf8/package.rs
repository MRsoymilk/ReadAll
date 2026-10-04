//! Deterministic in-memory package, consumed by the same EPUB reading engine.
use super::{Navigation, Section, resources::Resources};
use crate::{MobiBook, MobiError, Progress, Result, Stage, html::escape, package::Zip, progress};
use readall_core::DocumentId;
use std::collections::BTreeMap;

pub(super) fn build(
    book: &MobiBook<'_>,
    sections: &[Section],
    navigation: &[Navigation],
    links: &BTreeMap<(usize, usize), String>,
    resources: &Resources<'_, '_>,
    observer: &mut dyn FnMut(Progress) -> bool,
) -> Result<Vec<u8>> {
    if sections.len() + resources.resolved.len() + 5 > 8192 {
        return Err(MobiError::Limit("KF8 package entries"));
    }
    let cover = resources.cover().filter(|_| !resources.cover_referenced);
    let title = escape(&book.metadata.title);
    let mut opf = format!(
        "<package version=\"3.0\" unique-identifier=\"source\" xmlns=\"http://www.idpf.org/2007/opf\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"source\">readall-kf8-v1:{}</dc:identifier><dc:title>{title}</dc:title><dc:language>{}</dc:language><meta property=\"dcterms:modified\">2000-01-01T00:00:00Z</meta>",
        DocumentId::of(book.data),
        escape(book.metadata.language.as_deref().unwrap_or("und"))
    );
    if let Some(author) = &book.metadata.author {
        opf.push_str(&format!("<dc:creator>{}</dc:creator>", escape(author)));
    }
    opf.push_str("</metadata><manifest><item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>");
    for i in 0..sections.len() {
        opf.push_str(&format!(
            "<item id=\"s{i}\" href=\"section{i}.xhtml\" media-type=\"application/xhtml+xml\"/>"
        ));
    }
    if cover.is_some() {
        opf.push_str(
            "<item id=\"cover\" href=\"cover.xhtml\" media-type=\"application/xhtml+xml\"/>",
        );
    }
    for (i, resource) in resources.resolved.iter().enumerate() {
        opf.push_str(&format!(
            "<item id=\"r{i}\" href=\"{}\" media-type=\"{}\"{}/>",
            resource.path,
            resource.mime,
            if resources.cover() == Some(resource.path.as_str()) {
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
    for i in 0..sections.len() {
        opf.push_str(&format!("<itemref idref=\"s{i}\"/>"));
    }
    opf.push_str("</spine></package>");
    let mut nav = String::from(
        "<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>Contents</title></head><body><nav epub:type=\"toc\"><ol>",
    );
    if cover.is_some() {
        nav.push_str("<li><a href=\"cover.xhtml\">Cover</a></li>");
    }
    let mut depth = 0;
    let mut open = false;
    let mut count = 0;
    for item in navigation {
        let Some(href) = links.get(&item.target) else {
            continue;
        };
        let wanted = item.depth.min(if open { depth + 1 } else { 0 });
        if !open {
            nav.push_str("<li>");
            open = true;
        } else if wanted > depth {
            nav.push_str("<ol><li>");
            depth += 1;
        } else {
            nav.push_str("</li>");
            while depth > wanted {
                nav.push_str("</ol></li>");
                depth -= 1;
            }
            nav.push_str("<li>");
        }
        nav.push_str(&format!(
            "<a href=\"{}\">{}</a>",
            escape(href),
            escape(&item.label)
        ));
        count += 1;
    }
    if open {
        nav.push_str("</li>");
        while depth > 0 {
            nav.push_str("</ol></li>");
            depth -= 1;
        }
    }
    if count == 0 {
        for (i, section) in sections.iter().enumerate() {
            nav.push_str(&format!(
                "<li><a href=\"section{i}.xhtml\">{}</a></li>",
                escape(&section.label)
            ));
        }
    }
    nav.push_str("</ol></nav></body></html>");
    let mut zip = Zip::new(book.limits.max_package_bytes);
    zip.add("mimetype", b"application/epub+zip")?;
    zip.add("META-INF/container.xml",b"<container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\" version=\"1.0\"><rootfiles><rootfile full-path=\"OPS/book.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>")?;
    zip.add("OPS/book.opf", opf.as_bytes())?;
    zip.add("OPS/nav.xhtml", nav.as_bytes())?;
    if let Some(cover) = cover {
        zip.add("OPS/cover.xhtml",format!("<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{title}</title></head><body><img src=\"{cover}\" alt=\"Cover\"/></body></html>").as_bytes())?;
    }
    let total = sections.len() + resources.resolved.len();
    for (i, section) in sections.iter().enumerate() {
        progress(observer, Stage::Package, i, total)?;
        if section.text.len() > 4 * 1024 * 1024 {
            return Err(MobiError::Limit("KF8 adapted chapter XML bytes"));
        }
        zip.add(&format!("OPS/section{i}.xhtml"), section.text.as_bytes())?;
    }
    for (i, resource) in resources.resolved.iter().enumerate() {
        progress(observer, Stage::Package, sections.len() + i, total)?;
        zip.add(&format!("OPS/{}", resource.path), &resource.data)?;
    }
    progress(observer, Stage::Package, total, total)?;
    zip.finish()
}

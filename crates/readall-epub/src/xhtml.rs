//! Small XHTML reading subset: extract body text with deterministic block breaks.
//! This is not HTML error recovery, CSS layout, scripting, SVG, MathML, or image rendering.

use crate::{
    EpubError, Result,
    xml::{self, Event, XmlLimits, local_name},
};

use crate::css::{BoxStyle, StyleSheet, TextStyle};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub struct StyleRun {
    pub range: Range<usize>,
    pub style: TextStyle,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ImageReference {
    /// Offset in the unchanged canonical text stream, not an inserted character.
    pub offset: usize,
    pub source: String,
    pub alt: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub inline_svg: Option<String>,
}
/// Source-ordered box boundaries, including image order at a shared text offset.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockBoundary {
    pub offset: usize,
    pub images_before: usize,
    /// Some opens a block; None closes the innermost open block.
    pub style: Option<BoxStyle>,
}

#[derive(Debug, Clone)]
pub struct ChapterContent {
    pub font_families: crate::css::FontFamilies,
    pub font_faces: Vec<crate::css::FontFace>,
    pub text: String,
    pub runs: Vec<StyleRun>,
    pub images: Vec<ImageReference>,
    pub blocks: Vec<BlockBoundary>,
    pub links: Vec<crate::ContentLink>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExtractedText {
    pub(crate) text: String,
    anchors: Vec<(String, usize)>,
    pub(crate) runs: Vec<StyleRun>,
    pub(crate) images: Vec<ImageReference>,
    pub(crate) blocks: Vec<BlockBoundary>,
    pub(crate) links: Vec<crate::ContentLink>,
}

impl ExtractedText {
    pub(crate) fn anchor(&self, id: &str) -> Option<usize> {
        self.anchors
            .iter()
            .find(|(candidate, _)| candidate == id)
            .map(|(_, offset)| *offset)
    }
}

pub(crate) fn extract(bytes: &[u8], max_bytes: usize) -> Result<String> {
    Ok(extract_with_anchors(bytes, max_bytes)?.text)
}

pub(crate) fn extract_with_anchors(bytes: &[u8], max_bytes: usize) -> Result<ExtractedText> {
    extract_styled(bytes, max_bytes, None)
}

pub(crate) fn extract_styled(
    bytes: &[u8],
    max_bytes: usize,
    mut sheet: Option<&mut StyleSheet>,
) -> Result<ExtractedText> {
    let events = xml::parse(
        bytes,
        XmlLimits {
            max_bytes,
            ..XmlLimits::default()
        },
    )?;
    let root = events.iter().find_map(|event| match event {
        Event::Start(element) => Some(local_name(&element.name)),
        _ => None,
    });
    if root != Some("html") {
        return Err(EpubError::Invalid("spine document root is not XHTML html"));
    }

    let mut output = String::new();
    output
        .try_reserve(bytes.len().min(64 * 1024))
        .map_err(|_| EpubError::AllocationFailed)?;

    let mut anchors = Vec::new();
    anchors
        .try_reserve(32)
        .map_err(|_| EpubError::AllocationFailed)?;
    let mut named_anchors = Vec::new();
    let mut depth = 0_usize;
    let mut body_depth = None;
    let mut suppressed_depth = None;
    let mut seen_body = false;
    let mut pending_space = false;
    let mut styles = vec![TextStyle::default()];
    let mut runs: Vec<StyleRun> = Vec::new();
    let mut images = Vec::new();
    let mut blocks = Vec::new();
    let mut open_blocks = Vec::new();
    let mut links = crate::links::LinkCollector::default();
    let mut svg_capture: Option<(usize, usize, String)> = None;

    for event in &events {
        if let Some((_, _, buffer)) = &mut svg_capture {
            crate::svg_inline::append(buffer, event, false, max_bytes)?;
        }
        match event {
            Event::Start(element) => {
                let current = depth + 1;
                let name = local_name(&element.name);
                let parent = *styles.last().unwrap_or(&TextStyle::default());
                let (style, box_style) = match sheet.as_deref_mut() {
                    Some(sheet) => sheet.compute_with_box(element, parent)?,
                    None => (parent, BoxStyle::default()),
                };
                if sheet.is_some()
                    && name == "svg"
                    && body_depth.is_some()
                    && suppressed_depth.is_none()
                    && svg_capture.is_none()
                    && !style.hidden
                {
                    if images.len() >= 1024 {
                        return Err(EpubError::LimitExceeded("XHTML image references"));
                    }
                    let index = images.len();
                    let mut buffer = String::new();
                    crate::svg_inline::append(&mut buffer, event, true, max_bytes)?;
                    images.push(ImageReference {
                        offset: output.len(),
                        source: format!("<inline-svg-{index}>"),
                        alt: "SVG illustration".into(),
                        width: None,
                        height: None,
                        inline_svg: None,
                    });
                    svg_capture = Some((current, index, buffer));
                }
                if name == "body" && body_depth.is_none() {
                    body_depth = Some(current);
                    seen_body = true;
                } else if body_depth.is_some() && suppressed_depth.is_none() && is_suppressed(name)
                {
                    suppressed_depth = Some(current);
                } else if body_depth.is_some() && suppressed_depth.is_none() {
                    if name == "br" {
                        line_break(&mut output, max_bytes)?;
                        pending_space = false;
                    } else if is_block(name) {
                        block_break(&mut output, max_bytes)?;
                        pending_space = false;
                    }
                }
                let boxed = sheet.is_some()
                    && body_depth.is_some()
                    && suppressed_depth.is_none()
                    && svg_capture.is_none()
                    && !style.hidden
                    && (is_block(name) || name == "body")
                    && box_style.active();
                if boxed {
                    if blocks.len() >= 100_000 {
                        return Err(EpubError::LimitExceeded("XHTML block boundaries"));
                    }
                    blocks.push(BlockBoundary {
                        offset: output.len(),
                        images_before: images.len(),
                        style: Some(box_style),
                    });
                }
                if body_depth.is_some()
                    && suppressed_depth.is_none()
                    && !is_suppressed(name)
                    && let Some(id) = element.attribute("id")
                {
                    record_anchor(&mut anchors, id, output.len())?;
                }
                if name == "a"
                    && body_depth.is_some()
                    && suppressed_depth.is_none()
                    && let Some(name) = element.attribute("name")
                {
                    record_anchor(&mut named_anchors, name, output.len())?;
                }
                if anchors.len().saturating_add(named_anchors.len()) > 16_384 {
                    return Err(EpubError::LimitExceeded("XHTML anchor metadata"));
                }
                if sheet.is_some()
                    && name == "img"
                    && svg_capture.is_none()
                    && body_depth.is_some()
                    && suppressed_depth.is_none()
                    && !style.hidden
                {
                    if images.len() >= 1024 {
                        return Err(EpubError::LimitExceeded("XHTML image references"));
                    }
                    images.push(ImageReference {
                        offset: output.len(),
                        inline_svg: None,
                        source: element.attribute("src").unwrap_or("").to_owned(),
                        alt: element
                            .attribute("alt")
                            .unwrap_or("Image")
                            .chars()
                            .take(256)
                            .collect(),
                        width: element
                            .attribute("width")
                            .and_then(|v| v.parse::<u32>().ok())
                            .filter(|v| *v > 0),
                        height: element
                            .attribute("height")
                            .and_then(|v| v.parse::<u32>().ok())
                            .filter(|v| *v > 0),
                    });
                }
                if sheet.is_some()
                    && name == "a"
                    && body_depth.is_some()
                    && suppressed_depth.is_none()
                    && svg_capture.is_none()
                    && !style.hidden
                {
                    links.start(element, current, output.len(), images.len())?;
                }
                if !element.empty {
                    styles.push(style);
                    open_blocks.push(boxed);
                    depth = current;
                } else {
                    if boxed {
                        blocks.push(BlockBoundary {
                            offset: output.len(),
                            images_before: images.len(),
                            style: None,
                        });
                    }
                    if body_depth == Some(current) && name == "body" {
                        body_depth = None;
                    }
                    if suppressed_depth == Some(current) {
                        suppressed_depth = None;
                    }
                }
            }
            Event::Text(text) if body_depth.is_some() && suppressed_depth.is_none() => {
                let start = output.len();
                append_collapsed(&mut output, text, &mut pending_space, max_bytes)?;
                if sheet.is_some() && output.len() > start {
                    let mut style = *styles.last().unwrap_or(&TextStyle::default());
                    if svg_capture.is_some() {
                        style.hidden = true;
                    }
                    if let Some(last) = runs
                        .last_mut()
                        .filter(|last| last.range.end == start && last.style == style)
                    {
                        last.range.end = output.len();
                    } else {
                        if runs.len() >= 100_000 {
                            return Err(EpubError::LimitExceeded("XHTML style runs"));
                        }
                        runs.push(StyleRun {
                            range: start..output.len(),
                            style,
                        });
                    }
                }
            }
            Event::Text(_) => {}
            Event::End(name) => {
                if local_name(name) == "a" {
                    links.end(depth, output.len(), images.len());
                }
                if open_blocks.pop() == Some(true) {
                    blocks.push(BlockBoundary {
                        offset: output.len(),
                        images_before: images.len(),
                        style: None,
                    });
                }
                let local = local_name(name);
                if suppressed_depth == Some(depth) {
                    suppressed_depth = None;
                } else if body_depth.is_some() && is_block(local) {
                    block_break(&mut output, max_bytes)?;
                    pending_space = false;
                }
                if body_depth == Some(depth) && local == "body" {
                    body_depth = None;
                }
                depth = depth.saturating_sub(1);
                if styles.len() > 1 {
                    styles.pop();
                }
            }
        }
        let closed_svg = svg_capture.as_ref().is_some_and(|(level, _, _)| {
            *level == depth + 1
                && match event {
                    Event::End(name) => local_name(name) == "svg",
                    Event::Start(element) => element.empty && local_name(&element.name) == "svg",
                    _ => false,
                }
        });
        if closed_svg && let Some((_, index, buffer)) = svg_capture.take() {
            images[index].inline_svg = Some(buffer);
        }
    }
    if !seen_body {
        return Err(EpubError::Invalid("XHTML body element is absent"));
    }
    while output.ends_with([' ', '\n']) {
        output.pop();
    }
    // HTML fragment lookup prefers any matching id over legacy <a name>, even
    // when a same-named legacy anchor appears earlier in document order.
    for (name, offset) in named_anchors {
        record_anchor(&mut anchors, &name, offset)?;
    }
    for (_, offset) in &mut anchors {
        *offset = (*offset).min(output.len());
    }
    for run in &mut runs {
        run.range.end = run.range.end.min(output.len());
    }
    runs.retain(|run| !run.range.is_empty());
    for image in &mut images {
        image.offset = image.offset.min(output.len());
    }
    for boundary in &mut blocks {
        boundary.offset = boundary.offset.min(output.len());
    }
    let links = links.finish(output.len());
    Ok(ExtractedText {
        text: output,
        anchors,
        runs,
        images,
        blocks,
        links,
    })
}

fn record_anchor(anchors: &mut Vec<(String, usize)>, id: &str, offset: usize) -> Result<()> {
    if id.is_empty() || anchors.iter().any(|(existing, _)| existing == id) {
        return Ok(());
    }
    if id.len() > 4096 || anchors.len() >= 16_384 {
        return Err(EpubError::LimitExceeded("XHTML anchor metadata"));
    }
    let mut value = String::new();
    value
        .try_reserve_exact(id.len())
        .map_err(|_| EpubError::AllocationFailed)?;
    value.push_str(id);
    anchors
        .try_reserve(1)
        .map_err(|_| EpubError::AllocationFailed)?;
    anchors.push((value, offset));
    Ok(())
}

fn is_suppressed(name: &str) -> bool {
    matches!(name, "script" | "style" | "template" | "head")
}

pub(crate) fn is_block(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "div"
            | "dl"
            | "dt"
            | "dd"
            | "figcaption"
            | "figure"
            | "footer"
            | "header"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "section"
            | "table"
            | "tr"
            | "ul"
    )
}

fn append_collapsed(
    output: &mut String,
    text: &str,
    pending_space: &mut bool,
    limit: usize,
) -> Result<()> {
    for ch in text.chars() {
        if is_collapsible_whitespace(ch) {
            *pending_space = !output.is_empty() && !output.ends_with('\n');
            continue;
        }
        if *pending_space && !output.is_empty() && !output.ends_with([' ', '\n']) {
            push(output, ' ', limit)?;
        }
        *pending_space = false;
        push(output, ch, limit)?;
    }
    Ok(())
}

fn is_collapsible_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r')
}

fn block_break(output: &mut String, limit: usize) -> Result<()> {
    while output.ends_with(' ') {
        output.pop();
    }
    if !output.is_empty() && !output.ends_with("\n\n") {
        if !output.ends_with('\n') {
            push(output, '\n', limit)?;
        }
        push(output, '\n', limit)?;
    }
    Ok(())
}

fn line_break(output: &mut String, limit: usize) -> Result<()> {
    while output.ends_with(' ') {
        output.pop();
    }
    if !output.is_empty() && !output.ends_with('\n') {
        push(output, '\n', limit)?;
    }
    Ok(())
}

fn push(output: &mut String, ch: char, limit: usize) -> Result<()> {
    if output.len().saturating_add(ch.len_utf8()) > limit {
        return Err(EpubError::LimitExceeded("extracted XHTML text bytes"));
    }
    output
        .try_reserve(ch.len_utf8())
        .map_err(|_| EpubError::AllocationFailed)?;
    output.push(ch);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_blocks_entities_and_inline_text_are_extracted() {
        let text = extract(
            br#"<?xml version="1.0"?><html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Hidden title</title><style>p{color:red}</style></head>
<body><h1>Chapter &amp; One</h1><p>Hello <em>world</em>!<br/>Next line.</p>
<ul><li>First</li><li>Second</li></ul></body></html>"#,
            4096,
        )
        .unwrap();
        assert_eq!(
            text,
            "Chapter & One\n\nHello world!\nNext line.\n\nFirst\n\nSecond"
        );
    }

    #[test]
    fn element_ids_map_to_canonical_text_offsets() {
        let extracted = extract_with_anchors(
            br#"<html><body id="top"><p>Before</p><section id="target"><h2 xml:id="heading">Target</h2><p>After</p></section></body></html>"#,
            4096,
        )
        .unwrap();
        assert_eq!(extracted.anchor("top"), Some(0));
        let target = extracted.anchor("target").unwrap();
        let heading = extracted.anchor("heading").unwrap();
        assert_eq!(&extracted.text[target..target + "Target".len()], "Target");
        assert_eq!(target, heading);
        assert!(extracted.anchor("missing").is_none());
    }

    #[test]
    fn common_xhtml_doctype_does_not_block_body_text_extraction() {
        let text = extract(
            br#"<?xml version="1.0"?><!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd"><html xmlns="http://www.w3.org/1999/xhtml"><body><p>Chapter text</p></body></html>"#,
            1024,
        )
        .unwrap();
        assert_eq!(text, "Chapter text");
    }

    #[test]
    fn script_style_and_template_text_are_not_reader_content() {
        let text = extract(
            br#"<html><body>A<script>evil()</script><style>hidden</style><template>later</template>B</body></html>"#,
            1024,
        )
        .unwrap();
        assert_eq!(text, "AB");
    }

    #[test]
    fn legacy_named_entities_preserve_non_breaking_space_in_reader_text() {
        let text = extract(
            b"<!DOCTYPE html><html><body><p>A&nbsp;B&mdash;C&hellip;</p></body></html>",
            1024,
        )
        .unwrap();
        assert_eq!(text, "A\u{00a0}B—C…");
    }

    #[test]
    fn whitespace_is_deterministic_and_bounded() {
        assert_eq!(
            extract(b"<html><body><p>  A \n B\t C </p></body></html>", 128).unwrap(),
            "A B C"
        );
        assert!(matches!(
            extract(b"<html><body>abcdef</body></html>", 3),
            Err(EpubError::LimitExceeded(_))
        ));
    }

    #[test]
    fn malformed_or_bodyless_documents_fail() {
        for bytes in [
            b"<svg/>".as_slice(),
            b"<html><head><title>x</title></head></html>",
            b"<html><body><p>x</body></html>",
        ] {
            assert!(extract(bytes, 1024).is_err());
        }
    }
}

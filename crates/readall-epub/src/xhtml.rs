//! Small XHTML reading subset: extract body text with deterministic block breaks.
//! This is not HTML error recovery, CSS layout, scripting, SVG, MathML, or image rendering.

use crate::{
    EpubError, Result,
    xml::{self, Event, XmlLimits, local_name},
};

pub(crate) fn extract(bytes: &[u8], max_bytes: usize) -> Result<String> {
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

    let mut depth = 0_usize;
    let mut body_depth = None;
    let mut suppressed_depth = None;
    let mut seen_body = false;
    let mut pending_space = false;

    for event in &events {
        match event {
            Event::Start(element) => {
                let current = depth + 1;
                let name = local_name(&element.name);
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
                if !element.empty {
                    depth = current;
                } else if body_depth == Some(current) && name == "body" {
                    body_depth = None;
                }
            }
            Event::Text(text) if body_depth.is_some() && suppressed_depth.is_none() => {
                append_collapsed(&mut output, text, &mut pending_space, max_bytes)?;
            }
            Event::Text(_) => {}
            Event::End(name) => {
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
            }
        }
    }
    if !seen_body {
        return Err(EpubError::Invalid("XHTML body element is absent"));
    }
    while output.ends_with([' ', '\n']) {
        output.pop();
    }
    Ok(output)
}

fn is_suppressed(name: &str) -> bool {
    matches!(name, "script" | "style" | "template" | "head")
}

fn is_block(name: &str) -> bool {
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
        if ch.is_whitespace() {
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

//! Serialize one parsed inline SVG subtree, never raw XML concatenation from attributes.
use crate::{
    EpubError, Result,
    xml::{Event, local_name},
};
fn escape(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}
pub(crate) fn append(out: &mut String, event: &Event, root: bool, limit: usize) -> Result<()> {
    match event {
        Event::Start(element) => {
            out.push('<');
            out.push_str(local_name(&element.name));
            if root {
                out.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
            }
            for (name, value) in &element.attributes {
                if name == "xmlns" || name.starts_with("xmlns:") {
                    continue;
                }
                out.push(' ');
                out.push_str(if name.starts_with("xml:") {
                    name
                } else {
                    local_name(name)
                });
                out.push_str("=\"");
                escape(out, value);
                out.push('"');
            }
            out.push_str(if element.empty { "/>" } else { ">" });
        }
        Event::End(name) => {
            out.push_str("</");
            out.push_str(local_name(name));
            out.push('>');
        }
        Event::Text(text) => escape(out, text),
    }
    if out.len() > limit.min(4 * 1024 * 1024) {
        return Err(EpubError::LimitExceeded("inline SVG bytes"));
    }
    Ok(())
}

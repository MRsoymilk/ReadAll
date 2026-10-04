//! Optional code metadata over the existing logical text: no added characters,
//! no tokenizer execution, no changes to style runs or locator version selection.
use super::is_block;
use crate::{
    css::TextStyle,
    xml::{Element, local_name},
};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlock {
    pub range: Range<usize>,
    /// Explicit source language, or None for conservative reader-side detection.
    /// Unknown declarations remain explicit; "text" disables automatic coloring.
    pub language: Option<String>,
}
#[derive(Default)]
pub(super) struct CodeCollector {
    blocks: Vec<CodeBlock>,
    active: Option<(usize, CodeBlock)>,
}
impl CodeCollector {
    pub fn start(&mut self, element: &Element, depth: usize, offset: usize, style: TextStyle) {
        if element.empty || style.hidden {
            return;
        }
        let name = local_name(&element.name);
        if let Some((_, block)) = &mut self.active {
            if name == "code" && block.language.as_deref() != Some("text") {
                // A code child's declaration takes precedence over its pre wrapper.
                if let Some(language) = language(element) {
                    block.language = Some(language);
                }
            }
            return;
        }
        if self.blocks.len() >= 1024 {
            return;
        }
        if name == "pre"
            || style.white_space.preserves_breaks() && (name == "code" || is_block(name))
        {
            self.active = Some((
                depth,
                CodeBlock {
                    range: offset..offset,
                    language: language(element),
                },
            ));
        }
    }
    pub fn end(&mut self, depth: usize, offset: usize) {
        if self
            .active
            .as_ref()
            .is_some_and(|(level, _)| *level == depth)
            && let Some((_, mut block)) = self.active.take()
        {
            block.range.end = offset;
            if !block.range.is_empty() {
                self.blocks.push(block);
            }
        }
    }
    pub fn finish(mut self, text_length: usize) -> Vec<CodeBlock> {
        for block in &mut self.blocks {
            block.range.end = block.range.end.min(text_length);
        }
        self.blocks.retain(|block| !block.range.is_empty());
        self.blocks
    }
}
fn checked(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= 64
        && value.is_ascii()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_+-./#".contains(&b)))
    .then(|| value.to_ascii_lowercase())
}
fn language(element: &Element) -> Option<String> {
    let classes = element.attribute("class").unwrap_or("");
    if classes
        .split_ascii_whitespace()
        .any(|c| matches!(c, "nohighlight" | "no-highlight"))
    {
        return Some("text".into());
    }
    for key in ["data-language", "data-lang"] {
        if let Some(value) = element.attribute(key) {
            // An invalid/oversized explicit declaration must not trigger guessing.
            return Some(checked(value).unwrap_or_else(|| "text".into()));
        }
    }
    for class in classes.split_ascii_whitespace() {
        if let Some(value) = class
            .strip_prefix("language-")
            .or_else(|| class.strip_prefix("lang-"))
            .or_else(|| class.strip_prefix("highlight-source-"))
        {
            return Some(checked(value).unwrap_or_else(|| "text".into()));
        }
    }
    // Pandoc/Pygments also emit bare language classes next to sourceCode/highlight.
    classes
        .split_ascii_whitespace()
        .find(|c| {
            matches!(
                *c,
                "c" | "cpp"
                    | "c++"
                    | "rust"
                    | "python"
                    | "bash"
                    | "sh"
                    | "javascript"
                    | "typescript"
                    | "json"
                    | "plaintext"
            )
        })
        .and_then(checked)
}

#[cfg(test)]
mod tests {
    use crate::{css::StyleSheet, xhtml::extract_styled};
    fn extract(source: &str) -> super::super::ExtractedText {
        extract_styled(
            source.as_bytes(),
            1024 * 1024,
            Some(&mut StyleSheet::default()),
        )
        .unwrap()
    }
    #[test]
    fn metadata_keeps_code_ranges_and_does_not_capture_inline_or_hidden_text() {
        let content = extract(
            "<html><body><p id='before'>Before <code>inline</code></p><pre class='language-c'><code class='language-cpp'>int <span>main</span>() {\n  return 0;\n}</code></pre><pre hidden='hidden'>secret</pre><p id='after'>After</p></body></html>",
        );
        assert_eq!(content.codes.len(), 1);
        let code = &content.codes[0];
        assert_eq!(code.language.as_deref(), Some("cpp"));
        assert_eq!(
            &content.text[code.range.clone()],
            "int main() {\n  return 0;\n}"
        );
        assert_eq!(content.anchor("before"), Some(0));
        assert_eq!(content.anchor("after"), content.text.find("After"));
        assert!(content.text.contains("Before inline"));
    }
    #[test]
    fn declarations_plaintext_and_preformatted_css_containers_are_preserved() {
        for (attribute, language) in [
            ("class='lang-rust'", "rust"),
            ("class='sourceCode python'", "python"),
            ("data-language='C++'", "c++"),
            ("data-lang='bash'", "bash"),
            ("class='language-madeup'", "madeup"),
        ] {
            let content = extract(&format!(
                "<html><body><div style='white-space:pre-wrap' {attribute}><code>A\nW</code></div></body></html>"
            ));
            assert_eq!(content.codes[0].language.as_deref(), Some(language));
            assert_eq!(&content.text[content.codes[0].range.clone()], "A\nW");
        }
        let content = extract(
            "<html><body><pre class='nohighlight'><code class='language-c'>int x;</code></pre><pre>A</pre><pre/><code>B</code></body></html>",
        );
        assert_eq!(content.codes.len(), 2);
        assert_eq!(content.codes[0].language.as_deref(), Some("text"));
        assert_eq!(content.codes[1].language, None);
        assert!(
            content
                .codes
                .windows(2)
                .all(|p| p[0].range.end <= p[1].range.start)
        );
    }
    #[test]
    fn metadata_limit_does_not_reject_the_document() {
        let source = format!("<html><body>{}</body></html>", "<pre>A</pre>".repeat(1030));
        let content = extract(&source);
        assert_eq!(content.codes.len(), 1024);
        assert_eq!(content.text.matches('A').count(), 1030);
    }
}

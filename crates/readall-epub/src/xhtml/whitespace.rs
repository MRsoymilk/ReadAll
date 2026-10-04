//! Whitespace handling across inline syntax spans. Structural separators may be
//! trimmed; source whitespace in preformatted content must not be trimmed with them.
use super::{Result, append_collapsed, push};
use crate::css::WhiteSpace;

#[derive(Default)]
pub(super) struct Whitespace {
    pub protected_end: usize,
    previous_cr: bool,
}
impl Whitespace {
    pub fn append(
        &mut self,
        output: &mut String,
        text: &str,
        mode: WhiteSpace,
        pending: &mut bool,
        limit: usize,
    ) -> Result<()> {
        if !mode.preserves_breaks() {
            self.previous_cr = false;
            return append_collapsed(output, text, pending, limit);
        }
        for ch in text.chars() {
            if ch == '\n' && self.previous_cr {
                self.previous_cr = false;
                continue;
            }
            self.previous_cr = ch == '\r';
            let ch = if ch == '\r' { '\n' } else { ch };
            if mode.preserves_spaces() {
                if *pending && ch != '\n' && !output.is_empty() && !output.ends_with([' ', '\n']) {
                    push(output, ' ', limit)?;
                }
                *pending = false;
                push(output, ch, limit)?;
                self.protected_end = output.len();
            } else if ch == '\n' {
                *pending = false;
                // pre-line collapses indentation/spaces, but every source newline
                // (including consecutive newlines) is a real line break.
                push(output, '\n', limit)?;
                self.protected_end = output.len();
            } else {
                let mut buf = [0; 4];
                append_collapsed(output, ch.encode_utf8(&mut buf), pending, limit)?;
            }
        }
        Ok(())
    }
    pub fn block_break(&mut self, output: &mut String, limit: usize) -> Result<()> {
        self.previous_cr = false;
        while output.len() > self.protected_end && output.ends_with(' ') {
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
    pub fn explicit_break(&mut self, output: &mut String, limit: usize) -> Result<()> {
        self.previous_cr = false;
        while output.len() > self.protected_end && output.ends_with(' ') {
            output.pop();
        }
        push(output, '\n', limit)?;
        self.protected_end = output.len();
        Ok(())
    }
}

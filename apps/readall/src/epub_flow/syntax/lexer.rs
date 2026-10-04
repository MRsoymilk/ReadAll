//! Small bounded lexical highlighter, not a compiler/parser. All ranges index
//! original UTF-8. Strings/comments are scanned before identifiers; state spans
//! a whole code block, never an individual displayed page.
use super::{Kind, Token};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Language {
    Cpp,
    Rust,
    Python,
    Shell,
    JavaScript,
    Json,
}
impl Language {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "c" | "h" | "cc" | "cpp" | "c++" | "cxx" | "hpp" | "objective-c" => Self::Cpp,
            "rust" | "rs" => Self::Rust,
            "python" | "py" | "python3" => Self::Python,
            "sh" | "shell" | "bash" | "zsh" | "shellscript" => Self::Shell,
            "js" | "javascript" | "ts" | "typescript" => Self::JavaScript,
            "json" | "jsonc" => Self::Json,
            _ => return None,
        })
    }
    pub fn detect(text: &str) -> Option<Self> {
        let mut end = text.len().min(16 * 1024);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let text = &text[..end];
        for line in text.lines().take(160).map(str::trim_start) {
            if line.starts_with("#!") {
                if line.contains("python") {
                    return Some(Self::Python);
                }
                if line.contains("/sh")
                    || line.contains("bash")
                    || line.contains("zsh")
                    || line.ends_with(" sh")
                {
                    return Some(Self::Shell);
                }
            }
            if ["#include ", "#include<", "#define ", "#ifdef ", "#ifndef "]
                .iter()
                .any(|p| line.starts_with(p))
            {
                return Some(Self::Cpp);
            }
            if ["fn ", "pub fn ", "pub async fn ", "use std::", "let mut "]
                .iter()
                .any(|p| line.starts_with(p))
            {
                return Some(Self::Rust);
            }
            if (line.starts_with("def ")
                || line.starts_with("async def ")
                || line.starts_with("class "))
                && line.trim_end().ends_with(':')
                || line.starts_with("from ") && line.contains(" import ")
            {
                return Some(Self::Python);
            }
            if line.starts_with("function ")
                || line.starts_with("console.log(")
                || (line.starts_with("const ") || line.starts_with("let "))
                    && line.contains(" = ")
                    && (line.ends_with(';') || line.contains("=>"))
            {
                return Some(Self::JavaScript);
            }
            if (line.starts_with("int ")
                || line.starts_with("void ")
                || line.starts_with("static ")
                || line.starts_with("unsigned "))
                && (line.contains('(') || line.contains(';'))
            {
                return Some(Self::Cpp);
            }
        }
        let trimmed = text.trim();
        if (trimmed.starts_with('{') && trimmed.ends_with('}')
            || trimmed.starts_with('[') && trimmed.ends_with(']'))
            && trimmed.contains("\":")
        {
            return Some(Self::Json);
        }
        None
    }
    fn c_comments(self) -> bool {
        matches!(self, Self::Cpp | Self::Rust | Self::JavaScript | Self::Json)
    }
    fn word(self, text: &str) -> Option<Kind> {
        let (keywords, types, constants) = match self {
            Self::Cpp => (
                "alignas alignof asm auto break case catch class concept const constexpr consteval constinit const_cast continue co_await co_return co_yield decltype default delete do dynamic_cast else enum explicit export extern final for friend goto if inline mutable namespace new noexcept operator override private protected public register reinterpret_cast requires return sizeof static static_assert static_cast struct switch template this thread_local throw try typedef typeid typename union using virtual volatile while",
                "bool char char8_t char16_t char32_t double float int long short signed unsigned void wchar_t size_t ssize_t ptrdiff_t uint8_t uint16_t uint32_t uint64_t int8_t int16_t int32_t int64_t",
                "true false nullptr NULL",
            ),
            Self::Rust => (
                "as async await break const continue crate dyn else enum extern fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait type unsafe use where while yield",
                "bool char str u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize f32 f64 String Vec Option Result",
                "true false Some None Ok Err",
            ),
            Self::Python => (
                "and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield match case",
                "bool bytes bytearray complex dict float frozenset int list object range set str tuple",
                "True False None Ellipsis NotImplemented",
            ),
            Self::Shell => (
                "if then else elif fi case esac for select while until do done in function time export local readonly declare typeset unset return exit break continue",
                "",
                "true false",
            ),
            Self::JavaScript => (
                "as async await break case catch class const continue debugger declare default delete do else enum export extends finally for from function get if implements import in instanceof interface keyof let new of private protected public readonly return set static super switch this throw try type typeof var void while with yield",
                "any bigint boolean never number object string symbol unknown undefined",
                "true false null NaN Infinity",
            ),
            Self::Json => ("", "", "true false null"),
        };
        if constants.split_ascii_whitespace().any(|w| w == text) {
            Some(Kind::Constant)
        } else if types.split_ascii_whitespace().any(|w| w == text) {
            Some(Kind::Type)
        } else if keywords.split_ascii_whitespace().any(|w| w == text) {
            Some(Kind::Keyword)
        } else {
            None
        }
    }
}

/// None means the token budget was exceeded: callers discard this block's partial
/// result and preserve plain text instead of exposing a half-highlighted block.
pub(super) fn scan(
    text: &str,
    base: usize,
    language: Language,
    max_tokens: usize,
) -> Option<Vec<Token>> {
    let bytes = text.as_bytes();
    let (mut at, mut line_prefix, mut include) = (0, true, false);
    let mut tokens = Vec::new();
    while at < bytes.len() {
        let start = at;
        let b = bytes[at];
        if b.is_ascii_whitespace() {
            if b == b'\n' || b == b'\r' {
                line_prefix = true;
                include = false;
            }
            at += 1;
            continue;
        }
        let mut kind = None;
        if language.c_comments() && bytes[at..].starts_with(b"//") {
            at = line_end(bytes, at, language == Language::Cpp);
            kind = Some(Kind::Comment);
        } else if language.c_comments() && bytes[at..].starts_with(b"/*") {
            at = block_comment(bytes, at, language == Language::Rust);
            kind = Some(Kind::Comment);
        } else if b == b'#'
            && (language == Language::Python
                || language == Language::Shell
                    && (at == 0
                        || bytes[at - 1].is_ascii_whitespace()
                        || b";|&()".contains(&bytes[at - 1])))
        {
            at = line_end(bytes, at, false);
            kind = Some(Kind::Comment);
        } else if language == Language::Cpp && b == b'#' && line_prefix {
            at += 1;
            while bytes.get(at).is_some_and(|b| matches!(b, b' ' | b'\t')) {
                at += 1;
            }
            let word = at;
            while bytes.get(at).is_some_and(u8::is_ascii_alphabetic) {
                at += 1;
            }
            include = matches!(&text[word..at], "include" | "include_next");
            kind = Some(Kind::Directive);
        } else if language == Language::Cpp && include && b == b'<' {
            at += 1;
            while at < bytes.len() && !matches!(bytes[at], b'>' | b'\n') {
                at += 1;
            }
            if bytes.get(at) == Some(&b'>') {
                at += 1;
            }
            kind = Some(Kind::String);
        } else if let Some(end) = raw_string(text, at, language) {
            at = end;
            kind = Some(Kind::String);
        } else if language == Language::Python
            && let Some(quote) = python_prefix(bytes, at)
        {
            at = quoted(bytes, quote, bytes[quote], true, true, false);
            kind = Some(Kind::String);
        } else if b == b'"'
            || b == b'\''
                && language != Language::Json
                && (language != Language::Rust || rust_char(bytes, at))
            || b == b'`' && matches!(language, Language::JavaScript | Language::Shell)
        {
            at = quoted(
                bytes,
                at,
                b,
                language == Language::Python,
                language != Language::Shell || b != b'\'',
                language == Language::Shell,
            );
            kind = Some(Kind::String);
        } else if language == Language::Rust
            && b == b'\''
            && text[at + 1..].chars().next().is_some_and(identifier_start)
        {
            at = identifier_end(text, at + 1);
            kind = Some(Kind::Type);
        } else if language == Language::Shell && b == b'\\' {
            at += 1;
            if at < bytes.len() {
                at += text[at..].chars().next().unwrap().len_utf8();
            }
        } else if language == Language::Shell && b == b'$' {
            at += 1;
            if bytes.get(at) == Some(&b'{') {
                at += 1;
                while at < bytes.len() && !matches!(bytes[at], b'}' | b'\n') {
                    at += 1;
                }
                if bytes.get(at) == Some(&b'}') {
                    at += 1;
                }
            } else if at < bytes.len()
                && (bytes[at].is_ascii_digit() || b"?@#*$!-".contains(&bytes[at]))
            {
                at += 1;
            } else {
                at = identifier_end(text, at);
            }
            kind = Some(Kind::Constant);
        } else if b.is_ascii_digit()
            || b == b'.'
                && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)
                && (at == 0 || bytes[at - 1] != b'.')
        {
            at = number_end(bytes, at);
            kind = Some(Kind::Number);
        } else if text[at..].chars().next().is_some_and(identifier_start) {
            at = identifier_end(text, at);
            let word = &text[start..at];
            kind = language.word(word);
            if kind.is_none() && language != Language::Json {
                let mut next = at;
                while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
                    next += 1;
                }
                if bytes.get(next) == Some(&b'(')
                    || language == Language::Rust && bytes.get(next) == Some(&b'!')
                {
                    kind = Some(Kind::Function);
                } else if word.bytes().any(|b| b.is_ascii_uppercase())
                    && word
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                {
                    kind = Some(Kind::Constant);
                } else if language == Language::Shell
                    && matches!(
                        word,
                        "echo" | "printf" | "read" | "cd" | "pwd" | "test" | "exec"
                    )
                {
                    kind = Some(Kind::Function);
                }
            }
        } else {
            at += text[at..].chars().next().unwrap().len_utf8();
        }
        line_prefix = false;
        if let Some(kind) = kind {
            if tokens.len() >= max_tokens {
                return None;
            }
            tokens.push(Token {
                range: base + start..base + at,
                kind,
            });
        }
    }
    Some(tokens)
}
fn identifier_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphabetic()
}
fn identifier_end(text: &str, at: usize) -> usize {
    let mut end = at;
    for ch in text[at..].chars() {
        if ch == '_' || ch == '$' || ch.is_alphanumeric() {
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    end
}
fn line_end(bytes: &[u8], mut at: usize, continuations: bool) -> usize {
    while at < bytes.len() {
        if bytes[at] == b'\n' {
            let previous = if at > 0 && bytes[at - 1] == b'\r' {
                at - 1
            } else {
                at
            };
            if !continuations || previous == 0 || bytes[previous - 1] != b'\\' {
                break;
            }
        }
        at += 1;
    }
    at
}
fn block_comment(bytes: &[u8], mut at: usize, nested: bool) -> usize {
    at += 2;
    let mut depth = 1;
    while at < bytes.len() {
        if bytes[at..].starts_with(b"*/") {
            depth -= 1;
            at += 2;
            if depth == 0 {
                break;
            }
        } else if nested && bytes[at..].starts_with(b"/*") {
            depth += 1;
            at += 2;
        } else {
            at += 1;
        }
    }
    at
}
fn quoted(
    bytes: &[u8],
    start: usize,
    quote: u8,
    triple: bool,
    escapes: bool,
    multiline: bool,
) -> usize {
    let count = if triple && bytes.get(start..start + 3) == Some(&[quote; 3]) {
        3
    } else {
        1
    };
    let mut at = start + count;
    while at < bytes.len() {
        if bytes[at] == b'\\' && escapes {
            at = (at + 2).min(bytes.len());
        } else if bytes
            .get(at..at + count)
            .is_some_and(|p| p.iter().all(|b| *b == quote))
        {
            return at + count;
        } else if bytes[at] == b'\n' && count == 1 && quote != b'`' && !multiline {
            return at;
        } else {
            at += 1;
        }
    }
    at
}
fn python_prefix(bytes: &[u8], start: usize) -> Option<usize> {
    let mut end = start;
    while end < (start + 3).min(bytes.len()) && b"rRuUbBfF".contains(&bytes[end]) {
        end += 1;
    }
    (end > start && bytes.get(end).is_some_and(|b| matches!(b, b'\'' | b'"'))).then_some(end)
}
fn rust_char(bytes: &[u8], start: usize) -> bool {
    let tail = &bytes[start + 1..];
    if tail.first() == Some(&b'\\') {
        tail.iter().take(14).skip(1).any(|b| *b == b'\'')
    } else {
        // Input is already UTF-8. Inspect only the leading scalar instead of
        // validating the whole remaining block for every lifetime apostrophe.
        let width = match tail.first() {
            Some(0x00..=0x7f) => 1,
            Some(0xc0..=0xdf) => 2,
            Some(0xe0..=0xef) => 3,
            Some(0xf0..=0xf4) => 4,
            _ => return false,
        };
        tail.get(width) == Some(&b'\'')
    }
}
fn raw_string(text: &str, start: usize, language: Language) -> Option<usize> {
    let bytes = text.as_bytes();
    if language == Language::Rust {
        let prefix = ["br", "cr", "r"]
            .into_iter()
            .find(|p| text[start..].starts_with(p))?;
        let mut quote = start + prefix.len();
        while bytes.get(quote) == Some(&b'#') && quote - start <= 257 {
            quote += 1;
        }
        let hashes = quote - start - prefix.len();
        if hashes > 255 || bytes.get(quote) != Some(&b'"') {
            return None;
        }
        let end = format!("\"{}", "#".repeat(hashes));
        return Some(
            text[quote + 1..]
                .find(&end)
                .map_or(bytes.len(), |n| quote + 1 + n + end.len()),
        );
    }
    if language == Language::Cpp {
        let prefix = ["u8R\"", "uR\"", "UR\"", "LR\"", "R\""]
            .into_iter()
            .find(|p| text[start..].starts_with(p))?;
        let delimiter = start + prefix.len();
        let mut open = delimiter;
        while open < bytes.len() && open - delimiter <= 16 && bytes[open] != b'(' {
            if bytes[open].is_ascii_whitespace()
                || !bytes[open].is_ascii()
                || b"\\)".contains(&bytes[open])
            {
                return None;
            }
            open += 1;
        }
        if open - delimiter > 16 || bytes.get(open) != Some(&b'(') {
            return None;
        }
        let end = format!("){}\"", &text[delimiter..open]);
        return Some(
            text[open + 1..]
                .find(&end)
                .map_or(bytes.len(), |n| open + 1 + n + end.len()),
        );
    }
    None
}
fn number_end(bytes: &[u8], start: usize) -> usize {
    let hex = bytes[start..].starts_with(b"0x") || bytes[start..].starts_with(b"0X");
    let mut at = start;
    let mut dot = false;
    while at < bytes.len() {
        let b = bytes[at];
        if b.is_ascii_alphanumeric() || b == b'_' {
            at += 1;
        } else if b == b'.' && !dot && bytes.get(at + 1) != Some(&b'.') {
            dot = true;
            at += 1;
        } else if (matches!(b, b'+' | b'-')
            && at > start
            && if hex {
                matches!(bytes[at - 1], b'p' | b'P')
            } else {
                matches!(bytes[at - 1], b'e' | b'E')
            })
            || (b == b'\'' && bytes.get(at + 1).is_some_and(u8::is_ascii_hexdigit))
        {
            at += 1;
        } else {
            break;
        }
    }
    at
}

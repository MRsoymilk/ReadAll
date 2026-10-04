//! Optional paint-only code highlighting. It never edits logical text, CSS runs,
//! shaped glyphs, pagination, hit boxes, copied text or serialized locators.
mod lexer;
#[cfg(test)]
mod tests;
use super::*;
use lexer::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Keyword,
    Type,
    Comment,
    String,
    Number,
    Function,
    Directive,
    Constant,
}
#[derive(Debug, Clone)]
struct Token {
    range: Range<usize>,
    kind: Kind,
}
#[derive(Default)]
pub(super) struct Syntax {
    tokens: Vec<Token>,
    skipped: usize,
}
#[derive(Clone, Copy)]
struct Budget {
    block_bytes: usize,
    chapter_bytes: usize,
    tokens: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            block_bytes: 256 * 1024,
            chapter_bytes: 1024 * 1024,
            tokens: 65_536,
        }
    }
}
impl Syntax {
    pub(super) fn build(content: &ChapterContent) -> Result<Self> {
        Self::bounded(content, Budget::default())
    }
    fn bounded(content: &ChapterContent, budget: Budget) -> Result<Self> {
        let mut result = Self::default();
        let mut bytes = 0_usize;
        for (index, block) in content.codes.iter().enumerate() {
            crate::loading::step("代码语法高亮", index, content.codes.len())?;
            let Some(raw) = content.text.get(block.range.clone()) else {
                continue;
            };
            if raw.len() > budget.block_bytes
                || bytes.saturating_add(raw.len()) > budget.chapter_bytes
            {
                result.skipped += 1;
                continue;
            }
            bytes += raw.len();
            let start = content
                .runs
                .partition_point(|run| run.range.end <= block.range.start);
            let runs = &content.runs[start..];
            // Preserve author-provided multicolor syntax rather than applying a
            // second scheme over it. A single base text color is not syntax coloring.
            let mut base_color = None;
            let mut authored = false;
            for run in runs
                .iter()
                .take_while(|run| run.range.start < block.range.end)
            {
                if run.style.hidden {
                    continue;
                }
                let range =
                    run.range.start.max(block.range.start)..run.range.end.min(block.range.end);
                if !content.text[range].chars().any(|ch| !ch.is_whitespace()) {
                    continue;
                }
                if base_color.is_some_and(|color| color != run.style.color) {
                    authored = true;
                    break;
                }
                base_color = Some(run.style.color);
            }
            if authored {
                continue;
            }
            // Hidden characters must not introduce quotes/comments into visible
            // code. Keep byte width, so every token still refers to original text.
            let mut masked = raw.as_bytes().to_vec();
            for run in runs
                .iter()
                .take_while(|run| run.range.start < block.range.end)
                .filter(|run| run.style.hidden)
            {
                let range = run.range.start.max(block.range.start) - block.range.start
                    ..run.range.end.min(block.range.end) - block.range.start;
                masked[range].fill(b' ');
            }
            let masked = std::str::from_utf8(&masked)?;
            let language = match &block.language {
                Some(name) => Language::named(name),
                None => Language::detect(masked),
            };
            let Some(language) = language else {
                continue;
            };
            match lexer::scan(
                masked,
                block.range.start,
                language,
                budget.tokens.saturating_sub(result.tokens.len()),
            ) {
                Some(tokens) => result.tokens.extend(tokens),
                None => result.skipped += 1,
            }
        }
        crate::loading::check()?;
        Ok(result)
    }
    pub(super) fn warning(&self) -> Option<String> {
        (self.skipped != 0).then(|| {
            format!(
                "{} 个代码块超过语法高亮预算，已保留普通文字显示",
                self.skipped
            )
        })
    }
    pub(super) fn painter(&self, theme: Theme) -> Painter<'_> {
        Painter {
            syntax: self,
            theme,
            colors: BTreeMap::new(),
        }
    }
}

pub(super) struct Painter<'a> {
    syntax: &'a Syntax,
    theme: Theme,
    colors: BTreeMap<(Kind, [u8; 3]), Color>,
}
impl Painter<'_> {
    pub(super) fn color(&mut self, range: &Range<usize>, style: TextStyle) -> Color {
        let at = self
            .syntax
            .tokens
            .partition_point(|token| token.range.end <= range.start);
        if let Some(token) = self
            .syntax
            .tokens
            .get(at)
            .filter(|t| t.range.start <= range.start && range.end <= t.range.end)
        {
            let paper = self.theme.colors().0;
            let background = style.backdrop.unwrap_or([paper.r, paper.g, paper.b]);
            if self.colors.len() >= 256 {
                self.colors.clear();
            }
            return *self
                .colors
                .entry((token.kind, background))
                .or_insert_with(|| palette(token.kind, background));
        }
        if style.backdrop.is_some() {
            let [r, g, b] = style.color;
            Color::rgba(r, g, b, 255)
        } else {
            self.theme.text_color(style.color)
        }
    }
}
fn luminance(rgb: [u8; 3]) -> f32 {
    let linear = rgb.map(|c| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722
}
fn contrast(a: f32, b: f32) -> f32 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
fn palette(kind: Kind, background: [u8; 3]) -> Color {
    let back = luminance(background);
    let light = contrast(back, 1.0) > contrast(back, 0.0);
    let base = match (kind, light) {
        (Kind::Keyword, false) => [132, 42, 166],
        (Kind::Keyword, true) => [196, 143, 255],
        (Kind::Type, false) => [0, 101, 131],
        (Kind::Type, true) => [95, 201, 206],
        (Kind::Comment, false) => [75, 116, 64],
        (Kind::Comment, true) => [142, 182, 129],
        (Kind::String, false) => [151, 58, 37],
        (Kind::String, true) => [239, 170, 131],
        (Kind::Number, false) => [0, 111, 105],
        (Kind::Number, true) => [100, 205, 188],
        (Kind::Function, false) => [139, 75, 19],
        (Kind::Function, true) => [235, 209, 142],
        (Kind::Directive, false) => [43, 79, 163],
        (Kind::Directive, true) => [117, 179, 249],
        (Kind::Constant, false) => [160, 48, 76],
        (Kind::Constant, true) => [245, 143, 165],
    };
    let mut rgb = base;
    // Respect author-painted backgrounds as well as the active reader theme.
    // Cache the adjusted color per kind/background, not once per visible glyph.
    for step in 1..=10 {
        if contrast(back, luminance(rgb)) >= 4.5 {
            break;
        }
        let target = if light { 255_u16 } else { 0 };
        rgb = base.map(|c| ((u16::from(c) * (10 - step) + target * step) / 10) as u8);
    }
    Color::rgba(rgb[0], rgb[1], rgb[2], 255)
}

//! Headless first-page renderer, reusable by a future native window controller.
use readall_core::{
    Limits, TextDocument, TextLocator,
    layout::{LayoutConfig, MeasuredLayout, tab_advance},
    read_bounded,
};
use readall_font::{Font, FontError, FontLimits};
use readall_platform::LocalFileSource;
use readall_render::{
    Color, DrawCommand, Rect, RenderError, RenderLimits, Surface,
    glyph::{GlyphMask, RasterLimits, rasterize},
};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    ffi::OsString,
    fmt,
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::PathBuf,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[derive(Debug)]
struct Options {
    font: PathBuf,
    face: u32,
    width: u32,
    height: u32,
    size: u32,
    margin: u32,
    page: Option<usize>,
    at: Option<TextLocator>,
    allow_missing: bool,
}
impl Options {
    fn parse(args: &[OsString]) -> Result<Self> {
        if args.len() % 2 != 0 {
            return Err("each render-text option requires a value".into());
        }
        let mut result = Self {
            font: PathBuf::new(),
            face: 0,
            width: 800,
            height: 1000,
            size: 24,
            margin: 40,
            page: None,
            at: None,
            allow_missing: false,
        };
        let mut seen = HashSet::new();
        for pair in args.chunks_exact(2) {
            let option = pair[0].to_str().ok_or("option name must be UTF-8")?;
            if !seen.insert(option) {
                return Err("duplicate render-text option".into());
            }
            match option {
                "--font" => result.font = PathBuf::from(&pair[1]),
                "--face" => result.face = number(&pair[1])?,
                "--width" => result.width = number(&pair[1])?,
                "--height" => result.height = number(&pair[1])?,
                "--font-size" => result.size = number(&pair[1])?,
                "--margin" => result.margin = number(&pair[1])?,
                "--page" => {
                    result.page = Some(
                        number(&pair[1])?
                            .checked_sub(1)
                            .ok_or("page must be at least 1")? as usize,
                    )
                }
                "--at" => {
                    result.at = Some(pair[1].to_str().ok_or("locator must be UTF-8")?.parse()?)
                }
                "--missing" => {
                    result.allow_missing = match pair[1].to_str() {
                        Some("error") => false,
                        Some("replacement") => true,
                        _ => return Err("--missing expects error or replacement".into()),
                    }
                }
                _ => return Err("unknown render-text option".into()),
            }
        }
        if result.font.as_os_str().is_empty() {
            return Err("render-text requires --font <static TrueType/TTC file>".into());
        }
        if !(128..=4096).contains(&result.width)
            || !(128..=4096).contains(&result.height)
            || !(8..=256).contains(&result.size)
            || result.margin >= result.width / 2
            || result.margin >= result.height / 2
        {
            return Err("width/height must be 128..4096, font size 8..256, and margins less than half the page size".into());
        }
        if result.page.is_some() && result.at.is_some() {
            return Err("--page and --at cannot be combined".into());
        }
        Ok(result)
    }
}
fn number(value: &OsString) -> Result<u32> {
    let text = value.to_str().ok_or("expected an unsigned integer")?;
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err("expected an unsigned integer".into());
    }
    Ok(text.parse()?)
}
#[derive(Debug)]
enum PageError {
    Font(FontError),
    Render(RenderError),
    Missing(char),
    Budget(&'static str),
}
impl fmt::Display for PageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Font(error) => error.fmt(f),
            Self::Render(error) => error.fmt(f),
            Self::Missing(ch) => write!(
                f,
                "font does not contain U+{:04X}; choose another font or explicitly use --missing replacement",
                u32::from(*ch)
            ),
            Self::Budget(what) => write!(f, "text page budget exceeded: {what}"),
        }
    }
}
impl Error for PageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Font(error) => Some(error),
            Self::Render(error) => Some(error),
            _ => None,
        }
    }
}
impl From<FontError> for PageError {
    fn from(value: FontError) -> Self {
        Self::Font(value)
    }
}
impl From<RenderError> for PageError {
    fn from(value: RenderError) -> Self {
        Self::Render(value)
    }
}
struct CachedGlyph {
    advance: f32,
    mask: Option<GlyphMask>,
}
struct GlyphCache<'f, 'data> {
    font: &'f Font<'data>,
    scale: f32,
    allow_missing: bool,
    entries: HashMap<u16, CachedGlyph>,
    missing: HashSet<char>,
    decoded_points: usize,
    mask_bytes: usize,
    raster_work: u64,
}
impl<'f, 'data> GlyphCache<'f, 'data> {
    fn new(font: &'f Font<'data>, size: u32, allow_missing: bool) -> Self {
        Self {
            font,
            scale: size as f32 / f32::from(font.metrics().units_per_em),
            allow_missing,
            entries: HashMap::new(),
            missing: HashSet::new(),
            decoded_points: 0,
            mask_bytes: 0,
            raster_work: 0,
        }
    }
    fn ensure(&mut self, ch: char) -> std::result::Result<u16, PageError> {
        let index = self.font.glyph_index(ch)?;
        if index == 0 {
            if !self.allow_missing {
                return Err(PageError::Missing(ch));
            }
            if !self.missing.contains(&ch) {
                if self.missing.len() >= 8192 {
                    return Err(PageError::Budget("missing character inventory"));
                }
                self.missing
                    .try_reserve(1)
                    .map_err(|_| PageError::Budget("missing character allocation"))?;
                self.missing.insert(ch);
            }
        }
        if !self.entries.contains_key(&index) {
            if self.entries.len() >= 8192 {
                return Err(PageError::Budget("distinct glyphs"));
            }
            let glyph = self.font.glyph(index)?;
            self.charge_points(glyph.outline.points().len())?;
            self.entries
                .try_reserve(1)
                .map_err(|_| PageError::Budget("glyph cache allocation"))?;
            self.entries.insert(
                index,
                CachedGlyph {
                    advance: f32::from(glyph.metrics.advance_width) * self.scale,
                    mask: None,
                },
            );
        }
        Ok(index)
    }
    fn charge_points(&mut self, count: usize) -> std::result::Result<(), PageError> {
        self.decoded_points = self
            .decoded_points
            .checked_add(count)
            .filter(|n| *n <= 2_000_000)
            .ok_or(PageError::Budget("decoded outline points"))?;
        Ok(())
    }
    fn advance(&mut self, ch: char) -> std::result::Result<f32, PageError> {
        let index = self.ensure(ch)?;
        Ok(self.entries[&index].advance)
    }
    fn mask(&mut self, ch: char) -> std::result::Result<&GlyphMask, PageError> {
        let index = self.ensure(ch)?;
        if self.entries[&index].mask.is_none() {
            let glyph = self.font.glyph(index)?;
            self.charge_points(glyph.outline.points().len())?;
            let mask = rasterize(&glyph.outline, self.scale, RasterLimits::default())?;
            self.mask_bytes = self
                .mask_bytes
                .checked_add(mask.coverage().len())
                .filter(|n| *n <= 32 * 1024 * 1024)
                .ok_or(PageError::Budget("glyph mask cache"))?;
            self.raster_work = self
                .raster_work
                .checked_add(mask.work())
                .filter(|n| *n <= 128 * 1024 * 1024)
                .ok_or(PageError::Budget("aggregate raster work"))?;
            self.entries
                .get_mut(&index)
                .ok_or(PageError::Budget("missing cached glyph"))?
                .mask = Some(mask);
        }
        self.entries[&index]
            .mask
            .as_ref()
            .ok_or(PageError::Budget("missing glyph mask"))
    }
}
struct RenderedPage {
    surface: Surface,
    page: usize,
    pages: usize,
    locator: TextLocator,
    missing: Vec<char>,
    cached_masks: usize,
}
fn render(document: &TextDocument, font: &Font<'_>, options: &Options) -> Result<RenderedPage> {
    let mut cache = GlyphCache::new(font, options.size, options.allow_missing);
    let metrics = font.metrics();
    if metrics.ascender <= 0 || metrics.descender > 0 {
        return Err("unsupported horizontal font metrics for this first layout engine".into());
    }
    let ascender = f32::from(metrics.ascender) * cache.scale;
    let line_height = ((f32::from(metrics.ascender) - f32::from(metrics.descender)
        + f32::from(metrics.line_gap.max(0)))
        * cache.scale)
        .ceil()
        .max(options.size as f32);
    let content_width = options.width - options.margin * 2;
    let content_height = options.height - options.margin * 2;
    let rows = (content_height as f32 / line_height).floor() as usize;
    if rows == 0 {
        return Err("page is too short for one line with this font and size".into());
    }
    let tab_width = (cache.advance(' ')? * 4.0).max(options.size as f32);
    let layout = MeasuredLayout::build(
        document,
        LayoutConfig {
            width: content_width as f32,
            rows,
            tab_width,
            ..LayoutConfig::default()
        },
        |ch| cache.advance(ch),
    )?;
    let page = if let Some(at) = &options.at {
        layout.page_for_locator(document, at)?
    } else {
        options.page.unwrap_or(0)
    };
    let lines = layout.page(page).ok_or("page is outside this document")?;
    let locator = document.locator(lines[0].text_range.start)?;
    let mut surface = Surface::new(options.width, options.height, RenderLimits::default())?;
    surface.draw(&[DrawCommand::FillRect {
        rect: Rect::new(0, 0, options.width, options.height),
        color: Color::WHITE,
    }])?;
    let clip = Rect::new(
        options.margin as i32,
        options.margin as i32,
        content_width,
        content_height,
    );
    let (mut count, mut blend_work) = (0_usize, 0_u64);
    for (row, line) in lines.iter().enumerate() {
        let baseline = options.margin as f32 + ascender + row as f32 * line_height;
        let mut x = 0.0_f32;
        for ch in document.text()[line.text_range.clone()].chars() {
            if ch == '\t' {
                x += tab_advance(x, tab_width);
                continue;
            }
            count += 1;
            if count > 200_000 {
                return Err(PageError::Budget("visible glyphs").into());
            }
            let advance = cache.advance(ch)?;
            let mask = cache.mask(ch)?;
            blend_work = blend_work
                .checked_add(u64::from(mask.width()) * u64::from(mask.height()))
                .filter(|n| *n <= 64 * 1024 * 1024)
                .ok_or(PageError::Budget("aggregate glyph blending"))?;
            surface.draw_glyph(
                mask,
                (
                    (options.margin as f32 + x).round() as i32,
                    baseline.round() as i32,
                ),
                Color::rgba(24, 24, 24, 255),
                clip,
            )?;
            x += advance;
        }
    }
    let mut missing: Vec<_> = cache.missing.iter().copied().collect();
    missing.sort_unstable();
    let cached_masks = cache
        .entries
        .values()
        .filter(|glyph| glyph.mask.is_some())
        .count();
    Ok(RenderedPage {
        surface,
        page,
        pages: layout.page_count(),
        locator,
        missing,
        cached_masks,
    })
}

pub(crate) fn run(args: &[OsString], output: &mut impl Write) -> Result<()> {
    if args.len() < 2 {
        return Err("render-text expects <book.txt> <new-output.ppm> --font <font.ttf>".into());
    }
    let options = Options::parse(&args[2..])?;
    let document = TextDocument::open(
        &mut LocalFileSource::open(PathBuf::from(&args[0]))?,
        Limits::default(),
    )?;
    let font_limits = FontLimits::default();
    let font_bytes = read_bounded(
        &mut LocalFileSource::open(&options.font)?,
        font_limits.max_file_bytes,
    )?;
    let font = Font::parse(&font_bytes, options.face, font_limits)?;
    let rendered = render(&document, &font, &options)?;
    // Complete parsing/rasterization first. Never truncate an input or existing output file.
    let path = PathBuf::from(&args[1]);
    let mut writer = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?,
    );
    rendered.surface.write_ppm(&mut writer)?;
    writer.flush()?;
    writeln!(
        output,
        "Rendered TXT page (unhinted, scalar wrapping; no shaping/bidi)\nPage: {}/{}\nStart locator: {}\nImage: {}x{}\nCached glyph masks: {}\nMissing characters: {}\nOutput: {:?}",
        rendered.page + 1,
        rendered.pages,
        rendered.locator,
        rendered.surface.width(),
        rendered.surface.height(),
        rendered.cached_masks,
        rendered.missing.len(),
        path
    )?;
    if !rendered.missing.is_empty() {
        write!(output, "Explicit .notdef replacements:")?;
        for ch in rendered.missing.iter().take(16) {
            write!(output, " U+{:04X}", u32::from(*ch))?;
        }
        writeln!(output)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    #[test]
    fn options_validate_geometry_and_missing_policy() {
        let options = Options::parse(&args(&[
            "--font",
            "font.ttf",
            "--width",
            "640",
            "--missing",
            "replacement",
        ]))
        .unwrap();
        assert_eq!(options.width, 640);
        assert!(options.allow_missing);
        for values in [
            vec![],
            vec!["--font", "font.ttf", "--width", "0"],
            vec!["--font", "font.ttf", "--font-size", "NaN"],
            vec!["--font", "font.ttf", "--margin", "999999"],
            vec!["--font", "font.ttf", "--page", "0"],
            vec!["--font", "font.ttf", "--face", "-1"],
            vec!["--font", "font.ttf", "--missing", "silent"],
            vec!["--font", "a.ttf", "--font", "b.ttf"],
            vec!["--font", "font.ttf", "--unknown", "1"],
        ] {
            assert!(Options::parse(&args(&values)).is_err(), "{values:?}");
        }
    }
    #[test]
    fn page_and_content_locator_are_exclusive() {
        let document = TextDocument::from_bytes(b"A", Limits::default()).unwrap();
        let at = document.locator(0).unwrap().to_string();
        assert!(
            Options::parse(&args(&["--font", "font.ttf", "--page", "1", "--at", &at])).is_err()
        );
    }
}

//! TXT page renderer shared by headless export and the native reading session.
use readall_core::{
    DocumentId, Limits, TextDocument, TextLocator,
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
#[derive(Debug, Clone)]
pub(crate) struct Options {
    pub font: PathBuf,
    pub face: u32,
    pub width: u32,
    pub height: u32,
    pub size: u32,
    pub margin: u32,
    pub page: Option<usize>,
    pub at: Option<TextLocator>,
    pub allow_missing: bool,
}
impl Options {
    pub(crate) fn parse(args: &[OsString]) -> Result<Self> {
        if !args.len().is_multiple_of(2) {
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
pub(crate) enum PageError {
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
pub(crate) struct GlyphCache<'f, 'data> {
    font: std::borrow::Cow<'f, Font<'data>>,
    scale: f32,
    allow_missing: bool,
    entries: HashMap<u16, CachedGlyph>,
    missing: HashSet<char>,
    decoded_points: usize,
    mask_bytes: usize,
    mask_limit: usize,
    pub(crate) raster_work: u64,
}
impl<'f, 'data> GlyphCache<'f, 'data> {
    pub(crate) fn new(font: &'f Font<'data>, size: u32, allow_missing: bool) -> Self {
        Self::from_font(std::borrow::Cow::Borrowed(font), size, allow_missing)
    }
    pub(crate) fn owned(font: Font<'data>, size: u32, allow_missing: bool) -> Self {
        Self::from_font(std::borrow::Cow::Owned(font), size, allow_missing)
    }
    fn from_font(font: std::borrow::Cow<'f, Font<'data>>, size: u32, allow_missing: bool) -> Self {
        let scale = size as f32 / f32::from(font.metrics().units_per_em);
        Self {
            font,
            scale,
            allow_missing,
            entries: HashMap::new(),
            missing: HashSet::new(),
            decoded_points: 0,
            mask_bytes: 0,
            mask_limit: 32 * 1024 * 1024,
            raster_work: 0,
        }
    }
    fn ensure(&mut self, ch: char) -> std::result::Result<u16, PageError> {
        let index = self.font.glyph_index(ch)?;
        if index == 0 {
            self.record_missing(ch)?;
        }
        self.ensure_index(index)?;
        Ok(index)
    }
    pub(crate) fn record_missing(&mut self, ch: char) -> std::result::Result<(), PageError> {
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
        Ok(())
    }
    fn ensure_index(&mut self, index: u16) -> std::result::Result<(), PageError> {
        if index >= self.font.glyph_count() {
            return Err(PageError::Budget("shaped glyph outside font"));
        }
        if !self.entries.contains_key(&index) {
            if self.entries.len() >= 8192 {
                self.entries.clear();
                self.mask_bytes = 0;
            }
            let metrics = self.font.horizontal_metrics(index)?;
            self.entries
                .try_reserve(1)
                .map_err(|_| PageError::Budget("glyph cache allocation"))?;
            self.entries.insert(
                index,
                CachedGlyph {
                    advance: f32::from(metrics.advance_width) * self.scale,
                    mask: None,
                },
            );
        }
        Ok(())
    }
    fn charge_points(&mut self, count: usize) -> std::result::Result<(), PageError> {
        self.decoded_points = self
            .decoded_points
            .checked_add(count)
            .filter(|n| *n <= 2_000_000)
            .ok_or(PageError::Budget("decoded outline points"))?;
        Ok(())
    }
    pub(crate) fn advance(&mut self, ch: char) -> std::result::Result<f32, PageError> {
        let index = self.ensure(ch)?;
        Ok(self.entries[&index].advance)
    }
    pub(crate) fn begin_page(&mut self) {
        self.raster_work = 0;
        self.decoded_points = 0;
    }
    pub(crate) fn set_mask_limit(&mut self, bytes: usize) {
        self.mask_limit = bytes;
    }
    pub(crate) fn cached_masks(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.mask.is_some())
            .count()
    }
    pub(crate) fn missing(&self) -> &HashSet<char> {
        &self.missing
    }
    pub(crate) fn mask(&mut self, ch: char) -> std::result::Result<&GlyphMask, PageError> {
        let index = self.ensure(ch)?;
        self.mask_index(index)
    }
    pub(crate) fn mask_index(&mut self, index: u16) -> std::result::Result<&GlyphMask, PageError> {
        self.ensure_index(index)?;
        if self.entries[&index].mask.is_none() {
            let glyph = self.font.glyph(index)?;
            self.charge_points(glyph.outline.points().len())?;
            let mask = rasterize(&glyph.outline, self.scale, RasterLimits::default())?;
            if mask.coverage().len() > self.mask_limit {
                return Err(PageError::Budget("glyph mask cache"));
            }
            let work = self
                .raster_work
                .checked_add(mask.work())
                .filter(|n| *n <= 128 * 1024 * 1024)
                .ok_or(PageError::Budget("aggregate raster work"))?;
            if self.mask_bytes.saturating_add(mask.coverage().len()) > self.mask_limit {
                for entry in self.entries.values_mut() {
                    entry.mask = None;
                }
                self.mask_bytes = 0;
            }
            self.mask_bytes += mask.coverage().len();
            self.raster_work = work;
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
#[derive(Debug, Clone)]
pub(crate) struct TextHit {
    pub rect: Rect,
    pub start: usize,
    pub end: usize,
}
#[derive(Clone)]
pub(crate) struct RenderedPage {
    pub surface: Surface,
    pub page: usize,
    pub pages: usize,
    pub locator: TextLocator,
    pub image_index: Option<usize>,
    pub hits: Vec<TextHit>,
    pub image_hits: Vec<(Rect, usize)>,
    pub missing: Vec<char>,
    pub cached_masks: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LayoutKey {
    document_id: DocumentId,
    width: u32,
    height: u32,
    margin: u32,
}

struct CachedLayout {
    key: LayoutKey,
    layout: MeasuredLayout,
    ascender: f32,
    line_height: f32,
    tab_width: f32,
    content_width: u32,
    content_height: u32,
}

pub(crate) struct PageRenderer<'font, 'data> {
    cache: GlyphCache<'font, 'data>,
    size: u32,
    allow_missing: bool,
    layout: Option<CachedLayout>,
    layout_builds: usize,
}

impl<'font, 'data> PageRenderer<'font, 'data> {
    pub(crate) fn new(font: &'font Font<'data>, size: u32, allow_missing: bool) -> Result<Self> {
        if !(8..=256).contains(&size) {
            return Err("font size must be 8..256".into());
        }
        let metrics = font.metrics();
        if metrics.ascender <= 0 || metrics.descender > 0 {
            return Err("unsupported horizontal font metrics for this first layout engine".into());
        }
        Ok(Self {
            cache: GlyphCache::new(font, size, allow_missing),
            size,
            allow_missing,
            layout: None,
            layout_builds: 0,
        })
    }

    fn ensure_layout(&mut self, document: &TextDocument, options: &Options) -> Result<()> {
        validate_options(options)?;
        if options.size != self.size || options.allow_missing != self.allow_missing {
            return Err("page renderer font configuration does not match options".into());
        }
        let key = LayoutKey {
            document_id: document.id(),
            width: options.width,
            height: options.height,
            margin: options.margin,
        };
        if self.layout.as_ref().is_some_and(|layout| layout.key == key) {
            return Ok(());
        }

        let metrics = self.cache.font.metrics();
        let ascender = f32::from(metrics.ascender) * self.cache.scale;
        let line_height = ((f32::from(metrics.ascender) - f32::from(metrics.descender)
            + f32::from(metrics.line_gap.max(0)))
            * self.cache.scale)
            .ceil()
            .max(options.size as f32);
        let content_width = options.width - options.margin * 2;
        let content_height = options.height - options.margin * 2;
        let rows = (content_height as f32 / line_height).floor() as usize;
        if rows == 0 {
            return Err("page is too short for one line with this font and size".into());
        }
        let tab_width = (self.cache.advance(' ')? * 4.0).max(options.size as f32);
        let layout = MeasuredLayout::build(
            document,
            LayoutConfig {
                width: content_width as f32,
                rows,
                tab_width,
                ..LayoutConfig::default()
            },
            |ch| self.cache.advance(ch),
        )?;

        self.layout = Some(CachedLayout {
            key,
            layout,
            ascender,
            line_height,
            tab_width,
            content_width,
            content_height,
        });
        self.layout_builds = self.layout_builds.saturating_add(1);
        Ok(())
    }

    pub(crate) fn render(
        &mut self,
        document: &TextDocument,
        options: &Options,
    ) -> Result<RenderedPage> {
        self.ensure_layout(document, options)?;
        // Raster-work is a per-page safety budget. Cached masks persist across pages,
        // but work spent producing earlier pages must not eventually poison a normal
        // long-running reading session.
        self.cache.begin_page();
        let cached = self
            .layout
            .as_ref()
            .ok_or("measured layout cache was not initialized")?;
        let page = if let Some(at) = &options.at {
            cached.layout.page_for_locator(document, at)?
        } else {
            options.page.unwrap_or(0)
        };
        let lines = cached
            .layout
            .page(page)
            .ok_or("page is outside this document")?;
        let locator = document.locator(lines[0].text_range.start)?;

        let mut surface = Surface::new(options.width, options.height, RenderLimits::default())?;
        surface.draw(&[DrawCommand::FillRect {
            rect: Rect::new(0, 0, options.width, options.height),
            color: Color::WHITE,
        }])?;
        let clip = Rect::new(
            options.margin as i32,
            options.margin as i32,
            cached.content_width,
            cached.content_height,
        );
        let (mut count, mut blend_work) = (0_usize, 0_u64);
        for (row, line) in lines.iter().enumerate() {
            let baseline =
                options.margin as f32 + cached.ascender + row as f32 * cached.line_height;
            let mut x = 0.0_f32;
            for ch in document.text()[line.text_range.clone()].chars() {
                if ch == '\t' {
                    x += tab_advance(x, cached.tab_width);
                    continue;
                }
                count += 1;
                if count > 200_000 {
                    return Err(PageError::Budget("visible glyphs").into());
                }
                let advance = self.cache.advance(ch)?;
                let mask = self.cache.mask(ch)?;
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

        let mut missing: Vec<_> = self.cache.missing.iter().copied().collect();
        missing.sort_unstable();
        let cached_masks = self
            .cache
            .entries
            .values()
            .filter(|glyph| glyph.mask.is_some())
            .count();
        Ok(RenderedPage {
            surface,
            page,
            pages: cached.layout.page_count(),
            locator,
            image_index: None,
            hits: Vec::new(),
            image_hits: Vec::new(),
            missing,
            cached_masks,
        })
    }

    #[cfg(test)]
    pub(crate) fn stats(&self) -> (usize, usize) {
        (
            self.layout_builds,
            self.cache
                .entries
                .values()
                .filter(|glyph| glyph.mask.is_some())
                .count(),
        )
    }
}

fn validate_options(options: &Options) -> Result<()> {
    if !(128..=4096).contains(&options.width)
        || !(128..=4096).contains(&options.height)
        || !(8..=256).contains(&options.size)
        || options.margin >= options.width / 2
        || options.margin >= options.height / 2
        || (options.page.is_some() && options.at.is_some())
    {
        return Err("invalid page geometry or conflicting page and locator".into());
    }
    Ok(())
}

pub(crate) fn render(
    document: &TextDocument,
    font: &Font<'_>,
    options: &Options,
) -> Result<RenderedPage> {
    let mut renderer = PageRenderer::new(font, options.size, options.allow_missing)?;
    renderer.render(document, options)
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
    fn measuring_glyphs_does_not_decode_outlines_and_page_work_resets() {
        let bytes = crate::test_font::make_font();
        let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
        let mut renderer = PageRenderer::new(&font, 24, false).unwrap();
        renderer.cache.advance('A').unwrap();
        assert_eq!(renderer.cache.decoded_points, 0);
        renderer.cache.raster_work = 128 * 1024 * 1024;
        renderer.cache.decoded_points = 2_000_000;
        let options = Options::parse(&args(&[
            "--font",
            "fixture.ttf",
            "--width",
            "200",
            "--height",
            "128",
            "--margin",
            "16",
        ]))
        .unwrap();
        let document = TextDocument::from_bytes(b"AW", Limits::default()).unwrap();
        assert!(renderer.render(&document, &options).is_ok());
        assert!(renderer.cache.raster_work < 128 * 1024 * 1024);
    }

    #[test]
    fn bounded_glyph_masks_evict_without_poisoning_later_pages() {
        let bytes = crate::test_font::make_font();
        let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
        let mut cache = GlyphCache::new(&font, 24, false);
        let a = cache.mask('A').unwrap().coverage().len();
        let w = cache.mask('W').unwrap().coverage().len();
        let mut cache = GlyphCache::new(&font, 24, false);
        cache.set_mask_limit(a.max(w));
        for _ in 0..8 {
            cache.begin_page();
            cache.mask('A').unwrap();
            cache.mask('W').unwrap();
            assert!(cache.mask_bytes <= a.max(w));
        }
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

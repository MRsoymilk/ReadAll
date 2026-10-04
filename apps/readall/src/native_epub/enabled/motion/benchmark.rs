//! Run explicitly in release mode; measures CPU frame preparation, not desktop FPS.
use super::*;
use crate::{reader_data::Settings, test_epub};
#[test]
#[ignore = "manual release benchmark with the bundled Chinese font"]
fn cached_reader_motion_benchmark() {
    let text = "这是 ReadAll 平滑滚动测试。正文保留中文排版、代码高亮、选择和链接。This paragraph measures cached page composition, not glyph decoding.";
    let source = format!(
        "<html><body>{}</body></html>",
        format!("<p>{text}</p>").repeat(32)
    );
    let bytes = test_epub::make_epub_with_resources(&[&source], vec![]);
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let font_bytes = builtin_font_bytes();
    let font = Font::parse(font_bytes, 0, FontLimits::default()).unwrap();
    let options = Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "1280",
            "--height",
            "720",
            "--margin",
            "40",
            "--font-size",
            "24",
        ]
        .map(Into::into),
    )
    .unwrap();
    let settings = Settings {
        page_mode: PageMode::Scroll,
        ..Settings::default()
    };
    let session =
        EpubSession::new_with_preferences(&book, &font, options, Start::Beginning, &[], settings)
            .unwrap();
    let ui = UiFont::from_bytes(font_bytes.to_vec(), PathBuf::from("<built-in>")).unwrap();
    let mut reader = ReaderWindow::new_lazy(session, None, ui).unwrap();
    reader.toolbar = ToolbarMode::Collapsed;
    reader.prefetch_page().unwrap();
    reader.prefetch_page().unwrap();
    let stride = reader.session.scroll_stride();
    fn report(name: &str, mut values: Vec<f64>) {
        let average = values.iter().sum::<f64>() / values.len() as f64;
        values.sort_by(f64::total_cmp);
        println!(
            "MOTION_BENCH {name} frames={} avg_ms={average:.3} p95_ms={:.3} max_ms={:.3}",
            values.len(),
            values[values.len() * 95 / 100],
            values.last().unwrap()
        );
    }
    let mut samples = Vec::new();
    for n in 0..140 {
        let offset = (n % 100) as f64 / 100.0 * stride;
        let at = Instant::now();
        reader.session.compose_scroll(offset).unwrap();
        reader.refresh_surface().unwrap();
        // Include latest-frame snapshot and opaque XRGB conversion, not file I/O.
        let snapshot = std::hint::black_box(reader.surface.clone());
        let mut xrgb = Vec::with_capacity(snapshot.pixels().len());
        for c in snapshot.pixels() {
            xrgb.push((u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b));
        }
        std::hint::black_box(xrgb);
        if n >= 20 {
            samples.push(at.elapsed().as_secs_f64() * 1000.0);
        }
    }
    report("scroll-1280x720", samples);
    let from = reader.session.raw_surface().clone();
    let to = reader.session.neighbour_surface(1).unwrap().clone();
    for effect in [PageEffect::Slide, PageEffect::Book] {
        let mut out = to.clone();
        let mut samples = Vec::new();
        for n in 0..140 {
            let p = (n % 100) as f32 / 100.0;
            let at = Instant::now();
            out.page_transition(
                (&from, &to),
                Rect::new(0, 32, 1280, 684),
                p,
                false,
                effect,
                Color::WHITE,
            )
            .unwrap();
            std::hint::black_box(&out);
            if n >= 20 {
                samples.push(at.elapsed().as_secs_f64() * 1000.0);
            }
        }
        report(
            if effect == PageEffect::Slide {
                "slide-compositor-1280x720"
            } else {
                "book-compositor-1280x720"
            },
            samples,
        );
    }
}

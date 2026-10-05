use super::*;
use crate::{test_epub, test_font};
use readall_font::FontLimits;
fn options(margin: u32, dense: bool) -> Options {
    let mut options = Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "400",
            "--height",
            "520",
            "--font-size",
            "20",
            "--margin",
            &margin.to_string(),
        ]
        .map(Into::into),
    )
    .unwrap();
    if dense {
        options.raster_size = Some((1100, 1430));
    }
    options
}
#[test]
fn small_margins_never_put_text_in_the_strip_chrome_crop() {
    let body = format!(
        "<html><body>{}</body></html>",
        "<p>AAAA WWWW AAAA WWWW</p>".repeat(120)
    );
    let bytes = test_epub::make_epub_with_resources(&[&body], vec![]);
    let book = EpubBook::parse(&bytes, Default::default()).unwrap();
    let data = test_font::make_font();
    let font = Font::parse(&data, 0, FontLimits::default()).unwrap();
    for margin in [0, 8, 16, 31, 40] {
        for dense in [false, true] {
            let mut reader =
                EpubSession::new(&book, &font, options(margin, dense), Start::Beginning).unwrap();
            assert!(reader.frame.pages > 1);
            assert!(
                reader
                    .frame
                    .hits
                    .iter()
                    .all(|hit| hit.rect.y >= 32 && hit.rect.y + hit.rect.height as i32 <= 516)
            );
            reader.prepare_neighbour(1).unwrap();
            let next = &reader.paging.neighbours[1].as_ref().unwrap().frame;
            assert!(
                next.hits
                    .iter()
                    .all(|hit| hit.rect.y >= 32 && hit.rect.y + hit.rect.height as i32 <= 516)
            );
            let source = next.surface.clone();
            let band = source.pixel_rect(Rect::new(0, 32, 400, 484));
            let stride = reader.scroll_stride();
            let offset = stride / 2.0;
            reader.compose_scroll(offset).unwrap();
            let view = &reader.frame().surface;
            let shift =
                band.height as i32 - (offset * f64::from(source.pixel_scale().1)).round() as i32;
            let row_bytes = source.pixel_width() as usize;
            let rows = (band.height as i32 - shift).max(0) as usize;
            assert!(rows > 0);
            // The entire visible top of the next page, not only its bottom text pixels,
            // must appear at the page seam. Density rounding cannot skip a scanline.
            for row in 0..rows {
                let from = (band.y as usize + row) * row_bytes;
                let to = (band.y as usize + shift as usize + row) * row_bytes;
                assert_eq!(
                    &source.pixels()[from..from + row_bytes],
                    &view.pixels()[to..to + row_bytes],
                    "margin={margin}, dense={dense}, row={row}"
                );
            }
        }
    }
}
#[test]
fn small_margins_preserve_the_top_of_images_and_last_page_bottom() {
    let bytes = test_epub::make_epub_with_resources(
        &["<html><body><img src='sample.png'/><p>AAAA WWWW</p></body></html>"],
        vec![(
            "sample.png",
            "image/png",
            test_epub::make_png(50, 50, [10, 90, 180, 255]),
        )],
    );
    let book = EpubBook::parse(&bytes, Default::default()).unwrap();
    let data = test_font::make_font();
    let font = Font::parse(&data, 0, FontLimits::default()).unwrap();
    for margin in [0, 8, 16] {
        let mut reader =
            EpubSession::new(&book, &font, options(margin, true), Start::Beginning).unwrap();
        let (rect, _) = reader.frame.image_hits[0];
        assert!(rect.y >= 32);
        let before = reader.frame.surface.clone();
        reader.compose_scroll(0.0).unwrap();
        let band = before.pixel_rect(Rect::new(0, 32, 400, 484));
        let width = before.pixel_width() as usize;
        for row in band.y as usize..(band.y as usize + band.height as usize) {
            assert_eq!(
                &before.pixels()[row * width..(row + 1) * width],
                &reader.frame().surface.pixels()[row * width..(row + 1) * width]
            );
        }
    }
}

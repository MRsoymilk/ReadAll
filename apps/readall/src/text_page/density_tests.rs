use super::*;
use crate::test_font;
#[test]
fn density_rasterizes_the_outline_again_without_changing_advances() {
    let data = test_font::make_font();
    let font = Font::parse(&data, 0, FontLimits::default()).unwrap();
    let mut cache = GlyphCache::new(&font, 20, false);
    let advance = cache.advance('A').unwrap();
    let low = cache.mask('A').unwrap().coverage().to_vec();
    cache.set_raster_scale((2.75, 2.75));
    assert_eq!(cache.cached_masks(), 0);
    let glyph = font.glyph(font.glyph_index('A').unwrap()).unwrap();
    let expected = readall_render::glyph::rasterize(
        &glyph.outline,
        20.0 * 2.75 / f32::from(font.metrics().units_per_em),
        RasterLimits::default(),
    )
    .unwrap();
    let high = cache.mask('A').unwrap();
    assert_eq!(high.coverage(), expected.coverage());
    assert!(high.coverage().len() > low.len() * 6);
    assert_eq!(cache.advance('A').unwrap(), advance);
    cache.set_raster_scale((1.0, 1.0));
    assert_eq!(cache.mask('A').unwrap().coverage(), low);
}

use super::*;

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
}
fn simple_triangle() -> Vec<u8> {
    let mut bytes = vec![0; 10];
    put16(&mut bytes, 0, 1);
    put16(&mut bytes, 6, 1000);
    put16(&mut bytes, 8, 1000);
    bytes.extend_from_slice(&2_u16.to_be_bytes());
    bytes.extend_from_slice(&0_u16.to_be_bytes());
    bytes.extend_from_slice(&[1, 1, 1]);
    for delta in [0_i16, 500, 500, 0, 1000, -1000] {
        bytes.extend_from_slice(&delta.to_be_bytes());
    }
    bytes
}
fn compound(child: u16, flags: u16, x: i16, y: i16) -> Vec<u8> {
    let mut bytes = vec![0; 10];
    put16(&mut bytes, 0, 0xffff);
    for word in [flags, child, x as u16, y as u16] {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    bytes
}
fn cmap12() -> Vec<u8> {
    let mappings = [
        (32_u32, 3_u32),
        (65, 1),
        (233, 2),
        (0x4e2d, 1),
        (0x1f600, 1),
    ];
    let mut bytes = vec![0; 12 + 16 + mappings.len() * 12];
    put16(&mut bytes, 2, 1);
    put16(&mut bytes, 4, 3);
    put16(&mut bytes, 6, 10);
    put32(&mut bytes, 8, 12);
    put16(&mut bytes, 12, 12);
    put32(&mut bytes, 16, (16 + mappings.len() * 12) as u32);
    put32(&mut bytes, 24, mappings.len() as u32);
    for (i, (code, glyph)) in mappings.into_iter().enumerate() {
        let at = 28 + i * 12;
        put32(&mut bytes, at, code);
        put32(&mut bytes, at + 4, code);
        put32(&mut bytes, at + 8, glyph);
    }
    bytes
}
fn cmap4() -> Vec<u8> {
    // A uses idDelta, Z uses a relative glyphIdArray plus delta, sentinel maps to .notdef.
    let mut bytes = vec![0; 12 + 42];
    put16(&mut bytes, 2, 1);
    put16(&mut bytes, 4, 3);
    put16(&mut bytes, 6, 1);
    put32(&mut bytes, 8, 12);
    let s = &mut bytes[12..];
    put16(s, 0, 4);
    put16(s, 2, 42);
    put16(s, 6, 6);
    for (i, (code, delta, range)) in [(65, 1_u16.wrapping_sub(65), 0), (90, 1, 4), (0xffff, 1, 0)]
        .into_iter()
        .enumerate()
    {
        put16(s, 14 + i * 2, code);
        put16(s, 22 + i * 2, code);
        put16(s, 28 + i * 2, delta);
        put16(s, 34 + i * 2, range);
    }
    put16(s, 40, 1);
    bytes
}
fn font_with(mut glyphs: Vec<Vec<u8>>, cmap: Vec<u8>, short_loca: bool) -> Vec<u8> {
    let count = glyphs.len();
    let mut head = vec![0; 54];
    put32(&mut head, 0, 0x10000);
    put32(&mut head, 12, 0x5f0f3cf5);
    put16(&mut head, 18, 1000);
    put16(&mut head, 50, u16::from(!short_loca));
    let mut hhea = vec![0; 36];
    put32(&mut hhea, 0, 0x10000);
    put16(&mut hhea, 4, 1000);
    put16(&mut hhea, 6, (-200_i16) as u16);
    put16(&mut hhea, 34, count as u16);
    let mut maxp = vec![0; 32];
    put32(&mut maxp, 0, 0x10000);
    put16(&mut maxp, 4, count as u16);
    let mut hmtx = vec![0; count * 4];
    for i in 0..count {
        put16(&mut hmtx, i * 4, if i == 3 { 250 } else { 1100 });
    }
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for glyph in &mut glyphs {
        if short_loca {
            loca.extend_from_slice(&((glyf.len() / 2) as u16).to_be_bytes());
        } else {
            loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
        }
        glyf.append(glyph);
        if glyf.len() % 2 != 0 {
            glyf.push(0);
        }
    }
    if short_loca {
        loca.extend_from_slice(&((glyf.len() / 2) as u16).to_be_bytes());
    } else {
        loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
    }
    let tables = [
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
    ];
    let mut bytes = vec![0; 12 + tables.len() * 16];
    put32(&mut bytes, 0, 0x10000);
    put16(&mut bytes, 4, tables.len() as u16);
    for (i, (tag, data)) in tables.into_iter().enumerate() {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        let at = 12 + i * 16;
        bytes[at..at + 4].copy_from_slice(&tag);
        let offset = bytes.len();
        put32(&mut bytes, at + 8, offset as u32);
        put32(&mut bytes, at + 12, data.len() as u32);
        bytes.extend(data);
    }
    bytes
}
pub(super) fn fixture() -> Vec<u8> {
    font_with(
        vec![
            simple_triangle(),
            simple_triangle(),
            compound(1, 3, 50, 100),
            vec![],
        ],
        cmap12(),
        false,
    )
}
fn parse(bytes: &[u8]) -> Font<'_> {
    Font::parse(bytes, 0, FontLimits::default()).unwrap()
}
fn table_at(bytes: &[u8], tag: &[u8; 4]) -> usize {
    for i in 0..usize::from(u16_at(bytes, 4).unwrap()) {
        let at = 12 + i * 16;
        if &bytes[at..at + 4] == tag {
            return offset_at(bytes, at + 8).unwrap();
        }
    }
    panic!("missing test table")
}

#[test]
fn metrics_unicode_mapping_and_empty_space() {
    let bytes = fixture();
    let font = parse(&bytes);
    assert_eq!(font.glyph_count(), 4);
    assert_eq!(font.metrics().units_per_em, 1000);
    for (ch, expected) in [('A', 1), ('é', 2), ('中', 1), ('😀', 1), ('?', 0), (' ', 3)] {
        assert_eq!(font.glyph_index(ch).unwrap(), expected);
    }
    let glyph = font.glyph(3).unwrap();
    assert_eq!(glyph.outline.points().len(), 0);
    assert_eq!(glyph.metrics.advance_width, 250);
    assert!(font.glyph(4).is_err());
    assert!(Font::parse(&bytes, 1, FontLimits::default()).is_err());
}
#[test]
fn simple_and_translated_compound_outlines() {
    let bytes = fixture();
    let font = parse(&bytes);
    let glyph = font.glyph(1).unwrap();
    assert_eq!(glyph.outline.contour_count(), 1);
    assert_eq!(
        glyph.outline.points(),
        &[
            Point {
                x: 0.0,
                y: 0.0,
                on_curve: true
            },
            Point {
                x: 500.0,
                y: 1000.0,
                on_curve: true
            },
            Point {
                x: 1000.0,
                y: 0.0,
                on_curve: true
            }
        ]
    );
    assert_eq!(
        font.glyph(2).unwrap().outline.points()[1],
        Point {
            x: 550.0,
            y: 1100.0,
            on_curve: true
        }
    );
}
#[test]
fn format4_delta_array_and_missing_glyph() {
    let bytes = font_with(
        vec![
            simple_triangle(),
            simple_triangle(),
            simple_triangle(),
            vec![],
        ],
        cmap4(),
        true,
    );
    let font = parse(&bytes);
    assert_eq!(font.glyph_index('A').unwrap(), 1);
    assert_eq!(font.glyph_index('Z').unwrap(), 2);
    assert_eq!(font.glyph_index('B').unwrap(), 0);
    assert_eq!(font.glyph_index('😀').unwrap(), 0);
    assert_eq!(font.glyph(1).unwrap().outline.points().len(), 3);
    let mut zero = bytes.clone();
    let at = table_at(&zero, b"cmap");
    put16(&mut zero, at + 12 + 40, 0);
    assert_eq!(parse(&zero).glyph_index('Z').unwrap(), 0); // Delta must NOT be applied to missing glyph zero.
}
#[test]
fn horizontal_metrics_tail_reuses_last_advance() {
    let mut bytes = fixture();
    let hhea = table_at(&bytes, b"hhea");
    put16(&mut bytes, hhea + 34, 2);
    let hmtx = table_at(&bytes, b"hmtx");
    put16(&mut bytes, hmtx + 8, (-20_i16) as u16);
    assert_eq!(
        parse(&bytes).horizontal_metrics(2).unwrap(),
        HorizontalMetrics {
            advance_width: 1100,
            left_side_bearing: -20
        }
    );
}
#[test]
fn ttc_uses_absolute_offsets_and_validates_face_index() {
    let mut sfnt = fixture();
    let count = usize::from(u16_at(&sfnt, 4).unwrap());
    for i in 0..count {
        let at = 12 + i * 16 + 8;
        let offset = u32_at(&sfnt, at).unwrap();
        put32(&mut sfnt, at, offset + 16);
    }
    let mut bytes = b"ttcf".to_vec();
    bytes.extend_from_slice(&0x10000_u32.to_be_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&16_u32.to_be_bytes());
    bytes.extend(sfnt);
    assert_eq!(parse(&bytes).glyph_index('中').unwrap(), 1);
    assert!(Font::parse(&bytes, 1, FontLimits::default()).is_err());
}
#[test]
fn compound_scale_and_scaled_offset() {
    for (extra, expected_x) in [(0, 350.0), (0x800, 300.0)] {
        let mut glyph = compound(1, 3 | 8 | extra, 100, 0);
        glyph.extend_from_slice(&8192_i16.to_be_bytes());
        let bytes = font_with(
            vec![vec![], simple_triangle(), glyph, vec![]],
            cmap12(),
            false,
        );
        let outline = parse(&bytes).glyph(2).unwrap().outline;
        assert_eq!(outline.points()[1].x, expected_x);
        assert_eq!(outline.points()[1].y, 500.0);
    }
}
#[test]
fn compound_point_attachment_and_metrics() {
    let mut parent = compound(1, 3 | 0x20 | 0x200, 0, 0);
    for value in [1_u16, 1, 1, 0] {
        parent.extend_from_slice(&value.to_be_bytes());
    }
    let mut bytes = font_with(
        vec![vec![], simple_triangle(), parent, vec![]],
        cmap12(),
        false,
    );
    let hmtx = table_at(&bytes, b"hmtx");
    put16(&mut bytes, hmtx + 8, 999);
    let glyph = parse(&bytes).glyph(2).unwrap();
    assert_eq!(glyph.outline.contour_count(), 2);
    assert_eq!(glyph.outline.points()[3].x, 500.0);
    assert_eq!(glyph.outline.points()[3].y, 1000.0);
    assert_eq!(glyph.metrics.advance_width, 1100);
}
#[test]
fn packed_flags_and_short_coordinate_deltas() {
    let mut glyph = vec![0; 10];
    put16(&mut glyph, 0, 1);
    glyph.extend_from_slice(&2_u16.to_be_bytes());
    glyph.extend_from_slice(&0_u16.to_be_bytes());
    glyph.extend_from_slice(&[0x3b, 2, 5, 6, 7]); // repeated on-curve flags; positive short x, unchanged y
    let bytes = font_with(vec![glyph, vec![], vec![], vec![]], cmap12(), false);
    let result = parse(&bytes).glyph(0).unwrap();
    assert_eq!(result.outline.points()[2].x, 18.0);
    assert_eq!(result.outline.points()[2].y, 0.0);
}
#[test]
fn cyclic_compounds_and_resource_limits_fail() {
    let bytes = font_with(
        vec![vec![], compound(2, 3, 0, 0), compound(1, 3, 0, 0), vec![]],
        cmap12(),
        false,
    );
    assert!(matches!(
        parse(&bytes).glyph(1),
        Err(FontError::Invalid("composite glyph cycle"))
    ));
    let bytes = fixture();
    for limits in [
        FontLimits {
            max_points: 2,
            ..FontLimits::default()
        },
        FontLimits {
            max_contours: 0,
            ..FontLimits::default()
        },
        FontLimits {
            max_depth: 1,
            ..FontLimits::default()
        },
        FontLimits {
            max_components: 1,
            ..FontLimits::default()
        },
    ] {
        assert!(matches!(
            Font::parse(&bytes, 0, limits).unwrap().glyph(2),
            Err(FontError::LimitExceeded(_))
        ));
    }
    assert!(
        Font::parse(
            &bytes,
            0,
            FontLimits {
                max_file_bytes: 1,
                ..FontLimits::default()
            }
        )
        .is_err()
    );
}
#[test]
fn malformed_tables_and_unsupported_outlines_fail() {
    let original = fixture();
    for tag in [*b"OTTO", *b"wOF2"] {
        let mut bytes = original.clone();
        bytes[..4].copy_from_slice(&tag);
        assert!(matches!(
            Font::parse(&bytes, 0, FontLimits::default()),
            Err(FontError::Unsupported(_))
        ));
    }
    let mut bytes = original.clone();
    put32(&mut bytes, 20, u32::MAX);
    assert!(Font::parse(&bytes, 0, FontLimits::default()).is_err());
    let mut bytes = original.clone();
    let at = table_at(&bytes, b"loca");
    put32(&mut bytes, at + 4, u32::MAX);
    assert!(Font::parse(&bytes, 0, FontLimits::default()).is_err());
    let mut bytes = original.clone();
    let at = table_at(&bytes, b"cmap");
    put32(&mut bytes, at + 28 + 8, u32::MAX);
    assert!(Font::parse(&bytes, 0, FontLimits::default()).is_err());
    let mut bytes = original;
    let at = table_at(&bytes, b"glyf");
    bytes[at + 14] = 0x09;
    bytes[at + 15] = 255;
    assert!(parse(&bytes).glyph(0).is_err());
}
#[test]
fn every_truncation_and_deterministic_mutations_are_panic_free() {
    let original = fixture();
    for length in 0..original.len() {
        assert!(Font::parse(&original[..length], 0, FontLimits::default()).is_err());
    }
    for i in 0..original.len() {
        let mut bytes = original.clone();
        bytes[i] ^= 0xff;
        if let Ok(font) = Font::parse(&bytes, 0, FontLimits::default()) {
            for glyph in 0..font.glyph_count().min(8) {
                let _ = font.glyph(glyph);
            }
            for ch in ['A', 'é', '中', '😀', '\u{ffff}'] {
                let _ = font.glyph_index(ch);
            }
        }
    }
}

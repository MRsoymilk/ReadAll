//! Original miniature font assembled in Rust. No installed font is embedded or redistributed.
fn u16_at(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
}
fn u32_at(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
}
fn outline(points: &[(i16, i16)]) -> Vec<u8> {
    let mut bytes = vec![0; 10];
    u16_at(&mut bytes, 0, 1);
    u16_at(&mut bytes, 6, 900);
    u16_at(&mut bytes, 8, 700);
    bytes.extend_from_slice(&((points.len() - 1) as u16).to_be_bytes());
    bytes.extend_from_slice(&0_u16.to_be_bytes());
    bytes.resize(bytes.len() + points.len(), 1);
    for axis in 0..2 {
        let mut previous = 0_i16;
        for &(x, y) in points {
            let value = if axis == 0 { x } else { y };
            bytes.extend_from_slice(&(value - previous).to_be_bytes());
            previous = value;
        }
    }
    bytes
}
#[allow(dead_code)]
pub fn make_layout_font(
    script: [u8; 4],
    feature: [u8; 4],
    ligature: bool,
    mappings: &[(u32, u16)],
) -> Vec<u8> {
    fn words(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }
    let mut gsub = words(&[1, 0, 10, 30, 44]);
    gsub.extend(words(&[1]));
    gsub.extend(script);
    gsub.extend(words(&[8, 4, 0, 0, 65535, 1, 0]));
    gsub.extend(words(&[1]));
    gsub.extend(feature);
    gsub.extend(words(&[8, 0, 1, 0]));
    gsub.extend(words(&[1, 4, if ligature { 4 } else { 1 }, 0, 1, 8]));
    if ligature {
        gsub.extend(words(&[1, 18, 1, 8, 1, 4, 2, 2, 1, 1, 1, 1]));
    } else {
        gsub.extend(words(&[2, 8, 1, 2, 1, 1, 1]));
    }
    let original = make_font();
    let count = u16::from_be_bytes(original[4..6].try_into().unwrap()) as usize;
    let mut tables = Vec::new();
    for i in 0..count {
        let at = 12 + i * 16;
        let tag: [u8; 4] = original[at..at + 4].try_into().unwrap();
        let offset = u32::from_be_bytes(original[at + 8..at + 12].try_into().unwrap()) as usize;
        let length = u32::from_be_bytes(original[at + 12..at + 16].try_into().unwrap()) as usize;
        tables.push((tag, original[offset..offset + length].to_vec()));
    }
    let mut map = std::collections::BTreeMap::from([(32, 3), (65, 1), (87, 2)]);
    for &(cp, glyph) in mappings {
        map.insert(cp, glyph);
    }
    let mut cmap = vec![0; 28 + map.len() * 12];
    u16_at(&mut cmap, 2, 1);
    u16_at(&mut cmap, 4, 3);
    u16_at(&mut cmap, 6, 10);
    u32_at(&mut cmap, 8, 12);
    u16_at(&mut cmap, 12, 12);
    u32_at(&mut cmap, 16, (16 + map.len() * 12) as u32);
    u32_at(&mut cmap, 24, map.len() as u32);
    for (i, (cp, glyph)) in map.into_iter().enumerate() {
        let at = 28 + i * 12;
        u32_at(&mut cmap, at, cp);
        u32_at(&mut cmap, at + 4, cp);
        u32_at(&mut cmap, at + 8, u32::from(glyph));
    }
    tables.iter_mut().find(|(tag, _)| tag == b"cmap").unwrap().1 = cmap;
    tables.push((*b"GSUB", gsub));
    tables.sort_by_key(|(tag, _)| *tag);
    let mut bytes = vec![0; 12 + tables.len() * 16];
    u32_at(&mut bytes, 0, 0x10000);
    u16_at(&mut bytes, 4, tables.len() as u16);
    for (i, (tag, data)) in tables.into_iter().enumerate() {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        let at = 12 + i * 16;
        let start = bytes.len();
        bytes[at..at + 4].copy_from_slice(&tag);
        u32_at(&mut bytes, at + 8, start as u32);
        u32_at(&mut bytes, at + 12, data.len() as u32);
        bytes.extend(data);
    }
    bytes
}
pub fn make_font() -> Vec<u8> {
    let triangle = outline(&[(0, 0), (300, 700), (600, 0)]);
    let wide = outline(&[(0, 0), (800, 0), (800, 700), (0, 700)]);
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for glyph in [&triangle[..], &triangle[..], &wide[..], &[]] {
        loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
        glyf.extend_from_slice(glyph);
        if !glyf.len().is_multiple_of(2) {
            glyf.push(0);
        }
    }
    loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
    let mut head = vec![0; 54];
    u32_at(&mut head, 0, 0x10000);
    u32_at(&mut head, 12, 0x5f0f3cf5);
    u16_at(&mut head, 18, 1000);
    u16_at(&mut head, 50, 1);
    let mut hhea = vec![0; 36];
    u32_at(&mut hhea, 0, 0x10000);
    u16_at(&mut hhea, 4, 800);
    u16_at(&mut hhea, 6, (-200_i16) as u16);
    u16_at(&mut hhea, 34, 4);
    let mut maxp = vec![0; 32];
    u32_at(&mut maxp, 0, 0x10000);
    u16_at(&mut maxp, 4, 4);
    let mut hmtx = vec![0; 16];
    for (i, width) in [600, 600, 900, 250].into_iter().enumerate() {
        u16_at(&mut hmtx, i * 4, width);
    }
    let mut cmap = vec![0; 64];
    u16_at(&mut cmap, 2, 1);
    u16_at(&mut cmap, 4, 3);
    u16_at(&mut cmap, 6, 10);
    u32_at(&mut cmap, 8, 12);
    u16_at(&mut cmap, 12, 12);
    u32_at(&mut cmap, 16, 52);
    u32_at(&mut cmap, 24, 3);
    for (i, (code, glyph)) in [(32, 3), (65, 1), (87, 2)].into_iter().enumerate() {
        let at = 28 + i * 12;
        u32_at(&mut cmap, at, code);
        u32_at(&mut cmap, at + 4, code);
        u32_at(&mut cmap, at + 8, glyph);
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
    let mut bytes = vec![0; 124];
    u32_at(&mut bytes, 0, 0x10000);
    u16_at(&mut bytes, 4, 7);
    for (i, (tag, data)) in tables.into_iter().enumerate() {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        let at = 12 + i * 16;
        let start = bytes.len();
        bytes[at..at + 4].copy_from_slice(&tag);
        u32_at(&mut bytes, at + 8, start as u32);
        u32_at(&mut bytes, at + 12, data.len() as u32);
        bytes.extend(data);
    }
    bytes
}

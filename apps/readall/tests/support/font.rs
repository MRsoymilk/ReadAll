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
pub fn make_font() -> Vec<u8> {
    let triangle = outline(&[(0, 0), (300, 700), (600, 0)]);
    let wide = outline(&[(0, 0), (800, 0), (800, 700), (0, 700)]);
    let mut glyf = Vec::new();
    let mut loca = Vec::new();
    for glyph in [&triangle[..], &triangle[..], &wide[..], &[]] {
        loca.extend_from_slice(&(glyf.len() as u32).to_be_bytes());
        glyf.extend_from_slice(glyph);
        if glyf.len() % 2 != 0 {
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
        while bytes.len() % 4 != 0 {
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

//! Original, synthetic PalmDB fixtures; no third-party books or DRM keys.
#![allow(dead_code)]
#[derive(Clone)]
pub struct Options {
    pub compression: u16,
    pub encoding: u32,
    pub record_size: usize,
    pub images: Vec<Vec<u8>>,
    pub cover: Option<u32>,
    pub trailing: bool,
    pub extra: Vec<(u32, Vec<u8>)>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            compression: 2,
            encoding: 65001,
            record_size: 4096,
            images: Vec::new(),
            cover: None,
            trailing: false,
            extra: Vec::new(),
        }
    }
}
fn set32(bytes: &mut [u8], at: usize, n: u32) {
    bytes[at..at + 4].copy_from_slice(&n.to_be_bytes());
}
pub fn make_mobi(html: &str) -> Vec<u8> {
    build(html.as_bytes(), Options::default())
}
pub fn build(html: &[u8], options: Options) -> Vec<u8> {
    let mut records = vec![vec![]];
    for part in html.chunks(options.record_size) {
        let mut record = if options.compression == 2 {
            // Literal runs are valid PalmDOC; deliberately not the reader's code.
            part.chunks(8)
                .flat_map(|chunk| std::iter::once(chunk.len() as u8).chain(chunk.iter().copied()))
                .collect()
        } else {
            part.to_vec()
        };
        if options.trailing {
            record.extend_from_slice(b"\0abc\x84");
        }
        records.push(record);
    }
    let text_records = records.len() - 1;
    let huff = if options.compression == 17480 {
        let first = records.len();
        records.extend(huff_dictionary());
        Some(first)
    } else {
        None
    };
    let image_start = if options.images.is_empty() {
        u32::MAX
    } else {
        records.len() as u32
    };
    records.extend(options.images);
    let mut header = vec![0; 16 + 232];
    header[0..2].copy_from_slice(&options.compression.to_be_bytes());
    set32(&mut header, 4, html.len() as u32);
    header[8..10].copy_from_slice(&(text_records as u16).to_be_bytes());
    header[10..12].copy_from_slice(&(options.record_size as u16).to_be_bytes());
    header[16..20].copy_from_slice(b"MOBI");
    set32(&mut header, 20, 232);
    set32(&mut header, 24, 2);
    set32(&mut header, 28, options.encoding);
    set32(&mut header, 32, 1234567);
    set32(&mut header, 36, 6);
    set32(
        &mut header,
        92,
        if options.encoding == 65001 { 0x804 } else { 9 },
    );
    set32(&mut header, 108, image_start);
    set32(&mut header, 112, huff.map_or(u32::MAX, |n| n as u32));
    set32(&mut header, 116, if huff.is_some() { 2 } else { 0 });
    set32(&mut header, 128, 0x40);
    set32(&mut header, 244, u32::MAX);
    if options.trailing {
        header[242..244].copy_from_slice(&3_u16.to_be_bytes());
    }
    let mut fields = vec![
        (100, b"ReadAll Tests".to_vec()),
        (
            524,
            if options.encoding == 65001 {
                b"zh".to_vec()
            } else {
                b"en".to_vec()
            },
        ),
    ];
    if let Some(cover) = options.cover {
        fields.push((201, cover.to_be_bytes().to_vec()));
    }
    fields.extend(options.extra);
    let mut exth = b"EXTH\0\0\0\0".to_vec();
    exth.extend_from_slice(&(fields.len() as u32).to_be_bytes());
    for (kind, value) in fields {
        exth.extend_from_slice(&kind.to_be_bytes());
        exth.extend_from_slice(&((value.len() + 8) as u32).to_be_bytes());
        exth.extend_from_slice(&value);
    }
    let exth_len = exth.len() as u32;
    set32(&mut exth, 4, exth_len);
    header.extend(exth);
    let title = if options.encoding == 65001 {
        "ReadAll MOBI 中文"
    } else {
        "ReadAll MOBI Test"
    };
    let title_at = header.len() as u32;
    set32(&mut header, 84, title_at);
    set32(&mut header, 88, title.len() as u32);
    header.extend_from_slice(title.as_bytes());
    records[0] = header;
    assemble(records)
}
pub fn huff_dictionary() -> Vec<Vec<u8>> {
    let mut huff = vec![0; 24 + 1024 + 256];
    huff[..8].copy_from_slice(b"HUFF\0\0\0\x18");
    set32(&mut huff, 8, 24);
    set32(&mut huff, 12, 1048);
    for i in 0..256 {
        set32(&mut huff, 24 + i * 4, (255 << 8) | 128 | 8);
    }
    let mut cdic = b"CDIC".to_vec();
    cdic.extend_from_slice(&16_u32.to_be_bytes());
    cdic.extend_from_slice(&256_u32.to_be_bytes());
    cdic.extend_from_slice(&8_u32.to_be_bytes());
    for i in 0..256 {
        cdic.extend_from_slice(&((512 + i * 3) as u16).to_be_bytes());
    }
    for i in 0..256 {
        cdic.extend_from_slice(&0x8001_u16.to_be_bytes());
        cdic.push((255 - i) as u8);
    }
    vec![huff, cdic]
}
pub fn assemble(records: Vec<Vec<u8>>) -> Vec<u8> {
    let mut pdb = vec![0; 78];
    pdb[..12].copy_from_slice(b"ReadAll Test");
    pdb[60..68].copy_from_slice(b"BOOKMOBI");
    pdb[76..78].copy_from_slice(&(records.len() as u16).to_be_bytes());
    let mut offset = 78 + records.len() * 8 + 2;
    for (n, r) in records.iter().enumerate() {
        pdb.extend_from_slice(&(offset as u32).to_be_bytes());
        pdb.extend_from_slice(&(n as u32).to_be_bytes());
        offset += r.len();
    }
    pdb.extend_from_slice(&[0; 2]);
    for r in records {
        pdb.extend(r);
    }
    pdb
}

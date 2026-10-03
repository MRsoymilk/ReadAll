//! Original in-memory EPUB fixture builder for integration tests.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn push16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn push32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub fn make_epub() -> Vec<u8> {
    const CONTAINER: &[u8] = br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    const PACKAGE: &[u8] = br#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>ReadAll Test</dc:title></metadata><manifest><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>"#;
    let chapter_one = format!(
        "<html><body><h1>AAAA</h1><p>{}</p></body></html>",
        "AAAA WWWW ".repeat(80)
    );
    let chapter_two = format!(
        "<html><body><h1>WWWW</h1><p>{}</p></body></html>",
        "WWWW AAAA ".repeat(80)
    );
    let entries: Vec<(&str, Vec<u8>)> = vec![
        ("mimetype", b"application/epub+zip".to_vec()),
        ("META-INF/container.xml", CONTAINER.to_vec()),
        ("OEBPS/package.opf", PACKAGE.to_vec()),
        ("OEBPS/one.xhtml", chapter_one.into_bytes()),
        ("OEBPS/two.xhtml", chapter_two.into_bytes()),
    ];

    let mut bytes = Vec::new();
    let mut records = Vec::new();
    for (name, data) in &entries {
        let offset = bytes.len() as u32;
        let crc = crc32(data);
        push32(&mut bytes, 0x0403_4b50);
        push16(&mut bytes, 20);
        push16(&mut bytes, 0x0800);
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        push32(&mut bytes, crc);
        push32(&mut bytes, data.len() as u32);
        push32(&mut bytes, data.len() as u32);
        push16(&mut bytes, name.len() as u16);
        push16(&mut bytes, 0);
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(data);
        records.push((*name, data.len() as u32, offset, crc));
    }

    let central_offset = bytes.len() as u32;
    for (name, size, offset, crc) in &records {
        push32(&mut bytes, 0x0201_4b50);
        push16(&mut bytes, 20);
        push16(&mut bytes, 20);
        push16(&mut bytes, 0x0800);
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        push32(&mut bytes, *crc);
        push32(&mut bytes, *size);
        push32(&mut bytes, *size);
        push16(&mut bytes, name.len() as u16);
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        push32(&mut bytes, 0);
        push32(&mut bytes, *offset);
        bytes.extend_from_slice(name.as_bytes());
    }
    let central_size = bytes.len() as u32 - central_offset;
    push32(&mut bytes, 0x0605_4b50);
    push16(&mut bytes, 0);
    push16(&mut bytes, 0);
    push16(&mut bytes, entries.len() as u16);
    push16(&mut bytes, entries.len() as u16);
    push32(&mut bytes, central_size);
    push32(&mut bytes, central_offset);
    push16(&mut bytes, 0);
    bytes
}

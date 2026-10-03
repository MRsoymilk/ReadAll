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
    let chapter_one = format!(
        "<html><body><h1>AAAA</h1><p>{}</p></body></html>",
        "AAAA WWWW ".repeat(80)
    );
    let chapter_two = format!(
        "<html><body><h1>WWWW</h1><p>{}</p></body></html>",
        "WWWW AAAA ".repeat(80)
    );
    build_epub(vec![
        (
            "one",
            "one.xhtml",
            "application/xhtml+xml",
            chapter_one.into_bytes(),
        ),
        (
            "two",
            "two.xhtml",
            "application/xhtml+xml",
            chapter_two.into_bytes(),
        ),
    ])
}

#[allow(dead_code)]
pub fn make_epub_with_long_title() -> Vec<u8> {
    let chapter = format!(
        "<html><body><h1>AAAA</h1><p>{}</p></body></html>",
        "AAAA WWWW ".repeat(80)
    );
    build_epub_named(
        "A Very Long ReadAll Test Book Title That Must Scroll Horizontally Across The Header",
        vec![(
            "one",
            "one.xhtml",
            "application/xhtml+xml",
            chapter.into_bytes(),
        )],
    )
}

#[allow(dead_code)]
pub fn make_epub_with_empty_spines() -> Vec<u8> {
    let chapter_one = format!(
        "<html><body><h1>AAAA</h1><p>{}</p></body></html>",
        "AAAA WWWW ".repeat(80)
    );
    let chapter_two = format!(
        "<html><body><h1>WWWW</h1><p>{}</p></body></html>",
        "WWWW AAAA ".repeat(80)
    );
    build_epub(vec![
        (
            "cover",
            "cover.svg",
            "image/svg+xml",
            br#"<svg xmlns="http://www.w3.org/2000/svg"><image href="cover.jpg"/></svg>"#.to_vec(),
        ),
        (
            "one",
            "one.xhtml",
            "application/xhtml+xml",
            chapter_one.into_bytes(),
        ),
        (
            "blank",
            "blank.xhtml",
            "application/xhtml+xml",
            b"<html><body> 
	 </body></html>".to_vec(),
        ),
        (
            "two",
            "two.xhtml",
            "application/xhtml+xml",
            chapter_two.into_bytes(),
        ),
        (
            "end",
            "end.xhtml",
            "application/xhtml+xml",
            br#"<html><body><svg xmlns="http://www.w3.org/2000/svg"><image href="end.jpg"/></svg></body></html>"#
                .to_vec(),
        ),
    ])
}

fn build_epub(spines: Vec<(&str, &str, &str, Vec<u8>)>) -> Vec<u8> {
    build_epub_named("ReadAll Test", spines)
}

fn build_epub_named(title: &str, spines: Vec<(&str, &str, &str, Vec<u8>)>) -> Vec<u8> {
    const CONTAINER: &[u8] = br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;

    let mut package = format!(
        r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>{title}</dc:title></metadata><manifest>"#,
    );
    for (id, href, media_type, _) in &spines {
        package.push_str(&format!(
            r#"<item id="{id}" href="{href}" media-type="{media_type}"/>"#
        ));
    }
    package.push_str("</manifest><spine>");
    for (id, _, _, _) in &spines {
        package.push_str(&format!(r#"<itemref idref="{id}"/>"#));
    }
    package.push_str("</spine></package>");

    let mut entries: Vec<(String, Vec<u8>)> = vec![
        ("mimetype".into(), b"application/epub+zip".to_vec()),
        ("META-INF/container.xml".into(), CONTAINER.to_vec()),
        ("OEBPS/package.opf".into(), package.into_bytes()),
    ];
    for (_, href, _, data) in spines {
        entries.push((format!("OEBPS/{href}"), data));
    }

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
        records.push((name.clone(), data.len() as u32, offset, crc));
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

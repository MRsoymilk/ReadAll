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
pub fn make_epub_with_navigation() -> Vec<u8> {
    let chapter_one = format!(
        "<html><body><h1 id=\"intro\">AAAA</h1><p>{}</p><h2 id=\"details\">WWWW</h2><p>{}</p></body></html>",
        "AAAA WWWW ".repeat(40),
        "WWWW AAAA ".repeat(40)
    );
    let chapter_two = format!(
        "<html><body><h1 id=\"deep\">WWWW</h1><p>{}</p></body></html>",
        "WWWW AAAA ".repeat(80)
    );
    let nav = r#"<?xml version="1.0"?><!DOCTYPE html><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="one.xhtml#intro">正式目录第一章</a><ol><li><a href="one.xhtml#details">第一章详细部分</a></li></ol></li><li><a href="two.xhtml#deep">正式目录第二章</a></li></ol></nav></body></html>"#;
    build_epub_named_with_nav(
        "ReadAll Navigation Test",
        vec![
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
        ],
        Some(nav.as_bytes().to_vec()),
    )
}

#[allow(dead_code)]
pub fn make_epub_with_ncx_navigation() -> Vec<u8> {
    let chapter_one = format!(
        "<html><body><h1 id=\"intro\">AAAA</h1><p>{}</p><h2 id=\"details\">WWWW</h2><p>{}</p></body></html>",
        "AAAA WWWW ".repeat(40),
        "WWWW AAAA ".repeat(40)
    );
    let chapter_two = format!(
        "<html><body><h1 id=\"deep\">WWWW</h1><p>{}</p></body></html>",
        "WWWW AAAA ".repeat(80)
    );
    let ncx = r#"<?xml version="1.0"?><!DOCTYPE ncx PUBLIC "-//NISO//DTD ncx 2005-1//EN" "http://www.daisy.org/z3986/2005/ncx-2005-1.dtd"><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap><navPoint id="one"><navLabel><text>旧目录第一章</text></navLabel><content src="one.xhtml#intro"/><navPoint id="details"><navLabel><text>第一章子节</text></navLabel><content src="one.xhtml#details"/></navPoint></navPoint><navPoint id="two"><navLabel><text>旧目录第二章</text></navLabel><content src="two.xhtml#deep"/></navPoint></navMap></ncx>"#;
    build_epub_named_with_navigation_resources(
        "ReadAll NCX Test",
        vec![
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
        ],
        None,
        Some(ncx.as_bytes().to_vec()),
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
            "cover.bin",
            "application/octet-stream",
            b"unsupported spine format".to_vec(),
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
	 </body></html>"
                .to_vec(),
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
            b"<html><body><!-- empty trailing chapter --></body></html>".to_vec(),
        ),
    ])
}

#[allow(dead_code)]
pub fn make_epub_with_svg_spine(svg: &str, resources: Vec<(&str, &str, Vec<u8>)>) -> Vec<u8> {
    let ids: Vec<_> = (0..resources.len()).map(|i| format!("image{i}")).collect();
    let resources = resources
        .into_iter()
        .enumerate()
        .map(|(i, (path, mime, bytes))| (ids[i].as_str(), path, mime, bytes))
        .collect();
    build_epub_all(
        "SVG publication",
        vec![("svg", "cover.svg", "image/svg+xml", svg.as_bytes().to_vec())],
        None,
        None,
        resources,
    )
}

#[allow(dead_code)]
pub fn make_epub_with_resources(
    chapters: &[&str],
    resources: Vec<(&str, &str, Vec<u8>)>,
) -> Vec<u8> {
    let ids: Vec<_> = (0..chapters.len()).map(|i| format!("chapter{i}")).collect();
    let paths: Vec<_> = (0..chapters.len())
        .map(|i| format!("chapter{i}.xhtml"))
        .collect();
    let resource_ids: Vec<_> = (0..resources.len())
        .map(|i| format!("resource{i}"))
        .collect();
    let spines = chapters
        .iter()
        .enumerate()
        .map(|(i, body)| {
            (
                ids[i].as_str(),
                paths[i].as_str(),
                "application/xhtml+xml",
                body.as_bytes().to_vec(),
            )
        })
        .collect();
    let resources = resources
        .into_iter()
        .enumerate()
        .map(|(i, (path, media, bytes))| (resource_ids[i].as_str(), path, media, bytes))
        .collect();
    build_epub_all("ReadAll Styled Test", spines, None, None, resources)
}

#[allow(dead_code)]
pub fn make_png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(bytes);
        out.extend_from_slice(&crc32(&out[start..]).to_be_bytes());
    }
    let mut row = vec![0];
    for _ in 0..width {
        row.extend_from_slice(&rgba);
    }
    let raw = row.repeat(height as usize);
    let mut z = vec![0x78, 0x01];
    let chunks = raw.chunks(65535);
    let count = chunks.len();
    for (index, part) in chunks.enumerate() {
        let n = part.len() as u16;
        z.push(u8::from(index + 1 == count));
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(part);
    }
    let (mut a, mut b) = (1_u32, 0_u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&(b << 16 | a).to_be_bytes());
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut bytes, b"IHDR", &header);
    chunk(&mut bytes, b"IDAT", &z);
    chunk(&mut bytes, b"IEND", &[]);
    bytes
}

fn build_epub(spines: Vec<(&str, &str, &str, Vec<u8>)>) -> Vec<u8> {
    build_epub_named("ReadAll Test", spines)
}

fn build_epub_named(title: &str, spines: Vec<(&str, &str, &str, Vec<u8>)>) -> Vec<u8> {
    build_epub_named_with_nav(title, spines, None)
}

fn build_epub_named_with_nav(
    title: &str,
    spines: Vec<(&str, &str, &str, Vec<u8>)>,
    navigation: Option<Vec<u8>>,
) -> Vec<u8> {
    build_epub_named_with_navigation_resources(title, spines, navigation, None)
}

fn build_epub_named_with_navigation_resources(
    title: &str,
    spines: Vec<(&str, &str, &str, Vec<u8>)>,
    navigation: Option<Vec<u8>>,
    ncx: Option<Vec<u8>>,
) -> Vec<u8> {
    build_epub_all(title, spines, navigation, ncx, Vec::new())
}

fn build_epub_all(
    title: &str,
    spines: Vec<(&str, &str, &str, Vec<u8>)>,
    navigation: Option<Vec<u8>>,
    ncx: Option<Vec<u8>>,
    resources: Vec<(&str, &str, &str, Vec<u8>)>,
) -> Vec<u8> {
    const CONTAINER: &[u8] = br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;

    let mut package = format!(
        r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>{title}</dc:title></metadata><manifest>"#,
    );
    for (id, href, media_type, _) in spines.iter().chain(resources.iter()) {
        package.push_str(&format!(
            r#"<item id="{id}" href="{href}" media-type="{media_type}"/>"#
        ));
    }
    if navigation.is_some() {
        package.push_str(
            r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#,
        );
    }
    if ncx.is_some() {
        package
            .push_str(r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#);
    }
    if ncx.is_some() {
        package.push_str("</manifest><spine toc=\"ncx\">");
    } else {
        package.push_str("</manifest><spine>");
    }
    for (id, _, _, _) in &spines {
        package.push_str(&format!(r#"<itemref idref="{id}"/>"#));
    }
    package.push_str("</spine></package>");

    let mut entries: Vec<(String, Vec<u8>)> = vec![
        ("mimetype".into(), b"application/epub+zip".to_vec()),
        ("META-INF/container.xml".into(), CONTAINER.to_vec()),
        ("OEBPS/package.opf".into(), package.into_bytes()),
    ];
    if let Some(navigation) = navigation {
        entries.push(("OEBPS/nav.xhtml".into(), navigation));
    }
    if let Some(ncx) = ncx {
        entries.push(("OEBPS/toc.ncx".into(), ncx));
    }
    for (_, href, _, data) in spines.into_iter().chain(resources) {
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

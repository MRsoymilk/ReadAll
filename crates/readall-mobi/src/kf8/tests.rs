use super::*;
use crate::MobiLimits;
#[path = "../../../../apps/readall/tests/support/azw3.rs"]
mod fixture;
use readall_epub::{EpubBook, EpubLimits};
const HEAD: &str = "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>中文 AZW3</title><link rel=\"stylesheet\" href=\"kindle:flow:0001?mime=text/css\"/></head><body>";
const TAIL: &str = "</body></html>";
fn sample(compression: u16) -> Vec<u8> {
    fixture::build(
        &[
            fixture::Chapter {
                head: HEAD,
                fragments: vec![
                    "<h1>第一章</h1><p><a href=\"kindle:pos:fid:0002:off:0000000000\">下一章</a></p>",
                    "<pre class=\"language-c\">#define PAGE_SIZE 4096\n\treturn 42;\n</pre>",
                ],
                tail: TAIL,
            },
            fixture::Chapter {
                head: HEAD,
                fragments: vec![
                    "<h2 id=\"中文锚点\">第二章</h2><p>target 世界</p><a href=\"kindle:pos:fid:0000:off:0000000000\">返回</a>",
                ],
                tail: TAIL,
            },
        ],
        fixture::Options {
            compression,
            flows: vec![b"p { color:#123456; text-indent:2em }".to_vec()],
            navigation: vec![
                fixture::Nav {
                    label: "第一章",
                    fid: 0,
                    offset: 0,
                    parent: None,
                },
                fixture::Nav {
                    label: "代码",
                    fid: 1,
                    offset: 0,
                    parent: Some(0),
                },
                fixture::Nav {
                    label: "第二章",
                    fid: 2,
                    offset: 0,
                    parent: None,
                },
            ],
            ..Default::default()
        },
    )
}
#[test]
fn kf8_reconstructs_fragments_css_nested_ncx_links_and_code_in_all_compressions() {
    let mut reference = None;
    for compression in [1, 2, 17480] {
        let source = sample(compression);
        let mobi = MobiBook::parse(&source, MobiLimits::default()).unwrap();
        assert_eq!(mobi.metadata.version, 8);
        let converted = mobi.to_epub().unwrap();
        assert!(converted.warnings.is_empty(), "{:?}", converted.warnings);
        let book = EpubBook::parse(&converted.epub, EpubLimits::default()).unwrap();
        assert_eq!(book.spine().len(), 2);
        let a = book.read_spine_content(0).unwrap();
        let b = book.read_spine_content(1).unwrap();
        assert!(a.text.contains("第一章") && b.text.contains("target 世界"));
        assert!(a.text.contains("#define PAGE_SIZE 4096\n\treturn 42;"));
        assert!(a.runs.iter().any(|r| r.style.color == [18, 52, 86]));
        let nav = book.navigation().unwrap();
        assert_eq!(nav.len(), 3);
        assert_eq!(nav[1].depth(), 1);
        assert_eq!(nav[2].spine_index(), 1);
        let link = a.links.first().unwrap();
        assert!(link.href.starts_with("section1.xhtml#"));
        let target = link.href.split_once('#').unwrap().1;
        assert!(book.locator_for_fragment(1, target).unwrap().is_some());
        assert!(book.locator_for_fragment(1, "中文锚点").unwrap().is_some());
        if let Some(text) = &reference {
            assert_eq!(&a.text, text);
        } else {
            reference = Some(a.text);
        }
    }
}
#[test]
fn reconstructed_resource_uris_do_not_rewrite_visible_code_or_svg_case() {
    let svg=b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 40 20\"><image href=\"kindle:embed:0001?mime=image/png\" width=\"40\" height=\"20\"/></svg>".to_vec();
    let png = b"\x89PNG\r\n\x1a\nindependent placeholder payload".to_vec();
    let source = fixture::build(
        &[fixture::Chapter {
            head: HEAD,
            fragments: vec![
                "<pre>kindle:flow:0001?mime=text/css\nkindle:embed:0001?mime=image/png</pre><img src=\"kindle:flow:0002?mime=image/svg+xml\"/>",
            ],
            tail: TAIL,
        }],
        fixture::Options {
            flows: vec![b"p{color:red}".to_vec(), svg],
            resources: vec![png.clone()],
            ..Default::default()
        },
    );
    let converted = MobiBook::parse(&source, Default::default())
        .unwrap()
        .to_epub()
        .unwrap();
    assert!(converted.warnings.is_empty(), "{:?}", converted.warnings);
    let book = EpubBook::parse(&converted.epub, Default::default()).unwrap();
    let content = book.read_spine_content(0).unwrap();
    assert!(content.text.contains("kindle:flow:0001?mime=text/css"));
    let svg = String::from_utf8(book.read_spine_resource(0, "flow2", 4096).unwrap()).unwrap();
    assert!(svg.contains("viewBox=\"0 0 40 20\""));
    assert!(svg.contains("href=\"embed0\""));
    assert_eq!(book.read_spine_resource(0, "embed0", 4096).unwrap(), png);
}
#[test]
fn svg_wrapped_cover_is_not_inserted_a_second_time() {
    let source = fixture::build(&[fixture::Chapter {
        head: "<html><body>", fragments: vec!["<img src=\"kindle:flow:0001?mime=image/svg+xml\"/>"], tail: TAIL,
    }], fixture::Options {
        flows: vec![b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 40 20\"><image href=\"kindle:embed:0001?mime=image/png\" width=\"40\" height=\"20\"/></svg>".to_vec()],
        resources: vec![b"\x89PNG\r\n\x1a\nplaceholder payload".to_vec()], cover: Some(0), ..Default::default()
    });
    let converted = MobiBook::parse(&source, Default::default())
        .unwrap()
        .to_epub()
        .unwrap();
    assert!(converted.warnings.is_empty());
    let book = EpubBook::parse(&converted.epub, Default::default()).unwrap();
    assert_eq!(book.spine().len(), 1);
    assert_eq!(book.read_spine_content(0).unwrap().images.len(), 1);
}

#[test]
fn cycles_missing_images_and_remote_urls_do_not_fetch_or_recurse() {
    let source = fixture::build(
        &[fixture::Chapter {
            head: HEAD,
            fragments: vec![
                "<p>AAAA</p><img src=\"kindle:embed:VVVV?mime=image/png\"/><img src=\"https://example.invalid/private.png\"/>",
            ],
            tail: TAIL,
        }],
        fixture::Options {
            flows: vec![b"@import url('kindle:flow:0001?mime=text/css');p{color:blue}".to_vec()],
            ..Default::default()
        },
    );
    let converted = MobiBook::parse(&source, Default::default())
        .unwrap()
        .to_epub()
        .unwrap();
    assert!(converted.warnings.iter().any(|s| s.contains("unavailable")));
    let book = EpubBook::parse(&converted.epub, Default::default()).unwrap();
    assert!(book.read_spine_text(0).unwrap().contains("AAAA"));
    assert!(
        book.read_spine_resource(0, "https://example.invalid/private.png", 1024)
            .is_err()
    );
}
fn font_record(flags: u32) -> (Vec<u8>, Vec<u8>) {
    let mut raw_font = b"\0\x01\0\0".to_vec();
    raw_font.extend((0..64).map(|n| n as u8));
    let mut payload = raw_font.clone();
    if flags & 1 != 0 {
        let n = payload.len() as u16;
        let mut z = vec![0x78, 1, 1];
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(&payload);
        let (mut a, mut b) = (1_u32, 0_u32);
        for &v in &payload {
            a = (a + u32::from(v)) % 65521;
            b = (b + a) % 65521;
        }
        z.extend_from_slice(&(b << 16 | a).to_be_bytes());
        payload = z;
    }
    let mut data = vec![0; 24];
    data[..4].copy_from_slice(b"FONT");
    fixture::set32(&mut data, 4, raw_font.len() as u32);
    fixture::set32(&mut data, 8, flags);
    if flags & 2 != 0 {
        let key = [0x5a; 16];
        fixture::set32(&mut data, 16, 16);
        fixture::set32(&mut data, 20, 24);
        data.extend_from_slice(&key);
        for (i, b) in payload.iter_mut().take(1024).enumerate() {
            *b ^= key[i % 16];
        }
    }
    let start = data.len() as u32;
    fixture::set32(&mut data, 12, start);
    data.extend(payload);
    (data, raw_font)
}
#[test]
fn font_records_are_unwrapped_with_bounded_zlib_and_format_obfuscation() {
    for flags in 0..4 {
        let (raw, expected) = font_record(flags);
        let source=fixture::build(&[fixture::Chapter{head:HEAD,fragments:vec!["<p>AAAA</p>"],tail:TAIL}],fixture::Options{flows:vec![b"@font-face{font-family:Book;src:url('kindle:embed:0001?mime=application/x-font-ttf')}p{font-family:Book}".to_vec()],resources:vec![raw],..Default::default()});
        let converted = MobiBook::parse(&source, Default::default())
            .unwrap()
            .to_epub()
            .unwrap();
        assert!(converted.warnings.is_empty(), "{:?}", converted.warnings);
        let book = EpubBook::parse(&converted.epub, Default::default()).unwrap();
        assert_eq!(
            book.read_spine_resource(0, "embed0", 4096).unwrap(),
            expected
        );
    }
}
#[test]
fn bad_indexes_flows_drm_and_future_versions_fail_without_output() {
    let source = sample(2);
    let original = fixture::records(&source);
    for field in [192, 248, 252] {
        let mut records = original.clone();
        fixture::set32(&mut records[0], field, u32::MAX - 1);
        assert!(MobiBook::parse(&fixture::assemble(records), Default::default()).is_err());
    }
    let mut records = original.clone();
    fixture::set32(&mut records[0], 36, 9);
    assert!(matches!(
        MobiBook::parse(&fixture::assemble(records), Default::default()),
        Err(MobiError::Unsupported(_))
    ));
    let mut records = original.clone();
    records[0][12..14].copy_from_slice(&2_u16.to_be_bytes());
    assert!(matches!(
        MobiBook::parse(&fixture::assemble(records), Default::default()),
        Err(MobiError::Encrypted)
    ));
    for field in [192, 248, 252] {
        let mut records = original.clone();
        let n = u32be(&records[0], field).unwrap() as usize;
        records[n][0] = 0;
        assert!(
            MobiBook::parse(&fixture::assemble(records), Default::default())
                .and_then(|b| b.to_epub())
                .is_err()
        );
    }
    let mut records = original;
    let n = u32be(&records[0], 192).unwrap() as usize;
    fixture::set32(&mut records[n], 16, u32::MAX);
    assert!(
        MobiBook::parse(&fixture::assemble(records), Default::default())
            .and_then(|b| b.to_epub())
            .is_err()
    );
}
#[test]
fn synthetic_kf8_is_deterministic_cancellable_and_source_specific() {
    let source = sample(2);
    let book = MobiBook::parse(&source, Default::default()).unwrap();
    let first = book.to_epub().unwrap();
    assert_eq!(first.epub, book.to_epub().unwrap().epub);
    for stage in [
        Stage::Decompress,
        Stage::Index,
        Stage::Markup,
        Stage::Package,
    ] {
        assert!(matches!(
            book.to_epub_with_progress(|p| p.stage != stage),
            Err(MobiError::Cancelled)
        ));
    }
    let mut changed = source;
    changed[35] ^= 1;
    assert_ne!(
        first.epub,
        MobiBook::parse(&changed, Default::default())
            .unwrap()
            .to_epub()
            .unwrap()
            .epub
    );
}
#[test]
fn skeleton_without_fragments_is_preserved() {
    let source = fixture::build(
        &[fixture::Chapter {
            head: "<html><body><p>AAAA</p>",
            fragments: vec![],
            tail: TAIL,
        }],
        Default::default(),
    );
    let converted = MobiBook::parse(&source, Default::default())
        .unwrap()
        .to_epub()
        .unwrap();
    let book = EpubBook::parse(&converted.epub, Default::default()).unwrap();
    assert_eq!(book.read_spine_text(0).unwrap(), "AAAA");
}
#[test]
fn cncx_alignment_padding_is_accepted_but_nonzero_truncation_is_rejected() {
    let source = sample(2);
    let mut rows = fixture::records(&source);
    let ncx = u32be(&rows[0], 244).unwrap() as usize;
    let strings = ncx + u32be(&rows[ncx], 24).unwrap() as usize + 1;
    let raw_length = rows[strings].len();
    while !rows[strings].len().is_multiple_of(4) {
        rows[strings].push(0);
    }
    assert!(rows[strings].len() > raw_length);
    let source = fixture::assemble(rows.clone());
    let book = MobiBook::parse(&source, Default::default()).unwrap();
    let index = index::read(&book, ncx, &mut |_| true).unwrap();
    assert_eq!(index.entries.len(), 3);
    assert_eq!(index.strings.len(), 3);
    *rows[strings].last_mut().unwrap() = 1;
    let corrupt = fixture::assemble(rows);
    let book = MobiBook::parse(&corrupt, Default::default()).unwrap();
    assert!(index::read(&book, ncx, &mut |_| true).is_err());
}

#[test]
fn index_variable_integer_checks_termination_and_overflow() {
    for n in [0, 127, 128, 16384, u32::MAX] {
        let data = fixture::varint(n);
        let mut at = 0;
        assert_eq!(index::varint(&data, &mut at).unwrap(), n);
        assert_eq!(at, data.len());
    }
    for data in [vec![], vec![0, 0, 0, 0, 0], vec![127, 127, 127, 127, 255]] {
        assert!(index::varint(&data, &mut 0).is_err());
    }
}
#[test]
fn byte_mutations_and_truncations_do_not_panic() {
    let source = fixture::make_azw3("<p>测试 AAAA</p>");
    for at in (0..source.len()).step_by(17) {
        let mut data = source.clone();
        data[at] ^= 255;
        let _ = MobiBook::parse(&data, Default::default()).and_then(|b| b.to_epub());
    }
    for end in (0..source.len()).step_by(13) {
        let _ = MobiBook::parse(&source[..end], Default::default()).and_then(|b| b.to_epub());
    }
}

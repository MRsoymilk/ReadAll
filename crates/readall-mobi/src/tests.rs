use super::*;
#[path = "../../../apps/readall/tests/support/mobi.rs"]
mod fixture;
use readall_epub::{EpubBook, EpubLimits};
fn header_offset(bytes: &[u8]) -> usize {
    u32be(bytes, 78).unwrap() as usize
}
fn set32(bytes: &mut [u8], at: usize, n: u32) {
    bytes[at..at + 4].copy_from_slice(&n.to_be_bytes());
}
#[test]
fn all_three_compressions_utf8_boundaries_and_trailers_agree() {
    let html = "<html><body><h1>中文 MOBI</h1><p>Hello 世界 &amp; café</p><pre>#define PAGE_SIZE 4096\n\nint main() {\n\treturn 0;\n}</pre></body></html>";
    let mut reference = None;
    for compression in [1, 2, 17480] {
        for trailing in [false, true] {
            let bytes = fixture::build(
                html.as_bytes(),
                fixture::Options {
                    compression,
                    trailing,
                    record_size: 31,
                    ..Default::default()
                },
            );
            let mobi = MobiBook::parse(&bytes, MobiLimits::default()).unwrap();
            assert_eq!(mobi.metadata().title, "ReadAll MOBI 中文");
            let result = mobi.to_epub().unwrap();
            let epub = EpubBook::parse(&result.epub, EpubLimits::default()).unwrap();
            assert_eq!(epub.creator(), Some("ReadAll Tests"));
            let text = epub.read_spine_text(0).unwrap();
            assert!(text.contains("Hello 世界 & café"));
            assert!(text.contains("#define PAGE_SIZE 4096\n\nint main() {\n\treturn 0;\n}"));
            if let Some(expected) = &reference {
                assert_eq!(&text, expected);
            } else {
                reference = Some(text);
            }
        }
    }
}
#[test]
fn old_html_repairs_unquoted_attrs_void_tags_and_omitted_paragraph_closures() {
    let bytes = fixture::make_mobi(
        "<HTML><HEAD><TITLE>Hidden</TITLE></HEAD><BODY><H1>Heading</H1><P ALIGN=center>One&nbsp;&copy;&mdash;<BR>Two<P>Three<FONT COLOR=red SIZE=4>Red</FONT><script>alert('bad')</script></BODY></HTML>",
    );
    let adapted = MobiBook::parse(&bytes, MobiLimits::default())
        .unwrap()
        .to_epub()
        .unwrap();
    let book = EpubBook::parse(&adapted.epub, EpubLimits::default()).unwrap();
    let content = book.read_spine_content(0).unwrap();
    assert!(content.text.contains("One"));
    assert!(content.text.contains('©'));
    assert!(!content.text.contains("Hidden") && !content.text.contains("alert"));
    assert!(content.runs.iter().any(|r| r.style.color == [255, 0, 0]));
}
#[test]
fn windows_1252_and_numeric_entities_are_decoded() {
    let bytes = fixture::build(
        b"<html><body><p>Caf\xe9 \x93quoted\x94 &#20013;&#25991;</p></body></html>",
        fixture::Options {
            encoding: 1252,
            ..Default::default()
        },
    );
    let adapted = MobiBook::parse(&bytes, MobiLimits::default())
        .unwrap()
        .to_epub()
        .unwrap();
    let book = EpubBook::parse(&adapted.epub, EpubLimits::default()).unwrap();
    assert!(
        book.read_spine_text(0)
            .unwrap()
            .contains("Café “quoted” 中文")
    );
}
#[test]
fn filepos_named_links_and_toc_survive_multibyte_text_and_section_splits() {
    let mut html = "<html><head><guide><reference type=toc filepos=0000000000></guide></head><body><h1>书籍</h1><p><a filepos=1111111111>第二章</a></p><mbp:pagebreak/><h2 id=second>中文目标</h2><p><a href='#first'>返回</a></p><mbp:pagebreak/><h2 id=first>末尾</h2></body></html>".to_owned();
    let target = html.find("<h2 id=second>").unwrap();
    let toc = html.find("<p><a filepos").unwrap();
    html = html
        .replace("0000000000", &format!("{toc:010}"))
        .replace("1111111111", &format!("{target:010}"));
    let bytes = fixture::make_mobi(&html);
    let adapted = MobiBook::parse(&bytes, MobiLimits::default())
        .unwrap()
        .to_epub()
        .unwrap();
    let book = EpubBook::parse(&adapted.epub, EpubLimits::default()).unwrap();
    assert_eq!(book.spine().len(), 3);
    let nav = book.navigation().unwrap();
    assert!(
        nav.iter()
            .any(|n| n.label() == "第二章" && n.spine_index() == 1)
    );
    let anchor = book
        .locator_for_fragment(1, &format!("mobi-pos-{target}"))
        .unwrap()
        .unwrap();
    assert_eq!(book.restore(&anchor).unwrap().0, 1);
    let content = book.read_spine_content(1).unwrap();
    assert!(
        content
            .links
            .iter()
            .any(|link| link.href.contains("section2.xhtml#mobi-node-"))
    );
}
#[test]
fn filepos_in_utf8_continuation_or_entity_never_corrupts_text() {
    for middle in ["中", "&copy;"] {
        let mut html =
            format!("<html><body><a filepos=0000000000>go</a><p>{middle} end</p></body></html>");
        let target = html.find(&format!("{middle} end")).unwrap() + 1;
        html = html.replace("0000000000", &format!("{target:010}"));
        let adapted = MobiBook::parse(&fixture::make_mobi(&html), MobiLimits::default())
            .unwrap()
            .to_epub()
            .unwrap();
        let book = EpubBook::parse(&adapted.epub, EpubLimits::default()).unwrap();
        assert!(
            book.read_spine_text(0)
                .unwrap()
                .contains(if middle == "中" { "中 end" } else { "© end" })
        );
        assert!(
            book.locator_for_fragment(0, &format!("mobi-pos-{target}"))
                .unwrap()
                .is_some()
        );
    }
}
#[test]
fn package_is_deterministic_source_specific_and_cancellable() {
    let bytes = fixture::make_mobi("<html><body>Hello</body></html>");
    let mobi = MobiBook::parse(&bytes, MobiLimits::default()).unwrap();
    assert_eq!(mobi.to_epub().unwrap().epub, mobi.to_epub().unwrap().epub);
    for cancel_at in [Stage::Decompress, Stage::Markup, Stage::Package] {
        assert!(matches!(
            mobi.to_epub_with_progress(|p| p.stage != cancel_at),
            Err(MobiError::Cancelled)
        ));
    }
    let original = mobi.to_epub().unwrap().epub;
    let mut changed = bytes.clone();
    changed[35] ^= 1;
    assert_ne!(
        original,
        MobiBook::parse(&changed, MobiLimits::default())
            .unwrap()
            .to_epub()
            .unwrap()
            .epub
    );
}
#[test]
fn bad_headers_drm_kf8_unknown_compression_and_budgets_are_explicit() {
    let bytes = fixture::make_mobi("<html><body>Hello</body></html>");
    for end in 0..bytes.len() {
        let attempt =
            MobiBook::parse(&bytes[..end], MobiLimits::default()).and_then(|b| b.to_epub());
        assert!(attempt.is_err());
    }
    let at = header_offset(&bytes);
    let mut drm = bytes.clone();
    drm[at + 12..at + 14].copy_from_slice(&2_u16.to_be_bytes());
    assert!(matches!(
        MobiBook::parse(&drm, MobiLimits::default()),
        Err(MobiError::Encrypted)
    ));
    let mut drm = bytes.clone();
    set32(&mut drm, at + 168, 1);
    assert!(matches!(
        MobiBook::parse(&drm, MobiLimits::default()),
        Err(MobiError::Encrypted)
    ));
    let mut kf8 = bytes.clone();
    set32(&mut kf8, at + 36, 8);
    assert!(matches!(
        MobiBook::parse(&kf8, MobiLimits::default()),
        Err(MobiError::Unsupported(_))
    ));
    let mut bad = bytes.clone();
    set32(&mut bad, 86, 1);
    assert!(MobiBook::parse(&bad, MobiLimits::default()).is_err());
    assert!(
        MobiBook::parse(
            &bytes,
            MobiLimits {
                max_file_bytes: 8,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        MobiBook::parse(
            &bytes,
            MobiLimits {
                max_text_bytes: 8,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        MobiBook::parse(
            &bytes,
            MobiLimits {
                max_package_bytes: 8,
                ..Default::default()
            }
        )
        .unwrap()
        .to_epub()
        .is_err()
    );
}
#[test]
fn dual_book_selects_legacy_part_and_marks_the_limitation() {
    let bytes = fixture::build(
        b"<html><body>Legacy readable</body></html>",
        fixture::Options {
            extra: vec![(121, 42_u32.to_be_bytes().to_vec())],
            ..Default::default()
        },
    );
    let result = MobiBook::parse(&bytes, MobiLimits::default())
        .unwrap()
        .to_epub()
        .unwrap();
    assert!(result.metadata.dual_format);
    assert!(result.warnings.iter().any(|s| s.contains("KF8")));
}
#[test]
fn huff_cycles_fail_instead_of_recursing_forever() {
    let bytes = fixture::build(
        b"\xff",
        fixture::Options {
            encoding: 1252,
            compression: 17480,
            ..Default::default()
        },
    );
    let mut corrupt = bytes.clone();
    let cdic = u32be(&bytes, 78 + 3 * 8).unwrap() as usize;
    corrupt[cdic + 16 + 512..cdic + 16 + 514].copy_from_slice(&1_u16.to_be_bytes());
    corrupt[cdic + 16 + 514] = 255;
    let book = MobiBook::parse(&corrupt, MobiLimits::default()).unwrap();
    assert!(book.to_epub().unwrap_err().to_string().contains("cyclic"));
}
#[test]
fn huff_secondary_table_and_compressed_dictionary_phrases_expand() {
    let html = b"<html><body>Q</body></html>";
    let bytes = fixture::build(
        html,
        fixture::Options {
            compression: 17480,
            ..Default::default()
        },
    );
    let source = MobiBook::parse(&bytes, MobiLimits::default()).unwrap();
    let mut records: Vec<_> = (0..source.records.len())
        .map(|n| source.record(n).unwrap().to_vec())
        .collect();
    // Force codes through the secondary range table rather than the quick flag.
    for i in 0..256 {
        set32(&mut records[2], 24 + i * 4, 8);
    }
    set32(&mut records[2], 1048 + 7 * 8, 0);
    set32(&mut records[2], 1048 + 7 * 8 + 4, 255);
    let mut table = Vec::new();
    let mut phrases = Vec::new();
    for index in 0..256 {
        table.extend_from_slice(&((512 + phrases.len()) as u16).to_be_bytes());
        if index == 255 - usize::from(b'Q') {
            phrases.extend_from_slice(&2_u16.to_be_bytes()); // compressed, not a literal
            phrases.extend_from_slice(b"OK");
        } else {
            phrases.extend_from_slice(&0x8001_u16.to_be_bytes());
            phrases.push((255 - index) as u8);
        }
    }
    records[3].truncate(16);
    records[3].extend(table);
    records[3].extend(phrases);
    set32(&mut records[0], 4, html.len() as u32 + 1);
    let bytes = fixture::assemble(records);
    let converted = MobiBook::parse(&bytes, MobiLimits::default())
        .unwrap()
        .to_epub()
        .unwrap();
    let book = EpubBook::parse(&converted.epub, EpubLimits::default()).unwrap();
    assert_eq!(book.read_spine_text(0).unwrap(), "OK");
}
#[test]
fn long_preformatted_text_splits_at_safe_boundaries_without_losing_lines() {
    let source = "int main() { return 0; } // 中文\n".repeat(9500);
    let html = format!("<html><body><pre>{source}</pre></body></html>");
    let converted = MobiBook::parse(&fixture::make_mobi(&html), MobiLimits::default())
        .unwrap()
        .to_epub()
        .unwrap();
    let book = EpubBook::parse(&converted.epub, EpubLimits::default()).unwrap();
    assert!(book.spine().len() > 1);
    let mut text = String::new();
    for n in 0..book.spine().len() {
        text.push_str(&book.read_spine_text(n).unwrap());
    }
    assert_eq!(text.matches("int main()").count(), 9500);
    assert_eq!(text.matches("中文").count(), 9500);
    assert_eq!(text.replace('\n', ""), source.replace('\n', ""));
}

#[test]
fn parser_mutations_do_not_panic() {
    let original = fixture::make_mobi("<html><body><p>test 中文</p></body></html>");
    for i in (0..original.len()).step_by(3) {
        let mut data = original.clone();
        data[i] ^= 255;
        let _ = MobiBook::parse(&data, MobiLimits::default()).and_then(|b| b.to_epub());
    }
}

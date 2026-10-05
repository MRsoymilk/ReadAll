//! Original synthetic KF8 files with real TAGX/IDXT indexes and split skeletons.
#![allow(dead_code)]
use crate::test_mobi as mobi;
use std::collections::BTreeMap;
#[derive(Clone)]
pub struct Chapter<'a> {
    pub head: &'a str,
    pub fragments: Vec<&'a str>,
    pub tail: &'a str,
}
pub struct Nav<'a> {
    pub label: &'a str,
    pub fid: u32,
    pub offset: u32,
    pub parent: Option<u32>,
}
pub struct Options<'a> {
    pub compression: u16,
    pub flows: Vec<Vec<u8>>,
    pub resources: Vec<Vec<u8>>,
    pub cover: Option<u32>,
    pub navigation: Vec<Nav<'a>>,
}
impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            compression: 2,
            flows: Vec::new(),
            resources: Vec::new(),
            cover: None,
            navigation: Vec::new(),
        }
    }
}
pub fn set32(bytes: &mut [u8], at: usize, n: u32) {
    bytes[at..at + 4].copy_from_slice(&n.to_be_bytes());
}
pub fn varint(mut value: u32) -> Vec<u8> {
    let mut bytes = vec![(value as u8 & 127) | 128];
    value >>= 7;
    while value != 0 {
        bytes.push(value as u8 & 127);
        value >>= 7;
    }
    bytes.reverse();
    bytes
}
pub fn records(bytes: &[u8]) -> Vec<Vec<u8>> {
    let count = u16::from_be_bytes(bytes[76..78].try_into().unwrap()) as usize;
    let mut starts: Vec<_> = (0..count)
        .map(|i| u32::from_be_bytes(bytes[78 + i * 8..82 + i * 8].try_into().unwrap()) as usize)
        .collect();
    starts.push(bytes.len());
    starts
        .windows(2)
        .map(|p| bytes[p[0]..p[1]].to_vec())
        .collect()
}
pub fn assemble(records: Vec<Vec<u8>>) -> Vec<u8> {
    mobi::assemble(records)
}
type Row = (String, BTreeMap<u8, Vec<u32>>);
pub fn index(rows: Vec<Row>, descriptors: &[[u8; 4]], strings: Vec<u8>) -> Vec<Vec<u8>> {
    let mut main = vec![0; 192];
    main[..4].copy_from_slice(b"INDX");
    set32(&mut main, 4, 192);
    set32(&mut main, 24, 1);
    set32(&mut main, 28, 65001);
    set32(&mut main, 36, rows.len() as u32);
    set32(&mut main, 52, u32::from(!strings.is_empty()));
    main.extend_from_slice(b"TAGX");
    main.extend_from_slice(&((12 + descriptors.len() * 4 + 4) as u32).to_be_bytes());
    main.extend_from_slice(&1_u32.to_be_bytes());
    for tag in descriptors {
        main.extend_from_slice(tag);
    }
    main.extend_from_slice(&[0, 0, 0, 1]);
    let mut data = vec![0; 192];
    data[..4].copy_from_slice(b"INDX");
    set32(&mut data, 4, 192);
    set32(&mut data, 24, rows.len() as u32);
    let mut offsets = Vec::new();
    for (name, tags) in rows {
        offsets.push(data.len() as u16);
        data.push(name.len() as u8);
        data.extend_from_slice(name.as_bytes());
        let mut control = 0;
        let mut values = Vec::new();
        for [tag, n, mask, _] in descriptors {
            if let Some(items) = tags.get(tag) {
                assert_eq!(items.len(), usize::from(*n));
                control |= *mask;
                for v in items {
                    values.extend(varint(*v));
                }
            }
        }
        data.push(control);
        data.extend(values);
    }
    let idxt = data.len();
    set32(&mut data, 20, idxt as u32);
    data.extend_from_slice(b"IDXT");
    for at in offsets {
        data.extend_from_slice(&at.to_be_bytes());
    }
    let mut result = vec![main, data];
    if !strings.is_empty() {
        result.push(strings);
    }
    result
}
pub fn make_azw3(body: &str) -> Vec<u8> {
    build(
        &[Chapter {
            head: "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>Test AZW3</title></head><body>",
            fragments: vec![body],
            tail: "</body></html>",
        }],
        Options::default(),
    )
}
pub fn build(chapters: &[Chapter<'_>], options: Options<'_>) -> Vec<u8> {
    let mut text = Vec::new();
    let (mut skels, mut frags) = (Vec::new(), Vec::new());
    let mut fid = 0;
    for (file, chapter) in chapters.iter().enumerate() {
        let start = text.len();
        text.extend_from_slice(chapter.head.as_bytes());
        text.extend_from_slice(chapter.tail.as_bytes());
        let length = text.len() - start;
        skels.push((
            format!("SKEL{file:04}"),
            BTreeMap::from([
                (1, vec![chapter.fragments.len() as u32]),
                (6, vec![start as u32, length as u32]),
            ]),
        ));
        let mut offset = 0;
        for fragment in &chapter.fragments {
            frags.push((
                (start + chapter.head.len() + offset).to_string(),
                BTreeMap::from([
                    (3, vec![file as u32]),
                    (4, vec![fid]),
                    (6, vec![offset as u32, fragment.len() as u32]),
                ]),
            ));
            text.extend_from_slice(fragment.as_bytes());
            offset += fragment.len();
            fid += 1;
        }
    }
    let mut flow_ranges = vec![(0, text.len())];
    for flow in options.flows {
        let start = text.len();
        text.extend(flow);
        flow_ranges.push((start, text.len()));
    }
    let original = mobi::build(
        &text,
        mobi::Options {
            compression: options.compression,
            images: options.resources,
            cover: options.cover,
            record_size: 4096,
            ..Default::default()
        },
    );
    let mut records = records(&original);
    let old = records[0].clone();
    let mut header = vec![0; 280];
    header[..248].copy_from_slice(&old[..248]);
    header.extend_from_slice(&old[248..]);
    set32(&mut header, 20, 264);
    set32(&mut header, 36, 8);
    set32(
        &mut header,
        84,
        u32::from_be_bytes(old[84..88].try_into().unwrap()) + 32,
    );
    set32(&mut header, 260, u32::MAX);
    set32(&mut header, 244, u32::MAX);
    let frag = records.len();
    records.extend(index(
        frags,
        &[[3, 1, 1, 0], [4, 1, 2, 0], [6, 2, 4, 0]],
        vec![],
    ));
    set32(&mut header, 248, frag as u32);
    let skel = records.len();
    records.extend(index(skels, &[[1, 1, 1, 0], [6, 2, 2, 0]], vec![]));
    set32(&mut header, 252, skel as u32);
    if !options.navigation.is_empty() {
        let mut rows = Vec::new();
        let mut strings = Vec::new();
        let mut depths = Vec::new();
        for (i, nav) in options.navigation.iter().enumerate() {
            let label = strings.len();
            strings.extend(varint(nav.label.len() as u32));
            strings.extend_from_slice(nav.label.as_bytes());
            let depth = nav.parent.map_or(0, |p| depths[p as usize] + 1);
            depths.push(depth);
            let mut tags = BTreeMap::from([
                (3, vec![label as u32]),
                (4, vec![depth]),
                (6, vec![nav.fid, nav.offset]),
            ]);
            if let Some(p) = nav.parent {
                tags.insert(21, vec![p]);
            }
            rows.push((i.to_string(), tags));
        }
        let ncx = records.len();
        records.extend(index(
            rows,
            &[[3, 1, 1, 0], [4, 1, 2, 0], [6, 2, 4, 0], [21, 1, 8, 0]],
            strings,
        ));
        set32(&mut header, 244, ncx as u32);
    }
    let fdst = records.len();
    let mut table = b"FDST\0\0\0\x0c".to_vec();
    table.extend_from_slice(&(flow_ranges.len() as u32).to_be_bytes());
    for (start, end) in &flow_ranges {
        table.extend_from_slice(&(*start as u32).to_be_bytes());
        table.extend_from_slice(&(*end as u32).to_be_bytes());
    }
    records.push(table);
    set32(&mut header, 192, fdst as u32);
    set32(&mut header, 196, flow_ranges.len() as u32);
    records[0] = header;
    assemble(records)
}

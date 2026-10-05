//! KF8 reflowable document reconstruction, separate from the stable MOBI6 adapter.
//! References: foliate-js/mobi.js (MIT); see licenses/foliate-js-MIT.txt.
mod index;
mod markup;
mod package;
mod resources;
#[cfg(test)]
mod tests;
use crate::{ConvertedBook, MobiBook, MobiError, Progress, Result, Stage, progress, slice, u32be};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

#[derive(Debug)]
pub(crate) struct Header {
    fdst: Option<usize>,
    flows: usize,
    skel: usize,
    frag: Option<usize>,
    ncx: Option<usize>,
    guide: Option<usize>,
}
impl Header {
    pub fn parse(data: &[u8], length: usize, records: usize) -> Result<Self> {
        if length < 248 {
            return Err(MobiError::Invalid("KF8 header is too short"));
        }
        let pointer = |at| -> Result<Option<usize>> {
            let value = u32be(data, at)?;
            if value == u32::MAX {
                Ok(None)
            } else if value == 0 || value as usize >= records {
                Err(MobiError::Invalid("KF8 index pointer outside PalmDB"))
            } else {
                Ok(Some(value as usize))
            }
        };
        Ok(Self {
            fdst: pointer(192)?,
            flows: u32be(data, 196)? as usize,
            skel: pointer(252)?.ok_or(MobiError::Invalid("missing KF8 skeleton index"))?,
            frag: pointer(248)?,
            ncx: pointer(244)?,
            guide: pointer(260)?,
        })
    }
}
struct Section {
    text: String,
    label: String,
}
#[derive(Clone)]
struct Part {
    section: usize,
    source: Range<usize>,
    start: usize,
}
struct Navigation {
    label: String,
    target: (usize, usize),
    depth: usize,
}
#[derive(Clone)]
struct Piece {
    raw: Range<usize>,
    fragment: Option<(usize, usize)>,
}
struct Rebuilt {
    sections: Vec<Section>,
    fragments: BTreeMap<usize, Vec<Part>>,
}

fn flows(header: &Header, book: &MobiBook<'_>, text: &[u8]) -> Result<Vec<Range<usize>>> {
    let Some(pointer) = header.fdst else {
        if header.flows > 1 && header.flows != u32::MAX as usize {
            return Err(MobiError::Invalid("multiple KF8 flows without FDST"));
        }
        return Ok(std::iter::once(0..text.len()).collect());
    };
    let data = book.record(pointer)?;
    if slice(data, 0, 4)? != b"FDST" || u32be(data, 4)? != 12 {
        return Err(MobiError::Invalid("KF8 FDST header"));
    }
    let count = u32be(data, 8)? as usize;
    if count == 0 || count > 4096 {
        return Err(MobiError::Limit("KF8 flow count"));
    }
    if header.flows != count {
        return Err(MobiError::Invalid("KF8 FDST/header count mismatch"));
    }
    slice(data, 12, count * 8)?;
    let mut result = Vec::with_capacity(count);
    let mut previous = 0;
    for i in 0..count {
        let start = u32be(data, 12 + i * 8)? as usize;
        let end = u32be(data, 16 + i * 8)? as usize;
        if start != previous || end < start || end > text.len() {
            return Err(MobiError::Invalid("KF8 flow offsets"));
        }
        result.push(start..end);
        previous = end;
    }
    if previous != text.len() {
        return Err(MobiError::Invalid("KF8 flows do not cover text records"));
    }
    Ok(result)
}

fn reconstruct(
    book: &MobiBook<'_>,
    header: &Header,
    text: &[u8],
    observer: &mut dyn FnMut(Progress) -> bool,
) -> Result<Rebuilt> {
    let skels = index::read(book, header.skel, observer)?;
    if skels.entries.is_empty() || skels.entries.len() > book.limits.max_sections {
        return Err(MobiError::Limit("KF8 skeleton count"));
    }
    let fragments = if let Some(pointer) = header.frag {
        index::read(book, pointer, observer)?.entries
    } else {
        Vec::new()
    };
    let mut frag_at = 0_usize;
    let mut total = 0_usize;
    let mut work = 0_usize;
    let mut seen_ids = BTreeSet::new();
    let mut result = Rebuilt {
        sections: Vec::new(),
        fragments: BTreeMap::new(),
    };
    let mut previous_end = 0;
    for (section, skel) in skels.entries.iter().enumerate() {
        progress(observer, Stage::Markup, section, skels.entries.len())?;
        let count = skel.value(1, 0)?;
        let start = skel.value(6, 0)?;
        let length = skel.value(6, 1)?;
        let rows = fragments
            .get(
                frag_at
                    ..frag_at
                        .checked_add(count)
                        .ok_or(MobiError::Limit("KF8 fragment count"))?,
            )
            .ok_or(MobiError::Invalid("KF8 skeleton fragment count"))?;
        if start < previous_end {
            return Err(MobiError::Invalid("overlapping KF8 skeletons"));
        }
        slice(text, start, length)?;
        let mut pieces = vec![Piece {
            raw: start..start + length,
            fragment: None,
        }];
        let mut assembled = length;
        let mut end = start + length;
        for row in rows {
            if work.is_multiple_of(256) {
                progress(observer, Stage::Markup, section, skels.entries.len())?;
            }
            let insertion = row
                .name
                .parse::<usize>()
                .ok()
                .and_then(|n| n.checked_sub(start))
                .ok_or(MobiError::Invalid("KF8 fragment insertion position"))?;
            let id = row.value(4, 0)?;
            let source_offset = row.value(6, 0)?;
            let size = row.value(6, 1)?;
            if !seen_ids.insert(id) {
                return Err(MobiError::Invalid("duplicate KF8 fragment id"));
            }
            if row.optional(3).is_some_and(|file| file != section) {
                return Err(MobiError::Invalid(
                    "KF8 fragment belongs to a different skeleton",
                ));
            }
            let source = start
                .checked_add(length)
                .and_then(|n| n.checked_add(source_offset))
                .ok_or(MobiError::Invalid("KF8 fragment offset overflow"))?;
            slice(text, source, size)?;
            end = end.max(source + size);
            if insertion > assembled {
                return Err(MobiError::Invalid(
                    "KF8 fragment insertion outside skeleton",
                ));
            }
            assembled = assembled
                .checked_add(size)
                .filter(|n| *n <= 4 * 1024 * 1024)
                .ok_or(MobiError::Limit("KF8 reconstructed chapter bytes"))?;
            // Keep slices rather than copying the whole document after each insertion.
            // Fragment pieces retain their original offsets even for nested insertions.
            work = work.saturating_add(pieces.len());
            if work > 8_000_000 {
                return Err(MobiError::Limit("KF8 reconstruction work"));
            }
            let mut pos = 0;
            let mut inserted = false;
            for i in 0..pieces.len() {
                let next = pos + pieces[i].raw.len();
                if insertion <= next {
                    let split = insertion - pos;
                    let old = pieces[i].clone();
                    let mut replacement = Vec::with_capacity(3);
                    if split > 0 {
                        replacement.push(Piece {
                            raw: old.raw.start..old.raw.start + split,
                            fragment: old.fragment,
                        });
                    }
                    replacement.push(Piece {
                        raw: source..source + size,
                        fragment: Some((id, 0)),
                    });
                    if split < old.raw.len() {
                        replacement.push(Piece {
                            raw: old.raw.start + split..old.raw.end,
                            fragment: old.fragment.map(|(id, at)| (id, at + split)),
                        });
                    }
                    pieces.splice(i..=i, replacement);
                    inserted = true;
                    break;
                }
                pos = next;
            }
            if !inserted {
                return Err(MobiError::Invalid("KF8 fragment reconstruction failed"));
            }
        }
        total = total
            .checked_add(assembled)
            .filter(|n| *n <= book.limits.max_text_bytes)
            .ok_or(MobiError::Limit("KF8 total reconstructed bytes"))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(assembled)
            .map_err(|_| MobiError::Limit("KF8 chapter allocation"))?;
        for piece in pieces {
            if let Some((id, offset)) = piece.fragment {
                result.fragments.entry(id).or_default().push(Part {
                    section,
                    source: offset..offset + piece.raw.len(),
                    start: bytes.len(),
                });
            }
            bytes.extend_from_slice(&text[piece.raw]);
        }
        // UTF-8 may be split across text records, but not decoded until reconstruction.
        let decoded = crate::decode_text(&bytes, book.metadata.encoding)?.into_owned();
        // Positions above are in source bytes; convert for the less common CP1252 KF8.
        if book.metadata.encoding == 1252 {
            for parts in result.fragments.values_mut() {
                for part in parts.iter_mut().filter(|p| p.section == section) {
                    part.start = crate::decode_text(&bytes[..part.start], 1252)?.len();
                }
            }
        }
        result.sections.push(Section {
            text: decoded,
            label: format!("Chapter {}", section + 1),
        });
        previous_end = end;
        frag_at += count;
    }
    if frag_at != fragments.len() {
        return Err(MobiError::Invalid("unclaimed KF8 fragment rows"));
    }
    Ok(result)
}
impl Rebuilt {
    fn target(&self, fid: usize, offset: usize, encoding: u32) -> Option<(usize, usize)> {
        let parts = self.fragments.get(&fid)?;
        let part = parts
            .iter()
            .find(|p| p.source.contains(&offset))
            .or_else(|| parts.last().filter(|p| p.source.end == offset))?;
        let delta = offset.checked_sub(part.source.start)?;
        let section = &self.sections[part.section].text;
        let at = if encoding == 1252 {
            let tail = section.get(part.start..)?;
            tail.char_indices()
                .nth(delta)
                .map_or(section.len(), |(at, _)| part.start + at)
        } else {
            part.start.checked_add(delta)?
        };
        (at <= section.len()).then_some((part.section, at))
    }
}

fn navigation(
    book: &MobiBook<'_>,
    header: &Header,
    observer: &mut dyn FnMut(Progress) -> bool,
) -> Result<Vec<Navigation>> {
    let Some(pointer) = header.ncx else {
        return Ok(Vec::new());
    };
    let data = index::read(book, pointer, observer)?;
    let mut items = Vec::new();
    for (i, row) in data.entries.iter().enumerate() {
        let Some(values) = row.tags.get(&6).filter(|v| v.len() >= 2) else {
            continue;
        };
        let mut depth = 0;
        let mut parent = row.optional(21).filter(|n| *n != u32::MAX as usize);
        while let Some(p) = parent {
            if p >= i || depth >= 32 {
                return Err(MobiError::Invalid("KF8 navigation parent cycle/order"));
            }
            depth += 1;
            parent = data.entries[p]
                .optional(21)
                .filter(|n| *n != u32::MAX as usize);
        }
        let label = row
            .optional(3)
            .and_then(|n| data.strings.get(&n))
            .cloned()
            .unwrap_or_else(|| format!("Chapter {}", i + 1));
        if items.len() >= 4096 {
            return Err(MobiError::Limit("KF8 navigation entries"));
        }
        items.push(Navigation {
            label,
            target: (values[0] as usize, values[1] as usize),
            depth,
        });
    }
    Ok(items)
}

pub(crate) fn convert(
    book: &MobiBook<'_>,
    header: &Header,
    text: &[u8],
    observer: &mut dyn FnMut(Progress) -> bool,
) -> Result<ConvertedBook> {
    let flows = flows(header, book, text)?;
    let mut rebuilt = reconstruct(book, header, &text[flows[0].clone()], observer)?;
    let mut warnings = Vec::new();
    let mut navigation = match navigation(book, header, observer) {
        Ok(items) => items,
        Err(MobiError::Cancelled) => return Err(MobiError::Cancelled),
        Err(error) => {
            warnings.push(format!("KF8 navigation ignored: {error}"));
            Vec::new()
        }
    };
    // Some books supply a guide but omit NCX. It still provides useful linked entries.
    if navigation.is_empty()
        && let Some(pointer) = header.guide
    {
        match index::read(book, pointer, observer) {
            Ok(index) => {
                for row in index.entries {
                    if let Some(fid) = row.optional(6).or_else(|| row.optional(3)) {
                        let label = row
                            .optional(1)
                            .and_then(|n| index.strings.get(&n))
                            .cloned()
                            .unwrap_or(row.name);
                        navigation.push(Navigation {
                            label,
                            target: (fid, 0),
                            depth: 0,
                        });
                    }
                }
            }
            Err(MobiError::Cancelled) => return Err(MobiError::Cancelled),
            Err(error) => warnings.push(format!("KF8 guide ignored: {error}")),
        }
    }
    let mut targets: BTreeSet<(usize, usize)> = navigation.iter().map(|n| n.target).collect();
    for section in &rebuilt.sections {
        markup::collect_targets(&section.text, book.limits, &mut targets)?;
    }
    if targets.len() > 100_000 {
        return Err(MobiError::Limit("KF8 link targets"));
    }
    let mut locations: Vec<Vec<((usize, usize), usize)>> =
        (0..rebuilt.sections.len()).map(|_| Vec::new()).collect();
    for &(fid, offset) in &targets {
        if let Some((section, at)) = rebuilt.target(fid, offset, book.metadata.encoding) {
            locations[section].push(((fid, offset), at));
        }
    }
    let mut links = BTreeMap::new();
    let mut insertions = Vec::new();
    for (section, locations) in rebuilt.sections.iter().zip(&locations) {
        let (edits, anchors) = markup::anchors(&section.text, locations, book.limits)?;
        let index = insertions.len();
        for (target, id) in anchors {
            links.insert(
                target,
                format!("section{index}.xhtml#{}", markup::fragment(&id)),
            );
        }
        insertions.push(edits);
    }
    let mut resources = resources::Resources::new(book, text, &flows);
    for (i, (section, edits)) in rebuilt.sections.iter_mut().zip(insertions).enumerate() {
        progress(observer, Stage::Markup, i, locations.len())?;
        let anchored = markup::apply(&section.text, edits)?;
        section.text = markup::rewrite(&anchored, book.limits, &links, &mut resources)?;
        if let Some(title) = markup::title(&section.text, book.limits)? {
            section.label = title;
        }
    }
    resources.prepare(observer)?;
    warnings.append(&mut resources.warnings);
    let epub = package::build(
        book,
        &rebuilt.sections,
        &navigation,
        &links,
        &resources,
        observer,
    )?;
    Ok(ConvertedBook {
        epub,
        metadata: book.metadata.clone(),
        warnings,
    })
}

//! Horizontal text shaping and bidi reordering. Canonical byte ranges survive
//! ligatures, mark positioning, font fallback and visual-order changes.
use super::*;
#[cfg(test)]
mod tests;
use rustybuzz::{BufferFlags, Direction, Face, UnicodeBuffer};
use std::collections::{BTreeMap, BTreeSet};
use unicode_bidi::BidiInfo;
use unicode_script::{Script, UnicodeScript};

#[derive(Debug, Clone)]
pub(super) struct PositionedGlyph {
    pub source: Range<usize>,
    pub face: usize,
    pub size: u32,
    pub glyph: Option<u16>,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
    pub ascent: f32,
    pub height: f32,
    pub style: TextStyle,
    pub bold: u32,
    pub italic: bool,
}
#[derive(Default)]
pub(super) struct ShapedLine {
    pub glyphs: Vec<PositionedGlyph>,
    pub width: f32,
    pub ascent: f32,
    pub height: f32,
}
#[derive(Debug)]
pub(super) struct Unit {
    pub range: Range<usize>,
    pub advance: f32,
    pub opportunity: bool,
}
#[derive(Clone)]
struct Segment {
    range: Range<usize>,
    face: usize,
    size: u32,
    style: TextStyle,
    script: Script,
    tab: bool,
}

pub(super) fn ignorable(ch: char) -> bool {
    matches!(ch,'\0'|'\u{00ad}'|'\u{034f}'|'\u{061c}'|'\u{180b}'..='\u{180f}'|'\u{200b}'..='\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2060}'..='\u{206f}'|'\u{fe00}'..='\u{fe0f}'|'\u{feff}'|'\u{e0000}'..='\u{e0fff}')
}
/// Hidden content becomes same-byte-width bidi BN characters; original UTF-8
/// offsets remain indexable, while hidden strong letters cannot change direction.
pub(super) fn bidi_text(chapter: &Chapter<'_, '_>, range: Range<usize>) -> String {
    chapter.text()[range.clone()]
        .char_indices()
        .map(|(offset, ch)| {
            if chapter.style(range.start + offset).hidden {
                match ch.len_utf8() {
                    1 => '\0',
                    2 => '\u{ad}',
                    3 => '\u{feff}',
                    _ => '\u{e0001}',
                }
            } else {
                ch
            }
        })
        .collect()
}
pub(super) struct Paragraph<'a> {
    text: &'a str,
    range: Range<usize>,
    bidi: BidiInfo<'a>,
    breaks: BTreeSet<usize>,
}
impl<'a> Paragraph<'a> {
    pub fn new(chapter: &Chapter<'_, '_>, range: Range<usize>, masked: &'a str) -> Self {
        let mut visible = String::new();
        let mut mapping = Vec::new();
        for (offset, ch) in chapter.text()[range.clone()].char_indices() {
            if !chapter.style(range.start + offset).hidden {
                visible.push(ch);
                mapping.push((visible.len(), range.start + offset + ch.len_utf8()));
            }
        }
        let mut breaks = BTreeSet::new();
        for (cluster, allowed) in readall_core::layout::clusters(&visible) {
            if allowed {
                let at = mapping.partition_point(|(offset, _)| *offset < cluster.end);
                if let Some((_, canonical)) = mapping.get(at) {
                    breaks.insert(*canonical);
                }
            }
        }
        Self {
            text: masked,
            range,
            bidi: BidiInfo::new(masked, None),
            breaks,
        }
    }
    pub fn units(
        &self,
        chapter: &Chapter<'_, '_>,
        fonts: &mut Fonts<'_, '_>,
        work: &mut usize,
    ) -> Result<Vec<Unit>> {
        let shaped = self.line(chapter, self.range.clone(), fonts, work)?;
        let mut groups: BTreeMap<(usize, usize), f32> = BTreeMap::new();
        for glyph in shaped.glyphs {
            let entry = groups
                .entry((glyph.source.start, glyph.source.end))
                .or_default();
            *entry += glyph.advance;
        }
        let mut units = Vec::new();
        let mut at = self.range.start;
        for ((start, end), advance) in groups {
            if start < at {
                return Err("overlapping shaped source clusters".into());
            }
            if at < start {
                units.push(Unit {
                    range: at..start,
                    advance: 0.0,
                    opportunity: self.breaks.contains(&start),
                });
            }
            units.push(Unit {
                range: start..end,
                advance,
                opportunity: self.breaks.contains(&end),
            });
            at = end;
        }
        if at < self.range.end {
            units.push(Unit {
                range: at..self.range.end,
                advance: 0.0,
                opportunity: true,
            });
        }
        Ok(units)
    }
    pub fn line(
        &self,
        chapter: &Chapter<'_, '_>,
        range: Range<usize>,
        fonts: &mut Fonts<'_, '_>,
        work: &mut usize,
    ) -> Result<ShapedLine> {
        let mut output = ShapedLine::default();
        if range.is_empty() {
            return Ok(output);
        }
        let local = range.start - self.range.start..range.end - self.range.start;
        for paragraph in &self.bidi.paragraphs {
            let subrange =
                local.start.max(paragraph.range.start)..local.end.min(paragraph.range.end);
            if subrange.is_empty() {
                continue;
            }
            let (levels, runs) = self.bidi.visual_runs(paragraph, subrange);
            for run in runs {
                let rtl = levels[run.start].is_rtl();
                let mut segments: Vec<Segment> = Vec::new();
                let source =
                    &chapter.text()[self.range.start + run.start..self.range.start + run.end];
                let mut inherited_script = Script::Common;
                for (cluster, _) in readall_core::layout::clusters(source) {
                    let canonical = self.range.start + run.start + cluster.start
                        ..self.range.start + run.start + cluster.end;
                    let raw = &chapter.text()[canonical.clone()];
                    let first = raw
                        .char_indices()
                        .find(|(offset, _)| !chapter.style(canonical.start + offset).hidden);
                    let Some((visible_offset, _)) = first else {
                        continue;
                    };
                    let style = chapter.style(canonical.start + visible_offset);
                    let size = fonts.size(style);
                    let visible: String = raw
                        .char_indices()
                        .filter_map(|(offset, ch)| {
                            (!chapter.style(canonical.start + offset).hidden).then_some(ch)
                        })
                        .collect();
                    let face = fonts.choose_cluster(&visible, style);
                    let script = visible
                        .chars()
                        .map(|ch| ch.script())
                        .find(|script| {
                            !matches!(script, Script::Common | Script::Inherited | Script::Unknown)
                        })
                        .unwrap_or(inherited_script);
                    if !matches!(script, Script::Common | Script::Inherited | Script::Unknown) {
                        inherited_script = script;
                    }
                    let tab = visible == "\t";
                    if let Some(last) = segments.last_mut().filter(|last| {
                        !last.tab
                            && !tab
                            && last.face == face
                            && last.size == size
                            && last.style == style
                            && (last.script == script
                                || matches!(last.script, Script::Common | Script::Inherited))
                            && canonical.end - last.range.start <= 16 * 1024
                    }) {
                        last.range.end = canonical.end;
                        if matches!(last.script, Script::Common | Script::Inherited) {
                            last.script = script;
                        }
                    } else {
                        segments.push(Segment {
                            range: canonical,
                            face,
                            size,
                            style,
                            script,
                            tab,
                        });
                    }
                }
                if rtl {
                    segments.reverse();
                }
                for segment in segments {
                    self.segment(chapter, (&range, rtl), &segment, fonts, work, &mut output)?;
                }
            }
        }
        Ok(output)
    }
    fn segment(
        &self,
        chapter: &Chapter<'_, '_>,
        directional_line: (&Range<usize>, bool),
        segment: &Segment,
        fonts: &mut Fonts<'_, '_>,
        work: &mut usize,
        output: &mut ShapedLine,
    ) -> Result<()> {
        let (line, rtl) = directional_line;
        let font = fonts.face(segment.face).clone();
        let (face_bold, face_italic) = fonts.face_style(segment.face);
        let metrics = font.metrics();
        let scale = segment.size as f32 / f32::from(metrics.units_per_em);
        let natural = (f32::from(metrics.ascender) - f32::from(metrics.descender)
            + f32::from(metrics.line_gap.max(0)))
            * scale;
        let height = (natural.max(segment.size as f32 * segment.style.line_height)
            * fonts.line_spacing)
            .max(natural)
            .ceil();
        let ascent = f32::from(metrics.ascender) * scale + (height - natural) / 2.0;
        output.height = output.height.max(height);
        output.ascent = output.ascent.max(ascent);
        let bold = if segment.style.bold && !face_bold {
            (segment.size as f32 / 24.0).round().clamp(1.0, 8.0) as u32
        } else {
            0
        };
        let italic = segment.style.italic && !face_italic;
        if segment.tab {
            let advance = fonts.advance('\t', segment.size, output.width, segment.style)?;
            output.glyphs.push(PositionedGlyph {
                source: segment.range.clone(),
                face: segment.face,
                size: segment.size,
                glyph: None,
                x: output.width,
                y: 0.0,
                advance,
                ascent,
                height,
                style: segment.style,
                bold: 0,
                italic: false,
            });
            output.width += advance;
            return Ok(());
        }
        let face = Face::from_slice(font.data(), font.face_index())
            .ok_or("font cannot be opened by the shaping engine")?;
        let mut buffer = UnicodeBuffer::new();
        for (offset, ch) in chapter.text()[segment.range.clone()].char_indices() {
            *work = work.saturating_add(1);
            if (*work).is_multiple_of(256) {
                crate::loading::check()?;
            }
            if *work > 6_000_000 {
                return Err("EPUB shaping work budget exceeded".into());
            }
            if !chapter.style(segment.range.start + offset).hidden {
                buffer.add(ch, (segment.range.start + offset) as u32);
            }
        }
        buffer.set_direction(if rtl {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        });
        let tag = rustybuzz::ttf_parser::Tag::from_bytes(
            segment
                .script
                .short_name()
                .as_bytes()
                .try_into()
                .map_err(|_| "invalid Unicode script tag")?,
        );
        if let Some(script) = rustybuzz::Script::from_iso15924_tag(tag) {
            buffer.set_script(script);
        }
        buffer.guess_segment_properties();
        let mut flags = BufferFlags::REMOVE_DEFAULT_IGNORABLES;
        if segment.range.start == line.start {
            flags |= BufferFlags::BEGINNING_OF_TEXT;
        }
        if segment.range.end == line.end {
            flags |= BufferFlags::END_OF_TEXT;
        }
        buffer.set_flags(flags);
        buffer.set_pre_context(
            &self.text[line.start - self.range.start..segment.range.start - self.range.start],
        );
        buffer.set_post_context(
            &self.text[segment.range.end - self.range.start..line.end - self.range.start],
        );
        crate::loading::check()?;
        let glyphs = rustybuzz::shape(&face, &[], buffer);
        if output.glyphs.len().saturating_add(glyphs.len()) > 1_000_000 {
            return Err("EPUB shaped glyph count budget exceeded".into());
        }
        let mut boundaries: Vec<_> = glyphs
            .glyph_infos()
            .iter()
            .map(|info| info.cluster as usize)
            .collect();
        boundaries.push(segment.range.end);
        boundaries.sort_unstable();
        boundaries.dedup();
        for (info, position) in glyphs.glyph_infos().iter().zip(glyphs.glyph_positions()) {
            let start = info.cluster as usize;
            if start < segment.range.start
                || start >= segment.range.end
                || !chapter.text().is_char_boundary(start)
            {
                return Err("invalid source cluster from shaping engine".into());
            }
            let next = boundaries.partition_point(|offset| *offset <= start);
            let end = *boundaries.get(next).ok_or("missing shaped cluster end")?;
            if info.glyph_id == 0
                && let Some(ch) = chapter.text()[start..end]
                    .chars()
                    .find(|ch| !ignorable(*ch))
            {
                fonts
                    .cache_index(segment.face, segment.size)
                    .record_missing(ch)?;
            }
            let advance = position.x_advance as f32 * scale + bold as f32;
            if !advance.is_finite() || !(0.0..=262144.0).contains(&advance) {
                return Err("unsupported shaped glyph advance".into());
            }
            output.glyphs.push(PositionedGlyph {
                source: start..end,
                face: segment.face,
                size: segment.size,
                glyph: Some(info.glyph_id as u16),
                x: output.width + position.x_offset as f32 * scale,
                y: -(position.y_offset as f32) * scale,
                advance,
                ascent,
                height,
                style: segment.style,
                bold,
                italic,
            });
            output.width += advance;
        }
        Ok(())
    }
}

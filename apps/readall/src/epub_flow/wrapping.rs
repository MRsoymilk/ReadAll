//! Word/CJK break opportunities with grapheme-safe emergency wrapping.
use super::shaping::{Paragraph, bidi_text};
use super::*;

fn span(
    chapter: &Chapter<'_, '_>,
    range: Range<usize>,
    options: &Options,
    fonts: &mut Fonts<'_, '_>,
    builder: &mut Builder,
    paragraph: &mut bool,
    work: &mut usize,
) -> Result<()> {
    if range.is_empty() {
        return Ok(());
    }
    crate::loading::step("章节文字排版", range.start, chapter.text().len())?;
    let masked = bidi_text(chapter, range.clone());
    let prepared = Paragraph::new(chapter, range.clone(), &masked);
    let units = prepared.units(chapter, fonts, work)?;
    let mut start = 0;
    while start < units.len() {
        let first = units[start].range.start;
        crate::loading::step("章节文字排版", first, chapter.text().len())?;
        let style = chapter.text()[first..range.end]
            .char_indices()
            .find_map(|(offset, _)| {
                let style = chapter.style(first + offset);
                (!style.hidden).then_some(style)
            })
            .unwrap_or_default();
        let indent = if *paragraph {
            (style.indent * options.size as f32).clamp(0.0, builder.width / 2.0)
        } else {
            0.0
        };
        let available = builder.width - indent;
        let (mut end, mut opportunity, mut width) = (start, None, 0.0_f32);
        while end < units.len() {
            if end > start && width + units[end].advance > available {
                if let Some(at) = opportunity {
                    end = at;
                }
                break;
            }
            width += units[end].advance;
            end += 1;
            if units[end - 1].opportunity {
                opportunity = Some(end);
            }
        }
        let shaped = loop {
            let shaped = prepared.line(chapter, first..units[end - 1].range.end, fonts, work)?;
            if shaped.width <= available + 0.01 || end == start + 1 {
                break shaped;
            }
            let previous = (start..end - 1)
                .rev()
                .find(|index| units[*index].opportunity)
                .map(|index| index + 1)
                .unwrap_or(end - 1);
            end = previous;
        };
        if shaped.width > available + 0.01 {
            return Err("shaped cluster is wider than the available content width".into());
        }
        let mut line = PendingLine {
            start: (!shaped.glyphs.is_empty()).then_some(first),
            end: units[end - 1].range.end,
            width: shaped.width,
            indent,
            ascent: shaped.ascent,
            height: shaped.height,
            align: style.align,
            glyphs: shaped.glyphs,
        };
        builder.line(&mut line)?;
        *paragraph = false;
        start = end;
    }
    Ok(())
}

fn blank_source_line(
    builder: &mut Builder,
    fonts: &mut Fonts<'_, '_>,
    style: TextStyle,
    offset: usize,
    bytes: usize,
) -> Result<()> {
    let size = fonts.size(style);
    let face = fonts.choose_cluster(" ", style);
    let metrics = fonts.face(face).metrics();
    let scale = size as f32 / f32::from(metrics.units_per_em);
    let natural = (f32::from(metrics.ascender) - f32::from(metrics.descender)
        + f32::from(metrics.line_gap.max(0)))
        * scale;
    let height = (natural.max(size as f32 * style.line_height) * fonts.line_spacing)
        .max(natural)
        .ceil();
    builder.line(&mut PendingLine {
        start: Some(offset),
        end: offset + bytes,
        height,
        ascent: f32::from(metrics.ascender) * scale + (height - natural) / 2.0,
        ..PendingLine::default()
    })
}

pub(super) fn build(
    chapter: &Chapter<'_, '_>,
    options: &Options,
    fonts: &mut Fonts<'_, '_>,
    key: Key,
) -> Result<Layout> {
    crate::loading::step("章节文字排版", 0, chapter.text().len())?;
    if chapter.text().chars().count() > 2_000_000 {
        return Err("EPUB layout scalar budget exceeded".into());
    }
    let content = options.content_rect();
    let mut builder = Builder::new(content.width, content.height);
    let (mut start, mut image, mut block, mut work) = (0, 0, 0, 0);
    let (mut paragraph, mut previous_newline) = (true, false);
    for (offset, ch) in chapter
        .text()
        .char_indices()
        .chain(std::iter::once((chapter.text().len(), '\0')))
    {
        if chapter
            .content
            .images
            .get(image)
            .is_some_and(|item| item.offset <= offset)
            || chapter
                .content
                .blocks
                .get(block)
                .is_some_and(|item| item.offset <= offset)
        {
            span(
                chapter,
                start..offset,
                options,
                fonts,
                &mut builder,
                &mut paragraph,
                &mut work,
            )?;
            loop {
                crate::loading::check()?;
                let boundary = chapter
                    .content
                    .blocks
                    .get(block)
                    .filter(|item| item.offset <= offset);
                let picture = chapter
                    .content
                    .images
                    .get(image)
                    .filter(|item| item.offset <= offset);
                let take_block = boundary.is_some_and(|b| {
                    picture.is_none_or(|p| {
                        b.offset < p.offset || (b.offset == p.offset && b.images_before <= image)
                    })
                });
                if take_block {
                    let boundary = boundary.ok_or("missing block boundary")?;
                    if let Some(style) = boundary.style {
                        builder.open_box(style, boundary.offset, options.size as f32)?;
                    } else {
                        builder.close_box()?;
                    }
                    block += 1;
                    paragraph = true;
                } else if picture.is_some() {
                    builder.image(chapter, image)?;
                    image += 1;
                } else {
                    break;
                }
            }
            start = offset;
        }
        if offset == chapter.text().len() {
            span(
                chapter,
                start..offset,
                options,
                fonts,
                &mut builder,
                &mut paragraph,
                &mut work,
            )?;
            break;
        }
        let break_style = chapter.style(offset);
        if matches!(ch, '\n' | '\u{2028}' | '\u{2029}') && !break_style.hidden {
            if start < offset {
                span(
                    chapter,
                    start..offset,
                    options,
                    fonts,
                    &mut builder,
                    &mut paragraph,
                    &mut work,
                )?;
            } else if break_style.white_space.preserves_breaks() {
                blank_source_line(&mut builder, fonts, break_style, offset, ch.len_utf8())?;
            } else {
                builder.gap(options.size as f32 * 0.4);
            }
            paragraph = previous_newline && !break_style.white_space.preserves_breaks();
            previous_newline = true;
            start = offset + ch.len_utf8();
        } else {
            previous_newline = false;
        }
    }
    if !builder.boxes.is_empty() {
        return Err("unclosed EPUB block boxes".into());
    }
    crate::loading::step("章节文字排版", chapter.text().len(), chapter.text().len())?;
    Ok(Layout {
        key,
        pages: builder.pages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_epub, test_font};
    use readall_epub::EpubLimits;
    use readall_font::FontLimits;
    #[test]
    fn english_wrap_keeps_complete_words_and_all_bytes() {
        let data = test_epub::make_epub_with_resources(
            &["<html><body>A WWWW AAAA WWWW</body></html>"],
            vec![],
        );
        let book = EpubBook::parse(&data, EpubLimits::default()).unwrap();
        let chapter = Chapter::load(&book, 0).unwrap();
        let bytes = test_font::make_font();
        let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
        let opts = Options::parse(
            &[
                "--font",
                "fixture.ttf",
                "--width",
                "128",
                "--height",
                "128",
                "--margin",
                "16",
                "--font-size",
                "16",
            ]
            .map(Into::into),
        )
        .unwrap();
        let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
        renderer.render(&chapter, &opts).unwrap();
        let lines: Vec<_> = renderer
            .layout
            .as_ref()
            .unwrap()
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .filter_map(|item| {
                if let Item::Text(line) = item {
                    Some(&chapter.text()[line.range.clone()])
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(lines.concat(), chapter.text());
        assert!(lines.len() > 1);
        for line in &lines[..lines.len() - 1] {
            assert!(line.ends_with(' '), "{lines:?}");
        }
    }
}

//! Conservative MOBI HTML -> well-formed XHTML. No scripts, remote resources,
//! filesystem reads or code execution. Byte filepos and record indices are rewritten
//! before UTF-8/entity conversion; original source bytes remain untouched.
#[path = "html_tokens.rs"]
pub(crate) mod tokenizer;
use crate::{MobiError, MobiLimits, Progress, Result, Stage, decode_text, progress};
use std::collections::{BTreeMap, BTreeSet};
use tokenizer::{Kind, Tag};

pub(crate) struct Section {
    pub body: String,
    pub label: String,
}
pub(crate) struct Content {
    pub sections: Vec<Section>,
    pub css: String,
    pub images: BTreeSet<usize>,
    pub navigation: Vec<(String, String)>,
    pub warnings: Vec<String>,
}
struct Open {
    name: String,
    output: String,
    reopen: String,
}
fn section(cuts: &[usize], at: usize) -> usize {
    cuts.partition_point(|n| *n <= at).saturating_sub(1)
}
fn number(s: &str) -> Option<usize> {
    let s = s.trim().trim_end_matches('/');
    (!s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}
pub(crate) fn escape(text: &str) -> String {
    let mut result = String::new();
    for ch in text.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&apos;"),
            '\t' | '\n' | '\r' => result.push(ch),
            _ if ch.is_control() || matches!(ch, '\u{fffe}' | '\u{ffff}') => {}
            _ => result.push(ch),
        }
    }
    result
}
fn decoded(bytes: &[u8], encoding: u32) -> Result<String> {
    Ok(html_escape::decode_html_entities(&decode_text(bytes, encoding)?).into_owned())
}
fn close(stack: &mut Vec<Open>, body: &mut String, from: usize) {
    while stack.len() > from {
        let item = stack.pop().unwrap();
        body.push_str(&format!("</{}>", item.output));
    }
}
fn block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "section"
            | "article"
            | "blockquote"
            | "pre"
            | "ul"
            | "ol"
            | "li"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "table"
            | "tr"
            | "td"
            | "th"
            | "hr"
    )
}
fn allowed(name: &str) -> bool {
    block(name)
        || matches!(
            name,
            "span"
                | "a"
                | "b"
                | "strong"
                | "i"
                | "em"
                | "u"
                | "s"
                | "del"
                | "sub"
                | "sup"
                | "small"
                | "code"
                | "br"
                | "img"
                | "dl"
                | "dt"
                | "dd"
                | "figure"
                | "figcaption"
                | "thead"
                | "tbody"
                | "tfoot"
                | "caption"
        )
}
fn label(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(96)
        .collect()
}
fn pos_href(cuts: &[usize], target: usize, positions: &BTreeMap<usize, usize>) -> Option<String> {
    let effective = positions.get(&target)?;
    Some(format!(
        "section{}.xhtml#mobi-pos-{target}",
        section(cuts, *effective)
    ))
}
fn href(
    tag: &Tag,
    cuts: &[usize],
    positions: &BTreeMap<usize, usize>,
    ids: &BTreeMap<String, (usize, String)>,
) -> Option<String> {
    if let Some(target) = tag.attr("filepos").and_then(number) {
        return Some(
            pos_href(cuts, target, positions)
                .unwrap_or_else(|| format!("#mobi-invalid-pos-{target}")),
        );
    }
    let value = tag.attr("href")?.trim();
    if value.to_ascii_lowercase().starts_with("filepos:") {
        return number(&value[8..]).and_then(|n| pos_href(cuts, n, positions));
    }
    if let Some(fragment) = value.strip_prefix('#')
        && let Some((section, id)) = ids.get(fragment)
    {
        return Some(format!("section{section}.xhtml#{id}"));
    }
    Some(value.to_owned()) // Existing reader policy validates/asks before any external link.
}

pub(crate) fn normalize(
    data: &[u8],
    encoding: u32,
    limits: MobiLimits,
    observer: &mut dyn FnMut(Progress) -> bool,
) -> Result<Content> {
    progress(observer, Stage::Markup, 0, data.len())?;
    let tokens = tokenizer::tokens(data, encoding, limits.max_tokens)?;
    let mut cuts = vec![0];
    let mut head = false;
    let mut raw = false;
    for token in &tokens {
        if let Kind::Tag(tag) = &token.kind {
            if tag.name == "head" {
                head = !tag.closing;
            }
            if matches!(tag.name.as_str(), "style" | "script" | "title" | "textarea") {
                raw = !tag.closing && !tag.empty;
            }
            if !head
                && !raw
                && !tag.closing
                && matches!(tag.name.as_str(), "mbp:pagebreak" | "pagebreak")
                && token.range.end < data.len()
            {
                cuts.push(token.range.end);
            }
        }
        if !head && !raw && token.range.start.saturating_sub(*cuts.last().unwrap()) >= 256 * 1024 {
            cuts.push(token.range.start);
        }
        if cuts.len() > limits.max_sections {
            return Err(MobiError::Limit("section count"));
        }
    }
    cuts.dedup();
    let mut ids = BTreeMap::new();
    let mut positions = BTreeMap::new();
    let mut toc_target = None;
    for token in &tokens {
        if let Kind::Tag(tag) = &token.kind
            && !tag.closing
        {
            let n = section(&cuts, token.range.start);
            for value in [
                tag.attr("id"),
                tag.attr("xml:id"),
                (tag.name == "a").then(|| tag.attr("name")).flatten(),
            ]
            .into_iter()
            .flatten()
            {
                ids.entry(value.to_owned())
                    .or_insert_with(|| (n, format!("mobi-node-{}", token.range.start)));
            }
            if let Some(pos) = tag
                .attr("filepos")
                .and_then(number)
                .filter(|p| *p <= data.len())
            {
                positions.insert(pos, pos);
                if tag.name == "reference"
                    && tag
                        .attr("type")
                        .is_some_and(|s| s.eq_ignore_ascii_case("toc"))
                {
                    toc_target = Some(pos);
                }
            }
        }
    }
    if positions.len() + ids.len() > 100_000 {
        return Err(MobiError::Limit("HTML link anchors"));
    }
    // Bad producers can point into a tag, entity or UTF-8 continuation byte. Keep
    // the original target identity, but move the inserted marker to a safe boundary.
    for (target, effective) in &mut positions {
        let i = tokens.partition_point(|t| t.range.end <= *target);
        if let Some(token) = tokens.get(i) {
            match token.kind {
                Kind::Tag(_) => *effective = token.range.start,
                Kind::Skip => *effective = token.range.end,
                Kind::Text => {
                    if encoding == 65001 {
                        while *effective > token.range.start
                            && data.get(*effective).is_some_and(|b| b & 0xc0 == 0x80)
                        {
                            *effective -= 1;
                        }
                    }
                    for p in (token.range.start..*effective).rev().take(64) {
                        if data[p] == b'&' {
                            *effective = p;
                            break;
                        }
                        if data[p].is_ascii_whitespace() || data[p] == b';' {
                            break;
                        }
                    }
                }
            }
        }
    }
    let mut markers: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (&original, &effective) in &positions {
        markers.entry(effective).or_default().push(original);
    }
    let toc_section = toc_target
        .and_then(|n| positions.get(&n))
        .map(|n| section(&cuts, *n));
    let mut result = Content {
        sections: cuts
            .iter()
            .map(|_| Section {
                body: String::new(),
                label: String::new(),
            })
            .collect(),
        css: String::new(),
        images: BTreeSet::new(),
        navigation: Vec::new(),
        warnings: Vec::new(),
    };
    let mut stack: Vec<Open> = Vec::new();
    let mut current = 0;
    let mut head = false;
    let mut suppressed: Option<String> = None;
    let mut heading: Option<(String, String)> = None;
    let mut toc_link: Option<(String, String)> = None;
    let mut headings = Vec::new();
    let mut toc = Vec::new();
    let mut total = 0_usize;
    for (ordinal, token) in tokens.iter().enumerate() {
        if ordinal % 256 == 0 {
            progress(observer, Stage::Markup, token.range.start, data.len())?;
        }
        let next = section(&cuts, token.range.start);
        if next != current {
            for item in stack.iter().rev() {
                result.sections[current]
                    .body
                    .push_str(&format!("</{}>", item.output));
            }
            current = next;
            for item in &stack {
                result.sections[current].body.push_str(&item.reopen);
            }
        }
        if let Some(name) = &suppressed {
            if let Kind::Tag(tag) = &token.kind
                && tag.closing
                && tag.name == *name
            {
                suppressed = None;
            } else if name == "style" && matches!(token.kind, Kind::Text) {
                if result.css.len().saturating_add(token.range.len()) > 256 * 1024 {
                    return Err(MobiError::Limit("legacy style bytes"));
                }
                result
                    .css
                    .push_str(&decode_text(&data[token.range.clone()], encoding)?);
            }
            continue;
        }
        if let Kind::Tag(tag) = &token.kind {
            if tag.name == "head" {
                head = !tag.closing;
                continue;
            }
            if !tag.closing
                && matches!(
                    tag.name.as_str(),
                    "script"
                        | "style"
                        | "title"
                        | "textarea"
                        | "iframe"
                        | "object"
                        | "embed"
                        | "noscript"
                )
            {
                if !tag.empty && tag.name != "embed" {
                    suppressed = Some(tag.name.clone());
                }
                continue;
            }
        }
        if head {
            continue;
        }
        let body = &mut result.sections[current].body;
        if !matches!(token.kind, Kind::Text)
            && let Some(targets) = markers.get(&token.range.start)
        {
            for target in targets {
                body.push_str(&format!("<a id=\"mobi-pos-{target}\"></a>"));
            }
        }
        let before = body.len();
        match &token.kind {
            Kind::Skip => {}
            Kind::Text => {
                let mut at = token.range.start;
                for (&where_, targets) in markers.range(token.range.clone()) {
                    body.push_str(&escape(&decoded(&data[at..where_], encoding)?));
                    for target in targets {
                        body.push_str(&format!("<a id=\"mobi-pos-{target}\"></a>"));
                    }
                    at = where_;
                }
                let plain = decoded(&data[token.range.clone()], encoding)?;
                body.push_str(&escape(&decoded(&data[at..token.range.end], encoding)?));
                if let Some((_, text)) = &mut heading
                    && text.len() < 1024
                {
                    text.extend(plain.chars().take(128));
                }
                if let Some((_, text)) = &mut toc_link
                    && text.len() < 1024
                {
                    text.extend(plain.chars().take(128));
                }
                if result.sections[current].label.len() < 128 {
                    result.sections[current]
                        .label
                        .extend(plain.chars().take(96));
                }
            }
            Kind::Tag(tag) => {
                let name = tag.name.as_str();
                if matches!(
                    name,
                    "html"
                        | "body"
                        | "guide"
                        | "reference"
                        | "meta"
                        | "link"
                        | "mbp:pagebreak"
                        | "pagebreak"
                ) {
                    continue;
                }
                if tag.closing {
                    if matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
                        && let Some((href, text)) = heading.take()
                        && !text.trim().is_empty()
                        && headings.len() < 512
                    {
                        headings.push((label(&text), href));
                    }
                    if name == "a"
                        && let Some((href, text)) = toc_link.take()
                        && !text.trim().is_empty()
                        && toc.len() < 512
                    {
                        toc.push((label(&text), href));
                    }
                    if let Some(index) = stack.iter().rposition(|item| item.name == name) {
                        close(&mut stack, body, index);
                    }
                    continue;
                }
                let output = match name {
                    "font" | "big" => "span",
                    "tt" => "code",
                    "center" => "div",
                    _ => name,
                };
                if !allowed(output) {
                    continue;
                }
                if block(output)
                    && let Some(p) = stack.iter().rposition(|item| item.name == "p")
                {
                    close(&mut stack, body, p);
                }
                if matches!(
                    output,
                    "li" | "dt" | "dd" | "tr" | "td" | "th" | "h1" | "h2" | "h3"
                ) && let Some(i) = stack.iter().rposition(|item| item.name == name)
                {
                    close(&mut stack, body, i);
                }
                if output == "a"
                    && let Some(i) = stack.iter().rposition(|item| item.name == "a")
                {
                    close(&mut stack, body, i);
                }
                let mut attrs = String::new();
                for key in [
                    "class",
                    "lang",
                    "dir",
                    "title",
                    "alt",
                    "width",
                    "height",
                    "hidden",
                    "data-language",
                    "data-lang",
                    "role",
                ] {
                    if let Some(value) = tag.attr(key) {
                        attrs.push_str(&format!(" {key}=\"{}\"", escape(value)));
                    }
                }
                let mut style = tag.attr("style").unwrap_or("").to_owned();
                if let Some(align) = tag
                    .attr("align")
                    .filter(|s| matches!(*s, "left" | "center" | "right"))
                {
                    style.push_str(&format!(";text-align:{align}"));
                }
                if name == "center" {
                    style.push_str(";text-align:center");
                }
                if name == "big" {
                    style.push_str(";font-size:120%");
                }
                if name == "font" {
                    if let Some(color) = tag.attr("color") {
                        style.push_str(&format!(";color:{color}"));
                    }
                    if let Some(face) = tag.attr("face") {
                        style.push_str(&format!(";font-family:{face}"));
                    }
                    if let Some(size) = tag.attr("size").and_then(|s| s.parse::<i32>().ok()) {
                        let scale = match size {
                            i32::MIN..=1 => 75,
                            2 => 85,
                            3 => 100,
                            4 => 120,
                            5 => 150,
                            6 => 175,
                            _ => 200,
                        };
                        style.push_str(&format!(";font-size:{scale}%"));
                    }
                }
                if !style.is_empty() {
                    attrs.push_str(&format!(
                        " style=\"{}\"",
                        escape(style.trim_start_matches(';'))
                    ));
                }
                if output == "a"
                    && let Some(value) = href(tag, &cuts, &positions, &ids)
                {
                    attrs.push_str(&format!(" href=\"{}\"", escape(&value)));
                    if toc_section == Some(current) {
                        toc_link = Some((value, String::new()));
                    }
                }
                if output == "img" {
                    if let Some(index) = tag
                        .attr("recindex")
                        .and_then(number)
                        .and_then(|n| n.checked_sub(1))
                    {
                        result.images.insert(index);
                        attrs.push_str(&format!(" src=\"media/image{index}\""));
                    } else {
                        attrs.push_str(" src=\"media/unavailable\"");
                    }
                }
                let id = format!("mobi-node-{}", token.range.start);
                let has_id = tag.attr("id").is_some()
                    || tag.attr("xml:id").is_some()
                    || (name == "a" && tag.attr("name").is_some());
                let is_heading = matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
                let anchor = if has_id || is_heading {
                    format!(" id=\"{id}\"")
                } else {
                    String::new()
                };
                let void = tag.empty || matches!(output, "img" | "br" | "hr");
                body.push_str(&format!(
                    "<{output}{attrs}{anchor}{}>",
                    if void { "/" } else { "" }
                ));
                if is_heading {
                    heading = Some((format!("section{current}.xhtml#{id}"), String::new()));
                }
                if !void {
                    if stack.len() >= 96 {
                        return Err(MobiError::Limit("HTML nesting"));
                    }
                    stack.push(Open {
                        name: name.to_owned(),
                        output: output.to_owned(),
                        reopen: format!("<{output}{attrs}>"),
                    });
                }
            }
        }
        total = total.saturating_add(result.sections[current].body.len().saturating_sub(before));
        if result.sections[current].body.len() > 2 * 1024 * 1024
            || total > limits.max_text_bytes.saturating_mul(4)
        {
            return Err(MobiError::Limit("normalized XHTML bytes"));
        }
    }
    let body = &mut result.sections[current].body;
    if let Some(targets) = markers.get(&data.len()) {
        for target in targets {
            body.push_str(&format!("<a id=\"mobi-pos-{target}\"></a>"));
        }
    }
    close(&mut stack, body, 0);
    if let Some((href, text)) = heading
        && !text.trim().is_empty()
    {
        headings.push((label(&text), href));
    }
    result.navigation = if toc.is_empty() { headings } else { toc };
    for (n, item) in result.sections.iter_mut().enumerate() {
        item.label = label(&item.label);
        if item.label.is_empty() {
            item.label = format!("Section {}", n + 1);
        }
    }
    if result.navigation.is_empty() {
        result.navigation = result
            .sections
            .iter()
            .enumerate()
            .take(512)
            .map(|(n, s)| (s.label.clone(), format!("section{n}.xhtml")))
            .collect();
    }
    progress(observer, Stage::Markup, data.len(), data.len())?;
    Ok(result)
}

use std::{fmt, str};

#[derive(Debug, Clone, Copy)]
pub(crate) struct XmlLimits {
    pub max_bytes: usize,
    pub max_events: usize,
    pub max_depth: usize,
    pub max_attributes: usize,
}

impl Default for XmlLimits {
    fn default() -> Self {
        Self {
            max_bytes: 4 * 1024 * 1024,
            max_events: 100_000,
            max_depth: 256,
            max_attributes: 128,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum XmlError {
    Invalid(&'static str),
    LimitExceeded(&'static str),
    AllocationFailed,
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(f, "invalid XML: {reason}"),
            Self::LimitExceeded(what) => write!(f, "XML budget exceeded: {what}"),
            Self::AllocationFailed => f.write_str("cannot allocate XML data"),
        }
    }
}
impl std::error::Error for XmlError {}

type Result<T> = std::result::Result<T, XmlError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub empty: bool,
}

impl Element {
    pub fn attribute(&self, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(name, _)| local_name(name) == local)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    Start(Element),
    End(String),
    Text(String),
}

pub(crate) fn local_name(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

pub(crate) fn parse(bytes: &[u8], limits: XmlLimits) -> Result<Vec<Event>> {
    if bytes.len() > limits.max_bytes {
        return Err(XmlError::LimitExceeded("document bytes"));
    }
    if limits.max_events == 0
        || limits.max_events > 1_000_000
        || limits.max_depth == 0
        || limits.max_depth > 4096
        || limits.max_attributes == 0
        || limits.max_attributes > 4096
    {
        return Err(XmlError::Invalid("unsafe XML limit configuration"));
    }

    let input = str::from_utf8(bytes).map_err(|_| XmlError::Invalid("document is not UTF-8"))?;
    let input = input.strip_prefix('﻿').unwrap_or(input);
    let mut parser = Parser {
        input,
        position: 0,
        limits,
        events: Vec::new(),
        stack: Vec::new(),
        root_seen: false,
        root_closed: false,
    };
    parser.run()?;
    Ok(parser.events)
}

struct Parser<'a> {
    input: &'a str,
    position: usize,
    limits: XmlLimits,
    events: Vec<Event>,
    stack: Vec<String>,
    root_seen: bool,
    root_closed: bool,
}

impl Parser<'_> {
    fn run(&mut self) -> Result<()> {
        self.events
            .try_reserve(self.input.len().min(1024))
            .map_err(|_| XmlError::AllocationFailed)?;
        self.stack
            .try_reserve(self.limits.max_depth.min(64))
            .map_err(|_| XmlError::AllocationFailed)?;

        while self.position < self.input.len() {
            if self.remaining().starts_with("<!--") {
                self.comment()?;
            } else if self.remaining().starts_with("<![CDATA[") {
                self.cdata()?;
            } else if self.remaining().starts_with("<?") {
                self.processing_instruction()?;
            } else if self.remaining().starts_with("</") {
                self.end_tag()?;
            } else if self.remaining().starts_with("<!") {
                return Err(XmlError::Invalid(
                    "DTD and declarations other than comments/CDATA are unsupported",
                ));
            } else if self.remaining().starts_with('<') {
                self.start_tag()?;
            } else {
                self.text()?;
            }
        }
        if !self.root_seen || !self.root_closed || !self.stack.is_empty() {
            return Err(XmlError::Invalid("unclosed or absent root element"));
        }
        Ok(())
    }

    fn remaining(&self) -> &str {
        &self.input[self.position..]
    }

    fn charge_event(&self) -> Result<()> {
        if self.events.len() >= self.limits.max_events {
            Err(XmlError::LimitExceeded("events"))
        } else {
            Ok(())
        }
    }

    fn push_event(&mut self, event: Event) -> Result<()> {
        self.charge_event()?;
        self.events
            .try_reserve(1)
            .map_err(|_| XmlError::AllocationFailed)?;
        self.events.push(event);
        Ok(())
    }

    fn skip_whitespace(&mut self) {
        while self
            .input
            .as_bytes()
            .get(self.position)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.position += 1;
        }
    }

    fn name(&mut self) -> Result<String> {
        let start = self.position;
        while let Some(&byte) = self.input.as_bytes().get(self.position) {
            let valid = byte >= 0x80
                || byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b':' | b'-' | b'.');
            if !valid {
                break;
            }
            self.position += 1;
        }
        if self.position == start {
            return Err(XmlError::Invalid("expected XML name"));
        }
        let raw = &self.input[start..self.position];
        let first = raw.as_bytes()[0];
        if first < 0x80 && !(first.is_ascii_alphabetic() || matches!(first, b'_' | b':')) {
            return Err(XmlError::Invalid("invalid XML name start"));
        }
        owned(raw)
    }

    fn comment(&mut self) -> Result<()> {
        self.position += 4;
        let rest = self.remaining();
        let end = rest
            .find("-->")
            .ok_or(XmlError::Invalid("unterminated XML comment"))?;
        if rest[..end].contains("--") {
            return Err(XmlError::Invalid("double hyphen inside XML comment"));
        }
        self.position += end + 3;
        Ok(())
    }

    fn processing_instruction(&mut self) -> Result<()> {
        self.position += 2;
        let end = self
            .remaining()
            .find("?>")
            .ok_or(XmlError::Invalid("unterminated processing instruction"))?;
        self.position += end + 2;
        Ok(())
    }

    fn cdata(&mut self) -> Result<()> {
        self.position += 9;
        let end = self
            .remaining()
            .find("]]>")
            .ok_or(XmlError::Invalid("unterminated CDATA section"))?;
        let raw = &self.remaining()[..end];
        if !raw.is_empty() {
            if self.stack.is_empty() && !raw.chars().all(char::is_whitespace) {
                return Err(XmlError::Invalid("CDATA outside the root element"));
            }
            self.push_event(Event::Text(owned(raw)?))?;
        }
        self.position += end + 3;
        Ok(())
    }

    fn text(&mut self) -> Result<()> {
        let end = self
            .remaining()
            .find('<')
            .unwrap_or_else(|| self.remaining().len());
        let raw = &self.remaining()[..end];
        if !raw.is_empty() {
            let decoded = decode_entities(raw)?;
            if self.stack.is_empty() && !decoded.chars().all(char::is_whitespace) {
                return Err(XmlError::Invalid("text outside the root element"));
            }
            if !decoded.is_empty() {
                self.push_event(Event::Text(decoded))?;
            }
        }
        self.position += end;
        Ok(())
    }

    fn start_tag(&mut self) -> Result<()> {
        self.position += 1;
        if self.root_closed && self.stack.is_empty() {
            return Err(XmlError::Invalid("multiple XML root elements"));
        }
        let name = self.name()?;
        let mut attributes = Vec::new();
        attributes
            .try_reserve(8)
            .map_err(|_| XmlError::AllocationFailed)?;
        let empty = loop {
            self.skip_whitespace();
            match self.input.as_bytes().get(self.position) {
                Some(b'>') => {
                    self.position += 1;
                    break false;
                }
                Some(b'/') if self.input.as_bytes().get(self.position + 1) == Some(&b'>') => {
                    self.position += 2;
                    break true;
                }
                Some(_) => {
                    if attributes.len() >= self.limits.max_attributes {
                        return Err(XmlError::LimitExceeded("attributes per element"));
                    }
                    let attribute_name = self.name()?;
                    self.skip_whitespace();
                    if self.input.as_bytes().get(self.position) != Some(&b'=') {
                        return Err(XmlError::Invalid("attribute is missing '='"));
                    }
                    self.position += 1;
                    self.skip_whitespace();
                    let quote = *self
                        .input
                        .as_bytes()
                        .get(self.position)
                        .ok_or(XmlError::Invalid("attribute value is absent"))?;
                    if !matches!(quote, b'\'' | b'"') {
                        return Err(XmlError::Invalid("attribute value must be quoted"));
                    }
                    self.position += 1;
                    let value_start = self.position;
                    while self
                        .input
                        .as_bytes()
                        .get(self.position)
                        .is_some_and(|byte| *byte != quote)
                    {
                        if self.input.as_bytes()[self.position] == b'<' {
                            return Err(XmlError::Invalid("'<' inside attribute value"));
                        }
                        self.position += 1;
                    }
                    if self.input.as_bytes().get(self.position) != Some(&quote) {
                        return Err(XmlError::Invalid("unterminated attribute value"));
                    }
                    let value = decode_entities(&self.input[value_start..self.position])?;
                    self.position += 1;
                    if attributes
                        .iter()
                        .any(|(existing, _)| existing == &attribute_name)
                    {
                        return Err(XmlError::Invalid("duplicate XML attribute"));
                    }
                    attributes
                        .try_reserve(1)
                        .map_err(|_| XmlError::AllocationFailed)?;
                    attributes.push((attribute_name, value));
                }
                None => return Err(XmlError::Invalid("unterminated start tag")),
            }
        };

        let element_depth = self.stack.len() + 1;
        if element_depth > self.limits.max_depth {
            return Err(XmlError::LimitExceeded("element depth"));
        }
        if self.stack.is_empty() {
            if self.root_seen {
                return Err(XmlError::Invalid("multiple XML root elements"));
            }
            self.root_seen = true;
        }
        self.push_event(Event::Start(Element {
            name: name.clone(),
            attributes,
            empty,
        }))?;
        if empty {
            if self.stack.is_empty() {
                self.root_closed = true;
            }
        } else {
            self.stack
                .try_reserve(1)
                .map_err(|_| XmlError::AllocationFailed)?;
            self.stack.push(name);
        }
        Ok(())
    }

    fn end_tag(&mut self) -> Result<()> {
        self.position += 2;
        let name = self.name()?;
        self.skip_whitespace();
        if self.input.as_bytes().get(self.position) != Some(&b'>') {
            return Err(XmlError::Invalid("malformed end tag"));
        }
        self.position += 1;
        let open = self
            .stack
            .pop()
            .ok_or(XmlError::Invalid("end tag without start tag"))?;
        if open != name {
            return Err(XmlError::Invalid("mismatched XML end tag"));
        }
        self.push_event(Event::End(name))?;
        if self.stack.is_empty() {
            self.root_closed = true;
        }
        Ok(())
    }
}

fn owned(value: &str) -> Result<String> {
    let mut output = String::new();
    output
        .try_reserve_exact(value.len())
        .map_err(|_| XmlError::AllocationFailed)?;
    output.push_str(value);
    Ok(output)
}

fn decode_entities(raw: &str) -> Result<String> {
    if !raw.contains('&') {
        return owned(raw);
    }
    let mut output = String::new();
    output
        .try_reserve(raw.len())
        .map_err(|_| XmlError::AllocationFailed)?;
    let mut rest = raw;
    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        rest = &rest[start + 1..];
        let end = rest
            .find(';')
            .ok_or(XmlError::Invalid("unterminated XML entity"))?;
        let entity = &rest[..end];
        let character = match entity {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "apos" => '\'',
            "quot" => '"',
            _ if entity.starts_with("#x") => numeric_entity(&entity[2..], 16)?,
            _ if entity.starts_with('#') => numeric_entity(&entity[1..], 10)?,
            _ => return Err(XmlError::Invalid("unknown XML entity")),
        };
        output.push(character);
        rest = &rest[end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

fn numeric_entity(value: &str, radix: u32) -> Result<char> {
    if value.is_empty() || value.len() > 8 {
        return Err(XmlError::Invalid("invalid numeric XML entity"));
    }
    let code = u32::from_str_radix(value, radix)
        .map_err(|_| XmlError::Invalid("invalid numeric XML entity"))?;
    let character = char::from_u32(code).ok_or(XmlError::Invalid("invalid XML character"))?;
    if (character.is_control() && !matches!(character, '\t' | '\n' | '\r'))
        || matches!(code, 0xfffe | 0xffff)
    {
        return Err(XmlError::Invalid("disallowed XML character"));
    }
    Ok(character)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_entities_comments_and_cdata_parse() {
        let events = parse(
            br#"<?xml version="1.0"?><root xmlns:x="urn:x"><x:item a="A&amp;B" empty='yes'>text&#x20;<![CDATA[<raw>]]></x:item><!-- ok --></root>"#,
            XmlLimits::default(),
        )
        .unwrap();
        let item = events
            .iter()
            .find_map(|event| match event {
                Event::Start(element) if local_name(&element.name) == "item" => Some(element),
                _ => None,
            })
            .unwrap();
        assert_eq!(item.attribute("a"), Some("A&B"));
        let text: String = events
            .iter()
            .filter_map(|event| match event {
                Event::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "text <raw>");
    }

    #[test]
    fn malformed_or_dangerous_xml_is_rejected() {
        for xml in [
            "<a>",
            "<a></b>",
            "<a><b/></a><c/>",
            "<!DOCTYPE a><a/>",
            "<a x='1' x='2'/>",
            "<a>&custom;</a>",
            "<a x='<bad'/>",
            "text<a/>",
            "<a><!-- a--b --></a>",
        ] {
            assert!(
                parse(xml.as_bytes(), XmlLimits::default()).is_err(),
                "{xml}"
            );
        }
    }

    #[test]
    fn limits_are_enforced() {
        assert!(
            parse(
                b"<a><b/></a>",
                XmlLimits {
                    max_depth: 1,
                    ..XmlLimits::default()
                }
            )
            .is_err()
        );
        assert!(
            parse(
                b"<a x='1' y='2'/>",
                XmlLimits {
                    max_attributes: 1,
                    ..XmlLimits::default()
                }
            )
            .is_err()
        );
        assert!(
            parse(
                b"<a/>",
                XmlLimits {
                    max_bytes: 3,
                    ..XmlLimits::default()
                }
            )
            .is_err()
        );
    }
}

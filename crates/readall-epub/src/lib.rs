//! Dependency-free EPUB container/package foundation.
//! This validates an intentionally small, explicit subset; XHTML/CSS layout is not implemented here.
mod xhtml;
mod xml;

use readall_archive::{ArchiveError, ZipArchive, ZipLimits};
use readall_core::DocumentId;
use std::{
    collections::{HashMap, HashSet},
    fmt,
    str::FromStr,
};

use xml::{Event, XmlError, XmlLimits, local_name};

type Result<T> = std::result::Result<T, EpubError>;

#[derive(Debug, Clone)]
pub enum EpubError {
    Archive(ArchiveError),
    Invalid(&'static str),
    Unsupported(&'static str),
    LimitExceeded(&'static str),
    InvalidLocator(&'static str),
    AllocationFailed,
}

impl fmt::Display for EpubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Archive(error) => error.fmt(f),
            Self::Invalid(reason) => write!(f, "invalid EPUB: {reason}"),
            Self::Unsupported(reason) => write!(f, "unsupported EPUB feature: {reason}"),
            Self::LimitExceeded(what) => write!(f, "EPUB budget exceeded: {what}"),
            Self::InvalidLocator(reason) => write!(f, "invalid EPUB locator: {reason}"),
            Self::AllocationFailed => f.write_str("cannot allocate EPUB data"),
        }
    }
}
impl std::error::Error for EpubError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Archive(error) => Some(error),
            _ => None,
        }
    }
}
impl From<ArchiveError> for EpubError {
    fn from(value: ArchiveError) -> Self {
        Self::Archive(value)
    }
}
impl From<XmlError> for EpubError {
    fn from(value: XmlError) -> Self {
        match value {
            XmlError::Invalid(reason) => Self::Invalid(reason),
            XmlError::LimitExceeded(what) => Self::LimitExceeded(what),
            XmlError::AllocationFailed => Self::AllocationFailed,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EpubLimits {
    pub zip: ZipLimits,
    pub max_xml_bytes: usize,
    pub max_manifest_items: usize,
    pub max_spine_items: usize,
}

impl Default for EpubLimits {
    fn default() -> Self {
        Self {
            zip: ZipLimits::default(),
            max_xml_bytes: 4 * 1024 * 1024,
            max_manifest_items: 16_384,
            max_spine_items: 16_384,
        }
    }
}

/// Stable reading position in the canonical text extracted by the EPUB v1 subset.
/// Spine indices are zero-based in the serialized form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpubLocator {
    book_id: DocumentId,
    spine_index: usize,
    utf8_offset: u64,
}

impl EpubLocator {
    pub fn book_id(&self) -> DocumentId {
        self.book_id
    }
    pub fn spine_index(&self) -> usize {
        self.spine_index
    }
    pub fn utf8_offset(&self) -> u64 {
        self.utf8_offset
    }
}

impl fmt::Display for EpubLocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "epub-v1:{}:{}:{}",
            self.book_id, self.spine_index, self.utf8_offset
        )
    }
}

impl FromStr for EpubLocator {
    type Err = EpubError;

    fn from_str(text: &str) -> Result<Self> {
        let mut parts = text.split(':');
        if parts.next() != Some("epub-v1") {
            return Err(EpubError::InvalidLocator("unknown locator version"));
        }
        let id = parts
            .next()
            .ok_or(EpubError::InvalidLocator("missing book ID"))?
            .parse::<DocumentId>()
            .map_err(|_| EpubError::InvalidLocator("invalid book ID"))?;
        let spine = parts
            .next()
            .ok_or(EpubError::InvalidLocator("missing spine index"))?;
        let offset = parts
            .next()
            .ok_or(EpubError::InvalidLocator("missing UTF-8 offset"))?;
        if spine.is_empty()
            || offset.is_empty()
            || !spine.bytes().all(|byte| byte.is_ascii_digit())
            || !offset.bytes().all(|byte| byte.is_ascii_digit())
            || parts.next().is_some()
        {
            return Err(EpubError::InvalidLocator(
                "expected numeric spine index and UTF-8 offset",
            ));
        }
        Ok(Self {
            book_id: id,
            spine_index: spine
                .parse()
                .map_err(|_| EpubError::InvalidLocator("spine index overflow"))?,
            utf8_offset: offset
                .parse()
                .map_err(|_| EpubError::InvalidLocator("UTF-8 offset overflow"))?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ManifestItem {
    id: String,
    path: String,
    media_type: String,
    properties: String,
}

impl ManifestItem {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn media_type(&self) -> &str {
        &self.media_type
    }
    pub fn properties(&self) -> &str {
        &self.properties
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SpineItem {
    manifest_index: usize,
    linear: bool,
}

impl SpineItem {
    pub fn manifest_index(&self) -> usize {
        self.manifest_index
    }
    pub fn linear(&self) -> bool {
        self.linear
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationEntry {
    label: String,
    spine_index: usize,
    fragment: Option<String>,
    depth: usize,
}

impl NavigationEntry {
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn spine_index(&self) -> usize {
        self.spine_index
    }
    pub fn fragment(&self) -> Option<&str> {
        self.fragment.as_deref()
    }
    pub fn depth(&self) -> usize {
        self.depth
    }
}

#[derive(Debug)]
pub struct EpubBook<'a> {
    id: DocumentId,
    archive: ZipArchive<'a>,
    package_path: String,
    title: Option<String>,
    manifest: Vec<ManifestItem>,
    spine: Vec<SpineItem>,
    limits: EpubLimits,
}

impl<'a> EpubBook<'a> {
    pub fn parse(bytes: &'a [u8], limits: EpubLimits) -> Result<Self> {
        validate_limits(limits)?;
        let archive = ZipArchive::parse(bytes, limits.zip)?;
        validate_mimetype(&archive)?;

        if archive.entry("META-INF/encryption.xml").is_some() {
            return Err(EpubError::Unsupported(
                "encrypted or obfuscated resources are not handled yet",
            ));
        }

        let container = archive
            .read("META-INF/container.xml")
            .map_err(|_| EpubError::Invalid("META-INF/container.xml is absent or unreadable"))?;
        ensure_xml_size(&container, limits.max_xml_bytes)?;
        let package_path = parse_container(&container, limits.max_xml_bytes)?;
        if archive.entry(&package_path).is_none() {
            return Err(EpubError::Invalid(
                "package document is absent from the archive",
            ));
        }

        let package = archive.read(&package_path)?;
        ensure_xml_size(&package, limits.max_xml_bytes)?;
        let parsed = parse_package(&package, &package_path, &archive, limits)?;

        Ok(Self {
            id: DocumentId::of(bytes),
            archive,
            package_path,
            title: parsed.title,
            manifest: parsed.manifest,
            spine: parsed.spine,
            limits,
        })
    }

    pub fn id(&self) -> DocumentId {
        self.id
    }
    pub fn package_path(&self) -> &str {
        &self.package_path
    }
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    pub fn manifest(&self) -> &[ManifestItem] {
        &self.manifest
    }
    pub fn spine(&self) -> &[SpineItem] {
        &self.spine
    }
    pub fn spine_item(&self, index: usize) -> Option<&ManifestItem> {
        self.spine
            .get(index)
            .and_then(|item| self.manifest.get(item.manifest_index))
    }
    pub fn read_spine(&self, index: usize) -> Result<Vec<u8>> {
        let item = self
            .spine_item(index)
            .ok_or(EpubError::Invalid("spine index is out of range"))?;
        Ok(self.archive.read(item.path())?)
    }
    /// Extracts the initial XHTML reading subset from one spine item.
    /// CSS, images, SVG, MathML and scripting are intentionally not rendered here.
    pub fn read_spine_text(&self, index: usize) -> Result<String> {
        let item = self
            .spine_item(index)
            .ok_or(EpubError::Invalid("spine index is out of range"))?;
        if item.media_type() != "application/xhtml+xml" {
            return Err(EpubError::Unsupported(
                "spine item is not application/xhtml+xml",
            ));
        }
        let bytes = self.archive.read(item.path())?;
        ensure_xml_size(&bytes, self.limits.max_xml_bytes)?;
        xhtml::extract(&bytes, self.limits.max_xml_bytes)
    }
    pub fn locator(&self, spine_index: usize, utf8_offset: usize) -> Result<EpubLocator> {
        let text = self.read_spine_text(spine_index)?;
        if !text.is_char_boundary(utf8_offset) {
            return Err(EpubError::InvalidLocator(
                "offset is outside the chapter or inside a UTF-8 character",
            ));
        }
        Ok(EpubLocator {
            book_id: self.id,
            spine_index,
            utf8_offset: utf8_offset as u64,
        })
    }

    pub fn restore(&self, locator: &EpubLocator) -> Result<(usize, usize)> {
        if locator.book_id != self.id {
            return Err(EpubError::InvalidLocator(
                "locator belongs to another EPUB revision",
            ));
        }
        let offset = usize::try_from(locator.utf8_offset)
            .map_err(|_| EpubError::InvalidLocator("offset cannot fit this platform"))?;
        let text = self.read_spine_text(locator.spine_index)?;
        if !text.is_char_boundary(offset) {
            return Err(EpubError::InvalidLocator(
                "offset is outside the chapter or inside a UTF-8 character",
            ));
        }
        Ok((locator.spine_index, offset))
    }

    pub fn read_resource(&self, path: &str) -> Result<Vec<u8>> {
        Ok(self.archive.read(path)?)
    }

    /// Returns the EPUB 3 table of contents from the manifest `nav` document.
    /// Links that do not resolve to a spine document are ignored. Fragment IDs
    /// are preserved for callers that support intra-document navigation.
    pub fn navigation(&self) -> Result<Vec<NavigationEntry>> {
        let Some(item) = self
            .manifest
            .iter()
            .find(|item| has_token(item.properties(), "nav"))
        else {
            return Ok(Vec::new());
        };
        if item.media_type() != "application/xhtml+xml" {
            return Err(EpubError::Invalid(
                "EPUB navigation document is not application/xhtml+xml",
            ));
        }
        let bytes = self.archive.read(item.path())?;
        ensure_xml_size(&bytes, self.limits.max_xml_bytes)?;
        parse_navigation_document(
            &bytes,
            item.path(),
            &self.manifest,
            &self.spine,
            self.limits.max_xml_bytes,
            self.limits.max_spine_items,
        )
    }
}

struct ParsedPackage {
    title: Option<String>,
    manifest: Vec<ManifestItem>,
    spine: Vec<SpineItem>,
}

struct SpineReference {
    idref: String,
    linear: bool,
}

fn validate_limits(limits: EpubLimits) -> Result<()> {
    if limits.max_xml_bytes == 0
        || limits.max_xml_bytes > 64 * 1024 * 1024
        || limits.max_manifest_items == 0
        || limits.max_manifest_items > 1_000_000
        || limits.max_spine_items == 0
        || limits.max_spine_items > 1_000_000
    {
        return Err(EpubError::Invalid("unsafe EPUB limit configuration"));
    }
    Ok(())
}

fn ensure_xml_size(bytes: &[u8], limit: usize) -> Result<()> {
    if bytes.len() > limit {
        Err(EpubError::LimitExceeded("XML document bytes"))
    } else {
        Ok(())
    }
}

fn xml_limits(max_bytes: usize) -> XmlLimits {
    XmlLimits {
        max_bytes,
        ..XmlLimits::default()
    }
}

fn validate_mimetype(archive: &ZipArchive<'_>) -> Result<()> {
    let entry = archive
        .entry("mimetype")
        .ok_or(EpubError::Invalid("mimetype entry is absent"))?;
    // EPUB requires this exact first local entry, stored and without a local extra field.
    if entry.local_offset() != 0
        || entry.data_offset() != 38
        || entry.method() != 0
        || entry.compressed_size() != entry.uncompressed_size()
    {
        return Err(EpubError::Invalid(
            "mimetype must be the first uncompressed entry without an extra field",
        ));
    }
    if archive.read("mimetype")? != b"application/epub+zip" {
        return Err(EpubError::Invalid("incorrect EPUB mimetype"));
    }
    Ok(())
}

fn parse_container(bytes: &[u8], max_xml_bytes: usize) -> Result<String> {
    let events = xml::parse(bytes, xml_limits(max_xml_bytes))?;
    let first = events.iter().find_map(|event| match event {
        Event::Start(element) => Some(element),
        _ => None,
    });
    if first.is_none_or(|element| local_name(&element.name) != "container") {
        return Err(EpubError::Invalid("container.xml root element"));
    }

    let mut rootfile = None;
    for event in &events {
        let Event::Start(element) = event else {
            continue;
        };
        if local_name(&element.name) != "rootfile" {
            continue;
        }
        if element.attribute("media-type") != Some("application/oebps-package+xml") {
            continue;
        }
        let path = element
            .attribute("full-path")
            .ok_or(EpubError::Invalid("rootfile is missing full-path"))?;
        let path = resolve_path("", path)?;
        if rootfile.replace(path).is_some() {
            return Err(EpubError::Unsupported(
                "multiple package documents are not supported yet",
            ));
        }
    }
    rootfile.ok_or(EpubError::Invalid("EPUB package rootfile is absent"))
}

fn parse_package(
    bytes: &[u8],
    package_path: &str,
    archive: &ZipArchive<'_>,
    limits: EpubLimits,
) -> Result<ParsedPackage> {
    let events = xml::parse(bytes, xml_limits(limits.max_xml_bytes))?;
    let first = events.iter().find_map(|event| match event {
        Event::Start(element) => Some(element),
        _ => None,
    });
    if first.is_none_or(|element| local_name(&element.name) != "package") {
        return Err(EpubError::Invalid("package document root element"));
    }

    let mut manifest = Vec::new();
    manifest
        .try_reserve(limits.max_manifest_items.min(256))
        .map_err(|_| EpubError::AllocationFailed)?;
    let mut spine_refs = Vec::new();
    spine_refs
        .try_reserve(limits.max_spine_items.min(256))
        .map_err(|_| EpubError::AllocationFailed)?;
    let mut ids = HashSet::new();
    ids.try_reserve(limits.max_manifest_items.min(256))
        .map_err(|_| EpubError::AllocationFailed)?;

    let mut depth = 0_usize;
    let mut manifest_depth = None;
    let mut spine_depth = None;
    let mut title_depth = None;
    let mut title_text = String::new();
    for event in &events {
        match event {
            Event::Start(element) => {
                let element_depth = depth + 1;
                let local = local_name(&element.name);
                if local == "manifest" && manifest_depth.is_none() && !element.empty {
                    manifest_depth = Some(element_depth);
                } else if local == "spine" && spine_depth.is_none() && !element.empty {
                    spine_depth = Some(element_depth);
                } else if local == "title" && title_depth.is_none() && !element.empty {
                    title_depth = Some(element_depth);
                    title_text.clear();
                }

                if local == "item" && manifest_depth.is_some() {
                    if manifest.len() >= limits.max_manifest_items {
                        return Err(EpubError::LimitExceeded("manifest items"));
                    }
                    let id = required_attribute(element, "id", "manifest item id")?;
                    let href = required_attribute(element, "href", "manifest item href")?;
                    let media_type =
                        required_attribute(element, "media-type", "manifest media-type")?;
                    if !ids.insert(id.to_owned()) {
                        return Err(EpubError::Invalid("duplicate manifest id"));
                    }
                    let path = resolve_path(package_path, href)?;
                    if archive.entry(&path).is_none() {
                        return Err(EpubError::Unsupported(
                            "remote or absent manifest resources",
                        ));
                    }
                    manifest
                        .try_reserve(1)
                        .map_err(|_| EpubError::AllocationFailed)?;
                    manifest.push(ManifestItem {
                        id: owned(id)?,
                        path,
                        media_type: owned(media_type)?,
                        properties: owned(element.attribute("properties").unwrap_or(""))?,
                    });
                } else if local == "itemref" && spine_depth.is_some() {
                    if spine_refs.len() >= limits.max_spine_items {
                        return Err(EpubError::LimitExceeded("spine items"));
                    }
                    let idref = required_attribute(element, "idref", "spine idref")?;
                    let linear = match element.attribute("linear") {
                        None | Some("yes") => true,
                        Some("no") => false,
                        Some(_) => return Err(EpubError::Invalid("invalid spine linear value")),
                    };
                    spine_refs
                        .try_reserve(1)
                        .map_err(|_| EpubError::AllocationFailed)?;
                    spine_refs.push(SpineReference {
                        idref: owned(idref)?,
                        linear,
                    });
                }
                if !element.empty {
                    depth = element_depth;
                }
            }
            Event::Text(text) if title_depth.is_some() => title_text.push_str(text),
            Event::Text(_) => {}
            Event::End(name) => {
                let local = local_name(name);
                if title_depth == Some(depth) && local == "title" {
                    title_depth = None;
                }
                if manifest_depth == Some(depth) && local == "manifest" {
                    manifest_depth = None;
                }
                if spine_depth == Some(depth) && local == "spine" {
                    spine_depth = None;
                }
                depth = depth.saturating_sub(1);
            }
        }
    }

    if manifest.is_empty() {
        return Err(EpubError::Invalid("package manifest is empty"));
    }
    if spine_refs.is_empty() {
        return Err(EpubError::Invalid("package spine is empty"));
    }

    let mut by_id = HashMap::new();
    by_id
        .try_reserve(manifest.len())
        .map_err(|_| EpubError::AllocationFailed)?;
    for (index, item) in manifest.iter().enumerate() {
        by_id.insert(item.id.as_str(), index);
    }
    let mut spine = Vec::new();
    spine
        .try_reserve(spine_refs.len())
        .map_err(|_| EpubError::AllocationFailed)?;
    for reference in spine_refs {
        let manifest_index = *by_id
            .get(reference.idref.as_str())
            .ok_or(EpubError::Invalid("spine references an absent manifest id"))?;
        spine.push(SpineItem {
            manifest_index,
            linear: reference.linear,
        });
    }

    let title = {
        let title = title_text.trim();
        if title.is_empty() {
            None
        } else {
            Some(owned(title)?)
        }
    };
    Ok(ParsedPackage {
        title,
        manifest,
        spine,
    })
}

fn has_token(value: &str, token: &str) -> bool {
    value.split_ascii_whitespace().any(|value| value == token)
}

#[derive(Debug)]
struct NavigationLink {
    element_depth: usize,
    list_depth: usize,
    href: String,
    label: String,
}

fn parse_navigation_document(
    bytes: &[u8],
    navigation_path: &str,
    manifest: &[ManifestItem],
    spine: &[SpineItem],
    max_xml_bytes: usize,
    max_entries: usize,
) -> Result<Vec<NavigationEntry>> {
    let events = xml::parse(bytes, xml_limits(max_xml_bytes))?;
    let root = events.iter().find_map(|event| match event {
        Event::Start(element) => Some(local_name(&element.name)),
        _ => None,
    });
    if root != Some("html") {
        return Err(EpubError::Invalid(
            "EPUB navigation document root is not XHTML html",
        ));
    }

    let mut entries = Vec::new();
    entries
        .try_reserve(max_entries.min(128))
        .map_err(|_| EpubError::AllocationFailed)?;
    let mut depth = 0_usize;
    let mut toc_depth = None;
    let mut toc_complete = false;
    let mut list_depth = 0_usize;
    let mut link: Option<NavigationLink> = None;

    for event in &events {
        match event {
            Event::Start(element) => {
                let element_depth = depth + 1;
                let local = local_name(&element.name);
                if !toc_complete
                    && toc_depth.is_none()
                    && local == "nav"
                    && element
                        .attribute("type")
                        .is_some_and(|value| has_token(value, "toc"))
                {
                    toc_depth = Some(element_depth);
                } else if toc_depth.is_some() {
                    if local == "ol" && !element.empty {
                        list_depth = list_depth.saturating_add(1);
                    } else if local == "a" && link.is_none() && !element.empty {
                        if let Some(href) = element.attribute("href") {
                            link = Some(NavigationLink {
                                element_depth,
                                list_depth: list_depth.saturating_sub(1),
                                href: owned(href)?,
                                label: String::new(),
                            });
                        }
                    } else if let Some(active) = &mut link
                        && local != "a"
                        && let Some(alternative) = element
                            .attribute("alt")
                            .or_else(|| element.attribute("title"))
                    {
                        append_navigation_label(&mut active.label, alternative)?;
                    }
                }
                if !element.empty {
                    depth = element_depth;
                }
            }
            Event::Text(text) => {
                if let Some(active) = &mut link {
                    append_navigation_label(&mut active.label, text)?;
                }
            }
            Event::End(name) => {
                let local = local_name(name);
                if link
                    .as_ref()
                    .is_some_and(|active| active.element_depth == depth && local == "a")
                {
                    let active = link.take().expect("checked above");
                    let label = normalize_navigation_label(&active.label)?;
                    if !label.is_empty() {
                        let (path, fragment) =
                            resolve_navigation_href(navigation_path, &active.href)?;
                        if let Some(spine_index) = spine_index_for_path(manifest, spine, &path) {
                            if entries.len() >= max_entries {
                                return Err(EpubError::LimitExceeded("navigation entries"));
                            }
                            entries
                                .try_reserve(1)
                                .map_err(|_| EpubError::AllocationFailed)?;
                            entries.push(NavigationEntry {
                                label,
                                spine_index,
                                fragment,
                                depth: active.list_depth,
                            });
                        }
                    }
                }
                if toc_depth.is_some() && local == "ol" {
                    list_depth = list_depth.saturating_sub(1);
                }
                if toc_depth == Some(depth) && local == "nav" {
                    toc_depth = None;
                    toc_complete = true;
                    list_depth = 0;
                    link = None;
                }
                depth = depth.saturating_sub(1);
            }
        }
    }
    Ok(entries)
}

fn append_navigation_label(output: &mut String, value: &str) -> Result<()> {
    if output.len().saturating_add(value.len()) > 16 * 1024 {
        return Err(EpubError::LimitExceeded("navigation label bytes"));
    }
    output
        .try_reserve(value.len())
        .map_err(|_| EpubError::AllocationFailed)?;
    output.push_str(value);
    Ok(())
}

fn normalize_navigation_label(raw: &str) -> Result<String> {
    let mut output = String::new();
    output
        .try_reserve(raw.len().min(256))
        .map_err(|_| EpubError::AllocationFailed)?;
    let mut pending_space = false;
    for ch in raw.chars() {
        if ch.is_whitespace() {
            pending_space = !output.is_empty();
            continue;
        }
        if pending_space {
            output.push(' ');
            pending_space = false;
        }
        output.push(ch);
        if output.chars().count() >= 256 {
            output.push('…');
            break;
        }
    }
    Ok(output)
}

fn resolve_navigation_href(base_file: &str, href: &str) -> Result<(String, Option<String>)> {
    if href.is_empty() || href.contains('?') {
        return Err(EpubError::Unsupported("non-local navigation reference"));
    }
    let (resource, fragment) = href
        .split_once('#')
        .map_or((href, None), |(resource, fragment)| {
            (resource, (!fragment.is_empty()).then_some(fragment))
        });
    let path = if resource.is_empty() {
        owned(base_file)?
    } else {
        resolve_path(base_file, resource)?
    };
    let fragment = fragment.map(owned).transpose()?;
    Ok((path, fragment))
}

fn spine_index_for_path(
    manifest: &[ManifestItem],
    spine: &[SpineItem],
    path: &str,
) -> Option<usize> {
    spine.iter().enumerate().find_map(|(index, item)| {
        manifest
            .get(item.manifest_index)
            .filter(|manifest_item| manifest_item.path() == path)
            .map(|_| index)
    })
}

fn required_attribute<'a>(
    element: &'a xml::Element,
    name: &str,
    description: &'static str,
) -> Result<&'a str> {
    element
        .attribute(name)
        .filter(|value| !value.is_empty())
        .ok_or(EpubError::Invalid(description))
}

fn resolve_path(base_file: &str, reference: &str) -> Result<String> {
    if reference.is_empty()
        || reference.starts_with('/')
        || reference.contains('\\')
        || reference.contains('#')
        || reference.contains('?')
    {
        return Err(EpubError::Unsupported("non-local resource reference"));
    }
    let first = reference.split('/').next().unwrap_or(reference);
    if first.contains(':') {
        return Err(EpubError::Unsupported("remote resource URI"));
    }

    let mut components: Vec<String> = if base_file.is_empty() {
        Vec::new()
    } else {
        base_file
            .split('/')
            .take(base_file.split('/').count().saturating_sub(1))
            .map(owned)
            .collect::<Result<_>>()?
    };
    for raw in reference.split('/') {
        let component = percent_decode_component(raw)?;
        match component.as_str() {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(EpubError::Invalid("resource path escapes archive root"));
                }
            }
            _ => {
                components
                    .try_reserve(1)
                    .map_err(|_| EpubError::AllocationFailed)?;
                components.push(component);
            }
        }
    }
    if components.is_empty() {
        return Err(EpubError::Invalid("empty resolved resource path"));
    }
    Ok(components.join("/"))
}

fn percent_decode_component(raw: &str) -> Result<String> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve(raw.len())
        .map_err(|_| EpubError::AllocationFailed)?;
    let raw = raw.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' {
            if index + 2 >= raw.len() {
                return Err(EpubError::Invalid("truncated percent escape"));
            }
            let high = hex(raw[index + 1])?;
            let low = hex(raw[index + 2])?;
            let byte = high << 4 | low;
            if matches!(byte, 0 | b'/' | b'\\') {
                return Err(EpubError::Invalid("unsafe percent-encoded path byte"));
            }
            bytes.push(byte);
            index += 3;
        } else {
            bytes.push(raw[index]);
            index += 1;
        }
    }
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| EpubError::Invalid("resource path is not UTF-8"))?;
    owned(value)
}

fn hex(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(EpubError::Invalid("invalid percent escape")),
    }
}

fn owned(value: &str) -> Result<String> {
    let mut output = String::new();
    output
        .try_reserve_exact(value.len())
        .map_err(|_| EpubError::AllocationFailed)?;
    output.push_str(value);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use readall_archive::crc32;

    struct Entry<'a> {
        name: &'a str,
        data: &'a [u8],
    }
    fn push16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn push32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn epub(entries: &[Entry<'_>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut records = Vec::new();
        for entry in entries {
            let offset = bytes.len() as u32;
            let crc = crc32(entry.data);
            push32(&mut bytes, 0x0403_4b50);
            push16(&mut bytes, 20);
            push16(&mut bytes, 0x0800);
            push16(&mut bytes, 0);
            push16(&mut bytes, 0);
            push16(&mut bytes, 0);
            push32(&mut bytes, crc);
            push32(&mut bytes, entry.data.len() as u32);
            push32(&mut bytes, entry.data.len() as u32);
            push16(&mut bytes, entry.name.len() as u16);
            push16(&mut bytes, 0);
            bytes.extend_from_slice(entry.name.as_bytes());
            bytes.extend_from_slice(entry.data);
            records.push((entry, offset, crc));
        }
        let central_offset = bytes.len() as u32;
        for (entry, offset, crc) in &records {
            push32(&mut bytes, 0x0201_4b50);
            push16(&mut bytes, 20);
            push16(&mut bytes, 20);
            push16(&mut bytes, 0x0800);
            push16(&mut bytes, 0);
            push16(&mut bytes, 0);
            push16(&mut bytes, 0);
            push32(&mut bytes, *crc);
            push32(&mut bytes, entry.data.len() as u32);
            push32(&mut bytes, entry.data.len() as u32);
            push16(&mut bytes, entry.name.len() as u16);
            push16(&mut bytes, 0);
            push16(&mut bytes, 0);
            push16(&mut bytes, 0);
            push16(&mut bytes, 0);
            push32(&mut bytes, 0);
            push32(&mut bytes, *offset);
            bytes.extend_from_slice(entry.name.as_bytes());
        }
        let size = bytes.len() as u32 - central_offset;
        push32(&mut bytes, 0x0605_4b50);
        push16(&mut bytes, 0);
        push16(&mut bytes, 0);
        push16(&mut bytes, entries.len() as u16);
        push16(&mut bytes, entries.len() as u16);
        push32(&mut bytes, size);
        push32(&mut bytes, central_offset);
        push16(&mut bytes, 0);
        bytes
    }

    fn fixture() -> Vec<u8> {
        const CONTAINER: &[u8] = br#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
        const PACKAGE: &[u8] = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <metadata><dc:title>ReadAll &amp; Test</dc:title></metadata>
  <manifest>
    <item id="chapter" href="text/chapter.xhtml" media-type="application/xhtml+xml"/>
    <item id="style" href="styles/main.css" media-type="text/css"/>
  </manifest>
  <spine><itemref idref="chapter"/></spine>
</package>"#;
        epub(&[
            Entry {
                name: "mimetype",
                data: b"application/epub+zip",
            },
            Entry {
                name: "META-INF/container.xml",
                data: CONTAINER,
            },
            Entry {
                name: "OEBPS/package.opf",
                data: PACKAGE,
            },
            Entry {
                name: "OEBPS/text/chapter.xhtml",
                data: b"<html><body>Hello</body></html>",
            },
            Entry {
                name: "OEBPS/styles/main.css",
                data: b"body { margin: 0; }",
            },
        ])
    }

    fn navigation_fixture() -> Vec<u8> {
        const CONTAINER: &[u8] = br#"<container><rootfiles><rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        const PACKAGE: &[u8] = br#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>Navigation Test</dc:title></metadata><manifest><item id="one" href="text/one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="text/two.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="scripted nav cover-image"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>"#;
        const NAV: &str = r#"<?xml version="1.0"?><!DOCTYPE html><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="landmarks"><ol><li><a href="text/one.xhtml">Landmark</a></li></ol></nav><nav epub:type="toc"><h1>Contents</h1><ol><li><a href="text/one.xhtml#intro">第一章 <em>开始</em></a><ol><li><a href="text/one.xhtml#details">详细部分</a></li></ol></li><li><a href="text/two.xhtml"><img alt="第二章" src="cover.png"/></a></li><li><a href="extra.xhtml">Not in spine</a></li></ol></nav></body></html>"#;
        epub(&[
            Entry {
                name: "mimetype",
                data: b"application/epub+zip",
            },
            Entry {
                name: "META-INF/container.xml",
                data: CONTAINER,
            },
            Entry {
                name: "OEBPS/package.opf",
                data: PACKAGE,
            },
            Entry {
                name: "OEBPS/text/one.xhtml",
                data: b"<html><body><h1>Wrong first line one</h1></body></html>",
            },
            Entry {
                name: "OEBPS/text/two.xhtml",
                data: b"<html><body><h1>Wrong first line two</h1></body></html>",
            },
            Entry {
                name: "OEBPS/nav.xhtml",
                data: NAV.as_bytes(),
            },
            Entry {
                name: "OEBPS/extra.xhtml",
                data: b"<html><body>Extra</body></html>",
            },
        ])
    }

    #[test]
    fn epub3_navigation_document_maps_labels_depth_and_fragments_to_spine() {
        let bytes = navigation_fixture();
        let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
        let navigation = book.navigation().unwrap();
        assert_eq!(navigation.len(), 3);
        assert_eq!(navigation[0].label(), "第一章 开始");
        assert_eq!(navigation[0].spine_index(), 0);
        assert_eq!(navigation[0].fragment(), Some("intro"));
        assert_eq!(navigation[0].depth(), 0);
        assert_eq!(navigation[1].label(), "详细部分");
        assert_eq!(navigation[1].spine_index(), 0);
        assert_eq!(navigation[1].fragment(), Some("details"));
        assert_eq!(navigation[1].depth(), 1);
        assert_eq!(navigation[2].label(), "第二章");
        assert_eq!(navigation[2].spine_index(), 1);
        assert_eq!(navigation[2].fragment(), None);
        assert_eq!(navigation[2].depth(), 0);
    }

    #[test]
    fn books_without_epub3_navigation_return_an_empty_navigation_list() {
        let bytes = fixture();
        let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
        assert!(book.navigation().unwrap().is_empty());
    }

    #[test]
    fn container_manifest_spine_and_title_are_parsed() {
        let bytes = fixture();
        let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
        assert_eq!(book.package_path(), "OEBPS/package.opf");
        assert_eq!(book.title(), Some("ReadAll & Test"));
        assert_eq!(book.manifest().len(), 2);
        assert_eq!(book.spine().len(), 1);
        let chapter = book.spine_item(0).unwrap();
        assert_eq!(chapter.id(), "chapter");
        assert_eq!(chapter.path(), "OEBPS/text/chapter.xhtml");
        assert_eq!(chapter.media_type(), "application/xhtml+xml");
        assert_eq!(
            book.read_spine(0).unwrap(),
            b"<html><body>Hello</body></html>"
        );
        assert_eq!(book.read_spine_text(0).unwrap(), "Hello");
        let locator = book.locator(0, 2).unwrap();
        assert_eq!(locator.spine_index(), 0);
        assert_eq!(locator.utf8_offset(), 2);
        assert_eq!(book.restore(&locator).unwrap(), (0, 2));
        assert_eq!(locator.to_string().parse::<EpubLocator>().unwrap(), locator);
    }

    #[test]
    fn epub_locators_reject_other_books_bad_offsets_and_malformed_values() {
        let bytes = fixture();
        let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
        assert!(book.locator(0, usize::MAX).is_err());
        assert!(book.locator(99, 0).is_err());

        let foreign: EpubLocator = format!("epub-v1:{}:0:0", DocumentId::of(b"other"))
            .parse()
            .unwrap();
        assert!(book.restore(&foreign).is_err());

        for value in [
            "epub-v2:00:0:0",
            "epub-v1:bad:0:0",
            "epub-v1:0000000000000000000000000000000000000000000000000000000000000000:-1:0",
            "epub-v1:0000000000000000000000000000000000000000000000000000000000000000:0:+1",
            "epub-v1:0000000000000000000000000000000000000000000000000000000000000000:0:0:extra",
        ] {
            assert!(value.parse::<EpubLocator>().is_err(), "{value}");
        }
    }

    #[test]
    fn path_resolution_is_bounded_to_the_archive_root() {
        assert_eq!(
            resolve_path("OEBPS/package.opf", "text/ch%61pter.xhtml").unwrap(),
            "OEBPS/text/chapter.xhtml"
        );
        assert_eq!(
            resolve_path("OEBPS/package.opf", "../shared/chapter.xhtml").unwrap(),
            "shared/chapter.xhtml"
        );
        for reference in [
            "../../escape.xhtml",
            "/absolute.xhtml",
            "https://example.test/a.xhtml",
            "a%2fb.xhtml",
            "a\\b.xhtml",
            "a.xhtml#fragment",
        ] {
            assert!(resolve_path("OEBPS/package.opf", reference).is_err());
        }
    }

    #[test]
    fn mimetype_container_and_spine_invariants_are_enforced() {
        let mut wrong = fixture();
        let position = wrong
            .windows(b"application/epub+zip".len())
            .position(|window| window == b"application/epub+zip")
            .unwrap();
        wrong[position] = b'X';
        assert!(EpubBook::parse(&wrong, EpubLimits::default()).is_err());

        const CONTAINER: &[u8] = br#"<container><rootfiles><rootfile full-path="../escape.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        let unsafe_book = epub(&[
            Entry {
                name: "mimetype",
                data: b"application/epub+zip",
            },
            Entry {
                name: "META-INF/container.xml",
                data: CONTAINER,
            },
        ]);
        assert!(EpubBook::parse(&unsafe_book, EpubLimits::default()).is_err());
    }

    #[test]
    fn encryption_marker_is_rejected_in_initial_subset() {
        let mut entries = vec![
            Entry {
                name: "mimetype",
                data: b"application/epub+zip",
            },
            Entry {
                name: "META-INF/container.xml",
                data: br#"<container><rootfiles><rootfile full-path="package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            },
            Entry {
                name: "package.opf",
                data: br#"<package><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="a"/></spine></package>"#,
            },
            Entry {
                name: "a.xhtml",
                data: b"<html/>",
            },
        ];
        entries.push(Entry {
            name: "META-INF/encryption.xml",
            data: b"<encryption/>",
        });
        let bytes = epub(&entries);
        assert!(matches!(
            EpubBook::parse(&bytes, EpubLimits::default()),
            Err(EpubError::Unsupported(_))
        ));
    }
}

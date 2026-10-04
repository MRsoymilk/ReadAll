//! Lazy publication images. Header metadata is separate from the evictable RGBA cache.
use readall_epub::{EpubBook, ImageReference};
use readall_image::{ImageInfo, ImageLimits, RgbaImage, decode, probe};
use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    sync::Arc,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const CACHE_BYTES: usize = 64 * 1024 * 1024;
const CACHE_ENTRIES: usize = 64;

struct Source {
    reference: String,
    inline: Option<Vec<u8>>,
    info: Option<ImageInfo>,
    probed: bool,
    failed: bool,
}
struct Cached {
    source: usize,
    image: Arc<RgbaImage>,
}

pub(super) struct ImageStore<'book, 'archive> {
    book: &'book EpubBook<'archive>,
    spine: usize,
    sources: Vec<Source>,
    indices: Vec<usize>,
    cache: VecDeque<Cached>,
    cache_bytes: usize,
    max_cache_bytes: usize,
    warnings: usize,
    header_reads: usize,
    decode_attempts: usize,
}
impl<'book, 'archive> ImageStore<'book, 'archive> {
    pub(super) fn new(
        book: &'book EpubBook<'archive>,
        spine: usize,
        references: &[ImageReference],
    ) -> Self {
        let mut unique = HashMap::new();
        let mut sources = Vec::new();
        let mut indices = Vec::with_capacity(references.len());
        for reference in references {
            let index = *unique.entry(reference.source.as_str()).or_insert_with(|| {
                let index = sources.len();
                sources.push(Source {
                    reference: reference.source.clone(),
                    inline: reference
                        .inline_svg
                        .as_ref()
                        .map(|svg| svg.as_bytes().to_vec()),
                    info: None,
                    probed: false,
                    failed: false,
                });
                index
            });
            indices.push(index);
        }
        Self {
            book,
            spine,
            sources,
            indices,
            cache: VecDeque::new(),
            cache_bytes: 0,
            max_cache_bytes: CACHE_BYTES,
            warnings: 0,
            header_reads: 0,
            decode_attempts: 0,
        }
    }

    fn prefix(&self, source: usize, limit: usize, prefix: usize) -> Result<(Vec<u8>, usize)> {
        if let Some(bytes) = &self.sources[source].inline {
            if bytes.len() > limit {
                return Err("inline SVG resource limit exceeded".into());
            }
            return Ok((bytes[..bytes.len().min(prefix)].to_vec(), bytes.len()));
        }
        Ok(self.book.read_spine_resource_prefix(
            self.spine,
            &self.sources[source].reference,
            limit,
            prefix,
        )?)
    }
    fn bytes(&self, source: usize, limit: usize) -> Result<Vec<u8>> {
        if let Some(bytes) = &self.sources[source].inline {
            if bytes.len() > limit {
                return Err("inline SVG resource limit exceeded".into());
            }
            return Ok(bytes.clone());
        }
        Ok(self
            .book
            .read_spine_resource(self.spine, &self.sources[source].reference, limit)?)
    }
    /// Resolve dimensions before RGBA allocation; JPEG/SVG may need a larger bounded read.
    pub(super) fn info(&mut self, index: usize) -> Option<ImageInfo> {
        let source = *self.indices.get(index)?;
        if !self.sources[source].probed {
            self.sources[source].probed = true;
            self.header_reads += 1;
            let limits = ImageLimits::default();
            let result: Result<ImageInfo> = (|| {
                let (header, size) = self.prefix(source, limits.max_file_bytes, 33)?;
                match probe(&header, size, limits) {
                    Ok(info) => Ok(info),
                    Err(readall_image::ImageError::IncompleteHeader) => {
                        let (header, size) = self.prefix(source, limits.max_file_bytes, 4096)?;
                        match probe(&header, size, limits) {
                            Ok(info) => Ok(info),
                            Err(readall_image::ImageError::IncompleteHeader) => {
                                let bytes = self.bytes(source, limits.max_file_bytes)?;
                                Ok(probe(&bytes, bytes.len(), limits)?)
                            }
                            Err(error) => Err(error.into()),
                        }
                    }
                    Err(error) => Err(error.into()),
                }
            })();
            match result {
                Ok(info) => self.sources[source].info = Some(info),
                Err(error) => self.fail(source, error.as_ref()),
            }
        }
        self.sources[source].info
    }

    /// Decode only an image being painted. Evicted images can be decoded again
    /// when navigating backwards; crossing 64 resources is never a read failure.
    #[cfg(test)]
    pub(super) fn get(&mut self, index: usize) -> Option<Arc<RgbaImage>> {
        self.get_with_fonts(index, &[])
    }
    pub(super) fn get_with_fonts(
        &mut self,
        index: usize,
        fonts: &[&[u8]],
    ) -> Option<Arc<RgbaImage>> {
        let info = self.info(index)?;
        let source = self.indices[index];
        if self.sources[source].failed {
            return None;
        }
        if let Some(at) = self.cache.iter().position(|entry| entry.source == source) {
            let entry = self.cache.remove(at)?;
            let image = Arc::clone(&entry.image);
            self.cache.push_back(entry);
            return Some(image);
        }
        let required = info.rgba_bytes().ok()?;
        // Evict before allocating a new image. The cache cap is a residency cap,
        // not a lifetime total of bytes read across a chapter.
        while !self.cache.is_empty()
            && (self.cache.len() >= CACHE_ENTRIES
                || self.cache_bytes.saturating_add(required) > self.max_cache_bytes)
        {
            self.evict_oldest();
        }
        self.decode_attempts += 1;
        let result: Result<RgbaImage> = (|| {
            let limits = ImageLimits::default();
            let bytes = self.bytes(source, limits.max_file_bytes)?;
            let image = if info.format == readall_image::ImageFormat::Svg {
                let book = self.book;
                let spine = self.spine;
                let reference = &self.sources[source].reference;
                let inline = self.sources[source].inline.is_some();
                readall_image::decode_svg_with_resource_loader(&bytes, limits, fonts, &|href| {
                    if inline {
                        book.read_spine_resource(spine, href, limits.max_file_bytes)
                            .map_err(|error| error.to_string())
                    } else {
                        book.read_spine_nested_resource(
                            spine,
                            reference,
                            href,
                            limits.max_file_bytes,
                        )
                        .map_err(|error| error.to_string())
                    }
                })?
            } else {
                decode(&bytes, limits)?
            };
            if (image.width(), image.height()) != (info.width, info.height) {
                return Err("image dimensions changed between probe and decode".into());
            }
            Ok(image)
        })();
        match result {
            Ok(image) => {
                let image = Arc::new(image);
                if required <= self.max_cache_bytes {
                    self.cache_bytes += required;
                    self.cache.push_back(Cached {
                        source,
                        image: Arc::clone(&image),
                    });
                }
                Some(image)
            }
            Err(error) => {
                self.fail(source, error.as_ref());
                None
            }
        }
    }
    fn evict_oldest(&mut self) {
        if let Some(entry) = self.cache.pop_front() {
            self.cache_bytes -= entry.image.pixels().len();
        }
    }
    fn fail(&mut self, source: usize, error: &dyn Error) {
        if self.sources[source].failed {
            return;
        }
        self.sources[source].failed = true;
        self.warnings += 1;
        if self.warnings <= 32 {
            eprintln!(
                "ReadAll: EPUB chapter {}: image {:?} unavailable: {error}",
                self.spine + 1,
                self.sources[source].reference
            );
        } else if self.warnings == 33 {
            eprintln!(
                "ReadAll: EPUB chapter {}: additional image errors suppressed for this chapter",
                self.spine + 1
            );
        }
    }
    #[cfg(test)]
    pub(super) fn stats(&self) -> (usize, usize, usize, usize, usize) {
        (
            self.header_reads,
            self.decode_attempts,
            self.cache.len(),
            self.cache_bytes,
            self.warnings,
        )
    }
    #[cfg(test)]
    pub(super) fn set_cache_bytes(&mut self, bytes: usize) {
        self.max_cache_bytes = bytes;
        while self.cache_bytes > bytes {
            self.evict_oldest();
        }
    }
}

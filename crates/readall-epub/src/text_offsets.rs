//! Versioned whitespace-aware offsets. Existing epub-v1/v2 positions refer to
//! the old collapsed text; v3 positions refer to preserved code whitespace.
use crate::{ChapterContent, EpubError, EpubLocator, Result};
use readall_core::DocumentId;

impl ChapterContent {
    pub fn uses_preserved_whitespace(&self) -> bool {
        self.legacy_text.is_some()
    }

    pub(crate) fn locator(
        &self,
        book_id: DocumentId,
        spine_index: usize,
        offset: usize,
        image_index: Option<usize>,
    ) -> Result<EpubLocator> {
        if !self.text.is_char_boundary(offset) {
            return Err(EpubError::InvalidLocator(
                "offset is outside the chapter or inside a UTF-8 character",
            ));
        }
        if let Some(index) = image_index
            && self
                .images
                .get(index)
                .is_none_or(|image| image.offset != offset)
        {
            return Err(EpubError::InvalidLocator(
                "image index and canonical offset disagree",
            ));
        }
        Ok(EpubLocator {
            book_id,
            spine_index,
            utf8_offset: offset as u64,
            image_index,
            formatted: self.uses_preserved_whitespace(),
        })
    }

    /// Upgrade a position and optional range end in one loaded chapter. The caller
    /// must first verify that the locator belongs to this book and spine.
    /// Annotations use this to migrate each referenced chapter only once.
    pub fn normalize_range(
        &self,
        locator: &EpubLocator,
        end: Option<usize>,
    ) -> Result<(EpubLocator, Option<usize>)> {
        let offset = usize::try_from(locator.utf8_offset)
            .map_err(|_| EpubError::InvalidLocator("offset cannot fit this platform"))?;
        let mut current = self.translate_offset(locator.formatted, offset, false)?;
        if let Some(index) = locator.image_index {
            let image = self
                .images
                .get(index)
                .ok_or(EpubError::InvalidLocator("image index outside chapter"))?;
            if current != image.offset {
                // Legacy image anchors can lie at either end of a collapsed
                // whitespace sequence. Image identity resolves this ambiguity.
                let gap = current.min(image.offset)..current.max(image.offset);
                if locator.formatted
                    || self.legacy_text.is_none()
                    || !self.text.get(gap).is_some_and(|s| s.bytes().all(space))
                {
                    return Err(EpubError::InvalidLocator(
                        "image index and canonical offset disagree",
                    ));
                }
                current = image.offset;
            }
        }
        let end = end
            .map(|end| self.translate_offset(locator.formatted, end, true))
            .transpose()?;
        if end.is_some_and(|end| end <= current) || end.is_some() && locator.image_index.is_some() {
            return Err(EpubError::InvalidLocator("invalid annotation range"));
        }
        Ok((
            self.locator(
                locator.book_id,
                locator.spine_index,
                current,
                locator.image_index,
            )?,
            end,
        ))
    }
    fn translate_offset(&self, formatted: bool, offset: usize, range_end: bool) -> Result<usize> {
        if !formatted && let Some(legacy) = &self.legacy_text {
            return translate(legacy, &self.text, offset, range_end);
        }
        if !self.text.is_char_boundary(offset) {
            return Err(EpubError::InvalidLocator(
                "offset is outside the chapter or inside a UTF-8 character",
            ));
        }
        Ok(offset)
    }
}
fn space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// Linear, bounded conversion based on unchanged non-whitespace characters;
/// it never guesses from a fixed offset delta or uses quadratic text diffing.
fn translate(old: &str, new: &str, offset: usize, range_end: bool) -> Result<usize> {
    if !old.is_char_boundary(offset) {
        return Err(EpubError::InvalidLocator(
            "legacy offset is outside the chapter or inside a UTF-8 character",
        ));
    }
    if offset == 0 && !range_end {
        return Ok(0);
    }
    let (mut a, mut b) = (0, 0);
    loop {
        let (start_a, start_b) = (a, b);
        while a < old.len() && space(old.as_bytes()[a]) {
            a += 1;
        }
        while b < new.len() && space(new.as_bytes()[b]) {
            b += 1;
        }
        if offset < a {
            return Ok(start_b + (offset - start_a).min(b - start_b));
        }
        if offset == a {
            return Ok(if range_end && start_a == a {
                start_b
            } else {
                b
            });
        }
        let old_char = old[a..].chars().next().ok_or(EpubError::InvalidLocator(
            "legacy whitespace mapping failed",
        ))?;
        let new_char = new[b..].chars().next().ok_or(EpubError::InvalidLocator(
            "legacy whitespace mapping failed",
        ))?;
        if old_char != new_char {
            return Err(EpubError::InvalidLocator(
                "legacy text differs beyond whitespace",
            ));
        }
        a += old_char.len_utf8();
        b += new_char.len_utf8();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whitespace_mapping_preserves_unicode_and_range_affinity() {
        let old = "前文\n\n#define A 1 int f() { return A; }\n\n后文";
        let new = "前文\n\n#define A 1\nint f() {\n\treturn A;\n}\n\n后文";
        for word in ["#define", "int", "return", "后文"] {
            assert_eq!(
                translate(old, new, old.find(word).unwrap(), false).unwrap(),
                new.find(word).unwrap()
            );
        }
        let start = old.find("return").unwrap();
        let end = start + "return A;".len();
        assert_eq!(
            &new[translate(old, new, start, false).unwrap()
                ..translate(old, new, end, true).unwrap()],
            "return A;"
        );
        assert_eq!(translate("AB", "A\n\nB", 1, false).unwrap(), 3);
        assert_eq!(translate("AB", "A\n\nB", 1, true).unwrap(), 1);
        assert!(translate(old, new, 1, false).is_err());
        assert!(translate(old, new, old.len() + 1, false).is_err());
    }
}

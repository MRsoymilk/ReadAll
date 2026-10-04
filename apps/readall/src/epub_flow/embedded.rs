//! Publication-owned static fonts. Only families referenced by visible content
//! are considered; source failures are local and never replace the reader font.
use super::*;
use readall_epub::css::FontFace;
use readall_font::FontLimits;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILES: usize = 16;
const MAX_ATTEMPTS: usize = 64;

#[derive(Clone)]
pub(super) struct EmbeddedFace {
    pub(super) declaration: FontFace,
    pub(super) font: Font<'static>,
}
#[derive(Default)]
pub(super) struct EmbeddedFonts {
    pub(super) faces: Vec<EmbeddedFace>,
    #[cfg(test)]
    pub(super) loaded_bytes: usize,
    #[cfg(test)]
    pub(super) attempts: usize,
}
impl EmbeddedFonts {
    pub(super) fn load(book: &EpubBook<'_>, content: &mut ChapterContent) -> Self {
        let used: HashSet<String> = content
            .runs
            .iter()
            .filter(|run| !run.style.hidden)
            .flat_map(|run| {
                content
                    .font_families
                    .get(run.style.families)
                    .iter()
                    .cloned()
            })
            .collect();
        let mut result = Self::default();
        let mut cache: HashMap<String, Option<Font<'static>>> = HashMap::new();
        let (mut spent, mut files, mut attempts) = (0_usize, 0_usize, 0_usize);
        for declaration in &content.font_faces {
            if !used.contains(&declaration.family) {
                continue;
            }
            let mut chosen = None;
            for path in &declaration.sources {
                if let Some(previous) = cache.get(path) {
                    if let Some(font) = previous {
                        chosen = Some(font.clone());
                        break;
                    }
                    continue;
                }
                let loaded: Result<Font<'static>> = (|| {
                    if attempts >= MAX_ATTEMPTS || files >= MAX_FILES || spent >= MAX_BYTES {
                        return Err("embedded font chapter budget exceeded".into());
                    }
                    attempts += 1;
                    let bytes = book.read_font_resource(path, MAX_BYTES.saturating_sub(spent))?;
                    spent += bytes.len();
                    let font = Font::from_shared(
                        Arc::from(bytes),
                        0,
                        FontLimits {
                            max_file_bytes: 16 * 1024 * 1024,
                            ..FontLimits::default()
                        },
                    )?;
                    if font.metrics().ascender <= 0 || font.metrics().descender > 0 {
                        return Err("embedded font has unsupported horizontal metrics".into());
                    }
                    if rustybuzz::Face::from_slice(font.data(), font.face_index()).is_none() {
                        return Err("embedded font cannot be opened by the shaping engine".into());
                    }
                    Ok(font)
                })();
                match loaded {
                    Ok(font) => {
                        files += 1;
                        cache.insert(path.clone(), Some(font.clone()));
                        chosen = Some(font);
                        break;
                    }
                    Err(error) => {
                        // One diagnostic per source, with a bounded total even for hostile CSS.
                        if cache.len() < MAX_ATTEMPTS {
                            cache.insert(path.clone(), None);
                        }
                        if content.warnings.len() < 64 {
                            content
                                .warnings
                                .push(format!("embedded font {:?} ignored: {error}", path));
                        }
                    }
                }
            }
            if let Some(font) = chosen {
                result.faces.push(EmbeddedFace {
                    declaration: declaration.clone(),
                    font,
                });
            }
        }
        #[cfg(test)]
        {
            result.loaded_bytes = spent;
            result.attempts = attempts;
        }
        result
    }
}

//! Literal publication search with stable offsets and explicit work/result limits.
use crate::{EpubBook, EpubError, EpubLocator, Result};
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub locator: EpubLocator,
    pub end_offset: usize,
    pub excerpt: String,
}
#[derive(Debug, Clone, Copy)]
pub struct SearchLimits {
    pub max_hits: usize,
    pub max_text_bytes: usize,
}
impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            max_hits: 512,
            max_text_bytes: 32 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Default)]
pub struct SearchReport {
    pub hits: Vec<SearchHit>,
    pub chapters_scanned: usize,
    pub skipped_chapters: usize,
    pub truncated: bool,
}
impl EpubBook<'_> {
    /// `case_sensitive=false` folds ASCII letters only, preserving all UTF-8 offsets.
    /// Other Unicode characters are matched literally; no regex or remote resources.
    pub fn search(
        &self,
        query: &str,
        case_sensitive: bool,
        limits: SearchLimits,
    ) -> Result<SearchReport> {
        if query.is_empty()
            || query.len() > 1024
            || limits.max_hits == 0
            || limits.max_hits > 4096
            || limits.max_text_bytes > 128 * 1024 * 1024
        {
            return Err(EpubError::Invalid("invalid search query or limits"));
        }
        let mut report = SearchReport::default();
        let mut work = 0_usize;
        let mut match_work = 0_usize;
        let needle = if case_sensitive {
            query.to_owned()
        } else {
            query.to_ascii_lowercase()
        };
        for (spine, item) in self.spine().iter().enumerate() {
            if !item.linear() {
                continue;
            }
            let content = match self.read_spine_content(spine) {
                Ok(content) => content,
                Err(_) => {
                    report.skipped_chapters += 1;
                    continue;
                }
            };
            work = work.saturating_add(content.text.len());
            if work > limits.max_text_bytes {
                report.truncated = true;
                break;
            }
            report.chapters_scanned += 1;
            let haystack = if case_sensitive {
                content.text.clone()
            } else {
                content.text.to_ascii_lowercase()
            };
            for (offset, matched) in haystack.match_indices(&needle) {
                let end = offset + matched.len();
                match_work += 1;
                let first = content.runs.partition_point(|run| run.range.end <= offset);
                let mut hidden = false;
                for run in content.runs[first..]
                    .iter()
                    .take_while(|run| run.range.start < end)
                {
                    match_work += 1;
                    hidden |= run.style.hidden;
                }
                if match_work > 2_000_000 {
                    report.truncated = true;
                    return Ok(report);
                }
                if hidden {
                    continue;
                }
                if report.hits.len() >= limits.max_hits {
                    report.truncated = true;
                    return Ok(report);
                }
                let start_excerpt = content.text[..offset]
                    .char_indices()
                    .rev()
                    .nth(20)
                    .map_or(0, |(i, _)| i);
                let end_excerpt = content.text[end..]
                    .char_indices()
                    .nth(60)
                    .map_or(content.text.len(), |(i, _)| end + i);
                report.hits.push(SearchHit {
                    locator: EpubLocator {
                        book_id: self.id(),
                        spine_index: spine,
                        utf8_offset: offset as u64,
                        image_index: None,
                    },
                    end_offset: end,
                    excerpt: content.text[start_excerpt..end_excerpt].replace('\n', " "),
                });
            }
        }
        Ok(report)
    }
}

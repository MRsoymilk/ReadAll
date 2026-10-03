//! Transactional reading state. Reflow retains an exact content anchor, not the
//! start of whichever page happened to contain it on the previous resize.
use crate::text_page::{Options, PageRenderer, RenderedPage};
use readall_core::{TextDocument, TextLocator};
use readall_font::Font;
use std::error::Error;
type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Action {
    Next,
    Previous,
    First,
    Last,
    Larger,
    Smaller,
}
pub(crate) struct Session<'doc, 'font, 'bytes> {
    document: &'doc TextDocument,
    font: &'font Font<'bytes>,
    options: Options,
    renderer: PageRenderer<'font, 'bytes>,
    frame: RenderedPage,
    anchor: TextLocator,
}
impl<'doc, 'font, 'bytes> Session<'doc, 'font, 'bytes> {
    pub(crate) fn new(
        document: &'doc TextDocument,
        font: &'font Font<'bytes>,
        options: Options,
    ) -> Result<Self> {
        let mut renderer = PageRenderer::new(font, options.size, options.allow_missing)?;
        let frame = renderer.render(document, &options)?;
        let anchor = options.at.clone().unwrap_or_else(|| frame.locator.clone());
        Ok(Self {
            document,
            font,
            options,
            renderer,
            frame,
            anchor,
        })
    }
    pub(crate) fn frame(&self) -> &RenderedPage {
        &self.frame
    }
    pub(crate) fn anchor(&self) -> &TextLocator {
        &self.anchor
    }
    pub(crate) fn title(&self) -> String {
        format!(
            "ReadAll — {}/{} — {} px{}",
            self.frame.page + 1,
            self.frame.pages,
            self.options.size,
            if self.frame.missing.is_empty() {
                ""
            } else {
                " — missing glyph replacements"
            }
        )
    }
    pub(crate) fn resize(&mut self, width: u32, height: u32) -> Result<bool> {
        if (width, height) == (self.options.width, self.options.height) {
            return Ok(false);
        }
        let mut options = self.options.clone();
        options.width = width;
        options.height = height;
        self.reflow(options)
    }
    fn reflow(&mut self, mut options: Options) -> Result<bool> {
        options.page = None;
        options.at = Some(self.anchor.clone());
        if options.size != self.options.size || options.allow_missing != self.options.allow_missing
        {
            let mut renderer = PageRenderer::new(self.font, options.size, options.allow_missing)?;
            let frame = renderer.render(self.document, &options)?;
            self.renderer = renderer;
            self.options = options;
            self.frame = frame;
            return Ok(true);
        }

        let frame = self.renderer.render(self.document, &options)?;
        self.options = options;
        self.frame = frame;
        Ok(true)
    }
    #[cfg(test)]
    pub(crate) fn renderer_stats(&self) -> (usize, usize) {
        self.renderer.stats()
    }

    pub(crate) fn action(&mut self, action: Action) -> Result<bool> {
        let mut options = self.options.clone();
        let target = match action {
            Action::Next => (self.frame.page + 1).min(self.frame.pages - 1),
            Action::Previous => self.frame.page.saturating_sub(1),
            Action::First => 0,
            Action::Last => self.frame.pages - 1,
            Action::Larger | Action::Smaller => {
                let size = if matches!(action, Action::Larger) {
                    options.size.saturating_add(2).min(256)
                } else {
                    options.size.saturating_sub(2).max(8)
                };
                if size == options.size {
                    return Ok(false);
                }
                options.size = size;
                return self.reflow(options);
            }
        };
        if target == self.frame.page {
            return Ok(false);
        }
        options.page = Some(target);
        options.at = None;
        let frame = self.renderer.render(self.document, &options)?;
        self.anchor = frame.locator.clone();
        self.frame = frame;
        self.options = options;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "../tests/support/font.rs"]
mod test_font;
#[cfg(test)]
mod tests {
    use super::*;
    use readall_core::Limits;
    use readall_font::FontLimits;
    fn options() -> Options {
        Options::parse(
            &[
                "--font",
                "fixture.ttf",
                "--width",
                "200",
                "--height",
                "128",
                "--margin",
                "16",
            ]
            .map(Into::into),
        )
        .unwrap()
    }
    #[test]
    fn navigation_stays_within_the_document_and_updates_content_anchor() {
        let bytes = test_font::make_font();
        let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
        let doc =
            TextDocument::from_bytes("AAAA WWWW AAAA\n".repeat(30).as_bytes(), Limits::default())
                .unwrap();
        let mut session = Session::new(&doc, &font, options()).unwrap();
        assert!(!session.action(Action::Previous).unwrap());
        assert!(session.action(Action::Next).unwrap());
        assert_eq!(session.frame().page, 1);
        assert!(doc.restore(session.anchor()).unwrap() > 0);
        assert!(session.action(Action::Last).unwrap());
        assert_eq!(session.frame().page + 1, session.frame().pages);
        assert!(!session.action(Action::Next).unwrap());
        assert!(session.action(Action::First).unwrap());
        assert_eq!(doc.restore(session.anchor()).unwrap(), 0);
    }
    #[test]
    fn page_navigation_reuses_layout_and_glyph_cache() {
        let bytes = test_font::make_font();
        let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
        let doc =
            TextDocument::from_bytes("AAAA WWWW AAAA\n".repeat(30).as_bytes(), Limits::default())
                .unwrap();
        let mut session = Session::new(&doc, &font, options()).unwrap();
        let initial = session.renderer_stats();
        assert_eq!(initial.0, 1);

        assert!(session.action(Action::Next).unwrap());
        let after_next = session.renderer_stats();
        assert_eq!(after_next.0, 1);
        assert!(after_next.1 >= initial.1);

        assert!(session.action(Action::Previous).unwrap());
        let after_back = session.renderer_stats();
        assert_eq!(after_back.0, 1);
        assert_eq!(after_back.1, after_next.1);

        assert!(session.resize(240, 160).unwrap());
        assert_eq!(session.renderer_stats().0, 2);
    }

    #[test]
    fn repeated_reflow_preserves_exact_anchor_without_backwards_drift() {
        let bytes = test_font::make_font();
        let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
        let doc =
            TextDocument::from_bytes("AAAA WWWW AAAA\n".repeat(30).as_bytes(), Limits::default())
                .unwrap();
        let mut opts = options();
        opts.at = Some(doc.locator(101).unwrap());
        let mut session = Session::new(&doc, &font, opts).unwrap();
        let original = session.anchor().clone();
        let page = session.frame().page;
        for (w, h) in [(400, 200), (180, 160), (800, 600), (200, 128)] {
            session.resize(w, h).unwrap();
            assert_eq!(session.anchor(), &original);
        }
        assert_eq!(session.frame().page, page);
        session.action(Action::Larger).unwrap();
        session.action(Action::Smaller).unwrap();
        assert_eq!(session.anchor(), &original);
        assert_eq!(session.frame().page, page);
    }
    #[test]
    fn failed_reflow_leaves_frame_and_position_unchanged() {
        let bytes = test_font::make_font();
        let font = Font::parse(&bytes, 0, FontLimits::default()).unwrap();
        let doc = TextDocument::from_bytes(b"AAAA WWWW", Limits::default()).unwrap();
        let mut session = Session::new(&doc, &font, options()).unwrap();
        let original = session.frame().surface.pixels().to_vec();
        let anchor = session.anchor().clone();
        assert!(session.resize(0, u32::MAX).is_err());
        assert_eq!(session.frame().surface.pixels(), original);
        assert_eq!(session.anchor(), &anchor);
        assert!(!session.resize(200, 128).unwrap());
        assert!(session.title().contains("1/1"));
    }
}

//! Explicit external-link confirmation; no network/host action on parse, hover
//! or the first click. The same validated target is shown and passed as one argv.
use super::*;
use readall_platform::web_link::{OpenEvent, WebLink, open_browser};
use std::time::Duration;
type LaunchResult = std::result::Result<mpsc::Receiver<OpenEvent>, String>;
type Opener = fn(&WebLink) -> LaunchResult;

pub(super) struct ExternalLink {
    target: Option<WebLink>,
    scroll: usize,
    request: Option<mpsc::Receiver<OpenEvent>>,
    requested: Option<Instant>,
    submitted: bool,
    opener: Opener,
}
impl Default for ExternalLink {
    fn default() -> Self {
        Self {
            target: None,
            scroll: 0,
            request: None,
            requested: None,
            submitted: false,
            opener: open_browser,
        }
    }
}
impl ExternalLink {
    pub(super) fn pending(&self) -> bool {
        self.request.is_some()
    }
}

impl ReaderWindow<'_, '_, '_, '_> {
    pub(super) fn offer_external_link(&mut self, href: &str) -> WindowResult<bool> {
        match WebLink::parse(href) {
            Ok(link) => {
                self.tools.external.target = Some(link);
                self.tools.external.scroll = 0;
                self.tools.mode = Mode::External;
                self.tools.status = "确认后将使用系统默认浏览器打开，不改变阅读位置".into();
            }
            Err(error) => self.tools.status = format!("链接未打开：{error}"),
        }
        Ok(true)
    }
    fn close_external_link(&mut self) {
        self.tools.external.target = None;
        self.tools.mode = Mode::None;
        self.tools.status.clear();
    }
    fn confirm_external_link(&mut self) {
        let Some(link) = self.tools.external.target.take() else {
            return;
        };
        self.tools.mode = Mode::None;
        if let Some(effects) = &mut self.host_effects {
            if effects.len() >= 8 {
                self.tools.status = "系统操作队列已满".into();
                return;
            }
            effects.push_back(HostEffect::OpenUrl(link.as_str().to_owned()));
            self.tools.status = "正在请求系统浏览器打开…".into();
            return;
        }
        if self.tools.external.pending() {
            self.tools.status = "已有浏览器启动请求正在处理，请稍后再试".into();
            return;
        }
        match (self.tools.external.opener)(&link) {
            Ok(receiver) => {
                self.tools.external.request = Some(receiver);
                self.tools.external.requested = Some(Instant::now());
                self.tools.external.submitted = false;
                self.tools.status = "正在请求默认浏览器打开网页…".into();
            }
            Err(error) => self.tools.status = format!("链接打开失败：{error}"),
        }
    }
    pub(in super::super) fn poll_external_link(&mut self) -> bool {
        let mut changed = false;
        loop {
            let Some(receiver) = &self.tools.external.request else {
                return changed;
            };
            match receiver.try_recv() {
                Ok(OpenEvent::Submitted) => {
                    self.tools.external.submitted = true;
                    self.tools.status = "已请求默认浏览器打开网页；阅读位置保持不变".into();
                    changed = true;
                }
                Ok(OpenEvent::Finished(result)) => {
                    self.tools.external.request = None;
                    self.tools.external.requested = None;
                    self.tools.status = match result {
                        Ok(()) => "网页链接已交给默认浏览器；阅读位置保持不变".into(),
                        Err(error) => format!("链接打开失败：{error}"),
                    };
                    return true;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.tools.external.request = None;
                    self.tools.external.requested = None;
                    self.tools.status = "浏览器启动任务已结束，但未返回完成结果".into();
                    return true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        if self
            .tools
            .external
            .requested
            .is_some_and(|at| at.elapsed() >= Duration::from_secs(15))
        {
            // A host opener may legitimately remain alive with the browser. Stop
            // polling, but never terminate a process owned by the user's desktop.
            self.tools.external.request = None;
            self.tools.external.requested = None;
            self.tools.status = if self.tools.external.submitted {
                "已请求打开网页；系统启动器尚未返回，请检查浏览器"
            } else {
                "浏览器启动请求尚未返回结果，请检查桌面环境"
            }
            .into();
            return true;
        }
        changed
    }
    fn external_buttons(&self) -> (Rect, Rect) {
        let panel = self.tool_panel();
        let width = panel.width.saturating_sub(36) / 2;
        let y = panel.y + panel.height.saturating_sub(46) as i32;
        (
            Rect::new(panel.x + 12, y, width, 32),
            Rect::new(panel.x + 24 + width as i32, y, width, 32),
        )
    }
    pub(super) fn external_action(&mut self, action: Action) -> WindowResult<bool> {
        match action {
            Action::Close | Action::Back => self.close_external_link(),
            Action::Activate => self.confirm_external_link(),
            Action::Command(ReaderCommand::Copy) => {
                if let Some(target) = &self.tools.external.target {
                    self.start_clipboard(Some(target.as_str().to_owned()))?;
                }
            }
            Action::Next => {
                self.tools.external.scroll = self.tools.external.scroll.saturating_add(1)
            }
            Action::Previous => {
                self.tools.external.scroll = self.tools.external.scroll.saturating_sub(1)
            }
            Action::First => self.tools.external.scroll = 0,
            Action::Last => self.tools.external.scroll = usize::MAX,
            Action::Click { x, y } => {
                let (cancel, open) = self.external_buttons();
                if point_in(open, x, y) {
                    self.confirm_external_link();
                } else if point_in(cancel, x, y) || !point_in(self.tool_panel(), x, y) {
                    self.close_external_link();
                }
            }
            Action::PointerMove { .. } | Action::PointerRelease { .. } | Action::PointerLeave => {
                return Ok(false);
            }
            _ => {}
        }
        Ok(true)
    }
    pub(super) fn draw_external_link(&mut self) -> WindowResult<()> {
        let Some(target) = self.tools.external.target.clone() else {
            return Ok(());
        };
        let panel = self.tool_panel();
        let (cancel, open) = self.external_buttons();
        let (background, ink) = self.session.settings().theme.colors();
        self.surface.draw(&[
            DrawCommand::FillRect {
                rect: Rect::new(
                    0,
                    32,
                    self.surface.width(),
                    self.surface.height().saturating_sub(32),
                ),
                color: Color::rgba(0, 0, 0, 120),
            },
            DrawCommand::FillRect {
                rect: panel,
                color: background,
            },
            DrawCommand::FillRect {
                rect: cancel,
                color: Color::rgba(110, 130, 155, 45),
            },
            DrawCommand::FillRect {
                rect: open,
                color: Color::rgba(48, 101, 184, 255),
            },
        ])?;
        let mut text = UiPainter::new(&self.ui_font, &mut self.surface)?;
        let width = panel.width.saturating_sub(28);
        text.draw_clipped(panel.x + 14, panel.y + 12, 17, "打开外部网页？", ink, panel)?;
        let origin = text.fit(13, &format!("目标：{}", target.origin()), width)?;
        text.draw_clipped(panel.x + 14, panel.y + 40, 13, &origin, ink, panel)?;
        let lines = wrap_address(&text, target.as_str(), width)?;
        let rows = (panel.height.saturating_sub(154) / 18).max(1) as usize;
        self.tools.external.scroll = self
            .tools
            .external
            .scroll
            .min(lines.len().saturating_sub(rows));
        let address_clip = Rect::new(
            panel.x + 14,
            panel.y + 66,
            width,
            panel.height.saturating_sub(142),
        );
        for (i, line) in lines
            .iter()
            .skip(self.tools.external.scroll)
            .take(rows)
            .enumerate()
        {
            text.draw_clipped(
                panel.x + 14,
                panel.y + 66 + i as i32 * 18,
                12,
                line,
                ink,
                address_clip,
            )?;
        }
        let hint = format!(
            "↑↓ 查看网址 · Ctrl+C 复制 · Enter 打开 · Esc 取消  {}/{}",
            self.tools.external.scroll + 1,
            lines.len().max(1)
        );
        let hint = text.fit(11, &hint, width)?;
        text.draw_clipped(panel.x + 14, cancel.y - 26, 11, &hint, ink, panel)?;
        text.draw_clipped(cancel.x + 10, cancel.y + 8, 12, "取消", ink, cancel)?;
        text.draw_clipped(
            open.x + 10,
            open.y + 8,
            12,
            "在浏览器打开",
            Color::WHITE,
            open,
        )?;
        Ok(())
    }
}
fn wrap_address(text: &UiPainter<'_, '_>, address: &str, width: u32) -> WindowResult<Vec<String>> {
    let mut lines = Vec::new();
    let (mut line, mut used) = (String::new(), 0_u32);
    for ch in address.chars() {
        let advance = text.measure(12, &ch.to_string())?;
        if !line.is_empty() && used.saturating_add(advance) > width {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        line.push(ch);
        used = used.saturating_add(advance);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::super::link_tests::{book, click_link, with_reader};
    use super::*;
    fn accepted(_: &WebLink) -> LaunchResult {
        let (tx, rx) = mpsc::channel();
        tx.send(OpenEvent::Submitted).unwrap();
        tx.send(OpenEvent::Finished(Ok(()))).unwrap();
        Ok(rx)
    }
    fn rejected(_: &WebLink) -> LaunchResult {
        Err("mock missing xdg-open".into())
    }
    #[test]
    fn external_click_prompts_cancel_and_confirm_never_change_reading_position() {
        let bytes = book(&[
            "<html><body><p>AAAA</p><p><a href='https://example.invalid/path?a=1&amp;b=2#x'>W</a></p></body></html>",
        ]);
        with_reader(bytes, |reader| {
            let anchor = reader.session.anchor().clone();
            let pixels = reader.session.frame().surface.pixels().to_vec();
            reader.tools.external.opener = accepted; // Never launches a real browser.
            click_link(reader, "https://example.invalid/path?a=1&b=2#x");
            assert_eq!(reader.tools.mode, Mode::External);
            assert!(!reader.tools.pending());
            reader.action(Action::Next).unwrap();
            assert_eq!(reader.session.anchor(), &anchor);
            reader.action(Action::Close).unwrap();
            assert_eq!(reader.tools.mode, Mode::None);
            assert!(!reader.close_requested);
            click_link(reader, "https://example.invalid/path?a=1&b=2#x");
            reader.action(Action::Activate).unwrap();
            assert!(reader.tools.pending());
            reader.animation_tick().unwrap();
            assert!(!reader.tools.pending());
            assert!(reader.tools.status.contains("默认浏览器"));
            assert_eq!(reader.session.anchor(), &anchor);
            assert_eq!(reader.session.frame().surface.pixels(), pixels);
            assert!(reader.tools.link_history.is_empty());
        });
    }
    #[test]
    fn failed_launch_is_visible_and_cannot_turn_a_page() {
        with_reader(
            book(&[
                "<html><body><p>AAAA</p><p><a href='http://example.invalid'>W</a></p></body></html>",
            ]),
            |reader| {
                reader.tools.external.opener = rejected;
                let anchor = reader.session.anchor().clone();
                click_link(reader, "http://example.invalid");
                let (_, open) = reader.external_buttons();
                reader
                    .action(Action::Click {
                        x: open.x + 4,
                        y: open.y + 4,
                    })
                    .unwrap();
                assert!(reader.tools.status.contains("mock missing"));
                assert_eq!(reader.session.anchor(), &anchor);
                assert!(!reader.tools.pending());
            },
        );
    }
    #[test]
    fn long_addresses_wrap_losslessly_and_modal_buttons_fit_small_windows() {
        with_reader(book(&["<html><body>AAAA</body></html>"]), |reader| {
            reader.resize(256, 256).unwrap();
            let address = format!("https://example.invalid/?q={}", "abc%20".repeat(200));
            reader.offer_external_link(&address).unwrap();
            reader.refresh_surface().unwrap();
            let url = reader
                .tools
                .external
                .target
                .as_ref()
                .unwrap()
                .as_str()
                .to_owned();
            let width = reader.tool_panel().width - 28;
            let text = UiPainter::new(&reader.ui_font, &mut reader.surface).unwrap();
            assert_eq!(wrap_address(&text, &url, width).unwrap().concat(), url);
            reader.action(Action::Last).unwrap();
            assert!(reader.tools.external.scroll > 0);
            let panel = reader.tool_panel();
            for rect in [reader.external_buttons().0, reader.external_buttons().1] {
                assert_eq!(rect.intersection(panel), rect);
            }
            reader.action(Action::Back).unwrap();
            assert!(!reader.close_requested);
        });
    }
}

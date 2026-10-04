//! Explicitly confirmed HTTP(S) links only. Parsing has no I/O; launching never
//! feeds publication text to a shell. xdg-open runs outside the Wayland/UI thread.
use std::{
    io,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebLink {
    url: Url,
}
impl WebLink {
    /// Scheme-relative web links use HTTPS; all other non-HTTP(S) schemes remain
    /// book-local or unsupported and must never be sent to the host opener.
    pub fn is_web_reference(href: &str) -> bool {
        let href = href.trim();
        href.starts_with("//")
            || href.split_once(':').is_some_and(|(scheme, _)| {
                scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
            })
    }
    pub fn parse(href: &str) -> Result<Self, &'static str> {
        if href.len() > 4096 || href.chars().any(|ch| ch.is_control() || matches!(ch, '\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')) || href.contains('\\') {
            return Err("网址过长，或包含控制字符/反斜杠");
        }
        let href = href.trim();
        let normalized = if href.starts_with("//") {
            format!("https:{href}")
        } else {
            href.to_owned()
        };
        let (scheme, rest) = normalized
            .split_once("://")
            .ok_or("网址必须以 http:// 或 https:// 开头")?;
        if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https")
            || rest.is_empty()
            || rest.starts_with(['/', '?', '#'])
        {
            return Err("仅允许带有有效主机名的 HTTP/HTTPS 网页链接");
        }
        let mut chars = normalized.bytes();
        while let Some(ch) = chars.next() {
            if ch == b'%' {
                let a = chars
                    .next()
                    .and_then(|v| (v as char).to_digit(16))
                    .ok_or("网址包含无效的百分号编码")?;
                let b = chars
                    .next()
                    .and_then(|v| (v as char).to_digit(16))
                    .ok_or("网址包含无效的百分号编码")?;
                if a * 16 + b < 32 || a * 16 + b == 127 {
                    return Err("网址包含编码后的控制字符");
                }
            }
        }
        let url = Url::parse(&normalized).map_err(|_| "网址格式无效")?;
        if url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || normalized
                .split("://")
                .nth(1)
                .unwrap_or("")
                .split(['/', '?', '#'])
                .next()
                .unwrap_or("")
                .contains('@')
        {
            return Err("不允许空主机名或带有用户名/密码的网址");
        }
        if url.as_str().len() > 4096 {
            return Err("规范化后的网址过长");
        }
        Ok(Self { url })
    }
    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }
    /// ASCII/punycode origin is displayed separately to avoid hiding the host in
    /// a long URL or trusting the visible label supplied by the publication.
    pub fn origin(&self) -> String {
        self.url.origin().ascii_serialization()
    }
}

#[derive(Debug)]
pub enum OpenEvent {
    Submitted,
    Finished(Result<(), String>),
}
static LAUNCHERS: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        LAUNCHERS.fetch_sub(1, Ordering::AcqRel);
    }
}
fn browser_command(link: &WebLink) -> Command {
    let mut command = Command::new("xdg-open");
    command
        .arg(link.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}
/// At most four host launchers remain alive at once. Some xdg-open backends stay
/// alive with the browser; reap them, but never kill a user's browser on timeout.
pub fn open_browser(link: &WebLink) -> Result<mpsc::Receiver<OpenEvent>, String> {
    LAUNCHERS
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < 4).then_some(n + 1)
        })
        .map_err(|_| "系统浏览器启动请求仍在处理中，请稍后再试".to_owned())?;
    let permit = Permit;
    let link = link.clone();
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("readall-open-link".into())
        .spawn(move || {
            let _permit = permit;
            let result = match browser_command(&link).spawn() {
                Ok(mut child) => {
                    let _ = tx.send(OpenEvent::Submitted);
                    child
                        .wait()
                        .map_err(|error| format!("等待系统启动器失败：{error}"))
                        .and_then(|status| {
                            if status.success() {
                                Ok(())
                            } else {
                                Err(format!(
                                    "系统启动器退出异常（{status}）；请检查默认浏览器设置"
                                ))
                            }
                        })
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    Err("找不到 xdg-open；请安装 xdg-utils 并设置默认浏览器".into())
                }
                Err(error) => Err(format!("无法启动默认浏览器：{error}")),
            };
            let _ = tx.send(OpenEvent::Finished(result));
        })
        .map_err(|error| format!("无法创建浏览器启动任务：{error}"))?;
    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn web_urls_keep_queries_fragments_ports_and_unicode_paths() {
        for (source, target) in [
            (
                " HTTPS://Example.COM:443/文章?a=1&b=2#note ",
                "https://example.com/%E6%96%87%E7%AB%A0?a=1&b=2#note",
            ),
            ("//example.com/path", "https://example.com/path"),
            ("http://[::1]:8080/a", "http://[::1]:8080/a"),
            ("https://例子.测试/a", "https://xn--fsqu00a.xn--0zwm56d/a"),
        ] {
            assert!(WebLink::is_web_reference(source));
            assert_eq!(WebLink::parse(source).unwrap().as_str(), target);
        }
        assert_eq!(
            WebLink::parse("https://example.com:8443/path")
                .unwrap()
                .origin(),
            "https://example.com:8443"
        );
    }
    #[test]
    fn dangerous_or_ambiguous_uris_never_reach_the_host_opener() {
        for source in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,a",
            "mailto:x@y.test",
            "--help",
            "chapter.xhtml#x",
            "http:foo",
            "https:///host",
            "https://",
            "https://user:pass@host",
            "https://@host",
            "https://user%40name@host",
            "https://host:99999",
            "https://host/a\n--help",
            "https://host/\\x",
            "https://host/%0A",
            "https://host/%7f",
            "https://host/%GG",
            "https://host/%",
            "https://host/\u{202e}test",
        ] {
            assert!(WebLink::parse(source).is_err(), "accepted: {source:?}");
        }
        assert!(WebLink::parse(&format!("https://host/{}", "x".repeat(4096))).is_err());
        assert!(!WebLink::is_web_reference("chapter.xhtml#note"));
    }
    #[test]
    fn one_argument_command_keeps_shell_metacharacters_in_url_data() {
        let link =
            WebLink::parse("https://example.invalid/?q=$(touch%20never)&next=;test").unwrap();
        let command = browser_command(&link);
        assert_eq!(command.get_program(), "xdg-open");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [std::ffi::OsStr::new(link.as_str())]
        );
        // Do not execute xdg-open or open a real browser during tests.
    }
}

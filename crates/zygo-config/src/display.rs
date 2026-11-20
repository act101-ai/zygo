//! Output forms that never show URL userinfo (constraint C3; the precedent is spec §8.2 step 1, which drops userinfo).
//! Display only: the config keeps the source as written.

use regex::Regex;
use std::sync::LazyLock;

fn valid_scheme(scheme: &str) -> bool {
    let mut cs = scheme.chars();
    cs.next().is_some_and(|c| c.is_ascii_alphabetic()) && cs.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

/// A local path: `/…`, `./`, `../`, `.\`, `..\`, `~/`, a UNC path `\\…`, or a drive letter followed by `\` or `/` (`C:\…`, `C:/…`).
/// `C:a@b` (drive-relative) is not local: it cannot be told apart from `u:T@h`.
fn is_local(s: &str) -> bool {
    let b = s.as_bytes();
    ["/", "./", "../", ".\\", "..\\", "~/", "\\\\"].iter().any(|p| s.starts_with(p))
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/'))
}

/// A marketplace `source` without userinfo. One fail-closed rule:
/// 1. a local path (see `is_local`) is returned unchanged;
/// 2. else, if `s` holds `@`, everything through its LAST `@` is removed. A leading `<scheme>://` (scheme `[A-Za-z][A-Za-z0-9+.-]*`)
///    that ends before that `@` is kept: `https://u:T@h/x` -> `https://h/x`; otherwise only the tail is kept: `bob:T@h:path://x` -> `h:path://x`,
///    ` git+https://u:T@h/o` -> `h/o`. Any `@` counts, so a path `@` can mangle the host (`https://h/o/r@v1` -> `https://v1`); a mangled host leaks nothing;
/// 3. else `s` is returned unchanged (`github:o/r`, empty).
pub fn display_source(s: &str) -> String {
    if is_local(s) { return s.to_string(); }
    let Some(at) = s.rfind('@') else { return s.to_string() };
    let keep = s.find("://").filter(|&p| p + 3 <= at && valid_scheme(&s[..p])).map_or(0, |p| p + 3);
    format!("{}{}", &s[..keep], &s[at + 1..])
}

static URL_USERINFO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"([A-Za-z][A-Za-z0-9+.-]*://)[^\s'"`]*@"#).expect("static pattern"));

/// Free text (git stderr) with the userinfo of every `<scheme>://` token removed. A token runs to the next whitespace, `'`, `"` or backtick.
/// scp form is not touched: ssh has no password slot and git prints `git@host` as a login name.
/// Limit: userinfo that holds whitespace or a quote cannot be told from the text around it, so its tail stays; git does not print such a URL unquoted.
pub fn scrub_userinfo(text: &str) -> String {
    URL_USERINFO.replace_all(text, "$1").into_owned()
}

/// A verb error of several lines, each escaped by `new`; it is shown as it is (N72).
#[derive(Debug)]
pub struct EscapedLines(String);

impl EscapedLines {
    /// Each line escaped with `zygo_core::esc`, then joined with `\n`.
    pub fn new<S: AsRef<str>>(lines: impl IntoIterator<Item = S>) -> Self {
        EscapedLines(lines.into_iter().map(|l| zygo_core::esc(l.as_ref())).collect::<Vec<_>>().join("\n"))
    }
}
impl std::fmt::Display for EscapedLines {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}
impl std::error::Error for EscapedLines {}

/// The text of a verb error, for the CLI and for a slash reply: `EscapedLines` as it is, any other error escaped to one line.
pub fn error_text(e: &anyhow::Error) -> String {
    match e.downcast_ref::<EscapedLines>() {
        Some(l) => l.0.clone(),
        None => zygo_core::esc(&e.to_string()),
    }
}

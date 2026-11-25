//! The public check of a remote (S§8.2): the URL map, the anonymous probe, the verdict lists and the 1 h cache.

use crate::env::Verdict;
use crate::git::has_text;
use zygo_config::display::scrub_userinfo;
use zygo_work::repo::Whole;

/// S§8.2 step 2: a remote URL mapped to the `https` URL the probe asks, a local path, or a scheme with no `https` form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mapped { Https(String), Local, Unknown }

/// S§8.2 step 2: map a remote URL as git reports it to its `https` form; user info and an `ssh` port are dropped.
pub fn map_url(url: &str) -> Mapped {
    let s = url.trim();
    if let Some(i) = s.find("://") {
        let scheme = &s[..i];
        let rest = &s[i + 3..];
        let (auth, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        let host = auth.rsplit('@').next().unwrap_or("");
        if scheme.eq_ignore_ascii_case("file") {
            return Mapped::Local;
        }
        let https = scheme.eq_ignore_ascii_case("https");
        if !https && !scheme.eq_ignore_ascii_case("ssh") {
            return Mapped::Unknown;
        }
        let host = if https { host } else { drop_port(host) };
        if host.is_empty() {
            return Mapped::Unknown;
        }
        return if https {
            Mapped::Https(format!("https://{host}{path}"))
        } else {
            Mapped::Https(format!("https://{host}/{}", path.trim_start_matches('/')))
        };
    }
    if let Some(i) = s.find("::") {
        if !s[..i].contains('/') {
            return Mapped::Unknown; // a transport helper, `<transport>::<address>`
        }
    }
    let Some((h, rest)) = s.split_once(':') else { return Mapped::Local };
    if h.contains('/') {
        return Mapped::Local;
    }
    if h.len() == 1 && h.as_bytes()[0].is_ascii_alphabetic() && rest.starts_with(['/', '\\']) {
        return Mapped::Local; // Windows drive path `C:/…`; `h:p` stays scp form
    }
    let host = h.rsplit('@').next().unwrap_or("");
    if host.is_empty() {
        return Mapped::Unknown;
    }
    Mapped::Https(format!("https://{host}/{}", rest.trim_start_matches('/')))
}

/// `host` without a trailing `:<digits>`.
fn drop_port(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => host,
    }
}

/// S§8.2 step 4, S§18: the stderr text of an anonymous `git ls-remote` that means the remote is private.
pub const PRIVATE_TEXT: &[&str] = &[
    "could not read Username",
    "could not read Password",
    "Authentication failed",
    "Repository not found",
    "' not found",
    "returned error: 401",
    "returned error: 404",
    "HTTP Basic: Access denied",
    "could not be found or you don't have permission",
];

/// S§8.2 step 4: the verdict of one anonymous `git ls-remote`.
pub fn classify(w: &Whole) -> Verdict {
    if w.ok {
        return Verdict::Public;
    }
    if has_text(&w.err, PRIVATE_TEXT) {
        return Verdict::Private;
    }
    let err = String::from_utf8_lossy(&w.err);
    match err.lines().map(str::trim).find(|l| !l.is_empty()) {
        Some(line) => Verdict::Unknown(format!("git ls-remote failed: {}", scrub_userinfo(line))),
        None => Verdict::Unknown("git ls-remote failed".into()),
    }
}

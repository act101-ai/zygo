//! The public check of a remote (S§8.2): the URL map, the anonymous probe, the verdict lists and the 1 h cache.

use crate::env::{Clock, Probe, Verdict};
use crate::git::{has_text, sync_git};
use crate::state::{load, save, PublicCache, SyncPaths};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant, UNIX_EPOCH};
use zygo_config::display::scrub_userinfo;
use zygo_work::repo::{run_whole, RunError, Whole};

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

/// S§8.2 step 3: the anonymous `git ls-remote --heads <https_url>`: no credential helper, no ask-pass, no global or
/// system config, `HOME` an empty folder, no prompt; proxy variables stay as the environment has them.
pub fn probe_command(https_url: &str, empty_home: &Path) -> Command {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut c = sync_git(empty_home);
    c.args(["-c", "credential.helper=", "-c", "core.askPass=", "ls-remote", "--heads", https_url]);
    c.env("GIT_CONFIG_GLOBAL", null).env("GIT_CONFIG_SYSTEM", null).env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", empty_home).env("GIT_TERMINAL_PROMPT", "0").env_remove("GIT_ASKPASS").env_remove("SSH_ASKPASS");
    c
}

/// S§8.2 step 3, S-C9: the production probe, an anonymous `git ls-remote` killed after 10 s.
#[derive(Debug, Clone, Copy, Default)]
pub struct LsRemote;

impl Probe for LsRemote {
    fn probe(&self, https_url: &str) -> Verdict {
        let Ok(dir) = tempfile::TempDir::new() else {
            return Verdict::Unknown("cannot make an empty home folder".into());
        };
        match run_whole(probe_command(https_url, dir.path()), Instant::now() + Duration::from_secs(10)) {
            Ok(w) => classify(&w),
            Err(RunError::TimedOut) => Verdict::Unknown("git ls-remote timed out".into()),
            Err(RunError::NotRun) => Verdict::Unknown("git ls-remote did not start".into()),
            Err(RunError::Failed | RunError::Absent) => Verdict::Unknown("git ls-remote failed".into()),
        }
    }
}

/// S§8.2 step 1: the fetch URL of `remote`, then its push URL when that differs.
pub fn remote_urls(main_root: &Path, remote: &str, deadline: Instant) -> Result<Vec<String>, RunError> {
    let one = |args: &[&str]| -> Result<String, RunError> {
        let w = crate::git::run(main_root, args, deadline)?;
        let out = String::from_utf8(w.out).map_err(|_| RunError::Failed)?;
        let url = out.trim_end();
        if !w.ok || url.is_empty() {
            return Err(RunError::Failed);
        }
        Ok(url.to_string())
    };
    let fetch = one(&["remote", "get-url", remote])?;
    let push = one(&["remote", "get-url", "--push", remote])?;
    Ok(if push == fetch { vec![fetch] } else { vec![fetch, push] })
}

/// S§8.2: how long a cached verdict stands, in seconds of the injected clock (1 h).
const CACHE_SECS: u64 = 3600;

/// S§8.2, S§13 limit 7: the verdict of the remote with fetch and push URLs `urls`, from the cache when it is under
/// 1 h old and holds the same URLs, else from `probe` on each distinct `https` form. The caller holds
/// `mail/sync/lock` (S§15: the pass under the lock is the only writer of `state.json`).
pub fn check(sp: &SyncPaths, urls: &[String], probe: &dyn Probe, clock: &dyn Clock) -> Verdict {
    let now = clock.now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut keys: Vec<String> = urls.iter()
        .map(|u| hex::encode(Sha256::digest(scrub_userinfo(u).as_bytes()))).collect();
    keys.sort();
    keys.dedup();
    let state = load(sp);
    if let Ok(Some(c)) = state.as_ref().map(|s| s.public.as_ref()) {
        if c.at <= now && now - c.at <= CACHE_SECS && c.url_sha256 == keys {
            match c.verdict.as_str() {
                "public" => return Verdict::Public,
                "private" => return Verdict::Private,
                "local" => return Verdict::Local,
                "unknown" => return Verdict::Unknown("the last check, under 1 h ago, could not tell".into()),
                _ => {}
            }
        }
    }
    let verdict = fold(urls, probe);
    if let Ok(mut s) = state {
        let name = match verdict {
            Verdict::Public => "public",
            Verdict::Private => "private",
            Verdict::Local => "local",
            Verdict::Unknown(_) => "unknown",
        };
        s.public = Some(PublicCache { url_sha256: keys, verdict: name.into(), at: now });
        let _ = save(sp, &s); // a failed save leaves the verdict as it is
    }
    verdict
}

/// S§8.2 steps 1, 2 and 4: probe each distinct `https` form once; any `Public` wins, else the first `Unknown`,
/// else any `Private`, else `Local`.
fn fold(urls: &[String], probe: &dyn Probe) -> Verdict {
    if urls.is_empty() {
        return Verdict::Unknown("the remote has no URL".into());
    }
    let mut asked: Vec<String> = Vec::new();
    let mut verdicts = Vec::new();
    for u in urls {
        match map_url(u) {
            Mapped::Local => verdicts.push(Verdict::Local),
            Mapped::Unknown => verdicts.push(Verdict::Unknown("the remote URL has no https form".into())),
            Mapped::Https(h) if !asked.contains(&h) => {
                verdicts.push(probe.probe(&h));
                asked.push(h);
            }
            Mapped::Https(_) => {}
        }
    }
    if verdicts.contains(&Verdict::Public) {
        return Verdict::Public;
    }
    if let Some(u) = verdicts.iter().find(|v| matches!(v, Verdict::Unknown(_))) {
        return u.clone();
    }
    if verdicts.contains(&Verdict::Private) { Verdict::Private } else { Verdict::Local }
}

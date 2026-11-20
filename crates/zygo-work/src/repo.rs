//! Repository id (§8.2 step 1) and bounded, pinned `git` calls.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use regex::Regex;

/// Total budget of one public operation, shared by all its child commands.
pub(crate) const BOUND: Duration = Duration::from_secs(2);

/// A checkout: its top directory, the shared git directory and the main worktree (all canonical),
/// plus the deadline of the operation that found it.
pub struct Checkout {
    pub top: PathBuf,
    pub common_dir: PathBuf,
    pub main_root: PathBuf,
    pub deadline: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunError {
    NotRun,
    TimedOut,
    Failed,
    /// A `git` exit with a `zygo_config::repo_file::GIT_ABSENT` message: the thing asked for is not there.
    Absent,
}

impl RunError {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            RunError::NotRun => "tool not found",
            RunError::TimedOut => "timed out",
            RunError::Failed => "failed",
            RunError::Absent => "absent",
        }
    }
}

/// Runs `cmd` with stdin null and returns its stdout; see [`run_bounded_err`].
pub(crate) fn run_bounded(cmd: Command, deadline: Instant) -> Result<Vec<u8>, RunError> {
    run_bounded_err(cmd, deadline).map_err(|(e, _)| e)
}

/// Runs `cmd` with stdin null and returns its stdout; on a non-zero exit, `Failed` with up to 64 KiB of its stderr (the
/// rest is drained); every other error has empty stderr. At the deadline the child is killed and reaped. The drain
/// threads are abandoned then (a grandchild may still hold the pipes): a bounded leak per timeout, never joined.
pub(crate) fn run_bounded_err(mut cmd: Command, deadline: Instant) -> Result<Vec<u8>, (RunError, Vec<u8>)> {
    if Instant::now() >= deadline {
        return Err((RunError::TimedOut, Vec::new()));
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|_| (RunError::NotRun, Vec::new()))?;
    let (Some(mut out), Some(mut err)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err((RunError::Failed, Vec::new()));
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let (etx, erx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut err).take(64 * 1024).read_to_end(&mut buf);
        let _ = std::io::copy(&mut err, &mut std::io::sink());
        let _ = etx.send(buf);
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err((RunError::TimedOut, Vec::new()));
            }
        }
    };
    let left = || deadline.saturating_duration_since(Instant::now());
    let buf = rx.recv_timeout(left()).map_err(|_| (RunError::TimedOut, Vec::new()))?;
    if status.success() {
        Ok(buf)
    } else {
        Err((RunError::Failed, erx.recv_timeout(left()).unwrap_or_default()))
    }
}

/// `git -c core.fsmonitor=false -C <cwd> <args>` with `LC_ALL=C`: stdout text; `Absent` for a non-zero exit with a
/// `GIT_ABSENT` message, `Failed` for other exits and for stdout that is not UTF-8. The caller's repository-selecting
/// and config-injecting `GIT_*` variables are removed first.
pub(crate) fn git_run(cwd: &Path, args: &[&str], deadline: Instant) -> Result<String, RunError> {
    let mut c = zygo_config::repo_file::git_command();
    c.args(["-c", "core.fsmonitor=false", "-C"]).arg(cwd).args(args);
    c.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0");
    match run_bounded_err(c, deadline) {
        Ok(out) => String::from_utf8(out).map_err(|_| RunError::Failed),
        Err((RunError::Failed, err)) if zygo_config::repo_file::git_absent(&String::from_utf8_lossy(&err)) => {
            Err(RunError::Absent)
        }
        Err((e, _)) => Err(e),
    }
}

/// [`git_run`]: none on any failure.
pub(crate) fn git(cwd: &Path, args: &[&str], deadline: Instant) -> Option<String> {
    git_run(cwd, args, deadline).ok()
}

/// The checkout holding `cwd`: `Ok(None)` when git says it is not a repository; `Err((call, why))` when a call
/// (`rev-parse` or `worktree list`) fails or its output does not name an existing path.
pub(crate) fn find_checkout(cwd: &Path, deadline: Instant) -> Result<Option<Checkout>, (&'static str, RunError)> {
    let out = match git_run(cwd, &["rev-parse", "--path-format=absolute", "--show-toplevel", "--git-common-dir"], deadline) {
        Ok(out) => out,
        Err(RunError::Absent) => return Ok(None),
        Err(e) => return Err(("rev-parse", e)),
    };
    let bad = |call| (call, RunError::Failed);
    let canon = |l: Option<&str>, call| l.and_then(|l| Path::new(l).canonicalize().ok()).ok_or(bad(call));
    let mut lines = out.lines();
    let top = canon(lines.next(), "rev-parse")?;
    let common_dir = canon(lines.next(), "rev-parse")?;
    // After `rev-parse` found the checkout, an "absent" answer is a failed read, not a reason of its own.
    let list = git_run(cwd, &["worktree", "list", "--porcelain"], deadline)
        .map_err(|e| ("worktree list", if e == RunError::Absent { RunError::Failed } else { e }))?;
    let main_root = canon(list.lines().find_map(|l| l.strip_prefix("worktree ")), "worktree list")?;
    Ok(Some(Checkout { top, common_dir, main_root, deadline }))
}

pub fn checkout(cwd: &Path, deadline: Instant) -> Option<Checkout> {
    find_checkout(cwd, deadline).ok().flatten()
}

pub fn branch(cwd: &Path, deadline: Instant) -> Option<String> {
    let b = git(cwd, &["symbolic-ref", "--short", "-q", "HEAD"], deadline)?;
    let b = b.trim();
    (!b.is_empty()).then(|| b.to_string())
}

fn drop_port(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => host,
    }
}

pub fn normalize_remote(url: &str) -> Option<String> {
    let u = url.trim();
    if !u.is_ascii() {
        return None;
    }
    let (host, path) = if let Some(i) = u.find("://") {
        if u[..i].eq_ignore_ascii_case("file") {
            return None;
        }
        let rest = &u[i + 3..];
        let (auth, path) = rest.split_at(rest.find('/')?);
        (drop_port(auth.rsplit('@').next()?), path)
    } else {
        let (h, p) = u.split_once(':')?;
        if h.contains('/') || h.starts_with('.') {
            return None; // a local path, not scp form
        }
        let host = h.rsplit('@').next()?;
        if host.len() == 1 && host.as_bytes()[0].is_ascii_alphabetic() {
            return None; // Windows drive path
        }
        (host, p)
    };
    let joined = format!("{host}/{}", path.trim_start_matches('/')).to_ascii_lowercase();
    let s = joined.trim_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s).trim_matches('/');
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[a-z0-9.-]+(/[a-z0-9._-]+)+$").unwrap());
    let dots = s.split('/').any(|seg| seg == "." || seg == "..");
    (s.len() <= 128 && !dots && re.is_match(s)).then(|| s.to_string())
}

pub fn repository_id(cwd: &Path) -> Option<String> {
    repository_id_with_budget(cwd, BOUND)
}

/// [`repository_id`] with a different total budget. For tests only.
#[doc(hidden)]
pub fn repository_id_with_budget(cwd: &Path, budget: Duration) -> Option<String> {
    repository_id_by(cwd, Instant::now() + budget)
}

pub(crate) fn repository_id_by(cwd: &Path, deadline: Instant) -> Option<String> {
    let out = git(cwd, &["config", "--local", "--get-all", "remote.origin.url"], deadline)?;
    normalize_remote(out.lines().next()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // N53: a failed read is told from "not a repository"; `checkout` keeps its result
    fn find_checkout_tells_timeout_from_not_a_repository() {
        let ws = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
        assert_eq!(find_checkout(&ws, Instant::now() + BOUND).unwrap().unwrap().top, ws);
        assert!(find_checkout(Path::new("/"), Instant::now() + BOUND).unwrap().is_none());
        assert_eq!(find_checkout(&ws, Instant::now()).err(), Some(("rev-parse", RunError::TimedOut)));
        assert!(checkout(&ws, Instant::now()).is_none());
    }
}

//! Repository id (§8.2 step 1) and bounded, pinned `git` calls.

use std::ffi::OsStr;
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
pub(crate) fn run_bounded_err(cmd: Command, deadline: Instant) -> Result<Vec<u8>, (RunError, Vec<u8>)> {
    match run_bounded_out(cmd, deadline, None) {
        Ok(Ran { ok: true, out, .. }) => Ok(out),
        Ok(Ran { err, .. }) => Err((RunError::Failed, err)),
        Err(e) => Err((e, Vec::new())),
    }
}

/// The end of a bounded run that exited: exit success, stdout (at most the cut), whether stdout passed the cut,
/// and up to 64 KiB of stderr (read only on a non-zero exit, else empty). In the unix cut mode, a child that the kernel
/// reaped (SIGCHLD ignored) has an unknown status, and `ok` is true.
struct Ran {
    ok: bool,
    out: Vec<u8>,
    cut: bool,
    err: Vec<u8>,
}

/// Runs `cmd` with stdin null until it exits or `deadline`. `cut: None` is the plain mode of [`run_bounded_err`].
/// `Some(n)`: stdout keeps `n` bytes (the rest is drained) and `Ran.cut` tells whether there were more. The child is
/// also killed with its descendants: on unix it runs in its own process group, and the group is killed at the deadline
/// and once the child has exited, before it is reaped. On Windows the process tree is killed at the deadline.
fn run_bounded_out(mut cmd: Command, deadline: Instant, cut: Option<u64>) -> Result<Ran, RunError> {
    if Instant::now() >= deadline {
        return Err(RunError::TimedOut);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    if cut.is_some() {
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    }
    let mut child = cmd.spawn().map_err(|_| RunError::NotRun)?;
    let (Some(mut out), Some(mut err)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(RunError::Failed);
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let limit = cut.map_or(u64::MAX, |n| n.saturating_add(1));
        let mut buf = Vec::new();
        let _ = (&mut out).take(limit).read_to_end(&mut buf);
        let mut over = false;
        if let Some(n) = cut
            && buf.len() as u64 > n
        {
            over = true;
            buf.truncate(n as usize);
        }
        let _ = std::io::copy(&mut out, &mut std::io::sink());
        let _ = tx.send((buf, over));
    });
    let (etx, erx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut err).take(64 * 1024).read_to_end(&mut buf);
        let _ = std::io::copy(&mut err, &mut std::io::sink());
        let _ = etx.send(buf);
    });
    // `None`: the kernel reaped the child (unix cut mode, SIGCHLD ignored), so it has exited with an unknown status.
    #[cfg_attr(not(unix), allow(unused_labels))] // the label is used by the unix block only
    let status: Option<std::process::ExitStatus> = 'wait: {
        // Unix cut mode: kill the group before the leader is reaped (author's call 4). The first `waitid` error
        // (e.g. ECHILD under an ignored SIGCHLD) falls through to the plain loop, with no group kill after it.
        #[cfg(unix)]
        if cut.is_some() {
            loop {
                match zygo_config::repo_file::exited_unreaped(&child) {
                    Ok(true) => {
                        zygo_config::repo_file::kill_group(&child);
                        match child.wait() {
                            Ok(s) => break 'wait Some(s),
                            Err(_) => return Err(RunError::Failed),
                        }
                    }
                    Ok(false) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                    Ok(false) => {
                        zygo_config::repo_file::kill_tree(&mut child);
                        return Err(RunError::TimedOut);
                    }
                    Err(_) => break,
                }
            }
        }
        loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                // Unix cut mode with SIGCHLD ignored: the kernel reaped the child, so it has exited. No kill, because
                // its PID may already be reused; stdout is read as usual.
                #[cfg(unix)]
                Err(e) if cut.is_some() && e.raw_os_error() == Some(libc::ECHILD) => break None,
                _ => {
                    if cfg!(windows) && cut.is_some() {
                        zygo_config::repo_file::kill_tree(&mut child);
                    } else {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    return Err(RunError::TimedOut);
                }
            }
        }
    };
    let left = || deadline.saturating_duration_since(Instant::now());
    let (out, over) = rx.recv_timeout(left()).map_err(|_| RunError::TimedOut)?;
    // An unknown status counts as success: the caller's checks of the output decide.
    let ok = status.is_none_or(|s| s.success());
    let err = if ok { Vec::new() } else { erx.recv_timeout(left()).unwrap_or_default() };
    Ok(Ran { ok, out, cut: over, err })
}

/// `<program> -c core.fsmonitor=false -C <cwd> <args>` on `git_command_for`, with `GIT_OPTIONAL_LOCKS=0` and
/// `GIT_TERMINAL_PROMPT=0`.
fn git_cmd(program: &OsStr, cwd: &Path, args: &[&str]) -> Command {
    let mut c = zygo_config::repo_file::git_command_for(program);
    c.args(["-c", "core.fsmonitor=false", "-C"]).arg(cwd).args(args);
    c.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0");
    c
}

/// `git -c core.fsmonitor=false -C <cwd> <args>` with `LC_ALL=C`: stdout text; `Absent` for a non-zero exit with a
/// `GIT_ABSENT` message, `Failed` for other exits and for stdout that is not UTF-8. The caller's repository-selecting
/// and config-injecting `GIT_*` variables are removed first.
pub(crate) fn git_run(cwd: &Path, args: &[&str], deadline: Instant) -> Result<String, RunError> {
    let c = git_cmd(OsStr::new("git"), cwd, args);
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

/// Total budget cap of [`hud_git_with`]: no `Instant` arithmetic can overflow (C6).
const HUD_BUDGET_CAP: Duration = Duration::from_secs(24 * 60 * 60);
/// Stdout kept of each HUD git call (H§7.5).
const HUD_CUT: u64 = 4 * 1024;

/// The HUD's git facts (H§7.5): `top`, `git_dir` and `common_dir` are canonical; a linked worktree has
/// `git_dir != common_dir`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HudGit {
    pub top: PathBuf,
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
    /// The fourth `rev-parse` line (`--symbolic-full-name HEAD`) without `refs/heads/`; `None` when detached
    /// (`HEAD`), empty, or not under `refs/heads/`.
    pub branch: Option<String>,
    /// Both `None` with no upstream, any `rev-list` error, or output past the cut.
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
}

/// [`hud_git_with`] with the program `git`.
pub fn hud_git(cwd: &Path, budget: Duration) -> Option<HudGit> {
    hud_git_with(Path::new("git"), cwd, budget)
}

/// The two HUD git calls (H§7.5) in `cwd` within `budget` (`rev-parse` gets 3/8 of it), each with
/// `GIT_NO_LAZY_FETCH=1` and at most 4 KiB of stdout. On unix each call runs in its own process group, which is
/// killed at the deadline and when the call exits. Exception (SIGCHLD ignored, so the kernel reaps git): after the
/// first `waitid` error the call is watched with `try_wait` only and no group kill runs, because the PID may be
/// reused; at the deadline only the child itself is killed, and ECHILD from `try_wait` counts as an exit with an
/// unknown status, whose output is then checked as usual. On Windows the call's process tree is killed at the deadline.
/// None when `rev-parse` fails, times out, passes the cut, or does not print four lines that start with three
/// existing paths. Exception: with SIGCHLD ignored the exit status is lost, so a failed `rev-parse` whose output still
/// passes those checks counts. An unborn HEAD then shows the worktree with no branch (as a detached HEAD does).
pub fn hud_git_with(git: &Path, cwd: &Path, budget: Duration) -> Option<HudGit> {
    let budget = budget.min(HUD_BUDGET_CAP);
    let start = Instant::now();
    let deadline = start + budget;
    let rev_parse_deadline = start + budget / 8 * 3;
    let run = |args: &[&str], deadline: Instant| {
        let mut c = git_cmd(git.as_os_str(), cwd, args);
        c.env("GIT_NO_LAZY_FETCH", "1");
        run_bounded_out(c, deadline, Some(HUD_CUT))
    };

    let args = ["rev-parse", "--path-format=absolute", "--show-toplevel", "--git-dir", "--git-common-dir", "--symbolic-full-name", "HEAD"];
    let Ran { ok: true, out, cut: false, .. } = run(&args, rev_parse_deadline).ok()? else { return None };
    let text = out.strip_suffix(b"\n").unwrap_or(&out);
    let lines: Vec<&[u8]> = text.split(|&b| b == b'\n').collect();
    let [top, git_dir, common_dir, head] = lines[..] else { return None };
    let path = |l: &[u8]| -> Option<PathBuf> {
        #[cfg(unix)]
        let p = PathBuf::from(<OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(l));
        #[cfg(not(unix))]
        let p = PathBuf::from(String::from_utf8_lossy(l).into_owned());
        p.canonicalize().ok()
    };
    let (top, git_dir, common_dir) = (path(top)?, path(git_dir)?, path(common_dir)?);
    // `refs/heads/<name>` on a branch, also when a tag has the same name (`--abbrev-ref` would give `heads/<name>`);
    // `HEAD` when detached.
    let branch = head.strip_prefix(b"refs/heads/").filter(|b| !b.is_empty()).map(|b| String::from_utf8_lossy(b).into_owned());

    // `--` ends the revisions: a file named like the range cannot make the argument ambiguous.
    let marks = match run(&["rev-list", "--left-right", "--count", "HEAD...@{upstream}", "--"], deadline) {
        Ok(Ran { ok: true, out, cut: false, .. }) => std::str::from_utf8(&out).ok().and_then(|s| {
            let (a, b) = s.trim_end().split_once('\t')?;
            Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))
        }),
        _ => None,
    };
    let (ahead, behind) = marks.map_or((None, None), |(a, b)| (Some(a), Some(b)));
    Some(HudGit { top, git_dir, common_dir, branch, ahead, behind })
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
        // A one-hour deadline never expires here, so machine load cannot turn the two reads into timeouts.
        let far = Instant::now() + Duration::from_secs(3600);
        assert_eq!(find_checkout(&ws, far).unwrap().unwrap().top, ws);
        assert!(find_checkout(Path::new("/"), far).unwrap().is_none());
        assert_eq!(find_checkout(&ws, Instant::now()).err(), Some(("rev-parse", RunError::TimedOut)));
        assert!(checkout(&ws, Instant::now()).is_none());
    }
}

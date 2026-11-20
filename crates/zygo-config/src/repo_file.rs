//! The repository file `.zygo.toml`, read from the default branch (§13.5).

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use toml::{Table, Value};

use crate::schema::Config;

#[derive(Debug, Clone, PartialEq)]
pub enum RepoFile {
    Absent,
    /// A `git` call did not finish: `why` is `git <call>: <reason>` (§13.5 note 2026-11-19). Counts as absent.
    Unread { why: String },
    Invalid { branch: String, why: String },
    Found { branch: String, table: Table, ignored: Vec<String> },
}

/// A multi-line error as one line.
pub(crate) fn one_line(e: &dyn std::fmt::Display) -> String {
    e.to_string().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A TOML syntax error of `text` as `TOML parse error at line L, column C: <message>`. The error's own text quotes the
/// source line, which can hold a secret (C3); only its message and position are kept.
pub fn syntax_error(text: &str, message: &str, span: Option<std::ops::Range<usize>>) -> String {
    let msg = one_line(&message);
    let Some(at) = span.map(|r| r.start.min(text.len())) else { return format!("TOML parse error: {msg}") };
    let before = text.get(..at).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    format!("TOML parse error at line {line}, column {column}: {msg}")
}

const KEPT: [&str; 4] = ["guards", "navigation", "limits", "work"];
const GIT_BOUND: Duration = Duration::from_secs(2);

/// The stderr messages (under `LC_ALL=C`) of a `git` call that exits non-zero because the thing asked for is not there:
/// no repository, no `origin/HEAD` symbolic ref, no such branch, no `.zygo.toml` in it. Any other failure is a failed read.
pub const GIT_ABSENT: [&str; 5] = [
    "not a git repository (or any",
    "is not a symbolic ref",
    "invalid object name",
    "does not exist in",
    "exists on disk, but not in",
];

/// git's second line when repository discovery stops at a mount point (the first: `not a git repository (or any parent
/// up to mount point <dir>)`).
const BOUNDARY: &str = "Stopping at filesystem boundary (GIT_DISCOVERY_ACROSS_FILESYSTEM not set).";

/// True when `stderr` of a non-zero `git` exit is exactly one line holding one of [`GIT_ABSENT`], or exactly the
/// mount-point pair (that first line, then [`BOUNDARY`]). Any other line (e.g. `warning: ignoring broken ref …`) makes
/// it a failed read (fail closed).
pub fn git_absent(stderr: &str) -> bool {
    let text = stderr.trim_end_matches(['\n', '\r']);
    match text.split_once('\n') {
        None => GIT_ABSENT.iter().any(|m| text.contains(m)),
        Some((first, second)) => {
            first.contains("not a git repository (or any parent up to mount point ") && second.trim_end_matches('\r') == BOUNDARY
        }
    }
}

/// The caller's repository-selecting variables: a `git` child never gets them, so `-C <dir>` alone picks the repository.
pub const GIT_ENV_CLEARED: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_COMMON_DIR",
];

/// `git` with [`GIT_ENV_CLEARED`] and every `GIT_CONFIG*` variable removed and `LC_ALL=C` (the [`GIT_ABSENT`] messages
/// are matched in English); environment on the child only.
pub fn git_command() -> Command {
    let mut c = Command::new("git");
    for k in GIT_ENV_CLEARED {
        c.env_remove(k);
    }
    for (k, _) in std::env::vars_os() {
        if k.to_str().is_some_and(|k| k.starts_with("GIT_CONFIG")) {
            c.env_remove(k);
        }
    }
    c.env("LC_ALL", "C");
    c
}

/// Why a `git` call gave no stdout: the thing is absent (git's one stderr line), or the read failed (a fixed reason word).
enum GitFail {
    Absent(String),
    Read(&'static str),
}

/// A finished `git` call: exit code (none when killed by a signal), stdout and up to 64 KiB of stderr.
struct GitOut {
    code: Option<i32>,
    out: Vec<u8>,
    err: Vec<u8>,
}

/// `git -C <repo> <args>` with `LC_ALL=C`: stdout on success; [`GitFail::Absent`] for a non-zero exit with a
/// [`GIT_ABSENT`] message; else [`GitFail::Read`].
fn git(repo: &Path, args: &[&str], deadline: Instant) -> Result<String, GitFail> {
    let o = git_out(repo, args, deadline).map_err(GitFail::Read)?;
    if o.code != Some(0) {
        let err = String::from_utf8_lossy(&o.err).into_owned();
        return Err(if git_absent(&err) { GitFail::Absent(err) } else { GitFail::Read("failed") });
    }
    String::from_utf8(o.out).map_err(|_| GitFail::Read("output is not UTF-8"))
}

/// `git -C <repo> <args>` with `LC_ALL=C`, run to its end: a reason word when it does not start, does not finish by
/// `deadline` (the child is killed and its pipe readers, which a grandchild may hold open, are left behind) or its
/// stdout cannot be read.
fn git_out(repo: &Path, args: &[&str], deadline: Instant) -> Result<GitOut, &'static str> {
    let timed_out = "timed out";
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(timed_out);
    }
    let mut child = git_command()
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "tool not found")?;
    let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("failed");
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = tx.send(stdout.read_to_end(&mut b).map(|_| b));
    });
    let (etx, erx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = (&mut stderr).take(64 * 1024).read_to_end(&mut b);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        let _ = etx.send(b);
    });
    let kill = |child: &mut std::process::Child| {
        let _ = child.kill();
        let _ = child.wait();
    };
    let Ok(out) = rx.recv_timeout(left) else {
        kill(&mut child);
        return Err(timed_out);
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                kill(&mut child);
                return Err(timed_out);
            }
        }
    };
    let err = erx.recv_timeout(deadline.saturating_duration_since(Instant::now())).map_err(|_| timed_out)?;
    Ok(GitOut { code: status.code(), out: out.map_err(|_| "failed")?, err })
}

/// `git show` said `.zygo.toml` or the branch is absent (`said`, git's one stderr line); a corrupt object or ref can say
/// the same, so a second call under the same deadline confirms it. `None`: absent; `Some(reason)`: a failed read.
fn confirm_absent(repo: &Path, branch: &str, said: &str, deadline: Instant) -> Option<&'static str> {
    let (args, absent): (Vec<&str>, fn(&GitOut) -> bool) = if said.contains("invalid object name") {
        // No such ref: exit 1, nothing printed. A ref to a missing object resolves (exit 0); a broken one warns.
        (vec!["rev-parse", "--verify", "--quiet", branch], |o| o.code == Some(1) && o.out.is_empty() && o.err.is_empty())
    } else if said.contains("does not exist in") || said.contains("exists on disk, but not in") {
        // The branch's tree is readable and has no `.zygo.toml`: exit 0, nothing printed.
        (vec!["ls-tree", "--name-only", branch, "--", ".zygo.toml"], |o| o.code == Some(0) && o.out.is_empty() && o.err.is_empty())
    } else {
        // `not a git repository`: `repo` is no repository (the hooks find the checkout first and judge a `.git` entry).
        return None;
    };
    match git_out(repo, &args, deadline) {
        Ok(o) if absent(&o) => None,
        Ok(_) => Some("failed"),
        Err(w) => Some(w),
    }
}

/// The `git` calls share one 2 s budget.
pub fn read_default_branch(repo: &Path) -> RepoFile {
    let deadline = Instant::now() + GIT_BOUND;
    let branch = match git(repo, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"], deadline) {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        Ok(_) | Err(GitFail::Absent(_)) => "main".into(),
        Err(GitFail::Read(w)) => return RepoFile::Unread { why: format!("git symbolic-ref: {w}") },
    };
    let text = match git(repo, &["show", &format!("{branch}:.zygo.toml")], deadline) {
        Ok(t) => t,
        Err(GitFail::Absent(said)) => match confirm_absent(repo, &branch, &said, deadline) {
            None => return RepoFile::Absent,
            Some(w) => return RepoFile::Unread { why: format!("git show: {w}") },
        },
        Err(GitFail::Read(w)) => return RepoFile::Unread { why: format!("git show: {w}") },
    };
    let invalid = |why: String| RepoFile::Invalid { branch: branch.clone(), why };
    let parsed: Table = match text.parse() {
        Ok(t) => t,
        Err(e) => return invalid(syntax_error(&text, e.message(), e.span())),
    };
    let mut table = Table::new();
    let mut ignored = Vec::new();
    for (k, v) in parsed {
        if k == "version" {
            if v.as_integer() != Some(1) {
                return invalid("version is not 1".into());
            }
            table.insert(k, v);
        } else if KEPT.contains(&k.as_str()) {
            table.insert(k, v);
        } else {
            ignored.push(k);
        }
    }
    let mut kept = table.clone();
    kept.remove("version");
    // `load` drops `guards.min_level` with a warning; a bad value must not invalidate the file.
    if let Some(g) = kept.get_mut("guards").and_then(Value::as_table_mut) {
        g.remove("min_level");
    }
    if let Err(e) = crate::typed::from_value::<Config>(Value::Table(kept)) {
        return invalid(e.describe(Some(&text)));
    }
    RepoFile::Found { branch, table, ignored }
}

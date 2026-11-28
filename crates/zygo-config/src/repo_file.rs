//! The repository file `.zygo.toml`, read from the default branch (§13.5).

use std::ffi::OsStr;
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
    git_command_for(OsStr::new("git"))
}

/// [`git_command`] with `program` in place of `git` (a test's fake `git`).
pub fn git_command_for(program: &OsStr) -> Command {
    let mut c = Command::new(program);
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

/// Kills `child`'s process group (unix: `libc::kill(-(child.id() as i32), libc::SIGKILL)`; the child must have been
/// started with `process_group(0)` and not yet reaped) or its process tree (Windows: `taskkill /T /F /PID <id>`,
/// stdout and stderr null; a descendant that left the tree survives). Does not reap.
pub fn kill_group(child: &std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// [`kill_group`], then `child.kill()` and `child.wait()`, both results ignored.
pub fn kill_tree(child: &mut std::process::Child) {
    kill_group(child);
    let _ = child.kill();
    let _ = child.wait();
}

/// Whether `child` has exited, without reaping it: with `info` zeroed, `waitid(P_PID, child.id(), &mut info,
/// WEXITED | WNOHANG | WNOWAIT)`. A return of 0 gives `Ok(info.si_pid() != 0)`. A return of -1 gives
/// `Err(std::io::Error::last_os_error())`, e.g. `ECHILD` when SIGCHLD is ignored and the kernel reaps by itself.
#[cfg(unix)]
pub fn exited_unreaped(child: &std::process::Child) -> std::io::Result<bool> {
    // SAFETY: `siginfo_t` is plain data, valid when zeroed; `waitid` writes only into `info`.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let r = unsafe {
        libc::waitid(libc::P_PID, child.id() as libc::id_t, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT)
    };
    if r == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `waitid` returned 0, so `info` is either zeroed (no state change) or filled for a child.
    Ok(unsafe { info.si_pid() } != 0)
}

/// Why a `git` call gave no stdout: the thing is absent (git's one stderr line), or the read failed (a fixed reason word).
enum GitFail {
    Absent(String),
    Read(&'static str),
}

/// A finished `git` call: exit status, stdout and up to 64 KiB of stderr.
pub(crate) struct GitOut {
    pub(crate) status: std::process::ExitStatus,
    pub(crate) out: Vec<u8>,
    pub(crate) err: Vec<u8>,
}

/// `git -C <repo> <args>` with `LC_ALL=C`: stdout on success; [`GitFail::Absent`] for a non-zero exit with a
/// [`GIT_ABSENT`] message; else [`GitFail::Read`].
fn git(repo: &Path, args: &[&str], deadline: Instant) -> Result<String, GitFail> {
    let o = git_out(repo, args, deadline).map_err(GitFail::Read)?;
    if o.status.code() != Some(0) {
        let err = String::from_utf8_lossy(&o.err).into_owned();
        return Err(if git_absent(&err) { GitFail::Absent(err) } else { GitFail::Read("failed") });
    }
    String::from_utf8(o.out).map_err(|_| GitFail::Read("output is not UTF-8"))
}

/// `git -C <repo> <args>` with `LC_ALL=C`, run to its end: a reason word when it does not start, does not finish by
/// `deadline` (the child is killed and its pipe readers, which a grandchild may hold open, are left behind) or its
/// stdout cannot be read.
pub(crate) fn git_out(repo: &Path, args: &[&str], deadline: Instant) -> Result<GitOut, &'static str> {
    git_out_with(git_command(), repo, args, deadline)
}

/// [`git_out`] with `git` as the command (a caller's [`git_command`] with more environment set).
pub(crate) fn git_out_with(mut git: Command, repo: &Path, args: &[&str], deadline: Instant) -> Result<GitOut, &'static str> {
    let timed_out = "timed out";
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(timed_out);
    }
    let mut child = git
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
    Ok(GitOut { status, out: out.map_err(|_| "failed")?, err })
}

/// `git show` said `.zygo.toml` or the branch is absent (`said`, git's one stderr line); a corrupt object or ref can say
/// the same, so a second call under the same deadline confirms it. `None`: absent; `Some(reason)`: a failed read.
fn confirm_absent(repo: &Path, branch: &str, said: &str, deadline: Instant) -> Option<&'static str> {
    let (args, absent): (Vec<&str>, fn(&GitOut) -> bool) = if said.contains("invalid object name") {
        // No such ref: exit 1, nothing printed. A ref to a missing object resolves (exit 0); a broken one warns.
        (vec!["rev-parse", "--verify", "--quiet", branch], |o| o.status.code() == Some(1) && o.out.is_empty() && o.err.is_empty())
    } else if said.contains("does not exist in") || said.contains("exists on disk, but not in") {
        // The branch's tree is readable and has no `.zygo.toml`: exit 0, nothing printed.
        (vec!["ls-tree", "--name-only", branch, "--", ".zygo.toml"], |o| o.status.code() == Some(0) && o.out.is_empty() && o.err.is_empty())
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
    from_text(branch, &text)
}

/// The repository file read from `branch` with contents `text`: parsed, filtered to the kept tables and validated.
pub fn from_text(branch: String, text: &str) -> RepoFile {
    let invalid = |why: String| RepoFile::Invalid { branch: branch.clone(), why };
    let parsed: Table = match text.parse() {
        Ok(t) => t,
        Err(e) => return invalid(syntax_error(text, e.message(), e.span())),
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
    // `load` drops `guards.min_level` and `limits.max_members` with a warning; a bad value must not invalidate the file.
    if let Some(g) = kept.get_mut("guards").and_then(Value::as_table_mut) {
        g.remove("min_level");
    }
    if let Some(l) = kept.get_mut("limits").and_then(Value::as_table_mut) {
        l.remove("max_members");
    }
    if let Err(e) = crate::typed::from_value::<Config>(Value::Table(kept)) {
        return invalid(e.describe(Some(text)));
    }
    RepoFile::Found { branch, table, ignored }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use std::io::BufRead;
    use std::os::unix::process::CommandExt;

    /// No process `pid`, or a zombie not yet reaped by init.
    fn gone(pid: &str) -> bool {
        let o = Command::new("ps").args(["-o", "stat=", "-p", pid]).output().unwrap();
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        s.is_empty() || s.starts_with('Z')
    }

    #[test] // kill_group ends the child's whole group and does not reap the child
    #[ignore = "timing: polls for the grandchild exit with a 2 s bound"]
    fn kill_group_kills_the_childs_group() {
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 30 & echo $!; wait"]).stdin(Stdio::null()).stdout(Stdio::piped());
        c.process_group(0);
        let mut child = c.spawn().unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let grandchild = line.trim().to_string();
        assert!(!grandchild.is_empty() && !gone(&grandchild), "grandchild {grandchild:?} did not start");
        kill_group(&child);
        let status = child.wait().expect("kill_group must not reap the child");
        assert_eq!(std::os::unix::process::ExitStatusExt::signal(&status), Some(libc::SIGKILL));
        let until = Instant::now() + Duration::from_secs(2);
        while !gone(&grandchild) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(gone(&grandchild), "grandchild {grandchild} still runs");
    }

    #[test] // exited_unreaped: false while the child runs, true after it exits, and the status stays for wait
    #[ignore = "timing: polls for the child exit with a 5 s bound"]
    fn exited_unreaped_tells_an_exit_without_reaping() {
        let mut running = Command::new("sleep").arg("30").spawn().unwrap();
        assert!(!exited_unreaped(&running).unwrap());
        let _ = running.kill();
        let _ = running.wait();

        let mut child = Command::new("sh").args(["-c", "exit 3"]).spawn().unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !exited_unreaped(&child).unwrap() {
            assert!(Instant::now() < until, "child did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(exited_unreaped(&child).unwrap(), "a second look must still see the exit: not reaped");
        assert_eq!(child.try_wait().unwrap().and_then(|s| s.code()), Some(3));
    }
}

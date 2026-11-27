//! Workspace id, name slug and collision suffix (AU2), main worktree (AU77), validation (AU84). Owner: node W1 (W§5).

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use zygo_work::repo::Checkout;

/// Longest workspace name (AU2).
const NAME_MAX: usize = 32;

/// The workspace id (W§5): the first 16 hex characters of the SHA-256 of the bytes of `common_dir`. The path is used
/// as given; the caller gives the canonical `Checkout.common_dir`.
pub fn workspace_id(common_dir: &Path) -> String {
    let mut id = hex::encode(Sha256::digest(common_dir.as_os_str().as_encoded_bytes()));
    id.truncate(16);
    id
}

/// The name slug (AU2): ASCII letters in lower case, each other character outside `[a-z0-9-]` as `-`, runs of `-` as
/// one, no `-` at either end, at most 32 characters with no `-` at the end; `ws` when nothing is left.
pub fn slug(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    for c in name.chars() {
        let c = c.to_ascii_lowercase();
        let c = if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { '-' };
        if c != '-' || !s.ends_with('-') {
            s.push(c);
        }
    }
    let s: String = s.trim_matches('-').chars().take(NAME_MAX).collect();
    let s = s.trim_end_matches('-');
    if s.is_empty() { "ws".to_string() } else { s.to_string() }
}

/// `slug` when it is free; else the first free `<part>-<n>` for `n` = 2, 3, …, where `part` is the start of `slug`
/// cut so that the name has at most 32 characters, with no `-` at its end (AU2).
pub fn unique_name(slug: &str, taken: &dyn Fn(&str) -> bool) -> String {
    if !taken(slug) {
        return slug.to_string();
    }
    let mut n: u64 = 2;
    loop {
        let suffix = format!("-{n}");
        let part: String = slug.chars().take(NAME_MAX.saturating_sub(suffix.len())).collect();
        let candidate = format!("{}{suffix}", part.trim_end_matches('-'));
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// `^k[a-z2-7]{8}$` on the whole string (AU84).
pub fn valid_pane_key(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 9 && b[0] == b'k' && b[1..].iter().all(|c| matches!(c, b'a'..=b'z' | b'2'..=b'7'))
}

/// `^[0-9a-f]{16}$` on the whole string (AU84).
pub fn valid_ws_id(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// `^[a-z0-9-]{1,32}$` on the whole string (AU84).
pub fn valid_name(s: &str) -> bool {
    (1..=NAME_MAX).contains(&s.len()) && s.bytes().all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

/// `^zygo-[a-z0-9-]{1,32}$` on the whole string, and equal to `zygo-<name>` (AU84).
pub fn valid_tmux_session(s: &str, name: &str) -> bool {
    s.strip_prefix("zygo-").is_some_and(|rest| rest == name && valid_name(rest))
}

const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// A new pane key (W§5): `k` and 8 random characters of lower-case base32.
pub fn new_pane_key() -> String {
    let mut k = String::with_capacity(9);
    k.push('k');
    for _ in 0..8 {
        k.push(BASE32[rand::random_range(0..BASE32.len())] as char);
    }
    k
}

/// One entry of `git worktree list --porcelain`: `path` is canonical when the directory exists, `branch` has no
/// `refs/heads/` (none when detached or bare), `bare` marks the bare repository's own entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub bare: bool,
}

/// The main worktree (AU77): the first entry that is not bare; none when the list has only a bare entry or is empty.
pub fn main_worktree(worktrees: &[Worktree]) -> Option<&Worktree> {
    worktrees.iter().find(|w| !w.bare)
}

/// Why [`worktrees`] gave no list. `Display`: `not a git repository`, `git not found`, `git worktree list failed`,
/// `git worktree list timed out`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListError {
    /// git's own answer: the folder is in no repository (its stderr passes `zygo_config::repo_file::git_absent`).
    NotARepository,
    /// git could not be started.
    NotFound,
    /// Any other non-zero exit, a read error, a missing pipe or output that is not UTF-8.
    Failed,
    /// The deadline of the checkout passed.
    TimedOut,
}

impl std::fmt::Display for ListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ListError::NotARepository => "not a git repository",
            ListError::NotFound => "git not found",
            ListError::Failed => "git worktree list failed",
            ListError::TimedOut => "git worktree list timed out",
        })
    }
}

impl std::error::Error for ListError {}

/// The most stderr bytes kept from a `git worktree list`; the rest is read and dropped, so git never stops on a full
/// pipe.
const STDERR_CAP: u64 = 64 * 1024;

/// The worktrees of `checkout`, in git's order (the main worktree or the bare entry first). Runs
/// `git -c core.fsmonitor=false -C <checkout.top> worktree list --porcelain` on `zygo_config::repo_file::git_command`
/// with `GIT_OPTIONAL_LOCKS=0` and `GIT_TERMINAL_PROMPT=0`, stdin null, stdout and stderr piped (stderr drained in its
/// own thread, at most 64 KiB kept). The call ends at `checkout.deadline`: git is then killed and reaped, and the
/// result is [`ListError::TimedOut`]. A git that cannot be started is [`ListError::NotFound`]. A non-zero exit whose
/// stderr is git's "not a git repository" answer (`zygo_config::repo_file::git_absent`) is
/// [`ListError::NotARepository`]; that stderr is taken only on a non-zero exit, within the time left to the deadline
/// (else `TimedOut`). Every other non-zero exit, a read error, a missing pipe and output that is not UTF-8 are
/// [`ListError::Failed`].
pub fn worktrees(checkout: &Checkout) -> Result<Vec<Worktree>, ListError> { worktrees_with(OsStr::new("git"), checkout) }

/// [`worktrees`] with `git` as the program: the seam of a test's failing or missing `git` (FX-25), so no test changes
/// the `PATH` of its own process. Production calls go through [`worktrees`].
#[doc(hidden)]
pub fn worktrees_with(git: &OsStr, checkout: &Checkout) -> Result<Vec<Worktree>, ListError> {
    let left = checkout.deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(ListError::TimedOut);
    }
    let mut cmd = zygo_config::repo_file::git_command_for(git);
    cmd.args(["-c", "core.fsmonitor=false", "-C"]).arg(&checkout.top).args(["worktree", "list", "--porcelain"]);
    cmd.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0");
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|_| ListError::NotFound)?;
    let stop = |child: &mut Child, why: ListError| {
        let _ = child.kill();
        let _ = child.wait();
        why
    };
    let (Some(mut out), Some(mut err)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(stop(&mut child, ListError::Failed));
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let read = out.read_to_end(&mut buf).map(|_| buf);
        let _ = tx.send(read);
    });
    let (etx, erx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut err).take(STDERR_CAP).read_to_end(&mut buf);
        let _ = std::io::copy(&mut err, &mut std::io::sink());
        let _ = etx.send(buf);
    });
    let read = match rx.recv_timeout(left) {
        Ok(read) => read,
        Err(mpsc::RecvTimeoutError::Timeout) => return Err(stop(&mut child, ListError::TimedOut)),
        Err(mpsc::RecvTimeoutError::Disconnected) => return Err(stop(&mut child, ListError::Failed)),
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < checkout.deadline => std::thread::sleep(Duration::from_millis(5)),
            Ok(None) => return Err(stop(&mut child, ListError::TimedOut)),
            Err(_) => return Err(stop(&mut child, ListError::Failed)),
        }
    };
    if !status.success() {
        let left = checkout.deadline.saturating_duration_since(Instant::now());
        let stderr = match erx.recv_timeout(left) {
            Ok(bytes) => bytes,
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(ListError::TimedOut),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(ListError::Failed),
        };
        let absent = zygo_config::repo_file::git_absent(&String::from_utf8_lossy(&stderr));
        return Err(if absent { ListError::NotARepository } else { ListError::Failed });
    }
    let text = read.ok().and_then(|bytes| String::from_utf8(bytes).ok());
    text.map(|text| parse(&text)).ok_or(ListError::Failed)
}

/// The entries of `git worktree list --porcelain` output. Lines other than `worktree`, `bare` and a `branch` under
/// `refs/heads/` change nothing; a path that cannot be made canonical (the directory is gone) is kept as git wrote it.
fn parse(text: &str) -> Vec<Worktree> {
    let mut list: Vec<Worktree> = Vec::new();
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            let path = Path::new(p).canonicalize().unwrap_or_else(|_| PathBuf::from(p));
            list.push(Worktree { path, branch: None, bare: false });
        } else if let Some(current) = list.last_mut() {
            if line == "bare" {
                current.bare = true;
            } else if let Some(name) = line.strip_prefix("branch refs/heads/") {
                current.branch = Some(name.to_string());
            }
        }
    }
    list
}

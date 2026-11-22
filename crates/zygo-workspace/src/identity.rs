//! Workspace id, name slug and collision suffix (AU2), main worktree (AU77), validation (AU84). Owner: node W1 (W§5).

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

const TIMED_OUT: &str = "git worktree list timed out";
const FAILED: &str = "git worktree list failed";

/// The worktrees of `checkout`, in git's order (the main worktree or the bare entry first). Runs
/// `git -c core.fsmonitor=false -C <checkout.top> worktree list --porcelain` on `zygo_config::repo_file::git_command`
/// with `GIT_OPTIONAL_LOCKS=0` and `GIT_TERMINAL_PROMPT=0`, stdin and stderr null. The call ends at
/// `checkout.deadline`: git is then killed and reaped, and the result is an error. A git that is not found, a
/// non-zero exit, a read error and output that is not UTF-8 are errors too.
pub fn worktrees(checkout: &Checkout) -> anyhow::Result<Vec<Worktree>> {
    let left = checkout.deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        anyhow::bail!(TIMED_OUT);
    }
    let mut cmd = zygo_config::repo_file::git_command();
    cmd.args(["-c", "core.fsmonitor=false", "-C"]).arg(&checkout.top).args(["worktree", "list", "--porcelain"]);
    cmd.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0");
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|_| anyhow::anyhow!("git not found"))?;
    let stop = |child: &mut Child, why: &'static str| {
        let _ = child.kill();
        let _ = child.wait();
        anyhow::anyhow!(why)
    };
    let Some(mut out) = child.stdout.take() else {
        return Err(stop(&mut child, FAILED));
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let read = out.read_to_end(&mut buf).map(|_| buf);
        let _ = tx.send(read);
    });
    let read = match rx.recv_timeout(left) {
        Ok(read) => read,
        Err(mpsc::RecvTimeoutError::Timeout) => return Err(stop(&mut child, TIMED_OUT)),
        Err(mpsc::RecvTimeoutError::Disconnected) => return Err(stop(&mut child, FAILED)),
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < checkout.deadline => std::thread::sleep(Duration::from_millis(5)),
            Ok(None) => return Err(stop(&mut child, TIMED_OUT)),
            Err(_) => return Err(stop(&mut child, FAILED)),
        }
    };
    let text = read.ok().filter(|_| status.success()).and_then(|bytes| String::from_utf8(bytes).ok());
    text.map(|text| parse(&text)).ok_or_else(|| anyhow::anyhow!(FAILED))
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

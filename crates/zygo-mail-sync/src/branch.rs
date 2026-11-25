//! The side branch plumbing (S§6): the detached, index-only worktree, fetch, bootstrap, tree reads, commit and push.
//! Every git call goes through [`crate::git::run`] or [`crate::git::stream`] (S-C4), with a deadline. No function
//! runs `worktree prune`, `merge`, `rebase` or `checkout` (S§6.3, S§12), and none writes git's stdin.
use std::path::Path;
use std::time::Instant;

use zygo_work::repo::{Flow, RunError, Streamed};

use crate::git;

/// S§6.1: the bytes of `ZYGO-MAIL`, the marker that makes a branch zygo's.
pub const MARKER: &[u8] = b"1\n";

/// S§6.6: the message of every side-branch commit. It holds no count, address or host (S-C2).
pub const COMMIT_MESSAGE: &str = "zygo mail [skip ci]";

/// S§18: the reasons in a `[rejected]` porcelain line of the target ref that mean "branch moved" (git 2.50).
pub const MOVED_TEXT: &[&str] = &["fetch first", "non-fast-forward", "stale info"];

/// Why a branch step failed. `Git` names the git subcommand; `Refused` is a value refused before any spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchError {
    Git(RunError, &'static str),
    Io(String),
    Refused(&'static str),
}

/// The result of a fetch of the side branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    Tip(String),
    NoBranch,
}

/// One `git ls-tree --long` entry; `size` is `None` for a tree. The path is remote data (S-C5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub mode: String,
    pub kind: String,
    pub id: String,
    pub size: Option<u64>,
    pub path: String,
}

/// One change of a side-branch commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Add { path: String, bytes: Vec<u8> },
    Remove { path: String },
}

/// The class of a push. `Other` holds one fixed text, never server or URL text (S-C2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pushed {
    Accepted,
    Moved,
    Other(String),
}

/// `s` when it is 40 or 64 lower-case hex, else `Refused("object id")`.
fn oid(s: &str) -> Result<&str, BranchError> {
    let hex = s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if (s.len() == 40 || s.len() == 64) && hex { Ok(s) } else { Err(BranchError::Refused("object id")) }
}

/// The stdout of `git <args>` in `cwd` on a zero exit; a runner error is `Git(e, step)`, a non-zero exit
/// `Git(Failed, step)`.
fn run_git(cwd: &Path, args: &[&str], step: &'static str, deadline: Instant) -> Result<Vec<u8>, BranchError> {
    match git::run(cwd, args, deadline) {
        Ok(w) if w.ok => Ok(w.out),
        Ok(_) => Err(BranchError::Git(RunError::Failed, step)),
        Err(e) => Err(BranchError::Git(e, step)),
    }
}

/// The object id that `out` holds (one line), else `Git(Failed, step)`.
fn id_out(out: &[u8], step: &'static str) -> Result<String, BranchError> {
    let s = std::str::from_utf8(out).ok().map(|s| s.strip_suffix('\n').unwrap_or(s));
    match s {
        Some(s) if oid(s).is_ok() => Ok(s.to_string()),
        _ => Err(BranchError::Git(RunError::Failed, step)),
    }
}

/// `p` as UTF-8, else `Io`.
fn path_str(p: &Path) -> Result<&str, BranchError> {
    p.to_str().ok_or_else(|| BranchError::Io(format!("path is not UTF-8: {}", p.display())))
}

/// S§6.3 make/use: points the worktree `wt` at `commit`. An existing worktree (`wt/.git` a regular file) gets
/// `update-ref --no-deref HEAD`; else `git worktree add --force --detach --no-checkout` runs in `main_root`.
pub fn ensure_worktree(main_root: &Path, wt: &Path, commit: &str, deadline: Instant) -> Result<(), BranchError> {
    oid(commit)?;
    let _ = (main_root, wt, deadline);
    Err(BranchError::Io("not built".into()))
}

/// S§6.5: fetches `refs/heads/<branch>` into `refs/remotes/<remote>/<branch>` with no `FETCH_HEAD`. A remote
/// without the branch ("couldn't find remote ref", any case) is `NoBranch` (S§6.4).
pub fn fetch(cwd: &Path, remote: &str, branch: &str, deadline: Instant) -> Result<Fetched, BranchError> {
    let spec = format!("+refs/heads/{branch}:refs/remotes/{remote}/{branch}");
    git::run(cwd, &["fetch", "--no-write-fetch-head", "--", remote, &spec], deadline)
        .map_err(|e| BranchError::Git(e, "fetch"))?;
    Err(BranchError::Io("not built".into()))
}

/// S§6.4: makes the root commit whose tree is the one entry `ZYGO-MAIL` = [`MARKER`], points the worktree at it
/// and returns it.
pub fn bootstrap(main_root: &Path, wt: &Path, deadline: Instant) -> Result<String, BranchError> {
    let _ = (main_root, wt, deadline);
    Err(BranchError::Io("not built".into()))
}

/// The entries of `tip` (at the top, or under `prefix`) to `each`, through `git ls-tree -z --long --full-tree`. An
/// entry that does not parse, or whose path is not UTF-8, is not passed (S-C5).
pub fn ls(wt: &Path, tip: &str, prefix: Option<&str>, deadline: Instant, each: &mut dyn FnMut(Entry) -> Flow)
    -> Result<Streamed, BranchError> {
    oid(tip)?;
    let _ = (wt, prefix, deadline, each);
    Err(BranchError::Io("not built".into()))
}

/// The bytes of `blob`; a blob above `max` bytes is `Refused("too large")` before it is read.
pub fn cat(wt: &Path, blob: &str, max: u64, deadline: Instant) -> Result<Vec<u8>, BranchError> {
    oid(blob)?;
    let _ = (wt, max, deadline);
    Err(BranchError::Io("not built".into()))
}

/// S§6.1, S§8.3 step 7: whether `tip` has `ZYGO-MAIL` as a `100644` blob that holds exactly [`MARKER`].
pub fn has_marker(wt: &Path, tip: &str, deadline: Instant) -> Result<bool, BranchError> {
    oid(tip)?;
    let _ = (wt, deadline);
    Err(BranchError::Io("not built".into()))
}

/// S§6.3 use, S§6.5: the tree of `base` with `changes`, committed in the worktree's own index with `parent` (none: a
/// root); the worktree `HEAD` moves to the new commit, which is returned.
pub fn commit(wt: &Path, base: &str, parent: Option<&str>, changes: &[Change], deadline: Instant)
    -> Result<String, BranchError> {
    oid(base)?;
    parent.map(oid).transpose()?;
    let _ = (wt, changes, deadline);
    Err(BranchError::Io("not built".into()))
}

/// S§8.3 step 15: the class of a `git push --porcelain` (exit success `ok`, stdout `out`) to `refs/heads/<branch>`.
pub fn classify(ok: bool, out: &[u8], branch: &str) -> Pushed {
    let _ = (ok, out, branch);
    Pushed::Other("push failed".into())
}

/// S§8.3 step 14, S§9: pushes `commit` to `refs/heads/<branch>` on `remote`, with the lease `lease` when given.
pub fn push(wt: &Path, remote: &str, branch: &str, commit: &str, lease: Option<&str>, deadline: Instant)
    -> Result<Pushed, BranchError> {
    oid(commit)?;
    lease.map(oid).transpose()?;
    let _ = (wt, remote, branch, deadline);
    Err(BranchError::Io("not built".into()))
}

/// Moves `refs/remotes/<remote>/<branch>` to `commit` after an accepted push.
pub fn set_tracking(wt: &Path, remote: &str, branch: &str, commit: &str, deadline: Instant) -> Result<(), BranchError> {
    oid(commit)?;
    let _ = (wt, remote, branch, deadline);
    Err(BranchError::Io("not built".into()))
}

/// S§6.3 remove: `git worktree remove --force -- <wt>` in `main_root`; only that path.
pub fn remove_worktree(main_root: &Path, wt: &Path, deadline: Instant) -> Result<(), BranchError> {
    let _ = (main_root, wt, deadline);
    Err(BranchError::Io("not built".into()))
}

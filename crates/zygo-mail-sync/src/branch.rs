//! The side branch plumbing (S§6): the detached, index-only worktree, fetch, bootstrap, tree reads, commit and push.
//! Every git call goes through [`crate::git::run`] or [`crate::git::stream`] (S-C4), with a deadline. No function
//! runs `worktree prune`, `merge`, `rebase` or `checkout` (S§6.3, S§12), and none writes git's stdin.
use std::collections::HashMap;
use std::io::Write;
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

/// A local file error as `Io`.
fn io(e: std::io::Error) -> BranchError { BranchError::Io(e.to_string()) }

/// The prefix of the temp files of [`write_object`] and [`write_blobs`].
const TEMP_PREFIX: &str = ".zygo-object";

/// The folder of the worktree `wt` (`<workspace_cache_dir>/<id>/`), made when missing and set to `0700` (the sync
/// folders, `state.rs`): it holds the temp files of [`write_object`] and [`write_blobs`]. Temp files left by a crash are removed.
fn tmp_dir(wt: &Path) -> Result<&Path, BranchError> {
    let p = wt.parent().ok_or_else(|| BranchError::Io(format!("no parent folder: {}", wt.display())))?;
    std::fs::create_dir_all(p).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
    }
    for e in std::fs::read_dir(p).map_err(io)? {
        let e = e.map_err(io)?;
        let left = e.file_name().to_str().is_some_and(|n| n.starts_with(TEMP_PREFIX));
        if left && !e.file_type().map_err(io)?.is_dir() {
            std::fs::remove_file(e.path()).map_err(io)?;
        }
    }
    Ok(p)
}

/// Writes `bytes` as a git object of `kind` through a `0600` temp file in `tmp` (no stdin: the runner's is null)
/// and `git hash-object -t <kind> -w --no-filters` in `cwd`; returns its id. The temp file is deleted on every path.
fn write_object(cwd: &Path, tmp: &Path, kind: &str, bytes: &[u8], deadline: Instant) -> Result<String, BranchError> {
    let mut f = tempfile::Builder::new().prefix(TEMP_PREFIX).tempfile_in(tmp).map_err(io)?;
    f.write_all(bytes).and_then(|()| f.flush()).map_err(io)?;
    let out = run_git(cwd, &["hash-object", "-t", kind, "-w", "--no-filters", "--", path_str(f.path())?],
        "hash-object", deadline)?;
    id_out(&out, "hash-object")
}

/// The most characters that the variable arguments of one `hash-object` or `update-index` call of [`commit`] add up
/// to (each counted by [`arg_len`]). Windows `CreateProcessW` takes at most 32 767 characters on a command line; this
/// budget keeps well under it, with room for the git path, the fixed arguments and the `-c` overrides.
const ARGV_BUDGET: usize = 16_000;

/// The most body bytes that one chunk of adds of [`commit`] holds in temp files at once. A body above it goes alone.
const BODY_BUDGET: usize = 64 << 20;

/// The command-line characters of the argument `a`: its length plus 3 (a separator and Windows quotes).
fn arg_len(a: &str) -> usize { a.len() + 3 }

/// The command-line characters of one `--cacheinfo 100644,<id>,<path>`, with the longest (SHA-256) id.
fn cacheinfo_len(path: &str) -> usize { arg_len("--cacheinfo") + arg_len("100644,,") + 64 + path.len() }

/// `items` cut, in order, into runs whose `len` sum stays within [`ARGV_BUDGET`] and whose `bytes` sum stays within
/// [`BODY_BUDGET`]. A run ends before the item that would pass either budget; an item that passes one alone is a run
/// of its own. No run is empty.
fn chunks<T>(items: &[T], len: impl Fn(&T) -> usize, bytes: impl Fn(&T) -> usize) -> Vec<&[T]> {
    let (mut out, mut start, mut l, mut b) = (Vec::new(), 0, 0, 0);
    for (i, it) in items.iter().enumerate() {
        let (il, ib) = (len(it), bytes(it));
        if i > start && (l + il > ARGV_BUDGET || b + ib > BODY_BUDGET) {
            out.push(&items[start..i]);
            (start, l, b) = (i, 0, 0);
        }
        (l, b) = (l + il, b + ib);
    }
    if start < items.len() {
        out.push(&items[start..]);
    }
    out
}

/// Writes each of `bodies` as a git blob through its own `0600` temp file in `tmp` and `git hash-object -t blob -w
/// --no-filters` in `cwd`, one call per [`chunks`] run of the temp paths; returns the ids in order. A wrong count or
/// id is `Git(Failed, "hash-object")`. The temp files are deleted on every path.
fn write_blobs<'a>(cwd: &Path, tmp: &Path, bodies: impl Iterator<Item = &'a [u8]>, deadline: Instant)
    -> Result<Vec<String>, BranchError> {
    let mut files = Vec::new();
    for bytes in bodies {
        let mut f = tempfile::Builder::new().prefix(TEMP_PREFIX).tempfile_in(tmp).map_err(io)?;
        f.write_all(bytes).and_then(|()| f.flush()).map_err(io)?;
        // Closed here (no fd per body); the path is still deleted on drop.
        files.push(f.into_temp_path());
    }
    let paths = files.iter().map(|f| path_str(f)).collect::<Result<Vec<_>, _>>()?;
    let fail = || BranchError::Git(RunError::Failed, "hash-object");
    let mut ids = Vec::with_capacity(paths.len());
    for run in chunks(&paths, |p| arg_len(p), |_| 0) {
        let mut args = vec!["hash-object", "-t", "blob", "-w", "--no-filters", "--"];
        args.extend(run);
        let out = run_git(cwd, &args, "hash-object", deadline)?;
        let got: Vec<String> = std::str::from_utf8(&out).map_err(|_| fail())?.lines()
            .map(|l| oid(l).map(String::from).map_err(|_| fail())).collect::<Result<_, _>>()?;
        if got.len() != run.len() {
            return Err(fail());
        }
        ids.extend(got);
    }
    Ok(ids)
}

/// S§6.5: `commit-tree <tree> [-p <parent>]` in `cwd` as `zygo <zygo@zygo.invalid>`, unsigned, with
/// [`COMMIT_MESSAGE`]; returns the commit.
fn commit_tree(cwd: &Path, tree: &str, parent: Option<&str>, deadline: Instant) -> Result<String, BranchError> {
    let mut args = vec!["-c", "user.name=zygo", "-c", "user.email=zygo@zygo.invalid", "-c", "commit.gpgSign=false",
        "commit-tree", tree];
    if let Some(p) = parent {
        args.extend(["-p", p]);
    }
    args.extend(["-m", COMMIT_MESSAGE]);
    let out = run_git(cwd, &args, "commit-tree", deadline)?;
    id_out(&out, "commit-tree")
}

/// S§6.3 make/use: points the worktree `wt` at `commit`. An existing worktree (`wt/.git` a regular file) gets
/// `update-ref --no-deref HEAD`; else `git worktree add --force --detach --no-checkout` runs in `main_root`. When
/// that `update-ref` fails (a stale folder: its admin folder is gone), the folder is removed and made again; no
/// `worktree prune` runs (S§6.3).
pub fn ensure_worktree(main_root: &Path, wt: &Path, commit: &str, deadline: Instant) -> Result<(), BranchError> {
    oid(commit)?;
    if std::fs::symlink_metadata(wt.join(".git")).is_ok_and(|m| m.is_file()) {
        match run_git(wt, &["update-ref", "--no-deref", "HEAD", commit], "update-ref", deadline) {
            Ok(_) => return Ok(()),
            Err(BranchError::Git(RunError::Failed, _)) => std::fs::remove_dir_all(wt).map_err(io)?,
            Err(e) => return Err(e),
        }
    }
    tmp_dir(wt)?;
    let w = path_str(wt)?;
    run_git(main_root, &["worktree", "add", "--force", "--detach", "--no-checkout", "--", w, commit], "worktree add",
        deadline)?;
    Ok(())
}

/// S§6.5: fetches `refs/heads/<branch>` into `refs/remotes/<remote>/<branch>` with no `FETCH_HEAD` and no tags
/// (S-G2). A remote without the branch ([`no_branch`]) is `NoBranch` (S§6.4).
pub fn fetch(cwd: &Path, remote: &str, branch: &str, deadline: Instant) -> Result<Fetched, BranchError> {
    let spec = format!("+refs/heads/{branch}:refs/remotes/{remote}/{branch}");
    let w = git::run(cwd, &["fetch", "--no-write-fetch-head", "--no-tags", "--", remote, &spec], deadline)
        .map_err(|e| BranchError::Git(e, "fetch"))?;
    if !w.ok {
        if no_branch(&w.err) {
            return Ok(Fetched::NoBranch);
        }
        return Err(BranchError::Git(RunError::Failed, "fetch"));
    }
    let tip = format!("refs/remotes/{remote}/{branch}^{{commit}}");
    let out = run_git(cwd, &["rev-parse", "--verify", &tip], "rev-parse", deadline)?;
    Ok(Fetched::Tip(id_out(&out, "rev-parse")?))
}

/// S§6.4: whether the stderr of a failed fetch says that the remote has no such branch: a line of git's own that
/// starts `fatal: couldn't find remote ref` (any case). A `remote:` or `fatal: remote error:` line is server text.
pub fn no_branch(err: &[u8]) -> bool {
    const LINE: &[u8] = b"fatal: couldn't find remote ref";
    err.split(|&b| b == b'\n').any(|l| l.len() >= LINE.len() && l[..LINE.len()].eq_ignore_ascii_case(LINE))
}

/// S§6.4: makes the root commit whose tree is the one entry `ZYGO-MAIL` = [`MARKER`], points the worktree at it
/// and returns it.
pub fn bootstrap(main_root: &Path, wt: &Path, deadline: Instant) -> Result<String, BranchError> {
    let tmp = tmp_dir(wt)?;
    let b = write_object(main_root, tmp, "blob", MARKER, deadline)?;
    let mut t = b"100644 ZYGO-MAIL\0".to_vec();
    t.extend(hex::decode(&b).map_err(|_| BranchError::Git(RunError::Failed, "hash-object"))?);
    let tree = write_object(main_root, tmp, "tree", &t, deadline)?;
    let root = commit_tree(main_root, &tree, None, deadline)?;
    ensure_worktree(main_root, wt, &root, deadline)?;
    Ok(root)
}

/// The entries of `tip` (at the top, or under `prefix`, a literal path) to `each`, through `git --literal-pathspecs
/// ls-tree -z --long --full-tree`. An entry that does not parse, or whose path is not UTF-8, is not passed (S-C5).
pub fn ls(wt: &Path, tip: &str, prefix: Option<&str>, deadline: Instant, each: &mut dyn FnMut(Entry) -> Flow)
    -> Result<Streamed, BranchError> {
    oid(tip)?;
    let mut args = vec!["--literal-pathspecs", "ls-tree", "-z", "--long", "--full-tree", tip];
    if let Some(p) = prefix {
        args.extend(["--", p]);
    }
    let end = git::stream(wt, &args, deadline, &mut |e| match entry(e) {
        Some(e) => each(e),
        None => Flow::Go,
    }).map_err(|e| BranchError::Git(e, "ls-tree"))?;
    match end {
        Streamed::Done { ok: false, .. } => Err(BranchError::Git(RunError::Failed, "ls-tree")),
        end => Ok(end),
    }
}

/// One `ls-tree -z --long` entry: split at its first tab, the left part exactly 4 fields (mode, kind, id, size or
/// `-`), the rest a UTF-8 path. Anything else is `None`.
fn entry(e: &[u8]) -> Option<Entry> {
    let tab = e.iter().position(|&b| b == b'\t')?;
    let left = std::str::from_utf8(&e[..tab]).ok()?;
    let path = std::str::from_utf8(&e[tab + 1..]).ok()?;
    let f: Vec<&str> = left.split_ascii_whitespace().collect();
    let [mode, kind, id, size] = f[..] else { return None };
    let size = if size == "-" { None } else { Some(size.parse().ok()?) };
    Some(Entry { mode: mode.into(), kind: kind.into(), id: id.into(), size, path: path.into() })
}

/// The bytes of `blob`; a blob above `max` bytes is `Refused("too large")` before it is read.
pub fn cat(wt: &Path, blob: &str, max: u64, deadline: Instant) -> Result<Vec<u8>, BranchError> {
    oid(blob)?;
    let out = run_git(wt, &["cat-file", "-s", blob], "cat-file", deadline)?;
    let size: u64 = std::str::from_utf8(&out).ok().and_then(|s| s.trim_end().parse().ok())
        .ok_or(BranchError::Git(RunError::Failed, "cat-file"))?;
    if size > max {
        return Err(BranchError::Refused("too large"));
    }
    run_git(wt, &["cat-file", "blob", blob], "cat-file", deadline)
}

/// S§6.1, S§8.3 step 7: whether `tip` has `ZYGO-MAIL` as a `100644` blob that holds exactly [`MARKER`].
pub fn has_marker(wt: &Path, tip: &str, deadline: Instant) -> Result<bool, BranchError> {
    oid(tip)?;
    let mut found = None;
    ls(wt, tip, Some("ZYGO-MAIL"), deadline, &mut |e| {
        if e.path == "ZYGO-MAIL" {
            found = Some(e);
        }
        Flow::Go
    })?;
    match found {
        Some(e) if e.mode == "100644" && e.kind == "blob" && e.size == Some(MARKER.len() as u64) => {
            Ok(cat(wt, &e.id, MARKER.len() as u64, deadline)? == MARKER)
        }
        _ => Ok(false),
    }
}

/// S§6.3 use, S§6.5: the tree of `base` with `changes`, committed in the worktree's own index with `parent` (none: a
/// root); the worktree `HEAD` moves to the new commit, which is returned. Of the changes with the same path string
/// only the last is applied; all removes are applied before all adds. When the paths are distinct as git sees them
/// (no `./x` or `x//y` spelling of another path) and applying the changes one at a time in order succeeds, the tree
/// is the same as that one-at-a-time result. Removes first can succeed where in-order fails (`[Add "a", Remove
/// "a/b"]` with `a/b` in `base`). The git calls are cut by [`chunks`]; a failed call leaves `HEAD` as it was.
pub fn commit(wt: &Path, base: &str, parent: Option<&str>, changes: &[Change], deadline: Instant)
    -> Result<String, BranchError> {
    oid(base)?;
    parent.map(oid).transpose()?;
    run_git(wt, &["read-tree", base], "read-tree", deadline)?;
    // Only the last change of each path string is applied; the removes go first, then the adds.
    fn path_of(ch: &Change) -> &str {
        match ch {
            Change::Add { path, .. } | Change::Remove { path } => path,
        }
    }
    let last: HashMap<&str, usize> = changes.iter().enumerate().map(|(i, ch)| (path_of(ch), i)).collect();
    let (mut adds, mut removes) = (Vec::new(), Vec::new());
    for ch in changes.iter().enumerate().filter_map(|(i, ch)| (last[path_of(ch)] == i).then_some(ch)) {
        match ch {
            Change::Add { path, bytes } => adds.push((path.as_str(), bytes.as_slice())),
            Change::Remove { path } => removes.push(path.as_str()),
        }
    }
    for chunk in chunks(&removes, |p| arg_len(p), |_| 0) {
        let mut args = vec!["update-index", "--force-remove", "--"];
        args.extend(chunk);
        run_git(wt, &args, "update-index", deadline)?;
    }
    // `tmp_dir` removes left temp files: it runs once, before any temp file of this commit exists.
    let tmp = if adds.is_empty() { wt } else { tmp_dir(wt)? };
    for chunk in chunks(&adds, |&(p, _)| cacheinfo_len(p), |&(_, b)| b.len()) {
        let ids = write_blobs(wt, tmp, chunk.iter().map(|&(_, b)| b), deadline)?;
        let infos: Vec<String> = chunk.iter().zip(&ids).map(|(&(path, _), b)| format!("100644,{b},{path}")).collect();
        let mut args = vec!["update-index", "--add"];
        for info in &infos {
            args.extend(["--cacheinfo", info.as_str()]);
        }
        run_git(wt, &args, "update-index", deadline)?;
    }
    let out = run_git(wt, &["write-tree"], "write-tree", deadline)?;
    let tree = id_out(&out, "write-tree")?;
    let c = commit_tree(wt, &tree, parent, deadline)?;
    run_git(wt, &["update-ref", "--no-deref", "HEAD", &c], "update-ref", deadline)?;
    Ok(c)
}

/// S§8.3 step 15: the class of a `git push --porcelain` (exit success `ok`, stdout `out`) to `refs/heads/<branch>`.
pub fn classify(ok: bool, out: &[u8], branch: &str) -> Pushed {
    let target = format!("refs/heads/{branch}");
    // A ref line: exactly 3 tab-separated fields, a 1-byte flag, and `<from>:<to>` with `<to>` the target ref.
    let line = out.split(|&b| b == b'\n').map(|l| l.split(|&b| b == b'\t').collect::<Vec<_>>()).find(|f| {
        f.len() == 3 && f[0].len() == 1
            && f[1].iter().position(|&b| b == b':').is_some_and(|i| &f[1][i + 1..] == target.as_bytes())
    });
    let Some(f) = line else { return Pushed::Other("push failed".into()) };
    let (flag, summary) = (f[0][0], f[2]);
    if ok && matches!(flag, b' ' | b'*' | b'+' | b'=') {
        return Pushed::Accepted;
    }
    if flag != b'!' {
        return Pushed::Other("push failed".into());
    }
    // A `[remote rejected]` reason is server text: never "moved" (S§18).
    let rejected = summary.len() >= 10 && summary[..10].eq_ignore_ascii_case(b"[rejected]");
    if rejected && git::has_text(summary, MOVED_TEXT) { Pushed::Moved } else { Pushed::Other("push rejected".into()) }
}

/// S§8.3 step 14, S§9: pushes `commit` to `refs/heads/<branch>` on `remote`, with the lease `lease` when given,
/// unsigned (`-c push.gpgSign=false`, as `commit.gpgSign` in S§6.5).
pub fn push(wt: &Path, remote: &str, branch: &str, commit: &str, lease: Option<&str>, deadline: Instant)
    -> Result<Pushed, BranchError> {
    oid(commit)?;
    lease.map(oid).transpose()?;
    let lease = lease.map(|l| format!("--force-with-lease=refs/heads/{branch}:{l}"));
    let spec = format!("{commit}:refs/heads/{branch}");
    let mut args = vec!["-c", "push.gpgSign=false", "push", "--porcelain"];
    args.extend(lease.as_deref());
    args.extend(["--", remote, &spec]);
    let w = git::run(wt, &args, deadline).map_err(|e| BranchError::Git(e, "push"))?;
    Ok(classify(w.ok, &w.out, branch))
}

/// Moves `refs/remotes/<remote>/<branch>` to `commit` after an accepted push.
pub fn set_tracking(wt: &Path, remote: &str, branch: &str, commit: &str, deadline: Instant) -> Result<(), BranchError> {
    oid(commit)?;
    run_git(wt, &["update-ref", &format!("refs/remotes/{remote}/{branch}"), commit], "update-ref", deadline)?;
    Ok(())
}

/// S§6.3 remove: `git worktree remove --force -- <wt>` in `main_root`; only that path.
pub fn remove_worktree(main_root: &Path, wt: &Path, deadline: Instant) -> Result<(), BranchError> {
    run_git(main_root, &["worktree", "remove", "--force", "--", path_str(wt)?], "worktree remove", deadline)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `len` and `bytes` sums of each run of `items` (pairs of `len`, `bytes`).
    fn sums(items: &[(usize, usize)]) -> Vec<(usize, usize, usize)> {
        chunks(items, |x| x.0, |x| x.1).iter()
            .map(|r| (r.len(), r.iter().map(|x| x.0).sum(), r.iter().map(|x| x.1).sum())).collect()
    }

    #[test]
    fn no_run_passes_the_argv_budget() {
        // 300 `--cacheinfo` arguments of 97-character message paths.
        let len = cacheinfo_len(&"p".repeat(97));
        let items = vec![(len, 0); 300];
        let runs = sums(&items);
        assert!(runs.len() > 1);
        assert!(runs.iter().all(|&(n, l, _)| n > 0 && l <= ARGV_BUDGET), "{runs:?}");
        assert_eq!(runs.iter().map(|r| r.0).sum::<usize>(), 300);
    }

    #[test]
    fn no_run_passes_the_body_budget_but_a_large_body_goes_alone() {
        let mib = 1 << 20;
        let items = [(10, 8 * mib); 20].into_iter().chain([(10, BODY_BUDGET + 1), (10, 1)]).collect::<Vec<_>>();
        let runs = sums(&items);
        assert_eq!(runs.iter().map(|r| r.0).collect::<Vec<_>>(), [8, 8, 4, 1, 1]);
        assert!(runs.iter().all(|&(n, _, b)| b <= BODY_BUDGET || n == 1), "{runs:?}");
    }

    #[test]
    fn an_empty_list_has_no_run_and_order_is_kept() {
        assert!(chunks(&[] as &[usize], |_| 1, |_| 0).is_empty());
        let items: Vec<usize> = (0..5000).collect();
        let flat: Vec<usize> = chunks(&items, |_| 100, |_| 0).concat();
        assert_eq!(flat, items);
    }
}

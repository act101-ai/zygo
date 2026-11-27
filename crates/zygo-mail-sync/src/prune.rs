//! The prune (mail sync design S§9): an orphan commit of the base tree less each message file of a member name that
//! the valid ack of its host lists, then the one force push zygo does, with the base as lease (S-C3). No clock and
//! no keep (S-C8).
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use ulid::Ulid;
use zygo_work::repo::{Flow, RunError};

use crate::address;
use crate::branch::{self, BranchError, Change, Entry, Pushed};
use crate::identity::TipMeta;

/// S§9 When: a base whose `git rev-list --count` is this or more is due for a prune.
pub const COMMITS_DUE: u64 = 1000;
/// S§7.4 Size, S§9 When: an own ack of more than this many entries makes the prune due.
pub const OWN_ACK_DUE: usize = 100000;

/// What one prune did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pruned {
    /// The new root is pushed, with `removed` message files fewer than the base.
    Done { removed: usize },
    /// Another push came after the base: the lease failed and the prune is skipped in this pass (S§9 Push).
    LeaseLost,
    /// Not due (S§9 When); never returned by [`run`].
    NotDue,
}

/// S§9 When: `true` when `forced` (`zygo mail sync --prune`), when the own ack has more than [`OWN_ACK_DUE`] entries,
/// or when `git rev-list --count <base>` is [`COMMITS_DUE`] or more. The walk stops at [`COMMITS_DUE`] commits.
pub fn due(wt: &Path, base: &str, own_ack_len: usize, forced: bool, deadline: Instant) -> Result<bool, BranchError> {
    if forced || own_ack_len > OWN_ACK_DUE {
        return Ok(true);
    }
    let max = format!("--max-count={COMMITS_DUE}");
    let w = crate::git::run(wt, &["rev-list", "--count", &max, base, "--"], deadline)
        .map_err(|e| BranchError::Git(e, "rev-list"))?;
    let failed = BranchError::Git(RunError::Failed, "rev-list");
    if !w.ok {
        return Err(failed);
    }
    let s = std::str::from_utf8(&w.out).map_err(|_| failed.clone())?;
    let line = s.strip_suffix('\n').unwrap_or(s);
    if line.is_empty() || !line.bytes().all(|b| b.is_ascii_digit()) {
        return Err(failed);
    }
    let n: u64 = line.parse().map_err(|_| failed)?;
    Ok(n >= COMMITS_DUE)
}

/// S§9 Tree (MS-14 review F6): whether `e` is a message file that a prune can remove: a blob of mode `100644` or
/// `100755`. A link (`120000`), a submodule (`160000`) or a tree is not.
pub fn removable(e: &Entry) -> bool {
    e.kind == "blob" && matches!(e.mode.as_str(), "100644" | "100755")
}

/// S§7.3, S§9 Tree (FX-27 review round 2 F-B): the member names of the workspace with identity `identity`: the
/// `members` of each valid registry of `meta` with that identity. A pass writes its registry from the member records
/// that its import maps folder names with (`sync::own_names`), so a folder `<name>@<host>` whose name is not here is
/// one that the import of that workspace never reads, and no pass acknowledges an entry in it.
pub fn member_names<'m>(meta: &'m TipMeta, identity: &str) -> BTreeSet<&'m str> {
    meta.registries.values()
        .filter_map(|r| r.value.as_ref().ok())
        .filter(|r| r.identity == identity)
        .flat_map(|r| r.members.iter().map(String::as_str))
        .collect()
}

/// S§9 Tree, Commit, Push, Only its own branch: a commit with no parent of the tree of `base` less each message file
/// `<name>@<host>/<ULID>.json` ([`removable`]) whose ULID the valid ack of `<host>` lists and whose `<name>` is a
/// member name of that ack's identity ([`member_names`]); a refused ack lists nothing (S§7.4 Validation). A file of
/// another name is never removed (FX-27 review round 2 F-B). Just before the force push, `branch` must be a zygo
/// branch name and `base` must have the marker (S-C3); else `Refused("branch")` or `Refused("marker")`. The push has
/// `base` as lease: a failed lease is `LeaseLost`; any other failed push is `Git(Failed, "push")`.
pub fn run(wt: &Path, remote: &str, branch: &str, base: &str, meta: &TipMeta, deadline: Instant)
    -> Result<Pruned, BranchError> {
    let acked: BTreeMap<&str, (&BTreeSet<Ulid>, BTreeSet<&str>)> = meta.acks.iter()
        .filter_map(|(h, r)| r.value.as_ref().ok().map(|a| (h, a)))
        .map(|(h, a)| (h.as_str(), (&a.imported, member_names(meta, &a.identity))))
        .collect();
    let mut folders = Vec::new();
    branch::ls(wt, base, None, deadline, &mut |e| {
        if e.kind == "tree" && let Some(a) = address::parse(&e.path)
            && acked.get(a.host.as_str()).is_some_and(|(_, names)| names.contains(a.name.as_str()))
        {
            folders.push((e.path, a.host));
        }
        Flow::Go
    })?;
    let mut removes = Vec::new();
    for (f, host) in &folders {
        let set = acked[host.as_str()].0;
        let prefix = format!("{f}/");
        branch::ls(wt, base, Some(&prefix), deadline, &mut |e| {
            let listed = e.path.strip_prefix(&prefix).and_then(address::ulid_file).is_some_and(|u| set.contains(&u));
            if removable(&e) && listed {
                removes.push(Change::Remove { path: e.path });
            }
            Flow::Go
        })?;
    }
    let removed = removes.len();
    let root = branch::commit(wt, base, None, &removes, deadline)?;
    if !zygo_config::valid_sync_branch(branch) {
        return Err(BranchError::Refused("branch"));
    }
    if !branch::has_marker(wt, base, deadline)? {
        return Err(BranchError::Refused("marker"));
    }
    match branch::push(wt, remote, branch, &root, Some(base), deadline)? {
        Pushed::Accepted => Ok(Pruned::Done { removed }),
        Pushed::Moved => Ok(Pruned::LeaseLost),
        Pushed::Other(_) => Err(BranchError::Git(RunError::Failed, "push")),
    }
}

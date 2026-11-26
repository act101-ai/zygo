//! The prune (mail sync design S§9): an orphan commit of the base tree less each message file that the valid ack of
//! its host lists, then the one force push zygo does, with the base as lease (S-C3). No clock and no keep (S-C8).
use std::path::Path;
use std::time::Instant;

use zygo_work::repo::RunError;

use crate::branch::BranchError;

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
    /// Not due: the step-16 value of `sync.rs`, never returned by [`run`].
    NotDue,
}

/// S§9 When: `true` when `forced` (`zygo mail sync --prune`), when the own ack has more than [`OWN_ACK_DUE`] entries,
/// or when `git rev-list --count <base>` is [`COMMITS_DUE`] or more.
pub fn due(wt: &Path, base: &str, own_ack_len: usize, forced: bool, deadline: Instant) -> Result<bool, BranchError> {
    if forced || own_ack_len > OWN_ACK_DUE {
        return Ok(true);
    }
    let w = crate::git::run(wt, &["rev-list", "--count", base, "--"], deadline)
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

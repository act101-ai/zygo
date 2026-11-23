use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use zygo_core::session_dir::live;

pub const KEEP: Duration = Duration::from_secs(24 * 60 * 60);

/// Removes each session directory that is not live ([`live`]: its launcher holds the lock on `pid`) and whose newest
/// change ([`changed`]) is 24 h or more before `now`: an ended session's `results/` (§10.1) and what a launcher that
/// died before its exit left (§8.2 step 6).
///
/// A directory whose `results` is a symlink is kept, except one linked into the workspace cache (AU69 (a), AU16):
/// when [`in_workspace_cache`] finds the link under the canonical `workspace_cache` (`Paths::workspace_cache_dir`),
/// the same age rule applies, `live` is checked once more right before the removal, and then the link is removed
/// first and the directory only when that worked. A link whose target is gone is decided by its text: a text with a
/// `..` component, or one that goes through a link whose target is missing, keeps the directory. The link target is
/// never touched: the sweep never deletes a workspace results folder or anything in it. Every I/O error keeps the
/// directory and is ignored: this runs at the start of a command and must not stop it.
pub fn sweep(cache_root: &Path, workspace_cache: &Path, now: SystemTime) {
    let Ok(sessions) = std::fs::read_dir(cache_root.join("sessions")) else { return };
    for entry in sessions.flatten() {
        // `file_type` and `symlink_metadata` do not follow symlinks: a link is never swept
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let dir = entry.path();
        if live(&dir) {
            continue;
        }
        let results = dir.join("results");
        let linked = std::fs::symlink_metadata(&results).is_ok_and(|m| m.file_type().is_symlink());
        if linked && !in_workspace_cache(&results, workspace_cache) {
            continue;
        }
        let Some(changed) = changed(&dir) else { continue };
        if !now.duration_since(changed).is_ok_and(|age| age >= KEEP) {
            continue;
        }
        if !linked {
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        }
        // a launcher that started since the first check holds the lock now
        if live(&dir) {
            continue;
        }
        // the link first (its target stays), then the directory (`remove_dir_all` does not follow links)
        if std::fs::remove_file(&results).is_ok() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// Whether the target of the `results` link is under the canonical `workspace_cache`. An existing target is compared
/// canonical. A target that does not exist (after `zygo ws forget` or `forget-pane`) is decided by the link text: a
/// text with a `..` component gives `false` (the kernel takes `..` after a symlink from the link's target, so no
/// reading of the text alone is sure); any other text is made absolute against the link's (canonical) folder, and its
/// part that still exists is made canonical by [`existing_canonical`] (so each symlink in that part is followed, as
/// the kernel follows it). A cache that cannot be made canonical (missing), or any other error, gives `false`: the
/// directory is kept.
fn in_workspace_cache(results: &Path, workspace_cache: &Path) -> bool {
    let Ok(cache) = std::fs::canonicalize(workspace_cache) else { return false };
    match std::fs::canonicalize(results) {
        Ok(target) => target.starts_with(&cache),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let (Ok(text), Some(folder)) = (std::fs::read_link(results), results.parent()) else { return false };
            if text.components().any(|c| c == Component::ParentDir) {
                return false;
            }
            let Ok(folder) = std::fs::canonicalize(folder) else { return false };
            existing_canonical(&folder.join(text)).is_some_and(|p| p.starts_with(&cache))
        }
        Err(_) => false,
    }
}

/// `path` with its longest existing ancestor made canonical and the rest added back as it is. `None` when no ancestor
/// can be made canonical, or when a part on the way up is an entry that `canonicalize` cannot resolve although it
/// exists (`symlink_metadata` succeeds: a link whose target is missing, a link loop, an entry that cannot be read):
/// the kernel would follow that entry, so the text after it cannot show where the path points (plan review F7).
fn existing_canonical(path: &Path) -> Option<PathBuf> {
    let mut rest = Vec::new();
    let mut at = path;
    loop {
        if let Ok(base) = std::fs::canonicalize(at) {
            return Some(rest.iter().rev().fold(base, |p, name| p.join(name)));
        }
        if std::fs::symlink_metadata(at).is_ok() {
            return None;
        }
        rest.push(at.file_name()?);
        at = at.parent()?;
    }
}

/// The newest mtime of `dir` and of its direct entries (not followed): a result stored or a log line written changes
/// `results/` or `session.log`, not `dir` itself. `None` when `dir` cannot be read.
fn changed(dir: &Path) -> Option<SystemTime> {
    let own = std::fs::metadata(dir).and_then(|m| m.modified()).ok()?;
    let entries = std::fs::read_dir(dir).ok()?;
    Some(entries.flatten().filter_map(|e| e.metadata().and_then(|m| m.modified()).ok()).fold(own, SystemTime::max))
}

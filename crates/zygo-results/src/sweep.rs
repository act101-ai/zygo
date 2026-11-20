use std::path::Path;
use std::time::{Duration, SystemTime};

use zygo_core::session_dir::live;

pub const KEEP: Duration = Duration::from_secs(24 * 60 * 60);

/// Removes each session directory that is not live ([`live`]: its launcher holds the lock on `pid`) and whose newest
/// change ([`changed`]) is 24 h or more before `now`: an ended session's `results/` (§10.1) and what a launcher that
/// died before its exit left (§8.2 step 6). A directory whose `results` is a symlink is kept. Every I/O error is
/// ignored: this runs at the start of a command and must not stop it.
pub fn sweep(cache_root: &Path, now: SystemTime) {
    let Ok(sessions) = std::fs::read_dir(cache_root.join("sessions")) else { return };
    for entry in sessions.flatten() {
        // `file_type` and `symlink_metadata` do not follow symlinks: a link is never swept
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let dir = entry.path();
        if live(&dir) || std::fs::symlink_metadata(dir.join("results")).is_ok_and(|m| m.file_type().is_symlink()) {
            continue;
        }
        let Some(changed) = changed(&dir) else { continue };
        if now.duration_since(changed).is_ok_and(|age| age >= KEEP) {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// The newest mtime of `dir` and of its direct entries (not followed): a result stored or a log line written changes
/// `results/` or `session.log`, not `dir` itself. `None` when `dir` cannot be read.
fn changed(dir: &Path) -> Option<SystemTime> {
    let own = std::fs::metadata(dir).and_then(|m| m.modified()).ok()?;
    let entries = std::fs::read_dir(dir).ok()?;
    Some(entries.flatten().filter_map(|e| e.metadata().and_then(|m| m.modified()).ok()).fold(own, SystemTime::max))
}

//! The workspace phase of a member session for the HUD (H§7.6, AU25).
//!
//! The reader gets its environment as a closure and reads no process environment. It never fails: each failure
//! gives `None`, with no line on stderr and no line in a log (C6). It never waits on a lock: `workspace.json` is
//! replaced by rename (W-C4), so one read without a lock sees a complete file. `zygo-hooks` uses no `zygo-workspace`
//! code here: the reader has its own small struct and its own copy of the id rule.

use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;

const STATE_FILE: &str = "workspace.json";
/// The only `version` that the reader reads (AU25, WS AU29).
const VERSION: u32 = 1;
/// The largest `workspace.json` that the reader reads: the value of the stdin bound of the HUD (AU24).
const MAX_BYTES: u64 = 64 * 1024;

/// The two fields of `workspace.json` that the HUD reads (WS§7.2). Unknown fields are ignored (WS AU78).
#[derive(Deserialize)]
struct Marker {
    version: u32,
    phase: String,
}

/// `^[0-9a-f]{16}$` on the whole string (WS§5, WS AU84); equal to `zygo_workspace::identity::valid_ws_id`.
pub fn valid_ws_id(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// Opens `path` read-only. The open never waits: with `O_NONBLOCK`, the open of a FIFO that has no writer returns
/// at once. The flag does not change the read of a regular file.
#[cfg(unix)]
fn open(path: &Path) -> Option<File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open(path).ok()
}

/// Opens `path` read-only. The FIFO case is unix-only, so this open has no flag; the caller checks the handle.
#[cfg(not(unix))]
fn open(path: &Path) -> Option<File> {
    File::open(path).ok()
}

/// The `phase` of the workspace of this session (H§7.6, AU25); `None` in every other case.
///
/// The session is the one of `ZYGO_SESSION_DIR` (through `env`); its `view.json` gives `workspace.member.ws_id`. The
/// file is `paths.workspaces_dir()/<ws_id>/workspace.json`, read fresh on each call, only when the opened handle is
/// a regular file of at most 64 KiB. An id that fails `valid_ws_id` builds no path. The result is the string of the file with no
/// filter: the renderer has the rule for the phases that show.
pub fn read(paths: &Paths, env: &dyn Fn(&str) -> Option<OsString>) -> Option<String> {
    let session_dir = env("ZYGO_SESSION_DIR").filter(|v| !v.is_empty()).map(PathBuf::from)?;
    let ws_id = SessionView::load(&SessionDir::new(session_dir)).ok()?.workspace?.member.ws_id;
    if !valid_ws_id(&ws_id) {
        return None;
    }
    let path = paths.workspaces_dir().join(&ws_id).join(STATE_FILE);
    let file = open(&path)?;
    // The check is on the opened handle, not on the path: no other file can take its place after the check.
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    // The file can grow after the check, so the read has its own bound.
    let n = file.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if n as u64 > MAX_BYTES {
        return None;
    }
    let marker: Marker = serde_json::from_slice(&bytes).ok()?;
    (marker.version == VERSION).then_some(marker.phase)
}

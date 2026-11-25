//! The workspace phase of a member session for the HUD (H§7.6, AU25).
//!
//! The reader gets its environment as a closure and reads no process environment. It never fails: each failure
//! gives `None`, with no line on stderr and no line in a log (C6). It never waits on a lock: `workspace.json` is
//! replaced by rename (W-C4), so one read without a lock sees a complete file. `zygo-hooks` uses no `zygo-workspace`
//! code here: the reader has its own small struct and its own copy of the id rule.
//! The same read also gives the run-time start of session run time design T§2.2 (T§2.6).

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

/// The `members` field of `workspace.json`, parsed only for the start (T§2.2), so that a `members` value that does not
/// parse never changes the phase. Absent reads as `Null`.
#[derive(Deserialize)]
struct Members {
    #[serde(default)]
    members: serde_json::Value,
}

/// The fields of a member record that the start rule reads (T§2.2). Unknown fields are ignored.
#[derive(Deserialize)]
struct Record {
    #[serde(default)]
    zygo_session: Option<String>,
    #[serde(default)]
    pending: Option<PendingStart>,
    #[serde(default)]
    first_started_at: Option<u64>,
}

#[derive(Deserialize)]
struct PendingStart {
    zygo_session: String,
    started_at: u64,
}

/// The phase and the run-time start of one `workspace.json` read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberRead {
    pub phase: Option<String>,
    pub start: Option<u64>,
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
    Some(member_marker(paths, &SessionView::load(&SessionDir::new(session_dir)).ok()?)?.0.phase)
}

/// The checked marker and the bytes of the `workspace.json` of the member session of `view`; `None` outside a
/// workspace, for an id that fails `valid_ws_id` (no path is built), and for each failed read check.
fn member_marker(paths: &Paths, view: &SessionView) -> Option<(Marker, Vec<u8>)> {
    let ws_id = &view.workspace.as_ref()?.member.ws_id;
    if !valid_ws_id(ws_id) {
        return None;
    }
    read_marker(paths, ws_id)
}

/// The phase (H§7.6, AU25) and the run-time start (session run time design T§2.2) of the session of `view`, from one
/// `workspace.json` read. A start of 0 counts as none.
pub fn member_read(paths: &Paths, view: &SessionView) -> MemberRead {
    let own = zygo_core::ids::ulid_secs(&view.session).filter(|&s| s != 0);
    let Some(workspace) = &view.workspace else {
        return MemberRead { phase: None, start: own };
    };
    let Some((marker, bytes)) = member_marker(paths, view) else {
        return MemberRead::default();
    };
    let start = match serde_json::from_slice::<Members>(&bytes).map(|m| m.members) {
        Ok(serde_json::Value::Null) => own,
        Ok(serde_json::Value::Object(members)) => match members.get(&workspace.member.pane) {
            None => own,
            Some(value) => Record::deserialize(value).ok().and_then(|r| record_start(&r, &view.session, own)),
        },
        _ => None,
    };
    MemberRead { phase: Some(marker.phase), start }
}

/// The T§2.2 start of the session of `view`: `member_read(paths, view).start`.
pub fn start(paths: &Paths, view: &SessionView) -> Option<u64> {
    member_read(paths, view).start
}

/// Rules 3 to 6 of T§2.2 for a decoded record of the pane of session `session`.
fn record_start(r: &Record, session: &str, own: Option<u64>) -> Option<u64> {
    let pending = r.pending.as_ref().filter(|p| p.zygo_session == session);
    if pending.is_none() && r.zygo_session.as_deref() != Some(session) {
        return own;
    }
    r.first_started_at
        .filter(|&s| s != 0)
        .or_else(|| pending.map(|p| p.started_at).filter(|&s| s != 0))
        .or(own)
}

/// The checked read of `paths.workspaces_dir()/<ws_id>/workspace.json`, with its bytes; the caller has checked
/// `valid_ws_id`.
fn read_marker(paths: &Paths, ws_id: &str) -> Option<(Marker, Vec<u8>)> {
    let path = paths.workspaces_dir().join(ws_id).join(STATE_FILE);
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
    (marker.version == VERSION).then_some((marker, bytes))
}

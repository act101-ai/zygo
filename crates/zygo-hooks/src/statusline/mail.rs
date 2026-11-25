//! The unread mail count of a member session (M§9a).
//!
//! The count is the number of `*.json` names in `<workspaces_dir>/<ws id>/mail/<pane key>/new/`. It reads names
//! only: no message file is opened and no entry metadata is read, so no message text reaches the HUD (M-C2). The
//! status-line module does not depend on `zygo-workspace`, so the pane key check is a copy of
//! `zygo_workspace::identity::valid_pane_key`.

use std::time::Instant;

use zygo_core::paths::Paths;
use zygo_core::session_view::SessionView;

use super::phase::valid_ws_id;

/// `^k[a-z2-7]{8}$` on the whole string (WS AU84); equal to `zygo_workspace::identity::valid_pane_key`.
fn valid_pane_key(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 9 && b[0] == b'k' && b[1..].iter().all(|c| matches!(c, b'a'..=b'z' | b'2'..=b'7'))
}

/// The unread count; `None` outside a workspace, with an invalid ws id or pane key (no path is built), after
/// `deadline`, or on a read error.
pub fn unread(paths: &Paths, view: &SessionView, deadline: Instant) -> Option<usize> {
    let member = &view.workspace.as_ref()?.member;
    if !valid_ws_id(&member.ws_id) || !valid_pane_key(&member.pane) {
        return None;
    }
    let dir = paths.workspaces_dir().join(&member.ws_id).join("mail").join(&member.pane).join("new");
    if Instant::now() >= deadline {
        return None;
    }
    let mut count = 0;
    for entry in std::fs::read_dir(dir).ok()? {
        if Instant::now() >= deadline {
            return None;
        }
        let name = entry.ok()?.file_name();
        if name.to_str().is_some_and(|n| n.ends_with(".json")) {
            count += 1;
        }
    }
    Some(count)
}

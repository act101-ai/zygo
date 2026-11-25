//! The unread mail count of a member session (M§9a).
//!
//! The count is the store's unread count (M§5): the regular files named `<ULID>.json` in
//! `<workspaces_dir>/<ws id>/mail/<pane key>/new/`; a missing `new/` counts 0. The entry type comes from the
//! directory read: no link is followed and no message file is opened, so no message text reaches the HUD (M-C2).
//! The status-line module does not depend on `zygo-workspace`, so the pane key check and the ULID check are copies
//! of `zygo_workspace::identity::valid_pane_key` and `zygo_workspace::mail::store::valid_id`.

use std::time::Instant;

use zygo_core::paths::Paths;
use zygo_core::session_view::SessionView;

use super::phase::valid_ws_id;

/// `^k[a-z2-7]{8}$` on the whole string (WS AU84); equal to `zygo_workspace::identity::valid_pane_key`.
fn valid_pane_key(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 9 && b[0] == b'k' && b[1..].iter().all(|c| matches!(c, b'a'..=b'z' | b'2'..=b'7'))
}

/// `^[0-9A-HJKMNP-TV-Z]{26}$` on the whole string; equal to `zygo_workspace::mail::store::valid_id`.
fn valid_id(s: &str) -> bool {
    s.len() == 26 && s.bytes().all(|c| c.is_ascii_digit() || (c.is_ascii_uppercase() && !b"ILOU".contains(&c)))
}

/// The unread count: regular `<ULID>.json` files in `new/`, 0 when `new/` is missing; `None` outside a workspace,
/// with an invalid ws id or pane key (no path is built), after `deadline`, or on another read error.
pub fn unread(paths: &Paths, view: &SessionView, deadline: Instant) -> Option<usize> {
    let member = &view.workspace.as_ref()?.member;
    if !valid_ws_id(&member.ws_id) || !valid_pane_key(&member.pane) {
        return None;
    }
    let dir = paths.workspaces_dir().join(&member.ws_id).join("mail").join(&member.pane).join("new");
    if Instant::now() >= deadline {
        return None;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(0),
        Err(_) => return None,
    };
    let mut count = 0;
    for entry in entries {
        if Instant::now() >= deadline {
            return None;
        }
        let entry = entry.ok()?;
        // The type comes from the directory read; a link is not followed.
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        if entry.file_name().to_str().and_then(|n| n.strip_suffix(".json")).is_some_and(valid_id) {
            count += 1;
        }
    }
    Some(count)
}

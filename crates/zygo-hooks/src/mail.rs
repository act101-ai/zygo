//! Workspace mail in the session text (M§8a). Owner: node ML-9.
use zygo_core::esc;
use zygo_workspace::mail::names;
use zygo_workspace::state::{MemberRecord, Workspace};

const MAIL_TAIL: &str = "Send with mail_send (to a name, or `all`); read with mail_read; mail_list shows unread mail and the current members. zygo tells you when mail arrives: before your turn ends, at your next prompt, or with a wake line. Do not poll or wait for mail: end your turn, and you will be woken.";

/// Item 8 of the session text (M§8a item 1) for the member `own_key` of `ws`: its name and the workspace name, or the
/// no-name clause with its pane key, then the valid names of every member in key order. Each name passes `esc`.
pub fn session_item(ws: &Workspace, own_key: &str) -> String {
    let first = match ws.members.get(own_key).and_then(MemberRecord::valid_name) {
        Some(name) => format!("you are `{}` in workspace `{}`", esc(name), esc(&ws.name)),
        None => format!("you have no mail name; others reach you by pane {}", esc(own_key)),
    };
    let names: Vec<String> = names::members(ws).into_iter().filter_map(|t| t.name).map(|n| esc(&n)).collect();
    format!("Workspace mail: {first}. Members: `{}`. {MAIL_TAIL}", names.join(", "))
}

//! Workspace mail in the session text (M§8a; owner: node ML-9) and the hook delivery texts (M§7.1, M§7.2; owner:
//! node ML-10).
use serde_json::json;
use zygo_core::esc;
use zygo_core::paths::Paths;
use zygo_mail_sync::registry::read_cache;
use zygo_mail_sync::state::SyncPaths;
use zygo_workspace::mail::names;
use zygo_workspace::state::Workspace;

const MAIL_TAIL: &str = "Send with mail_send (to a name, or `all`); read with mail_read; mail_list shows unread mail and the current members. zygo tells you when mail arrives: before your turn ends, at your next prompt, or with a wake line. Do not poll or wait for mail: end your turn, and you will be woken.";

/// The most addresses the "Other machines" clause of item 8 names (mail sync design S§10.4).
const OTHER_MACHINES_MAX: usize = 64;

/// The "Other machines" clause of item 8 (mail sync design S§10.4): each address of `addresses` once, first occurrence
/// kept, each passing `esc`, at most 64, then `and <n> more; mail_list lists all` for the `n` distinct addresses left
/// out. `None` for no address.
pub fn other_machines(addresses: &[String]) -> Option<String> {
    let mut distinct: Vec<&String> = Vec::new();
    for a in addresses {
        if !distinct.contains(&a) {
            distinct.push(a);
        }
    }
    if distinct.is_empty() {
        return None;
    }
    let mut list: Vec<String> = distinct.iter().take(OTHER_MACHINES_MAX).map(|a| esc(a)).collect();
    if distinct.len() > OTHER_MACHINES_MAX {
        list.push(format!("and {} more; mail_list lists all", distinct.len() - OTHER_MACHINES_MAX));
    }
    Some(format!("Other machines: {}; mail them by the full address.", list.join(", ")))
}

/// Item 8 of the session text (M§8a item 1) for the member `own_key` of `ws`, with no remote address:
/// `session_item_with(ws, own_key, &[])`.
pub fn session_item(ws: &Workspace, own_key: &str) -> Option<String> {
    session_item_with(ws, own_key, &[])
}

/// Item 8 of the session text (M§8a item 1) for the member `own_key` of `ws`: its name and the workspace name, or the
/// no-name clause with its pane key, then the valid names of the members in key order, each name once, then the
/// "Other machines" clause of `remote` (S§10.4) when `remote` is not empty. Each name passes `esc`. `None` when
/// `own_key` has no member record: no one can reach that pane by mail (`names::resolve`).
pub fn session_item_with(ws: &Workspace, own_key: &str, remote: &[String]) -> Option<String> {
    let own = ws.members.get(own_key)?;
    let first = match own.valid_name() {
        Some(name) => format!("you are `{}` in workspace `{}`", esc(name), esc(&ws.name)),
        None => format!("you have no mail name; others reach you by pane {}", esc(own_key)),
    };
    let mut names: Vec<String> = Vec::new();
    for n in names::members(ws).into_iter().filter_map(|t| t.name) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let names: Vec<String> = names.iter().map(|n| esc(n)).collect();
    let other = other_machines(remote).map_or(String::new(), |c| format!(" {c}"));
    Some(format!("Workspace mail: {first}. Members: `{}`.{other} {MAIL_TAIL}", names.join(", ")))
}

/// The remote addresses of item 8 (S§10.4, S§7.3): from `mail/sync/members.json` when sync is on for `ws_name`; reads
/// config and the cache only, runs no git. A config that does not load, sync off or a bad id: none, and no line (S-C1).
pub(crate) fn remote_addresses(paths: &Paths, ws_id: &str, ws_name: &str) -> Vec<String> {
    let Ok(m) = zygo_config::load(&zygo_config::LoadContext { paths: paths.clone(), repo: None, team: None }) else {
        return Vec::new();
    };
    if zygo_config::sync_for(&m.config, ws_name).is_err() {
        return Vec::new();
    }
    SyncPaths::new(paths, ws_id).map_or_else(|_| Vec::new(), |sp| read_cache(&sp).addresses())
}

/// The delivery line of `n` unread messages (M§7.2): the prompt line, the SessionStart line and the Stop reason.
pub fn delivery_line(n: usize) -> String {
    format!("zygo: {n} new messages; read them with mail_read")
}

/// The Stop hook output that blocks the end of a turn once for `n` unread messages (M§7.1).
pub fn stop_block(n: usize) -> String {
    json!({"decision": "block", "reason": delivery_line(n)}).to_string()
}

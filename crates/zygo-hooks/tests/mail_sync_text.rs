//! The "Other machines" clause of session text item 8 (mail sync design S§10.4, S§19 #27 session text part;
//! M§8a item 1). The cache is written with `write_cache`; no test runs git or the sync child.
use zygo_core::Agent;
use zygo_hooks::mail;
use zygo_workspace::state::{MemberRecord, Workspace};

const MAIL_TAIL: &str = "Send with mail_send (to a name, or `all`); read with mail_read; mail_list shows unread mail and the current members. zygo tells you when mail arrives: before your turn ends, at your next prompt, or with a wake line. Do not poll or wait for mail: end your turn, and you will be woken.";
const ONE: &str = "Other machines: bob@b; mail them by the full address.";
const ID: &str = "0123456789abcdef";
const KEY: &str = "kaaaaaaaa";

fn addrs(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("m{i}@b")).collect()
}
fn first64(a: &[String]) -> String {
    a[..64].join(", ")
}
/// Item 8 of the member `elena` alone in `app`, with `clause` after the member list.
fn item(clause: Option<&str>) -> String {
    let c = clause.map_or(String::new(), |c| format!("{c} "));
    format!("Workspace mail: you are `elena` in workspace `app`. Members: `elena`. {c}{MAIL_TAIL}")
}

#[test] // S§10.4
fn other_machines_lists_each_address_then_the_rule() {
    assert_eq!(mail::other_machines(&["bob@b".to_string()]).as_deref(), Some(ONE));
    assert_eq!(
        mail::other_machines(&["bob@b".to_string(), "eve@c".to_string()]).as_deref(),
        Some("Other machines: bob@b, eve@c; mail them by the full address.")
    );
}

#[test] // S§10.4: at most 64 addresses, then `and <n> more; mail_list lists all`
fn other_machines_holds_at_most_64_addresses() {
    let a = addrs(65);
    assert_eq!(
        mail::other_machines(&a),
        Some(format!("Other machines: {}, and 1 more; mail_list lists all; mail them by the full address.", first64(&a)))
    );
    assert!(!mail::other_machines(&a).unwrap().contains("m64@b"));
    assert_eq!(mail::other_machines(&a[..64]), Some(format!("Other machines: {}; mail them by the full address.", first64(&a))));
    let a = addrs(300);
    assert_eq!(
        mail::other_machines(&a),
        Some(format!("Other machines: {}, and 236 more; mail_list lists all; mail them by the full address.", first64(&a)))
    );
}

#[test] // S§10.4: only when the member cache is not empty
fn other_machines_is_none_for_no_address() {
    assert_eq!(mail::other_machines(&[]), None);
}

#[test] // S§10.4: the clause comes after the member list; M§8a: no addresses give today's item
fn session_item_with_puts_the_clause_after_the_member_list() {
    let mut ws = Workspace::new(ID);
    ws.name = "app".into();
    ws.members.insert(KEY.into(), MemberRecord { name: Some("elena".into()), ..MemberRecord::new(Agent::ClaudeCode) });
    assert_eq!(mail::session_item_with(&ws, KEY, &["bob@b".to_string()]), Some(item(Some(ONE))));
    assert_eq!(mail::session_item_with(&ws, KEY, &[]), Some(item(None)));
    assert_eq!(mail::session_item_with(&ws, KEY, &[]), mail::session_item(&ws, KEY));
    assert_eq!(mail::session_item_with(&ws, "kbbbbbbbb", &["bob@b".to_string()]), None);
}

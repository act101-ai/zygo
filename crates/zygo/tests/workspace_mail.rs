//! ML-18: end-to-end workspace mail on tmux with stub agents (M§13 #1, #2, #4-#8; M-G1 to M-G6). Every case runs a
//! workspace verb, so the file is cfg(unix) (OP5).
#![cfg(unix)]
mod common;
#[cfg(unix)]
#[path = "common/e2e.rs"]
mod e2e;

use std::time::{Duration, Instant};

use serde_json::json;
use zygo_workspace::state::Workspace;

use crate::common::stub_control;
use crate::common::workspace::Harness;
use crate::e2e::*;

/// Loadout `mail`: one window, two Claude Code panes named `elena` and `bob` (M§6, M§11).
const TEAM: &str = "[loadout.mail]\n[[loadout.mail.window]]\n[[loadout.mail.window.pane]]\nkind = \"claude-code\"\nname = \"elena\"\n[[loadout.mail.window.pane]]\nkind = \"claude-code\"\nname = \"bob\"\n";

struct Team {
    id: String,
    ws: Workspace,
    elena: String,
    bob: String,
}

/// git_repo_at(h, &h.repo()); per_pane_claude(h, ""); config(h, TEAM); `cli(h, &["up", "--loadout", "mail",
/// "--no-attach"], &[])` exit 0; only_state; the pane key of each name from `ws.panes[].name`; wait_starts(h, k, 1)
/// for both; then asserts `state.members[k].name == Some(<name>)` for both.
fn team(h: &Harness) -> Team {
    git_repo_at(h, &h.repo());
    per_pane_claude(h, "");
    config(h, TEAM);
    let o = cli(h, &["up", "--loadout", "mail", "--no-attach"], &[]);
    assert_eq!(o.status.code(), Some(0), "up: {}{}", text(&o.stdout), text(&o.stderr));
    let (id, ws) = only_state(h);
    let key = |n: &str| {
        ws.panes
            .iter()
            .find(|p| p.name.as_deref() == Some(n))
            .unwrap_or_else(|| panic!("no pane {n}: {:?}", ws.panes.iter().map(|p| &p.name).collect::<Vec<_>>()))
            .key
            .clone()
    };
    let (elena, bob) = (key("elena"), key("bob"));
    for k in [&elena, &bob] {
        assert!(wait_starts(h, k, 1), "{k} did not start");
    }
    let st = state(h, &id);
    for (k, n) in [(&elena, "elena"), (&bob, "bob")] {
        assert_eq!(st.members[k.as_str()].name.as_deref(), Some(n));
    }
    Team { id, ws, elena, bob }
}

/// The M§13 #8 scan in two parts: while the members run, then after `down`.
fn scan_end(h: &Harness, t: &Team, m: &[&str]) {
    assert_no_mail_text(h, &t.id, m);
    down(h, &t.ws.name);
    assert_no_mail_text(h, &t.id, m);
}

#[test]
#[ignore = "timing: tmux e2e: 2 s wake and 1 s watch bounds, 2 s tmux calls and launcher probes"]
fn m13_1_mail_survives_down_up_and_a_killed_server() {
    let Some(h) = Harness::new() else { return };
    let t = team(&h);
    let m1 = mail_send_cli(&h, "elena", "ML18-S1-down", "ML18-B1-down\nline two");
    down(&h, &t.ws.name);
    assert_eq!(unread(&h, &t.id, &t.elena), 1);
    let ls = cli(&h, &["ls"], &[]);
    assert!(text(&ls.stdout).lines().any(|l| l.ends_with("  unread 1")), "{}", text(&ls.stdout)); // ListRow.unread
    let n = hook_records(&h, &t.elena, "SessionStart").len();
    assert_eq!(up(&h, &t.ws.name, &[]).status.code(), Some(0));
    assert!(wait_starts(&h, &t.elena, 2));
    let line = "zygo: 1 new messages; read them with mail_read";
    assert!(
        hook_records(&h, &t.elena, "SessionStart")[n..].iter().any(|r| r["out"].as_str().is_some_and(|o| o.contains(line))),
        "{:?}",
        hook_records(&h, &t.elena, "SessionStart")
    );
    let r = mail_tool(&h, &t.elena, "mail_read", json!({}));
    assert_eq!(r["messages"].as_array().unwrap().len(), 1, "{r}");
    let m = &r["messages"][0];
    assert_eq!((m["id"].as_str(), m["from"].as_str(), m["to"].as_str()), (Some(m1.as_str()), Some("cli"), Some("elena")));
    assert_eq!((m["subject"].as_str(), m["body"].as_str()), (Some("ML18-S1-down"), Some("ML18-B1-down\nline two")));
    let m2 = mail_send_cli(&h, "elena", "ML18-S1-kill", "ML18-B1-kill");
    h.server.as_ref().unwrap().kill_server();
    assert!(until(BOUND, || settled(&h, &t.id)));
    let o = cli(&h, &["up", "--all"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(wait_starts(&h, &t.elena, 3));
    let r = mail_tool(&h, &t.elena, "mail_read", json!({}));
    assert_eq!(r["messages"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!((r["messages"][0]["id"].as_str(), r["messages"][0]["body"].as_str()), (Some(m2.as_str()), Some("ML18-B1-kill")));
    assert_eq!(unread(&h, &t.id, &t.elena), 0);
    scan_end(&h, &t, &["ML18-S1-down", "ML18-B1-down", "ML18-S1-kill", "ML18-B1-kill"]);
}

#[test]
#[ignore = "timing: tmux e2e: 2 s wake and 1 s watch bounds, 2 s tmux calls and launcher probes"]
fn m13_2_send_by_name_reaches_the_right_member_after_every_pane_id_changed() {
    let Some(h) = Harness::new() else { return };
    let t = team(&h);
    let s = t.ws.tmux_session.clone();
    let old: Vec<String> = [&t.elena, &t.bob].iter().map(|k| row(&h, &s, k).id).collect();
    h.tmux(&["new-session", "-d", "-s", "hold", "-x", "80", "-y", "24", "/bin/sleep 600"]);
    down(&h, &t.ws.name);
    assert_eq!(up(&h, &t.ws.name, &[]).status.code(), Some(0));
    for k in [&t.elena, &t.bob] {
        assert!(wait_starts(&h, k, 2));
    }
    let (pe, pb) = (row(&h, &s, &t.elena).id, row(&h, &s, &t.bob).id);
    assert!(!old.contains(&pe) && !old.contains(&pb), "old {old:?}, new {pe} {pb}");
    let id = mail_send_cli(&h, "elena", "ML18-S2-cli", "ML18-B2-cli");
    assert!(until(Duration::from_secs(2), || wakes(&h, &pe) == 1), "{}", capture(&h, &pe));
    assert_eq!(wakes(&h, &pb), 0, "{}", capture(&h, &pb));
    let r = mail_tool(&h, &t.bob, "mail_send", json!({"to": "elena", "subject": "ML18-S2-mcp", "body": "ML18-B2-mcp"}));
    assert_eq!((r["sent"][0]["to"].as_str(), r["refused"].as_array().map(Vec::len)), (Some("elena"), Some(0)), "{r}");
    assert_eq!(mail_tool(&h, &t.bob, "mail_read", json!({}))["messages"], json!([]));
    let got = mail_tool(&h, &t.elena, "mail_read", json!({}));
    let pairs: Vec<(&str, &str)> =
        got["messages"].as_array().unwrap().iter().map(|m| (m["from"].as_str().unwrap(), m["subject"].as_str().unwrap())).collect();
    assert_eq!(pairs, [("cli", "ML18-S2-cli"), ("bob", "ML18-S2-mcp")]);
    assert_eq!(got["messages"][0]["id"].as_str(), Some(id.as_str()));
    scan_end(&h, &t, &["ML18-S2-cli", "ML18-B2-cli", "ML18-S2-mcp", "ML18-B2-mcp"]);
}

#[test]
#[ignore = "timing: tmux e2e: 2 s wake and 1 s watch bounds, 2 s tmux calls and launcher probes"]
fn m13_4_a_member_in_a_turn_gets_one_stop_block_and_reads() {
    let Some(h) = Harness::new() else { return };
    let t = team(&h);
    let pe = row(&h, &t.ws.tmux_session, &t.elena).id;
    let c = ctl(&h, &t.elena);
    let block = json!({"decision": "block", "reason": "zygo: 1 new messages; read them with mail_read"});
    stub_control(&c, "hook UserPromptSubmit").unwrap();
    assert!(wait_done(&h, &t.elena, "hook UserPromptSubmit", 1));
    assert_eq!(activity(&sdir(&h, &t.elena))["busy"], true);
    let id = mail_send_cli(&h, "elena", "ML18-S4a", "ML18-B4a");
    assert_eq!(wakes(&h, &pe), 0, "a busy member is not woken");
    let n = hook_records(&h, &t.elena, "Stop").len();
    stub_control(&c, "hook Stop").unwrap();
    assert!(wait_done(&h, &t.elena, "hook Stop", 1));
    assert_eq!(hook_reply(&h, &t.elena, "Stop", n, "workspace --event stop"), Some(block.clone()));
    assert_eq!(activity(&sdir(&h, &t.elena))["busy"], true, "a block keeps the turn open");
    let r = mail_tool(&h, &t.elena, "mail_read", json!({}));
    assert_eq!((r["messages"][0]["id"].as_str(), r["messages"][0]["body"].as_str()), (Some(id.as_str()), Some("ML18-B4a")));
    mail_send_cli(&h, "elena", "ML18-S4b", "ML18-B4b"); // new mail in the forced continuation
    let n = hook_records(&h, &t.elena, "Stop").len();
    let again = r#"hook Stop {"stop_hook_active":true}"#;
    stub_control(&c, again).unwrap();
    assert!(wait_done(&h, &t.elena, again, 1));
    assert_eq!(hook_reply(&h, &t.elena, "Stop", n, "workspace --event stop"), None, "no second block");
    assert_eq!(activity(&sdir(&h, &t.elena))["busy"], false);
    let blocks = hook_records(&h, &t.elena, "Stop")
        .iter()
        .filter(|r| hook_out(r, "workspace --event stop").is_some_and(|v| v["decision"] == "block"))
        .count();
    assert_eq!(blocks, 1);
    assert_eq!(wakes(&h, &pe), 0);
    assert_eq!(unread(&h, &t.id, &t.elena), 1); // ML18-S4b waits for the next turn (M§7.2)
    scan_end(&h, &t, &["ML18-S4a", "ML18-B4a", "ML18-S4b", "ML18-B4b"]);
}

#[test]
#[ignore = "timing: tmux e2e: 2 s wake and 1 s watch bounds, 2 s tmux calls and launcher probes"]
fn m13_5_an_idle_member_is_woken_within_2_s_and_once_per_30_s() {
    let Some(h) = Harness::new() else { return };
    let t = team(&h);
    let s = &t.ws.tmux_session;
    let (pe, pb) = (row(&h, s, &t.elena).id, row(&h, s, &t.bob).id);
    assert_eq!(wakes(&h, &pe), 0);
    let t0 = Instant::now();
    mail_send_cli(&h, "elena", "ML18-S5a", "ML18-B5a");
    assert!(until(Duration::from_secs(2), || wakes(&h, &pe) == 1), "{}", capture(&h, &pe));
    assert!(t0.elapsed() <= Duration::from_secs(2), "woken after {:?}", t0.elapsed());
    let first = json_file(&wake_file(&h, &t.id, &t.elena));
    mail_send_cli(&h, "elena", "ML18-S5b", "ML18-B5b");
    assert!(t0.elapsed() < Duration::from_secs(30));
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(wakes(&h, &pe), 1, "{}", capture(&h, &pe));
    assert_eq!(json_file(&wake_file(&h, &t.id, &t.elena)), first);
    assert_eq!(wakes(&h, &pb), 0);
    assert!(!capture(&h, &pe).contains("ML18-"), "no mail text is typed (M-G4, M-C5)");
    assert_eq!(unread(&h, &t.id, &t.elena), 2);
    scan_end(&h, &t, &["ML18-S5a", "ML18-B5a", "ML18-S5b", "ML18-B5b"]);
}

#[test]
#[ignore = "timing: tmux e2e: 2 s wake and 1 s watch bounds, 2 s tmux calls and launcher probes"]
fn m13_6_a_pane_whose_zygo_pane_changed_is_not_woken() {
    let Some(h) = Harness::new() else { return };
    let t = team(&h);
    let pe = row(&h, &t.ws.tmux_session, &t.elena).id;
    assert!(!t.ws.members.contains_key("kzzzzzzzz"));
    h.tmux(&["set-option", "-p", "-t", &pe, "@zygo-pane", "kzzzzzzzz"]);
    mail_send_cli(&h, "elena", "ML18-S6a", "ML18-B6a");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(wakes(&h, &pe), 0, "{}", capture(&h, &pe));
    assert!(!wake_file(&h, &t.id, &t.elena).exists());
    assert_eq!(unread(&h, &t.id, &t.elena), 1);
    h.tmux(&["set-option", "-p", "-t", &pe, "@zygo-pane", &t.elena]);
    mail_send_cli(&h, "elena", "ML18-S6b", "ML18-B6b");
    assert!(until(Duration::from_secs(2), || wakes(&h, &pe) == 1), "{}", capture(&h, &pe));
    scan_end(&h, &t, &["ML18-S6a", "ML18-B6a", "ML18-S6b", "ML18-B6b"]);
}

#[test]
#[ignore = "timing: tmux e2e: 2 s wake and 1 s watch bounds, 2 s tmux calls and launcher probes"]
fn m13_7_mail_watch_returns_within_1_s_and_exits_1_at_its_timeout() {
    let Some(h) = Harness::new() else { return };
    let t = team(&h);
    let t0 = Instant::now();
    let o = cli(&h, &["mail", "watch", "elena", "--timeout", "2s"], &[]);
    let el = t0.elapsed();
    assert_eq!(o.status.code(), Some(1), "{}", text(&o.stderr));
    assert_eq!(text(&o.stdout), "");
    assert!(el >= Duration::from_secs(2) && el < Duration::from_secs(4), "{el:?}");
    let w = spawn_down(&h, &["mail", "watch", "elena", "--timeout", "30s"], &[]);
    std::thread::sleep(Duration::from_secs(1)); // the watch is in its poll loop
    let body = body_file(&h, "ML18-S7", "ML18-B7");
    let t0 = Instant::now();
    let s = spawn_down(&h, &["mail", "send", "elena", "--subject", "ML18-S7", "--file", body.to_str().unwrap()], &[]);
    let o = w.output("mail watch");
    let el = t0.elapsed();
    let so = s.output("mail send");
    assert_eq!(so.status.code(), Some(0), "{}", text(&so.stderr));
    let id = text(&so.stdout).strip_prefix("sent ").and_then(|r| r.split(' ').next()).unwrap().to_string();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(el <= Duration::from_secs(1), "{el:?}");
    assert_eq!(text(&o.stdout), format!("{id} cli ML18-S7\n"));
    assert_eq!(unread(&h, &t.id, &t.elena), 1, "watch moves no file");
    scan_end(&h, &t, &["ML18-S7", "ML18-B7"]);
}

#[test]
#[ignore = "timing: tmux e2e: 2 s wake and 1 s watch bounds, 2 s tmux calls and launcher probes"]
fn m13_8_no_mail_text_outside_the_maildirs() {
    let Some(h) = Harness::new() else { return };
    let t = team(&h);
    let c = ctl(&h, &t.elena);
    let r = mail_tool(&h, &t.bob, "mail_send", json!({"to": "elena", "subject": "ML18-S8-mcp", "body": "ML18-B8-mcp"}));
    assert_eq!(r["refused"], json!([]), "{r}");
    let all = body_file(&h, "all", "ML18-B8-all");
    let out = cli(&h, &["mail", "send", "all", "--subject", "ML18-S8-all", "--file", all.to_str().unwrap()], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let n = hook_records(&h, &t.elena, "UserPromptSubmit").len();
    stub_control(&c, "hook UserPromptSubmit").unwrap();
    assert!(wait_done(&h, &t.elena, "hook UserPromptSubmit", 1));
    assert_eq!(
        hook_reply(&h, &t.elena, "UserPromptSubmit", n, "workspace --event prompt"),
        Some(json!({"hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "additionalContext": "zygo: 2 new messages; read them with mail_read"}}))
    );
    let n = hook_records(&h, &t.elena, "Stop").len();
    stub_control(&c, "hook Stop").unwrap();
    assert!(wait_done(&h, &t.elena, "hook Stop", 1));
    assert_eq!(
        hook_reply(&h, &t.elena, "Stop", n, "workspace --event stop"),
        Some(json!({"decision": "block", "reason": "zygo: 2 new messages; read them with mail_read"}))
    );
    assert_eq!(mail_tool(&h, &t.elena, "mail_list", json!({}))["unread"], 2);
    assert_eq!(mail_tool(&h, &t.elena, "mail_read", json!({}))["messages"].as_array().unwrap().len(), 2);
    // bob's one message (from `all`) is read by `mail read --name bob`, so the watch after it waits and exits 1.
    for (args, code) in [
        (&["mail", "ls", "bob"][..], 0),
        (&["mail", "read", "--name", "bob"], 0),
        (&["mail", "watch", "bob", "--timeout", "1s"], 1),
        (&["ls"], 0),
        (&["workspace"], 0),
    ] {
        let o = cli(&h, args, &[]);
        assert_eq!(o.status.code(), Some(code), "{args:?}: {}{}", text(&o.stdout), text(&o.stderr));
    }
    const M: [&str; 4] = ["ML18-S8-mcp", "ML18-B8-mcp", "ML18-S8-all", "ML18-B8-all"];
    let live = assert_no_mail_text(&h, &t.id, &M);
    // Only a member that had a turn has an `activity.json` (no file = idle, `zygo-workspace/src/activity.rs:40`).
    assert!(live.contains(&sdir(&h, &t.elena).join("activity.json")), "{live:?}");
    for k in [&t.elena, &t.bob] {
        assert!(live.iter().any(|p| p.starts_with(sdir(&h, k))), "{k}: {live:?}");
    }
    down(&h, &t.ws.name);
    let after = assert_no_mail_text(&h, &t.id, &M);
    assert!(after.contains(&h.store(&t.id).dir().join("workspace.json")), "{after:?}");
}

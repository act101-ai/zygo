//! E2: slash commands, saved overrides, OpenCode and the membership path end to end (W§18 #60, #66-#69, #71, #74).
//! Every case runs a workspace verb, so the file is cfg(unix) (OP5).
#![cfg(unix)]
mod common;
#[cfg(unix)]
#[path = "common/e2e.rs"]
mod e2e;

use std::ffi::{OsStr, OsString};
use std::time::{Duration, Instant};

use common::stub_control;
use common::workspace::{Guarded, Harness};
use e2e::*;
use serde_json::{json, Value};
use zygo_proxy::client::{SessionClient, SessionSettings, WindowArg};
use zygo_workspace::state::{CompactOverride, MemberRecord, MemberState, Phase, Workspace};

/// Loadout `l`: one window with one `claude-code` pane and one `shell` pane.
const L1: &str = "[loadout.l]\n[[loadout.l.window]]\n[[loadout.l.window.pane]]\nkind = \"claude-code\"\n\
                  [[loadout.l.window.pane]]\nkind = \"shell\"\n";

/// The repository, the config `toml`, then `up --loadout l --no-attach` with `extra` (exit 0); the id, the name and
/// the state of the one workspace.
fn ws_up(h: &Harness, toml: &str, extra: &[(&str, &OsStr)]) -> (String, String, Workspace) {
    git_repo_at(h, &h.repo());
    config(h, toml);
    let o = cli(h, &["up", "--loadout", "l", "--no-attach"], extra);
    assert_eq!(o.status.code(), Some(0), "up: {}{}", text(&o.stdout), text(&o.stderr));
    let (id, ws) = only_state(h);
    (id, ws.name.clone(), ws)
}

/// One stub step of the pane `k`, then a wait ([`BOUND`]) for its `done` line.
fn step(h: &Harness, k: &str, line: &str) {
    let n = done_count(h, k, line);
    stub_control(&ctl(h, k), line).unwrap();
    assert!(
        wait_done(h, k, line, n + 1),
        "step `{line}` not done: {}",
        std::fs::read_to_string(ctl(h, k).join("done")).unwrap_or_default()
    );
}

/// The `UserPromptSubmit` step of the stub of `k` with the prompt `line`: the step line, and the number of records
/// of the event before it.
fn prompt_step(h: &Harness, k: &str, line: &str) -> (String, usize) {
    let n = hook_records(h, k, "UserPromptSubmit").len();
    let s = format!("hook UserPromptSubmit {}", json!({ "prompt": line }));
    stub_control(&ctl(h, k), &s).unwrap();
    (s, n)
}

/// A `/zygo:` line typed in the member `k`: the reply of its `hook prompt` record (E2 decision 2: the record whose
/// `command` ends with `hook prompt`, never the last record, because the member's `workspace --event prompt` record
/// follows it).
fn slash(h: &Harness, k: &str, line: &str) -> Value {
    let (_, n) = prompt_step(h, k, line);
    let mut got = None;
    assert!(
        until(BOUND, || {
            got = hook_reply(h, k, "UserPromptSubmit", n, "hook prompt");
            got.is_some()
        }),
        "no reply to {line}: {:?}",
        hook_records(h, k, "UserPromptSubmit")
    );
    got.unwrap()
}

/// The record of the member `k` of the state `id`.
fn member(h: &Harness, id: &str, k: &str) -> MemberRecord {
    state(h, id).members[k].clone()
}

/// `busy` of the activity file of the pane `k`; `false` when the file is missing.
fn busy(h: &Harness, k: &str) -> bool {
    let p = sdir(h, k).join("activity.json");
    p.exists() && activity(&sdir(h, k))["busy"] == true
}

/// Waits ([`BOUND`]) until no launcher of the state `id` is live and the member `k` is `want`.
fn settle(h: &Harness, id: &str, k: &str, want: MemberState) {
    assert!(
        until(BOUND, || settled(h, id) && member(h, id, k).state == want),
        "not settled to {want:?}: {:?}",
        state(h, id).members
    );
}

/// `up <name>`: exit 0.
fn up_ok(h: &Harness, name: &str) {
    let o = up(h, name, &[]);
    assert_eq!(o.status.code(), Some(0), "up: {}{}", text(&o.stdout), text(&o.stderr));
}

/// `zygo <args>` with the member environment of `k` (`ZYGO_SESSION`, `ZYGO_SESSION_DIR`, `ZYGO_AGENT`,
/// `ZYGO_WORKSPACE`); stdout, exit 0.
fn in_member(h: &Harness, k: &str, args: &[&str]) -> String {
    let env: Vec<(String, OsString)> = member_env(h, k);
    let extra: Vec<(&str, &OsStr)> = env.iter().map(|(n, v)| (n.as_str(), v.as_os_str())).collect();
    let o = Guarded::spawn(h.zygo(args, &extra)).output(args[0]);
    assert_eq!(o.status.code(), Some(0), "{args:?}: {}{}", text(&o.stdout), text(&o.stderr));
    text(&o.stdout)
}

/// The agent session id of the first start of `k`, once the record holds it.
fn committed(h: &Harness, id: &str, k: &str) -> String {
    let (_, sid) = session_arg(h, k);
    assert!(until(BOUND, || member(h, id, k).agent_session.as_deref() == Some(sid.as_str())), "{:?}", member(h, id, k));
    sid
}

/// The ending of a stub whose test is over: `exit 0` when it still runs, then `down --now --wait`.
fn end(h: &Harness, k: &str, name: &str) {
    stub_control(&ctl(h, k), "exit 0").unwrap();
    down(h, name);
}

// ---- W§18 #60 (AU72) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_60_the_note_inputs_are_in_the_saved_view_before_session_start() {
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, &copy_prelude(&h, "view.json", "views"));
    let (id, name, ws) = ws_up(&h, L1, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    let sid = committed(&h, &id, &k);
    h.server.as_ref().unwrap().kill_server();
    settle(&h, &id, &k, MemberState::Resumable);
    up_ok(&h, &name);
    assert!(wait_starts(&h, &k, 2));
    let s2 = stub_env(&h, &k)["ZYGO_SESSION"].as_str().unwrap().to_string();
    let v = json_file(&h.root().join("views").join(format!("{s2}.json")));
    let note = &v["workspace"]["resume_note"];
    assert_eq!(note["clean_stop"], false, "{v}");
    assert_eq!(note["agent_session"], sid.as_str(), "{v}");
    let outs = session_start_outs(&h, &k);
    assert!(outs[1].contains(CRASH), "{outs:?}");
    let now = json_file(&sdir(&h, &k).join("view.json"));
    assert!(now["workspace"]["resume_note"].is_null(), "{now}");
    end(&h, &k, &name);
}

// ---- W§18 #66 (OP11, AU18) ----

#[test]
#[ignore = "timing: asserts the reply within 4.5 s; 5 s polls"]
fn w18_66_slash_down_ls_and_workspace_in_a_member() {
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "");
    let (id, name, ws) = ws_up(&h, L1, &[]);
    let k = agent_key(&ws, |_| true);
    let s = ws.tmux_session.clone();
    assert!(wait_starts(&h, &k, 1));
    let sid = committed(&h, &id, &k);
    let ls = slash(&h, &k, "/zygo:ls");
    let row = format!("zygo: {name}  {}  up  ", ws.main_root.display());
    assert!(ls["reason"].as_str().is_some_and(|r| r.starts_with(&row) && r.lines().count() == 1), "{ls}");
    let wsr = slash(&h, &k, "/zygo:workspace");
    assert_eq!(wsr["reason"], format!("zygo: workspace {name}: up\n{k} claude-code running {sid}"), "{wsr}");
    let help = slash(&h, &k, "/zygo:help")["reason"].clone();
    assert!(help.as_str().is_some_and(|t| t.contains("down")), "{help}");
    assert_eq!(slash(&h, &k, "/zygo:up")["reason"], help);
    assert_eq!(slash(&h, &k, "/zygo:attach")["reason"], help);
    let want = json!({
        "decision": "block",
        "reason": "zygo: down started; end your turn",
        "hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "suppressOriginalPrompt": true}
    });
    let reply = |n: usize| hook_reply(&h, &k, "UserPromptSubmit", n, "hook prompt");
    // `/zygo:down`: the reply within the hook bound, the detached `down` stops the workspace, no turn is set. The
    // detached `down` can end the stub before its `done` line, so the wait is on the record, not on `done`.
    let t = Instant::now();
    let (line, n) = prompt_step(&h, &k, "/zygo:down");
    assert!(until(FIVE, || reply(n).is_some()), "no reply to /zygo:down");
    let took = t.elapsed();
    assert!(took < Duration::from_millis(4500), "the reply took {took:?}");
    assert_eq!(reply(n), Some(want.clone()));
    assert!(until(BOUND, || phase(&h, &id) == Phase::Down && !session_up(&h, &s)), "{:?}", phase(&h, &id));
    assert!(!busy(&h, &k), "the consumed line set busy");
    skip_unfinished(&h, &id, &k, &line);
    // `/zygo:down --now` stops a busy member at once.
    up_ok(&h, &name);
    assert!(wait_starts(&h, &k, 2));
    step(&h, &k, "hook UserPromptSubmit");
    assert!(busy(&h, &k));
    let (line, n) = prompt_step(&h, &k, "/zygo:down --now");
    assert!(until(FIVE, || reply(n).is_some()), "no reply to /zygo:down --now");
    assert_eq!(reply(n), Some(want));
    assert!(until(FIVE, || phase(&h, &id) == Phase::Down), "{:?}", phase(&h, &id));
    skip_unfinished(&h, &id, &k, &line);
}

/// E2 D-3: a stub step that the detached `down` ended before its `done` line would run again at the next start (the
/// stub's cursor counts the `done` lines). Once no launcher of the state `id` is live, the line is written to `done`
/// for the stub.
fn skip_unfinished(h: &Harness, id: &str, k: &str, line: &str) {
    use std::io::Write;
    assert!(until(BOUND, || settled(h, id)), "{:?}", state(h, id).members);
    if done_count(h, k, line) == 0 {
        let mut f = std::fs::OpenOptions::new().append(true).open(ctl(h, k).join("done")).unwrap();
        f.write_all(format!("{line}\n").as_bytes()).unwrap();
    }
}

// ---- W§18 #67 (OP12, AU105) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_67_overrides_are_saved_at_once_and_restored_after_a_crash() {
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, &copy_prelude(&h, "overrides.json", "ovr"));
    let (id, name, ws) = ws_up(&h, L1, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    assert_eq!(in_member(&h, &k, &["results", "off"]), "results off for this session\n");
    assert_eq!(member(&h, &id, &k).overrides, Some(json_file(&sdir(&h, &k).join("overrides.json"))));
    assert_eq!(slash(&h, &k, "/zygo:guards repeat enforce")["reason"], "zygo: guards repeat enforce for this session");
    let saved = member(&h, &id, &k).overrides;
    assert_eq!(saved, Some(json_file(&sdir(&h, &k).join("overrides.json"))));
    let saved = saved.unwrap();
    assert_eq!((&saved["guards"]["repeat"], &saved["results"]), (&json!("enforce"), &json!(false)), "{saved}");
    h.server.as_ref().unwrap().kill_server();
    settle(&h, &id, &k, MemberState::Resumable);
    assert!(!member(&h, &id, &k).clean_stop);
    up_ok(&h, &name);
    assert!(wait_starts(&h, &k, 2));
    let s2 = stub_env(&h, &k)["ZYGO_SESSION"].as_str().unwrap().to_string();
    assert_eq!(json_file(&h.root().join("ovr").join(format!("{s2}.json"))), member(&h, &id, &k).overrides.unwrap());
    end(&h, &k, &name);
}

// ---- W§18 #68 (W§8.2 step 4) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_68_a_saved_redaction_setting_reaches_the_resumed_proxy_session() {
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "");
    let (id, name, ws) = ws_up(&h, L1, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    assert_eq!(slash(&h, &k, "/zygo:redaction content off")["reason"], "zygo: redaction content off for this session");
    assert_eq!(member(&h, &id, &k).overrides.unwrap()["redaction_content"], false);
    down(&h, &name);
    up_ok(&h, &name);
    assert!(wait_starts(&h, &k, 2));
    assert!(!out(&h, &k).join("requests.jsonl").exists(), "a request was made");
    let got = SessionClient::from_session_dir(&sdir(&h, &k)).unwrap().set_settings(&SessionSettings { redaction_content: None }).unwrap();
    assert_eq!(got.redaction_content, Some(false));
    end(&h, &k, &name);
}

// ---- W§18 #69 (AU32) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_69_the_compaction_override_follows_each_value() {
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "");
    let l = "[loadout.l]\n[[loadout.l.window]]\n[[loadout.l.window.pane]]\nkind = \"claude-code\"\ncompact = \"300k\"\n\
             [[loadout.l.window.pane]]\nkind = \"claude-code\"\ncompact = \"300k\"\n";
    let (id, name, ws) = ws_up(&h, l, &[]);
    let (a, b) = (key_at(&ws, 0, 0), key_at(&ws, 0, 1));
    let st = |k: &str| SessionClient::from_session_dir(&sdir(&h, k)).unwrap().status().unwrap().compact;
    let round = |n: usize| {
        down(&h, &name);
        up_ok(&h, &name);
        for k in [&a, &b] {
            assert!(wait_starts(&h, k, n), "round {n}: {k}");
        }
    };
    for k in [&a, &b] {
        assert!(wait_starts(&h, k, 1));
    }
    assert_eq!(st(&a), Some(WindowArg::Tokens(300_000)));
    assert_eq!(in_member(&h, &a, &["compact", "600k"]), "compact 600k for this session\n");
    assert_eq!(member(&h, &id, &a).compact, Some(CompactOverride::Set("600000".into())));
    let r = mcp_call(&h, &sdir(&h, &a), "compact", json!({}));
    assert_eq!(r["result"]["content"][0]["text"], "compaction requested", "{r}");
    assert_eq!(member(&h, &id, &a).compact, Some(CompactOverride::Set("600000".into())));
    round(2);
    assert_eq!((st(&a), st(&b)), (Some(WindowArg::Tokens(600_000)), Some(WindowArg::Tokens(300_000))));
    in_member(&h, &a, &["compact", "off"]);
    assert_eq!(member(&h, &id, &a).compact, Some(CompactOverride::Set("off".into())));
    round(3);
    assert_eq!(st(&a), Some(WindowArg::Off));
    in_member(&h, &a, &["compact", "on"]);
    assert_eq!(member(&h, &id, &a).compact, Some(CompactOverride::Cleared));
    round(4);
    assert_eq!((st(&a), st(&b)), (None, Some(WindowArg::Tokens(300_000))));
    stub_control(&ctl(&h, &b), "exit 0").unwrap();
    end(&h, &a, &name);
}

// ---- W§18 #71 (AU30, AU59, AU102) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_71_a_loadout_settings_argument_keeps_the_pane_a_member() {
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "");
    let user = h.root().join("user-settings.json");
    std::fs::write(&user, r#"{"env":{"E2_USER":"1"}}"#).unwrap();
    let l = format!(
        "[loadout.l]\n[[loadout.l.window]]\n[[loadout.l.window.pane]]\nkind = \"claude-code\"\nargs = ['--settings', '{}']\n\
         [[loadout.l.window.pane]]\nkind = \"shell\"\n",
        user.display()
    );
    let (id, name, ws) = ws_up(&h, &l, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    let m = member(&h, &id, &k);
    assert_eq!(m.state, MemberState::Running);
    assert_eq!(m.zygo_session.as_deref(), stub_env(&h, &k)["ZYGO_SESSION"].as_str());
    assert_eq!(m.launch.args, ["--settings".to_string(), user.display().to_string()]);
    let a = argv(&h, &k);
    let at: Vec<usize> = a.iter().enumerate().filter(|(_, t)| *t == "--settings").map(|(i, _)| i).collect();
    assert_eq!(at.len(), 1, "{a:?}");
    assert_eq!(a[at[0] + 1], sdir(&h, &k).join("settings.json").display().to_string(), "{a:?}");
    let s = json_file(&sdir(&h, &k).join("settings.json"));
    assert!(s["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().is_some_and(|c| c.ends_with("hook workspace --event stop")), "{s}");
    assert_eq!(s["env"]["E2_USER"], "1", "{s}");
    down(&h, &name);
    up_ok(&h, &name);
    assert!(wait_starts(&h, &k, 2));
    assert_eq!(session_arg(&h, &k).0, "--resume");
    end(&h, &k, &name);
}

// ---- W§18 #74 (AU104) ----

/// node is a prerequisite (no skip): OpenCode runs plugins in Bun; node runs the generated plugin file as a proxy for
/// the plugin logic; the live W§17 PW4 check covers the real runtime.
#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_74_an_opencode_member_commits_at_load_and_gets_the_note() {
    let Some(h) = Harness::new() else { return };
    let _node = node_exe();
    per_pane_opencode(&h, "");
    let l = "[loadout.l]\n[[loadout.l.window]]\n[[loadout.l.window.pane]]\nkind = \"opencode\"\n\
             [[loadout.l.window.pane]]\nkind = \"shell\"\n";
    let (id, name, ws) = ws_up(&h, l, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    let m = member(&h, &id, &k);
    assert!(m.pending.is_some() && m.agent_session.is_none(), "{m:?}");
    node_plugin(&h, &k, "[]", false);
    assert!(until(BOUND, || member(&h, &id, &k).pending.is_none()), "{:?}", member(&h, &id, &k));
    let m = member(&h, &id, &k);
    assert_eq!(m.agent_session, None);
    assert_eq!(m.zygo_session.as_deref(), stub_env(&h, &k)["ZYGO_SESSION"].as_str());
    node_plugin(
        &h,
        &k,
        r#"[["session.created",{"info":{"id":"ses_e2a"}}],["session.status",{"sessionID":"ses_e2a","status":{"type":"busy"}}],["session.idle",{"sessionID":"ses_e2a"}]]"#,
        false,
    );
    assert!(until(BOUND, || member(&h, &id, &k).agent_session.as_deref() == Some("ses_e2a")), "{:?}", member(&h, &id, &k));
    h.server.as_ref().unwrap().kill_server();
    settle(&h, &id, &k, MemberState::Resumable);
    up_ok(&h, &name);
    assert!(wait_starts(&h, &k, 2));
    assert_eq!(session_arg(&h, &k), ("--session".to_string(), "ses_e2a".to_string()));
    let r = node_plugin(&h, &k, "[]", true);
    let system: Vec<&str> = r["system"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(system.join("\n").contains(CRASH), "{r}");
    end(&h, &k, &name);
}

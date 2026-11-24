//! E2: down and drain end to end (W§18 #40-#43, #45, #48-#51, #82). Every case runs a workspace verb, so the file is
//! cfg(unix) (OP5).
#![cfg(unix)]
mod common;
#[cfg(unix)]
#[path = "common/e2e.rs"]
mod e2e;

use std::ffi::OsStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use common::stub_control;
use common::workspace::Harness;
use e2e::*;
use serde_json::json;
use zygo_workspace::state::{MemberRecord, MemberState, Phase, Workspace};

/// The tests with a tight time bound (#40, #48, #50) take this lock first, so they never run at the same time (E-13).
static TIGHT: Mutex<()> = Mutex::new(());

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

/// The record of the member `k` of the state `id`.
fn member(h: &Harness, id: &str, k: &str) -> MemberRecord {
    state(h, id).members[k].clone()
}

/// `busy` of the activity file of the pane `k`; `false` when the file is missing.
fn busy(h: &Harness, k: &str) -> bool {
    let p = sdir(h, k).join("activity.json");
    p.exists() && activity(&sdir(h, k))["busy"] == true
}

/// Waits ([`BOUND`]) until the state `id` has the phase `p`.
fn wait_phase(h: &Harness, id: &str, p: Phase) -> bool {
    until(BOUND, || phase(h, id) == p)
}

/// Waits ([`BOUND`]) until no launcher of the state `id` is live and the member `k` is `want`.
fn settle(h: &Harness, id: &str, k: &str, want: MemberState) {
    assert!(
        until(BOUND, || settled(h, id) && member(h, id, k).state == want),
        "not settled to {want:?}: {:?}",
        state(h, id).members
    );
}

/// The occurrences of `needle` in `s`.
fn count(s: &str, needle: &str) -> usize {
    s.matches(needle).count()
}

/// Sleeps until `t`; no sleep when `t` has passed.
fn sleep_until(t: Instant) {
    if let Some(d) = t.checked_duration_since(Instant::now()) {
        std::thread::sleep(d);
    }
}

/// Sends `sig` to the pid of a child that this test spawned and has not reaped.
fn signal(pid: u32, sig: libc::c_int) {
    // SAFETY: `kill` takes plain integers; `pid` is a `Guarded` child of this test that is not reaped yet.
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

/// The ending of a stub whose test is over: `exit 0` when it still runs, then `down --now --wait`.
fn end(h: &Harness, k: &str, name: &str) {
    stub_control(&ctl(h, k), "exit 0").unwrap();
    down(h, name);
}

/// The stdout of a finished `down` of `name`.
fn down_line(name: &str) -> String {
    format!("zygo: {name} is down (1 agents saved)\n")
}

// ---- W§18 #40 ----

#[test]
#[ignore = "timing: asserts down ends within 5 s"]
fn w18_40_an_idle_member_stops_within_5_s() {
    let _g = TIGHT.lock().unwrap_or_else(|e| e.into_inner());
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "");
    let (id, name, ws) = ws_up(&h, L1, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    let t = Instant::now();
    let o = spawn_down(&h, &["down", &name, "--wait"], &[]).output("down");
    let took = t.elapsed();
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    assert!(took < Duration::from_secs(5), "down took {took:?}: {err}");
    assert_eq!(text(&o.stdout), down_line(&name));
    assert_eq!(count(&err, &format!("zygo: {name} pane {k} is idle")), 1, "{err}");
    assert_eq!(phase(&h, &id), Phase::Down);
    assert!(!session_up(&h, &ws.tmux_session));
    settle(&h, &id, &k, MemberState::Resumable);
}

// ---- W§18 #41 (AU26, AU57) ----

#[test]
#[ignore = "timing: sleeps against the 10 s drain rule; asserts elapsed time"]
fn w18_41_a_turn_waits_for_stop_and_10_s_after_the_last_request() {
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "");
    let (port, _held) = held_upstream();
    let key = [("ZYGO_TEST_KEY", OsStr::new("k"))];
    let (id, name, ws) = ws_up(&h, &format!("{}{L1}", provider_config(port)), &key);
    let k = agent_key(&ws, |_| true);
    for (r, end_event) in [(1, "Stop"), (2, "StopFailure")] {
        if r == 2 {
            let o = up(&h, &name, &key);
            assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
        }
        assert!(wait_starts(&h, &k, r));
        step(&h, &k, "hook UserPromptSubmit");
        assert!(busy(&h, &k), "round {r}: not busy after the prompt");
        let g = spawn_down(&h, &["down", &name, "--wait"], &[]);
        assert!(wait_phase(&h, &id, Phase::Draining), "round {r}: {:?}", phase(&h, &id));
        std::thread::sleep(Duration::from_secs(2));
        assert_eq!(phase(&h, &id), Phase::Draining, "round {r}");
        step(&h, &k, "request 0");
        let t_req = Instant::now();
        step(&h, &k, &format!("hook {end_event}"));
        assert!(!busy(&h, &k), "round {r}: busy after {end_event}");
        sleep_until(t_req + Duration::from_secs(7));
        assert_eq!(phase(&h, &id), Phase::Draining, "round {r}: 7 s after the request");
        let o = g.output("down");
        let t_end = Instant::now();
        assert_eq!(o.status.code(), Some(0), "round {r}: {}", text(&o.stderr));
        assert!(t_end >= t_req + Duration::from_secs(9), "round {r}: down ended {:?} after the request", t_end - t_req);
        assert_eq!(phase(&h, &id), Phase::Down);
        assert_eq!(text(&o.stdout), down_line(&name));
    }
}

// ---- W§18 #42 (AU26, AU91) ----

#[test]
#[ignore = "timing: sleeps against the 10 s drain rule; asserts elapsed time"]
fn w18_42_a_held_request_keeps_the_member_busy_and_an_older_status_skips_rule_3() {
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "");
    let (port, held) = held_upstream();
    let key = [("ZYGO_TEST_KEY", OsStr::new("k"))];
    let (id, name, ws) = ws_up(&h, &format!("{}{L1}", provider_config(port)), &key);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    // Part 1: a request held 20 s by the upstream keeps the member busy, then 10 s more (rule 3).
    stub_control(&ctl(&h, &k), "request 20000").unwrap();
    assert!(until(BOUND, || held.lock().unwrap().len() == 1), "no request reached the upstream");
    let t0 = held.lock().unwrap()[0].recv;
    assert!(!busy(&h, &k), "busy with no prompt");
    let g = spawn_down(&h, &["down", &name, "--wait"], &[]);
    sleep_until(t0 + Duration::from_secs(25));
    assert_eq!(phase(&h, &id), Phase::Draining);
    let end = held.lock().unwrap()[0].end.expect("the held request has not ended 25 s after it came");
    let o = g.output("down");
    let t_end = Instant::now();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(t_end >= end + Duration::from_secs(9), "down ended {:?} after the reply", t_end - end);
    assert_eq!(phase(&h, &id), Phase::Down);
    // Part 2: a status from an older server (no `requests_in_flight`) skips rule 3 with one line.
    let o = up(&h, &name, &key);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(wait_starts(&h, &k, 2));
    step(&h, &k, "hook UserPromptSubmit");
    let sid2 = stub_env(&h, &k)["ZYGO_SESSION"].as_str().unwrap().to_string();
    point_url(&sdir(&h, &k), old_status_server(&sid2));
    let g = spawn_down(&h, &["down", &name, "--wait"], &[]);
    assert!(wait_phase(&h, &id, Phase::Draining));
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(phase(&h, &id), Phase::Draining);
    stub_control(&ctl(&h, &k), "hook Stop").unwrap();
    let o = g.output("down");
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    assert_eq!(count(&err, &format!("zygo: {name} pane {k}: proxy request state not usable; idle rule 3 skipped")), 1, "{err}");
    assert_eq!(phase(&h, &id), Phase::Down);
}

// ---- W§18 #43 (AU85, AU43) ----

#[test]
fn w18_43_a_subagent_without_its_stop_keeps_the_drain_waiting() {
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "");
    let (id, name, ws) = ws_up(&h, L1, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    step(&h, &k, "hook UserPromptSubmit");
    assert!(busy(&h, &k));
    step(&h, &k, r#"hook Stop {"agent_id":"bg0","agent_type":"general-purpose"}"#);
    assert!(busy(&h, &k), "a Stop with an agent_id changed busy");
    step(&h, &k, r#"hook SubagentStart {"agent_id":"bg1","agent_type":"general-purpose"}"#);
    step(&h, &k, "hook Stop");
    let a = activity(&sdir(&h, &k));
    assert_eq!(a["busy"], false, "{a}");
    assert_eq!(a["running"], json!({"bg1": "general-purpose"}), "{a}");
    step(&h, &k, r#"hook UserPromptSubmit {"agent_id":"bg1"}"#);
    assert!(!busy(&h, &k), "a prompt with an agent_id set busy");
    let g = spawn_down(&h, &["down", &name, "--wait"], &[]);
    assert!(wait_phase(&h, &id, Phase::Draining));
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(phase(&h, &id), Phase::Draining);
    stub_control(&ctl(&h, &k), r#"hook SubagentStop {"agent_id":"bg1","agent_type":"general-purpose"}"#).unwrap();
    let o = g.output("down");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(phase(&h, &id), Phase::Down);
    assert_eq!(text(&o.stdout), down_line(&name));
}

// ---- W§18 #45 (AU86) ----

#[test]
fn w18_45_stopping_blocks_prompts_and_agents_and_ignores_signals() {
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "trap '' TERM");
    let (id, name, ws) = ws_up(&h, L1, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    let g = spawn_down(&h, &["down", &name, "--wait"], &[]);
    assert!(wait_phase(&h, &id, Phase::Stopping), "{:?}", phase(&h, &id));
    let n = hook_records(&h, &k, "UserPromptSubmit").len();
    step(&h, &k, "hook UserPromptSubmit");
    assert_eq!(
        hook_reply(&h, &k, "UserPromptSubmit", n, "workspace --event prompt"),
        Some(json!({"decision": "block", "reason": "zygo: workspace is stopping"}))
    );
    let n = hook_records(&h, &k, "PreToolUse").len();
    step(&h, &k, "hook PreToolUse");
    assert_eq!(
        hook_reply(&h, &k, "PreToolUse", n, "workspace --event pre-tool"),
        Some(json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": "zygo: workspace is stopping"
        }}))
    );
    for sig in [libc::SIGINT, libc::SIGHUP, libc::SIGTERM] {
        signal(g.id(), sig);
        std::thread::sleep(Duration::from_millis(200));
    }
    assert_eq!(phase(&h, &id), Phase::Stopping);
    stub_control(&ctl(&h, &k), "exit 0").unwrap();
    let o = g.output("down");
    assert_eq!(o.status.code(), Some(0), "{:?}: {}", o.status, text(&o.stderr));
    assert_eq!(text(&o.stdout), down_line(&name));
    assert_eq!(phase(&h, &id), Phase::Down);
}

// ---- W§18 #48 (AU42, W§9.4) ----

#[test]
#[ignore = "timing: asserts down --now ends within 5 s"]
fn w18_48_now_saves_running_subagents_for_one_note() {
    let _g = TIGHT.lock().unwrap_or_else(|e| e.into_inner());
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "");
    let (id, name, ws) = ws_up(&h, L1, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    step(&h, &k, r#"hook SubagentStart {"agent_id":"sa1","agent_type":"general-purpose"}"#);
    let t = Instant::now();
    let o = spawn_down(&h, &["down", &name, "--now", "--wait"], &[]).output("down");
    let took = t.elapsed();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(took < Duration::from_secs(5), "down --now took {took:?}");
    let i = member(&h, &id, &k).interrupted;
    assert_eq!(i.iter().map(|s| (s.agent_id.as_str(), s.agent_type.as_str())).collect::<Vec<_>>(), [("sa1", "general-purpose")]);
    let o = up(&h, &name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(wait_starts(&h, &k, 2));
    let note = "zygo stopped this session while subagents general-purpose sa1 were running; their results were lost; dispatch them again if still needed.";
    let outs = session_start_outs(&h, &k);
    assert!(outs[1].contains(note), "{outs:?}");
    down(&h, &name);
    let o = up(&h, &name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(wait_starts(&h, &k, 3));
    let outs = session_start_outs(&h, &k);
    assert!(!outs[2].contains("zygo stopped this session"), "{outs:?}");
    end(&h, &k, &name);
}

// ---- W§18 #49 (AU31, AU76) ----

#[test]
fn w18_49_only_the_open_unit_is_named_and_both_rows_end() {
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "");
    let (id, name, ws) = ws_up(&h, L1, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    let s1 = stub_env(&h, &k)["ZYGO_SESSION"].as_str().unwrap().to_string();
    step(&h, &k, r#"hook PreToolUse {"tool_name":"mcp__zygo__work_begin","tool_input":{"unit":"e2:open","round":1}}"#);
    step(
        &h,
        &k,
        r#"hook PreToolUse {"agent_id":"zs1","agent_type":"zygo-coder","tool_name":"mcp__zygo__work_begin","tool_input":{"unit":"e2:done","round":1}}"#,
    );
    step(&h, &k, r#"hook SubagentStop {"agent_id":"zs1","agent_type":"zygo-coder"}"#);
    let work = sdir(&h, &k).join("work");
    let main = json_file(&work.join("main.begin.json"));
    assert!(main["stopped_at"].is_null(), "{main}");
    let zs1 = json_file(&work.join("zs1.begin.json"));
    assert!(zs1["stopped_at"].is_number(), "{zs1}");
    // `--now`: the stub sends no `PostToolUse`, so the member stays busy (P9).
    down(&h, &name);
    assert_eq!(member(&h, &id, &k).closed_units, ["e2:open"]);
    let rows = zygo_usage::Store::open(&h.paths.usage_db)
        .expect("usage db")
        .work_units(&zygo_usage::Filter { session: Some(s1), since: None })
        .unwrap();
    let mut units: Vec<&str> = rows.iter().map(|r| r.unit.as_str()).collect();
    units.sort();
    assert_eq!(units, ["e2:done", "e2:open"], "{rows:?}");
    assert!(rows.iter().all(|r| r.ended_at.is_some()), "{rows:?}");
    let o = up(&h, &name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(wait_starts(&h, &k, 2));
    let outs = session_start_outs(&h, &k);
    assert!(
        outs[1].contains("Units e2:open were closed when zygo stopped this session; call work_begin to open them again if work continues."),
        "{outs:?}"
    );
    assert!(!outs[1].contains("e2:done"), "{outs:?}");
    end(&h, &k, &name);
}

// ---- W§18 #50 (AU18, AU97) ----

#[test]
#[ignore = "timing: asserts 1 s returns, a 2 s poll and a 3 s drain timeout"]
fn w18_50_a_down_from_an_agent_detaches_and_keeps_its_timeout() {
    let _g = TIGHT.lock().unwrap_or_else(|e| e.into_inner());
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "");
    let (id, name, ws) = ws_up(&h, L1, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    step(&h, &k, "hook UserPromptSubmit");
    let caller = [("ZYGO_SESSION", OsStr::new(SID))];
    // Part 1: the CLI returns at once; `ls` shows the drain; `--timeout` reaches the detached process.
    let t = Instant::now();
    let o = cli(&h, &["down", &name, "--timeout", "3s"], &caller);
    let took = t.elapsed();
    assert!(took < Duration::from_secs(1), "down took {took:?}");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(text(&o.stdout), "zygo: down started; end your turn\n");
    let row = format!("{name}  {}  draining  ", ws.main_root.display());
    let mut ls = String::new();
    assert!(
        until(Duration::from_secs(2), || {
            ls = text(&cli(&h, &["ls"], &[]).stdout);
            ls.lines().any(|l| l.starts_with(&row))
        }),
        "{ls}"
    );
    assert!(wait_phase(&h, &id, Phase::Up), "{:?}", phase(&h, &id));
    let log = h.store(&id).dir().join("down.log");
    let mut body = String::new();
    let lines = [
        format!("zygo: {name}: drain timed out after 3 s; phase is up again"),
        format!("zygo: {name} pane {k}: busy: no Stop since its last prompt"),
    ];
    assert!(
        until(BOUND, || {
            body = std::fs::read_to_string(&log).unwrap_or_default();
            lines.iter().all(|l| body.contains(l.as_str()))
        }),
        "{body}"
    );
    // Part 2: `--all --now` writes `down-all.log` and cuts the old part to 32 KiB (AU97).
    step(&h, &k, "hook Stop");
    let all_log = h.paths.state_dir().join("down-all.log");
    let mut seed = String::new();
    for i in 0..1100 {
        let mut l = format!("seed line {i:04} ");
        while l.len() < 63 {
            l.push('x');
        }
        seed.push_str(&l);
        seed.push('\n');
    }
    assert_eq!(seed.len(), 70_400);
    std::fs::write(&all_log, &seed).unwrap();
    let t = Instant::now();
    let o = cli(&h, &["down", "--all", "--now"], &caller);
    let took = t.elapsed();
    assert!(took < Duration::from_secs(1), "down --all took {took:?}");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(text(&o.stdout), "zygo: down started; end your turn\n");
    assert!(wait_phase(&h, &id, Phase::Down), "{:?}", phase(&h, &id));
    assert!(state(&h, &id).down_all);
    let done = format!("zygo: {name} is down (");
    let mut body = String::new();
    assert!(
        until(BOUND, || {
            body = std::fs::read_to_string(&all_log).unwrap_or_default();
            body.contains(&done)
        }),
        "{body}"
    );
    assert!(body.lines().next().is_some_and(|l| l.starts_with("seed line ")), "{body}");
    assert!(!body.contains("seed line 0000 "));
    assert!(body.contains("seed line 1099 "));
    let seed_end = body.rfind("seed line ").map(|i| i + body[i..].find('\n').unwrap() + 1).unwrap();
    assert!(seed_end <= 32_768, "the kept seed part is {seed_end} bytes");
    assert!(body.len() <= 65_536, "down-all.log is {} bytes", body.len());
}

// ---- W§18 #51 (AU75) ----

/// The common part of #51 (b) and (c): a stub that ignores SIGTERM, a `down` held in `stopping` after its step 8
/// write, the `down` killed. Returns the harness, id, name, key, session and the zygo session of the stub.
fn held_stopping(check: impl FnOnce(&Harness, &str, &str)) -> Option<(Harness, String, String, String, String)> {
    let h = Harness::new()?;
    per_pane_claude(&h, "trap '' TERM");
    let (id, name, ws) = ws_up(&h, L1, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    let mut g = spawn_down(&h, &["down", &name, "--wait"], &[]);
    assert!(
        until(BOUND, || {
            let w = state(&h, &id);
            w.phase == Phase::Stopping && w.members[&k].saved_for.is_some()
        }),
        "{:?}",
        state(&h, &id)
    );
    check(&h, &id, &k);
    // E2 D-1: step 10 (SIGTERM to the launcher, `down.rs:1264-1272`) follows the step 8 write after one `list-panes`
    // and one `display` call, and has no mark outside the launcher. The kill comes 1 s later, so the launcher has
    // received SIGTERM and its agent's exit keeps the member `resumable` (W§8.2 step 7); a kill before step 10 gives
    // `ended` for the agent's own exit 0.
    std::thread::sleep(Duration::from_secs(1));
    signal(g.id(), libc::SIGKILL);
    g.wait("down");
    Some((h, id, name, k, ws.tmux_session))
}

#[test]
#[ignore = "timing: a 1 s sleep orders the kill after the launcher's SIGTERM"]
fn w18_51_a_killed_down_is_repaired() {
    // (a) killed in draining.
    {
        let Some(h) = Harness::new() else { return };
        per_pane_claude(&h, "");
        let (id, name, ws) = ws_up(&h, L1, &[]);
        let k = agent_key(&ws, |_| true);
        assert!(wait_starts(&h, &k, 1));
        step(&h, &k, "hook UserPromptSubmit");
        let mut g = spawn_down(&h, &["down", &name, "--wait"], &[]);
        assert!(wait_phase(&h, &id, Phase::Draining));
        signal(g.id(), libc::SIGKILL);
        g.wait("down");
        let o = up(&h, &name, &[]);
        assert_eq!(o.status.code(), Some(0), "(a) {}", text(&o.stderr));
        assert_eq!(phase(&h, &id), Phase::Up);
        assert_eq!(starts(&h, &k), 1);
        assert!(h.store(&id).op_lock_free());
        end(&h, &k, &name);
    }
    // (b-down) killed in stopping, the launcher ended after it; then `down`.
    {
        let Some((h, id, name, k, s)) = held_stopping(|_, _, _| {}) else { return };
        stub_control(&ctl(&h, &k), "exit 0").unwrap();
        assert!(until(BOUND, || settled(&h, &id)), "{:?}", state(&h, &id));
        assert!(session_up(&h, &s));
        let o = cli(&h, &["down", &name, "--now", "--wait"], &[]);
        assert_eq!(o.status.code(), Some(0), "(b-down) {}", text(&o.stderr));
        assert_eq!(phase(&h, &id), Phase::Down);
        assert!(!session_up(&h, &s));
    }
    // (b-up) the same; then `up`: the repair kills the session and sets `phase = down`, then `up` restores.
    {
        let Some((h, id, name, k, s)) = held_stopping(|_, _, _| {}) else { return };
        stub_control(&ctl(&h, &k), "exit 0").unwrap();
        settle(&h, &id, &k, MemberState::Resumable);
        assert!(session_up(&h, &s));
        let o = up(&h, &name, &[]);
        assert_eq!(o.status.code(), Some(0), "(b-up) {}", text(&o.stderr));
        assert_eq!(phase(&h, &id), Phase::Up);
        assert!(session_up(&h, &s));
        assert!(wait_starts(&h, &k, 2), "(b-up) {}\nmember: {:?}\npanes: {:?}", text(&o.stderr), member(&h, &id, &k), rows(&h, &s));
        assert_eq!(session_arg(&h, &k).0, "--resume");
        end(&h, &k, &name);
    }
    // (c) killed in stopping, the launcher still live; then at once `up`.
    {
        let check = |h: &Harness, id: &str, k: &str| {
            let m = member(h, id, k);
            assert!(m.clean_stop, "{m:?}");
            assert_eq!(m.saved_for.as_deref(), stub_env(h, k)["ZYGO_SESSION"].as_str(), "{m:?}");
        };
        let Some((h, id, name, k, _s)) = held_stopping(check) else { return };
        let o = up(&h, &name, &[]);
        assert_eq!(o.status.code(), Some(0), "(c) {}", text(&o.stderr));
        assert_eq!(phase(&h, &id), Phase::Up);
        let m = member(&h, &id, &k);
        assert!(!m.clean_stop, "{m:?}");
        assert!(m.closed_units.is_empty() && m.interrupted.is_empty() && m.saved_for.is_none(), "{m:?}");
        end(&h, &k, &name);
    }
    // (d) killed after `kill-session` and before the `phase` write: that state, made by hand (E-5).
    {
        let Some(h) = Harness::new() else { return };
        per_pane_claude(&h, "");
        let (id, name, ws) = ws_up(&h, L1, &[]);
        let k = agent_key(&ws, |_| true);
        let s = ws.tmux_session.clone();
        assert!(wait_starts(&h, &k, 1));
        assert!(until(BOUND, || member(&h, &id, &k).agent_session.is_some()), "{:?}", member(&h, &id, &k));
        let s1 = stub_env(&h, &k)["ZYGO_SESSION"].as_str().unwrap().to_string();
        h.tmux(&["kill-session", "-t", &format!("={s}:")]);
        settle(&h, &id, &k, MemberState::Resumable);
        h.store(&id)
            .update(Duration::from_secs(5), &mut |w| {
                w.phase = Phase::Stopping;
                let m = w.members.get_mut(&k).unwrap();
                m.clean_stop = true;
                m.saved_for = Some(s1.clone());
                Ok(())
            })
            .unwrap();
        let o = up(&h, &name, &[]);
        assert_eq!(o.status.code(), Some(0), "(d) {}", text(&o.stderr));
        assert_eq!(phase(&h, &id), Phase::Up);
        assert!(session_up(&h, &s));
        assert!(wait_starts(&h, &k, 2));
        assert_eq!(session_arg(&h, &k).0, "--resume");
        let outs = session_start_outs(&h, &k);
        assert!(!outs[1].contains(CRASH), "{outs:?}");
        end(&h, &k, &name);
    }
}

// ---- W§18 #82 (AU26, PW7 7.2, 7.5) ----

#[test]
fn w18_82_an_open_turn_and_an_unpaired_subagent_stop_hold_down() {
    let Some(h) = Harness::new() else { return };
    per_pane_claude(&h, "");
    let (id, name, ws) = ws_up(&h, L1, &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1));
    step(&h, &k, "hook UserPromptSubmit");
    step(&h, &k, r#"hook SubagentStop {"agent_id":"int1","agent_type":""}"#);
    let a = activity(&sdir(&h, &k));
    assert_eq!(a["busy"], true, "{a}");
    assert!(a["running"].as_object().is_none_or(|m| m.is_empty()), "{a}");
    let o = spawn_down(&h, &["down", &name, "--timeout", "3s", "--wait"], &[]).output("down");
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(1), "{err}");
    for l in [
        format!("zygo: {name}: drain timed out after 3 s; phase is up again"),
        format!("zygo: {name} pane {k}: busy: no Stop since its last prompt"),
        "zygo: run zygo down --now to stop at once".to_string(),
    ] {
        assert!(err.contains(&l), "no `{l}` in {err}");
    }
    assert_eq!(phase(&h, &id), Phase::Up);
    // SAFETY: signal 0 only checks that the stub of this test's own tmux server exists.
    assert_eq!(unsafe { libc::kill(stub_pid(&h, &k), 0) }, 0, "the stub was stopped");
    assert_eq!(starts(&h, &k), 1);
    let o = spawn_down(&h, &["down", &name, "--now", "--wait"], &[]).output("down");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(phase(&h, &id), Phase::Down);
    assert!(!session_up(&h, &ws.tmux_session));
}

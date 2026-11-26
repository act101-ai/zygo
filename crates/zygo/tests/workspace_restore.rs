//! E1: end-to-end up and restore checks (W§18 #5, #16, #17, #18, #20-#26, #30, #31, #33, #34, #37, #77) and the PW11 re-check on the real `zygo ws run`. Every case runs a workspace verb, so the file is cfg(unix) (OP5).
#![cfg(unix)]
mod common;
#[cfg(unix)]
#[path = "common/e2e.rs"]
mod e2e;

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime};

use common::stub_control;
use common::workspace::{ready_exe, zygo_exe, Guarded, Harness};
use e2e::*;
use serde_json::Value;
use zygo_workspace::mux::Multiplexer;
use zygo_workspace::state::{MemberState, Phase, Workspace};

/// The eleven names of W§18 #17 that no pane may hold (AU64).
const ELEVEN: [&str; 11] = [
    "ZYGO_SESSION",
    "ZYGO_SESSION_DIR",
    "ZYGO_AGENT",
    "ZYGO_URL",
    "ZYGO_REPO",
    "ZYGO_WORK",
    "ZYGO_MAIN_ROLE",
    "CLAUDECODE",
    "CLAUDE_CODE_SESSION_ID",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_CUSTOM_HEADERS",
];

/// A Claude Code pane of a loadout.
const CC: &str = "kind = \"claude-code\"";
/// A shell pane of a loadout.
const SH: &str = "kind = \"shell\"";

/// The `[loadout.<name>]` tables: one `[[…window]]` per item of `windows`, one `[[…window.pane]]` per pane body.
fn loadout(name: &str, windows: &[&[&str]]) -> String {
    let mut s = format!("[loadout.{name}]\n");
    for w in windows {
        s += &format!("[[loadout.{name}.window]]\n");
        for p in *w {
            s += &format!("[[loadout.{name}.window.pane]]\n{p}\n");
        }
    }
    s
}

/// `up --loadout <name> --no-attach` with `extra`: exit 0; the one state.
fn up_loadout(h: &Harness, name: &str, extra: &[(&str, &OsStr)]) -> (String, Workspace) {
    let o = cli(h, &["up", "--loadout", name, "--no-attach"], extra);
    assert_eq!(o.status.code(), Some(0), "up --loadout {name}: {}{}", text(&o.stdout), text(&o.stderr));
    only_state(h)
}

/// The occurrences of `needle` in `s`.
fn count(s: &str, needle: &str) -> usize {
    s.matches(needle).count()
}

/// Waits until the member `key` has an agent session id; returns it.
fn agent_id(h: &Harness, id: &str, key: &str) -> String {
    let mut got = None;
    assert!(
        until(BOUND, || {
            got = state(h, id).members.get(key).and_then(|m| m.agent_session.clone());
            got.is_some()
        }),
        "no agent session id for {key}: {:?}",
        state(h, id).members.get(key)
    );
    got.unwrap()
}

/// Waits until no launcher is live and every member in `keys` has `want`.
fn settle_to(h: &Harness, id: &str, keys: &[&str], want: MemberState) {
    assert!(
        until(BOUND, || settled(h, id) && keys.iter().all(|k| state(h, id).members.get(*k).is_some_and(|m| m.state == want))),
        "not settled to {want:?}: {:?}",
        state(h, id).members
    );
}

/// Quiet state: polls `snapshot_seq` every 100 ms until it has not changed for 1 s; bounded at 10 s, and fails with
/// `snapshot_seq still changed after 10 s: <first> -> <last>`. A snapshot that a start trigger (the member wrapper, the
/// `SessionStart` hook, W§7.4) began can still write after the stub has started; the checks that need "no other
/// trigger" start only after it. Prints the first and the last value.
fn quiet(h: &Harness, id: &str) {
    let first = state(h, id).snapshot_seq;
    let (mut last, mut since, t) = (first, Instant::now(), Instant::now());
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let now = state(h, id).snapshot_seq;
        if now != last {
            last = now;
            since = Instant::now();
        } else if since.elapsed() >= Duration::from_secs(1) {
            break;
        }
        assert!(t.elapsed() < Duration::from_secs(10), "snapshot_seq still changed after 10 s: {first} -> {last}");
    }
    let _ = std::io::stderr().write_all(format!("quiet: snapshot_seq {first} -> {last}\n").as_bytes());
}

/// The lowest probed tmux version that runs the workspace's `window-unlinked` hook for a `kill-window` while another
/// session is current (W§7.4). `fixtures/tmux/e1-hooks-3.8.txt` case D logs `window-unlinked other w0`; the same case
/// logs no line in `e1-hooks-3.2a.txt`, `e1-hooks-3.4.txt` and `e1-hooks-3.7c.txt`. 3.8 is the release after 3.7c.
const WINDOW_KILL_HOOK: (u32, u32) = (3, 8);

/// The lowest probed tmux version that runs the workspace's `window-layout-changed` hook for a `kill-pane` on the
/// active pane of a window that keeps other panes, while another session is current (W§7.4).
/// `fixtures/tmux/e1-active-pane-3.8.txt` (`kill-pane on pane 0`, modes `same` and `diff`) and `e1-hooks-3.8.txt`
/// case B log `window-layout-changed other w0`; the same cases log no `window-layout-changed` in the 3.2a, 3.4 and 3.7c
/// files. 3.8 is the release after 3.7c.
const ACTIVE_PANE_HOOK: (u32, u32) = (3, 8);

/// The tmux version of the harness server (`tmux -V`).
fn tmux_version(h: &Harness) -> (u32, u32) { h.server.as_ref().expect("a tmux server").tmux().version().unwrap() }

/// The message of a stub that did not start: the pane text of `key` when the pane exists.
fn no_start(h: &Harness, ws: &Workspace, key: &str) -> String {
    let p = rows(h, &ws.tmux_session).into_iter().find(|r| r.key == key);
    match p {
        Some(r) => format!("pane {key} ({}): {}", r.cmd, capture(h, &r.id)),
        None => format!("no pane {key}"),
    }
}

// ---- W§18 #5 (AU30) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_5_inline_content_flags_are_dropped_and_warned() {
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    let mcp = h.root().join("mcp.json");
    std::fs::write(&mcp, r#"{"mcpServers":{}}"#).unwrap();
    let a = format!(
        "{CC}\nargs = ['--agents', '{{\"r\":{{\"description\":\"d\",\"prompt\":\"p\"}}}}', '--mcp-config', '{}', '--plugin-url', 'https://h/p.zip']",
        mcp.display()
    );
    config(&h, &loadout("l5", &[&[&a, SH]]));
    let (id, ws) = up_loadout(&h, "l5", &[]);
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1), "{}", no_start(&h, &ws, &k));
    let body = std::fs::read_to_string(h.store(&id).dir().join("workspace.json")).unwrap();
    assert!(!body.contains("description") && !body.contains("p.zip") && !body.contains("https://h"), "{body}");
    let m = state(&h, &id).members[&k].clone();
    assert_eq!(m.launch.args, ["--mcp-config".to_string(), mcp.display().to_string()]);
    assert_eq!(m.dropped_flags, ["--agents", "--plugin-url"]);
    down(&h, &ws.name);
    settle_to(&h, &id, &[&k], MemberState::Resumable);
    let o = up(&h, &ws.name, &[]);
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    assert_eq!(count(&err, &format!("zygo: pane {k}: --agents had an inline value; not restored\n")), 1, "{err}");
    assert_eq!(count(&err, &format!("zygo: pane {k}: --plugin-url had an inline value; not restored\n")), 1, "{err}");
    assert!(wait_starts(&h, &k, 2), "{}", no_start(&h, &ws, &k));
    assert_eq!(session_arg(&h, &k).0, "--resume");
    down(&h, &ws.name);
}

// ---- W§18 #16 (AU55) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_16_format_text_in_paths_and_names_stays_literal() {
    let Some(h) = Harness::new() else { return };
    let main = h.root().join("a#b");
    std::fs::create_dir(&main).unwrap();
    git_repo_at(&h, &main);
    let wt = h.root().join("wt").join("#(touch pwned)");
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(&main)
        .args(["-c", "user.name=zygo-test", "-c", "user.email=zygo-test@example.invalid", "worktree", "add", "-q", "--detach"])
        .arg(&wt)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(o.status.success(), "git worktree add: {}", text(&o.stderr));
    let w0a = format!("{SH}\ncwd = '{}'", wt.display());
    config(&h, &loadout("l16", &[&[&w0a, SH], &[SH]]));
    let o = run_in(&h, &main, &["up", "--loadout", "l16", "--no-attach"], &[], true);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let (id, ws) = only_state(&h);
    let s = ws.tmux_session.clone();
    let kw = key_at(&ws, 0, 0);
    let t0 = format!("={s}:{}", win(&ws, 0));
    h.tmux(&["rename-window", "-t", &t0, "--", "##(touch pwned)"]);
    assert_eq!(h.tmux(&["display-message", "-p", "-t", &t0, "#{window_name}"]), "#(touch pwned)");
    assert!(pwned_under(h.root()).is_empty(), "{:?}", pwned_under(h.root()));
    down(&h, &ws.name);
    let saved = state(&h, &id);
    assert_eq!(saved.windows[0].name.as_deref(), Some("#(touch pwned)"), "{:?}", saved.windows);
    assert_eq!(saved.windows[1].name, None, "{:?}", saved.windows);
    assert_eq!(saved.panes.iter().find(|p| p.key == kw).unwrap().cwd, wt);
    let o = run_in(&h, &main, &["up", &ws.name, "--no-attach"], &[], true);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let ws2 = state(&h, &id);
    assert_eq!(h.tmux(&["display-message", "-p", "-t", &format!("={s}:{}", win(&ws2, 0)), "#{window_name}"]), "#(touch pwned)");
    // `-A`: the window inherits `automatic-rename` from the global options; without `-A` tmux shows only a window value.
    assert_eq!(h.tmux(&["show-options", "-w", "-v", "-A", "-t", &format!("={s}:{}", win(&ws2, 1)), "automatic-rename"]), "on");
    assert_eq!(ws2.windows[1].name, None);
    let pw = row(&h, &s, &kw).id;
    assert!(wait_cmd(&h, &pw, "sh"), "{:?}", row(&h, &s, &kw));
    let r = row(&h, &s, &kw);
    assert_eq!((r.path.as_str(), r.cmd.as_str()), (wt.to_str().unwrap(), "sh"));
    assert_eq!(h.tmux(&["display-message", "-p", "-t", &format!("={s}:"), "#{session_path}"]), main.to_str().unwrap());
    assert!(pwned_under(h.root()).is_empty(), "{:?}", pwned_under(h.root()));
    down(&h, &ws.name);
}

// ---- W§18 #17 (AU64, AU95) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_17_up_from_an_agent_tool_cleans_the_pane_environment() {
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    let (port, head) = mock_upstream();
    config(
        &h,
        &format!(
            "[[model]]\nid = \"test-env\"\n[[model.provider]]\nname = \"p-env\"\nbase_url = \"http://127.0.0.1:{port}\"\n\
             dialects = [\"anthropic\"]\nupstream_model = \"up-env\"\nkey = \"env:ZYGO_TEST_KEY\"\n\
             [role.lead]\nmodel = \"test-env\"\ndescription = \"d\"\nprompt = \"p\"\n{}",
            loadout("l17", &[&[CC, SH]])
        ),
    );
    let nosuch = h.root().join("nosuch");
    let vals: Vec<(&str, OsString)> = vec![
        ("ZYGO_SESSION", "01JAAAAAAAAAAAAAAAAAAAAAAA".into()),
        ("ZYGO_SESSION_DIR", nosuch.into_os_string()),
        ("ZYGO_AGENT", "claude-code".into()),
        ("ZYGO_URL", "http://127.0.0.1:1/".into()),
        ("ZYGO_REPO", "r".into()),
        ("ZYGO_WORK", "w".into()),
        ("ZYGO_MAIN_ROLE", "lead".into()),
        ("CLAUDECODE", "1".into()),
        ("CLAUDE_CODE_SESSION_ID", "x".into()),
        ("ANTHROPIC_BASE_URL", "http://127.0.0.1:1/anthropic".into()),
        ("ANTHROPIC_CUSTOM_HEADERS", "X-Zygo-Token: zyg_t_x".into()),
        ("ZYGO_TEST_KEY", "k".into()),
    ];
    let extra: Vec<(&str, &OsStr)> = vals.iter().map(|(k, v)| (*k, v.as_os_str())).collect();
    let o = cli(&h, &["up", "--loadout", "l17"], &extra);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let (id, ws) = only_state(&h);
    assert_eq!(text(&o.stdout), format!("zygo: {} is up\n", ws.name));
    // No `attach-session` and no `switch-client` (AU95): the harness has no tmux call log, so the private server must
    // have no client.
    let clients = h.tmux(&["list-clients"]);
    assert!(clients.trim().is_empty(), "{clients}");
    let s = ws.tmux_session.clone();
    let g = h.tmux(&["show-environment", "-g"]);
    let e = h.tmux(&["show-environment", "-t", &format!("={s}:")]);
    for name in ELEVEN {
        let p = format!("{name}=");
        assert!(!g.lines().any(|l| l.starts_with(&p)), "{name} in the global environment:\n{g}");
        assert!(!e.lines().any(|l| l.starts_with(&p)), "{name} in the session environment:\n{e}");
    }
    assert!(g.lines().any(|l| l == "ZYGO_TEST_KEY=k"), "{g}");
    assert!(e.lines().any(|l| l == format!("ZYGO_WORKSPACE={id}")), "{e}");
    assert!(!g.lines().any(|l| l.starts_with("ZYGO_WORKSPACE=")), "{g}");
    #[cfg(target_os = "linux")]
    for r in rows(&h, &s) {
        let env = std::fs::read(format!("/proc/{}/environ", r.pid)).unwrap();
        let items: Vec<String> = env.split(|b| *b == 0).map(text).collect();
        for name in ELEVEN {
            assert!(!items.iter().any(|i| i.starts_with(&format!("{name}="))), "{name} in pane {} ({})", r.id, r.cmd);
        }
        assert!(items.iter().any(|i| i == "ZYGO_TEST_KEY=k"), "pane {} ({}): {items:?}", r.id, r.cmd);
    }
    let k1 = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k1, 1), "{}", no_start(&h, &ws, &k1));
    assert!(stub_env(&h, &k1).get("ZYGO_TEST_KEY").is_none(), "{}", stub_env(&h, &k1));
    stub_control(&ctl(&h, &k1), "request 0").unwrap();
    let done = ctl(&h, &k1).join("done");
    assert!(
        until(BOUND, || std::fs::read_to_string(&done).unwrap_or_default().lines().any(|l| l == "request 0")),
        "{}",
        std::fs::read_to_string(out(&h, &k1).join("requests.jsonl")).unwrap_or_default()
    );
    let lines = head.lock().unwrap().clone();
    assert!(lines.iter().any(|l| l == "x-api-key: k"), "{lines:?}");
    stub_control(&ctl(&h, &k1), "exit 0").unwrap();
    down(&h, &ws.name);
}

// ---- W§18 #18 (AU93) ----

#[test]
#[ignore = "timing: sleeps let the shell read its terminal before typed input"]
fn w18_18_the_shell_after_the_agent_has_the_workspace_and_a_hand_start_joins() {
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    config(&h, &loadout("l18", &[&[CC, SH]]));
    let (id, ws) = up_loadout(&h, "l18", &[]);
    let s = ws.tmux_session.clone();
    let k = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &k, 1), "{}", no_start(&h, &ws, &k));
    let p = row(&h, &s, &k).id;
    stub_control(&ctl(&h, &k), "exit 0").unwrap();
    assert!(wait_shell(&h, &p, "sh"), "{}", capture(&h, &p));
    let e18 = h.root().join("e18");
    send(&h, &p, &format!("echo \"$ZYGO_WORKSPACE|${{ZYGO_PANE-unset}}\" > '{}'", e18.display()));
    assert_eq!(wait_file(&e18, BOUND), Some(format!("{id}|unset\n")), "{}", capture(&h, &p));
    send(&h, &p, &format!("'{}' claude", zygo_exe()));
    assert!(wait_starts(&h, &k, 2), "{}", capture(&h, &p));
    let e = stub_env(&h, &k);
    assert_eq!((e["ZYGO_PANE"].as_str(), e["ZYGO_WORKSPACE"].as_str()), (Some(k.as_str()), Some(id.as_str())), "{e}");
    let zs = e["ZYGO_SESSION"].as_str().unwrap().to_string();
    assert!(
        until(BOUND, || state(&h, &id)
            .members
            .get(&k)
            .is_some_and(|m| m.state == MemberState::Running && m.zygo_session.as_deref() == Some(zs.as_str()))),
        "{:?}",
        state(&h, &id).members.get(&k)
    );
    stub_control(&ctl(&h, &k), "exit 0").unwrap();
    down(&h, &ws.name);
}

// ---- W§18 #20 (W§8.1 steps 7-12, AU3) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_20_down_then_up_resumes_and_restores_the_layout() {
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    std::fs::create_dir(h.repo().join("sub")).unwrap();
    let sub = format!("{SH}\ncwd = \"sub\"");
    config(&h, &loadout("l20", &[&[CC, SH], &[&sub, SH]]));
    let (id, ws) = up_loadout(&h, "l20", &[]);
    let s = ws.tmux_session.clone();
    let c = agent_key(&ws, |_| true);
    assert!(wait_starts(&h, &c, 1), "{}", no_start(&h, &ws, &c));
    let (flag, sid) = session_arg(&h, &c);
    assert_eq!(flag, "--session-id");
    assert!(argv(&h, &c).contains(&format!("--session-id={sid}")), "{:?}", argv(&h, &c));
    assert_eq!(agent_id(&h, &id, &c), sid);
    assert_held_released(&h, &s);
    h.tmux(&["select-window", "-t", &format!("={s}:{}", win(&ws, 1))]);
    h.tmux(&["select-pane", "-t", &row(&h, &s, &key_at(&ws, 1, 1)).id]);
    let before = geometry(&h, &s);
    down(&h, &ws.name);
    let saved = state(&h, &id);
    let ka = key_at(&saved, 1, 1);
    let w1: Vec<_> = saved.panes.iter().filter(|p| p.window == win(&saved, 1)).collect();
    assert_eq!(w1.iter().filter(|p| p.active).map(|p| p.key.clone()).collect::<Vec<_>>(), std::slice::from_ref(&ka), "{:?}", saved.panes);
    assert!(saved.windows[1].active, "{:?}", saved.windows);
    let o = up(&h, &ws.name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(wait_starts(&h, &c, 2), "{}", no_start(&h, &saved, &c));
    assert_eq!(session_arg(&h, &c), ("--resume".to_string(), sid.clone()));
    assert!(argv(&h, &c).contains(&format!("--resume={sid}")), "{:?}", argv(&h, &c));
    assert_eq!(session_args(&argv(&h, &c)), 1, "{:?}", argv(&h, &c));
    assert_held_released(&h, &s);
    for p in saved.panes.iter().filter(|p| p.key != c) {
        let r = row(&h, &s, &p.key);
        assert!(wait_cmd(&h, &r.id, "sh"), "{r:?}");
        assert_eq!(Path::new(&row(&h, &s, &p.key).path), p.cwd, "{}", p.key);
    }
    assert_eq!(layouts(&h, &s), saved_layouts(&saved));
    assert_eq!(geometry(&h, &s), before);
    assert!(row(&h, &s, &ka).active, "{:?}", rows(&h, &s));
    assert_eq!(h.tmux(&["display-message", "-p", "-t", &format!("={s}:{}", win(&saved, 1)), "#{window_active}"]), "1");
    down(&h, &ws.name);
}

// ---- W§18 #21 (AU24, OP8) ----

#[test]
#[ignore = "timing: sleeps let the shell read its terminal before typed input"]
fn w18_21_a_command_in_a_shell_pane_does_not_come_back() {
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    config(&h, &loadout("l21", &[&[SH, SH]]));
    let (_id, ws) = up_loadout(&h, "l21", &[]);
    let s = ws.tmux_session.clone();
    let k = key_at(&ws, 0, 0);
    let p = row(&h, &s, &k).id;
    assert!(wait_shell(&h, &p, "sh"), "{}", capture(&h, &p));
    send(&h, &p, "/bin/sleep 1000");
    assert!(wait_cmd(&h, &p, "sleep"), "{}", capture(&h, &p));
    #[cfg(target_os = "linux")]
    let sp = {
        let pid = row(&h, &s, &k).pid;
        let children = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")).unwrap();
        let sp: i32 = children.split_whitespace().next().unwrap().parse().unwrap();
        assert_eq!(std::fs::read_to_string(format!("/proc/{sp}/comm")).unwrap(), "sleep\n");
        sp
    };
    let o = cli(&h, &["down", &ws.name, "--now", "--wait"], &[]);
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    // E1 A-2: `down` names the pane by its key (`down.rs:1357-1358`).
    assert_eq!(count(&err, &format!("zygo: {} pane {k}: sleep is running; it stops with the session", ws.name)), 1, "{err}");
    let o = up(&h, &ws.name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let r = row(&h, &s, &k);
    assert!(wait_cmd(&h, &r.id, "sh"), "{r:?}");
    assert!(row(&h, &s, &k).start.ends_with("--shell"), "{:?}", row(&h, &s, &k));
    assert!(!rows(&h, &s).iter().any(|r| r.cmd == "sleep"), "{:?}", rows(&h, &s));
    #[cfg(target_os = "linux")]
    assert_ne!(std::fs::read_to_string(format!("/proc/{sp}/comm")).unwrap_or_default(), "sleep\n");
    down(&h, &ws.name);
}

// ---- W§18 #22 (AU17, AU42, AU52) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_22_kill_server_then_up_resumes_with_the_crash_note() {
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    config(&h, &loadout("l22", &[&[CC, SH], &[CC]]));
    let (id, ws) = up_loadout(&h, "l22", &[]);
    let s = ws.tmux_session.clone();
    let a = key_at(&ws, 0, 0);
    let b = key_at(&ws, 1, 0);
    for k in [&a, &b] {
        assert!(wait_starts(&h, k, 1), "{}", no_start(&h, &ws, k));
    }
    let ia = agent_id(&h, &id, &a);
    let ib = agent_id(&h, &id, &b);
    let server = h.server.as_ref().unwrap();
    server.kill_server();
    settle_to(&h, &id, &[&a, &b], MemberState::Resumable);
    let last = state(&h, &id);
    let o = up(&h, &ws.name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    for (k, i) in [(&a, &ia), (&b, &ib)] {
        assert!(wait_starts(&h, k, 2), "{}", no_start(&h, &last, k));
        assert_eq!(session_arg(&h, k), ("--resume".to_string(), i.clone()));
        let outs = session_start_outs(&h, k);
        assert!(outs.get(1).is_some_and(|o| o.contains(CRASH)), "{k}: {outs:?}");
    }
    assert_eq!(layouts(&h, &s), saved_layouts(&last));
    server.kill_server();
    assert!(until(BOUND, || settled(&h, &id)), "{:?}", state(&h, &id).members);
    assert_eq!(state(&h, &id).phase, Phase::Up);
    let o = cli(&h, &["up", "--all"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(text(&o.stdout), format!("zygo: {} is up\n", ws.name), "{}", text(&o.stderr));
    assert!(server.run(&["has-session", "-t", &format!("={s}:")]).is_ok());
    for (k, i) in [(&a, &ia), (&b, &ib)] {
        assert!(wait_starts(&h, k, 3), "{}", no_start(&h, &last, k));
        assert_eq!(session_arg(&h, k), ("--resume".to_string(), i.clone()));
    }
    down(&h, &ws.name);
}

// ---- W§18 #23 (AU89, W§7.5 rule 2; D-2) ----

#[test]
#[ignore = "timing: a 1 s quiet window and a 5 s wait decide that no snapshot comes"]
fn w18_23_kill_session_by_hand_writes_no_layout_and_up_restores() {
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    config(&h, &loadout("l23", &[&[CC, SH], &[SH]]));
    let (id, ws) = up_loadout(&h, "l23", &[]);
    let s = ws.tmux_session.clone();
    let a = key_at(&ws, 0, 0);
    assert!(wait_starts(&h, &a, 1), "{}", no_start(&h, &ws, &a));
    let ia = agent_id(&h, &id, &a);
    quiet(&h, &id);
    let before = raw(&h, &id);
    let t0 = Instant::now();
    h.tmux(&["kill-session", "-t", &format!("={s}:")]);
    assert!(until(BOUND, || settled(&h, &id)), "{:?}", state(&h, &id).members);
    if let Some(rest) = Duration::from_secs(5).checked_sub(t0.elapsed()) {
        std::thread::sleep(rest);
    }
    let after = raw(&h, &id);
    for f in ["windows", "panes", "snapshot_seq", "snapshot_at"] {
        assert_eq!(after[f], before[f], "{f}");
    }
    assert_eq!(state(&h, &id).members[&a].state, MemberState::Resumable);
    let o = up(&h, &ws.name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(wait_starts(&h, &a, 2), "{}", no_start(&h, &ws, &a));
    assert_eq!(session_arg(&h, &a), ("--resume".to_string(), ia));
    let mut want: Vec<(u64, String, u32, u32)> = before["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            (
                w["index"].as_u64().unwrap(),
                layout_shape(w["layout"].as_str().unwrap()),
                w["width"].as_u64().unwrap() as u32,
                w["height"].as_u64().unwrap() as u32,
            )
        })
        .collect();
    want.sort();
    let want: Vec<(String, u32, u32)> = want.into_iter().map(|(_, l, w, h)| (l, w, h)).collect();
    assert_eq!(layouts(&h, &s), want);
    down(&h, &ws.name);
}

// ---- W§18 #24 and #77 (W§7.4, W§7.5, PW6 option A) ----

/// The setup of #24 and #77: loadout `l24` (window 0 = Claude Code A + shell; window 1 = Claude Code B; window 2 = two
/// shells; window 3 = two shells) up, A and B started with their ids, then a second session `other` made after the
/// workspace session with no client. Returns the id, the name and the state.
fn l24_setup(h: &Harness) -> (String, String, Workspace) {
    git_repo_at(h, &h.repo());
    per_pane_claude(h, "");
    config(h, &loadout("l24", &[&[CC, SH], &[CC], &[SH, SH], &[SH, SH]]));
    let (id, ws) = up_loadout(h, "l24", &[]);
    for k in [key_at(&ws, 0, 0), key_at(&ws, 1, 0)] {
        assert!(wait_starts(h, &k, 1), "{}", no_start(h, &ws, &k));
        agent_id(h, &id, &k);
    }
    quiet(h, &id);
    h.tmux(&["new-session", "-d", "-s", "other", "-x", "80", "-y", "24", "/bin/sleep 600"]);
    let name = ws.name.clone();
    (id, name, ws)
}

#[test]
#[ignore = "timing: tmux hook effects must come within 5 s; a 1 s quiet window"]
fn w18_24_closed_agent_panes_are_detached_while_another_session_is_current() {
    let Some(h) = Harness::new() else { return };
    let (id, name, ws) = l24_setup(&h);
    let s = ws.tmux_session.clone();
    let a = key_at(&ws, 0, 0);
    let b = key_at(&ws, 1, 0);
    let ia = state(&h, &id).members[&a].agent_session.clone().unwrap();
    let ib = state(&h, &id).members[&b].agent_session.clone().unwrap();
    h.tmux(&["kill-pane", "-t", &row(&h, &s, &a).id]);
    assert!(until(FIVE, || !keys(&state(&h, &id)).contains(&a)), "{:?}", keys(&state(&h, &id)));
    assert!(state(&h, &id).members.contains_key(&a));
    h.tmux(&["kill-window", "-t", &format!("={s}:{}", win(&ws, 1))]);
    assert!(until(FIVE, || !keys(&state(&h, &id)).contains(&b)), "{:?}", keys(&state(&h, &id)));
    assert!(state(&h, &id).members.contains_key(&b));
    let s2 = rows(&h, &s).into_iter().find(|r| r.window == win(&ws, 2) && !r.active).expect("a pane of window 2 that is not active").key;
    h.tmux(&["kill-pane", "-t", &row(&h, &s, &s2).id]);
    assert!(until(FIVE, || !keys(&state(&h, &id)).contains(&s2)), "{:?}", keys(&state(&h, &id)));
    settle_to(&h, &id, &[&a, &b], MemberState::Resumable);
    let cache = h.root().join(".cache/zygo/workspaces").join(&id).join("results").join(&a);
    assert!(cache.exists(), "{}", cache.display());
    let cache_b = h.root().join(".cache/zygo/workspaces").join(&id).join("results").join(&b);
    assert!(cache_b.exists(), "{}", cache_b.display());
    down(&h, &name);
    let o = up(&h, &name, &[]);
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    let tk = tmux_keys(&h, &s);
    assert!(!tk.contains(&a) && !tk.contains(&b), "{tk:?}");
    for (k, i) in [(&a, &ia), (&b, &ib)] {
        let line = format!(
            "zygo: claude-code agent session {i} is detached (its pane was closed); resume it by hand or run zygo ws forget-pane {k}"
        );
        assert!(err.contains(&line), "{line}\n{err}");
    }
    let o = cli(&h, &["ws", "forget-pane", &a], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(!state(&h, &id).members.contains_key(&a));
    assert!(!cache.exists(), "{}", cache.display());
    let o = cli(&h, &["ws", "forget-pane", &b], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(!state(&h, &id).members.contains_key(&b));
    assert!(!cache_b.exists(), "{}", cache_b.display());
    down(&h, &name);
}

#[test]
#[ignore = "timing: tmux hook effects must come within 5 s; a 1 s quiet window"]
fn w18_77_closed_shell_panes_while_another_session_is_current() {
    let Some(h) = Harness::new() else { return };
    let (id, name, ws) = l24_setup(&h);
    let s = ws.tmux_session.clone();
    let s4 = key_at(&ws, 3, 0);
    let s5 = key_at(&ws, 3, 1);
    let p = rows(&h, &s).into_iter().find(|r| r.window == win(&ws, 2) && r.active).expect("an active pane in window 2").key;
    let gone = |ks: &[&String]| { let k = keys(&state(&h, &id)); ks.iter().all(|x| !k.contains(*x)) };
    let v = tmux_version(&h);
    h.tmux(&["kill-window", "-t", &format!("={s}:{}", win(&ws, 3))]);
    if v >= WINDOW_KILL_HOOK {
        assert!(until(FIVE, || gone(&[&s4, &s5])), "{:?}", keys(&state(&h, &id)));
    } else {
        // W§18 #77: the keys are still there 5 s later (the item's own time).
        std::thread::sleep(FIVE);
        let k = keys(&state(&h, &id));
        assert!(k.contains(&s4) && k.contains(&s5), "{k:?}");
    }
    h.tmux(&["kill-pane", "-t", &row(&h, &s, &p).id]);
    if v < WINDOW_KILL_HOOK || v < ACTIVE_PANE_HOOK {
        h.tmux(&["split-window", "-d", "-t", &format!("={s}:{}", win(&ws, 0))]);
    }
    assert!(until(FIVE, || gone(&[&s4, &s5, &p])), "{:?}", keys(&state(&h, &id)));
    down(&h, &name);
}

// ---- W§18 #25 (AU17, AU73) ----

#[test]
#[ignore = "timing: a 300 ms sleep orders SIGINT after the forwarded SIGTERM"]
fn w18_25_an_agent_that_exits_by_itself() {
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "trap '' TERM");
    config(&h, &loadout("l25", &[&[CC, CC], &[CC]]));
    let (id, ws) = up_loadout(&h, "l25", &[]);
    let s = ws.tmux_session.clone();
    let (a, b, c) = (key_at(&ws, 0, 0), key_at(&ws, 0, 1), key_at(&ws, 1, 0));
    for k in [&a, &b, &c] {
        assert!(wait_starts(&h, k, 1), "{}", no_start(&h, &ws, k));
    }
    let ib = agent_id(&h, &id, &b);
    let ic = agent_id(&h, &id, &c);
    agent_id(&h, &id, &a);
    // Exit 0.
    stub_control(&ctl(&h, &a), "exit 0").unwrap();
    let pa = row(&h, &s, &a).id;
    assert!(wait_cmd(&h, &pa, "sh"), "{}", capture(&h, &pa));
    member_is(&h, &id, &a, MemberState::Ended);
    // Exit 130.
    stub_control(&ctl(&h, &b), "exit 130").unwrap();
    let pb = row(&h, &s, &b).id;
    assert!(wait_cmd(&h, &pb, "sh"), "{}", capture(&h, &pb));
    member_is(&h, &id, &b, MemberState::Ended);
    let line = format!(
        "zygo: claude exited 130; agent session {ib} is kept by the agent; to resume it: env -u CLAUDE_CONFIG_DIR zygo claude -- --resume='{ib}'"
    );
    assert!(until(BOUND, || capture(&h, &pb).contains(&line)), "{line}\n{}", capture(&h, &pb));
    // SIGINT after SIGTERM.
    let rc = row(&h, &s, &c);
    let sp = stub_pid(&h, &c);
    // SAFETY: `kill` takes plain integers; `rc.pid` is the `ws run` of a pane of this test's server.
    unsafe {
        libc::kill(rc.pid, libc::SIGTERM);
    }
    std::thread::sleep(Duration::from_millis(300));
    // SAFETY: as above; `sp` is the stub that this pane runs.
    assert_eq!(unsafe { libc::kill(sp, 0) }, 0, "the stub ended on the forwarded SIGTERM");
    // SAFETY: as above.
    unsafe {
        libc::kill(sp, libc::SIGINT);
    }
    assert!(wait_cmd(&h, &rc.id, "sh"), "{}", capture(&h, &rc.id));
    member_is(&h, &id, &c, MemberState::Resumable);
    assert!(!capture(&h, &rc.id).contains("exited"), "{}", capture(&h, &rc.id));
    down(&h, &ws.name);
    let o = up(&h, &ws.name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    for k in [&a, &b] {
        let r = row(&h, &s, k);
        assert!(wait_cmd(&h, &r.id, "sh"), "{r:?}");
        assert!(row(&h, &s, k).start.ends_with("--shell"), "{:?}", row(&h, &s, k));
        assert_eq!(starts(&h, k), 1, "{k}");
    }
    assert!(wait_starts(&h, &c, 2), "{}", no_start(&h, &ws, &c));
    assert_eq!(session_arg(&h, &c), ("--resume".to_string(), ic));
    stub_control(&ctl(&h, &c), "exit 0").unwrap();
    let pc = row(&h, &s, &c).id;
    assert!(wait_cmd(&h, &pc, "sh"), "{}", capture(&h, &pc));
    down(&h, &ws.name);
}

/// Waits until no launcher is live and the member `key` has `want`.
fn settle_member(h: &Harness, id: &str, key: &str, want: MemberState) {
    settle_to(h, id, &[key], want);
}

/// Waits ([`BOUND`]) until the member `key` has `want`; other members may still run.
fn member_is(h: &Harness, id: &str, key: &str, want: MemberState) {
    assert!(
        until(BOUND, || state(h, id).members.get(key).is_some_and(|m| m.state == want)),
        "{key} is not {want:?}: {:?}",
        state(h, id).members.get(key)
    );
}

// ---- W§18 #26 (AU12, AU60, W§8.2 step 7) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_26_a_resume_that_fails_at_once_stays_resumable() {
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    config(&h, &loadout("l26", &[&[CC, SH]]));
    let (id, ws) = up_loadout(&h, "l26", &[]);
    let s = ws.tmux_session.clone();
    let a = key_at(&ws, 0, 0);
    assert!(wait_starts(&h, &a, 1), "{}", no_start(&h, &ws, &a));
    let ia = agent_id(&h, &id, &a);
    down(&h, &ws.name);
    settle_member(&h, &id, &a, MemberState::Resumable);
    stub_control(&ctl(&h, &a), "exit 3").unwrap();
    let o = up(&h, &ws.name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(wait_starts(&h, &a, 2), "{}", no_start(&h, &ws, &a));
    let pa = row(&h, &s, &a).id;
    assert!(wait_cmd(&h, &pa, "sh"), "{}", capture(&h, &pa));
    let line = format!("zygo: resume of claude session {ia} failed (exit 3); try: env -u CLAUDE_CONFIG_DIR zygo claude -- --resume='{ia}'");
    assert!(until(BOUND, || capture(&h, &pa).contains(&line)), "{line}\n{}", capture(&h, &pa));
    assert!(until(BOUND, || settled(&h, &id)), "{:?}", state(&h, &id).members);
    let m = state(&h, &id).members[&a].clone();
    assert_eq!((m.state, m.agent_session.as_deref()), (MemberState::Resumable, Some(ia.as_str())));
    down(&h, &ws.name);
    let o = up(&h, &ws.name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(wait_starts(&h, &a, 3), "{}", no_start(&h, &ws, &a));
    assert_eq!(session_arg(&h, &a), ("--resume".to_string(), ia));
    down(&h, &ws.name);
}

// ---- W§18 #30 (AU16, AU69) ----

/// The sorted entry names of `dir`.
fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_30_a_result_survives_down_up_and_the_sweep() {
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    config(&h, &loadout("l30", &[&[CC, SH]]));
    let (id, ws) = up_loadout(&h, "l30", &[]);
    let a = key_at(&ws, 0, 0);
    assert!(wait_starts(&h, &a, 1), "{}", no_start(&h, &ws, &a));
    let s1 = PathBuf::from(stub_env(&h, &a)["ZYGO_SESSION_DIR"].as_str().unwrap());
    // Store a result through the trim hook (the PostToolUse Bash input of `hook_trim.rs:23-27`).
    let big: String = (0..5000).map(|i| format!("line {i:06} xxxxxxxx\n")).collect();
    let input = serde_json::to_vec(&serde_json::json!({"hook_event_name": "PostToolUse", "tool_name": "Bash",
        "tool_input": {"command": "x"},
        "tool_response": {"stdout": big, "stderr": "", "interrupted": false, "isImage": false, "noOutputExpected": false}}))
    .unwrap();
    let mut c = h.zygo(&["hook", "trim"], &[("ZYGO_SESSION_DIR", s1.as_os_str())]);
    c.stdin(Stdio::piped());
    let mut g = Guarded::spawn(c);
    common::write_stdin(g.stdin().unwrap(), &input);
    let o = g.output("hook trim");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o.stdout)));
    let view = v["hookSpecificOutput"]["updatedToolOutput"]["stdout"].as_str().unwrap().to_string();
    let rid = view.split("is saved as result ").nth(1).unwrap_or_else(|| panic!("{view}")).split('.').next().unwrap().to_string();
    let target = h.root().join(".cache/zygo/workspaces").join(&id).join("results").join(&a);
    let names = names_in(&target);
    assert!(!names.is_empty(), "{}", target.display());
    down(&h, &ws.name);
    settle_member(&h, &id, &a, MemberState::Resumable);
    let o = up(&h, &ws.name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(wait_starts(&h, &a, 2), "{}", no_start(&h, &ws, &a));
    let s2 = PathBuf::from(stub_env(&h, &a)["ZYGO_SESSION_DIR"].as_str().unwrap());
    assert_ne!(s2, s1);
    let link = std::fs::read_link(s2.join("results")).unwrap();
    assert_eq!(std::fs::canonicalize(link).unwrap(), std::fs::canonicalize(&target).unwrap());
    let req = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":\"2025-06-18\"}}}}\n\
         {{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}}\n\
         {{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{{\"name\":\"result_view\",\"arguments\":{{\"id\":\"{rid}\",\"lines\":\"1-2\"}}}}}}\n"
    );
    let mut c = h.zygo(&["mcp", "--agent", "claude-code"], &[("ZYGO_SESSION_DIR", s2.as_os_str())]);
    c.stdin(Stdio::piped());
    let mut g = Guarded::spawn(c);
    common::write_stdin(g.stdin().unwrap(), req.as_bytes());
    let o = g.output("mcp");
    let replies: Vec<Value> = text(&o.stdout).lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let r2 = replies.iter().find(|r| r["id"] == 2).unwrap_or_else(|| panic!("{}{}", text(&o.stdout), text(&o.stderr)));
    assert!(r2["result"]["content"][0]["text"].as_str().is_some_and(|t| t.contains("line 000000")), "{r2}");
    down(&h, &ws.name);
    assert!(until(BOUND, || settled(&h, &id)), "{:?}", state(&h, &id).members);
    assert!(s1.exists(), "{}", s1.display());
    let old = SystemTime::now() - Duration::from_secs(25 * 3600);
    for e in std::fs::read_dir(&s1).unwrap() {
        let p = e.unwrap().path();
        if p.symlink_metadata().unwrap().file_type().is_symlink() {
            set_link_mtime(&p, old);
        } else {
            set_mtime(&p, old);
        }
    }
    set_mtime(&s1, old);
    let o = Guarded::spawn(h.zygo(&["status"], &[])).output("status");
    assert!(s1.symlink_metadata().is_err(), "{} stays: {}", s1.display(), text(&o.stderr));
    assert_eq!(names_in(&target), names);
}

// ---- W§18 #31 (AU9, W§8.3, W§8.4) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_31_a_hand_start_in_a_new_split_joins_and_resumes() {
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    config(&h, &loadout("l31", &[&[SH]]));
    let (id, ws) = up_loadout(&h, "l31", &[]);
    let script = hand_script(&h, "hand", &["claude"], &[("ZYGO_WORKSPACE", OsStr::new(&id))]);
    let pane = h.tmux(&["split-window", "-P", "-F", "#{pane_id}", "-t", &format!("={}:", ws.tmux_session), &script]);
    std::fs::write(h.root().join("hand.go"), "").unwrap();
    let mut k = String::new();
    assert!(
        until(BOUND, || {
            k = h.tmux(&["show-options", "-p", "-qv", "-t", &pane, "@zygo-pane"]).trim().to_string();
            !k.is_empty()
        }),
        "{}",
        std::fs::read_to_string(h.root().join("hand.err")).unwrap_or_default()
    );
    assert!(wait_starts(&h, &k, 1), "{}", std::fs::read_to_string(h.root().join("hand.err")).unwrap_or_default());
    let (flag, sid) = session_arg(&h, &k);
    assert_eq!(flag, "--session-id");
    assert!(argv(&h, &k).contains(&format!("--session-id={sid}")), "{:?}", argv(&h, &k));
    assert_eq!(agent_id(&h, &id, &k), sid);
    down(&h, &ws.name);
    settle_member(&h, &id, &k, MemberState::Resumable);
    let o = up(&h, &ws.name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(wait_starts(&h, &k, 2), "{}", no_start(&h, &ws, &k));
    assert_eq!(session_arg(&h, &k), ("--resume".to_string(), sid.clone()));
    assert!(argv(&h, &k).contains(&format!("--resume={sid}")), "{:?}", argv(&h, &k));
    down(&h, &ws.name);
}

// ---- W§18 #33 (AU70) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_33_each_bypass_form_is_printed_and_resumed() {
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    let p1 = format!("{CC}\nargs = ['--dangerously-skip-permissions']");
    let p2 = format!("{CC}\nargs = ['--allow-dangerously-skip-permissions']");
    let p3 = format!("{CC}\nargs = ['--permission-mode', 'bypassPermissions']");
    let p4 = format!("{CC}\nyolo = true");
    config(&h, &loadout("l33", &[&[&p1, &p2], &[&p3, &p4]]));
    let (id, ws) = up_loadout(&h, "l33", &[]);
    let by_args = |a: &[&str]| agent_key(&ws, |m| !m.launch.yolo && m.launch.args == a);
    let forms = [
        (by_args(&["--dangerously-skip-permissions"]), "--dangerously-skip-permissions"),
        (by_args(&["--allow-dangerously-skip-permissions"]), "--allow-dangerously-skip-permissions"),
        (by_args(&["--permission-mode", "bypassPermissions"]), "--permission-mode bypassPermissions"),
        (agent_key(&ws, |m| m.launch.yolo), "yolo = true"),
    ];
    let mut lines = vec![];
    for (k, f) in &forms {
        assert!(wait_starts(&h, k, 1), "{}", no_start(&h, &ws, k));
        let i = agent_id(&h, &id, k);
        lines.push((k.clone(), i.clone(), format!("zygo: pane {k}: claude-code agent session {i} resumes with {f}\n")));
    }
    let ks: Vec<&str> = forms.iter().map(|(k, _)| k.as_str()).collect();
    down(&h, &ws.name);
    settle_to(&h, &id, &ks, MemberState::Resumable);
    let o = up(&h, &ws.name, &[]);
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    for (k, i, l) in &lines {
        assert_eq!(count(&err, l), 1, "{l}{err}");
        assert!(wait_starts(&h, k, 2), "{}", no_start(&h, &ws, k));
        assert_eq!(session_arg(&h, k), ("--resume".to_string(), i.clone()));
    }
    let o = cli(&h, &["down", "--all", "--now", "--wait"], &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(until(BOUND, || settled(&h, &id)), "{:?}", state(&h, &id).members);
    assert!(state(&h, &id).down_all);
    let o = cli(&h, &["up", "--all"], &[]);
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    assert_eq!(text(&o.stdout), format!("zygo: {} is up\n", ws.name), "{err}");
    for (k, _, l) in &lines {
        assert_eq!(count(&err, l), 1, "{l}{err}");
        assert!(wait_starts(&h, k, 3), "{}", no_start(&h, &ws, k));
    }
    down(&h, &ws.name);
}

// ---- W§18 #34 (AU62, AU87; D-3) ----

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w18_34_the_saved_socket_reaches_the_server_and_must_be_private() {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    let (ta, tb) = (h.root().join("tA"), h.root().join("tB"));
    for d in [&ta, &tb] {
        std::fs::DirBuilder::new().mode(0o700).create(d).unwrap();
    }
    h.tmux(&["new-session", "-d", "-s", "other", "/bin/sleep 600"]);
    config(&h, &loadout("l34", &[&[CC, SH]]));
    let repo = h.repo();
    let o = run_in(&h, &repo, &["up", "--loadout", "l34", "--no-attach"], &[("TMUX_TMPDIR", ta.as_os_str())], false);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let (id, ws) = only_state(&h);
    let s = ws.tmux_session.clone();
    let sock = ws.tmux_socket.clone().unwrap();
    // SAFETY: getuid has no precondition and cannot fail.
    let uid = unsafe { libc::getuid() };
    assert_eq!(sock, std::fs::canonicalize(&ta).unwrap().join(format!("tmux-{uid}")).join("default"));
    let server = h.server.as_ref().unwrap();
    let mut a_srv = ServerAt { tmux: server.program().to_path_buf(), socket: sock.clone(), pid: None };
    let a = key_at(&ws, 0, 0);
    assert!(wait_starts(&h, &a, 1), "stub A did not start");
    let on_a = |args: &[&str]| {
        std::process::Command::new(server.program())
            .arg("-S")
            .arg(&sock)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let b: [(&str, &OsStr); 1] = [("TMUX_TMPDIR", tb.as_os_str())];
    let o = run_in(&h, &repo, &["ls"], &b, true);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let lead = format!("{}  {}  up  ", ws.name, ws.main_root.display());
    assert_eq!(text(&o.stdout).lines().filter(|l| l.starts_with(&lead)).count(), 1, "{lead}\n{}", text(&o.stdout));
    let o = run_in(&h, &repo, &["down", &ws.name, "--now", "--wait"], &b, true);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(state(&h, &id).phase, Phase::Down);
    assert!(!on_a(&["has-session", "-t", &format!("={s}:")]).status.success());
    assert!(server.run(&["has-session", "-t", &format!("={s}:")]).is_err());
    let o = run_in(&h, &repo, &["up", &ws.name, "--no-attach"], &b, true);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(on_a(&["has-session", "-t", &format!("={s}:")]).status.success());
    assert!(server.run(&["has-session", "-t", &format!("={s}:")]).is_err());
    assert!(wait_starts(&h, &a, 2), "stub A did not start again");
    let pid: i32 = text(&on_a(&["display-message", "-p", "#{pid}"]).stdout).trim().parse().unwrap();
    a_srv.pid = Some(pid);
    std::fs::remove_file(&sock).unwrap();
    let line = format!("zygo: {} has live agents (pane {a}) but its tmux session cannot be reached at {}", ws.name, sock.display());
    let o = run_in(&h, &repo, &["up", &ws.name, "--no-attach"], &b, true);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o.stderr));
    assert!(text(&o.stderr).contains(&line), "{line}\n{}", text(&o.stderr));
    // An agent that this `up` started by mistake would start within the 5 s of W§18 (stub starts are counted).
    std::thread::sleep(FIVE);
    assert_eq!(starts(&h, &a), 2);
    let o = run_in(&h, &repo, &["down", &ws.name, "--now", "--wait"], &b, true);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o.stderr));
    assert!(text(&o.stderr).contains(&line), "{line}\n{}", text(&o.stderr));
    a_srv.kill_now(libc::SIGTERM);
    assert!(until(BOUND, || settled(&h, &id)), "{:?}", state(&h, &id).members);
    let open = h.root().join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    let open_s = open.join("s");
    h.store(&id)
        .update(Duration::from_secs(5), &mut |w| {
            w.tmux_socket = Some(open_s.clone());
            Ok(())
        })
        .unwrap();
    let o = run_in(&h, &repo, &["up", &ws.name, "--no-attach"], &b, true);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o.stderr));
    assert!(text(&o.stderr).contains(&format!("tmux socket {} is not private", open_s.display())), "{}", text(&o.stderr));
    // A saved socket whose parent folder exists and is owned by root (`/`). Only the AU54 path check
    // (`check_saved_socket`) runs before the AU62 check (`up.rs:977-979`); in `socket_is_private` the parent exists and
    // is a folder, so `meta.uid() != uid` (`up.rs:308`) refuses it before its mode is read.
    if uid == 0 {
        let _ = std::io::stderr().write_all(b"w18_34: the root-owned parent case skipped: the test runs as root\n");
    } else if std::fs::metadata("/").unwrap().uid() != 0 {
        let _ = std::io::stderr().write_all(b"w18_34: the root-owned parent case skipped: / is not owned by root\n");
    } else {
        let root_s = PathBuf::from("/zygo-e1-w18-34-s");
        h.store(&id)
            .update(Duration::from_secs(5), &mut |w| {
                w.tmux_socket = Some(root_s.clone());
                Ok(())
            })
            .unwrap();
        let o = run_in(&h, &repo, &["up", &ws.name, "--no-attach"], &b, true);
        assert_eq!(o.status.code(), Some(1), "{}", text(&o.stderr));
        let refused = format!(
            "zygo: tmux socket {} is not private: the socket and its folder must be yours and the folder must have mode 0700",
            root_s.display()
        );
        assert!(text(&o.stderr).lines().any(|l| l == refused), "{refused}\n{}", text(&o.stderr));
    }
}

// ---- W§18 #37 (AU67; E-5) ----

#[test]
#[ignore = "timing: the test's kill races the 2 s tmux call bound"]
fn w18_37_an_up_killed_after_the_first_window_is_repaired() {
    use std::os::unix::process::ExitStatusExt;
    let Some(h) = Harness::new() else { return };
    git_repo_at(&h, &h.repo());
    config(&h, &loadout("l37", &[&[SH, SH], &[SH]]));
    // `bin/tmux` is a link to the real tmux (`common/workspace.rs:243`): remove the link first, then write a new file.
    // Never write through the link.
    let t = h.bin.join("tmux");
    std::fs::remove_file(&t).unwrap();
    let real = h.server.as_ref().unwrap().program().to_path_buf();
    let root = h.root().display().to_string();
    {
        use std::os::unix::fs::PermissionsExt;
        let body = format!(
            "#!/bin/sh\ncase \" $* \" in\n*\" new-window \"*) if /bin/mkdir '{root}/nw-once' 2>/dev/null; then echo $$ > '{root}/nw-pid'; exec /bin/sleep 30; fi ;;\nesac\nexec '{}' \"$@\"\n",
            real.display()
        );
        std::fs::write(&t, body).unwrap();
        std::fs::set_permissions(&t, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    ready_exe(&t);
    let nw_pid = h.root().join("nw-pid");
    let mut k = KillOnDrop::new(nw_pid.clone());
    let repo = h.repo();
    let mut g = Guarded::spawn(cmd_in(&h, &repo, &["up", "--loadout", "l37", "--no-attach"], &[], true));
    let mut held = 0;
    assert!(
        until(BOUND, || {
            held = std::fs::read_to_string(&nw_pid).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            held > 0
        }),
        "the wrapper did not hold a new-window call"
    );
    // SAFETY: `kill` takes plain integers; `g` leads its own group and is not reaped.
    unsafe {
        libc::kill(-(g.id() as libc::pid_t), libc::SIGKILL);
    }
    // The wrapper is not in that group: zygo runs each tmux call in a process group of its own
    // (`crates/zygo-workspace/src/mux/tmux.rs:64`). It still sleeps (30 s), so it gets its one signal now, and the
    // helper is disarmed.
    k.kill_now(libc::SIGKILL);
    let st = g.wait("up");
    assert!(st.signal() == Some(libc::SIGKILL) || st.code() == Some(1), "{st:?}");
    let (id, ws) = only_state(&h);
    let s = ws.tmux_session.clone();
    assert_eq!(ws.phase, Phase::Building);
    assert!(ws.build_plan.is_some());
    assert_eq!(ws.panes.len(), 3, "{:?}", ws.panes);
    assert_eq!(window_count(&h, &s), 1);
    let o = up(&h, &ws.name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let now = state(&h, &id);
    assert_eq!(now.phase, Phase::Up);
    assert_eq!(now.build_plan, None);
    assert_eq!(tmux_keys(&h, &s), keys(&ws));
    assert_eq!(window_count(&h, &s), 2);
    assert_held_released(&h, &s);
    down(&h, &ws.name);
}

// ---- W§17 PW11 (re-check on the real `zygo ws run`; AU88) ----

#[test]
#[ignore = "timing: sleeps let the shell read its terminal; C-c effect within 5 s"]
fn pw11_ws_run_leaves_a_working_interactive_shell() {
    let mut shells: Vec<PathBuf> = vec![PathBuf::from("/bin/sh")];
    if Path::new("/bin/bash").exists() {
        shells.push(PathBuf::from("/bin/bash"));
    }
    for name in ["zsh", "fish"] {
        if let Ok(p) = which::which(name) {
            shells.push(p);
        }
    }
    let mut seen: Vec<PathBuf> = vec![];
    shells.retain(|p| {
        let new = !seen.contains(p);
        seen.push(p.clone());
        new
    });
    for x in &shells {
        let Some(h) = Harness::new() else { return };
        let xs = x.display().to_string();
        let base = x.file_name().unwrap().to_string_lossy().into_owned();
        git_repo_at(&h, &h.repo());
        per_pane_claude(&h, "");
        std::fs::write(h.root().join(".zshrc"), "").unwrap();
        config(&h, &loadout("lp", &[&[CC, SH]]));
        let (id, ws) = up_loadout(&h, "lp", &[("SHELL", x.as_os_str())]);
        let s = ws.tmux_session.clone();
        let a = key_at(&ws, 0, 0);
        // a: the pane runs `ws run`.
        assert!(wait_starts(&h, &a, 1), "{xs}: {}", no_start(&h, &ws, &a));
        let r = row(&h, &s, &a);
        assert!(r.start.contains(&format!("ws run --ws-id {id} --pane {a}")) && !r.start.ends_with("--shell"), "{xs}: {r:?}");
        // b: the agent exits; the `exec`ed shell keeps the pid.
        stub_control(&ctl(&h, &a), "exit 0").unwrap();
        assert!(wait_shell(&h, &r.id, &base), "{xs}: {} {}", cmd_of(&h, &r.id), capture(&h, &r.id));
        assert_eq!(row(&h, &s, &a).pid, r.pid, "{xs}");
        #[cfg(target_os = "linux")]
        {
            let (ign, blk, cgt) = sig_masks(r.pid);
            assert!(
                ign & SIGINT_BIT == 0 && blk & SIGINT_BIT == 0,
                "{xs}: SigIgn {ign:016x} SigBlk {blk:016x} SigCgt {cgt:016x}"
            );
        }
        // Interrupt.
        send(&h, &r.id, "/bin/sleep 30");
        assert!(wait_cmd(&h, &r.id, "sleep"), "{xs}: {}", capture(&h, &r.id));
        h.tmux(&["send-keys", "-t", &r.id, "C-c"]);
        assert!(until(FIVE, || cmd_of(&h, &r.id) == base), "{xs}: {} {}", cmd_of(&h, &r.id), capture(&h, &r.id));
        // c: the shell is on the pane's terminal.
        let tty = h.root().join("pw11-tty");
        send(&h, &r.id, &format!("/bin/sh -c 'test -t 0 && test -t 1 && echo ok > {}'", tty.display()));
        assert_eq!(wait_file(&tty, BOUND).as_deref(), Some("ok\n"), "{xs}: {}", capture(&h, &r.id));
        // d: a second command runs.
        let second = h.root().join("pw11-second");
        send(&h, &r.id, &format!("/bin/sh -c 'echo ok > {}'", second.display()));
        assert_eq!(wait_file(&second, BOUND).as_deref(), Some("ok\n"), "{xs}: {}", capture(&h, &r.id));
        // e: `exit` ends the pane; the session keeps the shell pane.
        send(&h, &r.id, "exit");
        assert!(until(BOUND, || !rows(&h, &s).iter().any(|q| q.id == r.id)), "{xs}: {:?}", rows(&h, &s));
        assert!(rows(&h, &s).iter().any(|q| q.key == key_at(&ws, 0, 1)), "{xs}: {:?}", rows(&h, &s));
        down(&h, &ws.name);
    }
}

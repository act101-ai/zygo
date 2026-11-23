use std::collections::HashSet;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod common;

fn zygo(args: &[&str]) -> std::process::Output {
    let h = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut cmd, h.path());
    cmd.args(args).stdin(Stdio::null()).output().expect("run zygo")
}

fn help_of(args: &[&str]) -> String {
    let mut v: Vec<&str> = args.to_vec();
    v.push("--help");
    let out = zygo(&v);
    assert!(out.status.success(), "{args:?} --help failed");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn help_lists_every_command() {
    let help = help_of(&[]);
    let listed: HashSet<&str> = help
        .lines()
        .skip_while(|l| l.trim() != "Commands:")
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    for c in [
        "claude", "opencode", "serve", "status", "doctor", "compact", "profile", "work", "usage",
        "marketplace", "plugins", "plugin", "keys", "config", "update", "help", "results",
        "redaction", "guards", "units", "compact-tool", "statusline", "mcp", "hook",
        "up", "down", "ls", "attach", "loadout", "workspace", "ws",
    ] {
        assert!(listed.contains(c), "--help Commands lacks {c}: {listed:?}");
    }
}

#[test]
fn every_command_exists() {
    let cmds = "claude|opencode|serve|serve token new|serve reload|status|doctor|compact|profile list|profile show|profile use|work|work report|usage|marketplace add|marketplace list|marketplace update|marketplace remove|plugins list|plugins sync|plugins enable|plugins disable|plugins approve|plugin check|keys set|keys rm|keys list|config show|config check|update|help|results|redaction|guards|units|compact-tool|statusline|mcp|hook|up|down|ls|attach|loadout list|workspace|ws forget|ws forget-pane|ws snapshot";
    for c in cmds.split('|') {
        let mut argv: Vec<&str> = c.split(' ').collect();
        argv.push("--help");
        let out = zygo(&argv);
        assert!(out.status.success(), "zygo {c} --help exited {:?}", out.status.code());
    }
}

/// W§12, AU44: the flags of the workspace verbs; `ws snapshot` is hidden.
#[test]
fn workspace_verb_flags() {
    let up = help_of(&["up"]);
    for f in ["--loadout", "--reset", "--no-attach", "--all"] {
        assert!(up.contains(f), "up lacks {f}: {up}");
    }
    let down = help_of(&["down"]);
    for f in ["--now", "--timeout", "--wait", "--all"] {
        assert!(down.contains(f), "down lacks {f}: {down}");
    }
    assert!(down.contains("Stop the workspace of this folder or of <name>: drain, snapshot, stop; in the foreground, detached when run from an agent or from a pane of that workspace"), "{down}");
    let pane = help_of(&["ws", "forget-pane"]);
    assert!(pane.contains("--name"), "{pane}");
    let ws = help_of(&["ws"]);
    let listed: HashSet<&str> = ws
        .lines()
        .skip_while(|l| l.trim() != "Commands:")
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert!(listed.contains("forget") && listed.contains("forget-pane"), "{ws}");
    assert!(!listed.contains("snapshot"), "{ws}");
}

/// OP11, decision 2026-11-23: `zygo help down` describes both forms.
#[test]
fn help_down_describes_both_forms() {
    let out = zygo(&["help", "down"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "down [--now] [--timeout <dur>] [--wait] [--all] [--] [<name>]\nzygo down: stop the workspace of this folder or of <name>, in the foreground (detached from an agent or from a pane of that workspace). /zygo:down takes only --now and stops the workspace of this session, detached\nscope: none\n"
    );
}

#[test]
fn flags() {
    let h = help_of(&["claude"]);
    for f in ["--passthrough", "--profile", "--server", "--compact", "--yolo", "--config-dir"] {
        assert!(h.contains(f), "claude lacks {f}");
    }
    assert!(h.contains("Claude Code config directory (sets CLAUDE_CONFIG_DIR)"));
    let o = help_of(&["opencode"]);
    assert!(!o.contains("--passthrough"));
    assert!(!o.contains("--config-dir"));
    let h = help_of(&["marketplace", "add"]);
    for f in ["--name", "--ref", "--enable"] {
        assert!(h.contains(f), "marketplace add lacks {f}");
    }
    let h = help_of(&["marketplace", "update"]);
    for f in ["--all", "--accept"] {
        assert!(h.contains(f), "marketplace update lacks {f}");
    }
    let h = help_of(&["usage"]);
    for f in ["--session", "--since", "--sync"] {
        assert!(h.contains(f), "usage lacks {f}");
    }
}

#[test]
fn verbs_match_spec() {
    let want: [(&str, &str); 17] = [
        ("help", "[<verb>]"),
        ("status", ""),
        ("usage", ""),
        ("compact", "<tokens|on|off>"),
        ("results", "on|off|threshold <bytes>"),
        ("redaction", "[content on|off]"),
        ("guards", "[<guard> <level>]"),
        ("units", "on|off"),
        ("work", "[<id>|clear]"),
        ("plugins", "[list|enable|disable|approve <m>:<p>]"),
        ("marketplace", "[list|add|update|remove …]"),
        ("profile", "[<name>|none]"),
        ("compact-tool", "on|off"),
        ("statusline", "[compact|verbose]"),
        ("down", "[--now] [--timeout <dur>] [--wait] [--all] [--] [<name>]"),
        ("ls", ""),
        ("workspace", ""),
    ];
    let specs = zygo_core::verbs::VERB_SPECS;
    assert_eq!(specs.len(), want.len());
    for (s, (n, a)) in specs.iter().zip(want) {
        assert_eq!((s.name, s.args), (n, a));
    }
    let names: Vec<&str> = specs.iter().map(|s| s.name).collect();
    assert_eq!(
        names,
        ["help", "status", "usage", "compact", "results", "redaction", "guards", "units", "work", "plugins", "marketplace", "profile", "compact-tool", "statusline", "down", "ls", "workspace"]
    );
    let verbs = zygo::cmd::verbs::VERBS;
    assert_eq!(verbs.len(), specs.len());
    for (i, v) in verbs.iter().enumerate() {
        assert!(std::ptr::eq(v.spec, &specs[i]));
    }
}

#[test]
fn binary_exit_zero() {
    let run = |args: &[&str], input: &str| {
        let h = tempfile::tempdir().unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        common::scratch_home(&mut cmd, h.path());
        let mut c = cmd
            .args(args)
            .env_remove("ZYGO_SESSION_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        c.wait_with_output().unwrap()
    };
    let o = run(&["hook", "role-guard"], "not json");
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&o.stderr).lines().count(), 1);
    let o = run(&["hook", "role-guard"], "{}");
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stderr.is_empty());
    let o = zygo(&["hook", "token"]);
    assert_eq!(o.status.code(), Some(0));
    let o = zygo(&["hook", "nope"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&o.stderr).lines().count(), 1);
    // `token` must not read stdin: an open pipe that is never written or closed.
    let h = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut cmd, h.path());
    let mut c = cmd
        .args(["hook", "token"])
        .env_remove("ZYGO_SESSION_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _held = c.stdin.take();
    let start = Instant::now();
    let status = loop {
        if let Some(s) = c.try_wait().unwrap() {
            break Some(s);
        }
        if start.elapsed() > Duration::from_secs(2) {
            let _ = c.kill();
            let _ = c.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.and_then(|s| s.code()), Some(0), "hook token waited on stdin");
}

/// C6: every `zygo hook …` exits 0 with at most one stderr line.
#[test]
fn hook_c6_exit_paths() {
    let cases: &[&[&str]] = &[
        &["hook"],
        &["hook", "nope"],
        &["hook", "role-guard", "--agent", "bogus"],
        &["hook", "role-guard", "--bogus-flag"],
        &["hook", "role-guard", "extra"],
        &["hook", "role-guard", "--event"],
    ];
    for args in cases {
        let o = zygo(args);
        assert_eq!(o.status.code(), Some(0), "{args:?}");
        assert!(o.stdout.is_empty(), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&o.stderr).lines().count(), 1, "{args:?}");
    }
    // With stderr a closed pipe, a write to stderr must not panic.
    for args in cases {
        let (r, w) = std::io::pipe().unwrap();
        drop(r);
        let h = tempfile::tempdir().unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        common::scratch_home(&mut cmd, h.path());
        let o = cmd
            .args(*args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(w)
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(0), "{args:?} with closed stderr");
    }
}

#[test]
fn hook_table_names() {
    let names: Vec<&str> = zygo::cmd::hook::TABLE.iter().map(|h| h.name).collect();
    assert_eq!(
        names,
        ["token", "statusline", "trim", "session-start", "role-guard", "prompt", "pre-compact", "work", "guard", "workspace"]
    );
    for h in zygo::cmd::hook::TABLE {
        // `token` gets no stdin; `statusline` reads its own stdin with a bound (AU24, H§11).
        assert_eq!(h.reads_stdin, !matches!(h.name, "token" | "statusline"), "reads_stdin of {}", h.name);
    }
}

// ---- H1: `zygo hook workspace` (D18) ----

#[cfg(unix)]
const WS_ID: &str = "0123456789abcdef";
#[cfg(unix)]
const WS_KEY: &str = "kaaaaaaaa";
#[cfg(unix)]
const WS_SID: &str = "01JTESTSESSIONAAAAAAAAAAAA";

/// A member session folder `h/s` and the paths of `HOME = h`. In the test process the state folder of these paths is
/// `<data_dir>/state` (the test's `HOME` is not `h`); the link `h/.local/state/zygo` to it makes the binary run with
/// `HOME = h` name the same files (a link above the state folder is followed, FIX-17).
#[cfg(unix)]
fn member_home(h: &std::path::Path, agent: zygo_core::Agent) -> (zygo_core::session_dir::SessionDir, zygo_core::paths::Paths) {
    use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
    let sd = zygo_core::session_dir::SessionDir::new(h.join("s"));
    sd.create().unwrap();
    let member = Member { ws_id: WS_ID.into(), pane: WS_KEY.into(), ..Default::default() };
    SessionView {
        session: WS_SID.into(),
        agent,
        main_role: "lead".into(),
        workspace: Some(ViewWorkspace { member, resume_note: None, ..Default::default() }),
        ..Default::default()
    }
    .save(&sd)
    .unwrap();
    let data = h.join(".local/share/zygo");
    let paths = zygo_core::paths::Paths {
        config_file: h.join("config.toml"),
        cache_dir: h.join(".cache/zygo"),
        data_dir: data.clone(),
        sessions_dir: h.join(".cache/zygo/sessions"),
        marketplaces_dir: h.join(".cache/zygo/marketplaces"),
        plugin_store: data.join("plugins"),
        approvals_file: data.join("approvals.toml"),
        usage_db: data.join("usage.db"),
    };
    std::fs::create_dir_all(data.join("state")).unwrap();
    std::fs::create_dir_all(h.join(".local/state")).unwrap();
    std::os::unix::fs::symlink(data.join("state"), h.join(".local/state/zygo")).unwrap();
    (sd, paths)
}

/// `zygo hook workspace <args>` with `HOME = h`, `ZYGO_SESSION_DIR = sd` and `stdin`.
#[cfg(unix)]
fn run_ws(h: &std::path::Path, sd: &zygo_core::session_dir::SessionDir, args: &[&str], stdin: &[u8]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut cmd, h);
    let mut child = cmd
        .args(["hook", "workspace"])
        .args(args)
        .env("ZYGO_SESSION_DIR", &sd.root)
        .env_remove("ZYGO_AGENT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

#[cfg(unix)]
fn hook_fixture(name: &str) -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/claude-code/hooks/{name}.json"))).unwrap()
}

/// A state of `WS_ID` in `phase` with the member records `members`.
#[cfg(unix)]
fn ws_state(
    paths: &zygo_core::paths::Paths, phase: zygo_workspace::state::Phase,
    members: std::collections::BTreeMap<String, zygo_workspace::state::MemberRecord>,
) -> zygo_workspace::state::Store {
    let store = zygo_workspace::state::Store::open(paths, WS_ID);
    store
        .update(Duration::from_secs(5), &mut |w| {
            w.name = "app".into();
            w.tmux_session = "zygo-app".into();
            w.common_dir = "/srv/app/.git".into();
            w.main_root = "/srv/app".into();
            w.loadout = "solo".into();
            w.phase = phase;
            w.members = members.clone();
            Ok(())
        })
        .unwrap();
    store
}

#[test]
fn workspace_hook_needs_a_known_event() {
    for (args, err) in [
        (&[][..], "zygo hook: workspace: --event is required\n"),
        (&["--event", "frob"][..], "zygo hook: workspace: unknown event \"frob\"\n"),
    ] {
        let h = tempfile::tempdir().unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        common::scratch_home(&mut cmd, h.path());
        let mut child = cmd
            .args(["hook", "workspace"])
            .args(args)
            .env_remove("ZYGO_SESSION_DIR")
            .env_remove("ZYGO_AGENT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"{}").unwrap();
        let o = child.wait_with_output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{args:?}");
        assert!(o.stdout.is_empty(), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&o.stderr), err, "{args:?}");
    }
}

/// One real `pre-tool` run leaves a running tool that the drain counts as busy (W10 round 2, D5).
#[cfg(unix)]
#[test]
fn workspace_hook_end_to_end() {
    use zygo_workspace::drain::{self, Idle};
    let h = tempfile::tempdir().unwrap();
    let (sd, _paths) = member_home(h.path(), zygo_core::Agent::ClaudeCode);
    let o = run_ws(h.path(), &sd, &["--event", "pre-tool"], &hook_fixture("PreToolUse-Bash-main"));
    assert_eq!((o.status.code(), o.stdout.as_slice(), o.stderr.as_slice()), (Some(0), &b""[..], &b""[..]));
    let a = zygo_workspace::activity::read(&sd).unwrap();
    assert_eq!((a.busy, a.tools_running), (false, 1));
    let quiet = zygo_proxy::client::SessionStatus { requests_in_flight: Some(0), last_request_end: None, ..Default::default() };
    assert_eq!(
        drain::idle(Ok(&a), Ok(&quiet), std::time::SystemTime::now()),
        Idle::Busy { reason: "tool running".into(), rule_skipped: false }
    );
}

/// The deny JSON of W§9.2 on the binary's stdout.
#[cfg(unix)]
#[test]
fn workspace_hook_denies_on_stdout() {
    use zygo_workspace::state::{MemberRecord, Phase};
    let h = tempfile::tempdir().unwrap();
    let (sd, paths) = member_home(h.path(), zygo_core::Agent::ClaudeCode);
    let members = std::collections::BTreeMap::from([(WS_KEY.to_string(), MemberRecord::new(zygo_core::Agent::ClaudeCode))]);
    let store = ws_state(&paths, Phase::Draining, members);
    let _op = store.op_lock().unwrap();
    let o = run_ws(h.path(), &sd, &["--event", "pre-tool"], &hook_fixture("PreToolUse-Agent-main"));
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stderr.is_empty(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap(),
        serde_json::json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
            "permissionDecisionReason": "zygo: workspace is draining for down; finish your turn"}})
    );
    assert_eq!(zygo_workspace::activity::read(&sd).unwrap().tools_running, 0);
}

/// W§18 #86, AU27, AU104: the OpenCode plugin's member calls through the binary.
#[cfg(unix)]
#[test]
fn opencode_workspace_hook_through_the_binary() {
    use zygo_workspace::state::{MemberRecord, MemberState, Pending, Phase};
    let h = tempfile::tempdir().unwrap();
    let (sd, paths) = member_home(h.path(), zygo_core::Agent::OpenCode);
    let mut rec = MemberRecord::new(zygo_core::Agent::OpenCode);
    rec.pending = Some(Pending { zygo_session: WS_SID.into(), agent_session: None, started_at: 7, ..Default::default() });
    let store = ws_state(&paths, Phase::Up, std::collections::BTreeMap::from([(WS_KEY.to_string(), rec)]));
    let stdin = br#"{"sessionID": "ses_A", "cwd": "/repo"}"#;
    let bt = || {
        let a = zygo_workspace::activity::read(&sd).unwrap();
        (a.busy, a.tools_running)
    };
    let quiet = |event: &str, stdin: &[u8]| {
        let o = run_ws(h.path(), &sd, &["--agent", "opencode", "--event", event], stdin);
        assert_eq!((o.status.code(), o.stdout.as_slice(), o.stderr.as_slice()), (Some(0), &b""[..], &b""[..]), "{event}");
    };
    quiet("prompt", stdin);
    assert_eq!(bt(), (true, 0));
    quiet("stop", stdin);
    assert_eq!(bt(), (false, 0));
    zygo_workspace::activity::update(&sd, Duration::from_secs(5), &mut |a| {
        a.busy = true;
        a.tools_running = 2;
    })
    .unwrap();
    // The load-time call with no id commits the pending start only (design H14, hook part).
    quiet("session", br#"{"cwd": "/repo"}"#);
    assert_eq!(bt(), (true, 2)); // `session` changes no activity
    let m = store.read().unwrap().unwrap().members[WS_KEY].clone();
    assert_eq!(
        (m.pending, m.zygo_session.as_deref(), m.agent_session.as_deref(), m.state),
        (None, Some(WS_SID), None, MemberState::Running)
    );
    // A call with an id records it; the snapshot child it starts is not waited for here.
    quiet("session", stdin);
    assert_eq!(bt(), (true, 2));
    let m = store.read().unwrap().unwrap().members[WS_KEY].clone();
    assert_eq!(m.agent_session.as_deref(), Some("ses_A"));
}

/// D2, D3 (H1 attempt-2 review round 1, F3): a member `SessionStart` (`--event member`) whose session folder is a link
/// or a relative path checks the folder first: one stderr line in the D2 form, the session text of today's read with
/// no note, and nothing written into the folder, also when the view does not load.
#[cfg(unix)]
#[test]
fn member_session_start_refuses_a_linked_or_relative_session_folder() {
    use zygo_core::session_view::{ResumeNote, SessionView};
    for case in ["link", "relative"] {
        for view_ok in [true, false] {
            let h = tempfile::tempdir().unwrap();
            let (sd, _paths) = member_home(h.path(), zygo_core::Agent::ClaudeCode);
            let mut v = SessionView::load(&sd).unwrap();
            v.workspace.as_mut().unwrap().resume_note =
                Some(ResumeNote { clean_stop: false, interrupted: vec![], closed_units: vec!["gh:7".into()], agent_session: None, ..Default::default() });
            v.save(&sd).unwrap();
            if !view_ok {
                std::fs::write(sd.view_json(), "{").unwrap();
            }
            let view_before = std::fs::read(sd.view_json()).unwrap();
            std::os::unix::fs::symlink(&sd.root, h.path().join("l")).unwrap();
            let (var, line) = match case {
                "link" => (h.path().join("l"), format!("zygo hook: {}: session folder is a link\n", h.path().join("l").display())),
                _ => (std::path::PathBuf::from("s"), "zygo hook: ZYGO_SESSION_DIR is not an absolute path: s\n".to_string()),
            };
            let input = hook_fixture("SessionStart-main");
            let want = if view_ok {
                let i = zygo_core::hook::HookInput::parse(zygo_core::Agent::ClaudeCode, &input).unwrap();
                zygo_hooks::session_start::run_in(&i, &sd).unwrap().to_stdout()
            } else {
                String::new()
            };
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
            common::scratch_home(&mut cmd, h.path());
            cmd.args(["hook", "session-start", "--event", "member"])
                .current_dir(h.path())
                .env("ZYGO_SESSION_DIR", &var)
                .env_remove("ZYGO_AGENT");
            let o = run_bounded(&mut cmd, &input);
            let tag = format!("{case}, view ok {view_ok}");
            assert_eq!(o.status.code(), Some(0), "{tag}");
            assert_eq!(String::from_utf8_lossy(&o.stderr), line, "{tag}");
            assert_eq!(String::from_utf8_lossy(&o.stdout), want, "{tag}");
            if view_ok {
                assert!(!want.contains("gh:7"), "{tag}: the note was printed");
            }
            assert_eq!(std::fs::read(sd.view_json()).unwrap(), view_before, "{tag}");
            assert!(!sd.log().exists(), "{tag}");
            assert!(!sd.activity_json().exists(), "{tag}");
        }
    }
}

/// `cmd` with `stdin` under a 5 s bound: a hook that waits fails the test instead of hanging it.
#[cfg(unix)]
fn run_bounded(cmd: &mut Command, stdin: &[u8]) -> std::process::Output {
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the hook waited");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// W-C6, OP9, D3: `zygo hook prompt` reads no session view, so no form of `view.json` changes its output or writes a
/// file (H1 attempt 1 round 3 F1).
#[cfg(unix)]
#[test]
fn prompt_hook_reads_no_session_view() {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::os::unix::ffi::OsStrExt;
    for case in ["fifo", "dangling link", "empty", "not JSON", "mode 0", "member"] {
        let h = tempfile::tempdir().unwrap();
        let s = h.path().join("s");
        std::fs::create_dir_all(&s).unwrap();
        let view = s.join("view.json");
        match case {
            "fifo" => {
                let c = std::ffi::CString::new(view.as_os_str().as_bytes()).unwrap();
                // SAFETY: `c` is a valid NUL-terminated path.
                assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
            }
            "dangling link" => std::os::unix::fs::symlink(h.path().join("missing"), &view).unwrap(),
            "empty" => std::fs::write(&view, "").unwrap(),
            "not JSON" => std::fs::write(&view, "{").unwrap(),
            "mode 0" => {
                let sd = zygo_core::session_dir::SessionDir::new(s.clone());
                zygo_core::session_view::SessionView { session: WS_SID.into(), main_role: "lead".into(), ..Default::default() }
                    .save(&sd)
                    .unwrap();
                std::fs::set_permissions(&view, std::fs::Permissions::from_mode(0o000)).unwrap();
            }
            _ => {
                member_home(h.path(), zygo_core::Agent::ClaudeCode);
            }
        }
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        common::scratch_home(&mut cmd, h.path());
        cmd.args(["hook", "prompt"]).env("ZYGO_SESSION_DIR", &s).env_remove("ZYGO_AGENT");
        let o = run_bounded(&mut cmd, &hook_fixture("UserPromptSubmit-main"));
        assert_eq!(o.status.code(), Some(0), "{case}");
        assert!(o.stdout.is_empty(), "{case}: {}", String::from_utf8_lossy(&o.stdout));
        assert!(o.stderr.is_empty(), "{case}: {}", String::from_utf8_lossy(&o.stderr));
        assert!(!s.join("session.log").exists(), "{case}");
        assert!(!s.join("activity.json").exists(), "{case}");
        if case == "fifo" {
            assert!(std::fs::symlink_metadata(&view).unwrap().file_type().is_fifo());
        }
    }
}

/// W§9.1, W§9.2, AU86, D4: the Claude Code member prompt entry prints the block on stdout while `down` holds
/// `op.lock` in `stopping`.
#[cfg(unix)]
#[test]
fn claude_code_workspace_prompt_blocks_on_stdout() {
    use zygo_workspace::state::{MemberRecord, Phase};
    let h = tempfile::tempdir().unwrap();
    let (sd, paths) = member_home(h.path(), zygo_core::Agent::ClaudeCode);
    let members = std::collections::BTreeMap::from([(WS_KEY.to_string(), MemberRecord::new(zygo_core::Agent::ClaudeCode))]);
    let store = ws_state(&paths, Phase::Stopping, members);
    let _op = store.op_lock().unwrap();
    let o = run_ws(h.path(), &sd, &["--event", "prompt"], &hook_fixture("UserPromptSubmit-main"));
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stderr.is_empty(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap(),
        serde_json::json!({"decision": "block", "reason": "zygo: workspace is stopping"})
    );
    assert!(!sd.activity_json().exists());
}

/// W§9.1 `UserPromptSubmit`, D4; H1 attempt-2 review round 3 S8: the binary registers the `/zygo:` dispatcher, so a
/// member prompt with a verb line of `VERB_SPECS` that `zygo hook prompt` consumes leaves `activity.json` unchanged,
/// and a plain line starts a busy turn.
#[cfg(unix)]
#[test]
fn member_prompt_hook_skips_a_consumed_verb_line() {
    let h = tempfile::tempdir().unwrap();
    let (sd, _paths) = member_home(h.path(), zygo_core::Agent::ClaudeCode);
    zygo_workspace::activity::update(&sd, Duration::from_secs(5), &mut |a| a.tools_running = 2).unwrap();
    let before = std::fs::read(sd.activity_json()).unwrap();
    let line = |p: &str| {
        let mut v: serde_json::Value = serde_json::from_slice(&hook_fixture("UserPromptSubmit-main")).unwrap();
        v["prompt"] = serde_json::json!(p);
        v.to_string().into_bytes()
    };
    assert!(zygo_core::verbs::VERB_SPECS.iter().any(|s| s.name == "status"));
    let o = run_ws(h.path(), &sd, &["--event", "prompt"], &line("/zygo:status"));
    assert_eq!((o.status.code(), o.stdout.as_slice(), o.stderr.as_slice()), (Some(0), &b""[..], &b""[..]));
    assert_eq!(std::fs::read(sd.activity_json()).unwrap(), before);
    let o = run_ws(h.path(), &sd, &["--event", "prompt"], &line("hello"));
    assert_eq!((o.status.code(), o.stdout.as_slice(), o.stderr.as_slice()), (Some(0), &b""[..], &b""[..]));
    let a = zygo_workspace::activity::read(&sd).unwrap();
    assert_eq!((a.busy, a.tools_running), (true, 0));
}

/// `--config-dir` is checked before any agent launch: exit 2, one stderr line.
#[test]
fn claude_config_dir_missing_exits_2() {
    let h = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let absent = h.path().join("absent");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut cmd, h.path());
    let o = cmd.args(["claude", "--config-dir"]).arg(&absent).env("PATH", bin.path()).env_remove("ZYGO_SESSION_DIR").env_remove("ZYGO_AGENT")
        .current_dir(h.path()).stdin(Stdio::null()).output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(2), "{err}");
    assert!(err.contains("not a directory") && err.lines().count() == 1, "{err:?}");
}

// ---- N72: verb and config errors are escaped ----

const FORGED_PROFILE: &str = "[profile.\"x\\u001b[2K\\nforged\"]\nprompt = 1\n";
const FORGED_LOAD_END: &str = "profile.\"x\\u{1b}[2K\\nforged\".prompt at line 2: expected a string, found an integer\n";

fn with_config(config: &str, args: &[&str]) -> std::process::Output {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.toml"), config).unwrap();
    Command::new(env!("CARGO_BIN_EXE_zygo")).args(args).env("HOME", d.path()).env("LOCALAPPDATA", d.path()).env("APPDATA", d.path())
        .env("ZYGO_CONFIG", d.path().join("config.toml")).env_remove("ZYGO_SESSION_DIR").env_remove("ZYGO_AGENT")
        .env_remove("XDG_DATA_HOME").env_remove("XDG_CACHE_HOME")
        .current_dir(d.path()).stdin(Stdio::null()).output().unwrap()
}

#[test]
fn verb_error_is_escaped_on_stderr() {
    let o = with_config("", &["guards", "x\u{1b}[2K\nforged", "on", "--save"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&o.stderr), "refused: unknown guard `x\\u{1b}[2K\\nforged`\n");
}

#[test]
fn config_check_violation_is_escaped() {
    let o = with_config("[role.\"x\\u001b[2K\\nforged\"]\nmodel = \"nope\"\n", &["config", "check"]);
    assert_eq!(o.status.code(), Some(11));
    assert_eq!(String::from_utf8_lossy(&o.stdout), "error: role.\"x\\u{1b}[2K\\nforged\".model: unknown model\n");
}

#[test]
fn config_show_load_error_is_escaped() {
    let o = with_config(FORGED_PROFILE, &["config", "show"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(11), "{err}");
    assert!(err.starts_with("zygo config: user config ") && err.ends_with(FORGED_LOAD_END), "{err:?}");
    assert!(!err.contains('\u{1b}') && err.lines().count() == 1, "{err:?}");
}

#[test]
fn mcp_load_error_is_escaped() {
    let o = with_config(FORGED_PROFILE, &["mcp", "--agent", "claude-code"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(11), "{err}");
    assert!(err.starts_with("zygo mcp: user config ") && err.ends_with(FORGED_LOAD_END), "{err:?}");
    assert!(!err.contains('\u{1b}') && err.lines().count() == 1, "{err:?}");
}

/// Round 2 F4: the `--layer` echo is escaped.
#[test]
fn config_show_layer_echo_is_escaped() {
    let o = with_config("", &["config", "show", "--layer", "x\u{1b}[2K\nforged"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(String::from_utf8_lossy(&o.stderr),
        "zygo config: unknown layer x\\u{1b}[2K\\nforged (builtin|team|user|repository|profile|flags|session)\n");
}

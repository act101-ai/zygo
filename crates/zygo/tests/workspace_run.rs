//! M2: `zygo ws run`, the member wrapper and the workspace test harness (W§8.2, W§8.3, W§9.4 inputs, W§7.3, W§7.4,
//! W§18 preamble). Every test runs a workspace verb, so the file is `cfg(unix)` (OP5).
#![cfg(unix)]
mod common;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::workspace::{output_child, ready_exe, wait_child, Harness, WS_ID};
use serde_json::{json, Value};
use zygo_core::Agent;
use zygo_workspace::state::{MemberRecord, MemberState, Pane, PaneKind};

const KEY: &str = "kaaaaaaaa";
const KEY2: &str = "kbbbbbbbb";
const OLD_ZS: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn json_file(p: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

/// Polls `f` every 20 ms until it is true or `bound` is over (a test bound).
fn until(bound: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = std::time::Instant::now();
    loop {
        if f() {
            return true;
        }
        if t0.elapsed() >= bound {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Writes `body` to `path` with mode 0755, then `ready_exe(path)` (D14).
fn write_exe(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    ready_exe(path);
}

// ---- Harness (W§18 preamble) ----

#[test]
fn harness_new_at_a_missing_tmux_is_none() {
    assert!(Harness::new_at(Path::new("/nonexistent/zygo-m2/tmux")).is_none());
}

#[test]
fn harness_seed_writes_where_the_binary_reads() {
    let h = Harness::no_tmux();
    h.seed(WS_ID, &mut |_| {});
    // W§7.1: the binary with HOME = root reads `<root>/.local/state/zygo/workspaces/<id>/workspace.json`.
    assert!(h.root().join(".local/state/zygo/workspaces").join(WS_ID).join("workspace.json").is_file());
    assert_eq!(h.store(WS_ID).read().unwrap().unwrap().name, "app");
}

#[test]
fn harness_new_starts_a_private_server() {
    let Some(h) = Harness::new() else { return };
    h.tmux(&["new-session", "-d", "-s", "zygo-app", "cat"]);
    assert_eq!(
        h.tmux(&["display-message", "-p", "-t", "=zygo-app:", "#{socket_path}"]),
        h.socket().display().to_string()
    );
}

#[test]
fn harness_new_at_starts_the_server_with_that_tmux() {
    let Some(real) = zygo_workspace::mux::testing::require_tmux() else { return };
    let d = tempfile::tempdir().unwrap();
    let calls = d.path().join("calls");
    let rec = d.path().join("tmux");
    write_exe(&rec, &format!("#!/bin/sh\necho \"$@\" >> '{}'\nexec '{}' \"$@\"\n", calls.display(), real.display()));
    let h = Harness::new_at(&rec).unwrap();
    h.tmux(&["new-session", "-d", "-s", "zygo-app", "cat"]);
    assert!(std::fs::read_to_string(&calls).unwrap().lines().any(|l| l.contains("new-session")));
    assert_eq!(std::fs::read_link(h.bin.join("tmux")).unwrap(), rec);
}

// ---- `ws run` without the agent (W§8.2 steps 1, 8; AU15, AU46, AU48, AU61 missing cwd, AU64, AU84, AU88, AU93) ----

fn fresh(kind: Agent, args: &[&str]) -> MemberRecord {
    let mut m = MemberRecord::new(kind);
    m.launch.args = args.iter().map(|a| a.to_string()).collect();
    m
}

fn resumable(kind: Agent, id: &str) -> MemberRecord {
    let mut m = fresh(kind, &[]);
    m.state = MemberState::Resumable;
    m.agent_session = Some(id.into());
    m.zygo_session = Some(OLD_ZS.into());
    m
}

/// panes[] = one pane KEY in `cwd` (kind from `m`, else shell); members[KEY] = m.
fn one_pane(h: &Harness, cwd: &Path, m: Option<MemberRecord>) {
    let kind = match m.as_ref().map(|m| m.launch.kind) {
        Some(Agent::ClaudeCode) => PaneKind::ClaudeCode,
        Some(Agent::OpenCode) => PaneKind::OpenCode,
        None => PaneKind::Shell,
    };
    h.seed(WS_ID, &mut |w| {
        w.panes = vec![Pane { key: KEY.into(), cwd: cwd.to_path_buf(), kind, ..Default::default() }];
        w.members.clear();
        if let Some(m) = &m {
            w.members.insert(KEY.into(), m.clone());
        }
    });
}

fn member(h: &Harness) -> MemberRecord {
    h.store(WS_ID).read().unwrap().unwrap().members[KEY].clone()
}

/// `zygo ws run --ws-id WS_ID --pane KEY [--shell]`, spawned and read through `output_child` (D15).
fn ws_run(h: &Harness, extra: &[(&str, &OsStr)], shell: bool) -> std::process::Output {
    let mut args = vec!["ws", "run", "--ws-id", WS_ID, "--pane", KEY];
    if shell {
        args.push("--shell");
    }
    output_child(h.zygo(&args, extra).spawn().unwrap(), "ws run")
}

/// The same command without `--shell`, spawned.
fn spawn_ws_run(h: &Harness, extra: &[(&str, &OsStr)]) -> std::process::Child {
    h.zygo(&["ws", "run", "--ws-id", WS_ID, "--pane", KEY], extra).spawn().unwrap()
}

fn shell_file(h: &Harness, name: &str) -> String {
    std::fs::read_to_string(h.root().join("shell").join(name)).unwrap_or_default()
}

/// The `(&'static str, OsString)` list of `Harness::stub` as `(&str, &OsStr)` pairs.
fn pairs<'a>(v: &'a [(&'static str, OsString)]) -> Vec<(&'a str, &'a OsStr)> {
    v.iter().map(|(k, v)| (*k, v.as_os_str())).collect()
}

/// The `SigIgn` mask of a copy of `/proc/<pid>/status`.
#[cfg(target_os = "linux")]
fn sig_ignored(status: &Path) -> u64 {
    let s = std::fs::read_to_string(status).unwrap();
    let hex = s.lines().find_map(|l| l.strip_prefix("SigIgn:")).unwrap().trim();
    u64::from_str_radix(hex, 16).unwrap()
}

#[cfg(target_os = "linux")]
const FOUR: [libc::c_int; 4] = [libc::SIGHUP, libc::SIGTERM, libc::SIGINT, libc::SIGQUIT];

#[test]
fn au46_au93_a_shell_pane_execs_the_shell_in_its_cwd() {
    let h = Harness::no_tmux();
    let sub = h.repo().join("sub");
    std::fs::create_dir(&sub).unwrap();
    one_pane(&h, &sub, None);
    let x = OsStr::new("x");
    let o = ws_run(
        &h,
        &[("ZYGO_SESSION", x), ("CLAUDECODE", x), ("ZYGO_PANE", OsStr::new("kzzzzzzzz")), ("ANTHROPIC_BASE_URL", OsStr::new("http://x.invalid"))],
        true,
    );
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(shell_file(&h, "cwd").trim_end(), sub.to_str().unwrap()); // AU48
    let env = shell_file(&h, "env");
    assert!(env.lines().any(|l| l == format!("ZYGO_WORKSPACE={WS_ID}")), "{env}"); // AU93
    for name in ["ZYGO_PANE=", "ZYGO_SESSION=", "CLAUDECODE=", "ANTHROPIC_BASE_URL="] {
        assert!(!env.lines().any(|l| l.starts_with(name)), "{name} in {env}"); // AU64
    }
}

#[test]
fn au15_without_shell_ws_run_execs_bin_sh() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), None);
    let mut c = h.zygo(&["ws", "run", "--ws-id", WS_ID, "--pane", KEY, "--shell"], &[]);
    c.env_remove("SHELL").stdin(std::process::Stdio::piped());
    let mut child = c.spawn().unwrap();
    let out = h.root().join("sh-cwd");
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), format!("pwd > '{}'\n", out.display()).as_bytes()).unwrap();
    assert!(wait_child(&mut child, "ws run").success());
    assert_eq!(std::fs::read_to_string(&out).unwrap().trim_end(), h.repo().to_str().unwrap());
}

/// AU88, W§8.2 step 8: `ws run` starts here with SIGHUP, SIGTERM, SIGINT and SIGQUIT ignored (as under `nohup`);
/// the shell after `exec` has none of the four ignored. `execve` keeps an ignored disposition, so this fails when
/// step 23 does not set the default handling. `execve` also resets every caught handler, so `SigCgt` of the shell
/// shows only the shell's own handlers; the test reads `SigIgn` only (D13). `$SHELL` is the stub agent in its
/// `ZYGO_TEST_SIGDUMP` mode, which `exec_shell` runs with no argument: no shell interpreter runs in between, because
/// bash ignores SIGQUIT by itself (bash manual, SIGNALS).
#[cfg(target_os = "linux")]
#[test]
fn au88_the_shell_after_exec_has_no_ignored_signal() {
    use std::os::unix::process::CommandExt;
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), None);
    let stub = h.bin.join("claude");
    let (out, _) = h.stub(Agent::ClaudeCode, "shell");
    let dump = h.root().join("shell-sigign");
    let mut c = h.zygo(
        &["ws", "run", "--ws-id", WS_ID, "--pane", KEY, "--shell"],
        &[("SHELL", stub.as_os_str()), ("ZYGO_STUB_OUT", out.as_os_str()), ("ZYGO_TEST_SIGDUMP", dump.as_os_str())],
    );
    // SAFETY: `signal` is async-signal-safe; the closure runs in the child between fork and exec.
    unsafe {
        c.pre_exec(|| {
            for s in FOUR {
                libc::signal(s, libc::SIG_IGN);
            }
            Ok(())
        });
    }
    let o = output_child(c.spawn().unwrap(), "ws run");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(json_file(&out.join("argv.json")), json!([])); // the stub ran as the shell, with no argument
    let ign = sig_ignored(&dump);
    for s in FOUR {
        assert_eq!(ign >> (s - 1) & 1, 0, "signal {s} ignored in the shell: {ign:#x}");
    }
}

#[test]
fn w18_28_a_missing_cwd_opens_a_shell_in_the_main_worktree() {
    for (kind, id, cmd) in [(Agent::ClaudeCode, "abc", "zygo claude -- --resume abc"), (Agent::OpenCode, "ses_1", "zygo opencode -- --session=ses_1")] {
        let h = Harness::no_tmux();
        let gone = h.repo().join("gone");
        one_pane(&h, &gone, Some(resumable(kind, id)));
        let (out, env) = h.stub(kind, "a");
        let o = ws_run(&h, &pairs(&env), false);
        let g = gone.display();
        assert!(
            text(&o.stderr).contains(&format!("zygo: {g} is gone; agent session {id} not resumed; to resume it: cd {g} && {cmd}\n")),
            "{}",
            text(&o.stderr)
        );
        assert_eq!(shell_file(&h, "cwd").trim_end(), h.repo().to_str().unwrap());
        assert!(!out.join("argv.json").exists()); // no resume (AU61)
        assert_eq!(member(&h), resumable(kind, id));
    }
}

#[test]
fn w82_invalid_values_exit_1_and_a_missing_state_opens_a_shell() { // AU84, D10
    let h = Harness::no_tmux();
    let run = |id: &str, key: &str| output_child(h.zygo(&["ws", "run", "--ws-id", id, "--pane", key], &[]).spawn().unwrap(), "ws run");
    let o = run("XYZ", KEY);
    assert_eq!((o.status.code(), text(&o.stderr)), (Some(1), "zygo: invalid workspace id XYZ\n".to_string()));
    let o = run(WS_ID, "bad");
    assert_eq!((o.status.code(), text(&o.stderr)), (Some(1), "zygo: invalid pane key bad\n".to_string()));
    let o = run(WS_ID, KEY);
    assert_eq!((o.status.code(), text(&o.stderr)), (Some(0), format!("zygo: no workspace with id {WS_ID}\n")));
    assert_eq!(shell_file(&h, "cwd").trim_end(), h.root().to_str().unwrap());
}

// ---- The member launch and its exit (W§8.2 steps 2-7, W§8.3, W§9.4 inputs; W§18 #4, #6, #27, #28, #58, #72) ----

/// The control folder of a stub named `name`.
fn ctl(h: &Harness, name: &str) -> PathBuf {
    h.root().join("ctl").join(name)
}

/// The control stub at `ctl` has started (its `done` file starts with `start `).
fn started(ctl: &Path) -> bool {
    until(Duration::from_secs(20), || std::fs::read_to_string(ctl.join("done")).is_ok_and(|d| d.starts_with("start ")))
}

/// The environment the stub received.
fn stub_env(out: &Path) -> Value {
    json_file(&out.join("env.json"))
}

/// Writes `h.bin/opencode` as a script: the version branch, then `body`.
fn opencode_script(h: &Harness, body: &str) {
    let v = zygo_config::builtin::min_agent_version(Agent::OpenCode);
    write_exe(
        &h.bin.join("opencode"),
        &format!("#!/bin/sh\nPATH=/usr/bin:/bin\nif [ \"$1\" = --version ]; then echo '{v}'; exit 0; fi\n{body}\n"),
    );
}

#[test]
fn w18_4_state_holds_no_secret_and_no_prompt_text() {
    let h = Harness::no_tmux();
    std::fs::write(h.repo().join("prompt.txt"), "p").unwrap();
    let args = [
        "--append-system-prompt",
        "PROMPT-TEXT-W18-4",
        "--append-system-prompt-file",
        "prompt.txt",
        "--resume",
        "RESUME-X-W18-4",
        "POSITIONAL-W18-4",
    ];
    one_pane(&h, &h.repo(), Some(fresh(Agent::ClaudeCode, &args)));
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let mut extra = pairs(&env);
    extra.push(("ANTHROPIC_API_KEY", OsStr::new("provider-key-value-w18-4")));
    assert_eq!(ws_run(&h, &extra, false).status.code(), Some(0));
    let token = json_file(&out.join("session.json"))["token"].as_str().unwrap().trim().to_string();
    let file = h.store(WS_ID).dir().join("workspace.json");
    let raw = std::fs::read_to_string(&file).unwrap();
    for bad in [token.as_str(), "provider-key-value-w18-4", "zyg_", "PROMPT-TEXT-W18-4", "RESUME-X-W18-4", "POSITIONAL-W18-4"] {
        assert!(!raw.contains(bad), "{bad}");
    }
    assert_eq!(member(&h).launch.args, ["--append-system-prompt-file", h.repo().join("prompt.txt").to_str().unwrap()]);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(file.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
}

#[test]
fn w18_6_session_args_and_prompt_text_are_not_saved() {
    for (args, kept) in [(&["--resume", "abc", "fix it"][..], &[][..]), (&["-r", "--model", "lead"][..], &["--model", "lead"][..])] {
        let h = Harness::no_tmux();
        one_pane(&h, &h.repo(), Some(fresh(Agent::ClaudeCode, args)));
        let (out, env) = h.stub(Agent::ClaudeCode, "a");
        ws_run(&h, &pairs(&env), false);
        assert!(out.join("argv.json").exists());
        assert_eq!(member(&h).launch.args, kept, "{args:?}");
    }
}

#[test]
fn w18_72_ws_run_does_no_tmux_detection() { // AU102: bin/tmux fails every call
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::ClaudeCode, &[])));
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    assert_eq!(ws_run(&h, &pairs(&env), false).status.code(), Some(0));
    let e = stub_env(&out);
    assert_eq!((e["ZYGO_WORKSPACE"].as_str(), e["ZYGO_PANE"].as_str()), (Some(WS_ID), Some(KEY)));
    let view: Value = serde_json::from_str(json_file(&out.join("session.json"))["view.json"].as_str().unwrap()).unwrap();
    assert_eq!(view["workspace"]["member"], json!({"ws_id": WS_ID, "pane": KEY}));
}

#[test]
fn au59_a_settings_argument_keeps_membership() {
    let h = Harness::no_tmux();
    let s = h.root().join("user-settings.json");
    std::fs::write(&s, r#"{"env":{"W":"1"}}"#).unwrap();
    one_pane(&h, &h.repo(), Some(fresh(Agent::ClaudeCode, &["--settings", s.to_str().unwrap()])));
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let o = ws_run(&h, &pairs(&env), false);
    let e = stub_env(&out);
    assert_eq!((e["ZYGO_WORKSPACE"].as_str(), e["ZYGO_PANE"].as_str()), (Some(WS_ID), Some(KEY)));
    assert_eq!(member(&h).launch.args, ["--settings", s.to_str().unwrap()]); // AU30
    assert!(!text(&o.stderr).contains("workspace"), "{}", text(&o.stderr));
}

#[test]
fn w83_a_fresh_claude_member_gets_a_preassigned_id_and_opencode_none() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::ClaudeCode, &["--model", "lead"])));
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    ws_run(&h, &pairs(&env), false);
    let a = json_file(&out.join("session-arg.json"));
    let id = a["id"].as_str().unwrap().to_string();
    assert_eq!(a["flag"], "--session-id");
    assert!(regex::Regex::new("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$").unwrap().is_match(&id));
    let argv: Vec<String> = serde_json::from_value(json_file(&out.join("argv.json"))).unwrap();
    let at = |t: &str| argv.iter().position(|x| x == t).unwrap();
    assert!(at(&format!("--session-id={id}")) < at("--model"), "{argv:?}");
    let m = member(&h);
    assert_eq!((m.agent_session.as_deref(), m.pending.is_none(), m.state), (Some(id.as_str()), true, MemberState::Ended));
    assert_eq!(m.zygo_session.as_deref(), stub_env(&out)["ZYGO_SESSION"].as_str());
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::OpenCode, &[])));
    let (out, env) = h.stub(Agent::OpenCode, "o");
    ws_run(&h, &pairs(&env), false);
    assert!(out.join("argv.json").exists() && !out.join("session-arg.json").exists());
    assert_eq!(member(&h).agent_session, None);
}

#[test]
fn au16_the_results_link_points_into_the_workspace_cache() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::ClaudeCode, &[])));
    let (_, env) = h.stub(Agent::ClaudeCode, "a");
    ws_run(&h, &pairs(&env), false);
    let zs = member(&h).zygo_session.unwrap();
    let link = h.paths.sessions_dir.join(&zs).join("results");
    let target = h.root().join(".cache/zygo/workspaces").join(WS_ID).join("results").join(KEY);
    assert_eq!(std::fs::read_link(&link).unwrap(), target);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o700);
}

#[test]
fn au11_au32_saved_overrides_and_compaction_reach_the_session() {
    let h = Harness::no_tmux();
    let mut m = fresh(Agent::ClaudeCode, &[]);
    m.overrides = Some(json!({"redaction_content": false, "results": false, "future_key": 7}));
    m.launch.compact = Some("600000".into());
    m.compact = Some(zygo_workspace::state::CompactOverride::Set("300000".into()));
    one_pane(&h, &h.repo(), Some(m));
    let (out, mut env) = h.stub(Agent::ClaudeCode, "a");
    let c = ctl(&h, "a");
    env.push(("ZYGO_STUB_CONTROL", c.clone().into()));
    let mut child = spawn_ws_run(&h, &pairs(&env));
    assert!(started(&c));
    let dir = PathBuf::from(stub_env(&out)["ZYGO_SESSION_DIR"].as_str().unwrap());
    assert_eq!(json_file(&dir.join("overrides.json")), json!({"redaction_content": false, "results": false, "future_key": 7}));
    let client = zygo_proxy::client::SessionClient::from_session_dir(&dir).unwrap();
    assert_eq!(client.set_settings(&zygo_proxy::session::SessionSettings::default()).unwrap().redaction_content, Some(false));
    assert_eq!(client.status().unwrap().compact, Some(zygo_proxy::client::WindowArg::Tokens(300_000)));
    common::stub_control(&c, "exit 0").unwrap();
    assert!(wait_child(&mut child, "ws run").success());
}

#[test]
fn h1_a_resumed_view_carries_the_resume_id_and_a_fresh_one_none() {
    for (m, want) in [(resumable(Agent::ClaudeCode, "abc"), Some("abc")), (fresh(Agent::ClaudeCode, &[]), None)] {
        let h = Harness::no_tmux();
        let mut m = m;
        m.note_pending = true;
        one_pane(&h, &h.repo(), Some(m));
        let (out, env) = h.stub(Agent::ClaudeCode, "a");
        ws_run(&h, &pairs(&env), false);
        let view: Value = serde_json::from_str(json_file(&out.join("session.json"))["view.json"].as_str().unwrap()).unwrap();
        assert!(view["workspace"]["resume_note"].is_object());
        assert_eq!(view["workspace"]["resume_note"]["agent_session"].as_str(), want);
    }
}

#[test]
fn w18_28_a_saved_config_dir_and_the_agent_on_path_are_used_at_resume() {
    let h = Harness::no_tmux();
    let dir = h.root().join("cc");
    std::fs::create_dir(&dir).unwrap();
    let old = h.root().join("old");
    std::fs::create_dir(&old).unwrap();
    write_exe(&old.join("claude"), &format!("#!/bin/sh\n[ \"$1\" = --version ] && exit 0\ntouch '{}'\n", h.root().join("old-ran").display()));
    let mut m = resumable(Agent::ClaudeCode, "abc");
    m.launch.config_dir = Some(dir.to_str().unwrap().into());
    m.config_dir = Some(dir.clone());
    m.program = Some(old.join("claude"));
    one_pane(&h, &h.repo(), Some(m));
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let mut extra = pairs(&env);
    let other = h.root().join("other");
    extra.push(("CLAUDE_CONFIG_DIR", other.as_os_str()));
    ws_run(&h, &extra, false);
    assert_eq!(stub_env(&out)["CLAUDE_CONFIG_DIR"].as_str(), dir.to_str());
    assert!(json_file(&out.join("argv.json")).as_array().unwrap().iter().any(|a| a == "--resume=abc"));
    assert!(!h.root().join("old-ran").exists());
}

#[test]
fn pw16_au61_claude_config_dir_is_set_for_claude_members_only() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(resumable(Agent::ClaudeCode, "abc")));
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let mut extra = pairs(&env);
    let other = h.root().join("other");
    extra.push(("CLAUDE_CONFIG_DIR", other.as_os_str()));
    ws_run(&h, &extra, false);
    assert!(stub_env(&out).get("CLAUDE_CONFIG_DIR").is_none()); // PW16: saved none → none
    assert!(json_file(&out.join("argv.json")).as_array().unwrap().iter().any(|a| a == "--resume=abc"));
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(resumable(Agent::OpenCode, "ses_1")));
    let (out, env) = h.stub(Agent::OpenCode, "o");
    let mut extra = pairs(&env);
    extra.push(("CLAUDE_CONFIG_DIR", other.as_os_str()));
    ws_run(&h, &extra, false);
    assert_eq!(stub_env(&out)["CLAUDE_CONFIG_DIR"].as_str(), other.to_str()); // D8: an OpenCode member keeps it
    assert_eq!(member(&h).config_dir, None);
}

#[test]
fn au61_the_saved_program_runs_when_path_has_none() {
    for (kind, name, id) in [(Agent::ClaudeCode, "claude", "abc"), (Agent::OpenCode, "opencode", "ses_1")] {
        let h = Harness::no_tmux();
        let saved = h.root().join("saved");
        std::fs::create_dir(&saved).unwrap();
        std::fs::copy(common::stub_agent(), saved.join(name)).unwrap();
        ready_exe(&saved.join(name));
        std::fs::write(saved.join("other-tool"), "").unwrap();
        let mut m = resumable(kind, id);
        m.program = Some(saved.join(name));
        one_pane(&h, &h.repo(), Some(m));
        let (out, env) = h.stub(kind, "a");
        let empty = h.root().join("empty-path");
        std::fs::create_dir(&empty).unwrap();
        let mut extra = pairs(&env);
        extra.push(("PATH", empty.as_os_str()));
        ws_run(&h, &extra, false);
        assert!(out.join("argv.json").exists(), "{name}");
        assert_eq!(member(&h).program, Some(saved.join(name)), "{name}");
        let path = stub_env(&out)["PATH"].as_str().unwrap().to_string();
        assert!(!std::env::split_paths(&path).any(|d| d == saved), "{name}: {path}"); // D8: the saved folder is not on PATH
    }
}

#[test]
fn pw16_au12_a_failed_resume_runs_once_and_stays_resumable() {
    let h = Harness::no_tmux();
    let count = h.root().join("count");
    std::fs::create_dir(&count).unwrap();
    write_exe(
        &count.join("claude"),
        &format!(
            "#!/bin/sh\ncase \"$1\" in --version) ;; *) echo \"${{CLAUDE_CONFIG_DIR-none}}\" >> '{}';; esac\nexec '{}' \"$@\"\n",
            h.root().join("launches").display(),
            common::stub_agent().display()
        ),
    );
    one_pane(&h, &h.repo(), Some(resumable(Agent::ClaudeCode, "abc")));
    let (_, env) = h.stub(Agent::ClaudeCode, "a");
    let mut extra = pairs(&env);
    let other = h.root().join("other");
    extra.extend([("PATH", count.as_os_str()), ("CLAUDE_CONFIG_DIR", other.as_os_str()), ("ZYGO_STUB_EXIT", OsStr::new("1"))]);
    let o = ws_run(&h, &extra, false);
    assert_eq!(std::fs::read_to_string(h.root().join("launches")).unwrap(), "none\n");
    assert!(
        text(&o.stderr).contains("zygo: resume of claude session abc failed (exit 1); try: zygo claude -- --resume abc\n"),
        "{}",
        text(&o.stderr)
    );
    assert_eq!(member(&h).state, MemberState::Resumable);
    assert_eq!(shell_file(&h, "cwd").trim_end(), h.repo().to_str().unwrap()); // AU12: a shell
}

#[test]
fn w18_27_an_exit_before_the_callback_keeps_the_member() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(resumable(Agent::ClaudeCode, "abc")));
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let empty = h.root().join("empty-path");
    std::fs::create_dir(&empty).unwrap();
    let mut extra = pairs(&env);
    extra.push(("PATH", empty.as_os_str())); // exit 13: no agent, no saved program
    let o = ws_run(&h, &extra, false);
    assert!(text(&o.stderr).contains("zygo: the claude launch failed before the agent started (exit 13)\n"), "{}", text(&o.stderr));
    assert!(!text(&o.stderr).contains("try: zygo"));
    assert!(!out.join("argv.json").exists());
    assert_eq!(member(&h), resumable(Agent::ClaudeCode, "abc"));
}

#[test]
fn w18_27_the_first_session_start_commits_and_clears() {
    let h = Harness::no_tmux();
    let mut m = resumable(Agent::ClaudeCode, "abc");
    m.clean_stop = true;
    m.closed_units = vec!["u1".into()];
    m.note_pending = true;
    one_pane(&h, &h.repo(), Some(m));
    let (out, mut env) = h.stub(Agent::ClaudeCode, "a");
    let c = ctl(&h, "a");
    env.push(("ZYGO_STUB_CONTROL", c.clone().into()));
    let mut child = spawn_ws_run(&h, &pairs(&env));
    assert!(started(&c));
    let m = member(&h);
    assert_eq!(m.zygo_session.as_deref(), stub_env(&out)["ZYGO_SESSION"].as_str());
    assert_eq!(
        (m.pending.is_none(), m.state, m.clean_stop, m.closed_units.len(), m.note_pending),
        (true, MemberState::Running, false, 0, false)
    );
    common::stub_control(&c, "exit 0").unwrap();
    assert!(wait_child(&mut child, "ws run").success());
}

#[test]
fn au73_an_ended_member_with_a_non_zero_exit_prints_the_resume_command() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::ClaudeCode, &[])));
    let (_, env) = h.stub(Agent::ClaudeCode, "a");
    let mut extra = pairs(&env);
    extra.push(("ZYGO_STUB_EXIT", OsStr::new("3")));
    let o = ws_run(&h, &extra, false);
    let m = member(&h);
    let id = m.agent_session.clone().unwrap();
    assert_eq!(m.state, MemberState::Ended);
    assert!(
        text(&o.stderr).contains(&format!(
            "zygo: claude exited 3; agent session {id} is kept by the agent; to resume it: zygo claude -- --resume {id}\n"
        )),
        "{}",
        text(&o.stderr)
    );
}

#[test]
fn au17_sigterm_to_ws_run_keeps_the_member_resumable() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::OpenCode, &[])));
    let (_, mut env) = h.stub(Agent::OpenCode, "o");
    let c = ctl(&h, "o");
    env.push(("ZYGO_STUB_CONTROL", c.clone().into()));
    let mut child = spawn_ws_run(&h, &pairs(&env));
    assert!(started(&c));
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    assert!(wait_child(&mut child, "ws run").success()); // the shell after exec
    assert_eq!(member(&h).state, MemberState::Resumable);
}

#[test]
fn au17_an_agent_that_catches_sigterm_and_exits_with_a_code_stays_resumable() { // controller M3
    let h = Harness::no_tmux();
    opencode_script(&h, &format!("trap 'exit 3' TERM\ntouch '{}'\nwhile :; do sleep 1; done", h.root().join("agent-up").display()));
    one_pane(&h, &h.repo(), Some(fresh(Agent::OpenCode, &[])));
    let child = spawn_ws_run(&h, &[]);
    assert!(until(Duration::from_secs(20), || h.root().join("agent-up").exists()));
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let o = output_child(child, "ws run");
    assert_eq!(member(&h).state, MemberState::Resumable); // received, no signal, code 3
    assert!(!text(&o.stderr).contains("exited 3"), "{}", text(&o.stderr));
}

/// AU88 (B1): the wrapper ignores no signal, so the agent starts with none of SIGHUP, SIGTERM, SIGINT and SIGQUIT
/// ignored. This fails when the wrapper ignores SIGHUP or SIGTERM before `Launcher::run` (the refuted AU88 text):
/// the launcher then installs no handler for it and the agent inherits the ignore (`signals.rs:151-153`). `execve`
/// resets every caught handler, so `SigCgt` of the agent shows only the agent's own handlers; the test reads
/// `SigIgn` only (D13). The agent is the stub agent (`h.bin/opencode`) in its `ZYGO_TEST_SIGDUMP` mode, so no shell
/// interpreter runs between the spawn and the read (bash ignores SIGQUIT by itself: bash manual, SIGNALS). The
/// version probe of the launcher writes the same file first; the agent, which runs last, writes it again.
#[cfg(target_os = "linux")]
#[test]
fn au88_the_agent_starts_with_no_ignored_signal() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::OpenCode, &[])));
    let (out, mut env) = h.stub(Agent::OpenCode, "o");
    let dump = h.root().join("agent-sigign");
    env.push(("ZYGO_TEST_SIGDUMP", dump.clone().into()));
    let o = output_child(spawn_ws_run(&h, &pairs(&env)), "ws run");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(out.join("argv.json").exists()); // the agent ran
    let ign = sig_ignored(&dump);
    for s in FOUR {
        assert_eq!(ign >> (s - 1) & 1, 0, "signal {s} ignored in the agent: {ign:#x}");
    }
}

#[test]
fn w18_58_signals_while_the_agent_runs_do_not_stop_the_record_write() { // AU88 as rewritten (B1)
    use std::os::unix::process::ExitStatusExt;
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::OpenCode, &[])));
    let (out, mut env) = h.stub(Agent::OpenCode, "o");
    let c = ctl(&h, "o");
    env.extend([("ZYGO_STUB_CONTROL", c.clone().into()), ("M2_SHELL_HUP", "1".into())]);
    let mut child = spawn_ws_run(&h, &pairs(&env));
    assert!(started(&c));
    unsafe {
        libc::kill(child.id() as i32, libc::SIGHUP);
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let st = wait_child(&mut child, "ws run");
    let m = member(&h);
    assert_eq!(
        (m.pending.is_none(), m.zygo_session.as_deref(), m.state),
        (true, stub_env(&out)["ZYGO_SESSION"].as_str(), MemberState::Resumable)
    );
    assert_eq!(st.signal(), Some(libc::SIGHUP)); // the shell's own `kill -HUP $$`: default handling
    assert!(h.root().join("shell/cwd").exists() && !h.root().join("shell/alive").exists());
}

#[test]
fn h1_the_wrapper_commit_moves_pending_and_clears_nothing() { // H1 seam b, c; AU60 known limit
    let h = Harness::no_tmux();
    let mut m = resumable(Agent::OpenCode, "ses_OLD");
    m.closed_units = vec!["u1".into()];
    m.interrupted = vec![zygo_workspace::state::Subagent { agent_id: "a1".into(), agent_type: "t1".into(), ..Default::default() }];
    m.clean_stop = true;
    m.saved_for = Some("01JSAVEDAAAAAAAAAAAAAAAAAA".into());
    m.note_pending = true;
    one_pane(&h, &h.repo(), Some(m.clone()));
    let (out, env) = h.stub(Agent::OpenCode, "o");
    ws_run(&h, &pairs(&env), false);
    let a = member(&h);
    assert_eq!(a.zygo_session.as_deref(), stub_env(&out)["ZYGO_SESSION"].as_str());
    assert_eq!((a.pending.is_none(), a.agent_session.as_deref(), a.state), (true, Some("ses_OLD"), MemberState::Ended));
    assert_eq!(
        (a.closed_units, a.interrupted, a.clean_stop, a.saved_for, a.note_pending),
        (m.closed_units, m.interrupted, m.clean_stop, m.saved_for, m.note_pending)
    );
}

#[test]
fn h14_an_early_exit_keeps_the_id_that_the_hook_recorded() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::OpenCode, &[])));
    let (out, mut env) = h.stub(Agent::OpenCode, "o");
    let c = ctl(&h, "o");
    env.push(("ZYGO_STUB_CONTROL", c.clone().into()));
    let mut child = spawn_ws_run(&h, &pairs(&env));
    assert!(started(&c));
    let member_env: std::collections::BTreeMap<String, String> = serde_json::from_value(stub_env(&out)).unwrap();
    let cwd = h.repo().display().to_string();
    for stdin in [json!({"cwd": cwd}), json!({"sessionID": "ses_A", "cwd": cwd})] {
        let mut hook = std::process::Command::new(env!("CARGO_BIN_EXE_zygo"))
            .env_clear()
            .envs(&member_env)
            .args(["hook", "workspace", "--agent", "opencode", "--event", "session"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut hook.stdin.take().unwrap(), stdin.to_string().as_bytes()).unwrap();
        assert_eq!(output_child(hook, "zygo hook workspace").status.code(), Some(0));
    }
    common::stub_control(&c, "exit 0").unwrap();
    assert!(wait_child(&mut child, "ws run").success());
    let m = member(&h);
    assert_eq!((m.agent_session.as_deref(), m.pending.is_none()), (Some("ses_A"), true));
}

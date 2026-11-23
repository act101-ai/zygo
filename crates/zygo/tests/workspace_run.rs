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

//! M2: `zygo ws run`, the member wrapper and the workspace test harness (W§8.2, W§8.3, W§9.4 inputs, W§7.3, W§7.4,
//! W§18 preamble). Every test runs a workspace verb, so the file is `cfg(unix)` (OP5).
#![cfg(unix)]
mod common;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::workspace::{output_child, ready_exe, Guarded, Harness, WS_ID};
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
#[ignore = "timing: starts a private tmux server; 2 s tmux bound"]
fn harness_new_starts_a_private_server() {
    let Some(h) = Harness::new() else { return };
    h.tmux(&["new-session", "-d", "-s", "zygo-app", "cat"]);
    assert_eq!(
        h.tmux(&["display-message", "-p", "-t", "=zygo-app:", "#{socket_path}"]),
        h.socket().display().to_string()
    );
}

#[test]
#[ignore = "timing: starts a private tmux server; 2 s tmux bound"]
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

/// Whether the process `pid` is gone within 10 s (it was killed and its new parent reaped it).
fn gone_within(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    until(Duration::from_secs(10), || unsafe { libc::kill(pid, 0) } != 0)
}

/// Round 3 finding 8: `Guarded` sends SIGKILL to the process group before it reaps the leader, on a normal exit too,
/// so a process that the child left behind in its group is gone.
#[test]
#[ignore = "timing: 10 s poll for a shell process to end"]
fn guarded_kills_the_group_before_the_reap_on_every_path() {
    let d = tempfile::tempdir().unwrap();
    for wait in [true, false] {
        let pid_file = d.path().join(format!("bg-{wait}"));
        let script = format!("sleep 600 & echo $! > '{}'; {}", pid_file.display(), if wait { "exit 0" } else { "sleep 600" });
        let mut c = std::process::Command::new("/bin/sh");
        c.args(["-c", &script]).stdin(std::process::Stdio::null());
        let mut g = Guarded::spawn(c);
        assert!(until(Duration::from_secs(10), || std::fs::read_to_string(&pid_file).is_ok_and(|s| s.ends_with('\n'))));
        let bg: libc::pid_t = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        if wait {
            assert!(g.wait("sh").success()); // the normal path
        } else {
            drop(g); // the drop path
        }
        let ok = gone_within(bg);
        if !ok {
            // SAFETY: `bg` is the background child of this test's own shell.
            unsafe { libc::kill(bg, libc::SIGKILL) };
        }
        assert!(ok, "wait {wait}: the background process {bg} outlived the guard");
    }
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

/// `zygo ws run --ws-id WS_ID --pane KEY [--shell]`, spawned in a [`Guarded`] and read through its `output` (D15).
fn ws_run(h: &Harness, extra: &[(&str, &OsStr)], shell: bool) -> std::process::Output {
    let mut args = vec!["ws", "run", "--ws-id", WS_ID, "--pane", KEY];
    if shell {
        args.push("--shell");
    }
    Guarded::spawn(h.zygo(&args, extra)).output("ws run")
}

/// The same command without `--shell`, spawned in a [`Guarded`] (finding 6: a failed assert leaves no process).
fn spawn_ws_run(h: &Harness, extra: &[(&str, &OsStr)]) -> Guarded {
    Guarded::spawn(h.zygo(&["ws", "run", "--ws-id", WS_ID, "--pane", KEY], extra))
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
    let mut child = Guarded::spawn(c);
    let out = h.root().join("sh-cwd");
    std::io::Write::write_all(&mut child.stdin().unwrap(), format!("pwd > '{}'\n", out.display()).as_bytes()).unwrap();
    assert!(child.wait("ws run").success());
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
    let o = Guarded::spawn(c).output("ws run");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert_eq!(json_file(&out.join("argv.json")), json!([])); // the stub ran as the shell, with no argument
    let ign = sig_ignored(&dump);
    for s in FOUR {
        assert_eq!(ign >> (s - 1) & 1, 0, "signal {s} ignored in the shell: {ign:#x}");
    }
}

/// AU61; review round 2 finding 2: each path of the printed command is one single-quoted shell word (`'` written as
/// `'\''`), and a Claude Code member with a saved config dir gets `CLAUDE_CONFIG_DIR=<dir>` in front.
#[test]
fn w18_28_a_missing_cwd_opens_a_shell_in_the_main_worktree() {
    for (kind, id, config, name) in [
        (Agent::ClaudeCode, "abc", false, "it's gone"),
        (Agent::OpenCode, "ses_1", false, "it's gone"),
        (Agent::ClaudeCode, "abc", true, "it's gone"),
        (Agent::OpenCode, "ses_1", false, "a\nb"), // round 3 finding 3: a control byte gives `$'…'`
    ] {
        let h = Harness::no_tmux();
        let gone = h.repo().join(name);
        let mut m = resumable(kind, id);
        let dir = h.root().join("c c'd");
        if config {
            m.config_dir = Some(dir.clone());
        }
        one_pane(&h, &gone, Some(m.clone()));
        let (out, env) = h.stub(kind, "a");
        let o = ws_run(&h, &pairs(&env), false);
        let cmd = match (kind, config) {
            (Agent::OpenCode, _) => "zygo opencode -- --session='ses_1'".to_string(),
            (_, false) => "env -u CLAUDE_CONFIG_DIR zygo claude -- --resume='abc'".to_string(),
            (_, true) => format!("CLAUDE_CONFIG_DIR='{}/c c'\\''d' zygo claude -- --resume='abc'", h.root().display()),
        };
        let g = zygo_core::esc(&gone.display().to_string());
        let cd = if name == "it's gone" {
            format!("cd '{}/it'\\''s gone'", h.repo().display())
        } else {
            format!("cd $'{}/a\\x0ab'", h.repo().display())
        };
        assert!(
            text(&o.stderr).contains(&format!("zygo: {g} is gone; agent session {id} not resumed; to resume it: {cd} && {cmd}\n")),
            "{}",
            text(&o.stderr)
        );
        assert_eq!(shell_file(&h, "cwd").trim_end(), h.repo().to_str().unwrap());
        assert!(!out.join("argv.json").exists()); // no resume (AU61)
        assert_eq!(member(&h), m);
    }
}

#[test]
fn w82_a_state_that_cannot_be_read_opens_a_shell_in_the_current_folder() { // D10; review round 2 finding 8
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::ClaudeCode, &[])));
    let file = h.store(WS_ID).dir().join("workspace.json");
    std::fs::write(&file, "{ not json").unwrap();
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let o = ws_run(&h, &pairs(&env), false);
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    assert!(err.starts_with("zygo: ") && err.ends_with('\n') && err.lines().count() == 1, "{err}");
    // The binary (HOME = root) names the file through the state link of the harness.
    let shown = h.root().join(".local/state/zygo/workspaces").join(WS_ID).join("workspace.json");
    assert!(err.contains(shown.to_str().unwrap()), "{err}");
    assert_eq!(shell_file(&h, "cwd").trim_end(), h.root().to_str().unwrap()); // the current folder of `ws run`
    assert!(!out.join("argv.json").exists());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json"); // not changed
}

#[test]
fn w82_invalid_values_exit_1_and_a_missing_state_opens_a_shell() { // AU84, D10
    let h = Harness::no_tmux();
    let run = |id: &str, key: &str| Guarded::spawn(h.zygo(&["ws", "run", "--ws-id", id, "--pane", key], &[])).output("ws run");
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
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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

/// FIX-32 B1: by the yargs-parser rule `-mc` is `-m` with no value and `-c` (`--continue`), a session argument, so `ws
/// run` resumes no saved session on top of it (ws.rs `launch_member`: `has_session_arg` gives no resume).
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn fix32_b1_a_saved_opencode_cluster_with_continue_gets_no_session_argument() {
    let h = Harness::no_tmux();
    let mut m = resumable(Agent::OpenCode, "ses_1");
    m.launch.args = vec!["-mc".into()];
    one_pane(&h, &h.repo(), Some(m));
    let (out, env) = h.stub(Agent::OpenCode, "o");
    let o = ws_run(&h, &pairs(&env), false);
    let argv: Vec<String> = serde_json::from_value(json_file(&out.join("argv.json"))).unwrap_or_else(|e| panic!("{e}: {}", text(&o.stderr)));
    assert!(argv.iter().any(|t| t == "-mc"), "{argv:?}");
    assert!(!argv.iter().any(|t| t.starts_with("--session") || t == "-s" || t.contains("ses_1")), "{argv:?}");
}

/// The zygo session of a live launcher in the tests below (a valid session id).
const LIVE_ZS: &str = "01JBBBBBBBBBBBBBBBBBBBBBBB";

/// Makes the session folder `LIVE_ZS` live, as the member fixture does (`workspace_member.rs`
/// `au94_a_live_launcher_on_the_key_refuses`): `token`, `pid` and a lock on `pid` that lives as long as the result.
fn live_launcher(h: &Harness) -> std::fs::File {
    let d = h.paths.sessions_dir.join(LIVE_ZS);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("token"), "t").unwrap();
    std::fs::write(d.join("pid"), "1").unwrap();
    let lock = std::fs::File::options().read(true).write(true).open(d.join("pid")).unwrap();
    assert!(fs4::FileExt::try_lock(&lock).is_ok());
    lock
}

/// FIX-32 B3, W-C9: `ws run` does not resume a conversation whose launcher is live. The callback refuses in its locked
/// write, before `record_start`: the W-C9 line of the hand start, no agent, the record unchanged (`pending` included).
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn fix32_b3_ws_run_refuses_a_conversation_whose_launcher_is_live() {
    for committed in [true, false] {
        let h = Harness::no_tmux();
        let _lock = live_launcher(&h);
        let mut m = resumable(Agent::ClaudeCode, "abc");
        if committed {
            m.zygo_session = Some(LIVE_ZS.into());
        } else {
            // a start of LIVE_ZS that no hook committed yet
            m.pending = Some(zygo_workspace::state::Pending { zygo_session: LIVE_ZS.into(), started_at: 7, ..Default::default() });
        }
        one_pane(&h, &h.repo(), Some(m.clone()));
        let before = std::fs::read(h.store(WS_ID).dir().join("workspace.json")).unwrap();
        let (out, env) = h.stub(Agent::ClaudeCode, "a");
        let o = ws_run(&h, &pairs(&env), false);
        let err = text(&o.stderr);
        assert!(
            err.contains("zygo: pane kaaaaaaaa holds session abc, whose launcher is still running; zygo starts no second agent on it\n"),
            "{committed}: {err}"
        );
        assert!(err.contains("zygo: the claude launch failed before the agent started (exit 1)\n"), "{committed}: {err}");
        assert!(!out.join("argv.json").exists(), "{committed}: the agent started");
        assert_eq!(member(&h), m, "{committed}");
        assert_eq!(std::fs::read(h.store(WS_ID).dir().join("workspace.json")).unwrap(), before, "{committed}");
        // W§8.2 step 8: the pane ends in its shell
        assert_eq!(shell_file(&h, "cwd").trim_end(), h.repo().to_str().unwrap(), "{committed}");
    }
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
    assert!(child.wait("ws run").success());
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
        text(&o.stderr).contains("zygo: resume of claude session abc failed (exit 1); try: env -u CLAUDE_CONFIG_DIR zygo claude -- --resume='abc'\n"),
        "{}",
        text(&o.stderr)
    );
    assert_eq!(member(&h).state, MemberState::Resumable);
    assert_eq!(shell_file(&h, "cwd").trim_end(), h.repo().to_str().unwrap()); // AU12: a shell
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
    assert!(child.wait("ws run").success());
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
            "zygo: claude exited 3; agent session {id} is kept by the agent; to resume it: env -u CLAUDE_CONFIG_DIR zygo claude -- --resume='{id}'\n"
        )),
        "{}",
        text(&o.stderr)
    );
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn au61_the_exit_line_names_the_config_dir_quoted() { // review round 2 finding 2
    let h = Harness::no_tmux();
    let dir = h.root().join("c c'd");
    std::fs::create_dir(&dir).unwrap();
    let mut m = fresh(Agent::ClaudeCode, &[]);
    m.launch.config_dir = Some(dir.to_str().unwrap().into());
    one_pane(&h, &h.repo(), Some(m));
    let (_, env) = h.stub(Agent::ClaudeCode, "a");
    let mut extra = pairs(&env);
    extra.push(("ZYGO_STUB_EXIT", OsStr::new("3")));
    let o = ws_run(&h, &extra, false);
    let id = member(&h).agent_session.unwrap();
    let q = format!("'{}/c c'\\''d'", h.root().display());
    assert!(
        text(&o.stderr).contains(&format!(
            "zygo: claude exited 3; agent session {id} is kept by the agent; to resume it: CLAUDE_CONFIG_DIR={q} zygo claude -- --resume='{id}'\n"
        )),
        "{}",
        text(&o.stderr)
    );
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn au17_sigterm_to_ws_run_keeps_the_member_resumable() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::OpenCode, &[])));
    let (_, mut env) = h.stub(Agent::OpenCode, "o");
    let c = ctl(&h, "o");
    env.push(("ZYGO_STUB_CONTROL", c.clone().into()));
    let mut child = spawn_ws_run(&h, &pairs(&env));
    assert!(started(&c));
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    assert!(child.wait("ws run").success()); // the shell after exec
    assert_eq!(member(&h).state, MemberState::Resumable);
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn au17_an_agent_that_catches_sigterm_and_exits_with_a_code_stays_resumable() { // controller M3
    let h = Harness::no_tmux();
    opencode_script(&h, &format!("trap 'exit 3' TERM\ntouch '{}'\nwhile :; do sleep 1; done", h.root().join("agent-up").display()));
    one_pane(&h, &h.repo(), Some(fresh(Agent::OpenCode, &[])));
    let child = spawn_ws_run(&h, &[]);
    assert!(until(Duration::from_secs(20), || h.root().join("agent-up").exists()));
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let o = child.output("ws run");
    assert_eq!(member(&h).state, MemberState::Resumable); // received, no signal, code 3
    assert!(!text(&o.stderr).contains("exited 3"), "{}", text(&o.stderr));
}

/// AU88 (B1): the wrapper ignores no signal, so the agent starts with none of SIGHUP, SIGTERM, SIGINT and SIGQUIT
/// ignored. This fails when the wrapper ignores SIGHUP or SIGTERM before `Launcher::run` (the refuted AU88 text):
/// the launcher then installs no handler for it and the agent inherits the ignore (`signals.rs:151-153`). `execve`
/// resets every caught handler, so `SigCgt` of the agent shows only the agent's own handlers; the test reads
/// `SigIgn` only (D13). The agent is the stub agent (`h.bin/opencode`) in its `ZYGO_TEST_SIGDUMP` mode, so no shell
/// interpreter runs between the spawn and the read (bash ignores SIGQUIT by itself: bash manual, SIGNALS). The
/// version probe of the launcher writes the same file first; the agent, which runs last, writes it again. `ws run`
/// starts with the default handling of the four signals (review round 2 finding 5: set in `pre_exec`), so the
/// result does not depend on what the test runner ignores.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn au88_the_agent_starts_with_no_ignored_signal() {
    use std::os::unix::process::CommandExt;
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::OpenCode, &[])));
    let (out, mut env) = h.stub(Agent::OpenCode, "o");
    let dump = h.root().join("agent-sigign");
    env.push(("ZYGO_TEST_SIGDUMP", dump.clone().into()));
    let mut c = h.zygo(&["ws", "run", "--ws-id", WS_ID, "--pane", KEY], &pairs(&env));
    // SAFETY: `signal` is async-signal-safe; the closure runs in the child between fork and exec.
    unsafe {
        c.pre_exec(|| {
            for s in FOUR {
                libc::signal(s, libc::SIG_DFL);
            }
            Ok(())
        });
    }
    let o = Guarded::spawn(c).output("ws run");
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(out.join("argv.json").exists()); // the agent ran
    let ign = sig_ignored(&dump);
    for s in FOUR {
        assert_eq!(ign >> (s - 1) & 1, 0, "signal {s} ignored in the agent: {ign:#x}");
    }
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
    let st = child.wait("ws run");
    let m = member(&h);
    assert_eq!(
        (m.pending.is_none(), m.zygo_session.as_deref(), m.state),
        (true, stub_env(&out)["ZYGO_SESSION"].as_str(), MemberState::Resumable)
    );
    assert_eq!(st.signal(), Some(libc::SIGHUP)); // the shell's own `kill -HUP $$`: default handling
    assert!(h.root().join("shell/cwd").exists() && !h.root().join("shell/alive").exists());
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
#[ignore = "timing: the agent launch has 2 s launcher probes"]
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
    assert!(child.wait("ws run").success());
    let m = member(&h);
    assert_eq!((m.agent_session.as_deref(), m.pending.is_none()), (Some("ses_A"), true));
}

// ---- Test gate, wrapper snapshots, forget races, exit-write failure (W§7.3, W§7.4; W§18 #27 exit 12, #73) ----

/// A gate hold (D9): `<gate>/<point>.hold` exists while this value lives; `Drop` removes it, so a failed test releases the gate.
struct Hold(PathBuf);

impl Hold {
    fn new(gate: &Path, point: &str) -> Hold {
        std::fs::create_dir_all(gate).unwrap();
        let p = gate.join(format!("{point}.hold"));
        std::fs::write(&p, "").unwrap();
        Hold(p)
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A team server on 127.0.0.1:0 with one token, as `tests/serve_cli.rs:57-87` builds it; returns its folder, handle and token.
fn team_server() -> (tempfile::TempDir, zygo_proxy::serve::ServeHandle, String) {
    use std::sync::Arc;
    let d = tempfile::tempdir().unwrap();
    let root = d.path().to_path_buf();
    let (token, sha) = zygo_proxy::team::new_token();
    let config_file = root.join("config.toml");
    std::fs::write(&config_file, format!("[serve]\ntokens = [{{ label = \"m2\", sha256 = \"{sha}\" }}]\n")).unwrap();
    let paths = zygo_core::paths::Paths {
        config_file: config_file.clone(),
        cache_dir: root.join("cache"),
        data_dir: root.join("data"),
        sessions_dir: root.join("cache/sessions"),
        marketplaces_dir: root.join("cache/marketplaces"),
        plugin_store: root.join("data/plugins"),
        approvals_file: root.join("data/approvals.toml"),
        usage_db: root.join("data/usage.db"),
    };
    let ctx = zygo_config::LoadContext { paths, repo: None, team: None };
    let mut builtin = toml::Table::try_from(zygo_config::builtin::defaults()).unwrap();
    builtin.remove("marketplace");
    let h = zygo_proxy::serve::start(zygo_proxy::serve::ServeOpts {
        listen: "127.0.0.1:0".into(),
        tls: None,
        config_file,
        load: Arc::new(move || Ok(zygo_config::load_with_builtin(&ctx, builtin.clone())?)),
        report: Arc::new(|_: &str| {}),
        pid_file: None,
        signals: false,
        start: zygo_proxy::server::StartOpts { usage_db: Some(root.join("data/usage.db")), ..Default::default() },
    })
    .unwrap();
    (d, h, token)
}

fn seq(h: &Harness) -> u64 {
    h.store(WS_ID).read().unwrap().unwrap().snapshot_seq
}

fn results_dir(h: &Harness) -> PathBuf {
    h.root().join(".cache/zygo/workspaces").join(WS_ID).join("results").join(KEY)
}

/// `<workspace cache>/<WS_ID>/program/KEY` (AU61, D8).
fn program_dir(h: &Harness) -> PathBuf {
    h.root().join(".cache/zygo/workspaces").join(WS_ID).join("program").join(KEY)
}

/// An OpenCode member that resumes `ses_1` with no `opencode` on `PATH` and the stub agent as its saved program; returns
/// the saved path and the empty `PATH` folder (AU61, D8).
fn opencode_saved_program(h: &Harness) -> (PathBuf, PathBuf) {
    let saved = h.root().join("saved");
    std::fs::create_dir_all(&saved).unwrap();
    std::fs::copy(common::stub_agent(), saved.join("opencode")).unwrap();
    ready_exe(&saved.join("opencode"));
    let mut m = resumable(Agent::OpenCode, "ses_1");
    m.program = Some(saved.join("opencode"));
    one_pane(h, &h.repo(), Some(m));
    h.seed(WS_ID, &mut |w| w.tmux_socket = None);
    let empty = h.root().join("empty-path");
    std::fs::create_dir_all(&empty).unwrap();
    (saved.join("opencode"), empty)
}

/// The renames onto one name in a folder (Linux inotify): every state write ends in one rename of a new temporary
/// file onto `workspace.json` (`write_atomic`), so the count of `IN_MOVED_TO` events of that name is the number of
/// writes (review round 2 finding 7). The kernel merges an event into an identical last event in the queue, and
/// that check does not compare the rename cookie (`event_compare`, `fs/notify/inotify/inotify_fsnotify.c`), so two
/// renames onto one name would read as one. The watch therefore also takes `IN_MOVED_FROM`: its name is the
/// temporary file, a new random name for each write, so two `IN_MOVED_TO` events are never next to each other.
#[cfg(target_os = "linux")]
struct Renames(libc::c_int);

#[cfg(target_os = "linux")]
impl Renames {
    fn watch(dir: &Path) -> Renames {
        use std::os::unix::ffi::OsStrExt;
        // SAFETY: plain syscalls on a new descriptor that this value owns; the path is a NUL-terminated copy.
        unsafe {
            let fd = libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC);
            assert!(fd >= 0, "inotify_init1: {}", std::io::Error::last_os_error());
            let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).unwrap();
            let mask = libc::IN_MOVED_FROM | libc::IN_MOVED_TO;
            assert!(libc::inotify_add_watch(fd, c.as_ptr(), mask) >= 0, "{}", std::io::Error::last_os_error());
            Renames(fd)
        }
    }

    /// The renames onto `name` queued since the last call.
    fn count(&self, name: &str) -> usize {
        let mut n = 0;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            // SAFETY: `buf` is valid for its length; the descriptor is open.
            let got = unsafe { libc::read(self.0, buf.as_mut_ptr().cast(), buf.len()) };
            if got <= 0 {
                break;
            }
            let mut at = 0;
            let head = std::mem::size_of::<libc::inotify_event>();
            while at + head <= got as usize {
                // `struct inotify_event`: wd, mask, cookie, len (4 bytes each), then `len` bytes of NUL-padded name.
                let mask = u32::from_ne_bytes(buf[at + 4..at + 8].try_into().unwrap());
                let len = u32::from_ne_bytes(buf[at + 12..at + 16].try_into().unwrap()) as usize;
                let raw = &buf[at + head..at + head + len];
                let end = raw.iter().position(|b| *b == 0).unwrap_or(len);
                if mask & libc::IN_MOVED_TO != 0 && &raw[..end] == name.as_bytes() {
                    n += 1;
                }
                at += head + len;
            }
        }
        n
    }
}

#[cfg(target_os = "linux")]
impl Drop for Renames {
    fn drop(&mut self) {
        // SAFETY: the descriptor is owned by this value.
        unsafe {
            libc::close(self.0);
        }
    }
}

/// A workspace verb with an empty `PATH` (no tmux found; the seed has no `tmux_socket`), read through `output_child`.
fn verb(h: &Harness, args: &[&str]) -> std::process::Output {
    let empty = h.root().join("empty-path");
    std::fs::create_dir_all(&empty).unwrap();
    output_child(h.zygo(args, &[("PATH", empty.as_os_str())]).spawn().unwrap(), args[1])
}

/// Starts `ws run` for a fresh OpenCode member with the gate held at `point`; returns after the wrapper reached it.
fn at_gate(h: &Harness, point: &str) -> (Guarded, Hold, PathBuf) {
    one_pane(h, &h.repo(), Some(fresh(Agent::OpenCode, &[])));
    h.seed(WS_ID, &mut |w| w.tmux_socket = None);
    let gate = h.root().join("gate");
    let hold = Hold::new(&gate, point);
    let (out, mut env) = h.stub(Agent::OpenCode, "o");
    env.push(("ZYGO_TEST_MEMBER_GATE", gate.clone().into()));
    let child = spawn_ws_run(h, &pairs(&env));
    assert!(until(Duration::from_secs(20), || gate.join(point).exists()));
    (child, hold, out)
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn w18_73_launch_and_pending_are_one_write_and_program_is_refreshed() {
    let h = Harness::no_tmux();
    let dir = h.root().join("cc");
    std::fs::create_dir(&dir).unwrap();
    let mut m = fresh(Agent::ClaudeCode, &["--agents", r#"{"r":{"description":"d","prompt":"p"}}"#, "--model", "lead"]);
    m.launch.config_dir = Some(dir.to_str().unwrap().into());
    one_pane(&h, &h.repo(), Some(m));
    let gate = h.root().join("gate");
    let hold = Hold::new(&gate, "callback");
    let (_, mut env) = h.stub(Agent::ClaudeCode, "a");
    env.push(("ZYGO_TEST_MEMBER_GATE", gate.clone().into()));
    #[cfg(target_os = "linux")]
    let renames = Renames::watch(h.store(WS_ID).dir());
    let mut child = spawn_ws_run(&h, &pairs(&env));
    assert!(until(Duration::from_secs(20), || gate.join("callback").exists()));
    // Up to the callback gate the only state writer is the callback's start write: one rename, so `launch` and
    // `pending` are in one write (AU92). The stub writes no hook (not in control mode); no `snapshot.pending`.
    #[cfg(target_os = "linux")]
    assert_eq!(renames.count("workspace.json"), 1);
    let a = member(&h);
    assert!(a.pending.as_ref().unwrap().agent_session.is_some());
    assert_eq!(
        (a.launch.args.clone(), a.dropped_flags.clone()),
        (vec!["--model".to_string(), "lead".to_string()], vec!["--agents".to_string()])
    );
    assert_eq!((a.config_dir.clone(), a.program.clone()), (Some(dir.clone()), Some(h.bin.join("claude"))));
    drop(hold);
    assert!(child.wait("ws run").success());
    let bin2 = h.root().join("bin2");
    std::fs::create_dir(&bin2).unwrap();
    std::fs::copy(common::stub_agent(), bin2.join("claude")).unwrap();
    ready_exe(&bin2.join("claude"));
    let (_, env) = h.stub(Agent::ClaudeCode, "b");
    let mut extra = pairs(&env);
    extra.push(("PATH", bin2.as_os_str()));
    ws_run(&h, &extra, false);
    assert_eq!(member(&h).program, Some(bin2.join("claude")));
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn w18_27_an_exit_after_the_callback_removes_only_pending() { // exit 12: the --compact call finds no server
    let h = Harness::no_tmux();
    let (_d, srv, token) = team_server();
    let mut m = resumable(Agent::ClaudeCode, "abc");
    m.launch.server = Some(srv.url().to_string());
    m.launch.compact = Some("600000".into());
    one_pane(&h, &h.repo(), Some(m));
    let gate = h.root().join("gate");
    let hold = Hold::new(&gate, "callback");
    let (out, mut env) = h.stub(Agent::ClaudeCode, "a");
    env.extend([("ZYGO_TEST_MEMBER_GATE", gate.clone().into()), ("ZYGO_TEAM_TOKEN", token.into())]);
    // Serve mode takes the team token from `[server] token` of the user config (`serve_client.rs:309-313`).
    h.config("[server]\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n");
    let child = spawn_ws_run(&h, &pairs(&env));
    assert!(until(Duration::from_secs(20), || gate.join("callback").exists()));
    assert!(member(&h).pending.is_some());
    srv.shutdown();
    drop(hold);
    let o = child.output("ws run");
    assert!(text(&o.stderr).contains("zygo: the claude launch failed before the agent started (exit 12)\n"), "{}", text(&o.stderr));
    assert!(!out.join("argv.json").exists());
    let a = member(&h);
    assert_eq!(
        (a.zygo_session.as_deref(), a.agent_session.as_deref(), a.state, a.pending.is_none()),
        (Some(OLD_ZS), Some("abc"), MemberState::Resumable, true)
    );
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn m2_a_slow_setup_then_a_fast_exit_is_still_a_failed_fast_resume() { // controller M2: "ran" counts from the spawn
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(resumable(Agent::ClaudeCode, "abc")));
    let gate = h.root().join("gate");
    let hold = Hold::new(&gate, "callback");
    let (_, mut env) = h.stub(Agent::ClaudeCode, "a");
    env.extend([("ZYGO_TEST_MEMBER_GATE", gate.clone().into()), ("ZYGO_STUB_EXIT", "1".into())]);
    let child = spawn_ws_run(&h, &pairs(&env));
    assert!(until(Duration::from_secs(20), || gate.join("callback").exists()));
    std::thread::sleep(Duration::from_secs(11)); // the setup before the spawn takes more than 10 s
    drop(hold);
    let o = child.output("ws run");
    assert!(
        text(&o.stderr).contains("zygo: resume of claude session abc failed (exit 1); try: env -u CLAUDE_CONFIG_DIR zygo claude -- --resume='abc'\n"),
        "{}",
        text(&o.stderr)
    );
    assert_eq!(member(&h).state, MemberState::Resumable);
}

/// Starts `ws run` with the gate held at `point`, for a fresh OpenCode member (`opencode` on `PATH`) or, with `saved`,
/// for an OpenCode member that resumes with its saved program and no `opencode` on `PATH`, so `program/<key>` is made
/// before the gate (round 3 finding 5). Returns after the wrapper reached the gate.
fn race_at(h: &Harness, point: &str, saved: bool) -> (Guarded, Hold, PathBuf) {
    if !saved {
        return at_gate(h, point);
    }
    let (_, empty) = opencode_saved_program(h);
    let gate = h.root().join("gate");
    let hold = Hold::new(&gate, point);
    let (out, mut env) = h.stub(Agent::OpenCode, "o");
    env.extend([("ZYGO_TEST_MEMBER_GATE", gate.clone().into()), ("PATH", empty.into())]);
    let child = spawn_ws_run(h, &pairs(&env));
    assert!(until(Duration::from_secs(20), || gate.join(point).exists()));
    (child, hold, out)
}

/// The line of a failed start write (W§8.2 step 4); never printed when the state or the record is gone (round 3
/// finding 6).
const NOT_RECORDED: &str = "zygo: could not record the launch of pane";

/// Second review finding 1; round 3 findings 5 and 6: a `ws forget` at the `launch` gate. A fresh member's launch goes
/// on and the callback makes nothing; a resumed member's program folder was made and is gone with the cache, so the
/// launch finds no program (exit 13).
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn race_ws_forget_before_the_callback_makes_nothing() {
    for saved in [false, true] {
        let h = Harness::no_tmux();
        let (child, hold, out) = race_at(&h, "launch", saved);
        assert_eq!(program_dir(&h).join("opencode").exists(), saved);
        let o = verb(&h, &["ws", "forget", "app"]);
        assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
        drop(hold);
        let o = child.output("ws run");
        let err = text(&o.stderr);
        assert!(o.status.success(), "{err}");
        assert!(std::fs::symlink_metadata(program_dir(&h)).is_err(), "saved {saved}");
        assert_eq!(out.join("argv.json").exists(), !saved, "{err}"); // the launch went on, or found no program
        if saved {
            assert!(err.contains("zygo: the opencode launch failed before the agent started (exit 13)\n"), "{err}");
        }
        assert!(!err.contains(NOT_RECORDED), "{err}"); // Ok(None): no line
        assert!(!h.store(WS_ID).dir().exists());
        assert!(!results_dir(&h).exists());
    }
}

/// Second review finding 1; round 2 finding 4; round 3 findings 5 and 6: a `ws forget-pane` at the `launch` gate, as
/// above; the program folder goes with the record.
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn race_ws_forget_pane_before_the_callback_makes_no_record_and_no_results() {
    for saved in [false, true] {
        let h = Harness::no_tmux();
        let (child, hold, out) = race_at(&h, "launch", saved);
        assert_eq!(program_dir(&h).join("opencode").exists(), saved);
        let o = verb(&h, &["ws", "forget-pane", KEY, "--name", "app"]);
        assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
        drop(hold);
        let o = child.output("ws run");
        let err = text(&o.stderr);
        assert!(o.status.success(), "{err}");
        assert!(std::fs::symlink_metadata(program_dir(&h)).is_err(), "saved {saved}");
        assert_eq!(out.join("argv.json").exists(), !saved, "{err}");
        if saved {
            assert!(err.contains("zygo: the opencode launch failed before the agent started (exit 13)\n"), "{err}");
        }
        assert!(!err.contains(NOT_RECORDED), "{err}"); // Refused (no record): no line
        assert!(!h.store(WS_ID).read().unwrap().unwrap().members.contains_key(KEY));
        assert!(!results_dir(&h).exists());
    }
}

/// Round 3 finding 4: a forget between the `ws run` read and the program folder. `ws run` reads the state again after
/// it made the folder; the state or the record is gone, so it deletes the folder it made, prints the line of that
/// case and opens the shell, with no launch.
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn race_a_forget_before_the_program_folder_leaves_no_folder() {
    for (verb_args, line) in [
        (&["ws", "forget-pane", KEY, "--name", "app"][..], format!("zygo: pane {KEY} has no member record; started a shell\n")),
        (&["ws", "forget", "app"][..], format!("zygo: no workspace with id {WS_ID}\n")),
    ] {
        let h = Harness::no_tmux();
        let (child, hold, out) = race_at(&h, "program", true);
        assert!(std::fs::symlink_metadata(program_dir(&h)).is_err()); // not made yet
        let o = verb(&h, verb_args);
        assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
        drop(hold);
        let o = child.output("ws run");
        let err = text(&o.stderr);
        assert!(o.status.success(), "{err}");
        assert!(err.contains(&line), "{verb_args:?}: {err}");
        assert!(!out.join("argv.json").exists(), "{err}");
        assert!(std::fs::symlink_metadata(program_dir(&h)).is_err(), "{verb_args:?}");
        if verb_args[1] == "forget" {
            let cache = h.root().join(".cache/zygo/workspaces").join(WS_ID);
            assert!(std::fs::symlink_metadata(&cache).is_err(), "{verb_args:?}: the parts made for the folder go too");
        }
        assert_eq!(shell_file(&h, "cwd").trim_end(), h.repo().to_str().unwrap());
    }
}

/// Round 3 finding 6: a start write that finds a state that cannot be read (corrupted after the `ws run` read) prints
/// the line of W§8.2 step 4, and the agent starts.
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn f6_a_start_write_on_an_invalid_state_prints_one_line_and_the_agent_starts() {
    let h = Harness::no_tmux();
    let (child, hold, out) = at_gate(&h, "launch");
    std::fs::write(h.store(WS_ID).dir().join("workspace.json"), "{ not json").unwrap();
    drop(hold);
    let o = child.output("ws run");
    let err = text(&o.stderr);
    let head = format!("{NOT_RECORDED} {KEY}: ");
    let line = err.lines().find(|l| l.starts_with(&head)).unwrap_or_else(|| panic!("{err}"));
    assert!(line.ends_with("; zygo cannot see this agent as live") && line.contains("workspace.json"), "{err}");
    assert!(out.join("argv.json").exists()); // the agent started
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn minor2_the_exit_write_after_ws_forget_makes_no_state() {
    let h = Harness::no_tmux();
    let (child, hold, _) = at_gate(&h, "returned");
    let o = verb(&h, &["ws", "forget", "app"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(!h.store(WS_ID).dir().exists());
    drop(hold);
    assert!(child.output("ws run").status.success());
    assert!(!h.store(WS_ID).dir().exists());
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn minor2_the_exit_write_after_ws_forget_pane_makes_no_record() {
    let h = Harness::no_tmux();
    let (child, hold, _) = at_gate(&h, "returned");
    let o = verb(&h, &["ws", "forget-pane", KEY, "--name", "app"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    drop(hold);
    assert!(child.output("ws run").status.success());
    assert!(!h.store(WS_ID).read().unwrap().unwrap().members.contains_key(KEY));
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn d5_an_exit_write_that_cannot_take_the_lock_prints_one_line() { // second review finding 8
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::ClaudeCode, &[])));
    let gate = h.root().join("gate");
    let hold = Hold::new(&gate, "returned");
    let (_, mut env) = h.stub(Agent::ClaudeCode, "a");
    env.extend([("ZYGO_TEST_MEMBER_GATE", gate.clone().into()), ("ZYGO_STUB_EXIT", "3".into())]);
    let child = spawn_ws_run(&h, &pairs(&env));
    assert!(until(Duration::from_secs(20), || gate.join("returned").exists()));
    let lock = std::fs::File::options().read(true).write(true).open(h.store(WS_ID).dir().join("lock")).unwrap();
    assert!(fs4::FileExt::try_lock(&lock).is_ok());
    drop(hold);
    let o = child.output("ws run");
    drop(lock);
    let err = text(&o.stderr);
    assert!(err.contains(&format!("zygo: could not save the exit record of pane {KEY}: workspace state is locked\n")), "{err}");
    assert!(!err.contains("exited 3"), "{err}"); // D7: no record, so no resume line
    assert!(member(&h).pending.is_some()); // the stub sent no SessionStart; nothing committed
    assert_eq!(shell_file(&h, "cwd").trim_end(), h.repo().to_str().unwrap());
}

/// Review round 2 finding 1: a start write that cannot take `lock` within its bound leaves a live agent that no live
/// check sees; the callback says so on the pane's stderr, and the launch goes on (C6).
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn f1_a_start_write_that_cannot_take_the_lock_prints_one_line_and_the_agent_starts() {
    let h = Harness::no_tmux();
    let (child, hold, out) = at_gate(&h, "launch");
    let lock = std::fs::File::options().read(true).write(true).open(h.store(WS_ID).dir().join("lock")).unwrap();
    assert!(fs4::FileExt::try_lock(&lock).is_ok());
    drop(hold);
    let o = child.output("ws run");
    drop(lock);
    let err = text(&o.stderr);
    assert!(
        err.contains(&format!(
            "zygo: could not record the launch of pane {KEY}: workspace state is locked; zygo cannot see this agent as live\n"
        )),
        "{err}"
    );
    assert!(out.join("argv.json").exists()); // the agent started
    let m = member(&h);
    assert_eq!((m.pending.is_none(), m.zygo_session), (true, None)); // nothing written
}

/// Review round 2 finding 3: a link at `results/<key>` is not followed; nothing is made through it, the error goes to
/// the session log (C6) and the launch goes on.
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn f3_a_link_at_the_results_entry_is_refused() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::ClaudeCode, &[])));
    let outside = h.root().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::create_dir_all(results_dir(&h).parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, results_dir(&h)).unwrap();
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    ws_run(&h, &pairs(&env), false);
    assert!(out.join("argv.json").exists());
    let zs = member(&h).zygo_session.unwrap();
    assert!(!h.paths.sessions_dir.join(&zs).exists()); // no `results` link kept the session folder
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    assert_eq!(std::fs::read_link(results_dir(&h)).unwrap(), outside);
}

/// Review round 2 finding 3: a link at `program/<key>` is not followed, so the file `opencode` of its target stays.
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn f3_a_link_at_the_program_entry_leaves_its_target_alone() {
    let h = Harness::no_tmux();
    let (_, empty) = opencode_saved_program(&h);
    let outside = h.root().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("opencode"), "keep").unwrap();
    std::fs::create_dir_all(program_dir(&h).parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&outside, program_dir(&h)).unwrap();
    let (out, env) = h.stub(Agent::OpenCode, "o");
    let mut extra = pairs(&env);
    extra.push(("PATH", empty.as_os_str()));
    let o = ws_run(&h, &extra, false);
    let err = text(&o.stderr);
    assert!(err.contains(&format!("zygo: {} is a link; it is not followed\n", program_dir(&h).display())), "{err}");
    assert_eq!(std::fs::read_to_string(outside.join("opencode")).unwrap(), "keep");
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    assert!(err.contains("zygo: the opencode launch failed before the agent started (exit 13)\n"), "{err}"); // no program found
    assert!(!out.join("argv.json").exists());
}

/// AU61, D8; review round 2 finding 8: at an OpenCode resume with no `opencode` on `PATH`, the last `PATH` folder of
/// the agent is the program folder, and it holds only the link `opencode` to the saved program.
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn au61_the_opencode_program_folder_holds_only_the_link() {
    let h = Harness::no_tmux();
    let (saved, empty) = opencode_saved_program(&h);
    let (out, env) = h.stub(Agent::OpenCode, "o");
    let mut extra = pairs(&env);
    extra.push(("PATH", empty.as_os_str()));
    let o = ws_run(&h, &extra, false);
    assert!(out.join("argv.json").exists(), "{}", text(&o.stderr));
    let path = stub_env(&out)["PATH"].as_str().unwrap().to_string();
    let last = std::env::split_paths(&path).last().unwrap();
    assert_eq!(last, program_dir(&h));
    let names: Vec<_> = std::fs::read_dir(&last).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, [OsString::from("opencode")]);
    assert_eq!(std::fs::read_link(last.join("opencode")).unwrap(), saved);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&last).unwrap().permissions().mode() & 0o777, 0o700);
}

/// Puts a folder in place of the state `lock` file while it lives; `Drop` puts the file back (round 3 finding 7).
struct LockIsAFolder(PathBuf);

impl LockIsAFolder {
    fn new(lock: PathBuf) -> LockIsAFolder {
        std::fs::remove_file(&lock).unwrap();
        std::fs::create_dir(&lock).unwrap();
        LockIsAFolder(lock)
    }
}

impl Drop for LockIsAFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
        let _ = std::fs::write(&self.0, "");
    }
}

/// Review round 2 finding 8, round 3 finding 7: an exit write that fails with an I/O error prints the D5 line and no
/// line that needs the record; the shell of step 8 still runs. The fault is a folder at the state `lock`: the lock
/// open fails for every user, root included (`open_regular`), unlike a folder mode. The cause names the lock path as
/// the binary sees it (FX-28 review round 2 F2).
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn d5_an_exit_write_that_fails_with_an_io_error_prints_one_line() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::ClaudeCode, &[])));
    let gate = h.root().join("gate");
    let hold = Hold::new(&gate, "returned");
    let (_, mut env) = h.stub(Agent::ClaudeCode, "a");
    env.extend([("ZYGO_TEST_MEMBER_GATE", gate.clone().into()), ("ZYGO_STUB_EXIT", "3".into())]);
    let child = spawn_ws_run(&h, &pairs(&env));
    assert!(until(Duration::from_secs(20), || gate.join("returned").exists()));
    let lock = LockIsAFolder::new(h.store(WS_ID).dir().join("lock"));
    drop(hold);
    let o = child.output("ws run");
    drop(lock);
    let err = text(&o.stderr);
    let head = format!("zygo: could not save the exit record of pane {KEY}: ");
    let shown = h.root().join(".local/state/zygo/workspaces").join(WS_ID).join("lock");
    assert!(err.contains(&format!("{head}{}: is not a regular file\n", shown.display())), "{err}");
    assert!(!err.contains("exited 3"), "{err}"); // D7: no record, so no resume line
    assert!(member(&h).pending.is_some()); // nothing written
    assert_eq!(o.status.code(), Some(0)); // the shell of step 8
    assert_eq!(shell_file(&h, "cwd").trim_end(), h.repo().to_str().unwrap());
}

/// Review round 2 finding 8: a results link that cannot be made (a file at `results`) is one session-log line, and the
/// launch goes on (C6, D3).
#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn au16_a_failed_results_link_is_logged_and_the_launch_goes_on() {
    let h = Harness::no_tmux();
    one_pane(&h, &h.repo(), Some(fresh(Agent::ClaudeCode, &[])));
    let results = results_dir(&h).parent().unwrap().to_path_buf();
    std::fs::create_dir_all(results.parent().unwrap()).unwrap();
    std::fs::write(&results, "a file").unwrap();
    let (out, mut env) = h.stub(Agent::ClaudeCode, "a");
    let c = ctl(&h, "a");
    env.push(("ZYGO_STUB_CONTROL", c.clone().into()));
    let mut child = spawn_ws_run(&h, &pairs(&env));
    assert!(started(&c)); // the launch went on
    let dir = PathBuf::from(stub_env(&out)["ZYGO_SESSION_DIR"].as_str().unwrap());
    let log = std::fs::read_to_string(dir.join("session.log")).unwrap();
    assert!(log.lines().any(|l| l.starts_with("session callback: results link: ")), "{log}");
    assert!(std::fs::symlink_metadata(dir.join("results")).is_err());
    common::stub_control(&c, "exit 0").unwrap();
    assert!(child.wait("ws run").success());
    assert_eq!(member(&h).state, MemberState::Ended);
}

/// Session zygo-app on the private server, first pane `cmd` with `@zygo-pane` = `key`, tagged with WS_ID.
fn tmux_session(h: &Harness, key: &str, cmd: &str) {
    h.tmux(&["new-session", "-d", "-s", "zygo-app", "-x", "120", "-y", "40", cmd]);
    h.tmux(&["set-option", "-t", "=zygo-app:", "@zygo-ws", WS_ID]);
    h.tmux(&["set-option", "-p", "-t", "=zygo-app:", "@zygo-pane", key]);
}

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w74_the_wrapper_snapshots_at_agent_start_and_at_agent_exit() {
    let Some(h) = Harness::new() else { return };
    tmux_session(&h, KEY, "cat");
    one_pane(&h, &h.repo(), Some(fresh(Agent::OpenCode, &[])));
    let (_, mut env) = h.stub(Agent::OpenCode, "o");
    let c = ctl(&h, "o");
    env.push(("ZYGO_STUB_CONTROL", c.clone().into()));
    let mut child = spawn_ws_run(&h, &pairs(&env));
    assert!(started(&c));
    assert_eq!(seq(&h), 1);
    common::stub_control(&c, "exit 0").unwrap();
    assert!(child.wait("ws run").success());
    assert_eq!(seq(&h), 2);
}

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w74_an_agent_pane_killed_by_hand_is_recorded_at_agent_exit() {
    let Some(h) = Harness::new() else { return };
    tmux_session(&h, KEY2, "cat");
    h.seed(WS_ID, &mut |w| {
        w.panes = vec![
            Pane { key: KEY2.into(), cwd: h.repo(), kind: PaneKind::Shell, ..Default::default() },
            Pane { key: KEY.into(), index: 1, cwd: h.repo(), kind: PaneKind::OpenCode, ..Default::default() },
        ];
        w.members.insert(KEY.into(), fresh(Agent::OpenCode, &[]));
    });
    let (_, mut env) = h.stub(Agent::OpenCode, "o");
    let c = ctl(&h, "o");
    env.push(("ZYGO_STUB_CONTROL", c.clone().into()));
    let run = h.script("run", &["ws", "run", "--ws-id", WS_ID, "--pane", KEY], &pairs(&env));
    let id = h.tmux(&["split-window", "-P", "-F", "#{pane_id}", "-t", "=zygo-app:", &run]);
    h.tmux(&["set-option", "-p", "-t", &id, "@zygo-pane", KEY]);
    std::fs::write(h.root().join("run.go"), "").unwrap();
    assert!(started(&c));
    assert_eq!(seq(&h), 1);
    h.tmux(&["kill-pane", "-t", &id]); // SIGHUP; the launcher's handlers catch it (B1)
    assert!(until(Duration::from_secs(20), || member(&h).state == MemberState::Resumable && seq(&h) == 2));
    let keys: Vec<String> = h.store(WS_ID).read().unwrap().unwrap().panes.into_iter().map(|p| p.key).collect();
    assert_eq!(keys, [KEY2]);
}

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn w73_a_member_write_runs_the_pending_snapshot() {
    let Some(h) = Harness::new() else { return };
    tmux_session(&h, KEY, "cat");
    one_pane(&h, &h.repo(), Some(fresh(Agent::OpenCode, &[])));
    let pending = h.store(WS_ID).dir().join("snapshot.pending");
    std::fs::write(&pending, "").unwrap();
    let gate = h.root().join("gate");
    let hold = Hold::new(&gate, "callback");
    let (_, mut env) = h.stub(Agent::OpenCode, "o");
    env.push(("ZYGO_TEST_MEMBER_GATE", gate.clone().into()));
    let mut child = spawn_ws_run(&h, &pairs(&env));
    assert!(until(Duration::from_secs(20), || gate.join("callback").exists()));
    assert!(!pending.exists());
    assert_eq!(seq(&h), 1); // the pending snapshot of the callback write, before the start snapshot
    drop(hold);
    assert!(child.wait("ws run").success());
    assert_eq!(seq(&h), 3); // start and exit
}

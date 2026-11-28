#![allow(dead_code)] // each test binary uses a part of this module

//! End-to-end workspace helpers (node E1, shared with E2): per-pane stub agents, tmux pane reads and typing, workspace
//! verbs through the binary, state reads. Unix only; the file that declares it gates it with cfg(unix).
//!
//! A test file of package `zygo` declares it next to `mod common;` as `#[cfg(unix)] #[path = "common/e2e.rs"] mod e2e;`.
//! It is not a module of `common/mod.rs`: `crates/zygo-launch/tests/stub_member.rs` compiles that file through
//! `#[path]` in a crate without `CARGO_BIN_EXE_zygo`, `regex` or `walkdir` (E1 plan, E-12).

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use zygo_core::Agent;
use zygo_workspace::state::{MemberRecord, Workspace};

use crate::common::stub_agent;
use crate::common::workspace::{quote, ready_exe, zygo_exe, Guarded, Harness};

/// The bound of every wait for a stub, a state change or a pane (a test bound).
pub const BOUND: Duration = Duration::from_secs(20);

/// The 5 s of W§18 #24, #77 and the `C-c` checks.
pub const FIVE: Duration = Duration::from_secs(5);

/// The crash note of the first `SessionStart` after a stop that zygo did not make (AU42; `zygo-hooks/src/workspace.rs`).
pub const CRASH: &str = "zygo did not stop this session cleanly; the list of closed units was not saved.";

/// The bit of SIGINT in a `/proc/<pid>/status` signal mask.
pub const SIGINT_BIT: u64 = 1 << (libc::SIGINT - 1);

/// Bytes as text (as `workspace_member.rs:36-38`).
pub fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// A JSON file (as `workspace_member.rs:40-42`).
pub fn json_file(p: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Polls `f` every 20 ms until it is true or `bound` is over (as `workspace_member.rs:44-56`).
pub fn until(bound: Duration, mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
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

/// `$TMUX` of a client of the private server (as `workspace_member.rs:178-180`).
pub fn tmux_env(h: &Harness) -> OsString {
    OsString::from(format!("{},1,0", h.socket().display()))
}

/// The one workspace state of the harness (as `workspace_member.rs:275-280`).
pub fn only_state(h: &Harness) -> (String, Workspace) {
    let mut all = zygo_workspace::state::list_all(&h.paths);
    assert_eq!(all.len(), 1, "{:?}", all.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>());
    let (id, ws) = all.remove(0);
    (id, ws.unwrap())
}

/// [`Harness::script`] (`root/<name>.sh`, mode 0644, waits for `root/<name>.go`, then `zygo <args>` with only the
/// harness environment and `extra`), plus `TMUX` and `TMUX_PANE` of the pane after the other names and stderr to
/// `root/<name>.err`. Returns the pane command of [`Harness::script`].
pub fn hand_script(h: &Harness, name: &str, args: &[&str], extra: &[(&str, &OsStr)]) -> String {
    let cmd = h.script(name, args, extra);
    let path = h.root().join(format!("{name}.sh"));
    let body = std::fs::read_to_string(&path).unwrap();
    let exe = format!(" {} ", quote(OsStr::new(zygo_exe())));
    let at = body.find(&exe).unwrap_or_else(|| panic!("no zygo word in {body}"));
    let err = quote(h.root().join(format!("{name}.err")).as_os_str());
    let body = format!("{} TMUX=\"$TMUX\" TMUX_PANE=\"$TMUX_PANE\"{} 2>{err}\n", &body[..at], body[at..].trim_end());
    // The file exists, so the write keeps its mode 0644.
    std::fs::write(&path, body).unwrap();
    cmd
}

/// `dir` as a git repository with one commit, and `git` on the harness `PATH` (as `workspace_member.rs:228-247`, with
/// `dir` in place of `h.repo()`).
pub fn git_repo_at(h: &Harness, dir: &Path) {
    for args in [&["init", "-q"][..], &["commit", "-q", "--allow-empty", "-m", "init"]] {
        let o = Command::new("git")
            .args(["-c", "user.name=zygo-test", "-c", "user.email=zygo-test@example.invalid", "-c", "init.defaultBranch=main"])
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o.stderr));
    }
    let link = h.bin.join("git");
    if !link.exists() {
        std::os::unix::fs::symlink(which::which("git").unwrap(), link).unwrap();
    }
}

/// Sets the mtime of `p` (as `workspace_cli.rs:73-75`; follows a link).
pub fn set_mtime(p: &Path, t: SystemTime) {
    std::fs::OpenOptions::new().read(true).open(p).unwrap().set_modified(t).unwrap();
}

/// Sets the mtime of the symlink `p` itself (as `workspace_cli.rs:201-212`).
pub fn set_link_mtime(p: &Path, t: SystemTime) {
    use std::os::unix::ffi::OsStrExt;
    let secs = t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    // SAFETY: a zeroed `timespec` is valid; its two fields are set below
    let mut ts: [libc::timespec; 2] = unsafe { std::mem::zeroed() };
    for x in &mut ts {
        x.tv_sec = secs as libc::time_t;
    }
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c` is a NUL-terminated path and `ts` holds two `timespec` values, as `utimensat` requires
    assert_eq!(unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), ts.as_ptr(), libc::AT_SYMLINK_NOFOLLOW) }, 0);
}

/// One request on 127.0.0.1 (the form of `crates/zygo-launch/tests/launcher.rs:886-925`): the header lines of the
/// request, each one lowercased, are kept; the reply is a minimal Messages response. A test matches a header by line
/// equality.
pub fn mock_upstream() -> (u16, Arc<Mutex<Vec<String>>>) {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    let got: Arc<Mutex<Vec<String>>> = Arc::default();
    let g = got.clone();
    std::thread::spawn(move || {
        let Ok((mut s, _)) = l.accept() else { return };
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let (head_end, len) = loop {
            let n = s.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                let len = head
                    .split("\r\n")
                    .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                    .unwrap_or(0);
                *g.lock().unwrap() = head.split("\r\n").map(String::from).collect();
                break (i + 4, len);
            }
        };
        while buf.len() < head_end + len {
            let n = s.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let reply = r#"{"id":"m","type":"message","role":"assistant","content":[],"usage":{"input_tokens":1,"output_tokens":1}}"#;
        let _ = write!(
            s,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
            reply.len()
        );
    });
    (port, got)
}

/// E-1: writes `bin/claude` (Claude Code) or `bin/opencode` (OpenCode), mode 0755, as a script that answers
/// `--version` with the pinned minimum, runs `prelude`, and, when `ZYGO_PANE` is set, gives the stub its own output
/// folder `root/out/<key>` and control folder `root/ctl/<key>`, then `exec`s the stub. A member launcher sets
/// `ZYGO_PANE` for both agent kinds (W§8.4 last paragraph). An ignored signal stays ignored across `exec`, so
/// `prelude = "trap '' TERM"` gives a stub that ignores SIGTERM.
pub fn per_pane_agent(h: &Harness, kind: Agent, prelude: &str) {
    use std::os::unix::fs::PermissionsExt;
    let name = match kind {
        Agent::ClaudeCode => "claude",
        Agent::OpenCode => "opencode",
    };
    let root = h.root().display().to_string();
    let body = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = --version ]; then echo '{v}'; exit 0; fi\n\
         {prelude}\n\
         if [ -n \"$ZYGO_PANE\" ]; then\n\
         \x20 ZYGO_STUB_OUT='{root}/out/'\"$ZYGO_PANE\"; ZYGO_STUB_CONTROL='{root}/ctl/'\"$ZYGO_PANE\"\n\
         \x20 export ZYGO_STUB_OUT ZYGO_STUB_CONTROL\n\
         \x20 /bin/mkdir -p \"$ZYGO_STUB_OUT\" \"$ZYGO_STUB_CONTROL\"\n\
         fi\n\
         exec '{stub}' \"$@\"\n",
        v = zygo_config::builtin::min_agent_version(kind),
        stub = stub_agent().display(),
    );
    let p = h.bin.join(name);
    // `bin/<name>` is a copy of the stub (`common/workspace.rs:262-266`), not a link: replace the file.
    let _ = std::fs::remove_file(&p);
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    ready_exe(&p);
}

/// [`per_pane_agent`] for Claude Code.
pub fn per_pane_claude(h: &Harness, prelude: &str) {
    per_pane_agent(h, Agent::ClaudeCode, prelude)
}

/// The stub output folder of the pane `key`.
pub fn out(h: &Harness, key: &str) -> PathBuf {
    h.root().join("out").join(key)
}

/// The stub control folder of the pane `key`.
pub fn ctl(h: &Harness, key: &str) -> PathBuf {
    h.root().join("ctl").join(key)
}

fn done_lines(h: &Harness, key: &str) -> Vec<String> {
    std::fs::read_to_string(ctl(h, key).join("done")).unwrap_or_default().lines().map(String::from).collect()
}

/// The stub starts of the pane `key`: the `start ` lines of its `done` file; 0 without the file.
pub fn starts(h: &Harness, key: &str) -> usize {
    done_lines(h, key).iter().filter(|l| l.starts_with("start ")).count()
}

/// Waits ([`BOUND`]) until the pane `key` has at least `n` stub starts.
pub fn wait_starts(h: &Harness, key: &str, n: usize) -> bool {
    until(BOUND, || starts(h, key) >= n)
}

/// The pid of the last stub start of the pane `key`.
pub fn stub_pid(h: &Harness, key: &str) -> i32 {
    let lines = done_lines(h, key);
    let last = lines.iter().rev().find_map(|l| l.strip_prefix("start ")).unwrap_or_else(|| panic!("no stub start for {key}"));
    last.trim().parse().unwrap()
}

/// `flag` and `id` of the session argument the last stub of `key` received.
pub fn session_arg(h: &Harness, key: &str) -> (String, String) {
    let v = json_file(&out(h, key).join("session-arg.json"));
    (v["flag"].as_str().unwrap().to_string(), v["id"].as_str().unwrap().to_string())
}

/// The argv the last stub of `key` received.
pub fn argv(h: &Harness, key: &str) -> Vec<String> {
    serde_json::from_value(json_file(&out(h, key).join("argv.json"))).unwrap()
}

/// The environment the last stub of `key` received.
pub fn stub_env(h: &Harness, key: &str) -> Value {
    json_file(&out(h, key).join("env.json"))
}

/// The session arguments of an argv: the tokens before the first `--` that are a session flag or a one-token form.
pub fn session_args(a: &[String]) -> usize {
    let end = a.iter().position(|t| t == "--").unwrap_or(a.len());
    a[..end]
        .iter()
        .filter(|t| {
            ["--session-id", "--resume", "-r", "-c", "--continue", "--fork-session", "--session", "-s"].contains(&t.as_str())
                || ["--session-id=", "--resume=", "--session="].iter().any(|p| t.starts_with(p))
        })
        .count()
}

/// The `out` field of each `SessionStart` line of `out/<key>/hooks.jsonl` (`stub.rs`, `call_hooks`).
pub fn session_start_outs(h: &Harness, key: &str) -> Vec<String> {
    std::fs::read_to_string(out(h, key).join("hooks.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["event"] == "SessionStart")
        .map(|v| v["out"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// `zygo <args>` with `SHELL=/bin/sh` (E-2), `TMUX` of the private server when `with_tmux`, then `extra` (a later name
/// wins), in `dir`.
pub fn cmd_in(h: &Harness, dir: &Path, args: &[&str], extra: &[(&str, &OsStr)], with_tmux: bool) -> Command {
    let t = tmux_env(h);
    let mut all: Vec<(&str, &OsStr)> = vec![("SHELL", OsStr::new("/bin/sh"))];
    if with_tmux {
        all.push(("TMUX", t.as_os_str()));
    }
    all.extend_from_slice(extra);
    let mut c = h.zygo(args, &all);
    c.current_dir(dir);
    c
}

/// [`cmd_in`], run to its end through `Guarded`.
pub fn run_in(h: &Harness, dir: &Path, args: &[&str], extra: &[(&str, &OsStr)], with_tmux: bool) -> Output {
    Guarded::spawn(cmd_in(h, dir, args, extra, with_tmux)).output(args[0])
}

/// A workspace verb in `h.repo()` on the private server.
pub fn cli(h: &Harness, args: &[&str], extra: &[(&str, &OsStr)]) -> Output {
    run_in(h, &h.repo(), args, extra, true)
}

/// Writes `root/config.toml`: `body`, then the harness `NO_MARKETPLACE` ([`Harness::config`], FX-23).
pub fn config(h: &Harness, body: &str) {
    h.config(body);
}

/// The state `id`.
pub fn state(h: &Harness, id: &str) -> Workspace {
    h.store(id).read().unwrap().unwrap()
}

/// The stored JSON of the state `id`.
pub fn raw(h: &Harness, id: &str) -> Value {
    json_file(&h.store(id).dir().join("workspace.json"))
}

/// No launcher of the state `id` is live.
pub fn settled(h: &Harness, id: &str) -> bool {
    h.store(id).live_launcher(&state(h, id)).is_none()
}

/// The tmux index of the `i`-th saved window.
pub fn win(ws: &Workspace, i: usize) -> u32 {
    ws.windows[i].index
}

/// The key of the pane `index` of the `i`-th saved window.
pub fn key_at(ws: &Workspace, i: usize, index: u32) -> String {
    let w = win(ws, i);
    ws.panes
        .iter()
        .find(|p| p.window == w && p.index == index)
        .unwrap_or_else(|| panic!("no pane {index} in window {w}: {:?}", ws.panes))
        .key
        .clone()
}

/// The first member key, in key order, whose record matches `pred`.
pub fn agent_key(ws: &Workspace, pred: impl Fn(&MemberRecord) -> bool) -> String {
    ws.members.iter().find(|(_, m)| pred(m)).map(|(k, _)| k.clone()).unwrap_or_else(|| panic!("no member: {:?}", ws.members))
}

/// One pane of a `list-panes -s` read.
#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub key: String,
    pub cmd: String,
    pub pid: i32,
    pub active: bool,
    pub window: u32,
    pub dead: bool,
    pub path: String,
    pub start: String,
}

/// The panes of `session`.
pub fn rows(h: &Harness, session: &str) -> Vec<Row> {
    let f = "#{pane_id}|#{@zygo-pane}|#{pane_current_command}|#{pane_pid}|#{pane_active}|#{window_index}|#{pane_dead}|#{pane_current_path}|#{pane_start_command}";
    h.tmux(&["list-panes", "-s", "-t", &format!("={session}:"), "-F", f])
        .lines()
        .map(|l| {
            let v: Vec<&str> = l.splitn(9, '|').collect();
            assert_eq!(v.len(), 9, "{l}");
            Row {
                id: v[0].into(),
                key: v[1].into(),
                cmd: v[2].into(),
                pid: v[3].parse().unwrap_or(0),
                active: v[4] == "1",
                window: v[5].parse().unwrap(),
                dead: v[6] == "1",
                path: v[7].into(),
                start: v[8].into(),
            }
        })
        .collect()
}

/// The pane of `session` whose key is `key`; panics with the rows when none.
pub fn row(h: &Harness, session: &str, key: &str) -> Row {
    let all = rows(h, session);
    all.iter().find(|r| r.key == key).cloned().unwrap_or_else(|| panic!("no pane {key}: {all:?}"))
}

/// The pane keys that tmux holds in `session`.
pub fn tmux_keys(h: &Harness, session: &str) -> BTreeSet<String> {
    rows(h, session).into_iter().map(|r| r.key).filter(|k| !k.is_empty()).collect()
}

/// The `panes[]` keys.
pub fn keys(ws: &Workspace) -> BTreeSet<String> {
    ws.panes.iter().map(|p| p.key.clone()).collect()
}

/// The windows of `session`.
pub fn window_count(h: &Harness, session: &str) -> usize {
    h.tmux(&["list-windows", "-t", &format!("={session}:"), "-F", "#{window_index}"]).lines().count()
}

/// `#{pane_current_command}` of `pane`.
pub fn cmd_of(h: &Harness, pane: &str) -> String {
    h.server
        .as_ref()
        .unwrap()
        .run(&["display-message", "-p", "-t", pane, "#{pane_current_command}"])
        .unwrap_or_default()
}

/// Waits ([`BOUND`]) until `pane` runs `cmd`.
pub fn wait_cmd(h: &Harness, pane: &str, cmd: &str) -> bool {
    until(BOUND, || cmd_of(h, pane) == cmd)
}

/// [`wait_cmd`], then 1.5 s for the shell to read its terminal (E-11, `scripts/premises/live-tmux.sh:596`).
pub fn wait_shell(h: &Harness, pane: &str, cmd: &str) -> bool {
    let ok = wait_cmd(h, pane, cmd);
    std::thread::sleep(Duration::from_millis(1500));
    ok
}

/// Types `line` into `pane`, then Enter (E-11, `scripts/premises/live-tmux.sh:573`).
pub fn send(h: &Harness, pane: &str, line: &str) {
    h.tmux(&["send-keys", "-t", pane, "-l", line]);
    std::thread::sleep(Duration::from_millis(300));
    h.tmux(&["send-keys", "-t", pane, "Enter"]);
}

/// The whole text of `pane`, wrapped lines joined.
pub fn capture(h: &Harness, pane: &str) -> String {
    h.tmux(&["capture-pane", "-p", "-J", "-S", "-", "-t", pane])
}

/// The text of `p` once it is not empty, polled every 50 ms for at most `bound`.
pub fn wait_file(p: &Path, bound: Duration) -> Option<String> {
    let t0 = Instant::now();
    loop {
        if let Ok(s) = std::fs::read_to_string(p)
            && !s.is_empty()
        {
            return Some(s);
        }
        if t0.elapsed() >= bound {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// E-7: a `#{window_layout}` without its checksum and without the pane id of each leaf cell.
pub fn layout_shape(layout: &str) -> String {
    let body = layout.get(5..).unwrap_or_default();
    let re = regex::Regex::new(r"(\d+x\d+,\d+,\d+),\d+").unwrap();
    re.replace_all(body, "$1").into_owned()
}

/// Layout shape, width and height of each window of `session`, by index.
pub fn layouts(h: &Harness, session: &str) -> Vec<(String, u32, u32)> {
    h.tmux(&["list-windows", "-t", &format!("={session}:"), "-F", "#{window_layout}|#{window_width}|#{window_height}"])
        .lines()
        .map(|l| {
            let v: Vec<&str> = l.split('|').collect();
            (layout_shape(v[0]), v[1].parse().unwrap(), v[2].parse().unwrap())
        })
        .collect()
}

/// Layout shape, width and height of each saved window, by index.
pub fn saved_layouts(ws: &Workspace) -> Vec<(String, u32, u32)> {
    let mut w: Vec<_> = ws.windows.iter().collect();
    w.sort_by_key(|w| w.index);
    w.into_iter().map(|w| (layout_shape(&w.layout), w.width, w.height)).collect()
}

/// Window index, pane index, left, top, width and height of each pane of `session`.
pub fn geometry(h: &Harness, session: &str) -> Vec<String> {
    let f = "#{window_index} #{pane_index} #{pane_left} #{pane_top} #{pane_width} #{pane_height}";
    h.tmux(&["list-panes", "-s", "-t", &format!("={session}:"), "-F", f]).lines().map(String::from).collect()
}

/// Every window of `session` has `remain-on-exit off` (AU68: the build hold is released before `phase = up`).
pub fn assert_held_released(h: &Harness, session: &str) {
    for i in h.tmux(&["list-windows", "-t", &format!("={session}:"), "-F", "#{window_index}"]).lines() {
        let v = h.tmux(&["show-options", "-w", "-v", "-t", &format!("={session}:{i}"), "remain-on-exit"]);
        assert_eq!(v.trim(), "off", "window {i} of {session}");
    }
}

/// `down <name> --now --wait`; exit 0.
pub fn down(h: &Harness, name: &str) {
    let o = cli(h, &["down", name, "--now", "--wait"], &[]);
    assert_eq!(o.status.code(), Some(0), "down: {}{}", text(&o.stdout), text(&o.stderr));
}

/// `up <name> --no-attach` with `extra`.
pub fn up(h: &Harness, name: &str, extra: &[(&str, &OsStr)]) -> Output {
    cli(h, &["up", name, "--no-attach"], extra)
}

/// A tmux server on `socket` that a test started outside the harness. `pid` arms the helper: [`ServerAt::kill_now`]
/// signals it once and disarms; `Drop` sends SIGTERM only while it is still armed, then runs `kill-server` (result
/// ignored). So the pid gets at most one signal, and none after the test has killed it.
pub struct ServerAt {
    pub tmux: PathBuf,
    pub socket: PathBuf,
    pub pid: Option<i32>,
}

impl ServerAt {
    /// Sends `sig` to the armed pid now and disarms the helper; no signal when it is not armed.
    pub fn kill_now(&mut self, sig: libc::c_int) {
        if let Some(pid) = self.pid.take() {
            // SAFETY: `kill` takes plain integers; `pid` is a tmux server that this test started, not signalled before.
            unsafe {
                libc::kill(pid, sig);
            }
        }
    }
}

impl Drop for ServerAt {
    fn drop(&mut self) {
        self.kill_now(libc::SIGTERM);
        let _ = Command::new(&self.tmux)
            .arg("-S")
            .arg(&self.socket)
            .arg("kill-server")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// The pid that a test's child writes in a file, armed from the start. [`KillOnDrop::kill_now`] signals it once and
/// disarms; `Drop` sends SIGKILL only while it is still armed. So the pid gets at most one signal, and none after the
/// test has killed it.
pub struct KillOnDrop {
    file: PathBuf,
    armed: bool,
}

impl KillOnDrop {
    /// An armed helper for the pid in `file`.
    pub fn new(file: PathBuf) -> KillOnDrop {
        KillOnDrop { file, armed: true }
    }

    /// Sends `sig` now to the pid in the file, when the helper is armed and the file holds a pid above 0; disarms the
    /// helper in every case.
    pub fn kill_now(&mut self, sig: libc::c_int) {
        if !std::mem::replace(&mut self.armed, false) {
            return;
        }
        if let Some(pid) = std::fs::read_to_string(&self.file).ok().and_then(|s| s.trim().parse::<i32>().ok())
            && pid > 0
        {
            // SAFETY: `kill` takes plain integers; the pid is a process that this test's child wrote, not signalled
            // before.
            unsafe {
                libc::kill(pid, sig);
            }
        }
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        self.kill_now(libc::SIGKILL);
    }
}

/// `SigIgn`, `SigBlk` and `SigCgt` of `/proc/<pid>/status`.
#[cfg(target_os = "linux")]
pub fn sig_masks(pid: i32) -> (u64, u64, u64) {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    let get = |name: &str| {
        let v = s.lines().find_map(|l| l.strip_prefix(name)).unwrap_or_else(|| panic!("no {name} in {s}"));
        u64::from_str_radix(v.trim(), 16).unwrap()
    };
    (get("SigIgn:"), get("SigBlk:"), get("SigCgt:"))
}

/// Every entry named `pwned` under `dir`.
pub fn pwned_under(dir: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_name() == "pwned")
        .map(|e| e.path().to_path_buf())
        .collect()
}

// ---- E2 helpers (node E2, plan steps 2 and 3): added items only; no E1 item above is moved or changed ----

use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;
use zygo_workspace::state::Phase;

/// E2 E-1: [`per_pane_agent`] for OpenCode: `bin/opencode` in the form of [`per_pane_claude`] (A-1: E1's
/// `per_pane_agent` already writes both kinds, so this one names the kind and adds no body).
pub fn per_pane_opencode(h: &Harness, prelude: &str) {
    per_pane_agent(h, Agent::OpenCode, prelude)
}

/// The absolute path of `node`. A prerequisite of the OpenCode plugin test, not a skip (E2 decision 12).
pub fn node_exe() -> PathBuf {
    which::which("node").unwrap_or_else(|_| panic!("w18_74: node is not on PATH; it is required to run the OpenCode plugin logic"))
}

static NODE_RUNS: AtomicUsize = AtomicUsize::new(0);

/// Runs the member's own generated plugin (`<OPENCODE_CONFIG_DIR>/plugins/zygo.js` of the stub of `key`) under `node`
/// with the stub's environment, as OpenCode would load it: the factory call, then one `event` call per
/// `[type, properties]` item of the JSON list `events`, then, when `transform`, one
/// `experimental.chat.system.transform` call for the main role. Returns the printed JSON (`{"system": [...]}` or
/// `{}`); exit 0 is asserted. OpenCode runs plugins in Bun; `node` runs the same file as a proxy for the plugin logic,
/// and the live W§17 PW4 check covers the real runtime.
pub fn node_plugin(h: &Harness, key: &str, events: &str, transform: bool) -> Value {
    let env = stub_env(h, key);
    let dir = env["OPENCODE_CONFIG_DIR"].as_str().unwrap_or_else(|| panic!("no OPENCODE_CONFIG_DIR in {env}"));
    let plugin = format!("file://{}", Path::new(dir).join("plugins/zygo.js").display());
    let repo = h.repo().display().to_string();
    let n = NODE_RUNS.fetch_add(1, Ordering::SeqCst);
    let drv = h.root().join(format!("drv-{n}.mjs"));
    let body = format!(
        "import {{ ZygoPlugin }} from {plugin};\n\
         const h = await ZygoPlugin({{ directory: {repo} }});\n\
         for (const [type, properties] of JSON.parse(process.env.EVENTS)) await h.event({{ event: {{ type, properties }} }});\n\
         if (process.env.TRANSFORM === \"1\") {{\n\
         \x20 const out = {{ system: [] }};\n\
         \x20 await h[\"experimental.chat.system.transform\"]({{ model: {{ id: process.env.ZYGO_MAIN_ROLE, providerID: \"zygo\" }} }}, out);\n\
         \x20 console.log(JSON.stringify({{ system: out.system }}));\n\
         }} else console.log(\"{{}}\");\n",
        plugin = Value::String(plugin),
        repo = Value::String(repo),
    );
    std::fs::write(&drv, body).unwrap();
    let mut c = Command::new(node_exe());
    c.arg(&drv).env_clear();
    for (k, v) in env.as_object().unwrap() {
        c.env(k, v.as_str().unwrap_or_default());
    }
    c.env("EVENTS", events).env("TRANSFORM", if transform { "1" } else { "0" });
    c.current_dir(h.repo()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let o = Guarded::spawn(c).output("node plugin");
    assert_eq!(o.status.code(), Some(0), "node plugin: {}{}", text(&o.stdout), text(&o.stderr));
    serde_json::from_str(text(&o.stdout).trim()).unwrap_or_else(|e| panic!("node plugin output: {e}: {}", text(&o.stdout)))
}

/// A caller session id for the AU18 detach rule (the form of E1 step 5).
pub const SID: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";

/// A catalog model `test-e2` on `http://127.0.0.1:<port>` whose key is `env:ZYGO_TEST_KEY`, and the role `lead` (E1
/// step 5). Every `up` of a test that uses it passes `ZYGO_TEST_KEY=k` (E2 E-3).
pub fn provider_config(port: u16) -> String {
    format!(
        "[[model]]\nid = \"test-e2\"\n[[model.provider]]\nname = \"p-e2\"\nbase_url = \"http://127.0.0.1:{port}\"\n\
         dialects = [\"anthropic\"]\nupstream_model = \"up-e2\"\nkey = \"env:ZYGO_TEST_KEY\"\n\
         [role.lead]\nmodel = \"test-e2\"\ndescription = \"d\"\nprompt = \"p\"\n"
    )
}

/// One request that [`held_upstream`] took: the time it was read, and the time its reply was written.
#[derive(Debug, Clone)]
pub struct Held {
    pub recv: Instant,
    pub end: Option<Instant>,
}

/// A loopback upstream (E2 E-3): each connection on its own thread; it reads the request head and its
/// `content-length` body, records a [`Held`], sleeps the `<ms>` of the first `stub hold <ms>` in the body (0 when
/// none), writes a minimal Messages reply and sets `end`.
pub fn held_upstream() -> (u16, Arc<Mutex<Vec<Held>>>) {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    let held: Arc<Mutex<Vec<Held>>> = Arc::default();
    let all = held.clone();
    let hold = regex::Regex::new(r"stub hold (\d+)").unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let all = all.clone();
            let hold = hold.clone();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head_end, len) = loop {
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                        let len = head
                            .split("\r\n")
                            .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        break (i + 4, len);
                    }
                };
                while buf.len() < head_end + len {
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let at = {
                    let mut v = all.lock().unwrap();
                    v.push(Held { recv: Instant::now(), end: None });
                    v.len() - 1
                };
                let body = String::from_utf8_lossy(&buf[head_end..]).into_owned();
                let ms = hold.captures(&body).and_then(|c| c[1].parse::<u64>().ok()).unwrap_or(0);
                std::thread::sleep(Duration::from_millis(ms));
                let reply = r#"{"id":"msg_e2","type":"message","role":"assistant","model":"up-e2","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}"#;
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = s.flush();
                all.lock().unwrap()[at].end = Some(Instant::now());
            });
        }
    });
    (port, held)
}

/// A loopback server that answers every request with `200` and the status reply of a server before HUD-5 and AU91
/// (no `requests_in_flight`) for `session` (E2 E-4).
pub fn old_status_server(session: &str) -> u16 {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    let body = json!({
        "session": session, "profile": null, "work": null, "used_context": null, "effective_window": null,
        "compact": null, "cache_read_share": null, "expires_at": ""
    })
    .to_string();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}

/// Points the session folder `sdir` at `http://127.0.0.1:<port>/`: `sdir/.url.tmp` (mode 0600), then a rename over
/// `sdir/url`.
pub fn point_url(sdir: &Path, port: u16) {
    use std::os::unix::fs::PermissionsExt;
    let tmp = sdir.join(".url.tmp");
    std::fs::write(&tmp, format!("http://127.0.0.1:{port}/")).unwrap();
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::rename(&tmp, sdir.join("url")).unwrap();
}

/// The lines of `out/<key>/hooks.jsonl` whose `event` is `event`, in order (`stub.rs`, `call_hooks`).
pub fn hook_records(h: &Harness, key: &str, event: &str) -> Vec<Value> {
    std::fs::read_to_string(out(h, key).join("hooks.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["event"] == event)
        .collect()
}

/// When `rec["command"]` ends with `cmd_end`, `rec["out"]` parsed as JSON; `None` for an empty `out` or another command.
pub fn hook_out(rec: &Value, cmd_end: &str) -> Option<Value> {
    if !rec["command"].as_str().is_some_and(|c| c.ends_with(cmd_end)) {
        return None;
    }
    let o = rec["out"].as_str().unwrap_or_default();
    if o.is_empty() {
        return None;
    }
    serde_json::from_str(o).ok()
}

/// Among `hook_records(h, key, event)[from..]`, the first record whose `command` ends with `cmd_end`, its
/// [`hook_out`] (E2 decision 2: never the last record of the event, because a member has several hooks per event).
pub fn hook_reply(h: &Harness, key: &str, event: &str, from: usize, cmd_end: &str) -> Option<Value> {
    hook_records(h, key, event)
        .iter()
        .skip(from)
        .find(|r| r["command"].as_str().is_some_and(|c| c.ends_with(cmd_end)))
        .and_then(|r| hook_out(r, cmd_end))
}

/// The lines of `ctl/<key>/done` equal to `line`.
pub fn done_count(h: &Harness, key: &str, line: &str) -> usize {
    std::fs::read_to_string(ctl(h, key).join("done")).unwrap_or_default().lines().filter(|l| *l == line).count()
}

/// Waits ([`BOUND`]) until `ctl/<key>/done` has at least `n` lines equal to `line`.
pub fn wait_done(h: &Harness, key: &str, line: &str, n: usize) -> bool {
    until(BOUND, || done_count(h, key, line) >= n)
}

/// `ZYGO_SESSION`, `ZYGO_SESSION_DIR`, `ZYGO_AGENT` and `ZYGO_WORKSPACE` of the last stub of `key`.
pub fn member_env(h: &Harness, key: &str) -> Vec<(String, OsString)> {
    let e = stub_env(h, key);
    ["ZYGO_SESSION", "ZYGO_SESSION_DIR", "ZYGO_AGENT", "ZYGO_WORKSPACE"]
        .iter()
        .map(|n| (n.to_string(), OsString::from(e[*n].as_str().unwrap_or_else(|| panic!("no {n} in {e}")))))
        .collect()
}

/// The session folder of the last stub of `key`.
pub fn sdir(h: &Harness, key: &str) -> PathBuf {
    PathBuf::from(stub_env(h, key)["ZYGO_SESSION_DIR"].as_str().unwrap())
}

/// `zygo <args>` in `h.repo()` on the private server, spawned as a [`Guarded`] child.
pub fn spawn_down(h: &Harness, args: &[&str], extra: &[(&str, &OsStr)]) -> Guarded {
    Guarded::spawn(cmd_in(h, &h.repo(), args, extra, true))
}

/// The phase of the state `id`.
pub fn phase(h: &Harness, id: &str) -> Phase {
    state(h, id).phase
}

/// The tmux session `s` exists on the private server.
pub fn session_up(h: &Harness, s: &str) -> bool {
    h.server.as_ref().unwrap().run(&["has-session", "-t", &format!("={s}:")]).is_ok()
}

/// `sdir/activity.json`.
pub fn activity(sdir: &Path) -> Value {
    json_file(&sdir.join("activity.json"))
}

/// One `tools/call` of `tool` with `args` on `zygo mcp --agent claude-code` for the session folder `sdir` (after
/// `initialize`, protocol `2025-06-18`, and `notifications/initialized`; E1 step 15). Returns the reply with id 2.
pub fn mcp_call(h: &Harness, sdir: &Path, tool: &str, args: Value) -> Value {
    let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": tool, "arguments": args}});
    let req = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":\"2025-06-18\"}}}}\n\
         {{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}}\n{call}\n"
    );
    let mut c = h.zygo(&["mcp", "--agent", "claude-code"], &[("ZYGO_SESSION_DIR", sdir.as_os_str())]);
    c.stdin(Stdio::piped());
    let mut g = Guarded::spawn(c);
    crate::common::write_stdin(g.stdin().unwrap(), req.as_bytes());
    let o = g.output("mcp");
    let replies: Vec<Value> = text(&o.stdout).lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    replies.into_iter().find(|r| r["id"] == 2).unwrap_or_else(|| panic!("no reply 2: {}{}", text(&o.stdout), text(&o.stderr)))
}

/// A [`per_pane_agent`] prelude (E2 E-7) that copies `$ZYGO_SESSION_DIR/<file>` to `root/<dir>/<ZYGO_SESSION>.json`
/// before the stub runs: after `view.save` and the agent's spawn, before the stub's `SessionStart`.
pub fn copy_prelude(h: &Harness, file: &str, dir: &str) -> String {
    let d = h.root().join(dir).display().to_string();
    format!("/bin/mkdir -p '{d}'; /bin/cp \"$ZYGO_SESSION_DIR/{file}\" '{d}/'\"$ZYGO_SESSION\"'.json' 2>/dev/null")
}

// ---- ML-18 helpers (workspace mail): added items only; no item above is moved or changed ----

use zygo_workspace::mail::{mail_dir, store};

/// The wake text of M§7.3 step 2.
pub const WAKE: &str = "zygo: you have mail; read it with mail_read";

/// Writes `body` to `root/input/<name>` (makes `input/`) and returns the path. assert_no_mail_text skips `input/`.
pub fn body_file(h: &Harness, name: &str, body: &str) -> PathBuf {
    let dir = h.root().join("input");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

/// `zygo mail send <to> --subject <subject> --file <body_file(h, subject, body)>` through `cli`: asserts exit 0 and
/// stdout `sent <id> to <to>\n` with an id of 26 characters; returns the id.
pub fn mail_send_cli(h: &Harness, to: &str, subject: &str, body: &str) -> String {
    let f = body_file(h, subject, body);
    let o = cli(h, &["mail", "send", to, "--subject", subject, "--file", f.to_str().unwrap()], &[]);
    assert_eq!(o.status.code(), Some(0), "mail send: {}{}", text(&o.stdout), text(&o.stderr));
    let out = text(&o.stdout);
    let id = out
        .strip_prefix("sent ")
        .and_then(|r| r.strip_suffix(&format!(" to {to}\n")))
        .unwrap_or_else(|| panic!("mail send stdout: {out:?}"));
    assert_eq!(id.len(), 26, "{out:?}");
    id.to_string()
}

/// `mcp_call(h, &sdir(h, key), tool, args)`: asserts `result.isError == false` and returns
/// `result.content[0].text` parsed as JSON (the call the member `key`'s agent makes).
pub fn mail_tool(h: &Harness, key: &str, tool: &str, args: Value) -> Value {
    let r = mcp_call(h, &sdir(h, key), tool, args);
    assert_eq!(r["result"]["isError"], false, "{tool}: {r}");
    let t = r["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{tool}: {r}"));
    serde_json::from_str(t).unwrap_or_else(|e| panic!("{tool}: {e}: {t}"))
}

/// `store::unread(&mail_dir(&h.paths, id), key)`, unwrapped.
pub fn unread(h: &Harness, id: &str, key: &str) -> usize {
    store::unread(&mail_dir(&h.paths, id), key).unwrap()
}

/// `mail_dir(&h.paths, id)/<key>/wake.json`.
pub fn wake_file(h: &Harness, id: &str, key: &str) -> PathBuf {
    mail_dir(&h.paths, id).join(key).join("wake.json")
}

/// The times WAKE shows in `capture(h, pane)`.
pub fn wakes(h: &Harness, pane: &str) -> usize {
    capture(h, pane).matches(WAKE).count()
}

/// M§13 #8: every regular file under `h.root()` (walkdir, links not followed), except the subtrees
/// `mail_dir(&h.paths, id)` and `root/input`, holds none of `markers` as a byte substring; panics with the path and
/// the marker. Returns the paths it read.
pub fn assert_no_mail_text(h: &Harness, id: &str, markers: &[&str]) -> Vec<PathBuf> {
    let skip = [mail_dir(&h.paths, id), h.root().join("input")];
    let mut read = Vec::new();
    let walk = walkdir::WalkDir::new(h.root()).follow_links(false).into_iter();
    for e in walk.filter_entry(|e| !skip.iter().any(|s| e.path() == s.as_path())) {
        // A launcher may remove its session folder during the walk.
        let Ok(e) = e else { continue };
        if !e.file_type().is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(e.path()) else { continue };
        for m in markers {
            let m = m.as_bytes();
            assert!(!bytes.windows(m.len()).any(|w| w == m), "{} holds {}", e.path().display(), String::from_utf8_lossy(m));
        }
        read.push(e.path().to_path_buf());
    }
    read
}

/// M-G6 (no daemon): the `ps -axo pid=,command=` lines whose command holds `h.root()`, except a tmux process (its
/// first word ends in `/tmux` or is `tmux`). After `down` it is empty: no process for mail outlives the verbs.
pub fn root_processes(h: &Harness) -> Vec<String> {
    let o = Command::new("ps").args(["-axo", "pid=,command="]).stdin(Stdio::null()).output().unwrap();
    assert!(o.status.success(), "ps: {}", text(&o.stderr));
    let root = h.root().to_str().unwrap();
    text(&o.stdout)
        .lines()
        .filter(|l| l.contains(root))
        .filter(|l| {
            let first = l.split_whitespace().nth(1).unwrap_or_default();
            first != "tmux" && !first.ends_with("/tmux")
        })
        .map(str::to_string)
        .collect()
}

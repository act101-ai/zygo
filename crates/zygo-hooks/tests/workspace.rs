//! The member hooks (H1): workspace events, the dispatch and prompt blocks, the busy state and the tool count, the
//! member record at a start, the resume note and the snapshot child (workspaces design W§9.1, W§9.2, W§9.4, W§7.3,
//! W§7.4, W§18 #44, #86-#89). The stub zygo program is a shell script and workspaces need tmux (OP5), so the file is
//! unix only. Every test takes `SERIAL`: scripts are written while other tests fork (ETXTBSY).
#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_core::hook::{guarded, HookInput, HookOutput, SlashDispatch};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{Member, ResumeNote, SessionView, SubagentRef, ViewWorkspace};
use zygo_core::Agent;
use zygo_hooks::{mail, prompt, session_start, session_text, workspace};
use zygo_workspace::activity::{self, Activity};
use zygo_workspace::mail::mail_dir;
use zygo_workspace::snapshot;
use zygo_workspace::state::{MemberRecord, MemberState, Pending, Phase, Store, Subagent};

const ID: &str = "0123456789abcdef";
const KEY: &str = "kaaaaaaaa";
const SID: &str = "01JTESTSESSIONAAAAAAAAAAAA";
const OLD: &str = "01JOLDSESSIONAAAAAAAAAAAAA";
const OTHER: &str = "01JOTHERSESSIONAAAAAAAAAAA";
/// The `session_id` of the fixtures.
const AS1: &str = "00000000-0000-4000-8000-000000000001";
const AS2: &str = "00000000-0000-4000-8000-000000000002";
const S5: Duration = Duration::from_secs(5);
const I1: &str = "zygo stopped this session while subagents Explore a1 were running; their results were lost; dispatch them again if still needed.";
const U12: &str = "Units u-1, u-2 were closed when zygo stopped this session; call work_begin to open them again if work continues.";
const U1: &str = "Units u-1 were closed when zygo stopped this session; call work_begin to open them again if work continues.";
const L2: &str = "zygo: 2 new messages; read them with mail_read";
const CRASH: &str = "zygo did not stop this session cleanly; the list of closed units was not saved.";

/// `data_dir` = `<p>/data` is not under `$HOME`, so the state folder is `<p>/data/state`.
fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}

fn deny(reason: &str) -> Value {
    json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}})
}

fn parsed(out: anyhow::Result<Option<String>>) -> Value {
    serde_json::from_str(&out.unwrap().expect("no output")).unwrap()
}

fn marker(ws_id: &str, pane: &str, note: Option<ResumeNote>) -> ViewWorkspace {
    ViewWorkspace { member: Member { ws_id: ws_id.into(), pane: pane.into(), ..Default::default() }, resume_note: note, ..Default::default() }
}

struct Fx {
    t: TempDir,
    paths: Paths,
    dir: SessionDir,
    zygo: PathBuf,
}

fn fx_with(agent: Agent, ws: Option<ViewWorkspace>) -> Fx {
    let t = TempDir::new().unwrap();
    let paths = paths(t.path());
    let dir = SessionDir::new(paths.sessions_dir.join(SID));
    dir.create().unwrap();
    SessionView { session: SID.into(), agent, main_role: "lead".into(), workspace: ws, ..Default::default() }.save(&dir).unwrap();
    let zygo = rec(t.path());
    Fx { t, paths, dir, zygo }
}

fn fx(agent: Agent) -> Fx {
    fx_with(agent, Some(marker(ID, KEY, None)))
}

fn ws_env(f: &Fx, event: &str, payload: Value, zygo: &Path, sd: Option<OsString>) -> anyhow::Result<Option<String>> {
    workspace::run(event, &serde_json::to_vec(&payload).unwrap(), &f.paths, &|k| if k == "ZYGO_SESSION_DIR" { sd.clone() } else { None }, zygo)
}

fn ws_with(f: &Fx, zygo: &Path, event: &str, payload: Value) -> anyhow::Result<Option<String>> {
    ws_env(f, event, payload, zygo, Some(f.dir.root.clone().into_os_string()))
}

fn ws(f: &Fx, event: &str, payload: Value) -> anyhow::Result<Option<String>> {
    ws_with(f, &f.zygo, event, payload)
}

/// A registered `/zygo:` dispatcher, as the `zygo` binary registers one before any hook runs. The test process
/// registers none, so `ws` and `ws_bg` run with no dispatcher (R1).
fn dispatcher(_: &str, _: &str) -> anyhow::Result<String> {
    Ok(String::new())
}

/// `ws` with the dispatcher `d` (R1).
fn ws_d(f: &Fx, event: &str, payload: Value, d: Option<SlashDispatch>) -> anyhow::Result<Option<String>> {
    let sd = Some(f.dir.root.clone().into_os_string());
    workspace::run_with(event, &serde_json::to_vec(&payload).unwrap(), &f.paths, &|k| if k == "ZYGO_SESSION_DIR" { sd.clone() } else { None }, &f.zygo, d)
}

/// `ws` in a thread: the answer must come within `S5`, so a hook that waits fails the test instead of hanging it.
fn ws_bg(f: &Fx, event: &str, payload: Value) -> anyhow::Result<Option<String>> {
    ws_bg_d(f, event, payload, None)
}

/// `ws_bg` with the dispatcher `d` (R1).
fn ws_bg_d(f: &Fx, event: &str, payload: Value, d: Option<SlashDispatch>) -> anyhow::Result<Option<String>> {
    let (paths, root, zygo, event) = (f.paths.clone(), f.dir.root.clone(), f.zygo.clone(), event.to_string());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let sd = Some(root.into_os_string());
        let env = move |k: &str| if k == "ZYGO_SESSION_DIR" { sd.clone() } else { None };
        let _ = tx.send(workspace::run_with(&event, &serde_json::to_vec(&payload).unwrap(), &paths, &env, &zygo, d));
    });
    rx.recv_timeout(S5).unwrap_or_else(|_| panic!("the hook waited"))
}

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/claude-code/hooks/{name}.json"));
    serde_json::from_slice(&fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

fn with(mut v: Value, key: &str, val: Value) -> Value {
    v.as_object_mut().unwrap().insert(key.into(), val);
    v
}

fn without(mut v: Value, key: &str) -> Value {
    v.as_object_mut().unwrap().remove(key);
    v
}

fn oc(id: &str) -> Value {
    json!({"sessionID": id, "cwd": "/repo"})
}

fn hook(agent: Agent, v: &Value) -> HookInput {
    HookInput::parse(agent, &serde_json::to_vec(v).unwrap()).unwrap()
}

fn ss(id: &str) -> HookInput {
    hook(Agent::ClaudeCode, &with(fixture("SessionStart-main"), "session_id", json!(id)))
}

fn oc_start() -> HookInput {
    hook(Agent::OpenCode, &json!({"input": {"model": {"providerID": "zygo", "id": "lead"}}, "output": {"system": []}}))
}

fn context(v: &Value) -> String {
    v["hookSpecificOutput"]["additionalContext"].as_str().expect("no additionalContext").to_string()
}

/// The session text that `session_start::run_member` gave, whether it printed it (a Claude Code member) or returned it.
fn ss_text(f: &Fx, input: &HookInput, zygo: &Path) -> String {
    let mut out = Vec::new();
    match session_start::run_member(input, &f.dir, &f.paths, zygo, &mut out).unwrap() {
        HookOutput::None => {
            assert!(!out.is_empty(), "nothing printed");
            context(&serde_json::from_slice(&out).unwrap())
        }
        HookOutput::Json(v) => {
            assert!(out.is_empty());
            context(&v)
        }
        HookOutput::Text(t) => {
            assert!(out.is_empty());
            t
        }
    }
}

fn base(f: &Fx) -> String {
    session_text::build(&SessionView::load(&f.dir).unwrap())
}

fn view_note(f: &Fx) -> Option<ResumeNote> {
    SessionView::load(&f.dir).unwrap().workspace.unwrap().resume_note
}

fn sub(id: &str, ty: &str) -> SubagentRef {
    SubagentRef { agent_id: id.into(), agent_type: ty.into(), ..Default::default() }
}

fn rn(clean_stop: bool, interrupted: Vec<SubagentRef>, units: &[&str]) -> ResumeNote {
    ResumeNote { clean_stop, interrupted, closed_units: units.iter().map(|u| u.to_string()).collect(), agent_session: None, ..Default::default() }
}

/// `n` bound to the agent session `id` (W§9.4, H1 D13).
fn bound(n: ResumeNote, id: &str) -> ResumeNote {
    ResumeNote { agent_session: Some(id.to_string()), ..n }
}

/// A Claude Code `UserPromptSubmit` payload with the prompt `p`.
fn ups(p: &str) -> Value {
    with(fixture("UserPromptSubmit-main"), "prompt", json!(p))
}

/// The values that `down` saves and the commit clears (AU60, H1 D8): units, the count of interrupted subagents,
/// `clean_stop`, `saved_for` and `note_pending`.
fn saved(m: &MemberRecord) -> (Vec<String>, usize, bool, Option<String>, bool) {
    (m.closed_units.clone(), m.interrupted.len(), m.clean_stop, m.saved_for.clone(), m.note_pending)
}

/// The note of `I1` and `U12`.
fn note_iu() -> ResumeNote {
    rn(true, vec![sub("a1", "Explore")], &["u-1", "u-2"])
}

fn wait_for(p: &Path) {
    let end = Instant::now() + S5;
    while !p.exists() {
        assert!(Instant::now() < end, "{} did not appear", p.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn act(f: &Fx) -> Activity {
    activity::read(&f.dir).unwrap()
}

fn bt(f: &Fx) -> (bool, u32) {
    let a = act(f);
    (a.busy, a.tools_running)
}

fn set_act(f: &Fx, busy: bool, tools: u32) {
    activity::update(&f.dir, S5, &mut |a| {
        a.busy = busy;
        a.tools_running = tools;
    })
    .unwrap();
}

fn rec_pending() -> MemberRecord {
    let mut m = MemberRecord::new(Agent::ClaudeCode);
    m.state = MemberState::Resumable;
    m.zygo_session = Some(OLD.into());
    m.agent_session = Some("old-id".into());
    m.pending = Some(Pending { zygo_session: SID.into(), agent_session: None, started_at: 7, ..Default::default() });
    m.clean_stop = true;
    m.closed_units = vec!["u-1".into()];
    m.interrupted = vec![serde_json::from_value::<Subagent>(json!({"agent_id": "a1", "agent_type": "Explore"})).unwrap()];
    m.note_pending = true;
    m
}

fn ws_state_with(f: &Fx, phase: Phase, members: BTreeMap<String, MemberRecord>) -> Store {
    let store = Store::open(&f.paths, ID);
    let t = f.t.path().to_path_buf();
    store
        .update(S5, &mut |w| {
            w.name = "app".into();
            w.tmux_session = "zygo-app".into();
            w.common_dir = t.join("app/.git");
            w.main_root = t.join("app");
            w.loadout = "solo".into();
            w.phase = phase;
            w.members = members.clone();
            Ok(())
        })
        .unwrap();
    store
}

fn ws_state(f: &Fx, phase: Phase, rec: MemberRecord) -> Store {
    ws_state_with(f, phase, BTreeMap::from([(KEY.to_string(), rec)]))
}

fn set_phase(s: &Store, p: Phase) {
    s.update(S5, &mut |w| {
        w.phase = p;
        Ok(())
    })
    .unwrap();
}

fn member(f: &Fx) -> MemberRecord {
    Store::open(&f.paths, ID).read().unwrap().unwrap().members[KEY].clone()
}

fn raw(f: &Fx) -> Vec<u8> {
    fs::read(f.paths.workspaces_dir().join(ID).join("workspace.json")).unwrap()
}

/// The lines of `errors.log` of the workspace, each without its `<secs> ` head.
fn errors(f: &Fx) -> Vec<String> {
    let Ok(text) = fs::read_to_string(f.paths.workspaces_dir().join(ID).join("errors.log")) else { return Vec::new() };
    text.lines().map(|l| l.split_once(' ').map_or(l, |(_, rest)| rest).to_string()).collect()
}

/// The lines of the session log.
fn slog(f: &Fx) -> Vec<String> {
    fs::read_to_string(f.dir.log()).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default()
}

/// The stub zygo program: it records its arguments and its pid, then waits.
fn rec(t: &Path) -> PathBuf {
    let p = t.join("zygo");
    let d = t.display();
    fs::write(&p, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" > '{d}/child.args.tmp' && mv '{d}/child.args.tmp' '{d}/child.args'\nprintf '%s\\n' \"$$\" > '{d}/child.pid.tmp' && mv '{d}/child.pid.tmp' '{d}/child.pid'\nexec sleep 30\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn started(t: &Path) -> i32 {
    let p = t.join("child.pid");
    wait_for(&p);
    fs::read_to_string(&p).unwrap().trim().parse().unwrap()
}

fn child_args(t: &Path) -> String {
    fs::read_to_string(t.join("child.args")).unwrap()
}

fn reap(pid: i32) {
    // SAFETY: `pid` is a child of this process that the stub started; kill and waitpid take plain values.
    unsafe {
        libc::kill(pid, libc::SIGKILL);
        libc::waitpid(pid, std::ptr::null_mut(), 0);
    }
}

fn clear_child(t: &Path) {
    let _ = fs::remove_file(t.join("child.args"));
    let _ = fs::remove_file(t.join("child.pid"));
}

/// A lock held by a producer's own function in a thread until the drop.
struct Held {
    release: Option<mpsc::Sender<()>>,
    join: Option<JoinHandle<()>>,
}

impl Drop for Held {
    fn drop(&mut self) {
        if let Some(r) = self.release.take() {
            let _ = r.send(());
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Restores the mode of a folder when the test ends, also when it fails, so the temporary folder can be removed.
struct Mode(PathBuf, u32);

impl Drop for Mode {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(self.1));
    }
}

fn hold_activity(dir: &SessionDir) -> Held {
    let dir = dir.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release, wait) = mpsc::channel::<()>();
    let join = std::thread::spawn(move || {
        activity::update(&dir, S5, &mut |_| {
            let _ = ready_tx.send(());
            let _ = wait.recv();
        })
        .unwrap();
    });
    ready_rx.recv_timeout(S5).expect("activity.lock not taken");
    Held { release: Some(release), join: Some(join) }
}

fn hold_state(s: &Store) -> Held {
    let s = s.clone();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release, wait) = mpsc::channel::<()>();
    let join = std::thread::spawn(move || {
        s.update(S5, &mut |_| {
            let _ = ready_tx.send(());
            let _ = wait.recv();
            Ok(())
        })
        .unwrap();
    });
    ready_rx.recv_timeout(S5).expect("lock not taken");
    Held { release: Some(release), join: Some(join) }
}

fn mkfifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo {}", path.display());
}

fn is_fifo(p: &Path) -> bool {
    fs::symlink_metadata(p).unwrap().file_type().is_fifo()
}

/// The count of opens of `dir/name` while `f` runs, seen by inotify.
#[cfg(target_os = "linux")]
fn opens(dir: &Path, name: &str, f: impl FnOnce()) -> usize {
    opened(dir, f).iter().filter(|n| *n == name).count()
}

/// The name of each open in `dir` while `f` runs, seen by inotify; an open of `dir` itself is `.`.
#[cfg(target_os = "linux")]
fn opened(dir: &Path, f: impl FnOnce()) -> Vec<String> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).unwrap();
    // SAFETY: plain inotify calls on a descriptor this function owns; each event is read from the buffer the kernel
    // filled, with `read_unaligned` for the header and the name length the header gives.
    unsafe {
        let fd = libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC);
        assert!(fd >= 0, "inotify_init1");
        assert!(libc::inotify_add_watch(fd, c.as_ptr(), libc::IN_OPEN) >= 0, "inotify_add_watch");
        f();
        let mut names = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let r = libc::read(fd, buf.as_mut_ptr().cast(), buf.len());
            if r <= 0 {
                break; // EAGAIN: no event left
            }
            let mut off = 0usize;
            while off < r as usize {
                let ev = std::ptr::read_unaligned(buf.as_ptr().add(off).cast::<libc::inotify_event>());
                let start = off + std::mem::size_of::<libc::inotify_event>();
                let raw_name = &buf[start..start + ev.len as usize];
                let name = raw_name.split(|b| *b == 0).next().unwrap_or_default();
                names.push(if name.is_empty() { ".".to_string() } else { String::from_utf8_lossy(name).into_owned() });
                off = start + ev.len as usize;
            }
        }
        libc::close(fd);
        names
    }
}

/// The sorted entry names of `dir`.
fn entries(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

#[test] // W§9.1 table, AU43, AU57, D4, D5
fn claude_code_events_change_busy_and_the_tool_count() {
    let _s = serial();
    let f = fx(Agent::ClaudeCode);
    for (ev, name, n) in [
        ("pre-tool", "PreToolUse-Bash-main", 1),
        ("pre-tool", "PreToolUse-Read-main", 2),
        ("tool-end", "PostToolUse-Bash-main", 1),
        ("tool-end", "PostToolUseFailure-Read-main", 0),
        ("tool-end", "PostToolUse-Read-main", 0),
    ] {
        assert_eq!(ws(&f, ev, fixture(name)).unwrap(), None, "{name}");
        assert_eq!(bt(&f), (false, n), "{name}"); // never below 0
    }
    set_act(&f, true, 2);
    assert_eq!(ws(&f, "stop", fixture("Stop-main")).unwrap(), None);
    assert_eq!(bt(&f), (false, 0));
    set_act(&f, true, 2);
    ws(&f, "stop", with(fixture("Stop-main"), "hook_event_name", json!("StopFailure"))).unwrap();
    assert_eq!(bt(&f), (false, 0));
    set_act(&f, true, 3);
    assert_eq!(ws(&f, "session-end", json!({"session_id": AS1, "hook_event_name": "SessionEnd", "reason": "other"})).unwrap(), None);
    assert_eq!(bt(&f), (true, 0)); // SessionEnd leaves busy
    set_act(&f, true, 1);
    for (ev, v) in [
        ("pre-tool", fixture("PreToolUse-Bash-sub")),
        ("tool-end", fixture("PostToolUse-Bash-sub")),
        ("stop", with(fixture("Stop-main"), "agent_id", json!("a1"))),
        ("session-end", json!({"agent_id": "a1", "hook_event_name": "SessionEnd"})),
    ] {
        assert_eq!(ws(&f, ev, v).unwrap(), None, "{ev}");
        assert_eq!(bt(&f), (true, 1), "{ev}");
    }
    ws(&f, "subagent-start", fixture("SubagentStart-sub")).unwrap();
    assert_eq!(act(&f).running, BTreeMap::from([("a1a80dd100f197242".to_string(), "claude".to_string())]));
    assert_eq!(bt(&f), (true, 1));
    ws(&f, "subagent-stop", fixture("SubagentStop-internal-sub")).unwrap(); // a47a5756291c7f553, never started (PW7 7.5)
    assert_eq!(act(&f).running.len(), 1);
    ws(&f, "subagent-stop", fixture("SubagentStop-sub")).unwrap();
    assert!(act(&f).running.is_empty());
    assert_eq!(bt(&f), (true, 1));
}

#[test] // W§18 #86, AU27, AU104, D5
fn opencode_turn_signals() {
    let _s = serial();
    let f = fx(Agent::OpenCode);
    assert_eq!(ws(&f, "prompt", oc("ses_A")).unwrap(), None);
    assert_eq!(bt(&f), (true, 0));
    set_act(&f, true, 2);
    ws(&f, "prompt", oc("ses_A")).unwrap();
    assert_eq!(bt(&f), (true, 0));
    assert_eq!(ws(&f, "stop", oc("ses_A")).unwrap(), None);
    assert_eq!(bt(&f), (false, 0));
    set_act(&f, true, 2);
    assert_eq!(ws(&f, "session", oc("ses_A")).unwrap(), None);
    assert_eq!(bt(&f), (true, 2)); // an open turn stays busy until its stop (F2)
    assert_eq!(ws(&f, "session", json!({"cwd": "/repo"})).unwrap(), None);
    assert_eq!(bt(&f), (true, 2));
    assert!(!f.paths.workspaces_dir().join(ID).exists()); // no state: no record write, no folder made
    set_act(&f, false, 0);
    let task = json!({"sessionID": "ses_A", "tool_name": "Agent", "cwd": "/repo"});
    assert_eq!(ws(&f, "pre-tool", task).unwrap(), None);
    assert_eq!(bt(&f), (false, 1));
    ws(&f, "tool-end", oc("ses_A")).unwrap();
    assert_eq!(bt(&f), (false, 0));
}

#[test] // W§9.1 `UserPromptSubmit`, W§9.2, AU83, AU86, D4
fn claude_code_prompt_entry_sets_busy_and_blocks_while_stopping() {
    let _s = serial();
    let f = fx(Agent::ClaudeCode);
    assert_eq!(ws(&f, "prompt", ups("hello")).unwrap(), None);
    assert_eq!(bt(&f), (true, 0));
    let store = ws_state(&f, Phase::Stopping, MemberRecord::new(Agent::ClaudeCode));
    set_act(&f, false, 0);
    let op = store.op_lock().unwrap();
    let Some(t) = ws(&f, "prompt", ups("hello")).unwrap() else { panic!("no block") };
    assert_eq!(serde_json::from_str::<Value>(&t).unwrap(), json!({"decision": "block", "reason": "zygo: workspace is stopping"}));
    assert_eq!(bt(&f), (false, 0)); // a blocked prompt starts no turn
    drop(op);
    assert_eq!(ws(&f, "prompt", ups("hello")).unwrap(), None);
    assert_eq!(bt(&f), (true, 0));
    set_phase(&store, Phase::Draining);
    set_act(&f, false, 0);
    let _op = store.op_lock().unwrap();
    assert_eq!(ws(&f, "prompt", ups("hello")).unwrap(), None);
    assert_eq!(bt(&f), (true, 0)); // draining blocks no prompt
    let o = fx(Agent::OpenCode);
    let st = ws_state(&o, Phase::Stopping, MemberRecord::new(Agent::OpenCode));
    let _op2 = st.op_lock().unwrap();
    assert_eq!(ws(&o, "prompt", oc("ses_A")).unwrap(), None); // AU86: an OpenCode prompt is never blocked
    assert_eq!(bt(&o), (true, 0));
}

#[test] // W§9.1 `UserPromptSubmit`, D4; H1 attempt-2 review round 2 R1
#[ignore = "timing: waits at most 5 s for the hook on a helper thread"]
fn slash_line_sets_busy_unless_the_dispatcher_consumes_it() {
    let _s = serial();
    let d = Some(dispatcher as SlashDispatch);
    // The one rule of both hooks: a `/zygo:` line (trailing whitespace trimmed) with a registered dispatcher.
    for p in ["/zygo:status", "/zygo:status  \n", "/zygo:compact 120k", "/zygo:usage", "/zygo:frob"] {
        assert!(prompt::consumes(p, d), "{p:?}");
        assert!(!prompt::consumes(p, None), "{p:?}");
    }
    for p in ["hello", " /zygo:status", "/zygo:", "/zygo:Status"] {
        assert!(!prompt::consumes(p, d), "{p:?}");
    }
    // A line that `zygo hook prompt` consumes runs as a verb (a verb of `VERB_SPECS`, or `help` for an unknown name):
    // no change, so a verb line from a member does not mark that member busy.
    let f = fx(Agent::ClaudeCode);
    set_act(&f, false, 2);
    for p in ["/zygo:status", "/zygo:usage", "/zygo:units on\n", "/zygo:frob"] {
        assert_eq!(ws_d(&f, "prompt", ups(p), d).unwrap(), None, "{p:?}");
        assert_eq!(bt(&f), (false, 2), "{p:?}");
    }
    // A `/zygo:` line with no dispatcher registered reaches the model: its turn is busy until its stop.
    assert_eq!(ws_d(&f, "prompt", ups("/zygo:status"), None).unwrap(), None);
    assert_eq!(bt(&f), (true, 0));
    set_act(&f, false, 2);
    assert_eq!(ws(&f, "prompt", ups("/zygo:usage")).unwrap(), None);
    assert_eq!(bt(&f), (true, 0));
    // A consumed line reads no view; one that passes reads it like any prompt.
    let s = fx(Agent::ClaudeCode);
    fs::remove_file(s.dir.view_json()).unwrap();
    mkfifo(&s.dir.view_json());
    assert_eq!(ws_bg_d(&s, "prompt", ups("/zygo:status"), d).unwrap(), None);
    assert!(slog(&s).is_empty() && is_fifo(&s.dir.view_json()));
    assert_eq!(ws_bg_d(&s, "prompt", ups("/zygo:status"), None).unwrap(), None);
    assert_eq!(slog(&s), [format!("workspace hook prompt: {}: is not a regular file; not read", s.dir.view_json().display())]);
}

/// The `view.json` and marker variants of `view_and_marker_lines_once_per_turn`.
#[derive(Clone, Copy, Debug)]
enum Bad {
    NotJson,
    Empty,
    PaneInt,
    WsIdNull,
    Fifo,
    Link,
    WsId,
    PaneKey,
}

#[test] // AU84, D6; H1 attempt 1 round 1 F3, round 2 R2-2, round 3 R3-3
#[ignore = "timing: waits at most 5 s for the hook on a helper thread"]
fn view_and_marker_lines_once_per_turn() {
    let _s = serial();
    let quiet = [
        ("pre-tool", fixture("PreToolUse-Bash-main")),
        ("tool-end", fixture("PostToolUse-Bash-main")),
        ("stop", fixture("Stop-main")),
        ("subagent-start", fixture("SubagentStart-sub")),
        ("subagent-stop", fixture("SubagentStop-sub")),
    ];
    let loud = [
        ("prompt", ups("hello")),
        ("session", oc("ses_A")),
        ("session-end", json!({"session_id": AS1, "hook_event_name": "SessionEnd", "reason": "other"})),
    ];
    for bad in [Bad::NotJson, Bad::Empty, Bad::PaneInt, Bad::WsIdNull, Bad::Fifo, Bad::Link, Bad::WsId, Bad::PaneKey] {
        let f = match bad {
            Bad::WsId => fx_with(Agent::ClaudeCode, Some(marker("../x", KEY, None))),
            Bad::PaneKey => fx_with(Agent::ClaudeCode, Some(marker(ID, "k$(x)", None))),
            _ => fx(Agent::ClaudeCode),
        };
        let view = f.dir.view_json().display().to_string();
        match bad {
            Bad::NotJson => fs::write(f.dir.view_json(), "{").unwrap(),
            Bad::Empty => fs::write(f.dir.view_json(), "").unwrap(),
            Bad::PaneInt | Bad::WsIdNull => {
                let mut v: Value = serde_json::from_slice(&fs::read(f.dir.view_json()).unwrap()).unwrap();
                if matches!(bad, Bad::PaneInt) {
                    v["workspace"]["member"]["pane"] = json!(5);
                } else {
                    v["workspace"]["member"]["ws_id"] = Value::Null;
                }
                fs::write(f.dir.view_json(), serde_json::to_vec(&v).unwrap()).unwrap();
            }
            Bad::Fifo => {
                fs::remove_file(f.dir.view_json()).unwrap();
                mkfifo(&f.dir.view_json());
            }
            Bad::Link => {
                fs::rename(f.dir.view_json(), f.t.path().join("v.json")).unwrap();
                std::os::unix::fs::symlink(f.t.path().join("v.json"), f.dir.view_json()).unwrap();
            }
            Bad::WsId | Bad::PaneKey => {}
        }
        // The lines of this variant: the pane key goes to `errors.log`, every other line to the session log.
        let lines = |f: &Fx| if matches!(bad, Bad::PaneKey) { errors(f) } else { slog(f) };
        let call = |ev: &str, v: Value| if matches!(bad, Bad::Fifo) { ws_bg(&f, ev, v) } else { ws(&f, ev, v) };
        for (ev, v) in &quiet {
            assert_eq!(call(ev, v.clone()).unwrap(), None, "{bad:?} {ev}");
        }
        assert!(lines(&f).is_empty(), "{bad:?}: {:?}", lines(&f));
        for (n, (ev, v)) in loud.iter().enumerate() {
            assert_eq!(call(ev, v.clone()).unwrap(), None, "{bad:?} {ev}");
            let got = lines(&f);
            assert_eq!(got.len(), n + 1, "{bad:?} {ev}: {got:?}");
            let l = &got[n];
            let head = format!("workspace hook {ev}: ");
            let ok = match bad {
                Bad::NotJson => *l == format!("{head}{view}: EOF while parsing an object at line 1 column 1"),
                Bad::Empty => *l == format!("{head}{view}: EOF while parsing a value at line 1 column 0"),
                Bad::PaneInt => l.starts_with(&format!("{head}{view}: invalid type: integer `5`, expected a string at line 1 column ")),
                Bad::WsIdNull => l.starts_with(&format!("{head}{view}: invalid type: null, expected a string at line 1 column ")),
                Bad::Fifo => *l == format!("{head}{view}: is not a regular file; not read"),
                Bad::Link => *l == format!("{head}{view}: is a link; not read"),
                Bad::WsId => *l == format!("{head}invalid workspace id \"../x\" in the session view"),
                Bad::PaneKey => *l == format!("{head}invalid pane key \"k$(x)\" in the session view"),
            };
            assert!(ok, "{bad:?} {ev}: {l}");
        }
        assert!(!f.dir.activity_json().exists(), "{bad:?}");
        match bad {
            Bad::Fifo => assert!(is_fifo(&f.dir.view_json())),
            Bad::Link => assert!(fs::symlink_metadata(f.dir.view_json()).unwrap().file_type().is_symlink()),
            _ => {}
        }
    }
}

/// `ss` with `--event member`: the Claude Code `SessionStart` entry of a member (D3).
fn ssm(id: &str) -> HookInput {
    HookInput { cli_event: Some("member".into()), ..ss(id) }
}

/// Moves `view.json` of `f` to `<t>/v.json` and puts a link to it in its place.
fn link_view(f: &Fx) {
    fs::rename(f.dir.view_json(), f.t.path().join("v.json")).unwrap();
    std::os::unix::fs::symlink(f.t.path().join("v.json"), f.dir.view_json()).unwrap();
}

#[test] // D3, D6; plan review round 1 minor 1, round 2 F1
#[ignore = "timing: waits at most 5 s for the hook on a helper thread"]
fn session_start_read_errors_member_and_not() {
    let _s = serial();
    // A member `SessionStart` whose view.json is not JSON: one line, no output, no change.
    let f = fx_note();
    let missing = f.t.path().join("missing");
    ws_state(&f, Phase::Up, rec_pending());
    fs::write(f.dir.view_json(), "{").unwrap();
    let mut out = Vec::new();
    assert!(matches!(session_start::run_member(&ssm(AS1), &f.dir, &f.paths, &missing, &mut out).unwrap(), HookOutput::None));
    assert!(out.is_empty());
    let lines = slog(&f);
    assert!(lines.len() == 1 && lines[0].starts_with("workspace hook session-start: "), "{lines:?}");
    assert_eq!(member(&f), rec_pending());
    // A member `SessionStart` whose view.json is a link: one line, the session text with no note, no change.
    let f = fx_note();
    let missing = f.t.path().join("missing");
    ws_state(&f, Phase::Up, rec_pending());
    let view = f.dir.view_json().display().to_string();
    link_view(&f);
    assert_eq!(ss_text(&f, &ssm(AS1), &missing), base(&f));
    assert_eq!(slog(&f), [format!("workspace hook session-start: {view}: is a link; not read")]);
    assert_eq!(member(&f), rec_pending());
    assert_eq!(view_note(&f), Some(note_iu())); // the copy still holds the note
    // A member SessionStart whose view.json is a FIFO (FIX-22): no wait, one line, no output, no change.
    let f = fx_note();
    let missing = f.t.path().join("missing");
    ws_state(&f, Phase::Up, rec_pending());
    fs::remove_file(f.dir.view_json()).unwrap();
    mkfifo(&f.dir.view_json());
    let (dir, paths, miss) = (f.dir.clone(), f.paths.clone(), missing.clone());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let r = session_start::run_member(&ssm(AS1), &dir, &paths, &miss, &mut out);
        let _ = tx.send((matches!(r, Ok(HookOutput::None)), out.is_empty()));
    });
    assert_eq!(rx.recv_timeout(S5).expect("the hook waited"), (true, true));
    assert_eq!(slog(&f), [format!("workspace hook session-start: {}: is a FIFO", f.dir.view_json().display())]);
    assert_eq!(member(&f), rec_pending());
    assert!(is_fifo(&f.dir.view_json()));
    // A launch that is not a member: the output or the error of `run_in`, and no line.
    for linked in [false, true] {
        let g = fx_with(Agent::ClaudeCode, None);
        if linked {
            link_view(&g);
        } else {
            fs::write(g.dir.view_json(), "{").unwrap();
        }
        let mut out = Vec::new();
        let a = session_start::run_member(&ss(AS1), &g.dir, &g.paths, &g.zygo, &mut out);
        let b = session_start::run_in(&ss(AS1), &g.dir);
        match (a, b) {
            (Ok(a), Ok(b)) => assert_eq!(a.to_stdout(), b.to_stdout(), "linked {linked}"),
            (Err(a), Err(b)) => assert_eq!(format!("{a:#}"), format!("{b:#}"), "linked {linked}"),
            (a, b) => panic!("linked {linked}: run_member ok {}, run_in ok {}", a.is_ok(), b.is_ok()),
        }
        assert!(out.is_empty() && !g.dir.log().exists(), "linked {linked}");
    }
    // The OpenCode per-request call (no flag) on a member view that is a link: the output of a regular view, no line.
    let f = fx_with(Agent::OpenCode, Some(marker(ID, KEY, Some(note_iu()))));
    let g = fx_with(Agent::OpenCode, Some(marker(ID, KEY, Some(note_iu()))));
    link_view(&f);
    let (missing, gm) = (f.t.path().join("missing"), g.t.path().join("missing"));
    assert_eq!(ss_text(&f, &oc_start(), &missing), ss_text(&g, &oc_start(), &gm));
    assert!(!f.dir.log().exists());
    assert!(fs::symlink_metadata(f.dir.view_json()).unwrap().file_type().is_symlink());
}

#[test] // W§9.2, AU83, W§18 #44 parts 1 and 3, D8; controller decision 4
#[ignore = "timing: waits at most 5 s for the hook on a helper thread"]
fn agent_dispatch_blocks_only_while_down_holds_the_lock() {
    let _s = serial();
    let f = fx(Agent::ClaudeCode);
    let store = ws_state(&f, Phase::Draining, MemberRecord::new(Agent::ClaudeCode));
    let agent = fixture("PreToolUse-Agent-main");
    let op = store.op_lock().unwrap();
    assert_eq!(parsed(ws(&f, "pre-tool", agent.clone())), deny(workspace::DRAINING));
    assert_eq!(bt(&f), (false, 0));
    assert_eq!(parsed(ws(&f, "pre-tool", with(agent.clone(), "agent_id", json!("a1")))), deny(workspace::DRAINING));
    assert_eq!(bt(&f), (false, 0));
    fs::write(&f.paths.config_file, "[workspace]\ndrain_block_dispatch = false\n").unwrap();
    assert_eq!(ws(&f, "pre-tool", agent.clone()).unwrap(), None);
    assert_eq!(bt(&f), (false, 1));
    fs::write(&f.paths.config_file, "[workspace]\ndrain_block_dispatch = \"yes\"\n").unwrap(); // a config error: the default
    assert_eq!(parsed(ws(&f, "pre-tool", agent.clone())), deny(workspace::DRAINING));
    assert_eq!(bt(&f), (false, 1));
    fs::remove_file(&f.paths.config_file).unwrap();
    set_phase(&store, Phase::Stopping);
    assert_eq!(parsed(ws(&f, "pre-tool", agent.clone())), deny(workspace::STOPPING));
    assert_eq!(bt(&f), (false, 1));
    drop(op);
    assert_eq!(ws(&f, "pre-tool", agent.clone()).unwrap(), None);
    assert_eq!(bt(&f), (false, 2));
    set_phase(&store, Phase::Up);
    let op = store.op_lock().unwrap();
    assert_eq!(ws(&f, "pre-tool", agent.clone()).unwrap(), None);
    assert_eq!(bt(&f), (false, 3)); // held, phase up: no deny
    set_phase(&store, Phase::Draining);
    assert_eq!(ws(&f, "pre-tool", fixture("PreToolUse-Bash-main")).unwrap(), None);
    assert_eq!(bt(&f), (false, 4)); // only Agent is denied
    drop(op);
    // Phase `up` never opens `op.lock`; the watch sees the open of phase `stopping` (its positive control).
    #[cfg(target_os = "linux")]
    {
        set_phase(&store, Phase::Up);
        assert_eq!(opens(store.dir(), "op.lock", || assert_eq!(ws(&f, "pre-tool", agent.clone()).unwrap(), None)), 0);
        set_phase(&store, Phase::Stopping);
        assert!(opens(store.dir(), "op.lock", || assert_eq!(ws(&f, "pre-tool", agent.clone()).unwrap(), None)) >= 1);
    }
    // An `op.lock` that is a FIFO: phase `up` gives no deny and no wait.
    fs::remove_file(store.dir().join("op.lock")).unwrap();
    mkfifo(&store.dir().join("op.lock"));
    set_phase(&store, Phase::Up);
    assert_eq!(ws_bg(&f, "pre-tool", agent.clone()).unwrap(), None);
    assert!(is_fifo(&store.dir().join("op.lock")));
}

/// The `down` stand-in of `killed_down_releases_the_block_at_once`: it holds `op.lock` until it is killed. In a normal
/// run it returns at once.
#[test]
fn op_lock_holder() {
    let Some(t) = std::env::var_os("ZYGO_H1_HOLDER") else { return };
    let _op = Store::open(&paths(Path::new(&t)), ID).op_lock().unwrap();
    fs::write(Path::new(&t).join("held"), "").unwrap();
    std::thread::sleep(Duration::from_secs(600));
}

#[test] // W§18 #44 part 2, AU83
#[ignore = "timing: waits at most 5 s for the child test process to take the lock"]
fn killed_down_releases_the_block_at_once() {
    let _s = serial();
    let f = fx(Agent::ClaudeCode);
    let store = ws_state(&f, Phase::Draining, MemberRecord::new(Agent::ClaudeCode));
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "op_lock_holder", "--nocapture", "--test-threads=1"])
        .env("ZYGO_H1_HOLDER", f.t.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for(&f.t.path().join("held"));
    let agent = fixture("PreToolUse-Agent-main");
    assert_eq!(parsed(ws(&f, "pre-tool", agent.clone())), deny(workspace::DRAINING));
    child.kill().unwrap(); // SIGKILL
    assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGKILL));
    assert_eq!(ws(&f, "pre-tool", agent).unwrap(), None); // the next call, no retry and no wait
    assert_eq!(store.phase_unlocked(), Some(Phase::Draining)); // the hook writes no phase
}

#[test] // AU92 as amended by FIX-20, AU60 order, W§7.4 trigger, D10, D12, D14, D15; controller decisions 6, 9, 13
fn view_rewrites_keep_unknown_fields() {
    // Attempt-2 review round 1, F4: `clear_note` and `bind_note` read, change and save the view; a field of a newer
    // zygo stays (AU78 pattern).
    let _s = serial();
    // Also inside `workspace`, `member`, `resume_note` and its subagents (R5).
    let add_field = |f: &Fx| {
        let mut v: Value = serde_json::from_slice(&fs::read(f.dir.view_json()).unwrap()).unwrap();
        v["future_field"] = json!({"kept": [1, 2]});
        v["workspace"]["future_w"] = json!(1);
        v["workspace"]["member"]["future_m"] = json!("m");
        v["workspace"]["resume_note"]["future_n"] = json!([3]);
        v["workspace"]["resume_note"]["interrupted"][0]["future_s"] = json!(false);
        fs::write(f.dir.view_json(), serde_json::to_vec(&v).unwrap()).unwrap();
    };
    let doc = |f: &Fx| -> Value { serde_json::from_slice::<Value>(&fs::read(f.dir.view_json()).unwrap()).unwrap() };
    let field = |f: &Fx| -> Value {
        let v = doc(f);
        assert_eq!((&v["workspace"]["future_w"], &v["workspace"]["member"]["future_m"]), (&json!(1), &json!("m")));
        v["future_field"].clone()
    };
    let note_fields = |f: &Fx| -> (Value, Value) {
        let v = doc(f);
        (v["workspace"]["resume_note"]["future_n"].clone(), v["workspace"]["resume_note"]["interrupted"][0]["future_s"].clone())
    };
    // Claude Code SessionStart: the note is cleared (clear_note).
    let f = fx_with(Agent::ClaudeCode, Some(marker(ID, KEY, Some(note_iu()))));
    ws_state(&f, Phase::Up, rec_pending());
    add_field(&f);
    let missing = f.t.path().join("missing");
    session_start::run_member(&ss(AS1), &f.dir, &f.paths, &missing, &mut Vec::new()).unwrap();
    assert_eq!(view_note(&f), None);
    assert_eq!(field(&f), json!({"kept": [1, 2]}));
    // OpenCode session: the note is bound (bind_note), then removed for another id.
    let g = fx_with(Agent::OpenCode, Some(marker(ID, KEY, Some(note_iu()))));
    add_field(&g);
    let gm = g.t.path().join("missing");
    ws_with(&g, &gm, "session", oc("ses_A")).unwrap();
    let known = |n: ResumeNote| ResumeNote {
        interrupted: n.interrupted.into_iter().map(|s| SubagentRef { extra: Default::default(), ..s }).collect(),
        extra: Default::default(),
        ..n
    };
    assert_eq!(view_note(&g).map(known), Some(bound(note_iu(), "ses_A")));
    assert_eq!(field(&g), json!({"kept": [1, 2]}));
    assert_eq!(note_fields(&g), (json!([3]), json!(false)));
    ws_with(&g, &gm, "session", oc("ses_B")).unwrap();
    assert_eq!(view_note(&g), None);
    assert_eq!(field(&g), json!({"kept": [1, 2]}));
}

#[test]
#[ignore = "timing: waits at most 5 s for the detached snapshot child to start"]
fn session_start_prints_then_commits_and_clears_once() {
    let _s = serial();
    let f = fx_with(Agent::ClaudeCode, Some(marker(ID, KEY, Some(note_iu()))));
    let store = ws_state(&f, Phase::Up, rec_pending());
    let base_text = base(&f);
    let mut out = Vec::new();
    assert!(matches!(session_start::run_member(&ss(AS1), &f.dir, &f.paths, &f.zygo, &mut out).unwrap(), HookOutput::None)); // printed to out
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v, json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": format!("{base_text}\n\n{I1}\n{U12}")}}));
    let m = member(&f);
    assert_eq!((m.pending.clone(), m.zygo_session.as_deref(), m.agent_session.as_deref(), m.state), (None, Some(SID), Some(AS1), MemberState::Running));
    assert_eq!((m.clean_stop, m.closed_units.len(), m.interrupted.len(), m.note_pending), (false, 0, 0, false));
    assert_eq!(member(&f).saved_for, None);
    assert_eq!(view_note(&f), None);
    let pid = started(f.t.path());
    assert_eq!(child_args(f.t.path()), "ws snapshot --ws-id 0123456789abcdef\n");
    reap(pid);
    clear_child(f.t.path());
    // The same start again, with a zygo program that does not exist: no write, no note, no start.
    let before = raw(&f);
    let missing = f.t.path().join("missing");
    assert_eq!(ss_text(&f, &ss(AS1), &missing), base_text);
    assert_eq!(raw(&f), before);
    assert!(errors(&f).is_empty());
    assert!(!store.dir().join("snapshot.pending").exists());
    // `down` saved values meanwhile; a /clear start with a new id keeps them and writes the id.
    store
        .update(S5, &mut |w| {
            let m = w.members.get_mut(KEY).unwrap();
            m.clean_stop = true;
            m.closed_units = vec!["u-9".into()];
            Ok(())
        })
        .unwrap();
    ss_text(&f, &ss(AS2), &f.zygo);
    let m = member(&f);
    assert_eq!((m.agent_session.as_deref(), m.clean_stop, m.closed_units.clone(), m.state), (Some(AS2), true, vec!["u-9".to_string()], MemberState::Running));
    let pid = started(f.t.path());
    assert_eq!(child_args(f.t.path()), "ws snapshot --ws-id 0123456789abcdef\n");
    reap(pid);
    // A start with no session_id keeps the stored id.
    let before = raw(&f);
    assert_eq!(ss_text(&f, &hook(Agent::ClaudeCode, &without(fixture("SessionStart-main"), "session_id")), &missing), base_text);
    assert_eq!(raw(&f), before);
    assert_eq!(member(&f).agent_session.as_deref(), Some(AS2));
    // A subagent start changes nothing.
    assert!(matches!(
        session_start::run_member(&hook(Agent::ClaudeCode, &fixture("SessionStart-sub")), &f.dir, &f.paths, &missing, &mut Vec::new()).unwrap(),
        HookOutput::None
    ));
    assert_eq!(raw(&f), before);
    // Then each in a fresh fixture with the note `note_iu()` and a zygo program that does not exist.
    first_start_without_an_id_keeps_the_stored_id();
    session_start_refusals();
    // AU60 clear rule (design H1, H3; D8): values saved for another session are cleared, values saved for this one kept.
    for (sf, keep) in [(Some(OLD), false), (Some(SID), true)] {
        let g = fx_note();
        let missing = g.t.path().join("missing");
        ws_state(&g, Phase::Up, MemberRecord { saved_for: sf.map(String::from), ..rec_pending() });
        assert_eq!(ss_text(&g, &ss(AS1), &missing), format!("{}\n\n{I1}\n{U12}", base(&g)), "{sf:?}");
        let m = member(&g);
        assert_eq!((m.pending.clone(), m.zygo_session.as_deref(), m.state), (None, Some(SID), MemberState::Running), "{sf:?}");
        let want = if keep { (vec!["u-1".to_string()], 1, true, Some(SID.to_string()), false) } else { (vec![], 0, false, None, false) };
        assert_eq!(saved(&m), want, "{sf:?}");
        assert_eq!(view_note(&g), None, "{sf:?}"); // AU60: printed, then removed
    }
}

fn fx_note() -> Fx {
    fx_with(Agent::ClaudeCode, Some(marker(ID, KEY, Some(note_iu()))))
}

/// Part of `session_start_prints_then_commits_and_clears_once` (D10; controller decision 6): a first start with no id
/// keeps the stored id.
fn first_start_without_an_id_keeps_the_stored_id() {
    for id in [None, Some(json!("")), Some(json!(7))] {
        let g = fx_note();
        let missing = g.t.path().join("missing");
        let store = ws_state(&g, Phase::Up, rec_pending());
        let payload = match &id {
            None => without(fixture("SessionStart-main"), "session_id"),
            Some(v) => with(fixture("SessionStart-main"), "session_id", v.clone()),
        };
        assert_eq!(ss_text(&g, &hook(Agent::ClaudeCode, &payload), &missing), format!("{}\n\n{I1}\n{U12}", base(&g)), "{id:?}");
        let m = member(&g);
        assert_eq!((m.pending.clone(), m.zygo_session.as_deref(), m.agent_session.as_deref(), m.state), (None, Some(SID), Some("old-id"), MemberState::Running), "{id:?}");
        assert!(errors(&g).is_empty(), "{id:?}");
        assert!(!store.dir().join("snapshot.pending").exists(), "{id:?}"); // the id did not change: no spawn was tried
    }
}

/// Part of `session_start_prints_then_commits_and_clears_once` (D10, D14, D15): another start's record, no record, a
/// print that fails, a view with no marker.
fn session_start_refusals() {
    // Another start's record.
    let g = fx_note();
    let missing = g.t.path().join("missing");
    ws_state(&g, Phase::Up, MemberRecord { pending: Some(Pending { zygo_session: OTHER.into(), agent_session: None, started_at: 7, ..Default::default() }), ..rec_pending() });
    let before = raw(&g);
    assert_eq!(ss_text(&g, &ss(AS1), &missing), format!("{}\n\n{I1}\n{U12}", base(&g)));
    assert_eq!(raw(&g), before);
    assert_eq!(errors(&g), ["workspace hook session-start: member record of pane kaaaaaaaa belongs to another session; not changed"]);
    assert_eq!(view_note(&g), None);
    // No member record.
    let g = fx_note();
    let missing = g.t.path().join("missing");
    ws_state_with(&g, Phase::Up, BTreeMap::new());
    ss_text(&g, &ss(AS1), &missing);
    assert_eq!(errors(&g), ["workspace hook session-start: no member record for pane kaaaaaaaa; not changed"]);
    // A print that fails: nothing written, so the next start is still the first one.
    struct Closed;
    impl std::io::Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let g = fx_note();
    let missing = g.t.path().join("missing");
    ws_state(&g, Phase::Up, rec_pending());
    assert!(session_start::run_member(&ss(AS1), &g.dir, &g.paths, &missing, &mut Closed).is_err());
    assert_eq!(member(&g), rec_pending());
    assert_eq!(view_note(&g), Some(note_iu()));
    // A view with no marker: today's output, and nothing else.
    let g = fx_with(Agent::ClaudeCode, None);
    let mut out = Vec::new();
    let got = session_start::run_member(&ss(AS1), &g.dir, &g.paths, &g.zygo, &mut out).unwrap();
    assert_eq!(got.to_stdout(), session_start::run_in(&ss(AS1), &g.dir).unwrap().to_stdout());
    assert!(out.is_empty());
    assert!(!g.paths.state_dir().exists());
}

/// The hook commit of a pending start by `agent` (T§3.2): the Claude Code `SessionStart` with no `session_id`, or the
/// OpenCode `session` call with only `cwd`. The zygo program does not exist and the agent id does not change, so no
/// snapshot child starts.
fn commit_by_hook(f: &Fx, agent: Agent) {
    let missing = f.t.path().join("missing");
    match agent {
        Agent::ClaudeCode => {
            let input = hook(Agent::ClaudeCode, &without(fixture("SessionStart-main"), "session_id"));
            session_start::run_member(&input, &f.dir, &f.paths, &missing, &mut Vec::new()).unwrap();
        }
        Agent::OpenCode => {
            ws_with(f, &missing, "session", json!({"cwd": "/repo"})).unwrap();
        }
    }
}

/// `members.<KEY>` of `workspace.json` as JSON.
fn member_json(f: &Fx) -> Value {
    serde_json::from_slice::<Value>(&raw(f)).unwrap()["members"][KEY].clone()
}

#[test] // T§3.2 (W AU92 addition), T§5 "Fresh member", T§7 item 4
fn first_started_at_is_set_by_the_hook_commit_of_each_agent() {
    let _s = serial();
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let f = fx(agent);
        ws_state(&f, Phase::Up, rec_pending()); // pending SID, started_at 7; no first_started_at
        commit_by_hook(&f, agent);
        let m = member(&f);
        assert_eq!((m.pending.clone(), m.zygo_session.as_deref(), m.first_started_at), (None, Some(SID), Some(7)), "{agent:?}");
        assert_eq!(member_json(&f)["first_started_at"], json!(7), "{agent:?}");
        assert!(errors(&f).is_empty(), "{agent:?}");
    }
}

#[test] // T§3.2 "never changed after it is set", T§5 "`zygo down`, then `zygo up`", T§7 item 4
fn first_started_at_is_kept_by_a_later_hook_commit() {
    let _s = serial();
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let f = fx(agent);
        ws_state(&f, Phase::Up, MemberRecord { first_started_at: Some(3), ..rec_pending() }); // pending started_at 7
        commit_by_hook(&f, agent);
        let m = member(&f);
        assert_eq!((m.pending.clone(), m.zygo_session.as_deref(), m.first_started_at), (None, Some(SID), Some(3)), "{agent:?}");
    }
}

#[test] // T§2.2 (0 is unknown), T§3.2, T§7 item 4; graph review focus 60
fn first_started_at_stays_absent_for_a_zero_start_until_a_later_nonzero_hook_commit() {
    let _s = serial();
    let zero = |at| MemberRecord { pending: Some(Pending { zygo_session: SID.into(), agent_session: None, started_at: at, ..Default::default() }), ..rec_pending() };
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let f = fx(agent);
        let store = ws_state(&f, Phase::Up, zero(0));
        commit_by_hook(&f, agent);
        let m = member(&f);
        assert_eq!((m.pending.clone(), m.zygo_session.as_deref(), m.first_started_at), (None, Some(SID), None), "{agent:?}");
        assert!(member_json(&f).get("first_started_at").is_none(), "{agent:?}");
        // a later start with a non-zero time: its commit sets the field
        store.update(S5, &mut |w| {
            w.members.get_mut(KEY).unwrap().pending = Some(Pending { zygo_session: SID.into(), agent_session: None, started_at: 11, ..Default::default() });
            Ok(())
        }).unwrap();
        commit_by_hook(&f, agent);
        assert_eq!(member(&f).first_started_at, Some(11), "{agent:?}");
        // a stored 0 counts as absent (T§3.2 "absent or 0")
        let g = fx(agent);
        ws_state(&g, Phase::Up, MemberRecord { first_started_at: Some(0), ..rec_pending() });
        commit_by_hook(&g, agent);
        assert_eq!(member(&g).first_started_at, Some(7), "{agent:?}");
    }
}

#[test] // W§9.4, AU42, AU104, D14, D15; controller decision 1
fn resume_note_texts_and_agents() {
    let _s = serial();
    assert_eq!(workspace::note(&rn(false, vec![sub("a1", "Explore")], &["u-1"])).as_deref(), Some(CRASH));
    assert_eq!(
        workspace::note(&rn(true, vec![sub("a2", ""), sub("a1", "Explore")], &[])).as_deref(),
        Some("zygo stopped this session while subagents a2, Explore a1 were running; their results were lost; dispatch them again if still needed.")
    );
    assert_eq!(workspace::note(&rn(true, vec![], &["u-1"])).as_deref(), Some(U1));
    assert_eq!(workspace::note(&rn(true, vec![], &[])), None);
    assert_eq!(workspace::note(&note_iu()).as_deref(), Some(format!("{I1}\n{U12}").as_str()));
    assert_eq!(
        workspace::note(&rn(true, vec![sub("a\u{1b}[2J", "T\n")], &["u\u{7}"])).as_deref(),
        Some("zygo stopped this session while subagents T\\n a\\u{1b}[2J were running; their results were lost; dispatch them again if still needed.\nUnits u\\u{07} were closed when zygo stopped this session; call work_begin to open them again if work continues.")
    );
    // The no-note case.
    let f = fx_with(Agent::ClaudeCode, Some(marker(ID, KEY, Some(rn(true, vec![], &[])))));
    let missing = f.t.path().join("missing");
    ws_state(&f, Phase::Up, rec_pending());
    assert_eq!(ss_text(&f, &ss(AS1), &missing), base(&f));
    assert_eq!(view_note(&f), None);
    // The crash case.
    let f = fx_with(Agent::ClaudeCode, Some(marker(ID, KEY, Some(rn(false, vec![], &[])))));
    let missing = f.t.path().join("missing");
    ws_state(&f, Phase::Up, rec_pending());
    assert_eq!(ss_text(&f, &ss(AS1), &missing), format!("{}\n\n{CRASH}", base(&f)));
    // Claude Code: a second call prints no note (it was cleared after it printed).
    let f = fx_note();
    let missing = f.t.path().join("missing");
    ws_state(&f, Phase::Up, rec_pending());
    assert_eq!(ss_text(&f, &ss(AS1), &missing), format!("{}\n\n{I1}\n{U12}", base(&f)));
    assert_eq!(ss_text(&f, &ss(AS1), &missing), base(&f));
    // OpenCode: the note is in every request; the `session` call owns the record.
    let f = fx_with(Agent::OpenCode, Some(marker(ID, KEY, Some(note_iu()))));
    let missing = f.t.path().join("missing");
    let store = ws_state(&f, Phase::Up, rec_pending());
    assert_eq!(ss_text(&f, &oc_start(), &missing), format!("{}\n\n{I1}\n{U12}", base(&f)));
    assert_eq!(ss_text(&f, &oc_start(), &missing), format!("{}\n\n{I1}\n{U12}", base(&f)));
    assert_eq!(view_note(&f), Some(note_iu()));
    assert_eq!(member(&f), rec_pending());
    assert!(errors(&f).is_empty());
    assert!(!store.dir().join("snapshot.pending").exists()); // no record write, so no spawn was tried
}

#[test] // AU60, AU92, AU104, design H1, H2, H4-H7, H10; D5, D8, D9
#[ignore = "timing: waits at most 5 s for the detached snapshot child to start"]
fn opencode_session_commits_and_records() {
    let _s = serial();
    // H1: a call with no id commits and clears old values; H10: no activity change
    let f = fx(Agent::OpenCode);
    let missing = f.t.path().join("missing");
    let store = ws_state(&f, Phase::Up, MemberRecord { saved_for: Some(OLD.into()), ..rec_pending() });
    set_act(&f, true, 2);
    assert_eq!(ws_with(&f, &missing, "session", json!({"cwd": "/repo"})).unwrap(), None);
    let m = member(&f);
    assert_eq!((m.pending.clone(), m.zygo_session.as_deref(), m.agent_session.as_deref(), m.state), (None, Some(SID), Some("old-id"), MemberState::Running));
    assert_eq!(saved(&m), (vec![], 0, false, None, false));
    assert_eq!(bt(&f), (true, 2));
    assert!(errors(&f).is_empty() && !store.dir().join("snapshot.pending").exists()); // no id change: no spawn
    // the same call again: nothing to change
    let before = raw(&f);
    ws_with(&f, &missing, "session", json!({"cwd": "/repo"})).unwrap();
    assert_eq!(raw(&f), before);
    // H2: down --now saved values for this session before the commit; H4: then an id
    let g = fx(Agent::OpenCode);
    let gs = ws_state(&g, Phase::Up, MemberRecord { saved_for: Some(SID.into()), ..rec_pending() });
    ws_with(&g, &g.t.path().join("missing"), "session", json!({"cwd": "/repo"})).unwrap();
    let kept = (vec!["u-1".to_string()], 1, true, Some(SID.to_string()), false);
    assert_eq!((member(&g).pending.clone(), member(&g).zygo_session.as_deref()), (None, Some(SID)));
    assert_eq!(saved(&member(&g)), kept);
    ws(&g, "session", oc("ses_A")).unwrap();
    assert_eq!(member(&g).agent_session.as_deref(), Some("ses_A"));
    assert_eq!(saved(&member(&g)), kept);
    let pid = started(g.t.path());
    assert_eq!(child_args(g.t.path()), "ws snapshot --ws-id 0123456789abcdef\n");
    reap(pid);
    assert!(!gs.dir().join("snapshot.pending").exists());
    // H5: commit and record in one call
    let h = fx(Agent::OpenCode);
    ws_state(&h, Phase::Up, MemberRecord { saved_for: Some(OLD.into()), ..rec_pending() });
    ws(&h, "session", oc("ses_A")).unwrap();
    let m = member(&h);
    assert_eq!((m.pending.clone(), m.zygo_session.as_deref(), m.agent_session.as_deref()), (None, Some(SID), Some("ses_A")));
    assert_eq!(saved(&m), (vec![], 0, false, None, false));
    reap(started(h.t.path()));
    // the pending start's own id is committed when the payload has none
    let p = fx(Agent::OpenCode);
    ws_state(&p, Phase::Up, MemberRecord { pending: Some(Pending { zygo_session: SID.into(), agent_session: Some("ses_P".into()), started_at: 7, ..Default::default() }), ..rec_pending() });
    ws(&p, "session", json!({"cwd": "/repo"})).unwrap();
    assert_eq!(member(&p).agent_session.as_deref(), Some("ses_P"));
    reap(started(p.t.path()));
    // H6: another start's pending: nothing changes, one line
    let q = fx(Agent::OpenCode);
    ws_state(&q, Phase::Up, MemberRecord { pending: Some(Pending { zygo_session: OTHER.into(), agent_session: None, started_at: 7, ..Default::default() }), ..rec_pending() });
    let before = raw(&q);
    ws_with(&q, &q.t.path().join("missing"), "session", json!({"cwd": "/repo"})).unwrap();
    assert_eq!(raw(&q), before);
    assert_eq!(errors(&q), ["workspace hook session: member record of pane kaaaaaaaa belongs to another session; not changed"]);
    // H7: no id, an empty id or a non-string id keeps the stored id; the same id writes nothing
    let before = raw(&g);
    for none in [json!({"cwd": "/repo"}), oc(""), json!({"sessionID": 7, "cwd": "/repo"}), oc("ses_A")] {
        ws_with(&g, &g.t.path().join("missing"), "session", none.clone()).unwrap();
        assert_eq!(raw(&g), before, "{none}");
    }
    assert!(errors(&g).is_empty());
    // a 2 MiB id: the result would pass the read limit, so nothing is written (D8)
    let before = raw(&g);
    fs::remove_file(gs.dir().join("errors.log")).ok();
    ws_with(&g, &g.t.path().join("missing"), "session", oc(&"x".repeat(2 * 1024 * 1024))).unwrap();
    assert_eq!(raw(&g), before);
    let e = errors(&g);
    assert_eq!(e.len(), 1, "{}", e.len());
    assert!(e[0].starts_with("workspace hook session: ") && e[0].ends_with("bytes that every reader accepts (AU84); not written"));
    assert!(gs.read().unwrap().is_some());
    // any non-empty id is recorded
    for id in ["-x", "a\u{7}b", "a b"] {
        ws_with(&g, &g.t.path().join("missing"), "session", oc(id)).unwrap();
        assert_eq!(member(&g).agent_session.as_deref(), Some(id));
    }
}

#[test] // W§9.4, AU104, design H8, H9; D10, D17
#[ignore = "timing: waits at most 5 s for a helper thread to take the contended lock"]
fn opencode_note_belongs_to_one_agent_session() {
    let _s = serial();
    // H8: a fresh start; the commit keeps the inputs, the first id binds them, another id removes them
    let f = fx_with(Agent::OpenCode, Some(marker(ID, KEY, Some(note_iu()))));
    let missing = f.t.path().join("missing");
    ws_state(&f, Phase::Up, rec_pending());
    ws_with(&f, &missing, "session", json!({"cwd": "/repo"})).unwrap();
    assert_eq!(view_note(&f), Some(note_iu()));
    ws_with(&f, &missing, "session", oc("ses_A")).unwrap();
    assert_eq!(view_note(&f), Some(bound(note_iu(), "ses_A")));
    assert_eq!(ss_text(&f, &oc_start(), &missing), format!("{}\n\n{I1}\n{U12}", base(&f)));
    ws_with(&f, &missing, "session", oc("ses_A")).unwrap();
    assert_eq!(view_note(&f), Some(bound(note_iu(), "ses_A")));
    ws_with(&f, &missing, "session", oc("ses_B")).unwrap();
    assert_eq!(view_note(&f), None);
    assert_eq!(ss_text(&f, &oc_start(), &missing), base(&f));
    // H9: a resume of ses_X: the inputs are bound to it already
    let g = fx_with(Agent::OpenCode, Some(marker(ID, KEY, Some(bound(note_iu(), "ses_X")))));
    let gm = g.t.path().join("missing");
    ws_state(&g, Phase::Up, MemberRecord { agent_session: Some("ses_X".into()), pending: Some(Pending { zygo_session: SID.into(), agent_session: Some("ses_X".into()), started_at: 7, ..Default::default() }), ..rec_pending() });
    ws_with(&g, &gm, "session", json!({"cwd": "/repo"})).unwrap();
    ws_with(&g, &gm, "session", oc("ses_X")).unwrap();
    assert_eq!(view_note(&g), Some(bound(note_iu(), "ses_X")));
    ws_with(&g, &gm, "session", oc("ses_Y")).unwrap();
    assert_eq!(view_note(&g), None);
    // D17: the rule runs when the record part does not write (no state, a held lock)
    let h = fx_with(Agent::OpenCode, Some(marker(ID, KEY, Some(note_iu()))));
    ws_with(&h, &h.t.path().join("missing"), "session", oc("ses_A")).unwrap();
    assert_eq!(view_note(&h), Some(bound(note_iu(), "ses_A")));
    assert!(!h.paths.state_dir().exists());
    let k = fx_with(Agent::OpenCode, Some(marker(ID, KEY, Some(note_iu()))));
    let ks = ws_state(&k, Phase::Up, rec_pending());
    { let _h = hold_state(&ks); ws_with(&k, &k.t.path().join("missing"), "session", oc("ses_A")).unwrap(); }
    assert_eq!(view_note(&k), Some(bound(note_iu(), "ses_A")));
    assert_eq!(member(&k), rec_pending());
}

#[test] // W§7.3, D12, D13
#[ignore = "timing: waits at most 5 s for the detached snapshot child to start"]
fn pending_snapshot_runs_in_a_detached_child() {
    let _s = serial();
    let f = fx(Agent::OpenCode);
    let store = ws_state(&f, Phase::Up, rec_pending());
    snapshot::queue_pending(&f.paths, ID, true).unwrap();
    ws(&f, "session", oc("ses_A")).unwrap(); // one write, an id change and a force file: one child
    let pid = started(f.t.path());
    // SAFETY: signal 0 only checks that the process exists.
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0); // still running: run returned before the child ended
    assert_eq!(child_args(f.t.path()), "ws snapshot --ws-id 0123456789abcdef --force\n");
    // SAFETY: getsid takes a plain pid.
    assert_eq!(unsafe { libc::getsid(pid) }, pid); // a new session leader
    #[cfg(target_os = "linux")]
    {
        for fd in 0..3 {
            assert_eq!(fs::read_link(format!("/proc/{pid}/fd/{fd}")).unwrap(), Path::new("/dev/null"), "fd {fd}");
        }
        assert_eq!(fs::read_link(format!("/proc/{pid}/cwd")).unwrap(), Path::new("/"));
    }
    assert!(!store.dir().join("snapshot.pending").exists());
    reap(pid);
    clear_child(f.t.path());
    // A file without force and no id change: the flag of the file alone.
    store
        .update(S5, &mut |w| {
            w.members.get_mut(KEY).unwrap().pending = Some(Pending { zygo_session: SID.into(), agent_session: None, started_at: 8, ..Default::default() });
            Ok(())
        })
        .unwrap();
    snapshot::queue_pending(&f.paths, ID, false).unwrap();
    ws(&f, "session", oc("ses_A")).unwrap();
    let pid = started(f.t.path());
    assert_eq!(child_args(f.t.path()), "ws snapshot --ws-id 0123456789abcdef\n");
    reap(pid);
}

#[test] // W§7.3, D13; controller decision 11
fn snapshot_child_that_cannot_start_or_fails_is_queued_again() {
    let _s = serial();
    let f = fx(Agent::OpenCode);
    let store = ws_state(&f, Phase::Up, rec_pending());
    let missing = f.t.path().join("missing");
    let pending = store.dir().join("snapshot.pending");
    snapshot::queue_pending(&f.paths, ID, true).unwrap();
    assert_eq!(ws_with(&f, &missing, "session", oc("ses_A")).unwrap(), None);
    assert_eq!(fs::read(&pending).unwrap(), b"force");
    let e = errors(&f);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].starts_with("workspace hook session: cannot start the snapshot: ") && e[0].ends_with("; snapshot.pending set again"), "{e:?}");
    fs::remove_file(&pending).unwrap();
    fs::remove_file(store.dir().join("errors.log")).unwrap();
    ws_with(&f, &missing, "session", oc("ses_B")).unwrap(); // the trigger alone: an empty file
    assert_eq!(fs::read(&pending).unwrap(), b"");
    assert_eq!(errors(&f).len(), 1);
    fs::remove_file(&pending).unwrap();
    // A child that started and failed.
    let e = workspace::snapshot_child(&f.paths, ID, true, &mut || anyhow::bail!("tmux gone")).unwrap_err();
    assert_eq!(format!("{e:#}"), "tmux gone");
    assert_eq!(fs::read(&pending).unwrap(), b"force");
    fs::remove_file(&pending).unwrap();
    let e = workspace::snapshot_child(&f.paths, ID, false, &mut || panic!("boom")).unwrap_err();
    assert_eq!(format!("{e:#}"), "the snapshot panicked: boom");
    assert_eq!(fs::read(&pending).unwrap(), b"");
    fs::remove_file(&pending).unwrap();
    workspace::snapshot_child(&f.paths, ID, true, &mut || Ok(())).unwrap();
    assert!(!pending.exists());
    let e = workspace::snapshot_child(&f.paths, "../x", false, &mut || anyhow::bail!("no")).unwrap_err();
    assert_eq!(format!("{e:#}"), "no; snapshot.pending not set: invalid workspace id \"../x\"");
}

#[test] // AU56, W§7.3, W-C1, D7, D17; controller decision 13
#[ignore = "timing: waits at most 5 s for a helper thread to take the contended lock"]
fn lock_bounds_write_one_line_and_change_nothing() {
    let _s = serial();
    let f = fx(Agent::ClaudeCode);
    set_act(&f, true, 2);
    let lock_line = |ev: &str| format!("workspace hook {ev}: {}: lock not taken within 1000 ms", f.dir.activity_lock().display());
    // A missed lock is a one-off failure (round 3 S2, S4): every event where it happens writes its line.
    let mut lines = Vec::new();
    for (ev, payload) in [
        ("stop", fixture("Stop-main")),
        ("pre-tool", fixture("PreToolUse-Bash-main")),
        ("tool-end", fixture("PostToolUse-Bash-main")),
        ("prompt", ups("hello")),
        ("session-end", json!({"session_id": AS1, "hook_event_name": "SessionEnd", "reason": "other"})),
    ] {
        {
            let _h = hold_activity(&f.dir);
            assert_eq!(ws(&f, ev, payload).unwrap(), None, "{ev}");
        }
        assert_eq!(bt(&f), (true, 2), "{ev}"); // the change was lost
        lines.push(lock_line(ev));
        assert_eq!(slog(&f), lines, "{ev}");
    }
    let store = ws_state(&f, Phase::Draining, MemberRecord::new(Agent::ClaudeCode));
    let op = store.op_lock().unwrap();
    {
        let _h = hold_activity(&f.dir);
        assert_eq!(parsed(ws(&f, "pre-tool", fixture("PreToolUse-Agent-main"))), deny(workspace::DRAINING));
    }
    assert_eq!(slog(&f), lines); // a deny needs no activity change
    drop(op);
    let g = fx(Agent::OpenCode);
    let store = ws_state(&g, Phase::Up, rec_pending());
    set_act(&g, true, 0);
    {
        let _h = hold_state(&store);
        assert_eq!(ws(&g, "session", oc("ses_A")).unwrap(), None);
    }
    assert_eq!(bt(&g), (true, 0)); // `session` changes no activity
    assert_eq!((member(&g).pending.is_some(), member(&g).agent_session.as_deref()), (true, Some("old-id")));
    assert_eq!(errors(&g), ["workspace hook session: workspace state is locked"]);
    assert!(!g.t.path().join("child.pid").exists() && !store.dir().join("snapshot.pending").exists()); // no write, no pending check
    // Claude Code: the note is printed and cleared from the view; the record keeps `pending`.
    let h = fx_with(Agent::ClaudeCode, Some(marker(ID, KEY, Some(rn(true, vec![], &["u-1"])))));
    let store = ws_state(&h, Phase::Up, rec_pending());
    {
        let _h = hold_state(&store);
        assert_eq!(ss_text(&h, &ss(AS1), &h.zygo), format!("{}\n\n{U1}", base(&h)));
    }
    assert_eq!(errors(&h), ["workspace hook session-start: workspace state is locked"]);
    assert_eq!(view_note(&h), None);
    assert_eq!(member(&h), rec_pending());
}

/// The member view of `f` padded with 2 MiB of spaces after the JSON object.
fn pad_view(f: &Fx) {
    let mut b = fs::read(f.dir.view_json()).unwrap();
    b.extend(std::iter::repeat_n(b' ', 2 << 20));
    fs::write(f.dir.view_json(), b).unwrap();
}

#[test] // C6, AU84, FIX-17 classes, D1-D3, D6, D18; controller decisions 2, 12, 13
#[ignore = "timing: waits at most 5 s for the hook on a helper thread"]
fn hostile_inputs_change_nothing() {
    let _s = serial();
    let run_raw = |f: &Fx, event: &str, stdin: &[u8]| {
        let sd = Some(f.dir.root.clone().into_os_string());
        workspace::run(event, stdin, &f.paths, &|k| if k == "ZYGO_SESSION_DIR" { sd.clone() } else { None }, &f.zygo)
    };
    // The event, then the session folder variable, then the input.
    let f = fx(Agent::ClaudeCode);
    assert_eq!(format!("{:#}", ws(&f, "frob", json!({})).unwrap_err()), "workspace: unknown event \"frob\"");
    assert_eq!(format!("{:#}", ws_env(&f, "frob", json!({}), &f.zygo, None).unwrap_err()), "workspace: unknown event \"frob\"");
    assert!(run_raw(&f, "stop", b"{").is_err());
    assert_eq!(format!("{:#}", run_raw(&f, "stop", b"[]").unwrap_err()), "hook input is not a JSON object");
    assert!(!f.dir.activity_json().exists());
    // No or an empty ZYGO_SESSION_DIR: nothing; a relative one: refused.
    let f = fx(Agent::ClaudeCode);
    assert_eq!(ws_env(&f, "stop", json!({}), &f.zygo, None).unwrap(), None);
    assert_eq!(ws_env(&f, "stop", json!({}), &f.zygo, Some(OsString::new())).unwrap(), None);
    assert!(!f.dir.activity_json().exists() && !f.dir.log().exists());
    assert_eq!(format!("{:#}", ws_env(&f, "stop", json!({}), &f.zygo, Some("s".into())).unwrap_err()), "ZYGO_SESSION_DIR is not an absolute path: s");
    // A session folder that is a link or not a folder.
    let f = fx(Agent::ClaudeCode);
    let t = f.t.path();
    std::os::unix::fs::symlink(&f.dir.root, t.join("link")).unwrap();
    fs::write(t.join("file"), "").unwrap();
    assert_eq!(
        format!("{:#}", ws_env(&f, "stop", json!({}), &f.zygo, Some(t.join("link").into_os_string())).unwrap_err()),
        format!("{}: session folder is a link", t.join("link").display())
    );
    assert_eq!(
        format!("{:#}", ws_env(&f, "stop", json!({}), &f.zygo, Some(t.join("file").into_os_string())).unwrap_err()),
        format!("{}: session folder is not a folder", t.join("file").display())
    );
    assert!(!f.dir.activity_json().exists());
    assert!(slog(&f).is_empty());
    // A view.json that is a FIFO, a link, or not JSON: `stop` writes no line, `prompt` writes one (R2-2).
    let f = fx(Agent::ClaudeCode);
    let view = f.dir.view_json().display().to_string();
    fs::remove_file(f.dir.view_json()).unwrap();
    mkfifo(&f.dir.view_json());
    assert_eq!(ws_bg(&f, "stop", fixture("Stop-main")).unwrap(), None);
    assert!(slog(&f).is_empty());
    assert_eq!(ws_bg(&f, "prompt", oc("ses_A")).unwrap(), None);
    assert_eq!(slog(&f), [format!("workspace hook prompt: {view}: is not a regular file; not read")]);
    assert!(!f.dir.activity_json().exists());
    let f = fx(Agent::ClaudeCode);
    let view = f.dir.view_json().display().to_string();
    fs::rename(f.dir.view_json(), f.t.path().join("v.json")).unwrap();
    std::os::unix::fs::symlink(f.t.path().join("v.json"), f.dir.view_json()).unwrap();
    assert_eq!(ws(&f, "stop", fixture("Stop-main")).unwrap(), None);
    assert!(slog(&f).is_empty());
    assert_eq!(ws(&f, "prompt", oc("ses_A")).unwrap(), None);
    assert_eq!(slog(&f), [format!("workspace hook prompt: {view}: is a link; not read")]);
    assert!(!f.dir.activity_json().exists());
    let f = fx(Agent::ClaudeCode);
    let view = f.dir.view_json().display().to_string();
    fs::write(f.dir.view_json(), "{").unwrap();
    assert_eq!(ws(&f, "stop", fixture("Stop-main")).unwrap(), None);
    assert!(slog(&f).is_empty());
    assert_eq!(ws(&f, "prompt", oc("ses_A")).unwrap(), None);
    assert_eq!(slog(&f), [format!("workspace hook prompt: {view}: EOF while parsing an object at line 1 column 1")]);
    // No cap: a member view of more than 2 MiB is read.
    let f = fx(Agent::ClaudeCode);
    pad_view(&f);
    set_act(&f, true, 1);
    assert_eq!(ws(&f, "stop", fixture("Stop-main")).unwrap(), None);
    assert_eq!(bt(&f), (false, 0));
    // A launch that is not a member keeps today's behaviour, any size.
    let f = fx_with(Agent::ClaudeCode, None);
    pad_view(&f);
    let text = ss_text(&f, &ss(AS1), &f.zygo);
    assert_eq!(text, context(&serde_json::from_str(&session_start::run_in(&ss(AS1), &f.dir).unwrap().to_stdout()).unwrap()));
    assert_eq!(text, base(&f));
    assert!(!f.dir.activity_json().exists() && !f.dir.log().exists() && !f.paths.state_dir().exists());
    // A non-member view.json that is a link: today's `session-start` read follows it.
    let f = fx_with(Agent::ClaudeCode, None);
    fs::rename(f.dir.view_json(), f.t.path().join("v.json")).unwrap();
    std::os::unix::fs::symlink(f.t.path().join("v.json"), f.dir.view_json()).unwrap();
    assert_eq!(ss_text(&f, &ss(AS1), &f.zygo), context(&serde_json::from_str(&session_start::run_in(&ss(AS1), &f.dir).unwrap().to_stdout()).unwrap()));
    assert!(!f.dir.log().exists());
    // An activity.json that is a link, a FIFO, both it and session.log FIFOs, or of a newer zygo. A link and a FIFO
    // are persistent file conditions (round 3 S2, S4): `pre-tool`, `tool-end` and `stop` write no line, a `prompt`
    // writes one.
    let f = fx(Agent::ClaudeCode);
    let act_path = f.dir.activity_json().display().to_string();
    fs::write(f.t.path().join("outside.json"), "{}").unwrap();
    std::os::unix::fs::symlink(f.t.path().join("outside.json"), f.dir.activity_json()).unwrap();
    assert_eq!(ws(&f, "pre-tool", fixture("PreToolUse-Bash-main")).unwrap(), None);
    assert_eq!(ws(&f, "tool-end", fixture("PostToolUse-Bash-main")).unwrap(), None);
    assert_eq!(ws(&f, "stop", fixture("Stop-main")).unwrap(), None);
    assert!(slog(&f).is_empty(), "{:?}", slog(&f));
    assert_eq!(ws(&f, "prompt", ups("hello")).unwrap(), None);
    assert_eq!(fs::read_to_string(f.t.path().join("outside.json")).unwrap(), "{}");
    assert_eq!(slog(&f), [format!("workspace hook prompt: {act_path}: is a link")]);
    let f = fx(Agent::ClaudeCode);
    let act_path = f.dir.activity_json().display().to_string();
    mkfifo(&f.dir.activity_json());
    assert_eq!(ws_bg(&f, "pre-tool", fixture("PreToolUse-Bash-main")).unwrap(), None);
    assert_eq!(ws_bg(&f, "tool-end", fixture("PostToolUse-Bash-main")).unwrap(), None);
    assert_eq!(ws_bg(&f, "stop", fixture("Stop-main")).unwrap(), None);
    assert!(slog(&f).is_empty(), "{:?}", slog(&f));
    assert_eq!(ws_bg(&f, "prompt", ups("hello")).unwrap(), None);
    assert!(is_fifo(&f.dir.activity_json()));
    assert_eq!(slog(&f), [format!("workspace hook prompt: {act_path}: is not a regular file")]);
    let f = fx(Agent::ClaudeCode);
    mkfifo(&f.dir.activity_json());
    mkfifo(&f.dir.log());
    assert_eq!(ws_bg(&f, "stop", fixture("Stop-main")).unwrap(), None);
    assert!(is_fifo(&f.dir.activity_json()) && is_fifo(&f.dir.log()));
    // A file of a newer zygo is a persistent condition (round 4 T3): `pre-tool`, `tool-end` and `stop` write no line,
    // a `prompt` writes one; the file is kept.
    let f = fx(Agent::ClaudeCode);
    let act_path = f.dir.activity_json().display().to_string();
    fs::write(f.dir.activity_json(), r#"{"version": 2, "busy": true}"#).unwrap();
    assert_eq!(ws(&f, "pre-tool", fixture("PreToolUse-Bash-main")).unwrap(), None);
    assert_eq!(ws(&f, "tool-end", fixture("PostToolUse-Bash-main")).unwrap(), None);
    assert_eq!(ws(&f, "stop", fixture("Stop-main")).unwrap(), None);
    assert!(slog(&f).is_empty(), "{:?}", slog(&f));
    assert_eq!(ws(&f, "prompt", ups("hello")).unwrap(), None);
    assert_eq!(fs::read_to_string(f.dir.activity_json()).unwrap(), r#"{"version": 2, "busy": true}"#);
    assert_eq!(slog(&f), [format!("workspace hook prompt: {act_path}: written by a newer zygo (version 2)")]);
    // An activity.lock that is a link is a persistent condition (round 4 T3): the same; the target is not changed.
    let f = fx(Agent::ClaudeCode);
    let lock_path = f.dir.activity_lock().display().to_string();
    fs::write(f.t.path().join("outside.lock"), "x").unwrap();
    std::os::unix::fs::symlink(f.t.path().join("outside.lock"), f.dir.activity_lock()).unwrap();
    assert_eq!(ws(&f, "pre-tool", fixture("PreToolUse-Bash-main")).unwrap(), None);
    assert_eq!(ws(&f, "tool-end", fixture("PostToolUse-Bash-main")).unwrap(), None);
    assert_eq!(ws(&f, "stop", fixture("Stop-main")).unwrap(), None);
    assert!(slog(&f).is_empty(), "{:?}", slog(&f));
    assert_eq!(ws(&f, "prompt", ups("hello")).unwrap(), None);
    assert_eq!(fs::read_to_string(f.t.path().join("outside.lock")).unwrap(), "x");
    assert!(!f.dir.activity_json().exists());
    assert_eq!(
        slog(&f),
        [format!("workspace hook prompt: {lock_path}: open lock file: {lock_path}: is a link; it is not followed")]
    );
    // A session folder made read-only: the write fails, a persistent condition (round 4 T3). The session log exists
    // before, so its lines can still be appended. Root writes in a read-only folder, so the case is skipped there.
    // SAFETY: `geteuid` takes no argument and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped the read-only session folder case: the test runs as root");
    } else {
        let f = fx(Agent::ClaudeCode);
        set_act(&f, true, 1);
        let before = fs::read(f.dir.activity_json()).unwrap();
        fs::write(f.dir.log(), "").unwrap();
        let _mode = Mode(f.dir.root.clone(), fs::metadata(&f.dir.root).unwrap().permissions().mode());
        fs::set_permissions(&f.dir.root, fs::Permissions::from_mode(0o500)).unwrap();
        assert_eq!(ws(&f, "pre-tool", fixture("PreToolUse-Bash-main")).unwrap(), None);
        assert_eq!(ws(&f, "tool-end", fixture("PostToolUse-Bash-main")).unwrap(), None);
        assert_eq!(ws(&f, "stop", fixture("Stop-main")).unwrap(), None);
        assert!(slog(&f).is_empty(), "{:?}", slog(&f));
        assert_eq!(ws(&f, "prompt", ups("hello")).unwrap(), None);
        assert!(fs::read(f.dir.activity_json()).unwrap() == before, "activity.json changed");
        let lines = slog(&f);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with(&format!("workspace hook prompt: {}: ", f.dir.activity_json().display())), "{lines:?}");
    }
    // A 2 MiB agent_id in an open turn (attempt-2 review round 1, F1): the result would pass the read limit, so
    // nothing is written, and the turn stays busy through the next tool events. The refused result is a one-off
    // failure (round 3 S2, S4; round 4 T3): each `subagent-start` writes one line; the next tool events succeed and
    // write none.
    let f = fx(Agent::ClaudeCode);
    set_act(&f, true, 0);
    let before = fs::read(f.dir.activity_json()).unwrap();
    let mut big = fixture("SubagentStart-sub");
    big["agent_id"] = json!("x".repeat(2 << 20));
    assert_eq!(ws(&f, "subagent-start", big.clone()).unwrap(), None);
    assert!(fs::read(f.dir.activity_json()).unwrap() == before, "activity.json changed");
    assert_eq!(slog(&f).len(), 1);
    assert_eq!(ws(&f, "subagent-start", big).unwrap(), None);
    assert!(fs::read(f.dir.activity_json()).unwrap() == before, "activity.json changed");
    let refused = slog(&f);
    assert_eq!(refused.len(), 2);
    assert!(refused[1].starts_with(&format!("workspace hook subagent-start: {}: the new activity is ", f.dir.activity_json().display())));
    assert!(
        refused[0].starts_with(&format!("workspace hook subagent-start: {}: the new activity is ", f.dir.activity_json().display()))
            && refused[0].ends_with(" bytes, more than the 1048576 bytes that every reader accepts; not written"),
        "{}",
        refused[0].chars().take(300).collect::<String>()
    );
    assert_eq!(bt(&f), (true, 0));
    assert_eq!(ws(&f, "pre-tool", fixture("PreToolUse-Bash-main")).unwrap(), None);
    assert_eq!(bt(&f), (true, 1));
    assert_eq!(ws(&f, "tool-end", fixture("PostToolUse-Bash-main")).unwrap(), None);
    assert_eq!(bt(&f), (true, 0));
    assert_eq!(slog(&f), refused);
    // An activity.json over the read limit or one that does not parse is kept (F1): the drain counts it busy, and
    // a write from the default would end the open turn. The update error writes one line per turn (R3): the tool
    // events write none, a `prompt` writes one.
    for (i, text) in [{ let mut b = br#"{"version":1,"busy":true,"running":{},"updated_at":1}"#.to_vec(); b.resize(2 << 20, b' '); b }, b"{not json".to_vec()].into_iter().enumerate() {
        let f = fx(Agent::ClaudeCode);
        fs::write(f.dir.activity_json(), &text).unwrap();
        assert_eq!(ws(&f, "pre-tool", fixture("PreToolUse-Bash-main")).unwrap(), None);
        assert_eq!(ws(&f, "tool-end", fixture("PostToolUse-Bash-main")).unwrap(), None);
        assert_eq!(ws(&f, "stop", fixture("Stop-main")).unwrap(), None);
        assert!(slog(&f).is_empty(), "{i}: {:?}", slog(&f));
        assert_eq!(ws(&f, "prompt", ups("hello")).unwrap(), None);
        assert!(fs::read(f.dir.activity_json()).unwrap() == text, "activity.json {i} changed");
        assert_eq!(slog(&f).len(), 1, "{i}");
        assert!(slog(&f)[0].starts_with(&format!("workspace hook prompt: {}: ", f.dir.activity_json().display())), "{i}");
        // `session-end` is one of the few calls per turn: it writes one more line.
        let end = json!({"session_id": AS1, "hook_event_name": "SessionEnd", "reason": "other"});
        assert_eq!(ws(&f, "session-end", end).unwrap(), None);
        assert!(fs::read(f.dir.activity_json()).unwrap() == text, "activity.json {i} changed");
        assert_eq!(slog(&f).len(), 2, "{i}");
        assert!(slog(&f)[1].starts_with(&format!("workspace hook session-end: {}: ", f.dir.activity_json().display())), "{i}");
    }
    // Marker values that fail their check: one line per turn, from `prompt`, `session`, `session-end` and the Claude
    // Code `SessionStart` only; the per-tool and per-request events write none (F3).
    let quiet = [
        ("pre-tool", fixture("PreToolUse-Bash-main")),
        ("tool-end", fixture("PostToolUse-Bash-main")),
        ("stop", fixture("Stop-main")),
        ("subagent-start", fixture("SubagentStart-sub")),
        ("subagent-stop", fixture("SubagentStop-sub")),
    ];
    let f = fx_with(Agent::ClaudeCode, Some(marker("../x", KEY, None)));
    for (ev, v) in &quiet {
        assert_eq!(ws(&f, ev, v.clone()).unwrap(), None, "{ev}");
    }
    assert!(slog(&f).is_empty());
    assert_eq!(ws(&f, "prompt", oc("ses_A")).unwrap(), None);
    assert_eq!(slog(&f), ["workspace hook prompt: invalid workspace id \"../x\" in the session view"]);
    assert!(!f.dir.activity_json().exists() && !f.paths.state_dir().exists());
    let f = fx_with(Agent::ClaudeCode, Some(marker(ID, "k$(x)", None)));
    for (ev, v) in &quiet {
        assert_eq!(ws(&f, ev, v.clone()).unwrap(), None, "{ev}");
    }
    assert!(errors(&f).is_empty());
    assert_eq!(ws(&f, "prompt", oc("ses_A")).unwrap(), None);
    assert_eq!(errors(&f), ["workspace hook prompt: invalid pane key \"k$(x)\" in the session view"]);
    for ev in ["session", "session-end"] {
        ws(&f, ev, oc("ses_A")).unwrap();
    }
    ss_text(&f, &ss(AS1), &f.zygo);
    assert_eq!(errors(&f).len(), 4);
    assert_eq!(errors(&f)[3], "workspace hook session-start: invalid pane key \"k$(x)\" in the session view");
    assert!(!f.dir.activity_json().exists());
    // A marker value of the wrong JSON type: `view.json` does not deserialize, and the read error follows the same
    // log flag as the marker checks: the per-tool events write no line, `prompt` writes one (H1 review round 2 R2-2).
    for (key, val) in [("pane", json!(5)), ("ws_id", Value::Null)] {
        let f = fx(Agent::ClaudeCode);
        let mut v: Value = serde_json::from_slice(&fs::read(f.dir.view_json()).unwrap()).unwrap();
        v["workspace"]["member"][key] = val;
        fs::write(f.dir.view_json(), serde_json::to_vec(&v).unwrap()).unwrap();
        assert_eq!(ws(&f, "pre-tool", fixture("PreToolUse-Bash-main")).unwrap(), None, "{key}");
        assert_eq!(ws(&f, "tool-end", fixture("PostToolUse-Bash-main")).unwrap(), None, "{key}");
        assert!(slog(&f).is_empty(), "{key}: {:?}", slog(&f));
        assert_eq!(ws(&f, "prompt", oc("ses_A")).unwrap(), None, "{key}");
        let lines = slog(&f);
        assert_eq!(lines.len(), 1, "{key}: {lines:?}");
        assert!(lines[0].starts_with(&format!("workspace hook prompt: {}: ", f.dir.view_json().display())), "{key}: {lines:?}");
        assert!(!f.dir.activity_json().exists(), "{key}");
    }
    // The OpenCode per-request `session-start` writes none.
    let f = fx_with(Agent::OpenCode, Some(marker(ID, "k$(x)", None)));
    assert_eq!(ss_text(&f, &oc_start(), &f.zygo), base(&f));
    assert!(errors(&f).is_empty());
    // A state folder that is a link.
    let f = fx(Agent::OpenCode);
    let t = f.t.path();
    fs::create_dir(t.join("elsewhere")).unwrap();
    fs::create_dir_all(f.paths.workspaces_dir()).unwrap();
    std::os::unix::fs::symlink(t.join("elsewhere"), f.paths.workspaces_dir().join(ID)).unwrap();
    assert_eq!(ws(&f, "session", oc("ses_A")).unwrap(), None);
    assert!(!f.dir.activity_json().exists());
    assert_eq!(slog(&f), [format!("workspace hook session: {}: state folder is a link", f.paths.workspaces_dir().join(ID).display())]);
    assert_eq!(fs::read_dir(t.join("elsewhere")).unwrap().count(), 0);
    // A subagent id that is not a string.
    let f = fx(Agent::ClaudeCode);
    ws(&f, "subagent-start", json!({"agent_id": 7, "agent_type": "x", "hook_event_name": "SubagentStart"})).unwrap();
    assert!(act(&f).running.is_empty());
    // An empty subagent id is no id, as a missing one: no change, so no write of activity.json (F5).
    let f = fx(Agent::ClaudeCode);
    ws(&f, "subagent-start", json!({"agent_id": "", "agent_type": "x", "hook_event_name": "SubagentStart"})).unwrap();
    assert!(act(&f).running.is_empty());
    ws(&f, "subagent-stop", json!({"agent_id": "", "hook_event_name": "SubagentStop"})).unwrap();
    assert!(!f.dir.activity_json().exists(), "an empty id wrote activity.json");
    ws(&f, "subagent-stop", json!({"agent_id": 7, "hook_event_name": "SubagentStop"})).unwrap();
    assert!(!f.dir.activity_json().exists(), "a non-string id wrote activity.json");
    ws(&f, "subagent-stop", json!({"hook_event_name": "SubagentStop"})).unwrap();
    assert!(!f.dir.activity_json().exists(), "no id wrote activity.json");
}

#[test] // FIX-22 review round 1, F4: a link or a FIFO at activity.json is refused before activity.lock
#[ignore = "timing: waits at most 5 s for a helper thread to take the contended lock; waits at most 5 s for the hook on a helper thread"]
fn a_link_or_fifo_activity_file_is_refused_before_the_lock() {
    let _s = serial();
    for case in ["link", "fifo"] {
        let f = fx(Agent::ClaudeCode);
        let act_path = f.dir.activity_json().display().to_string();
        let outside = f.t.path().join("outside.json");
        fs::write(&outside, "{}").unwrap();
        // The lock is held first; then the entry is put in place, so the holder's own update is not refused.
        let held = hold_activity(&f.dir);
        match case {
            "link" => std::os::unix::fs::symlink(&outside, f.dir.activity_json()).unwrap(),
            _ => mkfifo(&f.dir.activity_json()),
        }
        // A persistent condition, not a missed lock: the lock stays held for the whole case, and a lock not taken
        // within LOCK_BOUND is a one-off failure that `change` logs on every event (`<activity.lock>: lock not taken
        // within 1000 ms`), while a persistent condition logs only on `prompt`, with the refusal text. So no line for
        // these events, and the one line of `prompt`, show that the refusal came before the lock (review round 2).
        for (ev, payload) in [
            ("pre-tool", fixture("PreToolUse-Bash-main")),
            ("tool-end", fixture("PostToolUse-Bash-main")),
            ("stop", fixture("Stop-main")),
        ] {
            assert_eq!(ws_bg(&f, ev, payload).unwrap(), None, "{case} {ev}");
            assert!(slog(&f).is_empty(), "{case} {ev}: {:?}", slog(&f));
        }
        assert_eq!(ws_bg(&f, "prompt", ups("hello")).unwrap(), None, "{case}");
        let why = if case == "link" { "is a link" } else { "is not a regular file" };
        assert_eq!(slog(&f), [format!("workspace hook prompt: {act_path}: {why}")], "{case}");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "{}", "{case}");
        drop(held);
    }
}

#[test] // W-C6, OP9, D3; H1 attempt 1 round 3 F1
#[ignore = "timing: waits at most 5 s for the hook on a helper thread"]
fn non_member_session_start_is_unchanged() {
    let _s = serial();
    for case in ["plain", "padded", "link", "empty", "not JSON", "mode 0"] {
        let g = fx_with(Agent::ClaudeCode, None);
        match case {
            "padded" => pad_view(&g),
            "link" => link_view(&g),
            "empty" => fs::write(g.dir.view_json(), "").unwrap(),
            "not JSON" => fs::write(g.dir.view_json(), "{").unwrap(),
            "mode 0" => fs::set_permissions(g.dir.view_json(), fs::Permissions::from_mode(0o000)).unwrap(),
            _ => {}
        }
        let mut out = Vec::new();
        let a = session_start::run_member(&ss(AS1), &g.dir, &g.paths, &g.zygo, &mut out);
        let b = session_start::run_in(&ss(AS1), &g.dir);
        // When the test runs as root (`geteuid() == 0`) a mode-0 file is read by both, and the comparison still holds.
        match (a, b) {
            (Ok(a), Ok(b)) => assert_eq!(a.to_stdout(), b.to_stdout(), "{case}"),
            (Err(a), Err(b)) => assert_eq!(format!("{a:#}"), format!("{b:#}"), "{case}"),
            (a, b) => panic!("{case}: run_member ok {}, run_in ok {}", a.is_ok(), b.is_ok()),
        }
        assert!(out.is_empty(), "{case}");
        assert!(!g.dir.log().exists(), "{case}");
        assert!(!g.dir.activity_json().exists(), "{case}");
        assert!(!g.paths.state_dir().exists(), "{case}");
    }
    // A FIFO view.json (FIX-22, controller 2026-11-23): the read does not wait; run_member gives run_in's error and writes nothing.
    let g = fx_with(Agent::ClaudeCode, None);
    fs::remove_file(g.dir.view_json()).unwrap();
    mkfifo(&g.dir.view_json());
    let (dir, paths, zygo) = (g.dir.clone(), g.paths.clone(), g.zygo.clone());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let a = session_start::run_member(&ss(AS1), &dir, &paths, &zygo, &mut out);
        let b = session_start::run_in(&ss(AS1), &dir);
        let _ = tx.send((
            a.map(|o| o.to_stdout()).map_err(|e| format!("{e:#}")),
            b.map(|o| o.to_stdout()).map_err(|e| format!("{e:#}")),
            out.is_empty(),
        ));
    });
    let (a, b, quiet) = rx.recv_timeout(S5).expect("the hook waited");
    assert_eq!(a, Err(format!("{}: is a FIFO", g.dir.view_json().display())));
    assert_eq!(a, b);
    assert!(quiet);
    assert!(is_fifo(&g.dir.view_json()));
    assert!(!g.dir.log().exists());
    assert!(!g.dir.activity_json().exists());
    assert!(!g.paths.state_dir().exists());
}

/// A verb wait that never ends (`prompt::run_with` has no deadline past the end of the clock): the tests of a verb
/// that panics or replies give the same result on a loaded machine. The busy mark still waits at most `LOCK_BOUND`.
const NO_DEADLINE: Duration = Duration::MAX;

/// A `/zygo:` dispatcher whose verb panics (round 4 T2).
fn panicking(_: &str, _: &str) -> anyhow::Result<String> {
    panic!("verb panicked")
}

/// A `/zygo:` dispatcher whose verb replies (round 4 T2).
fn replying(_: &str, _: &str) -> anyhow::Result<String> {
    Ok("x".into())
}

/// `prompt::run_in_env` as `zygo hook prompt` runs it, through `guarded`, with the environment `env`.
fn prompt_hook(f: &Fx, p: &str, d: SlashDispatch, env: &dyn Fn(&str) -> Option<OsString>) -> (String, i32) {
    let bytes = serde_json::to_vec(&ups(p)).unwrap();
    guarded(
        &|b| prompt::run_in_env(&HookInput::parse(Agent::ClaudeCode, b)?, Some(d), NO_DEADLINE, &f.paths, env),
        &bytes,
    )
}

#[test] // W§9.1 `UserPromptSubmit` row, C6 (base spec N28), AU58; H1 attempt-2 review round 4 T2
fn panicking_verb_lets_the_line_pass_and_marks_only_a_member_busy() {
    let _s = serial();
    let text = "prompt: verb `status` ended without a reply";
    // A member (`ZYGO_WORKSPACE` set, member view): one stderr line (the error of C6), no block, and the turn of the
    // line that reaches the model is busy, as the `workspace --event prompt` entry would make it.
    let f = fx(Agent::ClaudeCode);
    let sd = f.dir.root.clone().into_os_string();
    let env = |k: &str| match k {
        "ZYGO_SESSION_DIR" => Some(sd.clone()),
        "ZYGO_WORKSPACE" => Some(OsString::from(ID)),
        _ => None,
    };
    set_act(&f, false, 2);
    let input = hook(Agent::ClaudeCode, &ups("/zygo:status"));
    let Err(e) = prompt::run_in_env(&input, Some(panicking), NO_DEADLINE, &f.paths, &env) else {
        panic!("not an error")
    };
    assert_eq!(format!("{e:#}"), text);
    assert_eq!(bt(&f), (true, 0));
    set_act(&f, false, 2);
    assert_eq!(prompt_hook(&f, "/zygo:status", panicking, &env), (String::new(), 0));
    assert_eq!(bt(&f), (true, 0));
    assert!(slog(&f).is_empty(), "{:?}", slog(&f));
    // A verb that replies blocks its line and marks nothing; a line that is not a verb passes with no mark here (the
    // `workspace --event prompt` entry marks it).
    set_act(&f, false, 2);
    assert_eq!(serde_json::from_str::<Value>(&prompt_hook(&f, "/zygo:status", replying, &env).0).unwrap()["decision"], "block");
    assert_eq!(prompt_hook(&f, "hello", panicking, &env), (String::new(), 0));
    assert_eq!(bt(&f), (false, 2));
    // The mark follows the logging rules of the `prompt` event: a persistent condition of `activity.json` writes one
    // session-log line and changes nothing.
    let g = fx(Agent::ClaudeCode);
    let gsd = g.dir.root.clone().into_os_string();
    let genv = |k: &str| match k {
        "ZYGO_SESSION_DIR" => Some(gsd.clone()),
        "ZYGO_WORKSPACE" => Some(OsString::from(ID)),
        _ => None,
    };
    fs::write(g.dir.activity_json(), "{not json").unwrap();
    assert_eq!(prompt_hook(&g, "/zygo:status", panicking, &genv), (String::new(), 0));
    assert_eq!(fs::read_to_string(g.dir.activity_json()).unwrap(), "{not json");
    let lines = slog(&g);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with(&format!("workspace hook prompt: {}: ", g.dir.activity_json().display())), "{lines:?}");
    // A launch that is not a member (`ZYGO_WORKSPACE` unset or empty) keeps today's behaviour: the error, no block,
    // and no read or write in the session folder; the hook asks the environment for nothing else. A member view in
    // the folder does not change that.
    for (case, ws) in [("no marker, unset", None), ("marker, unset", None), ("marker, empty", Some(OsString::new()))] {
        let h = if case.starts_with("no marker") { fx_with(Agent::ClaudeCode, None) } else { fx(Agent::ClaudeCode) };
        let hsd = h.dir.root.clone().into_os_string();
        let asked = Mutex::new(Vec::new());
        let henv = |k: &str| {
            asked.lock().unwrap().push(k.to_string());
            match k {
                "ZYGO_SESSION_DIR" => Some(hsd.clone()),
                "ZYGO_WORKSPACE" => ws.clone(),
                _ => None,
            }
        };
        let before = entries(&h.dir.root);
        let run = || assert_eq!(prompt_hook(&h, "/zygo:status", panicking, &henv), (String::new(), 0), "{case}");
        #[cfg(target_os = "linux")]
        assert_eq!(opened(&h.dir.root, run), Vec::<String>::new(), "{case}");
        #[cfg(not(target_os = "linux"))]
        run();
        assert_eq!(entries(&h.dir.root), before, "{case}");
        assert_eq!(*asked.lock().unwrap(), ["ZYGO_WORKSPACE"], "{case}");
    }
    // The watch sees the member's reads (a check of the check above).
    #[cfg(target_os = "linux")]
    assert!(opened(&f.dir.root, || {
        prompt_hook(&f, "/zygo:status", panicking, &env);
    })
    .contains(&"view.json".to_string()));
}

/// The deadline of the round 4 follow-up F1 tests, and the time its dispatcher waits before it panics: a panic seen
/// just before the deadline.
const F1_DEADLINE: Duration = Duration::from_millis(600);
const F1_PANIC_AT: Duration = Duration::from_millis(550);

/// A `/zygo:` dispatcher whose verb panics just before `F1_DEADLINE` (round 4 follow-up F1).
fn late_panic(_: &str, _: &str) -> anyhow::Result<String> {
    std::thread::sleep(F1_PANIC_AT);
    panic!("verb panicked")
}

#[test] // base spec §17.1 (the hook ends within 5 s), C6; H1 attempt-2 review round 4 follow-up F1
#[ignore = "timing: a 550 ms sleep races a 600 ms deadline and the elapsed time is asserted"]
fn busy_mark_after_a_late_panic_keeps_the_hook_bound() {
    let _s = serial();
    let f = fx(Agent::ClaudeCode);
    let sd = f.dir.root.clone().into_os_string();
    let env = |k: &str| match k {
        "ZYGO_SESSION_DIR" => Some(sd.clone()),
        "ZYGO_WORKSPACE" => Some(OsString::from(ID)),
        _ => None,
    };
    set_act(&f, false, 2);
    let input = hook(Agent::ClaudeCode, &ups("/zygo:status"));
    let held = hold_activity(&f.dir);
    let start = Instant::now();
    let r = prompt::run_in_env(&input, Some(late_panic), F1_DEADLINE, &f.paths, &env);
    let took = start.elapsed();
    drop(held);
    // `HOOK_DEADLINE` (4.5 s) leaves 500 ms of the 5 s bound of §17.1 after the verb wait; the busy mark waits for
    // `activity.lock` only for the time left of the verb wait, so the hook ends within that margin of its deadline.
    assert!(took < F1_DEADLINE + Duration::from_millis(500), "the hook took {took:?}");
    let Err(e) = r else { panic!("not an error") };
    let text = format!("{e:#}");
    let head = format!(
        "prompt: verb `status` ended without a reply; busy mark: {}: lock not taken within ",
        f.dir.activity_lock().display()
    );
    assert!(text.starts_with(&head) && text.ends_with(" ms") && !text.contains('\n'), "{text}");
    // The lock miss is in the one stderr line of C6, not in the session log; nothing changed.
    assert!(slog(&f).is_empty(), "{:?}", slog(&f));
    assert_eq!(bt(&f), (false, 2));
    // No time left: the mark tries the lock once, does not wait, and returns the miss for the C6 line.
    let held = hold_activity(&f.dir);
    let start = Instant::now();
    let e = workspace::mark_busy(&f.paths, &env, Duration::ZERO).unwrap_err();
    assert!(start.elapsed() < Duration::from_millis(100), "{:?}", start.elapsed());
    drop(held);
    assert_eq!(format!("{e:#}"), format!("{}: lock not taken within 0 ms", f.dir.activity_lock().display()));
    assert!(slog(&f).is_empty(), "{:?}", slog(&f));
    assert_eq!(bt(&f), (false, 2));
    // With the lock free, a zero bound still takes it and marks the member busy.
    workspace::mark_busy(&f.paths, &env, Duration::ZERO).unwrap();
    assert_eq!(bt(&f), (true, 0));
}

#[test] // W§9.1 `UserPromptSubmit` row, AU58, C6; H1 attempt-2 review round 4 follow-up F6
fn busy_mark_needs_the_view_marker_and_keeps_one_stderr_line() {
    let _s = serial();
    // `ZYGO_WORKSPACE` set, but the view has no marker: no write, no `activity.json`, no session-log line.
    let f = fx_with(Agent::ClaudeCode, None);
    let sd = f.dir.root.clone().into_os_string();
    let env = |k: &str| match k {
        "ZYGO_SESSION_DIR" => Some(sd.clone()),
        "ZYGO_WORKSPACE" => Some(OsString::from(ID)),
        _ => None,
    };
    let before = entries(&f.dir.root);
    let input = hook(Agent::ClaudeCode, &ups("/zygo:status"));
    let Err(e) = prompt::run_in_env(&input, Some(panicking), NO_DEADLINE, &f.paths, &env) else {
        panic!("not an error")
    };
    assert_eq!(format!("{e:#}"), "prompt: verb `status` ended without a reply");
    assert_eq!(entries(&f.dir.root), before);
    assert!(!f.dir.activity_json().exists() && !f.dir.log().exists() && !f.paths.state_dir().exists());
    // A refused session folder (a relative `ZYGO_SESSION_DIR`): the mark's error joins the one stderr line that
    // `guarded` writes. `guarded` writes to the process stderr, so the test binary runs `prompt_hook_child` in a child
    // and reads its stderr.
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "prompt_hook_child", "--nocapture", "--test-threads=1"])
        .env("ZYGO_H1_PROMPT_CHILD", f.t.path())
        .env("ZYGO_SESSION_DIR", "s")
        .env("ZYGO_WORKSPACE", ID)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "zygo hook: prompt: verb `status` ended without a reply; busy mark: ZYGO_SESSION_DIR is not an absolute path: s\n"
    );
    assert!(String::from_utf8(out.stdout).unwrap().contains("prompt_hook_child ... ok"));
}

/// Child half of `busy_mark_needs_the_view_marker_and_keeps_one_stderr_line`: `zygo hook prompt` as the binary runs
/// it (`guarded`, the process environment) with a panicking verb; the parent reads its stderr and sets
/// `ZYGO_H1_PROMPT_CHILD` (a test-owned folder for the paths) on the child only. Without it the test returns at once.
#[test]
#[ignore]
fn prompt_hook_child() {
    let Some(root) = std::env::var_os("ZYGO_H1_PROMPT_CHILD") else { return };
    let paths = paths(Path::new(&root));
    let bytes = serde_json::to_vec(&ups("/zygo:status")).unwrap();
    let env = |k: &str| std::env::var_os(k);
    let out = guarded(
        &|b| prompt::run_in_env(&HookInput::parse(Agent::ClaudeCode, b)?, Some(panicking), NO_DEADLINE, &paths, &env),
        &bytes,
    );
    assert_eq!(out, (String::new(), 0));
}

/// Writes `n` message files with body `{}` into `new/` of the member `KEY` (made when missing) and returns that folder.
fn put_new(f: &Fx, n: usize) -> PathBuf {
    let new = mail_dir(&f.paths, ID).join(KEY).join("new");
    fs::create_dir_all(&new).unwrap();
    for i in 0..n {
        fs::write(new.join(format!("01J{i:023}.json")), "{}").unwrap();
    }
    new
}

/// Sets `[features] mail` on in the view of `f` (M§8, M§8a), as the launcher writes it for a member with mail on.
fn mail_on(f: &Fx) {
    let mut v = SessionView::load(&f.dir).unwrap();
    v.mail = true;
    v.save(&f.dir).unwrap();
}

#[test] // M§7.1, M§7.2
fn mail_lines_name_the_count_and_the_tool() {
    assert_eq!(mail::delivery_line(2), L2);
    assert_eq!(serde_json::from_str::<Value>(&mail::stop_block(2)).unwrap(), json!({"decision": "block", "reason": L2}));
}

#[test] // M§7.1, AU43; ML-10 D1, D2
fn mail_stop_blocks_once_and_keeps_busy() {
    let _s = serial();
    let f = fx(Agent::ClaudeCode);
    mail_on(&f);
    put_new(&f, 2);
    set_act(&f, true, 2);
    assert_eq!(parsed(ws(&f, "stop", fixture("Stop-main"))), json!({"decision": "block", "reason": L2}));
    assert_eq!(bt(&f), (true, 2)); // blocking: activity.json not changed
    assert_eq!(ws(&f, "stop", with(fixture("Stop-main"), "stop_hook_active", json!(true))).unwrap(), None);
    assert_eq!(bt(&f), (false, 0));
}

#[test] // M§7.1, M§13 #4, AU43; ML-10 D1, D2
fn mail_stop_without_a_block_clears_busy() {
    let _s = serial();
    let f = fx(Agent::ClaudeCode);
    mail_on(&f);
    set_act(&f, true, 1);
    assert_eq!(ws(&f, "stop", fixture("Stop-main")).unwrap(), None); // no mail/ at all
    assert_eq!(bt(&f), (false, 0));
    let new = put_new(&f, 0);
    set_act(&f, true, 1);
    assert_eq!(ws(&f, "stop", fixture("Stop-main")).unwrap(), None); // empty new/
    assert_eq!(bt(&f), (false, 0));
    put_new(&f, 2);
    set_act(&f, true, 1);
    assert_eq!(ws(&f, "stop", with(fixture("Stop-main"), "hook_event_name", json!("StopFailure"))).unwrap(), None);
    assert_eq!(bt(&f), (false, 0));
    set_act(&f, true, 1);
    assert_eq!(ws(&f, "stop", with(fixture("Stop-main"), "agent_id", json!("a1"))).unwrap(), None);
    assert_eq!(bt(&f), (true, 1)); // not the main loop (AU43)
    let o = fx(Agent::OpenCode);
    mail_on(&o);
    put_new(&o, 2);
    set_act(&o, true, 0);
    assert_eq!(ws(&o, "stop", oc("ses_A")).unwrap(), None);
    assert_eq!(bt(&o), (false, 0));
    assert_eq!(entries(&new).len(), 2); // no message moved
}

#[test] // M§7.2, AU86; ML-10 D3
fn mail_prompt_adds_the_line_and_the_stopping_block_wins() {
    let _s = serial();
    let f = fx(Agent::ClaudeCode);
    mail_on(&f);
    assert_eq!(ws(&f, "prompt", ups("hello")).unwrap(), None); // no mail
    put_new(&f, 2);
    set_act(&f, false, 0);
    assert_eq!(
        parsed(ws(&f, "prompt", ups("hello"))),
        json!({"hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "additionalContext": L2}})
    );
    assert_eq!(bt(&f), (true, 0));
    let store = ws_state(&f, Phase::Stopping, MemberRecord::new(Agent::ClaudeCode));
    set_act(&f, false, 0);
    let _op = store.op_lock().unwrap();
    assert_eq!(parsed(ws(&f, "prompt", ups("hello"))), json!({"decision": "block", "reason": "zygo: workspace is stopping"}));
    assert_eq!(bt(&f), (false, 0));
    let o = fx(Agent::OpenCode);
    mail_on(&o);
    put_new(&o, 2);
    assert_eq!(ws(&o, "prompt", oc("ses_A")).unwrap(), None); // D3
    assert_eq!(bt(&o), (true, 0));
}

#[test] // M§7.2, M§8a, W§9.4; ML-10 D5
fn mail_session_start_line_follows_the_resume_note() {
    let _s = serial();
    let f = fx_note();
    mail_on(&f);
    let store = ws_state(&f, Phase::Up, rec_pending());
    let missing = f.t.path().join("missing");
    put_new(&f, 2);
    let item = mail::session_item(&store.read().unwrap().unwrap(), KEY).unwrap();
    assert_eq!(ss_text(&f, &ss(AS1), &missing), format!("{}\n\n{item}\n\n{I1}\n{U12}\n\n{L2}", base(&f)));
    // No mail: no line.
    let g = fx_note();
    mail_on(&g);
    let item = mail::session_item(&ws_state(&g, Phase::Up, rec_pending()).read().unwrap().unwrap(), KEY).unwrap();
    let missing = g.t.path().join("missing");
    assert_eq!(ss_text(&g, &ss(AS1), &missing), format!("{}\n\n{item}\n\n{I1}\n{U12}", base(&g)));
}

#[test] // M§7.2, AU104; ML-10 D5
fn mail_opencode_request_text_holds_the_line_while_new_is_not_empty() {
    let _s = serial();
    let f = fx(Agent::OpenCode);
    mail_on(&f);
    let missing = f.t.path().join("missing");
    let new = put_new(&f, 2);
    for _ in 0..2 {
        assert_eq!(ss_text(&f, &oc_start(), &missing), format!("{}\n\n{L2}", base(&f)));
    }
    for e in entries(&new) {
        fs::remove_file(new.join(e)).unwrap();
    }
    assert_eq!(ss_text(&f, &oc_start(), &missing), base(&f));
}

/// Child half of `mail_unreadable_folder_changes_no_decision`: one hook call as the binary runs it, so its stderr line
/// can be read. `ZYGO_ML10_CHILD` is the fixture's temp root and `ZYGO_ML10_EVENT` the event (`stop`, `prompt` or
/// `session-start`); an `Err` fails the child. It prints `OUT=<JSON string of the output text>` (`""` for none). In a
/// normal run it returns at once.
#[test]
fn mail_hook_child() {
    let Some(root) = std::env::var_os("ZYGO_ML10_CHILD") else { return };
    let event = std::env::var("ZYGO_ML10_EVENT").unwrap();
    let root = PathBuf::from(root);
    let paths = paths(&root);
    let dir = SessionDir::new(paths.sessions_dir.join(SID));
    let zygo = root.join("missing");
    let sd = dir.root.clone().into_os_string();
    let env = |k: &str| if k == "ZYGO_SESSION_DIR" { Some(sd.clone()) } else { None };
    let out = match event.as_str() {
        "stop" | "prompt" => {
            let payload = if event == "stop" { fixture("Stop-main") } else { ups("hello") };
            workspace::run(&event, &serde_json::to_vec(&payload).unwrap(), &paths, &env, &zygo).unwrap().unwrap_or_default()
        }
        "session-start" => {
            let input = match SessionView::load(&dir).unwrap().agent {
                Agent::ClaudeCode => ss(AS1),
                Agent::OpenCode => oc_start(),
            };
            let mut printed = Vec::new();
            match session_start::run_member(&input, &dir, &paths, &zygo, &mut printed).unwrap() {
                HookOutput::None if printed.is_empty() => String::new(),
                HookOutput::None => context(&serde_json::from_slice(&printed).unwrap()),
                HookOutput::Json(v) => context(&v),
                HookOutput::Text(t) => t,
            }
        }
        e => panic!("unknown event {e}"),
    };
    println!("OUT={}", serde_json::to_string(&out).unwrap());
}

/// Runs `mail_hook_child` for `event` on the fixture `f`; asserts it exits 0 and returns the `OUT=` text (libtest
/// prints it after `test mail_hook_child ... ` on the same line) and the
/// stderr lines that start with `zygo hook: `.
fn child(f: &Fx, event: &str) -> (String, Vec<String>) {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "mail_hook_child", "--nocapture", "--test-threads=1"])
        .env("ZYGO_ML10_CHILD", f.t.path())
        .env("ZYGO_ML10_EVENT", event)
        .env_remove("ZYGO_SESSION_DIR")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let line = stdout.lines().find_map(|l| l.split_once("OUT=").map(|(_, t)| t)).unwrap_or_else(|| panic!("no OUT= line: {stdout}"));
    let text: String = serde_json::from_str(line).unwrap();
    let err = String::from_utf8(out.stderr).unwrap().lines().filter(|l| l.starts_with("zygo hook: ")).map(String::from).collect();
    (text, err)
}

/// `put_new(f, 2)`, then the mail folder of `f` to mode `0o000`.
fn lock_mail(f: &Fx) -> PathBuf {
    put_new(f, 2);
    let m = mail_dir(&f.paths, ID);
    fs::set_permissions(&m, fs::Permissions::from_mode(0o000)).unwrap();
    m
}

#[test] // M-C1, M§10 "Hook cannot read `mail/`", M§13 #9, C6; ML-10 D4
fn mail_unreadable_folder_changes_no_decision() {
    let _s = serial();
    // Root reads a folder of mode 0o000, so the case is skipped there.
    // SAFETY: `geteuid` takes no argument and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped the unreadable mail folder case: the test runs as root");
        return;
    }
    // Claude Code
    let f = fx(Agent::ClaudeCode);
    mail_on(&f);
    let m = lock_mail(&f);
    set_act(&f, true, 1);
    let stop = child(&f, "stop");
    let stop_bt = bt(&f);
    set_act(&f, false, 0);
    let prompt = child(&f, "prompt");
    let prompt_bt = bt(&f);
    let start = child(&f, "session-start");
    fs::set_permissions(&m, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!((stop.0.as_str(), stop.1.len()), ("", 1), "{stop:?}");
    assert_eq!(stop_bt, (false, 0));
    assert_eq!((prompt.0.as_str(), prompt.1.len()), ("", 1), "{prompt:?}");
    assert_eq!(prompt_bt, (true, 0));
    assert_eq!((start.0, start.1.len()), (base(&f), 1), "{:?}", start.1);
    // OpenCode
    let o = fx(Agent::OpenCode);
    mail_on(&o);
    let m = lock_mail(&o);
    let start = child(&o, "session-start");
    fs::set_permissions(&m, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!((start.0, start.1.len()), (base(&o), 1), "{:?}", start.1);
}

#[test] // M§7.2 last sentence, M-C2
fn mail_no_hook_opens_a_message_file() {
    let _s = serial();
    let f = fx_note();
    mail_on(&f);
    ws_state(&f, Phase::Up, rec_pending());
    let missing = f.t.path().join("missing");
    let new = put_new(&f, 2);
    for e in entries(&new) {
        fs::set_permissions(new.join(e), fs::Permissions::from_mode(0o000)).unwrap();
    }
    assert_eq!(parsed(ws(&f, "stop", fixture("Stop-main"))), json!({"decision": "block", "reason": L2})); // counted unopened
    assert_eq!(
        parsed(ws(&f, "prompt", ups("hello"))),
        json!({"hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "additionalContext": L2}})
    );
    assert!(ss_text(&f, &ss(AS1), &missing).ends_with(&format!("\n\n{L2}")));
    // The watch needs readable message files: an open that fails gives no `IN_OPEN` event, so it would see nothing.
    #[cfg(target_os = "linux")]
    {
        for e in entries(&new) {
            fs::set_permissions(new.join(e), fs::Permissions::from_mode(0o600)).unwrap();
        }
        let names = opened(&new, || {
            assert_eq!(parsed(ws(&f, "stop", fixture("Stop-main"))), json!({"decision": "block", "reason": L2}));
            assert_eq!(
                parsed(ws(&f, "prompt", ups("hello"))),
                json!({"hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "additionalContext": L2}})
            );
            assert!(ss_text(&f, &ss(AS1), &missing).ends_with(&format!("\n\n{L2}")));
        });
        assert!(!names.iter().any(|n| n.ends_with(".json")), "{names:?}");
        assert!(names.iter().any(|n| n == "."), "{names:?}"); // positive control: the hooks opened `new/` itself
    }
}

#[test] // M§8, M§8a item 1; ML-10 review R1
fn mail_off_reads_no_mail() {
    let _s = serial();
    // Claude Code: no block, `busy` cleared, no prompt line, no SessionStart line.
    let f = fx(Agent::ClaudeCode);
    let missing = f.t.path().join("missing");
    put_new(&f, 2);
    set_act(&f, true, 1);
    assert_eq!(ws(&f, "stop", fixture("Stop-main")).unwrap(), None);
    assert_eq!(bt(&f), (false, 0));
    assert_eq!(ws(&f, "prompt", ups("hello")).unwrap(), None);
    assert_eq!(bt(&f), (true, 0));
    assert_eq!(ss_text(&f, &ss(AS1), &missing), base(&f));
    // OpenCode: no per-request line.
    let o = fx(Agent::OpenCode);
    let missing = o.t.path().join("missing");
    put_new(&o, 2);
    assert_eq!(ss_text(&o, &oc_start(), &missing), base(&o));
    // `mail/` is not read at all: an unreadable `mail/` writes no stderr line. Root reads it, so that part is skipped.
    // SAFETY: `geteuid` takes no argument and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped the unreadable mail folder part: the test runs as root");
        return;
    }
    let g = fx(Agent::ClaudeCode);
    let m = lock_mail(&g);
    set_act(&g, true, 1);
    let stop = child(&g, "stop");
    let stop_bt = bt(&g);
    let prompt = child(&g, "prompt");
    let start = child(&g, "session-start");
    fs::set_permissions(&m, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!((stop.0.as_str(), stop.1.len()), ("", 0), "{stop:?}");
    assert_eq!(stop_bt, (false, 0));
    assert_eq!((prompt.0.as_str(), prompt.1.len()), ("", 0), "{prompt:?}");
    assert_eq!((start.0, start.1.len()), (base(&g), 0), "{:?}", start.1);
}

/// A Claude Code `stop` with 2 unread in phase `phase`: no block, and `busy` is cleared (M§9: `down` does not wait for
/// unread mail).
fn stop_in_phase_clears(phase: Phase) {
    let f = fx(Agent::ClaudeCode);
    mail_on(&f);
    ws_state(&f, phase, MemberRecord::new(Agent::ClaudeCode));
    put_new(&f, 2);
    set_act(&f, true, 1);
    let stop = fixture("Stop-main");
    assert_eq!(stop.get("stop_hook_active"), Some(&json!(false)));
    assert_eq!(ws(&f, "stop", stop).unwrap(), None, "{phase:?}");
    assert_eq!(bt(&f), (false, 0), "{phase:?}");
}

#[test] // M§7.1, M§9; ML-10 review R3
fn mail_stop_does_not_block_while_stopping() {
    let _s = serial();
    stop_in_phase_clears(Phase::Stopping);
}

#[test] // M§7.1, M§9; ML-10 review R3
fn mail_stop_does_not_block_while_draining() {
    let _s = serial();
    stop_in_phase_clears(Phase::Draining);
}

#[test] // M§7.1; ML-10 review R6
fn mail_stop_without_the_active_key_does_not_block() {
    let _s = serial();
    let f = fx(Agent::ClaudeCode);
    mail_on(&f);
    put_new(&f, 2);
    set_act(&f, true, 1);
    assert_eq!(ws(&f, "stop", without(fixture("Stop-main"), "stop_hook_active")).unwrap(), None);
    assert_eq!(bt(&f), (false, 0));
}

#[test] // M-C1, M§10, C6; ML-10 review R6
fn mail_new_that_is_a_file_changes_no_decision() {
    let _s = serial();
    let f = fx(Agent::ClaudeCode);
    mail_on(&f);
    let key = mail_dir(&f.paths, ID).join(KEY);
    fs::create_dir_all(&key).unwrap();
    fs::write(key.join("new"), "x").unwrap();
    set_act(&f, true, 1);
    let stop = child(&f, "stop");
    let stop_bt = bt(&f);
    set_act(&f, false, 0);
    let prompt = child(&f, "prompt");
    let prompt_bt = bt(&f);
    let start = child(&f, "session-start");
    assert_eq!((stop.0.as_str(), stop.1.len()), ("", 1), "{stop:?}");
    assert_eq!(stop_bt, (false, 0));
    assert_eq!((prompt.0.as_str(), prompt.1.len()), ("", 1), "{prompt:?}");
    assert_eq!(prompt_bt, (true, 0));
    assert_eq!((start.0, start.1.len()), (base(&f), 1), "{:?}", start.1);
}

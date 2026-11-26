//! The start of the mail sync child from the member hooks and from a send (mail sync design S§8.1 rows 1 and 2,
//! S§8.4, S§15 `mail/sync/last_start`, S§19 #22, S-C1, S-C9). Every test has a counting spawn seam and a fixed
//! injected clock: no test waits and no test runs the real child. Workspaces need tmux (OP5), so the file is unix
//! only.
#![cfg(unix)]

use std::cell::Cell;
use std::fs;
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_core::hook::{guarded, HookOutput};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{Member, ResumeNote, SessionView, ViewWorkspace};
use zygo_core::Agent;
use zygo_hooks::workspace;
use zygo_mail_sync::env::{Clock, Env, NoSeam, Probe, Spawner, Verdict};
use zygo_mail_sync::spawn::{self, due, ChildSpawner};
use zygo_mail_sync::state::{read_last_start, write_last_start, SyncPaths};
use zygo_workspace::mail::mail_dir;
use zygo_workspace::state::{MemberRecord, Phase, Store};

const ID: &str = "0123456789abcdef";
const KEY: &str = "kaaaaaaaa";
const SID: &str = "01JTESTSESSIONAAAAAAAAAAAA";
const T: u64 = 1_800_000_000;
const S5: Duration = Duration::from_secs(5);
const GAP: Duration = Duration::from_secs(60);
const L2: &str = "zygo: 2 new messages; read them with mail_read";

/// Sync on for the workspace `app`.
const ON: &str = "[mail]\nhost = \"studio\"\n\n[mail.workspace.app]\nsync = \"zygo/mail\"\n";
/// Mail on, sync off.
const OFF: &str = "[mail]\nhost = \"studio\"\n";

/// `data_dir` = `<p>/data` is not under `$HOME`, so the state folder is `<p>/data/state`.
fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}

struct Fx {
    _t: TempDir,
    paths: Paths,
    dir: SessionDir,
    zygo: PathBuf,
}

/// A member `KEY` of the workspace `app` (`ID`), phase `up`, whose record belongs to the session `SID`, so a
/// `session` call changes nothing and logs nothing; `config` is the user config.
fn fx(agent: Agent, config: &str) -> Fx {
    let t = TempDir::new().unwrap();
    let paths = paths(t.path());
    let dir = SessionDir::new(paths.sessions_dir.join(SID));
    dir.create().unwrap();
    let marker = ViewWorkspace { member: Member { ws_id: ID.into(), pane: KEY.into(), ..Default::default() }, ..Default::default() };
    SessionView { session: SID.into(), agent, main_role: "lead".into(), workspace: Some(marker), ..Default::default() }
        .save(&dir)
        .unwrap();
    let mut rec = MemberRecord::new(agent);
    rec.zygo_session = Some(SID.into());
    let root = t.path().to_path_buf();
    Store::open(&paths, ID)
        .update(S5, &mut |w| {
            w.name = "app".into();
            w.tmux_session = "zygo-app".into();
            w.common_dir = root.join("app/.git");
            w.main_root = root.join("app");
            w.loadout = "solo".into();
            w.phase = Phase::Up;
            w.members.insert(KEY.into(), rec.clone());
            Ok(())
        })
        .unwrap();
    fs::write(&paths.config_file, config).unwrap();
    let zygo = t.path().join("missing");
    Fx { _t: t, paths, dir, zygo }
}

fn sp(f: &Fx) -> SyncPaths {
    SyncPaths::new(&f.paths, ID).unwrap()
}

/// The counting spawn seam: each call counts and records the `last_start` it sees.
#[derive(Clone)]
struct Spawns {
    n: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<Option<u64>>>>,
    sp: SyncPaths,
    fail: bool,
}

impl Spawner for Spawns {
    fn spawn_sync(&self, _ws_id: &str) -> io::Result<()> {
        self.n.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push(read_last_start(&self.sp));
        if self.fail { Err(io::Error::other("refused")) } else { Ok(()) }
    }
}

impl Spawns {
    fn count(&self) -> usize {
        self.n.load(Ordering::SeqCst)
    }
    fn seen(&self) -> Vec<Option<u64>> {
        self.seen.lock().unwrap().clone()
    }
}

fn spawns(f: &Fx, fail: bool) -> Spawns {
    Spawns { n: Arc::default(), seen: Arc::default(), sp: sp(f), fail }
}

/// The fixed clock.
struct At(SystemTime);

impl Clock for At {
    fn now(&self) -> SystemTime {
        self.0
    }
}

/// The hook path runs no git (review focus #49).
struct NoGit;

impl Probe for NoGit {
    fn probe(&self, _https_url: &str) -> Verdict {
        panic!("the hook path runs no git")
    }
}

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn env_at(f: &Fx, secs: u64, s: &Spawns) -> Env {
    Env { paths: f.paths.clone(), clock: Box::new(At(at(secs))), probe: Box::new(NoGit), seam: Box::new(NoSeam), spawner: Box::new(s.clone()) }
}

fn hook(f: &Fx, event: &str, payload: &Value, sync: &Env) -> anyhow::Result<Option<String>> {
    let sd = f.dir.root.clone().into_os_string();
    workspace::run_with_sync(
        event,
        &serde_json::to_vec(payload).unwrap(),
        &f.paths,
        &|k| (k == "ZYGO_SESSION_DIR").then(|| sd.clone()),
        &f.zygo,
        None,
        sync,
    )
}

/// `hook` under the C6 wrapper, as `zygo hook workspace` runs it (`crates/zygo/src/cmd/hook.rs`).
fn guarded_hook(f: &Fx, event: &str, payload: &Value, sync: &Env) -> (String, i32) {
    let bytes = serde_json::to_vec(payload).unwrap();
    guarded(
        &|_| {
            Ok(match hook(f, event, payload, sync)? {
                Some(t) => HookOutput::Text(t),
                None => HookOutput::None,
            })
        },
        &bytes,
    )
}

/// The lines of `errors.log` of the workspace, each without its `<secs> ` head.
fn errors(f: &Fx) -> Vec<String> {
    let Ok(text) = fs::read_to_string(f.paths.workspaces_dir().join(ID).join("errors.log")) else { return Vec::new() };
    text.lines().map(|l| l.split_once(' ').map_or(l, |(_, rest)| rest).to_string()).collect()
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

fn tool_end() -> Value {
    json!({})
}
fn prompt() -> Value {
    json!({"prompt": "hi"})
}
fn stop() -> Value {
    json!({"hook_event_name": "Stop", "stop_hook_active": false})
}
fn session() -> Value {
    json!({})
}
fn sub_session() -> Value {
    json!({"agent_id": "a1"})
}

/// The Claude Code member part of `SessionStart` (`session-start --event member`, W§9.1) with the session text
/// `TEXT`: the result and the bytes written to stdout.
fn session_start(f: &Fx, payload: &Value, sync: &Env) -> (Option<String>, Vec<u8>) {
    let v = SessionView::load(&f.dir).unwrap();
    let mut out = Vec::new();
    let r = workspace::session_start_with_sync(payload, &f.dir, &v, &f.paths, &f.zygo, &mut out, "TEXT", sync).unwrap();
    (r, out)
}

/// Puts resume note inputs into the view of `f` (W§9.4).
fn note_on(f: &Fx) {
    let mut v = SessionView::load(&f.dir).unwrap();
    v.workspace.as_mut().unwrap().resume_note =
        Some(ResumeNote { clean_stop: true, closed_units: vec!["u1".into()], ..Default::default() });
    v.save(&f.dir).unwrap();
}

/// Makes `mail/sync` a regular file, so the write of `mail/sync/last_start` fails.
fn block_last_start(f: &Fx) {
    let mail = mail_dir(&f.paths, ID);
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&mail).unwrap();
    fs::write(mail.join("sync"), "").unwrap();
}

// ---- Rate limit (S§19 #22, S§8.4) ----

#[test] // S§19 #22, S§8.1 the `tool-end` reason
fn ten_tool_end_events_start_one_child() {
    let f = fx(Agent::ClaudeCode, ON);
    let s = spawns(&f, false);
    for _ in 0..10 {
        assert!(hook(&f, "tool-end", &tool_end(), &env_at(&f, T, &s)).unwrap().is_none());
    }
    assert_eq!(s.count(), 1);
    assert_eq!(read_last_start(&sp(&f)), Some(T));
    assert!(hook(&f, "tool-end", &tool_end(), &env_at(&f, T + 59, &s)).unwrap().is_none());
    assert_eq!(s.count(), 1);
    assert!(hook(&f, "tool-end", &tool_end(), &env_at(&f, T + 60, &s)).unwrap().is_none());
    assert_eq!(s.count(), 2);
    assert_eq!(read_last_start(&sp(&f)), Some(T + 60));
    assert!(errors(&f).is_empty());
}

#[test] // S§8.1 rows 1 and 2, S§8.4
fn session_prompt_and_stop_start_a_child_by_the_same_gap() {
    for (event, payload, agent) in
        [("session", session(), Agent::OpenCode), ("prompt", prompt(), Agent::ClaudeCode), ("stop", stop(), Agent::ClaudeCode)]
    {
        let f = fx(agent, ON);
        let s = spawns(&f, false);
        hook(&f, event, &payload, &env_at(&f, T, &s)).unwrap();
        assert_eq!(s.count(), 1, "{event}");
        hook(&f, event, &payload, &env_at(&f, T + 59, &s)).unwrap();
        assert_eq!(s.count(), 1, "{event}");
        hook(&f, event, &payload, &env_at(&f, T + 60, &s)).unwrap();
        assert_eq!(s.count(), 2, "{event}");
        assert!(errors(&f).is_empty(), "{event}");
    }
}

#[test] // S§8.1 row 1: `session` (main loop)
fn session_outside_the_main_loop_starts_no_child() {
    let f = fx(Agent::OpenCode, ON);
    let s = spawns(&f, false);
    hook(&f, "session", &sub_session(), &env_at(&f, T, &s)).unwrap();
    assert_eq!(s.count(), 0);
    assert!(!sp(&f).last_start.exists());
}

#[test] // S§8.1 row 2, S§10.2 item 2, W§9.1: Claude Code `SessionStart` is the member `session` event (review F1)
fn claude_code_session_start_starts_a_child_by_the_gap() {
    let f = fx(Agent::ClaudeCode, ON);
    let s = spawns(&f, false);
    let (r, out) = session_start(&f, &session(), &env_at(&f, T, &s));
    assert_eq!(r, None);
    assert!(!out.is_empty());
    assert_eq!(s.count(), 1);
    assert_eq!(read_last_start(&sp(&f)), Some(T));
    session_start(&f, &session(), &env_at(&f, T + 59, &s));
    assert_eq!(s.count(), 1);
    session_start(&f, &session(), &env_at(&f, T + 60, &s));
    assert_eq!(s.count(), 2);
    assert!(errors(&f).is_empty());
}

#[test] // S§8.1 row 1: `session` (main loop) (review F1)
fn a_subagent_session_start_starts_no_child() {
    let f = fx(Agent::ClaudeCode, ON);
    let s = spawns(&f, false);
    session_start(&f, &sub_session(), &env_at(&f, T, &s));
    assert_eq!(s.count(), 0);
    assert!(!sp(&f).last_start.exists());
}

#[test] // S§8.1, lead decision 5: only `session` is limited to the main loop (review F8)
fn a_subagent_tool_end_starts_a_child_by_the_gap() {
    let f = fx(Agent::ClaudeCode, ON);
    let s = spawns(&f, false);
    let sub = json!({"agent_id": "a1"});
    for (secs, n) in [(T, 1), (T + 59, 1), (T + 60, 2)] {
        assert!(hook(&f, "tool-end", &sub, &env_at(&f, secs, &s)).unwrap().is_none());
        assert_eq!(s.count(), n, "at T + {}", secs - T);
    }
}

#[test] // S§8.4: a `last_start` later than now (a clock set back, or a corrupt file) does not stop hook starts (review F2)
fn a_last_start_later_than_now_does_not_stop_the_start() {
    let f = fx(Agent::ClaudeCode, ON);
    write_last_start(&sp(&f), 99_999_999_999).unwrap();
    let s = spawns(&f, false);
    hook(&f, "tool-end", &tool_end(), &env_at(&f, T, &s)).unwrap();
    assert_eq!(s.count(), 1);
    assert_eq!(read_last_start(&sp(&f)), Some(T));
}

#[test] // S§8.4, S§11 `sync_gap`
fn the_gap_comes_from_mail_sync_gap() {
    let f = fx(Agent::ClaudeCode, &ON.replacen("[mail]\n", "[mail]\nsync_gap = \"5m\"\n", 1));
    let s = spawns(&f, false);
    for (secs, n) in [(T, 1), (T + 60, 1), (T + 299, 1), (T + 300, 2)] {
        hook(&f, "tool-end", &tool_end(), &env_at(&f, secs, &s)).unwrap();
        assert_eq!(s.count(), n, "at T + {}", secs - T);
    }
}

#[test] // S§8.4: `last_start` is written before the spawn
fn last_start_is_written_before_the_spawn() {
    let f = fx(Agent::ClaudeCode, ON);
    let s = spawns(&f, false);
    hook(&f, "tool-end", &tool_end(), &env_at(&f, T, &s)).unwrap();
    assert_eq!(s.seen(), vec![Some(T)]);
}

// ---- A send, and sync off ----

#[test] // S§8.1 row 3, S§15: the hook path is the only writer of `last_start`
fn a_send_starts_a_child_with_no_gap() {
    let f = fx(Agent::ClaudeCode, ON);
    let s = spawns(&f, false);
    let logged = Cell::new(0);
    for _ in 0..3 {
        spawn::trigger(&env_at(&f, T, &s), ID, None, &|_| logged.set(logged.get() + 1));
    }
    assert_eq!(s.count(), 3);
    assert_eq!(s.seen(), vec![None, None, None]);
    assert_eq!(read_last_start(&sp(&f)), None);
    assert_eq!(logged.get(), 0);
}

#[test] // S-G6, S§11: no sync key, another workspace's key or a bad branch: no child
fn sync_off_starts_no_child() {
    let other = ON.replace("[mail.workspace.app]", "[mail.workspace.other]");
    let bad = ON.replace("zygo/mail", "main");
    for config in [OFF, "", other.as_str(), bad.as_str()] {
        for (event, payload, agent) in [
            ("tool-end", tool_end(), Agent::ClaudeCode),
            ("prompt", prompt(), Agent::ClaudeCode),
            ("stop", stop(), Agent::ClaudeCode),
            ("session", session(), Agent::OpenCode),
        ] {
            let f = fx(agent, config);
            if config.is_empty() {
                fs::remove_file(&f.paths.config_file).unwrap();
            }
            let s = spawns(&f, false);
            hook(&f, event, &payload, &env_at(&f, T, &s)).unwrap();
            assert_eq!(s.count(), 0, "{event} {config:?}");
            assert!(!sp(&f).last_start.exists(), "{event} {config:?}");
            assert!(errors(&f).is_empty(), "{event} {config:?}");
        }
    }
}

// ---- S-C1 (review focus #49) ----

#[test] // S-C1, S§14 "Sync child does not start"
fn a_failed_spawn_changes_nothing_but_one_error_line() {
    for (event, payload, agent, mail) in [
        ("tool-end", tool_end(), Agent::ClaudeCode, 0),
        ("prompt", prompt(), Agent::ClaudeCode, 2),
        ("stop", stop(), Agent::ClaudeCode, 2),
        ("session", session(), Agent::OpenCode, 0),
    ] {
        let a = fx(agent, OFF);
        let b = fx(agent, ON);
        if mail > 0 {
            for f in [&a, &b] {
                mail_on(f);
                put_new(f, mail);
            }
        }
        let sa = spawns(&a, false);
        let sb = spawns(&b, true);
        let off = guarded_hook(&a, event, &payload, &env_at(&a, T, &sa));
        let on = guarded_hook(&b, event, &payload, &env_at(&b, T, &sb));
        assert_eq!(off, on, "{event}");
        assert_eq!(on.1, 0, "{event}");
        match event {
            "prompt" => assert!(on.0.contains(L2), "{event}: {}", on.0),
            "stop" => assert_eq!(serde_json::from_str::<Value>(&on.0).unwrap(), json!({"decision": "block", "reason": L2})),
            _ => assert_eq!(on.0, "", "{event}"),
        }
        assert_eq!(sa.count(), 0, "{event}");
        assert_eq!(sb.count(), 1, "{event}");
        assert!(errors(&a).is_empty(), "{event}");
        assert_eq!(errors(&b), vec![format!("workspace hook {event}: cannot start the mail sync child: refused")]);
    }
}

#[test] // S-C1, S§8.4: the write comes before the spawn, so a failed write starts no child
fn an_unwritable_last_start_gives_one_line_and_no_child() {
    let f = fx(Agent::ClaudeCode, ON);
    let mail = mail_dir(&f.paths, ID);
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&mail).unwrap();
    fs::write(mail.join("sync"), "").unwrap();
    let s = spawns(&f, false);
    assert!(hook(&f, "tool-end", &tool_end(), &env_at(&f, T, &s)).unwrap().is_none());
    assert_eq!(s.count(), 0);
    let e = errors(&f);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].starts_with("workspace hook tool-end: cannot write mail/sync/last_start: "), "{e:?}");
}

#[test] // S-C1: a failed `last_start` write under the ML-10 `stop_block` and the prompt delivery line (review F8)
fn a_failed_last_start_write_changes_no_mail_output() {
    for (event, payload) in [("prompt", prompt()), ("stop", stop())] {
        let a = fx(Agent::ClaudeCode, OFF);
        let b = fx(Agent::ClaudeCode, ON);
        block_last_start(&b);
        for f in [&a, &b] {
            mail_on(f);
            put_new(f, 2);
        }
        let sa = spawns(&a, false);
        let sb = spawns(&b, false);
        let off = guarded_hook(&a, event, &payload, &env_at(&a, T, &sa));
        let on = guarded_hook(&b, event, &payload, &env_at(&b, T, &sb));
        assert_eq!(off, on, "{event}");
        assert_eq!(on.1, 0, "{event}");
        match event {
            "prompt" => assert!(on.0.contains(L2), "{event}: {}", on.0),
            _ => assert_eq!(serde_json::from_str::<Value>(&on.0).unwrap(), json!({"decision": "block", "reason": L2})),
        }
        assert_eq!(sb.count(), 0, "{event}");
        assert!(errors(&a).is_empty(), "{event}");
        let e = errors(&b);
        assert_eq!(e.len(), 1, "{event}: {e:?}");
        assert!(e[0].starts_with(&format!("workspace hook {event}: cannot write mail/sync/last_start: ")), "{e:?}");
    }
}

#[test] // S-C1 for the Claude Code session start: stdout, result and the resume note do not change (review F1)
fn a_failed_spawn_at_session_start_changes_nothing_but_one_error_line() {
    let a = fx(Agent::ClaudeCode, OFF);
    let b = fx(Agent::ClaudeCode, ON);
    for f in [&a, &b] {
        note_on(f);
        mail_on(f);
        put_new(f, 2);
    }
    let sa = spawns(&a, false);
    let sb = spawns(&b, true);
    let off = session_start(&a, &session(), &env_at(&a, T, &sa));
    let on = session_start(&b, &session(), &env_at(&b, T, &sb));
    assert_eq!(off, on);
    let text = String::from_utf8(on.1).unwrap();
    assert!(text.contains(L2), "{text}");
    assert!(text.contains("u1"), "{text}");
    let note = |f: &Fx| SessionView::load(&f.dir).unwrap().workspace.unwrap().resume_note;
    assert_eq!(note(&a), None);
    assert_eq!(note(&b), None);
    assert_eq!(sa.count(), 0);
    assert_eq!(sb.count(), 1);
    assert!(errors(&a).is_empty());
    assert_eq!(errors(&b), vec!["workspace hook session-start: cannot start the mail sync child: refused".to_string()]);
}

// ---- `spawn` ----

#[test] // S§8.4: `now - last_start >= sync_gap`, or `last_start` later than `now`
fn due_follows_now_minus_last_start() {
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let sp = SyncPaths::new(&p, ID).unwrap();
    assert!(due(&sp, at(T), GAP));
    write_last_start(&sp, T).unwrap();
    assert!(!due(&sp, at(T + 59), GAP));
    assert!(due(&sp, at(T + 60), GAP));
    assert!(due(&sp, at(T), Duration::ZERO));
    // Review F2: a `last_start` later than `now` (a clock set back, or a corrupt file) is due.
    write_last_start(&sp, T + 100).unwrap();
    assert!(due(&sp, at(T), GAP));
    assert!(due(&sp, at(T), Duration::ZERO));
    fs::write(&sp.last_start, "x").unwrap();
    assert!(due(&sp, at(T), GAP));
}

#[test] // S§8.1 the child start
fn child_spawner_command_is_mail_sync_background() {
    let cmd = ChildSpawner { zygo: "/opt/z/zygo".into() }.command(ID);
    assert_eq!(cmd.get_program(), "/opt/z/zygo");
    assert_eq!(cmd.get_args().collect::<Vec<_>>(), ["mail", "sync", "--ws-id", ID, "--background"]);
    assert_eq!(cmd.get_current_dir(), Some(Path::new("/")));
}

#[test] // S§14 "Sync child does not start"
fn child_spawner_reports_a_program_that_cannot_start() {
    let t = TempDir::new().unwrap();
    assert!(ChildSpawner { zygo: t.path().join("missing") }.spawn_sync(ID).is_err());
}

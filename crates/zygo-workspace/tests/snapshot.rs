//! Node W6: the snapshot: list-panes read, shrink rule, key assignment, pending rule (W§7.3, W§7.4, W§7.5).
use std::collections::{BTreeMap, VecDeque};
#[cfg(unix)]
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
#[cfg(unix)]
use std::sync::Barrier;
use std::time::{Duration, Instant, SystemTime};

use tempfile::TempDir;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_workspace::identity::valid_pane_key;
#[cfg(unix)]
use zygo_workspace::mux::testing::{require_tmux, PrivateServer};
#[cfg(unix)]
use zygo_workspace::mux::tmux::Tmux;
use zygo_workspace::mux::{HookScope, Multiplexer, MuxError, NewSession, NewWindow, SessionAnswer, Split};
use zygo_core::session_dir::write_atomic;
use zygo_workspace::snapshot::{
    after_write, assign_key, queue_pending, run, take_pending, SkipReason, SnapshotOutcome, HOOK_WAIT, OP_WAIT,
};
use zygo_workspace::state::{MemberRecord, MemberState, Pane, PaneKind, Phase, Store, Workspace};

const ID: &str = "0123456789abcdef";
const S5: Duration = Duration::from_secs(5);

fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}

/// Sets the identity fields of `up`, the phase, `tmux_socket` and `panes[]` (one shell pane per `(key, window, index)`).
fn state(store: &Store, phase: Phase, socket: Option<&Path>, panes: &[(&str, u32, u32)]) {
    store
        .update(S5, &mut |ws| {
            ws.name = "app".into(); ws.tmux_session = "zygo-app".into(); ws.common_dir = "/srv/app/.git".into();
            ws.main_root = "/srv/app".into(); ws.loadout = "solo".into(); ws.phase = phase;
            ws.tmux_socket = socket.map(Path::to_path_buf);
            ws.panes = panes
                .iter()
                .map(|&(k, w, i)| Pane { key: k.into(), window: w, index: i, cwd: "/srv/app".into(), kind: PaneKind::Shell, ..Default::default() })
                .collect();
            Ok(())
        })
        .unwrap();
}

fn raw(store: &Store) -> Vec<u8> { fs::read(store.dir().join("workspace.json")).unwrap() }
fn keys(ws: &Workspace) -> Vec<String> { ws.panes.iter().map(|p| p.key.clone()).collect() }
/// The lines of `errors.log`; empty when it is missing.
fn log_lines(store: &Store) -> Vec<String> {
    fs::read_to_string(store.dir().join("errors.log")).map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default()
}
fn hold(path: PathBuf) -> fs::File {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let f = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).unwrap();
    fs4::FileExt::try_lock(&f).unwrap();
    f
}
/// The content of `snapshot.pending`; `None` when it is missing.
fn pending(store: &Store) -> Option<String> { fs::read_to_string(store.dir().join("snapshot.pending")).ok() }
/// The number of entries whose name starts with `snapshot.pending.taken-`.
fn taken_files(store: &Store) -> usize {
    fs::read_dir(store.dir())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("snapshot.pending.taken-"))
        .count()
}
/// Replaces `workspace.json` with an empty folder.
fn to_dir(store: &Store) {
    let f = store.dir().join("workspace.json");
    fs::remove_file(&f).unwrap();
    fs::create_dir(&f).unwrap();
}

/// One `list-panes` line of `PANE_FORMAT`.
#[allow(clippy::too_many_arguments)]
fn row(w: u32, name: &str, auto: u8, wa: u8, zoom: u8, id: &str, idx: u32, active: u8, key: &str, cwd: &str) -> String {
    format!("{w}|{name}|{auto}|b25d,200x50,0,0,0|200|50|{wa}|{zoom}|{id}|{idx}|{active}|{key}|{cwd}")
}
/// [`row`] of an active window that is not zoomed.
#[allow(clippy::too_many_arguments)]
fn line(w: u32, name: &str, auto: u8, id: &str, idx: u32, active: u8, key: &str, cwd: &str) -> String {
    row(w, name, auto, 1, 0, id, idx, active, key, cwd)
}

type ListHook = Box<dyn Fn(usize) + Send + Sync>;
type SetHook = Box<dyn Fn(&str, &str) + Send + Sync>;

/// A scripted `Multiplexer` (D18).
struct Fake {
    ws_opt: Option<String>,
    exists: SessionAnswer,
    reads: Mutex<VecDeque<Result<Vec<String>, MuxError>>>,
    pane_opts: Arc<Mutex<BTreeMap<String, String>>>,
    fail: Mutex<BTreeMap<&'static str, MuxError>>,
    calls: Mutex<Vec<String>>,
    on_list: Option<ListHook>,
    on_set: Option<SetHook>,
}

impl Fake {
    fn new(ws_opt: Option<&str>, reads: Vec<Result<Vec<String>, MuxError>>) -> Fake {
        Fake {
            ws_opt: ws_opt.map(str::to_string),
            exists: SessionAnswer::Exists,
            reads: Mutex::new(reads.into()),
            pane_opts: Arc::new(Mutex::new(BTreeMap::new())),
            fail: Mutex::new(BTreeMap::new()),
            calls: Mutex::new(Vec::new()),
            on_list: None,
            on_set: None,
        }
    }
    /// The number of recorded calls that start with `prefix`.
    fn calls(&self, prefix: &str) -> usize { self.calls.lock().unwrap().iter().filter(|c| c.starts_with(prefix)).count() }
    fn opt(&self, pane: &str, value: &str) { self.pane_opts.lock().unwrap().insert(pane.into(), value.into()); }
    fn fail(&self, method: &'static str, e: MuxError) { self.fail.lock().unwrap().insert(method, e); }
    /// Records `<method> <arg>` and gives the scripted failure of `method`.
    fn record(&self, method: &'static str, arg: &str) -> Result<(), MuxError> {
        self.calls.lock().unwrap().push(format!("{method} {arg}"));
        match self.fail.lock().unwrap().get(method) {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
    fn other<T>(&self, name: &'static str) -> Result<T, MuxError> {
        self.calls.lock().unwrap().push(name.to_string());
        Err(MuxError::Failed { message: format!("fake: {name} is not scripted") })
    }
}

impl Multiplexer for Fake {
    fn version(&self) -> Result<(u32, u32), MuxError> { self.other("version") }
    fn socket_path(&self, _: &str) -> Result<PathBuf, MuxError> { self.other("socket_path") }
    fn session_exists(&self, session: &str) -> SessionAnswer {
        self.calls.lock().unwrap().push(format!("session_exists {session}"));
        self.exists.clone()
    }
    fn new_session(&self, _: &NewSession<'_>) -> Result<u32, MuxError> { self.other("new_session") }
    fn resize_window(&self, _: &str, _: u32, _: u32, _: u32) -> Result<(), MuxError> { self.other("resize_window") }
    fn unset_window_option(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { self.other("unset_window_option") }
    fn set_window_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { self.other("set_window_option") }
    fn set_env(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { self.other("set_env") }
    fn hide_env(&self, _: &str, _: &str) -> Result<(), MuxError> { self.other("hide_env") }
    fn set_session_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { self.other("set_session_option") }
    fn show_session_option(&self, session: &str, _: &str) -> Result<Option<String>, MuxError> {
        self.record("show_session_option", session)?;
        Ok(self.ws_opt.clone().filter(|v| !v.is_empty()))
    }
    fn set_hook(&self, _: &str, _: HookScope, _: &str, _: &str) -> Result<(), MuxError> { self.other("set_hook") }
    fn new_window(&self, _: &NewWindow<'_>) -> Result<u32, MuxError> { self.other("new_window") }
    fn split_window(&self, _: &Split<'_>) -> Result<String, MuxError> { self.other("split_window") }
    fn select_layout(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { self.other("select_layout") }
    fn select_window(&self, _: &str, _: u32) -> Result<(), MuxError> { self.other("select_window") }
    fn select_pane(&self, _: &str) -> Result<(), MuxError> { self.other("select_pane") }
    fn zoom(&self, _: &str) -> Result<(), MuxError> { self.other("zoom") }
    fn list_panes(&self, session: &str) -> Result<Vec<String>, MuxError> {
        self.calls.lock().unwrap().push(format!("list_panes {session}"));
        let n = self.calls("list_panes");
        if let Some(f) = &self.on_list {
            f(n);
        }
        if let Some(e) = self.fail.lock().unwrap().get("list_panes") {
            return Err(e.clone());
        }
        let mut reads = self.reads.lock().unwrap();
        if reads.len() > 1 {
            reads.pop_front().unwrap()
        } else {
            reads.front().cloned().unwrap_or(Ok(Vec::new()))
        }
    }
    fn show_pane_option(&self, pane: &str, _: &str) -> Result<Option<String>, MuxError> {
        self.record("show_pane_option", pane)?;
        Ok(self.pane_opts.lock().unwrap().get(pane).filter(|v| !v.is_empty()).cloned())
    }
    fn set_pane_option(&self, pane: &str, _: &str, value: &str) -> Result<(), MuxError> {
        self.record("set_pane_option", pane)?;
        if let Some(f) = &self.on_set {
            f(pane, value);
        }
        let mut opts = self.pane_opts.lock().unwrap();
        match opts.get_mut(pane) {
            Some(v) => {
                *v = value.to_string();
                Ok(())
            }
            None => Err(MuxError::NoTarget { message: format!("no such pane: {pane}") }),
        }
    }
    fn display(&self, _: &str, _: &str) -> Result<String, MuxError> { self.other("display") }
    fn kill_session(&self, _: &str) -> Result<(), MuxError> { self.other("kill_session") }
    fn attach(&self, _: &str) -> Result<i32, MuxError> { self.other("attach") }
    fn switch_client(&self, _: &str) -> Result<(), MuxError> { self.other("switch_client") }
}

// Live harness (D19): a private tmux server per test, serialized by `LIVE`.

#[cfg(unix)]
static LIVE: Mutex<()> = Mutex::new(());
#[cfg(unix)]
fn live_lock() -> std::sync::MutexGuard<'static, ()> { LIVE.lock().unwrap_or_else(|e| e.into_inner()) }

#[cfg(unix)]
fn os(v: &[&str]) -> Vec<OsString> { v.iter().map(OsString::from).collect() }

#[cfg(unix)]
struct Live {
    _dir: TempDir,
    srv: PrivateServer,
    tmux: Tmux,
    paths: Paths,
    store: Store,
    panes: Vec<String>,
}

/// The session `zygo-app` with `@zygo-ws` set, `1 + extra` panes in window 0, and a state in phase `up` with the
/// server's socket and no pane.
#[cfg(unix)]
fn live(extra: usize, start: Option<&Path>) -> Live {
    let dir = TempDir::new().unwrap();
    let srv = PrivateServer::start();
    let tmux = srv.tmux();
    let p = paths(dir.path());
    let store = Store::open(&p, ID);
    let env: Vec<(OsString, OsString)> =
        ["PATH", "HOME"].iter().filter_map(|k| std::env::var_os(k).map(|v| (OsString::from(k), v))).collect();
    let argv = os(&["sleep", "600"]);
    let start_dir = start.unwrap_or(dir.path()).to_path_buf();
    tmux.new_session(&NewSession { name: "zygo-app", start_dir: &start_dir, width: 200, height: 50, env: &env, argv: &argv })
        .unwrap();
    tmux.set_session_option("zygo-app", "@zygo-ws", ID).unwrap();
    let mut panes = vec![srv.run(&["display-message", "-p", "-t", "=zygo-app:", "#{pane_id}"]).unwrap()];
    for _ in 0..extra {
        let id = tmux.split_window(&Split { target: &panes[0], start_dir: None, argv: &argv }).unwrap();
        panes.push(id);
    }
    state(&store, Phase::Up, Some(srv.socket()), &[]);
    Live { _dir: dir, srv, tmux, paths: p, store, panes }
}

#[test] // W§7.2 snapshot_seq: absent at 0, written when not 0
fn snapshot_seq_is_absent_at_zero_and_round_trips() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let store = Store::open(&p, ID);
    state(&store, Phase::Up, None, &[]);
    assert!(!String::from_utf8(raw(&store)).unwrap().contains("snapshot_seq"));
    assert_eq!(store.read().unwrap().unwrap().snapshot_seq, 0);
    assert_eq!(Workspace::new(ID).snapshot_seq, 0);
    store.update(S5, &mut |ws| { ws.snapshot_seq = 7; Ok(()) }).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw(&store)).unwrap();
    assert_eq!(v["snapshot_seq"], 7);
}

#[test] // AU38, D2, D16: phase gate with and without --force; no tmux_socket
fn phase_gate() {
    assert_eq!((HOOK_WAIT, OP_WAIT), (Duration::from_secs(1), Duration::from_secs(5)));
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let store = Store::open(&p, ID);
    let fake = Fake::new(Some(ID), vec![Ok(vec![line(0, "main", 0, "%0", 0, 1, "kaaaaaaaa", "/srv/app")])]);
    for (ph, force) in [(Phase::Building, false), (Phase::Stopping, false), (Phase::Down, false), (Phase::Down, true)] {
        state(&store, ph, Some(Path::new("/tmp/s")), &[]);
        let before = raw(&store);
        assert_eq!(
            run(&p, &fake, ID, Some("after-split-window"), force, HOOK_WAIT).unwrap(),
            SnapshotOutcome::Skipped(SkipReason::Phase),
            "{ph:?} {force}"
        );
        assert_eq!(fake.calls(""), 0);
        assert_eq!(raw(&store), before);
    }
    for (ph, force) in [(Phase::Up, false), (Phase::Draining, false), (Phase::Building, true), (Phase::Draining, true), (Phase::Stopping, true)] {
        state(&store, ph, Some(Path::new("/tmp/s")), &[]);
        assert_eq!(run(&p, &fake, ID, None, force, HOOK_WAIT).unwrap(), SnapshotOutcome::Written, "{ph:?} {force}");
        assert_eq!(keys(&store.read().unwrap().unwrap()), ["kaaaaaaaa"]);
    }
    state(&store, Phase::Up, None, &[]); // no tmux_socket
    assert_eq!(run(&p, &fake, ID, None, true, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::NoSocket));
}

/// A new state in phase `up` with the socket `/tmp/s` and the stored panes `stored` (window 0, index in order).
fn up(stored: &[&str]) -> (TempDir, Paths, Store) {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let store = Store::open(&p, ID);
    let panes: Vec<(&str, u32, u32)> = stored.iter().enumerate().map(|(i, k)| (*k, 0, i as u32)).collect();
    state(&store, Phase::Up, Some(Path::new("/tmp/s")), &panes);
    (d, p, store)
}

/// One read with `%0` holding `key`.
fn one(key: &str) -> Vec<String> { vec![line(0, "main", 0, "%0", 0, 1, key, "/srv/app")] }

/// `%0` with `k0` and `%1` with no key.
fn two(k0: &str) -> Vec<String> {
    vec![line(0, "main", 0, "%0", 0, 1, k0, "/srv/app"), line(0, "main", 0, "%1", 1, 0, "", "/srv/app")]
}

/// Runs a hook snapshot on a state with stored pane `kbbbbbbbb`: `Skipped(Logged)`, no write, one log line that
/// holds `needle`.
fn refused(fake: &Fake, needle: Option<&str>) {
    let (_d, p, store) = up(&["kbbbbbbbb"]);
    let before = raw(&store);
    assert_eq!(
        run(&p, fake, ID, Some("after-split-window"), false, HOOK_WAIT).unwrap(),
        SnapshotOutcome::Skipped(SkipReason::Logged)
    );
    assert_eq!(raw(&store), before);
    let lines = log_lines(&store);
    assert_eq!(lines.len(), 1, "{lines:?}");
    if let Some(n) = needle {
        assert!(lines[0].contains(n), "{lines:?}");
    }
}

#[test] // W§7.4 step 2, AU63, AU84, AU87, AU99, W§18 #12: one log line, no write
fn refusals_log_one_line_and_change_nothing() {
    let a = Fake::new(Some("abc"), vec![Ok(one("kbbbbbbbb"))]);
    refused(&a, Some("@zygo-ws"));
    assert_eq!(a.calls("list_panes"), 0);
    let mut b = Fake::new(None, vec![Ok(one("kbbbbbbbb"))]);
    b.exists = SessionAnswer::Exists;
    refused(&b, Some("@zygo-ws"));
    refused(&Fake::new(Some(ID), vec![Ok(vec!["0|main|0|lay|200".into()])]), Some("fields"));
    refused(&Fake::new(Some(ID), vec![Ok(one("k$(x)"))]), Some("k$(x)"));
    refused(&Fake::new(Some(ID), vec![Ok(one("#{pane_id}"))]), None);
    let f = Fake::new(
        Some(ID),
        vec![Ok(vec![line(0, "main", 0, "%0", 0, 1, "kbbbbbbbb", "/srv/app"), line(0, "main", 0, "%1", 1, 0, "kbbbbbbbb", "/srv/app")])],
    );
    refused(&f, Some("two panes"));
    refused(&Fake::new(Some(ID), vec![Err(MuxError::Failed { message: "boom".into() })]), Some("boom"));
    refused(&Fake::new(Some(ID), vec![Err(MuxError::TimedOut)]), None);
    refused(&Fake::new(Some(ID), vec![Ok(vec![])]), None);
    let j = Fake::new(Some(ID), vec![Ok(two("kbbbbbbbb"))]);
    j.fail("show_pane_option", MuxError::Failed { message: "perm".into() });
    refused(&j, Some("perm"));
    let k = Fake::new(Some(ID), vec![Ok(two("kbbbbbbbb"))]);
    k.opt("%1", "");
    k.fail("set_pane_option", MuxError::TimedOut);
    refused(&k, None);
}

#[test] // D9, review round 2 M1: an update that fails is never reported as written
fn failed_update_is_never_written() {
    // (l) write_atomic fails: workspace.json became a folder (finding 6)
    let (_d, p, store) = up(&["kbbbbbbbb"]);
    let mut fake = Fake::new(Some(ID), vec![Ok(two("kbbbbbbbb"))]);
    fake.opt("%1", "");
    let s2 = store.clone();
    fake.on_set = Some(Box::new(move |_, _| to_dir(&s2)));
    assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::Logged));
    assert_eq!(log_lines(&store).len(), 1);
    assert!(store.dir().join("workspace.json").is_dir());
    // (m) the read in update fails its checks
    let (_d, p, store) = up(&["kbbbbbbbb"]);
    let mut bad: serde_json::Value = serde_json::from_slice(&raw(&store)).unwrap();
    bad["name"] = "a b".into();
    let bytes = serde_json::to_vec(&bad).unwrap();
    let target = store.dir().join("workspace.json");
    let mut fake = Fake::new(Some(ID), vec![Ok(one("kbbbbbbbb"))]);
    let (t2, b2) = (target.clone(), bytes.clone());
    fake.on_list = Some(Box::new(move |_| fs::write(&t2, &b2).unwrap()));
    assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::Logged));
    assert_eq!(log_lines(&store).len(), 1);
    assert_eq!(fs::read(&target).unwrap(), bytes);
}

#[test] // W§7.4 step 1, AU84, W§18 #10 (snapshot part), D1, D10
fn states_that_are_refused_or_missing() {
    for (field, value) in [("name", "a b"), ("tmux_session", "other")] {
        let (_d, p, store) = up(&["kbbbbbbbb"]);
        let mut v: serde_json::Value = serde_json::from_slice(&raw(&store)).unwrap();
        v[field] = value.into();
        fs::write(store.dir().join("workspace.json"), serde_json::to_vec(&v).unwrap()).unwrap();
        let before = raw(&store);
        let fake = Fake::new(Some(ID), vec![Ok(one("kbbbbbbbb"))]);
        assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::Logged));
        assert_eq!(raw(&store), before);
        assert_eq!(log_lines(&store).len(), 1, "{field}");
        assert_eq!(fake.calls(""), 0);
    }
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let fake = Fake::new(Some(ID), vec![Ok(one("kbbbbbbbb"))]);
    assert_eq!(run(&p, &fake, "abc", None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::InvalidId));
    assert!(!p.workspaces_dir().exists());
    assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::NoState));
    assert!(!Store::open(&p, ID).dir().exists());
    assert_eq!(fake.calls(""), 0);
}

fn gone() -> MuxError { MuxError::Gone { message: "no server running on /tmp/s".into() } }

/// Runs a snapshot on a state with the stored panes `stored`: `Skipped(Gone)`, no write, no log line.
fn quiet(stored: &[&str], fake: &Fake) {
    let (_d, p, store) = up(stored);
    let before = raw(&store);
    assert_eq!(run(&p, fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::Gone));
    assert_eq!(raw(&store), before);
    assert!(log_lines(&store).is_empty());
}

#[test] // D4, AU87, review B2: Gone from each tmux call is a quiet exit
fn gone_from_each_call_is_quiet() {
    let f = Fake::new(Some(ID), vec![Ok(two("kaaaaaaaa"))]);
    f.fail("show_session_option", gone());
    quiet(&["kaaaaaaaa"], &f);
    let mut f = Fake::new(None, vec![Ok(two("kaaaaaaaa"))]);
    f.exists = SessionAnswer::Gone;
    quiet(&["kaaaaaaaa"], &f);
    quiet(&["kaaaaaaaa"], &Fake::new(Some(ID), vec![Err(gone())]));
    let f = Fake::new(Some(ID), vec![Ok(two("kaaaaaaaa")), Err(gone())]);
    quiet(&["kaaaaaaaa", "kbbbbbbbb"], &f); // shrink trigger, then the second read
    assert_eq!(f.calls("list_panes"), 2);
    let f = Fake::new(Some(ID), vec![Ok(two("kaaaaaaaa"))]);
    f.fail("show_pane_option", gone());
    quiet(&["kaaaaaaaa"], &f);
    let f = Fake::new(Some(ID), vec![Ok(two("kaaaaaaaa"))]);
    f.opt("%1", "");
    f.fail("set_pane_option", gone());
    quiet(&["kaaaaaaaa"], &f);
}

#[test] // D4: a pane that closed under the lock is checked with has-session after the lock
fn no_target_checks_the_session() {
    for (answer, out, lines) in [
        (SessionAnswer::Gone, SnapshotOutcome::Skipped(SkipReason::Gone), 0),
        (SessionAnswer::Exists, SnapshotOutcome::Skipped(SkipReason::Logged), 1),
    ] {
        let (_d, p, store) = up(&["kaaaaaaaa"]);
        let mut fake = Fake::new(Some(ID), vec![Ok(two("kaaaaaaaa"))]); // %1 is not in pane_opts
        fake.exists = answer;
        let before = raw(&store);
        assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), out);
        assert_eq!(raw(&store), before);
        assert_eq!(log_lines(&store).len(), lines);
        assert_eq!(fake.calls("session_exists"), 1);
        let calls = fake.calls.lock().unwrap().clone();
        let set = calls.iter().position(|c| c.starts_with("set_pane_option")).unwrap();
        let exists = calls.iter().position(|c| c.starts_with("session_exists")).unwrap();
        assert!(set < exists, "{calls:?}");
    }
}

fn member(agent: Agent, st: MemberState) -> MemberRecord {
    let mut m = MemberRecord::new(agent);
    m.state = st;
    m
}

#[test] // W§7.4 step 5, D7, AU53, AU99: the written shape
fn write_shape() {
    let (_d, p, store) = up(&["kaaaaaaaa", "kbbbbbbbb"]);
    store
        .update(S5, &mut |ws| {
            ws.members.insert("kaaaaaaaa".into(), member(Agent::ClaudeCode, MemberState::Running));
            ws.members.insert("kbbbbbbbb".into(), member(Agent::OpenCode, MemberState::Resumable));
            ws.members.insert("kcccccccc".into(), member(Agent::ClaudeCode, MemberState::Ended));
            ws.members.insert("kdddddddd".into(), member(Agent::ClaudeCode, MemberState::Resumable));
            Ok(())
        })
        .unwrap();
    let fake = Fake::new(
        Some(ID),
        vec![Ok(vec![
            row(1, "logs", 0, 0, 1, "%3", 0, 1, "kcccccccc", "/b"),
            row(0, "zsh", 1, 1, 0, "%1", 1, 0, "kbbbbbbbb", "/a|b:c"),
            row(0, "zsh", 1, 1, 0, "%0", 0, 1, "kaaaaaaaa", "/srv/app"),
            row(1, "logs", 0, 0, 1, "%4", 1, 0, "", "/c"),
            row(5, "zsh", 1, 0, 0, "%0", 0, 1, "kaaaaaaaa", "/srv/app"),
            row(5, "zsh", 1, 0, 0, "%1", 1, 0, "kbbbbbbbb", "/a|b:c"),
        ])],
    );
    fake.opt("%4", "");
    let t0 = Instant::now();
    assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
    assert!(t0.elapsed() < Duration::from_secs(2)); // no shrink wait
    let ws = store.read().unwrap().unwrap();
    assert_eq!(
        ws.windows.iter().map(|w| (w.index, w.name.clone(), w.active, w.zoomed)).collect::<Vec<_>>(),
        [(0, None, true, false), (1, Some("logs".into()), false, true)]
    );
    assert_eq!(
        ws.panes.iter().map(|p| (p.window, p.index, p.active)).collect::<Vec<_>>(),
        [(0, 0, true), (0, 1, false), (1, 0, true), (1, 1, false)]
    );
    assert_eq!(
        ws.panes.iter().map(|p| p.kind).collect::<Vec<_>>(),
        [PaneKind::ClaudeCode, PaneKind::OpenCode, PaneKind::Shell, PaneKind::Shell]
    );
    assert_eq!(ws.panes[1].cwd, PathBuf::from("/a|b:c"));
    let k4 = ws.panes[3].key.clone();
    assert!(valid_pane_key(&k4));
    assert_eq!(fake.pane_opts.lock().unwrap()["%4"], k4);
    assert!(!["kaaaaaaaa", "kbbbbbbbb", "kcccccccc", "kdddddddd"].contains(&k4.as_str()));
    assert_eq!(ws.members.len(), 4);
    assert!(ws.snapshot_at.is_some());
    assert_eq!(ws.snapshot_seq, 1);
}

/// A shrink case: stored `kaaaaaaaa` and `kbbbbbbbb` (member `Resumable`), phase `phase`, the scripted reads.
fn shrink_run(reads: Vec<Vec<String>>, phase: Phase, force: bool) -> (SnapshotOutcome, Duration, Fake, Store, Vec<u8>, TempDir) {
    let (d, p, store) = up(&["kaaaaaaaa", "kbbbbbbbb"]);
    store
        .update(S5, &mut |ws| {
            ws.phase = phase;
            ws.members.insert("kbbbbbbbb".into(), member(Agent::ClaudeCode, MemberState::Resumable));
            Ok(())
        })
        .unwrap();
    let fake = Fake::new(Some(ID), reads.into_iter().map(Ok).collect());
    fake.opt("%5", "");
    let before = raw(&store);
    let t0 = Instant::now();
    let out = run(&p, &fake, ID, None, force, HOOK_WAIT).unwrap();
    (out, t0.elapsed(), fake, store, before, d)
}

#[test] // AU89, D5, W§7.5 rule 1
fn shrink() {
    let a = line(0, "main", 0, "%0", 0, 1, "kaaaaaaaa", "/srv/app");
    let b = line(0, "main", 0, "%5", 1, 0, "", "/srv/app");
    // stable after one wait: the second read is used
    let (out, e, fake, store, _, _d) = shrink_run(vec![vec![a.clone()], vec![a.replace("|200|50|", "|180|50|")]], Phase::Up, false);
    assert_eq!(out, SnapshotOutcome::Written);
    assert!(e >= Duration::from_secs(2), "{e:?}");
    let ws = store.read().unwrap().unwrap();
    assert_eq!(keys(&ws), ["kaaaaaaaa"]);
    assert_eq!(ws.windows[0].width, 180);
    assert!(ws.members.contains_key("kbbbbbbbb"));
    assert_eq!(fake.calls("list_panes"), 2);
    // never stable: queued, no write
    let reads = vec![vec![a.clone()], vec![a.clone(), b.clone()], vec![a.clone()], vec![a.clone(), b.clone()]];
    let (out, e, fake, store, before, _d) = shrink_run(reads.clone(), Phase::Up, false);
    assert_eq!(out, SnapshotOutcome::Pending);
    assert_eq!(fake.calls("list_panes"), 4);
    assert_eq!(pending(&store), Some(String::new()));
    assert_eq!(raw(&store), before);
    assert!(e >= Duration::from_secs(6) && e < Duration::from_secs(12), "{e:?}");
    // the same, forced in stopping: the queued file holds force
    let (out, _, _, store, _, _d) = shrink_run(reads, Phase::Stopping, true);
    assert_eq!(out, SnapshotOutcome::Pending);
    assert_eq!(pending(&store), Some("force".into()));
    // stable at the third read
    let (out, _, fake, store, _, _d) = shrink_run(vec![vec![a.clone()], vec![a.clone(), b.clone()], vec![a, b]], Phase::Up, false);
    assert_eq!(out, SnapshotOutcome::Written);
    assert_eq!(store.read().unwrap().unwrap().panes.len(), 2);
    assert_eq!(fake.calls("list_panes"), 3);
}

fn bump(store: &Store) { store.update(S5, &mut |ws| { ws.snapshot_seq += 1; Ok(()) }).unwrap(); }

#[test] // D8, D9, AU41, review B1: the snapshot_seq rule and the checks under the lock
fn conflicts_under_the_lock() {
    // one conflict: one new whole attempt
    let (_d, p, store) = up(&["kaaaaaaaa"]);
    let mut fake = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
    let s2 = store.clone();
    fake.on_list = Some(Box::new(move |n| if n == 1 { bump(&s2) }));
    assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
    assert_eq!(fake.calls("list_panes"), 2);
    assert_eq!(store.read().unwrap().unwrap().snapshot_seq, 2);
    // two conflicts: queued
    let (_d, p, store) = up(&["kaaaaaaaa"]);
    let mut fake = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
    let s2 = store.clone();
    fake.on_list = Some(Box::new(move |_| bump(&s2)));
    assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Pending);
    assert_eq!(fake.calls("list_panes"), 2);
    assert_eq!(pending(&store), Some(String::new()));
    assert_eq!(store.read().unwrap().unwrap().snapshot_seq, 2);
    // down began: stopping with a newer layout; the hook snapshot does not overwrite it
    let (_d, p, store) = up(&["kaaaaaaaa"]);
    let mut fake = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
    let s2 = store.clone();
    fake.on_list = Some(Box::new(move |n| {
        if n == 1 {
            s2.update(S5, &mut |ws| {
                ws.phase = Phase::Stopping;
                ws.snapshot_seq += 1;
                ws.panes = vec![Pane { key: "kffffffff".into(), window: 0, index: 0, cwd: "/f".into(), ..Default::default() }];
                Ok(())
            })
            .unwrap();
        }
    }));
    assert_eq!(run(&p, &fake, ID, Some("pane-exited"), false, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::Phase));
    assert_eq!(keys(&store.read().unwrap().unwrap()), ["kffffffff"]);
    assert!(log_lines(&store).is_empty());
    // phase down only
    let (_d, p, store) = up(&["kaaaaaaaa"]);
    let mut fake = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
    let s2 = store.clone();
    fake.on_list = Some(Box::new(move |_| {
        s2.update(S5, &mut |ws| { ws.phase = Phase::Down; Ok(()) }).unwrap();
    }));
    assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::Phase));
    // tmux_session changed
    let (_d, p, store) = up(&["kaaaaaaaa"]);
    let mut fake = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
    let s2 = store.clone();
    fake.on_list = Some(Box::new(move |_| {
        s2.update(S5, &mut |ws| { ws.name = "other".into(); ws.tmux_session = "zygo-other".into(); Ok(()) }).unwrap();
    }));
    assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::Changed));
    // AU41 key race: another writer set the key after the read; it is used, not replaced
    let (_d, p, store) = up(&["kaaaaaaaa"]);
    let mut fake = Fake::new(
        Some(ID),
        vec![Ok(vec![line(0, "main", 0, "%0", 0, 1, "kaaaaaaaa", "/srv/app"), line(0, "main", 0, "%4", 1, 0, "", "/srv/app")])],
    );
    fake.opt("%4", "");
    let opts = fake.pane_opts.clone();
    fake.on_list = Some(Box::new(move |_| {
        opts.lock().unwrap().insert("%4".into(), "kzzzzzzzz".into());
    }));
    assert_eq!(run(&p, &fake, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
    assert_eq!(store.read().unwrap().unwrap().panes.iter().filter(|p| p.key == "kzzzzzzzz").count(), 1);
    assert_eq!(fake.calls("set_pane_option"), 0);
}

/// Waits at most 10 s until `path` exists.
fn wait_for(path: &Path) {
    let t0 = Instant::now();
    while !path.exists() && t0.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Sets the file time of `path` 120 s back: older than the 60 s of the `.tmp*` sweep.
fn age(path: &Path) {
    let f = fs::File::options().write(true).open(path).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(120)).unwrap();
}

#[test] // W§7.3, D14, D15, review round 2 B1, B2, M2, M4, W§18 #8
fn pending_file() {
    let snap = |p: &Paths, fake: &Fake, force: bool| run(p, fake, ID, None, force, HOOK_WAIT).unwrap();
    // (a), (b), (f): the lock is held, so each run queues
    {
        let (_d, p, store) = up(&["kaaaaaaaa"]);
        let fake = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
        let before = raw(&store);
        let held = hold(store.dir().join("lock"));
        let t0 = Instant::now();
        assert_eq!(snap(&p, &fake, false), SnapshotOutcome::Pending);
        let e = t0.elapsed();
        assert!(e >= Duration::from_secs(1) && e < Duration::from_secs(4), "{e:?}");
        assert_eq!(pending(&store), Some(String::new()));
        assert_eq!(raw(&store), before);
        // (b)
        assert_eq!(snap(&p, &fake, true), SnapshotOutcome::Pending);
        assert_eq!(pending(&store), Some("force".into()));
        assert_eq!(snap(&p, &fake, false), SnapshotOutcome::Pending);
        assert_eq!(pending(&store), Some("force".into()));
        assert_eq!(take_pending(&p, ID), Some(true));
        // (f) order A
        assert_eq!(snap(&p, &fake, false), SnapshotOutcome::Pending);
        assert_eq!(take_pending(&p, ID), Some(false));
        assert_eq!(snap(&p, &fake, true), SnapshotOutcome::Pending);
        assert_eq!(take_pending(&p, ID), Some(true));
        // order B
        assert_eq!(snap(&p, &fake, false), SnapshotOutcome::Pending);
        assert_eq!(snap(&p, &fake, true), SnapshotOutcome::Pending);
        assert_eq!(take_pending(&p, ID), Some(true));
        // order C
        assert_eq!(snap(&p, &fake, true), SnapshotOutcome::Pending);
        assert_eq!(snap(&p, &fake, false), SnapshotOutcome::Pending);
        assert_eq!(take_pending(&p, ID), Some(true));
        // order D: a take during the forced write, before its rename
        fs::write(store.dir().join(".tmpabcdef"), "force").unwrap();
        assert_eq!(pending(&store), None);
        assert_eq!(take_pending(&p, ID), None);
        assert_eq!(raw(&store), before);
        drop(held);
    }
    // (c) review M4: the lock is free right after the queue; the probe runs the snapshot
    {
        let (_d, p, store) = up(&["kaaaaaaaa"]);
        let fake = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
        let held = hold(store.dir().join("lock"));
        let pf = store.dir().join("snapshot.pending");
        let out = std::thread::scope(|s| {
            s.spawn(|| {
                wait_for(&pf);
                drop(held);
            });
            snap(&p, &fake, false)
        });
        assert_eq!(out, SnapshotOutcome::Written);
        assert_eq!(pending(&store), None);
        assert_eq!(fake.calls("list_panes"), 2);
    }
    // (d) round 2 M2: another writer took the file
    {
        let (_d, p, store) = up(&["kaaaaaaaa"]);
        let fake = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
        let held = hold(store.dir().join("lock"));
        let pf = store.dir().join("snapshot.pending");
        let out = std::thread::scope(|s| {
            s.spawn(|| {
                wait_for(&pf);
                assert_eq!(take_pending(&p, ID), Some(false));
                drop(held);
            });
            snap(&p, &fake, false)
        });
        assert_eq!(out, SnapshotOutcome::Pending);
        assert_eq!(fake.calls("list_panes"), 1);
        assert_eq!(pending(&store), None);
    }
    // (e) round 2 M2: force merge; the attempt after the probe has a conflict
    {
        let (_d, p, store) = up(&["kaaaaaaaa"]);
        let mut fake = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
        let s2 = store.clone();
        fake.on_list = Some(Box::new(move |n| if n >= 2 { bump(&s2) }));
        let held = hold(store.dir().join("lock"));
        let pf = store.dir().join("snapshot.pending");
        let out = std::thread::scope(|s| {
            s.spawn(|| {
                wait_for(&pf);
                write_atomic(&pf, b"force").unwrap();
                drop(held);
            });
            snap(&p, &fake, false)
        });
        assert_eq!(out, SnapshotOutcome::Pending);
        assert_eq!(pending(&store), Some("force".into()));
    }
    // (g) round 2 B2: the sweep never deletes a pending or taken file
    {
        let (_d, p, store) = up(&["kaaaaaaaa"]);
        let pf = store.dir().join("snapshot.pending");
        fs::write(&pf, "force").unwrap();
        age(&pf);
        store.update(S5, &mut |_| Ok(())).unwrap();
        assert!(pf.exists());
        assert_eq!(take_pending(&p, ID), Some(true));
        std::thread::scope(|s| {
            s.spawn(|| {
                for _ in 0..200 {
                    store.update(S5, &mut |_| Ok(())).unwrap();
                }
            });
            for _ in 0..50 {
                fs::write(&pf, "force").unwrap();
                age(&pf);
                assert_eq!(take_pending(&p, ID), Some(true));
            }
        });
        assert_eq!(taken_files(&store), 0);
    }
    // (h) take_pending
    {
        let (_d, p, store) = up(&["kaaaaaaaa"]);
        let pf = store.dir().join("snapshot.pending");
        fs::write(&pf, "force").unwrap();
        assert_eq!(take_pending(&p, ID), Some(true));
        fs::write(&pf, "").unwrap();
        assert_eq!(take_pending(&p, ID), Some(false));
        assert_eq!(take_pending(&p, ID), None);
        fs::create_dir(&pf).unwrap();
        assert_eq!(take_pending(&p, ID), Some(false));
        assert_eq!(log_lines(&store).len(), 1);
        let left = fs::read_dir(store.dir()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("snapshot.pending")).count();
        assert_eq!(left, 0);
    }
    // (i) after_write
    {
        let (_d, p, store) = up(&["kaaaaaaaa"]);
        let pf = store.dir().join("snapshot.pending");
        let fake = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
        assert_eq!(after_write(&p, &fake, ID, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::NoPending));
        assert_eq!(fake.calls(""), 0);
        store.update(S5, &mut |ws| { ws.phase = Phase::Stopping; Ok(()) }).unwrap();
        fs::write(&pf, "force").unwrap();
        assert_eq!(after_write(&p, &fake, ID, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
        assert!(!pf.exists());
        fs::write(&pf, "").unwrap();
        assert_eq!(after_write(&p, &fake, ID, HOOK_WAIT).unwrap(), SnapshotOutcome::Skipped(SkipReason::Phase));
        assert!(!pf.exists());
        // W§18 #8: a member write, then the pending snapshot
        let fake = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
        store
            .update(S5, &mut |ws| {
                ws.phase = Phase::Up;
                ws.members.insert("kmmmmmmmm".into(), member(Agent::ClaudeCode, MemberState::Running));
                Ok(())
            })
            .unwrap();
        fs::write(&pf, "").unwrap();
        assert_eq!(after_write(&p, &fake, ID, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
        assert!(store.read().unwrap().unwrap().members.contains_key("kmmmmmmmm"));
        assert_eq!(fake.calls("list_panes"), 1);
    }
    // (j) queue_pending
    {
        let (_d, p, store) = up(&["kaaaaaaaa"]);
        let pf = store.dir().join("snapshot.pending");
        fs::write(&pf, "force").unwrap();
        queue_pending(&p, ID, false).unwrap();
        assert_eq!(pending(&store), Some("force".into()));
        fs::write(&pf, "").unwrap();
        queue_pending(&p, ID, true).unwrap();
        assert_eq!(pending(&store), Some("force".into()));
    }
}

#[test] // D12, AU41, AU79, review M7, round 2 B3, M5
fn assign_key_with_fake() {
    let (_d, _p, store) = up(&["kaaaaaaaa"]);
    let fake = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
    for pane in ["-t", "%", "%1x", "zygo-app:0", ""] {
        assert!(assign_key(&store, &fake, pane).is_err(), "{pane:?}");
    }
    let empty = TempDir::new().unwrap();
    assert!(assign_key(&Store::open(&paths(empty.path()), ID), &fake, "%2").is_err());
    for ph in [Phase::Building, Phase::Stopping, Phase::Down] {
        store.update(S5, &mut |ws| { ws.phase = ph; Ok(()) }).unwrap();
        assert!(assign_key(&store, &fake, "%2").is_err(), "{ph:?}");
    }
    assert_eq!(fake.calls(""), 0);
    store.update(S5, &mut |ws| { ws.phase = Phase::Up; Ok(()) }).unwrap();
    fake.opt("%2", "");
    let k = assign_key(&store, &fake, "%2").unwrap();
    assert!(valid_pane_key(&k));
    assert_eq!(fake.pane_opts.lock().unwrap()["%2"], k);
    assert_eq!(assign_key(&store, &fake, "%2").unwrap(), k);
    fake.opt("%3", "k$(x)");
    assert!(assign_key(&store, &fake, "%3").is_err());
    assert_eq!(fake.pane_opts.lock().unwrap()["%3"], "k$(x)");
    assert!(assign_key(&store, &fake, "%9").is_err());
    store.update(S5, &mut |ws| { ws.phase = Phase::Draining; Ok(()) }).unwrap();
    assert_eq!(assign_key(&store, &fake, "%2").unwrap(), k);
    // rollback: the write after the key fails (finding 6)
    let mut f2 = Fake::new(Some(ID), vec![Ok(one("kaaaaaaaa"))]);
    f2.opt("%6", "");
    let s2 = store.clone();
    f2.on_set = Some(Box::new(move |pane, value| {
        if pane == "%6" && !value.is_empty() {
            to_dir(&s2);
        }
    }));
    assert!(assign_key(&store, &f2, "%6").is_err());
    assert_eq!(f2.show_pane_option("%6", "@zygo-pane").unwrap(), None);
    assert_eq!(f2.calls("set_pane_option %6"), 2);
    fs::remove_dir(store.dir().join("workspace.json")).unwrap();
    // the pending check after the key
    state(&store, Phase::Up, Some(Path::new("/tmp/s")), &[("kaaaaaaaa", 0, 0)]);
    let f3 = Fake::new(Some(ID), vec![Ok(vec![line(0, "main", 0, "%0", 0, 1, "kaaaaaaaa", "/srv/app"), line(0, "main", 0, "%2", 1, 0, "", "/srv/app")])]);
    f3.opt("%2", "");
    fs::write(store.dir().join("snapshot.pending"), "").unwrap();
    let k = assign_key(&store, &f3, "%2").unwrap();
    assert_eq!(pending(&store), None);
    assert_eq!(f3.calls("list_panes"), 1);
    assert!(keys(&store.read().unwrap().unwrap()).contains(&k));
}

#[cfg(unix)]
#[test] // W§7.4 steps 2-5 on a real server: keys, layout, names, flags
fn live_keys_layout_and_flags() {
    let _g = live_lock();
    let Some(_) = require_tmux() else { return };
    let l = live(2, None);
    let argv = os(&["sleep", "600"]);
    l.tmux.new_window(&NewWindow { session: "zygo-app", name: Some("logs"), start_dir: l._dir.path(), argv: &argv }).unwrap();
    l.srv.run(&["resize-pane", "-Z", "-t", &l.panes[1]]).unwrap();
    assert_eq!(run(&l.paths, &l.tmux, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
    let ws = l.store.read().unwrap().unwrap();
    assert_eq!(ws.panes.len(), 4);
    for p in &ws.panes {
        let target = format!("=zygo-app:{}.{}", p.window, p.index);
        let id = l.srv.run(&["display-message", "-p", "-t", &target, "#{pane_id}"]).unwrap();
        assert_eq!(l.tmux.show_pane_option(&id, "@zygo-pane").unwrap().as_deref(), Some(p.key.as_str()));
    }
    assert_eq!(ws.windows[0].name, None);
    assert_eq!(ws.windows[1].name.as_deref(), Some("logs"));
    assert!(ws.windows[0].zoomed && !ws.windows[1].zoomed);
    assert!(ws.windows[0].active && !ws.windows[1].active);
    assert_eq!(ws.windows[0].layout, l.srv.run(&["display-message", "-p", "-t", "=zygo-app:0", "#{window_layout}"]).unwrap());
    let before = keys(&ws);
    run(&l.paths, &l.tmux, ID, None, false, HOOK_WAIT).unwrap();
    assert_eq!(keys(&l.store.read().unwrap().unwrap()), before);
}

#[cfg(unix)]
#[test] // W§18 #12, AU99: pane paths with the separator, a colon, LF and CR
fn live_cwd_with_separator_colon_newline_and_cr() {
    let _g = live_lock();
    let Some(_) = require_tmux() else { return };
    let td = TempDir::new().unwrap();
    let mk = |n: &str| {
        let d = td.path().join(n);
        fs::create_dir(&d).unwrap();
        fs::canonicalize(&d).unwrap()
    };
    let (d1, d2, d3) = (mk("a|b:c"), mk("nl\nx"), mk("cr\r"));
    let l = live(0, Some(&d1));
    let argv = os(&["sleep", "600"]);
    l.tmux.split_window(&Split { target: &l.panes[0], start_dir: Some(&d2), argv: &argv }).unwrap();
    l.tmux.split_window(&Split { target: &l.panes[0], start_dir: Some(&d3), argv: &argv }).unwrap();
    assert_eq!(run(&l.paths, &l.tmux, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
    let mut got: Vec<PathBuf> = l.store.read().unwrap().unwrap().panes.iter().map(|p| p.cwd.clone()).collect();
    got.sort();
    let mut want = vec![d1, d2, d3];
    want.sort();
    assert_eq!(got, want);
    assert!(log_lines(&l.store).is_empty());
}

#[cfg(unix)]
#[test] // W§18 #9, AU41, AU79: two snapshots and a hand start give one key
fn live_two_snapshots_and_a_hand_start_give_one_key() {
    let _g = live_lock();
    let Some(_) = require_tmux() else { return };
    let l = live(0, None);
    let argv = os(&["sleep", "600"]);
    for round in 0..5 {
        let new_id = l.tmux.split_window(&Split { target: &l.panes[0], start_dir: None, argv: &argv }).unwrap();
        l.srv.run(&["select-layout", "-t", "=zygo-app:0", "tiled"]).unwrap(); // room for the next split
        let barrier = Barrier::new(3);
        let (r1, r2, hand) = std::thread::scope(|s| {
            let snap = || {
                barrier.wait();
                run(&l.paths, &l.tmux, ID, Some("after-split-window"), false, HOOK_WAIT)
            };
            let a = s.spawn(snap);
            let b = s.spawn(snap);
            let c = s.spawn(|| {
                barrier.wait();
                assign_key(&l.store, &l.tmux, &new_id)
            });
            (a.join().unwrap(), b.join().unwrap(), c.join().unwrap())
        });
        r1.unwrap();
        r2.unwrap();
        after_write(&l.paths, &l.tmux, ID, HOOK_WAIT).unwrap();
        run(&l.paths, &l.tmux, ID, None, false, HOOK_WAIT).unwrap();
        let k = hand.unwrap();
        assert!(valid_pane_key(&k));
        assert_eq!(l.tmux.show_pane_option(&new_id, "@zygo-pane").unwrap().as_deref(), Some(k.as_str()), "round {round}");
        let ws = l.store.read().unwrap().unwrap();
        assert_eq!(ws.panes.iter().filter(|p| p.key == k).count(), 1, "round {round}");
        let mut ks = keys(&ws);
        ks.sort();
        ks.dedup();
        assert_eq!(ks.len(), ws.panes.len());
    }
}

#[cfg(unix)]
#[test] // W§18 #8, W§7.3: a queued snapshot and concurrent member writers
fn live_pending_and_concurrent_writers() {
    let _g = live_lock();
    let Some(_) = require_tmux() else { return };
    let l = live(0, None);
    // part 1
    let held = hold(l.store.dir().join("lock"));
    assert_eq!(run(&l.paths, &l.tmux, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Pending);
    assert!(l.store.read().unwrap().unwrap().panes.is_empty());
    drop(held);
    l.store
        .update(S5, &mut |ws| {
            ws.members.insert("kmmmmmmmm".into(), member(Agent::ClaudeCode, MemberState::Running));
            Ok(())
        })
        .unwrap();
    assert_eq!(after_write(&l.paths, &l.tmux, ID, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
    assert_eq!(pending(&l.store), None);
    let ws = l.store.read().unwrap().unwrap();
    assert_eq!(ws.panes.len(), 1);
    assert!(valid_pane_key(&ws.panes[0].key));
    assert!(ws.members.contains_key("kmmmmmmmm"));
    // part 2
    std::thread::scope(|s| {
        s.spawn(|| {
            for _ in 0..5 {
                run(&l.paths, &l.tmux, ID, None, false, HOOK_WAIT).unwrap();
            }
        });
        s.spawn(|| {
            for x in 'a'..='e' {
                for y in 'a'..='e' {
                    let k = format!("kaaaaaa{x}{y}");
                    l.store
                        .update(S5, &mut |ws| {
                            ws.members.insert(k.clone(), member(Agent::ClaudeCode, MemberState::Running));
                            Ok(())
                        })
                        .unwrap();
                    after_write(&l.paths, &l.tmux, ID, HOOK_WAIT).unwrap();
                }
            }
        });
    });
    after_write(&l.paths, &l.tmux, ID, HOOK_WAIT).unwrap();
    let ws = l.store.read().unwrap().unwrap();
    assert_eq!(ws.members.len(), 26);
    assert_eq!(ws.panes.len(), 1);
    assert_eq!(pending(&l.store), None);
    assert!(ws.snapshot_seq >= 1);
}

#[cfg(unix)]
#[test] // W§7.5 rule 1, AU89: kill-pane drops the pane entry, the member record stays
fn live_kill_pane_drops_the_key() {
    let _g = live_lock();
    let Some(_) = require_tmux() else { return };
    let l = live(1, None);
    assert_eq!(run(&l.paths, &l.tmux, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
    let k2 = l.tmux.show_pane_option(&l.panes[1], "@zygo-pane").unwrap().unwrap();
    l.store
        .update(S5, &mut |ws| {
            let mut m = member(Agent::ClaudeCode, MemberState::Resumable);
            m.agent_session = Some("a-1".into());
            ws.members.insert(k2.clone(), m);
            Ok(())
        })
        .unwrap();
    l.srv.run(&["kill-pane", "-t", &l.panes[1]]).unwrap();
    let t0 = Instant::now();
    assert_eq!(run(&l.paths, &l.tmux, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
    assert!(t0.elapsed() >= Duration::from_secs(2));
    let ws = l.store.read().unwrap().unwrap();
    assert_eq!(ws.panes.len(), 1);
    assert!(!keys(&ws).contains(&k2));
    assert!(ws.members.contains_key(&k2));
}

#[cfg(unix)]
#[test] // AU89, W§7.5 rule 2, PW6: kill-session during the shrink wait writes nothing
fn live_kill_session_during_the_wait_writes_nothing() {
    let _g = live_lock();
    let Some(_) = require_tmux() else { return };
    let l = live(1, None);
    assert_eq!(run(&l.paths, &l.tmux, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
    l.srv.run(&["kill-pane", "-t", &l.panes[1]]).unwrap();
    let before = raw(&l.store);
    let out = std::thread::scope(|s| {
        let h = s.spawn(|| run(&l.paths, &l.tmux, ID, Some("pane-exited"), false, HOOK_WAIT));
        std::thread::sleep(Duration::from_millis(300));
        let _ = l.srv.run(&["kill-session", "-t", "=zygo-app:"]);
        h.join().unwrap()
    });
    assert_eq!(out.unwrap(), SnapshotOutcome::Skipped(SkipReason::Gone));
    assert_eq!(raw(&l.store), before);
    assert!(log_lines(&l.store).is_empty());
}

#[cfg(unix)]
#[test] // D7: a linked window's panes are read once, in their first window
fn live_linked_window_is_read_once() {
    let _g = live_lock();
    let Some(_) = require_tmux() else { return };
    let l = live(1, None);
    l.srv.run(&["link-window", "-s", "=zygo-app:0", "-t", "=zygo-app:5"]).unwrap();
    assert_eq!(run(&l.paths, &l.tmux, ID, None, false, HOOK_WAIT).unwrap(), SnapshotOutcome::Written);
    let ws = l.store.read().unwrap().unwrap();
    assert_eq!(ws.panes.len(), 2);
    assert_eq!(ws.windows.iter().map(|w| w.index).collect::<Vec<_>>(), [0]);
    let mut ks = keys(&ws);
    ks.sort();
    ks.dedup();
    assert_eq!(ks.len(), 2);
}

#[cfg(unix)]
#[test] // D12, AU79: a hand start on a real server
fn live_assign_key() {
    let _g = live_lock();
    let Some(_) = require_tmux() else { return };
    let l = live(1, None);
    let k = assign_key(&l.store, &l.tmux, &l.panes[1]).unwrap();
    assert_eq!(l.tmux.show_pane_option(&l.panes[1], "@zygo-pane").unwrap().as_deref(), Some(k.as_str()));
    assert_eq!(assign_key(&l.store, &l.tmux, &l.panes[1]).unwrap(), k);
    l.tmux.set_pane_option(&l.panes[0], "@zygo-pane", "k$(x)").unwrap();
    assert!(assign_key(&l.store, &l.tmux, &l.panes[0]).is_err());
    assert_eq!(l.tmux.show_pane_option(&l.panes[0], "@zygo-pane").unwrap().as_deref(), Some("k$(x)"));
    assert!(assign_key(&l.store, &l.tmux, "%9999").is_err());
}

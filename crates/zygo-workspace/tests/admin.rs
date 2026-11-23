//! Node W8: workspace admin: `ls`, `workspace`, `ws forget`, `ws forget-pane` (W§12, W§7.3).
//!
//! Every test and helper is `cfg(unix)`: the tmux stand-in is a shell script (W7 D26). Every test takes `serial()`, so
//! no test writes a script while another test forks (ETXTBSY). Most cases use the stand-in tmux `answer_tmux`; the live
//! cases use a private server (`PrivateServer`), so no call reaches the default tmux server. Every `forget` runs on the
//! state folders of a `TempDir` (`assert_temp`).
#![cfg(unix)]

use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_workspace::admin::{self, ListRow, ListState};
use zygo_workspace::identity::workspace_id;
use zygo_workspace::mux::testing::{require_tmux, PrivateServer};
use zygo_workspace::mux::{HookScope, Multiplexer, MuxError, NewSession, NewWindow, SessionAnswer, Split};
use zygo_workspace::state::{MemberRecord, MemberState, Pane, PaneKind, Phase, Store, Workspace};
use zygo_workspace::up;

const S5: Duration = Duration::from_secs(5);
const ID: &str = "0123456789abcdef";
const OTHER: &str = "fedcba9876543210";
const AU29: &str = "zygo: workspace state is from a newer zygo; run zygo update";
/// The `Display` of `MuxError::TimedOut` (`mux/mod.rs:415`).
const TIMED_OUT: &str = "tmux timed out after 2 s";

// ---- helpers (step 1) ----

fn paths(p: &Path) -> Paths {
    let paths = Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") };
    assert_temp(&paths, p);
    paths
}

/// Every path of `p` lies in the test folder `t`: no `forget` reads or deletes the operator's own state.
fn assert_temp(p: &Paths, t: &Path) {
    for q in [&p.config_file, &p.cache_dir, &p.data_dir, &p.sessions_dir, &p.workspaces_dir(), &p.workspace_cache_dir()] {
        assert!(q.starts_with(t), "{} is outside the test folder {}", q.display(), t.display());
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").current_dir(dir)
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid"]).args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
        .output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn canon(p: &Path) -> PathBuf { p.canonicalize().unwrap() }

/// `<parent>/<name>`: a repository on `main` with one commit; returns the canonical path.
fn repo(parent: &Path, name: &str) -> PathBuf {
    fs::create_dir_all(parent).unwrap();
    git(parent, &["init", "-q", "-b", "main", name]);
    git(&parent.join(name), &["commit", "-q", "--allow-empty", "-m", "a"]);
    canon(&parent.join(name))
}

/// An executable `#!/bin/sh` program `<dir>/<name>`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let f = dir.join(name);
    fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
    f
}

fn tmux_log(dir: &Path) -> Vec<String> {
    let s = fs::read_to_string(dir.join("tmux.log")).unwrap_or_default();
    s.split('\u{1e}').filter(|l| !l.is_empty()).map(str::to_string).collect()
}

const ANSWER_BODY: &str = r#"d='@D'
printf '%s\036' "$*" >> "$d/tmux.log"
sub=; tgt=
while [ $# -gt 0 ]; do
  case "$1" in
    -V) echo 'tmux 3.4'; exit 0 ;;
    -S|-t|-F|-s|-c|-x|-y|-n) [ "$1" = -t ] && tgt=$2; shift ;;
    -*) ;;
    *) [ -z "$sub" ] && sub=$1 ;;
  esac
  shift
done
s=${tgt#=}; s=${s%%:*}
case "$sub" in
  has-session) [ -f "$d/exists-$s" ] && exit 0; [ -f "$d/error-$s" ] && { echo 'permission denied' >&2; exit 1; } ;;
  show-options) [ -f "$d/ws-$s" ] && { cat "$d/ws-$s"; exit 0; }; [ -f "$d/exists-$s" ] && exit 0 ;;
  display-message) [ -f "$d/clients-$s" ] && { cat "$d/clients-$s"; exit 0; } ;;
esac
echo 'no server running on /tmp/zygo-none/s' >&2
exit 1"#;

/// The stand-in tmux of step 1: it logs each call and answers from the files `exists-<s>`, `error-<s>`, `ws-<s>` and
/// `clients-<s>` of `dir`; every other call gets "no server running".
fn answer_tmux(dir: &Path) -> PathBuf { script(dir, "atmux", &ANSWER_BODY.replace("@D", &dir.display().to_string())) }

/// The `-t` value of each logged `has-session` call, in log order.
fn has_sessions(dir: &Path) -> Vec<String> {
    tmux_log(dir)
        .iter()
        .filter(|l| l.contains("has-session"))
        .filter_map(|l| l.split(' ').skip_while(|w| *w != "-t").nth(1).map(str::to_string))
        .collect()
}

fn member(kind: Agent, state: MemberState, session: Option<&str>) -> MemberRecord {
    let mut m = MemberRecord::new(kind);
    m.state = state;
    m.agent_session = session.map(String::from);
    m
}

fn member_in(kind: Agent, state: MemberState, session: Option<&str>, zygo_session: &str) -> MemberRecord {
    let mut m = member(kind, state, session);
    m.zygo_session = Some(zygo_session.into());
    m
}

fn pane(key: &str, window: u32, index: u32, kind: PaneKind, cwd: &Path) -> Pane {
    Pane { key: key.into(), window, index, cwd: cwd.to_path_buf(), kind, ..Pane::default() }
}

/// Writes `<dir>/workspace.json` as given.
fn put_raw(dir: &Path, v: &Value) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("workspace.json"), serde_json::to_vec_pretty(v).unwrap()).unwrap();
}

static LIVE: Mutex<()> = Mutex::new(());
fn serial() -> std::sync::MutexGuard<'static, ()> { LIVE.lock().unwrap_or_else(|e| e.into_inner()) }

fn hold(path: PathBuf) -> fs::File {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let f = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).unwrap();
    fs4::FileExt::try_lock(&f).unwrap();
    f
}

/// A readable state: `root/.git` made, name, session, paths, loadout `solo`, phase `down`, then `f`.
fn ws_state(p: &Paths, id: &str, name: &str, root: &Path, f: impl Fn(&mut Workspace)) -> Store {
    fs::create_dir_all(root.join(".git")).unwrap();
    let store = Store::open(p, id);
    store
        .update(S5, &mut |ws| {
            ws.name = name.into();
            ws.tmux_session = format!("zygo-{name}");
            ws.common_dir = root.join(".git");
            ws.main_root = root.to_path_buf();
            ws.loadout = "solo".into();
            ws.phase = Phase::Down;
            f(ws);
            Ok(())
        })
        .unwrap();
    store
}

/// A folder with mode 0700.
fn private_dir(p: &Path) -> PathBuf {
    fs::create_dir_all(p).unwrap();
    fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
    p.to_path_buf()
}

/// A live session `<sessions_dir>/<id>`: `token` and a held `pid` lock, which lasts as long as the returned file.
fn live_session(p: &Paths, id: &str) -> fs::File {
    let dir = p.sessions_dir.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("token"), "t").unwrap();
    hold(dir.join("pid"))
}

/// A version 1 document that fails AU84: `invalid pane key` (or `invalid name` when `name` fails AU2).
fn invalid_doc(id: &str, name: Value, socket: Option<&Path>) -> Value {
    let session = format!("zygo-{}", name.as_str().unwrap_or("x"));
    let mut v = json!({"version": 1, "id": id, "name": name, "common_dir": "/x/.git", "main_root": "/x",
        "tmux_session": session, "loadout": "solo", "phase": "up", "phase_since": 1,
        "panes": [{"key": "bad", "window": 0, "index": 0, "cwd": "/x", "kind": "shell"}]});
    if let Some(s) = socket {
        v["tmux_socket"] = json!(s);
    }
    v
}

fn text(b: Vec<u8>) -> String { String::from_utf8(b).unwrap() }

fn forget_run(p: &Paths, tmux: &Path, env: Option<&str>, name: &str) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = admin::forget(p, tmux, env.map(OsStr::new), name, &mut out, &mut err);
    (code, text(out), text(err))
}

fn pane_run(p: &Paths, tmux: &Path, name: Option<&str>, cwd: &Path, key: &str) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = admin::forget_pane(p, tmux, name, cwd, key, &mut out, &mut err);
    (code, text(out), text(err))
}

fn cache(p: &Paths, id: &str) -> PathBuf { p.workspace_cache_dir().join(id) }

/// Makes `<cache>/results/<key>/r1`; returns `<cache>/results/<key>`.
fn result(p: &Paths, id: &str, key: &str) -> PathBuf {
    let d = cache(p, id).join("results").join(key);
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("r1"), "r").unwrap();
    d
}

fn repair_line(id: &str, reason: &str) -> String {
    format!("zygo: workspace {id} not forgotten: {reason}; its state cannot be read, so repair it with zygo up --reset in its checkout\n")
}

fn state_bytes(p: &Paths, id: &str) -> Vec<u8> { fs::read(p.workspaces_dir().join(id).join("workspace.json")).unwrap() }

fn raw_invalid_name(p: &Paths) -> PathBuf {
    let dir = p.workspaces_dir().join("e100000000000001");
    put_raw(&dir, &json!({"version": 1, "id": "e100000000000001", "name": "a b", "common_dir": "/x/.git", "main_root": "/x",
        "tmux_session": "zygo-a b", "loadout": "solo", "phase": "down", "phase_since": 1}));
    dir.join("workspace.json")
}

fn raw_newer(p: &Paths, name: &str) {
    put_raw(&p.workspaces_dir().join("e200000000000002"), &json!({"version": 2, "id": "e200000000000002", "name": name,
        "common_dir": "/x/.git", "main_root": "/x", "tmux_session": format!("zygo-{name}"), "loadout": "solo", "phase": "down",
        "phase_since": 1}));
}

// ---- the `ls` fake (D11) ----

#[derive(Clone)]
struct ListFake {
    socket: PathBuf,
    answer: SessionAnswer,
    owner: Result<Option<String>, MuxError>,
    clients: Result<String, MuxError>,
    cost: Duration,
    clock: Arc<Mutex<Duration>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl ListFake {
    fn new(socket: &Path, answer: SessionAnswer, cost: Duration) -> ListFake {
        ListFake {
            socket: socket.to_path_buf(),
            answer,
            owner: Ok(None),
            clients: Ok("0".into()),
            cost,
            clock: Arc::new(Mutex::new(Duration::ZERO)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }
    fn spend(&self, method: &str) {
        self.calls.lock().unwrap().push(format!("{method} {}", self.socket.display()));
        *self.clock.lock().unwrap() += self.cost;
    }
    fn other<T>(&self) -> Result<T, MuxError> { Err(MuxError::Failed { message: "not scripted".into() }) }
}

impl Multiplexer for ListFake {
    fn socket(&self) -> Option<&Path> { Some(&self.socket) }
    fn version(&self) -> Result<(u32, u32), MuxError> { self.other() }
    fn socket_path(&self, _: &str) -> Result<PathBuf, MuxError> { self.other() }
    fn session_exists(&self, _: &str) -> SessionAnswer {
        self.spend("session_exists");
        self.answer.clone()
    }
    fn new_session(&self, _: &NewSession<'_>) -> Result<u32, MuxError> { self.other() }
    fn resize_window(&self, _: &str, _: u32, _: u32, _: u32) -> Result<(), MuxError> { self.other() }
    fn unset_window_option(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { self.other() }
    fn set_window_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { self.other() }
    fn set_env(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { self.other() }
    fn hide_env(&self, _: &str, _: &str) -> Result<(), MuxError> { self.other() }
    fn set_session_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { self.other() }
    fn show_session_option(&self, _: &str, _: &str) -> Result<Option<String>, MuxError> {
        self.spend("show_session_option");
        self.owner.clone()
    }
    fn set_hook(&self, _: &str, _: HookScope, _: &str, _: &str) -> Result<(), MuxError> { self.other() }
    fn new_window(&self, _: &NewWindow<'_>) -> Result<u32, MuxError> { self.other() }
    fn rename_window(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { self.other() }
    fn split_window(&self, _: &Split<'_>) -> Result<String, MuxError> { self.other() }
    fn select_layout(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { self.other() }
    fn select_window(&self, _: &str, _: u32) -> Result<(), MuxError> { self.other() }
    fn select_pane(&self, _: &str) -> Result<(), MuxError> { self.other() }
    fn zoom(&self, _: &str) -> Result<(), MuxError> { self.other() }
    fn list_panes(&self, _: &str) -> Result<Vec<String>, MuxError> { self.other() }
    fn show_pane_option(&self, _: &str, _: &str) -> Result<Option<String>, MuxError> { self.other() }
    fn set_pane_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { self.other() }
    fn display(&self, _: &str, _: &str) -> Result<String, MuxError> {
        self.spend("display");
        self.clients.clone()
    }
    fn kill_session(&self, _: &str) -> Result<(), MuxError> { self.other() }
    fn attach(&self, _: &str) -> Result<i32, MuxError> { self.other() }
    fn switch_client(&self, _: &str) -> Result<(), MuxError> { self.other() }
}

/// The rows of `admin::testing::list_by` with the fakes (one per socket) on one shared virtual clock, the calls in
/// order, and the virtual time used.
fn fake_list(p: &Paths, fakes: Vec<ListFake>, no_tmux: bool) -> (Vec<ListRow>, Vec<String>, Duration) {
    let base = Instant::now();
    let clock = Arc::new(Mutex::new(Duration::ZERO));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let fakes: Vec<ListFake> =
        fakes.into_iter().map(|f| ListFake { clock: clock.clone(), calls: calls.clone(), ..f }).collect();
    let mux_of = |s: &Path| -> Box<dyn Multiplexer> {
        Box::new(fakes.iter().find(|f| f.socket == s).expect("a fake for each socket").clone())
    };
    let now = || base + *clock.lock().unwrap();
    let rows = admin::testing::list_by(p, no_tmux, &mux_of, &now);
    let used = *clock.lock().unwrap();
    let calls = calls.lock().unwrap().clone();
    (rows, calls, used)
}

fn states(rows: &[ListRow]) -> Vec<(String, ListState)> { rows.iter().map(|r| (r.name.clone(), r.state)).collect() }

// ---- step 9 ----

#[test] // D10: the `ls` line and the state words
fn list_display() {
    let _s = serial();
    let row = |name: &str, root: Option<&str>, state, agents, clients| ListRow {
        id: "0123456789abcdef".into(),
        name: name.into(),
        main_root: root.map(PathBuf::from),
        state,
        agents,
        clients,
    };
    assert_eq!(
        row("app", Some("/srv/app"), ListState::Up, Some((1, 2, 0)), Some(1)).to_string(),
        "app  /srv/app  up  agents 1/2/0  clients 1"
    );
    assert_eq!(
        row("app", Some("/srv/a\nb"), ListState::StoppedTmuxGone, Some((0, 0, 3)), None).to_string(),
        "app  /srv/a\\nb  stopped (tmux gone)  agents 0/0/3  clients -"
    );
    assert_eq!(
        row("0123456789abcdef", None, ListState::Unreadable, None, None).to_string(),
        "0123456789abcdef  -  unreadable  agents -  clients -"
    );
    assert_eq!(
        row("app", Some("/srv/app"), ListState::Unknown("tmux timed out"), Some((0, 1, 0)), None).to_string(),
        "app  /srv/app  unknown (tmux timed out)  agents 0/1/0  clients -"
    );
    for (s, w) in [
        (ListState::Down, "down"),
        (ListState::Building, "building"),
        (ListState::Draining, "draining"),
        (ListState::Stopping, "stopping"),
        (ListState::Unreachable, "unreachable"),
        (ListState::RepoMissing, "repo missing"),
        (ListState::Unknown("no tmux"), "unknown (no tmux)"),
        (ListState::Unknown("tmux error"), "unknown (tmux error)"),
    ] {
        assert_eq!(s.to_string(), w);
    }
}

// ---- step 10 ----

/// One `ls` row as step 10 checks it: name, state, agents, clients.
type Seen = (String, ListState, Option<(usize, usize, usize)>, Option<u32>);

#[test] // D8, D9, AU87, W§12 `ls`: every state, the agents and the clients
fn list_gives_each_state() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    let sock = private_dir(&t.join("sock")).join("s");
    let _live = live_session(&p, "01LIVE");
    let r = |n: &str| t.join("r").join(n);
    let id = |n: u8| format!("00000000000000{n:02x}");
    ws_state(&p, &id(1), "down", &r("down"), |ws| {
        let root = ws.main_root.clone();
        ws.panes = vec![pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, &root), pane("kcccccccc", 0, 1, PaneKind::ClaudeCode, &root)];
        ws.members = [
            ("kaaaaaaaa", member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1"))),
            ("kcccccccc", member(Agent::ClaudeCode, MemberState::Ended, Some("c-3"))),
            ("kbbbbbbbb", member(Agent::ClaudeCode, MemberState::Resumable, Some("c-2"))),
            ("kdddddddd", member(Agent::ClaudeCode, MemberState::Running, None)),
        ]
        .into_iter()
        .map(|(k, m)| (k.to_string(), m))
        .collect();
    });
    ws_state(&p, &id(2), "gone-repo", &r("gone-repo"), |ws| ws.main_root = t.join("none"));
    let up_sock = |ws: &mut Workspace| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(sock.clone());
    };
    ws_state(&p, &id(3), "up", &r("up"), |ws| {
        up_sock(ws);
        ws.members.insert("kaaaaaaaa".into(), member_in(Agent::ClaudeCode, MemberState::Running, Some("c-9"), "01LIVE"));
    });
    fs::write(t.join("exists-zygo-up"), "").unwrap();
    fs::write(t.join("ws-zygo-up"), id(3)).unwrap();
    fs::write(t.join("clients-zygo-up"), "2").unwrap();
    ws_state(&p, &id(4), "stopped", &r("stopped"), up_sock);
    ws_state(&p, &id(5), "live-gone", &r("live-gone"), |ws| {
        up_sock(ws);
        ws.members.insert("kaaaaaaaa".into(), member_in(Agent::ClaudeCode, MemberState::Running, Some("c-5"), "01LIVE"));
    });
    ws_state(&p, &id(6), "foreign", &r("foreign"), up_sock);
    fs::write(t.join("exists-zygo-foreign"), "").unwrap();
    fs::write(t.join("ws-zygo-foreign"), OTHER).unwrap();
    ws_state(&p, &id(7), "building", &r("building"), |ws| ws.phase = Phase::Building);
    ws_state(&p, &id(8), "draining", &r("draining"), |ws| ws.phase = Phase::Draining);
    ws_state(&p, &id(9), "stopping", &r("stopping"), |ws| ws.phase = Phase::Stopping);
    ws_state(&p, &id(10), "nosock", &r("nosock"), |ws| ws.phase = Phase::Up);
    ws_state(&p, &id(11), "moved", &r("moved"), |ws| {
        up_sock(ws);
        ws.main_root = t.join("gone-too");
    });
    raw_invalid_name(&p);
    raw_newer(&p, "e");

    let rows = admin::list(&p, &tmux);
    let got: Vec<Seen> = rows.iter().map(|r| (r.name.clone(), r.state, r.agents, r.clients)).collect();
    use ListState::*;
    let want: Vec<Seen> = vec![
        ("building".into(), Building, Some((0, 0, 0)), None),
        ("down".into(), Down, Some((0, 1, 1)), None),
        ("draining".into(), Draining, Some((0, 0, 0)), None),
        ("e100000000000001".into(), Unreadable, None, None),
        ("e200000000000002".into(), Unreadable, None, None),
        ("foreign".into(), Unreachable, Some((0, 0, 0)), None),
        ("gone-repo".into(), RepoMissing, Some((0, 0, 0)), None),
        ("live-gone".into(), Unreachable, Some((1, 0, 0)), None),
        ("moved".into(), RepoMissing, Some((0, 0, 0)), None),
        ("nosock".into(), Unreachable, Some((0, 0, 0)), None),
        ("stopped".into(), StoppedTmuxGone, Some((0, 0, 0)), None),
        ("stopping".into(), Stopping, Some((0, 0, 0)), None),
        ("up".into(), Up, Some((1, 0, 0)), Some(2)),
    ];
    assert_eq!(got, want);
    assert_eq!(rows[1].main_root.as_deref(), Some(r("down").as_path()));
    assert_eq!((rows[3].id.as_str(), rows[3].main_root.clone()), ("e100000000000001", None));
    assert_eq!(has_sessions(t), ["=zygo-foreign:", "=zygo-live-gone:", "=zygo-moved:", "=zygo-stopped:", "=zygo-up:"]);
}

// ---- step 11 ----

#[test] // D8, D11, AU62: a failed socket is not asked again; a socket that is not private or has no folder gets no call
fn list_failed_socket_and_private_socket() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    let s2 = private_dir(&t.join("sock2")).join("s");
    let open = t.join("open");
    fs::create_dir_all(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
    let sockets = [("x1", s2.clone()), ("x2", s2.clone()), ("y", open.join("s")), ("z", t.join("gone/s"))];
    for (i, (n, s)) in sockets.iter().enumerate() {
        ws_state(&p, &format!("a00000000000000{i}"), n, &t.join("r").join(n), |ws| {
            ws.phase = Phase::Up;
            ws.tmux_socket = Some(s.clone());
        });
    }
    fs::write(t.join("error-zygo-x1"), "").unwrap();
    let rows = admin::list(&p, &tmux);
    assert_eq!(
        states(&rows),
        [
            ("x1".to_string(), ListState::Unknown("tmux error")),
            ("x2".into(), ListState::Unknown("tmux error")),
            ("y".into(), ListState::Unreachable),
            ("z".into(), ListState::StoppedTmuxGone)
        ]
    );
    assert_eq!(has_sessions(t), ["=zygo-x1:"]);
    let (o, g) = (open.join("s").display().to_string(), t.join("gone/s").display().to_string());
    assert!(!tmux_log(t).iter().any(|l| l.contains(&o) || l.contains(&g)));
    assert!(!t.join("gone").exists());
}

// ---- step 12 ----

/// States `Up`, each on the socket `<t>/s-<socket>/s` (a 0700 folder) with an existing root; returns the sockets.
fn up_states(p: &Paths, t: &Path, rows: &[(&str, &str, &str)]) -> Vec<(String, PathBuf)> {
    rows.iter()
        .map(|(id, name, s)| {
            let sock = private_dir(&t.join(format!("s-{s}"))).join("s");
            ws_state(p, id, name, &t.join("r").join(name), |ws| {
                ws.phase = Phase::Up;
                ws.tmux_socket = Some(sock.clone());
            });
            (s.to_string(), sock)
        })
        .collect()
}

#[test] // D11: the 4 s deadline and the failed set, on a virtual clock with no real wait
fn list_deadline_and_timeouts() {
    let _s = serial();
    let hung = || SessionAnswer::Error { message: TIMED_OUT.into() };
    let s2 = Duration::from_secs(2);
    let timed_out = ListState::Unknown("tmux timed out");

    // (a) one hung socket: one call, then the failed set.
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let socks = up_states(&p, t, &[("a000000000000001", "a1", "A"), ("a000000000000002", "a2", "A")]);
    let a = &socks[0].1;
    let (rows, calls, used) = fake_list(&p, vec![ListFake::new(a, hung(), s2)], false);
    assert_eq!(states(&rows), [("a1".to_string(), timed_out), ("a2".into(), timed_out)]);
    assert_eq!(calls, [format!("session_exists {}", a.display())]);
    assert_eq!(used, s2);

    // (b) two hung sockets use the 4 s; the third socket gets no call.
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let socks = up_states(&p, t, &[("b000000000000001", "b1", "B"), ("c000000000000001", "c1", "C"), ("d000000000000001", "d1", "D")]);
    let (b, c, d) = (socks[0].1.clone(), socks[1].1.clone(), socks[2].1.clone());
    let fakes_b = vec![
        ListFake::new(&b, hung(), s2),
        ListFake::new(&c, hung(), s2),
        ListFake::new(&d, SessionAnswer::Gone, Duration::ZERO),
    ];
    let (rows, calls, used) = fake_list(&p, fakes_b.clone(), false);
    assert_eq!(states(&rows), [("b1".to_string(), timed_out), ("c1".into(), timed_out), ("d1".into(), timed_out)]);
    assert_eq!(calls, [format!("session_exists {}", b.display()), format!("session_exists {}", c.display())]);
    assert_eq!(used, Duration::from_secs(4));

    // (e) no tmux: every row `unknown (no tmux)`, no call.
    let (rows, calls, _) = fake_list(&p, fakes_b, true);
    let no = ListState::Unknown("no tmux");
    assert_eq!(states(&rows), [("b1".to_string(), no), ("c1".into(), no), ("d1".into(), no)]);
    assert!(calls.is_empty(), "{calls:?}");

    // (c) a failed option read marks the socket.
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let socks = up_states(&p, t, &[("e000000000000001", "e1", "E"), ("e000000000000002", "e2", "E")]);
    let e = socks[0].1.clone();
    let fake = ListFake { owner: Err(MuxError::TimedOut), ..ListFake::new(&e, SessionAnswer::Exists, Duration::ZERO) };
    let (rows, calls, _) = fake_list(&p, vec![fake.clone()], false);
    assert_eq!(states(&rows), [("e1".to_string(), timed_out), ("e2".into(), timed_out)]);
    assert_eq!(calls, [format!("session_exists {}", e.display()), format!("show_session_option {}", e.display())]);
    let fake = ListFake { owner: Err(MuxError::Failed { message: "x".into() }), ..fake };
    let (rows, _, _) = fake_list(&p, vec![fake], false);
    let err = ListState::Unknown("tmux error");
    assert_eq!(states(&rows), [("e1".to_string(), err), ("e2".into(), err)]);

    // (d) a failed clients read: `up` with no clients, and the next row on that socket gets no call.
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let socks = up_states(&p, t, &[("f000000000000001", "f1", "F"), ("f000000000000002", "f2", "F")]);
    let f = socks[0].1.clone();
    let fake = ListFake {
        owner: Ok(Some("f000000000000001".into())),
        clients: Err(MuxError::TimedOut),
        ..ListFake::new(&f, SessionAnswer::Exists, Duration::ZERO)
    };
    let (rows, calls, _) = fake_list(&p, vec![fake], false);
    assert_eq!(states(&rows), [("f1".to_string(), ListState::Up), ("f2".into(), timed_out)]);
    assert_eq!(rows[0].clients, None);
    assert_eq!(calls.len(), 3, "{calls:?}");
}

// ---- step 13 ----

#[test] // D11: no tmux program: `ls` still gives every row
fn list_without_tmux() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let sock = private_dir(&t.join("sock")).join("s");
    ws_state(&p, "a000000000000001", "u", &t.join("r/u"), |ws| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(sock.clone());
    });
    ws_state(&p, "a000000000000002", "n", &t.join("r/n"), |ws| ws.phase = Phase::Up);
    ws_state(&p, "a000000000000003", "d", &t.join("r/d"), |_| {});
    ws_state(&p, "a000000000000004", "b", &t.join("r/b"), |ws| ws.phase = Phase::Building);
    let rows = admin::list(&p, &t.join("none"));
    assert_eq!(
        states(&rows),
        [
            ("b".to_string(), ListState::Building),
            ("d".into(), ListState::Down),
            ("n".into(), ListState::Unknown("no tmux")),
            ("u".into(), ListState::Unknown("no tmux"))
        ]
    );
}

// ---- step 14 ----

#[test] // W§12 `ls` on a real private server: `up` with its clients, another owner, a session gone
fn list_reads_a_real_session() {
    let _s = serial();
    let Some(_) = require_tmux() else { return };
    let srv = PrivateServer::start();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    srv.run(&["new-session", "-d", "-s", "zygo-app", "sleep", "600"]).unwrap();
    srv.run(&["set-option", "-t", "=zygo-app:", "@zygo-ws", ID]).unwrap();
    let root = t.join("r/app");
    ws_state(&p, ID, "app", &root, |ws| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(srv.socket().to_path_buf());
    });
    let one = |rows: Vec<ListRow>| {
        assert_eq!(rows.len(), 1);
        (rows[0].state, rows[0].clients)
    };
    assert_eq!(one(admin::list(&p, srv.wrapper())), (ListState::Up, Some(0)));
    srv.run(&["set-option", "-t", "=zygo-app:", "@zygo-ws", OTHER]).unwrap();
    assert_eq!(one(admin::list(&p, srv.wrapper())), (ListState::Unreachable, None));
    srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
    assert_eq!(one(admin::list(&p, srv.wrapper())), (ListState::StoppedTmuxGone, None));
}

// ---- step 15 ----

#[test] // D12, D13; W§12 `workspace`
fn status_texts() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let plain = t.join("plain");
    fs::create_dir_all(&plain).unwrap();
    let app = ws_state(&p, ID, "app", &t.join("r/app"), |ws| {
        ws.phase = Phase::Up;
        ws.members = [
            ("kaaaaaaaa", member(Agent::ClaudeCode, MemberState::Running, Some("c-1"))),
            ("kbbbbbbbb", member(Agent::OpenCode, MemberState::Resumable, None)),
            ("kcccccccc", member(Agent::ClaudeCode, MemberState::Ended, Some("c\n3"))),
        ]
        .into_iter()
        .map(|(k, m)| (k.to_string(), m))
        .collect();
    });
    let want = "workspace app: up\nkaaaaaaaa claude-code running c-1\nkbbbbbbbb opencode resumable -\nkcccccccc claude-code ended c\\n3";
    assert_eq!(admin::status(&p, &plain, Some(OsStr::new(ID))).unwrap(), want);
    let r = repo(t, "own");
    let own = workspace_id(&canon(&r.join(".git")));
    Store::open(&p, &own)
        .update(S5, &mut |ws| {
            ws.name = "own".into();
            ws.tmux_session = "zygo-own".into();
            ws.common_dir = canon(&r.join(".git"));
            ws.main_root = r.clone();
            ws.phase = Phase::Down;
            Ok(())
        })
        .unwrap();
    assert_eq!(admin::status(&p, &r, None).unwrap(), "workspace own: down\nno members"); // cwd
    assert_eq!(admin::status(&p, &r, Some(OsStr::new(ID))).unwrap(), want); // ZYGO_WORKSPACE wins
    let e = |z: Option<&str>, cwd: &Path| admin::status(&p, cwd, z.map(OsStr::new)).unwrap_err().to_string();
    assert_eq!(e(Some("../x"), &plain), "invalid workspace id ../x");
    assert_eq!(e(Some(OTHER), &plain), "no workspace with id fedcba9876543210");
    assert_eq!(e(None, &plain), "no workspace for this session or folder");
    assert_eq!(e(None, &repo(t, "nostate")), "no workspace for this session or folder");
    raw_invalid_name(&p);
    raw_newer(&p, "e");
    let invalid = e(Some("e100000000000001"), &plain);
    assert!(invalid.ends_with("workspace.json: invalid name"), "{invalid}");
    assert_eq!(e(Some("e200000000000002"), &plain), "workspace state is from a newer zygo; run zygo update");
    for (phase, word) in [(Phase::Building, "building"), (Phase::Draining, "draining"), (Phase::Stopping, "stopping")] {
        app.update(S5, &mut |ws| {
            ws.phase = phase;
            Ok(())
        })
        .unwrap();
        let s = admin::status(&p, &plain, Some(OsStr::new(ID))).unwrap();
        assert_eq!(s.lines().next().unwrap(), format!("workspace app: {word}"));
    }
    let bad = admin::status(&p, &plain, Some(OsStr::from_bytes(b"\xff"))).unwrap_err().to_string();
    assert!(bad.starts_with("invalid workspace id "), "{bad}");
}

// ---- step 16 ----

#[test] // D14, D15; W§12 `ws forget`: every refusal removes nothing
fn forget_refusals() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    let sock_dir = private_dir(&t.join("sock"));
    let sock = sock_dir.join("s");
    ws_state(&p, ID, "app", &t.join("r/app"), |ws| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(sock.clone());
        ws.members.insert("kaaaaaaaa".into(), member_in(Agent::ClaudeCode, MemberState::Resumable, Some("c-1"), "01S"));
    });
    let res = result(&p, ID, "kaaaaaaaa");
    let bytes = state_bytes(&p, ID);
    let run = |name: &str| forget_run(&p, &tmux, None, name);
    let refused = |s: &str| (1, String::new(), format!("{s}\n"));

    fs::write(t.join("exists-zygo-app"), "").unwrap();
    fs::write(t.join("ws-zygo-app"), ID).unwrap();
    assert_eq!(run("app"), refused("zygo: app is running; run zygo down app first"));
    fs::remove_file(t.join("exists-zygo-app")).unwrap();
    fs::remove_file(t.join("ws-zygo-app")).unwrap();

    fs::write(t.join("error-zygo-app"), "").unwrap();
    assert_eq!(run("app"), refused("zygo: tmux: permission denied"));
    fs::remove_file(t.join("error-zygo-app")).unwrap();

    let l = live_session(&p, "01S");
    let _ = fs::remove_file(t.join("tmux.log"));
    assert_eq!(run("app"), refused("zygo: app has a live agent launcher (pane kaaaaaaaa); it is not forgotten"));
    assert!(tmux_log(t).iter().all(|l| !l.contains("has-session")));
    drop(l);

    let held = Store::open(&p, ID).op_lock().unwrap();
    assert_eq!(run("app"), refused("zygo: up or down is in progress for app"));
    drop(held);

    fs::set_permissions(&sock_dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        run("app"),
        refused(&format!(
            "zygo: tmux socket {} is not private: the socket and its folder must be yours and the folder must have mode 0700",
            sock.display()
        ))
    );
    fs::set_permissions(&sock_dir, fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(run("nosuch"), refused("zygo: no workspace named nosuch"));
    assert_eq!(run("a\nb"), refused("zygo: no workspace named a\\nb"));

    ws_state(&p, OTHER, "app", &t.join("r/app2"), |_| {});
    assert_eq!(
        run("app"),
        refused("zygo: more than one workspace has the name app; forget it by id: 0123456789abcdef, fedcba9876543210")
    );
    fs::remove_dir_all(p.workspaces_dir().join(OTHER)).unwrap();

    raw_newer(&p, "newapp");
    assert_eq!(run("e200000000000002"), refused(AU29)); // by id
    assert_eq!(run("newapp"), refused(AU29)); // by its JSON name, D14 (3)
    assert!(p.workspaces_dir().join("e200000000000002/workspace.json").exists());

    // The state vanishes after the lookup (D15).
    let dir = p.workspaces_dir().join(ID);
    let keep = t.join("keep.json");
    up::testing::hook(
        &dir,
        "forget_before_op_lock",
        Some(Box::new({
            let (dir, keep) = (dir.clone(), keep.clone());
            move || {
                fs::copy(dir.join("workspace.json"), &keep).map_err(|e| e.to_string())?;
                fs::remove_file(dir.join("workspace.json")).map_err(|e| e.to_string())
            }
        })),
    );
    assert_eq!(run("app"), refused("zygo: no workspace named app"));
    up::testing::hook(&dir, "forget_before_op_lock", None);
    fs::copy(&keep, dir.join("workspace.json")).unwrap();

    assert_eq!(state_bytes(&p, ID), bytes);
    assert!(res.join("r1").exists());
}

// ---- step 17 ----

#[test] // D14-D16: the state and its results cache go; a session of another workspace does not refuse; links stay links
fn forget_removes_state_and_cache() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |_| {});
    result(&p, ID, "kaaaaaaaa");
    ws_state(&p, OTHER, "other", &t.join("r/other"), |_| {});
    result(&p, OTHER, "kbbbbbbbb");
    fs::write(t.join("exists-zygo-app"), "").unwrap();
    fs::write(t.join("ws-zygo-app"), OTHER).unwrap();
    assert_eq!(forget_run(&p, &tmux, None, "app"), (0, "zygo: forgot app\n".to_string(), String::new()));
    assert!(!p.workspaces_dir().join(ID).exists());
    assert!(!cache(&p, ID).exists());
    assert!(p.workspaces_dir().join(OTHER).join("workspace.json").exists());
    assert!(cache(&p, OTHER).join("results/kbbbbbbbb/r1").exists());
    let tomb = fs::read_dir(p.workspaces_dir()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with(".forget-"));
    assert!(!tomb);

    let gone = t.join("gone/s");
    ws_state(&p, "2000000000000002", "app2", &t.join("r/app2"), |ws| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(gone.clone());
    });
    assert_eq!(forget_run(&p, &tmux, None, "app2"), (0, "zygo: forgot app2\n".to_string(), String::new()));
    assert!(!tmux_log(t).iter().any(|l| l.contains(&gone.display().to_string())));
    assert!(!t.join("gone").exists());

    let store3 = ws_state(&p, "3000000000000003", "app3", &t.join("r/app3"), |_| {});
    fs::create_dir_all(t.join("outside")).unwrap();
    fs::write(t.join("outside/keep"), "k").unwrap();
    fs::create_dir_all(t.join("outside2")).unwrap();
    fs::write(t.join("outside2/keep2"), "k").unwrap();
    fs::create_dir_all(p.workspace_cache_dir()).unwrap();
    std::os::unix::fs::symlink(t.join("outside"), cache(&p, "3000000000000003")).unwrap();
    std::os::unix::fs::symlink(t.join("outside2"), store3.dir().join("link")).unwrap();
    assert_eq!(forget_run(&p, &t.join("none"), None, "app3"), (0, "zygo: forgot app3\n".to_string(), String::new()));
    assert!(fs::symlink_metadata(cache(&p, "3000000000000003")).is_err());
    assert!(t.join("outside/keep").exists() && t.join("outside2/keep2").exists());
}

// ---- step 18 ----

#[test] // D17: a state that cannot be read is checked from its raw JSON
fn forget_invalid_state() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    let sock = private_dir(&t.join("sock")).join("s");
    let put = |id: &str, v: Value| put_raw(&p.workspaces_dir().join(id), &v);

    put("e100000000000001", invalid_doc("e100000000000001", json!("app"), Some(&sock)));
    fs::write(t.join("exists-zygo-app"), "").unwrap();
    assert_eq!(
        forget_run(&p, &tmux, None, "e100000000000001"),
        (1, String::new(), repair_line("e100000000000001", "its session runs"))
    );
    assert!(p.workspaces_dir().join("e100000000000001/workspace.json").exists());

    let _ = fs::remove_file(t.join("tmux.log"));
    put("e200000000000002", invalid_doc("e200000000000002", json!("a b"), Some(&sock)));
    assert_eq!(
        forget_run(&p, &tmux, None, "e200000000000002"),
        (1, String::new(), repair_line("e200000000000002", "it has no valid name"))
    );
    assert!(!tmux_log(t).iter().any(|l| l.contains("has-session")));

    put("e300000000000003", invalid_doc("e300000000000003", json!("bee"), None));
    assert_eq!(
        forget_run(&p, &tmux, None, "e300000000000003"),
        (1, String::new(), repair_line("e300000000000003", "it has no usable tmux socket"))
    );

    let mut v = invalid_doc("e400000000000004", json!("cee"), Some(&sock));
    let m = member_in(Agent::ClaudeCode, MemberState::Resumable, Some("c-1"), "01S");
    v["members"] = json!({"kaaaaaaaa": serde_json::to_value(&m).unwrap()});
    put("e400000000000004", v);
    let _l = live_session(&p, "01S");
    assert_eq!(
        forget_run(&p, &tmux, None, "e400000000000004"),
        (1, String::new(), repair_line("e400000000000004", "a member launcher is live (pane kaaaaaaaa)"))
    );

    let mut v = invalid_doc("e500000000000005", json!("dee"), Some(&sock));
    v["members"] = json!(7);
    put("e500000000000005", v);
    assert_eq!(
        forget_run(&p, &tmux, None, "e500000000000005"),
        (1, String::new(), repair_line("e500000000000005", "its members cannot be read"))
    );

    put("e600000000000006", invalid_doc("e600000000000006", json!("eff"), Some(&sock)));
    result(&p, "e600000000000006", "kaaaaaaaa");
    assert_eq!(
        forget_run(&p, &tmux, None, "e600000000000006"),
        (0, "zygo: forgot e600000000000006\n".to_string(), String::new())
    );
    assert!(!p.workspaces_dir().join("e600000000000006").exists());
    assert!(!cache(&p, "e600000000000006").exists());
    assert_eq!(has_sessions(t).last().map(String::as_str), Some("=zygo-eff:")); // the check ran
}

// ---- step 19 ----

#[test] // D14: a workspace id is matched first, then a name
fn forget_matches_an_id_first() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    let plain = t.join("plain");
    fs::create_dir_all(&plain).unwrap();
    ws_state(&p, ID, "app", &t.join("r/y"), |_| {});
    ws_state(&p, OTHER, ID, &t.join("r/x"), |_| {});
    assert_eq!(forget_run(&p, &tmux, None, ID), (0, "zygo: forgot app\n".to_string(), String::new())); // the id
    assert!(p.workspaces_dir().join(OTHER).exists());
    assert_eq!(forget_run(&p, &tmux, None, ID), (0, format!("zygo: forgot {ID}\n"), String::new())); // now the name
    assert!(!p.workspaces_dir().join(OTHER).exists());

    let with_member = |ws: &mut Workspace| {
        ws.members.insert("kaaaaaaaa".into(), member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1")));
    };
    ws_state(&p, ID, "app", &t.join("r/y2"), with_member);
    let x2 = ws_state(&p, OTHER, ID, &t.join("r/x2"), with_member);
    assert_eq!(
        pane_run(&p, &tmux, Some(ID), &plain, "kaaaaaaaa"),
        (0, "zygo: forgot pane kaaaaaaaa of app\n".to_string(), String::new())
    );
    assert!(x2.read().unwrap().unwrap().members.contains_key("kaaaaaaaa"));
    assert!(!Store::open(&p, ID).read().unwrap().unwrap().members.contains_key("kaaaaaaaa"));
}

// ---- step 20 ----

#[test] // D16, D21: a failure after the cache keeps the state, and the same `forget` runs again
fn forget_failure_keeps_the_state() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |_| {});
    result(&p, ID, "kaaaaaaaa");
    let free = Arc::new(AtomicBool::new(true));
    up::testing::hook(
        &p.workspaces_dir().join(ID),
        "forget_before_state",
        Some(Box::new({
            let (s, f) = (Store::open(&p, ID), free.clone());
            move || {
                f.store(s.op_lock_free(), Ordering::SeqCst);
                Err("injected".into())
            }
        })),
    );
    assert_eq!(forget_run(&p, &tmux, None, "app"), (1, String::new(), "zygo: cannot forget app: injected\n".to_string()));
    assert!(!free.load(Ordering::SeqCst)); // op.lock held during the removal
    assert!(!cache(&p, ID).exists());
    assert_eq!(Store::open(&p, ID).read().unwrap().unwrap().name, "app");
    up::testing::hook(&p.workspaces_dir().join(ID), "forget_before_state", None);
    assert_eq!(forget_run(&p, &tmux, None, "app"), (0, "zygo: forgot app\n".to_string(), String::new()));
    assert!(!p.workspaces_dir().join(ID).exists());
}

// ---- step 21 ----

#[test] // D16 step 4, D26: the next `forget` removes a leftover folder and orphans whose locks are free
fn forget_removes_leftover_and_orphan_folders() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |_| {});
    up::testing::hook(&p.workspaces_dir().join(ID), "forget_before_tombstone_remove", Some(Box::new(|| Err("injected".into()))));
    let (code, out, err) = forget_run(&p, &tmux, None, "app");
    let left: Vec<PathBuf> = fs::read_dir(p.workspaces_dir())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|q| q.file_name().unwrap().to_str().unwrap().starts_with(".forget-"))
        .collect();
    assert_eq!(left.len(), 1);
    assert_eq!(
        (code, out.as_str(), err),
        (1, "", format!("zygo: app forgotten, but {} was not removed: injected\n", left[0].display()))
    );
    assert!(admin::list(&p, &tmux).is_empty()); // the leftover is no workspace
    up::testing::hook(&p.workspaces_dir().join(ID), "forget_before_tombstone_remove", None);

    let o1 = p.workspaces_dir().join("1111111111111111");
    fs::create_dir_all(&o1).unwrap();
    fs::write(o1.join("lock"), "").unwrap();
    fs::write(o1.join("errors.log"), "1 x\n").unwrap();
    let o2 = p.workspaces_dir().join("2222222222222222");
    fs::create_dir_all(&o2).unwrap();
    fs::write(o2.join("lock"), "").unwrap();
    let _held = Store::open(&p, "2222222222222222").op_lock().unwrap(); // an `up` that has not written yet
    ws_state(&p, OTHER, "other", &t.join("r/other"), |_| {});
    assert_eq!(forget_run(&p, &tmux, None, "nosuch"), (1, String::new(), "zygo: no workspace named nosuch\n".to_string()));
    assert!(!left[0].exists());
    assert!(!o1.exists());
    assert!(o2.exists());
    assert!(p.workspaces_dir().join(OTHER).join("workspace.json").exists());
}

// ---- step 22 ----

#[test] // W§12 `ws forget` on a real private server: refused while the session runs
fn forget_refused_while_the_session_runs() {
    let _s = serial();
    let Some(_) = require_tmux() else { return };
    let srv = PrivateServer::start();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    srv.run(&["new-session", "-d", "-s", "zygo-app", "sleep", "600"]).unwrap();
    srv.run(&["set-option", "-t", "=zygo-app:", "@zygo-ws", ID]).unwrap();
    ws_state(&p, ID, "app", &t.join("r/app"), |ws| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(srv.socket().to_path_buf());
    });
    assert_eq!(
        forget_run(&p, srv.wrapper(), None, "app"),
        (1, String::new(), "zygo: app is running; run zygo down app first\n".to_string())
    );
    srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
    assert_eq!(forget_run(&p, srv.wrapper(), None, "app"), (0, "zygo: forgot app\n".to_string(), String::new()));
}

// ---- steps 23 and 24 ----

struct PaneSetup {
    td: TempDir,
    p: Paths,
    tmux: PathBuf,
    r: PathBuf,
    id: String,
    store: Store,
    plain: PathBuf,
    seq0: u64,
}

fn pane_setup() -> PaneSetup {
    let td = TempDir::new().unwrap();
    let t = td.path().to_path_buf();
    let p = paths(&t);
    let tmux = answer_tmux(&t);
    let r = repo(&t, "app");
    let id = workspace_id(&canon(&r.join(".git")));
    let store = Store::open(&p, &id);
    let ws = store
        .update(S5, &mut |ws| {
            ws.name = "app".into();
            ws.tmux_session = "zygo-app".into();
            ws.common_dir = canon(&r.join(".git"));
            ws.main_root = r.clone();
            ws.loadout = "solo".into();
            ws.phase = Phase::Down;
            ws.panes = vec![pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, &r)];
            ws.members.insert("kaaaaaaaa".into(), member_in(Agent::ClaudeCode, MemberState::Resumable, Some("c-1"), "01P"));
            ws.members.insert("kbbbbbbbb".into(), member(Agent::ClaudeCode, MemberState::Resumable, Some("c-2")));
            Ok(())
        })
        .unwrap();
    result(&p, &id, "kaaaaaaaa");
    result(&p, &id, "kbbbbbbbb");
    let plain = t.join("plain");
    fs::create_dir_all(&plain).unwrap();
    PaneSetup { td, p, tmux, r, id, store, plain, seq0: ws.snapshot_seq }
}

#[test] // D18-D20; W§12 `ws forget-pane`
fn forget_pane_cases() {
    let _s = serial();
    let PaneSetup { td, p, tmux, r, id, store, plain, seq0 } = pane_setup();
    let t = td.path();
    fs::write(store.dir().join("snapshot.pending"), "").unwrap();
    assert_eq!(
        pane_run(&p, &tmux, Some("app"), &plain, "kbbbbbbbb"),
        (0, "zygo: forgot pane kbbbbbbbb of app\n".to_string(), String::new())
    );
    let ws = store.read().unwrap().unwrap();
    assert_eq!(ws.members.keys().collect::<Vec<_>>(), ["kaaaaaaaa"]);
    assert_eq!((ws.panes[0].kind, ws.snapshot_seq), (PaneKind::ClaudeCode, seq0)); // no pane entry for kbbbbbbbb: panes[] unchanged
    assert!(!cache(&p, &id).join("results/kbbbbbbbb").exists() && cache(&p, &id).join("results/kaaaaaaaa/r1").exists());
    assert!(!store.dir().join("snapshot.pending").exists()); // W§7.3 check ran
    let l = live_session(&p, "01P");
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kaaaaaaaa"),
        (1, String::new(), "zygo: pane kaaaaaaaa of app has a live agent launcher; it is not forgotten\n".to_string())
    );
    assert!(store.read().unwrap().unwrap().members.contains_key("kaaaaaaaa"));
    drop(l);
    let held = store.op_lock().unwrap();
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kaaaaaaaa"),
        (1, String::new(), "zygo: up or down is in progress for app\n".to_string())
    );
    drop(held);
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kaaaaaaaa"),
        (0, "zygo: forgot pane kaaaaaaaa of app\n".to_string(), String::new())
    ); // by cwd
    let ws = store.read().unwrap().unwrap();
    assert_eq!((ws.phase, ws.panes[0].kind, ws.snapshot_seq), (Phase::Down, PaneKind::Shell, seq0 + 1)); // D20, AU53, in phase down
    assert_eq!(pane_run(&p, &tmux, None, &r, "k$"), (1, String::new(), "zygo: invalid pane key k$\n".to_string()));
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kzzzzzzzz"),
        (1, String::new(), "zygo: app has no member record for pane kzzzzzzzz\n".to_string())
    );
    let orphan = result(&p, &id, "kyyyyyyyy");
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kyyyyyyyy"),
        (0, "zygo: forgot pane kyyyyyyyy of app\n".to_string(), String::new())
    );
    assert!(!orphan.exists());
    let nowhere = (1, String::new(), "zygo: no workspace for this folder; run in the workspace's checkout or pass --name\n".to_string());
    assert_eq!(pane_run(&p, &tmux, None, &plain, "kaaaaaaaa"), nowhere);
    assert_eq!(pane_run(&p, &tmux, None, &repo(t, "nostate"), "kaaaaaaaa"), nowhere);
    assert_eq!(
        pane_run(&p, &tmux, Some("nosuch"), &plain, "kaaaaaaaa"),
        (1, String::new(), "zygo: no workspace named nosuch\n".to_string())
    );

    ws_state(&p, OTHER, "app", &t.join("r/app2"), |_| {});
    assert_eq!(
        pane_run(&p, &tmux, Some("app"), &plain, "kaaaaaaaa"),
        (1, String::new(), "zygo: more than one workspace has the name app; run in its checkout\n".to_string())
    );
    let bad = repo(t, "bad");
    let bad_id = workspace_id(&canon(&bad.join(".git")));
    put_raw(
        &p.workspaces_dir().join(&bad_id),
        &json!({"version": 1, "id": bad_id, "name": "a b", "common_dir": canon(&bad.join(".git")), "main_root": bad,
            "tmux_session": "zygo-a b", "loadout": "solo", "phase": "down", "phase_since": 1}),
    );
    let (code, out, err) = pane_run(&p, &tmux, None, &bad, "kaaaaaaaa");
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.starts_with("zygo: ") && err.ends_with("invalid name\n"), "{err}");
}

#[test] // D18, D19, D21, D22: a failure before the cache, a retry, `lock` held past 5 s, a state that vanishes
fn forget_pane_failure_vanish_and_busy() {
    let _s = serial();
    let PaneSetup { td: _td, p, tmux, r, id, store, plain, .. } = pane_setup();
    up::testing::hook(store.dir(), "forget_pane_before_cache", Some(Box::new(|| Err("injected".into()))));
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kbbbbbbbb"),
        (1, String::new(), "zygo: cannot forget pane kbbbbbbbb of app: injected\n".to_string())
    );
    assert!(!store.read().unwrap().unwrap().members.contains_key("kbbbbbbbb"));
    assert!(cache(&p, &id).join("results/kbbbbbbbb/r1").exists());
    up::testing::hook(store.dir(), "forget_pane_before_cache", None);
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kbbbbbbbb"),
        (0, "zygo: forgot pane kbbbbbbbb of app\n".to_string(), String::new())
    );
    assert!(!cache(&p, &id).join("results/kbbbbbbbb").exists());
    let lock = hold(store.dir().join("lock")); // Busy: held for the whole 5 s OP_WAIT
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kaaaaaaaa"),
        (1, String::new(), "zygo: workspace app is busy; try again\n".to_string())
    );
    drop(lock);
    assert!(store.read().unwrap().unwrap().members.contains_key("kaaaaaaaa"));

    let dir = store.dir().to_path_buf();
    up::testing::hook(
        store.dir(),
        "forget_pane_before_op_lock",
        Some(Box::new(move || fs::remove_file(dir.join("workspace.json")).map_err(|e| e.to_string()))),
    );
    assert_eq!(
        pane_run(&p, &tmux, Some("app"), &plain, "kaaaaaaaa"),
        (1, String::new(), "zygo: no workspace named app\n".to_string())
    );
    up::testing::hook(store.dir(), "forget_pane_before_op_lock", None);
}

// ---- Round 2 ----

/// Every entry below `dir` (and `dir` itself as ""), with its mode and, for a file, its bytes; sorted.
fn snapshot_tree(dir: &Path) -> Vec<(PathBuf, u32, Vec<u8>)> {
    fn walk(root: &Path, at: &Path, out: &mut Vec<(PathBuf, u32, Vec<u8>)>) {
        let m = fs::symlink_metadata(at).unwrap();
        let bytes = if m.is_file() { fs::read(at).unwrap() } else { Vec::new() };
        out.push((at.strip_prefix(root).unwrap().to_path_buf(), m.permissions().mode(), bytes));
        if m.is_dir() {
            for e in fs::read_dir(at).unwrap().flatten() {
                walk(root, &e.path(), out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// A folder `target` (mode 0755) that holds a readable state of `id` named `name` with member `kaaaaaaaa`, and a file
/// `keep`; the state is made in the scratch state folder `<t>/scratch` and copied.
fn state_target(t: &Path, target: &Path, id: &str, name: &str) {
    let scratch = paths(&t.join("scratch"));
    let store = ws_state(&scratch, id, name, &t.join("r").join(name), |ws| {
        ws.members.insert("kaaaaaaaa".into(), member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1")));
    });
    fs::create_dir_all(target).unwrap();
    fs::copy(store.dir().join("workspace.json"), target.join("workspace.json")).unwrap();
    fs::write(target.join("keep"), "k").unwrap();
    fs::set_permissions(target, fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_dir_all(scratch.workspaces_dir().join(id)).unwrap();
}

fn link_line(id: &str) -> String { format!("zygo: the state folder of workspace {id} is a link; it is not changed\n") }

#[test] // Round 2 item 1: a state folder that is a link is never opened, locked, written or chmod-ed
fn a_state_folder_that_is_a_link() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    let plain = t.join("plain");
    fs::create_dir_all(&plain).unwrap();
    let target = t.join("target");
    state_target(t, &target, ID, "app");
    fs::create_dir_all(p.workspaces_dir()).unwrap();
    let link = p.workspaces_dir().join(ID);
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let before = snapshot_tree(&target);

    assert_eq!(pane_run(&p, &tmux, Some(ID), &plain, "kaaaaaaaa"), (1, String::new(), link_line(ID)));
    assert_eq!(snapshot_tree(&target), before); // forget-pane writes nothing in the target
    assert_eq!(forget_run(&p, &tmux, None, "app"), (1, String::new(), "zygo: no workspace named app\n".to_string()));
    assert_eq!(snapshot_tree(&target), before);
    assert_eq!(
        forget_run(&p, &tmux, None, ID),
        (0, format!("zygo: {} was a link, not a state folder; removed the link\n", link.display()), String::new())
    );
    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(snapshot_tree(&target), before); // mode, contents and files unchanged
    assert!(!target.join("op.lock").exists() && !target.join("lock").exists());
    assert!(admin::list(&p, &tmux).is_empty());

    // By the checkout: the workspace id of the cwd names a link.
    let r = repo(t, "own");
    let own = workspace_id(&canon(&r.join(".git")));
    let target2 = t.join("target2");
    state_target(t, &target2, &own, "own");
    std::os::unix::fs::symlink(&target2, p.workspaces_dir().join(&own)).unwrap();
    let before2 = snapshot_tree(&target2);
    assert_eq!(pane_run(&p, &tmux, None, &r, "kaaaaaaaa"), (1, String::new(), link_line(&own)));
    assert_eq!(snapshot_tree(&target2), before2);
}

#[test] // Round 2 items 2 and 3: `forget` takes `op.lock` and `lock` before it deletes anything; the next `forget` sweeps an orphan cache
fn forget_takes_both_locks_first_and_sweeps_orphan_caches() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    let store = ws_state(&p, ID, "app", &t.join("r/app"), |_| {});
    result(&p, ID, "kaaaaaaaa");
    let bytes = state_bytes(&p, ID);
    let lock = hold(store.dir().join("lock")); // held for the whole 5 s wait: a fixed hold, not a race
    assert_eq!(forget_run(&p, &tmux, None, "app"), (1, String::new(), "zygo: workspace app is busy; try again\n".to_string()));
    drop(lock);
    assert_eq!(state_bytes(&p, ID), bytes);
    assert!(cache(&p, ID).join("results/kaaaaaaaa/r1").exists()); // nothing deleted, the cache included
    assert_eq!(forget_run(&p, &tmux, None, "app"), (0, "zygo: forgot app\n".to_string(), String::new()));

    // A writer makes the cache again for the forgotten id; an `up` that holds `op.lock` keeps its cache.
    result(&p, ID, "kaaaaaaaa");
    let o2 = p.workspaces_dir().join("2222222222222222");
    fs::create_dir_all(&o2).unwrap();
    let _held = Store::open(&p, "2222222222222222").op_lock().unwrap();
    result(&p, "2222222222222222", "kaaaaaaaa");
    ws_state(&p, OTHER, "other", &t.join("r/other"), |_| {});
    result(&p, OTHER, "kbbbbbbbb");
    assert!(admin::list(&p, &tmux).iter().all(|r| r.name == "other"));
    assert!(cache(&p, ID).exists()); // `ls` is read-only: it removes no cache
    assert_eq!(forget_run(&p, &tmux, None, "nosuch"), (1, String::new(), "zygo: no workspace named nosuch\n".to_string()));
    assert!(!cache(&p, ID).exists()); // the next `forget` sweeps it
    assert!(cache(&p, "2222222222222222").join("results/kaaaaaaaa/r1").exists());
    assert!(cache(&p, OTHER).join("results/kbbbbbbbb/r1").exists());
}

#[test] // Round 2 item 4: each part of the cache path is checked; a link in the middle is refused, a link at the end removed as a link
fn forget_pane_checks_each_part_of_the_cache_path() {
    let _s = serial();
    let PaneSetup { td, p, tmux, r, id, store, .. } = pane_setup();
    let t = td.path();
    let refused = |path: &Path| {
        (1, String::new(), format!("zygo: cannot forget pane kbbbbbbbb of app: {} is a link; it is not followed\n", path.display()))
    };
    // A link at <id>.
    let outside = t.join("outside");
    fs::create_dir_all(outside.join("results/kbbbbbbbb")).unwrap();
    fs::write(outside.join("results/kbbbbbbbb/r1"), "r").unwrap();
    let real = t.join("real-cache");
    fs::rename(cache(&p, &id), &real).unwrap();
    std::os::unix::fs::symlink(&outside, cache(&p, &id)).unwrap();
    let before = snapshot_tree(&outside);
    assert_eq!(pane_run(&p, &tmux, None, &r, "kbbbbbbbb"), refused(&cache(&p, &id)));
    assert_eq!(snapshot_tree(&outside), before);
    assert!(store.read().unwrap().unwrap().members.contains_key("kbbbbbbbb")); // nothing written
    fs::remove_file(cache(&p, &id)).unwrap();
    fs::rename(&real, cache(&p, &id)).unwrap();

    // A link at results.
    let results = cache(&p, &id).join("results");
    let real_results = t.join("real-results");
    fs::rename(&results, &real_results).unwrap();
    std::os::unix::fs::symlink(outside.join("results"), &results).unwrap();
    assert_eq!(pane_run(&p, &tmux, None, &r, "kbbbbbbbb"), refused(&results));
    assert_eq!(snapshot_tree(&outside), before);
    assert!(store.read().unwrap().unwrap().members.contains_key("kbbbbbbbb"));
    fs::remove_file(&results).unwrap();
    fs::rename(&real_results, &results).unwrap();

    // A link at <key>: removed as a link.
    fs::remove_dir_all(results.join("kbbbbbbbb")).unwrap();
    std::os::unix::fs::symlink(outside.join("results/kbbbbbbbb"), results.join("kbbbbbbbb")).unwrap();
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kbbbbbbbb"),
        (0, "zygo: forgot pane kbbbbbbbb of app\n".to_string(), String::new())
    );
    assert!(fs::symlink_metadata(results.join("kbbbbbbbb")).is_err());
    assert_eq!(snapshot_tree(&outside), before);
}

#[test] // Round 2 item 5: with no tmux, `forget` still runs the AU62 check of the socket
fn forget_without_tmux_checks_the_socket() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let open = t.join("open");
    fs::create_dir_all(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
    let sock = open.join("s");
    ws_state(&p, ID, "app", &t.join("r/app"), |ws| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(sock.clone());
    });
    let bytes = state_bytes(&p, ID);
    assert_eq!(
        forget_run(&p, &t.join("none"), None, "app"),
        (1, String::new(), format!(
            "zygo: tmux socket {} is not private: the socket and its folder must be yours and the folder must have mode 0700\n",
            sock.display()
        ))
    );
    assert_eq!(state_bytes(&p, ID), bytes);
}

#[test] // Round 2 item 6: W8 reads at most 1 MiB of a state file; a larger file is invalid
fn a_state_file_over_1_mib_is_invalid() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    let plain = t.join("plain");
    fs::create_dir_all(&plain).unwrap();
    let store = ws_state(&p, ID, "big", &t.join("r/big"), |ws| {
        ws.members.insert("kaaaaaaaa".into(), member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1")));
    });
    let file = store.dir().join("workspace.json");
    let mut v: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    v["pad"] = json!("x".repeat(2 * 1024 * 1024));
    fs::write(&file, serde_json::to_vec(&v).unwrap()).unwrap();
    let bytes = fs::read(&file).unwrap();
    let too_large = repair_line(ID, "state file too large");
    assert_eq!(forget_run(&p, &tmux, None, ID), (1, String::new(), too_large)); // by id
    // FIX-17: the store reads at most 1 MiB + 1 byte (AU84), so the name in a larger file is never read and the state
    // is found by its id only, like any other state that cannot be read (D14).
    let no_name = (1, String::new(), "zygo: no workspace named big\n".to_string());
    assert_eq!(forget_run(&p, &tmux, None, "big"), no_name); // by name
    assert_eq!(pane_run(&p, &tmux, Some("big"), &plain, "kaaaaaaaa"), no_name);
    assert_eq!(
        pane_run(&p, &tmux, Some(ID), &plain, "kaaaaaaaa"),
        (1, String::new(), format!("zygo: {}: state file too large\n", file.display()))
    );
    assert_eq!(fs::read(&file).unwrap(), bytes);
}

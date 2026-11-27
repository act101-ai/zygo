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
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_workspace::admin::{self, ListRow, ListState};
use zygo_workspace::identity::workspace_id;
use zygo_workspace::mail::{mail_dir, store as mail_store, Message};
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
/// The AU54 line (`mux/mod.rs:697-698`).
const AU54: &str =
    "zygo: the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it";

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

/// FIX-30: the socket `<t>/so'ck/s` in a 0700 folder: it passes AU62 and fails AU54.
fn quoted_socket(t: &Path) -> PathBuf { private_dir(&t.join("so'ck")).join("s") }

/// FIX-30: the line of a `forget` that a saved socket refuses by AU54 (W§12 `ws forget`).
fn forget_au54_line(p: &Paths, id: &str) -> String {
    format!("{AU54}; remove {} by hand to forget this workspace\n", p.workspaces_dir().join(id).display())
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
    forget_with(p, tmux, env, name, &mut |_: &Workspace| Ok(true))
}

/// `admin::forget` with the `before_remove` step `before` (S§7.6, MS-21).
fn forget_with(
    p: &Paths, tmux: &Path, env: Option<&str>, name: &str, before: &mut dyn FnMut(&Workspace) -> Result<bool, String>,
) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = admin::forget(p, tmux, env.map(OsStr::new), name, before, &mut out, &mut err);
    (code, text(out), text(err))
}

/// Makes `mail/sync/` of workspace `id` (the folder the first sync makes) and returns it.
fn sync_dir(p: &Paths, id: &str) -> PathBuf {
    let d = mail_dir(p, id).join("sync");
    fs::create_dir_all(&d).unwrap();
    d
}

/// Whether a new open of the existing file `path` can take its lock now; the lock ends at once.
fn lock_free(path: &Path) -> bool {
    let f = fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
    fs4::FileExt::try_lock(&f).is_ok()
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

/// Delivers `n` messages to `mail/<key>/new` of workspace `id` (M§5 send).
fn mail_to(p: &Paths, id: &str, key: &str, n: usize) {
    for i in 0..n {
        let m = Message::new("cli", "x", &format!("s{i}"), "b", SystemTime::now()).unwrap();
        mail_store::deliver(&mail_dir(p, id), key, "x", &m, 500).unwrap();
    }
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
    fn send_keys_literal(&self, _: &str, _: &str) -> Result<(), MuxError> { self.other() }
    fn send_enter(&self, _: &str) -> Result<(), MuxError> { self.other() }
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
    let row = |name: &str, root: Option<&str>, state, agents, clients, unread: Option<usize>| ListRow {
        id: "0123456789abcdef".into(),
        name: name.into(),
        main_root: root.map(PathBuf::from),
        state,
        agents,
        clients,
        unread,
    };
    assert_eq!(
        row("app", Some("/srv/app"), ListState::Up, Some((1, 2, 0)), Some(1), Some(3)).to_string(),
        "app  /srv/app  up  agents 1/2/0  clients 1  unread 3"
    );
    assert_eq!(
        row("app", Some("/srv/a\nb"), ListState::StoppedTmuxGone, Some((0, 0, 3)), None, Some(0)).to_string(),
        "app  /srv/a\\nb  stopped (tmux gone)  agents 0/0/3  clients -  unread 0"
    );
    assert_eq!(
        row("0123456789abcdef", None, ListState::Unreadable, None, None, None).to_string(),
        "0123456789abcdef  -  unreadable  agents -  clients -  unread -"
    );
    assert_eq!(
        row("app", Some("/srv/app"), ListState::Unknown("tmux timed out"), Some((0, 1, 0)), None, None).to_string(),
        "app  /srv/app  unknown (tmux timed out)  agents 0/1/0  clients -  unread -"
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
#[ignore = "timing: stand-in tmux under the 2 s tmux bound and the 4 s ls deadline"]
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
#[ignore = "timing: stand-in tmux under the 2 s tmux bound and the 4 s ls deadline"]
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
#[ignore = "timing: real tmux under the fixed 2 s tmux bound and the 4 s ls deadline"]
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
#[ignore = "timing: git under the fixed 2 s status git bound"]
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
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
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
    assert_eq!(run("app"), refused("zygo: app is running; run zygo down -- app first"));
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

#[test] // W§12 `ws forget`, FIX-16: the running hint gives the name after `--`
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
fn forget_running_hint_passes_a_dash_name_after_double_dash() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    ws_state(&p, ID, "-x", &t.join("r/x"), |ws| ws.phase = Phase::Up);
    let bytes = state_bytes(&p, ID);
    fs::write(t.join("exists-zygo--x"), "").unwrap();
    fs::write(t.join("ws-zygo--x"), ID).unwrap();
    assert_eq!(forget_run(&p, &tmux, None, "-x"), (1, String::new(), "zygo: -x is running; run zygo down -- -x first\n".to_string()));
    assert_eq!(state_bytes(&p, ID), bytes);                          // nothing removed
}

// ---- step 17 ----

#[test] // D14-D16: the state and its results cache go; a session of another workspace does not refuse; links stay links
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
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
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
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
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
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

#[test] // D16, D21: a failure before the state rename keeps the state and the cache, and the same `forget` runs again
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
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
    assert!(cache(&p, ID).join("results/kaaaaaaaa/r1").exists()); // M2 round 3: the cache goes after the state
    assert_eq!(Store::open(&p, ID).read().unwrap().unwrap().name, "app");
    up::testing::hook(&p.workspaces_dir().join(ID), "forget_before_state", None);
    assert_eq!(forget_run(&p, &tmux, None, "app"), (0, "zygo: forgot app\n".to_string(), String::new()));
    assert!(!p.workspaces_dir().join(ID).exists());
}

// ---- step 21 ----

#[test] // D16 step 4, D26: the next `forget` removes a leftover folder and orphans whose locks are free
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
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
#[ignore = "timing: real tmux under the fixed 2 s tmux bound"]
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
        (1, String::new(), "zygo: app is running; run zygo down -- app first\n".to_string())
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
#[ignore = "timing: git under the fixed 5 s forget-pane git bound"]
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
#[ignore = "timing: git under the fixed 5 s forget-pane git bound"]
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
#[ignore = "timing: stand-in tmux under the 2 s tmux bound and the 4 s ls deadline"]
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
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
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
#[ignore = "timing: git under the fixed 5 s forget-pane git bound"]
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

/// Makes `<cache>/program/<key>/opencode` as a link to `target`; returns `<cache>/program/<key>`.
fn program(p: &Paths, id: &str, key: &str, target: &Path) -> PathBuf {
    let d = cache(p, id).join("program").join(key);
    fs::create_dir_all(&d).unwrap();
    std::os::unix::fs::symlink(target, d.join("opencode")).unwrap();
    d
}

#[test] // M2 review round 2 finding 4: W§12 `ws forget-pane` also deletes `program/<key>` (AU61), with the rules of `results/<key>`
#[ignore = "timing: git under the fixed 5 s forget-pane git bound"]
fn forget_pane_removes_the_program_folder() {
    let _s = serial();
    let PaneSetup { td, p, tmux, r, id, .. } = pane_setup();
    let t = td.path();
    let saved = t.join("saved/opencode");
    fs::create_dir_all(saved.parent().unwrap()).unwrap();
    fs::write(&saved, "bin").unwrap();
    let b = program(&p, &id, "kbbbbbbbb", &saved);
    let a = program(&p, &id, "kaaaaaaaa", &saved);
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kbbbbbbbb"),
        (0, "zygo: forgot pane kbbbbbbbb of app\n".to_string(), String::new())
    );
    assert!(fs::symlink_metadata(&b).is_err() && a.join("opencode").exists());
    assert_eq!(fs::read(&saved).unwrap(), b"bin"); // the link target stays
    // A key with no member record and only a program folder: the folder goes.
    let orphan = program(&p, &id, "kyyyyyyyy", &saved);
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kyyyyyyyy"),
        (0, "zygo: forgot pane kyyyyyyyy of app\n".to_string(), String::new())
    );
    assert!(fs::symlink_metadata(&orphan).is_err());
    // A link at `program` refuses before anything is written.
    let outside = t.join("outside-program");
    fs::create_dir_all(outside.join("kaaaaaaaa")).unwrap();
    fs::write(outside.join("kaaaaaaaa/opencode"), "keep").unwrap();
    let prog = cache(&p, &id).join("program");
    fs::remove_dir_all(&prog).unwrap();
    std::os::unix::fs::symlink(&outside, &prog).unwrap();
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kaaaaaaaa"),
        (1, String::new(), format!("zygo: cannot forget pane kaaaaaaaa of app: {} is a link; it is not followed\n", prog.display()))
    );
    assert_eq!(fs::read(outside.join("kaaaaaaaa/opencode")).unwrap(), b"keep");
}

#[test] // M2 review round 3 finding 4: `ws forget` removes the state before the cache, so no later writer of the cache finds the state
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
fn forget_removes_the_state_before_the_cache() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |_| {});
    result(&p, ID, "kaaaaaaaa");
    let seen = Arc::new(std::sync::Mutex::new(None));
    up::testing::hook(
        &p.workspaces_dir().join(ID),
        "forget_before_cache",
        Some(Box::new({
            let (seen, state, cache) = (seen.clone(), p.workspaces_dir().join(ID).join("workspace.json"), cache(&p, ID));
            move || {
                *seen.lock().unwrap() = Some((state.exists(), cache.exists()));
                Ok(())
            }
        })),
    );
    assert_eq!(forget_run(&p, &tmux, None, "app"), (0, "zygo: forgot app\n".to_string(), String::new()));
    up::testing::hook(&p.workspaces_dir().join(ID), "forget_before_cache", None);
    assert_eq!(*seen.lock().unwrap(), Some((false, true))); // the state is gone, the cache is still there
    assert!(!cache(&p, ID).exists() && !p.workspaces_dir().join(ID).exists());
}

#[test] // M2 review round 3 finding 4: a cache that cannot be removed after the state is gone is swept by the next `forget`
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
fn forget_reports_a_cache_left_after_the_state_and_the_next_forget_sweeps_it() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |_| {});
    result(&p, ID, "kaaaaaaaa");
    up::testing::hook(&p.workspaces_dir().join(ID), "forget_before_cache", Some(Box::new(|| Err("injected".into()))));
    assert_eq!(
        forget_run(&p, &tmux, None, "app"),
        (1, String::new(), format!("zygo: app forgotten, but {} was not removed: injected\n", cache(&p, ID).display()))
    );
    up::testing::hook(&p.workspaces_dir().join(ID), "forget_before_cache", None);
    assert!(!p.workspaces_dir().join(ID).exists() && cache(&p, ID).join("results/kaaaaaaaa/r1").exists());
    assert_eq!(forget_run(&p, &tmux, None, "app"), (1, String::new(), "zygo: no workspace named app\n".to_string()));
    assert!(!cache(&p, ID).exists()); // D26 sweep: a cache with no state folder
}

#[test] // M2 review round 3 finding 9: the second pass tries every cache, then reports the first error; a second run recovers
#[ignore = "timing: git under the fixed 5 s forget-pane git bound"]
fn forget_pane_tries_every_cache_and_a_second_run_recovers() {
    let _s = serial();
    let PaneSetup { td, p, tmux, r, id, store, .. } = pane_setup();
    let saved = td.path().join("saved/opencode");
    fs::create_dir_all(saved.parent().unwrap()).unwrap();
    fs::write(&saved, "bin").unwrap();
    let prog = program(&p, &id, "kbbbbbbbb", &saved);
    let results = cache(&p, &id).join("results/kbbbbbbbb");
    assert!(results.join("r1").exists());
    up::testing::hook(store.dir(), "forget_pane_remove_results", Some(Box::new(|| Err("injected".into()))));
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kbbbbbbbb"),
        (1, String::new(), "zygo: cannot forget pane kbbbbbbbb of app: injected\n".to_string())
    );
    up::testing::hook(store.dir(), "forget_pane_remove_results", None);
    assert!(!store.read().unwrap().unwrap().members.contains_key("kbbbbbbbb")); // the record went first
    assert!(fs::symlink_metadata(&prog).is_err()); // the program folder went after the failed results
    assert!(results.join("r1").exists());
    assert_eq!(
        pane_run(&p, &tmux, None, &r, "kbbbbbbbb"),
        (0, "zygo: forgot pane kbbbbbbbb of app\n".to_string(), String::new())
    );
    assert!(fs::symlink_metadata(&results).is_err());
}

#[test] // M2 review round 3 finding 10: each part that `pane_cache_create` opens or makes gets mode 0700 through its fd
fn pane_cache_create_sets_mode_0700_on_each_part() {
    use std::os::unix::fs::PermissionsExt;
    let _s = serial();
    let td = TempDir::new().unwrap();
    let p = paths(td.path());
    let mode = |q: &Path| fs::metadata(q).unwrap().permissions().mode() & 0o777;
    for sub in ["results", "program"] {
        let parts = [cache(&p, ID), cache(&p, ID).join(sub), cache(&p, ID).join(sub).join("kaaaaaaaa")];
        // Made here: 0700.
        assert_eq!(admin::pane_cache_create(&p, ID, sub, "kaaaaaaaa"), Ok(parts[2].clone()));
        for q in &parts {
            assert_eq!(mode(q), 0o700, "{}", q.display());
        }
        // Already there with mode 0777: 0700 again.
        for q in &parts {
            fs::set_permissions(q, fs::Permissions::from_mode(0o777)).unwrap();
        }
        assert_eq!(admin::pane_cache_create(&p, ID, sub, "kaaaaaaaa"), Ok(parts[2].clone()));
        for q in &parts {
            assert_eq!(mode(q), 0o700, "{}", q.display());
        }
        fs::remove_dir_all(cache(&p, ID)).unwrap();
    }
}

#[test] // M2 review round 2 finding 3: the create check of `results/<key>` and `program/<key>` refuses every link
fn pane_cache_create_refuses_a_link_at_each_part() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let outside = t.join("outside");
    fs::create_dir_all(&outside).unwrap();
    for sub in ["results", "program"] {
        let want = cache(&p, ID).join(sub).join("kaaaaaaaa");
        assert_eq!(admin::pane_cache_create(&p, ID, sub, "kaaaaaaaa"), Ok(want.clone())); // nothing there yet: made
        for at in [cache(&p, ID), cache(&p, ID).join(sub), want.clone()] {
            let _ = fs::remove_dir_all(cache(&p, ID));
            fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(&outside, &at).unwrap();
            assert_eq!(
                admin::pane_cache_create(&p, ID, sub, "kaaaaaaaa"),
                Err(format!("{} is a link; it is not followed", at.display())),
                "{sub}: {}",
                at.display()
            );
            fs::remove_file(&at).unwrap();
        }
        fs::create_dir_all(&want).unwrap();
        assert_eq!(admin::pane_cache_create(&p, ID, sub, "kaaaaaaaa"), Ok(want)); // an existing folder is used
        fs::remove_dir_all(cache(&p, ID)).unwrap();
    }
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0); // nothing made through a link
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
    // FIX-17 Round 2 F1 (AU29, AU84): the file may be from a newer zygo, so every writer refuses it before `op.lock`.
    let too_large = (1, String::new(), too_large_line(&file));
    assert_eq!(forget_run(&p, &tmux, None, ID), too_large); // by id
    assert!(!store.dir().join("op.lock").exists());
    // FIX-17 D7: the store reads at most 1 MiB + 1 byte (AU84), so the name in a larger file is never read and the
    // state is found by its id only (D14).
    let no_name = (1, String::new(), "zygo: no workspace named big\n".to_string());
    assert_eq!(forget_run(&p, &tmux, None, "big"), no_name); // by name
    assert_eq!(pane_run(&p, &tmux, Some("big"), &plain, "kaaaaaaaa"), no_name);
    assert_eq!(pane_run(&p, &tmux, Some(ID), &plain, "kaaaaaaaa"), too_large);
    // Readers: `zygo workspace` gives the reason; `ls` shows the row by its id as unreadable.
    let e = admin::status(&p, &plain, Some(OsStr::new(ID))).unwrap_err();
    assert_eq!(format!("zygo: {e}\n"), too_large.2);
    let rows = admin::list(&p, &tmux);
    let row = rows.iter().find(|r| r.id == ID).unwrap();
    assert_eq!((row.name.as_str(), row.state), (ID, ListState::Unreadable));
    assert!(!store.dir().join("op.lock").exists());
    assert_eq!(fs::read(&file).unwrap(), bytes);
}

/// The `TooLarge` line of FIX-17 Round 2 F1 for the state file `file`.
fn too_large_line(file: &Path) -> String {
    format!("zygo: {}: state file too large; it may be from a newer zygo, so zygo does not change it\n", file.display())
}

#[test] // FIX-17 Round 2 F2: the raw read of an invalid state is capped by the read itself; a file that grows after
        // the size check is not read past 1 MiB + 1 byte, and the state is not forgotten
fn raw_read_of_an_invalid_state_is_capped_when_the_file_grows() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    let id = "e100000000000001";
    let dir = p.workspaces_dir().join(id);
    // A socket whose folder is missing: no tmux check runs, so a full read of the grown file would forget the state.
    put_raw(&dir, &invalid_doc(id, json!("app"), Some(&t.join("gone/s"))));
    let file = dir.join("workspace.json");
    let mut grown = fs::read(&file).unwrap();
    grown.resize(2 * 1024 * 1024, b' ');
    up::testing::hook(
        &dir,
        "raw_json_before_read",
        Some(Box::new({
            let (file, grown) = (file.clone(), grown.clone());
            move || fs::write(&file, &grown).map_err(|e| e.to_string())
        })),
    );
    let r = forget_run(&p, &tmux, None, id);
    up::testing::hook(&dir, "raw_json_before_read", None);
    assert_eq!(r, (1, String::new(), too_large_line(&file)));
    assert_eq!(fs::read(&file).unwrap(), grown);
}

// ---- FIX-30: a tmux socket that fails AU54 ----

#[test] // FIX-30, AU54, W§12 `ls`: the row is unreachable and no tmux call runs
fn au54_ls_shows_a_quoted_saved_socket_as_unreachable_with_no_tmux_call() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let sock = quoted_socket(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |ws| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(sock.clone());
    });
    let (rows, calls, _) = fake_list(&p, vec![ListFake::new(&sock, SessionAnswer::Exists, Duration::ZERO)], false);
    assert_eq!(states(&rows), [("app".to_string(), ListState::Unreachable)]);
    assert!(calls.is_empty(), "{calls:?}");
}

#[test] // FIX-30, AU54, W§12 `ws forget`, D17: exit 2, the line names the state folder, nothing deleted; other errors exit 1
#[ignore = "timing: stand-in tmux version probe under the fixed 2 s tmux bound"]
fn au54_forget_refuses_a_quoted_saved_socket_and_names_the_state_folder() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    let sock = quoted_socket(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |ws| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(sock.clone());
    });
    let bytes = state_bytes(&p, ID);
    assert_eq!(forget_run(&p, &tmux, None, "app"), (2, String::new(), forget_au54_line(&p, ID)));
    assert_eq!(state_bytes(&p, ID), bytes);
    assert_eq!(tmux_log(t), ["-u -V"]); // only the `no_tmux` probe, no call on the socket
    let _ = fs::remove_file(t.join("tmux.log"));
    // the raw JSON socket of a state that cannot be read
    put_raw(&p.workspaces_dir().join(OTHER), &invalid_doc(OTHER, json!("bad"), Some(&sock)));
    assert_eq!(forget_run(&p, &tmux, None, OTHER), (2, String::new(), forget_au54_line(&p, OTHER)));
    assert!(p.workspaces_dir().join(OTHER).join("workspace.json").exists());
    assert_eq!(tmux_log(t), ["-u -V"]);
    // every other refusal still exits 1
    assert_eq!(forget_run(&p, &tmux, None, "nosuch"), (1, String::new(), "zygo: no workspace named nosuch\n".to_string()));
}

#[test] // FIX-30, AU54, W§12 `ws forget`: no saved socket; the `$TMUX` socket with a quote: exit 2, the AU54 line
#[ignore = "timing: stand-in tmux version probe under the fixed 2 s tmux bound"]
fn au54_forget_refuses_a_quoted_tmux_env_socket_with_exit_2() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |ws| ws.phase = Phase::Up);
    let env = format!("{},1,0", t.join("so'ck").join("s").display());
    let bytes = state_bytes(&p, ID);
    assert_eq!(forget_run(&p, &tmux, Some(env.as_str()), "app"), (2, String::new(), format!("{AU54}\n")));
    assert_eq!(state_bytes(&p, ID), bytes);
    assert_eq!(tmux_log(t), ["-u -V"]);
}

#[test] // FIX-30, AU54, W§12 `ws forget`: a quoted socket whose folder is gone gets no tmux call, so no check: forgotten
fn au54_forget_with_a_quoted_socket_whose_folder_is_gone_succeeds() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let tmux = answer_tmux(t);
    let gone = t.join("so'ck").join("s"); // `so'ck` is not made
    ws_state(&p, ID, "app", &t.join("r/app"), |ws| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(gone.clone());
    });
    assert_eq!(forget_run(&p, &tmux, None, "app"), (0, "zygo: forgot app\n".to_string(), String::new()));
    put_raw(&p.workspaces_dir().join(OTHER), &invalid_doc(OTHER, json!("bad"), Some(&gone)));
    assert_eq!(forget_run(&p, &tmux, None, OTHER), (0, format!("zygo: forgot {OTHER}\n"), String::new()));
    assert!(tmux_log(t).is_empty(), "{:?}", tmux_log(t));
}

#[test] // FIX-30, AU54, W§12 `ws forget-pane`, W§7.4 step 1: the record goes; the pending snapshot logs one line, no tmux call
fn au54_forget_pane_with_a_quoted_saved_socket_removes_the_record_and_the_snapshot_logs() {
    let _s = serial();
    let PaneSetup { td, p, tmux, id, store, plain, .. } = pane_setup();
    let t = td.path();
    let sock = quoted_socket(t);
    store
        .update(S5, &mut |ws| {
            ws.phase = Phase::Up;
            ws.tmux_socket = Some(sock.clone());
            Ok(())
        })
        .unwrap();
    fs::write(store.dir().join("snapshot.pending"), "").unwrap();
    assert_eq!(
        pane_run(&p, &tmux, Some("app"), &plain, "kbbbbbbbb"),
        (0, "zygo: forgot pane kbbbbbbbb of app\n".to_string(), String::new())
    );
    assert!(!store.read().unwrap().unwrap().members.contains_key("kbbbbbbbb"));
    assert!(!cache(&p, &id).join("results/kbbbbbbbb").exists());
    let log = fs::read_to_string(store.dir().join("errors.log")).unwrap_or_default();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].ends_with(&format!("snapshot: {}", AU54.trim_start_matches("zygo: "))), "{lines:?}");
    assert!(!store.dir().join("snapshot.pending").exists());
    assert!(tmux_log(t).is_empty(), "{:?}", tmux_log(t));
}

// ---- ML-8: `zygo ws name` ----

fn name_run(p: &Paths, tmux: &Path, ws: Option<&str>, cwd: &Path, key: &str, name: &str) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = admin::rename_member(p, tmux, ws, cwd, key, name, &mut out, &mut err);
    (code, text(out), text(err))
}

/// Workspace `app` (id `ID`, phase down): `kaaaaaaaa` named `old` with a pane, `kbbbbbbbb` named `bob` with no
/// pane, a shell pane `kcccccccc` with no record; record and pane of `kaaaaaaaa` carry an unknown field (AU78).
fn name_setup(t: &Path, id: &str, root: &Path) -> (Paths, PathBuf, Store) {
    let p = paths(t);
    let tmux = answer_tmux(t);
    let store = ws_state(&p, id, "app", root, |ws| {
        let mut a = member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1"));
        a.name = Some("old".into());
        a.extra.insert("future".into(), json!(1));
        let mut b = member(Agent::ClaudeCode, MemberState::Resumable, Some("c-2"));
        b.name = Some("bob".into());
        ws.members.insert("kaaaaaaaa".into(), a);
        ws.members.insert("kbbbbbbbb".into(), b);
        let mut pa = pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, root);
        pa.name = Some("old".into());
        pa.extra.insert("future".into(), json!(2));
        ws.panes = vec![pa, pane("kcccccccc", 0, 1, PaneKind::Shell, root)];
    });
    (p, tmux, store)
}

#[test] // M§6: the record and the panes[] entry in one write; mail stays in mail/<key>/; W§7.3 pending check; AU78
fn rename_member_changes_the_record_and_the_pane_in_one_write() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let (p, tmux, store) = name_setup(td.path(), ID, &td.path().join("r/app"));
    let inbox = mail_dir(&p, ID).join("kaaaaaaaa/new");
    fs::create_dir_all(&inbox).unwrap();
    fs::write(inbox.join("m1.json"), "{}").unwrap();
    fs::write(store.dir().join("snapshot.pending"), "").unwrap();
    assert_eq!(
        name_run(&p, &tmux, Some("app"), Path::new(""), "kaaaaaaaa", "elena"),
        (0, "zygo: pane kaaaaaaaa of app is now named elena\n".to_string(), String::new())
    );
    let ws = store.read().unwrap().unwrap();
    let a = &ws.members["kaaaaaaaa"];
    assert_eq!((a.name.as_deref(), a.extra.get("future")), (Some("elena"), Some(&json!(1))));
    assert_eq!((ws.panes[0].name.as_deref(), ws.panes[0].extra.get("future")), (Some("elena"), Some(&json!(2))));
    assert_eq!(ws.members["kbbbbbbbb"].name.as_deref(), Some("bob"));
    assert!(inbox.join("m1.json").exists());
    assert!(!mail_dir(&p, ID).join("elena").exists());
    assert!(!store.dir().join("snapshot.pending").exists()); // W§7.3 check ran
    assert_eq!(
        name_run(&p, &tmux, Some("app"), Path::new(""), "kbbbbbbbb", "ops"),
        (0, "zygo: pane kbbbbbbbb of app is now named ops\n".to_string(), String::new())
    ); // a member with no panes[] entry: the record only
    let ws = store.read().unwrap().unwrap();
    assert_eq!(ws.members["kbbbbbbbb"].name.as_deref(), Some("ops"));
    assert_eq!((ws.panes.len(), ws.panes[1].name.as_deref()), (2, None));
}

#[test] // W§7.3, graph review focus 57: the write waits for `lock`
#[ignore = "timing: waits on a held state lock for 300 ms"]
fn rename_member_waits_for_the_state_lock() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let (p, tmux, store) = name_setup(td.path(), ID, &td.path().join("r/app"));
    let held = hold(store.dir().join("lock"));
    let t0 = Instant::now();
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        drop(held);
    });
    assert_eq!(name_run(&p, &tmux, Some("app"), Path::new(""), "kaaaaaaaa", "elena").0, 0);
    assert!(t0.elapsed() >= Duration::from_millis(300));
    h.join().unwrap();
    assert_eq!(store.read().unwrap().unwrap().members["kaaaaaaaa"].name.as_deref(), Some("elena"));
}

#[test] // M§6, AU84, graph review focus 56: bad key, bad or reserved name, a held name: exit 1, one line, no change
fn rename_member_refuses_bad_values_and_changes_nothing() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let (p, tmux, _store) = name_setup(td.path(), ID, &td.path().join("r/app"));
    let before = state_bytes(&p, ID);
    let bad = |n: &str| format!("zygo: bad name `{n}`: use ^[a-z][a-z0-9-]{{0,31}}$, not all, cli, zygo or a pane key\n");
    let long = "a".repeat(33);
    for (key, name, line) in [
        ("k$", "elena", "zygo: invalid pane key k$\n".to_string()),
        ("kaaaaaaaa", "Elena", bad("Elena")),
        ("kaaaaaaaa", "", bad("")),
        ("kaaaaaaaa", "9lives", bad("9lives")),
        ("kaaaaaaaa", long.as_str(), bad(&long)),
        ("kaaaaaaaa", "a\u{1b}", bad("a\\u{1b}")),
        ("kaaaaaaaa", "all", bad("all")),
        ("kaaaaaaaa", "cli", bad("cli")),
        ("kaaaaaaaa", "zygo", bad("zygo")),
        ("kaaaaaaaa", "kbbbbbbbb", bad("kbbbbbbbb")),
        ("kaaaaaaaa", "bob", "zygo: name bob belongs to pane kbbbbbbbb; nothing changed\n".to_string()),
    ] {
        assert_eq!(name_run(&p, &tmux, Some("app"), Path::new(""), key, name), (1, String::new(), line), "{key} {name:?}");
        assert_eq!(state_bytes(&p, ID), before, "{key} {name:?}");
    }
}

#[test] // M§6: a key with no member record, a panes[] entry with no record, an unknown workspace: exit 1, no change
fn rename_member_unknown_key_or_workspace_exits_1() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let (p, tmux, _store) = name_setup(td.path(), ID, &td.path().join("r/app"));
    let before = state_bytes(&p, ID);
    for (ws, key, line) in [
        ("app", "kzzzzzzzz", "zygo: app has no member record for pane kzzzzzzzz\n"),
        ("app", "kcccccccc", "zygo: app has no member record for pane kcccccccc\n"),
        ("nosuch", "kaaaaaaaa", "zygo: no workspace named nosuch\n"),
    ] {
        assert_eq!(name_run(&p, &tmux, Some(ws), Path::new(""), key, "elena"), (1, String::new(), line.to_string()), "{ws} {key}");
        assert_eq!(state_bytes(&p, ID), before);
    }
}

#[test] // M§6, W§12: no --workspace: the workspace of the checkout of cwd
#[ignore = "timing: git under the fixed 5 s forget-pane git bound"]
fn rename_member_by_the_checkout_folder() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let r = canon(&repo(t, "app"));
    let id = workspace_id(&r.join(".git"));
    let (p, tmux, store) = name_setup(t, &id, &r);
    assert_eq!(
        name_run(&p, &tmux, None, &r, "kaaaaaaaa", "elena"),
        (0, "zygo: pane kaaaaaaaa of app is now named elena\n".to_string(), String::new())
    );
    assert_eq!(store.read().unwrap().unwrap().members["kaaaaaaaa"].name.as_deref(), Some("elena"));
    let plain = t.join("plain");
    fs::create_dir_all(&plain).unwrap();
    assert_eq!(
        name_run(&p, &tmux, None, &plain, "kaaaaaaaa", "ops"),
        (1, String::new(), "zygo: no workspace for this folder; run in the workspace's checkout or pass --workspace\n".to_string())
    );
}

#[test] // ML-8 review round 1, F6: a shell pane has no name; the record of its key is renamed, the pane is not
fn rename_member_leaves_a_shell_pane_unnamed() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let (p, tmux, store) = name_setup(td.path(), ID, &td.path().join("r/app"));
    store
        .update(S5, &mut |ws| {
            ws.members.insert("kcccccccc".into(), member(Agent::ClaudeCode, MemberState::Ended, None));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        name_run(&p, &tmux, Some("app"), Path::new(""), "kcccccccc", "ops"),
        (0, "zygo: pane kcccccccc of app is now named ops\n".to_string(), String::new())
    );
    let ws = store.read().unwrap().unwrap();
    assert_eq!(ws.members["kcccccccc"].name.as_deref(), Some("ops"));
    assert_eq!((ws.panes[1].key.as_str(), ws.panes[1].kind, ws.panes[1].name.as_deref()), ("kcccccccc", PaneKind::Shell, None));
}

#[test] // ML-8 review round 1, F3 (AU84): a new state over 1 MiB is `StateError::Invalid`: exit 1, one line, no change
fn rename_member_reports_an_invalid_state() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let (p, tmux, store) = name_setup(td.path(), ID, &td.path().join("r/app"));
    let max = 1024 * 1024;
    let pad = |n: usize| {
        store
            .update(S5, &mut |ws| {
                ws.members.get_mut("kaaaaaaaa").unwrap().extra.insert("pad".into(), json!("x".repeat(n)));
                Ok(())
            })
            .unwrap();
        state_bytes(&p, ID).len()
    };
    // The state is 20 bytes under the limit; `old` to a 32-byte name adds 29 bytes to the record and to its pane.
    let size = pad(1000);
    assert_eq!(pad(1000 + max - 20 - size), max - 20);
    let before = state_bytes(&p, ID);
    let long = "e".repeat(32);
    let file = store.dir().join("workspace.json");
    assert_eq!(
        name_run(&p, &tmux, Some("app"), Path::new(""), "kaaaaaaaa", &long),
        (
            1,
            String::new(),
            format!(
                "zygo: {}: the new state is {} bytes, more than the {max} bytes that every reader accepts (AU84); not written\n",
                file.display(),
                max - 20 + 58
            )
        )
    );
    assert_eq!(state_bytes(&p, ID), before);
}

// ---- ML-14: unread counts and maildir removal (M§5, M§9) ----

#[test] // M§9: `zygo workspace` shows the unread count per member
fn mail_status_shows_the_unread_count_per_member() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let plain = t.join("plain");
    fs::create_dir_all(&plain).unwrap();
    ws_state(&p, ID, "app", &t.join("r/app"), |ws| {
        ws.phase = Phase::Up;
        ws.members = [
            ("kaaaaaaaa", member(Agent::ClaudeCode, MemberState::Running, Some("c-1"))),
            ("kbbbbbbbb", member(Agent::OpenCode, MemberState::Resumable, None)),
            ("kcccccccc", member(Agent::ClaudeCode, MemberState::Ended, Some("c-3"))),
        ]
        .into_iter()
        .map(|(k, m)| (k.to_string(), m))
        .collect();
    });
    mail_to(&p, ID, "kaaaaaaaa", 2);
    mail_to(&p, ID, "kzzzzzzzz", 1); // no member record: not shown
    fs::write(mail_dir(&p, ID).join("kcccccccc"), "").unwrap(); // not a folder: the count cannot be read
    assert_eq!(
        admin::status(&p, &plain, Some(OsStr::new(ID))).unwrap(),
        "workspace app: up\nkaaaaaaaa claude-code running c-1 unread 2\nkbbbbbbbb opencode resumable -\nkcccccccc claude-code ended c-3 unread -"
    );
}

#[test] // M§9: each `ls` row shows the workspace total of unread messages
fn mail_list_rows_show_the_workspace_unread_total() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let two = |ws: &mut Workspace| {
        ws.members.insert("kaaaaaaaa".into(), member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1")));
        ws.members.insert("kbbbbbbbb".into(), member(Agent::OpenCode, MemberState::Resumable, Some("o-2")));
    };
    ws_state(&p, ID, "app", &t.join("r/app"), two);
    ws_state(&p, OTHER, "other", &t.join("r/other"), two);
    ws_state(&p, "2000000000000002", "third", &t.join("r/third"), two);
    raw_invalid_name(&p);
    mail_to(&p, ID, "kaaaaaaaa", 2);
    mail_to(&p, ID, "kbbbbbbbb", 3);
    mail_to(&p, ID, "kzzzzzzzz", 4); // no member record: not counted
    mail_to(&p, "2000000000000002", "kaaaaaaaa", 1);
    fs::write(mail_dir(&p, "2000000000000002").join("kbbbbbbbb"), "").unwrap(); // one count cannot be read
    let (rows, calls, _) = fake_list(&p, vec![], true);
    assert!(calls.is_empty(), "{calls:?}");
    assert_eq!(
        rows.iter().map(|r| (r.name.as_str(), r.unread)).collect::<Vec<_>>(),
        [("app", Some(5)), ("e100000000000001", None), ("other", Some(0)), ("third", None)]
    );
    assert!(rows[0].to_string().ends_with("  clients -  unread 5"), "{}", rows[0]);
    assert!(rows[1].to_string().ends_with("  clients -  unread -"), "{}", rows[1]);
    assert!(rows[2].to_string().ends_with("  clients -  unread 0"), "{}", rows[2]);
}

#[test] // M§5: `zygo ws forget` deletes mail/
fn mail_forget_leaves_no_mail_folder() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let one = |ws: &mut Workspace| {
        ws.members.insert("kaaaaaaaa".into(), member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1")));
    };
    ws_state(&p, ID, "app", &t.join("r/app"), one);
    ws_state(&p, OTHER, "other", &t.join("r/other"), one);
    mail_to(&p, ID, "kaaaaaaaa", 2);
    mail_to(&p, OTHER, "kaaaaaaaa", 1);
    assert_eq!(forget_run(&p, &t.join("none"), None, "app"), (0, "zygo: forgot app\n".to_string(), String::new()));
    assert!(fs::symlink_metadata(mail_dir(&p, ID)).is_err());
    assert!(fs::symlink_metadata(p.workspaces_dir().join(ID)).is_err());
    assert_eq!(mail_store::unread(&mail_dir(&p, OTHER), "kaaaaaaaa").unwrap(), 1);
}

// ---- MS-21: the before_remove step (mail sync design S§7.6, S§14) ----

#[test] // S§7.6 steps 1-4, S§14: before_remove runs after the checks, under the sync lock, before any delete
fn forget_runs_before_remove_under_the_sync_lock_before_any_delete() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |_| {});
    result(&p, ID, "kaaaaaaaa");
    let lock = sync_dir(&p, ID).join("lock");
    let state = p.workspaces_dir().join(ID).join("workspace.json");
    let mut seen = Vec::new();
    let mut before = |ws: &Workspace| {
        seen.push((ws.id.clone(), ws.main_root.clone(), state.exists(), cache(&p, ID).exists(), lock_free(&lock)));
        Ok(true)
    };
    assert_eq!(forget_with(&p, &t.join("none"), None, "app", &mut before), (0, "zygo: forgot app\n".to_string(), String::new()));
    assert_eq!(seen, vec![(ID.to_string(), t.join("r/app"), true, true, false)]);
    assert!(fs::symlink_metadata(p.workspaces_dir().join(ID)).is_err() && !cache(&p, ID).exists());
}

#[test] // S§7.6 step 2: a "no" keeps everything, exit 0; only the sync lock that forget made stays
fn forget_keeps_everything_when_before_remove_says_no() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |_| {});
    result(&p, ID, "kaaaaaaaa");
    mail_to(&p, ID, "kaaaaaaaa", 1);
    let bytes = state_bytes(&p, ID);
    assert_eq!(
        forget_with(&p, &t.join("none"), None, "app", &mut |_: &Workspace| Ok(false)),
        (0, "zygo: app not forgotten\n".to_string(), String::new())
    );
    assert_eq!(state_bytes(&p, ID), bytes);
    assert!(cache(&p, ID).join("results/kaaaaaaaa/r1").exists());
    assert_eq!(mail_store::unread(&mail_dir(&p, ID), "kaaaaaaaa").unwrap(), 1);
    let sync = mail_dir(&p, ID).join("sync");
    assert_eq!(common_names(&sync), vec!["lock".to_string()]);
}

/// The names in folder `dir`, sorted.
fn common_names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    v.sort();
    v
}

/// The permission bits of `path`, not following a link.
fn bits(path: &Path) -> u32 { fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777 }

#[test] // S§7.6 step 1 (MS-21 review F1): with no mail/sync/, forget makes mail/ and mail/sync/ (0700) and the lock
        // (0600) and holds the lock during before_remove, so a first sync waits for it
fn forget_with_no_sync_folder_makes_and_holds_the_sync_lock() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |_| {});
    let mail = mail_dir(&p, ID);
    assert!(fs::symlink_metadata(&mail).is_err());
    let lock = mail.join("sync/lock");
    let mut seen = Vec::new();
    let mut before = |_: &Workspace| {
        let held = fs::symlink_metadata(&lock).is_ok_and(|m| m.is_file()) && !lock_free(&lock);
        seen.push((held, bits(&mail), bits(&mail.join("sync")), bits(&lock)));
        Ok(true)
    };
    assert_eq!(forget_with(&p, &t.join("none"), None, "app", &mut before), (0, "zygo: forgot app\n".to_string(), String::new()));
    assert_eq!(seen, vec![(true, 0o700, 0o700, 0o600)]);
    assert!(fs::symlink_metadata(p.workspaces_dir().join(ID)).is_err());
}

#[test] // S§7.6 step 1 (MS-21 review F3): a mail/ that is a link or not a folder is not followed: no mail, no lock
fn forget_does_not_follow_a_mail_link() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let elsewhere = t.join("elsewhere");
    fs::create_dir_all(elsewhere.join("sync")).unwrap();
    ws_state(&p, ID, "app", &t.join("r/app"), |_| {});
    std::os::unix::fs::symlink(&elsewhere, mail_dir(&p, ID)).unwrap();
    ws_state(&p, OTHER, "bee", &t.join("r/bee"), |_| {});
    fs::write(mail_dir(&p, OTHER), b"x").unwrap();
    for name in ["app", "bee"] {
        assert_eq!(forget_run(&p, &t.join("none"), None, name), (0, format!("zygo: forgot {name}\n"), String::new()));
    }
    assert_eq!(common_names(&elsewhere), vec!["sync".to_string()]);
    assert!(common_names(&elsewhere.join("sync")).is_empty());
}

#[test] // S§7.6 steps 2-3: an Err of before_remove refuses with exit 1 and its reason; nothing deleted
fn forget_refuses_with_the_reason_of_before_remove() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |_| {});
    result(&p, ID, "kaaaaaaaa");
    let bytes = state_bytes(&p, ID);
    assert_eq!(
        forget_with(&p, &t.join("none"), None, "app", &mut |_: &Workspace| Err("the outbox cannot be read: x".to_string())),
        (1, String::new(), "zygo: cannot forget app: the outbox cannot be read: x\n".to_string())
    );
    assert_eq!(state_bytes(&p, ID), bytes);
    assert!(cache(&p, ID).join("results/kaaaaaaaa/r1").exists());
}

#[test] // S§7.6 step 1: a sync that holds mail/sync/lock refuses at once; before_remove does not run
fn forget_refuses_while_a_mail_sync_holds_its_lock() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    ws_state(&p, ID, "app", &t.join("r/app"), |_| {});
    let held = hold(sync_dir(&p, ID).join("lock"));
    let bytes = state_bytes(&p, ID);
    let mut calls = 0;
    assert_eq!(
        forget_with(&p, &t.join("none"), None, "app", &mut |_: &Workspace| {
            calls += 1;
            Ok(true)
        }),
        (1, String::new(), "zygo: a mail sync runs for app; try again\n".to_string())
    );
    assert_eq!((calls, state_bytes(&p, ID)), (0, bytes));
    drop(held);
    assert_eq!(forget_run(&p, &t.join("none"), None, "app"), (0, "zygo: forgot app\n".to_string(), String::new()));
}

#[test] // S§7.6 last paragraph: a state that cannot be read gets before_remove with its raw absolute main_root
fn forget_of_an_invalid_state_passes_its_raw_main_root() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let p = paths(t);
    let gone = t.join("gone/s");
    put_raw(&p.workspaces_dir().join("e100000000000001"), &invalid_doc("e100000000000001", json!("app"), Some(&gone)));
    let mut rel = invalid_doc("e200000000000002", json!("bee"), Some(&gone));
    rel["main_root"] = json!("x/rel");
    put_raw(&p.workspaces_dir().join("e200000000000002"), &rel);
    let mut seen = Vec::new();
    for id in ["e100000000000001", "e200000000000002"] {
        let mut before = |ws: &Workspace| {
            seen.push((ws.id.clone(), ws.main_root.clone()));
            Ok(true)
        };
        assert_eq!(forget_with(&p, &t.join("none"), None, id, &mut before), (0, format!("zygo: forgot {id}\n"), String::new()));
    }
    assert_eq!(
        seen,
        vec![("e100000000000001".to_string(), PathBuf::from("/x")), ("e200000000000002".to_string(), PathBuf::new())]
    );
}

#[test] // M§5, W§5, AU84: a key that fails ^k[a-z2-7]{8}$ exits 1 before any path is built
fn mail_forget_pane_refuses_a_key_of_bad_form_and_deletes_nothing() {
    let _s = serial();
    let PaneSetup { td, p, tmux, id, store, plain, .. } = pane_setup();
    let m = mail_dir(&p, &id);
    mail_to(&p, &id, "kaaaaaaaa", 1);
    fs::create_dir_all(m.join("k/aaaaaaa")).unwrap();
    fs::write(m.join("k/aaaaaaa/keep"), "k").unwrap();
    fs::create_dir_all(m.join("kAAAAAAAA/new")).unwrap();
    fs::create_dir_all(store.dir().join("x")).unwrap(); // what mail/../x names
    fs::write(store.dir().join("x/keep"), "k").unwrap();
    let before = snapshot_tree(td.path());
    for bad in ["../x", "k/aaaaaaa", "kAAAAAAAA"] {
        assert_eq!(pane_run(&p, &tmux, Some("app"), &plain, bad), (1, String::new(), format!("zygo: invalid pane key {bad}\n")));
    }
    assert_eq!(snapshot_tree(td.path()), before);
}

#[test] // M§5: `zygo ws forget-pane <key>` deletes mail/<key>/ and keeps the other maildirs
fn mail_forget_pane_removes_only_that_maildir() {
    let _s = serial();
    let PaneSetup { td: _td, p, tmux, id, store, plain, .. } = pane_setup();
    let m = mail_dir(&p, &id);
    mail_to(&p, &id, "kaaaaaaaa", 1);
    mail_to(&p, &id, "kbbbbbbbb", 2);
    fs::create_dir_all(m.join("sync")).unwrap();
    assert_eq!(
        pane_run(&p, &tmux, Some("app"), &plain, "kbbbbbbbb"),
        (0, "zygo: forgot pane kbbbbbbbb of app\n".to_string(), String::new())
    );
    assert!(fs::symlink_metadata(m.join("kbbbbbbbb")).is_err());
    assert!(!store.read().unwrap().unwrap().members.contains_key("kbbbbbbbb"));
    assert_eq!(mail_store::unread(&m, "kaaaaaaaa").unwrap(), 1);
    assert!(m.join("sync").is_dir());
    // A maildir with no member record and no cache is removed too (a second run after a failed removal).
    mail_to(&p, &id, "kyyyyyyyy", 1);
    assert_eq!(
        pane_run(&p, &tmux, Some("app"), &plain, "kyyyyyyyy"),
        (0, "zygo: forgot pane kyyyyyyyy of app\n".to_string(), String::new())
    );
    assert!(fs::symlink_metadata(m.join("kyyyyyyyy")).is_err());
    assert_eq!(mail_store::unread(&m, "kaaaaaaaa").unwrap(), 1);
}

#[test] // M§5: mail/<key> that is a link is removed as a link; the folder it points to is kept
fn mail_forget_pane_removes_a_linked_maildir_as_a_link() {
    let _s = serial();
    let PaneSetup { td, p, tmux, id, plain, .. } = pane_setup();
    let outside = td.path().join("outside");
    fs::create_dir_all(outside.join("new")).unwrap();
    fs::write(outside.join("new/keep.json"), "k").unwrap();
    fs::create_dir_all(mail_dir(&p, &id)).unwrap();
    std::os::unix::fs::symlink(&outside, mail_dir(&p, &id).join("kbbbbbbbb")).unwrap();
    let before = snapshot_tree(&outside);
    assert_eq!(
        pane_run(&p, &tmux, Some("app"), &plain, "kbbbbbbbb"),
        (0, "zygo: forgot pane kbbbbbbbb of app\n".to_string(), String::new())
    );
    assert!(fs::symlink_metadata(mail_dir(&p, &id).join("kbbbbbbbb")).is_err());
    assert_eq!(snapshot_tree(&outside), before);
}

#[test] // M§5, Round 2 R4 pattern: a link at mail/ is not followed; nothing is written or deleted
fn mail_forget_pane_refuses_a_linked_mail_folder() {
    let _s = serial();
    let PaneSetup { td, p, tmux, id, store, plain, .. } = pane_setup();
    let outside = td.path().join("outside");
    fs::create_dir_all(outside.join("kbbbbbbbb/new")).unwrap();
    fs::write(outside.join("kbbbbbbbb/new/keep.json"), "k").unwrap();
    std::os::unix::fs::symlink(&outside, mail_dir(&p, &id)).unwrap();
    let before = snapshot_tree(&outside);
    assert_eq!(
        pane_run(&p, &tmux, Some("app"), &plain, "kbbbbbbbb"),
        (1, String::new(), format!("zygo: cannot forget pane kbbbbbbbb of app: {} is a link; it is not followed\n", mail_dir(&p, &id).display()))
    );
    assert_eq!(snapshot_tree(&outside), before);
    assert!(store.read().unwrap().unwrap().members.contains_key("kbbbbbbbb")); // nothing written
    assert!(cache(&p, &id).join("results/kbbbbbbbb/r1").exists()); // nothing deleted
}

#[test] // M§5: a regular file at mail/<key> is removed as a file; the other maildirs are kept
fn mail_forget_pane_removes_a_file_at_the_maildir_path() {
    let _s = serial();
    let PaneSetup { td: _td, p, tmux, id, plain, .. } = pane_setup();
    let m = mail_dir(&p, &id);
    mail_to(&p, &id, "kaaaaaaaa", 1);
    fs::write(m.join("kbbbbbbbb"), "not a maildir").unwrap();
    assert_eq!(
        pane_run(&p, &tmux, Some("app"), &plain, "kbbbbbbbb"),
        (0, "zygo: forgot pane kbbbbbbbb of app\n".to_string(), String::new())
    );
    assert!(fs::symlink_metadata(m.join("kbbbbbbbb")).is_err());
    assert_eq!(mail_store::unread(&m, "kaaaaaaaa").unwrap(), 1);
}

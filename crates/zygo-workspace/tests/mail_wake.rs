#![cfg(unix)]
//! ML-6: the wake of an idle member (M§7.3, M§9, M§10, M§13 #5 #6).
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tempfile::TempDir;
use zygo_config::{parse_duration, MailConfig};
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_workspace::mail::wake::{after_send, after_send_with, wake, Outcome, Probe, RealProbe, WAKE_TEXT};
use zygo_workspace::mail::{mail_dir, store as mail_store, Message};
use zygo_workspace::mux::testing::{require_tmux, PrivateServer};
use zygo_workspace::mux::{
    HookScope, Multiplexer, MuxError, NewSession, NewWindow, SessionAnswer, Split,
};
use zygo_workspace::state::{MemberRecord, Pending, Phase, Store, Workspace};

const ID: &str = "0123456789abcdef";
const ELENA: &str = "kelena222";
const OTHER: &str = "kother222";
/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;
const GAP: Duration = Duration::from_secs(30);

fn at(s: u64) -> SystemTime { UNIX_EPOCH + Duration::from_secs(s) }
fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}
fn hold(path: PathBuf) -> fs::File {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let f = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).unwrap();
    fs4::FileExt::try_lock(&f).unwrap();
    f
}
/// A live session `<sessions_dir>/<id>`: `token` and a held `pid` lock, which lasts as long as the returned file.
fn live(p: &Paths, id: &str) -> fs::File {
    let dir = p.sessions_dir.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("token"), "t").unwrap();
    hold(dir.join("pid"))
}
/// Polls `f` every 10 ms for at most 5 s.
fn until(f: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    f()
}
/// Identity fields as tests/mail_names.rs; one member ELENA, name `elena`, session `s1`.
fn ws() -> Workspace {
    let mut ws = Workspace::new(ID);
    ws.name = "app".into(); ws.common_dir = "/srv/app/.git".into(); ws.main_root = "/srv/app".into();
    ws.tmux_session = "zygo-app".into(); ws.loadout = "solo".into(); ws.phase = Phase::Up;
    let mut m = MemberRecord::new(Agent::ClaudeCode);
    m.name = Some("elena".into());
    m.zygo_session = Some("s1".into());
    ws.members.insert(ELENA.into(), m);
    ws
}
/// One test at a time: the tmux test forks, and a child that has not yet run `exec` keeps an inherited `flock` (the
/// `wake.lock` of another test's wake, or a `pid` lock) held, so a parallel test could wait or see `NotLive`.
static FORKS: Mutex<()> = Mutex::new(());
struct Setup { _t: TempDir, p: Paths, s: Store, w: Workspace, guard: Option<fs::File>, _g: MutexGuard<'static, ()> }
/// A temporary folder, a live `s1`, and one message to ELENA in `new/`.
fn setup() -> Setup {
    let g = FORKS.lock().unwrap_or_else(|e| e.into_inner());
    let t = TempDir::new().unwrap();
    let p = paths(t.path());
    let s = Store::open(&p, ID);
    let guard = Some(live(&p, "s1"));
    let m = Message::new("lead", "elena", "subj-7f3a", "body-9c1e", at(T0)).unwrap();
    mail_store::deliver(&mail_dir(&p, ID), ELENA, "elena", &m, 500).unwrap();
    Setup { _t: t, p, s, w: ws(), guard, _g: g }
}

struct At { idle: bool, now: SystemTime }
impl Probe for At {
    fn idle(&self, _session_dir: &Path) -> bool { self.idle }
    fn now(&self) -> SystemTime { self.now }
}
fn idle_at(s: u64) -> At { At { idle: true, now: at(s) } }

fn row(id: &str, key: &str) -> String { format!("0|w|1|l|80|24|1|0|{id}|0|1|{key}|/srv/app") }

struct Fake {
    ws_opt: Result<Option<String>, MuxError>,
    panes: Result<Vec<String>, MuxError>,
    pane_opt: Result<Option<String>, MuxError>,
    keys: Result<(), MuxError>,
    enter: Result<(), MuxError>,
    /// When true, `show_pane_option` gives the key of the `panes` row with that pane id, not `pane_opt`.
    pane_keys: bool,
    calls: Mutex<Vec<String>>,
    /// Each `send_enter` pane with its time.
    enters: Mutex<Vec<(String, Instant)>>,
    on_send: Option<Box<dyn Fn() + Send + Sync>>,
}
impl Fake {
    fn good() -> Fake {
        Fake {
            ws_opt: Ok(Some(ID.into())),
            panes: Ok(vec![row("%3", ELENA), row("%4", OTHER)]),
            pane_opt: Ok(Some(ELENA.into())),
            keys: Ok(()),
            enter: Ok(()),
            pane_keys: false,
            calls: Mutex::new(Vec::new()),
            enters: Mutex::new(Vec::new()),
            on_send: None,
        }
    }
    fn log(&self, s: String) { self.calls.lock().unwrap().push(s) }
    fn calls(&self) -> Vec<String> { self.calls.lock().unwrap().clone() }
    fn sends(&self) -> usize { self.calls().iter().filter(|c| c.starts_with("send_")).count() }
}
impl Multiplexer for Fake {
    fn socket(&self) -> Option<&Path> { panic!("not used by wake") }
    fn version(&self) -> Result<(u32, u32), MuxError> { panic!("not used by wake") }
    fn socket_path(&self, _: &str) -> Result<PathBuf, MuxError> { panic!("not used by wake") }
    fn session_exists(&self, _: &str) -> SessionAnswer { panic!("not used by wake") }
    fn new_session(&self, _: &NewSession<'_>) -> Result<u32, MuxError> { panic!("not used by wake") }
    fn resize_window(&self, _: &str, _: u32, _: u32, _: u32) -> Result<(), MuxError> { panic!("not used by wake") }
    fn unset_window_option(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn set_window_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn set_env(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn hide_env(&self, _: &str, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn set_session_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn show_session_option(&self, session: &str, name: &str) -> Result<Option<String>, MuxError> {
        self.log(format!("show_session_option {session} {name}"));
        self.ws_opt.clone()
    }
    fn set_hook(&self, _: &str, _: HookScope, _: &str, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn new_window(&self, _: &NewWindow<'_>) -> Result<u32, MuxError> { panic!("not used by wake") }
    fn rename_window(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn split_window(&self, _: &Split<'_>) -> Result<String, MuxError> { panic!("not used by wake") }
    fn select_layout(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn select_window(&self, _: &str, _: u32) -> Result<(), MuxError> { panic!("not used by wake") }
    fn select_pane(&self, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn zoom(&self, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn list_panes(&self, session: &str) -> Result<Vec<String>, MuxError> {
        self.log(format!("list_panes {session}"));
        self.panes.clone()
    }
    fn show_pane_option(&self, pane: &str, name: &str) -> Result<Option<String>, MuxError> {
        self.log(format!("show_pane_option {pane} {name}"));
        if self.pane_keys {
            let rows = self.panes.clone()?;
            let f = |r: &String| r.split('|').map(String::from).collect::<Vec<_>>();
            return Ok(rows.iter().map(f).find(|c| c[8] == pane).map(|c| c[11].clone()));
        }
        self.pane_opt.clone()
    }
    fn set_pane_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn display(&self, _: &str, _: &str) -> Result<String, MuxError> { panic!("not used by wake") }
    fn kill_session(&self, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn attach(&self, _: &str) -> Result<i32, MuxError> { panic!("not used by wake") }
    fn switch_client(&self, _: &str) -> Result<(), MuxError> { panic!("not used by wake") }
    fn send_keys_literal(&self, pane: &str, text: &str) -> Result<(), MuxError> {
        if let Some(f) = &self.on_send {
            f();
        }
        self.log(format!("send_keys_literal {pane} {text}"));
        self.keys.clone()
    }
    fn send_enter(&self, pane: &str) -> Result<(), MuxError> {
        self.log(format!("send_enter {pane}"));
        self.enters.lock().unwrap().push((pane.to_string(), Instant::now()));
        self.enter.clone()
    }
}

/// The lines of `errors.log`; empty when it is missing.
fn errors(s: &Store) -> Vec<String> {
    fs::read_to_string(s.dir().join("errors.log")).unwrap_or_default().lines().map(String::from).collect()
}
fn wake_path(s: &Store) -> PathBuf { s.dir().join("mail").join(ELENA).join("wake.json") }
/// `mail/ELENA/wake.json` as text.
fn wake_json(s: &Store) -> Option<String> { fs::read_to_string(wake_path(s)).ok() }
fn new_count(p: &Paths) -> usize { mail_store::unread(&mail_dir(p, ID), ELENA).unwrap() }

#[test]
fn wake_text_is_fixed() {
    assert_eq!(WAKE_TEXT, "zygo: you have mail; read it with mail_read");
}

#[test]
fn idle_live_member_gets_the_text_then_enter() {
    let x = setup();
    let f = Fake::good();
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Woken);
    assert_eq!(
        f.calls(),
        [
            "show_session_option zygo-app @zygo-ws",
            "list_panes zygo-app",
            "show_pane_option %3 @zygo-pane",
            "send_keys_literal %3 zygo: you have mail; read it with mail_read",
            "send_enter %3",
        ]
    );
    assert_eq!(wake_json(&x.s).as_deref(), Some(r#"{"woken_at":"2023-11-14T22:13:20Z"}"#));
    assert_eq!(fs::metadata(wake_path(&x.s)).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(new_count(&x.p), 1);
    assert!(errors(&x.s).is_empty());
}

#[test]
fn busy_member_gets_no_call() {
    let x = setup();
    let f = Fake::good();
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &At { idle: false, now: at(T0) }), Outcome::Busy);
    let mut w = x.w.clone();
    w.members.get_mut(ELENA).unwrap().launch.events_off = true;
    assert_eq!(wake(&x.s, &w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Busy);
    assert!(f.calls().is_empty());
    assert_eq!(wake_json(&x.s), None);
    assert!(errors(&x.s).is_empty());
}

#[test]
fn member_whose_launcher_is_not_live_gets_no_call() {
    let mut x = setup();
    let f = Fake::good();
    x.guard = None;
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::NotLive);
    let _g = live(&x.p, "s1");
    let mut w = x.w.clone();
    w.members.get_mut(ELENA).unwrap().zygo_session = None;
    assert_eq!(wake(&x.s, &w, &f, ELENA, GAP, &idle_at(T0)), Outcome::NotLive);
    assert_eq!(wake(&x.s, &x.w, &f, "kzzzzzzzz", GAP, &idle_at(T0)), Outcome::NotLive);
    assert!(f.calls().is_empty());
    assert!(errors(&x.s).is_empty());
}

#[test]
fn pane_whose_zygo_pane_differs_is_refused() {
    let x = setup();
    let cases: Vec<Fake> = vec![
        Fake { pane_opt: Ok(Some(OTHER.into())), ..Fake::good() },
        Fake { pane_opt: Ok(None), ..Fake::good() },
        Fake { panes: Ok(vec![row("%3", ELENA), row("%5", ELENA)]), ..Fake::good() },
    ];
    for f in cases {
        assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Mismatch);
        assert_eq!(f.sends(), 0, "{:?}", f.calls());
        assert_eq!(wake_json(&x.s), None);
    }
    assert!(errors(&x.s).is_empty());
}

#[test]
fn session_whose_zygo_ws_differs_is_refused() {
    let x = setup();
    for v in [Some("fedcba9876543210".to_string()), None] {
        let f = Fake { ws_opt: Ok(v), ..Fake::good() };
        assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Mismatch);
        assert_eq!(f.calls(), ["show_session_option zygo-app @zygo-ws"]);
    }
    assert!(errors(&x.s).is_empty());
}

#[test]
fn no_pane_with_the_key() {
    let x = setup();
    let f = Fake { panes: Ok(vec![row("%4", OTHER)]), ..Fake::good() };
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::NoPane);
    assert_eq!(f.sends(), 0);
    assert!(errors(&x.s).is_empty());
}

#[test]
fn second_wake_inside_the_gap_is_skipped() {
    let x = setup();
    let f = Fake::good();
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Woken);
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0 + 29)), Outcome::Gap);
    assert_eq!(f.sends(), 2);
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0 + 30)), Outcome::Woken);
    assert_eq!(f.sends(), 4);
    assert!(wake_json(&x.s).unwrap().contains("\"2023-11-14T22:13:50Z\""));
    let put = |text: &str| fs::write(wake_path(&x.s), text).unwrap();
    // 2023-11-14T22:13:25Z = T0 + 5: a clock that went back blocks for less than the gap.
    put(r#"{"woken_at":"2023-11-14T22:13:25Z"}"#);
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Gap);
    // T0 + 86400.
    put(r#"{"woken_at":"2023-11-15T22:13:20Z"}"#);
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Woken);
    put("x");
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Woken);
    let huge = parse_duration("213503982334601d").unwrap();
    fs::remove_file(wake_path(&x.s)).unwrap();
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, huge, &idle_at(T0)), Outcome::Woken);
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, huge, &idle_at(T0 + 1)), Outcome::Gap);
    assert!(errors(&x.s).is_empty());
}

#[test]
fn wake_json_is_written_under_the_mail_lock() {
    let x = setup();
    let lock = x.s.dir().join("mail").join(ELENA).join("wake.lock");
    let seen = Arc::new(AtomicBool::new(false));
    let (l, s2) = (lock.clone(), seen.clone());
    let f = Fake {
        on_send: Some(Box::new(move || {
            let h = fs::OpenOptions::new().read(true).write(true).open(&l).unwrap();
            assert!(matches!(fs4::FileExt::try_lock(&h), Err(fs4::TryLockError::WouldBlock)));
            s2.store(true, Ordering::SeqCst);
        })),
        ..Fake::good()
    };
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Woken);
    assert!(seen.load(Ordering::SeqCst));
    assert!(errors(&x.s).is_empty());
}

#[test]
fn tmux_error_logs_one_line_and_keeps_the_message() {
    let x = setup();
    let msg = "no server running on /tmp/zygo-x/s";
    let f = Fake { ws_opt: Err(MuxError::Gone { message: msg.into() }), ..Fake::good() };
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Failed(msg.into()));
    let e = errors(&x.s);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].ends_with(&format!(" mail wake kelena222: {msg}")), "{e:?}");
    assert!(e.iter().all(|l| !l.contains("subj-7f3a") && !l.contains("body-9c1e")));
    assert_eq!(new_count(&x.p), 1);
    assert_eq!(wake_json(&x.s), None);
}

#[test]
fn wake_writes_no_activity_json() {
    let x = setup();
    let dir = x.p.sessions_dir.join("s1");
    let bytes = br#"{"version":1,"busy":false}"#;
    fs::write(dir.join("activity.json"), bytes).unwrap();
    let f = Fake::good();
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Woken);
    // No proxy url, so the proxy status is not read and the member is busy (AU26).
    assert!(!RealProbe.idle(&dir));
    assert_eq!(fs::read(dir.join("activity.json")).unwrap(), bytes);
    assert!(!dir.join("activity.lock").exists());
    assert!(errors(&x.s).is_empty());
}

#[test]
fn after_send_skips_busy_and_not_live_members() {
    let x = setup();
    let mut w = x.w.clone();
    let mut anon = MemberRecord::new(Agent::ClaudeCode);
    anon.zygo_session = Some("s2".into());
    w.members.insert("kanon2222".into(), anon);
    let tmux = Path::new("/nonexistent/tmux");
    let keys = [ELENA.to_string(), "kanon2222".to_string()];
    after_send(&x.p, &w, tmux, &keys, &MailConfig::default());
    assert!(errors(&x.s).is_empty());
    assert_eq!(wake_json(&x.s), None);
    let cfg = MailConfig { wake_gap: "soon".into(), ..MailConfig::default() };
    after_send(&x.p, &w, tmux, &keys, &cfg);
    let e = errors(&x.s);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].ends_with(r#"mail wake: [mail] wake_gap "soon" is not <n>s, <n>m, <n>h or <n>d; using 30s"#), "{e:?}");
    after_send(&x.p, &w, tmux, &[], &cfg);
    assert_eq!(errors(&x.s).len(), 1);
}

#[test]
#[ignore = "timing: real tmux under the fixed 2 s tmux bound"]
fn wake_text_reaches_a_stub_program_on_a_private_server() {
    let Some(_) = require_tmux() else { return };
    let mut x = setup();
    let srv = PrivateServer::start();
    let dir = TempDir::new().unwrap();
    let got = dir.path().join("got");
    let script = format!("read -r l; printf '%s' \"$l\" > '{}'; sleep 600", got.display());
    let argv: Vec<OsString> = ["sh", "-c", script.as_str()].iter().map(OsString::from).collect();
    let env: Vec<(OsString, OsString)> =
        ["PATH", "HOME"].iter().filter_map(|k| std::env::var_os(k).map(|v| (OsString::from(k), v))).collect();
    let t = srv.tmux();
    t.new_session(&NewSession {
        name: "zygo-app",
        start_dir: dir.path(),
        width: 100,
        height: 30,
        env: &env,
        argv: &argv,
        owner: Some((ID, ELENA)),
    })
    .unwrap();
    x.w.tmux_socket = Some(srv.socket().into());
    assert_eq!(wake(&x.s, &x.w, &t, ELENA, GAP, &idle_at(T0)), Outcome::Woken);
    assert!(until(|| fs::read_to_string(&got).is_ok_and(|s| s == WAKE_TEXT)));
    assert_eq!(wake(&x.s, &x.w, &t, ELENA, GAP, &idle_at(T0 + 29)), Outcome::Gap);
    assert!(errors(&x.s).is_empty());
}

/// Review fix R1: a held `wake.lock` makes the wake wait; after the release its outcome follows `wake.json`.
#[test]
fn held_wake_lock_waits_then_follows_wake_json() {
    let x = setup();
    let lock = x.s.dir().join("mail").join(ELENA).join("wake.lock");
    let (s, w) = (&x.s, &x.w);
    // The holder woke the pane: it wrote `woken_at`, so the waiter gets `Gap`.
    let h = hold(lock.clone());
    std::thread::scope(|sc| {
        let t = sc.spawn(|| {
            let f = Fake::good();
            (wake(s, w, &f, ELENA, GAP, &idle_at(T0 + 10)), f.sends())
        });
        std::thread::sleep(Duration::from_millis(300));
        assert!(!t.is_finished(), "the wake did not wait for the held lock");
        fs::write(wake_path(s), r#"{"woken_at":"2023-11-14T22:13:20Z"}"#).unwrap();
        drop(h);
        assert_eq!(t.join().unwrap(), (Outcome::Gap, 0));
    });
    // The holder failed: it wrote nothing, so the waiter wakes the pane.
    fs::remove_file(wake_path(s)).unwrap();
    let h = hold(lock);
    std::thread::scope(|sc| {
        let t = sc.spawn(|| {
            let f = Fake::good();
            (wake(s, w, &f, ELENA, GAP, &idle_at(T0 + 10)), f.sends())
        });
        std::thread::sleep(Duration::from_millis(300));
        assert!(!t.is_finished(), "the wake did not wait for the held lock");
        drop(h);
        assert_eq!(t.join().unwrap(), (Outcome::Woken, 2));
    });
    assert!(errors(s).is_empty());
}

/// Review fix R2: a pane-list line that does not parse fails the wake with one log line.
#[test]
fn pane_line_that_does_not_parse_fails() {
    let x = setup();
    let f = Fake { panes: Ok(vec![row("%3", ELENA), "not a pane line".into()]), ..Fake::good() };
    assert!(matches!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Failed(_)));
    assert_eq!(f.sends(), 0);
    let e = errors(&x.s);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].contains(" mail wake kelena222: "), "{e:?}");
    assert_eq!(wake_json(&x.s), None);
}

const BOB: &str = "kbob22222";
/// ELENA (`s1`) and BOB (`s2`), both live, each with a maildir.
fn two_members(x: &Setup) -> (Workspace, fs::File) {
    let mut w = x.w.clone();
    let mut m = MemberRecord::new(Agent::ClaudeCode);
    m.name = Some("bob".into());
    m.zygo_session = Some("s2".into());
    w.members.insert(BOB.into(), m);
    let g = live(&x.p, "s2");
    let msg = Message::new("lead", "bob", "s", "b", at(T0)).unwrap();
    mail_store::deliver(&mail_dir(&x.p, ID), BOB, "bob", &msg, 500).unwrap();
    (w, g)
}
/// Idle; the `idle` check of the session `s1` takes 1.5 s.
struct SlowS1;
impl Probe for SlowS1 {
    fn idle(&self, d: &Path) -> bool {
        if d.ends_with("s1") {
            std::thread::sleep(Duration::from_millis(1500));
        }
        true
    }
    fn now(&self) -> SystemTime { at(T0) }
}

/// Review fix R3: one slow member does not delay the wake of another (M§13 #5, 2 s).
#[test]
#[ignore = "timing: a 1.5 s probe against a 1 s bound"]
fn slow_member_does_not_delay_another() {
    let x = setup();
    let (w, _g) = two_members(&x);
    let f = Fake { panes: Ok(vec![row("%3", ELENA), row("%4", BOB)]), pane_keys: true, ..Fake::good() };
    let start = Instant::now();
    after_send_with(&x.p, &w, &[ELENA.into(), BOB.into()], &MailConfig::default(), &f, &SlowS1, at(T0));
    let enters = f.enters.lock().unwrap().clone();
    let bob = enters.iter().find(|(p, _)| p == "%4").expect("BOB was not woken");
    assert!(bob.1.duration_since(start) < Duration::from_secs(1), "{:?}", bob.1.duration_since(start));
    assert!(enters.iter().any(|(p, _)| p == "%3"), "ELENA was not woken");
    assert!(errors(&x.s).is_empty());
}

/// Review fix R4: `after_send_with` with a fake idle probe and a fake mux wakes the target once.
#[test]
fn after_send_with_wakes_the_target_once() {
    let x = setup();
    let f = Fake::good();
    // The probe's own clock is not used: `now` is the clock.
    after_send_with(&x.p, &x.w, &[ELENA.into()], &MailConfig::default(), &f, &idle_at(T0 + 999), at(T0));
    assert_eq!(f.sends(), 2);
    assert_eq!(wake_json(&x.s).as_deref(), Some(r#"{"woken_at":"2023-11-14T22:13:20Z"}"#));
    after_send_with(&x.p, &x.w, &[ELENA.into()], &MailConfig::default(), &f, &idle_at(T0), at(T0 + 1));
    assert_eq!(f.sends(), 2);
    assert!(errors(&x.s).is_empty());
}

/// Review fix R5: a key that is not a pane key gives `NotLive` and builds no path.
#[test]
fn invalid_key_builds_no_path() {
    let x = setup();
    let mut w = x.w.clone();
    let m = w.members[ELENA].clone();
    w.members.insert("..".into(), m);
    let f = Fake::good();
    assert_eq!(wake(&x.s, &w, &f, "..", GAP, &idle_at(T0)), Outcome::NotLive);
    assert!(f.calls().is_empty());
    assert!(!x.s.dir().join("wake.lock").exists());
    assert!(!x.s.dir().join("wake.json").exists());
    assert!(errors(&x.s).is_empty());
}

/// Review fix R6: each tmux error gives `Failed` with one log line; after a failed Enter the text counts for the gap.
#[test]
fn each_tmux_error_fails_with_one_line() {
    let boom = || MuxError::Failed { message: "boom".into() };
    let cases: Vec<Fake> = vec![
        Fake { keys: Err(boom()), ..Fake::good() },
        Fake { enter: Err(boom()), ..Fake::good() },
        Fake { panes: Err(boom()), ..Fake::good() },
        Fake { pane_opt: Err(boom()), ..Fake::good() },
    ];
    for (i, f) in cases.into_iter().enumerate() {
        let x = setup();
        assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::Failed("boom".into()), "case {i}");
        let e = errors(&x.s);
        assert_eq!(e.len(), 1, "case {i}: {e:?}");
        assert!(e[0].ends_with(" mail wake kelena222: boom"), "case {i}: {e:?}");
        if i == 1 {
            let g = Fake::good();
            assert_eq!(wake(&x.s, &x.w, &g, ELENA, GAP, &idle_at(T0 + 29)), Outcome::Gap);
            assert_eq!(g.sends(), 0);
        }
    }
}

/// Review fix R7: `woken_at` keeps its fraction of a second, so the gap is not shorter than `wake_gap`.
#[test]
fn gap_keeps_the_fraction_of_a_second() {
    let x = setup();
    let f = Fake::good();
    let t1 = at(T0) + Duration::from_millis(900);
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &At { idle: true, now: t1 }), Outcome::Woken);
    let t2 = t1 + GAP - Duration::from_millis(500);
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &At { idle: true, now: t2 }), Outcome::Gap);
    assert_eq!(f.sends(), 2);
}

/// Review fix R8: `Store::live_session` prefers `pending` and refuses an id that names a path outside `sessions/`.
#[test]
fn live_session_prefers_pending_and_refuses_bad_ids() {
    let x = setup();
    let _g2 = live(&x.p, "s2");
    let mut m = x.w.members[ELENA].clone();
    assert_eq!(x.s.live_session(&m), Some(x.p.sessions_dir.join("s1")));
    m.pending = Some(Pending {
        zygo_session: "s2".into(),
        agent_session: None,
        own_session: false,
        started_at: 0,
        extra: Default::default(),
    });
    assert_eq!(x.s.live_session(&m), Some(x.p.sessions_dir.join("s2")));
    let _g3 = live(&x.p, "../x");
    let mut bad = MemberRecord::new(Agent::ClaudeCode);
    bad.zygo_session = Some("../x".into());
    assert_eq!(x.s.live_session(&bad), None);
}

/// Review fix R9: a member with no maildir gives `NotLive` and creates no folder or file.
#[test]
fn missing_maildir_gives_not_live() {
    let x = setup();
    let md = x.s.dir().join("mail").join(ELENA);
    fs::remove_dir_all(&md).unwrap();
    let f = Fake::good();
    assert_eq!(wake(&x.s, &x.w, &f, ELENA, GAP, &idle_at(T0)), Outcome::NotLive);
    assert!(!md.exists());
    assert!(f.calls().is_empty());
    assert!(errors(&x.s).is_empty());
}

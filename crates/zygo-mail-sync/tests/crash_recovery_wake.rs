#![cfg(unix)]
//! FX-30: a crash-recovered delivery is counted and woken (mail sync design S§8.3 steps 5 and 10, S§10.1, S§19 #14).
//! Two machines on one local bare repository, the injected clock, the S-C9 crash seam and pass deadline seam, and a
//! multiplexer stub that records each typed text. No test sleeps or reads the wall clock: each pass has an hour of
//! deadline (`sync::run_with_deadline`), and the clock of the wake is the injected one.
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{live, Machine, Remote, ALICE, BOB, WS_ID};
use ulid::Ulid;
use zygo_config::{LoadContext, MailConfig};
use zygo_core::paths::Paths;
use zygo_mail_sync::address::Address;
use zygo_mail_sync::env::{self, Crash, Env, Seam, Spawner, Verdict, Wake};
use zygo_mail_sync::state::load;
use zygo_mail_sync::sync::{run_with_deadline, Mode, Opts, Report, SyncError};
use zygo_redact::DeadlinePolicy;
use zygo_workspace::mail::wake::{after_send_with, Probe as IdleProbe, WAKE_TEXT};
use zygo_workspace::mux::{HookScope, Multiplexer, MuxError, NewSession, NewWindow, SessionAnswer, Split};
use zygo_workspace::state::Workspace;

/// The tests of this file run one at a time: each runs git children and takes sync locks, and a child that another
/// thread starts holds a copy of each open file description until its exec (see `sync_pass.rs`, `SPAWNS`).
static ONE: Mutex<()> = Mutex::new(());
fn one_at_a_time() -> MutexGuard<'static, ()> { ONE.lock().unwrap_or_else(|e| e.into_inner()) }

/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;
/// The pass deadline of every pass: an hour, as the S-C9 seam gives, so no result depends on how fast git runs.
const FAR_PASS: Duration = Duration::from_secs(3600);
/// M§11: the default `wake_gap`.
const WAKE_GAP: u64 = 30;

/// The injected clock: `now()` gives `UNIX_EPOCH + secs`.
struct Clock(Arc<AtomicU64>);
impl env::Clock for Clock {
    fn now(&self) -> SystemTime { UNIX_EPOCH + Duration::from_secs(self.0.load(Ordering::SeqCst)) }
}

struct Says(Verdict);
impl env::Probe for Says {
    fn probe(&self, _: &str) -> Verdict { self.0.clone() }
}

/// The crash seam: `Err(Crash(point))` at the first call at `point`; `None` never stops.
struct CrashAt(Option<&'static str>, Mutex<bool>);
impl CrashAt {
    fn none() -> CrashAt { CrashAt(None, Mutex::new(false)) }
    fn at(point: &'static str) -> CrashAt { CrashAt(Some(point), Mutex::new(false)) }
}
impl Seam for CrashAt {
    fn at(&self, p: &'static str) -> Result<(), Crash> {
        let mut done = self.1.lock().unwrap();
        if self.0 == Some(p) && !*done {
            *done = true;
            return Err(Crash(p));
        }
        Ok(())
    }
}

struct NoSpawn;
impl Spawner for NoSpawn {
    fn spawn_sync(&self, _: &str) -> std::io::Result<()> { Ok(()) }
}

/// The idle rule of a wake: fixed, with the clock of the sync.
struct Idle(bool, SystemTime);
impl IdleProbe for Idle {
    fn idle(&self, _: &Path) -> bool { self.0 }
    fn now(&self) -> SystemTime { self.1 }
}

type Typed = Arc<Mutex<Vec<(String, String)>>>;

/// A session `@zygo-ws` = [`WS_ID`] with one pane `%3` that holds `key`; each typed text is recorded (as
/// `sync_pass.rs`, `StubMux`).
struct StubMux {
    key: String,
    typed: Typed,
}
impl Multiplexer for StubMux {
    fn socket(&self) -> Option<&Path> { None }
    fn version(&self) -> Result<(u32, u32), MuxError> { unreachable!() }
    fn socket_path(&self, _: &str) -> Result<PathBuf, MuxError> { unreachable!() }
    fn session_exists(&self, _: &str) -> SessionAnswer { unreachable!() }
    fn new_session(&self, _: &NewSession<'_>) -> Result<u32, MuxError> { unreachable!() }
    fn resize_window(&self, _: &str, _: u32, _: u32, _: u32) -> Result<(), MuxError> { unreachable!() }
    fn unset_window_option(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn set_window_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn set_env(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn hide_env(&self, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn set_session_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn show_session_option(&self, _: &str, _: &str) -> Result<Option<String>, MuxError> { Ok(Some(WS_ID.into())) }
    fn set_hook(&self, _: &str, _: HookScope, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn new_window(&self, _: &NewWindow<'_>) -> Result<u32, MuxError> { unreachable!() }
    fn rename_window(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn split_window(&self, _: &Split<'_>) -> Result<String, MuxError> { unreachable!() }
    fn select_layout(&self, _: &str, _: u32, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn select_window(&self, _: &str, _: u32) -> Result<(), MuxError> { unreachable!() }
    fn select_pane(&self, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn zoom(&self, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn list_panes(&self, _: &str) -> Result<Vec<String>, MuxError> {
        Ok(vec![format!("0|w|1|l|80|24|1|0|%3|0|1|{}|/srv/app", self.key)])
    }
    fn show_pane_option(&self, _: &str, _: &str) -> Result<Option<String>, MuxError> { Ok(Some(self.key.clone())) }
    fn set_pane_option(&self, _: &str, _: &str, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn display(&self, _: &str, _: &str) -> Result<String, MuxError> { unreachable!() }
    fn kill_session(&self, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn attach(&self, _: &str) -> Result<i32, MuxError> { unreachable!() }
    fn switch_client(&self, _: &str) -> Result<(), MuxError> { unreachable!() }
    fn send_keys_literal(&self, pane: &str, text: &str) -> Result<(), MuxError> {
        self.typed.lock().unwrap().push((pane.to_string(), text.to_string()));
        Ok(())
    }
    fn send_enter(&self, _: &str) -> Result<(), MuxError> { Ok(()) }
}

/// The wake seam: the M§7.3 wake of each key through a [`StubMux`] and a fixed idle rule.
struct StubWake {
    idle: bool,
    typed: Typed,
}
impl Wake for StubWake {
    fn wake(&self, paths: &Paths, ws: &Workspace, keys: &[String], cfg: &MailConfig, now: SystemTime) {
        for k in keys {
            let mux = StubMux { key: k.clone(), typed: self.typed.clone() };
            after_send_with(paths, ws, std::slice::from_ref(k), cfg, &mux, &Idle(self.idle, now), now);
        }
    }
}

/// The env of `m` with the seam `seam`, the clock at `secs`, and an idle wake that records into `typed`.
fn env_at(m: &Machine, seam: Box<dyn Seam>, secs: u64, typed: &Typed) -> Env {
    Env {
        paths: m.paths.clone(),
        clock: Box::new(Clock(Arc::new(AtomicU64::new(secs)))),
        probe: Box::new(Says(Verdict::Local)),
        seam,
        spawner: Box::new(NoSpawn),
        wake: Box::new(StubWake { idle: true, typed: typed.clone() }),
        secret_policy: DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() },
    }
}

/// One CLI pass of `m` with the seam `seam` at `secs`, an hour of pass deadline.
fn pass(m: &Machine, seam: CrashAt, secs: u64, typed: &Typed) -> Result<Report, SyncError> {
    pass_with(m, Box::new(seam), secs, typed)
}

/// [`pass`] with any seam.
fn pass_with(m: &Machine, seam: Box<dyn Seam>, secs: u64, typed: &Typed) -> Result<Report, SyncError> {
    run_with_deadline(&env_at(m, seam, secs, typed), WS_ID, Mode::Cli, &Opts::default(), FAR_PASS)
}

/// One CLI pass of `m` with no seam at `T0`, and a wake that records nowhere a test reads.
fn sync(m: &Machine) -> Report { pass(m, CrashAt::none(), T0, &Typed::default()).unwrap() }

/// One message from `alice` on `a` to `bob@b`, through the send path of MS-9, at `secs`.
fn send(a: &Machine, secs: u64) -> Ulid {
    let cfg = zygo_config::load(&LoadContext { paths: a.paths.clone(), repo: Some(a.root.clone()), team: None })
        .unwrap()
        .config;
    let s = zygo_config::sync_for(&cfg, "app");
    let to = Address { name: "bob".into(), host: "b".into() };
    zygo_mail_sync::outbox::send_remote_with(&a.sp(), s.as_ref(), Some("alice"), &to, "status", "hello", &cfg.redaction,
        &a.root, UNIX_EPOCH + Duration::from_secs(secs), &DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() })
        .unwrap()
}

/// A remote, machine `a` (host `a`, member `alice`) and machine `b` (host `b`, member `bob`); B, A and B sync, so
/// each member cache knows the other host.
fn pair() -> (Remote, Machine, Machine) {
    let r = Remote::new();
    let a = Machine::new(&r);
    a.config(Some("a"), "");
    a.workspace(&[(ALICE, "alice")]);
    let b = Machine::new(&r);
    b.config(Some("b"), "");
    b.workspace(&[(BOB, "bob")]);
    sync(&b);
    sync(&a);
    sync(&b);
    (r, a, b)
}

/// How many times the file of `id` is in `mail/<BOB>/new/`.
fn in_new(b: &Machine, id: Ulid) -> usize {
    let Ok(d) = fs::read_dir(b.sp().mail.join(BOB).join("new")) else { return 0 };
    d.filter(|e| e.as_ref().unwrap().file_name().to_str() == Some(&format!("{id}.json"))).count()
}

fn wake_once() -> Vec<(String, String)> { vec![("%3".to_string(), WAKE_TEXT.to_string())] }

/// `mail/sync/state.json` of `b` as JSON (read as JSON, so this file builds and runs on the code before FX-30).
fn state_json(b: &Machine) -> serde_json::Value { serde_json::from_slice(&fs::read(b.sp().state).unwrap()).unwrap() }

/// The `pending_wake` map of `b`'s state.json; empty when the key is missing.
fn pending_wake(b: &Machine) -> serde_json::Map<String, serde_json::Value> {
    state_json(b).get("pending_wake").and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

/// FX-30 Acceptance, S§8.3 steps 5 and 10, S§10.1, S§19 #14: a crash at `point` (`import::run`), then the recovery
/// pass counts the message once and wakes the idle `bob` with a live launcher once, and the pass after, with the
/// clock past `wake_gap`, counts and wakes nothing. Each point runs twice on one pair, so the second run starts from
/// the state that the first one's three passes left. Each point is its own test, so a failure names its point.
fn crash_recovery_counts_and_wakes_once(point: &'static str) {
    let _g = one_at_a_time();
    let (_r, a, b) = pair();
    let _live = live(&b.paths, "sbob");
    for k in 1u64..=2 {
        let id = send(&a, T0 + k);
        assert_eq!(sync(&a).pushed, 1, "{point}");
        let crashed = Typed::default();
        let e = pass(&b, CrashAt::at(point), T0 + 1000 * k, &crashed).unwrap_err();
        assert_eq!(e, SyncError::Stopped { line: format!("zygo: stopped at seam {point}"), check: "seam" }, "{point}");
        assert!(crashed.lock().unwrap().is_empty(), "{point}: the crashed run woke");
        // The recovery pass: step 5 or the pending-wake record gives the count and the wake of the recovered message.
        let typed = Typed::default();
        let t = T0 + 1000 * k + 1;
        let rep = pass(&b, CrashAt::none(), t, &typed).unwrap();
        assert_eq!(rep.delivered, 1, "{point}: the recovery pass count");
        assert_eq!(*typed.lock().unwrap(), wake_once(), "{point}: the recovery pass wake");
        assert_eq!(in_new(&b, id), 1, "{point}");
        // The pass after, past `wake_gap`: a wake here could only come from a second count.
        let rep = pass(&b, CrashAt::none(), t + WAKE_GAP + 1, &typed).unwrap();
        assert_eq!(rep.delivered, 0, "{point}: the pass after the recovery counts again");
        assert_eq!(*typed.lock().unwrap(), wake_once(), "{point}: the pass after the recovery wakes again");
        assert_eq!(in_new(&b, id), 1, "{point}");
        assert!(pending_wake(&b).is_empty(), "{point}");
    }
}

#[test]
fn crash_after_incoming_is_counted_and_woken_once() { crash_recovery_counts_and_wakes_once("after_incoming") }

#[test]
fn crash_after_log_is_counted_and_woken_once() { crash_recovery_counts_and_wakes_once("after_log") }

#[test]
fn crash_after_new_is_counted_and_woken_once() { crash_recovery_counts_and_wakes_once("after_new") }

#[test]
fn crash_after_ack_is_counted_and_woken_once() { crash_recovery_counts_and_wakes_once("after_ack") }

#[test] // FX-30 author's call 3: a pending entry whose message never arrived is dropped, not counted
fn pending_entry_whose_message_never_arrived_is_dropped() {
    let _g = one_at_a_time();
    let (_r, _a, b) = pair();
    let _live = live(&b.paths, "sbob");
    let ghost = Ulid::from_parts(T0 * 1000, 7);
    let mut st = state_json(&b);
    st.as_object_mut().unwrap().insert("pending_wake".into(), serde_json::json!({ ghost.to_string(): BOB }));
    fs::write(b.sp().state, serde_json::to_vec_pretty(&st).unwrap()).unwrap();
    load(&b.sp()).unwrap();
    let typed = Typed::default();
    let rep = pass(&b, CrashAt::none(), T0 + 100, &typed).unwrap();
    assert_eq!(rep.delivered, 0);
    assert!(typed.lock().unwrap().is_empty(), "a message that never arrived woke its member");
    assert_eq!(in_new(&b, ghost), 0);
    assert!(pending_wake(&b).is_empty(), "the entry was not dropped");
}

#[test] // FX-30: `pending_wake` is `serde(default)`: a state.json written before FX-30 loads, and the sync runs on it
fn old_state_without_pending_wake_loads() {
    let _g = one_at_a_time();
    let (_r, a, b) = pair();
    let _live = live(&b.paths, "sbob");
    let mut old = state_json(&b);
    old.as_object_mut().unwrap().remove("pending_wake");
    fs::write(b.sp().state, serde_json::to_vec_pretty(&old).unwrap()).unwrap();
    assert!(load(&b.sp()).unwrap().last_ok.is_some(), "the state.json before FX-30 does not load");
    assert!(pending_wake(&b).is_empty());
    let id = send(&a, T0 + 50);
    sync(&a);
    let typed = Typed::default();
    assert_eq!(pass(&b, CrashAt::none(), T0 + 100, &typed).unwrap().delivered, 1);
    assert_eq!(*typed.lock().unwrap(), wake_once());
    assert_eq!(in_new(&b, id), 1);
}

/// `bob` reads the message `id`: its file moves from `mail/<BOB>/new/` to `cur/`.
fn read_by_bob(b: &Machine, id: Ulid) {
    let md = b.sp().mail.join(BOB);
    let n = format!("{id}.json");
    fs::create_dir_all(md.join("cur")).unwrap();
    fs::rename(md.join("new").join(&n), md.join("cur").join(&n)).unwrap();
}

/// How many times the file of `id` is in `mail/<BOB>/cur/`.
fn in_cur(b: &Machine, id: Ulid) -> usize {
    let Ok(d) = fs::read_dir(b.sp().mail.join(BOB).join("cur")) else { return 0 };
    d.filter(|e| e.as_ref().unwrap().file_name().to_str() == Some(&format!("{id}.json"))).count()
}

/// The names in `b`'s `mail/sync/incoming/`.
fn incoming(b: &Machine) -> Vec<String> {
    let Ok(d) = fs::read_dir(b.sp().incoming) else { return Vec::new() };
    d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
}

/// Puts the bare repository back at `.0` when it was moved to `<bare>.away` (the guard of [`BreakPush`] and of a
/// pass whose fetch fails).
struct BareBack(PathBuf);
impl Drop for BareBack {
    fn drop(&mut self) {
        let away = self.0.with_extension("away");
        if away.exists() {
            fs::rename(&away, &self.0).unwrap();
        }
    }
}

/// At `before_push`: a crash (`crash`), or the bare repository moved to `<bare>.away`, so the push fails (a stop
/// that is not a crash). [`BareBack`] puts it back.
struct BreakPush {
    bare: PathBuf,
    crash: bool,
}
impl Seam for BreakPush {
    fn at(&self, p: &'static str) -> Result<(), Crash> {
        if p == "before_push" {
            if self.crash {
                return Err(Crash(p));
            }
            fs::rename(&self.bare, self.bare.with_extension("away")).unwrap();
        }
        Ok(())
    }
}

/// At `point`: the mode of `dir` becomes `mode`; its mode before goes into `before`, which [`ModeBack`] puts back.
struct ModeAt {
    point: &'static str,
    dir: PathBuf,
    mode: u32,
    before: Arc<Mutex<Option<u32>>>,
}
impl Seam for ModeAt {
    fn at(&self, p: &'static str) -> Result<(), Crash> {
        use std::os::unix::fs::PermissionsExt;
        if p == self.point {
            let old = fs::metadata(&self.dir).unwrap().permissions().mode() & 0o7777;
            self.before.lock().unwrap().get_or_insert(old);
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(self.mode)).unwrap();
        }
        Ok(())
    }
}

/// Puts back the mode of `.0` that a [`ModeAt`] changed.
struct ModeBack(PathBuf, Arc<Mutex<Option<u32>>>);
impl Drop for ModeBack {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        if let Some(m) = *self.1.lock().unwrap() {
            fs::set_permissions(&self.0, fs::Permissions::from_mode(m)).unwrap();
        }
    }
}

/// A [`ModeAt`] seam and the guard that puts the mode back.
fn mode_at(point: &'static str, dir: PathBuf, mode: u32) -> (Box<dyn Seam>, ModeBack) {
    let before = Arc::new(Mutex::new(None));
    (Box::new(ModeAt { point, dir: dir.clone(), mode, before: before.clone() }), ModeBack(dir, before))
}

/// At `point`: the folder `dir` moves to `<dir>.away` and a regular file takes its name, so a delivery into it
/// fails and its staged file stays in `incoming/` (`make_dirs` puts the mode of a folder back to `0700`, so a
/// read-only folder does not stop it). [`DirBack`] puts the folder back.
struct NotDirAt {
    point: &'static str,
    dir: PathBuf,
}
impl Seam for NotDirAt {
    fn at(&self, p: &'static str) -> Result<(), Crash> {
        if p == self.point {
            fs::rename(&self.dir, self.dir.with_extension("away")).unwrap();
            fs::write(&self.dir, b"").unwrap();
        }
        Ok(())
    }
}

/// Puts back the folder `.0` that a [`NotDirAt`] moved away.
struct DirBack(PathBuf);
impl Drop for DirBack {
    fn drop(&mut self) {
        let away = self.0.with_extension("away");
        if away.exists() {
            fs::remove_file(&self.0).unwrap();
            fs::rename(&away, &self.0).unwrap();
        }
    }
}

#[test] // FX-30 review F1: a pass that wakes and then stops at its push clears the record of what it counted
fn push_stops_after_the_wake_wake_once() {
    let _g = one_at_a_time();
    let (r, a, b) = pair();
    let _live = live(&b.paths, "sbob");
    let id = send(&a, T0 + 1);
    assert_eq!(sync(&a).pushed, 1);
    let typed = Typed::default();
    for k in 0..3u64 {
        let _back = BareBack(r.bare.clone());
        let e = pass_with(&b, Box::new(BreakPush { bare: r.bare.clone(), crash: false }), T0 + 1000 + 120 * k, &typed)
            .unwrap_err();
        assert!(matches!(e, SyncError::Stopped { check, .. } if check != "seam"), "pass {k}: {e:?}");
        // Only the first pass delivers and counts. A stopped pass gives no Report, so its count shows in the record
        // it leaves: the step 10 write removed the counted entry.
        assert_eq!(*typed.lock().unwrap(), wake_once(), "pass {k}: a stopped pass wakes again");
        assert!(pending_wake(&b).is_empty(), "pass {k}: the counted entry is still recorded");
    }
    let rep = pass(&b, CrashAt::none(), T0 + 2000, &typed).unwrap();
    assert_eq!(rep.delivered, 0, "the pass that works counts the message again");
    assert_eq!(*typed.lock().unwrap(), wake_once(), "the pass that works wakes again");
    assert_eq!(in_new(&b, id), 1);
}

#[test] // FX-30 review F1: a crash at `before_push`, after the wake, does not count or wake the message again
fn crash_before_push_after_the_wake_wakes_once() {
    let _g = one_at_a_time();
    let (r, a, b) = pair();
    let _live = live(&b.paths, "sbob");
    let id = send(&a, T0 + 1);
    assert_eq!(sync(&a).pushed, 1);
    let typed = Typed::default();
    let e = pass_with(&b, Box::new(BreakPush { bare: r.bare.clone(), crash: true }), T0 + 1000, &typed).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: stopped at seam before_push".into(), check: "seam" });
    assert_eq!(*typed.lock().unwrap(), wake_once());
    let rep = pass(&b, CrashAt::none(), T0 + 1000 + WAKE_GAP + 1, &typed).unwrap();
    assert_eq!(rep.delivered, 0, "the pass after the crash counts the message again");
    assert_eq!(*typed.lock().unwrap(), wake_once(), "the pass after the crash wakes again");
    assert_eq!(in_new(&b, id), 1);
    assert!(pending_wake(&b).is_empty());
}

#[test] // FX-30 review F1: a stop after the wake and before the commit's state save (the outbox step) keeps no entry
fn outbox_stop_after_the_wake_wakes_once() {
    use std::os::unix::fs::DirBuilderExt;
    let _g = one_at_a_time();
    let (_r, a, b) = pair();
    let _live = live(&b.paths, "sbob");
    let id = send(&a, T0 + 1);
    assert_eq!(sync(&a).pushed, 1);
    let outbox = b.sp().outbox;
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&outbox).unwrap();
    let typed = Typed::default();
    let (seam, back) = mode_at("after_ack", outbox, 0o000);
    let e = pass_with(&b, seam, T0 + 1000, &typed).unwrap_err();
    drop(back);
    assert!(matches!(e, SyncError::Stopped { check: "outbox", .. }), "{e:?}");
    assert_eq!(*typed.lock().unwrap(), wake_once());
    assert!(pending_wake(&b).is_empty(), "the counted entry is still recorded after the stop");
    let rep = pass(&b, CrashAt::none(), T0 + 1000 + WAKE_GAP + 1, &typed).unwrap();
    assert_eq!(rep.delivered, 0, "the pass after the stop counts the message again");
    assert_eq!(*typed.lock().unwrap(), wake_once(), "the pass after the stop wakes again");
    assert_eq!(in_new(&b, id), 1);
}

#[test] // FX-30 review F2: a recovered message that bob already read is counted once and wakes no one (S§10.1)
fn recovered_message_already_read_is_counted_not_woken() {
    let _g = one_at_a_time();
    let (_r, a, b) = pair();
    let _live = live(&b.paths, "sbob");
    let id = send(&a, T0 + 1);
    assert_eq!(sync(&a).pushed, 1);
    let typed = Typed::default();
    let e = pass(&b, CrashAt::at("after_new"), T0 + 1000, &typed).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: stopped at seam after_new".into(), check: "seam" });
    read_by_bob(&b, id);
    let rep = pass(&b, CrashAt::none(), T0 + 1001, &typed).unwrap();
    assert_eq!(rep.delivered, 1, "the recovered message in cur/ is not counted");
    assert!(typed.lock().unwrap().is_empty(), "bob is woken for a message he already read");
    let rep = pass(&b, CrashAt::none(), T0 + 1001 + WAKE_GAP + 1, &typed).unwrap();
    assert_eq!(rep.delivered, 0, "the pass after the recovery counts again");
    assert!(typed.lock().unwrap().is_empty());
    assert_eq!((in_new(&b, id), in_cur(&b, id)), (0, 1));
    assert!(pending_wake(&b).is_empty());
}

#[test] // FX-30 review F3: an entry whose delivery is still staged in `incoming/` is kept until a pass counts it
fn staged_delivery_keeps_its_entry_until_counted() {
    let _g = one_at_a_time();
    let (r, a, b) = pair();
    let _live = live(&b.paths, "sbob");
    let id = send(&a, T0 + 1);
    assert_eq!(sync(&a).pushed, 1);
    let typed = Typed::default();
    // Pass 1 stages and logs the delivery, then cannot move it into `new/` (not a folder from `after_log`): the
    // staged file and the pending entry are what a crash at `after_log` leaves, and this pass runs to its end.
    let new_dir = b.sp().mail.join(BOB).join("new");
    let back = DirBack(new_dir.clone());
    let rep = pass_with(&b, Box::new(NotDirAt { point: "after_log", dir: new_dir }), T0 + 1000, &typed).unwrap();
    drop(back);
    assert_eq!(rep.delivered, 0);
    assert!(typed.lock().unwrap().is_empty());
    assert_eq!(incoming(&b), vec![format!("{BOB}.{id}.json")]);
    assert_eq!(pending_wake(&b).get(&id.to_string()), Some(&serde_json::json!(BOB)),
        "the pass end removed the entry of a delivery still staged");
    // Pass 2: step 5 moves the staged file into `new/`, then the fetch fails, before the first round.
    {
        let _bare = BareBack(r.bare.clone());
        fs::rename(&r.bare, r.bare.with_extension("away")).unwrap();
        pass(&b, CrashAt::none(), T0 + 1100, &typed).unwrap_err();
    }
    assert_eq!(in_new(&b, id), 1);
    assert!(incoming(&b).is_empty());
    assert!(typed.lock().unwrap().is_empty());
    assert_eq!(pending_wake(&b).get(&id.to_string()), Some(&serde_json::json!(BOB)));
    // Pass 3 counts and wakes it; pass 4, past `wake_gap`, does neither.
    let rep = pass(&b, CrashAt::none(), T0 + 1200, &typed).unwrap();
    assert_eq!(rep.delivered, 1, "the message step 5 moved into new/ is not counted");
    assert_eq!(*typed.lock().unwrap(), wake_once(), "bob is not woken for it");
    assert!(pending_wake(&b).is_empty());
    let rep = pass(&b, CrashAt::none(), T0 + 1200 + WAKE_GAP + 1, &typed).unwrap();
    assert_eq!(rep.delivered, 0);
    assert_eq!(*typed.lock().unwrap(), wake_once());
    assert_eq!(in_new(&b, id), 1);
}

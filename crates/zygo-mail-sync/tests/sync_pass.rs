#![cfg(unix)]
//! MS-13: the sync (mail sync design S§8.3, S§8.6 "At commit", S§10.1; S§19 #1-#8, #16, #23). Two machines on one
//! local bare repository, the injected clock and probe, and a multiplexer stub. No test sleeps or reads the wall
//! clock: the secret pass gets an hour of budget, and a test that needs time passing steps the injected clock.
mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{advance, blob, commit_tree, git, live, plain, tip_file, tree_literally, ForgetClock, Machine, Remote, ALICE, BOB, WS_ID};
use serde_json::{json, Value};
use ulid::Ulid;
use zygo_config::{Config, LoadContext, MailConfig, SyncOff};
use zygo_core::paths::Paths;
use zygo_mail_sync::address::Address;
use zygo_mail_sync::env::{self, Crash, Env, Seam, Spawner, Verdict, Wake};
use zygo_mail_sync::registry::{read_cache, write_cache, MemberCache};
use zygo_mail_sync::state::{load, make_identity, mark_again, save, SyncPaths};
use zygo_mail_sync::sync::{run, run_with, run_with_deadline, Mode, Opts, SyncError, LOCK_WAIT, MAX_ATTEMPTS, OUTBOX_CAP, PASS_DEADLINE};
use zygo_redact::DeadlinePolicy;
use zygo_workspace::mail::wake::{after_send_with, Probe as IdleProbe, WAKE_TEXT};
use zygo_workspace::mux::{HookScope, Multiplexer, MuxError, NewSession, NewWindow, SessionAnswer, Split};
use zygo_workspace::state::{Phase, Store, Workspace};

/// Every test holds this lock: a lock-sensitive test for writing, every other test for reading. A child that another
/// thread of this process starts holds a copy of each open file description from its fork to its exec (O_CLOEXEC
/// closes the copy only at the exec), so an `flock` this process released can stay busy for that moment. With no
/// other test running, nothing spawns while a lock-sensitive test takes or retakes the sync lock (MS-13 round 2).
static SPAWNS: RwLock<()> = RwLock::new(());

/// The guard of a test that takes the sync lock in child mode, retakes it after a mark, takes it itself, or steps the
/// injected clock while it waits for it.
fn exclusive() -> RwLockWriteGuard<'static, ()> { SPAWNS.write().unwrap_or_else(|e| e.into_inner()) }

/// The guard of every other test: it may spawn while others of its kind run.
fn shared() -> RwLockReadGuard<'static, ()> { SPAWNS.read().unwrap_or_else(|e| e.into_inner()) }

/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;
const BRANCH: &str = "zygo/mail";

/// The injected clock: `now()` gives `UNIX_EPOCH + secs`, then adds `step` seconds (0: a fixed clock).
struct Clock(Arc<AtomicU64>, u64);
impl Clock {
    fn at(secs: u64) -> Clock { Clock(Arc::new(AtomicU64::new(secs)), 0) }
    fn stepping(secs: u64, step: u64) -> Clock { Clock(Arc::new(AtomicU64::new(secs)), step) }
}
impl env::Clock for Clock {
    fn now(&self) -> SystemTime { UNIX_EPOCH + Duration::from_secs(self.0.fetch_add(self.1, Ordering::SeqCst)) }
}

/// The injected public probe.
struct Says(Verdict);
impl env::Probe for Says {
    fn probe(&self, _: &str) -> Verdict { self.0.clone() }
}

type HookFn = Box<dyn FnMut(&'static str) -> Result<(), Crash> + Send>;
/// The test seam: a closure called at each named point.
struct Hook(Mutex<HookFn>);
impl Hook {
    fn none() -> Hook { Hook::new(|_| Ok(())) }
    fn new(f: impl FnMut(&'static str) -> Result<(), Crash> + Send + 'static) -> Hook { Hook(Mutex::new(Box::new(f))) }
    /// `Err(Crash(point))` at the first call at `point`.
    fn crash_at(point: &'static str) -> Hook {
        let mut done = false;
        Hook::new(move |p| if p == point && !done { done = true; Err(Crash(p)) } else { Ok(()) })
    }
}
impl Seam for Hook {
    fn at(&self, p: &'static str) -> Result<(), Crash> { (self.0.lock().unwrap())(p) }
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

/// A session `@zygo-ws` = [`WS_ID`] with one pane `%3` that holds `key`; each typed text is recorded.
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
impl StubWake {
    fn busy() -> StubWake { StubWake { idle: false, typed: Typed::default() } }
}
impl Wake for StubWake {
    fn wake(&self, paths: &Paths, ws: &Workspace, keys: &[String], cfg: &MailConfig, now: SystemTime) {
        for k in keys {
            let mux = StubMux { key: k.clone(), typed: self.typed.clone() };
            after_send_with(paths, ws, std::slice::from_ref(k), cfg, &mux, &Idle(self.idle, now), now);
        }
    }
}

fn env(m: &Machine, seam: Hook, clock: Clock, probe: Verdict, wake: StubWake, policy: DeadlinePolicy) -> Env {
    Env {
        paths: m.paths.clone(),
        clock: Box::new(clock),
        probe: Box::new(Says(probe)),
        seam: Box::new(seam),
        spawner: Box::new(NoSpawn),
        wake: Box::new(wake),
        secret_policy: policy,
    }
}

/// The default policy with an hour of budget: the secret pass never times out under machine load.
fn far_policy() -> DeadlinePolicy { DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() } }

fn plain_env(m: &Machine) -> Env { env(m, Hook::none(), Clock::at(T0), Verdict::Local, StubWake::busy(), far_policy()) }
fn hook_env(m: &Machine, h: Hook) -> Env { env(m, h, Clock::at(T0), Verdict::Local, StubWake::busy(), far_policy()) }
fn sync(m: &Machine) -> zygo_mail_sync::sync::Report { run(&plain_env(m), WS_ID, Mode::Cli).unwrap() }

/// The `imported` array of the ack file `path` at the tip; empty when absent.
fn imported(r: &Remote, path: &str) -> BTreeSet<String> {
    let Some(b) = tip_file(r, BRANCH, path) else { return BTreeSet::new() };
    let v: Value = serde_json::from_slice(&b).unwrap();
    v["imported"].as_array().unwrap().iter().map(|u| u.as_str().unwrap().to_string()).collect()
}

/// The blob id of `<tip>:<path>` in the bare repository.
fn blob_at(r: &Remote, path: &str) -> String {
    git(&r.bare, &["rev-parse", &format!("{}:{path}", r.tip(BRANCH).unwrap())])
}

/// Each message of `mail/<key>/new/`, parsed; empty when absent.
fn new_messages(m: &Machine, key: &str) -> Vec<Value> {
    let Ok(d) = fs::read_dir(m.sp().mail.join(key).join("new")) else { return Vec::new() };
    d.map(|e| serde_json::from_slice(&fs::read(e.unwrap().path()).unwrap()).unwrap()).collect()
}

fn new_count(m: &Machine, key: &str) -> usize { new_messages(m, key).len() }

/// One message from member `from` to `to.0@to.1` through the send path of MS-9, at `secs`.
fn send_at(m: &Machine, from: &str, to: (&str, &str), body: &str, secs: u64) -> Ulid {
    send_with(m, &config(m), from, to, body, secs)
}

/// The config of machine `m`, as the sync loads it.
fn config(m: &Machine) -> Config {
    zygo_config::load(&LoadContext { paths: m.paths.clone(), repo: Some(m.root.clone()), team: None }).unwrap().config
}

/// [`send_at`] with the config `cfg` of `m`.
fn send_with(m: &Machine, cfg: &Config, from: &str, to: (&str, &str), body: &str, secs: u64) -> Ulid {
    let s = zygo_config::sync_for(&cfg, "app");
    let to = Address { name: to.0.into(), host: to.1.into() };
    zygo_mail_sync::outbox::send_remote_with(&m.sp(), s.as_ref(), Some(from), &to, "status", body, &cfg.redaction,
        &m.root, UNIX_EPOCH + Duration::from_secs(secs), &far_policy())
        .unwrap()
}

/// [`send_at`] at `T0` plus a counter of this process, so the ULIDs of two sends differ in time.
fn send(m: &Machine, from: &str, to: (&str, &str), body: &str) -> Ulid {
    static N: AtomicU64 = AtomicU64::new(1);
    send_at(m, from, to, body, T0 + N.fetch_add(1, Ordering::SeqCst))
}

/// A remote and one machine `a` (host `a`, member `alice`), not synced.
fn one() -> (Remote, Machine) {
    let r = Remote::new();
    let a = Machine::new(&r);
    a.config(Some("a"), "");
    a.workspace(&[(ALICE, "alice")]);
    (r, a)
}

/// A remote, machine `a` (host `a`, member `alice`) and machine `b` (host `b`, member `bob`), not synced.
fn machines() -> (Remote, Machine, Machine) {
    let (r, a) = one();
    let b = Machine::new(&r);
    b.config(Some("b"), "");
    b.workspace(&[(BOB, "bob")]);
    (r, a, b)
}

/// [`machines`], then B, A and B sync, so each member cache knows the other host.
fn pair() -> (Remote, Machine, Machine) {
    let (r, a, b) = machines();
    sync(&b);
    sync(&a);
    sync(&b);
    (r, a, b)
}

/// A new commit on the bare tip: `adds` (path, bytes) put in and `removes` taken out of the tip's tree.
fn edit_tip(r: &Remote, adds: &[(&str, &[u8])], removes: &[&str]) {
    let tip = r.tip(BRANCH).unwrap();
    let idx = r.dir.path().join("edit-index");
    let _ = fs::remove_file(&idx);
    let g = |args: &[&str]| {
        let o = plain(&r.bare).env("GIT_INDEX_FILE", &idx).args(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim_end().to_string()
    };
    g(&["read-tree", &tip]);
    for (path, bytes) in adds {
        let b = blob(&r.bare, bytes);
        g(&["update-index", "--add", "--cacheinfo", &format!("100644,{b},{path}")]);
    }
    if !removes.is_empty() {
        // Mode 0 removes the entry; `--force-remove` needs a work tree, which a bare repository has not.
        let info: String = removes.iter().map(|p| format!("0 {}\t{p}\n", "0".repeat(40))).collect();
        let mut ch = plain(&r.bare).env("GIT_INDEX_FILE", &idx).args(["update-index", "--index-info"])
            .stdin(std::process::Stdio::piped()).spawn().unwrap();
        ch.stdin.take().unwrap().write_all(info.as_bytes()).unwrap();
        assert!(ch.wait().unwrap().success());
    }
    let tree = g(&["write-tree"]);
    let c = commit_tree(&r.bare, &tree, Some(&tip));
    git(&r.bare, &["update-ref", &format!("refs/heads/{BRANCH}"), &c]);
}

#[test]
fn spec_values() {
    let _g = shared();
    assert_eq!(
        (PASS_DEADLINE, MAX_ATTEMPTS, OUTBOX_CAP, LOCK_WAIT),
        (Duration::from_secs(120), 5, 1000, Duration::from_secs(30))
    );
    let stop = SyncError::Stopped { line: "x".into(), check: "push" };
    for (e, child, cli) in [(SyncError::Off(SyncOff::NotOn), 0, 1), (SyncError::Busy, 0, 1), (stop, 1, 1)] {
        assert_eq!((e.exit_code(Mode::Child), e.exit_code(Mode::Cli)), (child, cli));
    }
}

#[test] // S§8.3 step 4, S-C9 (FX-26 review round 2 F-3): the pass deadline seam bounds each pass; `run_with` keeps
        // `PASS_DEADLINE`
fn pass_deadline_seam_bounds_the_pass() {
    let _g = shared();
    let (r, a, _b) = pair();
    const FAR: Duration = Duration::from_secs(3600);
    // An env whose injected clock reads `from` until the seam point `after_fetch` (after `pass_end` is set, before the
    // outbox step), then `to`.
    let jump = |from: u64, to: u64| {
        let now = Arc::new(AtomicU64::new(from));
        let n = Arc::clone(&now);
        let h = Hook::new(move |p| {
            if p == "after_fetch" {
                n.store(to, Ordering::SeqCst);
            }
            Ok(())
        });
        env(&a, h, Clock(now, 0), Verdict::Local, StubWake::busy(), far_policy())
    };
    // FX-26 re-plan review round 1 F1: the `Instant` half. The seam deadline is zero, and the injected clock can never
    // reach `pass_end`: it reads `T0 + FAR` when `pass` sets `pass_end` (so `pass_end` is `T0 + FAR`) and goes back to
    // `T0` at `after_fetch`, before the two `clock.now() >= pass_end` checks of the outbox step. So only the `Instant`
    // bound can stop the pass. It stops at the pass's first git call, `git remote` of the public check of the first
    // pass (`public_check`, S§8.2 step 1, `stop("remote")`), before any fetch: the outbox is kept and the tip does
    // not move.
    let id = send(&a, "alice", ("bob", "b"), "hello");
    let tip0 = git(&r.bare, &["rev-parse", &format!("refs/heads/{BRANCH}")]);
    let e = run_with_deadline(&jump(T0 + FAR.as_secs(), T0), WS_ID, Mode::Cli, &Opts::default(), Duration::ZERO).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: mail sync stopped (remote); the outbox is kept".into(), check: "remote" });
    assert_eq!(git(&r.bare, &["rev-parse", &format!("refs/heads/{BRANCH}")]), tip0);
    assert!(a.sp().outbox.join(format!("{id}.json")).is_file());
    assert_eq!(run_with(&plain_env(&a), WS_ID, Mode::Cli, &Opts::default()).unwrap().pushed, 1);
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_some());
    // FX-26 R3-4: the injected-clock half. The seam gives a far deadline, so the `Instant` bound never ends the pass;
    // the injected clock moves at `after_fetch`. Moved past the seam's deadline, the pass stops at the `pass_end` check
    // of the outbox step (`stop("deadline")`), before the secret check: the outbox is kept and the tip does not move.
    // Moved past `PASS_DEADLINE` but not past the seam's deadline, the pass goes on and pushes.
    let id2 = send(&a, "alice", ("bob", "b"), "hello again");
    let tip1 = git(&r.bare, &["rev-parse", &format!("refs/heads/{BRANCH}")]);
    let e = run_with_deadline(&jump(T0, T0 + FAR.as_secs() + 1), WS_ID, Mode::Cli, &Opts::default(), FAR).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: mail sync stopped (deadline); the outbox is kept".into(), check: "deadline" });
    assert_eq!(git(&r.bare, &["rev-parse", &format!("refs/heads/{BRANCH}")]), tip1);
    assert!(a.sp().outbox.join(format!("{id2}.json")).is_file());
    let rep = run_with_deadline(&jump(T0, T0 + PASS_DEADLINE.as_secs() + 1), WS_ID, Mode::Cli, &Opts::default(), FAR).unwrap();
    assert_eq!(rep.pushed, 1);
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id2}.json")).is_some());
    // FX-26 re-plan 2 review round 3 F2: the seam's deadline is the one it is given, pinned with a middle value (600 s)
    // from both sides. The clock at `T0 + 599 s` at the `pass_end` check: no stop, and the pass pushes; at `T0 + 601 s`:
    // it stops with the check `deadline`. A seam that scales or bounds its argument fails one of the two.
    const MID: Duration = Duration::from_secs(600);
    let id_mid = send(&a, "alice", ("bob", "b"), "hello in the middle");
    let tip_mid = git(&r.bare, &["rev-parse", &format!("refs/heads/{BRANCH}")]);
    let e = run_with_deadline(&jump(T0, T0 + MID.as_secs() + 1), WS_ID, Mode::Cli, &Opts::default(), MID).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: mail sync stopped (deadline); the outbox is kept".into(), check: "deadline" });
    assert_eq!(git(&r.bare, &["rev-parse", &format!("refs/heads/{BRANCH}")]), tip_mid);
    assert!(a.sp().outbox.join(format!("{id_mid}.json")).is_file());
    let rep = run_with_deadline(&jump(T0, T0 + MID.as_secs() - 1), WS_ID, Mode::Cli, &Opts::default(), MID)
        .expect("the seam stopped before its deadline");
    assert_eq!(rep.pushed, 1);
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id_mid}.json")).is_some());
    assert!(!a.sp().outbox.join(format!("{id_mid}.json")).exists());
    // FX-26 re-plan 2 review round 1 F6: `run_with` keeps `PASS_DEADLINE`. The same clock move past `T0 +
    // PASS_DEADLINE` that the far seam deadline lets through stops `run_with` at the `pass_end` check.
    let id3 = send(&a, "alice", ("bob", "b"), "hello a third time");
    let tip2 = git(&r.bare, &["rev-parse", &format!("refs/heads/{BRANCH}")]);
    let e = run_with(&jump(T0, T0 + PASS_DEADLINE.as_secs() + 1), WS_ID, Mode::Cli, &Opts::default()).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: mail sync stopped (deadline); the outbox is kept".into(), check: "deadline" });
    assert_eq!(git(&r.bare, &["rev-parse", &format!("refs/heads/{BRANCH}")]), tip2);
    assert!(a.sp().outbox.join(format!("{id3}.json")).is_file());
    // FX-26 re-plan 2 review round 2 F2: the lower bound. With the injected clock at `T0 + PASS_DEADLINE - 1 s` at
    // the `pass_end` check, `run_with` does not stop (`deadline`): the pass pushes the kept message. So `run_with`'s
    // pass deadline is `PASS_DEADLINE`, not less and (above) not more.
    let rep = run_with(&jump(T0, T0 + PASS_DEADLINE.as_secs() - 1), WS_ID, Mode::Cli, &Opts::default())
        .expect("run_with stopped before PASS_DEADLINE");
    assert_eq!(rep.pushed, 1);
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id3}.json")).is_some());
    assert!(!a.sp().outbox.join(format!("{id3}.json")).exists());
}

#[test] // S§19 #1, S-G1, S§7.3, S§14 Normal, S-C4
fn normal_message_arrives_with_bytes_unchanged() {
    let _g = shared();
    let (r, a, b) = pair();
    assert_eq!(read_cache(&a.sp()).hosts, BTreeMap::from([("b".to_string(), vec!["bob".to_string()])]));
    let id = send(&a, "alice", ("bob", "b"), "hello");
    let ra = sync(&a);
    assert_eq!((ra.passes, ra.pushed), (1, 1));
    let rb = sync(&b);
    assert_eq!(rb.delivered, 1);
    let pushed = tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).unwrap();
    let got = fs::read(b.sp().mail.join(BOB).join("new").join(format!("{id}.json"))).unwrap();
    assert_eq!(got, pushed);
    let m: Value = serde_json::from_slice(&got).unwrap();
    assert_eq!((m["from"].as_str(), m["to"].as_str()), (Some("alice@a"), Some("bob@b")));
    assert!(imported(&r, "ack/b.json").contains(&id.to_string()));
    assert!(fs::read_dir(a.sp().outbox).unwrap().all(|e| !e.unwrap().path().is_file()));
    assert_eq!((common::gpg_calls(), common::ssh_calls()), (0, 0));
}

/// The parts of the user repository that the sync must not change (S-G2).
#[derive(Debug, PartialEq)]
struct Snap {
    head: String,
    branches: String,
    status: String,
    index: Vec<u8>,
    fetch_head: Option<Vec<u8>>,
    packs: BTreeSet<String>,
    worktrees: String,
}

fn snap(root: &Path) -> Snap {
    let packs = fs::read_dir(root.join(".git/objects/pack"))
        .map(|d| d.map(|e| e.unwrap().file_name().into_string().unwrap()).collect())
        .unwrap_or_default();
    Snap {
        head: git(root, &["rev-parse", "HEAD"]),
        branches: git(root, &["for-each-ref", "refs/heads"]),
        status: git(root, &["status", "--porcelain"]),
        index: fs::read(root.join(".git/index")).unwrap(),
        fetch_head: fs::read(root.join(".git/FETCH_HEAD")).ok(),
        packs,
        worktrees: git(root, &["worktree", "list", "--porcelain"]),
    }
}

/// The count of `.pack` files of the repository at `root`.
fn pack_count(root: &Path) -> usize {
    fs::read_dir(root.join(".git/objects/pack")).unwrap()
        .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().ends_with(".pack")).count()
}

/// SP6 setup (MS-25 review F1): a user commit packed into a second pack beside the clone's one, then `gc.auto = 1`,
/// `gc.autoPackLimit = 1` and a foreground gc and maintenance, so git's own estimate (more packs than the limit)
/// starts an automatic gc at the next command that runs one, such as `fetch`.
fn gc_due(root: &Path) {
    fs::write(root.join("work.txt"), "w\n").unwrap();
    git(root, &["add", "work.txt"]);
    git(root, &["-c", "gc.auto=0", "commit", "-q", "-m", "work"]);
    git(root, &["-c", "gc.auto=0", "repack", "-q", "-d"]);
    for (k, v) in [("gc.auto", "1"), ("gc.autoPackLimit", "1"), ("gc.autoDetach", "false"),
        ("maintenance.autoDetach", "false")] {
        git(root, &["config", k, v]);
    }
    assert_eq!(pack_count(root), 2);
}

#[test] // S§19 #2, S-G2, SP6
fn user_repository_is_untouched() {
    let _g = shared();
    let (r, a, b) = machines();
    // Control: in the same setup, a plain `git fetch` (no overrides) starts the automatic gc, which repacks the two
    // packs into one.
    let c = Machine::new(&r);
    gc_due(&c.root);
    let o = plain(&c.root).args(["fetch", "-q"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(pack_count(&c.root), 1, "the control's plain fetch started no automatic gc");
    for m in [&a, &b] {
        gc_due(&m.root);
    }
    let before = [snap(&a.root), snap(&b.root)];
    sync(&b);
    sync(&a);
    sync(&b);
    let id = send(&a, "alice", ("bob", "b"), "hello");
    sync(&a);
    assert_eq!(sync(&b).delivered, 1);
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_some());
    for (m, s0) in [&a, &b].into_iter().zip(before) {
        let s = snap(&m.root);
        assert_eq!((&s.head, &s.branches, &s.status), (&s0.head, &s0.branches, &s0.status));
        assert_eq!(s.index, s0.index);
        assert_eq!(s.fetch_head, s0.fetch_head);
        assert_eq!(s.packs, s0.packs);
        assert_eq!(pack_count(&m.root), 2, "an automatic gc ran in the user repository");
        assert!(!m.root.join(".git/gc.log").exists());
        let added = s.worktrees.strip_prefix(&s0.worktrees).unwrap();
        let blocks: Vec<&str> = added.split("\n\n").map(str::trim).filter(|b| !b.is_empty()).collect();
        assert_eq!(blocks.len(), 1, "{added}");
        assert!(blocks[0].starts_with(&format!("worktree {}\n", m.wt.display())), "{added}");
        assert!(blocks[0].lines().any(|l| l == "detached"), "{added}");
    }
}

#[test] // S§19 #8a, S§6.4
fn bootstrap_makes_the_branch() {
    let _g = shared();
    let (r, a) = one();
    assert_eq!(r.tip(BRANCH), None);
    sync(&a);
    assert_eq!(tip_file(&r, BRANCH, "ZYGO-MAIL").unwrap(), b"1\n");
    assert!(tip_file(&r, BRANCH, "registry/a.json").is_some());
    assert!(a.wt.join(".git").is_file());
}

/// Writes the member cache of `m` with the one host `host` and its one member `name`.
fn knows(m: &Machine, host: &str, name: &str) {
    let mut c = MemberCache::default();
    c.hosts.insert(host.into(), vec![name.into()]);
    write_cache(&m.sp(), &c).unwrap();
}

#[test] // S§19 #8b
fn two_bootstraps_give_one_root() {
    let _g = exclusive();
    let (r, a, b) = machines();
    knows(&a, "b", "bob");
    knows(&b, "a", "alice");
    let ida = send(&a, "alice", ("bob", "b"), "from a");
    let idb = send(&b, "bob", ("alice", "a"), "from b");
    let b = Arc::new(b);
    let b2 = b.clone();
    let mut done = false;
    let h = Hook::new(move |p| {
        if p == "before_push" && !done {
            done = true;
            run(&plain_env(&b2), WS_ID, Mode::Child).unwrap();
        }
        Ok(())
    });
    run(&hook_env(&a, h), WS_ID, Mode::Cli).unwrap();
    let tip = r.tip(BRANCH).unwrap();
    assert_eq!(git(&r.bare, &["rev-list", "--max-parents=0", &tip]).lines().count(), 1);
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{ida}.json")).is_some());
    assert!(tip_file(&r, BRANCH, &format!("alice@a/{idb}.json")).is_some());
    assert_eq!(git(&r.bare, &["for-each-ref", "--format=%(refname)", "refs/heads/zygo/"]), "refs/heads/zygo/mail");
}

#[test] // S§19 #7a, S§8.3 step 7, S§14 Hostile branch
fn branch_without_marker_stops() {
    let _g = shared();
    let (r, a) = one();
    let readme = blob(&r.bare, b"x\n");
    let tree = tree_literally(&r.bare, &[("100644", b"README", &readme)]);
    let root = commit_tree(&r.bare, &tree, None);
    git(&r.bare, &["update-ref", &format!("refs/heads/{BRANCH}"), &root]);
    let e = run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: the side branch is not a zygo mail branch".into(), check: "marker" });
    assert_eq!(e.exit_code(Mode::Cli), 1);
    assert_eq!(r.tip(BRANCH).unwrap(), root);
    assert!(a.errors().contains("mail sync: stopped: marker"));
    assert_eq!(load(&a.sp()).unwrap().last_error.as_deref(), Some("marker"));
}

#[test] // S§19 #7b, S§8.3 step 1, S§7.1
fn no_host_is_no_sync() {
    let _g = shared();
    let (r, a) = one();
    a.config(None, "");
    let e = run(&plain_env(&a), WS_ID, Mode::Child).unwrap_err();
    assert_eq!(e, SyncError::Off(SyncOff::NoHost));
    assert_eq!((e.exit_code(Mode::Child), e.exit_code(Mode::Cli)), (0, 1));
    assert!(!a.sp().again.exists() && !a.sp().lock.exists());
    assert_eq!(r.tip(BRANCH), None);
    fs::write(&a.paths.config_file, "[mail]\nhost = \"a\"\n[mail.workspace.app]\nsync = \"main\"\n").unwrap();
    assert_eq!(run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err(), SyncError::Off(SyncOff::BadBranch));
}

/// A registry file of host `host` and identity `identity` with `members`, in the S§7.3 form.
fn registry_json(host: &str, identity: &str, members: &[&str]) -> Vec<u8> {
    serde_json::to_vec(&json!({"version": 1, "host": host, "identity": identity, "members": members})).unwrap()
}

#[test] // S§14 Hostile branch, S§7.3
fn bad_registry_is_not_in_the_member_cache() {
    let _g = shared();
    let (r, a, _b) = pair();
    edit_tip(&r, &[("registry/c.json", &registry_json("d", &"0".repeat(32), &["carol"]))], &[]);
    sync(&a);
    let c = read_cache(&a.sp());
    assert!(!c.hosts.contains_key("c") && !c.hosts.contains_key("d"));
    assert!(a.errors().contains("mail sync: registry file refused: check host"));
}

#[test] // review focus 47, S§7.4, S§7.5 own hosts
fn own_ack_follows_the_tip_across_own_hosts() {
    let _g = shared();
    let (r, a, b) = pair();
    let u1 = send(&a, "alice", ("bob", "b"), "one");
    sync(&a);
    sync(&b);
    assert_eq!(imported(&r, "ack/b.json"), BTreeSet::from([u1.to_string()]));
    b.config(Some("b2"), "");
    let u2 = send(&a, "alice", ("bob", "b"), "two");
    sync(&a);
    sync(&b);
    assert_eq!(imported(&r, "ack/b.json"), BTreeSet::from([u1.to_string(), u2.to_string()]));
    assert!(tip_file(&r, BRANCH, "ack/b2.json").is_some());
    assert!(imported(&r, "ack/b2.json").is_empty());
    edit_tip(&r, &[], &[&format!("bob@b/{u1}.json")]);
    sync(&b);
    assert_eq!(imported(&r, "ack/b.json"), BTreeSet::from([u2.to_string()]));
    assert_eq!(new_count(&b, BOB), 2);
}

#[test] // review focus 51, S§7.5, S-C2
fn collision_line_goes_to_the_caller_only() {
    let _g = exclusive();
    let (r, a, _b) = pair();
    let other = SyncPaths::new(&a.paths, "fedcba9876543210").unwrap();
    fs::create_dir_all(other.mail.parent().unwrap()).unwrap(); // the workspace exists (MS-21 review R2-1)
    let x = make_identity(&other).unwrap();
    edit_tip(&r, &[("registry/a.json", &registry_json("a", &x, &["alice"]))], &[]);
    for mode in [Mode::Child, Mode::Cli] {
        let SyncError::Stopped { line, check } = run(&plain_env(&a), WS_ID, mode).unwrap_err() else { panic!() };
        assert_eq!(check, "collision");
        assert!(line.ends_with("The workspace state fedcba9876543210 on this machine also holds that identity."), "{line}");
    }
    let log = a.errors();
    assert!(log.contains("mail sync: stopped: collision"));
    assert!(!log.contains("fedcba9876543210") && !log.contains("alice"), "{log}");
    assert_eq!(new_count(&a, ALICE), 0);
}

#[test] // MS-11 F9, S§7.5
fn accepted_push_not_recorded_is_no_twin() {
    let _g = shared();
    let (r, a, _b) = pair();
    a.workspace(&[(ALICE, "alice"), ("kcarol222", "carol")]);
    let e = run(&hook_env(&a, Hook::crash_at("after_push")), WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: stopped at seam after_push".into(), check: "seam" });
    a.workspace(&[(ALICE, "alice"), ("kcarol222", "carol"), ("kdave2222", "dave")]);
    let r2 = Arc::new(r);
    let r3 = r2.clone();
    let mut done = false;
    let h = Hook::new(move |p| {
        if p == "before_push" && !done {
            done = true;
            advance(&r3, BRANCH);
        }
        Ok(())
    });
    run(&hook_env(&a, h), WS_ID, Mode::Cli).unwrap();
    let st = load(&a.sp()).unwrap();
    assert_eq!(st.pushed.get("registry/a.json"), Some(&blob_at(&r2, "registry/a.json")));
    assert!(st.pending.is_empty());
    let reg: Value = serde_json::from_slice(&tip_file(&r2, BRANCH, "registry/a.json").unwrap()).unwrap();
    assert_eq!(reg["members"], json!(["alice", "carol", "dave"]));
}

#[test] // S§14 "Crash after B's delivery, before its ack push"
fn crash_before_ack_push_delivers_once() {
    let _g = shared();
    let (r, a, b) = pair();
    let u1 = send(&a, "alice", ("bob", "b"), "one");
    sync(&a);
    run(&hook_env(&b, Hook::crash_at("before_push")), WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(new_count(&b, BOB), 1);
    sync(&b);
    assert_eq!(new_count(&b, BOB), 1);
    assert_eq!(imported(&r, "ack/b.json"), BTreeSet::from([u1.to_string()]));
}

#[test] // S§14 "Crash during a commit"
fn crash_during_commit_leaves_no_trace() {
    let _g = shared();
    let (r, a, _b) = pair();
    let x = blob(&a.wt, b"x");
    git(&a.wt, &["update-index", "--add", "--cacheinfo", &format!("100644,{x},junk.json")]);
    let id = send(&a, "alice", ("bob", "b"), "hi");
    sync(&a);
    assert!(tip_file(&r, BRANCH, "junk.json").is_none());
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_some());
}

#[test] // S§14 "No network, authentication fails", S§8.3 step 15
fn no_network_keeps_the_outbox() {
    let _g = shared();
    let (r, a, _b) = pair();
    git(&a.root, &["remote", "set-url", "origin", "file:///nonexistent/x.git"]);
    let id = send(&a, "alice", ("bob", "b"), "hi");
    let e = run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: mail sync stopped (fetch); the outbox is kept".into(), check: "fetch" });
    assert!(a.sp().outbox.join(format!("{id}.json")).is_file());
    let log = a.errors();
    assert!(log.contains("mail sync: stopped: fetch") && !log.contains("nonexistent"));
    git(&a.root, &["remote", "set-url", "origin", &r.url]);
    assert_eq!(sync(&a).pushed, 1);
}

#[test] // S§19 #4 (commit part), S§8.6 "At commit"
fn secret_found_at_commit_is_refused_with_a_notice() {
    let _g = shared();
    let (r, a, _b) = pair();
    let id = send(&a, "alice", ("bob", "b"), "say zz-forbidden-zz now");
    a.config(Some("a"), "[[redaction.pattern]]\nid = \"ship_word\"\npattern = \"zz-forbidden-zz\"\naction = \"redact\"\ndescription = \"d\"\n");
    sync(&a);
    assert!(a.sp().refused.join(format!("{id}.json")).is_file());
    assert!(!a.sp().outbox.join(format!("{id}.json")).exists());
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_none());
    let log = a.errors();
    let line = log.lines().find(|l| l.contains(&id.to_string())).unwrap();
    assert!(line.ends_with(&format!("mail sync: outbox {id}: refused: ship_word")), "{line}");
    assert!(!log.contains("zz-forbidden-zz") && !log.contains("bob@b"), "{log}");
    let notes = new_messages(&a, ALICE);
    let n = notes.iter().find(|m| m["from"] == "zygo").unwrap();
    assert_eq!(n["to"], "alice");
    assert_eq!(n["subject"], format!("not sent: {id}"));
    assert_eq!(n["body"], format!("zygo did not send message {id} to bob@b: it holds a secret (ship_word). The message is kept on this machine in mail/outbox/refused/."));
}

#[test] // S§19 #5
fn secret_check_that_cannot_run_keeps_the_message() {
    let _g = shared();
    let (r, a, _b) = pair();
    let id = send(&a, "alice", ("bob", "b"), "hi");
    let expire = DeadlinePolicy { expire_after_scans: Some(0), base: Duration::from_secs(3600), ..Default::default() };
    run(&env(&a, Hook::none(), Clock::at(T0), Verdict::Local, StubWake::busy(), expire), WS_ID, Mode::Cli).unwrap();
    assert!(a.sp().outbox.join(format!("{id}.json")).is_file());
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_none());
    assert!(a.errors().contains(&format!("mail sync: outbox {id}: secret check could not run: secret_timeout")));
    assert!(new_messages(&a, ALICE).is_empty());
    let store = zygo_redact::act_lite_store::store_path(&a.root, &zygo_redact::act_lite_store::Lookup::system());
    fs::create_dir_all(store.parent().unwrap()).unwrap();
    fs::write(&store, "not an assignment\n").unwrap();
    sync(&a);
    assert!(a.errors().contains(&format!("mail sync: outbox {id}: secret check could not run: act_lite_store")));
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_none() && new_messages(&a, ALICE).is_empty());
    fs::remove_file(&store).unwrap();
    assert_eq!(sync(&a).pushed, 1);
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_some());
}

#[test] // S§8.3 step 11 first case, S§14 "Crash after A's push, before the outbox delete"
fn path_already_at_the_tip_leaves_the_outbox() {
    let _g = shared();
    let (r, a, _b) = pair();
    let id = send(&a, "alice", ("bob", "b"), "hi");
    run(&hook_env(&a, Hook::crash_at("before_outbox_delete")), WS_ID, Mode::Cli).unwrap_err();
    assert!(a.sp().outbox.join(format!("{id}.json")).is_file());
    assert_eq!(sync(&a).pushed, 0);
    assert!(!a.sp().outbox.join(format!("{id}.json")).exists());
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_some());
}

#[test] // S§8.3 step 11
fn outbox_cap_is_1000_per_pass() {
    let _g = shared();
    let (_r, a, _b) = pair();
    let cfg = config(&a);
    for i in 0..1001 {
        send_with(&a, &cfg, "alice", ("bob", "b"), "n", T0 + 10_000 + i);
    }
    assert_eq!(sync(&a).pushed, 1000);
    assert_eq!(zygo_mail_sync::outbox::count(&a.sp()).unwrap(), 1);
    assert_eq!(sync(&a).pushed, 1);
}

/// A hook that counts the calls at each point in `counts`, then runs `f`.
fn counting(counts: Arc<Mutex<BTreeMap<&'static str, usize>>>, mut f: impl FnMut(&'static str, usize) + Send + 'static)
    -> Hook {
    Hook::new(move |p| {
        let n = {
            let mut c = counts.lock().unwrap();
            let n = c.entry(p).or_default();
            *n += 1;
            *n
        };
        f(p, n);
        Ok(())
    })
}

#[test] // S§19 #3a, S§14 "A and B push at the same time"
fn push_race_retries_on_the_new_tip() {
    let _g = exclusive();
    let (r, a, b) = pair();
    let ia = send(&a, "alice", ("bob", "b"), "from a");
    let ib = send(&b, "bob", ("alice", "a"), "from b");
    let counts = Arc::new(Mutex::new(BTreeMap::new()));
    let b = Arc::new(b);
    let b2 = b.clone();
    let h = counting(counts.clone(), move |p, n| {
        if p == "before_push" && n == 1 {
            run(&plain_env(&b2), WS_ID, Mode::Child).unwrap();
        }
    });
    let rep = run(&hook_env(&a, h), WS_ID, Mode::Cli).unwrap();
    let c = counts.lock().unwrap();
    assert_eq!((rep.passes, c["after_fetch"], c["before_push"]), (1, 2, 2));
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{ia}.json")).is_some());
    assert!(tip_file(&r, BRANCH, &format!("alice@a/{ib}.json")).is_some());
}

#[test] // S§19 #3b, S§8.3 step 15
fn five_moves_stop_with_the_outbox_kept() {
    let _g = shared();
    let (r, a, _b) = pair();
    let ia = send(&a, "alice", ("bob", "b"), "from a");
    let counts = Arc::new(Mutex::new(BTreeMap::new()));
    let r = Arc::new(r);
    let r2 = r.clone();
    let h = counting(counts.clone(), move |p, _| {
        if p == "before_push" {
            advance(&r2, BRANCH);
        }
    });
    let e = run(&hook_env(&a, h), WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: the side branch moved 5 times during the sync; try again".into(), check: "moved" });
    assert_eq!(e.exit_code(Mode::Cli), 1);
    assert_eq!(counts.lock().unwrap()["before_push"], 5);
    assert!(a.sp().outbox.join(format!("{ia}.json")).is_file());
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{ia}.json")).is_none());
}

#[test] // S§19 #6 public, S§8.2, S§8.3 step 5
fn public_remote_stops_before_any_fetch() {
    let _g = shared();
    let (r, a) = one();
    git(&a.root, &["remote", "set-url", "--push", "origin", "https://example.invalid/r.git"]);
    let refs0 = git(&r.bare, &["for-each-ref"]);
    let e = run(&env(&a, Hook::none(), Clock::at(T0), Verdict::Public, StubWake::busy(), far_policy()), WS_ID, Mode::Cli)
        .unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: the remote can be read without a login; mail sync refused".into(), check: "public" });
    assert_eq!(e.exit_code(Mode::Cli), 1);
    assert_eq!(git(&r.bare, &["for-each-ref"]), refs0);
    assert!(!a.root.join(".git/refs/remotes/origin/zygo").exists());
    assert!(!a.wt.exists());
}

/// The injected public probe that records each URL it is asked and says `Public`.
struct Asked(Arc<Mutex<Vec<String>>>);
impl env::Probe for Asked {
    fn probe(&self, url: &str) -> Verdict {
        self.0.lock().unwrap().push(url.to_string());
        Verdict::Public
    }
}

#[test] // FX-40 (S-F1), S§8.2 step 1: `git push` sends to every push URL, so a public second push URL stops the pass
fn second_push_url_that_is_public_refuses_the_pass() {
    let _g = shared();
    let (r, a) = one();
    let fetch = git(&a.root, &["remote", "get-url", "origin"]);
    git(&a.root, &["remote", "set-url", "--add", "--push", "origin", &fetch]);
    git(&a.root, &["remote", "set-url", "--add", "--push", "origin", "https://example.invalid/mirror.git"]);
    let refs0 = git(&r.bare, &["for-each-ref"]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut e = plain_env(&a);
    e.probe = Box::new(Asked(asked.clone()));
    let err = run(&e, WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(err, SyncError::Stopped { line: "zygo: the remote can be read without a login; mail sync refused".into(), check: "public" });
    assert_eq!(*asked.lock().unwrap(), vec!["https://example.invalid/mirror.git".to_string()]);
    assert_eq!(git(&r.bare, &["for-each-ref"]), refs0);
    assert!(!a.root.join(".git/refs/remotes/origin/zygo").exists());
    assert!(!a.wt.exists());
}

#[test] // FX-40 review F1, S§8.3 step 15: two `file://` push URLs, the second moved: the first takes the commit and the
        // second refuses it; the pass stops with the `push` line (exit 1), keeps the outbox and records no own blob as
        // pushed, and does not report success
fn push_taken_by_one_push_url_only_stops_the_pass() {
    let _g = shared();
    let (r, a, _b) = pair();
    let r2 = r.mirror();
    common::push_urls(&a, &[&r.url, &r2.url]);
    let moved = common::advance(&r2, BRANCH);
    let id = send(&a, "alice", ("bob", "b"), "two urls");
    let pushed0 = load(&a.sp()).unwrap().pushed;
    let e = run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: mail sync stopped (push); the outbox is kept".into(), check: "push" });
    assert_eq!(e.exit_code(Mode::Cli), 1);
    assert!(a.sp().outbox.join(format!("{id}.json")).is_file());
    let st = load(&a.sp()).unwrap();
    assert_eq!(st.pushed, pushed0);
    assert!(!st.pending.is_empty());
    assert_eq!(st.last_error.as_deref(), Some("push"));
    assert!(a.errors().contains("push"), "{}", a.errors());
    assert_eq!(r2.tip(BRANCH), Some(moved));
}

/// The `push` stop of S§8.3 step 15.
fn push_stop() -> SyncError {
    SyncError::Stopped { line: "zygo: mail sync stopped (push); the outbox is kept".into(), check: "push" }
}

#[test] // FX-40 review, S§8.3 steps 11 and 15: after a split stop the flag is set; a pass with nothing new keeps the
        // outbox file and pushes the tip again (a split again: the `push` stop, flag kept); when the second URL
        // takes it, the flag clears, the outbox file is deleted and both URLs hold the message
fn split_flag_keeps_the_outbox_and_pushes_the_tip_until_every_url_takes_it() {
    let _g = shared();
    let (r, a, _b) = pair();
    let r2 = r.mirror();
    common::push_urls(&a, &[&r.url, &r2.url]);
    let before = r2.tip(BRANCH).unwrap();
    common::advance(&r2, BRANCH);
    let id = send(&a, "alice", ("bob", "b"), "two urls");
    let outbox_file = a.sp().outbox.join(format!("{id}.json"));
    let path = format!("bob@b/{id}.json");
    assert_eq!(run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err(), push_stop());
    assert!(load(&a.sp()).unwrap().push_split);
    assert!(tip_file(&r, BRANCH, &path).is_some() && tip_file(&r2, BRANCH, &path).is_none());
    // the second URL still refuses: the tip is pushed again, split again
    assert_eq!(run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err(), push_stop());
    assert!(outbox_file.is_file());
    assert!(load(&a.sp()).unwrap().push_split);
    // the second URL is back where this machine knew it: the tip push is a fast-forward there
    git(&r2.bare, &["update-ref", &format!("refs/heads/{BRANCH}"), &before]);
    let rep = run(&plain_env(&a), WS_ID, Mode::Cli).unwrap();
    assert_eq!(rep.passes, 1);
    let st = load(&a.sp()).unwrap();
    assert!(!st.push_split);
    assert!(st.pending.is_empty());
    assert!(!outbox_file.exists());
    assert_eq!(r2.tip(BRANCH), r.tip(BRANCH));
    assert!(tip_file(&r2, BRANCH, &path).is_some());
}

#[test] // FX-40 review, S§9 Push: a pass with the split flag set does not prune: a forced prune pass pushes the tip
        // again, stops with the `push` stop, and no URL's history is rewritten
fn split_flag_stops_the_prune() {
    let _g = shared();
    let (r, a, _b) = pair();
    let r2 = r.mirror();
    common::push_urls(&a, &[&r.url, &r2.url]);
    let moved = common::advance(&r2, BRANCH);
    send(&a, "alice", ("bob", "b"), "two urls");
    assert_eq!(run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err(), push_stop());
    let tip = r.tip(BRANCH).unwrap();
    let e = run_with(&plain_env(&a), WS_ID, Mode::Cli, &Opts { prune: true }).unwrap_err();
    assert_eq!(e, push_stop());
    assert_eq!(r.tip(BRANCH), Some(tip));
    assert_eq!(r2.tip(BRANCH), Some(moved));
    assert!(load(&a.sp()).unwrap().push_split);
}

/// A valid message file for `bob@b` from `alice@a`, named by `id`.
fn bob_message(id: Ulid) -> Vec<u8> {
    format!(r#"{{"version":1,"id":"{id}","from":"alice@a","to":"bob@b","sent_at":"2026-11-25T10:00:00Z","subject":"s","body":"b"}}"#)
        .into_bytes()
}

/// `n` messages for `bob@b` put on the tip, with ULID time `secs` and random parts `from..from + n`.
fn bob_messages_on_tip(r: &Remote, secs: u64, from: u128, n: u128) -> Vec<Ulid> {
    let ids: Vec<Ulid> = (from..from + n).map(|i| Ulid::from_parts(secs * 1000, i)).collect();
    let files: Vec<(String, Vec<u8>)> = ids.iter().map(|id| (format!("bob@b/{id}.json"), bob_message(*id))).collect();
    let adds: Vec<(&str, &[u8])> = files.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect();
    edit_tip(r, &adds, &[]);
    ids
}

#[test] // FX-40 (S-F2), S§8.5, S§19 #13: the 500 candidates are counted over every push attempt of the pass
fn one_pass_reads_at_most_500_candidates_across_moved_attempts() {
    let _g = shared();
    let (r, _a, b) = pair();
    b.config(Some("b\"\nmax_unread = 5000\n#"), "");
    let late = bob_messages_on_tip(&r, T0 + 5000, 0, 600);
    let counts = Arc::new(Mutex::new(BTreeMap::new()));
    let r = Arc::new(r);
    let r2 = r.clone();
    let early = Arc::new(Mutex::new(Vec::new()));
    let early2 = early.clone();
    let h = counting(counts.clone(), move |p, n| {
        if p == "before_push" && n == 1 {
            // older ULIDs: they sort before the 500 that the first attempt took
            *early2.lock().unwrap() = bob_messages_on_tip(&r2, T0 + 1000, 0, 10);
        }
    });
    let rep = run(&hook_env(&b, h), WS_ID, Mode::Cli).unwrap();
    assert_eq!(counts.lock().unwrap()["before_push"], 2);
    assert_eq!(rep.delivered, 500);
    let got: BTreeSet<String> = new_messages(&b, BOB).iter().map(|v| v["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(got.len(), 500);
    let first: BTreeSet<String> = late[..500].iter().map(|u| u.to_string()).collect();
    assert_eq!(got, first);
    assert_eq!(imported(&r, "ack/b.json"), first);
    let rep = sync(&b);
    assert_eq!(rep.delivered, 110);
    let got: Vec<String> = new_messages(&b, BOB).iter().map(|v| v["id"].as_str().unwrap().to_string()).collect();
    let all: BTreeSet<String> = late.iter().chain(early.lock().unwrap().iter()).map(|u| u.to_string()).collect();
    assert_eq!(got.len(), 610);
    assert_eq!(got.into_iter().collect::<BTreeSet<_>>(), all);
}

#[test] // S§19 #6 unknown
fn unknown_public_verdict_is_a_note() {
    let _g = shared();
    let (_r, a, _b) = pair();
    git(&a.root, &["remote", "set-url", "--push", "origin", "https://example.invalid/r.git"]);
    let probe = Verdict::Unknown("probe says no".into());
    let rep = run(&env(&a, Hook::none(), Clock::at(T0), probe, StubWake::busy(), far_policy()), WS_ID, Mode::Cli).unwrap();
    assert_eq!(rep.notes, vec!["zygo: cannot check if the remote is public: probe says no".to_string()]);
    assert_eq!(rep.passes, 1);
}

#[test] // S§11, Acceptance
fn unlisted_remote_stops() {
    let _g = shared();
    let (_r, a) = one();
    a.config(Some("a"), "remote = \"upstream\"\n");
    assert_eq!(run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err(), SyncError::Stopped { line: "zygo: no such remote".into(), check: "remote" });
}

#[test] // S§8.3 steps 2 and 3, S§14 "Two syncs on one machine"
fn child_skips_and_cli_waits_for_a_held_lock() {
    let _g = exclusive();
    let (_r, a, _b) = pair();
    let held = take(&a.sp().lock);
    assert_eq!(run(&plain_env(&a), WS_ID, Mode::Child).unwrap_err(), SyncError::Busy);
    assert!(a.sp().again.exists());
    let stepping = env(&a, Hook::none(), Clock::stepping(T0, 10), Verdict::Local, StubWake::busy(), far_policy());
    assert_eq!(run(&stepping, WS_ID, Mode::Cli).unwrap_err(), SyncError::Busy);
    drop(held);
    assert_eq!(sync(&a).passes, 1);
    assert!(!a.sp().again.exists());
}

#[test] // S§8.3 step 3 (MS-17 review round 1, T1): the CLI busy path on the injected clock, with no wall-clock wait
fn cli_run_with_a_held_lock_is_busy_after_the_wait_and_exits_1() {
    let _g = exclusive();
    let (_r, a, _b) = pair();
    let held = take(&a.sp().lock);
    let e = env(&a, Hook::none(), Clock::stepping(T0, LOCK_WAIT.as_secs() + 1), Verdict::Local, StubWake::busy(), far_policy());
    let err = run_with(&e, WS_ID, Mode::Cli, &Opts::default()).unwrap_err();
    assert_eq!(err, SyncError::Busy);
    assert_eq!(err.exit_code(Mode::Cli), 1);
    assert!(a.sp().again.exists()); // S§8.3 step 2: the holder sees the mark
    drop(held);
}

#[test] // S§8.3 step 3 (MS-21 review F1): a CLI run that waited for the lock of a forget writes nothing after it
fn cli_run_that_waited_for_a_forget_writes_nothing() {
    let _g = exclusive();
    let (_r, a) = one();
    let sp = a.sp();
    fs::create_dir_all(&sp.sync).unwrap();
    fs::File::create(&sp.lock).unwrap();
    let held = take(&sp.lock);
    let state = a.paths.workspaces_dir().join(WS_ID);
    let (state2, cache) = (state.clone(), a.paths.workspace_cache_dir().join(WS_ID));
    let forget: Box<dyn FnOnce() + Send> = Box::new(move || {
        fs::remove_dir_all(&state2).unwrap();
        let _ = fs::remove_dir_all(&cache);
        drop(held);
    });
    let e = Env { clock: Box::new(ForgetClock::new(T0, forget)), ..plain_env(&a) };
    let err = run(&e, WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(err, SyncError::Stopped { line: "zygo: the workspace was forgotten; nothing synced".into(), check: "forgotten" });
    assert_eq!(err.exit_code(Mode::Cli), 1);
    assert!(fs::symlink_metadata(&state).is_err(), "the state folder was made again");
    assert!(fs::symlink_metadata(&sp.identity).is_err() && fs::symlink_metadata(&sp.worktree).is_err());
    assert_eq!(git(&a.root, &["worktree", "list", "--porcelain"]).matches("worktree ").count(), 1);
}

/// The CLI line of a sync whose workspace was forgotten (S§8.3 step 3).
const FORGOTTEN: &str = "zygo: the workspace was forgotten; nothing synced";

/// A hook that runs `f` once at the seam point `after_gate` (between S§8.3 steps 1 and 2).
fn after_gate(f: impl FnOnce() + Send + 'static) -> Hook {
    let f = Mutex::new(Some(f));
    counting(Arc::default(), move |p, _| {
        if p == "after_gate" && let Some(f) = f.lock().unwrap().take() {
            f();
        }
    })
}

#[test] // S§8.3 steps 2-3 (MS-21 review R2-1): a forget after the gate: the sync never makes the state folder again
fn sync_whose_state_folder_went_after_the_gate_makes_no_state_folder() {
    let _g = exclusive();
    let (_r, a) = one();
    let state = a.paths.workspaces_dir().join(WS_ID);
    for mode in [Mode::Cli, Mode::Child] {
        a.workspace(&[(ALICE, "alice")]);
        let gone = state.clone();
        let h = after_gate(move || fs::remove_dir_all(&gone).unwrap());
        let r = run(&hook_env(&a, h), WS_ID, mode);
        match mode {
            Mode::Cli => assert_eq!(r.unwrap_err(), SyncError::Stopped { line: FORGOTTEN.into(), check: "forgotten" }),
            Mode::Child => assert_eq!(r.unwrap().passes, 0),
        }
        assert!(fs::symlink_metadata(&state).is_err(), "{mode:?}: the state folder was made again");
    }
}

#[test] // S§8.3 step 3 (MS-21 review R2-2): a new record of the same id written after the gate stops the sync
fn sync_whose_record_was_made_again_after_the_gate_stops_as_forgotten() {
    let _g = exclusive();
    let (_r, a) = one();
    let a = Arc::new(a);
    let state = a.paths.workspaces_dir().join(WS_ID);
    // The same record in a new state folder, then a record with another name.
    for rename in [false, true] {
        for mode in [Mode::Cli, Mode::Child] {
            a.workspace(&[(ALICE, "alice")]);
            let (a2, gone) = (a.clone(), state.clone());
            let h = after_gate(move || {
                fs::remove_dir_all(&gone).unwrap();
                a2.workspace(&[(ALICE, "alice")]);
                if rename {
                    Store::open(&a2.paths, WS_ID).update(Duration::from_secs(10), &mut |ws| {
                        ws.name = "other".into();
                        ws.tmux_session = "zygo-other".into();
                        Ok(())
                    }).unwrap();
                }
            });
            let r = run(&hook_env(&a, h), WS_ID, mode);
            match mode {
                Mode::Cli => assert_eq!(r.unwrap_err(), SyncError::Stopped { line: FORGOTTEN.into(), check: "forgotten" }),
                Mode::Child => assert_eq!(r.unwrap().passes, 0),
            }
            assert!(fs::symlink_metadata(a.sp().identity).is_err(), "rename {rename} {mode:?}: a pass ran");
            assert_eq!(a.errors(), "", "rename {rename} {mode:?}");
        }
    }
}

#[test] // S§8.3 step 3 (MS-21 review R2-2): a record change that is not a new workspace (the phase) does not stop the sync
fn sync_whose_record_phase_changed_after_the_gate_runs() {
    let _g = exclusive();
    let (_r, a) = one();
    let a = Arc::new(a);
    let a2 = a.clone();
    let h = after_gate(move || {
        Store::open(&a2.paths, WS_ID).update(Duration::from_secs(10), &mut |ws| {
            ws.phase = Phase::Down;
            ws.snapshot_seq += 1;
            Ok(())
        }).unwrap();
    });
    assert_eq!(run(&hook_env(&a, h), WS_ID, Mode::Cli).unwrap().passes, 1);
}

#[test] // S§8.3 step 3 (MS-21 review R2-2): a lock file unlinked and made again while the CLI waits: the lock taken is
        // not the lock at `mail/sync/lock`, so the sync stops as forgotten
fn cli_run_whose_lock_file_was_made_again_stops_as_forgotten() {
    let _g = exclusive();
    let (_r, a) = one();
    let sp = a.sp();
    fs::create_dir_all(&sp.sync).unwrap();
    fs::File::create(&sp.lock).unwrap();
    let held = take(&sp.lock);
    let lock = sp.lock.clone();
    let swap: Box<dyn FnOnce() + Send> = Box::new(move || {
        fs::remove_file(&lock).unwrap();
        fs::File::create(&lock).unwrap();
        drop(held);
    });
    let e = Env { clock: Box::new(ForgetClock::new(T0, swap)), ..plain_env(&a) };
    let err = run(&e, WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(err, SyncError::Stopped { line: FORGOTTEN.into(), check: "forgotten" });
    assert!(fs::symlink_metadata(&sp.identity).is_err() && fs::symlink_metadata(&sp.worktree).is_err());
}

#[test] // S§8.3 step 3 (MS-21 review R2-3): a record that cannot be read after the wait is the gate's record stop
fn cli_run_whose_record_became_newer_during_the_wait_is_the_record_stop() {
    let _g = exclusive();
    let (_r, a) = one();
    let sp = a.sp();
    fs::create_dir_all(&sp.sync).unwrap();
    fs::File::create(&sp.lock).unwrap();
    let held = take(&sp.lock);
    let file = a.paths.workspaces_dir().join(WS_ID).join("workspace.json");
    let newer: Box<dyn FnOnce() + Send> = Box::new(move || {
        let mut v: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        v["version"] = json!(v["version"].as_u64().unwrap() + 1);
        fs::write(&file, serde_json::to_vec(&v).unwrap()).unwrap();
        drop(held);
    });
    let e = Env { clock: Box::new(ForgetClock::new(T0, newer)), ..plain_env(&a) };
    let err = run(&e, WS_ID, Mode::Cli).unwrap_err();
    let line = "zygo: mail sync cannot run: the workspace record cannot be read";
    assert_eq!(err, SyncError::Stopped { line: line.into(), check: "workspace" });
    assert_eq!((err.exit_code(Mode::Cli), err.exit_code(Mode::Child)), (1, 0));
    assert!(fs::symlink_metadata(&sp.identity).is_err());
    assert_eq!(a.errors(), "");
}

#[test] // S§8.3 steps 3 and 17 (MS-21 review F1): a child whose record goes before its next pass ends silently, Ok
fn child_pass_after_the_record_went_does_not_run() {
    let _g = exclusive();
    let (_r, a, _b) = pair();
    let a = Arc::new(a);
    let a2 = a.clone();
    let h = counting(Arc::default(), move |p, n| {
        if p == "after_release" && n == 1 {
            mark_again(&a2.sp()).unwrap();
            fs::remove_file(a2.paths.workspaces_dir().join(WS_ID).join("workspace.json")).unwrap();
        }
    });
    assert_eq!(run(&hook_env(&a, h), WS_ID, Mode::Child).unwrap().passes, 1);
    assert!(a.sp().again.exists(), "a second pass took the mark");
}

#[test] // S§19 #16a
fn mark_during_the_pass_runs_a_second_pass() {
    let _g = exclusive();
    let (r, a, _b) = pair();
    let a = Arc::new(a);
    let a2 = a.clone();
    let inner = Arc::new(Mutex::new(None));
    let (inner2, late) = (inner.clone(), Arc::new(Mutex::new(None)));
    let late2 = late.clone();
    let h = counting(Arc::default(), move |p, n| {
        if p == "before_release" && n == 1 {
            *late2.lock().unwrap() = Some(send(&a2, "alice", ("bob", "b"), "late"));
            *inner2.lock().unwrap() = Some(run(&plain_env(&a2), WS_ID, Mode::Child));
        }
    });
    let rep = run(&hook_env(&a, h), WS_ID, Mode::Child).unwrap();
    assert_eq!(inner.lock().unwrap().take().unwrap(), Err(SyncError::Busy));
    assert_eq!(rep.passes, 2);
    let id2 = late.lock().unwrap().unwrap();
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id2}.json")).is_some());
    assert!(!a.sp().again.exists());
}

#[test] // S§6.6 (MS-17 review round 1, F2): a CLI sync whose later pass stops does not use the CI note up
fn ci_note_is_kept_when_a_later_pass_stops() {
    let _g = exclusive();
    let (r, a, _b) = pair();
    let mut st = load(&a.sp()).unwrap();
    st.ci_note_shown = false;
    save(&a.sp(), &st).unwrap();
    let a = Arc::new(a);
    let (a2, r) = (a.clone(), Arc::new(r));
    let r2 = r.clone();
    let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let h = counting(Arc::default(), move |p, n| {
        if p == "before_release" && n == 1 {
            send(&a2, "alice", ("bob", "b"), "late");
            mark_again(&a2.sp()).unwrap();
            released.store(true, Ordering::SeqCst);
        }
        if p == "before_push" && released.load(Ordering::SeqCst) {
            advance(&r2, BRANCH);
        }
    });
    let e = run(&hook_env(&a, h), WS_ID, Mode::Cli).unwrap_err();
    assert!(matches!(e, SyncError::Stopped { check: "moved", .. }), "{e:?}");
    assert!(!load(&a.sp()).unwrap().ci_note_shown);
    assert!(sync(&a).ci_note);
    assert!(load(&a.sp()).unwrap().ci_note_shown);
    assert!(!sync(&a).ci_note);
}

#[test] // S§19 #16b, review focus 9
fn starter_between_release_and_look_is_never_lost() {
    let _g = exclusive();
    let (r, a, _b) = pair();
    let a = Arc::new(a);
    // A starter that runs between the release and the look takes the free lock and runs the pass itself.
    let (a2, inner, late) = (a.clone(), Arc::new(Mutex::new(None)), Arc::new(Mutex::new(None)));
    let (inner2, late2) = (inner.clone(), late.clone());
    let h = counting(Arc::default(), move |p, n| {
        if p == "after_release" && n == 1 {
            *late2.lock().unwrap() = Some(send(&a2, "alice", ("bob", "b"), "late"));
            *inner2.lock().unwrap() = Some(run(&plain_env(&a2), WS_ID, Mode::Child));
        }
    });
    let rep = run(&hook_env(&a, h), WS_ID, Mode::Child).unwrap();
    assert_eq!(inner.lock().unwrap().take().unwrap().unwrap().passes, 1);
    assert_eq!(rep.passes, 1);
    let id2 = late.lock().unwrap().unwrap();
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id2}.json")).is_some());
    // A starter that has written its mark but not reached the lock: the holder's look sees the mark.
    let (a3, late3) = (a.clone(), Arc::new(Mutex::new(None)));
    let late4 = late3.clone();
    let h = counting(Arc::default(), move |p, n| {
        if p == "after_release" && n == 1 {
            *late4.lock().unwrap() = Some(send(&a3, "alice", ("bob", "b"), "later"));
            mark_again(&a3.sp()).unwrap();
        }
    });
    let rep = run(&hook_env(&a, h), WS_ID, Mode::Child).unwrap();
    assert_eq!(rep.passes, 2);
    let id3 = late3.lock().unwrap().unwrap();
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id3}.json")).is_some());
    assert!(!a.sp().again.exists());
}

#[test] // S§19 #23, S§10.1, M§7.3, M§7.1
fn idle_member_is_woken_once_busy_member_is_not() {
    let _g = shared();
    let (_r, a, b) = pair();
    let _s = live(&b.paths, "sbob");
    let typed = Typed::default();
    let clock = Arc::new(AtomicU64::new(T0));
    let b_env = |idle: bool, typed: &Typed| {
        env(&b, Hook::none(), Clock(clock.clone(), 0), Verdict::Local, StubWake { idle, typed: typed.clone() }, far_policy())
    };
    send(&a, "alice", ("bob", "b"), "one");
    sync(&a);
    assert_eq!(run(&b_env(true, &typed), WS_ID, Mode::Cli).unwrap().delivered, 1);
    send(&a, "alice", ("bob", "b"), "two");
    sync(&a);
    clock.store(T0 + 10, Ordering::SeqCst); // within wake_gap (30 s, M§11 default)
    assert_eq!(run(&b_env(true, &typed), WS_ID, Mode::Cli).unwrap().delivered, 1);
    assert_eq!(*typed.lock().unwrap(), vec![("%3".to_string(), WAKE_TEXT.to_string())]);
    let busy = Typed::default();
    clock.store(T0 + 100, Ordering::SeqCst);
    send(&a, "alice", ("bob", "b"), "three");
    sync(&a);
    assert_eq!(run(&b_env(false, &busy), WS_ID, Mode::Cli).unwrap().delivered, 1);
    assert!(busy.lock().unwrap().is_empty());
    // The M§7.1 Stop block of a busy member reads this count (zygo-hooks, ML-10 tests the block on it; Open O2).
    assert_eq!(zygo_workspace::mail::store::unread(&b.sp().mail, BOB).unwrap(), 3);
    assert!(include_str!("../../zygo-hooks/src/mail.rs").contains("new messages; read them with mail_read"));
}

/// Each outbox file name in `dir`, as ULIDs; empty when the folder is missing.
fn ulids_in(dir: &Path) -> BTreeSet<Ulid> {
    let Ok(d) = fs::read_dir(dir) else { return BTreeSet::new() };
    d.filter_map(|e| zygo_mail_sync::address::ulid_file(e.unwrap().file_name().to_str()?)).collect()
}

#[test] // R1 (review F2), S§8.3 step 11
fn unreadable_outbox_files_are_refused_and_take_no_cap_slot() {
    let _g = shared();
    let (r, a, _b) = pair();
    let sp = a.sp();
    fs::create_dir_all(&sp.outbox).unwrap();
    let mut bad = BTreeSet::new();
    for i in 0..1000u64 {
        let u = Ulid::from_parts(1_000 + i, 0);
        let bytes: &[u8] = if i % 2 == 0 { b"not json" } else { br#"{"to":"not an address"}"# };
        fs::write(sp.outbox.join(format!("{u}.json")), bytes).unwrap();
        bad.insert(u);
    }
    let id = send(&a, "alice", ("bob", "b"), "hello");
    assert_eq!(sync(&a).pushed, 1);
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_some());
    assert_eq!(ulids_in(&sp.refused), bad);
    assert!(ulids_in(&sp.outbox).is_empty());
    let first = bad.first().unwrap();
    let log = a.errors();
    assert_eq!(log.lines().filter(|l| l.contains(&first.to_string())).count(), 1);
    assert!(log.contains(&format!("mail sync: outbox {first}: read")));
}

#[test] // R1 (review F2): a link is moved, never followed; a FIFO is moved without a wait
fn link_or_fifo_in_the_outbox_is_moved_not_followed() {
    let _g = shared();
    use std::os::unix::fs::FileTypeExt;
    let (_r, a, _b) = pair();
    let sp = a.sp();
    fs::create_dir_all(&sp.outbox).unwrap();
    let target = a.dir.path().join("target.json");
    fs::write(&target, "target").unwrap();
    let (ul, uf) = (Ulid::from_parts(1_000, 1), Ulid::from_parts(1_000, 2));
    std::os::unix::fs::symlink(&target, sp.outbox.join(format!("{ul}.json"))).unwrap();
    let fifo = std::ffi::CString::new(sp.outbox.join(format!("{uf}.json")).to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    sync(&a);
    let moved = fs::symlink_metadata(sp.refused.join(format!("{ul}.json"))).unwrap();
    assert!(moved.file_type().is_symlink());
    assert_eq!(fs::read_to_string(&target).unwrap(), "target");
    assert!(fs::symlink_metadata(sp.refused.join(format!("{uf}.json"))).unwrap().file_type().is_fifo());
    assert!(ulids_in(&sp.outbox).is_empty());
    let log = a.errors();
    assert!(log.contains(&format!("mail sync: outbox {ul}: read")) && log.contains(&format!("mail sync: outbox {uf}: read")));
}

/// Takes the lock of `path` at once. Its caller holds [`exclusive`], so no child of another test holds the lock.
fn take(path: &Path) -> fs::File {
    let f = fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
    fs4::FileExt::try_lock(&f).unwrap();
    f
}

#[test] // R2 (review F3), S§8.3 step 17: the mark's owner runs the next pass
fn busy_lock_after_a_pass_ends_the_cli_run_with_its_report() {
    let _g = exclusive();
    let (_r, a, _b) = pair();
    let a = Arc::new(a);
    let (a2, held) = (a.clone(), Arc::new(Mutex::new(None)));
    let held2 = held.clone();
    let h = counting(Arc::default(), move |p, n| {
        if p == "after_release" && n == 1 {
            *held2.lock().unwrap() = Some(take(&a2.sp().lock));
            mark_again(&a2.sp()).unwrap();
        }
    });
    let e = env(&a, h, Clock::stepping(T0, 10), Verdict::Local, StubWake::busy(), far_policy());
    assert_eq!(run(&e, WS_ID, Mode::Cli).unwrap().passes, 1);
    assert!(a.sp().again.exists());
    drop(held);
}

#[test] // R3 (review F4): a lock failure is a stop with its errors.log line
fn lock_failure_is_logged_for_a_child() {
    let _g = exclusive();
    let (_r, a, _b) = pair();
    let sp = a.sp();
    fs::remove_file(&sp.lock).unwrap();
    std::os::unix::fs::symlink(a.dir.path().join("elsewhere"), &sp.lock).unwrap();
    let e = run(&plain_env(&a), WS_ID, Mode::Child).unwrap_err();
    assert!(matches!(e, SyncError::Stopped { check: "lock", .. }), "{e:?}");
    assert!(a.errors().contains("mail sync: stopped: lock"));
}

#[test] // FX-24 review R3 F2 (c), S§8.3 step 3, S§7.6 step 1: a FIFO at mail/sync/lock is never taken as the lock: the
// sync refuses with the lock stop and runs no pass
fn fifo_at_the_sync_lock_is_the_lock_stop_and_no_pass_runs() {
    let _g = exclusive();
    let (r, a, _b) = pair();
    let id = send(&a, "alice", ("bob", "b"), "one");
    let sp = a.sp();
    fs::remove_file(&sp.lock).unwrap();
    let fifo = std::ffi::CString::new(sp.lock.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let e = run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err();
    assert!(matches!(e, SyncError::Stopped { check: "lock", .. }), "{e:?}");
    assert!(a.errors().contains("mail sync: stopped: lock"));
    assert!(sp.outbox.join(format!("{id}.json")).is_file(), "a pass sent the queued message");
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_none(), "a pass pushed the queued message");
}

#[test] // R4 (review F5), S§8.3 step 1
fn missing_workspace_record_is_a_gate_failure() {
    let _g = shared();
    let r = Remote::new();
    let a = Machine::new(&r);
    a.config(Some("a"), "");
    let e = run(&plain_env(&a), WS_ID, Mode::Child).unwrap_err();
    assert_eq!(e.exit_code(Mode::Child), 0);
    assert!(!a.paths.workspaces_dir().join(WS_ID).exists());
    let e = run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: mail sync cannot run: the workspace record cannot be read".into(), check: "workspace" });
    assert_eq!(e.exit_code(Mode::Cli), 1);
    assert!(!a.paths.workspaces_dir().join(WS_ID).exists());
}

#[test] // R4 (review F5), S§8.3 step 1
fn config_that_does_not_load_is_a_gate_failure() {
    let _g = shared();
    let (_r, a) = one();
    fs::write(&a.paths.config_file, "[mail\n").unwrap();
    let e = run(&plain_env(&a), WS_ID, Mode::Child).unwrap_err();
    assert_eq!(e.exit_code(Mode::Child), 0);
    assert_eq!(a.errors(), "");
    let e = run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: mail sync cannot run: the config does not load".into(), check: "config" });
    assert_eq!(e.exit_code(Mode::Cli), 1);
    assert!(!a.sp().again.exists());
}

#[test] // R5 (review F6): only this pass's outbox paths are kept from a recipient folder of the tip
fn many_tip_entries_under_one_recipient() {
    let _g = shared();
    let (r, a, _b) = pair();
    let tip = r.tip(BRANCH).unwrap();
    let idx = r.dir.path().join("many-index");
    let g = |args: &[&str], input: Option<&[u8]>| {
        let mut c = plain(&r.bare);
        c.env("GIT_INDEX_FILE", &idx).args(args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped());
        let mut ch = c.spawn().unwrap();
        if let Some(i) = input {
            ch.stdin.take().unwrap().write_all(i).unwrap();
        }
        drop(ch.stdin.take());
        let o = ch.wait_with_output().unwrap();
        assert!(o.status.success());
        String::from_utf8(o.stdout).unwrap().trim_end().to_string()
    };
    g(&["read-tree", &tip], None);
    let x = blob(&r.bare, b"x");
    let info: String = (0..2000u64).map(|i| format!("100644 {x}\tbob@b/{}.json\n", Ulid::from_parts(5_000 + i, 0))).collect();
    g(&["update-index", "--index-info"], Some(info.as_bytes()));
    let tree = g(&["write-tree"], None);
    let c = commit_tree(&r.bare, &tree, Some(&tip));
    git(&r.bare, &["update-ref", &format!("refs/heads/{BRANCH}"), &c]);
    let id = send(&a, "alice", ("bob", "b"), "hello");
    assert_eq!(sync(&a).pushed, 1);
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_some());
}

#[test] // R5 (review F6): the pass deadline is checked before each secret check, on the injected clock
fn deadline_mid_outbox_stops_and_keeps_the_outbox() {
    let _g = exclusive();
    let (r, a, _b) = pair();
    let ids: BTreeSet<Ulid> = (0..3).map(|_| send(&a, "alice", ("bob", "b"), "hi")).collect();
    // Each now() adds 50 s: the lock (T0), the pass start (T0+50, end T0+170), the public check (T0+100), the first
    // secret check (T0+150) and the second (T0+200, past the end).
    let e = env(&a, Hook::none(), Clock::stepping(T0, 50), Verdict::Local, StubWake::busy(), far_policy());
    let err = run(&e, WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(err, SyncError::Stopped { line: "zygo: mail sync stopped (deadline); the outbox is kept".into(), check: "deadline" });
    assert_eq!(ulids_in(&a.sp().outbox), ids);
    assert!(ids.iter().all(|id| tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_none()));
}

/// `n` members `(pane key, name)`: keys `k` + 8 base-32 letters of `i`, names `m<i>`.
fn many_members(n: usize) -> Vec<(String, String)> {
    const B32: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    (0..n).map(|i| {
        let key: String = (0..8).map(|d| B32[(i >> (5 * (7 - d))) & 31] as char).collect();
        (format!("k{key}"), format!("m{i}"))
    }).collect()
}

#[test] // R7 (review F8), O3, MS-6 review note
fn own_registry_over_the_limit_is_not_written() {
    let _g = shared();
    let (r, a, _b) = pair();
    let before = tip_file(&r, BRANCH, "registry/a.json").unwrap();
    let m = many_members(zygo_mail_sync::registry::MEMBERS_MAX + 1);
    let refs: Vec<(&str, &str)> = m.iter().map(|(k, n)| (k.as_str(), n.as_str())).collect();
    a.workspace(&refs);
    sync(&a);
    assert_eq!(tip_file(&r, BRANCH, "registry/a.json").unwrap(), before);
    assert_eq!(a.errors().lines().filter(|l| l.ends_with("mail sync: own file over limit: members")).count(), 1);
}

#[test] // R7 (review F8): a failed state.json write is the `state` stop, and no last_error is saved
fn failed_state_write_stops_with_check_state() {
    let _g = shared();
    let (_r, a, _b) = pair();
    send(&a, "alice", ("bob", "b"), "hi");
    let state = a.sp().state;
    let s2 = state.clone();
    let h = Hook::new(move |p| {
        if p == "after_push" {
            fs::remove_file(&s2).unwrap();
            fs::create_dir(&s2).unwrap();
            fs::write(s2.join("x"), "").unwrap();
        }
        Ok(())
    });
    let e = run(&hook_env(&a, h), WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: mail sync stopped (state); the outbox is kept".into(), check: "state" });
    assert!(a.errors().contains("mail sync: stopped: state"));
    assert_eq!(common::names(&state), vec!["x".to_string()]);
}

#[test] // R7 (review F8), S§15: a stop with no lock held writes no state.json
fn stop_without_the_lock_writes_no_state() {
    let _g = shared();
    let (_r, a) = one();
    let sp = a.sp();
    fs::create_dir_all(sp.again.join("x")).unwrap();
    let e = run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err();
    assert!(matches!(e, SyncError::Stopped { check: "again", .. }), "{e:?}");
    assert!(a.errors().contains("mail sync: stopped: again"));
    assert!(!sp.state.exists());
}

#[test] // MS-13 round 2: each test holds the spawn guard, and each lock-sensitive test holds it for writing
fn every_test_holds_the_spawn_guard() {
    let _g = shared();
    // Tests that never reach the lock: a child run that ends at the gate, or exit codes only.
    const NO_LOCK: [&str; 4] = ["spec_values", "no_host_is_no_sync", "missing_workspace_record_is_a_gate_failure",
        "config_that_does_not_load_is_a_gate_failure"];
    let src = include_str!("sync_pass.rs");
    let tests: Vec<&str> = src.split("\n#[test]").skip(1).collect();
    assert!(tests.len() > 30);
    for t in tests {
        let name = t.split("fn ").nth(1).unwrap().split('(').next().unwrap();
        let body = t.split_once("() {\n").unwrap().1;
        let guard = body.lines().next().unwrap().trim();
        assert!(guard == "let _g = shared();" || guard == "let _g = exclusive();", "{name}: {guard}");
        let sensitive = ["Mode::Child", "Clock::stepping", "mark_again(", "take(&"].iter().any(|k| body.split("\n#[").next().unwrap().contains(k));
        if sensitive && !NO_LOCK.contains(&name) && name != "every_test_holds_the_spawn_guard" {
            assert_eq!(guard, "let _g = exclusive();", "{name}");
        }
    }
}

#[test] // d1 (FX-24), S§8.6 "At commit": the notice resolves its sender on the read under mail/lock, not the gate's
fn notice_to_a_sender_whose_record_went_makes_no_maildir() {
    let _g = exclusive();
    let (r, a, _b) = pair();
    let id = send(&a, "alice", ("bob", "b"), "say zz-forbidden-zz now");
    a.config(Some("a"), "[[redaction.pattern]]\nid = \"ship_word\"\npattern = \"zz-forbidden-zz\"\naction = \"redact\"\ndescription = \"d\"\n");
    let w = Store::open(&a.paths, WS_ID);
    let h = after_gate(move || {
        w.update(Duration::from_secs(10), &mut |ws| {
            ws.members.remove(ALICE);
            Ok(())
        })
        .unwrap();
    });
    run(&hook_env(&a, h), WS_ID, Mode::Cli).unwrap();
    assert!(a.sp().refused.join(format!("{id}.json")).is_file());
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_none());
    assert!(fs::symlink_metadata(a.sp().mail.join(ALICE)).is_err(), "the notice made mail/<alice>/");
    let log = a.errors();
    assert!(log.lines().any(|l| l.ends_with(&format!("mail sync: outbox {id}: notice: no-record"))), "{log}");
}

#[test] // d1 (FX-24), review F5, S§8.6 "At commit": a notice that cannot take mail/lock keeps the message for the next pass
fn notice_that_cannot_take_mail_lock_keeps_the_message_for_the_next_pass() {
    let _g = exclusive();
    let (r, a, _b) = pair();
    let id = send(&a, "alice", ("bob", "b"), "say zz-forbidden-zz now");
    a.config(Some("a"), "[[redaction.pattern]]\nid = \"ship_word\"\npattern = \"zz-forbidden-zz\"\naction = \"redact\"\ndescription = \"d\"\n");
    let held = {
        use std::os::unix::fs::OpenOptionsExt;
        let f = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600)
            .open(a.sp().mail.join("lock")).unwrap();
        fs4::FileExt::try_lock(&f).unwrap();
        f
    };
    sync(&a);
    drop(held);
    let (queued, refused) = (a.sp().outbox.join(format!("{id}.json")), a.sp().refused.join(format!("{id}.json")));
    assert!(queued.is_file(), "the message left the outbox with no notice");
    assert!(fs::symlink_metadata(&refused).is_err());
    assert!(fs::symlink_metadata(a.sp().mail.join(ALICE)).is_err(), "a notice was delivered under a held lock");
    assert!(a.errors().lines().any(|l| l.ends_with(&format!("mail sync: outbox {id}: notice"))));
    // The lock is free: the next pass sends the notice, then moves the message to refused/, once.
    sync(&a);
    sync(&a);
    assert!(fs::symlink_metadata(&queued).is_err());
    assert!(refused.is_file());
    assert_eq!(fs::read_dir(a.sp().mail.join(ALICE).join("new")).unwrap().count(), 1);
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_none());
}

/// The new pane key of `bob` and of `alice` in the FX-24 review F2 tests.
const NEW_BOB: &str = "kbobnew22";
const NEW_ALICE: &str = "kalicenew";

/// Renames the member `key` to `renamed` (session `s<renamed>`) and adds `new_key` named `name` (session `s<name>`),
/// as `zygo ws name` and a new pane do between the gate's read and a delivery.
fn moved_name(w: &Store, key: &str, renamed: &str, new_key: &str, name: &str) {
    w.update(Duration::from_secs(10), &mut |ws| {
        let m = ws.members.get_mut(key).unwrap();
        m.name = Some(renamed.into());
        m.zygo_session = Some(format!("s{renamed}"));
        let mut n = zygo_workspace::state::MemberRecord::new(zygo_core::Agent::ClaudeCode);
        n.name = Some(name.into());
        n.zygo_session = Some(format!("s{name}"));
        ws.members.insert(new_key.into(), n);
        Ok(())
    })
    .unwrap();
}

#[test] // FX-24 review F2, S§8.5, S§10.1: the import maps the folder name to a key, and wakes, on the read under the lock
fn import_delivers_and_wakes_by_the_read_under_the_lock() {
    let _g = exclusive();
    let (_r, a, b) = pair();
    let id = send(&a, "alice", ("bob", "b"), "one");
    sync(&a);
    let _s = live(&b.paths, "sbob");
    let typed = Typed::default();
    let w = Store::open(&b.paths, WS_ID);
    let h = after_gate(move || moved_name(&w, BOB, "robert", NEW_BOB, "bob"));
    let e = env(&b, h, Clock::at(T0), Verdict::Local, StubWake { idle: true, typed: typed.clone() }, far_policy());
    assert_eq!(run(&e, WS_ID, Mode::Cli).unwrap().delivered, 1);
    assert!(b.sp().mail.join(NEW_BOB).join("new").join(format!("{id}.json")).is_file(), "not in bob's new maildir");
    assert!(fs::symlink_metadata(b.sp().mail.join(BOB).join("new").join(format!("{id}.json"))).is_err());
    assert_eq!(*typed.lock().unwrap(), vec![("%3".to_string(), WAKE_TEXT.to_string())]);
}

#[test] // FX-24 review F2, R2 F5, S§8.6, S§10.1: the notice goes to the sender's key of the pass's first read, and wakes
// it from the read under the lock (here: a new session of the same member, live only in that read)
fn notice_wakes_the_sender_from_the_read_under_the_lock() {
    let _g = exclusive();
    let (_r, a, _b) = pair();
    let id = send(&a, "alice", ("bob", "b"), "say zz-forbidden-zz now");
    a.config(Some("a"), "[[redaction.pattern]]\nid = \"ship_word\"\npattern = \"zz-forbidden-zz\"\naction = \"redact\"\ndescription = \"d\"\n");
    let _s = live(&a.paths, "salice2");
    let typed = Typed::default();
    let w = Store::open(&a.paths, WS_ID);
    let h = after_gate(move || {
        w.update(Duration::from_secs(10), &mut |ws| {
            ws.members.get_mut(ALICE).unwrap().zygo_session = Some("salice2".into());
            Ok(())
        })
        .unwrap();
    });
    let e = env(&a, h, Clock::at(T0), Verdict::Local, StubWake { idle: true, typed: typed.clone() }, far_policy());
    run(&e, WS_ID, Mode::Cli).unwrap();
    assert!(a.sp().refused.join(format!("{id}.json")).is_file());
    assert_eq!(fs::read_dir(a.sp().mail.join(ALICE).join("new")).unwrap().count(), 1);
    assert_eq!(*typed.lock().unwrap(), vec![("%3".to_string(), WAKE_TEXT.to_string())]);
}

#[test] // FX-24 review R2 F5, S§8.6: a sender name that maps to another key under the lock gets no notice: `moved`
fn notice_to_a_renamed_sender_whose_name_another_member_took_is_moved() {
    let _g = exclusive();
    let (r, a, _b) = pair();
    let id = send(&a, "alice", ("bob", "b"), "say zz-forbidden-zz now");
    a.config(Some("a"), "[[redaction.pattern]]\nid = \"ship_word\"\npattern = \"zz-forbidden-zz\"\naction = \"redact\"\ndescription = \"d\"\n");
    let _s = live(&a.paths, "salice");
    let typed = Typed::default();
    let w = Store::open(&a.paths, WS_ID);
    let h = after_gate(move || moved_name(&w, ALICE, "alicia", NEW_ALICE, "alice"));
    let e = env(&a, h, Clock::at(T0), Verdict::Local, StubWake { idle: true, typed: typed.clone() }, far_policy());
    run(&e, WS_ID, Mode::Cli).unwrap();
    assert!(a.sp().refused.join(format!("{id}.json")).is_file());
    assert!(fs::symlink_metadata(a.sp().outbox.join(format!("{id}.json"))).is_err());
    assert!(fs::symlink_metadata(a.sp().mail.join(NEW_ALICE)).is_err(), "the new alice got the notice");
    assert_eq!(new_count(&a, ALICE), 0, "alicia got a notice for a name she no longer has");
    assert!(typed.lock().unwrap().is_empty(), "a member was woken");
    let log = a.errors();
    assert!(log.lines().any(|l| l.ends_with(&format!("mail sync: outbox {id}: notice: moved"))), "{log}");
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_none());
}

#[test] // FX-24 review R2 F1, S§8.6: a full sender inbox is a lasting refusal of the notice: the message moves once
fn notice_to_a_full_sender_inbox_moves_the_message_to_refused() {
    let _g = exclusive();
    let (r, a, _b) = pair();
    let id = send(&a, "alice", ("bob", "b"), "say zz-forbidden-zz now");
    a.config(Some("a\"\nmax_unread = 2\n#"), "[[redaction.pattern]]\nid = \"ship_word\"\npattern = \"zz-forbidden-zz\"\naction = \"redact\"\ndescription = \"d\"\n");
    let new = a.sp().mail.join(ALICE).join("new");
    fs::create_dir_all(&new).unwrap();
    for n in 0..2u64 {
        let u = ulid::Ulid::from_parts(1_764_000_000_000 + n, n as u128);
        fs::write(new.join(format!("{u}.json")), b"{}").unwrap();
    }
    let (queued, refused) = (a.sp().outbox.join(format!("{id}.json")), a.sp().refused.join(format!("{id}.json")));
    sync(&a);
    assert!(refused.is_file(), "the message is not in refused/ after one pass");
    assert!(fs::symlink_metadata(&queued).is_err(), "the message is still in the outbox");
    assert_eq!(fs::read_dir(&new).unwrap().count(), 2, "a notice went past max_unread");
    let lines = |log: String| log.lines().filter(|l| l.contains(&id.to_string())).map(String::from).collect::<Vec<_>>();
    let once = lines(a.errors());
    assert_eq!(once.len(), 2, "{once:?}");
    assert!(once[0].ends_with(&format!("mail sync: outbox {id}: refused: ship_word")), "{once:?}");
    assert!(once[1].ends_with(&format!("mail sync: outbox {id}: notice: full")), "{once:?}");
    sync(&a);
    sync(&a);
    assert_eq!(lines(a.errors()), once, "a later pass wrote about the moved message");
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_none());
}

#[test] // FX-24 review R3 F1, S§8.6: a sender whose name the pass's first read does not have (here: alice renamed herself
// to alicia after the gate and kept her key) is notified at the key of the read under the lock
fn notice_to_a_sender_renamed_after_the_gate_with_the_same_key_is_sent() {
    let _g = exclusive();
    let (r, a, _b) = pair();
    let id = send(&a, "alicia", ("bob", "b"), "say zz-forbidden-zz now");
    a.config(Some("a"), "[[redaction.pattern]]\nid = \"ship_word\"\npattern = \"zz-forbidden-zz\"\naction = \"redact\"\ndescription = \"d\"\n");
    let w = Store::open(&a.paths, WS_ID);
    let h = after_gate(move || {
        w.update(Duration::from_secs(10), &mut |ws| {
            ws.members.get_mut(ALICE).unwrap().name = Some("alicia".into());
            Ok(())
        })
        .unwrap();
    });
    let e = env(&a, h, Clock::at(T0), Verdict::Local, StubWake { idle: true, typed: Typed::default() }, far_policy());
    run(&e, WS_ID, Mode::Cli).unwrap();
    let log = a.errors();
    assert_eq!(new_count(&a, ALICE), 1, "the same member (same key) got no notice; log: {log}");
    assert!(a.sp().refused.join(format!("{id}.json")).is_file(), "{log}");
    assert!(fs::symlink_metadata(a.sp().outbox.join(format!("{id}.json"))).is_err(), "{log}");
    assert!(!log.lines().any(|l| l.contains(&format!("mail sync: outbox {id}: notice"))), "{log}");
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_none());
}

#[test] // FX-24 review R3 F3, S§8.6: a write into the sender's maildir that fails (a link at mail/<sender>/new) is a lasting
// refusal of the notice: the message moves to refused/ once, with `notice: maildir`, and later passes write nothing
fn notice_whose_maildir_write_fails_moves_the_message_to_refused() {
    let _g = exclusive();
    let (r, a, _b) = pair();
    let id = send(&a, "alice", ("bob", "b"), "say zz-forbidden-zz now");
    a.config(Some("a"), "[[redaction.pattern]]\nid = \"ship_word\"\npattern = \"zz-forbidden-zz\"\naction = \"redact\"\ndescription = \"d\"\n");
    let md = a.sp().mail.join(ALICE);
    fs::create_dir_all(&md).unwrap();
    let elsewhere = a.dir.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, md.join("new")).unwrap();
    let (queued, refused) = (a.sp().outbox.join(format!("{id}.json")), a.sp().refused.join(format!("{id}.json")));
    sync(&a);
    let lines = |log: String| log.lines().filter(|l| l.contains(&id.to_string())).map(String::from).collect::<Vec<_>>();
    let once = lines(a.errors());
    assert!(refused.is_file(), "the message is not in refused/ after one pass: {once:?}");
    assert!(fs::symlink_metadata(&queued).is_err(), "the message is still in the outbox: {once:?}");
    assert_eq!(once.len(), 2, "{once:?}");
    assert!(once[0].ends_with(&format!("mail sync: outbox {id}: refused: ship_word")), "{once:?}");
    assert!(once[1].ends_with(&format!("mail sync: outbox {id}: notice: maildir")), "{once:?}");
    sync(&a);
    sync(&a);
    assert_eq!(lines(a.errors()), once, "a later pass wrote about the moved message");
    assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0, "a notice was written through the link");
    assert!(fs::symlink_metadata(md.join("new")).unwrap().file_type().is_symlink());
    assert!(tip_file(&r, BRANCH, &format!("bob@b/{id}.json")).is_none());
}

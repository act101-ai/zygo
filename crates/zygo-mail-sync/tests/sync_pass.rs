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
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{advance, blob, commit_tree, git, live, plain, tip_file, tree_literally, Machine, Remote, ALICE, BOB, WS_ID};
use serde_json::{json, Value};
use ulid::Ulid;
use zygo_config::{Config, LoadContext, MailConfig, SyncOff};
use zygo_core::paths::Paths;
use zygo_mail_sync::address::Address;
use zygo_mail_sync::env::{self, Crash, Env, Seam, Spawner, Verdict, Wake};
use zygo_mail_sync::registry::{read_cache, write_cache, MemberCache};
use zygo_mail_sync::state::{load, make_identity, mark_again, SyncPaths};
use zygo_mail_sync::sync::{run, Mode, SyncError, LOCK_WAIT, MAX_ATTEMPTS, OUTBOX_CAP, PASS_DEADLINE};
use zygo_redact::DeadlinePolicy;
use zygo_workspace::mail::wake::{after_send_with, Probe as IdleProbe, WAKE_TEXT};
use zygo_workspace::mux::{HookScope, Multiplexer, MuxError, NewSession, NewWindow, SessionAnswer, Split};
use zygo_workspace::state::Workspace;

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
    assert_eq!(
        (PASS_DEADLINE, MAX_ATTEMPTS, OUTBOX_CAP, LOCK_WAIT),
        (Duration::from_secs(120), 5, 1000, Duration::from_secs(30))
    );
    let stop = SyncError::Stopped { line: "x".into(), check: "push" };
    for (e, child, cli) in [(SyncError::Off(SyncOff::NotOn), 0, 1), (SyncError::Busy, 0, 1), (stop, 1, 1)] {
        assert_eq!((e.exit_code(Mode::Child), e.exit_code(Mode::Cli)), (child, cli));
    }
}

#[test] // S§19 #1, S-G1, S§7.3, S§14 Normal, S-C4
fn normal_message_arrives_with_bytes_unchanged() {
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

#[test] // S§19 #2, S-G2, SP6
fn user_repository_is_untouched() {
    let (r, a, b) = machines();
    for m in [&a, &b] {
        git(&m.root, &["config", "gc.auto", "1"]);
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
    let (r, a, _b) = pair();
    edit_tip(&r, &[("registry/c.json", &registry_json("d", &"0".repeat(32), &["carol"]))], &[]);
    sync(&a);
    let c = read_cache(&a.sp());
    assert!(!c.hosts.contains_key("c") && !c.hosts.contains_key("d"));
    assert!(a.errors().contains("mail sync: registry file refused: check host"));
}

#[test] // review focus 47, S§7.4, S§7.5 own hosts
fn own_ack_follows_the_tip_across_own_hosts() {
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
    let (r, a, _b) = pair();
    let other = SyncPaths::new(&a.paths, "fedcba9876543210").unwrap();
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

#[test] // S§19 #6 unknown
fn unknown_public_verdict_is_a_note() {
    let (_r, a, _b) = pair();
    git(&a.root, &["remote", "set-url", "--push", "origin", "https://example.invalid/r.git"]);
    let probe = Verdict::Unknown("probe says no".into());
    let rep = run(&env(&a, Hook::none(), Clock::at(T0), probe, StubWake::busy(), far_policy()), WS_ID, Mode::Cli).unwrap();
    assert_eq!(rep.notes, vec!["zygo: cannot check if the remote is public: probe says no".to_string()]);
    assert_eq!(rep.passes, 1);
}

#[test] // S§11, Acceptance
fn unlisted_remote_stops() {
    let (_r, a) = one();
    a.config(Some("a"), "remote = \"upstream\"\n");
    assert_eq!(run(&plain_env(&a), WS_ID, Mode::Cli).unwrap_err(), SyncError::Stopped { line: "zygo: no such remote".into(), check: "remote" });
}

#[test] // S§8.3 steps 2 and 3, S§14 "Two syncs on one machine"
fn child_skips_and_cli_waits_for_a_held_lock() {
    let (_r, a, _b) = pair();
    let held = fs::OpenOptions::new().read(true).write(true).open(a.sp().lock).unwrap();
    fs4::FileExt::try_lock(&held).unwrap();
    assert_eq!(run(&plain_env(&a), WS_ID, Mode::Child).unwrap_err(), SyncError::Busy);
    assert!(a.sp().again.exists());
    let stepping = env(&a, Hook::none(), Clock::stepping(T0, 10), Verdict::Local, StubWake::busy(), far_policy());
    assert_eq!(run(&stepping, WS_ID, Mode::Cli).unwrap_err(), SyncError::Busy);
    drop(held);
    assert_eq!(sync(&a).passes, 1);
    assert!(!a.sp().again.exists());
}

#[test] // S§19 #16a
fn mark_during_the_pass_runs_a_second_pass() {
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

#[test] // S§19 #16b, review focus 9
fn starter_between_release_and_look_is_never_lost() {
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
    let n = zygo_workspace::mail::store::unread(&b.sp().mail, BOB).unwrap();
    assert_eq!(n, 3);
    assert_eq!(format!("zygo: {n} new messages; read them with mail_read"), "zygo: 3 new messages; read them with mail_read");
}

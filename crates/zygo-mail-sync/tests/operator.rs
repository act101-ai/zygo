#![cfg(unix)]
//! MS-15: the operator commands (mail sync design S§7.6): take-host, new-identity, rename, drop-host; S§19 #9, #11, #19, #20.
mod common;

use std::collections::BTreeSet;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{git, tip_file, Machine, Remote, ALICE, BOB, WS_ID};
use serde_json::Value;
use ulid::Ulid;
use zygo_config::{LoadContext, MailConfig};
use zygo_core::paths::Paths;
use zygo_mail_sync::address::Address;
use zygo_mail_sync::env::{self, Env, Spawner, Verdict, Wake};
use zygo_mail_sync::ops::{new_identity, take_host};
use zygo_mail_sync::outbox::SendError;
use zygo_mail_sync::state;
use zygo_mail_sync::sync::{run, run_with, Mode, Opts, Report, SyncError};
use zygo_redact::DeadlinePolicy;
use zygo_workspace::state::Workspace;

/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;
const BRANCH: &str = "zygo/mail";

/// The injected fixed clock (S-C9).
struct Clock(u64);
impl env::Clock for Clock {
    fn now(&self) -> SystemTime { UNIX_EPOCH + Duration::from_secs(self.0) }
}

struct Says(Verdict);
impl env::Probe for Says {
    fn probe(&self, _: &str) -> Verdict { self.0.clone() }
}

struct NoSpawn;
impl Spawner for NoSpawn {
    fn spawn_sync(&self, _: &str) -> std::io::Result<()> { Ok(()) }
}

/// No wake: MS-13 tests the wake.
struct NoWake;
impl Wake for NoWake {
    fn wake(&self, _: &Paths, _: &Workspace, _: &[String], _: &MailConfig, _: SystemTime) {}
}

/// The default secret policy with an hour of budget.
fn far_policy() -> DeadlinePolicy { DeadlinePolicy { base: Duration::from_secs(3600), ..Default::default() } }

/// The seams of machine `m`: no seam, the clock fixed at `T0`, the probe verdict `v`.
fn env_with(m: &Machine, v: Verdict) -> Env {
    Env {
        paths: m.paths.clone(),
        clock: Box::new(Clock(T0)),
        probe: Box::new(Says(v)),
        seam: Box::new(env::NoSeam),
        spawner: Box::new(NoSpawn),
        wake: Box::new(NoWake),
        secret_policy: far_policy(),
    }
}

/// [`env_with`] a local remote.
fn env(m: &Machine) -> Env { env_with(m, Verdict::Local) }

/// One CLI sync of `m`.
fn sync(m: &Machine) -> Report { run(&env(m), WS_ID, Mode::Cli).unwrap() }

/// One CLI sync of `m` with `zygo mail sync --prune`.
fn prune_now(m: &Machine) -> Report { run_with(&env(m), WS_ID, Mode::Cli, &Opts { prune: true }).unwrap() }

/// One message from member `from` to `to.0@to.1` through the send path (MS-9), at `T0` plus a counter.
fn try_send(m: &Machine, from: &str, to: (&str, &str)) -> Result<Ulid, SendError> {
    static N: AtomicU64 = AtomicU64::new(1);
    let cfg = zygo_config::load(&LoadContext { paths: m.paths.clone(), repo: Some(m.root.clone()), team: None })
        .unwrap().config;
    let s = zygo_config::sync_for(&cfg, "app");
    let to = Address { name: to.0.into(), host: to.1.into() };
    let at = UNIX_EPOCH + Duration::from_secs(T0 + N.fetch_add(1, Ordering::SeqCst));
    zygo_mail_sync::outbox::send_remote_with(&m.sp(), s.as_ref(), Some(from), &to, "status", "t", &cfg.redaction,
        &m.root, at, &far_policy())
}

fn send(m: &Machine, from: &str, to: (&str, &str)) -> Ulid { try_send(m, from, to).unwrap() }

/// A machine of `r` with `[mail] host = "<host>"` and the workspace members `members`.
fn machine(r: &Remote, host: &str, members: &[(&str, &str)]) -> Machine {
    let m = Machine::new(r);
    m.config(Some(host), "");
    m.workspace(members);
    m
}

/// Machines `a` (host `a`, `alice`) and `b` (host `b`, `bob`); B, A, B sync.
fn pair() -> (Remote, Machine, Machine) {
    let r = Remote::new();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    let b = machine(&r, "b", &[(BOB, "bob")]);
    sync(&b);
    sync(&a);
    sync(&b);
    (r, a, b)
}

/// The bytes of `path` at the remote tip; `None` when absent.
fn file(r: &Remote, path: &str) -> Option<Vec<u8>> { tip_file(r, BRANCH, path) }

/// The JSON of `path` at the remote tip.
fn json(r: &Remote, path: &str) -> Value { serde_json::from_slice(&file(r, path).unwrap()).unwrap() }

/// The `imported` array of the ack file `path` at the tip; empty when absent.
fn imported(r: &Remote, path: &str) -> BTreeSet<String> {
    let Some(b) = file(r, path) else { return BTreeSet::new() };
    let v: Value = serde_json::from_slice(&b).unwrap();
    v["imported"].as_array().unwrap().iter().map(|u| u.as_str().unwrap().to_string()).collect()
}

/// The file names in `mail/<key>/new/` of `m`; empty when absent.
fn new_ids(m: &Machine, key: &str) -> BTreeSet<String> {
    let Ok(d) = fs::read_dir(m.sp().mail.join(key).join("new")) else { return BTreeSet::new() };
    d.map(|e| e.unwrap().file_name().into_string().unwrap()).collect()
}

/// The mail identity of `m`.
fn ident(m: &Machine) -> String { state::identity(&m.sp()).unwrap().unwrap() }

#[test] // S§19 #19, S§7.6 Rename, S§8.3 step 12, S§14 "Host renamed while mail waits", review focus 47, S-C8
fn host_rename_while_mail_waits() {
    let (r, a, b) = pair();
    let ids: BTreeSet<String> = (0..3).map(|_| send(&a, "alice", ("bob", "b")).to_string()).collect();
    sync(&a);
    let a_files = (file(&r, "registry/a.json"), file(&r, "ack/a.json"));
    b.config(Some("b2"), "");
    let rb = sync(&b);
    assert_eq!(rb.delivered, 3);
    assert_eq!(new_ids(&b, BOB), ids.iter().map(|u| format!("{u}.json")).collect());
    assert_eq!(file(&r, "registry/b.json"), None);
    assert_eq!(json(&r, "registry/b2.json")["identity"], ident(&b));
    assert_eq!(imported(&r, "ack/b.json"), ids); // the files wait at the tip: the old ack stays
    sync(&a);
    assert_eq!(try_send(&a, "alice", ("bob", "b")), Err(SendError::UnknownAddress(vec!["bob@b2".into()])));
    assert!(prune_now(&b).notes.contains(&"pruned 3 files".to_string()));
    assert!(file(&r, "ack/b.json").is_some()); // the prune removes message files only
    sync(&b);
    assert_eq!(file(&r, "ack/b.json"), None);
    let u = send(&a, "alice", ("bob", "b2"));
    sync(&a);
    assert_eq!(sync(&b).delivered, 1);
    assert!(new_ids(&b, BOB).contains(&format!("{u}.json")));
    assert_eq!((file(&r, "registry/a.json"), file(&r, "ack/a.json")), a_files);
}

/// The collision line of S§7.5, as `identity.rs:19` holds it.
const COLLISION: &str = "zygo: the mail host name is taken on the side branch by another identity. If that is another machine, set another [mail] host. If it is this workspace from before (a moved repository, a ws forget, a restored backup), run zygo mail sync --take-host.";

fn stopped(e: SyncError) -> (String, &'static str) {
    let SyncError::Stopped { line, check } = e else { panic!("{e:?}") };
    (line, check)
}

#[test] // S§19 #9, S§7.6 --take-host, S§14 "Moved repository, ws forget, restored backup", S-G5
fn take_host_after_a_collision() {
    let r = Remote::new();
    let x1 = machine(&r, "x", &[(ALICE, "alice")]);
    sync(&x1);
    let s = machine(&r, "s", &[(BOB, "bob")]);
    sync(&s);
    let u1 = send(&s, "bob", ("alice", "x"));
    sync(&s);
    assert_eq!(sync(&x1).delivered, 1);
    let x2 = machine(&r, "x", &[(ALICE, "alice")]);
    let e = run(&env(&x2), WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(e.exit_code(Mode::Cli), 1);
    assert_eq!(stopped(e), (COLLISION.to_string(), "collision"));
    assert!(new_ids(&x2, ALICE).is_empty());
    let rep = take_host(&env(&x2), WS_ID).unwrap();
    assert_eq!(rep.delivered, 0);
    assert!(new_ids(&x2, ALICE).is_empty()); // acked by x1: not delivered again
    assert_eq!(json(&r, "registry/x.json")["identity"], ident(&x2));
    assert_eq!(json(&r, "ack/x.json")["identity"], ident(&x2));
    assert_eq!(imported(&r, "ack/x.json"), BTreeSet::from([u1.to_string()]));
    let e = run(&env(&x1), WS_ID, Mode::Cli).unwrap_err();
    assert_eq!((e.exit_code(Mode::Cli), stopped(e).1), (1, "collision"));
    sync(&s);
    let u2 = send(&s, "bob", ("alice", "x"));
    sync(&s);
    assert_eq!(sync(&x2).delivered, 1);
    assert_eq!(new_ids(&x2, ALICE), BTreeSet::from([format!("{u2}.json")]));
}

#[test] // S§8.3 step 5, S§8.2: --take-host is a sync; a public remote stops it before any fetch or push
fn take_host_on_a_public_remote_stops_before_any_fetch() {
    let r = Remote::new();
    let x1 = machine(&r, "x", &[(ALICE, "alice")]);
    sync(&x1);
    let x2 = machine(&r, "x", &[(ALICE, "alice")]);
    git(&x2.root, &["remote", "set-url", "--push", "origin", "https://example.invalid/r.git"]);
    let refs0 = git(&r.bare, &["for-each-ref"]);
    let e = take_host(&env_with(&x2, Verdict::Public), WS_ID).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: the remote can be read without a login; mail sync refused".into(), check: "public" });
    assert_eq!(e.exit_code(Mode::Cli), 1);
    assert_eq!(git(&r.bare, &["for-each-ref"]), refs0);
    assert!(!x2.root.join(".git/refs/remotes/origin/zygo").exists());
    assert!(!x2.wt.exists());
}

/// The twin line of S§7.5, as `identity.rs:22` holds it.
const TWIN: &str = "zygo: another workspace writes with this mail identity (copied state?); run zygo mail sync --new-identity here, and set another [mail] host";

/// S§19 #11 set-up: [`pair`], then machine C (host `a`, `alice`) with copies of A's `mail/sync/identity` and
/// `state.json`; B sends to `alice@a`, C imports it and writes `ack/a.json`; A's next sync stops with the twin line.
fn twin() -> (Remote, Machine, Machine, Machine) {
    let (r, a, b) = pair();
    let c = machine(&r, "a", &[(ALICE, "alice")]);
    for d in [c.sp().mail, c.sp().sync] {
        zygo_workspace::mail::ensure_mail_root(&d).unwrap();
    }
    fs::copy(a.sp().identity, c.sp().identity).unwrap();
    fs::copy(a.sp().state, c.sp().state).unwrap();
    send(&b, "bob", ("alice", "a"));
    sync(&b);
    assert_eq!(sync(&c).delivered, 1);
    assert_eq!(stopped(run(&env(&a), WS_ID, Mode::Cli).unwrap_err()), (TWIN.to_string(), "twin"));
    (r, a, b, c)
}

#[test] // S§19 #11 --new-identity part, S§7.6 --new-identity
fn new_identity_with_a_new_host_syncs_and_deletes_nothing() {
    let (r, a, _b, _c) = twin();
    let (old, before) = (ident(&a), (file(&r, "registry/a.json"), file(&r, "ack/a.json")));
    a.config(Some("a2"), "");
    assert_eq!(new_identity(&env(&a), WS_ID).unwrap().passes, 1);
    let new = ident(&a);
    assert_ne!(new, old);
    assert_eq!(json(&r, "registry/a2.json")["identity"], new);
    assert_eq!((file(&r, "registry/a.json"), file(&r, "ack/a.json")), before);
    let pushed: BTreeSet<String> = state::load(&a.sp()).unwrap().pushed.into_keys().collect();
    assert_eq!(pushed, BTreeSet::from(["ack/a2.json".to_string(), "registry/a2.json".to_string()]));
}

#[test] // S§7.6 --new-identity: with the same host the pass stops with the collision line
fn new_identity_with_the_same_host_stops_at_the_collision() {
    let (r, a, _b, _c) = twin();
    let (old, reg) = (ident(&a), file(&r, "registry/a.json"));
    let e = new_identity(&env(&a), WS_ID).unwrap_err();
    assert_eq!(stopped(e), (COLLISION.to_string(), "collision"));
    assert_ne!(ident(&a), old);
    assert_eq!(file(&r, "registry/a.json"), reg);
}

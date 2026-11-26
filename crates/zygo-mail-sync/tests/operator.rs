#![cfg(unix)]
//! MS-15: the operator commands (mail sync design S§7.6): take-host, new-identity, rename, drop-host; S§19 #9, #11, #19, #20.
mod common;

use std::collections::BTreeSet;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{blob, commit_tree, git, git_in, plain, tip_file, Machine, Remote, ALICE, BOB, WS_ID};
use serde_json::Value;
use ulid::Ulid;
use zygo_config::{LoadContext, MailConfig};
use zygo_core::paths::Paths;
use zygo_mail_sync::address::Address;
use zygo_mail_sync::env::{self, Env, Spawner, Verdict, Wake};
use zygo_mail_sync::ops::{drop_host, drop_prompt, new_identity, take_host, waiting_for};
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

#[test] // S§7.6 --drop-host prompt
fn drop_prompt_text() {
    assert_eq!(drop_prompt(3, "b"), "zygo: 3 messages wait for host b; they will be deleted. Delete them? [y/N]");
}

#[test] // S§19 #20 drop part, S§7.6 --drop-host, S-C8, S-G5
fn drop_host_counts_asks_and_deletes_in_one_commit() {
    let (r, a, b) = pair();
    let u0 = send(&a, "alice", ("bob", "b"));
    sync(&a);
    sync(&b); // B imports and acks u0; its file stays at the tip
    let waiting: Vec<Ulid> = (0..3).map(|_| send(&a, "alice", ("bob", "b"))).collect();
    sync(&a);
    let a_files = (file(&r, "registry/a.json"), file(&r, "ack/a.json"));
    assert_eq!(waiting_for(&env(&a), WS_ID, "b"), Ok(3));
    let mut asked = Vec::new();
    let mut bad = |_: &str| -> bool { panic!("asked") };
    assert_eq!(stopped(drop_host(&env(&a), WS_ID, "B/..", &mut bad).unwrap_err()).1, "host");
    let before = r.tip(BRANCH).unwrap();
    assert_eq!(drop_host(&env(&a), WS_ID, "b", &mut |p: &str| { asked.push(p.to_string()); false }), Ok(None));
    assert_eq!(asked, vec![drop_prompt(3, "b")]);
    assert_eq!(r.tip(BRANCH).unwrap(), before);
    assert_eq!(drop_host(&env(&a), WS_ID, "b", &mut |_| true), Ok(Some(3)));
    let after = r.tip(BRANCH).unwrap();
    assert_eq!(git(&r.bare, &["rev-parse", &format!("{after}^")]), before);
    let mut gone: Vec<String> = [u0].iter().chain(&waiting).map(|u| format!("bob@b/{u}.json")).collect();
    gone.extend(["ack/b.json".into(), "registry/b.json".into()]);
    gone.sort();
    let diff = git(&r.bare, &["diff-tree", "-r", "--name-only", "--no-commit-id", &before, &after]);
    assert_eq!(diff.lines().collect::<Vec<_>>(), gone);
    assert_eq!((file(&r, "registry/a.json"), file(&r, "ack/a.json")), a_files);
    sync(&b); // a dropped host registers again; the deleted mail is gone
    assert!(file(&r, "registry/b.json").is_some());
    assert_eq!(new_ids(&b, BOB), BTreeSet::from([format!("{u0}.json")]));
}

#[test] // S§8.3 step 5, S§8.2: --drop-host is a sync; a public remote stops it before any fetch and before the ask
fn drop_host_on_a_public_remote_stops_before_any_fetch() {
    let r = Remote::new();
    let a = machine(&r, "a", &[(ALICE, "alice")]);
    sync(&a);
    git(&a.root, &["remote", "set-url", "--push", "origin", "https://example.invalid/r.git"]);
    let (refs0, tracking0) = (git(&r.bare, &["for-each-ref"]), git(&a.root, &["for-each-ref", "refs/remotes"]));
    let mut ask = |_: &str| -> bool { panic!("asked") };
    let e = drop_host(&env_with(&a, Verdict::Public), WS_ID, "b", &mut ask).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: the remote can be read without a login; mail sync refused".into(), check: "public" });
    assert_eq!(e.exit_code(Mode::Cli), 1);
    assert_eq!(git(&r.bare, &["for-each-ref"]), refs0);
    assert_eq!(git(&a.root, &["for-each-ref", "refs/remotes"]), tracking0);
}

/// The paths of the remote tip, one per line, as `git ls-tree -r --name-only` quotes them.
fn tip_paths(r: &Remote) -> Vec<String> {
    let tip = r.tip(BRANCH).unwrap();
    git(&r.bare, &["ls-tree", "-r", "--name-only", &tip]).lines().map(String::from).collect()
}

/// Whether a tip path, quoted or not, is under `prefix`.
fn under(paths: &[String], prefix: &str) -> bool { paths.iter().any(|p| p.trim_start_matches('"').starts_with(prefix)) }

#[test] // S§7.6 --drop-host, S-G5: new mail between the count and the answer is counted and asked about again
fn drop_host_asks_again_when_new_mail_arrives() {
    let (r, a, _b) = pair();
    let c = machine(&r, "c", &[("kcarol222", "carol")]);
    sync(&c);
    for _ in 0..3 {
        send(&a, "alice", ("bob", "b"));
    }
    sync(&a);
    let mut asked = Vec::new();
    let n = drop_host(&env(&a), WS_ID, "b", &mut |p: &str| {
        if asked.is_empty() {
            send(&c, "carol", ("bob", "b"));
            send(&c, "carol", ("bob", "b"));
            sync(&c);
        }
        asked.push(p.to_string());
        true
    });
    assert_eq!(asked, vec![drop_prompt(3, "b"), drop_prompt(5, "b")]);
    assert_eq!(n, Ok(Some(5)));
    assert!(!under(&tip_paths(&r), "bob@b/"));
}

#[test] // S§7.6 --drop-host: each `*@<host>/` file goes, in subfolders and with names that are not UTF-8
fn drop_host_removes_subfolders() {
    let (r, a, _b) = pair();
    send(&a, "alice", ("bob", "b"));
    sync(&a);
    let s = a.dir.path().join("scratch");
    git(a.dir.path(), &["clone", "-q", "--no-checkout", "--branch", BRANCH, &r.url, "scratch"]);
    git(&s, &["read-tree", "HEAD"]);
    let y = blob(&s, b"y");
    git(&s, &["update-index", "--add", "--cacheinfo", &format!("100644,{y},bob@b/x/y")]);
    let mut info = format!("100644 {y}\tbob@b/").into_bytes();
    info.extend_from_slice(b"\xff.json\0");
    git_in(&s, &["update-index", "--add", "-z", "--index-info"], &info);
    git(&s, &["commit", "-q", "-m", "t"]);
    git(&s, &["push", "-q", "origin", &format!("HEAD:refs/heads/{BRANCH}")]);
    let before = tip_paths(&r);
    assert!(before.contains(&"bob@b/x/y".to_string()));
    assert!(before.iter().any(|p| p.starts_with("\"bob@b/\\377")));
    assert_eq!(drop_host(&env(&a), WS_ID, "b", &mut |_| true), Ok(Some(1)));
    assert!(!under(&tip_paths(&r), "bob@b/"));
}

#[test] // S§8.3 steps 3 and 17: a send that met the lock of an operator command runs when it releases
fn drop_host_runs_the_waiting_send() {
    let (r, _a, b) = pair();
    let u = send(&b, "bob", ("alice", "a"));
    state::mark_again(&b.sp()).unwrap();
    assert_eq!(drop_host(&env(&b), WS_ID, "c", &mut |_| true), Ok(Some(0)));
    assert!(file(&r, &format!("alice@a/{u}.json")).is_some());
}

/// `n` members `(pane key, name)` with valid keys and names.
fn many(n: usize) -> Vec<(String, String)> {
    (0..n).map(|i| {
        let mut k = String::from("k");
        let mut x = i;
        for _ in 0..8 {
            k.push((b'a' + (x % 26) as u8) as char);
            x /= 26;
        }
        (k, format!("m{i}"))
    }).collect()
}

fn refs(v: &[(String, String)]) -> Vec<(&str, &str)> { v.iter().map(|(k, n)| (k.as_str(), n.as_str())).collect() }

#[test] // S§7.6 --take-host, O3: a registry over MEMBERS_MAX is not written, so neither file is
fn take_host_over_member_limit_writes_nothing() {
    let r = Remote::new();
    let x1 = machine(&r, "x", &[(ALICE, "alice")]);
    sync(&x1);
    let m = many(257);
    let x2 = machine(&r, "x", &refs(&m));
    let before = (file(&r, "registry/x.json"), file(&r, "ack/x.json"));
    assert_eq!(stopped(take_host(&env(&x2), WS_ID).unwrap_err()).1, "collision");
    assert_eq!((file(&r, "registry/x.json"), file(&r, "ack/x.json")), before);
}

#[test] // S§7.6 Rename, O3: when the new registry is over MEMBERS_MAX, the old one stays
fn rename_over_member_limit_keeps_old_registry() {
    let (r, _a, b) = pair();
    let m = many(257);
    b.workspace(&refs(&m));
    b.config(Some("b2"), "");
    sync(&b);
    assert_eq!(file(&r, "registry/b2.json"), None);
    assert!(file(&r, "registry/b.json").is_some());
}

#[test] // S§8.3 step 11, S§7.6 Rename, S-G5 (review F1): mail to a host with no registry at the tip is refused, not stranded
fn mail_to_a_renamed_host_from_a_stale_cache_is_refused() {
    let (r, a, b) = pair();
    b.config(Some("b2"), "");
    sync(&b);
    assert_eq!(file(&r, "registry/b.json"), None);
    let id = send(&a, "alice", ("bob", "b")); // A has not synced: its member cache still lists bob@b
    sync(&a);
    assert!(a.sp().refused.join(format!("{id}.json")).is_file());
    assert!(!a.sp().outbox.join(format!("{id}.json")).exists());
    assert_eq!(file(&r, &format!("bob@b/{id}.json")), None);
    let log = a.errors();
    let line = log.lines().find(|l| l.contains(&id.to_string())).unwrap();
    assert!(line.ends_with(&format!("mail sync: outbox {id}: no-host")), "{line}");
    let notes: Vec<Value> = fs::read_dir(a.sp().mail.join(ALICE).join("new")).unwrap()
        .map(|e| serde_json::from_slice(&fs::read(e.unwrap().path()).unwrap()).unwrap())
        .collect();
    let n = notes.iter().find(|m| m["from"] == "zygo").unwrap();
    assert_eq!(n["to"], "alice");
    assert_eq!(n["subject"], format!("not sent: {id}"));
    assert_eq!(n["body"], format!("zygo did not send message {id} to bob@b: host b has no registry on the side branch. The message is kept on this machine in mail/outbox/refused/."));
}

#[test] // S§8.3 step 11, S§7.6 Rename: a file pushed before a crash and before the rename is deleted, not refused
fn pushed_mail_to_a_since_renamed_host_is_not_refused() {
    let (r, a, b) = pair();
    let id = send(&a, "alice", ("bob", "b"));
    let queued = a.sp().outbox.join(format!("{id}.json"));
    let bytes = fs::read(&queued).unwrap();
    sync(&a);
    assert!(file(&r, &format!("bob@b/{id}.json")).is_some());
    fs::write(&queued, &bytes).unwrap(); // a crash after the push, before the outbox delete
    b.config(Some("b2"), "");
    sync(&b);
    assert_eq!(file(&r, "registry/b.json"), None);
    sync(&a);
    assert!(!queued.exists());
    assert!(!a.sp().refused.join(format!("{id}.json")).exists());
    assert!(!a.errors().contains("no-host"), "{}", a.errors());
    assert!(!fs::read_dir(a.sp().mail.join(ALICE).join("new")).map_or(false, |d| {
        d.map(|e| fs::read(e.unwrap().path()).unwrap())
            .any(|b| serde_json::from_slice::<Value>(&b).unwrap()["from"] == "zygo")
    }));
}

/// The notices from `zygo` in `mail/<key>/new/` of `m`.
fn notices(m: &Machine, key: &str) -> Vec<Value> {
    let Ok(d) = fs::read_dir(m.sp().mail.join(key).join("new")) else { return Vec::new() };
    d.map(|e| serde_json::from_slice::<Value>(&fs::read(e.unwrap().path()).unwrap()).unwrap())
        .filter(|v| v["from"] == "zygo")
        .collect()
}

/// A new remote tip with `path` set to `bytes` and each of `removes` gone, on a scratch index of the bare repository.
fn rewrite(r: &Remote, path: &str, bytes: &[u8], removes: &[&str]) {
    use std::io::Write;
    let tip = r.tip(BRANCH).unwrap();
    let idx = r.dir.path().join("rewrite-index");
    let b = blob(&r.bare, bytes);
    let mut info = format!("100644 {b}\t{path}\n");
    for p in removes {
        info.push_str(&format!("0 {}\t{p}\n", "0".repeat(40))); // mode 0 removes the entry
    }
    assert!(plain(&r.bare).env("GIT_INDEX_FILE", &idx).args(["read-tree", &tip]).status().unwrap().success());
    let mut ch = plain(&r.bare).env("GIT_INDEX_FILE", &idx).args(["update-index", "--index-info"])
        .stdin(std::process::Stdio::piped()).spawn().unwrap();
    ch.stdin.take().unwrap().write_all(info.as_bytes()).unwrap();
    assert!(ch.wait().unwrap().success());
    let out = plain(&r.bare).env("GIT_INDEX_FILE", &idx).arg("write-tree").output().unwrap();
    let tree = String::from_utf8(out.stdout).unwrap().trim().to_string();
    let c = commit_tree(&r.bare, &tree, Some(&tip));
    git(&r.bare, &["update-ref", &format!("refs/heads/{BRANCH}"), &c]);
}

#[test] // S§8.3 step 11, S§7.6 Rename, S§8.5 Candidates, S§19 #19 (no-host review F1): the old ack keeps the old host read
fn mail_to_an_old_host_whose_ack_remains_is_pushed() {
    let (r, a, b) = pair();
    for _ in 0..2 {
        send(&a, "alice", ("bob", "b"));
    }
    sync(&a);
    b.config(Some("b2"), "");
    sync(&b);
    assert_eq!(file(&r, "registry/b.json"), None);
    assert!(file(&r, "ack/b.json").is_some()); // the files wait at the tip: the old ack stays
    let id = send(&a, "alice", ("bob", "b")); // A's member cache is stale
    sync(&a);
    assert!(file(&r, &format!("bob@b/{id}.json")).is_some());
    assert!(!a.sp().refused.join(format!("{id}.json")).exists());
    assert!(notices(&a, ALICE).is_empty());
    assert_eq!(sync(&b).delivered, 1);
    assert!(new_ids(&b, BOB).contains(&format!("{id}.json")));
}

#[test] // S§8.3 step 11, S§7.3 Validation (no-host review F2): a registry that is not used counts as none
fn mail_to_a_host_with_an_invalid_registry_is_refused() {
    let (r, a, _b) = pair();
    let id = send(&a, "alice", ("bob", "b"));
    rewrite(&r, "registry/b.json", b"{}", &["ack/b.json"]);
    sync(&a);
    assert!(a.sp().refused.join(format!("{id}.json")).is_file());
    assert_eq!(file(&r, &format!("bob@b/{id}.json")), None);
    assert!(a.errors().contains(&format!("mail sync: outbox {id}: no-host")), "{}", a.errors());
    assert_eq!(notices(&a, ALICE).len(), 1);
}

/// A clock that adds `.1` seconds at each read, starting at `.0`.
struct Stepping(AtomicU64, u64);
impl env::Clock for Stepping {
    fn now(&self) -> SystemTime { UNIX_EPOCH + Duration::from_secs(self.0.fetch_add(self.1, Ordering::SeqCst)) }
}

#[test] // S§8.3 step 4, step 11 (no-host review F3): the pass deadline stops the no-host refusals; the rest stays
fn deadline_stops_the_no_host_refusals() {
    let (_r, a, b) = pair();
    b.config(Some("b2"), "");
    sync(&b);
    let ids: Vec<Ulid> = (0..3).map(|_| send(&a, "alice", ("bob", "b"))).collect();
    let mut e = env(&a);
    e.clock = Box::new(Stepping(AtomicU64::new(T0), 50));
    let err = run(&e, WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(err, SyncError::Stopped { line: "zygo: mail sync stopped (deadline); the outbox is kept".into(), check: "deadline" });
    assert!(ids.iter().any(|id| a.sp().outbox.join(format!("{id}.json")).is_file()));
}

#[test] // S§7.6 Rename, S-G5, review focus 47 (review round 2 F1): the old ack stays while the old registry stays
fn rename_over_member_limit_keeps_old_ack_and_delivers() {
    let (r, a, b) = pair();
    let m = many(257);
    let mut members = refs(&m);
    members.push((BOB, "bob"));
    b.workspace(&members);
    b.config(Some("b2"), "");
    sync(&b);
    sync(&a);
    let id = send(&a, "alice", ("bob", "b"));
    sync(&a);
    assert!(file(&r, &format!("bob@b/{id}.json")).is_some());
    let delivered = sync(&b).delivered + sync(&b).delivered;
    assert!(file(&r, "registry/b.json").is_some());
    assert!(file(&r, "ack/b.json").is_some(), "the old ack is gone while the old registry stays");
    assert_eq!(delivered, 1);
    assert!(new_ids(&b, BOB).contains(&format!("{id}.json")));
}

#[test] // S§7.6 --drop-host (review round 2 F3): folder paths past one command line's budget all go, in one commit
fn drop_host_removes_folders_past_one_command_line() {
    let (r, a, _b) = pair();
    let s = a.dir.path().join("scratch");
    git(a.dir.path(), &["clone", "-q", "--no-checkout", "--branch", BRANCH, &r.url, "scratch"]);
    git(&s, &["read-tree", "HEAD"]);
    let y = blob(&s, b"y");
    // 2500 folder arguments of at least 10 command-line characters each: past the 16 000 of one call.
    let info: String = (0..2500).map(|i| format!("100644 {y}\tm{i}@b/y\n")).collect();
    git_in(&s, &["update-index", "--add", "--index-info"], info.as_bytes());
    git(&s, &["commit", "-q", "-m", "t"]);
    git(&s, &["push", "-q", "origin", &format!("HEAD:refs/heads/{BRANCH}")]);
    let before = r.tip(BRANCH).unwrap();
    assert_eq!(tip_paths(&r).iter().filter(|p| p.contains("@b/")).count(), 2500);
    assert_eq!(drop_host(&env(&a), WS_ID, "b", &mut |_| true), Ok(Some(0)));
    let after = r.tip(BRANCH).unwrap();
    assert_eq!(git(&r.bare, &["rev-parse", &format!("{after}^")]), before);
    assert!(!tip_paths(&r).iter().any(|p| p.contains("@b/")));
    assert!(file(&r, "registry/b.json").is_none() && file(&r, "ack/b.json").is_none());
}

#[test] // S§7.6, S§7.5 Twin, S§8.3 step 7, S§19 #11 (review round 3 R2-1): a drop in a twin state stops and writes nothing
fn drop_host_does_not_mask_a_twin() {
    let (r, a, _b, _c) = twin();
    let before = r.tip(BRANCH).unwrap();
    assert_eq!(stopped(drop_host(&env(&a), WS_ID, "b", &mut |_| true).unwrap_err()), (TWIN.to_string(), "twin"));
    assert_eq!(r.tip(BRANCH).unwrap(), before);
    assert_eq!(stopped(run(&env(&a), WS_ID, Mode::Cli).unwrap_err()), (TWIN.to_string(), "twin"));
}

#[test] // S§7.6, S§7.5 Twin, S§8.3 step 7, S§19 #11 (review round 3 R2-1): a take-host in a twin state stops and writes nothing
fn take_host_does_not_mask_a_twin() {
    let (r, a, _b, c) = twin();
    a.config(Some("a3"), "");
    assert_eq!(stopped(take_host(&env(&a), WS_ID).unwrap_err()), (TWIN.to_string(), "twin"));
    assert!(file(&r, "registry/a3.json").is_none());
    assert!(file(&r, "ack/a3.json").is_none());
    assert!(run(&env(&c), WS_ID, Mode::Cli).is_ok());
    a.config(Some("a4"), "");
    assert_eq!(new_identity(&env(&a), WS_ID).unwrap().passes, 1);
    assert_eq!(json(&r, "registry/a4.json")["identity"], ident(&a));
}

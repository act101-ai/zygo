//! MS-14: the prune (mail sync design S§9), step 16 of the sync (S§8.3), the S§14 prune rows, S§19 #14, #15,
//! #17, #18 and SP5. Local bare repositories, the injected clock, no wall-clock wait (S-C9).
#![cfg(unix)]
mod common;

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{blob, commit_tree, far, git, git_in, plain, tip_file, tree_literally, Machine, Remote, ALICE, BOB, WS_ID};
use serde_json::Value;
use ulid::Ulid;
use zygo_config::{LoadContext, MailConfig};
use zygo_core::paths::Paths;
use zygo_mail_sync::address::Address;
use zygo_mail_sync::branch::{self, BranchError, Fetched};
use zygo_mail_sync::env::{self, Crash, Env, Seam, Spawner, Verdict, Wake};
use zygo_mail_sync::identity::{read_meta, TipMeta};
use zygo_mail_sync::prune::{self, Pruned, COMMITS_DUE, OWN_ACK_DUE};
use zygo_mail_sync::sync::{run, Mode, Report};
use zygo_redact::DeadlinePolicy;
use zygo_workspace::state::Workspace;

/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;
const BRANCH: &str = "zygo/mail";

/// The commit count of `refs/heads/zygo/mail` in the bare repository.
fn count(r: &Remote) -> u64 {
    git(&r.bare, &["rev-list", "--count", "refs/heads/zygo/mail"]).parse().unwrap()
}

/// `n` new commits on `refname` in `repo` with one `git fast-import` (one process): each has the tree of the one
/// before and that one as parent; the first starts at the current `refname`, or is a root when it does not exist.
/// Returns the new tip.
fn grow(repo: &Path, refname: &str, n: u64) -> String {
    let o = plain(repo).args(["rev-parse", "--verify", "-q", refname]).output().unwrap();
    let from = String::from_utf8(o.stdout).unwrap().trim_end().to_string();
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!("commit {refname}\ncommitter t <t@t.invalid> {T0} +0000\ndata 0\n"));
        if i == 0 && !from.is_empty() {
            s.push_str(&format!("from {from}\n"));
        }
        s.push('\n');
    }
    git_in(repo, &["fast-import", "--quiet"], s.as_bytes());
    git(repo, &["rev-parse", refname])
}

#[test] // S§9 When, S§7.4 Size
fn t1_spec_values() {
    assert_eq!((COMMITS_DUE, OWN_ACK_DUE), (1000, 100_000));
}

#[test] // S§9 When: 1000 commits or more, more than 100 000 own ack entries, or forced
fn t2_due_at_1000_commits_over_100000_entries_or_forced() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let c999 = grow(&m.root, "refs/heads/t", 999);
    assert_eq!(prune::due(&m.root, &c999, 0, false, far()), Ok(false));
    assert_eq!(prune::due(&m.root, &c999, 100_000, false, far()), Ok(false));
    assert_eq!(prune::due(&m.root, &c999, 100_001, false, far()), Ok(true));
    assert_eq!(prune::due(&m.root, &c999, 0, true, far()), Ok(true));
    let c1000 = grow(&m.root, "refs/heads/t", 1);
    assert_eq!(prune::due(&m.root, &c1000, 0, false, far()), Ok(true));
}

/// The injected fixed clock (S-C9).
struct Clock(u64);
impl env::Clock for Clock {
    fn now(&self) -> SystemTime { UNIX_EPOCH + Duration::from_secs(self.0) }
}

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
}
impl Seam for Hook {
    fn at(&self, p: &'static str) -> Result<(), Crash> { (self.0.lock().unwrap())(p) }
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

/// The seams of machine `m`: `seam`, the clock fixed at `secs`, a private remote.
fn env_at(m: &Machine, seam: Hook, secs: u64) -> Env {
    Env {
        paths: m.paths.clone(),
        clock: Box::new(Clock(secs)),
        probe: Box::new(Says(Verdict::Local)),
        seam: Box::new(seam),
        spawner: Box::new(NoSpawn),
        wake: Box::new(NoWake),
        secret_policy: far_policy(),
    }
}

/// One CLI sync of `m` at `T0`.
fn sync(m: &Machine) -> Report { run(&env_at(m, Hook::none(), T0), WS_ID, Mode::Cli).unwrap() }

/// One message from member `from` to `to.0@to.1` through the send path (MS-9), at `T0` plus a counter.
fn send(m: &Machine, from: &str, to: (&str, &str), body: &str) -> Ulid {
    static N: AtomicU64 = AtomicU64::new(1);
    let cfg = zygo_config::load(&LoadContext { paths: m.paths.clone(), repo: Some(m.root.clone()), team: None })
        .unwrap().config;
    let s = zygo_config::sync_for(&cfg, "app");
    let to = Address { name: to.0.into(), host: to.1.into() };
    let at = UNIX_EPOCH + Duration::from_secs(T0 + N.fetch_add(1, Ordering::SeqCst));
    zygo_mail_sync::outbox::send_remote_with(&m.sp(), s.as_ref(), Some(from), &to, "status", body, &cfg.redaction,
        &m.root, at, &far_policy()).unwrap()
}

/// `[mail]` with `host` and the lines `mail_extra`, then `[mail.workspace.app] sync = "zygo/mail"`.
fn write_config(m: &Machine, host: &str, mail_extra: &str) {
    fs::write(&m.paths.config_file,
        format!("[mail]\nhost = \"{host}\"\n{mail_extra}[mail.workspace.app]\nsync = \"zygo/mail\"\n")).unwrap();
}

/// A remote and machine `a` (host `a`, member `alice`), synced once (the branch exists).
fn one() -> (Remote, Machine) {
    let r = Remote::new();
    let a = Machine::new(&r);
    write_config(&a, "a", "");
    a.workspace(&[(ALICE, "alice")]);
    sync(&a);
    (r, a)
}

/// Machines `a` (host `a`, `alice`, `[mail]` lines `a_extra`) and `b` (host `b`, `bob`, `b_extra`); B, A, B sync.
fn machines_with(a_extra: &str, b_extra: &str) -> (Remote, Machine, Machine) {
    let r = Remote::new();
    let a = Machine::new(&r);
    write_config(&a, "a", a_extra);
    a.workspace(&[(ALICE, "alice")]);
    let b = Machine::new(&r);
    write_config(&b, "b", b_extra);
    b.workspace(&[(BOB, "bob")]);
    sync(&b);
    sync(&a);
    sync(&b);
    (r, a, b)
}

fn pair() -> (Remote, Machine, Machine) { machines_with("", "") }

/// The `imported` array of the ack file `path` at the tip; empty when absent.
#[allow(dead_code)]
fn imported(r: &Remote, path: &str) -> BTreeSet<String> {
    let Some(b) = tip_file(r, BRANCH, path) else { return BTreeSet::new() };
    let v: Value = serde_json::from_slice(&b).unwrap();
    v["imported"].as_array().unwrap().iter().map(|u| u.as_str().unwrap().to_string()).collect()
}

/// Whether `<addr>/<id>.json` is at the remote tip.
fn at_tip(r: &Remote, addr: &str, id: Ulid) -> bool { tip_file(r, BRANCH, &format!("{addr}/{id}.json")).is_some() }

/// The tip of `m`'s fetch of the side branch, with its objects in `m`'s repository.
fn fetched(m: &Machine) -> String {
    match branch::fetch(&m.root, "origin", BRANCH, far()).unwrap() {
        Fetched::Tip(t) => t,
        Fetched::NoBranch => panic!("no branch"),
    }
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

#[test] // S§14 "A and B prune at the same time"; S§9 Tree, Push
fn t6_two_prunes_from_one_base_the_second_loses_the_lease() {
    let (r, a, b) = pair();
    let u0 = send(&a, "alice", ("bob", "b"), "zero");
    sync(&a);
    sync(&b);
    let base = fetched(&a);
    assert_eq!(fetched(&b), base);
    let ma = read_meta(&a.wt, &base, far(), &mut |_| {}).unwrap();
    let mut mb = read_meta(&b.wt, &base, far(), &mut |_| {}).unwrap();
    // The same base and meta give the same root within one second of git's commit date, and git reports a push of
    // the commit the branch already has as up to date before it checks the lease. Without `ack/b.json` B's root
    // keeps `u0`, so it differs from A's whatever the wall clock.
    mb.acks.remove("b");
    assert_eq!(prune::run(&a.wt, "origin", BRANCH, &base, &ma, far()), Ok(Pruned::Done { removed: 1 }));
    let root = r.tip(BRANCH).unwrap();
    assert_eq!(count(&r), 1);
    assert!(!at_tip(&r, "bob@b", u0));
    assert_eq!(prune::run(&b.wt, "origin", BRANCH, &base, &mb, far()), Ok(Pruned::LeaseLost));
    assert_eq!(r.tip(BRANCH), Some(root));
}

#[test] // MS-6 review: a refused ack (`parse_ack` → `Err`) is empty for the prune; S§7.4 Validation
fn t11_a_refused_ack_is_empty_for_the_prune() {
    let id = "ab".repeat(16);
    let u = Ulid::from_parts(T0 * 1000, 7);
    let cases: [(&str, Vec<u8>); 2] = [
        ("json", br#"{"version":1,"host":"c""#.to_vec()),
        ("host", format!(r#"{{"version":1,"host":"d","identity":"{id}","imported":["{u}"]}}"#).into_bytes()),
    ];
    for (case, ack) in cases {
        let (r, a, b) = pair();
        let u0 = send(&a, "alice", ("bob", "b"), "zero");
        sync(&a);
        sync(&b);
        edit_tip(&r, &[(&format!("carol@c/{u}.json"), b"{}"), ("ack/c.json", &ack)], &[]);
        let base = fetched(&a);
        let meta = read_meta(&a.wt, &base, far(), &mut |_| {}).unwrap();
        assert!(meta.acks["c"].value.is_err(), "{case}");
        assert_eq!(prune::run(&a.wt, "origin", BRANCH, &base, &meta, far()), Ok(Pruned::Done { removed: 1 }), "{case}");
        assert!(at_tip(&r, "carol@c", u), "{case}");
        assert!(!at_tip(&r, "bob@b", u0), "{case}");
        assert!(tip_file(&r, BRANCH, "ack/c.json").is_some(), "{case}");
    }
}

#[test] // S§9 "Only its own branch", S-C3, review focus 10: both checks run again just before the force push
fn t12_force_push_only_to_a_zygo_branch_whose_base_has_the_marker() {
    let (r, a) = one();
    let tip = fetched(&a);
    let meta = read_meta(&a.wt, &tip, far(), &mut |_| {}).unwrap();
    for name in ["mail", "zygo-mail"] {
        git(&r.bare, &["update-ref", &format!("refs/heads/{name}"), &tip]);
        assert_eq!(prune::run(&a.wt, "origin", name, &tip, &meta, far()), Err(BranchError::Refused("branch")), "{name}");
        assert_eq!(r.tip(name), Some(tip.clone()), "{name}");
    }
    let x = blob(&a.root, b"x");
    let t = tree_literally(&a.root, &[("100644", b"x", &x)]);
    let c = commit_tree(&a.root, &t, None);
    git(&a.root, &["push", "-q", "--force", "origin", &format!("{c}:refs/heads/{BRANCH}")]);
    assert_eq!(prune::run(&a.wt, "origin", BRANCH, &c, &TipMeta::default(), far()), Err(BranchError::Refused("marker")));
    assert_eq!(r.tip(BRANCH), Some(c));
    git(&r.bare, &["update-ref", &format!("refs/heads/{BRANCH}"), &tip]);
    assert_eq!(prune::run(&a.wt, "origin", BRANCH, &tip, &meta, far()), Ok(Pruned::Done { removed: 0 }));
    assert_ne!(r.tip(BRANCH), Some(tip));
    assert_eq!(count(&r), 1);
}

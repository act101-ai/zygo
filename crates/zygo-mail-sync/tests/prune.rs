//! MS-14: the prune (mail sync design S§9), step 16 of the sync (S§8.3), the S§14 prune rows, S§19 #14, #15,
//! #17, #18 and SP5. Local bare repositories, the injected clock, no wall-clock wait (S-C9).
#![cfg(unix)]
mod common;

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{advance, blob, commit_tree, far, git, git_in, plain, tip_file, tree_literally, Machine, Remote, ALICE, BOB, WS_ID};
use serde_json::Value;
use ulid::Ulid;
use zygo_config::{LoadContext, MailConfig};
use zygo_core::paths::Paths;
use zygo_mail_sync::address::Address;
use zygo_mail_sync::branch::{self, BranchError, Fetched};
use zygo_mail_sync::env::{self, Crash, Env, Seam, Spawner, Verdict, Wake};
use zygo_mail_sync::identity::{read_meta, TipMeta};
use zygo_mail_sync::prune::{self, Pruned, COMMITS_DUE, OWN_ACK_DUE};
use zygo_mail_sync::state;
use zygo_mail_sync::sync::{run, run_with, Mode, Opts, Report, SyncError};
use zygo_redact::DeadlinePolicy;
use zygo_workspace::state::Workspace;

/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;
/// One year, in seconds (S§19 #18).
const YEAR: u64 = 365 * 86_400;
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
    let c1500 = grow(&m.root, "refs/heads/t", 500); // MS-14 review F7: the bounded count still answers 1000 or more
    assert_eq!(prune::due(&m.root, &c1500, 0, false, far()), Ok(true));
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

/// One CLI sync of `m` with `zygo mail sync --prune`, the clock at `secs`.
fn prune_now(m: &Machine, secs: u64) -> Report {
    run_with(&env_at(m, Hook::none(), secs), WS_ID, Mode::Cli, &Opts { prune: true }).unwrap()
}

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
fn imported(r: &Remote, path: &str) -> BTreeSet<String> {
    let Some(b) = tip_file(r, BRANCH, path) else { return BTreeSet::new() };
    let v: Value = serde_json::from_slice(&b).unwrap();
    v["imported"].as_array().unwrap().iter().map(|u| u.as_str().unwrap().to_string()).collect()
}

/// The file names in `mail/<key>/new/` of `m`; empty when absent.
fn new_ids(m: &Machine, key: &str) -> BTreeSet<String> {
    let Ok(d) = fs::read_dir(m.sp().mail.join(key).join("new")) else { return BTreeSet::new() };
    d.map(|e| e.unwrap().file_name().into_string().unwrap()).collect()
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

/// A new commit on the bare tip: `adds` (mode, path, bytes) put in and `removes` taken out of the tip's tree.
fn edit_tip(r: &Remote, adds: &[(&str, &str, &[u8])], removes: &[&str]) {
    let tip = r.tip(BRANCH).unwrap();
    let idx = r.dir.path().join("edit-index");
    let _ = fs::remove_file(&idx);
    let g = |args: &[&str]| {
        let o = plain(&r.bare).env("GIT_INDEX_FILE", &idx).args(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim_end().to_string()
    };
    g(&["read-tree", &tip]);
    for (mode, path, bytes) in adds {
        let b = blob(&r.bare, bytes);
        g(&["update-index", "--add", "--cacheinfo", &format!("{mode},{b},{path}")]);
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
fn t1b_opts_default_is_no_forced_prune() {
    assert_eq!(Opts::default(), Opts { prune: false });
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

#[test] // FX-40 review F2, S§9 Push: with two push URLs, a lease push that one takes and the other refuses is `Split`,
        // never `LeaseLost`: the first URL's history was rewritten
fn prune_taken_by_one_push_url_only_is_split() {
    let (r, a, b) = pair();
    send(&a, "alice", ("bob", "b"), "zero");
    sync(&a);
    sync(&b);
    let r2 = r.mirror();
    common::push_urls(&a, &[&r.url, &r2.url]);
    let moved = advance(&r2, BRANCH);
    let base = fetched(&a);
    let meta = read_meta(&a.wt, &base, far(), &mut |_| {}).unwrap();
    assert_eq!(prune::run(&a.wt, "origin", BRANCH, &base, &meta, far()), Ok(Pruned::Split));
    assert_eq!(count(&r), 1);
    assert_eq!(r2.tip(BRANCH), Some(moved));
}

#[test] // FX-40 review F2, S§9 Push, S§13: a forced prune that one push URL takes and the other refuses stops the pass
        // with the `push` line (exit 1), and so does the next pass while the URLs disagree
fn forced_prune_split_across_push_urls_stops_the_pass() {
    let (r, a, b) = pair();
    send(&a, "alice", ("bob", "b"), "zero");
    sync(&a);
    sync(&b);
    sync(&a);
    let r2 = r.mirror();
    common::push_urls(&a, &[&r.url, &r2.url]);
    advance(&r2, BRANCH);
    let stop = SyncError::Stopped { line: "zygo: mail sync stopped (push); the outbox is kept".into(), check: "push" };
    for _ in 0..2 {
        let e = run_with(&env_at(&a, Hook::none(), T0), WS_ID, Mode::Cli, &Opts { prune: true }).unwrap_err();
        assert_eq!(e, stop);
        assert_eq!(e.exit_code(Mode::Cli), 1);
    }
    assert!(a.errors().contains("push"), "{}", a.errors());
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
        edit_tip(&r, &[("100644", &format!("carol@c/{u}.json"), b"{}"), ("100644", "ack/c.json", &ack)], &[]);
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

#[test] // S§19 #17a, S§9 Base, S§8.3 step 16, review focus 10 (base older than this pass's own push)
fn t3_prune_after_push_keeps_the_pushed_message() {
    let (r, a, b) = pair();
    let u0 = send(&a, "alice", ("bob", "b"), "zero");
    sync(&a);
    sync(&b);
    assert!(imported(&r, "ack/b.json").contains(&u0.to_string()));
    grow(&r.bare, "refs/heads/zygo/mail", 999 - count(&r));
    let before = r.tip(BRANCH);
    sync(&a); // nothing to push; 999 commits: not due
    assert_eq!((r.tip(BRANCH), count(&r)), (before, 999));
    let u1 = send(&a, "alice", ("bob", "b"), "one");
    assert_eq!(sync(&a).pushed, 1); // the push makes 1000 commits: the same pass prunes
    assert_eq!(count(&r), 1);
    assert!(at_tip(&r, "bob@b", u1));
    assert!(!at_tip(&r, "bob@b", u0));
    assert_eq!(tip_file(&r, BRANCH, "ZYGO-MAIL").unwrap(), b"1\n");
    for p in ["registry/a.json", "registry/b.json", "ack/a.json", "ack/b.json"] {
        assert!(tip_file(&r, BRANCH, p).is_some(), "{p}");
    }
    assert_eq!(git(&r.bare, &["log", "-1", "--format=%s", "refs/heads/zygo/mail"]), "zygo mail [skip ci]");
    sync(&b);
    assert_eq!(new_ids(&b, BOB), BTreeSet::from([format!("{u0}.json"), format!("{u1}.json")]));
    assert_eq!(imported(&r, "ack/b.json"), BTreeSet::from([u1.to_string()]));
}

#[test] // S§19 #17b, S§14 "A prunes while B pushes" (B's push after A's base)
fn t4_a_push_before_the_prune_push_loses_the_lease() {
    let (r, a, _b) = pair();
    grow(&r.bare, "refs/heads/zygo/mail", 999 - count(&r));
    let u1 = send(&a, "alice", ("bob", "b"), "one");
    let r = Arc::new(r);
    let (r2, moved) = (r.clone(), Arc::new(Mutex::new(None)));
    let m2 = moved.clone();
    let h = Hook::new(move |p| {
        if p == "before_prune_push" {
            *m2.lock().unwrap() = Some(advance(&r2, BRANCH));
        }
        Ok(())
    });
    assert_eq!(run(&env_at(&a, h, T0), WS_ID, Mode::Cli).unwrap().pushed, 1);
    let other = moved.lock().unwrap().clone().expect("the prune was due");
    assert_eq!(r.tip(BRANCH), Some(other));
    assert_eq!(count(&r), 1001);
    assert!(at_tip(&r, "bob@b", u1));
    sync(&a); // the next pass tries again
    assert_eq!(count(&r), 1);
    assert!(at_tip(&r, "bob@b", u1));
}

#[test] // S§14 "Crash during a prune": the branch is the base or the new root
fn t5_crash_during_a_prune_leaves_the_base() {
    let (r, a, _b) = pair();
    grow(&r.bare, "refs/heads/zygo/mail", 999 - count(&r));
    let u1 = send(&a, "alice", ("bob", "b"), "one");
    let e = run(&env_at(&a, Hook::crash_at("before_prune_push"), T0), WS_ID, Mode::Cli).unwrap_err();
    assert_eq!(e, SyncError::Stopped { line: "zygo: stopped at seam before_prune_push".into(), check: "seam" });
    assert_eq!(count(&r), 1000);
    assert!(at_tip(&r, "bob@b", u1));
    assert!(!a.sp().outbox.join(format!("{u1}.json")).exists());
    sync(&a);
    assert_eq!(count(&r), 1);
    assert!(at_tip(&r, "bob@b", u1));
}

#[test] // S§14 "A prunes while B pushes" (A's prune lands first; B builds on the new root)
fn t7_a_prune_that_lands_first_makes_the_push_build_on_the_new_root() {
    let (r, a, b) = pair();
    let u0 = send(&a, "alice", ("bob", "b"), "zero");
    sync(&a);
    sync(&b);
    let ib = send(&b, "bob", ("alice", "a"), "from b");
    let a = Arc::new(a);
    let a2 = a.clone();
    let mut n = 0;
    let h = Hook::new(move |p| {
        if p == "before_push" {
            n += 1;
            if n == 1 {
                run_with(&env_at(&a2, Hook::none(), T0), WS_ID, Mode::Cli, &Opts { prune: true }).unwrap();
            }
        }
        Ok(())
    });
    assert_eq!(run(&env_at(&b, h, T0), WS_ID, Mode::Cli).unwrap().pushed, 1);
    assert_eq!(count(&r), 2);
    assert!(at_tip(&r, "alice@a", ib));
    assert!(!at_tip(&r, "bob@b", u0));
}

#[test] // S§19 #18, S-C8, S§14 "Different keep on A and B", "B idle for months", S§10.3
fn t8_prune_keeps_what_no_ack_lists_whatever_the_clock_or_keep() {
    let (r, a, b) = machines_with("keep = \"1d\"\n", "keep = \"90d\"\n");
    let u0 = send(&b, "bob", ("alice", "a"), "to a");
    sync(&b); // B is idle from here
    let u1 = send(&a, "alice", ("bob", "b"), "one");
    let u2 = send(&a, "alice", ("bob", "b"), "two");
    sync(&a);
    assert!(imported(&r, "ack/a.json").contains(&u0.to_string()));
    prune_now(&a, T0 + YEAR);
    assert_eq!(count(&r), 1);
    assert!(!at_tip(&r, "alice@a", u0));
    assert!(at_tip(&r, "bob@b", u1) && at_tip(&r, "bob@b", u2));
    for p in ["registry/a.json", "registry/b.json", "ack/a.json", "ack/b.json"] {
        assert!(tip_file(&r, BRANCH, p).is_some(), "{p}");
    }
    run(&env_at(&b, Hook::none(), T0 + YEAR), WS_ID, Mode::Cli).unwrap();
    run(&env_at(&b, Hook::none(), T0 + YEAR), WS_ID, Mode::Cli).unwrap();
    assert_eq!(new_ids(&b, BOB), BTreeSet::from([format!("{u1}.json"), format!("{u2}.json")]));
    assert_eq!(imported(&r, "ack/b.json"), BTreeSet::from([u1.to_string(), u2.to_string()]));
    run(&env_at(&a, Hook::none(), T0 + YEAR), WS_ID, Mode::Cli).unwrap();
    assert!(imported(&r, "ack/a.json").is_empty()); // S§7.4: the owner drops the pruned ULID
}

#[test] // S§19 #15 prune part
fn t9_a_message_a_full_inbox_did_not_ack_survives_a_prune() {
    let (r, a, b) = machines_with("", "max_unread = 1\n");
    let new = b.sp().mail.join(BOB).join("new");
    fs::create_dir_all(&new).unwrap();
    let old = format!("{}.json", Ulid::from_parts(T0 * 1000, 1));
    fs::write(new.join(&old), b"{}").unwrap();
    let u1 = send(&a, "alice", ("bob", "b"), "one");
    sync(&a);
    sync(&b);
    assert_eq!(new_ids(&b, BOB), BTreeSet::from([old.clone()]));
    assert!(!imported(&r, "ack/b.json").contains(&u1.to_string()));
    prune_now(&a, T0);
    assert_eq!(count(&r), 1);
    assert!(at_tip(&r, "bob@b", u1));
    let cur = b.sp().mail.join(BOB).join("cur");
    fs::create_dir_all(&cur).unwrap();
    fs::rename(new.join(&old), cur.join(&old)).unwrap(); // bob reads
    sync(&b);
    sync(&b);
    assert_eq!(new_ids(&b, BOB), BTreeSet::from([format!("{u1}.json")]));
}

#[test] // S§19 #14 push-crash part, S§14 "Crash after A's push, before the outbox delete", S-G5
fn t10_crash_after_push_then_prune_gives_no_second_delivery() {
    let (r, a, b) = pair();
    let u1 = send(&a, "alice", ("bob", "b"), "one");
    run(&env_at(&a, Hook::crash_at("before_outbox_delete"), T0), WS_ID, Mode::Cli).unwrap_err();
    assert!(a.sp().outbox.join(format!("{u1}.json")).is_file());
    sync(&b);
    assert_eq!(new_ids(&b, BOB), BTreeSet::from([format!("{u1}.json")]));
    prune_now(&b, T0);
    assert!(!at_tip(&r, "bob@b", u1));
    sync(&b);
    assert!(!imported(&r, "ack/b.json").contains(&u1.to_string()));
    assert_eq!(sync(&a).pushed, 1); // the path is not at the tip: A adds it again
    assert!(at_tip(&r, "bob@b", u1));
    assert!(!a.sp().outbox.join(format!("{u1}.json")).exists());
    sync(&b);
    assert_eq!(new_ids(&b, BOB), BTreeSet::from([format!("{u1}.json")])); // zero more deliveries
    assert!(imported(&r, "ack/b.json").contains(&u1.to_string())); // acked again
    prune_now(&b, T0);
    assert!(!at_tip(&r, "bob@b", u1)); // a later prune removes it
}

#[test] // S§20 SP5 (as a CI test)
fn sp5_plain_fetch_after_a_prune() {
    let (r, a, b) = pair();
    send(&a, "alice", ("bob", "b"), "zero");
    sync(&a);
    sync(&b);
    let old = git(&b.root, &["rev-parse", "refs/remotes/origin/zygo/mail"]);
    prune_now(&a, T0);
    let root = r.tip(BRANCH).unwrap();
    assert_ne!(old, root);
    fs::write(a.root.join("work.txt"), "w\n").unwrap();
    git(&a.root, &["add", "work.txt"]);
    git(&a.root, &["commit", "-q", "-m", "work"]);
    git(&a.root, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let work = git(&a.root, &["rev-parse", "HEAD"]);
    let o = plain(&b.root).arg("fetch").output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(git(&b.root, &["rev-parse", "refs/remotes/origin/zygo/mail"]), root);
    let o = plain(&b.root).args(["pull", "-q"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(git(&b.root, &["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert_eq!(git(&b.root, &["rev-parse", "HEAD"]), work);
    assert_eq!(git(&b.root, &["status", "--porcelain"]), "");
}

#[test] // MS-14 review F1, S§9 Push (D11): a prune push the server refuses skips the prune, not the pass
fn t13_a_refused_prune_push_skips_the_prune_not_the_pass() {
    let (r, a, _b) = pair();
    git(&r.bare, &["config", "receive.denyNonFastForwards", "true"]);
    grow(&r.bare, "refs/heads/zygo/mail", 999 - count(&r));
    let u1 = send(&a, "alice", ("bob", "b"), "one");
    let rep = sync(&a);
    assert_eq!(rep.pushed, 1);
    assert_eq!(rep.notes, vec!["prune skipped (push)".to_string()]);
    assert_eq!(count(&r), 1000);
    assert!(at_tip(&r, "bob@b", u1));
    assert!(!a.sp().outbox.join(format!("{u1}.json")).exists());
    let st = state::load(&a.sp()).unwrap();
    assert_eq!((st.last_ok, st.last_error), (Some(T0), None));
}

#[test] // MS-14 review F2: a forced prune that is done says so
fn t14_a_forced_prune_reports_the_files_it_removed() {
    let (r, a, b) = pair();
    let u0 = send(&a, "alice", ("bob", "b"), "zero");
    sync(&a);
    sync(&b);
    assert_eq!(prune_now(&a, T0).notes, vec!["pruned 1 files".to_string()]);
    assert_eq!(count(&r), 1);
    assert!(!at_tip(&r, "bob@b", u0));
}

#[test] // MS-14 review F2: a forced prune that loses its lease says to run again
fn t15_a_forced_prune_that_loses_the_lease_says_run_again() {
    let (r, a, _b) = pair();
    let r = Arc::new(r);
    let (r2, moved) = (r.clone(), Arc::new(Mutex::new(None)));
    let m2 = moved.clone();
    let h = Hook::new(move |p| {
        if p == "before_prune_push" {
            *m2.lock().unwrap() = Some(advance(&r2, BRANCH));
        }
        Ok(())
    });
    let rep = run_with(&env_at(&a, h, T0), WS_ID, Mode::Cli, &Opts { prune: true }).unwrap();
    assert_eq!(rep.notes, vec!["prune skipped: the branch moved; run again".to_string()]);
    assert_eq!(r.tip(BRANCH), moved.lock().unwrap().clone());
}

#[test] // MS-14 review F6, S§9 Tree "Nothing else is removed": only regular-file blobs are message files
fn t16_an_acknowledged_symlink_entry_stays() {
    let (r, a, b) = pair();
    let u0 = send(&a, "alice", ("bob", "b"), "zero");
    let u1 = send(&a, "alice", ("bob", "b"), "one");
    sync(&a);
    sync(&b);
    assert!(imported(&r, "ack/b.json").is_superset(&BTreeSet::from([u0.to_string(), u1.to_string()])));
    edit_tip(&r, &[("120000", &format!("bob@b/{u0}.json"), b"x"), ("100755", &format!("bob@b/{u1}.json"), b"{}")], &[]);
    let base = fetched(&a);
    let meta = read_meta(&a.wt, &base, far(), &mut |_| {}).unwrap();
    assert_eq!(prune::run(&a.wt, "origin", BRANCH, &base, &meta, far()), Ok(Pruned::Done { removed: 1 }));
    assert!(at_tip(&r, "bob@b", u0));
    assert!(!at_tip(&r, "bob@b", u1));
}

#[test] // S§7.6 Rename (b), S§8.5 Checks (FX-27): a link that a full inbox left unacked keeps the old ack
fn t17_renamed_host_ack_stays_while_a_full_inbox_leaves_a_link_unacked() {
    let (r, a, b) = machines_with("", "max_unread = 1\n");
    let m = send(&a, "alice", ("bob", "b"), "m");
    sync(&a);
    sync(&b);
    assert_eq!(new_ids(&b, BOB), BTreeSet::from([format!("{m}.json")])); // the inbox is full
    assert!(prune_now(&b, T0).notes.contains(&"pruned 1 files".to_string()));
    assert!(!at_tip(&r, "bob@b", m));
    let l = Ulid::from_parts(T0 * 1000, 7);
    edit_tip(&r, &[("120000", &format!("bob@b/{l}.json"), b"../ZYGO-MAIL")], &[]);
    write_config(&b, "b2", "max_unread = 1\n");
    assert_eq!(sync(&b).delivered, 0);
    assert_eq!(tip_file(&r, BRANCH, "registry/b.json"), None);
    assert!(tip_file(&r, BRANCH, "ack/b.json").is_some());
    assert_eq!(imported(&r, "ack/b.json"), BTreeSet::new());
    assert!(at_tip(&r, "bob@b", l));
    let inbox = b.sp().mail.join(BOB);
    fs::create_dir_all(inbox.join("cur")).unwrap();
    fs::rename(inbox.join("new").join(format!("{m}.json")), inbox.join("cur").join(format!("{m}.json"))).unwrap();
    sync(&b);
    assert_eq!(tip_file(&r, BRANCH, "ack/b.json"), None);
}

#[test] // S§9 Tree, S§7.4 Meaning (FX-27 round-3 F1 guard): a member rename does not keep a delivered file
fn t18_a_member_rename_does_not_keep_a_delivered_file_from_the_prune() {
    let (r, a, b) = pair();
    let m = send(&a, "alice", ("bob", "b"), "m");
    sync(&a);
    sync(&b);
    assert!(imported(&r, "ack/b.json").contains(&m.to_string()));
    b.workspace(&[(BOB, "robert")]);
    sync(&b);
    let reg: Value = serde_json::from_slice(&tip_file(&r, BRANCH, "registry/b.json").unwrap()).unwrap();
    assert_eq!(reg["members"], serde_json::json!(["robert"]));
    assert!(prune_now(&b, T0).notes.contains(&"pruned 1 files".to_string()));
    assert!(!at_tip(&r, "bob@b", m));
    assert_eq!(sync(&b).delivered, 0);
    assert_eq!(new_ids(&b, BOB), BTreeSet::from([format!("{m}.json")]));
}

#[test] // S§9 Tree (MS-14 review F6, FX-27): a message file is a blob of mode 100644 or 100755
fn t19_removable_is_a_message_file_blob_only() {
    let e = |mode: &str, kind: &str| branch::Entry {
        mode: mode.into(),
        kind: kind.into(),
        id: "0".repeat(40),
        size: None,
        path: "bob@b/x.json".into(),
    };
    assert!(prune::removable(&e("100644", "blob")));
    assert!(prune::removable(&e("100755", "blob")));
    assert!(!prune::removable(&e("120000", "blob")));
    assert!(!prune::removable(&e("160000", "commit")));
    assert!(!prune::removable(&e("040000", "tree")));
    assert!(!prune::removable(&e("100644", "tree")));
}

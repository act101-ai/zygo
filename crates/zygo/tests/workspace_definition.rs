//! WD-14: pinned launches (R§7.4, R§18 items 15 and 16). L-tests run the launcher in process with an injected git
//! runner. B-tests run `zygo ws run` and E-tests `zygo up` on tmux; both launch the stub agent and are
//! `#[ignore = "timing: …"]` as the agent-launch tests of `workspace_run.rs` and `workspace_restore.rs`. Workspace
//! state and the test shell need unix, so the file is `cfg(unix)` (OP5).
#![cfg(unix)]
mod common;
#[path = "common/e2e.rs"]
mod e2e;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::workspace::{ready_exe, Guarded, Harness, WS_ID};
use e2e::*;
use serde_json::{json, Value};
use zygo_config::profile::Profile;
use zygo_config::repo_def::{approve_workspace_in, check_default, GitRunner, Pin, SystemGit, Use};
use zygo_core::paths::Paths;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;
use zygo_launch::env::Env;
use zygo_launch::{AgentAdapter, ChildSpec, LaunchCtx, LaunchOpts, Launcher, Preflight};
use zygo_proxy::session::SessionConfig;
use zygo_work::repo::checkout;
use zygo_workspace::identity::workspace_id;
use zygo_workspace::state::{MemberRecord, MemberState, Pane, PaneKind};

const RUST: &str = "overrides = { code = \"claude-haiku-5-5\" }\ndisabled = [\"design\"]\n";
const NEWER: &str = "disabled = [\"review\"]\n";
const NO_LONGER: &str = "this repository no longer has or no longer approves";
const KEY: &str = "kaaaaaaaa";
const OLD_ZS: &str = "01JAAAAAAAAAAAAAAAAAAAAAAA";

/// A test-side git call: no user or system config, no signing, a fixed identity; asserts success; trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-C"])
        .arg(dir)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("git on PATH");
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// `<root>/app`: a clone of the bare `<root>/o.git`, whose `main` holds `files` (paths from the repository root),
/// so `refs/remotes/origin/HEAD` is `refs/remotes/origin/main` (R§18 preamble).
fn repo_at(root: &Path, files: &[(&str, &str)]) -> PathBuf {
    let w = root.join("w");
    std::fs::create_dir_all(&w).unwrap();
    for (path, body) in files {
        let p = w.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    git(root, &["init", "-q", "--bare", "-b", "main", "o.git"]);
    git(&w, &["init", "-q", "-b", "main"]);
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "-q", "-m", "c"]);
    git(&w, &["remote", "add", "origin", "../o.git"]);
    git(&w, &["push", "-q", "origin", "main"]);
    git(root, &["clone", "-q", "o.git", "app"]);
    root.join("app")
}

/// The workspace id of `app`, from the common dir that the launcher's own `checkout` finds.
fn ws_id(app: &Path) -> String {
    workspace_id(&checkout(app, Instant::now() + Duration::from_secs(3600)).unwrap().common_dir)
}

/// Approves the default branch's files of `app` when they are not approved yet; returns the pin of the approved read.
fn approve(app: &Path, approvals: &Path) -> Pin {
    let id = ws_id(app);
    if let Use::NotApproved { digest, .. } = check_default(app, &id, approvals, &SystemGit) {
        approve_workspace_in(approvals, &id, &digest).unwrap();
    }
    match check_default(app, &id, approvals, &SystemGit) {
        Use::Approved { pin, .. } => pin,
        other => panic!("not approved: {other:?}"),
    }
}

/// Commits `.zygo/profiles/rust.toml` = `body` in `app` and pushes it to `main`: the default branch moves.
fn commit_default(app: &Path, body: &str) {
    std::fs::write(app.join(".zygo/profiles/rust.toml"), body).unwrap();
    git(app, &["add", "-A"]);
    git(app, &["commit", "-q", "-m", "change"]);
    git(app, &["push", "-q", "origin", "HEAD:main"]);
}

/// R§18 item 16: makes `pinned` unreachable in `app` (an orphan commit with `NEWER` becomes `main` and `origin/main`),
/// expires the reflogs and prunes; asserts that `app` no longer has `pinned`.
fn prune(app: &Path, pinned: &str) {
    git(app, &["checkout", "-q", "--orphan", "gone"]);
    std::fs::write(app.join(".zygo/profiles/rust.toml"), NEWER).unwrap();
    git(app, &["add", "-A"]);
    git(app, &["commit", "-q", "-m", "orphan"]);
    git(app, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(app, &["branch", "-q", "-f", "main", "HEAD"]);
    git(app, &["checkout", "-q", "main"]);
    git(app, &["branch", "-q", "-D", "gone"]);
    git(app, &["reflog", "expire", "--expire=now", "--all"]);
    git(app, &["gc", "-q", "--prune=now"]);
    let o = std::process::Command::new("git").arg("-C").arg(app).args(["cat-file", "-e", pinned]).output().unwrap();
    assert!(!o.status.success(), "the pinned commit {pinned} is still in the repository");
}

/// The R§7.4 line for profile `rust` and `pin`, with `which` after "which".
fn pin_line(pin: &Pin, which: &str) -> String {
    format!(
        "zygo: profile rust of this member comes from the workspace files at commit {} (digest {}), which {which}; the agent was not started; to start from the current files: zygo down, then zygo up --reset",
        &pin.commit[..12],
        &pin.digest[..12]
    )
}

/// The lines of `err` equal to `line`.
fn count(err: &str, line: &str) -> usize {
    err.lines().filter(|l| *l == line).count()
}

/// The built-in layer without its marketplace (no network).
fn builtin() -> toml::Table {
    let mut t = toml::Table::try_from(zygo_config::builtin::defaults()).unwrap();
    t.remove("marketplace");
    t
}

/// Records the args of every call, then runs `SystemGit`.
#[derive(Default)]
struct Rec(Mutex<Vec<Vec<String>>>);

impl GitRunner for Rec {
    fn git(&self, top: &Path, args: &[&str], deadline: Instant) -> Result<Output, String> {
        self.0.lock().unwrap().push(args.iter().map(|a| a.to_string()).collect());
        SystemGit.git(top, args, deadline)
    }
}

impl Rec {
    /// A copy of the args of every call so far.
    fn calls(&self) -> Vec<Vec<String>> {
        self.0.lock().unwrap().clone()
    }
}

/// Records what `build` sees, then the preflight refusal stops the launch: no child, no 10 s status call.
struct Stub {
    program: String,
    refuse: PathBuf,
    seen: Mutex<Option<(Option<Profile>, SessionConfig, SessionView)>>,
}

impl AgentAdapter for Stub {
    fn agent(&self) -> Agent {
        Agent::ClaudeCode
    }
    fn program(&self) -> &str {
        &self.program
    }
    fn install_hint(&self) -> &str {
        "stub"
    }
    fn picked_model(&self, _args: &[String]) -> Option<String> {
        None
    }
    fn build(&self, ctx: &LaunchCtx) -> anyhow::Result<ChildSpec> {
        *self.seen.lock().unwrap() = Some((ctx.profile.clone(), ctx.config.clone(), ctx.view.clone()));
        Ok(ChildSpec { args: vec![], env: vec![], lines: vec![], log: vec![] })
    }
    fn preflight(&self, _ctx: &LaunchCtx) -> Result<(), Preflight> {
        Err(Preflight { file: self.refuse.clone(), key: "env.ANTHROPIC_API_KEY".into() })
    }
}

/// Fails each `ls-tree` call with `tool not found`; every other call runs `SystemGit`.
struct FailLsTree;

impl GitRunner for FailLsTree {
    fn git(&self, top: &Path, args: &[&str], deadline: Instant) -> Result<Output, String> {
        if args.first() == Some(&"ls-tree") {
            return Err("tool not found".into());
        }
        SystemGit.git(top, args, deadline)
    }
}

struct Fx {
    _t: tempfile::TempDir,
    root: PathBuf,
    paths: Paths,
    app: PathBuf,
}

/// A scratch home and `app` with `RUST` as `.zygo/profiles/rust.toml` on `main` (not approved).
fn fx() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    let h = root.join("home");
    let paths = Paths {
        config_file: h.join("config.toml"),
        cache_dir: h.join("cache"),
        data_dir: h.join("data"),
        sessions_dir: h.join("cache/sessions"),
        marketplaces_dir: h.join("cache/marketplaces"),
        plugin_store: h.join("data/plugins"),
        approvals_file: h.join("data/approvals.toml"),
        usage_db: h.join("data/usage.db"),
    };
    for d in [&h, &root.join("bin")] {
        std::fs::create_dir_all(d).unwrap();
    }
    let app = repo_at(&root, &[(".zygo/profiles/rust.toml", RUST)]);
    Fx { _t: t, root, paths, app }
}

/// What one in-process launch gave: its code, its captured lines, what `build` saw (when it ran), and whether the
/// session callback ran (AU92: the record write).
struct Run {
    code: i32,
    err: String,
    seen: Option<(Option<Profile>, SessionConfig, SessionView)>,
    called: bool,
}

/// `Launcher::run` of the stub with `--profile rust`, cwd `fx.app`, `PATH` an empty `bin`, `builtin()`, the
/// captured lines, the runner `git` and a session callback that records its call; then `f`. The stub's preflight
/// refuses, so a launch that reaches the agent exits 2 after `build`.
fn run(fx: &Fx, git: Arc<dyn GitRunner + Send + Sync>, f: impl FnOnce(&mut LaunchOpts)) -> Run {
    let env = vec![
        (OsString::from("PATH"), OsString::from(fx.root.join("bin"))),
        (OsString::from("ACT_CONFIG_DIR"), OsString::from(fx.root.join("act"))),
    ];
    let mut o = LaunchOpts::new(Env(env), fx.app.clone(), fx.paths.clone());
    o.builtin = Some(builtin());
    let buf: Arc<Mutex<Vec<u8>>> = Arc::default();
    o.stderr = Some(buf.clone());
    o.definition_git = Some(git);
    o.profile = Some("rust".into());
    let called = Arc::new(AtomicBool::new(false));
    let c = called.clone();
    o.on_session = Some(Box::new(move |_, _, _, _, _| {
        c.store(true, Ordering::SeqCst);
        Ok(())
    }));
    f(&mut o);
    let stub = Stub {
        program: std::env::current_exe().unwrap().display().to_string(),
        refuse: fx.root.join("settings.json"),
        seen: Mutex::new(None),
    };
    let code = Launcher::run(&stub, o).code;
    let err = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    Run { code, err, seen: stub.seen.lock().unwrap().take(), called: called.load(Ordering::SeqCst) }
}

#[test]
fn l1_a_pinned_launch_sends_the_profile_of_the_pinned_commit() { // R§7.4 bullets 3, 7; R§18 item 16 (a), (b); review focus 33
    for approve_new in [true, false] {
        let fx = fx();
        let pin0 = approve(&fx.app, &fx.paths.approvals_file);
        commit_default(&fx.app, NEWER);
        if approve_new {
            assert_ne!(approve(&fx.app, &fx.paths.approvals_file), pin0);
        }
        let rec = Arc::new(Rec::default());
        let r = run(&fx, rec.clone(), |o| o.definition = Some(pin0.clone()));
        assert_eq!(r.code, 2, "{}", r.err);
        let (profile, config, view) = r.seen.expect("build ran");
        let p = profile.expect("the pinned profile was sent");
        assert_eq!(p.name, "rust");
        assert_eq!(p.overrides, BTreeMap::from([("code".to_string(), "claude-haiku-5-5".to_string())]));
        assert_eq!(p.disabled, ["design"]);
        assert_eq!(config.roles["code"].model, "claude-haiku-5-5");
        assert!(!config.roles.contains_key("design") && config.roles.contains_key("review"));
        assert_eq!(view.disabled_roles, ["design"]);
        let calls = rec.calls();
        assert!(calls.iter().all(|c| c[0] != "symbolic-ref" && c[0] != "show-ref"), "a default-branch lookup: {calls:?}");
        assert!(calls.iter().any(|c| c[0] == "ls-tree" && c.contains(&pin0.commit)), "{calls:?}");
        assert!(!r.err.contains("profile"), "{}", r.err);
        assert!(r.called);
    }
}

#[test]
fn l2_a_failing_pinned_listing_stops_before_the_session() { // R§7.4 bullet 5 (rewritten); R§18 item 16
    let fx = fx();
    let pin = approve(&fx.app, &fx.paths.approvals_file);
    let r = run(&fx, Arc::new(FailLsTree), |o| o.definition = Some(pin.clone()));
    assert_eq!(r.code, 1, "{}", r.err);
    assert!(r.seen.is_none() && !r.called, "the launch went on: {}", r.err);
    assert_eq!(count(&r.err, &pin_line(&pin, "were not read (git ls-tree: tool not found)")), 1, "{}", r.err);
    assert!(!r.err.contains("launching without"), "{}", r.err);
}

#[test]
fn l3_a_pruned_pin_stops_with_the_no_longer_line() { // R§7.4 bullet 4; R§11 row; R§18 item 16
    let fx = fx();
    let pin = approve(&fx.app, &fx.paths.approvals_file);
    prune(&fx.app, &pin.commit);
    let r = run(&fx, Arc::new(Rec::default()), |o| o.definition = Some(pin.clone()));
    assert_eq!(r.code, 1, "{}", r.err);
    assert!(r.seen.is_none() && !r.called, "the launch went on: {}", r.err);
    assert_eq!(count(&r.err, &pin_line(&pin, NO_LONGER)), 1, "{}", r.err);
}

#[test]
fn l4_a_pin_whose_approval_is_removed_stops() { // R§7.4 bullet 4 ("not in the store any more"); R§11 row
    let fx = fx();
    let pin = approve(&fx.app, &fx.paths.approvals_file);
    std::fs::write(&fx.paths.approvals_file, "").unwrap();
    let r = run(&fx, Arc::new(Rec::default()), |o| o.definition = Some(pin.clone()));
    assert_eq!(r.code, 1, "{}", r.err);
    assert!(r.seen.is_none() && !r.called);
    assert_eq!(count(&r.err, &pin_line(&pin, NO_LONGER)), 1, "{}", r.err);
}

#[test]
fn l5_a_pin_with_another_digest_stops() { // R§7.4 bullet 3: the pinned read must give the saved digest
    let fx = fx();
    let pin = approve(&fx.app, &fx.paths.approvals_file);
    let other = Pin::new(pin.commit.clone(), "0".repeat(64));
    let r = run(&fx, Arc::new(Rec::default()), |o| o.definition = Some(other.clone()));
    assert_eq!(r.code, 1, "{}", r.err);
    assert!(r.seen.is_none() && !r.called);
    assert_eq!(count(&r.err, &pin_line(&other, NO_LONGER)), 1, "{}", r.err);
}

#[test]
fn l6_passthrough_does_no_pinned_read_and_is_not_stopped() { // R§7.4 bullet 6; R§18 item 16 (pool member)
    let fx = fx();
    let pin = approve(&fx.app, &fx.paths.approvals_file);
    prune(&fx.app, &pin.commit);
    let rec = Arc::new(Rec::default());
    let r = run(&fx, rec.clone(), |o| {
        o.definition = Some(pin.clone());
        o.passthrough = true;
    });
    assert_eq!(r.code, 2, "{}", r.err);
    assert!(r.seen.expect("build ran").0.is_none());
    assert!(rec.calls().is_empty(), "{:?}", rec.calls());
    assert_eq!(count(&r.err, "zygo: profile rust does not apply in passthrough"), 1, "{}", r.err);
}

#[test]
fn l7_a_user_profile_of_the_name_wins_over_a_pin_and_nothing_is_read() { // R§8 user first; R§7.4 bullet 3
    let fx = fx();
    let pin = approve(&fx.app, &fx.paths.approvals_file);
    prune(&fx.app, &pin.commit);
    std::fs::write(&fx.paths.config_file, "[profile.rust]\ndisabled = [\"review\"]\n").unwrap();
    let rec = Arc::new(Rec::default());
    let r = run(&fx, rec.clone(), |o| o.definition = Some(pin.clone()));
    assert_eq!(r.code, 2, "{}", r.err);
    assert_eq!(r.seen.expect("build ran").0.expect("the user profile").disabled, ["review"]);
    assert!(rec.calls().is_empty(), "{:?}", rec.calls());
}

#[test]
fn l8_an_approved_pin_without_the_name_gives_the_not_found_line() { // R§7.4 bullet 3 (rewritten)
    let fx = fx();
    let pin = approve(&fx.app, &fx.paths.approvals_file);
    let r = run(&fx, Arc::new(Rec::default()), |o| {
        o.profile = Some("go".into());
        o.definition = Some(pin.clone());
    });
    assert_eq!(r.code, 2, "{}", r.err);
    assert!(r.seen.expect("build ran").0.is_none());
    assert_eq!(count(&r.err, "zygo: profile go not found; launching without it"), 1, "{}", r.err);
}

#[test]
fn l9_the_stop_line_escapes_the_saved_commit() { // R§7.4 bullet 4 (rewritten), R-C7, C6
    let fx = fx();
    let pin = approve(&fx.app, &fx.paths.approvals_file);
    let bad = Pin::new("\u{1b}[2Jab".into(), pin.digest.clone());
    let r = run(&fx, Arc::new(Rec::default()), |o| o.definition = Some(bad.clone()));
    assert_eq!(r.code, 1, "{}", r.err);
    assert!(!r.err.contains('\u{1b}'), "{:?}", r.err);
    let line = format!(
        "zygo: profile rust of this member comes from the workspace files at commit \\u{{1b}}[2Jab (digest {}), which were not read (pinned commit \\u{{1b}}[2Jab is not an object id); the agent was not started; to start from the current files: zygo down, then zygo up --reset",
        &pin.digest[..12]
    );
    assert_eq!(count(&r.err, &line), 1, "{:?}", r.err);
}

#[test]
fn l10_a_default_branch_read_gives_its_pin_to_the_caller() { // R§7.4 bullet 2 (rewritten), R§12 row (rewritten)
    let fx = fx();
    let cell: Arc<Mutex<Option<Pin>>> = Arc::default();
    let r = run(&fx, Arc::new(Rec::default()), |o| o.definition_out = Some(cell.clone()));
    assert_eq!((r.code, cell.lock().unwrap().clone()), (2, None), "not approved: {}", r.err);
    let pin = approve(&fx.app, &fx.paths.approvals_file);
    let r = run(&fx, Arc::new(Rec::default()), |o| o.definition_out = Some(cell.clone()));
    assert_eq!((r.code, cell.lock().unwrap().clone()), (2, Some(pin.clone())), "{}", r.err);
    let pinned: Arc<Mutex<Option<Pin>>> = Arc::default();
    let r = run(&fx, Arc::new(Rec::default()), |o| {
        o.definition = Some(pin.clone());
        o.definition_out = Some(pinned.clone());
    });
    assert_eq!((r.code, pinned.lock().unwrap().clone()), (2, None), "a pinned launch reports no pin: {}", r.err);
    std::fs::write(&fx.paths.config_file, "[profile.rust]\ndisabled = [\"review\"]\n").unwrap();
    let user: Arc<Mutex<Option<Pin>>> = Arc::default();
    let r = run(&fx, Arc::new(Rec::default()), |o| o.definition_out = Some(user.clone()));
    assert_eq!((r.code, user.lock().unwrap().clone()), (2, None), "a user profile reads nothing: {}", r.err);
}

#[test]
fn l11_a_pin_outside_a_checkout_stops() { // R§7.4 bullet 4 (rewritten)
    let fx = fx();
    let pin = approve(&fx.app, &fx.paths.approvals_file);
    let plain = fx.root.join("plain");
    std::fs::create_dir(&plain).unwrap();
    let r = run(&fx, Arc::new(Rec::default()), |o| {
        o.cwd = plain.clone();
        o.definition = Some(pin.clone());
    });
    assert_eq!(r.code, 1, "{}", r.err);
    assert!(r.seen.is_none() && !r.called);
    assert_eq!(count(&r.err, &pin_line(&pin, NO_LONGER)), 1, "{}", r.err);
}

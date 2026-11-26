//! WD-14: pinned launches (R§7.4, R§18 items 15 and 16). L-tests run the launcher in process with an injected git
//! runner. B-tests run `zygo ws run` and E-tests `zygo up` on tmux; both launch the stub agent and are
//! `#[ignore = "timing: …"]` as the agent-launch tests of `workspace_run.rs` and `workspace_restore.rs`. Workspace
//! state and the test shell need unix, so the file is `cfg(unix)` (OP5). No test reaches the network: the L-tests
//! remove the built-in marketplace from the built-in layer, the B- and E-tests turn it off in the user config
//! ([`NO_MARKETPLACE`]).
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

/// The user config of the B- and E-tests turns the built-in marketplace off as the L-tests' [`builtin`] does: a user
/// `act101` entry with an empty `ref` (the built-in source stays, §15.1), and sync skips a built-in entry without a
/// `ref` (`zygo-plugins` `sync.rs` `engine`), so no run reaches the network.
const NO_MARKETPLACE: &str = "[[marketplace]]\nname = \"act101\"\nref = \"\"\n";

/// Writes the user config: [`NO_MARKETPLACE`], then `body`.
fn user_config(h: &Harness, body: &str) {
    config(h, &format!("{NO_MARKETPLACE}{body}"));
}

/// No line of `err` names a marketplace: the run did no marketplace sync (no network).
fn no_marketplace_line(err: &str) {
    assert!(!err.lines().any(|l| l.contains("marketplace")), "{err}");
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

#[test]
fn l13_a_huge_non_id_pin_gives_a_short_stop_line() { // R§7.4 bullet 4 (rewritten); review F5
    let fx = fx();
    let pin = approve(&fx.app, &fx.paths.approvals_file);
    let bad = Pin::new("z".repeat(1 << 20), pin.digest.clone());
    let r = run(&fx, Arc::new(Rec::default()), |o| o.definition = Some(bad.clone()));
    assert_eq!(r.code, 1, "{}", r.err.len());
    let line = r.err.lines().find(|l| l.starts_with("zygo: profile rust of this member")).expect("the stop line");
    assert!(line.len() < 1024, "the stop line has {} bytes", line.len());
    assert!(r.err.len() < 1024, "stderr has {} bytes", r.err.len());
}

#[test]
fn l12_a_read_profile_that_validate_refuses_gives_no_pin() { // R§7.4 bullet 2 (a first start that applied the profile), R§12 row; review F2
    let fx = fx();
    commit_default(&fx.app, "overrides = { code = \"no-such-model\" }\n");
    approve(&fx.app, &fx.paths.approvals_file);
    let cell: Arc<Mutex<Option<Pin>>> = Arc::default();
    let r = run(&fx, Arc::new(Rec::default()), |o| o.definition_out = Some(cell.clone()));
    assert_eq!(r.code, 2, "{}", r.err);
    assert!(r.err.lines().any(|l| l.starts_with("zygo: ") && l.ends_with("; launching without the profile")), "{}", r.err);
    assert!(r.seen.expect("build ran").0.is_none(), "no profile was sent");
    assert_eq!(cell.lock().unwrap().clone(), None, "a profile that was not applied gives no pin: {}", r.err);
}

/// `bin/git`: appends each call (`$*`) to `<root>/git.log`; with `fail_ls_tree`, an `ls-tree` call exits 1 with no
/// output; else it runs the git of the test's `PATH` (the harness `PATH` is `bin` only). The log starts empty.
fn fake_git(h: &Harness, fail_ls_tree: bool) {
    use std::os::unix::fs::PermissionsExt;
    let real = which::which("git").unwrap();
    let fail = if fail_ls_tree { "for a in \"$@\"; do [ \"$a\" = ls-tree ] && exit 1; done\n" } else { "" };
    let body = format!("#!/bin/sh\necho \"$*\" >> '{}'\n{fail}exec '{}' \"$@\"\n", h.root().join("git.log").display(), real.display());
    let p = h.bin.join("git");
    let _ = std::fs::remove_file(&p);
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    ready_exe(&p);
    let _ = std::fs::remove_file(h.root().join("git.log"));
}

fn git_log(h: &Harness) -> String {
    std::fs::read_to_string(h.root().join("git.log")).unwrap_or_default()
}

/// panes[] = KEY (Claude Code) in `app`; members[KEY] = `m`; the workspace's checkout is `app`.
fn seed_member(h: &Harness, app: &Path, m: &MemberRecord) {
    h.seed(WS_ID, &mut |w| {
        w.common_dir = app.join(".git");
        w.main_root = app.to_path_buf();
        w.panes = vec![Pane { key: KEY.into(), cwd: app.to_path_buf(), kind: PaneKind::ClaudeCode, ..Default::default() }];
        w.members.clear();
        w.members.insert(KEY.into(), m.clone());
    });
}

/// The record `up` writes for a repository member with profile `rust` and job `worker` (`up.rs:681-701`), after a
/// stop: resumable.
fn pinned(pin: Option<&Pin>) -> MemberRecord {
    let mut m = MemberRecord::new(Agent::ClaudeCode);
    m.state = MemberState::Resumable;
    m.agent_session = Some("abc".into());
    m.zygo_session = Some(OLD_ZS.into());
    m.launch.profile = Some("rust".into());
    m.launch.job = Some("worker".into());
    m.launch.definition = pin.cloned();
    m
}

/// `zygo ws run --ws-id WS_ID --pane KEY` with the stub environment `env`, through `Guarded`.
fn ws_run(h: &Harness, env: &[(&'static str, OsString)]) -> Output {
    let extra: Vec<(&str, &OsStr)> = env.iter().map(|(k, v)| (*k, v.as_os_str())).collect();
    Guarded::spawn(h.zygo(&["ws", "run", "--ws-id", WS_ID, "--pane", KEY], &extra)).output("ws run")
}

/// The session view that the stub with output folder `out` received.
fn view(out: &Path) -> Value {
    serde_json::from_str(json_file(&out.join("session.json"))["view.json"].as_str().unwrap()).unwrap()
}

/// The session view of the last stub start of the pane `key` (E-tests).
fn view_of(h: &Harness, key: &str) -> Value {
    view(&out(h, key))
}

fn member_of(h: &Harness) -> MemberRecord {
    h.store(WS_ID).read().unwrap().unwrap().members[KEY].clone()
}

fn state_bytes(h: &Harness) -> Vec<u8> {
    std::fs::read(h.store(WS_ID).dir().join("workspace.json")).unwrap()
}

/// The R§6.1 step 2 default-branch lookup of the definition read (`read.rs:80`), as `bin/git` logs it.
const DEFAULT_LOOKUP: &str = "symbolic-ref --quiet refs/remotes/origin/HEAD";

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn b1_ws_run_resumes_with_the_pinned_profile_and_keeps_job_and_pin() { // R§7.4 bullets 3, 7; R§9.3 effect 2; R§10 `ws run`; review focus 33
    for approve_new in [true, false] {
        let h = Harness::no_tmux();
        user_config(&h, "");
        let app = repo_at(h.root(), &[(".zygo/profiles/rust.toml", RUST)]);
        let pin0 = approve(&app, &h.paths.approvals_file);
        commit_default(&app, NEWER);
        if approve_new {
            approve(&app, &h.paths.approvals_file);
        }
        seed_member(&h, &app, &pinned(Some(&pin0)));
        fake_git(&h, false);
        let (out, env) = h.stub(Agent::ClaudeCode, "a");
        let o = ws_run(&h, &env);
        no_marketplace_line(&text(&o.stderr));
        let v = view(&out);
        assert_eq!((v["profile"].clone(), v["disabled_roles"].clone()), (json!("rust"), json!(["design"])), "{}", text(&o.stderr));
        let log = git_log(&h);
        assert!(!log.contains(DEFAULT_LOOKUP), "{log}");
        assert!(log.lines().any(|l| l.contains("ls-tree") && l.contains(&pin0.commit)), "{log}");
        let m = member_of(&h);
        assert_eq!((m.launch.job.as_deref(), m.launch.definition.as_ref()), (Some("worker"), Some(&pin0)));
    }
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn b2_a_failing_pinned_listing_stops_ws_run_and_keeps_the_record() { // R§7.4 bullet 5 (rewritten); R§18 item 16
    let h = Harness::no_tmux();
    user_config(&h, "");
    let app = repo_at(h.root(), &[(".zygo/profiles/rust.toml", RUST)]);
    let pin = approve(&app, &h.paths.approvals_file);
    seed_member(&h, &app, &pinned(Some(&pin)));
    fake_git(&h, true);
    let before = state_bytes(&h);
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let err = text(&ws_run(&h, &env).stderr);
    assert_eq!(count(&err, &pin_line(&pin, "were not read (git ls-tree: failed)")), 1, "{err}");
    assert_eq!(count(&err, "zygo: the claude launch failed before the agent started (exit 1)"), 1, "{err}");
    assert!(!out.join("argv.json").exists(), "the agent started");
    assert_eq!(state_bytes(&h), before);
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn b3_a_pruned_pin_stops_ws_run_and_keeps_the_record() { // R§7.4 bullet 4; R§11 row; R§18 item 16
    let h = Harness::no_tmux();
    user_config(&h, "");
    let app = repo_at(h.root(), &[(".zygo/profiles/rust.toml", RUST)]);
    let pin = approve(&app, &h.paths.approvals_file);
    prune(&app, &pin.commit);
    seed_member(&h, &app, &pinned(Some(&pin)));
    fake_git(&h, false);
    let before = state_bytes(&h);
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let err = text(&ws_run(&h, &env).stderr);
    assert_eq!(count(&err, &pin_line(&pin, NO_LONGER)), 1, "{err}");
    assert!(!out.join("argv.json").exists(), "the agent started");
    assert_eq!(state_bytes(&h), before);
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn b4_a_pool_member_with_a_gone_pin_starts_with_no_pinned_read() { // R§7.4 bullet 6; R§9.5; R§18 item 16
    let h = Harness::no_tmux();
    user_config(&h, "");
    let app = repo_at(h.root(), &[(".zygo/profiles/rust.toml", RUST)]);
    let pin = approve(&app, &h.paths.approvals_file);
    prune(&app, &pin.commit);
    let pool = h.root().join("pool/a");
    std::fs::create_dir_all(&pool).unwrap();
    let mut m = pinned(Some(&pin));
    m.launch.passthrough = true;
    m.launch.config_dir = Some(pool.display().to_string());
    m.config_dir = Some(pool.clone());
    seed_member(&h, &app, &m);
    fake_git(&h, false);
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let err = text(&ws_run(&h, &env).stderr);
    assert!(out.join("argv.json").exists(), "the agent did not start: {err}");
    assert_eq!(count(&err, "zygo: profile rust does not apply in passthrough"), 1, "{err}");
    assert!(view(&out)["profile"].is_null());
    let log = git_log(&h);
    assert!(!log.lines().any(|l| l.contains("ls-tree") && l.contains(&pin.commit)), "{log}");
}

#[test]
#[ignore = "timing: the agent launch has 2 s launcher probes"]
fn b5_a_first_start_saves_the_pin_of_its_read_and_keeps_it() { // R§7.4 bullet 2 (rewritten); R§12 row (rewritten)
    let h = Harness::no_tmux();
    user_config(&h, "");
    let app = repo_at(h.root(), &[(".zygo/profiles/rust.toml", RUST)]);
    let pin0 = approve(&app, &h.paths.approvals_file);
    let mut m = MemberRecord::new(Agent::ClaudeCode);
    m.launch.profile = Some("rust".into()); // a user-loadout member: no pin
    seed_member(&h, &app, &m);
    fake_git(&h, false);
    let (out, env) = h.stub(Agent::ClaudeCode, "a");
    let o = ws_run(&h, &env);
    assert_eq!(view(&out)["disabled_roles"], json!(["design"]), "{}", text(&o.stderr));
    assert_eq!(member_of(&h).launch.definition.as_ref(), Some(&pin0));
    commit_default(&app, NEWER);
    approve(&app, &h.paths.approvals_file);
    let o = ws_run(&h, &env);
    assert_eq!(view(&out)["disabled_roles"], json!(["design"]), "{}", text(&o.stderr));
    assert_eq!(member_of(&h).launch.definition.as_ref(), Some(&pin0));
}

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn e1_a_fresh_up_pins_each_member_and_a_restore_sends_the_pinned_profile() { // R§7.4 bullets 1-3, 7; R§10 `up` (a restore); R§18 item 16
    for approve_new in [true, false] {
        let Some(h) = Harness::new() else { return };
        user_config(&h, "");
        git_repo_at(&h, &h.repo());
        per_pane_claude(&h, "");
        let app = repo_at(
            h.root(),
            &[(".zygo/workspace.toml", "version = 1\n[members.worker-1]\nprofile = \"rust\"\n"), (".zygo/profiles/rust.toml", RUST)],
        );
        let pin0 = approve(&app, &h.paths.approvals_file);
        let o = run_in(&h, &app, &["up", "--no-attach"], &[], true);
        assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
        let (id, ws) = only_state(&h);
        let k = agent_key(&ws, |m| m.name.as_deref() == Some("worker-1"));
        assert_eq!(state(&h, &id).members[&k].launch.definition.as_ref(), Some(&pin0));
        assert!(wait_starts(&h, &k, 1));
        assert_eq!((view_of(&h, &k)["profile"].clone(), view_of(&h, &k)["disabled_roles"].clone()), (json!("rust"), json!(["design"])));
        commit_default(&app, NEWER);
        if approve_new {
            approve(&app, &h.paths.approvals_file);
        }
        down(&h, &ws.name);
        let o = up(&h, &ws.name, &[]);
        assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
        assert!(wait_starts(&h, &k, 2));
        assert_eq!((view_of(&h, &k)["profile"].clone(), view_of(&h, &k)["disabled_roles"].clone()), (json!("rust"), json!(["design"])));
        assert_eq!(state(&h, &id).members[&k].launch.definition.as_ref(), Some(&pin0));
        down(&h, &ws.name);
    }
}

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn e2_a_hand_start_with_a_repository_profile_gets_the_pin_and_keeps_it() { // R§7.4 bullet 2 (rewritten); R§18 item 16
    let Some(h) = Harness::new() else { return };
    user_config(&h, "");
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    let app = repo_at(h.root(), &[(".zygo/profiles/rust.toml", RUST)]);
    let pin0 = approve(&app, &h.paths.approvals_file);
    user_config(&h, "[loadout.l14]\n[[loadout.l14.window]]\n[[loadout.l14.window.pane]]\nkind = \"shell\"\n");
    let o = run_in(&h, &app, &["up", "--loadout", "l14", "--no-attach"], &[], true);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let (id, ws) = only_state(&h);
    let script = hand_script(&h, "hand", &["claude", "--profile", "rust"], &[("ZYGO_WORKSPACE", OsStr::new(&id))]);
    let at = app.display().to_string();
    let pane = h.tmux(&["split-window", "-P", "-F", "#{pane_id}", "-c", &at, "-t", &format!("={}:", ws.tmux_session), &script]);
    std::fs::write(h.root().join("hand.go"), "").unwrap();
    let hand_err = || std::fs::read_to_string(h.root().join("hand.err")).unwrap_or_default();
    let mut k = String::new();
    assert!(
        until(BOUND, || {
            k = h.tmux(&["show-options", "-p", "-qv", "-t", &pane, "@zygo-pane"]).trim().to_string();
            !k.is_empty()
        }),
        "{}",
        hand_err()
    );
    assert!(wait_starts(&h, &k, 1), "{}", hand_err());
    assert_eq!(view_of(&h, &k)["disabled_roles"], json!(["design"]));
    assert!(
        until(BOUND, || state(&h, &id).members.get(&k).is_some_and(|m| m.launch.definition.as_ref() == Some(&pin0))),
        "{}",
        hand_err()
    );
    commit_default(&app, NEWER);
    approve(&app, &h.paths.approvals_file);
    down(&h, &ws.name);
    let o = up(&h, &ws.name, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    assert!(wait_starts(&h, &k, 2));
    assert_eq!(view_of(&h, &k)["disabled_roles"], json!(["design"]));
    assert_eq!(state(&h, &id).members[&k].launch.definition.as_ref(), Some(&pin0));
    down(&h, &ws.name);
}

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn e4_a_hand_resume_of_a_kept_record_launches_from_its_pin() { // R§7.4 bullet 2 (a hand resume), bullet 3; review F3
    let Some(h) = Harness::new() else { return };
    user_config(&h, "[loadout.l14]\n[[loadout.l14.window]]\n[[loadout.l14.window.pane]]\nkind = \"shell\"\n");
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    let app = repo_at(h.root(), &[(".zygo/profiles/rust.toml", RUST)]);
    let pin0 = approve(&app, &h.paths.approvals_file);
    let o = run_in(&h, &app, &["up", "--loadout", "l14", "--no-attach"], &[], true);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let (id, ws) = only_state(&h);
    commit_default(&app, NEWER);
    approve(&app, &h.paths.approvals_file);
    // The kept record: a stopped member of key KEY with agent session `abc` and the pin of the `RUST` read.
    h.store(&id)
        .update(Duration::from_secs(5), &mut |w| {
            w.members.insert(KEY.into(), pinned(Some(&pin0)));
            Ok(())
        })
        .unwrap();
    let script =
        hand_script(&h, "hand", &["claude", "--profile", "rust", "--", "--resume", "abc"], &[("ZYGO_WORKSPACE", OsStr::new(&id))]);
    let at = app.display().to_string();
    let pane = h.tmux(&["split-window", "-P", "-F", "#{pane_id}", "-c", &at, "-t", &format!("={}:", ws.tmux_session), &script]);
    h.tmux(&["set-option", "-p", "-t", pane.trim(), "@zygo-pane", KEY]);
    fake_git(&h, false);
    std::fs::write(h.root().join("hand.go"), "").unwrap();
    let hand_err = || std::fs::read_to_string(h.root().join("hand.err")).unwrap_or_default();
    assert!(wait_starts(&h, KEY, 1), "{}", hand_err());
    let v = view_of(&h, KEY);
    assert_eq!((v["profile"].clone(), v["disabled_roles"].clone()), (json!("rust"), json!(["design"])), "{}", hand_err());
    let log = git_log(&h);
    assert!(!log.contains(DEFAULT_LOOKUP), "{log}");
    assert!(log.lines().any(|l| l.contains("ls-tree") && l.contains(&pin0.commit)), "{log}");
    assert_eq!(state(&h, &id).members[KEY].launch.definition.as_ref(), Some(&pin0));
    down(&h, &ws.name);
}

#[test]
#[ignore = "timing: tmux e2e: 2 s tmux calls, 2 s launcher probes and lock waits"]
fn e3_item_15_a_pool_member_with_a_profile() { // R§18 item 15 (stub launch part); R§9.5 "Launch"; R§14 row §12
    let Some(h) = Harness::new() else { return };
    user_config(&h, "");
    git_repo_at(&h, &h.repo());
    per_pane_claude(&h, "");
    let app = repo_at(
        h.root(),
        &[(".zygo/workspace.toml", "version = 1\n[members.worker-1]\nprofile = \"rust\"\npool = true\n"), (".zygo/profiles/rust.toml", RUST)],
    );
    approve(&app, &h.paths.approvals_file);
    let headers = |k: &str| stub_env(&h, k)["ANTHROPIC_CUSTOM_HEADERS"].as_str().unwrap_or_default().to_string();
    // No pool: no passthrough; the `rust` profile is sent.
    let o = run_in(&h, &app, &["up", "--no-attach"], &[], true);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o.stderr));
    let (id, ws) = only_state(&h);
    let k = agent_key(&ws, |m| m.name.as_deref() == Some("worker-1"));
    assert!(wait_starts(&h, &k, 1));
    assert_eq!((view_of(&h, &k)["profile"].clone(), view_of(&h, &k)["disabled_roles"].clone()), (json!("rust"), json!(["design"])));
    let m = state(&h, &id).members[&k].clone();
    assert!(!m.launch.passthrough && m.launch.config_dir.is_none());
    assert!(!headers(&k).contains("X-Zygo-Token"), "{}", headers(&k));
    down(&h, &ws.name);
    // A pool: passthrough on the pool directory; no profile.
    let pool = h.root().join("pool");
    std::fs::create_dir_all(pool.join("a")).unwrap();
    user_config(&h, &format!("[workspace]\npool = \"{}\"\n", pool.display()));
    let o = run_in(&h, &app, &["up", "--reset", "--no-attach"], &[], true);
    let err = text(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    assert!(
        err.lines().any(|l| l.starts_with("zygo: worker-1 run in passthrough on pool ")
            && l.ends_with("; profile rust of worker-1 does not apply in passthrough")),
        "{err}"
    );
    let (id, ws) = only_state(&h);
    let k2 = agent_key(&ws, |m| m.name.as_deref() == Some("worker-1"));
    assert!(wait_starts(&h, &k2, if k2 == k { 2 } else { 1 }));
    let m = state(&h, &id).members[&k2].clone();
    assert!(m.launch.passthrough);
    assert_eq!(m.config_dir.as_deref(), Some(pool.join("a").as_path()));
    assert!(view_of(&h, &k2)["profile"].is_null());
    assert!(headers(&k2).contains("X-Zygo-Token"), "{}", headers(&k2));
    down(&h, &ws.name);
}

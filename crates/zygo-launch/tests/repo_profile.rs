//! WD-13: the launcher's repository profile by a default-branch read (R§8, R§7.3, R§18 items 9 and 24).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zygo_config::profile::Profile;
use zygo_config::repo_def::{approve_workspace_in, check_default, short, GitRunner, SystemGit, Use};
use zygo_core::paths::Paths;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;
use zygo_launch::env::Env;
use zygo_launch::{AgentAdapter, ChildSpec, LaunchCtx, LaunchOpts, Launcher, Preflight};
use zygo_proxy::session::SessionConfig;
use zygo_work::repo::checkout;
use zygo_workspace::identity::workspace_id;

const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
const RUST: &str = "overrides = { code = \"claude-haiku-5-5\" }\ndisabled = [\"design\"]\n";

/// A test-side git call: no system or global config (as tests/launcher.rs:312).
fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .env("GIT_CONFIG_GLOBAL", NULL)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git on PATH");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// The built-in layer without its marketplace (no network).
fn builtin() -> toml::Table {
    let mut t = toml::Table::try_from(zygo_config::builtin::defaults()).unwrap();
    t.remove("marketplace");
    t
}

struct Fx {
    _t: tempfile::TempDir,
    root: PathBuf,
    paths: Paths,
    cwd: PathBuf,
}

/// `src` with `.zygo/profiles/<n>.toml` committed on `main`, a bare `origin.git` cloned from it, and the
/// checkout `work` cloned from that, so `refs/remotes/origin/HEAD` is `refs/remotes/origin/main` (R§18 preamble).
fn fx(profiles: &[(&str, &str)]) -> Fx {
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
    let src = root.join("src");
    for d in [&h, &src, &root.join("bin")] {
        std::fs::create_dir_all(d).unwrap();
    }
    git(&src, &["init", "-q", "-b", "main"]);
    let dir = src.join(".zygo").join("profiles");
    std::fs::create_dir_all(&dir).unwrap();
    for (n, text) in profiles {
        std::fs::write(dir.join(format!("{n}.toml")), text).unwrap();
    }
    git(&src, &["add", "-A"]);
    git(&src, &["commit", "-q", "-m", "profiles"]);
    git(&root, &["clone", "-q", "--bare", "src", "origin.git"]);
    git(&root, &["clone", "-q", "origin.git", "work"]);
    let cwd = root.join("work");
    Fx { _t: t, root, paths, cwd }
}

fn ws_id(fx: &Fx) -> String {
    workspace_id(&checkout(&fx.cwd, Instant::now() + Duration::from_secs(3600)).unwrap().common_dir)
}

/// The digest of the default branch's files (not approved yet); `approve` also stores it.
fn digest(fx: &Fx) -> String {
    let Use::NotApproved { digest, .. } = check_default(&fx.cwd, &ws_id(fx), &fx.paths.approvals_file, &SystemGit) else {
        panic!("the files are not in the not-approved state");
    };
    digest
}

fn approve(fx: &Fx) -> String {
    let d = digest(fx);
    approve_workspace_in(&fx.paths.approvals_file, &ws_id(fx), &d).unwrap();
    d
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

/// Fails every call with `tool not found`.
struct Fail;

impl GitRunner for Fail {
    fn git(&self, _top: &Path, _args: &[&str], _deadline: Instant) -> Result<Output, String> {
        Err("tool not found".into())
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

struct Seen {
    profile: Option<Profile>,
    config: SessionConfig,
    view: SessionView,
    err: String,
}

/// LaunchOpts::new with PATH = an empty `bin` dir (plus `SystemRoot` on Windows, as tests/launcher.rs:180-185),
/// cwd = fx.cwd, builtin() and a captured stderr; `top` is not set, so the launcher's own `checkout` runs.
fn launch(fx: &Fx, git: Arc<dyn GitRunner + Send + Sync>, f: impl FnOnce(&mut LaunchOpts)) -> Seen {
    let mut env = vec![
        (OsString::from("PATH"), OsString::from(fx.root.join("bin"))),
        (OsString::from("ACT_CONFIG_DIR"), OsString::from(fx.root.join("act"))),
    ];
    if cfg!(windows)
        && let Some(v) = std::env::var_os("SystemRoot")
    {
        env.push(("SystemRoot".into(), v));
    }
    let mut o = LaunchOpts::new(Env(env), fx.cwd.clone(), fx.paths.clone());
    o.builtin = Some(builtin());
    let buf: Arc<Mutex<Vec<u8>>> = Arc::default();
    o.stderr = Some(buf.clone());
    o.definition_git = Some(git);
    f(&mut o);
    let stub = Stub {
        program: std::env::current_exe().unwrap().display().to_string(),
        refuse: fx.root.join("settings.json"),
        seen: Mutex::new(None),
    };
    let code = Launcher::run(&stub, o).code;
    let err = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert_eq!(code, 2, "{err}");
    let (profile, config, view) = stub.seen.lock().unwrap().take().expect("build ran");
    Seen { profile, config, view, err }
}

#[test]
fn approved_repository_profile_sends_its_overrides() {
    let fx = fx(&[("rust", RUST)]);
    approve(&fx);
    let rec = Arc::new(Rec::default());
    let s = launch(&fx, rec.clone(), |o| o.profile = Some("rust".into()));
    let p = s.profile.expect("the profile was sent");
    assert_eq!(p.name, "rust");
    assert_eq!(p.overrides, BTreeMap::from([("code".to_string(), "claude-haiku-5-5".to_string())]));
    assert_eq!(p.disabled, ["design"]);
    assert_eq!(s.config.roles["code"].model, "claude-haiku-5-5");
    assert!(!s.config.roles.contains_key("design"));
    assert_eq!(s.view.profile.as_deref(), Some("rust"));
    assert!(!s.err.contains("profile"), "{}", s.err);
    assert!(rec.0.lock().unwrap().iter().any(|c| c[0] == "ls-tree"), "the read went through the hook");
}

#[test]
fn user_profile_of_the_same_name_is_used_and_nothing_is_read() {
    let fx = fx(&[("rust", RUST)]);
    approve(&fx);
    std::fs::write(&fx.paths.config_file, "[profile.rust]\ndisabled = [\"review\"]\n").unwrap();
    let rec = Arc::new(Rec::default());
    let s = launch(&fx, rec.clone(), |o| o.profile = Some("rust".into()));
    let p = s.profile.unwrap();
    assert_eq!(p.disabled, ["review"]);
    assert!(p.overrides.is_empty());
    assert_ne!(s.config.roles["code"].model, "claude-haiku-5-5");
    assert!(rec.0.lock().unwrap().is_empty());
}

#[test]
fn not_approved_prints_the_line_with_the_digest() {
    let fx = fx(&[("rust", RUST)]);
    let d = digest(&fx);
    let s = launch(&fx, Arc::new(Rec::default()), |o| o.profile = Some("rust".into()));
    let line = format!(
        "zygo: profile rust is in this repository's workspace files, which are not approved on this machine (digest {}); launching without it; see zygo workspace approve",
        short(&d)
    );
    assert_eq!(short(&d).len(), 12);
    assert_eq!(s.err.lines().filter(|l| *l == line).count(), 1, "{}", s.err);
    assert!(!s.err.contains("not found"), "{}", s.err);
    assert_eq!(s.profile, None);
    assert_eq!(s.view.profile, None);
}

#[test]
fn invalid_files_print_the_invalid_line() {
    let fx = fx(&[("rust", "prompt = \"x\"\n")]);
    let s = launch(&fx, Arc::new(Rec::default()), |o| o.profile = Some("rust".into()));
    let line = "zygo: profile rust: this repository's workspace files are invalid (.zygo/profiles/rust.toml: prompt: not allowed in a repository file); launching without it";
    assert_eq!(s.err.lines().filter(|l| *l == line).count(), 1, "{}", s.err);
    assert!(!s.err.contains("not found"), "{}", s.err);
    assert_eq!(s.profile, None);
}

#[test]
fn model_not_in_the_catalog_prints_the_11_3_line() {
    let fx = fx(&[("rust", "overrides = { code = \"no-such-model\" }\n")]);
    approve(&fx);
    let s = launch(&fx, Arc::new(Rec::default()), |o| o.profile = Some("rust".into()));
    let line = "zygo: profile.rust.overrides.code: model is not in the catalog; launching without the profile";
    assert_eq!(s.err.lines().filter(|l| *l == line).count(), 1, "{}", s.err);
    assert_eq!(s.profile, None);
}

#[test]
fn no_profile_flag_makes_no_ls_tree_call() {
    let fx = fx(&[("rust", RUST)]);
    approve(&fx);
    // nothing requested
    let rec = Arc::new(Rec::default());
    let s = launch(&fx, rec.clone(), |_| {});
    assert_eq!(s.profile, None);
    assert!(!rec.0.lock().unwrap().iter().any(|c| c[0] == "ls-tree"));
    assert!(rec.0.lock().unwrap().is_empty());
    // the name from [user] default_profile, defined in user config
    std::fs::write(&fx.paths.config_file, "[user]\ndefault_profile = \"rust\"\n[profile.rust]\ndisabled = [\"review\"]\n").unwrap();
    let rec = Arc::new(Rec::default());
    let s = launch(&fx, rec.clone(), |_| {});
    assert_eq!(s.profile.unwrap().disabled, ["review"]);
    assert!(!rec.0.lock().unwrap().iter().any(|c| c[0] == "ls-tree"));
    assert!(rec.0.lock().unwrap().is_empty());
}

#[test]
fn passthrough_applies_no_profile_from_any_source() {
    let fx = fx(&[("rust", RUST)]);
    approve(&fx);
    // repository source
    let rec = Arc::new(Rec::default());
    let s = launch(&fx, rec.clone(), |o| {
        o.profile = Some("rust".into());
        o.passthrough = true;
    });
    assert_eq!(s.profile, None);
    assert_eq!(s.view.profile, None);
    assert!(s.view.disabled_roles.is_empty());
    assert!(rec.0.lock().unwrap().is_empty());
    assert!(!s.err.contains("profile"), "{}", s.err);
    // user source, by flag and by default_profile
    std::fs::write(&fx.paths.config_file, "[user]\ndefault_profile = \"mine\"\n[profile.mine]\ndisabled = [\"review\"]\n").unwrap();
    for flag in [Some("mine"), None] {
        let s = launch(&fx, Arc::new(Rec::default()), |o| {
            o.profile = flag.map(String::from);
            o.passthrough = true;
        });
        assert_eq!(s.profile, None, "{flag:?}");
        assert!(s.view.disabled_roles.is_empty(), "{flag:?}");
    }
}

#[test]
fn unread_read_prints_one_line() {
    let fx = fx(&[("rust", RUST)]);
    approve(&fx);
    let s = launch(&fx, Arc::new(Fail), |o| o.profile = Some("rust".into()));
    let line = "zygo: profile rust: this repository's workspace files were not read (git symbolic-ref: tool not found); launching without it";
    assert_eq!(s.err.lines().filter(|l| *l == line).count(), 1, "{}", s.err);
    assert!(!s.err.contains("not found;"), "{}", s.err);
    assert_eq!(s.profile, None);
}

#[test]
fn files_without_the_name_print_not_found() {
    let fx = fx(&[("go", RUST)]);
    approve(&fx);
    let s = launch(&fx, Arc::new(Rec::default()), |o| o.profile = Some("rust".into()));
    assert_eq!(
        s.err.lines().filter(|l| *l == "zygo: profile rust not found; launching without it").count(),
        1,
        "{}",
        s.err
    );
    assert_eq!(s.profile, None);
}

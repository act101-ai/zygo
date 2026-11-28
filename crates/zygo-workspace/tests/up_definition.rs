//! WD-11: zygo up chooses the repository loadout from one checked read (R§10, R§7.3, R§7.4, R§9.4).
//!
//! "R§" is `docs/specs/2026-11-24-workspace-definition-design.md`. Every test and helper is `cfg(unix)`: the tmux
//! stand-in is a shell script (workspace verbs are unix-only by OP5). Every test takes `serial()`, so no test writes a
//! script while another test forks (ETXTBSY). The git calls go to a bare repository in a temporary folder; no network.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tempfile::TempDir;
use zygo_config::repo_def::{approve_workspace_in, check_default, GitRunner, Pin, SystemGit, Use};
use zygo_config::{Config, Loadout, LoadoutPane, LoadoutWindow};
use zygo_core::paths::Paths;
use zygo_core::{esc, Agent};
use zygo_workspace::down::{self, DownOpts, DownTarget};
use zygo_workspace::identity::workspace_id;
use zygo_workspace::state::{BuildPlan, MemberRecord, MemberState, Pane, PaneKind, Phase, Store, Window, Workspace};
use zygo_workspace::up::{self, UpOpts};

const S5: Duration = Duration::from_secs(5);
const MAIN: &str = "refs/remotes/origin/main";
const WS: &str = ".zygo/workspace.toml";
const GONE: &str = "zygo: tmux: no server running on /tmp/zygo-none/s\n";

fn not_approved(r: &str, d: &str) -> String {
    format!(
        "zygo: this repository's workspace files ({r}, digest {}) are not approved on this machine; nothing from them was applied; see zygo workspace approve",
        &d[..12]
    )
}

fn invalid(r: &str, why: &str) -> String {
    format!("zygo: this repository's workspace files ({r}) are invalid ({}); nothing from them was applied", esc(why))
}

// ---- helpers (from tests/up.rs) ----

fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}

fn canon(p: &Path) -> PathBuf { p.canonicalize().unwrap() }

/// An executable `#!/bin/sh` program `<dir>/<name>`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let f = dir.join(name);
    fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
    f
}

/// A tmux stand-in that logs each call to `<dir>/tmux.log`, answers `-V` with 3.4 and every other call with "no server".
fn gone_tmux(dir: &Path) -> PathBuf {
    let log = dir.join("tmux.log");
    let body = format!(
        "printf '%s\\036' \"$*\" >> '{}'\nfor a in \"$@\"; do if [ \"$a\" = -V ]; then echo 'tmux 3.4'; exit 0; fi; done\n\
         echo 'no server running on /tmp/zygo-none/s' >&2\nexit 1",
        log.display()
    );
    script(dir, "gtmux", &body)
}

const ANSWER_BODY: &str = r#"d='@D'
printf '%s\036' "$*" >> "$d/tmux.log"
sub=; tgt=
while [ $# -gt 0 ]; do
  case "$1" in
    -V) echo 'tmux 3.4'; exit 0 ;;
    -S|-t|-F|-s|-c|-x|-y|-n) [ "$1" = -t ] && tgt=$2; shift ;;
    -*) ;;
    *) [ -z "$sub" ] && sub=$1 ;;
  esac
  shift
done
s=${tgt#=}; s=${s%%:*}
case "$sub" in
  has-session) [ -f "$d/exists-$s" ] && exit 0; [ -f "$d/error-$s" ] && { echo 'permission denied' >&2; exit 1; } ;;
  show-options) [ -f "$d/ws-$s" ] && { cat "$d/ws-$s"; exit 0; }; [ -f "$d/exists-$s" ] && exit 0 ;;
  display-message) [ -f "$d/clients-$s" ] && { cat "$d/clients-$s"; exit 0; } ;;
esac
echo 'no server running on /tmp/zygo-none/s' >&2
exit 1"#;

/// The stand-in tmux of `tests/up_all.rs`: it logs each call and answers from the files `exists-<s>`, `error-<s>`,
/// `ws-<s>` and `clients-<s>` of `dir`; every other call gets "no server running".
fn answer_tmux(dir: &Path) -> PathBuf { script(dir, "atmux", &ANSWER_BODY.replace("@D", &dir.display().to_string())) }

fn tmux_log(dir: &Path) -> Vec<String> {
    let s = fs::read_to_string(dir.join("tmux.log")).unwrap_or_default();
    s.split('\u{1e}').filter(|l| !l.is_empty()).map(str::to_string).collect()
}

fn text(b: Vec<u8>) -> String { String::from_utf8(b).unwrap() }

static LIVE: Mutex<()> = Mutex::new(());
fn serial() -> std::sync::MutexGuard<'static, ()> { LIVE.lock().unwrap_or_else(|e| e.into_inner()) }

fn member(kind: Agent, state: MemberState, session: Option<&str>) -> MemberRecord {
    let mut m = MemberRecord::new(kind);
    m.state = state;
    m.agent_session = session.map(String::from);
    m
}

fn win(index: u32, name: Option<&str>, layout: &str, width: u32, height: u32) -> Window {
    Window { index, name: name.map(String::from), layout: layout.into(), width, height, ..Window::default() }
}

fn pane(key: &str, window: u32, index: u32, kind: PaneKind, cwd: &Path) -> Pane {
    Pane { key: key.into(), window, index, cwd: cwd.to_path_buf(), kind, ..Pane::default() }
}

/// A saved state of the workspace of `repo`: name `app`, phase `down`, loadout `solo`, and the three lists.
fn saved(store: &Store, repo: &Path, windows: Vec<Window>, panes: Vec<Pane>, members: Vec<(&str, MemberRecord)>) {
    let common = canon(&repo.join(".git"));
    store
        .update(S5, &mut |ws| {
            ws.name = "app".into();
            ws.tmux_session = "zygo-app".into();
            ws.common_dir = common.clone();
            ws.main_root = repo.to_path_buf();
            ws.loadout = "solo".into();
            ws.phase = Phase::Down;
            ws.windows = windows.clone();
            ws.panes = panes.clone();
            ws.members = members.iter().map(|(k, m)| (k.to_string(), m.clone())).collect();
            Ok(())
        })
        .unwrap();
}

fn set(store: &Store, f: impl Fn(&mut Workspace)) {
    store
        .update(S5, &mut |ws| {
            f(ws);
            Ok(())
        })
        .unwrap();
}

// ---- git helpers (from crates/zygo-config/tests/repo_def_check.rs) ----

/// `git -C <dir> <args>` with no user or system config, no signing and a fixed identity.
fn git_cmd(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid",
               "-c", "core.autocrlf=false", "-C"]).arg(dir).args(args);
    c
}

/// Runs it, asserts success, returns stdout trimmed.
fn git(dir: &Path, args: &[&str]) -> String {
    let o = git_cmd(dir, args).stdin(Stdio::null()).output().expect("git on PATH");
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// `hash-object -w --stdin` of `bytes` in `dir`; returns the blob id.
fn blob(dir: &Path, bytes: &[u8]) -> String {
    let mut child = git_cmd(dir, &["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("git on PATH");
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "hash-object: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// Adds each regular file to the index of `dir` (nothing on disk), then `commit -q -m c`.
fn commit_files(dir: &Path, ents: &[(&str, &[u8])]) {
    for (path, bytes) in ents {
        let oid = blob(dir, bytes);
        git(dir, &["update-index", "--add", "--cacheinfo", &format!("100644,{oid},{path}")]);
    }
    git(dir, &["commit", "-q", "-m", "c"]);
}

// ---- the counting runner ----

/// Delegates to `SystemGit`; records each call's args joined by " " and the `phase` of workspace.json at that call
/// ("" when none).
struct Counting {
    state: PathBuf,
    calls: Mutex<Vec<String>>,
    phases: Mutex<Vec<String>>,
}

impl GitRunner for Counting {
    fn git(&self, top: &Path, args: &[&str], deadline: Instant) -> Result<Output, String> {
        self.calls.lock().unwrap().push(args.join(" "));
        let phase = fs::read(&self.state)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get("phase").and_then(|p| p.as_str()).map(String::from))
            .unwrap_or_default();
        self.phases.lock().unwrap().push(phase);
        SystemGit.git(top, args, deadline)
    }
}

impl Counting {
    /// The default-branch reads: each read starts with `symbolic-ref` (R§6.1).
    fn reads(&self) -> usize { self.calls.lock().unwrap().iter().filter(|c| c.starts_with("symbolic-ref ")).count() }
    fn calls(&self) -> Vec<String> { self.calls.lock().unwrap().clone() }
    fn clear(&self) {
        self.calls.lock().unwrap().clear();
        self.phases.lock().unwrap().clear();
    }
}

/// A runner whose every call fails: the read is unread (R§10).
struct Failing;

impl GitRunner for Failing {
    fn git(&self, _top: &Path, _args: &[&str], _deadline: Instant) -> Result<Output, String> { Err("failed".into()) }
}

// ---- the fixture ----

struct Fx {
    _t: TempDir,
    t: PathBuf,
    w: PathBuf,
    c: PathBuf,
    o: UpOpts,
    git: Arc<Counting>,
    store: Store,
}

/// Bare `o.git`; `w` = README seed then `ents` (none: no commit), pushed; `c` (`app`) = a checked-out clone.
/// `o.cwd = c`, `o.tmux = gone_tmux`, `o.git = Some(git)`; a `before_undo` hook copies workspace.json to
/// `t/planned.json`.
fn fx(ents: &[(&str, &[u8])]) -> Fx {
    let td = TempDir::new().unwrap();
    let t = canon(td.path());
    let (o, w) = (t.join("o.git"), t.join("w"));
    git(&t, &["init", "-q", "--bare", "-b", "main", o.to_str().unwrap()]);
    git(&t, &["init", "-q", "-b", "main", w.to_str().unwrap()]);
    commit_files(&w, &[("README", b"seed")]);
    if !ents.is_empty() {
        commit_files(&w, ents);
    }
    git(&w, &["remote", "add", "origin", "../o.git"]);
    git(&w, &["push", "-q", "origin", "main"]);
    git(&t, &["clone", "-q", "o.git", "app"]);
    let c = canon(&t.join("app"));
    let p = paths(&t);
    let id = workspace_id(&canon(&c.join(".git")));
    let store = Store::open(&p, &id);
    let state = store.dir().join("workspace.json");
    let counting = Arc::new(Counting { state: state.clone(), calls: Mutex::new(vec![]), phases: Mutex::new(vec![]) });
    let mut vars = BTreeMap::new();
    vars.insert(OsString::from("PATH"), std::env::var_os("PATH").unwrap_or_default());
    vars.insert(OsString::from("HOME"), t.join("home").into_os_string());
    let runner: Arc<dyn GitRunner + Send + Sync> = counting.clone();
    let opts = UpOpts {
        name: None,
        loadout: None,
        reset: false,
        no_attach: true,
        cwd: c.clone(),
        zygo: t.join("zygo"),
        tmux: gone_tmux(&t),
        tmux_env: None,
        zygo_session: None,
        vars,
        paths: p,
        config: Config::default(),
        default_loadout_set: false,
        git: Some(runner),
    };
    let planned = t.join("planned.json");
    up::testing::hook(
        store.dir(),
        "before_undo",
        Some(Box::new(move || fs::copy(&state, &planned).map(|_| ()).map_err(|e| e.to_string()))),
    );
    Fx { _t: td, t, w, c, o: opts, git: counting, store }
}

impl Fx {
    fn id(&self) -> String { workspace_id(&canon(&self.c.join(".git"))) }

    /// A commit of `ents` on `main` in `w`, pushed, then fetched in `c`.
    fn push(&self, ents: &[(&str, &[u8])]) {
        commit_files(&self.w, ents);
        git(&self.w, &["push", "-q", "origin", "main"]);
        git(&self.c, &["fetch", "-q", "origin"]);
    }

    fn read(&self) -> Use { check_default(&self.c, &self.id(), &self.o.paths.approvals_file, &SystemGit) }

    /// Approves the digest of the current read; returns its pin.
    fn approve(&self) -> Pin {
        let (commit, digest) = match self.read() {
            Use::Approved { pin, .. } => (pin.commit, pin.digest),
            Use::NotApproved { files, digest, .. } => (files.commit, digest),
            other => panic!("not a valid read: {other:?}"),
        };
        approve_workspace_in(&self.o.paths.approvals_file, &self.id(), &digest).unwrap();
        Pin::new(commit, digest)
    }

    fn up(&self) -> (i32, String, String) {
        let _ = fs::remove_file(self.t.join("planned.json"));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = up::testing::run_with(&self.o, &mut out, &mut err);
        (code, text(out), text(err))
    }

    /// The step 8 write, as the `before_undo` hook copied it.
    fn planned(&self) -> Workspace {
        serde_json::from_slice(&fs::read(self.t.join("planned.json")).expect("a step 8 write")).unwrap()
    }

    fn head(&self, r: &str) -> String { git(&self.c, &["rev-parse", r]) }

    fn state_file(&self) -> PathBuf { self.store.dir().join("workspace.json") }

    fn no_new_session(&self) -> bool { tmux_log(&self.t).iter().all(|l| !l.contains("new-session")) }
}

fn one_pane(kind: &str, name: Option<&str>) -> Loadout {
    let pane = LoadoutPane { kind: kind.into(), name: name.map(String::from), ..LoadoutPane::default() };
    Loadout { window: vec![LoadoutWindow { name: None, layout: None, pane: vec![pane] }] }
}

fn gone() -> (i32, String, String) { (1, String::new(), GONE.to_string()) }

// ---- tests ----

#[test] // R§18 item 10, R§9.3 effect 2, R§7.4: the default set, each member with its job and the pin
fn default_set_members_hold_job_and_pin() {
    let _g = serial();
    let f = fx(&[(WS, b"version = 1\n")]);
    let pin = f.approve();
    assert_eq!(f.up(), gone());
    assert_eq!(f.git.reads(), 1);
    let ws = f.planned();
    assert_eq!((ws.loadout.as_str(), ws.windows.len(), ws.windows[0].layout.as_str()), ("repository", 1, "main-vertical"));
    let names: Vec<_> = ws.panes.iter().map(|p| p.name.clone().unwrap()).collect();
    assert_eq!(names, ["orchestrator", "worker-1", "worker-2", "worker-3", "worker-4"]);
    for p in &ws.panes {
        let m = &ws.members[&p.key];
        assert_eq!(m.launch.kind, Agent::ClaudeCode);
        let job = if p.name.as_deref() == Some("orchestrator") { "orchestrator" } else { "worker" };
        assert_eq!(m.launch.job.as_deref(), Some(job));
        assert_eq!(m.launch.definition, Some(Pin::approved(f.head(MAIN), pin.digest.clone(), f.id())));
    }
}

#[test] // R§18 item 10: profile files only, not approved and approved: solo, no line
fn profile_files_only_plan_solo_with_no_line() {
    let _g = serial();
    let f = fx(&[(".zygo/profiles/rust.toml", b"disabled = [\"design\"]\n")]);
    for approve in [false, true] {
        if approve {
            f.approve();
        }
        assert_eq!(f.up(), gone(), "approved {approve}");
        let ws = f.planned();
        assert_eq!(ws.loadout, "solo");
        assert_eq!(ws.members.len(), 1);
        let m = ws.members.values().next().unwrap();
        assert_eq!((m.launch.job.as_ref(), m.launch.definition.as_ref()), (None, None));
    }
}

#[test] // R§10, R§18 item 4: chosen by the default, not approved or invalid: solo and the line
fn default_choice_not_approved_or_invalid_plans_solo_with_the_line() {
    let _g = serial();
    let f = fx(&[(WS, b"version = 1\n")]);
    let Use::NotApproved { digest, .. } = f.read() else { panic!("not approved") };
    assert_eq!(f.up(), (1, String::new(), format!("{}\n{GONE}", not_approved(MAIN, &digest))));
    assert_eq!(f.planned().loadout, "solo");
    for bytes in [&b"version = 2\n"[..], &b"version = 1\n[members.a]\nargs = [\"-x\"]\n"[..]] {
        let f = fx(&[(WS, bytes)]);
        let Use::Invalid { why, .. } = f.read() else { panic!("invalid") };
        assert_eq!(f.up(), (1, String::new(), format!("{}\n{GONE}", invalid(MAIN, &why))));
        let ws = f.planned();
        assert_eq!(ws.loadout, "solo");
        assert!(ws.members.values().all(|m| m.launch.definition.is_none()));
    }
}

#[test] // R§18 item 11, review focus 35: `--loadout repository` refuses each case and writes nothing
fn explicit_repository_refuses_and_writes_nothing() {
    let _g = serial();
    let refused = |f: &Fx, line: String| {
        assert_eq!(f.up(), (1, String::new(), format!("{line}\n")));
        assert!(!f.state_file().exists());
        assert!(f.no_new_session());
    };
    let explicit = |ents: &[(&str, &[u8])]| {
        let mut f = fx(ents);
        f.o.loadout = Some("repository".into());
        f
    };
    let f = explicit(&[(WS, b"version = 1\n")]);
    let Use::NotApproved { digest, .. } = f.read() else { panic!("not approved") };
    refused(&f, not_approved(MAIN, &digest));
    let f = explicit(&[(WS, b"version = 2\n")]);
    let Use::Invalid { why, .. } = f.read() else { panic!("invalid") };
    refused(&f, invalid(MAIN, &why));
    let f = explicit(&[]);
    refused(&f, "zygo: this repository has no workspace files at refs/remotes/origin/main".into());
    let f = explicit(&[(".zygo/profiles/rust.toml", b"disabled = [\"design\"]\n")]);
    f.approve();
    refused(&f, "zygo: this repository's workspace files at refs/remotes/origin/main have no .zygo/workspace.toml; nothing started".into());
    // Decision 2: a user `default_loadout = "repository"` is an explicit choice.
    let mut f = fx(&[(WS, b"version = 1\n")]);
    f.o.config.workspace.default_loadout = "repository".into();
    f.o.default_loadout_set = true;
    let Use::NotApproved { digest, .. } = f.read() else { panic!("not approved") };
    refused(&f, not_approved(MAIN, &digest));
}

#[test] // R§18 item 11: `--loadout solo` and a user default win, with no read
fn flag_and_user_default_win_with_no_read() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    f.approve();
    f.o.loadout = Some("solo".into());
    assert_eq!(f.up(), gone());
    assert_eq!(f.planned().loadout, "solo");
    assert!(f.git.calls().is_empty(), "{:?}", f.git.calls());
    f.o.loadout = None;
    f.o.config.loadout.insert("pair".into(), one_pane("claude-code", None));
    f.o.config.workspace.default_loadout = "pair".into();
    f.o.default_loadout_set = true;
    assert_eq!(f.up(), gone());
    assert_eq!(f.planned().loadout, "pair");
    assert!(f.git.calls().is_empty(), "{:?}", f.git.calls());
    f.o.default_loadout_set = false;
    assert_eq!(f.up(), gone());
    assert_eq!(f.planned().loadout, "repository");
    assert_eq!(f.git.reads(), 1);
}

#[test] // R§10, R§9.1: a user `[loadout.repository]` is the user's loadout
fn user_repository_loadout_is_used() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    f.approve();
    f.o.config.loadout.insert("repository".into(), one_pane("claude-code", Some("elena")));
    let check = |f: &Fx| {
        assert_eq!(f.up(), gone());
        let ws = f.planned();
        assert_eq!(ws.loadout, "repository");
        assert_eq!(ws.panes.len(), 1);
        assert_eq!(ws.panes[0].name.as_deref(), Some("elena"));
        assert_eq!(ws.members[&ws.panes[0].key].launch.definition, None);
    };
    check(&f);
    assert_eq!(f.git.reads(), 1);
    f.git.clear();
    f.o.loadout = Some("repository".into());
    check(&f);
    assert!(f.git.calls().is_empty(), "{:?}", f.git.calls());
}

#[test] // R§18 item 6, review focus 3: a new commit is not approved; one read
fn new_commit_is_not_approved_and_read_once() {
    let _g = serial();
    let f = fx(&[(WS, b"version = 1\n")]);
    f.approve();
    f.push(&[(WS, b"version = 1\nlayout = \"tiled\"\n")]);
    let Use::NotApproved { digest, .. } = f.read() else { panic!("not approved") };
    f.git.clear();
    assert_eq!(f.up(), (1, String::new(), format!("{}\n{GONE}", not_approved(MAIN, &digest))));
    assert_eq!(f.planned().loadout, "solo");
    assert_eq!(f.git.reads(), 1);
}

#[test] // R§18 item 17, R§7.2: content that returns to an approved digest applies with no new approval
fn revert_to_approved_content_applies_it() {
    let _g = serial();
    let d1: &[u8] = b"version = 1\n";
    let f = fx(&[(WS, d1)]);
    let p1 = f.approve();
    f.push(&[(WS, b"version = 1\nlayout = \"tiled\"\n")]);
    f.approve();
    f.push(&[(WS, d1)]);
    let before = fs::read(&f.o.paths.approvals_file).unwrap();
    assert_eq!(f.up(), gone());
    let ws = f.planned();
    assert_eq!(ws.loadout, "repository");
    assert!(!ws.members.is_empty());
    for m in ws.members.values() {
        assert_eq!(m.launch.definition, Some(Pin::approved(f.head(MAIN), p1.digest.clone(), f.id())));
    }
    assert_eq!(fs::read(&f.o.paths.approvals_file).unwrap(), before);
}

#[test] // R§18 item 18: `origin/HEAD` chooses the branch; only an approved one applies
fn origin_head_chooses_among_approved_branches() {
    let _g = serial();
    let f = fx(&[(WS, b"version = 1\n")]);
    git(&f.w, &["checkout", "-q", "-b", "dev", "main"]);
    commit_files(&f.w, &[(WS, b"version = 1\nlayout = \"even-vertical\"\n")]);
    git(&f.w, &["push", "-q", "origin", "dev"]);
    git(&f.w, &["checkout", "-q", "-b", "other", "main"]);
    commit_files(&f.w, &[(WS, b"version = 1\nlayout = \"tiled\"\n")]);
    git(&f.w, &["push", "-q", "origin", "other"]);
    git(&f.c, &["fetch", "-q", "origin"]);
    git(&f.c, &["remote", "set-head", "origin", "dev"]);
    let pin = f.approve();
    assert_eq!(f.up(), gone());
    let ws = f.planned();
    assert_eq!(ws.windows[0].layout, "even-vertical");
    for m in ws.members.values() {
        assert_eq!(m.launch.definition, Some(Pin::approved(f.head("refs/remotes/origin/dev"), pin.digest.clone(), f.id())));
    }
    git(&f.c, &["remote", "set-head", "origin", "other"]);
    let Use::NotApproved { digest, .. } = f.read() else { panic!("not approved") };
    assert_eq!(f.up(), (1, String::new(), format!("{}\n{GONE}", not_approved("refs/remotes/origin/other", &digest))));
    assert_eq!(f.planned().loadout, "solo");
}

#[test] // R§18 item 12, R§9.4, review focus 35: the member maximum refuses before any write
fn member_maximum_refuses_before_any_write() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    f.approve();
    f.o.config.limits.max_members = 4;
    assert_eq!(
        f.up(),
        (1, String::new(), "zygo: this repository's workspace files start 5 agents; [limits] max_members is 4; nothing started\n".into())
    );
    assert!(!f.state_file().exists());
    assert!(f.no_new_session());
    let mut f = fx(&[(WS, b"version = 1\n")]);
    f.approve();
    f.o.config.limits.max_members = 5;
    assert_eq!(f.up(), gone());
    assert_eq!(f.planned().members.len(), 5);
    // User loadouts are not counted.
    let mut f = fx(&[(WS, b"version = 1\n")]);
    f.approve();
    f.o.config.limits.max_members = 1;
    f.o.loadout = Some("solo".into());
    assert_eq!(f.up(), gone());
}

/// The saved state of C12: one claude-code pane `kaaaaaaaa` whose member holds `pin`.
fn saved_member(f: &Fx, pin: Pin) {
    let mut m = member(Agent::ClaudeCode, MemberState::Resumable, Some("s1"));
    m.launch.definition = Some(pin);
    saved(&f.store, &f.c, vec![win(0, None, "tiled", 200, 50)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, &f.c)], vec![("kaaaaaaaa", m)]);
}

#[test] // R§10 restore row, AU10: a restore and `up --all` make no default-branch read
fn restore_and_up_all_make_no_read() {
    let _g = serial();
    let f = fx(&[(WS, b"version = 1\n")]);
    let pin = f.approve();
    saved_member(&f, pin);
    let _ = f.up();
    assert!(f.git.calls().is_empty(), "{:?}", f.git.calls());
    set(&f.store, |w| w.phase = Phase::Up);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let _ = up::testing::run_all_with(&f.o, &mut out, &mut err);
    assert!(f.git.calls().is_empty(), "{:?}", f.git.calls());
    // R§10 `zygo up --all` row: a candidate with no pane plans the user default or `solo`, with no read.
    set(&f.store, |w| {
        w.phase = Phase::Up;
        w.windows.clear();
        w.panes.clear();
        w.members.clear();
    });
    let _ = fs::remove_file(f.t.join("planned.json"));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let _ = up::testing::run_all_with(&f.o, &mut out, &mut err);
    assert!(f.git.calls().is_empty(), "{:?} {}", f.git.calls(), text(err));
    assert_eq!(f.planned().loadout, "solo");
}

#[test] // R§10 AU67 row, R§11 "`up` crashes during a repository build": the repair reads nothing
fn au67_repair_reads_nothing_and_the_taken_back_plan_reads_once() {
    let _g = serial();
    let f = fx(&[(WS, b"version = 1\n")]);
    let pin = f.approve();
    saved_member(&f, pin);
    set(&f.store, |w| {
        w.phase = Phase::Building;
        w.build_plan = Some(BuildPlan::Loadout);
        w.fresh_members = vec!["kaaaaaaaa".into()];
        w.loadout = "repository".into();
    });
    let _ = f.up();
    let p = f.git.phases.lock().unwrap().clone();
    // The repair finished (phase `down`) before the first read; the read is the fresh plan that follows.
    assert!(!p.is_empty() && p.iter().all(|s| s == "down"), "{p:?}");
    assert_eq!(f.git.reads(), 1);
}

#[test] // R§10 AU67 row: with the session present, the repair of a repository build stops at its first tmux write (the
// stand-in answers "no server running" to `set-environment -r`, `mux/tmux.rs:376`); it reads nothing before it stops
// and keeps the panes. A repair that completes is not driven: no stand-in answers the session setup.
fn au67_repair_with_the_session_present_reads_nothing_before_it_stops() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    let pin = f.approve();
    saved_member(&f, pin);
    set(&f.store, |w| {
        w.phase = Phase::Building;
        w.build_plan = Some(BuildPlan::Loadout);
        w.fresh_members = vec!["kaaaaaaaa".into()];
        w.loadout = "repository".into();
    });
    f.o.tmux = answer_tmux(&f.t);
    fs::write(f.t.join("exists-zygo-app"), "").unwrap();
    fs::write(f.t.join("ws-zygo-app"), f.id()).unwrap();
    assert_eq!(f.up(), gone());
    let log = tmux_log(&f.t);
    assert!(log.last().is_some_and(|l| l.contains("set-environment -r")), "{log:?}");
    assert!(f.git.calls().is_empty(), "{:?}", f.git.calls());
    let ws = f.store.read().unwrap().unwrap();
    assert_eq!(ws.phase, Phase::Building);
    assert_eq!(ws.panes.iter().map(|p| p.key.as_str()).collect::<Vec<_>>(), ["kaaaaaaaa"]);
    assert!(ws.members["kaaaaaaaa"].launch.definition.is_some());
}

#[test] // R§10 `zygo up --all` row: a user `default_loadout = "repository"` with no user table plans `solo` and one line
fn up_all_with_a_repository_default_plans_solo() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    f.approve();
    f.o.config.workspace.default_loadout = "repository".into();
    f.o.default_loadout_set = true;
    saved(&f.store, &f.c, vec![], vec![], vec![]);
    set(&f.store, |w| w.phase = Phase::Up);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = up::testing::run_all_with(&f.o, &mut out, &mut err);
    let err = text(err);
    assert_eq!(code, 1, "{err}");
    assert!(err.starts_with("zygo: up --all reads no repository workspace files; using solo\n"), "{err}");
    // The 1: `up_target` fails on the gone tmux and `run_all` skips `app` (`zygo-workspace/src/up.rs:1747`).
    assert!(err.ends_with("zygo: tmux: no server running on /tmp/zygo-none/s\nzygo: app skipped\n"), "{err}");
    assert!(f.git.calls().is_empty(), "{:?}", f.git.calls());
    assert_eq!(f.planned().loadout, "solo");
}

#[test] // R§10: a read that fails is unread: the line and `solo` by default; exit 1 and no write when explicit
fn unread_plans_solo_or_refuses() {
    let _g = serial();
    let line = "zygo: this repository's workspace files are unread (git symbolic-ref: failed); nothing from them was applied";
    let mut f = fx(&[(WS, b"version = 1\n")]);
    f.o.git = Some(Arc::new(Failing));
    assert_eq!(f.up(), (1, String::new(), format!("{line}\n{GONE}")));
    assert_eq!(f.planned().loadout, "solo");
    let mut f = fx(&[(WS, b"version = 1\n")]);
    f.o.git = Some(Arc::new(Failing));
    f.o.loadout = Some("repository".into());
    assert_eq!(f.up(), (1, String::new(), format!("{line}\n")));
    assert!(!f.state_file().exists());
    assert!(f.no_new_session());
}

#[test] // R§10 lines, `esc`: a ref with U+202E prints as `\u{202e}`, not the raw character
fn line_escapes_the_ref() {
    let _g = serial();
    let f = fx(&[(WS, b"version = 1\n")]);
    git(&f.w, &["checkout", "-q", "-b", "dev\u{202e}x", "main"]);
    commit_files(&f.w, &[(WS, b"version = 1\nlayout = \"tiled\"\n")]);
    git(&f.w, &["push", "-q", "origin", "dev\u{202e}x"]);
    git(&f.c, &["fetch", "-q", "origin"]);
    git(&f.c, &["remote", "set-head", "origin", "dev\u{202e}x"]);
    let Use::NotApproved { digest, .. } = f.read() else { panic!("not approved") };
    let (code, out, err) = f.up();
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(err, format!("{}\n{GONE}", not_approved("refs/remotes/origin/dev\\u{202e}x", &digest)));
    assert!(!err.contains('\u{202e}'));
}

#[test] // R§11 "agent edits `.zygo/` in a worktree": only the default branch counts
fn worktree_and_branch_edits_change_nothing() {
    let _g = serial();
    let f = fx(&[(WS, b"version = 1\n")]);
    f.approve();
    git(&f.c, &["checkout", "-q", "-b", "work"]);
    fs::write(f.c.join(WS), "version = 2\n").unwrap();
    git(&f.c, &["add", WS]);
    git(&f.c, &["commit", "-q", "-m", "edit"]);
    assert_eq!(f.up(), gone());
    assert_eq!(f.planned().loadout, "repository");
}

// ---- WD-12: pool (R§9.5) ----

/// `root/<sub>` with one empty directory per name; returns `root/<sub>`.
fn pool_dirs(root: &Path, sub: &str, names: &[&str]) -> PathBuf {
    let p = root.join(sub);
    fs::create_dir_all(&p).unwrap();
    for n in names {
        fs::create_dir(p.join(n)).unwrap();
    }
    p
}

/// The approved default set (`version = 1`) with `[workspace] pool = text`.
fn pool_fx(text: Option<&str>) -> Fx {
    pool_fx_with(b"version = 1\n", text)
}

/// The approved `workspace.toml` `ws` with `[workspace] pool = text`.
fn pool_fx_with(ws: &[u8], text: Option<&str>) -> Fx {
    let mut f = fx(&[(WS, ws)]);
    f.approve();
    f.o.config.workspace.pool = text.map(String::from);
    f
}

/// name → (member.config_dir, launch.config_dir, launch.passthrough) of the planned members, by pane name.
fn pool_records(ws: &Workspace) -> BTreeMap<String, (Option<PathBuf>, Option<String>, bool)> {
    ws.panes
        .iter()
        .filter_map(|p| {
            let m = ws.members.get(&p.key)?;
            Some((p.name.clone()?, (m.config_dir.clone(), m.launch.config_dir.clone(), m.launch.passthrough)))
        })
        .collect()
}

/// `zygo: <names> run in passthrough on pool <dir>`, the R§9.5 line.
fn pool_line(names: &str, dir: &Path) -> String { format!("zygo: {names} run in passthrough on pool {}", esc(&dir.to_string_lossy())) }

const POOL_ALL: &str = "orchestrator, worker-1, worker-2, worker-3, worker-4";
const POOL_AB: [(&str, &str); 5] = [("orchestrator", "a"), ("worker-1", "b"), ("worker-2", "a"), ("worker-3", "b"), ("worker-4", "a")];

/// The records of step 2: `orchestrator → a`, `worker-1 → b`, … each in passthrough.
fn pool_assert_ab(ws: &Workspace, p: &Path) {
    let r = pool_records(ws);
    assert_eq!(r.len(), 5, "{r:?}");
    for (n, d) in POOL_AB {
        assert_eq!(r[n], (Some(p.join(d)), Some(p.join(d).to_str().unwrap().into()), true), "{n}");
    }
}

#[test] // R§18 items 13, 14: round-robin in name order, `config_dir` and passthrough in the record
fn pool_assigns_round_robin_with_passthrough() {
    let _g = serial();
    let f = pool_fx(Some("~/pool"));
    let p = pool_dirs(&f.t.join("home"), "pool", &["a", "b"]);
    assert_eq!(f.up(), (1, String::new(), format!("{}\n{GONE}", pool_line(POOL_ALL, &p))));
    pool_assert_ab(&f.planned(), &p);
}

#[test] // R§18 items 13, 14: the line prints once; a file in a pool directory is not read
fn pool_line_prints_once_and_unreadable_files_change_nothing() {
    let _g = serial();
    let f = pool_fx(Some("~/pool"));
    let p = pool_dirs(&f.t.join("home"), "pool", &["a", "b"]);
    let creds = p.join("a/creds");
    fs::write(&creds, "x").unwrap();
    fs::set_permissions(&creds, fs::Permissions::from_mode(0o000)).unwrap();
    let (code, out, err) = f.up();
    assert_eq!((code, out, err.clone()), (1, String::new(), format!("{}\n{GONE}", pool_line(POOL_ALL, &p))));
    assert_eq!(err.matches("run in passthrough").count(), 1);
    pool_assert_ab(&f.planned(), &p);
}

#[test] // R§18 item 15: a pool member with a profile adds the note; the record keeps the profile
fn pool_profile_adds_the_does_not_apply_note() {
    let _g = serial();
    let ws = b"version = 1\n[members.orchestrator]\njob = \"orchestrator\"\npool = true\n[members.worker-1]\nprofile = \"rust\"\npool = true\n";
    let f = pool_fx_with(ws, Some("~/pool"));
    let p = pool_dirs(&f.t.join("home"), "pool", &["a", "b"]);
    assert_eq!(
        f.up().2,
        format!("{}; profile rust of worker-1 does not apply in passthrough\n{GONE}", pool_line("orchestrator, worker-1", &p))
    );
    let ws = f.planned();
    let key = &ws.panes.iter().find(|p| p.name.as_deref() == Some("worker-1")).unwrap().key;
    // the record keeps the profile name; the launcher decides (WD-13)
    assert_eq!(ws.members[key].launch.profile.as_deref(), Some("rust"));
}

/// Step 2's build, then its planned records saved as a stopped workspace whose members can resume; returns the pool.
fn pool_saved(f: &Fx) -> PathBuf {
    let p = pool_dirs(&f.t.join("home"), "pool", &["a", "b"]);
    assert_eq!(f.up().0, 1);
    let planned = f.planned();
    pool_assert_ab(&planned, &p);
    set(&f.store, |w| {
        *w = planned.clone();
        w.phase = Phase::Down;
        w.build_plan = None;
        w.fresh_members.clear();
        for m in w.members.values_mut() {
            m.state = MemberState::Resumable;
            m.agent_session = Some("s".into());
        }
    });
    p
}

#[test] // R§18 item 13, R§9.5 "Stable across restarts": a restore assigns nothing and prints no pool line
fn pool_restore_keeps_saved_directories() {
    let _g = serial();
    let f = pool_fx(Some("~/pool"));
    let p = pool_saved(&f);
    fs::create_dir(p.join("0")).unwrap();
    assert_eq!(f.up(), gone());
    pool_assert_ab(&f.planned(), &p);
}

#[test] // R§18 item 14: the pool removed: a restore keeps passthrough and the directory; `up --reset` gives neither
fn pool_removed_restore_keeps_passthrough_and_reset_drops_it() {
    let _g = serial();
    let mut f = pool_fx(Some("~/pool"));
    let p = pool_saved(&f);
    f.o.config.workspace.pool = None;
    assert_eq!(f.up(), gone());
    pool_assert_ab(&f.planned(), &p);
    f.o.reset = true;
    // `--reset` drops the saved sessions with one line each; no pool line.
    let (code, out, err) = f.up();
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.ends_with(GONE) && !err.contains("passthrough"), "{err}");
    let r = pool_records(&f.planned());
    assert_eq!(r.len(), 5, "{r:?}");
    assert!(r.values().all(|v| *v == (None, None, false)), "{r:?}");
}

#[test] // R§9.5 "A pool that does not work", review focus 34, 35: exit 1 before any write
fn pool_broken_exits_1_and_writes_nothing() {
    let _g = serial();
    let mut f = pool_fx(None);
    fs::write(f.t.join("file"), "x").unwrap();
    pool_dirs(&f.t, "empty", &[".x"]);
    let none = f.t.join("none").to_str().unwrap().to_string();
    let file = f.t.join("file").to_str().unwrap().to_string();
    let empty = f.t.join("empty").to_str().unwrap().to_string();
    for (text, reason) in
        [(none.as_str(), "not a directory"), (file.as_str(), "not a directory"), (empty.as_str(), "no subdirectory"), ("rel", "must be absolute after ~ expansion")]
    {
        f.o.config.workspace.pool = Some(text.into());
        assert_eq!(f.up(), (1, String::new(), format!("zygo: pool {}: {reason}; nothing started\n", esc(text))), "{text}");
        assert!(!f.state_file().exists(), "{text}");
        assert!(f.no_new_session(), "{text}");
    }
}

#[test] // R§9.5 ruling 8: a plan with no pool member does not read the pool
fn pool_broken_with_loadout_solo_succeeds() {
    let _g = serial();
    let mut f = pool_fx(None);
    f.o.config.workspace.pool = Some(f.t.join("none").to_str().unwrap().into());
    f.o.loadout = Some("solo".into());
    assert_eq!(f.up(), gone());
    assert_eq!(f.planned().loadout, "solo");
}

#[test] // R§9.5 "No pool": `pool = true` has no effect
fn pool_absent_gives_no_config_dir_or_passthrough() {
    let _g = serial();
    let f = pool_fx(None);
    assert_eq!(f.up(), gone());
    let r = pool_records(&f.planned());
    assert_eq!(r.len(), 5, "{r:?}");
    assert!(r.values().all(|v| *v == (None, None, false)), "{r:?}");
}

#[test] // R§9.5: a member that does not take the pool is unchanged
fn pool_non_pool_member_gets_neither() {
    let _g = serial();
    let ws = b"version = 1\n[members.orchestrator]\njob = \"orchestrator\"\npool = true\n[members.plain]\n";
    let f = pool_fx_with(ws, Some("~/pool"));
    let p = pool_dirs(&f.t.join("home"), "pool", &["a"]);
    assert_eq!(f.up(), (1, String::new(), format!("{}\n{GONE}", pool_line("orchestrator", &p))));
    let r = pool_records(&f.planned());
    assert_eq!(r["orchestrator"], (Some(p.join("a")), Some(p.join("a").to_str().unwrap().into()), true));
    assert_eq!(r["plain"], (None, None, false));
}

#[test] // R§18 item 14, R§9.5 "Local mode only": `[server] url` with a pool member refuses before any pane starts
fn pool_with_server_url_exits_1_and_starts_nothing() {
    let _g = serial();
    let mut f = pool_fx(Some("~/pool"));
    let p = pool_dirs(&f.t.join("home"), "pool", &["a", "b"]);
    f.o.config.server.url = Some("https://zygo.invalid".into());
    assert_eq!(
        f.up(),
        (1, String::new(), format!("zygo: pool {}: passthrough is local mode only, and [server] url is set; nothing started\n", p.display()))
    );
    assert!(!f.state_file().exists());
    assert!(f.no_new_session());
}

#[test] // WD-10 F3, D1: the `config check` job and pool rules apply to a user loadout before any write
fn pool_user_loadout_job_and_pool_errors_refuse() {
    let _g = serial();
    let mut f = fx(&[]);
    f.o.loadout = Some("pair".into());
    let pane = |kind: &str, job: Option<&str>, pool: bool, config_dir: Option<&str>| LoadoutPane {
        kind: kind.into(),
        job: job.map(String::from),
        pool,
        config_dir: config_dir.map(String::from),
        ..LoadoutPane::default()
    };
    let refused = [
        (vec![pane("shell", Some("worker"), false, None)], "loadout.pair.window[0].pane[0].job: only with kind = \"claude-code\" or \"opencode\""),
        (
            vec![pane("claude-code", Some("orchestrator"), false, None), pane("claude-code", Some("orchestrator"), false, None)],
            "loadout.pair.window[0].pane[1].job: only one orchestrator per loadout",
        ),
        (vec![pane("opencode", None, true, None)], "loadout.pair.window[0].pane[0].pool: only with kind = \"claude-code\""),
        (vec![pane("claude-code", None, true, Some("/c"))], "loadout.pair.window[0].pane[0].pool: not with config_dir"),
    ];
    for (panes, line) in refused {
        f.o.config.loadout.insert("pair".into(), Loadout { window: vec![LoadoutWindow { name: None, layout: None, pane: panes }] });
        assert_eq!(f.up(), (1, String::new(), format!("zygo: {}; nothing started\n", esc(line))), "{line}");
        assert!(!f.state_file().exists(), "{line}");
    }
    let ok = vec![pane("claude-code", Some("worker"), false, None)];
    f.o.config.loadout.insert("pair".into(), Loadout { window: vec![LoadoutWindow { name: None, layout: None, pane: ok }] });
    assert_eq!(f.up(), gone());
}

/// A pane of a user loadout.
fn pool_pane(kind: &str, name: Option<&str>, pool: bool, profile: Option<&str>) -> LoadoutPane {
    LoadoutPane { kind: kind.into(), name: name.map(String::from), pool, profile: profile.map(String::from), ..LoadoutPane::default() }
}

/// No workspace files; the user loadout `pair` of one window `window` with `panes`; `[workspace] pool = "~/pool"` with
/// directories `a`, `b`; returns the pool.
fn pool_user(window: Option<&str>, panes: Vec<LoadoutPane>) -> (Fx, PathBuf) {
    let mut f = fx(&[]);
    let w = LoadoutWindow { name: window.map(String::from), layout: None, pane: panes };
    f.o.config.loadout.insert("pair".into(), Loadout { window: vec![w] });
    f.o.config.workspace.pool = Some("~/pool".into());
    let p = pool_dirs(&f.t.join("home"), "pool", &["a", "b"]);
    (f, p)
}

fn pool_xy() -> Vec<LoadoutPane> {
    vec![pool_pane("claude-code", Some("y"), true, None), pool_pane("claude-code", Some("x"), true, None), pool_pane("shell", None, false, None)]
}

/// `x → a`, `y → b` in passthrough, through the user loadout `pair`.
fn pool_assert_xy(f: &Fx, p: &Path) {
    assert_eq!(f.up(), (1, String::new(), format!("{}\n{GONE}", pool_line("x, y", p))));
    let ws = f.planned();
    assert_eq!(ws.loadout, "pair");
    let r = pool_records(&ws);
    assert_eq!(r["x"], (Some(p.join("a")), Some(p.join("a").to_str().unwrap().into()), true));
    assert_eq!(r["y"], (Some(p.join("b")), Some(p.join("b").to_str().unwrap().into()), true));
    assert!(f.git.calls().is_empty(), "{:?}", f.git.calls());
}

#[test] // review F1: a refusal after step 7 (check_plan) prints no pool line
fn pool_line_not_printed_when_a_later_step_refuses() {
    let _g = serial();
    let (mut f, _p) = pool_user(Some("w\u{1}"), pool_xy());
    f.o.loadout = Some("pair".into());
    let (code, out, err) = f.up();
    assert_eq!((code, out.as_str()), (1, ""));
    assert_eq!(err, "zygo: window 0: name \"w\\u{1}\" refused: window name holds a control character\n");
    assert!(!f.state_file().exists());
}

#[test] // review F2a, R§9.5: `--loadout <user>` takes the pool
fn pool_user_loadout_by_flag_assigns() {
    let _g = serial();
    let (mut f, p) = pool_user(None, pool_xy());
    f.o.loadout = Some("pair".into());
    pool_assert_xy(&f, &p);
}

#[test] // review F2b, R§9.5: `[workspace] default_loadout` naming a user loadout takes the pool
fn pool_user_default_loadout_assigns() {
    let _g = serial();
    let (mut f, p) = pool_user(None, pool_xy());
    f.o.config.workspace.default_loadout = "pair".into();
    f.o.default_loadout_set = true;
    pool_assert_xy(&f, &p);
}

#[test] // review F2c, R§9.5 "Local mode only": the refusal on a user loadout
fn pool_user_loadout_with_server_url_refuses() {
    let _g = serial();
    let (mut f, p) = pool_user(None, pool_xy());
    f.o.loadout = Some("pair".into());
    f.o.config.server.url = Some("https://zygo.invalid".into());
    assert_eq!(
        f.up(),
        (1, String::new(), format!("zygo: pool {}: passthrough is local mode only, and [server] url is set; nothing started\n", p.display()))
    );
    assert!(!f.state_file().exists());
    assert!(f.no_new_session());
}

#[test] // review F2d, D3: an unnamed pool member is `pane <n>` (1-based in the plan), after the named ones
fn pool_unnamed_member_is_pane_n() {
    let _g = serial();
    let panes = vec![pool_pane("shell", None, false, None), pool_pane("claude-code", None, true, None), pool_pane("claude-code", Some("z"), true, None)];
    let (mut f, p) = pool_user(None, panes);
    f.o.loadout = Some("pair".into());
    assert_eq!(f.up(), (1, String::new(), format!("{}\n{GONE}", pool_line("z, pane 2", &p))));
    let ws = f.planned();
    let unnamed = &ws.members[&ws.panes[1].key];
    assert_eq!((unnamed.config_dir.clone(), unnamed.launch.passthrough), (Some(p.join("b")), true));
    assert_eq!(pool_records(&ws)["z"].0, Some(p.join("a")));
}

#[test] // review F2e, R§9.5 "not readable": a pool that cannot be read or searched refuses; a closed subdirectory is
// listed (names and types only, R-C6)
fn pool_unreadable_root_exits_1() {
    let _g = serial();
    let (mut f, p) = pool_user(None, pool_xy());
    f.o.loadout = Some("pair".into());
    for mode in [0o000, 0o600] {
        fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
        let root = fs::read_dir(&p).and_then(|mut d| d.try_for_each(|e| e?.metadata().map(|_| ()))).is_ok();
        let got = f.up();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        if root {
            continue; // running as root: permissions do not apply
        }
        assert_eq!(got, (1, String::new(), format!("zygo: pool {}: not readable (PermissionDenied); nothing started\n", p.display())), "{mode:o}");
        assert!(!f.state_file().exists());
        assert!(f.no_new_session());
    }
    fs::set_permissions(p.join("a"), fs::Permissions::from_mode(0o000)).unwrap();
    let got = f.up();
    fs::set_permissions(p.join("a"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(got, (1, String::new(), format!("{}\n{GONE}", pool_line("x, y", &p))));
}

#[test] // review F2f: names, profiles and the pool path go through `esc`
fn pool_line_escapes_control_characters() {
    let _g = serial();
    let panes = vec![pool_pane("claude-code", Some("a\u{1}"), true, Some("r\u{7}x")), pool_pane("claude-code", Some("b"), true, None)];
    let mut f = fx(&[]);
    f.o.config.loadout.insert("pair".into(), Loadout { window: vec![LoadoutWindow { name: None, layout: None, pane: panes }] });
    let p = pool_dirs(&f.t, "p\u{1b}ool", &["a", "b"]);
    f.o.config.workspace.pool = Some(p.to_str().unwrap().into());
    f.o.loadout = Some("pair".into());
    let (code, _, err) = f.up();
    assert_eq!(code, 1);
    // `a\u{1}` is not a valid member name (AU84), so the pane is unnamed: `pane 1`.
    let line = format!("{}; profile {} of pane 1 does not apply in passthrough\n", pool_line("b, pane 1", &p), esc("r\u{7}x"));
    assert!(line.contains("r\\u{07}x"), "{line}");
    assert!(line.contains("p\\u{1b}ool"), "{line}");
    assert!(err.contains(&line), "{err}");
    assert!(!err.chars().any(|c| c.is_control() && c != '\n'), "{err:?}");
}

#[test] // FX-25 f5, R§10 zygo up row, R§6.1 step 1: the read runs at the top of the current checkout
fn first_worktree_deleted_reads_the_definition_at_the_current_checkout() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    git(&f.t, &["clone", "-q", "--bare", "o.git", "b.git"]);
    let b = canon(&f.t.join("b.git"));
    git(&b, &["worktree", "add", "-q", "--detach", "../one", "main"]);
    git(&b, &["worktree", "add", "-q", "--detach", "../two", "main"]);
    let two = canon(&f.t.join("two"));
    // `one` stays the first non-bare entry of `git worktree list` (the main worktree, AU77) with its folder gone.
    fs::remove_dir_all(f.t.join("one")).unwrap();
    let id = workspace_id(&b);
    // The bare clone has no `origin/HEAD`, so the read is `refs/heads/main` (R§6.1 step 2).
    let Use::NotApproved { digest, .. } = check_default(&two, &id, &f.o.paths.approvals_file, &SystemGit) else {
        panic!("not approved")
    };
    approve_workspace_in(&f.o.paths.approvals_file, &id, &digest).unwrap();
    let store = Store::open(&f.o.paths, &id);
    let (state, planned) = (store.dir().join("workspace.json"), f.t.join("planned.json"));
    up::testing::hook(
        store.dir(),
        "before_undo",
        Some(Box::new(move || fs::copy(&state, &planned).map(|_| ()).map_err(|e| e.to_string()))),
    );
    f.o.cwd = two;
    // Stderr is only the tmux line: no not-approved and no unread line.
    assert_eq!(f.up(), gone());
    assert_eq!(f.git.reads(), 1);
    let ws = f.planned();
    assert_eq!((ws.loadout.as_str(), ws.common_dir.clone()), ("repository", b.clone()));
    assert!(ws.members.values().all(|m| m.launch.definition.as_ref().is_some_and(|p| p.digest == digest)));
}

/// A bare clone `b.git` of `o.git` with the detached worktrees `one` and `two` (in that order), the digest of the
/// read at `two` approved, and the `before_undo` hook of its store; returns (`b.git`, `one`, `two`, the store).
fn two_worktrees(f: &Fx) -> (PathBuf, PathBuf, PathBuf, Store) {
    git(&f.t, &["clone", "-q", "--bare", "o.git", "b.git"]);
    let b = canon(&f.t.join("b.git"));
    git(&b, &["worktree", "add", "-q", "--detach", "../one", "main"]);
    git(&b, &["worktree", "add", "-q", "--detach", "../two", "main"]);
    let (one, two) = (canon(&f.t.join("one")), canon(&f.t.join("two")));
    let id = workspace_id(&b);
    let Use::NotApproved { digest, .. } = check_default(&two, &id, &f.o.paths.approvals_file, &SystemGit) else {
        panic!("not approved")
    };
    approve_workspace_in(&f.o.paths.approvals_file, &id, &digest).unwrap();
    let store = Store::open(&f.o.paths, &id);
    let (state, planned) = (store.dir().join("workspace.json"), f.t.join("planned.json"));
    up::testing::hook(
        store.dir(),
        "before_undo",
        Some(Box::new(move || fs::copy(&state, &planned).map(|_| ()).map_err(|e| e.to_string()))),
    );
    (b, one, two, store)
}

/// One pane of [`plan_of`]: kind, name, cwd and pin.
type PlanPane = (PaneKind, Option<String>, PathBuf, Option<Pin>);

/// The parts of a planned state that do not depend on the random pane keys: name, loadout, `main_root`, and per pane
/// its kind, name, cwd and pin.
fn plan_of(ws: &Workspace) -> (String, String, PathBuf, Vec<PlanPane>) {
    let pane = |p: &Pane| (p.kind.clone(), p.name.clone(), p.cwd.clone(), ws.members.get(&p.key).and_then(|m| m.launch.definition.clone()));
    (ws.name.clone(), ws.loadout.clone(), ws.main_root.clone(), ws.panes.iter().map(pane).collect())
}

#[test] // FX-25 review F1, AU77, W§11: with the first worktree's folder deleted, the main worktree is the first that exists
fn first_worktree_deleted_saves_an_existing_main_root_and_pane_folders() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    let (b, one, two, _store) = two_worktrees(&f);
    fs::remove_dir_all(&one).unwrap();
    f.o.cwd = two.clone();
    assert_eq!(f.up(), gone());
    let ws = f.planned();
    assert_eq!(
        (ws.loadout.as_str(), ws.common_dir.clone(), ws.main_root.clone(), ws.name.as_str()),
        ("repository", b, two.clone(), "two")
    );
    assert_eq!(ws.panes.len(), 5);
    for p in &ws.panes {
        assert_eq!(p.cwd, two, "{:?}", p.name);
        assert!(p.cwd.is_dir(), "{:?}", p.cwd);
    }
}

#[test] // FX-25 review F2, W§8.1 step 3, AU20: by name, a saved main_root that is gone is found again from common_dir
fn by_name_and_from_the_checkout_plan_the_same_after_the_main_worktree_is_deleted() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    let (b, one, two, store) = two_worktrees(&f);
    // The state as an `up` left it while `one` existed: `main_root` = `one`, every pane in `one`.
    f.o.cwd = two.clone();
    assert_eq!(f.up(), gone());
    let first = f.planned();
    assert_eq!((first.main_root.clone(), first.name.as_str()), (one.clone(), "one"));
    let down = |ws: &Workspace| {
        set(&store, |w| {
            *w = ws.clone();
            w.phase = Phase::Down;
        })
    };
    fs::remove_dir_all(&one).unwrap();
    f.o.reset = true;
    // By name, from a folder outside the repository.
    down(&first);
    f.o.cwd = f.t.clone();
    f.o.name = Some("one".into());
    let by_name = f.up();
    let named = f.planned();
    // From the checkout `two`.
    down(&first);
    f.o.cwd = two.clone();
    f.o.name = None;
    let from_two = f.up();
    let here = f.planned();
    // FX-25 review R2 D3: the move from the saved `main_root`, which git now marks prunable, is one line.
    let moved = format!("zygo: the main worktree {} is gone; panes start in {}\n", esc(&one.to_string_lossy()), esc(&two.to_string_lossy()));
    assert_eq!(by_name, (1, String::new(), format!("{moved}{GONE}")));
    assert_eq!(by_name, from_two);
    assert_eq!(plan_of(&named), plan_of(&here));
    for ws in [&named, &here] {
        assert_eq!((ws.loadout.as_str(), ws.common_dir.clone(), ws.main_root.clone()), ("repository", b.clone(), two.clone()));
        for p in &ws.panes {
            assert_eq!(p.cwd, two, "{:?}", p.name);
            assert!(p.cwd.is_dir(), "{:?}", p.cwd);
        }
    }
}

#[test] // FX-25 review R2 F1, D2, W§8.1 step 3, AU77: the saved main_root wins while it is valid. The first worktree is
        // away during the first `up`, so `two` is saved; when `one` is back, `up <name>` and `up` from `one` keep `two`
fn saved_main_root_wins_while_valid_by_name_and_from_any_checkout() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    let (_b, one, two, store) = two_worktrees(&f);
    let off = f.t.join("one.off");
    fs::rename(&one, &off).unwrap();
    f.o.cwd = two.clone();
    assert_eq!(f.up(), gone());
    let first = f.planned();
    assert_eq!((first.main_root.clone(), first.name.as_str()), (two.clone(), "two"));
    fs::rename(&off, &one).unwrap();
    let down = |ws: &Workspace| {
        set(&store, |w| {
            *w = ws.clone();
            w.phase = Phase::Down;
        })
    };
    f.o.reset = true;
    // By name, from a folder outside the repository.
    down(&first);
    f.o.cwd = f.t.clone();
    f.o.name = Some("two".into());
    let by_name = f.up();
    let named = f.planned();
    // From the checkout `one`, which is again the first worktree of the list.
    down(&first);
    f.o.cwd = one.clone();
    f.o.name = None;
    let from_one = f.up();
    let here = f.planned();
    // No move line: the saved `main_root` is still valid.
    assert_eq!((by_name, from_one), (gone(), gone()));
    assert_eq!((named.main_root.clone(), here.main_root.clone()), (two.clone(), two.clone()));
    assert_eq!((named.name.as_str(), here.name.as_str()), ("two", "two"));
    assert_eq!(plan_of(&named), plan_of(&here));
    assert!(named.panes.iter().chain(&here.panes).all(|p| p.cwd == two), "{:?} {:?}", named.panes, here.panes);
}

/// A workspace of `two_worktrees` that an `up` from `two` saved (`main_root` = `one`), then both worktree folders
/// deleted (git marks both `prunable`), its state set to `phase = down` with the tmux socket `<t>/sock/s` (a folder of
/// mode 0700), and `f.o` set to `up <name>` with `--reset` from a folder outside the repository. Returns the store,
/// the saved state and `one`.
fn every_worktree_folder_gone(f: &mut Fx) -> (Store, Workspace, PathBuf) {
    let (store, first, one, two) = saved_down_by_name(f);
    fs::remove_dir_all(&one).unwrap();
    fs::remove_dir_all(&two).unwrap();
    (store, first, one)
}

/// A workspace of `two_worktrees` that an `up` from `two` saved (`main_root` = `one`), its state set to `phase = down`
/// with the tmux socket `<t>/sock/s` (a folder of mode 0700), and `f.o` set to `up <name>` with `--reset` from a folder
/// outside the repository. Returns the store, the saved state, `one` and `two`.
fn saved_down_by_name(f: &mut Fx) -> (Store, Workspace, PathBuf, PathBuf) {
    let (_b, one, two, store) = two_worktrees(f);
    f.o.cwd = two.clone();
    assert_eq!(f.up(), gone());
    let first = f.planned();
    assert_eq!(first.main_root, one);
    let sock = f.t.join("sock");
    fs::create_dir_all(&sock).unwrap();
    fs::set_permissions(&sock, fs::Permissions::from_mode(0o700)).unwrap();
    set(&store, |w| {
        *w = first.clone();
        w.phase = Phase::Down;
        w.tmux_socket = Some(sock.join("s"));
    });
    f.o.reset = true;
    f.o.cwd = f.t.clone();
    f.o.name = Some(first.name.clone());
    (store, first, one, two)
}

/// The D4 line of `up <name>` for the saved `main_root` `one`.
fn no_checkout_line(one: &Path) -> String { format!("zygo: no checkout at the main worktree {}", esc(&one.to_string_lossy())) }

/// The tmux calls of the stand-in log of `f` that check the session on the saved socket `<t>/sock/s`.
fn session_checks(f: &Fx) -> usize {
    let sock = format!("-S {}", f.t.join("sock/s").display());
    tmux_log(&f.t).iter().filter(|l| l.contains(&sock) && l.contains("has-session")).count()
}

#[test] // FX-25 review R2 F4, D4, W§8.1 step 3: by name, with no valid worktree left, `up` exits 1, reads nothing and saves
        // nothing; it plans no pane in a folder that is gone
fn by_name_with_no_valid_worktree_exits_1_and_saves_nothing() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    let (store, _first, one) = every_worktree_folder_gone(&mut f);
    let state = store.dir().join("workspace.json");
    let before = fs::read(&state).unwrap();
    let reads = f.git.reads();
    let sessions_before = tmux_log(&f.t).iter().filter(|l| l.contains("new-session")).count();
    let got = f.up();
    assert_eq!(got, (1, String::new(), format!("{}\n", no_checkout_line(&one))));
    assert!(!got.2.contains("symbolic-ref"), "{got:?}");
    assert_eq!(fs::read(&state).unwrap(), before);
    assert_eq!(f.git.reads(), reads);
    assert!(!f.t.join("planned.json").exists());
    let sessions = |log: Vec<String>| log.iter().filter(|l| l.contains("new-session")).count();
    assert_eq!(sessions(tmux_log(&f.t)), sessions_before);
}

#[test] // FX-25 close, D4 (W§8.1 step 3): D4 is for `zygo up` only; `zygo down <name>` of a workspace whose every worktree
        // folder is gone uses the saved state, reads no definition and reaches its tmux step
fn down_by_name_with_no_valid_worktree_reaches_its_tmux_step() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    let (store, first, one) = every_worktree_folder_gone(&mut f);
    let before = fs::read(store.dir().join("workspace.json")).unwrap();
    let reads = f.git.reads();
    let checks = session_checks(&f);
    let o = DownOpts {
        target: DownTarget::Name(first.name.clone()),
        now: false,
        timeout: None,
        wait: false,
        cwd: f.t.clone(),
        zygo: f.t.join("zygo"),
        tmux: f.o.tmux.clone(),
        tmux_env: None,
        tmux_pane: None,
        zygo_session: None,
        proc_probe: |_| None,
        paths: f.o.paths.clone(),
        config: Config::default(),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = down::run(&o, &mut out, &mut err);
    let got = (code, text(out), text(err));
    assert!(!got.2.contains(&no_checkout_line(&one)), "{got:?}");
    assert_eq!(got, (0, format!("zygo: {} is not running\n", first.name), String::new()));
    assert!(session_checks(&f) > checks, "{:?}", tmux_log(&f.t));
    assert_eq!(f.git.reads(), reads);
    assert_eq!(fs::read(store.dir().join("workspace.json")).unwrap(), before);
}

#[test] // FX-25 close, D4 (W§8.1 step 3): `zygo attach <name>` of a workspace whose every worktree folder is gone uses the
        // saved state and reaches its tmux session check
fn attach_by_name_with_no_valid_worktree_reaches_its_tmux_step() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    let (_store, first, one) = every_worktree_folder_gone(&mut f);
    let reads = f.git.reads();
    let checks = session_checks(&f);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = up::testing::attach_with(&f.o.paths, &f.o.tmux, None, Some(&first.name), &f.t, &mut out, &mut err);
    let got = (code, text(out), text(err));
    assert!(!got.2.contains(&no_checkout_line(&one)), "{got:?}");
    let n = &first.name;
    assert_eq!(got, (1, String::new(), format!("zygo: {n} is down; run zygo up -- {n}\n")));
    assert!(session_checks(&f) > checks, "{:?}", tmux_log(&f.t));
    assert_eq!(f.git.reads(), reads);
}

/// The first `git` file on the `PATH` of the test process (read only: no test changes that `PATH`).
fn real_git() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).map(|d| d.join("git")).find(|g| g.is_file()).expect("git on PATH")
}

/// `<dir>/<sub>/git`: a call whose arguments hold `worktree list` adds its arguments as one line to `<dir>/<sub>/list.log`,
/// then runs the shell code `list`; every other call is `exec '<real_git()>' "$@"`. Returns the program and the log.
fn list_git(dir: &Path, sub: &str, list: &str) -> (PathBuf, PathBuf) {
    let bin = dir.join(sub);
    fs::create_dir_all(&bin).unwrap();
    let log = bin.join("list.log");
    let body = format!(
        "case \" $* \" in *\" worktree list \"*) printf '%s\\n' \"$*\" >> '{}'; {list};; esac\nexec '{}' \"$@\"",
        log.display(),
        real_git().display()
    );
    (script(&bin, "git", &body), log)
}

/// The `worktree list` calls that the stub of [`list_git`] logged.
fn list_calls(log: &Path) -> usize { fs::read_to_string(log).unwrap_or_default().lines().count() }

/// FX-25 close (W§8.1 step 3, R§10 `zygo up` row): with the stub `git`, whose `worktree list` in the saved `common_dir`
/// fails or does not end, `zygo down <name>` and `zygo attach <name>` use the saved state, run no list and reach their
/// tmux step; `zygo up <name>` exits 1 with the reason `want`, saves nothing and runs the list once.
fn down_and_attach_by_name_need_no_worktree_list(f: &mut Fx, git: &Path, log: &Path, want: &str) {
    let (store, first, _one, _two) = saved_down_by_name(f);
    let state = store.dir().join("workspace.json");
    let before = fs::read(&state).unwrap();
    let n = first.name.clone();
    // `zygo down <name>`: exit 0, `not running`, `has-session` on the saved socket.
    let checks = session_checks(f);
    let o = DownOpts {
        target: DownTarget::Name(n.clone()),
        now: false,
        timeout: None,
        wait: false,
        cwd: f.t.clone(),
        zygo: f.t.join("zygo"),
        tmux: f.o.tmux.clone(),
        tmux_env: None,
        tmux_pane: None,
        zygo_session: None,
        proc_probe: |_| None,
        paths: f.o.paths.clone(),
        config: Config::default(),
    };
    let mut hooks = down::testing::Hooks::new(std::time::SystemTime::now());
    hooks.git = Some(git.as_os_str().to_os_string());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = down::testing::run_with(&o, &hooks, &mut out, &mut err);
    assert_eq!((code, text(out), text(err)), (0, format!("zygo: {n} is not running\n"), String::new()));
    assert!(session_checks(f) > checks, "{:?}", tmux_log(&f.t));
    assert_eq!(fs::read(&state).unwrap(), before);
    // `zygo attach <name>`: its session check on the saved socket, then the `is down` line.
    let checks = session_checks(f);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = up::testing::attach_git_with(&f.o.paths, &f.o.tmux, None, Some(&n), &f.t, git.as_os_str(), &mut out, &mut err);
    assert_eq!((code, text(out), text(err)), (1, String::new(), format!("zygo: {n} is down; run zygo up -- {n}\n")));
    assert!(session_checks(f) > checks, "{:?}", tmux_log(&f.t));
    assert_eq!(list_calls(log), 0);
    // `zygo up <name>`: exit 1 with the reason of the list, before any read or write.
    let reads = f.git.reads();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = up::testing::run_git_with(&f.o, git.as_os_str(), &mut out, &mut err);
    assert_eq!((code, text(out), text(err)), (1, String::new(), format!("{want}\n")));
    assert_eq!(list_calls(log), 1);
    assert_eq!(f.git.reads(), reads);
    assert_eq!(fs::read(&state).unwrap(), before);
}

#[test] // FX-25 close, W§8.1 step 3: a worktree list that fails in the saved `common_dir` stops only `zygo up <name>`
fn down_and_attach_by_name_reach_tmux_when_the_worktree_list_fails() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    let (git, log) = list_git(&f.t, "failing", "echo 'fatal: stub' >&2; exit 1");
    down_and_attach_by_name_need_no_worktree_list(&mut f, &git, &log, "zygo: git worktree list failed: fatal: stub");
}

#[test] // FX-25 close, W§8.1 step 3: a worktree list that times out in the saved `common_dir` stops only `zygo up <name>`;
        // the list opens a FIFO that no one writes, so only `up::GIT_BOUND` ends it
fn down_and_attach_by_name_reach_tmux_when_the_worktree_list_times_out() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    let fifo = f.t.join("list.fifo");
    assert!(Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    let (git, log) = list_git(&f.t, "blocking", &format!("exec cat '{}'", fifo.display()));
    down_and_attach_by_name_need_no_worktree_list(&mut f, &git, &log, "zygo: git worktree list timed out");
}

/// The main worktree that `up::find_checkout` gives from `cwd` with the cache folder of `f` (D1).
fn main_from(f: &Fx, cwd: &Path) -> Result<PathBuf, up::Exit> {
    up::find_checkout(cwd, S5, &f.o.paths.workspace_cache_dir()).map(|(_, _, main)| main)
}

/// The paths of `git worktree list --porcelain` in `dir`, in git's order.
fn listed(dir: &Path) -> Vec<PathBuf> {
    let text = git(dir, &["worktree", "list", "--porcelain"]);
    text.lines().filter_map(|l| l.strip_prefix("worktree ")).map(PathBuf::from).collect()
}

#[test] // FX-25 review R2 F2, D1, AU77: zygo's own mail-sync worktree under the workspaces cache folder sorts first in the
        // list of git; it is never the main worktree
fn mail_sync_worktree_listed_first_is_not_the_main_worktree() {
    let _g = serial();
    let mut f = fx(&[(WS, b"version = 1\n")]);
    let (b, one, two, _store) = two_worktrees(&f);
    let sync = f.o.paths.workspace_cache_dir().join(workspace_id(&b)).join("mail-sync");
    fs::create_dir_all(sync.parent().unwrap()).unwrap();
    git(&b, &["worktree", "add", "-q", "--detach", sync.to_str().unwrap(), "main"]);
    let sync = canon(&sync);
    // git's order: the bare entry, then the sync worktree before `one` and `two`.
    assert_eq!(listed(&b), [b.clone(), sync.clone(), one.clone(), two.clone()]);
    assert_eq!(main_from(&f, &two), Ok(one.clone()));
    assert_eq!(main_from(&f, &b), Ok(one.clone()));
    f.o.cwd = two.clone();
    assert_eq!(f.up(), gone());
    let ws = f.planned();
    assert_eq!((ws.main_root.clone(), ws.name.as_str()), (one.clone(), "one"));
    assert!(ws.panes.iter().all(|p| p.cwd == one), "{:?}", ws.panes);
}

#[test] // FX-25 review R2 F3, D1, AU77: an empty folder at the place of a deleted worktree (an empty mount point) is
        // marked `prunable` by git; it is not the main worktree, also from the bare folder
fn empty_folder_of_a_deleted_worktree_is_not_the_main_worktree() {
    let _g = serial();
    let f = fx(&[(WS, b"version = 1\n")]);
    let (b, one, two, _store) = two_worktrees(&f);
    fs::remove_dir_all(&one).unwrap();
    fs::create_dir(&one).unwrap();
    assert!(git(&b, &["worktree", "list", "--porcelain"]).contains("\nprunable "));
    assert_eq!(main_from(&f, &two), Ok(two.clone()));
    assert_eq!(main_from(&f, &b), Ok(two.clone()));
}

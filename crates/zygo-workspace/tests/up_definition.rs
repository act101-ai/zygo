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
        assert_eq!(m.launch.definition, Some(Pin::new(f.head(MAIN), pin.digest.clone())));
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
        assert_eq!(m.launch.definition, Some(Pin::new(f.head(MAIN), p1.digest.clone())));
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
        assert_eq!(m.launch.definition, Some(Pin::new(f.head("refs/remotes/origin/dev"), pin.digest.clone())));
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
    let _ = up::testing::run_all_with(&f.o, &mut out, &mut err);
    let err = text(err);
    assert!(err.starts_with("zygo: up --all reads no repository workspace files; using solo\n"), "{err}");
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

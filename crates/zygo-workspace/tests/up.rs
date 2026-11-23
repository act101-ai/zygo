//! Node W7: `zygo up` for one workspace: plan, build, repair, attach (W§8.1 steps 2-13, W§12).
//!
//! Every test and helper is `cfg(unix)`: the stand-in programs are shell scripts (D26). Every test takes `serial()`, so
//! no test writes a script while another test forks (ETXTBSY). The tmux tests use a private server
//! (`PrivateServer`), so no call reaches the default tmux server.
#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_config::{Config, Loadout, LoadoutPane, LoadoutWindow};
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_workspace::identity::workspace_id;
use zygo_workspace::mux::testing::{require_tmux, PrivateServer};
use zygo_workspace::mux::tmux::Tmux;
use zygo_workspace::mux::{Multiplexer, UnsafePath};
use zygo_workspace::repair;
use zygo_workspace::state::{
    BuildPlan, MemberRecord, MemberState, Pane, PaneKind, Pending, Phase, Store, Subagent, Window, Workspace,
};
use zygo_workspace::up::{self, preflight, Exit, UpOpts};

const S5: Duration = Duration::from_secs(5);
const OTHER_ID: &str = "fedcba9876543210";
const AU54: &str =
    "zygo: the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it";

// ---- helpers (step 1) ----

fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").current_dir(dir)
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid"]).args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1")
        .output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn canon(p: &Path) -> PathBuf { p.canonicalize().unwrap() }

/// `<parent>/<name>`: a repository on `main` with one commit; returns the canonical path.
fn repo(parent: &Path, name: &str) -> PathBuf {
    fs::create_dir_all(parent).unwrap();
    git(parent, &["init", "-q", "-b", "main", name]);
    git(&parent.join(name), &["commit", "-q", "--allow-empty", "-m", "a"]);
    canon(&parent.join(name))
}

/// An executable `#!/bin/sh` program `<dir>/<name>`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let f = dir.join(name);
    fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
    f
}

/// A tmux stand-in that prints `line` for every call.
fn version_tmux(dir: &Path, name: &str, line: &str) -> PathBuf { script(dir, name, &format!("printf '%s\\n' '{line}'")) }

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

fn tmux_log(dir: &Path) -> Vec<String> {
    let s = fs::read_to_string(dir.join("tmux.log")).unwrap_or_default();
    s.split('\u{1e}').filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// The zygo stand-in of D25: `ws run` copies the state to `<dir>/runs/<key>.json`, waits `delay`, changes to the pane's
/// cwd read from that copy and runs `sleep 600`; every other call exits 0.
fn stand_in(dir: &Path, state: &Path, delay: &str) -> PathBuf {
    let runs = dir.join("runs");
    let body = format!(
        "if [ \"$1\" = ws ] && [ \"$2\" = run ]; then\n\
         mkdir -p '{runs}'\ncp '{state}' '{runs}'/\"$6\".json\nsleep {delay}\n\
         d=$(awk -v k=\"\\\"$6\\\"\" '/\"key\": / {{ f = index($0, k) > 0 }} f && /\"cwd\": / {{ sub(/.*\"cwd\": \"/, \"\"); sub(/\",?$/, \"\"); print; exit }}' '{runs}'/\"$6\".json)\n\
         cd \"$d\"\nexec sleep 600\nfi\nexit 0",
        runs = runs.display(),
        state = state.display()
    );
    script(dir, "zygo", &body)
}

fn opts(t: &Path, cwd: &Path, tmux: &Path, zygo: &Path) -> UpOpts {
    let mut vars = BTreeMap::new();
    vars.insert(OsString::from("PATH"), std::env::var_os("PATH").unwrap_or_default());
    vars.insert(OsString::from("HOME"), t.join("home").into_os_string());
    UpOpts {
        name: None,
        loadout: None,
        reset: false,
        no_attach: true,
        cwd: cwd.to_path_buf(),
        zygo: zygo.to_path_buf(),
        tmux: tmux.to_path_buf(),
        tmux_env: None,
        zygo_session: None,
        vars,
        paths: paths(t),
        config: Config::default(),
    }
}

fn text(b: Vec<u8>) -> String { String::from_utf8(b).unwrap() }

fn run(o: &UpOpts) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = up::testing::run_with(o, &mut out, &mut err);
    (code, text(out), text(err))
}

fn attach_run(o: &UpOpts, tmux_env: Option<&str>, name: Option<&str>) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let env = tmux_env.map(OsString::from);
    let code = up::testing::attach_with(&o.paths, &o.tmux, env.as_deref(), name, &o.cwd, &mut out, &mut err);
    (code, text(out), text(err))
}

fn repair_run(store: &Store, mux: &dyn Multiplexer, zygo: &Path) -> (anyhow::Result<()>, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let r = repair::repair(store, mux, zygo, &mut out, &mut err);
    (r, text(out), text(err))
}

fn state_of(o: &UpOpts, repo: &Path) -> Store { Store::open(&o.paths, &workspace_id(&canon(&repo.join(".git")))) }

fn state_file(store: &Store) -> PathBuf { store.dir().join("workspace.json") }

fn raw(store: &Store) -> Vec<u8> { fs::read(state_file(store)).unwrap() }

fn put_raw(store: &Store, v: &Value) {
    fs::create_dir_all(store.dir()).unwrap();
    fs::write(state_file(store), serde_json::to_vec_pretty(v).unwrap()).unwrap();
}

/// A version 1 state document for `store`'s workspace; a test changes one field.
fn doc(store: &Store, repo: &Path) -> Value {
    let id = store.dir().file_name().unwrap().to_str().unwrap().to_string();
    json!({"version": 1, "id": id, "name": "app", "common_dir": canon(&repo.join(".git")), "main_root": repo,
        "tmux_session": "zygo-app", "loadout": "solo", "phase": "down", "phase_since": 1700000000u64})
}

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

fn read(store: &Store) -> Workspace { store.read().unwrap().unwrap() }

fn lp(kind: &str) -> LoadoutPane { LoadoutPane { kind: kind.into(), ..LoadoutPane::default() } }

fn lw(name: Option<&str>, layout: Option<&str>, pane: Vec<LoadoutPane>) -> LoadoutWindow {
    LoadoutWindow { name: name.map(String::from), layout: layout.map(String::from), pane }
}

fn with_loadout(name: &str, window: Vec<LoadoutWindow>) -> Config {
    let mut c = Config::default();
    c.loadout.insert(name.into(), Loadout { window });
    c
}

/// `pair`: window `agents` (main-vertical; claude-code and a shell on `branch:feature-x`) and window `logs` (a shell).
fn pair() -> Config {
    let shell = LoadoutPane { cwd: Some("branch:feature-x".into()), ..lp("shell") };
    with_loadout("pair", vec![lw(Some("agents"), Some("main-vertical"), vec![lp("claude-code"), shell]), lw(Some("logs"), None, vec![lp("shell")])])
}

/// A `split` hook that refuses call `n` (1-based) with the tmux "no space" text.
fn fail_split(dir: &Path, n: usize) {
    let calls = AtomicUsize::new(0);
    up::testing::hook(
        dir,
        "split",
        Some(Box::new(move || {
            if calls.fetch_add(1, Ordering::SeqCst) + 1 == n { Err("no space for new pane".into()) } else { Ok(()) }
        })),
    );
}

// ---- live harness ----

static LIVE: Mutex<()> = Mutex::new(());
fn serial() -> std::sync::MutexGuard<'static, ()> { LIVE.lock().unwrap_or_else(|e| e.into_inner()) }

struct Live {
    t: TempDir,
    srv: PrivateServer,
    repo: PathBuf,
    wt: PathBuf,
    id: String,
    runs: PathBuf,
    o: UpOpts,
}

impl Live {
    fn store(&self) -> Store { Store::open(&self.o.paths, &self.id) }
}

/// Repository `app` with worktree `../wt-x` on the new branch `feature-x`, a private server and the stand-in zygo.
fn live(delay: &str) -> Live {
    let t = TempDir::new().unwrap();
    let srv = PrivateServer::start();
    let repo = repo(t.path(), "app");
    git(&repo, &["worktree", "add", "-q", "-b", "feature-x", "../wt-x"]);
    let wt = canon(&t.path().join("wt-x"));
    let id = workspace_id(&canon(&repo.join(".git")));
    let p = paths(t.path());
    let state = p.workspaces_dir().join(&id).join("workspace.json");
    let zygo = stand_in(t.path(), &state, delay);
    let o = opts(t.path(), &repo, srv.wrapper(), &zygo);
    let runs = t.path().join("runs");
    Live { t, srv, repo, wt, id, runs, o }
}

fn globals(srv: &PrivateServer) -> Vec<String> {
    [&["show-options", "-g"][..], &["show-options", "-gw"], &["show-hooks", "-g"], &["show-hooks", "-gw"]]
        .iter()
        .map(|a| srv.run(a).unwrap())
        .collect()
}

/// `@zygo-pane` → (window index, pane id) for each pane of `zygo-app`.
fn by_key(srv: &PrivateServer) -> BTreeMap<String, (u32, String)> {
    let out = srv.run(&["list-panes", "-s", "-t", "=zygo-app:", "-F", "#{@zygo-pane} #{window_index} #{pane_id}"]).unwrap();
    out.lines()
        .filter_map(|l| {
            let mut f = l.split(' ');
            let (k, w, p) = (f.next()?, f.next()?, f.next()?);
            (!k.is_empty()).then(|| (k.to_string(), (w.parse().unwrap(), p.to_string())))
        })
        .collect()
}

/// `@zygo-pane` → `#{pane_start_command}` for each pane of `zygo-app`.
fn start_commands(srv: &PrivateServer) -> BTreeMap<String, String> {
    let out = srv.run(&["list-panes", "-s", "-t", "=zygo-app:", "-F", "#{@zygo-pane} #{pane_start_command}"]).unwrap();
    out.lines().filter(|l| l.len() > 10).map(|l| (l[..9].to_string(), l[10..].to_string())).collect()
}

fn wait_for(limit: Duration, f: impl Fn() -> bool) -> bool {
    let t0 = Instant::now();
    loop {
        if f() {
            return true;
        }
        if t0.elapsed() >= limit {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A tmux client of `cols`×`rows` attached to `session`, on a pseudo-terminal (D25, M5).
fn pty_client(srv: &PrivateServer, session: &str, cols: u16, rows: u16) -> Child {
    let (mut master, mut slave) = (0, 0);
    let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: both out pointers are valid; name and termios are null, the size is a valid struct.
    let rc = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &ws) };
    assert_eq!(rc, 0, "openpty");
    // SAFETY: openpty gave two open descriptors that nothing else owns.
    let (master, slave) = unsafe { (fs::File::from_raw_fd(master), fs::File::from_raw_fd(slave)) };
    let mut c = Command::new(srv.program());
    c.arg("-S").arg(srv.socket()).args(["-f", "/dev/null", "attach-session", "-t", &format!("={session}:")]);
    c.env_remove("TMUX").env("TERM", "xterm-256color");
    c.stdin(Stdio::from(slave.try_clone().unwrap())).stdout(Stdio::from(slave.try_clone().unwrap())).stderr(Stdio::from(slave));
    // SAFETY: setsid and ioctl are async-signal-safe; fd 0 is the slave after the redirections.
    unsafe {
        c.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = c.spawn().unwrap();
    let mut m = master;
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while matches!(m.read(&mut buf), Ok(n) if n > 0) {}
    });
    child
}

// ---- tests without a tmux server (steps 2-6) ----

#[test] // W§18 #65, W§8.1 step 2
fn preflight_tmux_version() {
    let _g = serial();
    let t = TempDir::new().unwrap();
    let r = repo(t.path(), "app");
    let old = version_tmux(t.path(), "tmux31", "tmux 3.1");
    let o = opts(t.path(), &r, &old, &stand_in(t.path(), &t.path().join("s"), "0"));
    let msg = "zygo: tmux 3.1 is too old; workspaces need tmux 3.2 or later";
    assert_eq!(run(&o), (2, String::new(), format!("{msg}\n")));
    assert_eq!(preflight(&o.paths, &old, None, &r), Err(Exit { code: 2, message: msg.into() }));
    assert_eq!(
        preflight(&o.paths, &t.path().join("none"), None, &r),
        Err(Exit { code: 2, message: "zygo: workspaces need tmux; install tmux 3.2 or later".into() })
    );
    let odd = version_tmux(t.path(), "tmuxm", "tmux master");
    assert!(matches!(preflight(&o.paths, &odd, None, &r), Err(Exit { code: 2, message }) if message.starts_with("zygo: cannot read the tmux version: ")));
    assert!(!o.paths.workspaces_dir().exists());
}

#[test] // W§18 #3, #11, #29, AU2, AU20, AU25, AU77, D1, D4, D5
fn preflight_finds_the_workspace() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = &canon(td.path());
    let p = paths(t);
    let ok = version_tmux(t, "tmux34", "tmux 3.4");
    let plain = t.join("plain");
    fs::create_dir_all(&plain).unwrap();
    let app = repo(&t.join("x"), "app");
    Store::open(&p, OTHER_ID)
        .update(S5, &mut |ws| {
            ws.name = "app".into();
            ws.tmux_session = "zygo-app".into();
            ws.common_dir = "/srv/app/.git".into();
            ws.main_root = "/srv/app".into();
            ws.loadout = "solo".into();
            Ok(())
        })
        .unwrap();
    assert_eq!(preflight(&p, &ok, None, &plain), Err(Exit { code: 2, message: "zygo up needs a git repository".into() })); // #29
    repo(t, "w");
    git(t, &["clone", "-q", "--bare", "w", "repo.git"]);
    assert_eq!(
        preflight(&p, &ok, None, &t.join("repo.git")),
        Err(Exit { code: 2, message: "zygo up needs a worktree; this repository is bare".into() })
    ); // #3, AU77
    git(&t.join("repo.git"), &["worktree", "add", "-q", "../main", "main"]);
    let (main, bare) = (canon(&t.join("main")), canon(&t.join("repo.git")));
    for cwd in [&main, &bare] {
        let tg = preflight(&p, &ok, None, cwd).unwrap();
        assert_eq!(
            (tg.id.clone(), tg.name.clone(), tg.main_root.clone(), tg.common_dir.clone()),
            (workspace_id(&bare), "main".into(), main.clone(), bare.clone()),
            "{cwd:?}"
        );
        assert_eq!(tg.state, Ok(None));
    }
    assert_eq!(preflight(&p, &ok, None, &app).unwrap().name, "app-2"); // AU2
    assert_eq!(preflight(&p, &ok, Some("app"), &plain).unwrap().id, OTHER_ID); // AU20
    assert_eq!(preflight(&p, &ok, Some("nosuch"), &plain), Err(Exit { code: 1, message: "zygo: no workspace named nosuch".into() }));
    // #11: a newer state is refused and stays unchanged
    let store = Store::open(&p, &workspace_id(&canon(&app.join(".git"))));
    let mut v = doc(&store, &app);
    v["version"] = json!(2);
    put_raw(&store, &v);
    let before = raw(&store);
    assert_eq!(
        preflight(&p, &ok, None, &app),
        Err(Exit { code: 1, message: "zygo: workspace state is from a newer zygo; run zygo update".into() })
    );
    assert_eq!(raw(&store), before);
    // D1: a state that fails AU84 is given to the caller
    let mut v = doc(&store, &app);
    v["name"] = json!("a b");
    put_raw(&store, &v);
    let tg = preflight(&p, &ok, None, &app).unwrap();
    let Err(text) = &tg.state else { panic!("{:?}", tg.state) };
    assert!(text.contains("workspace.json") && text.contains("invalid name"), "{text}");
    assert_eq!(tg.name, "app-2");
}

#[test] // W§18 #11, #15, AU54, AU83, D1, D28
fn refused_before_anything_changes() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let r = repo(t, "app");
    let g = gone_tmux(t);
    let mut o = opts(t, &r, &g, &t.join("zygo"));
    let store = state_of(&o, &r);
    let fresh = || {
        let _ = fs::remove_file(t.join("tmux.log"));
    };
    for z in ["zy'go", "zy$go"] {
        fresh();
        o.zygo = t.join(z);
        assert_eq!(run(&o), (2, String::new(), format!("{AU54}\n")), "{z}");
        assert!(!state_file(&store).exists());
        assert_eq!(tmux_log(t), ["-u -V"]);
    }
    o.zygo = t.join("zygo");
    // #11, D1
    let mut v = doc(&store, &r);
    v["version"] = json!(2);
    put_raw(&store, &v);
    let before = raw(&store);
    let newer = (1, String::new(), "zygo: workspace state is from a newer zygo; run zygo update\n".to_string());
    fresh();
    assert_eq!(run(&o), newer);
    assert_eq!(raw(&store), before);
    o.reset = true;
    assert_eq!(run(&o), newer);
    assert_eq!(raw(&store), before);
    o.reset = false;
    // W§10, D28
    let mut v = doc(&store, &r);
    v["name"] = json!("a b");
    put_raw(&store, &v);
    let before = raw(&store);
    let (code, out, err) = run(&o);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.starts_with("zygo: ") && err.contains("workspace.json"), "{err}");
    assert_eq!(raw(&store), before);
    assert!(!store.dir().join("workspace.json.bad").exists());
    // AU83
    fs::remove_file(state_file(&store)).unwrap();
    let held = store.op_lock().unwrap();
    fresh();
    assert_eq!(run(&o), (1, String::new(), "zygo: up or down is in progress for app\n".to_string()));
    assert!(!state_file(&store).exists());
    assert_eq!(tmux_log(t), ["-u -V"]);
    drop(held);
    // an unknown loadout
    o.loadout = Some("nosuch".into());
    fresh();
    assert_eq!(run(&o), (2, String::new(), "zygo: unknown loadout `nosuch`\n".to_string()));
    assert!(!state_file(&store).exists());
    assert!(tmux_log(t).iter().all(|l| !l.contains("new-session")));
}

#[test] // D31, AU96
fn plan_values_are_checked_before_the_write() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let r = repo(t, "app");
    let g = gone_tmux(t);
    let mut o = opts(t, &r, &g, &t.join("zygo"));
    let store = state_of(&o, &r);
    let cases: [(&str, Window); 4] = [
        ("zygo: window 0: name \"a\\nb\" refused: window name holds a control character\n", win(0, Some("a\nb"), "tiled", 200, 50)),
        ("zygo: window 0: size 0x50 is outside 1..=10000\n", win(0, None, "tiled", 0, 50)),
        ("zygo: window 0: size 10001x50 is outside 1..=10000\n", win(0, None, "tiled", 10001, 50)),
        ("zygo: window 0: size 200x10001 is outside 1..=10000\n", win(0, None, "tiled", 200, 10001)),
    ];
    for (line, w) in cases {
        saved(&store, &r, vec![w], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &r)], vec![]);
        let before = raw(&store);
        let _ = fs::remove_file(t.join("tmux.log"));
        assert_eq!(run(&o), (1, String::new(), line.to_string()));
        assert_eq!(raw(&store), before);
        assert!(tmux_log(t).iter().all(|l| !l.contains("new-session")));
    }
    fs::remove_file(state_file(&store)).unwrap();
    o.config = with_loadout("bad", vec![lw(Some("x\u{1b}y"), None, vec![lp("shell")])]);
    o.loadout = Some("bad".into());
    assert_eq!(
        run(&o),
        (1, String::new(), "zygo: window 0: name \"x\\u{1b}y\" refused: window name holds a control character\n".to_string())
    );
    assert!(!state_file(&store).exists());
    assert!(tmux_log(t).iter().all(|l| !l.contains("new-session")));
}

#[test] // AU62, W§18 #34 part, D17
fn saved_socket_must_be_private() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = &canon(td.path());
    let r = repo(t, "app");
    let g = gone_tmux(t);
    let o = opts(t, &r, &g, &t.join("zygo"));
    let store = state_of(&o, &r);
    let open = t.join("open");
    fs::create_dir_all(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
    saved(&store, &r, vec![win(0, None, "tiled", 200, 50)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &r)], vec![]);
    set(&store, |ws| ws.tmux_socket = Some(open.join("s")));
    let before = raw(&store);
    let (code, out, err) = run(&o);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.contains(&open.join("s").display().to_string()) && err.contains("not private"), "{err}");
    assert_eq!(raw(&store), before);
    assert_eq!(tmux_log(t), ["-u -V"]);
    set(&store, |ws| ws.tmux_socket = Some(t.join("new/sub/s")));
    let (code, _, err) = run(&o);
    assert_eq!(code, 1);
    assert!(err.ends_with("zygo: tmux: no server running on /tmp/zygo-none/s\n"), "{err}");
    assert_eq!(fs::metadata(t.join("new/sub")).unwrap().permissions().mode() & 0o777, 0o700);
}

// ---- live tests (steps 7-24) ----

#[test] // W§18 #13, AU68, W-C3, W§7.4 triggers
fn first_up_builds_the_loadout() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    l.srv.run(&["new-session", "-d", "-s", "base", "sleep", "600"]).unwrap();
    // A global `destroy-unattached on` ends every unattached session at once, `base` too; `exit-empty off` keeps the
    // server (probe, tmux 3.2a and 3.7c; W7 I2).
    l.srv.run(&["set-option", "-g", "exit-empty", "off"]).unwrap();
    l.srv.run(&["set-option", "-g", "destroy-unattached", "on"]).unwrap();
    l.srv.run(&["set-option", "-gw", "remain-on-exit", "on"]).unwrap();
    let g0 = globals(&l.srv);
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    let (code, out, err) = run(&l.o);
    assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");
    let ws = read(&l.store());
    assert_eq!((ws.phase, ws.down_all, ws.loadout.as_str()), (Phase::Up, false, "pair"));
    assert_eq!(ws.windows.iter().map(|w| w.name.clone()).collect::<Vec<_>>(), [Some("agents".to_string()), Some("logs".to_string())]);
    assert_eq!(ws.panes.iter().map(|p| p.kind).collect::<Vec<_>>(), [PaneKind::ClaudeCode, PaneKind::Shell, PaneKind::Shell]);
    let m = &ws.members[&ws.panes[0].key];
    assert_eq!((ws.members.len(), m.state, m.agent_session.clone(), m.launch.kind), (1, MemberState::Running, None, Agent::ClaudeCode));
    let w0 = format!("=zygo-app:{}", ws.windows[0].index);
    let lay = l.srv.run(&["display-message", "-p", "-t", &w0, "#{window_layout}"]).unwrap();
    l.srv.run(&["select-layout", "-t", &w0, "main-vertical"]).unwrap(); // main-vertical again changes nothing
    assert_eq!(l.srv.run(&["display-message", "-p", "-t", &w0, "#{window_layout}"]).unwrap(), lay);
    assert_eq!(l.srv.run(&["show-environment", "-t", "=zygo-app:", "ZYGO_WORKSPACE"]).unwrap(), format!("ZYGO_WORKSPACE={}", l.id));
    assert_eq!(l.srv.run(&["show-environment", "-t", "=zygo-app:", "ZYGO_SESSION"]).unwrap(), "-ZYGO_SESSION");
    assert_eq!(l.srv.run(&["show-options", "-v", "-t", "=zygo-app:", "destroy-unattached"]).unwrap(), "off");
    for w in &ws.windows {
        assert_eq!(l.srv.run(&["show-options", "-wv", "-t", &format!("=zygo-app:{}", w.index), "remain-on-exit"]).unwrap(), "off");
    }
    assert_eq!(globals(&l.srv), g0); // also: global destroy-unattached and remain-on-exit still on
    let s = l.srv.run(&["show-hooks", "-t", "=zygo-app:"]).unwrap();
    assert!(!s.contains("after-select-pane") && !s.contains("after-select-window"));
    for h in ["after-new-window", "after-split-window", "client-detached", "after-kill-pane"] {
        assert!(s.contains(h), "{h}: {s}");
    }
    let v = l.srv.tmux().version().unwrap();
    for w in &ws.windows {
        let wh = l.srv.run(&["show-hooks", "-w", "-t", &format!("=zygo-app:{}", w.index)]).unwrap();
        for h in ["window-layout-changed", "window-renamed", "pane-exited"] {
            assert!(wh.contains(h), "{h}: {wh}");
        }
        if v < (3, 3) {
            assert!(wh.contains("window-unlinked"), "{wh}");
        }
    }
    if v >= (3, 3) {
        assert!(s.contains("window-unlinked"), "{s}");
    }
}

#[test] // W§18 #14, AU48, AU15, AU46, D8
fn panes_are_written_before_ws_run_reads_them() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0.5");
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    let t0 = Instant::now();
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    let ws = read(&l.store());
    assert!(wait_for(S5 * 2, || ws.panes.iter().all(|p| l.runs.join(format!("{}.json", p.key)).exists())));
    for p in &ws.panes {
        let copy: Workspace = serde_json::from_slice(&fs::read(l.runs.join(format!("{}.json", p.key))).unwrap()).unwrap();
        assert_eq!(copy.phase, Phase::Building);
        assert!(copy.panes.iter().any(|q| q.key == p.key));
    }
    assert_eq!(ws.panes[1].cwd, l.wt); // the branch: cwd survived the final snapshot (D8)
    assert!(t0.elapsed() >= Duration::from_millis(500));
    let sock = l.srv.run(&["display-message", "-p", "-t", "=zygo-app:", "#{socket_path}"]).unwrap();
    assert_eq!(ws.tmux_socket.as_deref(), Some(Path::new(&sock)));
    assert!(Path::new(&sock).is_absolute());
    let starts = start_commands(&l.srv);
    assert_eq!(starts.len(), 3, "{starts:?}");
    for p in &ws.panes {
        let c = &starts[&p.key];
        assert!(c.contains(&format!("ws run --ws-id {} --pane {}", l.id, p.key)), "{c}");
        assert_eq!(c.ends_with("--shell"), p.kind == PaneKind::Shell, "{c}");
    }
    // AU55: a split that the user makes in an attached client opens in `#{session_path}`, the `-c` of new-session. A
    // split from a command-line client opens in that client's own folder (probe, tmux 3.7c; W7 I4), so the test reads
    // the session path.
    assert_eq!(l.srv.run(&["display-message", "-p", "-t", "=zygo-app:", "#{session_path}"]).unwrap(), l.repo.display().to_string());
}

#[test] // W§18 #15, AU54
fn hook_commands_are_literal() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let l = live("0");
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    let ws = read(&l.store());
    let want = format!("'{}' ws snapshot --ws-id {} --event ", l.o.zygo.display(), l.id);
    let mut lists = vec![l.srv.run(&["show-hooks", "-t", "=zygo-app:"]).unwrap()];
    for w in &ws.windows {
        lists.push(l.srv.run(&["show-hooks", "-w", "-t", &format!("=zygo-app:{}", w.index)]).unwrap());
    }
    let mut events = std::collections::BTreeSet::new();
    for line in lists.iter().flat_map(|s| s.lines()).filter(|l| !l.trim().is_empty()) {
        assert!(line.contains(&want) && !line.contains("#{@"), "{line}");
        let e = line.split("--event ").nth(1).unwrap().trim_end_matches('"');
        events.insert(e.to_string());
    }
    let eight = ["after-kill-pane", "after-new-window", "after-split-window", "client-detached", "pane-exited", "window-layout-changed",
        "window-renamed", "window-unlinked"];
    assert_eq!(events.into_iter().collect::<Vec<_>>(), eight);
}

#[test] // W§18 #15, AU54 (the socket check after the server exists)
fn socket_with_a_dollar_kills_the_new_session() {
    let _g = serial();
    let Some(real) = require_tmux() else { return };
    let mut l = live("0");
    let folder = tempfile::Builder::new().prefix("zygo-w7$").tempdir_in("/tmp").unwrap();
    fs::set_permissions(folder.path(), fs::Permissions::from_mode(0o700)).unwrap();
    l.o.tmux = script(l.t.path(), "tmuxf", &format!("exec '{}' -f /dev/null \"$@\"", real.display()));
    l.o.tmux_env = Some(OsString::from(format!("{}/s,1,0", folder.path().display())));
    let (code, out, err) = run(&l.o);
    assert_eq!((code, out.as_str(), err.as_str()), (2, "", format!("{AU54}\n").as_str()));
    let ws = read(&l.store());
    assert_eq!((ws.phase, ws.tmux_socket), (Phase::Down, None));
    let sock = folder.path().join("s");
    let has = Command::new(&real).arg("-S").arg(&sock).args(["has-session", "-t", "=zygo-app:"]).output().unwrap();
    assert!(!has.status.success());
    let _ = Command::new(&real).arg("-S").arg(&sock).arg("kill-server").output();
}

#[test] // W§18 #3, AU77
fn bare_repository_is_named_after_its_worktree() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let t = canon(l.t.path());
    git(&t, &["clone", "-q", "--bare", "app", "repo.git"]);
    git(&t.join("repo.git"), &["worktree", "add", "-q", "../main", "main"]);
    let main = canon(&t.join("main"));
    l.o.cwd = main.clone();
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    let ws = read(&Store::open(&l.o.paths, &workspace_id(&canon(&t.join("repo.git")))));
    assert_eq!((ws.name.as_str(), ws.main_root.clone(), ws.panes[0].cwd.clone()), ("main", main.clone(), main));
    l.srv.run(&["has-session", "-t", "=zygo-main:"]).unwrap();
}

#[test] // W§18 #29, AU37, D22
fn reset_and_running() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    saved(
        &store,
        &l.repo,
        vec![win(0, None, "tiled", 200, 50)],
        vec![pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, &l.repo)],
        vec![
            ("kaaaaaaaa", member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1"))),
            ("kbbbbbbbb", member(Agent::ClaudeCode, MemberState::Resumable, Some("c-2"))),
            ("kcccccccc", member(Agent::ClaudeCode, MemberState::Ended, Some("c-3"))),
        ],
    );
    l.o.reset = true;
    let (code, out, err) = run(&l.o);
    assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");
    assert!(err.contains("zygo: --reset drops claude-code agent session c-1 (pane kaaaaaaaa)\n"), "{err}");
    assert!(err.contains("zygo: --reset drops claude-code agent session c-2 (pane kbbbbbbbb)\n"), "{err}");
    assert!(!err.contains("c-3"), "{err}");
    let ws = read(&store);
    assert_eq!((ws.loadout.as_str(), ws.panes.len(), ws.members.len()), ("solo", 1, 1));
    assert!(["kaaaaaaaa", "kbbbbbbbb", "kcccccccc"].iter().all(|k| !ws.members.contains_key(*k)));
    let before = raw(&store);
    let running = (1, String::new(), "zygo: app is running; run zygo down -- app first\n".to_string());
    assert_eq!(run(&l.o), running);
    l.o.reset = false;
    l.o.loadout = Some("solo".into());
    assert_eq!(run(&l.o), running);
    assert_eq!(raw(&store), before);
}

#[test] // W§18 #35, AU63
fn session_of_another_workspace_is_not_touched() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let l = live("0");
    let store = l.store();
    saved(&store, &l.repo, vec![win(0, None, "tiled", 200, 50)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &l.repo)], vec![]);
    let sock = l.srv.socket().to_path_buf();
    set(&store, |ws| {
        ws.tmux_socket = Some(sock.clone());
        ws.phase = Phase::Up;
    });
    l.srv.run(&["new-session", "-d", "-s", "zygo-app", "sleep", "600"]).unwrap();
    l.srv.run(&["set-option", "-t", "=zygo-app:", "@zygo-ws", OTHER_ID]).unwrap();
    let before = raw(&store);
    let msg = (1, String::new(), "zygo: tmux session zygo-app does not belong to this workspace; rename it or run zygo ws forget -- app\n".to_string());
    assert_eq!(run(&l.o), msg);
    assert_eq!(attach_run(&l.o, None, None), msg);
    assert_eq!(l.srv.run(&["show-options", "-qv", "-t", "=zygo-app:", "@zygo-ws"]).unwrap(), OTHER_ID);
    assert_eq!(l.srv.run(&["list-panes", "-s", "-t", "=zygo-app:", "-F", "#{pane_id}"]).unwrap().lines().count(), 1);
    assert_eq!(raw(&store), before);
    l.srv.run(&["set-option", "-u", "-t", "=zygo-app:", "@zygo-ws"]).unwrap();
    assert_eq!(run(&l.o), msg);
    assert_eq!(raw(&store), before);
}

#[test] // AU37, AU63, W§12 `zygo attach`, FIX-16: each hint gives the name after `--`, so a name that starts with `-` works
fn dash_name_hints_pass_the_name_after_double_dash() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    saved(&store, &l.repo, vec![win(0, None, "tiled", 200, 50)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &l.repo)], vec![]);
    let sock = l.srv.socket().to_path_buf();
    set(&store, |ws| {
        ws.name = "-x".into();
        ws.tmux_session = "zygo--x".into();
        ws.tmux_socket = Some(sock.clone());
        ws.phase = Phase::Up;
    });
    l.srv.run(&["new-session", "-d", "-s", "zygo--x", "sleep", "600"]).unwrap();
    let before = raw(&store);
    // AU63: a session of another workspace.
    l.srv.run(&["set-option", "-t", "=zygo--x:", "@zygo-ws", OTHER_ID]).unwrap();
    let owner = (1, String::new(), "zygo: tmux session zygo--x does not belong to this workspace; rename it or run zygo ws forget -- -x\n".to_string());
    assert_eq!(run(&l.o), owner);
    assert_eq!(attach_run(&l.o, None, None), owner);
    assert_eq!(raw(&store), before);
    // AU37: the workspace's own session runs.
    l.srv.run(&["set-option", "-t", "=zygo--x:", "@zygo-ws", l.id.as_str()]).unwrap();
    let running = (1, String::new(), "zygo: -x is running; run zygo down -- -x first\n".to_string());
    l.o.reset = true;
    assert_eq!(run(&l.o), running);
    l.o.reset = false;
    l.o.loadout = Some("solo".into());
    assert_eq!(run(&l.o), running);
    assert_eq!(raw(&store), before);
    // W§12 `zygo attach`: a stopped workspace with no saved socket.
    l.srv.run(&["kill-session", "-t", "=zygo--x:"]).unwrap();
    set(&store, |ws| {
        ws.phase = Phase::Down;
        ws.tmux_socket = None;
    });
    assert_eq!(attach_run(&l.o, None, None), (1, String::new(), "zygo: -x is down; run zygo up -- -x\n".to_string()));
}

#[test] // W§10, D28, W§18 #11 part 2, AU78
fn unreadable_state_reset_and_extra_fields() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    let t = canon(l.t.path());
    let open = t.join("open");
    fs::create_dir_all(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
    let mut v = doc(&store, &l.repo);
    v["name"] = json!("a b");
    v["phase"] = json!("building");
    v["tmux_socket"] = json!(open.join("s"));
    put_raw(&store, &v);
    let old = raw(&store);
    l.o.reset = true;
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("not private") && !err.contains("does not belong"), "{err}");
    assert_eq!(fs::read(store.dir().join("workspace.json.bad")).unwrap(), old);
    let ws = read(&store);
    assert_eq!((ws.name.as_str(), ws.loadout.as_str(), ws.tmux_socket.clone()), ("app", "solo", Some(l.srv.socket().to_path_buf())));
    drop(l);
    // (b) AU78 in a new folder
    let mut l = live("0");
    let store = l.store();
    saved(&store, &l.repo, vec![win(0, None, "tiled", 200, 50)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &l.repo)], vec![]);
    let mut v: Value = serde_json::from_slice(&raw(&store)).unwrap();
    v["x_future"] = json!(7);
    v["panes"][0]["glow"] = json!(true);
    put_raw(&store, &v);
    l.o.reset = false;
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_slice(&raw(&store)).unwrap();
    assert_eq!((v["x_future"].clone(), v["panes"][0]["glow"].clone()), (json!(7), json!(true)));
}

#[test] // W§8.1 steps 7 and 12, AU55, AU70, AU30, W§7.5 rule 3, W§10, D6, D15, D23
fn restore_first_window_name_resumes_and_reports() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let l = live("0");
    let store = l.store();
    let mut w0 = win(0, Some("#(touch pwned) a"), "even-horizontal", 200, 50);
    w0.active = false;
    let mut w1 = win(1, None, "tiled", 200, 50);
    w1.active = true;
    w1.zoomed = true;
    let mut pd = pane("kdddddddd", 1, 1, PaneKind::OpenCode, &l.repo);
    pd.active = true;
    let mut ma = member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1"));
    ma.launch.args = vec!["--dangerously-skip-permissions".into()];
    ma.launch.yolo = true;
    ma.dropped_flags = vec!["--agents".into()];
    saved(
        &store,
        &l.repo,
        vec![w0, w1],
        vec![
            pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, &l.repo),
            pane("kbbbbbbbb", 0, 1, PaneKind::ClaudeCode, &l.repo),
            pane("kcccccccc", 1, 0, PaneKind::Shell, &l.repo),
            pd,
        ],
        vec![
            ("kaaaaaaaa", ma),
            ("kbbbbbbbb", member(Agent::ClaudeCode, MemberState::Running, None)),
            ("kdddddddd", member(Agent::OpenCode, MemberState::Ended, Some("o-1"))),
            ("keeeeeeee", member(Agent::ClaudeCode, MemberState::Resumable, Some("c-5"))),
        ],
    );
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    for line in [
        "zygo: pane kaaaaaaaa: claude-code agent session c-1 resumes with --dangerously-skip-permissions, yolo = true\n",
        "zygo: pane kaaaaaaaa: --agents had an inline value; not restored\n",
        "zygo: pane kbbbbbbbb: no agent session id saved; started a shell\n",
        "zygo: claude-code agent session c-5 is detached (its pane was closed); resume it by hand or run zygo ws forget-pane keeeeeeee\n",
    ] {
        assert!(err.contains(line), "{err}");
    }
    let ws = read(&store);
    // Round 2 M4: the member of a pane restored as a shell (no agent session id) is deleted
    assert!(ws.members["kaaaaaaaa"].note_pending && !ws.members.contains_key("kbbbbbbbb"));
    assert_eq!(ws.windows.iter().map(|w| w.name.clone()).collect::<Vec<_>>(), [Some("#(touch pwned) a".to_string()), None]);
    assert!(!l.repo.join("pwned").exists() && !l.o.paths.data_dir.join("pwned").exists());
    let starts = start_commands(&l.srv);
    for (k, c) in &starts {
        assert_eq!(!c.ends_with("--shell"), k == "kaaaaaaaa", "{k}: {c}");
    }
    let keys = by_key(&l.srv);
    let (wd, pdid) = keys["kdddddddd"].clone();
    assert_eq!(l.srv.run(&["display-message", "-p", "-t", "=zygo-app:", "#{window_index}"]).unwrap(), wd.to_string());
    let wt = format!("=zygo-app:{wd}");
    assert_eq!(l.srv.run(&["display-message", "-p", "-t", &wt, "#{window_zoomed_flag}"]).unwrap(), "1");
    assert_eq!(l.srv.run(&["display-message", "-p", "-t", &wt, "#{pane_id}"]).unwrap(), pdid);
    assert_eq!(ws.phase, Phase::Up);
    assert!(ws.snapshot_seq >= 2, "{}", ws.snapshot_seq);
}

#[test] // W§18 #79, AU96
fn first_window_keeps_its_saved_size_and_follows_a_client() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let l = live("0");
    let store = l.store();
    l.srv.run(&["new-session", "-d", "-s", "scratch", "-x", "173", "-y", "47", "sleep", "600"]).unwrap();
    l.srv.run(&["resize-window", "-t", "=scratch:0", "-x", "173", "-y", "47"]).unwrap();
    for _ in 0..2 {
        l.srv.run(&["split-window", "-d", "-t", "=scratch:0", "sleep", "600"]).unwrap();
    }
    l.srv.run(&["select-layout", "-t", "=scratch:0", "main-vertical"]).unwrap();
    let lay = l.srv.run(&["display-message", "-p", "-t", "=scratch:0", "#{window_layout}"]).unwrap();
    let sizes = l.srv.run(&["list-panes", "-t", "=scratch:0", "-F", "#{pane_width}x#{pane_height}"]).unwrap();
    l.srv.run(&["kill-session", "-t", "=scratch:"]).unwrap();
    let mut w0 = win(0, None, &lay, 173, 47);
    w0.active = true;
    saved(
        &store,
        &l.repo,
        vec![w0],
        ["kaaaaaaaa", "kbbbbbbbb", "kcccccccc"].iter().enumerate().map(|(i, k)| pane(k, 0, i as u32, PaneKind::Shell, &l.repo)).collect(),
        vec![],
    );
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    let w = format!("=zygo-app:{}", read(&store).windows[0].index);
    assert_eq!(l.srv.run(&["display-message", "-p", "-t", &w, "#{window_width}x#{window_height}"]).unwrap(), "173x47");
    assert_eq!(l.srv.run(&["list-panes", "-t", &w, "-F", "#{pane_width}x#{pane_height}"]).unwrap(), sizes);
    let global = l.srv.run(&["show-options", "-gwv", "window-size"]).unwrap();
    assert_ne!(global, "manual");
    assert_eq!(l.srv.run(&["display-message", "-p", "-t", &w, "#{window-size}"]).unwrap(), global);
    let mut c = pty_client(&l.srv, "zygo-app", 100, 30);
    assert!(wait_for(S5 * 2, || l.srv.run(&["display-message", "-p", "-t", &w, "#{window_width}x#{window_height}"]).unwrap() == "100x29"));
    c.kill().unwrap();
    c.wait().unwrap();
}

#[test] // W§18 #38, AU96, D10, D13
fn splits_that_do_not_fit() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let keys: Vec<String> = "abcdefghijkl".chars().map(|c| format!("k{}", c.to_string().repeat(8))).collect();
    for case in ['A', 'B'] {
        let l = live("0");
        let store = l.store();
        let (w, h, n) = if case == 'A' { (80, 24, 8) } else { (10, 5, 12) };
        let mut members = vec![];
        let panes: Vec<Pane> = keys[..n]
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let agent = case == 'A' && (i == 1 || i == 2);
                pane(k, 0, i as u32, if agent { PaneKind::ClaudeCode } else { PaneKind::Shell }, &l.repo)
            })
            .collect();
        if case == 'A' {
            members.push((keys[1].as_str(), member(Agent::ClaudeCode, MemberState::Resumable, Some("s-1"))));
            members.push((keys[2].as_str(), member(Agent::ClaudeCode, MemberState::Resumable, Some("s-2"))));
        } else {
            members.push((keys[11].as_str(), member(Agent::ClaudeCode, MemberState::Resumable, Some("s-11"))));
        }
        saved(&store, &l.repo, vec![win(0, None, "even-vertical", w, h)], panes, members);
        let (code, _, err) = run(&l.o);
        assert_eq!(code, 0, "{case}: {err}");
        let ws = read(&store);
        let live_keys = by_key(&l.srv);
        let mut reported_any = false;
        for k in &keys[..n] {
            let built = ws.panes.iter().any(|p| &p.key == k) && live_keys.contains_key(k);
            let reported = err.contains(&format!("zygo: pane {k} not built (no space)"));
            assert!(built != reported, "{case} {k}: {err}");
            reported_any |= reported;
            if reported && let Some(m) = ws.members.get(k) {
                assert_eq!(m.state, MemberState::Resumable);
            }
        }
        assert_eq!(ws.phase, Phase::Up);
        if case == 'B' {
            assert!(reported_any, "{err}");
            if err.contains("zygo: pane kllllllll not built (no space)") {
                assert!(err.contains("zygo: pane kllllllll not built (no space); agent session s-11 is detached\n"), "{err}");
            }
        }
    }
}

#[test] // D30, M2
fn fresh_member_whose_split_failed_has_no_record() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    l.o.config = with_loadout("duo", vec![lw(None, None, vec![lp("shell"), lp("claude-code")])]);
    l.o.loadout = Some("duo".into());
    fs::create_dir_all(store.dir()).unwrap();
    fail_split(store.dir(), 1);
    let (code, _, err) = run(&l.o);
    up::testing::hook(store.dir(), "split", None);
    assert_eq!(code, 0, "{err}");
    let ws = read(&store);
    assert_eq!(ws.panes.len(), 1);
    assert!(ws.members.is_empty());
    let shell = &ws.panes[0].key;
    assert!(wait_for(S5, || l.runs.join(format!("{shell}.json")).exists()));
    let copy: Workspace = serde_json::from_slice(&fs::read(l.runs.join(format!("{shell}.json"))).unwrap()).unwrap();
    let k = copy.panes.iter().map(|p| p.key.clone()).find(|k| !ws.panes.iter().any(|p| &p.key == k)).unwrap();
    assert!(err.contains(&format!("zygo: pane {k} not built (no space)\n")), "{err}");
    l.srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
    l.o.loadout = None;
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("no agent session id saved"), "{err}");
}

#[test] // D29, B3, AU98
fn loadout_server_with_user_info_stays_a_v1_pane() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let agent = LoadoutPane { server: Some("https://u:p@h".into()), ..lp("claude-code") };
    l.o.config = with_loadout("u", vec![lw(None, None, vec![agent, lp("shell")])]);
    l.o.loadout = Some("u".into());
    let (code, out, err) = run(&l.o);
    assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");
    assert_eq!(err.matches("zygo: --server holds user info; this agent is not saved by the workspace\n").count(), 1, "{err}");
    let ws = read(&l.store());
    assert!(ws.members.is_empty());
    assert!(ws.panes.iter().all(|p| p.kind == PaneKind::Shell));
    let starts = start_commands(&l.srv);
    assert_eq!(starts.len(), 2);
    assert!(starts.values().all(|c| c.ends_with("--shell")), "{starts:?}");
}

#[test] // D32, M4, AU30
fn loadout_argument_paths_use_the_pane_folder() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    fs::write(l.wt.join("x.json"), "{}").unwrap();
    assert!(!Path::new("x.json").exists());
    let agent = LoadoutPane { cwd: Some("branch:feature-x".into()), args: vec!["--agents".into(), "./x.json".into()], ..lp("claude-code") };
    l.o.config = with_loadout("a", vec![lw(None, None, vec![agent])]);
    l.o.loadout = Some("a".into());
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    let ws = read(&l.store());
    let m = ws.members.values().next().unwrap();
    let want = std::path::absolute(l.wt.join("./x.json")).unwrap().to_str().unwrap().to_string();
    assert_eq!(m.launch.args, ["--agents".to_string(), want]);
    assert!(m.dropped_flags.is_empty());
}

#[test] // W§18 #39, AU95, AU100, AU64 (D19), W§12
fn attach_paths() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let sock = l.srv.socket().display().to_string();
    let calls = l.t.path().join("calls.log");
    l.o.tmux = script(l.t.path(), "ltmux", &format!("printf '%s\\n' \"$*\" >> '{}'\nexec '{}' \"$@\"", calls.display(), l.srv.wrapper().display()));
    l.o.no_attach = false;
    l.o.zygo_session = Some("01X".into());
    let (code, out, err) = run(&l.o);
    assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");
    assert!(err.contains("zygo: started a new tmux server from inside a zygo agent; its panes have no provider keys from the environment\n"), "{err}");
    assert_eq!(l.srv.run(&["show-environment", "-t", "=zygo-app:", "ANTHROPIC_BASE_URL"]).unwrap(), "-ANTHROPIC_BASE_URL");
    l.o.zygo_session = None;
    l.o.tmux_env = Some("/tmp/zygo-other/s,1,0".into());
    let other = (0, format!("zygo: app runs on another tmux server; attach with: tmux -S '{sock}' attach -t =zygo-app\n"), String::new());
    assert_eq!(run(&l.o), other);
    assert_eq!(attach_run(&l.o, Some("/tmp/zygo-other/s,1,0"), None), other);
    assert!(!fs::read_to_string(&calls).unwrap().contains("switch-client"));
    l.o.tmux_env = Some(format!("{sock},1,0").into());
    let (code, out, err) = run(&l.o);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.starts_with("zygo: tmux: "), "{err}");
    assert!(fs::read_to_string(&calls).unwrap().contains("switch-client -t =zygo-app:"));
    l.srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
    assert_eq!(attach_run(&l.o, None, None), (1, String::new(), "zygo: app is down; run zygo up -- app\n".to_string()));
}

#[test] // W§18 #46, AU83, D33
fn op_lock_held_at_the_wait_is_retried() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let l = live("0");
    let store = l.store();
    let lock = store.op_lock().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        rx.recv().unwrap();
        drop(lock);
    });
    let held = Arc::new(AtomicBool::new(false));
    let count = Arc::new(AtomicUsize::new(0));
    let (h, c, s) = (held.clone(), count.clone(), store.clone());
    let tx = Mutex::new(tx);
    up::testing::hook(
        store.dir(),
        "before_op_lock",
        Some(Box::new(move || {
            h.store(!s.op_lock_free(), Ordering::SeqCst);
            c.fetch_add(1, Ordering::SeqCst);
            let _ = tx.lock().unwrap().send(());
            Ok(())
        })),
    );
    let (code, _, err) = run(&l.o);
    up::testing::hook(store.dir(), "before_op_lock", None);
    holder.join().unwrap();
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("in progress"), "{err}");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(held.load(Ordering::SeqCst));
}

#[test] // D34, M10
fn pending_final_snapshot_uses_the_written_socket() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let l = live("0");
    let store = l.store();
    saved(&store, &l.repo, vec![win(0, None, "tiled", 200, 50)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &l.repo)], vec![]);
    let sock = l.srv.socket().to_path_buf();
    set(&store, |ws| {
        ws.tmux_socket = Some(sock.clone());
        ws.phase = Phase::Up;
    });
    let held: Arc<Mutex<Option<fs::File>>> = Arc::new(Mutex::new(None));
    let (h1, h2) = (held.clone(), held.clone());
    let lock_path = store.dir().join("lock");
    up::testing::hook(
        store.dir(),
        "before_final_snapshot",
        Some(Box::new(move || {
            let f = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock_path).unwrap();
            fs4::FileExt::try_lock(&f).unwrap();
            *h1.lock().unwrap() = Some(f);
            Ok(())
        })),
    );
    up::testing::hook(
        store.dir(),
        "after_final_snapshot",
        Some(Box::new(move || {
            h2.lock().unwrap().take();
            Ok(())
        })),
    );
    let (code, _, err) = run(&l.o);
    up::testing::hook(store.dir(), "before_final_snapshot", None);
    up::testing::hook(store.dir(), "after_final_snapshot", None);
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("final snapshot not saved"), "{err}");
    assert!(!store.dir().join("snapshot.pending").exists());
    let ws = read(&store);
    assert_eq!(ws.snapshot_seq, 2);
    assert!(ws.snapshot_at.is_some());
}

// ---- repair (step 24) ----

/// `up` with `pair`, then the mux of the written state.
fn up_pair(l: &mut Live) -> Tmux {
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    l.o.loadout = None;
    let ws = read(&l.store());
    up::server(&l.o.tmux, ws.tmux_socket.as_deref(), None)
}

fn second_agents_pane(l: &Live) -> String {
    let ws = read(&l.store());
    ws.panes.iter().filter(|p| p.window == ws.windows[0].index).nth(1).unwrap().key.clone()
}

#[test] // AU67, W§8.1 step 5, D20
fn repair_from_building() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let mux = up_pair(&mut l);
    let store = l.store();
    let ws = read(&store);
    let keys: Vec<String> = ws.panes.iter().map(|p| p.key.clone()).collect();
    let second = second_agents_pane(&l);
    let logs_key = ws.panes[2].key.clone();
    let seq = ws.snapshot_seq;
    l.srv.run(&["kill-pane", "-t", &by_key(&l.srv)[&second].1]).unwrap();
    l.srv.run(&["kill-window", "-t", &format!("=zygo-app:{}", ws.windows[1].index)]).unwrap();
    set(&store, |w| w.phase = Phase::Building);
    let (r, _, err) = repair_run(&store, &mux, &l.o.zygo);
    r.unwrap();
    let now = by_key(&l.srv);
    assert!(keys.iter().all(|k| now.contains_key(k)), "{now:?} {err}");
    let logs_w = now[&logs_key].0;
    assert_eq!(l.srv.run(&["display-message", "-p", "-t", &format!("=zygo-app:{logs_w}"), "#{window_name}"]).unwrap(), "logs");
    let ws = read(&store);
    assert_eq!((ws.phase, ws.snapshot_seq), (Phase::Up, seq));
    l.srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
    set(&store, |w| w.phase = Phase::Building);
    let panes = read(&store).panes;
    repair_run(&store, &mux, &l.o.zygo).0.unwrap();
    let ws = read(&store);
    assert_eq!((ws.phase, ws.panes), (Phase::Down, panes));
}

#[test] // B1, D12, D27
fn repair_after_a_failure_after_new_session() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    fs::create_dir_all(store.dir()).unwrap();
    up::testing::hook(store.dir(), "after_new_session", Some(Box::new(|| Err("injected".into()))));
    let r1 = run(&l.o);
    up::testing::hook(store.dir(), "after_new_session", None);
    assert_eq!((r1.0, r1.2.as_str()), (1, "zygo: tmux: injected\n"));
    let ws = read(&store);
    assert_eq!(ws.phase, Phase::Building);
    assert_eq!(remain(&l, 0), "on"); // the build hold that the stopped up left
    assert_eq!(l.srv.run(&["show-options", "-qv", "-t", "=zygo-app:", "@zygo-ws"]).unwrap(), l.id);
    assert!(by_key(&l.srv).contains_key(&ws.panes[0].key));
    l.o.loadout = None;
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    let now = by_key(&l.srv);
    assert!(ws.panes.iter().all(|p| now.contains_key(&p.key)), "{now:?}");
    assert_eq!(read(&store).phase, Phase::Up);
    // W7 I3: the dead `up` wrote no `tmux_socket`; repair records it, so later snapshots run
    assert_eq!(ws.tmux_socket, None);
    assert_eq!(read(&store).tmux_socket.as_deref(), Some(l.srv.socket()));
    assert_eq!(l.srv.run(&["show-environment", "-t", "=zygo-app:", "ZYGO_WORKSPACE"]).unwrap(), format!("ZYGO_WORKSPACE={}", l.id));
    let hooks = l.srv.run(&["show-hooks", "-t", "=zygo-app:"]).unwrap();
    // Each line names its hook twice (`after-new-window[0] … --event after-new-window`): count the entries.
    assert_eq!(hooks.lines().filter(|l| l.starts_with("after-new-window[")).count(), 1, "{hooks}");
    for w in by_key(&l.srv).values().map(|(w, _)| *w).collect::<BTreeSet<u32>>() {
        assert_eq!(remain(&l, w), "off", "window {w}"); // AU67 repair releases every window
    }
}

#[test] // M1, M7, AU96
fn repair_removes_panes_that_do_not_fit() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let mux = up_pair(&mut l);
    let store = l.store();
    let second = second_agents_pane(&l);
    let seq = read(&store).snapshot_seq;
    l.srv.run(&["kill-pane", "-t", &by_key(&l.srv)[&second].1]).unwrap();
    set(&store, |w| w.phase = Phase::Building);
    fail_split(store.dir(), 1);
    let (r, _, err) = repair_run(&store, &mux, &l.o.zygo);
    up::testing::hook(store.dir(), "split", None);
    r.unwrap();
    assert!(err.contains(&format!("zygo: pane {second} not built (no space)\n")), "{err}");
    let ws = read(&store);
    assert!(!ws.panes.iter().any(|p| p.key == second));
    assert_eq!(ws.snapshot_seq, seq + 1);
}

#[test] // W§8.1 step 5
fn repair_from_draining() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let mux = up_pair(&mut l);
    let store = l.store();
    set(&store, |w| w.phase = Phase::Draining);
    repair_run(&store, &mux, &l.o.zygo).0.unwrap();
    assert_eq!(read(&store).phase, Phase::Up);
    l.srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
    set(&store, |w| w.phase = Phase::Draining);
    repair_run(&store, &mux, &l.o.zygo).0.unwrap();
    assert_eq!(read(&store).phase, Phase::Down);
}

/// A live launcher session `sessions/<name>` (as `tests/state.rs`): a token and a held lock on `pid`.
fn live_session(p: &Paths, name: &str) -> fs::File {
    let dir = p.sessions_dir.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("token"), "t").unwrap();
    let f = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("pid")).unwrap();
    fs4::FileExt::try_lock(&f).unwrap();
    f
}

#[test] // AU75, W§8.1 step 5
fn repair_from_stopping() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let mux = up_pair(&mut l);
    let store = l.store();
    // session there, no live launcher: the stop is completed
    set(&store, |w| w.phase = Phase::Stopping);
    repair_run(&store, &mux, &l.o.zygo).0.unwrap();
    assert!(l.srv.run(&["has-session", "-t", "=zygo-app:"]).is_err());
    assert_eq!(read(&store).phase, Phase::Down);
    // gone
    set(&store, |w| w.phase = Phase::Stopping);
    repair_run(&store, &mux, &l.o.zygo).0.unwrap();
    assert_eq!(read(&store).phase, Phase::Down);
    // session there and a live member
    let mux = up_pair(&mut l);
    let _live = live_session(&l.o.paths, "01LIVE");
    let agent = read(&store).panes[0].key.clone();
    let mut z = MemberRecord::new(Agent::ClaudeCode);
    z.clean_stop = true;
    z.closed_units = vec!["u2".into()];
    z.saved_for = Some("01OLD".into());
    let (a2, z2) = (agent.clone(), z.clone());
    set(&store, move |w| {
        w.phase = Phase::Stopping;
        // the restore made this pane a shell and deleted its record (Round 2 M4); the live member is written here
        let m = w.members.entry(a2.clone()).or_insert_with(|| MemberRecord::new(Agent::ClaudeCode));
        m.zygo_session = Some("01LIVE".into());
        m.clean_stop = true;
        m.closed_units = vec!["u1".into()];
        m.interrupted = vec![Subagent { agent_id: "a".into(), agent_type: "t".into(), ..Subagent::default() }];
        m.saved_for = Some("01LIVE".into());
        w.members.insert("kzzzzzzzz".into(), z2.clone());
    });
    repair_run(&store, &mux, &l.o.zygo).0.unwrap();
    let ws = read(&store);
    assert_eq!(ws.phase, Phase::Up);
    let m = &ws.members[&agent];
    assert!(!m.clean_stop && m.closed_units.is_empty() && m.interrupted.is_empty());
    assert_eq!(m.saved_for, None);
    assert_eq!(ws.members["kzzzzzzzz"], z);
    l.srv.run(&["has-session", "-t", "=zygo-app:"]).unwrap();
}

#[test] // AU87, AU63, B1, D20
fn repair_refusals() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let mux = up_pair(&mut l);
    let store = l.store();
    let agent = read(&store).panes[0].key.clone();
    // Building, the session gone, a live member
    l.srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
    let live_file = live_session(&l.o.paths, "01LIVE");
    let a2 = agent.clone();
    set(&store, move |w| {
        w.phase = Phase::Building;
        w.members.get_mut(&a2).unwrap().zygo_session = Some("01LIVE".into());
    });
    let e = repair_run(&store, &mux, &l.o.zygo).0.unwrap_err();
    assert!(format!("{e:#}").contains("has live agents (pane "), "{e:#}");
    assert_eq!(read(&store).phase, Phase::Building);
    drop(live_file);
    // a session without our `@zygo-ws` is never adopted
    l.srv.run(&["new-session", "-d", "-s", "zygo-app", "sleep", "600"]).unwrap();
    for phase in [Phase::Building, Phase::Stopping] {
        for value in [None, Some(OTHER_ID)] {
            match value {
                None => l.srv.run(&["set-option", "-u", "-t", "=zygo-app:", "@zygo-ws"]).map(|_| ()).unwrap(),
                Some(v) => l.srv.run(&["set-option", "-t", "=zygo-app:", "@zygo-ws", v]).map(|_| ()).unwrap(),
            }
            set(&store, |w| w.phase = phase);
            let e = repair_run(&store, &mux, &l.o.zygo).0.unwrap_err();
            assert!(format!("{e:#}").contains("does not belong"), "{e:#}");
            assert_eq!(read(&store).phase, phase);
            assert_eq!(l.srv.run(&["list-panes", "-s", "-t", "=zygo-app:", "-F", "#{pane_id}"]).unwrap().lines().count(), 1);
        }
    }
    // phases up and down: no tmux call
    let stub = script(l.t.path(), "logtmux", &format!("echo \"$*\" >> '{}'", l.t.path().join("stub.log").display()));
    let quiet = Tmux::new(stub, None);
    for phase in [Phase::Up, Phase::Down] {
        set(&store, |w| w.phase = phase);
        let before = raw(&store);
        repair_run(&store, &quiet, &l.o.zygo).0.unwrap();
        assert_eq!(raw(&store), before);
    }
    assert!(!l.t.path().join("stub.log").exists());
}

// ---- Round 2 ----

/// A tmux wrapper that runs the real tmux only for a call with `-S` (or `-V`); every other call gets "no server", so
/// no call can reach the default tmux server.
fn socket_only_tmux(dir: &Path, real: &Path) -> PathBuf {
    let body = format!(
        "for a in \"$@\"; do case \"$a\" in -S|-V) exec '{}' -f /dev/null \"$@\";; esac; done\n\
         echo 'no server running on /tmp/zygo-none/s' >&2\nexit 1",
        real.display()
    );
    script(dir, "stmux", &body)
}

fn agent_was_built(l: &Live, ws: &Workspace) {
    assert_eq!(ws.panes.iter().map(|p| p.kind).collect::<Vec<_>>(), [PaneKind::ClaudeCode, PaneKind::Shell, PaneKind::Shell]);
    assert_eq!(ws.members[&ws.panes[0].key].state, MemberState::Running);
    let starts = start_commands(&l.srv);
    assert!(!starts[&ws.panes[0].key].ends_with("--shell"), "{starts:?}");
}

#[test] // Round 2 B1: a failed first `up` leaves no plan behind
fn failed_first_up_leaves_no_plan_and_the_next_up_builds_the_loadout() {
    let _g = serial();
    let Some(real) = require_tmux() else { return };
    let mut l = live("0");
    let folder = tempfile::Builder::new().prefix("zygo-w7$").tempdir_in("/tmp").unwrap();
    fs::set_permissions(folder.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let wrapper = l.o.tmux.clone();
    l.o.tmux = script(l.t.path(), "tmuxf", &format!("exec '{}' -f /dev/null \"$@\"", real.display()));
    l.o.tmux_env = Some(OsString::from(format!("{}/s,1,0", folder.path().display())));
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    let (code, _, err) = run(&l.o);
    let _ = Command::new(&real).arg("-S").arg(folder.path().join("s")).arg("kill-server").output();
    assert_eq!(code, 2, "{err}");
    let ws = read(&l.store());
    assert_eq!((ws.phase, ws.tmux_socket.clone()), (Phase::Down, None));
    assert!(ws.windows.is_empty() && ws.panes.is_empty() && ws.members.is_empty(), "{ws:?}");
    l.o.tmux = wrapper;
    l.o.tmux_env = None;
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("no agent session id saved") && !err.contains("--loadout applies only"), "{err}");
    agent_was_built(&l, &read(&l.store()));
}

#[test] // Round 2 B1: repair from `building` to `down` when no build ever finished
fn crash_in_building_without_a_snapshot_rebuilds_the_loadout() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    fs::create_dir_all(store.dir()).unwrap();
    up::testing::hook(store.dir(), "after_new_session", Some(Box::new(|| Err("injected".into()))));
    let first = run(&l.o);
    up::testing::hook(store.dir(), "after_new_session", None);
    assert_eq!(first.0, 1, "{}", first.2);
    l.srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
    let ws = read(&store);
    assert_eq!((ws.phase, ws.snapshot_at, ws.panes.len()), (Phase::Building, None, 3));
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("no agent session id saved") && !err.contains("--loadout applies only"), "{err}");
    agent_was_built(&l, &read(&store));
}

#[test] // Round 2 M1: a value that ends with `;` would end a tmux command
fn trailing_semicolon_is_refused_before_the_write() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = &canon(td.path());
    let g = gone_tmux(t);
    let refused = |o: &UpOpts, repo: &Path, value: &str| {
        let _ = fs::remove_file(t.join("tmux.log"));
        let (code, out, err) = run(o);
        assert_eq!((code, out.as_str()), (1, ""), "{err}");
        assert!(err.contains(&format!("{value:?}")) && err.contains("ends with `;`"), "{value}: {err}");
        assert!(!state_file(&state_of(o, repo)).exists());
        assert!(tmux_log(t).iter().all(|l| !l.contains("new-session")));
    };
    // a path: the main worktree, the `-c` of new-session and new-window
    let r = repo(t, "app;");
    refused(&opts(t, &r, &g, &t.join("zygo")), &r, &r.display().to_string());
    // an argument: the zygo path is the first item of each pane's argument list
    let r = repo(t, "app2");
    let z = t.join("zygo;");
    let mut o = opts(t, &r, &g, &z);
    refused(&o, &r, &z.display().to_string());
    // window names
    o.zygo = t.join("zygo");
    for name in ["a;", ";"] {
        o.config = with_loadout("w", vec![lw(Some(name), None, vec![lp("shell")])]);
        o.loadout = Some("w".into());
        refused(&o, &r, name);
    }
}

#[test] // Round 2 M2: the attach path after a repair from `building` takes the final snapshot
fn building_repair_then_attach_path_takes_a_snapshot() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let _ = up_pair(&mut l);
    let store = l.store();
    let ws = read(&store);
    let logs = format!("=zygo-app:{}", ws.windows[1].index);
    l.srv.run(&["kill-window", "-t", &logs]).unwrap();
    l.srv.run(&["new-window", "-d", "-t", &logs, "sleep", "600"]).unwrap(); // the old index is taken
    set(&store, |w| w.phase = Phase::Building);
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    let ws = read(&store);
    assert_eq!(ws.phase, Phase::Up);
    let tmux_windows: std::collections::BTreeSet<u32> = l
        .srv
        .run(&["list-windows", "-t", "=zygo-app:", "-F", "#{window_index}"])
        .unwrap()
        .lines()
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(ws.windows.iter().map(|w| w.index).collect::<std::collections::BTreeSet<_>>(), tmux_windows);
    let keys = by_key(&l.srv);
    for p in &ws.panes {
        assert_eq!(keys[&p.key].0, p.window, "{}", p.key);
    }
}

#[test] // Round 2 M3: the server of `$TMUX` is in `tmux_socket` before `new_session`
fn the_server_of_tmux_env_is_written_before_new_session() {
    let _g = serial();
    let Some(real) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    l.o.tmux = socket_only_tmux(l.t.path(), &real);
    l.o.tmux_env = Some(OsString::from(format!("{},1,0", l.srv.socket().display())));
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    fs::create_dir_all(store.dir()).unwrap();
    up::testing::hook(store.dir(), "after_new_session", Some(Box::new(|| Err("injected".into()))));
    let first = run(&l.o);
    up::testing::hook(store.dir(), "after_new_session", None);
    assert_eq!((first.0, first.2.as_str()), (1, "zygo: tmux: injected\n"));
    assert_eq!(read(&store).tmux_socket.as_deref(), Some(l.srv.socket()));
    // the next `up`, outside tmux, repairs on the same server
    l.o.tmux_env = None;
    l.o.loadout = None;
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    let ws = read(&store);
    assert_eq!((ws.phase, ws.tmux_socket.as_deref()), (Phase::Up, Some(l.srv.socket())));
    let keys = by_key(&l.srv);
    assert!(ws.panes.iter().all(|p| keys.contains_key(&p.key)), "{keys:?}");
}

#[test] // Round 2 M4: a pane restored as a shell has no member, so its kind stays shell
fn a_pane_restored_as_a_shell_loses_its_member_and_stays_a_shell() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let l = live("0");
    let store = l.store();
    saved(
        &store,
        &l.repo,
        vec![win(0, None, "tiled", 200, 50)],
        vec![pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, &l.repo)],
        vec![("kaaaaaaaa", member(Agent::ClaudeCode, MemberState::Running, None))],
    );
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("zygo: pane kaaaaaaaa: no agent session id saved; started a shell\n"), "{err}");
    let ws = read(&store);
    assert!(!ws.members.contains_key("kaaaaaaaa"));
    assert_eq!(ws.panes[0].kind, PaneKind::Shell);
    let mux = Tmux::new(l.o.tmux.clone(), ws.tmux_socket.clone());
    let r = zygo_workspace::snapshot::run(&l.o.paths, &mux, &l.id, None, true, zygo_workspace::snapshot::OP_WAIT).unwrap();
    assert_eq!(r, zygo_workspace::snapshot::SnapshotOutcome::Written);
    assert_eq!(read(&store).panes[0].kind, PaneKind::Shell);
    l.srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("no agent session id saved"), "{err}");
}

#[test] // Round 2 M5: `up <name>` with a newer or an invalid state
fn preflight_by_name_refuses_a_newer_or_invalid_state() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = &canon(td.path());
    let p = paths(t);
    let ok = version_tmux(t, "tmux34", "tmux 3.4");
    let plain = t.join("plain");
    fs::create_dir_all(&plain).unwrap();
    let r = repo(t, "app");
    // Round 3 R3-5: the folder id is the hash of `common_dir`
    let id = workspace_id(&canon(&r.join(".git")));
    let store = Store::open(&p, &id);
    let mut v = doc(&store, &r);
    v["version"] = json!(2);
    put_raw(&store, &v);
    assert_eq!(
        preflight(&p, &ok, Some("app"), &plain),
        Err(Exit { code: 1, message: "zygo: workspace state is from a newer zygo; run zygo update".into() })
    );
    let mut v = doc(&store, &r);
    v["tmux_session"] = json!("zygo-other");
    put_raw(&store, &v);
    let tg = preflight(&p, &ok, Some("app"), &plain).unwrap();
    assert_eq!((tg.id.as_str(), tg.name.as_str(), tg.main_root.clone()), (id.as_str(), "app", r.clone()));
    let Err(text) = &tg.state else { panic!("{:?}", tg.state) };
    assert!(text.contains("workspace.json") && text.contains("invalid tmux_session"), "{text}");
    // `up app` names the file and exits 1; with `--reset` the file becomes `workspace.json.bad`
    let g = gone_tmux(t);
    let mut o = opts(t, &plain, &g, &t.join("zygo"));
    o.name = Some("app".into());
    let before = raw(&store);
    let (code, out, err) = run(&o);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(err.starts_with("zygo: ") && err.contains("workspace.json"), "{err}");
    assert_eq!(raw(&store), before);
    assert!(!store.dir().join("workspace.json.bad").exists());
    o.reset = true;
    let (code, _, err) = run(&o);
    assert_eq!(fs::read(store.dir().join("workspace.json.bad")).unwrap(), before);
    assert_eq!(code, 1);
    assert!(err.ends_with("zygo: tmux: no server running on /tmp/zygo-none/s\n"), "{err}");
}

#[test] // FIX-17 D8, AU84: the name claim reads at most 1 MiB + 1 byte; a larger invalid state does not match by name
fn preflight_by_name_skips_an_invalid_state_over_1_mib() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = &canon(td.path());
    let p = paths(t);
    let ok = version_tmux(t, "tmux34", "tmux 3.4");
    let plain = t.join("plain");
    fs::create_dir_all(&plain).unwrap();
    let r = repo(t, "app");
    let store = Store::open(&p, &workspace_id(&canon(&r.join(".git"))));
    // An invalid state that claims the name `app` by the rules of Round 3 R3-5, padded with spaces to `len` bytes.
    let put_padded = |len: usize| {
        let mut v = doc(&store, &r);
        v["tmux_session"] = json!("zygo-other");
        let mut b = serde_json::to_vec_pretty(&v).unwrap();
        b.resize(len, b' ');
        fs::create_dir_all(store.dir()).unwrap();
        fs::write(state_file(&store), &b).unwrap();
        b
    };
    // Exactly 1 MiB: still read, so it claims the name (the boundary).
    put_padded(1024 * 1024);
    let tg = preflight(&p, &ok, Some("app"), &plain).unwrap();
    assert!(matches!(&tg.state, Err(text) if text.contains("invalid tmux_session")), "{:?}", tg.state);
    // 2 MiB: its name is never read, so `up app` finds no workspace and changes nothing.
    let before = put_padded(2 * 1024 * 1024);
    assert_eq!(preflight(&p, &ok, Some("app"), &plain), Err(Exit { code: 1, message: "zygo: no workspace named app".into() }));
    let g = gone_tmux(t);
    let mut o = opts(t, &plain, &g, &t.join("zygo"));
    o.name = Some("app".into());
    assert_eq!(run(&o), (1, String::new(), "zygo: no workspace named app\n".to_string()));
    assert!(raw(&store) == before, "up changed the state file");
}

#[test] // FIX-17 Round 2 F1, AU29, AU84: a state file over 1 MiB may be from a newer zygo: `up` and `up --reset` refuse
        // it with exit 1 and leave it in place (no `.bad` rename)
fn up_reset_refuses_a_state_file_over_1_mib() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = &canon(td.path());
    let r = repo(t, "app");
    let store = Store::open(&paths(t), &workspace_id(&canon(&r.join(".git"))));
    let mut bytes = serde_json::to_vec_pretty(&doc(&store, &r)).unwrap();
    bytes.resize(2 * 1024 * 1024, b' ');
    fs::create_dir_all(store.dir()).unwrap();
    fs::write(state_file(&store), &bytes).unwrap();
    let line = format!(
        "zygo: {}: state file too large; it may be from a newer zygo, so zygo does not change it\n",
        state_file(&store).display()
    );
    let g = gone_tmux(t);
    let mut o = opts(t, &r, &g, &t.join("zygo"));
    o.reset = true;
    assert_eq!(run(&o), (1, String::new(), line.clone()));
    assert!(raw(&store) == bytes, "up --reset changed the state file");
    assert!(!store.dir().join("workspace.json.bad").exists());
    o.reset = false;
    assert_eq!(run(&o), (1, String::new(), line));
    assert!(raw(&store) == bytes, "up changed the state file");
}

// ---- Round 3 ----

/// A tmux wrapper as `socket_only_tmux` whose `new-session` exits 1 with `injected`: `fail` makes no session, `make`
/// runs the real call first (the session is ours), `foreign` first makes a session `zygo-app` with no `@zygo-ws`.
fn failing_new_session(dir: &Path, real: &Path, mode: &str) -> PathBuf {
    let r = real.display();
    let body = format!(
        "sock=; prev=; for a in \"$@\"; do [ \"$prev\" = -S ] && sock=$a; prev=$a; done\n\
         case \"$*\" in *new-session*)\n\
         case {mode} in make) '{r}' -f /dev/null \"$@\" ;; foreign) '{r}' -f /dev/null -S \"$sock\" new-session -d -s zygo-app sleep 600 ;; esac\n\
         echo injected >&2; exit 1 ;;\n\
         esac\n\
         for a in \"$@\"; do case \"$a\" in -S|-V) exec '{r}' -f /dev/null \"$@\";; esac; done\n\
         echo 'no server running on /tmp/zygo-none/s' >&2\nexit 1"
    );
    script(dir, &format!("ftmux-{mode}"), &body)
}

/// `$TMUX` of a client of the private server.
fn inside(l: &Live) -> Option<OsString> { Some(OsString::from(format!("{},1,0", l.srv.socket().display()))) }

const AU67_OLDER: &str = "zygo: app: the state has no build plan (an older zygo wrote it); its layout and members stay";

#[test] // Round 3 R3-1: a failed `new-session` that left our session keeps `building`; the next `up` repairs it
fn new_session_error_with_our_session_keeps_building() {
    let _g = serial();
    let Some(real) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    l.o.tmux = failing_new_session(l.t.path(), &real, "make");
    l.o.tmux_env = inside(&l);
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("injected"), "{err}");
    let ws = read(&store);
    assert_eq!((ws.phase, ws.panes.len(), ws.tmux_socket.as_deref()), (Phase::Building, 3, Some(l.srv.socket())));
    assert_eq!((ws.build_plan, ws.fresh_members.len()), (Some(BuildPlan::Loadout), 1));
    assert_eq!(l.srv.run(&["show-options", "-qv", "-t", "=zygo-app:", "@zygo-ws"]).unwrap(), l.id);
    l.o.tmux = socket_only_tmux(l.t.path(), &real);
    l.o.tmux_env = None;
    l.o.loadout = None;
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    let ws = read(&store);
    assert_eq!((ws.phase, ws.build_plan, ws.fresh_members.len()), (Phase::Up, None, 0));
    let keys = by_key(&l.srv);
    assert_eq!(keys.len(), 3, "{keys:?}");
    assert!(ws.panes.iter().all(|p| keys.contains_key(&p.key)), "{keys:?}");
    agent_was_built(&l, &ws);
}

#[test] // Round 3 R3-1: a failed `new-session` with no session undoes the plan; a session of another owner is AU63
fn new_session_error_without_our_session_undoes_the_plan() {
    let _g = serial();
    let Some(real) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    l.o.tmux_env = inside(&l);
    for mode in ["fail", "foreign"] {
        l.o.tmux = failing_new_session(l.t.path(), &real, mode);
        let (code, _, err) = run(&l.o);
        assert_eq!(code, 1, "{mode}: {err}");
        assert!(err.contains("injected"), "{mode}: {err}");
        let ws = read(&store);
        assert_eq!((ws.phase, ws.tmux_socket.clone(), ws.build_plan), (Phase::Down, None, None), "{mode}");
        assert!(ws.windows.is_empty() && ws.panes.is_empty() && ws.members.is_empty() && ws.fresh_members.is_empty(), "{ws:?}");
        if mode == "foreign" {
            assert!(err.contains("zygo: tmux session zygo-app does not belong to this workspace"), "{err}");
            assert_eq!(l.srv.run(&["list-panes", "-s", "-t", "=zygo-app:", "-F", "#{pane_id}"]).unwrap().lines().count(), 1);
            l.srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
        }
    }
    l.o.tmux = socket_only_tmux(l.t.path(), &real);
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    agent_was_built(&l, &read(&store));
}

#[test] // Round 3 R3-2: a crash in `building` after `--reset`, with `snapshot_at` set, rebuilds the loadout
fn crash_after_reset_with_a_snapshot_rebuilds_the_loadout() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let _ = up_pair(&mut l);
    let store = l.store();
    assert!(read(&store).snapshot_at.is_some());
    l.srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
    set(&store, |w| w.phase = Phase::Down);
    l.o.reset = true;
    l.o.loadout = Some("pair".into());
    up::testing::hook(store.dir(), "after_new_session", Some(Box::new(|| Err("injected".into()))));
    let first = run(&l.o);
    up::testing::hook(store.dir(), "after_new_session", None);
    assert_eq!(first.0, 1, "{}", first.2);
    let ws = read(&store);
    assert_eq!((ws.phase, ws.build_plan, ws.fresh_members.len()), (Phase::Building, Some(BuildPlan::Loadout), 1));
    assert!(ws.snapshot_at.is_some());
    l.srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
    l.o.reset = false;
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("no agent session id saved") && !err.contains("--loadout applies only"), "{err}");
    agent_was_built(&l, &read(&store));
}

#[test] // Round 3 R3-2: a restore that crashed in `building` keeps its layout, also when no snapshot was saved
fn restore_crash_keeps_the_layout() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let _ = up_pair(&mut l);
    let store = l.store();
    let agent = read(&store).panes[0].key.clone();
    set(&store, move |w| w.members.get_mut(&agent).unwrap().agent_session = Some("s-1".into()));
    // the second pass: the build finished with "final snapshot not saved", so `snapshot_at` is unset
    for snapshot_saved in [true, false] {
        let _ = l.srv.run(&["kill-session", "-t", "=zygo-app:"]);
        set(&store, |w| {
            w.phase = Phase::Down;
            if !snapshot_saved {
                w.snapshot_at = None;
            }
        });
        let before = read(&store);
        up::testing::hook(store.dir(), "after_new_session", Some(Box::new(|| Err("injected".into()))));
        let first = run(&l.o);
        up::testing::hook(store.dir(), "after_new_session", None);
        assert_eq!(first.0, 1, "{}", first.2);
        let mid = read(&store);
        assert_eq!((mid.phase, mid.build_plan, mid.fresh_members.len()), (Phase::Building, Some(BuildPlan::Restore), 0));
        l.srv.run(&["kill-session", "-t", "=zygo-app:"]).unwrap();
        let mux = up::server(&l.o.tmux, mid.tmux_socket.as_deref(), None);
        let (r, _, err) = repair_run(&store, &mux, &l.o.zygo);
        r.unwrap();
        let after = read(&store);
        assert_eq!((after.phase, after.build_plan), (Phase::Down, None), "{err}");
        assert_eq!((&after.windows, &after.panes), (&before.windows, &before.panes), "snapshot saved: {snapshot_saved}");
        let ids = |w: &Workspace| w.members.iter().map(|(k, m)| (k.clone(), m.agent_session.clone())).collect::<Vec<_>>();
        assert_eq!(ids(&after), ids(&before));
    }
}

#[test] // Round 3 R3-3, R3-4: the repair deletes only `fresh_members` with no agent session id; an older state keeps all
fn repair_deletes_only_fresh_members_without_an_id() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = &canon(td.path());
    let r = repo(t, "app");
    let g = gone_tmux(t);
    let o = opts(t, &r, &g, &t.join("zygo"));
    let store = state_of(&o, &r);
    let mut with_pending = member(Agent::ClaudeCode, MemberState::Running, None);
    with_pending.pending =
        Some(Pending { zygo_session: "01DEAD".into(), agent_session: Some("p-1".into()), started_at: 1, ..Pending::default() });
    let older = member(Agent::ClaudeCode, MemberState::Resumable, None);
    let panes = vec![
        pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, &r),
        pane("kbbbbbbbb", 0, 1, PaneKind::ClaudeCode, &r),
        pane("kcccccccc", 0, 2, PaneKind::Shell, &r),
    ];
    saved(
        &store,
        &r,
        vec![win(0, None, "tiled", 200, 50)],
        panes.clone(),
        vec![
            ("kaaaaaaaa", member(Agent::ClaudeCode, MemberState::Running, None)),
            ("kbbbbbbbb", with_pending.clone()),
            ("kdddddddd", older.clone()),
        ],
    );
    set(&store, |w| {
        w.phase = Phase::Building;
        w.build_plan = Some(BuildPlan::Loadout);
        w.fresh_members = vec!["kaaaaaaaa".into(), "kbbbbbbbb".into()];
    });
    let seq = read(&store).snapshot_seq;
    let quiet = Tmux::new(g.clone(), None);
    let (res, _, err) = repair_run(&store, &quiet, &o.zygo);
    res.unwrap();
    let ws = read(&store);
    assert_eq!((ws.phase, ws.build_plan, ws.snapshot_seq), (Phase::Down, None, seq + 1), "{err}");
    assert!(ws.windows.is_empty() && ws.panes.is_empty() && ws.fresh_members.is_empty(), "{ws:?}");
    assert_eq!(ws.members.keys().map(String::as_str).collect::<Vec<_>>(), ["kbbbbbbbb", "kdddddddd"]);
    assert_eq!((&ws.members["kbbbbbbbb"], &ws.members["kdddddddd"]), (&with_pending, &older));
    // a state of an older zygo (no build plan): everything stays, and one line says so
    set(&store, |w| {
        w.phase = Phase::Building;
        w.panes = panes.clone();
    });
    let before = read(&store);
    let (res, _, err) = repair_run(&store, &quiet, &o.zygo);
    res.unwrap();
    assert_eq!(err, format!("{AU67_OLDER}\n"));
    let ws = read(&store);
    assert_eq!((ws.phase, &ws.panes, &ws.members, ws.snapshot_seq), (Phase::Down, &before.panes, &before.members, before.snapshot_seq));
}

#[test] // Round 3 R3-4: a restore deletes only a bare member record; a failed restore puts back what it deleted
fn restore_forgets_only_bare_members_and_undo_puts_them_back() {
    let _g = serial();
    let Some(real) = require_tmux() else { return };
    let l = live("0");
    let store = l.store();
    let mut pend = member(Agent::ClaudeCode, MemberState::Running, None);
    pend.pending =
        Some(Pending { zygo_session: "01DEAD".into(), agent_session: Some("p-1".into()), started_at: 1, ..Pending::default() });
    let mut start = member(Agent::ClaudeCode, MemberState::Running, None);
    start.pending = Some(Pending { zygo_session: "01DEAD2".into(), agent_session: None, started_at: 2, ..Pending::default() });
    let mut bare = member(Agent::ClaudeCode, MemberState::Running, None);
    bare.launch.profile = Some("fast".into());
    bare.dropped_flags = vec!["--agents".into()];
    saved(
        &store,
        &l.repo,
        vec![win(0, None, "tiled", 200, 50)],
        vec![
            pane("kaaaaaaaa", 0, 0, PaneKind::ClaudeCode, &l.repo),
            pane("kbbbbbbbb", 0, 1, PaneKind::ClaudeCode, &l.repo),
            pane("kcccccccc", 0, 2, PaneKind::ClaudeCode, &l.repo),
        ],
        vec![("kaaaaaaaa", bare.clone()), ("kbbbbbbbb", pend.clone()), ("kcccccccc", start.clone())],
    );
    let sock = l.srv.socket().to_path_buf();
    set(&store, move |w| w.tmux_socket = Some(sock.clone()));
    // a restore whose new-session fails: the undo puts back the bare record with all its fields
    let mut o = l.o.clone();
    o.tmux = failing_new_session(l.t.path(), &real, "fail");
    let (code, _, err) = run(&o);
    assert_eq!(code, 1, "{err}");
    let ws = read(&store);
    assert_eq!((ws.phase, ws.build_plan, ws.panes.len()), (Phase::Down, None, 3));
    assert_eq!((&ws.members["kaaaaaaaa"], &ws.members["kbbbbbbbb"], &ws.members["kcccccccc"]), (&bare, &pend, &start));
    // the restore: only the bare record goes
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    let ws = read(&store);
    assert!(!ws.members.contains_key("kaaaaaaaa"), "{err}");
    assert_eq!((&ws.members["kbbbbbbbb"], &ws.members["kcccccccc"]), (&pend, &start));
}

#[test] // Round 3 R3-5: an invalid state matches a name only with a valid name, absolute paths and its own hash
fn preflight_by_name_matches_an_invalid_state_only_by_its_rules() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = &canon(td.path());
    let p = paths(t);
    let ok = version_tmux(t, "tmux34", "tmux 3.4");
    let plain = t.join("plain");
    fs::create_dir_all(&plain).unwrap();
    let (r, r2) = (repo(t, "app"), repo(t, "app2"));
    let store = Store::open(&p, &workspace_id(&canon(&r.join(".git"))));
    let s2 = Store::open(&p, &workspace_id(&canon(&r2.join(".git"))));
    let invalid = |store: &Store, repo: &Path| {
        let mut v = doc(store, repo);
        v["tmux_session"] = json!("zygo-other");
        v
    };
    let none = |name: &str| {
        let e = preflight(&p, &ok, Some(name), &plain).unwrap_err();
        assert_eq!(e.code, 1);
        assert!(e.message.starts_with("zygo: no workspace named "), "{name}: {}", e.message);
    };
    // missing, empty or relative paths
    for (k, value) in [("common_dir", None), ("main_root", None), ("common_dir", Some("")), ("main_root", Some("rel/app"))] {
        let mut v = invalid(&store, &r);
        match value {
            None => drop(v.as_object_mut().unwrap().remove(k)),
            Some(s) => v[k] = json!(s),
        }
        put_raw(&store, &v);
        none("app");
    }
    // a name that is not valid
    let mut v = invalid(&store, &r);
    v["name"] = json!("App!");
    put_raw(&store, &v);
    none("App!");
    // a folder that is not the hash of `common_dir`
    fs::remove_dir_all(store.dir()).unwrap();
    let other = Store::open(&p, OTHER_ID);
    put_raw(&other, &invalid(&other, &r));
    none("app");
    fs::remove_dir_all(other.dir()).unwrap();
    // two claimants of one name
    put_raw(&store, &invalid(&store, &r));
    put_raw(&s2, &invalid(&s2, &r2));
    let e = preflight(&p, &ok, Some("app"), &plain).unwrap_err();
    assert_eq!(e.code, 1);
    for s in [&store, &s2] {
        assert!(e.message.contains(&state_file(s).display().to_string()), "{}", e.message);
    }
}

#[test] // Round 3 R3-6: layouts are checked too, before every write of this run, also before the `.bad` rename
fn trailing_semicolon_in_a_layout_and_before_the_bad_rename() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = &canon(td.path());
    let g = gone_tmux(t);
    let r = repo(t, "app");
    let mut o = opts(t, &r, &g, &t.join("zygo"));
    o.config = with_loadout("w", vec![lw(None, Some("tiled;"), vec![lp("shell")])]);
    o.loadout = Some("w".into());
    let store = state_of(&o, &r);
    let refused = |o: &UpOpts, value: &str, tail: &str| {
        let (code, out, err) = run(o);
        assert_eq!((code, out.as_str()), (1, ""), "{err}");
        assert!(err.contains(&format!("{value:?}")) && err.ends_with(&format!("ends with `;`, which tmux reads as the end of a command; {tail}\n")), "{err}");
    };
    // a loadout layout
    refused(&o, "tiled;", "nothing was written");
    assert!(!state_file(&store).exists());
    // a saved layout
    saved(&store, &r, vec![win(0, None, "even;", 200, 50)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &r)], vec![]);
    o.loadout = None;
    let before = raw(&store);
    refused(&o, "even;", "nothing was written");
    assert_eq!(raw(&store), before);
    // `--reset` on a state that fails AU84: no `.bad` file, the state as it was
    let mut v = doc(&store, &r);
    v["tmux_session"] = json!("zygo-other");
    put_raw(&store, &v);
    let before = raw(&store);
    o.reset = true;
    o.loadout = Some("w".into());
    refused(&o, "tiled;", "nothing was written");
    assert!(!store.dir().join("workspace.json.bad").exists());
    assert_eq!(raw(&store), before);
    // after the step 5 repair wrote: "nothing of this plan was written"
    fs::remove_file(state_file(&store)).unwrap();
    o.reset = false;
    saved(&store, &r, vec![], vec![], vec![]);
    set(&store, |w| w.phase = Phase::Draining);
    refused(&o, "tiled;", "nothing of this plan was written");
    assert_eq!(read(&store).phase, Phase::Down);
}

#[test] // Round 3 R3-7: an undo whose write fails says so
fn failed_undo_write_is_reported() {
    let _g = serial();
    let Some(real) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    l.o.tmux = failing_new_session(l.t.path(), &real, "fail");
    l.o.tmux_env = inside(&l);
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    fs::create_dir_all(store.dir()).unwrap();
    let file = state_file(&store);
    up::testing::hook(store.dir(), "before_undo", Some(Box::new(move || fs::write(&file, b"{").map_err(|e| e.to_string()))));
    let (code, _, err) = run(&l.o);
    up::testing::hook(store.dir(), "before_undo", None);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("\nzygo: state not cleaned: ") || err.starts_with("zygo: state not cleaned: "), "{err}");
    assert_eq!(raw(&store), b"{");
}

#[test] // Round 3 R3-8: a socket that another writer set before step 8 stays after the undo
fn undo_keeps_a_socket_that_another_writer_set() {
    let _g = serial();
    let Some(real) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    saved(&store, &l.repo, vec![], vec![], vec![]);
    let folder = tempfile::Builder::new().prefix("zygo-w7r3").tempdir_in("/tmp").unwrap();
    fs::set_permissions(folder.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let x = folder.path().join("s");
    l.o.tmux = failing_new_session(l.t.path(), &real, "fail");
    l.o.tmux_env = inside(&l);
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    let (s2, x2) = (store.clone(), x.clone());
    up::testing::hook(
        store.dir(),
        "before_plan_write",
        Some(Box::new(move || {
            s2.update(S5, &mut |w| {
                w.tmux_socket = Some(x2.clone());
                Ok(())
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
        })),
    );
    let (code, _, err) = run(&l.o);
    up::testing::hook(store.dir(), "before_plan_write", None);
    assert_eq!(code, 1, "{err}");
    let ws = read(&store);
    assert_eq!((ws.phase, ws.tmux_socket.as_deref()), (Phase::Down, Some(x.as_path())), "{err}");
}

#[test] // M5 round 2 finding 1 (FIX-28 folded), W§8.1 step 9: the first pane starts with `ZYGO_WORKSPACE`; the server does not
fn first_pane_of_a_new_session_gets_zygo_workspace_in_session_scope_only() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    // A zygo stand-in whose `ws run` writes its process environment to `<envs>/<key>.env` (complete when it exists).
    let envs = l.t.path().join("envs");
    let body = format!(
        "if [ \"$1\" = ws ] && [ \"$2\" = run ]; then\nmkdir -p '{d}'\nenv > '{d}'/\"$6\".tmp\nmv '{d}'/\"$6\".tmp '{d}'/\"$6\".env\nexec sleep 600\nfi\nexit 0",
        d = envs.display()
    );
    l.o.zygo = script(l.t.path(), "zygo-env", &body);
    let (code, _, err) = run(&l.o);
    assert_eq!(code, 0, "{err}");
    let ws = read(&l.store());
    let first = envs.join(format!("{}.env", ws.panes[0].key));
    assert!(wait_for(S5 * 2, || first.exists()), "no environment from the first pane");
    let env = fs::read_to_string(&first).unwrap();
    assert!(env.lines().any(|x| x == format!("ZYGO_WORKSPACE={}", l.id)), "{env}");
    // This `up` started the server, which copied the client environment into its global environment.
    let global = l.srv.run(&["show-environment", "-g"]).unwrap();
    assert!(!global.lines().any(|x| x.starts_with("ZYGO_WORKSPACE")), "{global}");
}

// ---- FIX-29: panes that exit at once during the build ----

/// FIX-29: a zygo stand-in whose `ws run` runs `body` at once; every other call exits 0.
fn ws_run_does(l: &Live, name: &str, body: &str) -> PathBuf {
    script(l.t.path(), name, &format!("if [ \"$1\" = ws ] && [ \"$2\" = run ]; then {body}; fi\nexit 0"))
}

/// FIX-29: true when `zygo-app` has `n` panes and all are dead, or when the session or its server is gone (the
/// defect: then there is nothing to wait for).
fn all_dead(wrapper: &Path, n: usize) -> bool {
    let out = Command::new(wrapper).args(["list-panes", "-s", "-t", "=zygo-app:", "-F", "#{pane_dead}"]).output().unwrap();
    if !out.status.success() {
        return true;
    }
    let s = String::from_utf8_lossy(&out.stdout).into_owned();
    let v: Vec<&str> = s.lines().collect();
    v.len() == n && v.iter().all(|x| *x == "1")
}

type Hook = Box<dyn Fn() -> Result<(), String> + Send + Sync>;

/// FIX-29: hooks that make the panes exit before `up` goes on: at `after_new_session` the first pane, at
/// `before_final_snapshot` all `n` panes (each wait at most 5 s). `None` clears both.
fn wait_for_dead_panes(dir: &Path, wrapper: Option<&Path>, n: usize) {
    let (a, b) = match wrapper {
        Some(w) => {
            let (w1, w2) = (w.to_path_buf(), w.to_path_buf());
            let a: Hook = Box::new(move || {
                wait_for(S5, || all_dead(&w1, 1));
                Ok(())
            });
            let b: Hook = Box::new(move || {
                wait_for(S5, || all_dead(&w2, n));
                Ok(())
            });
            (Some(a), Some(b))
        }
        None => (None, None),
    };
    up::testing::hook(dir, "after_new_session", a);
    up::testing::hook(dir, "before_final_snapshot", b);
}

/// FIX-29: the line of a pane that exited while held; `how` is `exit <n>` or `no exit status`.
fn exited(key: &str, how: &str, socket: &Path, pane: &str) -> String {
    format!(
        "zygo: pane {key} exited during up ({how}); its dead pane stays: tmux -S '{}' respawn-pane -t {pane} starts it again\n",
        socket.display()
    )
}

/// FIX-29: `remain-on-exit` of window `w` of `zygo-app`.
fn remain(l: &Live, w: u32) -> String {
    l.srv.run(&["show-options", "-wv", "-t", &format!("=zygo-app:{w}"), "remain-on-exit"]).unwrap()
}

#[test] // FIX-29, W§8.1 steps 9-12, AU68, W-C3: panes whose command exits at once stay as dead panes; one line each
fn restored_panes_that_exit_at_once_stay_dead_and_are_reported() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    saved(
        &store,
        &l.repo.clone(),
        vec![win(0, Some("one"), "tiled", 120, 40), win(1, Some("two"), "tiled", 120, 40)],
        vec![
            pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &l.repo),
            pane("kbbbbbbbb", 0, 1, PaneKind::Shell, &l.wt),
            pane("kcccccccc", 1, 0, PaneKind::Shell, &l.wt),
        ],
        vec![],
    );
    l.o.zygo = ws_run_does(&l, "zygo-exits", "exit 3");
    l.srv.run(&["new-session", "-d", "-s", "base", "sleep", "600"]).unwrap();
    let g0 = globals(&l.srv);
    wait_for_dead_panes(store.dir(), Some(l.srv.wrapper()), 3);
    let t0 = Instant::now();
    let (code, out, err) = run(&l.o);
    let took = t0.elapsed();
    wait_for_dead_panes(store.dir(), None, 0);
    // The test bound proves no hang; the product has no time limit.
    assert!(took < Duration::from_secs(30), "up took {took:?}");
    assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");
    assert!(!err.contains("zygo: tmux:"), "{err}");
    let sock = read(&store).tmux_socket.unwrap();
    assert_eq!(sock, l.srv.socket());
    let ids = by_key(&l.srv);
    assert_eq!(ids.keys().cloned().collect::<Vec<_>>(), ["kaaaaaaaa", "kbbbbbbbb", "kcccccccc"]);
    for (k, (_, id)) in &ids {
        assert_eq!(err.matches(&exited(k, "exit 3", &sock, id)).count(), 1, "{err}");
    }
    assert_eq!(
        l.srv.run(&["list-panes", "-s", "-t", "=zygo-app:", "-F", "#{@zygo-pane} #{pane_dead} #{pane_dead_status}"]).unwrap(),
        "kaaaaaaaa 1 3\nkbbbbbbbb 1 3\nkcccccccc 1 3"
    );
    assert_eq!((remain(&l, 0), remain(&l, 1)), ("off".to_string(), "off".to_string()));
    assert_eq!(globals(&l.srv), g0);
    let ws = read(&store);
    assert_eq!((ws.phase, ws.down_all, ws.build_plan, ws.fresh_members.len()), (Phase::Up, false, None, 0));
    assert_eq!(ws.windows.iter().map(|w| w.name.clone()).collect::<Vec<_>>(), [Some("one".to_string()), Some("two".to_string())]);
    assert_eq!(
        ws.panes.iter().map(|p| (p.key.clone(), p.cwd.clone())).collect::<Vec<_>>(),
        [
            ("kaaaaaaaa".to_string(), l.repo.clone()),
            ("kbbbbbbbb".to_string(), l.wt.clone()),
            ("kcccccccc".to_string(), l.wt.clone())
        ]
    );
}

#[test] // FIX-29 reproduction (M5 implementer): up --all after a server death, panes whose command exits at once
fn up_all_after_a_server_death_reports_panes_that_exit_at_once() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    up_pair(&mut l);
    let store = l.store();
    let before = read(&store);
    let keys: Vec<String> = before.panes.iter().map(|p| p.key.clone()).collect();
    let cwds: Vec<PathBuf> = before.panes.iter().map(|p| p.cwd.clone()).collect();
    assert_eq!(keys.len(), 3);
    // There is no `base` session, so the server ends with the workspace session, as in the reproduction.
    l.srv.kill_server();
    l.o.zygo = ws_run_does(&l, "zygo-exits", "exit 3");
    wait_for_dead_panes(store.dir(), Some(l.srv.wrapper()), 3);
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let code = up::testing::run_all_with(&l.o, &mut o, &mut e);
    wait_for_dead_panes(store.dir(), None, 0);
    let (out, err) = (text(o), text(e));
    assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");
    assert!(!err.contains("zygo: tmux:") && !err.contains("skipped"), "{err}");
    let sock = read(&store).tmux_socket.unwrap();
    let ids = by_key(&l.srv);
    for k in &keys {
        assert_eq!(err.matches(&exited(k, "exit 3", &sock, &ids[k].1)).count(), 1, "{err}");
    }
    let ws = read(&store);
    assert_eq!(ws.phase, Phase::Up);
    assert_eq!(ws.panes.iter().map(|p| p.key.clone()).collect::<Vec<_>>(), keys);
    assert_eq!(ws.panes.iter().map(|p| p.cwd.clone()).collect::<Vec<_>>(), cwds);
}

#[test] // FIX-29 (review item 7b): a pane ended by a signal has no exit status
fn a_pane_ended_by_a_signal_is_reported_with_no_exit_status() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    saved(&store, &l.repo.clone(), vec![win(0, None, "tiled", 120, 40)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &l.repo)], vec![]);
    l.o.zygo = ws_run_does(&l, "zygo-killed", "kill -9 $$");
    wait_for_dead_panes(store.dir(), Some(l.srv.wrapper()), 1);
    let (code, out, err) = run(&l.o);
    wait_for_dead_panes(store.dir(), None, 0);
    assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");
    let sock = read(&store).tmux_socket.unwrap();
    let ids = by_key(&l.srv);
    assert_eq!(err.matches(&exited("kaaaaaaaa", "no exit status", &sock, &ids["kaaaaaaaa"].1)).count(), 1, "{err}");
}

#[test] // FIX-29 (review items 2, 7c): a release failure is one line; up goes on and keeps its exit code
fn a_failed_release_is_reported_and_up_goes_on() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    fs::create_dir_all(store.dir()).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    up::testing::hook(
        store.dir(),
        "release",
        Some(Box::new(move || if c.fetch_add(1, Ordering::SeqCst) == 0 { Err("injected".into()) } else { Ok(()) })),
    );
    l.o.config = pair();
    l.o.loadout = Some("pair".into());
    let (code, out, err) = run(&l.o);
    up::testing::hook(store.dir(), "release", None);
    assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");
    // Both windows were tried: there was no early return.
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(err.matches("zygo: app: could not release the build hold of window 0: injected\n").count(), 1, "{err}");
    assert_eq!((remain(&l, 0), remain(&l, 1)), ("on".to_string(), "off".to_string()));
    assert_eq!(read(&store).phase, Phase::Up);
}

#[test] // FIX-29 (review item 8), AU67: a repair prints the line of a pane that died during the hold
fn repair_reports_a_pane_that_died_during_the_hold() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let mut l = live("0");
    let store = l.store();
    let normal = l.o.zygo.clone();
    saved(
        &store,
        &l.repo.clone(),
        vec![win(0, Some("one"), "tiled", 120, 40)],
        vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &l.repo), pane("kbbbbbbbb", 0, 1, PaneKind::Shell, &l.wt)],
        vec![],
    );
    l.o.zygo = ws_run_does(&l, "zygo-exits", "exit 3");
    let w = l.srv.wrapper().to_path_buf();
    up::testing::hook(
        store.dir(),
        "after_new_session",
        Some(Box::new(move || {
            wait_for(S5, || all_dead(&w, 1));
            Err("injected".into())
        })),
    );
    let r1 = run(&l.o);
    up::testing::hook(store.dir(), "after_new_session", None);
    assert_eq!((r1.0, r1.2.as_str()), (1, "zygo: tmux: injected\n"));
    assert_eq!(read(&store).phase, Phase::Building);
    assert_eq!(remain(&l, 0), "on");
    l.o.zygo = normal;
    let (code, out, err) = run(&l.o);
    assert_eq!((code, out.as_str()), (0, "zygo: app is up\n"), "{err}");
    let sock = read(&store).tmux_socket.unwrap();
    let ids = by_key(&l.srv);
    assert_eq!(err.matches(&exited("kaaaaaaaa", "exit 3", &sock, &ids["kaaaaaaaa"].1)).count(), 1, "{err}");
    // `kbbbbbbbb` was built alive by the repair.
    assert_eq!(err.matches("exited during up").count(), 1, "{err}");
    assert_eq!(remain(&l, 0), "off");
    assert_eq!(read(&store).phase, Phase::Up);
}

/// FIX-29 round 2: the number of dead panes of `zygo-app`; 0 when the session or its server is gone.
fn dead_count(wrapper: &Path) -> usize {
    let out = Command::new(wrapper).args(["list-panes", "-s", "-t", "=zygo-app:", "-F", "#{pane_dead}"]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).lines().filter(|x| *x == "1").count()
}

#[test] // FIX-29 diff review round 2, finding 1, AU67: a repair builds with the hold; a pane it creates may exit at once
fn repair_holds_the_panes_it_creates_and_reports_them() {
    let _g = serial();
    let Some(_) = require_tmux() else { return };
    let l = live("0");
    let store = l.store();
    saved(
        &store,
        &l.repo.clone(),
        vec![win(0, Some("one"), "tiled", 120, 40), win(1, Some("two"), "tiled", 120, 40)],
        vec![
            pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &l.repo),
            pane("kbbbbbbbb", 0, 1, PaneKind::Shell, &l.wt),
            pane("kcccccccc", 1, 0, PaneKind::Shell, &l.wt),
        ],
        vec![],
    );
    // The first `up` stops after `new-session`: only `kaaaaaaaa` exists, alive.
    up::testing::hook(store.dir(), "after_new_session", Some(Box::new(|| Err("injected".into()))));
    let r1 = run(&l.o);
    up::testing::hook(store.dir(), "after_new_session", None);
    assert_eq!((r1.0, r1.2.as_str()), (1, "zygo: tmux: injected\n"));
    assert_eq!(read(&store).phase, Phase::Building);
    // The repair creates `kbbbbbbbb` (a split) and `kcccccccc` (a new window); both exit at once.
    let exits = ws_run_does(&l, "zygo-exits", "exit 3");
    let w = l.srv.wrapper().to_path_buf();
    up::testing::hook(
        store.dir(),
        "release",
        Some(Box::new(move || {
            assert!(wait_for(S5, || dead_count(&w) == 2), "the two repaired panes did not die within 5 s");
            Ok(())
        })),
    );
    let mux = up::server(&l.o.tmux, Some(l.srv.socket()), None);
    let (r, _, err) = repair_run(&store, &mux, &exits);
    up::testing::hook(store.dir(), "release", None);
    assert!(r.is_ok(), "repair failed: {r:?} {err}");
    assert!(!err.contains("tmux:"), "{err}");
    let ids = by_key(&l.srv);
    assert_eq!(
        ids.iter().map(|(k, (w, _))| (k.clone(), *w)).collect::<Vec<_>>(),
        [("kaaaaaaaa".to_string(), 0), ("kbbbbbbbb".to_string(), 0), ("kcccccccc".to_string(), 1)]
    );
    let sock = l.srv.socket().to_path_buf();
    for k in ["kbbbbbbbb", "kcccccccc"] {
        assert_eq!(err.matches(&exited(k, "exit 3", &sock, &ids[k].1)).count(), 1, "{err}");
    }
    assert_eq!(err.matches("exited during up").count(), 2, "{err}");
    assert_eq!(read(&store).phase, Phase::Up);
    for w in ids.values().map(|(w, _)| *w).collect::<BTreeSet<u32>>() {
        assert_eq!(remain(&l, w), "off", "window {w}");
    }
}

/// FIX-29 round 2, finding 2: a state in phase `building` whose saved `tmux_socket` holds `'`.
fn quoted_socket_state(t: &Path) -> (UpOpts, Store, PathBuf) {
    let r = repo(t, "app");
    let g = gone_tmux(t);
    let o = opts(t, &r, &g, &t.join("zygo"));
    let store = state_of(&o, &r);
    let sock = t.join("so'ck").join("s");
    saved(&store, &r, vec![win(0, None, "tiled", 200, 50)], vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &r)], vec![]);
    set(&store, |ws| {
        ws.phase = Phase::Building;
        ws.tmux_socket = Some(sock.clone());
    });
    (o, store, sock)
}

#[test] // FIX-29 diff review round 2, finding 2, AU54: up refuses a saved socket with `'` before any use, as at step 9
fn up_refuses_a_saved_socket_that_holds_a_quote_before_the_repair() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = &canon(td.path());
    let (o, store, sock) = quoted_socket_state(t);
    let before = raw(&store);
    assert_eq!(run(&o), (2, String::new(), format!("{AU54}\n")));
    assert_eq!(raw(&store), before);
    assert_eq!(tmux_log(t), ["-u -V"]);
    assert!(!sock.parent().unwrap().exists(), "the socket folder was created");
}

#[test] // FIX-29 diff review round 2, finding 2, AU54: repair itself refuses the saved socket before any tmux call
fn repair_refuses_a_saved_socket_that_holds_a_quote() {
    let _g = serial();
    let td = TempDir::new().unwrap();
    let t = &canon(td.path());
    let (o, store, sock) = quoted_socket_state(t);
    let before = raw(&store);
    let mux = up::server(&o.tmux, Some(&sock), None);
    let (r, out, err) = repair_run(&store, &mux, &o.zygo);
    let e = r.unwrap_err();
    assert_eq!(e.downcast_ref::<UnsafePath>(), Some(&UnsafePath), "{e:#}");
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
    assert_eq!(raw(&store), before);
    assert!(tmux_log(t).is_empty(), "{:?}", tmux_log(t));
}

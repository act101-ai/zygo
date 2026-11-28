//! Node W8: `zygo up --all` (W§8.1, AU52, AU80).
//!
//! Every test and helper is `cfg(unix)`: the tmux and zygo stand-ins are shell scripts (W7 D26). Every test takes
//! `serial()`, so no test writes a script while another test forks (ETXTBSY). Most cases use the stand-in tmux
//! `answer_tmux`, which answers from files in the test folder; the live case uses a private server (`PrivateServer`),
//! so no call reaches the default tmux server.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_config::Config;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_workspace::identity::workspace_id;
use zygo_workspace::mux::testing::{require_tmux, PrivateServer};
use zygo_workspace::state::{MemberRecord, MemberState, Pane, PaneKind, Phase, Store, Window, Workspace};
use zygo_workspace::up::{self, UpOpts};

const S5: Duration = Duration::from_secs(5);
const AU54: &str =
    "zygo: the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it";
const NO_TMUX: &str = "zygo: workspaces need tmux; install tmux 3.2 or later";
const NOTHING: &str = "zygo: up --all found no workspace to restore\n";

// ---- helpers (step 1) ----

fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}

/// Every path of `p` lies in the test folder `t`: no test reads or deletes the operator's own state.
fn assert_temp(p: &Paths, t: &Path) {
    for q in [&p.config_file, &p.cache_dir, &p.data_dir, &p.sessions_dir, &p.workspaces_dir(), &p.workspace_cache_dir()] {
        assert!(q.starts_with(t), "{} is outside the test folder {}", q.display(), t.display());
    }
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

fn tmux_log(dir: &Path) -> Vec<String> {
    let s = fs::read_to_string(dir.join("tmux.log")).unwrap_or_default();
    s.split('\u{1e}').filter(|l| !l.is_empty()).map(str::to_string).collect()
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

/// The stand-in tmux of step 1: it logs each call and answers from the files `exists-<s>`, `error-<s>`, `ws-<s>` and
/// `clients-<s>` of `dir`; every other call gets "no server running".
fn answer_tmux(dir: &Path) -> PathBuf { script(dir, "atmux", &ANSWER_BODY.replace("@D", &dir.display().to_string())) }

/// The `-t` value of each logged `has-session` call, in log order.
fn has_sessions(dir: &Path) -> Vec<String> {
    tmux_log(dir)
        .iter()
        .filter(|l| l.contains("has-session"))
        .filter_map(|l| l.split(' ').skip_while(|w| *w != "-t").nth(1).map(str::to_string))
        .collect()
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

/// Writes `<dir>/workspace.json` as given.
fn put_raw(dir: &Path, v: &Value) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("workspace.json"), serde_json::to_vec_pretty(v).unwrap()).unwrap();
}

static LIVE: Mutex<()> = Mutex::new(());
fn serial() -> std::sync::MutexGuard<'static, ()> { LIVE.lock().unwrap_or_else(|e| e.into_inner()) }

fn hold(path: PathBuf) -> fs::File {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let f = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).unwrap();
    fs4::FileExt::try_lock(&f).unwrap();
    f
}

/// A readable state: `root/.git` made, name, session, paths, loadout `solo`, phase `down`, then `f`.
fn ws_state(p: &Paths, id: &str, name: &str, root: &Path, f: impl Fn(&mut Workspace)) -> Store {
    fs::create_dir_all(root.join(".git")).unwrap();
    let store = Store::open(p, id);
    store
        .update(S5, &mut |ws| {
            ws.name = name.into();
            ws.tmux_session = format!("zygo-{name}");
            ws.common_dir = root.join(".git");
            ws.main_root = root.to_path_buf();
            ws.loadout = "solo".into();
            ws.phase = Phase::Down;
            f(ws);
            Ok(())
        })
        .unwrap();
    store
}

/// A folder with mode 0700.
fn private_dir(p: &Path) -> PathBuf {
    fs::create_dir_all(p).unwrap();
    fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
    p.to_path_buf()
}

/// A live session `<sessions_dir>/<id>`: `token` and a held `pid` lock, which lasts as long as the returned file.
fn live_session(p: &Paths, id: &str) -> fs::File {
    let dir = p.sessions_dir.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("token"), "t").unwrap();
    hold(dir.join("pid"))
}

/// A zygo stand-in: `ws run` sleeps; every other call exits 0.
fn sleeper(dir: &Path) -> PathBuf {
    script(dir, "zygo", "if [ \"$1\" = ws ] && [ \"$2\" = run ]; then exec sleep 600; fi\nexit 0")
}

fn opts(t: &Path, tmux: &Path) -> UpOpts {
    let mut vars = BTreeMap::new();
    vars.insert(OsString::from("PATH"), std::env::var_os("PATH").unwrap_or_default());
    vars.insert(OsString::from("HOME"), t.join("home").into_os_string());
    let o = UpOpts {
        name: None,
        loadout: None,
        reset: false,
        no_attach: true,
        cwd: t.to_path_buf(),
        zygo: sleeper(t),
        tmux: tmux.to_path_buf(),
        tmux_env: None,
        zygo_session: None,
        vars,
        paths: paths(t),
        config: Config::default(),
        default_loadout_set: false,
        git: None,
    };
    assert_temp(&o.paths, t);
    o
}

fn text(b: Vec<u8>) -> String { String::from_utf8(b).unwrap() }

fn run_all(o: &UpOpts) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = up::testing::run_all_with(o, &mut out, &mut err);
    (code, text(out), text(err))
}

/// One window and one shell pane in `main_root`: a layout that `up` restores.
fn restorable(ws: &mut Workspace) {
    ws.windows = vec![win(0, None, "tiled", 200, 50)];
    ws.panes = vec![pane("kaaaaaaaa", 0, 0, PaneKind::Shell, &ws.main_root.clone())];
}

/// The lines of `err` that hold ` skipped`.
fn skipped(err: &str) -> Vec<String> { err.lines().filter(|l| l.contains(" skipped")).map(str::to_string).collect() }

fn state_bytes(p: &Paths, id: &str) -> Vec<u8> { fs::read(p.workspaces_dir().join(id).join("workspace.json")).unwrap() }

/// A raw state that fails AU84 (`"name": "a b"`), at `e100000000000001`.
fn raw_invalid(p: &Paths) -> PathBuf {
    let dir = p.workspaces_dir().join("e100000000000001");
    put_raw(&dir, &json!({"version": 1, "id": "e100000000000001", "name": "a b", "common_dir": "/x/.git", "main_root": "/x",
        "tmux_session": "zygo-a b", "loadout": "solo", "phase": "down", "phase_since": 1}));
    dir.join("workspace.json")
}

/// A raw state of a newer zygo (`"version": 2`), at `e200000000000002`.
fn raw_newer(p: &Paths) {
    put_raw(&p.workspaces_dir().join("e200000000000002"), &json!({"version": 2, "id": "e200000000000002", "name": "e",
        "common_dir": "/x/.git", "main_root": "/x", "tmux_session": "zygo-e", "loadout": "solo", "phase": "down", "phase_since": 1}));
}

// ---- step 2 ----

#[test] // D4: flags, the zygo path and the tmux check come before any session check
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
fn up_all_refusals() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let tmux = answer_tmux(t);
    let p = paths(t);
    ws_state(&p, "a000000000000001", "a", &t.join("r/a"), |ws| {
        ws.down_all = true;
        restorable(ws);
    });
    let before = state_bytes(&p, "a000000000000001");
    let flags = (2, String::new(), "zygo: up --all takes no workspace name, --loadout or --reset\n".to_string());
    let mut o = opts(t, &tmux);
    o.reset = true;
    assert_eq!(run_all(&o), flags);
    let mut o = opts(t, &tmux);
    o.name = Some("a".into());
    assert_eq!(run_all(&o), flags);
    let mut o = opts(t, &tmux);
    o.loadout = Some("solo".into());
    assert_eq!(run_all(&o), flags);
    assert!(tmux_log(t).is_empty());
    assert_eq!(state_bytes(&p, "a000000000000001"), before);
    let mut o = opts(t, &tmux);
    o.zygo = t.join("zy'go");
    assert_eq!(run_all(&o), (2, String::new(), format!("{AU54}\n")));
    assert!(tmux_log(t).is_empty());
    assert_eq!(state_bytes(&p, "a000000000000001"), before);
    let old = script(t, "tmux31", "printf '%s\\n' 'tmux 3.1'");
    assert_eq!(
        run_all(&opts(t, &old)),
        (2, String::new(), "zygo: tmux 3.1 is too old; workspaces need tmux 3.2 or later\n".to_string())
    );
    assert_eq!(state_bytes(&p, "a000000000000001"), before);

    // No tmux program: `tmux_check` gives `Spawn` before any `session_exists`.
    let n = t.join("n");
    fs::create_dir_all(&n).unwrap();
    let pn = paths(&n);
    let sock = private_dir(&n.join("sock")).join("s");
    ws_state(&pn, "b000000000000001", "u1", &n.join("r/u1"), |ws| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(sock.clone());
    });
    ws_state(&pn, "b000000000000002", "u2", &n.join("r/u2"), |ws| ws.phase = Phase::Up);
    let (b1, b2) = (state_bytes(&pn, "b000000000000001"), state_bytes(&pn, "b000000000000002"));
    assert_eq!(run_all(&opts(&n, &n.join("none"))), (2, String::new(), format!("{NO_TMUX}\n")));
    assert_eq!((state_bytes(&pn, "b000000000000001"), state_bytes(&pn, "b000000000000002")), (b1, b2));
}

// ---- step 3 ----

#[test] // AU52, AU80, D2, D3, D7: the candidates, the skips and their lines
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
fn up_all_selects_and_skips() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let tmux = answer_tmux(t);
    let o = opts(t, &tmux);
    let p = o.paths.clone();
    let r = |n: &str| t.join("r").join(n);
    ws_state(&p, "a000000000000001", "a", &r("a"), |ws| {
        ws.down_all = true;
        restorable(ws);
    });
    ws_state(&p, "b000000000000002", "b", &r("b"), |ws| {
        ws.phase = Phase::Up;
        restorable(ws);
    });
    ws_state(&p, "c000000000000003", "c", &r("c"), restorable);
    ws_state(&p, "d000000000000004", "d", &r("d"), |ws| {
        ws.phase = Phase::Up;
        restorable(ws);
    });
    fs::write(t.join("exists-zygo-d"), "").unwrap();
    ws_state(&p, "f000000000000005", "f", &r("f"), |ws| {
        ws.down_all = true;
        restorable(ws);
    });
    let _held = Store::open(&p, "f000000000000005").op_lock().unwrap();
    ws_state(&p, "9000000000000006", "g", &r("g"), |ws| {
        ws.phase = Phase::Up;
        restorable(ws);
    });
    fs::write(t.join("error-zygo-g"), "").unwrap();
    ws_state(&p, "9000000000000007", "h", &r("h"), |ws| {
        ws.phase = Phase::Building;
        restorable(ws);
    });
    let e1 = raw_invalid(&p);
    raw_newer(&p);
    let ids = ["c000000000000003", "d000000000000004", "9000000000000007", "e100000000000001", "e200000000000002"];
    let before: Vec<Vec<u8>> = ids.iter().map(|id| state_bytes(&p, id)).collect();

    let (code, out, err) = run_all(&o);
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    assert_eq!(
        skipped(&err),
        [
            format!("zygo: workspace e100000000000001 skipped: {}: invalid name", e1.display()),
            "zygo: workspace e200000000000002 skipped: workspace state is from a newer zygo; run zygo update".to_string(),
            "zygo: a skipped".into(),
            "zygo: b skipped".into(),
            "zygo: f skipped".into(),
            "zygo: g skipped: tmux: permission denied".into()
        ]
    );
    assert!(err.contains("zygo: up or down is in progress for f\n"), "{err}");
    assert!(!err.contains("zygo: d") && !out.contains("zygo: d"), "{err}"); // D6: no line
    let made: Vec<String> = tmux_log(t)
        .iter()
        .filter(|l| l.contains("new-session"))
        .map(|l| l.split(' ').skip_while(|w| *w != "-s").nth(1).unwrap().to_string())
        .collect();
    assert_eq!(made, ["zygo-a", "zygo-b"]);
    let asked = has_sessions(t);
    for s in ["=zygo-d:", "=zygo-g:"] {
        assert_eq!(asked.iter().filter(|a| a.as_str() == s).count(), 1, "{asked:?}"); // the D2 checks only
    }
    assert!(asked.iter().any(|a| a == "=zygo-b:")); // D2, then W7 steps 6 and 9 ask again
    assert!(!tmux_log(t).iter().any(|l| l.contains("zygo-c") || l.contains("zygo-h")));
    assert_eq!(tmux_log(t)[0], "-u -V"); // D4: the tmux check comes first
    let after: Vec<Vec<u8>> = ids.iter().map(|id| state_bytes(&p, id)).collect();
    assert_eq!(after, before);
}

// ---- step 4 ----

#[test] // D3, D4: no workspace chosen
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
fn up_all_with_nothing_chosen() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let tmux = answer_tmux(t);
    let o = opts(t, &tmux);
    ws_state(&o.paths, "c000000000000003", "c", &t.join("r/c"), restorable);
    assert_eq!(run_all(&o), (0, String::new(), NOTHING.to_string()));
    assert_eq!(tmux_log(t), ["-u -V"]); // the D4 tmux check, no session check
    let e1 = raw_invalid(&o.paths);
    assert_eq!(
        run_all(&o),
        (1, String::new(), format!("zygo: workspace e100000000000001 skipped: {}: invalid name\n{NOTHING}", e1.display()))
    );
}

// ---- step 5 ----

#[test] // D5: two readable states with one name are not restored twice
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
fn up_all_two_states_one_name() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let tmux = answer_tmux(t);
    let o = opts(t, &tmux);
    for (id, root) in [("a000000000000001", "a1"), ("a000000000000002", "a2")] {
        ws_state(&o.paths, id, "a", &t.join("r").join(root), |ws| {
            ws.down_all = true;
            restorable(ws);
        });
    }
    let (code, _out, err) = run_all(&o);
    assert_eq!(code, 1, "{err}");
    assert_eq!(skipped(&err), ["zygo: a skipped", "zygo: a skipped: more than one workspace state has the name a"]);
    assert_eq!(tmux_log(t).iter().filter(|l| l.contains("new-session")).count(), 1);
}

// ---- step 6 ----

#[test] // D6: a running workspace of this id gets `down_all = false`, under `op.lock`
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
fn up_all_clears_down_all_of_a_running_workspace() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let tmux = answer_tmux(t);
    let p = paths(t);
    let a = ws_state(&p, "a000000000000001", "a", &t.join("r/a"), |ws| {
        ws.phase = Phase::Up;
        ws.down_all = true;
        restorable(ws);
    });
    fs::write(t.join("exists-zygo-a"), "").unwrap();
    fs::write(t.join("ws-zygo-a"), "a000000000000001").unwrap();
    ws_state(&p, "a000000000000002", "o2", &t.join("r/o2"), |ws| {
        ws.phase = Phase::Up;
        ws.down_all = true;
    });
    fs::write(t.join("exists-zygo-o2"), "").unwrap();
    fs::write(t.join("ws-zygo-o2"), "fedcba9876543210").unwrap();

    assert_eq!(run_all(&opts(t, &tmux)), (0, String::new(), NOTHING.to_string()));
    assert!(!Store::open(&p, "a000000000000001").read().unwrap().unwrap().down_all); // cleared: the workspace is up
    assert!(Store::open(&p, "a000000000000002").read().unwrap().unwrap().down_all); // another owner: unchanged
    assert!(!tmux_log(t).iter().any(|l| l.contains("new-session")));

    a.update(S5, &mut |ws| {
        ws.down_all = true;
        Ok(())
    })
    .unwrap();
    let held = Store::open(&p, "a000000000000001").op_lock().unwrap();
    assert_eq!(run_all(&opts(t, &tmux)), (0, String::new(), NOTHING.to_string()));
    assert!(Store::open(&p, "a000000000000001").read().unwrap().unwrap().down_all); // OpHeld: nothing changes
    drop(held);
}

// ---- step 7 ----

#[test] // D2, D3; AU62, AU87: the skip lines of a socket that is not private and of a live launcher
#[ignore = "timing: stand-in tmux under the fixed 2 s tmux bound"]
fn up_all_skip_lines() {
    let _s = serial();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let tmux = answer_tmux(t);
    let p = paths(t);
    let open = t.join("open");
    fs::create_dir_all(&open).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
    let sock = open.join("s");
    ws_state(&p, "a000000000000001", "p62", &t.join("r/p62"), |ws| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(sock.clone());
        restorable(ws);
    });
    let (code, _out, err) = run_all(&opts(t, &tmux));
    assert_eq!(code, 1, "{err}");
    assert_eq!(
        skipped(&err),
        [format!(
            "zygo: p62 skipped: tmux socket {} is not private: the socket and its folder must be yours and the folder must have mode 0700",
            sock.display()
        )]
    );
    assert!(!tmux_log(t).iter().any(|l| l.contains(&sock.display().to_string())));

    fs::remove_dir_all(p.workspaces_dir().join("a000000000000001")).unwrap();
    ws_state(&p, "a000000000000002", "l87", &t.join("r/l87"), |ws| {
        ws.phase = Phase::Up;
        restorable(ws);
        let mut m = member(Agent::ClaudeCode, MemberState::Resumable, Some("c-1"));
        m.zygo_session = Some("01L".into());
        ws.members.insert("kaaaaaaaa".into(), m);
    });
    let _live = live_session(&p, "01L");
    let (code, _out, err) = run_all(&opts(t, &tmux));
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains(
            "zygo: l87 has live agents (pane kaaaaaaaa) but its tmux session cannot be reached at the default tmux socket\nzygo: l87 skipped\n"
        ),
        "{err}"
    );
    assert!(!tmux_log(t).iter().any(|l| l.contains("new-session")));
}

// ---- step 8 ----

#[test] // AU52: `up --all` restores on a private server and attaches to none
#[ignore = "timing: real tmux under the fixed 2 s tmux bound and the 2 s cwd wait"]
fn up_all_restores_on_a_private_server() {
    let _s = serial();
    let Some(_) = require_tmux() else { return };
    let srv = PrivateServer::start();
    let td = TempDir::new().unwrap();
    let t = td.path();
    let o = opts(t, srv.wrapper());
    let (ra, rb, rc) = (repo(t, "a"), repo(t, "b"), repo(t, "c"));
    let id = |r: &Path| workspace_id(&canon(&r.join(".git")));
    let put = |n: &str, r: &Path, f: &dyn Fn(&mut Workspace)| {
        Store::open(&o.paths, &id(r))
            .update(S5, &mut |ws| {
                ws.name = n.into();
                ws.tmux_session = format!("zygo-{n}");
                ws.common_dir = canon(&r.join(".git"));
                ws.main_root = r.to_path_buf();
                ws.loadout = "solo".into();
                ws.phase = Phase::Down;
                restorable(ws);
                f(ws);
                Ok(())
            })
            .unwrap();
    };
    put("a", &ra, &|ws| ws.down_all = true);
    put("b", &rb, &|ws| {
        ws.phase = Phase::Up;
        ws.tmux_socket = Some(srv.socket().to_path_buf());
    });
    put("c", &rc, &|_| {});

    let (code, out, err) = run_all(&o);
    assert_eq!((code, out.as_str()), (0, "zygo: a is up\nzygo: b is up\n"), "{err}");
    assert!(!err.contains("skipped"), "{err}");
    for (n, r) in [("a", &ra), ("b", &rb)] {
        let ws = Store::open(&o.paths, &id(r)).read().unwrap().unwrap();
        assert_eq!((ws.phase, ws.down_all, ws.tmux_socket.is_some()), (Phase::Up, false, true));
        assert!(srv.run(&["has-session", "-t", &format!("=zygo-{n}:")]).is_ok());
    }
    assert!(srv.run(&["has-session", "-t", "=zygo-c:"]).is_err());
    assert_eq!(Store::open(&o.paths, &id(&rc)).read().unwrap().unwrap().phase, Phase::Down);
    assert_eq!(run_all(&o), (0, String::new(), NOTHING.to_string())); // D6
}

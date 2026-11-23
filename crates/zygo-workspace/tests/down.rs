//! Node W10: `zygo down`: detach, drain loop, timeout, stop, `--now`, `--all` (W§9.3, W§18 #10 #36 #47 #52 #53 #80
//! #81 #84).
//!
//! Every test is `cfg(unix)` (D25): the stand-in programs are shell scripts. Every test takes `serial()`: no script is
//! written while another test forks (ETXTBSY), and signal dispositions are process-wide. Time is virtual
//! (`down::testing::Hooks`, D24); the only real waits wait for an event and have a 5 s bound. Every case has its own
//! `TempDir`, and every state path points into it. The tmux tests use a private server (`PrivateServer`), so no call
//! reaches the default tmux server. Signals go only to this process or to its own children.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;
use tempfile::TempDir;
use zygo_config::Config;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
use zygo_proxy::client::SessionStatus;
use zygo_workspace::activity;
use zygo_workspace::down::{self, testing::{Fault, Hooks}, DownOpts, DownTarget, ProcIdentity};
use zygo_workspace::identity::workspace_id;
use zygo_workspace::mux::testing::{require_tmux, PrivateServer};
use zygo_workspace::snapshot;
use zygo_workspace::state::{MemberRecord, MemberState, Phase, Store};
use zygo_workspace::up::{self, UpOpts};

const ID: &str = "0123456789abcdef";
const KEY: &str = "kaaaaaaaa";
const KEY2: &str = "kbbbbbbbb";
const SID: &str = "01JTESTSESSIONAAAAAAAAAAAA";
const SID2: &str = "01JTESTSESSIONBBBBBBBBBBBB";
const S5: Duration = Duration::from_secs(5);
const AU18: &str =
    "zygo: down cannot run detached from a sandboxed shell; use the workspace_down tool ([features] workspace_tool) or /zygo:down";
const AU54: &str =
    "zygo: the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it";
/// The M2 line of round 3 for workspace `name` and the tmux error `e`.
fn unreachable_line(name: &str, e: &str) -> String {
    format!(
        "zygo: cannot reach the tmux server of {name} ({e}); in a sandboxed shell, run zygo down outside it; if that server is gone, run zygo down --wait -- {name} from a shell outside the workspace\n"
    )
}
const AU63: &str ="zygo: tmux session zygo-app does not belong to this workspace; rename it or run zygo ws forget app";
const IDLE: &str = "zygo: app pane kaaaaaaaa is idle";
const TURN: &str = "zygo: app pane kaaaaaaaa: no Stop since its last prompt (interrupted turn); treated as idle";
const SKIP: &str = "zygo: app pane kaaaaaaaa: proxy request state not usable; idle rule 3 skipped";
const NOW_LINE: &str = "zygo: run zygo down --now to stop at once";

fn t0() -> SystemTime { UNIX_EPOCH + Duration::from_secs(1_800_000_000) }

fn hooks() -> Hooks { Hooks::new(t0()) }

// ---- helpers (step 1) ----

fn paths(p: &Path) -> Paths {
    Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
}

/// An executable `#!/bin/sh` program `<dir>/<name>`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let f = dir.join(name);
    fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o755)).unwrap();
    f
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

/// The tmux stand-in `<dir>/tmux` that needs no tmux (step 1). `list-panes` answers "can't find session", so the
/// snapshot gives `Skipped(Gone)` and step 9 warns about nothing; `<dir>/nosrv` makes every call but `-V` answer as a
/// server that does not exist. `list-sessions` prints the server pid (`<dir>/server.pid`, else 4242) twice, one line
/// per session; a `display-message` of `#{pid}` on a gone session fails (round 2 item 7). `<dir>/dead-<name>` makes
/// every call on the socket `.../<name>` answer as a server that does not exist (round 3, M2).
fn fake_tmux(dir: &Path) -> PathBuf {
    let d = dir.display();
    let body = format!(
        "printf '%s\\036' \"$*\" >> '{d}/tmux.log'\n\
         s=; sp=; prev=; for a in \"$@\"; do case \"$a\" in -V) echo 'tmux 3.4'; exit 0;; =zygo-*) s=${{a#=}}; s=${{s%%:*}};; esac; [ \"$prev\" = -S ] && sp=$a; prev=$a; done\n\
         if [ -e '{d}/nosrv' ]; then echo \"error connecting to {d}/sock/s (No such file or directory)\" >&2; exit 1; fi\n\
         if [ -n \"$sp\" ] && [ -e '{d}/dead-'\"${{sp##*/}}\" ]; then echo \"no server running on $sp\" >&2; exit 1; fi\n\
         pid=$(cat '{d}/server.pid' 2>/dev/null || echo 4242)\n\
         case \" $* \" in\n\
         \x20 *\" has-session \"*) if [ -e '{d}/gone-'\"$s\" ]; then echo \"can't find session: $s\" >&2; exit 1; fi; exit 0;;\n\
         \x20 *\" show-options \"*) [ -e '{d}/gone-'\"$s\" ] || cat '{d}/ws-'\"$s\" 2>/dev/null; exit 0;;\n\
         \x20 *\" kill-session \"*) : > '{d}/gone-'\"$s\"; exit 0;;\n\
         \x20 *\" list-sessions \"*) echo \"$pid\"; echo \"$pid\"; exit 0;;\n\
         \x20 *'#{{session_name}}'*) echo zygo-app; exit 0;;\n\
         \x20 *'#{{pid}}'*) if [ -e '{d}/gone-'\"$s\" ]; then echo \"can't find session: $s\" >&2; exit 1; fi; echo \"$pid\"; exit 0;;\n\
         \x20 *) echo \"can't find session: $s\" >&2; exit 1;;\n\
         esac"
    );
    script(dir, "tmux", &body)
}

fn tmux_log(dir: &Path) -> Vec<String> {
    let s = fs::read_to_string(dir.join("tmux.log")).unwrap_or_default();
    s.split('\u{1e}').filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// `@zygo-ws` of `session` in the stand-in.
fn owner(dir: &Path, session: &str, id: &str) { fs::write(dir.join(format!("ws-{session}")), id).unwrap(); }

/// The stand-in's `session` is gone.
fn gone(dir: &Path, session: &str) { fs::write(dir.join(format!("gone-{session}")), "").unwrap(); }

/// #36: `-V` gives 3.4; every other call fails with `permission denied`.
fn perm_tmux(dir: &Path) -> PathBuf {
    script(dir, "ptmux", "for a in \"$@\"; do if [ \"$a\" = -V ]; then echo 'tmux 3.4'; exit 0; fi; done\necho 'permission denied' >&2\nexit 1")
}

fn member(sid: Option<&str>) -> MemberRecord {
    let mut m = MemberRecord::new(Agent::ClaudeCode);
    m.state = MemberState::Running;
    m.zygo_session = sid.map(String::from);
    m
}

/// The TempDir of `o` (every helper path is made from `paths(t)`).
fn tdir(o: &DownOpts) -> PathBuf { o.paths.data_dir.parent().unwrap().to_path_buf() }

fn ws_state(o: &DownOpts, id: &str, name: &str, phase: Phase, members: Vec<(&str, MemberRecord)>) -> Store {
    let t = tdir(o);
    fs::create_dir_all(t.join("sock")).unwrap();
    fs::set_permissions(t.join("sock"), fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open(&o.paths, id);
    store
        .update(S5, &mut |ws| {
            ws.name = name.into();
            ws.tmux_session = format!("zygo-{name}");
            ws.common_dir = t.join(name).join(".git");
            ws.main_root = t.join(name);
            ws.loadout = "solo".into();
            ws.tmux_socket = Some(t.join("sock/s"));
            ws.phase = phase;
            ws.phase_since = 1_700_000_000;
            ws.members = members.iter().map(|(k, m)| (k.to_string(), m.clone())).collect();
            Ok(())
        })
        .unwrap();
    assert!(store.dir().starts_with(&t), "state outside the TempDir: {}", store.dir().display());
    store
}

fn set_down_all(store: &Store) {
    store.update(S5, &mut |ws| { ws.down_all = true; Ok(()) }).unwrap();
}

fn raw(store: &Store) -> Vec<u8> { fs::read(store.dir().join("workspace.json")).unwrap() }

fn put_raw(store: &Store, bytes: &[u8]) {
    fs::create_dir_all(store.dir()).unwrap();
    fs::write(store.dir().join("workspace.json"), bytes).unwrap();
}

fn phase(o: &DownOpts, id: &str) -> Phase { Store::open(&o.paths, id).read().unwrap().unwrap().phase }

fn phase_all(o: &DownOpts, id: &str) -> (Phase, bool) {
    let ws = Store::open(&o.paths, id).read().unwrap().unwrap();
    (ws.phase, ws.down_all)
}

/// A process identity of the stand-in probes: `comm` is ours for `self`, else the tmux server's.
fn ident(p: &str, mnt: &str, pid: &str) -> ProcIdentity {
    let comm = if p == "self" { "down-test" } else { "tmux: server" };
    ProcIdentity { mnt_ns: mnt.into(), pid_ns: pid.into(), comm: Some(comm.into()) }
}
fn ns_same(p: &str) -> Option<ProcIdentity> { Some(ident(p, "mnt:[1]", "pid:[1]")) }
fn ns_differs(p: &str) -> Option<ProcIdentity> { Some(ident(p, if p == "self" { "mnt:[1]" } else { "mnt:[2]" }, "pid:[1]")) }
fn pidns_differs(p: &str) -> Option<ProcIdentity> { Some(ident(p, "mnt:[1]", if p == "self" { "pid:[1]" } else { "pid:[2]" })) }
fn ns_unreadable(p: &str) -> Option<ProcIdentity> { (p == "self").then(|| ident(p, "mnt:[1]", "pid:[1]")) }
fn ns_self_unreadable(p: &str) -> Option<ProcIdentity> { (p != "self").then(|| ident(p, "mnt:[1]", "pid:[1]")) }
/// The server's namespaces match, but its comm is not tmux.
fn comm_not_tmux(p: &str) -> Option<ProcIdentity> {
    let mut i = ident(p, "mnt:[1]", "pid:[1]");
    if p != "self" {
        i.comm = Some("sleep".into());
    }
    Some(i)
}

fn opts(t: &Path, tmux: &Path) -> DownOpts {
    DownOpts {
        target: DownTarget::Name("app".into()),
        now: false,
        timeout: None,
        wait: false,
        cwd: t.to_path_buf(),
        zygo: script(t, "nozygo", "exit 1"),
        tmux: tmux.to_path_buf(),
        tmux_env: None,
        tmux_pane: None,
        zygo_session: None,
        proc_probe: ns_same,
        paths: paths(t),
        config: Config::default(),
    }
}

fn text(b: Vec<u8>) -> String { String::from_utf8(b).unwrap() }

fn run(o: &DownOpts, h: &Hooks) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = down::testing::run_with(o, h, &mut out, &mut err);
    (code, text(out), text(err))
}

fn count(text: &str, line: &str) -> usize { text.lines().filter(|l| *l == line).count() }

/// The fs4 lock on a launcher's `pid` file, held by a test until it takes it out.
type HeldLock = Arc<Mutex<Option<File>>>;

/// A live launcher: its session folder with `token` and `url`, a `sleep 600` child whose pid is in `pid`, and the
/// fs4 lock on `pid` held by this test (an in-process lock makes `session_dir::live` true).
struct Launcher {
    lock: HeldLock,
    child: Child,
}

impl Drop for Launcher {
    /// The child is this test's own; a reaped child is not signalled again (std keeps its status).
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn session_files(o: &DownOpts, sid: &str) -> (PathBuf, Child) {
    let dir = o.paths.sessions_dir.join(sid);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("token"), "t").unwrap();
    fs::write(dir.join("url"), "http://127.0.0.1:1/").unwrap();
    let child = Command::new("sleep").arg("600").spawn().unwrap();
    fs::write(dir.join("pid"), child.id().to_string()).unwrap();
    (dir, child)
}

fn launcher(o: &DownOpts, sid: &str) -> Launcher {
    let (dir, child) = session_files(o, sid);
    let f = fs::OpenOptions::new().read(true).write(true).open(dir.join("pid")).unwrap();
    fs4::FileExt::try_lock(&f).unwrap();
    Launcher { lock: Arc::new(Mutex::new(Some(f))), child }
}

/// The same files as `launcher`, with the pid of an unrelated `sleep 600` child and no lock (#52).
fn dead_launcher(o: &DownOpts, sid: &str) -> Child { session_files(o, sid).1 }

fn session_dir(o: &DownOpts, sid: &str) -> SessionDir { SessionDir::new(o.paths.sessions_dir.join(sid)) }

fn set_activity(o: &DownOpts, sid: &str, busy: bool, running: &[(&str, &str)]) {
    let running: BTreeMap<String, String> = running.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    activity::update(&session_dir(o, sid), S5, &mut |a| {
        a.busy = busy;
        a.running = running.clone();
    })
    .unwrap();
}

fn quiet() -> SessionStatus { SessionStatus { requests_in_flight: Some(0), last_request_end: None, ..Default::default() } }

fn in_flight() -> SessionStatus { SessionStatus { requests_in_flight: Some(1), ..quiet() } }

type StatusMap = Arc<Mutex<BTreeMap<String, Result<SessionStatus, String>>>>;

fn status_map(entries: Vec<(&str, Result<SessionStatus, String>)>) -> StatusMap {
    Arc::new(Mutex::new(entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect()))
}

/// Sets `h.status`: it records each session id and returns its map entry (a missing id gives `Err("no status")`).
fn statuses(h: &mut Hooks, map: StatusMap) -> Arc<Mutex<Vec<String>>> {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let c = calls.clone();
    h.status = Some(Box::new(move |sid: &str| {
        c.lock().unwrap().push(sid.to_string());
        map.lock().unwrap().get(sid).cloned().unwrap_or_else(|| Err("no status".into()))
    }));
    calls
}

/// The point `after_sigterm`: for each `(workspace id, member key)`, paired in order with a held launcher lock, whose
/// workspace is in `stopping` (the point runs after the step 10 of every workspace, so with `--all` another
/// workspace's stop must not release this member): drops the lock and sets the member to `resumable`, as a launcher
/// and its wrapper do after SIGTERM.
fn release_on_sigterm(h: &mut Hooks, o: &DownOpts, locks: Vec<HeldLock>, keys: &[(&str, &str)]) {
    assert_eq!(locks.len(), keys.len());
    let pairs: Vec<(HeldLock, String, String)> =
        locks.into_iter().zip(keys).map(|(l, (a, b))| (l, a.to_string(), b.to_string())).collect();
    let p = o.paths.clone();
    h.points.insert(
        "after_sigterm",
        Box::new(move || {
            for (lock, id, key) in &pairs {
                let store = Store::open(&p, id);
                if store.read().unwrap().unwrap().phase != Phase::Stopping {
                    continue;
                }
                lock.lock().unwrap().take();
                store
                    .update(S5, &mut |ws| {
                        if let Some(m) = ws.members.get_mut(key) {
                            m.state = MemberState::Resumable;
                        }
                        Ok(())
                    })
                    .unwrap();
            }
        }),
    );
}

fn disp(sig: i32) -> libc::sighandler_t {
    // SAFETY: a null new action only reads the current one into `old`.
    unsafe {
        let mut old: libc::sigaction = std::mem::zeroed();
        assert_eq!(libc::sigaction(sig, std::ptr::null(), &mut old), 0);
        old.sa_sigaction
    }
}

fn disps() -> [libc::sighandler_t; 3] { [disp(libc::SIGINT), disp(libc::SIGHUP), disp(libc::SIGTERM)] }

fn begin(dir: &Path, agent: &str, unit: &str, stopped: Option<i64>) {
    fs::create_dir_all(dir.join("work")).unwrap();
    let v = json!({"unit": unit, "round": 1, "check": "c", "files": [], "began_at": 1, "offset": 0, "session": "s",
        "agent_type": null, "stopped_at": stopped});
    fs::write(dir.join("work").join(format!("{agent}.begin.json")), serde_json::to_vec(&v).unwrap()).unwrap();
}

/// The zygo stand-in of the detached start: it writes its arguments, then its pid, and runs `sleep 30`.
fn rec(t: &Path) -> PathBuf {
    let d = t.display();
    script(t, "rec", &format!(
        "printf '%s\\n' \"$*\" > '{d}/child.args'; printf '%s\\n' \"$$\" > '{d}/child.pid.tmp'; mv '{d}/child.pid.tmp' '{d}/child.pid'; exec sleep 30"
    ))
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

/// The pid of the detached stand-in, once it runs `sleep` (5 s bound).
fn started(t: &Path) -> u32 {
    let file = t.join("child.pid");
    assert!(wait_for(S5, || fs::read_to_string(&file).is_ok_and(|s| s.ends_with('\n'))), "no child.pid");
    let pid: u32 = fs::read_to_string(&file).unwrap().trim().parse().unwrap();
    #[cfg(target_os = "linux")]
    assert!(wait_for(S5, || fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c == "sleep\n")), "not sleep");
    pid
}

fn args(t: &Path) -> String { fs::read_to_string(t.join("child.args")).unwrap() }

/// Ends `pid` only while `waitpid(WNOHANG)` says it is a live child of this process (round 2 item 10).
fn end_own_child(pid: i32) {
    // SAFETY: plain system calls; `kill` runs only for a live child of this process, which `waitpid` then reaps.
    unsafe {
        if pid > 1 && libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) == 0 {
            libc::kill(pid, libc::SIGKILL);
            libc::waitpid(pid, std::ptr::null_mut(), 0);
        }
    }
}

/// The detached stand-in that `down` started in this process (a child of this process): ended when the guard drops,
/// also when an assertion fails.
struct Detached {
    t: PathBuf,
}

impl Detached {
    fn new(t: &Path) -> Detached { Detached { t: t.to_path_buf() } }
}

impl Drop for Detached {
    fn drop(&mut self) {
        if let Some(pid) = fs::read_to_string(self.t.join("child.pid")).ok().and_then(|s| s.trim().parse().ok()) {
            end_own_child(pid);
        }
    }
}

/// A `sleep 30` child of this process, ended on drop: a stand-in pid whose comm is not tmux (round 2 item 6).
struct Sleeper(Child);

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn set_tools(o: &DownOpts, sid: &str, n: u32) {
    activity::update(&session_dir(o, sid), S5, &mut |a| a.tools_running = n).unwrap();
}

/// Every call of the write or step `name` fails with `f` (round 2 item 2).
fn fault(h: &mut Hooks, name: &'static str, f: Fault) { h.faults.insert(name, Box::new(move || Some(f.clone()))); }

fn disk_full() -> Fault { Fault::Error("disk full".into()) }

fn no_socket(store: &Store) { store.update(S5, &mut |ws| { ws.tmux_socket = None; Ok(()) }).unwrap(); }

fn unprivate(t: &Path) { fs::set_permissions(t.join("sock"), fs::Permissions::from_mode(0o755)).unwrap(); }

fn not_private_line(t: &Path) -> String {
    format!(
        "zygo: tmux socket {}/sock/s is not private: the socket and its folder must be yours and the folder must have mode 0700\n",
        t.display()
    )
}

static SERIAL: Mutex<()> = Mutex::new(());
fn serial() -> std::sync::MutexGuard<'static, ()> { SERIAL.lock().unwrap_or_else(|e| e.into_inner()) }

// ---- live harness (copy of tests/up.rs) ----

/// The zygo stand-in of W7 D25: `ws run` copies the state to `<dir>/runs/<key>.json`, waits `delay`, changes to the
/// pane's cwd read from that copy and runs `sleep 600`; every other call exits 0.
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

struct Live {
    t: TempDir,
    srv: PrivateServer,
    id: String,
}

impl Live {
    fn store(&self) -> Store { Store::open(&paths(self.t.path()), &self.id) }
}

/// Repository `app`, a private server, the stand-in zygo, and `zygo up` (no attach) on them.
fn live() -> Live {
    let t = TempDir::new().unwrap();
    let srv = PrivateServer::start();
    let repo = repo(t.path(), "app");
    let id = workspace_id(&canon(&repo.join(".git")));
    let p = paths(t.path());
    let state = p.workspaces_dir().join(&id).join("workspace.json");
    let zygo = stand_in(t.path(), &state, "0");
    let mut vars = BTreeMap::new();
    vars.insert(OsString::from("PATH"), std::env::var_os("PATH").unwrap_or_default());
    vars.insert(OsString::from("HOME"), t.path().join("home").into_os_string());
    let o = UpOpts {
        name: None,
        loadout: None,
        reset: false,
        no_attach: true,
        cwd: repo.clone(),
        zygo,
        tmux: srv.wrapper().to_path_buf(),
        tmux_env: None,
        zygo_session: None,
        vars,
        paths: p,
        config: Config::default(),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = up::testing::run_with(&o, &mut out, &mut err);
    assert_eq!((code, text(out).as_str()), (0, "zygo: app is up\n"), "{}", text(err));
    Live { t, srv, id }
}

fn live_opts(l: &Live) -> DownOpts { opts(l.t.path(), l.srv.wrapper()) }

fn errors_lines(store: &Store) -> usize {
    fs::read_to_string(store.dir().join("errors.log")).map_or(0, |s| s.lines().count())
}

// ---- step 2 ----

#[test]
fn constants_and_mount_namespace() {
    let _s = serial();
    for bad in ["", "../1", "1/ns", "12345678901", "-1", "self/.."] {
        assert_eq!(down::proc_identity(bad), None, "{bad:?}");
    }
    #[cfg(target_os = "linux")]
    {
        // Round 2 item 6: both namespaces and the comm.
        let link = |n: &str| fs::read_link(format!("/proc/self/ns/{n}")).unwrap().to_string_lossy().into_owned();
        let me = down::proc_identity("self").unwrap();
        assert!(me.mnt_ns.starts_with("mnt:[") && me.pid_ns.starts_with("pid:["), "{me:?}");
        assert_eq!((me.mnt_ns.clone(), me.pid_ns.clone()), (link("mnt"), link("pid")));
        let comm = fs::read_to_string("/proc/self/comm").unwrap();
        assert_eq!(me.comm.as_deref(), Some(comm.trim_end_matches('\n')));
        assert_eq!(down::proc_identity(&std::process::id().to_string()), Some(me));
        assert_eq!(down::proc_identity("4000000000"), None);
    }
    #[cfg(not(target_os = "linux"))]
    assert_eq!(down::proc_identity("self"), Some(ProcIdentity { mnt_ns: String::new(), pid_ns: String::new(), comm: None }));
}

// ---- step 3 ----

#[test]
fn refusals_change_nothing() {
    let _s = serial();
    let h = hooks();
    {
        // No tmux, an old tmux.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &t.join("none"));
        assert_eq!(run(&o, &h), (2, String::new(), "zygo: workspaces need tmux; install tmux 3.2 or later\n".into()));
        o.target = DownTarget::All;
        assert_eq!(run(&o, &h), (2, String::new(), "zygo: workspaces need tmux; install tmux 3.2 or later\n".into()));
        o.tmux = script(t, "t31", "echo 'tmux 3.1'");
        assert_eq!(run(&o, &h), (2, String::new(), "zygo: tmux 3.1 is too old; workspaces need tmux 3.2 or later\n".into()));
    }
    {
        // A state from a newer zygo.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        let store = Store::open(&o.paths, ID);
        put_raw(&store, br#"{"version": 2, "id": "0123456789abcdef", "name": "app"}"#);
        let before = raw(&store);
        assert_eq!(run(&o, &h), (1, String::new(), "zygo: workspace state is from a newer zygo; run zygo update\n".into()));
        assert_eq!(raw(&store), before);
    }
    for (field, value, reason) in [("name", "a b", "invalid name"), ("tmux_session", "other", "invalid tmux_session")] {
        // #10 a, b: a state that fails AU84, each in its own TempDir.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &fake_tmux(t));
        let r = repo(t, "app");
        o.target = DownTarget::Cwd;
        o.cwd = r.clone();
        let common = canon(&r.join(".git"));
        let store = Store::open(&o.paths, &workspace_id(&common));
        let mut v = json!({"version": 1, "id": workspace_id(&common), "name": "app", "common_dir": common, "main_root": r,
            "tmux_session": "zygo-app", "loadout": "solo", "phase": "up", "phase_since": 1700000000u64});
        v[field] = json!(value);
        put_raw(&store, &serde_json::to_vec_pretty(&v).unwrap());
        let before = raw(&store);
        let (code, out, err) = run(&o, &h);
        assert_eq!((code, out.as_str()), (1, ""), "{err}");
        assert!(err.starts_with("zygo: ") && err.contains("workspace.json") && err.contains(reason), "{err}");
        assert_eq!(err.lines().count(), 1, "{err}");
        assert_eq!(tmux_log(t), ["-u -V"]);
        assert_eq!(raw(&store), before);
    }
    {
        // #10 c, R9: another owner of the session.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
        owner(t, "zygo-app", "abc");
        let before = raw(&store);
        assert_eq!(run(&o, &h), (1, String::new(), format!("{AU63}\n")));
        assert!(!tmux_log(t).iter().any(|l| l.contains("kill-session")));
        assert_eq!(raw(&store), before);
        assert!(store.op_lock_free());
    }
    {
        // #36: a tmux error that is not "gone", for `down` and for `up`.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
        owner(t, "zygo-app", ID);
        let before = raw(&store);
        o.tmux = perm_tmux(t);
        assert_eq!(run(&o, &h), (1, String::new(), "zygo: tmux: permission denied\n".into()));
        assert_eq!(raw(&store), before);
        let u = UpOpts {
            name: Some("app".into()),
            loadout: None,
            reset: false,
            no_attach: true,
            cwd: t.to_path_buf(),
            zygo: o.zygo.clone(),
            tmux: perm_tmux(t),
            tmux_env: None,
            zygo_session: None,
            vars: BTreeMap::new(),
            paths: paths(t),
            config: Config::default(),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = up::testing::run_with(&u, &mut out, &mut err);
        assert_eq!((code, text(out), text(err)), (1, String::new(), "zygo: tmux: permission denied\n".into()));
        assert_eq!(raw(&store), before);
    }
    {
        // `op.lock` held.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
        owner(t, "zygo-app", ID);
        let before = raw(&store);
        let _held = store.op_lock().unwrap();
        assert_eq!(run(&o, &h), (1, String::new(), "zygo: up or down is in progress for app\n".into()));
        assert_eq!(raw(&store), before);
    }
    {
        // AU87: the session is gone but a launcher is live.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![(KEY, member(Some(SID)))]);
        owner(t, "zygo-app", ID);
        gone(t, "zygo-app");
        let mut l = launcher(&o, SID);
        let before = raw(&store);
        assert_eq!(
            run(&o, &h),
            (1, String::new(), format!("zygo: app has live agents (pane kaaaaaaaa) but its tmux session cannot be reached at {}/sock/s\n", t.display()))
        );
        assert_eq!(raw(&store), before);
        assert!(l.child.try_wait().unwrap().is_none()); // no signal
        l.child.kill().unwrap();
    }
    {
        // R4: the session marker.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &fake_tmux(t));
        o.target = DownTarget::Session(None);
        assert_eq!(run(&o, &h), (1, String::new(), "zygo: this session is not in a workspace\n".into()));
        o.target = DownTarget::Session(Some("../x".into()));
        assert_eq!(run(&o, &h), (1, String::new(), "zygo: invalid workspace id ../x\n".into()));
        o.target = DownTarget::Session(Some("fedcba9876543210".into()));
        assert_eq!(run(&o, &h), (1, String::new(), "zygo: no workspace with id fedcba9876543210\n".into()));
    }
    {
        // R4: the cwd decides; another workspace is not touched.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
        owner(t, "zygo-app", ID);
        let before = raw(&store);
        o.target = DownTarget::Cwd;
        o.cwd = repo(t, "other");
        assert_eq!(run(&o, &h), (0, "zygo: other is not running\n".into(), String::new()));
        assert_eq!(raw(&store), before);
    }
    {
        // No state (R12, its own TempDir).
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &fake_tmux(t));
        o.target = DownTarget::Cwd;
        o.cwd = repo(t, "app");
        assert_eq!(run(&o, &h), (0, "zygo: app is not running\n".into(), String::new()));
        assert!(!o.paths.workspaces_dir().exists());
    }
}

// ---- step 4 ----

#[test]
fn gone_repair_already_down_and_session_target() {
    let _s = serial();
    let h = hooks();
    for target in [DownTarget::Name("app".into()), DownTarget::Session(Some(ID.into()))] {
        // Gone, no live launcher: `phase = down`, `down_all` stays false.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &fake_tmux(t));
        o.target = target;
        let store = ws_state(&o, ID, "app", Phase::Up, vec![(KEY, member(None))]);
        gone(t, "zygo-app");
        assert_eq!(run(&o, &h), (0, "zygo: app is not running\n".into(), String::new()));
        assert_eq!(phase_all(&o, ID), (Phase::Down, false));
        assert!(store.op_lock_free());
    }
    {
        // R5: a workspace that is already down gets no write.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Down, vec![]);
        set_down_all(&store);
        gone(t, "zygo-app");
        let before = raw(&store);
        assert_eq!(run(&o, &h), (0, "zygo: app is not running\n".into(), String::new()));
        assert_eq!(raw(&store), before);
    }
    {
        // R5: only --all sets `down_all`; nothing in `down` clears it.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
        set_down_all(&store);
        owner(t, "zygo-app", ID);
        assert_eq!(run(&o, &h), (0, "zygo: app is down (0 agents saved)\n".into(), String::new()));
        assert_eq!(phase_all(&o, ID), (Phase::Down, true));
    }
    {
        // Repair from `draining`.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        ws_state(&o, ID, "app", Phase::Draining, vec![]);
        owner(t, "zygo-app", ID);
        assert_eq!(run(&o, &h), (0, "zygo: app is down (0 agents saved)\n".into(), String::new()));
        assert_eq!(phase(&o, ID), Phase::Down);
        assert!(tmux_log(t).iter().any(|c| c.ends_with("kill-session -t =zygo-app:")));
    }
    {
        // Repair from `stopping` with no live launcher: `repair` kills and writes `down`; step 3 then finds it gone.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        ws_state(&o, ID, "app", Phase::Stopping, vec![(KEY, member(None))]);
        owner(t, "zygo-app", ID);
        assert_eq!(run(&o, &h), (0, "zygo: app is not running\n".into(), String::new()));
        assert_eq!(phase(&o, ID), Phase::Down);
    }
    {
        // R6: a `repair` error exits 1 with its text.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        ws_state(&o, ID, "app", Phase::Stopping, vec![]);
        owner(t, "zygo-app", "abc");
        assert_eq!(run(&o, &h), (1, String::new(), format!("{AU63}\n")));
        assert_eq!(phase(&o, ID), Phase::Stopping);
        assert!(!tmux_log(t).iter().any(|l| l.contains("kill-session")));
    }
}

// ---- step 5 ----

/// The state of step 5 in `t`: member `KEY` with `SID`, a live launcher, `@zygo-ws` set.
fn one_member(t: &Path) -> (DownOpts, Store, Launcher) {
    let o = opts(t, &fake_tmux(t));
    let store = ws_state(&o, ID, "app", Phase::Up, vec![(KEY, member(Some(SID)))]);
    owner(t, "zygo-app", ID);
    let l = launcher(&o, SID);
    (o, store, l)
}

#[test]
fn idle_member_is_saved_and_stopped() {
    let _s = serial();
    let t = TempDir::new().unwrap();
    let t = t.path();
    let (o, store, mut l) = one_member(t);
    set_activity(&o, SID, false, &[]);
    let mut h = hooks();
    statuses(&mut h, status_map(vec![(SID, Ok(quiet()))]));
    release_on_sigterm(&mut h, &o, vec![l.lock.clone()], &[(ID, KEY)]);
    let sd = o.paths.sessions_dir.join(SID);
    begin(&sd, "main", "u-1", None);
    begin(&sd, "sub", "u-2", Some(5));
    begin(&sd, "x.y", "u-4", None);
    begin(&sd, "evil", "u\n3", None);
    let p = o.paths.clone();
    h.points.insert("after_draining", Box::new(move || snapshot::queue_pending(&p, ID, true).unwrap()));
    let p = o.paths.clone();
    h.points.insert(
        "after_stopping",
        Box::new(move || {
            snapshot::queue_pending(&p, ID, true).unwrap();
            // SAFETY: a signal to this process; the signal owner of `down` catches it.
            unsafe { libc::raise(libc::SIGTERM) };
        }),
    );
    let before = disps();
    let (code, out, err) = run(&o, &h);
    assert_eq!((code, out.as_str()), (0, "zygo: app is down (1 agents saved)\n"), "{err}");
    assert_eq!(count(&err, IDLE), 1);
    let ws = store.read().unwrap().unwrap();
    assert_eq!((ws.phase, ws.down_all), (Phase::Down, false));
    assert_eq!(ws.phase_since, 1_800_000_000); // round 2 item 10: the virtual clock, at 0 s
    let m = &ws.members[KEY];
    assert_eq!(
        (m.clean_stop, m.closed_units.clone(), m.interrupted.len(), m.state),
        (true, vec!["u-1".to_string()], 0, MemberState::Resumable)
    );
    assert_eq!(l.child.wait().unwrap().signal(), Some(libc::SIGTERM));
    assert!(tmux_log(t).iter().any(|c| c.ends_with("kill-session -t =zygo-app:")));
    assert_eq!(disps(), before); // restored once, when run returned
    assert!(!store.dir().join("snapshot.pending").exists());
    assert!(store.op_lock_free());
}

// ---- step 6 ----

#[test]
fn now_saves_running_subagents_and_reports_a_slow_agent() {
    let _s = serial();
    {
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store, mut l) = one_member(t);
        set_activity(&o, SID, true, &[("a2", ""), ("a1", "Explore")]);
        let mut h = hooks();
        let calls = statuses(&mut h, status_map(vec![]));
        o.now = true;
        let (code, out, err) = run(&o, &h);
        assert_eq!((code, out.as_str()), (0, "zygo: app is down (1 agents saved)\n"), "{err}");
        assert_eq!(count(&err, "zygo: app pane kaaaaaaaa: agent still running after 30 s; it stops with the session"), 1);
        let m = &store.read().unwrap().unwrap().members[KEY];
        assert_eq!(
            m.interrupted.iter().map(|s| (s.agent_id.as_str(), s.agent_type.as_str())).collect::<Vec<_>>(),
            [("a1", "Explore"), ("a2", "")]
        );
        assert_eq!((m.clean_stop, m.state), (true, MemberState::Running));
        assert!(calls.lock().unwrap().is_empty()); // --now: no drain, no status call
        assert_eq!(l.child.wait().unwrap().signal(), Some(libc::SIGTERM));
    }
    {
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![(KEY2, member(Some(SID2)))]);
        owner(t, "zygo-app", ID);
        let mut l2 = launcher(&o, SID2);
        fs::write(session_dir(&o, SID2).activity_json(), "{").unwrap();
        o.now = true;
        let (code, _, err) = run(&o, &hooks());
        assert_eq!(code, 0, "{err}");
        assert!(
            err.lines().any(|l| l.starts_with("zygo: app pane kbbbbbbbb: activity file unreadable: ")
                && l.ends_with("; its running subagents are not saved")),
            "{err}"
        );
        let m = &store.read().unwrap().unwrap().members[KEY2];
        assert!(m.interrupted.is_empty());
        assert!(m.clean_stop);
        assert_eq!(l2.child.wait().unwrap().signal(), Some(libc::SIGTERM));
    }
}

// ---- step 7 ----

#[test]
fn open_turn_is_busy_until_the_drain_timeout() {
    // FIX-20 (#81): an open turn (`busy` true, no running tool, a quiet proxy) is busy at every poll; no quiet time
    // counts it as idle.
    let _s = serial();
    {
        // (a) No Stop comes: at the drain timeout `down` exits 1, `phase` is up, and the launcher gets no SIGTERM.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store, mut l) = one_member(t);
        set_activity(&o, SID, true, &[]);
        let mut h = hooks();
        h.status = Some(Box::new(|_: &str| Ok(quiet())));
        o.timeout = Some(Duration::from_secs(120));
        let (code, out, err) = run(&o, &h);
        assert_eq!((code, out.as_str()), (1, ""), "{err}");
        assert_eq!(count(&err, TURN), 0);
        assert_eq!(count(&err, "zygo: app pane kaaaaaaaa: busy: no Stop since its last prompt"), 1, "{err}");
        assert_eq!(count(&err, SKIP), 0);
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Up);
        assert!(l.child.try_wait().unwrap().is_none()); // no SIGTERM
        l.child.kill().unwrap();
    }
    {
        // (b) A Stop written during the drain lets `down` go on and stop the workspace.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store, mut l) = one_member(t);
        set_activity(&o, SID, true, &[]);
        let mut h = hooks();
        h.status = Some(Box::new(|_: &str| Ok(quiet())));
        let last = Arc::new(Mutex::new(Duration::ZERO));
        let lp = last.clone();
        let o2 = o.clone();
        h.on_poll = Some(Box::new(move |_, t| {
            *lp.lock().unwrap() = t;
            if t == Duration::from_secs(90) {
                set_activity(&o2, SID, false, &[]);
            }
            false
        }));
        release_on_sigterm(&mut h, &o, vec![l.lock.clone()], &[(ID, KEY)]);
        o.timeout = Some(Duration::from_secs(600));
        let (code, out, err) = run(&o, &h);
        assert_eq!((code, out.as_str()), (0, "zygo: app is down (1 agents saved)\n"), "{err}");
        assert_eq!(count(&err, TURN), 0);
        assert!(*last.lock().unwrap() >= Duration::from_secs(90), "{:?}", *last.lock().unwrap());
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Down);
        l.child.wait().unwrap();
    }
}

#[test]
fn open_turn_has_no_fallback_before_a_long_drain_timeout() {
    // FIX-20 guard: no quiet time, however long, counts an open turn as idle. With a 3600 s drain timeout and no
    // Stop, `down` keeps polling to the timeout; a fallback at any earlier time would stop the member and exit 0.
    let _s = serial();
    let t = TempDir::new().unwrap();
    let t = t.path();
    let (mut o, store, mut l) = one_member(t);
    set_activity(&o, SID, true, &[]);
    let mut h = hooks();
    h.status = Some(Box::new(|_: &str| Ok(quiet())));
    let last = Arc::new(Mutex::new(Duration::ZERO));
    let lp = last.clone();
    h.on_poll = Some(Box::new(move |_, t| {
        *lp.lock().unwrap() = t;
        false
    }));
    o.timeout = Some(Duration::from_secs(3600));
    let (code, out, err) = run(&o, &h);
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    assert!(*last.lock().unwrap() >= Duration::from_secs(3600), "{:?}", *last.lock().unwrap());
    assert_eq!(count(&err, "zygo: app: drain timed out after 3600 s; phase is up again"), 1, "{err}");
    assert_eq!(count(&err, "zygo: app pane kaaaaaaaa: busy: no Stop since its last prompt"), 1, "{err}");
    assert_eq!(count(&err, TURN), 0);
    assert_eq!(count(&err, SKIP), 0);
    let ws = store.read().unwrap().unwrap();
    assert_eq!(ws.phase, Phase::Up);
    assert_eq!(ws.phase_since, 1_800_003_600); // the virtual clock at the timeout
    assert!(!ws.members[KEY].clean_stop);
    assert!(l.child.try_wait().unwrap().is_none()); // never signalled
    l.child.kill().unwrap();
}

// ---- step 8 ----

#[test]
fn busy_member_times_out() {
    let _s = serial();
    {
        // (a) #81 part 3: a member with a request in flight.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store, mut l) = one_member(t);
        set_activity(&o, SID, true, &[]);
        let mut h = hooks();
        h.status = Some(Box::new(|_: &str| Ok(in_flight())));
        let last = Arc::new(Mutex::new(Duration::ZERO));
        let lp = last.clone();
        h.on_poll = Some(Box::new(move |_, t| {
            *lp.lock().unwrap() = t;
            false
        }));
        o.timeout = Some(Duration::from_secs(70));
        let (code, out, err) = run(&o, &h);
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(count(&err, "zygo: app: drain timed out after 70 s; phase is up again"), 1);
        assert_eq!(count(&err, "zygo: app pane kaaaaaaaa: busy: model requests in flight: 1"), 1);
        assert_eq!(err.lines().last(), Some(NOW_LINE));
        assert_eq!(count(&err, TURN), 0);
        assert_eq!(*last.lock().unwrap(), Duration::from_secs(70));
        let ws = store.read().unwrap().unwrap();
        assert_eq!(ws.phase, Phase::Up);
        assert_eq!(ws.phase_since, 1_800_000_070); // round 2 item 10: the virtual clock at the timeout
        assert!(!ws.members[KEY].clean_stop);
        assert!(store.op_lock_free());
        assert!(l.child.try_wait().unwrap().is_none()); // no signal
        l.child.kill().unwrap();
    }
    {
        // (b) #47: a running subagent.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store, mut l) = one_member(t);
        set_activity(&o, SID, false, &[("a1", "Plan")]);
        let mut h = hooks();
        statuses(&mut h, status_map(vec![(SID, Ok(quiet()))]));
        o.timeout = Some(Duration::from_secs(2));
        let (code, _, err) = run(&o, &h);
        assert_eq!(code, 1, "{err}");
        assert_eq!(count(&err, "zygo: app pane kaaaaaaaa: busy: subagents running: \"a1\"; running subagents: Plan a1"), 1, "{err}");
        assert_eq!(err.lines().last(), Some(NOW_LINE));
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Up);
        assert!(store.op_lock_free());
        assert!(!tmux_log(t).iter().any(|l| l.contains("kill-session")));
        assert!(l.child.try_wait().unwrap().is_none());
        l.child.kill().unwrap();
    }
}

// ---- step 9 ----

#[test]
fn rule_skipped_waits_until_the_timeout() {
    let _s = serial();
    let t = TempDir::new().unwrap();
    let t = t.path();
    let mut o = opts(t, &fake_tmux(t));
    let store = ws_state(&o, ID, "app", Phase::Up, vec![(KEY, member(Some(SID))), (KEY2, member(Some(SID2)))]);
    owner(t, "zygo-app", ID);
    let mut l = launcher(&o, SID);
    let mut l2 = launcher(&o, SID2);
    set_activity(&o, SID, true, &[]);
    set_activity(&o, SID2, false, &[]);
    let mut h = hooks();
    statuses(
        &mut h,
        status_map(vec![
            (SID, Ok(SessionStatus { requests_in_flight: None, ..quiet() })),
            (SID2, Err("connection refused".into())),
        ]),
    );
    o.timeout = Some(Duration::from_secs(90));
    let (code, _, err) = run(&o, &h);
    assert_eq!(code, 1);
    assert_eq!(count(&err, SKIP), 1);
    assert_eq!(count(&err, TURN), 0);
    assert_eq!(count(&err, "zygo: app pane kaaaaaaaa: busy: no Stop since its last prompt"), 1);
    assert_eq!(count(&err, "zygo: app pane kbbbbbbbb: busy: proxy status not read: connection refused"), 1);
    assert_eq!(count(&err, "zygo: app: still running: pane kaaaaaaaa, pane kbbbbbbbb"), 3); // at 30, 60 and 90 s
    assert_eq!(store.read().unwrap().unwrap().phase, Phase::Up);
    l.child.kill().unwrap();
    l2.child.kill().unwrap();
}

// ---- step 10 ----

#[test]
fn dead_launcher_gets_no_signal_and_no_status() {
    let _s = serial();
    let t = TempDir::new().unwrap();
    let t = t.path();
    let o = opts(t, &fake_tmux(t));
    let store = ws_state(&o, ID, "app", Phase::Up, vec![(KEY, member(Some(SID))), (KEY2, member(Some(SID2)))]);
    owner(t, "zygo-app", ID);
    let mut dead = dead_launcher(&o, SID);
    let mut l2 = launcher(&o, SID2);
    set_activity(&o, SID2, false, &[]);
    let mut h = hooks();
    let calls = statuses(&mut h, status_map(vec![(SID2, Ok(quiet()))]));
    release_on_sigterm(&mut h, &o, vec![l2.lock.clone()], &[(ID, KEY2)]);
    let (code, out, err) = run(&o, &h);
    assert_eq!((code, out.as_str()), (0, "zygo: app is down (1 agents saved)\n"), "{err}");
    assert!(dead.try_wait().unwrap().is_none()); // no signal to the unrelated process
    assert!(calls.lock().unwrap().iter().all(|s| s == SID2)); // no status read for SID
    let ws = store.read().unwrap().unwrap();
    assert_eq!((ws.members[KEY].clean_stop, ws.members[KEY].state), (false, MemberState::Running));
    assert!(ws.members[KEY2].clean_stop);
    dead.kill().unwrap();
    dead.wait().unwrap();
    l2.child.wait().unwrap();
}

// ---- step 11 ----

const A: &str = "aaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbb";

/// `app` (id `A`) with member `KEY` on `SID`, and `bpp` (id `B`) with the members given; both owned.
fn two(o: &DownOpts, bpp_members: Vec<(&str, MemberRecord)>) {
    let t = tdir(o);
    ws_state(o, A, "app", Phase::Up, vec![(KEY, member(Some(SID)))]);
    ws_state(o, B, "bpp", Phase::Up, bpp_members);
    owner(&t, "zygo-app", A);
    owner(&t, "zygo-bpp", B);
}

fn phases(o: &DownOpts) -> (Phase, Phase) { (phase(o, B), phase(o, A)) }

#[test]
fn all_workspaces_drain_under_one_timeout() {
    let _s = serial();
    {
        // (a)
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &fake_tmux(t));
        o.target = DownTarget::All;
        two(&o, vec![]);
        let mut l = launcher(&o, SID);
        set_activity(&o, SID, false, &[]);
        let c = ws_state(&o, "cccccccccccccccc", "cpp", Phase::Up, vec![]);
        let _held = c.op_lock().unwrap();
        put_raw(&Store::open(&o.paths, "dddddddddddddddd"), b"{");
        ws_state(&o, "eeeeeeeeeeeeeeee", "epp", Phase::Up, vec![]);
        gone(t, "zygo-epp");
        let f = ws_state(&o, "ffffffffffffffff", "fpp", Phase::Down, vec![]);
        let f_before = raw(&f);
        let mut h = hooks();
        statuses(&mut h, status_map(vec![(SID, Ok(quiet()))]));
        release_on_sigterm(&mut h, &o, vec![l.lock.clone()], &[(A, KEY)]);
        let (code, out, err) = run(&o, &h);
        assert_eq!(code, 1, "{err}");
        // step 3 runs for every target first (epp is reported there); the stops follow in the first tick
        assert_eq!(out, "zygo: epp is not running\nzygo: app is down (1 agents saved)\nzygo: bpp is down (0 agents saved)\n", "{err}");
        assert_eq!(count(&err, "zygo: up or down is in progress for cpp"), 1);
        assert!(err.lines().any(|l| l.starts_with("zygo: skipped workspace dddddddddddddddd: ")), "{err}");
        for (id, ph, all) in [(A, Phase::Down, true), (B, Phase::Down, true), ("cccccccccccccccc", Phase::Up, false), ("eeeeeeeeeeeeeeee", Phase::Down, true)] {
            assert_eq!(phase_all(&o, id), (ph, all), "{id}");
        }
        assert_eq!(raw(&f), f_before);
        l.child.wait().unwrap();
    }
    {
        // (b) each workspace stops as soon as its own members are idle.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &fake_tmux(t));
        o.target = DownTarget::All;
        two(&o, vec![]);
        let mut l = launcher(&o, SID);
        set_activity(&o, SID, false, &[]);
        let mut h = hooks();
        let map = status_map(vec![(SID, Ok(in_flight()))]);
        statuses(&mut h, map.clone());
        release_on_sigterm(&mut h, &o, vec![l.lock.clone()], &[(A, KEY)]);
        let seen = Arc::new(Mutex::new(None));
        let (s2, o2) = (seen.clone(), o.clone());
        h.on_poll = Some(Box::new(move |n, _| {
            if n == 2 {
                *s2.lock().unwrap() = Some(phases(&o2));
            }
            if n == 4 {
                map.lock().unwrap().insert(SID.into(), Ok(quiet()));
            }
            false
        }));
        let (code, _, err) = run(&o, &h);
        assert_eq!(code, 0, "{err}");
        assert_eq!(*seen.lock().unwrap(), Some((Phase::Down, Phase::Draining)), "{err}");
        assert_eq!((phase_all(&o, A), phase_all(&o, B)), ((Phase::Down, true), (Phase::Down, true)));
        l.child.wait().unwrap();
    }
    {
        // (c) one timeout for every workspace still draining.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &fake_tmux(t));
        o.target = DownTarget::All;
        two(&o, vec![]);
        let mut l = launcher(&o, SID);
        set_activity(&o, SID, false, &[]);
        let mut h = hooks();
        statuses(&mut h, status_map(vec![(SID, Ok(in_flight()))]));
        o.timeout = Some(Duration::from_secs(5));
        let (code, _, err) = run(&o, &h);
        assert_eq!(code, 1, "{err}");
        assert_eq!((phase_all(&o, A), phase_all(&o, B)), ((Phase::Up, false), (Phase::Down, true)));
        l.child.kill().unwrap();
    }
    {
        // (d) R11: a stop wait does not hold up the timeout of another workspace.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &fake_tmux(t));
        o.target = DownTarget::All;
        two(&o, vec![(KEY2, member(Some(SID2)))]);
        let mut l = launcher(&o, SID);
        let mut l2 = launcher(&o, SID2);
        set_activity(&o, SID, false, &[]);
        set_activity(&o, SID2, false, &[]);
        let mut h = hooks();
        statuses(&mut h, status_map(vec![(SID, Ok(quiet())), (SID2, Ok(in_flight()))]));
        let seen = Arc::new(Mutex::new(None));
        let (s2, o2) = (seen.clone(), o.clone());
        h.on_poll = Some(Box::new(move |_, t| {
            if t == Duration::from_millis(5_500) {
                *s2.lock().unwrap() = Some(phases(&o2));
            }
            false
        }));
        o.timeout = Some(Duration::from_secs(5));
        let (code, _, err) = run(&o, &h);
        assert_eq!(code, 1, "{err}");
        assert_eq!(*seen.lock().unwrap(), Some((Phase::Up, Phase::Stopping)));
        assert_eq!((phase_all(&o, A), phase_all(&o, B)), ((Phase::Down, true), (Phase::Up, false)));
        assert_eq!(count(&err, "zygo: bpp: drain timed out after 5 s; phase is up again"), 1, "{err}");
        assert_eq!(count(&err, "zygo: app pane kaaaaaaaa: agent still running after 30 s; it stops with the session"), 1, "{err}");
        assert_eq!(l.child.wait().unwrap().signal(), Some(libc::SIGTERM));
        assert!(l2.child.try_wait().unwrap().is_none());
        l2.child.kill().unwrap();
    }
}

// ---- step 12 ----

#[test]
fn signals_cancel_drains_and_restore_once() {
    let _s = serial();
    {
        // (a) a signal (on_poll) during the drain.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (o, store, mut l) = one_member(t);
        set_activity(&o, SID, false, &[]);
        let mut h = hooks();
        statuses(&mut h, status_map(vec![(SID, Ok(in_flight()))]));
        h.on_poll = Some(Box::new(|n, _| n == 3));
        let before = disps();
        let (code, out, err) = run(&o, &h);
        assert_eq!(disps(), before);
        assert_eq!((code, out.as_str()), (130, ""), "{err}");
        assert_eq!(count(&err, "zygo: app: drain cancelled; phase is up again"), 1, "{err}");
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Up);
        assert!(store.op_lock_free());
        assert!(l.child.try_wait().unwrap().is_none());
        l.child.kill().unwrap();
    }
    {
        // (b) R2: with --all, a workspace that stopped stays down; the one that drains is cancelled.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let mut o = opts(t, &fake_tmux(t));
        o.target = DownTarget::All;
        two(&o, vec![(KEY2, member(Some(SID2)))]);
        let mut l = launcher(&o, SID);
        let mut l2 = launcher(&o, SID2);
        set_activity(&o, SID, false, &[]);
        set_activity(&o, SID2, false, &[]);
        let mut h = hooks();
        statuses(&mut h, status_map(vec![(SID, Ok(quiet())), (SID2, Ok(in_flight()))]));
        release_on_sigterm(&mut h, &o, vec![l.lock.clone()], &[(A, KEY)]);
        h.on_poll = Some(Box::new(|n, _| n == 2));
        let before = disps();
        let (code, out, err) = run(&o, &h);
        assert_eq!(disps(), before);
        assert_eq!(code, 130, "{err}");
        assert_eq!(phase_all(&o, A), (Phase::Down, true));
        assert_eq!(phase_all(&o, B), (Phase::Up, false));
        assert_eq!(count(&err, "zygo: bpp: drain cancelled; phase is up again"), 1, "{err}");
        assert_eq!(out, "zygo: app is down (1 agents saved)\n");
        l.child.wait().unwrap();
        assert!(l2.child.try_wait().unwrap().is_none());
        l2.child.kill().unwrap();
    }
    {
        // (c) R7: a real SIGINT during step 4.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (o, store, mut l) = one_member(t);
        set_activity(&o, SID, true, &[]);
        let mut h = hooks();
        statuses(&mut h, status_map(vec![(SID, Ok(in_flight()))]));
        // SAFETY: a signal to this process; the signal owner of `down` is installed and catches it.
        h.points.insert("before_final_snapshot", Box::new(|| unsafe { libc::raise(libc::SIGINT); }));
        let polled = Arc::new(Mutex::new(false));
        let p2 = polled.clone();
        h.on_poll = Some(Box::new(move |_, _| {
            *p2.lock().unwrap() = true;
            false
        }));
        let before = disps();
        let (code, out, err) = run(&o, &h);
        assert_eq!(disps(), before);
        assert_eq!((code, out.as_str()), (130, ""), "{err}");
        assert_eq!(count(&err, "zygo: app: drain cancelled; phase is up again"), 1, "{err}");
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Up);
        assert!(store.op_lock_free());
        assert!(!tmux_log(t).iter().any(|l| l.contains("kill-session")));
        assert!(!*polled.lock().unwrap());
        l.child.kill().unwrap();
    }
    {
        // (d) a `drain_timeout` that does not parse: one warning, 10 min.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, _store, mut l) = one_member(t);
        set_activity(&o, SID, false, &[]);
        o.config.workspace.drain_timeout = "ten".into();
        o.timeout = None;
        let mut h = hooks();
        statuses(&mut h, status_map(vec![(SID, Ok(in_flight()))]));
        let last = Arc::new(Mutex::new(Duration::ZERO));
        let lp = last.clone();
        h.on_poll = Some(Box::new(move |_, t| {
            *lp.lock().unwrap() = t;
            false
        }));
        let before = disps();
        let (code, _, err) = run(&o, &h);
        assert_eq!(disps(), before);
        assert_eq!(code, 1, "{err}");
        assert_eq!(count(&err, "zygo: [workspace] drain_timeout \"ten\" is not <n>s, <n>m or <n>h; using 10m"), 1);
        assert_eq!(*last.lock().unwrap(), Duration::from_secs(600));
        l.child.kill().unwrap();
    }
}

// ---- step 13 ----

#[test]
fn owner_changed_before_the_kill() {
    let _s = serial();
    let t = TempDir::new().unwrap();
    let t = t.path();
    let o = opts(t, &fake_tmux(t));
    let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
    owner(t, "zygo-app", ID);
    let mut h = hooks();
    let dir = t.to_path_buf();
    h.points.insert("before_kill_session", Box::new(move || owner(&dir, "zygo-app", "fedcba9876543210")));
    assert_eq!(run(&o, &h), (1, String::new(), format!("{AU63}\n")));
    assert!(!tmux_log(t).iter().any(|l| l.contains("kill-session")));
    assert_eq!(store.read().unwrap().unwrap().phase, Phase::Stopping);
    assert!(store.op_lock_free());
}

// ---- step 14 ----

/// A case of step 14 in `t`: a state in `up` with no member, owned; `zygo` is the recording stand-in.
fn detach_case(t: &Path) -> (DownOpts, Store) {
    let mut o = opts(t, &fake_tmux(t));
    o.zygo = rec(t);
    let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
    owner(t, "zygo-app", ID);
    (o, store)
}

const STARTED: &str = "zygo: down started; end your turn\n";

#[test]
fn detach_rule_guard_and_log() {
    let _s = serial();
    let h = hooks();
    {
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, store) = detach_case(t);
        o.zygo_session = Some("01JCALLER".into());
        assert_eq!(run(&o, &h), (0, STARTED.into(), String::new()));
        started(t);
        assert_eq!(args(t), "down --wait -- app\n");
        assert!(store.dir().join("down.log").exists());
    }
    {
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, _) = detach_case(t);
        o.zygo_session = Some("01JCALLER".into());
        o.now = true;
        o.timeout = Some(Duration::from_secs(90));
        assert_eq!(run(&o, &h), (0, STARTED.into(), String::new()));
        started(t);
        assert_eq!(args(t), "down --now --timeout 90s --wait -- app\n");
    }
    {
        // R3: a name that starts with `-` goes after `--`.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let mut o = opts(t, &fake_tmux(t));
        o.zygo = rec(t);
        ws_state(&o, ID, "-x", Phase::Up, vec![]);
        owner(t, "zygo--x", ID);
        o.target = DownTarget::Name("-x".into());
        o.zygo_session = Some("01JCALLER".into());
        assert_eq!(run(&o, &h), (0, STARTED.into(), String::new()));
        started(t);
        assert_eq!(args(t), "down --wait -- -x\n");
    }
    for ns in [ns_differs as down::ProcProbe, pidns_differs, comm_not_tmux, ns_unreadable, ns_self_unreadable] {
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, store) = detach_case(t);
        o.zygo_session = Some("01JCALLER".into());
        o.proc_probe = ns;
        let before = raw(&store);
        assert_eq!(run(&o, &h), (1, String::new(), format!("{AU18}\n")));
        assert!(!store.dir().join("down.log").exists());
        assert!(!t.join("child.pid").exists());
        assert_eq!(raw(&store), before);
    }
    {
        // R1, round 3 M2: a server that cannot be reached refuses with the M2 line.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, store) = detach_case(t);
        fs::write(t.join("nosrv"), "").unwrap();
        o.zygo_session = Some("01JCALLER".into());
        let before = raw(&store);
        let e = format!("error connecting to {}/sock/s (No such file or directory)", t.display());
        assert_eq!(run(&o, &h), (1, String::new(), unreachable_line("app", &e)));
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Up);
        assert_eq!(raw(&store), before);
        assert!(!store.dir().join("down.log").exists());
        assert!(!t.join("child.pid").exists());
        assert!(!store.dir().join("op.lock").exists());
    }
    {
        // The guard passes, the session is gone: foreground.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, store) = detach_case(t);
        gone(t, "zygo-app");
        o.zygo_session = Some("01JCALLER".into());
        assert_eq!(run(&o, &h), (0, "zygo: app is not running\n".into(), String::new()));
        assert!(!store.dir().join("down.log").exists());
        assert!(!t.join("child.pid").exists());
    }
    {
        // A workspace pane on the server of $TMUX.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, _) = detach_case(t);
        o.tmux_pane = Some("%3".into());
        o.tmux_env = Some(format!("{}/sock/s,1,0", t.display()).into());
        assert_eq!(run(&o, &h), (0, STARTED.into(), String::new()));
        started(t);
        assert_eq!(args(t), "down --wait -- app\n");
    }
    {
        // A pane value that is not a pane id is not looked up.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, _) = detach_case(t);
        o.tmux_pane = Some("%3;x".into());
        o.tmux_env = Some(format!("{}/sock/s,1,0", t.display()).into());
        assert_eq!(run(&o, &h), (0, "zygo: app is down (0 agents saved)\n".into(), String::new()));
        assert!(!tmux_log(t).iter().any(|l| l.contains("%3;x")));
    }
    {
        // A pane of another server never matches.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, _) = detach_case(t);
        o.tmux_pane = Some("%3".into());
        o.tmux_env = Some("/tmp/zygo-other/s,1,0".into());
        assert_eq!(run(&o, &h), (0, "zygo: app is down (0 agents saved)\n".into(), String::new()));
        assert!(!tmux_log(t).iter().any(|l| l.contains("-t %3")));
    }
    {
        // --wait forces the foreground.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, _) = detach_case(t);
        o.zygo_session = Some("01JCALLER".into());
        o.wait = true;
        assert_eq!(run(&o, &h), (0, "zygo: app is down (0 agents saved)\n".into(), String::new()));
        assert!(!t.join("child.pid").exists());
    }
    for held in [false, true] {
        // R10: the log is cut in place only while `op.lock` is free.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, store) = detach_case(t);
        o.zygo_session = Some("01JCALLER".into());
        let log = store.dir().join("down.log");
        let body: String = (0..7000).map(|i| format!("line {i:05}\n")).collect();
        assert_eq!(body.len(), 77_000);
        fs::write(&log, &body).unwrap();
        let ino = fs::metadata(&log).unwrap().ino();
        let lock = held.then(|| store.op_lock().unwrap());
        assert_eq!(run(&o, &h), (0, STARTED.into(), String::new()));
        let cut = fs::read_to_string(&log).unwrap();
        if held {
            assert_eq!(cut.len(), 77_000);
        } else {
            assert!(cut.len() <= 32_768, "{}", cut.len());
            assert!(cut.starts_with("line ") && cut.ends_with("line 06999\n"));
            assert_eq!(fs::metadata(&log).unwrap().ino(), ino); // cut in place, no rename
        }
        drop(lock);
        started(t);
    }
    {
        // --all.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, _) = detach_case(t);
        o.target = DownTarget::All;
        o.zygo_session = Some("01JCALLER".into());
        assert_eq!(run(&o, &h), (0, STARTED.into(), String::new()));
        started(t);
        assert_eq!(args(t), "down --wait --all\n");
        assert!(o.paths.state_dir().join("down-all.log").exists());
        assert!(o.paths.state_dir().starts_with(t));
    }
}

// ---- step 15 ----

/// The child process of `detached_child_holds_no_caller_descriptor`; it does nothing unless `ZYGO_W10_HELPER` is set.
/// After `down::down` returns, it puts `/dev/null` on its stdout and stderr (the caller's pipes), waits for EOF on
/// stdin, then ends its own child, the detached stand-in, and exits with the code of `down` (round 2 item 10: each
/// process signals only its own children).
#[test]
fn detach_helper() {
    let Some(d) = std::env::var_os("ZYGO_W10_HELPER") else { return };
    let d = PathBuf::from(d);
    let mut o = opts(&d, &d.join("tmux"));
    o.zygo = d.join("rec");
    o.zygo_session = Some("01JCALLER".into());
    let code = down::down(&o);
    let null = fs::OpenOptions::new().write(true).open("/dev/null").unwrap();
    // SAFETY: `dup2` of an open descriptor onto 1 and 2; nothing else writes to them from here on.
    unsafe {
        use std::os::fd::AsRawFd;
        libc::dup2(null.as_raw_fd(), 1);
        libc::dup2(null.as_raw_fd(), 2);
    }
    let mut rest = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut rest);
    if let Some(pid) = fs::read_to_string(d.join("child.pid")).ok().and_then(|s| s.trim().parse().ok()) {
        end_own_child(pid);
    }
    std::process::exit(code);
}

/// The helper process of step 15, the leader of its own process group: on drop it closes the helper's stdin (the
/// helper then ends its child and exits); a helper still running after 5 s is killed with its group.
struct Helper {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
    /// The folder where the detached stand-in reports its pid (`child.pid`).
    dir: PathBuf,
}

/// Round 3 M5: `pid` is the detached stand-in of the helper `parent`: on Linux, `/proc/<pid>/stat` names `parent` as
/// its parent and `pid` itself as its session leader (`setsid`). `false` when that cannot be read (other systems too).
fn own_grandchild(pid: u32, parent: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else { return false };
        let Some((_, rest)) = stat.rsplit_once(") ") else { return false };
        let f: Vec<&str> = rest.split(' ').collect();
        // After the command name: state, ppid, pgrp, session.
        f.len() > 3 && f[1] == parent.to_string() && f[3] == pid.to_string()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (pid, parent);
        false
    }
}

impl Helper {
    /// Closes stdin and waits (5 s bound) for the exit.
    fn finish(&mut self) -> Option<std::process::ExitStatus> {
        self.stdin = None;
        let t0 = Instant::now();
        loop {
            if let Ok(Some(s)) = self.child.try_wait() {
                return Some(s);
            }
            if t0.elapsed() >= S5 {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        // Round 3 M5: while the helper runs and holds its stdin open, it reaps nothing, so a stand-in that it started
        // keeps its pid (also as a zombie) and still names the helper as its parent; only then is it signalled. When
        // the check cannot be made, nothing is signalled and the stand-in ends by itself (`sleep 30`).
        if let Some(pid) = fs::read_to_string(self.dir.join("child.pid")).ok().and_then(|s| s.trim().parse::<u32>().ok())
            && self.stdin.is_some()
            && own_grandchild(pid, self.child.id())
        {
            // SAFETY: a plain `kill` of a process checked above as the stand-in of our own child.
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }
        if self.finish().is_none() {
            // SAFETY: the group of our own child, which it leads (`process_group(0)`).
            unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
            let _ = self.child.wait();
        }
    }
}

#[test]
fn detached_child_holds_no_caller_descriptor() {
    let _s = serial();
    let t = TempDir::new().unwrap();
    let d = t.path();
    let o = opts(d, &fake_tmux(d));
    let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
    owner(d, "zygo-app", ID);
    rec(d);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "detach_helper", "--nocapture", "--test-threads=1"])
        .env("ZYGO_W10_HELPER", d)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let (out, err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let stdin = child.stdin.take();
    let helper_pid = child.id();
    let mut helper = Helper { child, stdin, dir: d.to_path_buf() };
    let (tx, rx) = mpsc::channel::<String>();
    for mut pipe in [Box::new(out) as Box<dyn Read + Send>, Box::new(err)] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = pipe.read_to_string(&mut s);
            let _ = tx.send(s);
        });
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let a = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("first pipe at EOF before 5 s");
    let b = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).expect("second pipe at EOF before 5 s");
    assert!(a.contains("zygo: down started; end your turn") || b.contains("zygo: down started; end your turn"), "{a}{b}");
    let pid = started(d); // the detached child still runs: it lives 30 s, longer than the deadline
    #[cfg(target_os = "linux")]
    {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        let f: Vec<&str> = stat.rsplit_once(") ").unwrap().1.split(' ').collect();
        assert_eq!(f[3], pid.to_string()); // the session id field: setsid
        // Round 3 M5: the Drop check names the stand-in, and never this process or a child that is no session leader.
        assert!(own_grandchild(pid, helper_pid));
        assert!(!own_grandchild(std::process::id(), helper_pid));
        let sleeper = Sleeper(Command::new("sleep").arg("30").spawn().unwrap());
        assert!(!own_grandchild(sleeper.0.id(), std::process::id()));
        let fds: BTreeMap<String, PathBuf> = fs::read_dir(format!("/proc/{pid}/fd"))
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (e.file_name().into_string().unwrap(), fs::read_link(e.path()).unwrap())
            })
            .collect();
        assert_eq!(fds.keys().collect::<Vec<_>>(), ["0", "1", "2"], "{fds:?}");
        assert_eq!(fds["0"], Path::new("/dev/null"));
        assert_eq!((fds["1"].clone(), fds["2"].clone()), (store.dir().join("down.log"), store.dir().join("down.log")));
    }
    // The helper ends its own child, the detached stand-in, and exits with the code of `down`.
    let status = helper.finish().expect("helper exit before 5 s");
    assert!(status.success(), "{a}{b}");
    #[cfg(target_os = "linux")]
    assert!(wait_for(S5, || !Path::new(&format!("/proc/{pid}")).exists()), "detached stand-in still runs");
}

// ---- step 16 ----

#[test]
fn live_idle_down_stops_the_session() {
    let _s = serial();
    let Some(_) = require_tmux() else { return };
    let l = live();
    let o = live_opts(&l);
    assert!(wait_for(S5, || l
        .srv
        .run(&["display-message", "-p", "-t", "=zygo-app:", "#{pane_current_command}"])
        .is_ok_and(|c| c == "sleep")));
    let ws = l.store().read().unwrap().unwrap();
    assert_eq!(ws.panes.len(), 1);
    let key = ws.panes[0].key.clone();
    let (code, out, err) = run(&o, &hooks());
    assert_eq!((code, out.as_str()), (0, "zygo: app is down (0 agents saved)\n"), "{err}");
    assert_eq!(count(&err, &format!("zygo: app pane {key}: sleep is running; it stops with the session")), 1, "{err}");
    assert!(l.srv.run(&["has-session", "-t", "=zygo-app:"]).is_err());
    assert_eq!(l.store().read().unwrap().unwrap().phase, Phase::Down);
}

// ---- step 17 ----

#[test]
fn live_pane_key_refused_stops_nothing() {
    let _s = serial();
    let Some(_) = require_tmux() else { return };
    for bad in ["k$(x)", "#{pane_id}"] {
        let l = live();
        let pane = l.srv.run(&["split-window", "-d", "-P", "-F", "#{pane_id}", "-t", "=zygo-app:", "sleep", "600"]).unwrap();
        l.srv.run(&["set-option", "-p", "-t", &pane, "@zygo-pane", bad]).unwrap();
        let n = errors_lines(&l.store());
        let (code, out, err) = run(&live_opts(&l), &hooks());
        assert_eq!((code, out.as_str()), (1, ""), "{bad}: {err}");
        assert_eq!(err, format!("zygo: app: final snapshot refused (see {}/errors.log); nothing was stopped\n", l.store().dir().display()));
        assert_eq!(l.store().read().unwrap().unwrap().phase, Phase::Up);
        assert!(l.srv.run(&["has-session", "-t", "=zygo-app:"]).is_ok());
        assert_eq!(errors_lines(&l.store()), n + 1, "{bad}");
        assert!(l.store().op_lock_free());
    }
}

// ---- step 18 ----

/// The real identity, with the mount namespace changed for a process whose comm starts with `tmux`.
#[cfg(target_os = "linux")]
fn ns_server_differs(p: &str) -> Option<ProcIdentity> {
    let mut i = down::proc_identity(p)?;
    if p != "self" && i.comm.as_deref().is_some_and(|c| c.starts_with("tmux")) {
        i.mnt_ns = "mnt:[other]".into();
    }
    Some(i)
}

#[cfg(target_os = "linux")]
#[test]
fn live_guard_reads_the_server_pid() {
    let _s = serial();
    let Some(_) = require_tmux() else { return };
    let l = live();
    let _g = Detached::new(l.t.path());
    let mut o = live_opts(&l);
    o.zygo_session = Some("01JCALLER".into());
    o.zygo = rec(l.t.path());
    o.proc_probe = ns_server_differs;
    assert_eq!(run(&o, &hooks()), (1, String::new(), format!("{AU18}\n")));
    o.proc_probe = down::proc_identity;
    assert_eq!(run(&o, &hooks()), (0, STARTED.into(), String::new()));
    started(l.t.path());
    assert_eq!(args(l.t.path()), "down --wait -- app\n");
    l.srv.kill_server();
    // Round 3 M2: a server that is gone cannot be reached; the tmux error text depends on the tmux version.
    let (code, out, err) = run(&o, &hooks());
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    let tail = unreachable_line("app", "\u{0}");
    let (head, tail) = tail.split_once('\u{0}').unwrap();
    assert!(err.starts_with(head) && err.ends_with(tail) && err.lines().count() == 1, "{err}");
    assert_eq!(l.store().read().unwrap().unwrap().phase, Phase::Up);
}

/// Round 2 item 7: `list-sessions -F '#{pid}'` gives the server pid, whatever session exists; checked on the tmux of
/// `PATH` (3.7c here, 3.2a with the scratch build first on `PATH`).
#[cfg(target_os = "linux")]
#[test]
fn live_list_sessions_gives_the_server_pid() {
    let _s = serial();
    let Some(_) = require_tmux() else { return };
    let srv = PrivateServer::start();
    srv.run(&["new-session", "-d", "-s", "zygo-one", "sleep", "100"]).unwrap();
    srv.run(&["new-session", "-d", "-s", "zygo-two", "sleep", "100"]).unwrap();
    let first = |s: String| s.lines().next().unwrap_or_default().to_string();
    let pid = first(srv.run(&["list-sessions", "-F", "#{pid}"]).unwrap());
    assert_eq!(srv.run(&["display-message", "-p", "-t", "=zygo-two:", "#{pid}"]).unwrap(), pid);
    let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap();
    assert!(comm.starts_with("tmux"), "{comm:?}");
    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap();
    assert!(String::from_utf8_lossy(&cmdline).contains(&srv.socket().display().to_string()));
    srv.run(&["kill-session", "-t", "=zygo-one"]).unwrap();
    assert_eq!(first(srv.run(&["list-sessions", "-F", "#{pid}"]).unwrap()), pid);
}

// ---- round 2 ----

#[test] // item 1: a tool call of 70 s and more, with no model request, never counts as idle
fn a_long_tool_call_is_never_idle() {
    let _s = serial();
    let t = TempDir::new().unwrap();
    let t = t.path();
    let (mut o, store, mut l) = one_member(t);
    set_activity(&o, SID, true, &[]);
    set_tools(&o, SID, 1);
    let mut h = hooks();
    statuses(&mut h, status_map(vec![(SID, Ok(quiet()))]));
    o.timeout = Some(Duration::from_secs(90));
    let (code, out, err) = run(&o, &h);
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    assert_eq!(count(&err, TURN), 0, "{err}");
    assert_eq!(count(&err, "zygo: app pane kaaaaaaaa: busy: tool running"), 1, "{err}");
    assert_eq!(store.read().unwrap().unwrap().phase, Phase::Up);
    assert!(l.child.try_wait().unwrap().is_none()); // no signal
    l.child.kill().unwrap();
}

#[test] // item 2: each failure path gives its exit code and line, and leaves a known state
fn failure_paths_report_and_leave_state() {
    let _s = serial();
    for (f, line) in [(disk_full(), "zygo: app: disk full\n"), (Fault::Busy, "zygo: workspace app is busy; try again\n")] {
        // The `draining` write fails: nothing changed.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
        owner(t, "zygo-app", ID);
        let before = raw(&store);
        let mut h = hooks();
        fault(&mut h, "draining", f);
        assert_eq!(run(&o, &h), (1, String::new(), line.into()));
        assert_eq!(raw(&store), before);
        assert!(store.op_lock_free());
        assert!(!tmux_log(t).iter().any(|l| l.contains("kill-session")));
    }
    {
        // The `phase = down` write of a gone session fails.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
        gone(t, "zygo-app");
        let mut h = hooks();
        fault(&mut h, "gone", disk_full());
        assert_eq!(run(&o, &h), (1, String::new(), "zygo: app: disk full\n".into()));
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Up);
        assert!(store.op_lock_free());
    }
    {
        // The step 8 write fails: the stop goes on; the member is not marked as a clean stop.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (o, store, mut l) = one_member(t);
        set_activity(&o, SID, false, &[]);
        let mut h = hooks();
        statuses(&mut h, status_map(vec![(SID, Ok(quiet()))]));
        release_on_sigterm(&mut h, &o, vec![l.lock.clone()], &[(ID, KEY)]);
        fault(&mut h, "members", disk_full());
        let (code, out, err) = run(&o, &h);
        assert_eq!((code, out.as_str()), (0, "zygo: app is down (0 agents saved)\n"), "{err}");
        assert_eq!(count(&err, "zygo: app: member records not saved: disk full"), 1, "{err}");
        let ws = store.read().unwrap().unwrap();
        assert_eq!((ws.phase, ws.members[KEY].clean_stop), (Phase::Down, false));
        assert_eq!(l.child.wait().unwrap().signal(), Some(libc::SIGTERM));
    }
    {
        // A revert write fails (at the timeout): the phase stays `draining`, `op.lock` is released.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store, mut l) = one_member(t);
        set_activity(&o, SID, false, &[]);
        let mut h = hooks();
        statuses(&mut h, status_map(vec![(SID, Ok(in_flight()))]));
        fault(&mut h, "revert", disk_full());
        o.timeout = Some(Duration::from_secs(2));
        let (code, _, err) = run(&o, &h);
        assert_eq!(code, 1, "{err}");
        let lines: Vec<&str> = err.lines().collect();
        assert_eq!(&lines[..2], ["zygo: app: phase not restored: disk full", "zygo: app: drain timed out after 2 s; phase is up again"], "{err}");
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Draining);
        assert!(store.op_lock_free());
        l.child.kill().unwrap();
    }
    {
        // The `phase = down` write fails after the kill: the phase stays `stopping` (AU75 completes it).
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
        owner(t, "zygo-app", ID);
        let mut h = hooks();
        fault(&mut h, "down", disk_full());
        assert_eq!(run(&o, &h), (1, String::new(), "zygo: app: phase not written: disk full\n".into()));
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Stopping);
        assert!(tmux_log(t).iter().any(|c| c.ends_with("kill-session -t =zygo-app:")));
        assert!(store.op_lock_free());
    }
    for (pid, why) in [("x", "pid file holds no process id: x"), ("1", "pid file holds 1, which no launcher has")] {
        // The launcher is not signalled: the stop wait reports it after 30 s; the workspace still stops.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (o, store, mut l) = one_member(t);
        set_activity(&o, SID, false, &[]);
        fs::write(o.paths.sessions_dir.join(SID).join("pid"), pid).unwrap();
        let mut h = hooks();
        statuses(&mut h, status_map(vec![(SID, Ok(quiet()))]));
        let (code, out, err) = run(&o, &h);
        assert_eq!((code, out.as_str()), (0, "zygo: app is down (1 agents saved)\n"), "{err}");
        assert_eq!(count(&err, &format!("zygo: app pane kaaaaaaaa: launcher not signalled: {why}")), 1, "{err}");
        assert_eq!(count(&err, "zygo: app pane kaaaaaaaa: agent still running after 30 s; it stops with the session"), 1);
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Down);
        assert!(l.child.try_wait().unwrap().is_none()); // no signal went anywhere
        l.child.kill().unwrap();
    }
    {
        // AU62: a saved socket whose folder is not private; no tmux call names it.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
        owner(t, "zygo-app", ID);
        unprivate(t);
        let before = raw(&store);
        assert_eq!(run(&o, &hooks()), (1, String::new(), not_private_line(t)));
        assert_eq!(raw(&store), before);
        assert!(!tmux_log(t).iter().any(|l| l.contains("-S")), "{:?}", tmux_log(t));
        assert!(store.op_lock_free());
    }
    {
        // The step 4 snapshot returns `Err`: one line, the stop goes on.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
        owner(t, "zygo-app", ID);
        let mut h = hooks();
        fault(&mut h, "snapshot", Fault::Error("cannot set snapshot.pending".into()));
        assert_eq!(
            run(&o, &h),
            (0, "zygo: app is down (0 agents saved)\n".into(), "zygo: final snapshot not saved: cannot set snapshot.pending\n".into())
        );
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Down);
    }
}

#[test] // items 2 and 3: the `stopping` write fails; the fallback `phase = up` write
fn stopping_write_failure_and_its_fallback() {
    let _s = serial();
    let setup = |t: &Path| -> (DownOpts, Store, Hooks) {
        let o = opts(t, &fake_tmux(t));
        let store = ws_state(&o, ID, "app", Phase::Up, vec![]);
        owner(t, "zygo-app", ID);
        let mut h = hooks();
        let p = o.paths.clone();
        h.points.insert("after_draining", Box::new(move || snapshot::queue_pending(&p, ID, true).unwrap()));
        (o, store, h)
    };
    {
        // Busy: the D18 busy line; the fallback succeeds and runs the pending check.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (o, store, mut h) = setup(t);
        fault(&mut h, "stopping", Fault::Busy);
        assert_eq!(run(&o, &h), (1, String::new(), "zygo: workspace app is busy; try again\n".into()));
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Up);
        assert!(!store.dir().join("snapshot.pending").exists()); // `after_write` of the fallback took it
        assert!(store.op_lock_free());
        assert!(!tmux_log(t).iter().any(|l| l.contains("kill-session")));
    }
    {
        // Another error: `zygo: <name>: <e>`; the fallback fails too and says so.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (o, store, mut h) = setup(t);
        fault(&mut h, "stopping", disk_full());
        fault(&mut h, "stopping_fallback", disk_full());
        assert_eq!(run(&o, &h), (1, String::new(), "zygo: app: disk full\nzygo: app: phase not restored: disk full\n".into()));
        assert_eq!(store.read().unwrap().unwrap().phase, Phase::Draining);
        assert!(store.dir().join("snapshot.pending").exists()); // no write ran, so no pending check
        assert!(store.op_lock_free());
    }
}

#[test] // items 4, 5, 6, 7, 8, 11 and the detach failure paths of item 2
fn detach_round_2() {
    let _s = serial();
    let h = hooks();
    let caller = |o: &mut DownOpts| o.zygo_session = Some("01JCALLER".into());
    for (body, expect) in [
        // Item 4: one long unterminated line of 3-byte characters: the last 32 KiB from a character boundary.
        ("€".repeat(14_000), "€".repeat(10_922)),
        // One long line that ends with a newline: never cut to empty.
        (format!("{}\n", "x".repeat(40_000)), format!("{}\n", "x".repeat(32_767))),
        // A line that starts exactly at the window edge is kept whole.
        (format!("{}\n{}\n", "a".repeat(10_000), "b".repeat(32_767)), format!("{}\n", "b".repeat(32_767))),
    ] {
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, store) = detach_case(t);
        caller(&mut o);
        let log = store.dir().join("down.log");
        fs::write(&log, &body).unwrap();
        assert_eq!(run(&o, &h), (0, STARTED.into(), String::new()));
        let cut = fs::read_to_string(&log).unwrap();
        assert!(!cut.is_empty() && cut.len() <= 32_768 && body.ends_with(&cut), "{}", cut.len());
        assert_eq!(cut, expect);
        started(t);
    }
    {
        // Item 5: a state with no `tmux_socket` (the default server) and the caller in its session.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, store) = detach_case(t);
        no_socket(&store);
        o.tmux_pane = Some("%3".into());
        o.tmux_env = Some("/tmp/zygo-w10-none/default,1,0".into());
        assert_eq!(run(&o, &h), (0, STARTED.into(), String::new()));
        started(t);
        assert_eq!(args(t), "down --wait -- app\n");
    }
    for (probe, name) in [(pidns_differs as down::ProcProbe, "pid ns"), (comm_not_tmux, "comm")] {
        // Item 6: the pid namespace and the comm of the server.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store) = detach_case(t);
        caller(&mut o);
        o.proc_probe = probe;
        assert_eq!(run(&o, &h), (1, String::new(), format!("{AU18}\n")), "{name}");
        assert!(!store.dir().join("down.log").exists());
    }
    #[cfg(target_os = "linux")]
    {
        // Item 6 with the real probe: the server pid is a `sleep` child of this test (same namespaces).
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, _) = detach_case(t);
        caller(&mut o);
        let sleeper = Sleeper(Command::new("sleep").arg("30").spawn().unwrap());
        fs::write(t.join("server.pid"), sleeper.0.id().to_string()).unwrap();
        o.proc_probe = down::proc_identity;
        assert_eq!(run(&o, &h), (1, String::new(), format!("{AU18}\n")));
        assert!(!t.join("child.pid").exists());
    }
    {
        // Item 7: with --all, a stale first target does not decide the server pid.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let mut o = opts(t, &fake_tmux(t));
        o.zygo = rec(t);
        o.target = DownTarget::All;
        caller(&mut o);
        two(&o, vec![]);
        gone(t, "zygo-app");
        assert_eq!(run(&o, &h), (0, STARTED.into(), String::new()));
        started(t);
        assert_eq!(args(t), "down --wait --all\n");
        assert!(tmux_log(t).iter().any(|l| l.contains("list-sessions -F #{pid}")));
    }
    for pane in [false, true] {
        // Item 8: AU62 before any tmux call on the saved socket, in the detach path too.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store) = detach_case(t);
        if pane {
            o.tmux_pane = Some("%3".into());
            o.tmux_env = Some(format!("{}/sock/s,1,0", t.display()).into());
        } else {
            caller(&mut o);
        }
        unprivate(t);
        assert_eq!(run(&o, &h), (1, String::new(), not_private_line(t)), "pane {pane}");
        assert!(!tmux_log(t).iter().any(|l| l.contains("-S")), "{:?}", tmux_log(t));
        assert!(!store.dir().join("down.log").exists());
    }
    {
        // Item 11: a relative zygo path is made absolute against the cwd before the start.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let (mut o, _) = detach_case(t);
        caller(&mut o);
        o.zygo = PathBuf::from("./rec");
        o.cwd = t.to_path_buf();
        assert_eq!(run(&o, &h), (0, STARTED.into(), String::new()));
        started(t);
        assert_eq!(args(t), "down --wait -- app\n");
    }
    {
        // Item 11: AU54 before the detached start.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store) = detach_case(t);
        caller(&mut o);
        o.zygo = script(t, "re'c", "exit 0");
        assert_eq!(run(&o, &h), (2, String::new(), format!("{AU54}\n")));
        assert!(!store.dir().join("down.log").exists());
    }
    {
        // Item 2: the log cannot be opened (a symlink; O_NOFOLLOW).
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store) = detach_case(t);
        caller(&mut o);
        let log = store.dir().join("down.log");
        std::os::unix::fs::symlink(t.join("elsewhere"), &log).unwrap();
        let before = raw(&store);
        let (code, out, err) = run(&o, &h);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(err.starts_with(&format!("zygo: cannot open {}: ", log.display())) && err.lines().count() == 1, "{err}");
        assert!(!t.join("elsewhere").exists() && !t.join("child.pid").exists());
        assert_eq!(raw(&store), before);
    }
    {
        // Item 2: the detached child cannot start.
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store) = detach_case(t);
        caller(&mut o);
        o.zygo = t.join("missing");
        let before = raw(&store);
        let (code, out, err) = run(&o, &h);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(err.starts_with("zygo: cannot start the detached down: ") && err.lines().count() == 1, "{err}");
        assert_eq!(raw(&store), before);
    }
}

#[test] // round 3 B1: a tool that Esc ended fires no hook (P-N24-1): busy until the drain timeout, reverted, nothing stopped
fn an_escaped_tool_stays_busy_until_the_timeout() {
    let _s = serial();
    for busy in [true, false] {
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store, mut l) = one_member(t);
        set_activity(&o, SID, busy, &[]);
        set_tools(&o, SID, 1);
        let mut h = hooks();
        statuses(&mut h, status_map(vec![(SID, Ok(quiet()))]));
        o.timeout = Some(Duration::from_secs(120));
        let (code, out, err) = run(&o, &h);
        assert_eq!((code, out.as_str()), (1, ""), "busy {busy}: {err}");
        assert_eq!(count(&err, TURN), 0, "{err}");
        assert_eq!(count(&err, IDLE), 0, "{err}");
        assert_eq!(count(&err, "zygo: app: drain timed out after 120 s; phase is up again"), 1, "{err}");
        assert_eq!(count(&err, "zygo: app pane kaaaaaaaa: busy: tool running"), 1, "{err}");
        assert_eq!(count(&err, NOW_LINE), 1, "{err}");
        let ws = store.read().unwrap().unwrap();
        assert_eq!((ws.phase, ws.members[KEY].clean_stop), (Phase::Up, false));
        assert!(!tmux_log(t).iter().any(|c| c.contains("kill-session")));
        assert!(l.child.try_wait().unwrap().is_none()); // no signal
        l.child.kill().unwrap();
    }
}

#[test] // round 3 M2: a server that cannot be reached; with --all that target is skipped and the others go on
fn unreachable_server_refuses_or_is_skipped() {
    let _s = serial();
    let h = hooks();
    {
        let t = TempDir::new().unwrap();
        let t = t.path();
        let (mut o, store) = detach_case(t);
        o.zygo_session = Some("01JCALLER".into());
        fs::write(t.join("dead-s"), "").unwrap();
        let before = raw(&store);
        let e = format!("no server running on {}/sock/s", t.display());
        assert_eq!(run(&o, &h), (1, String::new(), unreachable_line("app", &e)));
        assert_eq!(raw(&store), before);
        assert!(!store.dir().join("down.log").exists() && !t.join("child.pid").exists());
        assert!(store.op_lock_free());
    }
    {
        let t = TempDir::new().unwrap();
        let t = t.path();
        let _g = Detached::new(t);
        let mut o = opts(t, &fake_tmux(t));
        o.zygo = rec(t);
        o.target = DownTarget::All;
        o.zygo_session = Some("01JCALLER".into());
        two(&o, vec![]);
        let bpp = Store::open(&o.paths, B);
        bpp.update(S5, &mut |ws| {
            ws.tmux_socket = Some(t.join("sock/s2"));
            Ok(())
        })
        .unwrap();
        fs::write(t.join("dead-s2"), "").unwrap();
        let before = raw(&bpp);
        let e = format!("no server running on {}/sock/s2", t.display());
        assert_eq!(run(&o, &h), (1, STARTED.into(), unreachable_line("bpp", &e)));
        started(t);
        assert_eq!(args(t), "down --wait --all\n");
        assert_eq!(raw(&bpp), before);
    }
}

#[test] // round 3: AU54 checks the zygo path after it is joined to the cwd
fn relative_zygo_path_in_an_unsafe_cwd_is_refused() {
    let _s = serial();
    let t = TempDir::new().unwrap();
    let t = t.path();
    let (mut o, store) = detach_case(t);
    o.zygo_session = Some("01JCALLER".into());
    let cwd = t.join("we'ird");
    fs::create_dir(&cwd).unwrap();
    fs::copy(t.join("rec"), cwd.join("rec")).unwrap();
    o.cwd = cwd;
    o.zygo = PathBuf::from("./rec");
    assert_eq!(run(&o, &hooks()), (2, String::new(), format!("{AU54}\n")));
    assert!(!store.dir().join("down.log").exists() && !t.join("child.pid").exists());
}

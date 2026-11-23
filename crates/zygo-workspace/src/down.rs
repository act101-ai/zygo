//! `zygo down`: detach, drain loop, timeout, stop, `--now`, `--all`. Owner: node W10 (W§9.3).
//!
//! The library reads no process environment and no device: `$TMUX`, `$TMUX_PANE`, `ZYGO_SESSION`, the target and the
//! mount-namespace probe come in through [`DownOpts`]; only the entry point (M1) reads the real values. Only
//! `zygo: <name> is down (<n> agents saved)`, `zygo: <name> is not running`, `zygo: no workspace is up` and
//! `zygo: down started; end your turn` go to `out`; every other line goes to `err` (D1). Nothing here calls
//! `process::exit`.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use zygo_config::{parse_duration, Config};
use zygo_core::esc;
use zygo_core::paths::Paths;
use zygo_core::session_dir::{self, SessionDir};
use zygo_proxy::client::{SessionClient, SessionStatus};

use crate::activity::{self, Activity};
use crate::drain::{self, Idle};
use crate::identity::{valid_pane_key, valid_ws_id};
use crate::mux::tmux::Tmux;
use crate::mux::{check_path, parse_pane_line, Multiplexer, MuxError, SessionAnswer, MIN_VERSION};
use crate::repair::{check_saved_socket, repair};
use crate::snapshot::{self, SkipReason, SnapshotOutcome, OP_WAIT};
use crate::state::{list_all, MemberRecord, MemberState, OpLock, Phase, StateError, Store, Subagent, Workspace};
use crate::up::{self, not_owned, preflight, socket_is_private, unreachable, Exit};

/// What the AU18 guard compares for a process: its mount and pid namespaces and its command name (round 2 item 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcIdentity {
    /// The link text of `/proc/<pid>/ns/mnt` (`mnt:[<inode>]`).
    pub mnt_ns: String,
    /// The link text of `/proc/<pid>/ns/pid` (`pid:[<inode>]`).
    pub pid_ns: String,
    /// `/proc/<pid>/comm` without its newline; `None` on a system where it is not read (the guard then does not
    /// check it).
    pub comm: Option<String>,
}

/// The process probe of the AU18 guard: `self` or a process id to its identity, `None` when it cannot be read (D10,
/// round 2 item 6).
pub type ProcProbe = fn(&str) -> Option<ProcIdentity>;

/// The workspace that `down` acts on (review round 1, M1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownTarget {
    /// `zygo down <name>`.
    Name(String),
    /// `zygo down` with no name: the workspace of the cwd checkout (W§9.3 step 1); no environment value overrides it.
    Cwd,
    /// `/zygo:down`: the workspace id of the session view marker; `None` when the session has no marker.
    Session(Option<String>),
    /// `zygo down --all`.
    All,
}

/// The inputs of `zygo down` (W§9.3). `tmux_env` is `$TMUX`, `tmux_pane` is `$TMUX_PANE` and `zygo_session` is
/// `ZYGO_SESSION`; only the entry point reads them from the process.
#[derive(Debug, Clone)]
pub struct DownOpts {
    pub target: DownTarget,
    pub now: bool,
    pub timeout: Option<Duration>,
    pub wait: bool,
    pub cwd: PathBuf,
    pub zygo: PathBuf,
    pub tmux: PathBuf,
    pub tmux_env: Option<OsString>,
    pub tmux_pane: Option<OsString>,
    pub zygo_session: Option<OsString>,
    pub proc_probe: ProcProbe,
    pub paths: Paths,
    pub config: Config,
}

/// The drain poll (W§9.3 step 5).
const POLL: Duration = Duration::from_millis(500);
/// The time between two progress lines of one workspace (W§9.3 step 5).
const PROGRESS_EVERY: Duration = Duration::from_secs(30);
/// The stop wait of step 10 (W§9.3 step 10, W§19).
const STOP_WAIT: Duration = Duration::from_secs(30);
/// The bound of one proxy status call (W§9.3 step 5).
const STATUS_BOUND: Duration = Duration::from_secs(2);
/// The drain timeout when `drain_timeout` does not parse (D16).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);
/// The log is cut to its newest whole lines within this size (AU97, D9).
const LOG_KEEP: u64 = 32 * 1024;
/// The most bytes read from a launcher's `pid` file (D21).
const PID_MAX_BYTES: u64 = 32;
/// Pane commands that are shells: step 9 does not warn about them (AU24, D20).
const SHELLS: [&str; 13] = ["sh", "bash", "zsh", "fish", "dash", "ksh", "mksh", "tcsh", "csh", "ash", "nu", "elvish", "xonsh"];
const WS_OPTION: &str = "@zygo-ws";

const NO_TMUX: &str = "zygo: workspaces need tmux; install tmux 3.2 or later";
const NEWER: &str = "zygo: workspace state is from a newer zygo; run zygo update";
const AU18: &str =
    "zygo: down cannot run detached from a sandboxed shell; use the workspace_down tool ([features] workspace_tool) or /zygo:down";
const NOT_IN_WORKSPACE: &str = "zygo: this session is not in a workspace";
const NO_WORKSPACE_UP: &str = "zygo: no workspace is up";
const STARTED: &str = "zygo: down started; end your turn";
const NOW_HINT: &str = "zygo: run zygo down --now to stop at once";
const TURN_OPEN: &str = "no Stop since its last prompt";

/// `self` or 1 to 10 ASCII digits (D10).
fn valid_pid_text(pid: &str) -> bool {
    pid == "self" || ((1..=10).contains(&pid.len()) && pid.bytes().all(|b| b.is_ascii_digit()))
}

/// The identity of `pid` (`self` or 1 to 10 ASCII digits; any other value gives `None`, so no value names another
/// path). Linux: the link texts of `/proc/<pid>/ns/mnt` and `/proc/<pid>/ns/pid` and `/proc/<pid>/comm`, `None` when
/// any of them cannot be read. Other systems: empty namespaces and no comm for every valid `pid`, so the guard passes
/// there (D10).
pub fn proc_identity(pid: &str) -> Option<ProcIdentity> {
    if !valid_pid_text(pid) {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        let link = |ns: &str| fs::read_link(format!("/proc/{pid}/ns/{ns}")).ok().map(|p| p.to_string_lossy().into_owned());
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        Some(ProcIdentity { mnt_ns: link("mnt")?, pid_ns: link("pid")?, comm: Some(comm.trim_end_matches('\n').to_string()) })
    }
    #[cfg(not(target_os = "linux"))]
    {
        Some(ProcIdentity { mnt_ns: String::new(), pid_ns: String::new(), comm: None })
    }
}

/// Time, the test hooks and the proxy status replacement (D24). [`Real`] is the production form.
trait Probe {
    fn now(&self) -> SystemTime;
    fn elapsed(&self) -> Duration;
    fn sleep(&self, d: Duration);
    fn poll(&self, n: u32, t: Duration) -> bool;
    fn status(&self, sid: &str) -> Option<Result<SessionStatus, String>>;
    fn fire(&self, point: &str);
    /// The fault set for the write or step `name` (round 2 item 2); `None` lets it run.
    fn fault(&self, name: &str) -> Option<StateError>;
}

/// The real clock and no hook.
struct Real {
    start: Instant,
}

impl Probe for Real {
    fn now(&self) -> SystemTime { SystemTime::now() }

    fn elapsed(&self) -> Duration { self.start.elapsed() }

    fn sleep(&self, d: Duration) { std::thread::sleep(d) }

    fn poll(&self, _n: u32, _t: Duration) -> bool { false }

    fn status(&self, _sid: &str) -> Option<Result<SessionStatus, String>> { None }

    fn fire(&self, _point: &str) {}

    fn fault(&self, _name: &str) -> Option<StateError> { None }
}

/// Test hooks (D24). Only the `test-hooks` feature builds them.
#[cfg(feature = "test-hooks")]
pub mod testing {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::io::Write;
    use std::time::{Duration, SystemTime};

    use zygo_proxy::client::SessionStatus;

    use super::{DownOpts, Probe};
    use crate::state::StateError;

    /// The hooks of one or more `run_with` calls. Each call starts a new virtual clock at `start`: `sleep` adds to
    /// the virtual time and returns at once, so no state of one run reaches the next.
    #[allow(clippy::type_complexity)]
    pub struct Hooks {
        /// The wall time at virtual time 0.
        pub start: SystemTime,
        /// Runs at the start of each poll tick with the tick number (from 0) and the time since the first tick;
        /// `true` acts as a SIGINT.
        pub on_poll: Option<Box<dyn Fn(u32, Duration) -> bool>>,
        /// Replaces the proxy status call; the argument is the session id.
        pub status: Option<Box<dyn Fn(&str) -> Result<SessionStatus, String>>>,
        /// Closures run at the points `after_draining`, `before_final_snapshot`, `after_stopping`, `after_sigterm`
        /// and `before_kill_session`.
        pub points: BTreeMap<&'static str, Box<dyn Fn()>>,
        /// Faults by name (round 2 item 2): `Some` makes that write or step fail with it before it runs. Names:
        /// `draining`, `gone` (the `phase = down` write of a gone session), `stopping`, `stopping_fallback`,
        /// `members` (step 8), `down` (step 11), `revert` (timeout, cancel and refused snapshot), `snapshot` (step 4).
        pub faults: BTreeMap<&'static str, Box<dyn Fn() -> Option<Fault>>>,
    }

    /// A fault of [`Hooks::faults`].
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Fault {
        /// The `lock` of `workspace.json` was not taken within its bound (`StateError::Busy`).
        Busy,
        /// Any other error, with its text.
        Error(String),
    }

    impl Hooks {
        /// No hook, no point, no fault.
        pub fn new(start: SystemTime) -> Hooks {
            Hooks { start, on_poll: None, status: None, points: BTreeMap::new(), faults: BTreeMap::new() }
        }
    }

    /// The virtual clock of one run.
    struct Virtual<'a> {
        hooks: &'a Hooks,
        t: Cell<Duration>,
    }

    impl Probe for Virtual<'_> {
        fn now(&self) -> SystemTime { self.hooks.start + self.t.get() }

        fn elapsed(&self) -> Duration { self.t.get() }

        fn sleep(&self, d: Duration) { self.t.set(self.t.get() + d) }

        fn poll(&self, n: u32, t: Duration) -> bool { self.hooks.on_poll.as_ref().is_some_and(|f| f(n, t)) }

        fn status(&self, sid: &str) -> Option<Result<SessionStatus, String>> { self.hooks.status.as_ref().map(|f| f(sid)) }

        fn fire(&self, point: &str) {
            if let Some(f) = self.hooks.points.get(point) {
                f();
            }
        }

        fn fault(&self, name: &str) -> Option<StateError> {
            self.hooks.faults.get(name).and_then(|f| f()).map(|f| match f {
                Fault::Busy => StateError::Busy,
                Fault::Error(text) => StateError::Io(std::io::Error::other(text)),
            })
        }
    }

    /// [`super::run`] with the hooks and a new virtual clock.
    pub fn run_with(opts: &DownOpts, hooks: &Hooks, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
        super::run_inner(opts, &Virtual { hooks, t: Cell::new(Duration::ZERO) }, out, err)
    }
}

fn say(w: &mut dyn Write, line: &str) { let _ = writeln!(w, "{line}"); }

/// Unix seconds of `t`, for `phase_since` (round 2 item 10: the probe clock, so tests use the virtual clock).
fn secs(t: SystemTime) -> u64 { t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) }

/// A failed write of `workspace.json` (D18, round 2 item 3).
enum WriteError {
    /// `lock` was not taken within its bound.
    Busy,
    /// Any other error, with its text.
    Other(String),
}

impl WriteError {
    fn from_state(e: StateError) -> WriteError {
        match e {
            StateError::Busy => WriteError::Busy,
            e => WriteError::Other(e.to_string()),
        }
    }

    /// The error inside a longer line: `workspace <name> is busy; try again` or the error text.
    fn text(&self, name: &str) -> String {
        match self {
            WriteError::Busy => format!("workspace {name} is busy; try again"),
            WriteError::Other(t) => t.clone(),
        }
    }

    /// The line of a write that ends the workspace: the D18 busy line, else `zygo: <name>: <e>`.
    fn line(&self, name: &str) -> String {
        match self {
            WriteError::Busy => format!("zygo: workspace {name} is busy; try again"),
            WriteError::Other(t) => format!("zygo: {name}: {t}"),
        }
    }
}

/// `<name> pane <key>`: both are checked at each state read (AU84), so the label needs no escape (D2).
fn label(name: &str, key: &str) -> String { format!("{name} pane {key}") }

// ---- the signal owner (D17) ----

/// The first SIGINT, SIGHUP or SIGTERM since the install; 0 for none.
static PENDING: AtomicI32 = AtomicI32::new(0);

#[cfg(unix)]
const SIGINT: i32 = libc::SIGINT;
#[cfg(not(unix))]
const SIGINT: i32 = 2;

#[cfg(unix)]
extern "C" fn on_signal(sig: libc::c_int) {
    // Async-signal-safe: one atomic compare-exchange; the first signal wins.
    let _ = PENDING.compare_exchange(0, sig, Ordering::SeqCst, Ordering::SeqCst);
}

/// The one signal owner of a `run` (D17): installed once before the first `draining` write; `Drop` restores the three
/// previous actions in reverse order, once.
struct Signals {
    #[cfg(unix)]
    saved: Vec<(libc::c_int, libc::sigaction)>,
}

impl Signals {
    fn install() -> Signals {
        PENDING.store(0, Ordering::SeqCst);
        #[cfg(unix)]
        {
            let mut saved = Vec::new();
            for sig in [libc::SIGINT, libc::SIGHUP, libc::SIGTERM] {
                // SAFETY: plain `sigaction` calls with zero-initialized structs; the handler only does an atomic store.
                unsafe {
                    let mut new: libc::sigaction = std::mem::zeroed();
                    new.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
                    new.sa_flags = libc::SA_RESTART;
                    libc::sigemptyset(&mut new.sa_mask);
                    let mut old: libc::sigaction = std::mem::zeroed();
                    if libc::sigaction(sig, &new, &mut old) == 0 {
                        saved.push((sig, old));
                    }
                }
            }
            Signals { saved }
        }
        #[cfg(not(unix))]
        {
            Signals {}
        }
    }

    fn pending(&self) -> Option<i32> {
        match PENDING.load(Ordering::SeqCst) {
            0 => None,
            s => Some(s),
        }
    }

    /// `on_poll` returned `true`: acts as a SIGINT.
    fn raise_for_test(&self) { let _ = PENDING.compare_exchange(0, SIGINT, Ordering::SeqCst, Ordering::SeqCst); }
}

impl Drop for Signals {
    fn drop(&mut self) {
        #[cfg(unix)]
        for (sig, old) in self.saved.iter().rev() {
            // SAFETY: restores the action that `install` saved.
            unsafe {
                libc::sigaction(*sig, old, std::ptr::null_mut());
            }
        }
    }
}

// ---- resolution (D4, D5) ----

/// The tmux version check of W§8.1 step 2, with the texts of `up::preflight` (D5).
fn tmux_check(tmux: &Path) -> Result<(), Exit> {
    let usage = |message: String| Exit { code: 2, message };
    match Tmux::new(tmux.to_path_buf(), None).version() {
        Ok(v) if v >= MIN_VERSION => Ok(()),
        Ok((major, minor)) => Err(usage(format!("zygo: tmux {major}.{minor} is too old; workspaces need tmux 3.2 or later"))),
        Err(MuxError::Spawn { .. }) => Err(usage(NO_TMUX.into())),
        Err(e) => Err(usage(format!("zygo: cannot read the tmux version: {e}"))),
    }
}

/// The targets and the base exit code (1 when `--all` skipped a state), or the exit code of a `down` that ends here
/// (its lines are printed).
fn resolve(opts: &DownOpts, out: &mut dyn Write, err: &mut dyn Write) -> Result<(Vec<Workspace>, i32), i32> {
    let name: Option<String> = match &opts.target {
        DownTarget::All => return resolve_all(opts, out, err),
        DownTarget::Session(None) => {
            say(err, NOT_IN_WORKSPACE);
            return Err(1);
        }
        DownTarget::Session(Some(id)) => {
            if !valid_ws_id(id) {
                say(err, &format!("zygo: invalid workspace id {}", esc(id)));
                return Err(1);
            }
            match Store::open(&opts.paths, id).read() {
                Ok(Some(ws)) => Some(ws.name),
                Ok(None) => {
                    say(err, &format!("zygo: no workspace with id {id}"));
                    return Err(1);
                }
                Err(StateError::Newer { .. }) => {
                    say(err, NEWER);
                    return Err(1);
                }
                Err(e) => {
                    say(err, &format!("zygo: {e}"));
                    return Err(1);
                }
            }
        }
        DownTarget::Name(n) => Some(n.clone()),
        DownTarget::Cwd => None,
    };
    let t = preflight(&opts.paths, &opts.tmux, name.as_deref(), &opts.cwd).map_err(|e| {
        say(err, &e.message);
        e.code
    })?;
    match t.state {
        Err(text) => {
            say(err, &format!("zygo: {text}"));
            Err(1)
        }
        Ok(None) => {
            say(out, &format!("zygo: {} is not running", t.name));
            Err(0)
        }
        Ok(Some(ws)) => Ok((vec![ws], 0)),
    }
}

/// `--all` (D5, AU80): the tmux check, then each readable state whose phase is not `down`, in id order.
fn resolve_all(opts: &DownOpts, out: &mut dyn Write, err: &mut dyn Write) -> Result<(Vec<Workspace>, i32), i32> {
    tmux_check(&opts.tmux).map_err(|e| {
        say(err, &e.message);
        e.code
    })?;
    let mut found = Vec::new();
    let mut code = 0;
    for (id, r) in list_all(&opts.paths) {
        match r {
            Ok(ws) if ws.phase != Phase::Down => found.push(ws),
            Ok(_) => {}
            Err(e) => {
                say(err, &format!("zygo: skipped workspace {id}: {e}"));
                code = 1;
            }
        }
    }
    if found.is_empty() {
        say(out, NO_WORKSPACE_UP);
        return Err(code);
    }
    Ok((found, code))
}

/// The drain timeout (D16): `--timeout`, else `drain_timeout`, else 10 min with one warning.
fn timeout(opts: &DownOpts, err: &mut dyn Write) -> Duration {
    if let Some(t) = opts.timeout {
        return t;
    }
    let text = &opts.config.workspace.drain_timeout;
    parse_duration(text).unwrap_or_else(|| {
        say(err, &format!("zygo: [workspace] drain_timeout \"{}\" is not <n>s, <n>m or <n>h; using 10m", esc(text)));
        DEFAULT_TIMEOUT
    })
}

// ---- detach (D6-D9) ----

/// `%` and 1 to 10 ASCII digits; any other value is not a pane id and is never looked up.
fn pane_id(v: &OsStr) -> Option<&str> {
    let s = v.to_str()?;
    let digits = s.strip_prefix('%')?;
    ((1..=10).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())).then_some(s)
}

/// AU62 before any tmux call on a saved socket in the detach path (round 2 item 8): `Err` with the AU62 line when the
/// socket's folder exists and fails the check. A folder that does not exist holds no server, so the tmux call that
/// follows answers for itself; nothing is created here.
fn check_socket(ws: &Workspace) -> Result<(), Exit> {
    let Some(sock) = ws.tmux_socket.as_deref() else {
        return Ok(());
    };
    let folder_missing = sock
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .is_some_and(|p| matches!(fs::symlink_metadata(p), Err(e) if e.kind() == std::io::ErrorKind::NotFound));
    if folder_missing {
        return Ok(());
    }
    socket_is_private(sock, false).map_err(|t| Exit { code: 1, message: format!("zygo: {t}") })
}

/// D6: `--wait` forces the foreground; else `ZYGO_SESSION`, or a `$TMUX_PANE` in the session of any target (round 2
/// item 5): a target with a saved socket is looked up only when that socket is the socket of `$TMUX`, after the AU62
/// check; a target with no saved socket is on the server that `up::server` gives it (the `$TMUX` server, else
/// tmux's default). A tmux error counts as "not in that session". `Err` is the AU62 refusal.
fn detach_wanted(opts: &DownOpts, targets: &[Workspace]) -> Result<bool, Exit> {
    if opts.wait {
        return Ok(false);
    }
    if opts.zygo_session.as_ref().is_some_and(|s| !s.is_empty()) {
        return Ok(true);
    }
    let Some(pane) = opts.tmux_pane.as_deref().and_then(pane_id) else {
        return Ok(false);
    };
    let env_server = up::server(&opts.tmux, None, opts.tmux_env.as_deref());
    for ws in targets {
        if let Some(sock) = ws.tmux_socket.as_deref() {
            if env_server.socket() != Some(sock) {
                continue;
            }
            check_socket(ws)?;
        }
        let mux = up::server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref());
        if mux.display(pane, "#{session_name}").is_ok_and(|s| s == ws.tmux_session) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The pid of the server of `mux` (round 2 item 7): the first line of `list-sessions -F '#{pid}'` (one line per
/// session, each the server pid, on tmux 3.2a and 3.7c), so no one target session decides it (empty when there is
/// no line). `Err` holds the tmux error: the server cannot be reached (round 3, M2).
fn server_pid(opts: &DownOpts, mux: &Tmux) -> Result<String, String> {
    let mut c = Command::new(&opts.tmux);
    if let Some(sock) = mux.socket() {
        c.arg("-S").arg(sock);
    }
    c.arg("-u").args(["list-sessions", "-F", "#{pid}"]).env("LC_ALL", "C");
    let ran = crate::mux::tmux::run_bounded(c, crate::mux::BOUND).map_err(|e| e.to_string())?;
    let out = crate::mux::tmux::finish(ran).map_err(|e| e.to_string())?;
    Ok(out.lines().next().unwrap_or_default().to_string())
}

/// The line of a target whose tmux server cannot be reached (round 3, M2).
fn unreachable_server(name: &str, error: &str) -> String {
    format!(
        "zygo: cannot reach the tmux server of {name} ({error}); in a sandboxed shell, run zygo down outside it; if that server is gone, run zygo down --wait -- {name} from a shell outside the workspace"
    )
}

/// D7, AU18 (round 2 items 6-8, round 3 M2): for each distinct server, the AU62 check of its saved socket, then its
/// pid. A server that cannot be reached refuses a single target with the M2 line (exit 1); with `--all`, each of its
/// targets gets that line and is left out. Each server reached must share the mount and the pid namespace of this
/// process, and its command name must start with `tmux` when it is read; a pid that is not a number, or an identity
/// that cannot be read or differs, refuses with the AU18 line (exit 1). `Ok` holds the targets on servers reached
/// and the lines of the targets left out.
fn guard(opts: &DownOpts, targets: &[Workspace]) -> Result<(Vec<Workspace>, Vec<String>), Exit> {
    let refuse = || Exit { code: 1, message: AU18.into() };
    let mut seen: Vec<(Option<&Path>, Result<(), String>)> = Vec::new();
    let mut pids: Vec<String> = Vec::new();
    for ws in targets {
        let socket = ws.tmux_socket.as_deref();
        if seen.iter().any(|(s, _)| *s == socket) {
            continue;
        }
        check_socket(ws)?;
        let mux = up::server(&opts.tmux, socket, opts.tmux_env.as_deref());
        match server_pid(opts, &mux) {
            Ok(pid) if pid == "self" || !valid_pid_text(&pid) => return Err(refuse()),
            Ok(pid) => {
                pids.push(pid);
                seen.push((socket, Ok(())));
            }
            Err(e) if opts.target != DownTarget::All => {
                return Err(Exit { code: 1, message: unreachable_server(&ws.name, &e) });
            }
            Err(e) => seen.push((socket, Err(e))),
        }
    }
    if !pids.is_empty() {
        let own = (opts.proc_probe)("self").ok_or_else(refuse)?;
        for pid in &pids {
            let server = (opts.proc_probe)(pid).ok_or_else(refuse)?;
            let not_tmux = server.comm.as_deref().is_some_and(|c| !c.starts_with("tmux"));
            if server.mnt_ns != own.mnt_ns || server.pid_ns != own.pid_ns || not_tmux {
                return Err(refuse());
            }
        }
    }
    let mut reached = Vec::new();
    let mut lines = Vec::new();
    for ws in targets {
        match seen.iter().find(|(s, _)| *s == ws.tmux_socket.as_deref()).map(|(_, r)| r) {
            Some(Err(e)) => lines.push(unreachable_server(&ws.name, e)),
            _ => reached.push(ws.clone()),
        }
    }
    Ok((reached, lines))
}

/// D6: whether any target session exists (no lock), each saved socket checked first (AU62, round 2 item 8); a tmux
/// error other than "gone" ends `down`.
fn any_session(opts: &DownOpts, targets: &[Workspace]) -> Result<bool, Exit> {
    let mut any = false;
    for ws in targets {
        check_socket(ws)?;
        match up::server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref()).session_exists(&ws.tmux_session) {
            SessionAnswer::Exists => any = true,
            SessionAnswer::Gone => {}
            SessionAnswer::Error { message } => return Err(Exit { code: 1, message: format!("zygo: tmux: {message}") }),
        }
    }
    Ok(any)
}

/// Open options of a log: no symlink is followed, and a new file has mode 0600 (D8).
fn log_options() -> fs::OpenOptions {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut o = fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    o
}

/// D9, AU97: with `op.lock` of every target taken, a log larger than [`LOG_KEEP`] is cut in place to its newest whole
/// lines within that size, through one handle (the file keeps its inode). When the window holds no line start (round
/// 2 item 4), the window is kept as it is from its first UTF-8 character boundary, so a log with content is never
/// cut to empty. With any lock held, nothing is cut. The locks are released before the return, so the detached
/// process takes `op.lock` itself.
fn cut_log(opts: &DownOpts, targets: &[Workspace], log: &Path) -> std::io::Result<()> {
    let mut locks: Vec<OpLock> = Vec::new();
    for ws in targets {
        match Store::open(&opts.paths, &ws.id).op_lock() {
            Ok(l) => locks.push(l),
            Err(_) => return Ok(()),
        }
    }
    let mut f = match log_options().read(true).write(true).open(log) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let len = f.metadata()?.len();
    if len <= LOG_KEEP {
        return Ok(());
    }
    // One byte before the kept window, so a line that starts exactly at the window is kept whole.
    f.seek(SeekFrom::Start(len - LOG_KEEP - 1))?;
    let mut buf = Vec::new();
    (&mut f).take(LOG_KEEP + 1).read_to_end(&mut buf)?;
    let keep = cut_point(&buf);
    f.set_len(0)?;
    f.seek(SeekFrom::Start(0))?;
    f.write_all(keep)?;
    drop(locks);
    Ok(())
}

/// The kept part of `buf`, the last [`LOG_KEEP`] bytes of a log and the byte before them: after the first `\n` that is
/// not the last byte, else the window (`buf` without its first byte) from its first byte that is not a UTF-8
/// continuation byte, skipping at most 3 (round 2 item 4).
fn cut_point(buf: &[u8]) -> &[u8] {
    let body = &buf[..buf.len().saturating_sub(1)];
    if let Some(p) = body.iter().position(|&b| b == b'\n') {
        return &buf[p + 1..];
    }
    let window = buf.get(1..).unwrap_or_default();
    let skip = window.iter().take(3).take_while(|&&b| b & 0xC0 == 0x80).count();
    &window[skip..]
}

/// D8: `<zygo> down [--now] [--timeout <n>s] --wait (--all | -- <name>)`, a new session leader with its cwd `/`,
/// stdin `/dev/null`, stdout and stderr the log. The parent does not wait for it.
fn spawn_detached(opts: &DownOpts, zygo: &Path, name: Option<&str>, log: File) -> std::io::Result<()> {
    let mut c = Command::new(zygo);
    c.arg("down");
    if opts.now {
        c.arg("--now");
    }
    if let Some(t) = opts.timeout {
        c.arg("--timeout").arg(format!("{}s", t.as_secs()));
    }
    c.arg("--wait");
    match name {
        None => c.arg("--all"),
        Some(n) => c.arg("--").arg(n),
    };
    c.current_dir("/").stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: `setsid` is async-signal-safe and touches no memory of the parent.
        unsafe {
            c.pre_exec(|| if libc::setsid() == -1 { Err(std::io::Error::last_os_error()) } else { Ok(()) });
        }
    }
    c.spawn().map(drop)
}

/// W§9.3 step 2 after the guard: the zygo path made absolute against the cwd (the child runs in `/`) and the AU54
/// check of that joined path (round 2 item 11, round 3), cut the log, start the detached `down`, print the line.
fn detach(opts: &DownOpts, targets: &[Workspace], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let zygo = if opts.zygo.is_absolute() { opts.zygo.clone() } else { opts.cwd.join(&opts.zygo) };
    if let Err(e) = check_path(&zygo) {
        say(err, &e.to_string());
        return 2;
    }
    let all = opts.target == DownTarget::All;
    let log = if all {
        opts.paths.state_dir().join("down-all.log")
    } else {
        Store::open(&opts.paths, &targets[0].id).dir().join("down.log")
    };
    // A cut that fails leaves the log as it is; the append open below reports a log that cannot be used.
    let _ = cut_log(opts, targets, &log);
    let file = match log_options().append(true).create(true).open(&log) {
        Ok(f) => f,
        Err(e) => {
            say(err, &format!("zygo: cannot open {}: {e}", log.display()));
            return 1;
        }
    };
    let name = if all { None } else { Some(targets[0].name.as_str()) };
    if let Err(e) = spawn_detached(opts, &zygo, name, file) {
        say(err, &format!("zygo: cannot start the detached down: {e}"));
        return 1;
    }
    say(out, STARTED);
    0
}

// ---- one workspace ----

/// The signal mode of a workspace (D17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Drain,
    Stopping,
}

/// Where a workspace is in the poll loop (D14).
#[derive(Debug)]
enum State {
    /// Steps 3-4 run.
    Preparing,
    /// Step 5; `since` is the clock time at the end of its step 4.
    Draining { since: Duration },
    /// Step 10's wait for the members signalled (key to session folder).
    StopWait { since: Duration, keys: BTreeMap<String, PathBuf> },
    Done,
}

/// The drain record of one member (D15).
#[derive(Debug, Default)]
struct Track {
    said_skip: bool,
    said_idle: bool,
    /// The result and the running set of the last poll that read the member.
    last: Option<(Idle, BTreeMap<String, String>)>,
}

struct Job {
    id: String,
    name: String,
    store: Store,
    ws: Workspace,
    op: Option<OpLock>,
    mode: Mode,
    state: State,
    track: BTreeMap<String, Track>,
    saved: usize,
    next_progress: Duration,
}

/// The live session folder of a member (D14): `sessions/<pending.zygo_session>` when live, else
/// `sessions/<zygo_session>` when live; an id that is empty or not ASCII alphanumeric is not looked up.
fn live_session(paths: &Paths, m: &MemberRecord) -> Option<PathBuf> {
    let ok = |id: &&str| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric());
    let pending = m.pending.as_ref().map(|p| p.zygo_session.as_str());
    pending
        .into_iter()
        .chain(m.zygo_session.as_deref())
        .filter(ok)
        .map(|id| paths.sessions_dir.join(id))
        .find(|d| session_dir::live(d))
}

/// D19, AU31: the open work units of the work folder, sorted, without duplicates.
fn closed_units(work: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(work) else {
        return Vec::new();
    };
    let mut units = Vec::new();
    for e in entries.flatten() {
        let Ok(file) = e.file_name().into_string() else {
            continue;
        };
        let Some(agent) = file.strip_suffix(".begin.json") else {
            continue;
        };
        let valid = (1..=64).contains(&agent.len()) && agent.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if !valid {
            continue;
        }
        if let Some(b) = zygo_work::log::open_unit(work, agent)
            && b.stopped_at.is_none()
            && !b.unit.is_empty()
            && !b.unit.chars().any(char::is_control)
        {
            units.push(b.unit);
        }
    }
    units.sort();
    units.dedup();
    units
}

/// The name after the last `/`, without a leading `-` (a login shell), is one of [`SHELLS`] (D20).
fn is_shell(command: &str) -> bool {
    let base = command.rsplit('/').next().unwrap_or(command);
    SHELLS.contains(&base.strip_prefix('-').unwrap_or(base))
}

/// D21: SIGTERM to the launcher whose pid is in `<dir>/pid`; a value that is not a number or is 1 or less is refused.
fn sigterm(dir: &Path) -> Result<(), String> {
    let mut text = String::new();
    File::open(dir.join(session_dir::PID_FILE))
        .and_then(|f| f.take(PID_MAX_BYTES).read_to_string(&mut text))
        .map_err(|e| e.to_string())?;
    let pid: i32 = text.trim().parse().map_err(|_| format!("pid file holds no process id: {}", esc(text.trim())))?;
    if pid <= 1 {
        return Err(format!("pid file holds {pid}, which no launcher has"));
    }
    #[cfg(unix)]
    {
        // SAFETY: a plain `kill`; `pid` is above 1, so no process group and no broadcast is named.
        if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        Err("signals need unix".into())
    }
}

/// One `run`: its writers, its jobs and its exit state.
struct Run<'a> {
    opts: &'a DownOpts,
    probe: &'a dyn Probe,
    signals: Signals,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
    jobs: Vec<Job>,
    code: i32,
    cancelled: Option<i32>,
    timed_out: bool,
    clients: BTreeMap<PathBuf, SessionClient>,
}

impl Run<'_> {
    fn e(&mut self, line: &str) { say(self.err, line); }

    fn o(&mut self, line: &str) { say(self.out, line); }

    fn fail(&mut self, code: i32) { self.code = self.code.max(code); }

    fn all(&self) -> bool { self.opts.target == DownTarget::All }

    /// The `snapshot.pending` check after a write (D13).
    fn after_write(&mut self, id: &str, socket: Option<PathBuf>) {
        let opts = self.opts;
        if let Err(e) = snapshot::after_write(&opts.paths, &Tmux::new(opts.tmux.clone(), socket), id, OP_WAIT) {
            self.e(&format!("zygo: final snapshot not saved: {e:#}"));
        }
    }

    /// `phase_since` for a write now: Unix seconds of the probe clock (round 2 item 10).
    fn now_secs(&self) -> u64 { secs(self.probe.now()) }

    /// One write of `store` under `lock` (5 s), then the pending check with the state it wrote (D13). The fault
    /// `fault` (tests only) fails it before it runs, so nothing is written and no pending check runs.
    fn update(&mut self, store: &Store, fault: &str, f: &mut dyn FnMut(&mut Workspace)) -> Result<Workspace, WriteError> {
        if let Some(e) = self.probe.fault(fault) {
            return Err(WriteError::from_state(e));
        }
        let ws = store
            .update(OP_WAIT, &mut |c| {
                f(c);
                Ok(())
            })
            .map_err(WriteError::from_state)?;
        self.after_write(&ws.id.clone(), ws.tmux_socket.clone());
        Ok(ws)
    }

    /// [`Run::update`] of the state of job `i`; the job keeps the state written.
    fn write(&mut self, i: usize, fault: &str, f: &mut dyn FnMut(&mut Workspace)) -> Result<(), WriteError> {
        let store = self.jobs[i].store.clone();
        let ws = self.update(&store, fault, f)?;
        self.jobs[i].ws = ws;
        Ok(())
    }

    /// Ends a job: its `op.lock` is released.
    fn done(&mut self, i: usize) {
        self.jobs[i].op = None;
        self.jobs[i].state = State::Done;
    }

    /// `phase = up` (a failed write prints the "phase not restored" line), release, then `line` (D3).
    fn revert(&mut self, i: usize, line: &str) {
        self.restore(i, "revert");
        self.done(i);
        self.e(line);
    }

    /// The `phase = up` write of a revert or of the step 7 fallback (D18); a failure prints
    /// `zygo: <name>: phase not restored: <e>`. A write that succeeds runs the pending check.
    fn restore(&mut self, i: usize, fault: &str) {
        let now = self.now_secs();
        if let Err(e) = self.write(i, fault, &mut |c| {
            c.phase = Phase::Up;
            c.phase_since = now;
        }) {
            let name = self.jobs[i].name.clone();
            self.e(&format!("zygo: {name}: phase not restored: {}", e.text(&name)));
        }
    }

    /// D17: a pending signal cancels every workspace still in `drain` mode.
    fn check_signal(&mut self) {
        let Some(sig) = self.signals.pending() else {
            return;
        };
        let mut any = false;
        for i in 0..self.jobs.len() {
            if self.jobs[i].mode == Mode::Drain && !matches!(self.jobs[i].state, State::Done) {
                let line = format!("zygo: {}: drain cancelled; phase is up again", self.jobs[i].name);
                self.revert(i, &line);
                any = true;
            }
        }
        if any {
            self.cancelled.get_or_insert(sig);
        }
    }

    fn read_state(&self, store: &Store) -> Result<Option<Workspace>, String> {
        match store.read() {
            Ok(s) => Ok(s),
            Err(StateError::Newer { .. }) => Err(NEWER.into()),
            Err(e) => Err(format!("zygo: {e}")),
        }
    }

    /// Steps 3-4 for one target (D11, D12). A refusal prints its line and sets the exit code; a workspace that is not
    /// running prints its line and leaves no job.
    fn prepare(&mut self, target: &Workspace) {
        let opts = self.opts;
        let (id, name) = (target.id.clone(), target.name.clone());
        if let Err(e) = check_path(&opts.zygo) {
            self.e(&e.to_string());
            return self.fail(2);
        }
        let store = Store::open(&opts.paths, &id);
        let op = match store.op_lock() {
            Ok(l) => l,
            Err(StateError::OpHeld) => {
                self.e(&format!("zygo: up or down is in progress for {name}"));
                return self.fail(1);
            }
            Err(e) => {
                self.e(&format!("zygo: {e}"));
                return self.fail(1);
            }
        };
        // The state again under `op.lock` (W7 I1).
        let ws = match self.read_state(&store) {
            Ok(Some(ws)) => ws,
            Ok(None) => return self.o(&format!("zygo: {name} is not running")),
            Err(t) => {
                self.e(&t);
                return self.fail(1);
            }
        };
        // FIX-29 round 2: the AU54 check of a saved socket that the repair uses, before any use (as `up`).
        if let Err(e) = check_saved_socket(&ws) {
            self.e(&e.to_string());
            return self.fail(2);
        }
        if let Some(sock) = &ws.tmux_socket
            && let Err(t) = socket_is_private(sock, true)
        {
            self.e(&format!("zygo: {t}"));
            return self.fail(1);
        }
        let mux = up::server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref());
        if let Err(e) = repair(&store, &mux, &opts.zygo, &mut *self.out, &mut *self.err) {
            self.e(&format!("zygo: {e:#}"));
            return self.fail(1);
        }
        let ws = match self.read_state(&store) {
            Ok(Some(ws)) => ws,
            Ok(None) => return self.o(&format!("zygo: {name} is not running")),
            Err(t) => {
                self.e(&t);
                return self.fail(1);
            }
        };
        self.after_write(&id, ws.tmux_socket.clone());
        let mux = up::server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref());
        match mux.session_exists(&ws.tmux_session) {
            SessionAnswer::Gone => {
                if let Some(key) = store.live_launcher(&ws) {
                    self.e(&format!("zygo: {}", unreachable(&name, &key, mux.socket())));
                    return self.fail(1);
                }
                if ws.phase != Phase::Down {
                    let (all, now) = (self.all(), self.now_secs());
                    if let Err(e) = self.update(&store, "gone", &mut |c| {
                        c.phase = Phase::Down;
                        c.phase_since = now;
                        if all {
                            c.down_all = true;
                        }
                    }) {
                        self.e(&e.line(&name));
                        return self.fail(1);
                    }
                }
                drop(op);
                return self.o(&format!("zygo: {name} is not running"));
            }
            SessionAnswer::Error { message } => {
                self.e(&format!("zygo: tmux: {message}"));
                return self.fail(1);
            }
            SessionAnswer::Exists => {}
        }
        match mux.show_session_option(&ws.tmux_session, WS_OPTION) {
            Ok(Some(v)) if v == id => {}
            Ok(_) => {
                self.e(&format!("zygo: {}", not_owned(&ws.tmux_session, &name)));
                return self.fail(1);
            }
            Err(e) => {
                self.e(&format!("zygo: tmux: {e}"));
                return self.fail(1);
            }
        }
        self.jobs.push(Job {
            id: id.clone(),
            name: name.clone(),
            store,
            ws,
            op: Some(op),
            mode: Mode::Drain,
            state: State::Preparing,
            track: BTreeMap::new(),
            saved: 0,
            next_progress: PROGRESS_EVERY,
        });
        let i = self.jobs.len() - 1;
        let cancelled = |r: &Self| matches!(r.jobs[i].state, State::Done);
        self.check_signal();
        if cancelled(self) {
            return;
        }
        let now = self.now_secs();
        if let Err(e) = self.write(i, "draining", &mut |c| {
            c.phase = Phase::Draining;
            c.phase_since = now;
        }) {
            self.done(i);
            self.e(&e.line(&name));
            return self.fail(1);
        }
        self.probe.fire("after_draining");
        self.check_signal();
        if cancelled(self) {
            return;
        }
        // Step 4 (D12).
        self.probe.fire("before_final_snapshot");
        let snap = Tmux::new(opts.tmux.clone(), self.jobs[i].ws.tmux_socket.clone());
        let snapped = match self.probe.fault("snapshot") {
            Some(e) => Err(anyhow::Error::from(e)),
            None => snapshot::run(&opts.paths, &snap, &id, None, true, OP_WAIT),
        };
        match snapped {
            Err(e) => self.e(&format!("zygo: final snapshot not saved: {e:#}")),
            Ok(SnapshotOutcome::Skipped(SkipReason::Logged)) => {
                let line = format!(
                    "zygo: {name}: final snapshot refused (see {}/errors.log); nothing was stopped",
                    self.jobs[i].store.dir().display()
                );
                self.revert(i, &line);
                return self.fail(1);
            }
            Ok(_) => {}
        }
        self.check_signal();
        if cancelled(self) {
            return;
        }
        let since = self.probe.elapsed();
        if opts.now {
            self.begin_stop(i);
        } else {
            self.jobs[i].state = State::Draining { since };
        }
    }

    /// The proxy status of the live session folder `dir` through one client per folder for the run (D14).
    fn real_status(&mut self, dir: &Path) -> Result<SessionStatus, String> {
        if !self.clients.contains_key(dir) {
            let c = SessionClient::from_session_dir(dir).map_err(|e| e.to_string())?;
            self.clients.insert(dir.to_path_buf(), c.timeout(STATUS_BOUND));
        }
        self.clients[dir].status().map_err(|e| e.to_string())
    }

    /// The idle rule for one live member, and its running set (empty when `activity.json` cannot be read).
    fn read_member(&mut self, dir: &Path) -> (Idle, BTreeMap<String, String>) {
        let act: Result<Activity, String> = activity::read(&SessionDir::new(dir.to_path_buf())).map_err(|e| format!("{e:#}"));
        let sid = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let status = match self.probe.status(&sid) {
            Some(r) => r,
            None => self.real_status(dir),
        };
        let idle = drain::idle(act.as_ref().map_err(String::as_str), status.as_ref().map_err(String::as_str), self.probe.now());
        (idle, act.map(|a| a.running).unwrap_or_default())
    }

    /// One poll of a draining workspace (D14, D15, D16).
    fn drain_tick(&mut self, i: usize, since: Duration, timeout: Duration) {
        if let Ok(Some(ws)) = self.jobs[i].store.read() {
            self.jobs[i].ws = ws;
        }
        let drain_time = self.probe.elapsed().saturating_sub(since);
        let name = self.jobs[i].name.clone();
        let members: Vec<(String, MemberRecord)> = self.jobs[i].ws.members.iter().map(|(k, m)| (k.clone(), m.clone())).collect();
        let mut lines: Vec<String> = Vec::new();
        let mut busy: Vec<String> = Vec::new();
        for (key, m) in members {
            let who = label(&name, &key);
            let read = live_session(&self.opts.paths, &m).map(|dir| self.read_member(&dir));
            let track = self.jobs[i].track.entry(key.clone()).or_default();
            let idle = match read {
                None => {
                    track.last = None;
                    true
                }
                Some((r, running)) => {
                    let skipped = matches!(
                        r,
                        Idle::IdleRuleSkipped | Idle::TurnOpen { rule_skipped: true } | Idle::Busy { rule_skipped: true, .. }
                    );
                    if skipped && !track.said_skip {
                        track.said_skip = true;
                        lines.push(format!("zygo: {who}: proxy request state not usable; idle rule 3 skipped"));
                    }
                    let idle = matches!(r, Idle::Idle | Idle::IdleRuleSkipped);
                    track.last = Some((r, running));
                    idle
                }
            };
            if idle {
                if !track.said_idle {
                    track.said_idle = true;
                    lines.push(format!("zygo: {who} is idle"));
                }
            } else {
                busy.push(key);
            }
        }
        for l in &lines {
            self.e(l);
        }
        if busy.is_empty() {
            return self.begin_stop(i);
        }
        if drain_time >= self.jobs[i].next_progress {
            self.jobs[i].next_progress += PROGRESS_EVERY;
            let panes: Vec<String> = busy.iter().map(|k| format!("pane {k}")).collect();
            self.e(&format!("zygo: {name}: still running: {}", panes.join(", ")));
        }
        if drain_time >= timeout {
            self.time_out(i, timeout, &busy);
        }
    }

    /// D16: `phase = up`, release, then the report from the last poll.
    fn time_out(&mut self, i: usize, timeout: Duration, busy: &[String]) {
        let name = self.jobs[i].name.clone();
        let report: Vec<String> = busy
            .iter()
            .map(|key| {
                let (reason, running) = match self.jobs[i].track.get(key).and_then(|t| t.last.as_ref()) {
                    Some((Idle::Busy { reason, .. }, running)) => (reason.clone(), running.clone()),
                    Some((_, running)) => (TURN_OPEN.to_string(), running.clone()),
                    None => (TURN_OPEN.to_string(), BTreeMap::new()),
                };
                let mut line = format!("zygo: {}: busy: {reason}", label(&name, key));
                if !running.is_empty() {
                    let list: Vec<String> = running.iter().map(|(id, ty)| format!("{} {}", esc(ty), esc(id))).collect();
                    line.push_str(&format!("; running subagents: {}", list.join(", ")));
                }
                line
            })
            .collect();
        self.revert(i, &format!("zygo: {name}: drain timed out after {} s; phase is up again", timeout.as_secs()));
        for l in &report {
            self.e(l);
        }
        self.timed_out = true;
        self.fail(1);
    }

    /// Steps 7-10 (D18-D21), then the first stop-wait check.
    fn begin_stop(&mut self, i: usize) {
        self.check_signal();
        if matches!(self.jobs[i].state, State::Done) {
            return;
        }
        // Step 7.
        self.jobs[i].mode = Mode::Stopping;
        let now = self.now_secs();
        if let Err(e) = self.write(i, "stopping", &mut |c| {
            c.phase = Phase::Stopping;
            c.phase_since = now;
        }) {
            // D18, round 2 item 3: the line of the failed write, then the fallback `phase = up` with its own report.
            let name = self.jobs[i].name.clone();
            self.e(&e.line(&name));
            self.restore(i, "stopping_fallback");
            self.done(i);
            return self.fail(1);
        }
        self.probe.fire("after_stopping");
        // Step 8.
        let live = self.save_members(i);
        // Step 9.
        self.busy_shells(i, &live);
        // Step 10.
        let name = self.jobs[i].name.clone();
        for (key, dir) in &live {
            if !session_dir::live(dir) {
                continue;
            }
            if let Err(e) = sigterm(dir) {
                self.e(&format!("zygo: {}: launcher not signalled: {e}", label(&name, key)));
            }
        }
        self.probe.fire("after_sigterm");
        self.jobs[i].state = State::StopWait { since: self.probe.elapsed(), keys: live };
        self.stop_wait(i);
    }

    /// D19, AU31, AU76, AU5, AU42: one write that saves the members live now; returns them (key to session folder).
    fn save_members(&mut self, i: usize) -> BTreeMap<String, PathBuf> {
        let opts = self.opts;
        let name = self.jobs[i].name.clone();
        let members: Vec<(String, MemberRecord)> = self.jobs[i].ws.members.iter().map(|(k, m)| (k.clone(), m.clone())).collect();
        let mut live: BTreeMap<String, PathBuf> = BTreeMap::new();
        let mut values: BTreeMap<String, (Vec<String>, Vec<Subagent>, String)> = BTreeMap::new();
        for (key, m) in members {
            let Some(dir) = live_session(&opts.paths, &m) else {
                continue;
            };
            // W§9.3 step 8, AU60: the zygo session that this stop ends, so its own commit keeps these values
            let sid = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
            let units = closed_units(&dir.join("work"));
            let interrupted = if opts.now {
                match activity::read(&SessionDir::new(dir.clone())) {
                    Ok(a) => a
                        .running
                        .iter()
                        .map(|(id, ty)| Subagent { agent_id: id.clone(), agent_type: ty.clone(), extra: Default::default() })
                        .collect(),
                    Err(e) => {
                        self.e(&format!("zygo: {}: activity file unreadable: {e:#}; its running subagents are not saved", label(&name, &key)));
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            values.insert(key.clone(), (units, interrupted, sid));
            live.insert(key, dir);
        }
        if values.is_empty() {
            return live;
        }
        let mut set = 0;
        let written = self.write(i, "members", &mut |c| {
            set = 0;
            for (key, (units, interrupted, sid)) in &values {
                if let Some(m) = c.members.get_mut(key) {
                    m.closed_units = units.clone();
                    m.interrupted = interrupted.clone();
                    m.clean_stop = true;
                    m.saved_for = Some(sid.clone());
                    set += 1;
                }
            }
        });
        match written {
            Ok(()) => self.jobs[i].saved = set,
            Err(e) => self.e(&format!("zygo: {name}: member records not saved: {}", e.text(&name))),
        }
        live
    }

    /// D20, AU24: one warning for each pane that is not a member live at step 8 and runs something other than a
    /// shell; a dead pane (`#{pane_dead}` = 1) is skipped (FIX-29). Every tmux error is ignored.
    fn busy_shells(&mut self, i: usize, live: &BTreeMap<String, PathBuf>) {
        let opts = self.opts;
        let (name, ws) = (self.jobs[i].name.clone(), self.jobs[i].ws.clone());
        let mux = up::server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref());
        let Ok(lines) = mux.list_panes(&ws.tmux_session) else {
            return;
        };
        for line in lines {
            let Ok((_, p)) = parse_pane_line(&line) else {
                continue;
            };
            if live.contains_key(&p.key) {
                continue;
            }
            let Ok(v) = mux.display(&p.id, "#{pane_dead}|#{pane_current_command}") else {
                continue;
            };
            let (dead, command) = v.split_once('|').unwrap_or(("", v.as_str()));
            // FIX-29: a dead pane (held by `remain-on-exit`) runs nothing; tmux still names its last program.
            if dead == "1" || command.is_empty() || is_shell(command) {
                continue;
            }
            let pane = if valid_pane_key(&p.key) { p.key.clone() } else { p.id.clone() };
            self.e(&format!("zygo: {name} pane {pane}: {} is running; it stops with the session", esc(command)));
        }
    }

    /// D21: a member is stopped when its record is not `running` and its launcher is not live. When all are, or after
    /// [`STOP_WAIT`], step 11 runs.
    fn stop_wait(&mut self, i: usize) {
        let State::StopWait { since, keys } = &self.jobs[i].state else {
            return;
        };
        let (since, keys) = (*since, keys.clone());
        if let Ok(Some(ws)) = self.jobs[i].store.read() {
            self.jobs[i].ws = ws;
        }
        let ws = &self.jobs[i].ws;
        let waiting: Vec<String> = keys
            .iter()
            .filter(|(k, dir)| ws.members.get(*k).is_some_and(|m| m.state == MemberState::Running) || session_dir::live(dir))
            .map(|(k, _)| k.clone())
            .collect();
        if !waiting.is_empty() {
            if self.probe.elapsed().saturating_sub(since) < STOP_WAIT {
                return;
            }
            let name = self.jobs[i].name.clone();
            for k in &waiting {
                self.e(&format!("zygo: {}: agent still running after 30 s; it stops with the session", label(&name, k)));
            }
        }
        self.finish_stop(i);
    }

    /// Step 11 (D22): the owner check again, `kill-session`, `phase = down`, release, the line.
    fn finish_stop(&mut self, i: usize) {
        let opts = self.opts;
        self.probe.fire("before_kill_session");
        let (id, name, ws) = (self.jobs[i].id.clone(), self.jobs[i].name.clone(), self.jobs[i].ws.clone());
        let mux = up::server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref());
        let refused = match mux.session_exists(&ws.tmux_session) {
            SessionAnswer::Exists => match mux.show_session_option(&ws.tmux_session, WS_OPTION) {
                Ok(Some(v)) if v == id => match mux.kill_session(&ws.tmux_session) {
                    Ok(()) => None,
                    Err(e) if e.is_gone() => None,
                    Err(e) => Some(format!("zygo: tmux: {e}")),
                },
                Ok(_) => Some(format!("zygo: {}", not_owned(&ws.tmux_session, &name))),
                Err(e) => Some(format!("zygo: tmux: {e}")),
            },
            SessionAnswer::Gone => None,
            SessionAnswer::Error { message } => Some(format!("zygo: tmux: {message}")),
        };
        if let Some(line) = refused {
            // `phase` stays `stopping`: AU75 completes it at the next `up` or `down`.
            self.done(i);
            self.e(&line);
            return self.fail(1);
        }
        let (all, now) = (self.all(), self.now_secs());
        if let Err(e) = self.write(i, "down", &mut |c| {
            c.phase = Phase::Down;
            c.phase_since = now;
            if all {
                c.down_all = true;
            }
        }) {
            self.done(i);
            self.e(&format!("zygo: {name}: phase not written: {}", e.text(&name)));
            return self.fail(1);
        }
        let saved = self.jobs[i].saved;
        self.done(i);
        self.o(&format!("zygo: {name} is down ({saved} agents saved)"));
    }

    fn active(&self) -> bool { self.jobs.iter().any(|j| matches!(j.state, State::Draining { .. } | State::StopWait { .. })) }

    /// The poll loop (D14): until no workspace drains or waits.
    fn run_loop(&mut self, timeout: Duration) {
        let start = self.probe.elapsed();
        let mut n: u32 = 0;
        while self.active() {
            let t = self.probe.elapsed().saturating_sub(start);
            if self.probe.poll(n, t) {
                self.signals.raise_for_test();
            }
            self.check_signal();
            for i in 0..self.jobs.len() {
                match self.jobs[i].state {
                    State::Draining { since } => self.drain_tick(i, since, timeout),
                    State::StopWait { .. } => self.stop_wait(i),
                    State::Preparing | State::Done => {}
                }
            }
            if !self.active() {
                break;
            }
            self.probe.sleep(POLL);
            n = n.saturating_add(1);
        }
    }
}

/// W§9.3 with the given probe (D23 exit codes).
fn run_inner(opts: &DownOpts, probe: &dyn Probe, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (mut targets, mut base) = match resolve(opts, out, err) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let wanted = match detach_wanted(opts, &targets) {
        Ok(w) => w,
        Err(e) => {
            say(err, &e.message);
            return e.code;
        }
    };
    if wanted {
        let (reached, skipped) = match guard(opts, &targets) {
            Ok(r) => r,
            Err(e) => {
                say(err, &e.message);
                return e.code;
            }
        };
        for line in &skipped {
            say(err, line);
        }
        // Round 3 M2: with `--all`, a target whose server cannot be reached is left out and the exit code is 1.
        if !skipped.is_empty() {
            base = base.max(1);
        }
        if reached.is_empty() {
            return base;
        }
        match any_session(opts, &reached) {
            Err(e) => {
                say(err, &e.message);
                return e.code;
            }
            Ok(true) => return detach(opts, &reached, out, err).max(base),
            Ok(false) => targets = reached,
        }
    }
    let timeout = if opts.now { Duration::MAX } else { timeout(opts, err) };
    let mut r = Run {
        opts,
        probe,
        signals: Signals::install(),
        out,
        err,
        jobs: Vec::new(),
        code: base,
        cancelled: None,
        timed_out: false,
        clients: BTreeMap::new(),
    };
    for ws in &targets {
        // A signal before a target is prepared leaves it untouched and ends the run with 128 + the signal.
        if let Some(sig) = r.signals.pending() {
            r.check_signal();
            r.cancelled.get_or_insert(sig);
            break;
        }
        r.prepare(ws);
    }
    r.run_loop(timeout);
    if r.timed_out {
        r.e(NOW_HINT);
    }
    let code = r.cancelled.map_or(r.code, |s| 128 + s);
    // The signal dispositions are restored here, once (D17).
    drop(r);
    code
}

/// `zygo down` with the given writers (the `/zygo:down` reply of M1).
pub fn run(opts: &DownOpts, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    run_inner(opts, &Real { start: Instant::now() }, out, err)
}

/// `zygo down` (W§9.3) on the process stdout and stderr.
pub fn down(opts: &DownOpts) -> i32 {
    let (stdout, stderr) = (std::io::stdout(), std::io::stderr());
    run(opts, &mut stdout.lock(), &mut stderr.lock())
}

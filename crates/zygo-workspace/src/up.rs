//! `zygo up`: one workspace (owner: node W7, W§8.1) and `--all` (owner: node W8, AU80).
//!
//! The library reads no process environment: `$TMUX`, `ZYGO_SESSION` and the caller environment come in through
//! [`UpOpts`]. Only `zygo: <name> is up` and the AU100 attach line go to `out`; every other line goes to `err` (D2).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use zygo_config::Config;
use zygo_core::esc;
use zygo_core::paths::Paths;
use zygo_work::repo::{checkout, Checkout};

use crate::args::{bypass_flags, filter_from, has_user_info};
use crate::env::{cleaned, REMOVE_ALWAYS, REMOVE_INSIDE_AGENT};
use crate::identity::{main_worktree, new_pane_key, slug, unique_name, workspace_id, worktrees, Worktree};
use crate::loadout::{self, LoadoutError, PaneKind as PlanKind, PlanPane};
use crate::mux::tmux::Tmux;
use crate::mux::{
    check_path, check_window_name, hook_command, parse_pane_line, session_target, window_target, HookScope, Multiplexer,
    MuxError, NewSession, NewWindow, SessionAnswer, Split, UnsafePath, MIN_VERSION,
};
use crate::repair::repair;
use crate::snapshot;
use crate::state::{list_all, MemberRecord, MemberState, Pane, PaneKind, Phase, StateError, Store, Window, Workspace};

/// The lock wait of every write of `up` (W§7.3).
const LOCK_WAIT: Duration = snapshot::OP_WAIT;
/// The bound of the git calls of `preflight` (finding 9).
const GIT_BOUND: Duration = Duration::from_secs(5);
/// The size of a loadout window and of a window with no saved entry (D7, D15).
pub(crate) const LOADOUT_SIZE: (u32, u32) = (200, 50);
/// tmux's window maximum (D31).
const SIZE_MAX: u32 = 10_000;
/// How long step 12 waits for the panes to reach their cwd (D8).
const CWD_WAIT: Duration = Duration::from_secs(2);
const CWD_POLL: Duration = Duration::from_millis(50);
const PANE_OPTION: &str = "@zygo-pane";
const WS_OPTION: &str = "@zygo-ws";
/// The session-scoped hooks of W§7.4.
const SESSION_HOOKS: [&str; 4] = ["after-new-window", "after-split-window", "client-detached", "after-kill-pane"];
/// The window-scoped hooks of W§7.4, set with `-w` on each window (PW6 option A).
const WINDOW_HOOKS: [&str; 4] = ["window-layout-changed", "window-renamed", "pane-exited", "window-unlinked"];

const NO_TMUX: &str = "zygo: workspaces need tmux; install tmux 3.2 or later";
const NOT_A_REPO: &str = "zygo up needs a git repository";
const BARE: &str = "zygo up needs a worktree; this repository is bare";
const NEWER: &str = "zygo: workspace state is from a newer zygo; run zygo update";
const AU98: &str = "zygo: --server holds user info; this agent is not saved by the workspace";
const NEW_SERVER: &str =
    "zygo: started a new tmux server from inside a zygo agent; its panes have no provider keys from the environment";
const LOADOUT_IGNORED: &str = "zygo: --loadout applies only with --reset or to a new workspace; restoring the saved layout";

/// The inputs of `zygo up` for one workspace. `tmux_env` is `$TMUX`, `zygo_session` is `ZYGO_SESSION` and `vars` is
/// the caller environment; only the entry point reads them from the process.
#[derive(Debug, Clone)]
pub struct UpOpts {
    pub name: Option<String>,
    pub loadout: Option<String>,
    pub reset: bool,
    pub no_attach: bool,
    pub cwd: PathBuf,
    pub zygo: PathBuf,
    pub tmux: PathBuf,
    pub tmux_env: Option<OsString>,
    pub zygo_session: Option<OsString>,
    pub vars: BTreeMap<OsString, OsString>,
    pub paths: Paths,
    pub config: Config,
}

/// A refusal: the exit code and the whole line as printed, without `\n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exit {
    pub code: i32,
    pub message: String,
}

/// The workspace that `up`, `attach` or `down` acts on (W§8.1 steps 2-4, D1). `state` is `Err` with the text that names
/// the file when the state does not parse or fails AU84.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub common_dir: PathBuf,
    pub main_root: PathBuf,
    pub worktrees: Vec<Worktree>,
    pub state: Result<Option<Workspace>, String>,
}

/// Test hooks (D33). Only the `test-hooks` feature builds them.
#[cfg(feature = "test-hooks")]
pub mod testing {
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use zygo_core::paths::Paths;

    use super::UpOpts;

    type Hook = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

    /// The hooks by (point, state folder), so parallel tests do not meet.
    static HOOKS: Mutex<BTreeMap<(&'static str, PathBuf), Hook>> = Mutex::new(BTreeMap::new());

    /// Sets (`Some`) or clears (`None`) the hook of `point` for the state folder `dir`. Points: `before_op_lock`,
    /// `after_new_session`, `split` (before each `split_window`), `before_final_snapshot`, `after_final_snapshot`. An
    /// `Err(text)` of the hook becomes `MuxError::Failed { message: text }` at that point.
    pub fn hook(dir: &Path, point: &'static str, f: Option<Box<dyn Fn() -> Result<(), String> + Send + Sync>>) {
        let mut hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
        match f {
            Some(h) => {
                hooks.insert((point, dir.to_path_buf()), Arc::from(h));
            }
            None => {
                hooks.remove(&(point, dir.to_path_buf()));
            }
        }
    }

    /// Runs the hook of (`point`, `dir`), if one is set, outside the lock of the hook table.
    pub(super) fn fire(dir: &Path, point: &'static str) -> Result<(), String> {
        let hook = HOOKS.lock().unwrap_or_else(|e| e.into_inner()).get(&(point, dir.to_path_buf())).cloned();
        hook.map_or(Ok(()), |h| h())
    }

    /// [`super::up`] with the given writers.
    pub fn run_with(opts: &UpOpts, out: &mut dyn Write, err: &mut dyn Write) -> i32 { super::run(opts, out, err) }

    /// [`super::attach`] with the given writers.
    pub fn attach_with(
        paths: &Paths, tmux: &Path, tmux_env: Option<&OsStr>, name: Option<&str>, cwd: &Path, out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> i32 {
        super::attach_to(paths, tmux, tmux_env, name, cwd, out, err)
    }
}

/// The test hook of `point` (D33); without the `test-hooks` feature it does nothing.
pub(crate) fn fire(dir: &Path, point: &'static str) -> Result<(), MuxError> {
    #[cfg(feature = "test-hooks")]
    {
        testing::fire(dir, point).map_err(|message| MuxError::Failed { message })
    }
    #[cfg(not(feature = "test-hooks"))]
    {
        let _ = (dir, point);
        Ok(())
    }
}

fn say(w: &mut dyn Write, line: &str) { let _ = writeln!(w, "{line}"); }

fn now_secs() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) }

fn fail(message: String) -> Exit { Exit { code: 1, message } }

fn tmux_err(e: MuxError) -> Exit { fail(format!("zygo: tmux: {e}")) }

/// The AU63 text, without the `zygo: ` prefix.
pub(crate) fn not_owned(session: &str, name: &str) -> String {
    format!("tmux session {session} does not belong to this workspace; rename it or run zygo ws forget {}", esc(name))
}

/// The AU87 "unreachable" text, without the `zygo: ` prefix.
pub(crate) fn unreachable(name: &str, key: &str, socket: Option<&Path>) -> String {
    let at = socket.map_or_else(|| "the default tmux socket".to_string(), |s| s.display().to_string());
    format!("{} has live agents (pane {key}) but its tmux session cannot be reached at {at}", esc(name))
}

/// The AU96 line of a pane that did not fit; `member` is its saved member record, if any.
pub(crate) fn not_built_line(key: &str, member: Option<&MemberRecord>) -> String {
    match member.and_then(|m| m.agent_session.as_deref()) {
        Some(s) => format!("zygo: pane {key} not built (no space); agent session {} is detached", esc(s)),
        None => format!("zygo: pane {key} not built (no space)"),
    }
}

/// Whether the launcher of the member `key` alone is live (D15, AU75 per member).
pub(crate) fn member_live(store: &Store, ws: &Workspace, key: &str) -> bool {
    let mut one = ws.clone();
    one.members.retain(|k, _| k == key);
    store.live_launcher(&one).is_some()
}

/// The socket of `$TMUX`: the text before its first `,`; empty gives none (D18).
fn env_socket(tmux_env: &OsStr) -> Option<PathBuf> {
    let b = tmux_env.as_encoded_bytes();
    let end = b.iter().position(|&c| c == b',').unwrap_or(b.len());
    if end == 0 {
        return None;
    }
    // SAFETY: the split is at an ASCII byte (or the end) of bytes that `as_encoded_bytes` gave.
    let s = unsafe { OsStr::from_encoded_bytes_unchecked(&b[..end]) };
    Some(PathBuf::from(s))
}

/// The tmux server of a workspace (D18): the saved socket, else the socket of `$TMUX`, else tmux's own choice.
pub fn server(tmux: &Path, tmux_socket: Option<&Path>, tmux_env: Option<&OsStr>) -> Tmux {
    let socket = tmux_socket.map(Path::to_path_buf).or_else(|| tmux_env.and_then(env_socket));
    Tmux::new(tmux.to_path_buf(), socket)
}

/// The AU62 check (D17): the parent folder of `path` is a folder owned by the current uid with mode 0700, and the
/// socket, when it exists, is owned by the current uid. A missing parent is created with mode 0700 only when
/// `create_parent`. `Err` holds the line without the `zygo: ` prefix.
#[cfg(unix)]
pub(crate) fn socket_is_private(path: &Path, create_parent: bool) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let refused = || {
        format!(
            "tmux socket {} is not private: the socket and its folder must be yours and the folder must have mode 0700",
            path.display()
        )
    };
    // SAFETY: getuid has no precondition and cannot fail.
    let uid = unsafe { libc::getuid() };
    let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return Err(refused());
    };
    let meta = match fs::symlink_metadata(parent) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && create_parent => {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(|e| format!("tmux socket {}: cannot create its folder: {e}", path.display()))?;
            fs::symlink_metadata(parent).map_err(|_| refused())?
        }
        Err(_) => return Err(refused()),
    };
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o777 != 0o700 {
        return Err(refused());
    }
    match fs::symlink_metadata(path) {
        Ok(m) if m.uid() != uid => Err(refused()),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(refused()),
    }
}

/// Other systems: M1 exits 2 there before any workspace verb (OP5).
#[cfg(not(unix))]
pub(crate) fn socket_is_private(_path: &Path, _create_parent: bool) -> Result<(), String> { Ok(()) }

/// The checkout of `cwd`, the worktree list and the main worktree (AU25, AU77, D4).
fn find_checkout(cwd: &Path) -> Result<(Checkout, Vec<Worktree>, PathBuf), Exit> {
    let usage = |m: &str| Exit { code: 2, message: m.to_string() };
    let deadline = Instant::now() + GIT_BOUND;
    let co = match checkout(cwd, deadline) {
        Some(c) => c,
        None => {
            // A bare repository's own folder: `rev-parse --show-toplevel` fails there (finding 8).
            let probe = Checkout { top: cwd.to_path_buf(), common_dir: cwd.to_path_buf(), main_root: cwd.to_path_buf(), deadline };
            let list = worktrees(&probe).map_err(|_| usage(NOT_A_REPO))?;
            if list.is_empty() {
                return Err(usage(NOT_A_REPO));
            }
            let main = main_worktree(&list).ok_or_else(|| usage(BARE))?;
            checkout(&main.path, deadline).ok_or_else(|| usage(NOT_A_REPO))?
        }
    };
    let list = worktrees(&co).map_err(|_| usage(NOT_A_REPO))?;
    let main = main_worktree(&list).ok_or_else(|| usage(BARE))?.path.clone();
    Ok((co, list, main))
}

/// W§8.1 steps 2-3 and the id of step 4 (D1, D4, D5).
pub fn preflight(paths: &Paths, tmux: &Path, name: Option<&str>, cwd: &Path) -> Result<Target, Exit> {
    let usage = |m: String| Exit { code: 2, message: m };
    match Tmux::new(tmux.to_path_buf(), None).version() {
        Ok(v) if v >= MIN_VERSION => {}
        Ok((major, minor)) => {
            return Err(usage(format!("zygo: tmux {major}.{minor} is too old; workspaces need tmux 3.2 or later")));
        }
        Err(MuxError::Spawn { .. }) => return Err(usage(NO_TMUX.into())),
        Err(e) => return Err(usage(format!("zygo: cannot read the tmux version: {e}"))),
    }
    let all = list_all(paths);
    if let Some(n) = name {
        let Some(ws) = all.into_iter().find_map(|(_, r)| r.ok().filter(|ws| ws.name == n)) else {
            return Err(fail(format!("zygo: no workspace named {}", esc(n))));
        };
        let deadline = Instant::now() + GIT_BOUND;
        let co = Checkout { top: ws.main_root.clone(), common_dir: ws.common_dir.clone(), main_root: ws.main_root.clone(), deadline };
        let list = worktrees(&co).unwrap_or_default();
        return Ok(Target {
            id: ws.id.clone(),
            name: ws.name.clone(),
            common_dir: ws.common_dir.clone(),
            main_root: ws.main_root.clone(),
            worktrees: list,
            state: Ok(Some(ws)),
        });
    }
    let (co, list, main_root) = find_checkout(cwd)?;
    let id = workspace_id(&co.common_dir);
    let taken: BTreeSet<String> =
        all.iter().filter(|(i, _)| *i != id).filter_map(|(_, r)| r.as_ref().ok().map(|w| w.name.clone())).collect();
    let folder = main_root.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let fresh_name = || unique_name(&slug(&folder), &|n| taken.contains(n));
    let (name, state) = match Store::open(paths, &id).read() {
        Ok(Some(ws)) => (ws.name.clone(), Ok(Some(ws))),
        Ok(None) => (fresh_name(), Ok(None)),
        Err(StateError::Newer { .. }) => return Err(fail(NEWER.into())),
        Err(e @ StateError::Invalid { .. }) => (fresh_name(), Err(e.to_string())),
        Err(e) => return Err(fail(format!("zygo: {e}"))),
    };
    Ok(Target { id, name, common_dir: co.common_dir, main_root, worktrees: list, state })
}

/// One pane to build (crate-private, for `repair`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuildPane {
    pub key: String,
    pub shell: bool,
    pub cwd: PathBuf,
    pub active: bool,
}

/// One window to build (crate-private, for `repair`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuildWindow {
    pub name: Option<String>,
    pub layout: Option<String>,
    pub width: u32,
    pub height: u32,
    pub active: bool,
    pub zoomed: bool,
    pub panes: Vec<BuildPane>,
}

/// The tmux steps of W§8.1 steps 10-11 on one session (crate-private, for `repair`).
pub(crate) struct Builder<'a> {
    pub mux: &'a dyn Multiplexer,
    pub session: &'a str,
    pub zygo: &'a Path,
    pub id: &'a str,
    pub main_root: &'a Path,
    /// The state folder, for the test hooks.
    pub dir: &'a Path,
}

impl Builder<'_> {
    /// `<zygo> ws run --ws-id <id> --pane <key>`, then `--shell` for a shell pane (AU15, AU46).
    pub fn argv(&self, key: &str, shell: bool) -> Vec<OsString> {
        let mut v: Vec<OsString> = vec![self.zygo.as_os_str().to_os_string()];
        v.extend(["ws", "run", "--ws-id", self.id, "--pane", key].iter().map(OsString::from));
        if shell {
            v.push("--shell".into());
        }
        v
    }

    fn hook(&self, event: &str) -> Result<String, MuxError> {
        hook_command(self.zygo, self.id, event).map_err(|e: UnsafePath| MuxError::Failed { message: e.to_string() })
    }

    /// `remain-on-exit off` and the window hooks of W§7.4 on one window (AU68, fixtures 6.22n, 6.42).
    pub fn window_setup(&self, window: u32) -> Result<(), MuxError> {
        self.mux.set_window_option(&window_target(self.session, window), "remain-on-exit", "off")?;
        for event in WINDOW_HOOKS {
            self.mux.set_hook(self.session, HookScope::Window(window), event, &self.hook(event)?)?;
        }
        Ok(())
    }

    /// W§8.1 step 10 on the session: the hidden names (D9), `ZYGO_WORKSPACE`, `destroy-unattached off` and the session
    /// hooks. `@zygo-ws` is set at creation (D27).
    pub fn session_setup(&self, hide_inside_agent: bool) -> Result<(), MuxError> {
        for name in REMOVE_ALWAYS {
            self.mux.hide_env(self.session, name)?;
        }
        if hide_inside_agent {
            for name in REMOVE_INSIDE_AGENT {
                self.mux.hide_env(self.session, name)?;
            }
        }
        self.mux.set_env(self.session, "ZYGO_WORKSPACE", self.id)?;
        self.mux.set_session_option(self.session, "destroy-unattached", "off")?;
        for event in SESSION_HOOKS {
            self.mux.set_hook(self.session, HookScope::Session, event, &self.hook(event)?)?;
        }
        Ok(())
    }

    /// Sets the key of the only pane of a new window; returns the pane id.
    pub fn key_first(&self, window: u32, key: &str) -> Result<String, MuxError> {
        let id = self.mux.display(&window_target(self.session, window), "#{pane_id}")?;
        if !id.starts_with('%') {
            return Err(MuxError::Failed { message: format!("tmux printed no pane id for window {window}: {id}") });
        }
        self.mux.set_pane_option(&id, PANE_OPTION, key)?;
        Ok(id)
    }

    /// A later window (W§8.1 step 11): `default-size`, `new-window` with its name and first pane, the key, the window
    /// setup. Returns the window index and the first pane id.
    pub fn add_window(&self, w: &BuildWindow) -> Result<(u32, String), MuxError> {
        let first = w.panes.first().ok_or_else(|| MuxError::Failed { message: "window with no pane".into() })?;
        self.mux.set_session_option(self.session, "default-size", &format!("{}x{}", w.width, w.height))?;
        let argv = self.argv(&first.key, first.shell);
        let spec = NewWindow { session: self.session, name: w.name.as_deref(), start_dir: self.main_root, argv: &argv };
        let index = self.mux.new_window(&spec)?;
        let id = self.key_first(index, &first.key)?;
        self.window_setup(index)?;
        Ok((index, id))
    }

    /// One more pane in `window` (D10): the split, its key, then `tiled` so the next split finds room. `Ok(None)` when
    /// the pane does not fit (AU96).
    pub fn split(&self, window: u32, p: &BuildPane) -> Result<Option<String>, MuxError> {
        let argv = self.argv(&p.key, p.shell);
        let target = window_target(self.session, window);
        let made = fire(self.dir, "split")
            .and_then(|()| self.mux.split_window(&Split { target: &target, start_dir: Some(self.main_root), argv: &argv }));
        let id = match made {
            Ok(id) => id,
            Err(e) if e.is_no_space() => return Ok(None),
            Err(e) => return Err(e),
        };
        self.mux.set_pane_option(&id, PANE_OPTION, &p.key)?;
        match self.mux.select_layout(self.session, window, "tiled") {
            Err(e) if e.is_gone() => return Err(e),
            _ => {}
        }
        Ok(Some(id))
    }

    /// The layout of one window (D11): a refused layout gives `tiled` and one line.
    pub fn layout(&self, window: u32, layout: Option<&str>, err: &mut dyn Write) -> Result<(), MuxError> {
        let Some(l) = layout.filter(|l| !l.is_empty()) else {
            return Ok(());
        };
        match self.mux.select_layout(self.session, window, l) {
            Ok(()) => Ok(()),
            Err(e) if e.is_gone() => Err(e),
            Err(e) => {
                say(err, &format!("zygo: window {window}: layout {l:?} refused ({e}); using tiled"));
                match self.mux.select_layout(self.session, window, "tiled") {
                    Err(e) if e.is_gone() => Err(e),
                    _ => Ok(()),
                }
            }
        }
    }
}

/// A fresh loadout member (AU45, D14, D32).
fn fresh_member(agent: zygo_core::Agent, pp: &PlanPane) -> MemberRecord {
    let mut m = MemberRecord::new(agent);
    m.state = MemberState::Running;
    m.launch.profile = pp.profile.clone();
    m.launch.yolo = pp.yolo;
    m.launch.passthrough = pp.passthrough;
    m.launch.server = pp.server.clone();
    m.launch.compact = pp.compact.clone();
    m.launch.config_dir = pp.config_dir.clone();
    let f = filter_from(agent, &pp.args, &pp.cwd);
    m.launch.args = f.kept;
    m.dropped_flags = f.dropped_flags;
    m
}

fn agent_kind(agent: zygo_core::Agent) -> PaneKind {
    match agent {
        zygo_core::Agent::ClaudeCode => PaneKind::ClaudeCode,
        zygo_core::Agent::OpenCode => PaneKind::OpenCode,
    }
}

/// The plan of W§8.1 step 7.
struct Plan {
    windows: Vec<BuildWindow>,
    /// The planned index of each window: the saved index or the loadout ordinal (D16).
    indexes: Vec<u32>,
    /// The planned kind of each pane.
    kinds: BTreeMap<String, PaneKind>,
    /// Fresh loadout members by key (D30).
    fresh: BTreeMap<String, MemberRecord>,
    resumed: Vec<String>,
    loadout: String,
    from_loadout: bool,
}

/// The lines of step 12, by group (D23).
#[derive(Default)]
struct Report {
    au98: Vec<String>,
    au70: Vec<String>,
    au30: Vec<String>,
    no_id: Vec<String>,
}

fn kind_name(m: &MemberRecord) -> &'static str { m.launch.kind.as_str() }

/// The loadout plan (W§8.1 step 7, first case).
fn loadout_plan(opts: &UpOpts, t: &Target, ws: Option<&Workspace>, report: &mut Report, err: &mut dyn Write) -> Result<Plan, Exit> {
    let plan = loadout::resolve(&opts.config, opts.loadout.as_deref(), &t.main_root, &t.worktrees).map_err(|e| Exit {
        code: if matches!(e, LoadoutError::Unknown { .. }) { 2 } else { 1 },
        message: format!("zygo: {e}"),
    })?;
    for w in &plan.warnings {
        say(err, &format!("zygo: {w}"));
    }
    let mut taken: BTreeSet<String> = match ws {
        Some(w) if !opts.reset => w.members.keys().chain(w.panes.iter().map(|p| &p.key)).cloned().collect(),
        _ => BTreeSet::new(),
    };
    let mut out = Plan {
        windows: vec![],
        indexes: vec![],
        kinds: BTreeMap::new(),
        fresh: BTreeMap::new(),
        resumed: vec![],
        loadout: plan.loadout.clone(),
        from_loadout: true,
    };
    for (wi, pw) in plan.windows.iter().enumerate() {
        let mut panes = Vec::with_capacity(pw.panes.len());
        for (pi, pp) in pw.panes.iter().enumerate() {
            let mut key = new_pane_key();
            while taken.contains(&key) {
                key = new_pane_key();
            }
            taken.insert(key.clone());
            let shell = match pp.kind {
                PlanKind::Shell => true,
                PlanKind::Agent(_) if pp.server.as_deref().is_some_and(has_user_info) => {
                    report.au98.push(AU98.into());
                    true
                }
                PlanKind::Agent(a) => {
                    out.fresh.insert(key.clone(), fresh_member(a, pp));
                    out.kinds.insert(key.clone(), agent_kind(a));
                    false
                }
            };
            if shell {
                out.kinds.insert(key.clone(), PaneKind::Shell);
            }
            panes.push(BuildPane { key, shell, cwd: pp.cwd.clone(), active: pi == 0 });
        }
        out.windows.push(BuildWindow {
            name: pw.name.clone().filter(|n| !n.is_empty()),
            layout: pw.layout.clone().filter(|l| !l.is_empty()),
            width: LOADOUT_SIZE.0,
            height: LOADOUT_SIZE.1,
            active: wi == 0,
            zoomed: false,
            panes,
        });
        out.indexes.push(wi as u32);
    }
    Ok(out)
}

/// The restore plan (W§8.1 step 7, second case; D15).
fn restore_plan(store: &Store, cur: &Workspace, report: &mut Report) -> Plan {
    let mut out = Plan {
        windows: vec![],
        indexes: vec![],
        kinds: BTreeMap::new(),
        fresh: BTreeMap::new(),
        resumed: vec![],
        loadout: cur.loadout.clone(),
        from_loadout: false,
    };
    let indexes: BTreeSet<u32> = cur.panes.iter().map(|p| p.window).collect();
    for idx in indexes {
        let saved = cur.windows.iter().find(|w| w.index == idx);
        let mut ps: Vec<&Pane> = cur.panes.iter().filter(|p| p.window == idx).collect();
        ps.sort_by_key(|p| p.index);
        let mut panes = Vec::with_capacity(ps.len());
        for p in ps {
            let m = cur.members.get(&p.key).filter(|m| matches!(m.state, MemberState::Running | MemberState::Resumable));
            let resume = match m {
                Some(m) if m.agent_session.is_some() => !member_live(store, cur, &p.key),
                Some(_) => {
                    report.no_id.push(format!("zygo: pane {}: no agent session id saved; started a shell", p.key));
                    false
                }
                None => false,
            };
            match m {
                Some(m) if resume => {
                    let id = esc(m.agent_session.as_deref().unwrap_or(""));
                    let flags = bypass_flags(&m.launch.args, m.launch.yolo);
                    if !flags.is_empty() {
                        report.au70.push(format!(
                            "zygo: pane {}: {} agent session {id} resumes with {}",
                            p.key,
                            kind_name(m),
                            flags.join(", ")
                        ));
                    }
                    for f in &m.dropped_flags {
                        report.au30.push(format!("zygo: pane {}: {} had an inline value; not restored", p.key, esc(f)));
                    }
                    out.resumed.push(p.key.clone());
                    out.kinds.insert(p.key.clone(), agent_kind(m.launch.kind));
                }
                _ => {
                    out.kinds.insert(p.key.clone(), PaneKind::Shell);
                }
            }
            panes.push(BuildPane { key: p.key.clone(), shell: !resume, cwd: p.cwd.clone(), active: p.active });
        }
        out.windows.push(BuildWindow {
            name: saved.and_then(|w| w.name.clone()).filter(|n| !n.is_empty()),
            layout: saved.map(|w| w.layout.clone()).filter(|l| !l.is_empty()),
            width: saved.map_or(LOADOUT_SIZE.0, |w| w.width),
            height: saved.map_or(LOADOUT_SIZE.1, |w| w.height),
            active: saved.is_some_and(|w| w.active),
            zoomed: saved.is_some_and(|w| w.zoomed),
            panes,
        });
        out.indexes.push(idx);
    }
    out
}

/// The plan-time checks of D31.
fn check_plan(plan: &Plan) -> Result<(), Exit> {
    for (w, idx) in plan.windows.iter().zip(&plan.indexes) {
        if let Some(n) = &w.name
            && check_window_name(n).is_err()
        {
            return Err(fail(format!("zygo: window {idx}: name {n:?} refused: window name holds a control character")));
        }
        if !(1..=SIZE_MAX).contains(&w.width) || !(1..=SIZE_MAX).contains(&w.height) {
            return Err(fail(format!("zygo: window {idx}: size {}x{} is outside 1..=10000", w.width, w.height)));
        }
    }
    Ok(())
}

/// The owner check of AU63 on a session that exists.
fn owner_check(mux: &dyn Multiplexer, session: &str, id: &str, name: &str) -> Result<(), Exit> {
    match mux.show_session_option(session, WS_OPTION) {
        Ok(Some(v)) if v == id => Ok(()),
        Ok(_) => Err(fail(format!("zygo: {}", not_owned(session, name)))),
        Err(e) => Err(tmux_err(e)),
    }
}

/// W§8.1 step 13: `--no-attach` or `ZYGO_SESSION` (`quiet`), AU100, `switch-client` or `attach-session`.
fn attach_step(
    mux: &dyn Multiplexer, name: &str, socket: &Path, tmux_env: Option<&OsStr>, quiet: bool, out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let session = format!("zygo-{name}");
    if quiet {
        say(out, &format!("zygo: {name} is up"));
        return 0;
    }
    if let Some(env_sock) = tmux_env.and_then(env_socket) {
        if env_sock != socket {
            say(out, &format!("zygo: {name} runs on another tmux server; attach with: tmux -S '{}' attach -t =zygo-{name}", socket.display()));
            return 0;
        }
        return match mux.switch_client(&session) {
            Ok(()) => 0,
            Err(e) => {
                say(err, &format!("zygo: tmux: {e}"));
                1
            }
        };
    }
    match mux.attach(&session) {
        Ok(code) => code,
        Err(e) => {
            say(err, &format!("zygo: tmux: {e}"));
            1
        }
    }
}

/// W§8.1 steps 4-12 for one target, and 13 when `attach` (crate-private, for W8).
pub(crate) fn up_target(opts: &UpOpts, target: Target, out: &mut dyn Write, err: &mut dyn Write, attach: bool) -> i32 {
    match build(opts, target, out, err, attach) {
        Ok(code) => code,
        Err(e) => {
            say(err, &e.message);
            e.code
        }
    }
}

fn build(opts: &UpOpts, target: Target, out: &mut dyn Write, err: &mut dyn Write, attach: bool) -> Result<i32, Exit> {
    let id = target.id.clone();
    let name = target.name.clone();
    let session = format!("zygo-{name}");
    // Step 4.
    check_path(&opts.zygo).map_err(|e| Exit { code: 2, message: e.to_string() })?;
    let store = Store::open(&opts.paths, &id);
    fire(store.dir(), "before_op_lock").map_err(tmux_err)?;
    let op = match store.op_lock() {
        Ok(l) => l,
        Err(StateError::OpHeld) => return Err(fail(format!("zygo: up or down is in progress for {name}"))),
        Err(e) => return Err(fail(format!("zygo: {e}"))),
    };
    let write_err = |e: StateError| match e {
        StateError::Busy => fail(format!("zygo: workspace {name} is busy; try again")),
        e => fail(format!("zygo: {e}")),
    };
    let read = || match store.read() {
        Ok(s) => Ok(s),
        Err(StateError::Newer { .. }) => Err(fail(NEWER.into())),
        Err(e) => Err(fail(format!("zygo: {e}"))),
    };
    // The state again under `op.lock` (implementation decision I1); a state that cannot be read: D28.
    let mut ws = match store.read() {
        Ok(s) => s,
        Err(StateError::Newer { .. }) => return Err(fail(NEWER.into())),
        Err(e @ StateError::Invalid { .. }) if !opts.reset => return Err(fail(format!("zygo: {e}"))),
        Err(StateError::Invalid { path, .. }) => {
            let bad = store.dir().join("workspace.json.bad");
            fs::rename(&path, &bad).map_err(|e| fail(format!("zygo: cannot rename {}: {e}", path.display())))?;
            None
        }
        Err(e) => return Err(fail(format!("zygo: {e}"))),
    };
    // Step 5.
    if let Some(cur) = &ws {
        if let Some(sock) = &cur.tmux_socket {
            socket_is_private(sock, true).map_err(|t| fail(format!("zygo: {t}")))?;
        }
        let mux = server(&opts.tmux, cur.tmux_socket.as_deref(), opts.tmux_env.as_deref());
        repair(&store, &mux, &opts.zygo, out, err).map_err(|e| fail(format!("zygo: {e:#}")))?;
        ws = read()?;
        let socket = ws.as_ref().and_then(|w| w.tmux_socket.clone());
        let _ = snapshot::after_write(&opts.paths, &Tmux::new(opts.tmux.clone(), socket), &id, snapshot::OP_WAIT);
    }
    // Step 6.
    let quiet = opts.no_attach || opts.zygo_session.is_some();
    let saved_socket = ws.as_ref().and_then(|w| w.tmux_socket.clone());
    let mux6 = server(&opts.tmux, saved_socket.as_deref(), opts.tmux_env.as_deref());
    match mux6.session_exists(&session) {
        SessionAnswer::Exists => {
            owner_check(&mux6, &session, &id, &name)?;
            if opts.reset || opts.loadout.is_some() {
                return Err(fail(format!("zygo: {name} is running; run zygo down {name} first")));
            }
            let socket = match saved_socket {
                Some(s) => s,
                None => mux6.socket_path(&session).map_err(tmux_err)?,
            };
            drop(op);
            return Ok(if attach { attach_step(&mux6, &name, &socket, opts.tmux_env.as_deref(), quiet, out, err) } else { 0 });
        }
        SessionAnswer::Gone => {
            if let Some(cur) = &ws
                && let Some(key) = store.live_launcher(cur)
            {
                return Err(fail(format!("zygo: {}", unreachable(&name, &key, mux6.socket()))));
            }
        }
        SessionAnswer::Error { message } => return Err(fail(format!("zygo: tmux: {message}"))),
    }
    // Step 7.
    let mut report = Report::default();
    let use_loadout = opts.reset || ws.as_ref().is_none_or(|w| w.panes.is_empty());
    let plan = match &ws {
        Some(cur) if !use_loadout => {
            if opts.loadout.is_some() {
                say(err, LOADOUT_IGNORED);
            }
            restore_plan(&store, cur, &mut report)
        }
        _ => loadout_plan(opts, &target, ws.as_ref(), &mut report, err)?,
    };
    check_plan(&plan)?;
    if opts.reset
        && let Some(cur) = &ws
    {
        for (k, m) in &cur.members {
            if let (MemberState::Running | MemberState::Resumable, Some(s)) = (m.state, m.agent_session.as_deref()) {
                say(err, &format!("zygo: --reset drops {} agent session {} (pane {k})", kind_name(m), esc(s)));
            }
        }
    }
    // Step 8.
    let planned_keys: BTreeSet<String> = plan.windows.iter().flat_map(|w| w.panes.iter().map(|p| p.key.clone())).collect();
    let written = store
        .update(LOCK_WAIT, &mut |cur| {
            cur.name = name.clone();
            cur.tmux_session = session.clone();
            cur.common_dir = target.common_dir.clone();
            cur.main_root = target.main_root.clone();
            cur.loadout = plan.loadout.clone();
            cur.phase = Phase::Building;
            cur.phase_since = now_secs();
            if plan.from_loadout {
                if opts.reset {
                    cur.members.clear();
                }
                cur.windows = plan
                    .windows
                    .iter()
                    .zip(&plan.indexes)
                    .map(|(w, &index)| Window {
                        index,
                        name: w.name.clone(),
                        layout: w.layout.clone().unwrap_or_default(),
                        width: w.width,
                        height: w.height,
                        active: w.active,
                        zoomed: w.zoomed,
                        ..Window::default()
                    })
                    .collect();
                cur.panes = plan
                    .windows
                    .iter()
                    .zip(&plan.indexes)
                    .flat_map(|(w, &window)| {
                        w.panes.iter().enumerate().map(move |(i, p)| (window, i, p))
                    })
                    .map(|(window, i, p)| Pane {
                        key: p.key.clone(),
                        window,
                        index: i as u32,
                        active: p.active,
                        cwd: p.cwd.clone(),
                        kind: plan.kinds.get(&p.key).copied().unwrap_or_default(),
                        ..Pane::default()
                    })
                    .collect();
                for (k, m) in &plan.fresh {
                    cur.members.insert(k.clone(), m.clone());
                }
            } else {
                for p in cur.panes.iter_mut() {
                    if let Some(k) = plan.kinds.get(&p.key) {
                        p.kind = *k;
                    }
                }
                for k in &plan.resumed {
                    if let Some(m) = cur.members.get_mut(k) {
                        m.note_pending = true;
                    }
                }
            }
            cur.snapshot_seq += 1;
            Ok(())
        })
        .map_err(write_err)?;
    let _ = snapshot::after_write(&opts.paths, &Tmux::new(opts.tmux.clone(), written.tmux_socket.clone()), &id, snapshot::OP_WAIT);
    // Step 9.
    let first = &plan.windows[0];
    let first_pane = &first.panes[0];
    if written.tmux_socket.is_none()
        && let Some(s) = opts.tmux_env.as_deref().and_then(env_socket)
    {
        socket_is_private(&s, false).map_err(|t| fail(format!("zygo: {t}")))?;
    }
    let mux9 = server(&opts.tmux, written.tmux_socket.as_deref(), opts.tmux_env.as_deref());
    let inside_agent = opts.zygo_session.is_some() || opts.vars.contains_key(OsStr::new("ZYGO_SESSION"));
    let new_server = opts.zygo_session.is_some()
        && matches!(mux9.display(&session_target(&session), "#{pid}"),
            Err(MuxError::Gone { message }) if message.starts_with("no server running on ") || message.starts_with("error connecting to "));
    let mut vars: Vec<(OsString, OsString)> = opts.vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    if let Some(z) = &opts.zygo_session
        && !opts.vars.contains_key(OsStr::new("ZYGO_SESSION"))
    {
        vars.push(("ZYGO_SESSION".into(), z.clone()));
    }
    let env = cleaned(vars);
    let b9 = Builder { mux: &mux9, session: &session, zygo: &opts.zygo, id: &id, main_root: &target.main_root, dir: store.dir() };
    let argv = b9.argv(&first_pane.key, first_pane.shell);
    let spec = NewSession {
        name: &session,
        start_dir: &target.main_root,
        width: first.width,
        height: first.height,
        env: &env,
        argv: &argv,
        owner: Some((&id, &first_pane.key)),
    };
    let first_index = mux9.new_session(&spec).map_err(tmux_err)?;
    if new_server {
        say(err, NEW_SERVER);
    }
    fire(store.dir(), "after_new_session").map_err(tmux_err)?;
    mux9.resize_window(&session, first_index, first.width, first.height).map_err(tmux_err)?;
    if let Some(n) = &first.name {
        mux9.rename_window(&session, first_index, n).map_err(tmux_err)?;
    }
    let socket = mux9.socket_path(&session).map_err(tmux_err)?;
    let refuse = |code: i32, message: String| {
        let _ = mux9.kill_session(&session);
        let _ = store.update(LOCK_WAIT, &mut |c| {
            c.phase = Phase::Down;
            c.phase_since = now_secs();
            Ok(())
        });
        Exit { code, message }
    };
    if check_path(&socket).is_err() {
        return Err(refuse(2, UnsafePath.to_string()));
    }
    if let Err(t) = socket_is_private(&socket, false) {
        return Err(refuse(1, format!("zygo: {t}")));
    }
    let mut last = store
        .update(LOCK_WAIT, &mut |c| {
            c.tmux_socket = Some(socket.clone());
            Ok(())
        })
        .map_err(write_err)?;
    let mux = Tmux::new(opts.tmux.clone(), last.tmux_socket.clone());
    let _ = snapshot::after_write(&opts.paths, &mux, &id, snapshot::OP_WAIT);
    // Steps 10-11.
    let b = Builder { mux: &mux, session: &session, zygo: &opts.zygo, id: &id, main_root: &target.main_root, dir: store.dir() };
    b.session_setup(inside_agent).map_err(tmux_err)?;
    b.window_setup(first_index).map_err(tmux_err)?;
    let mut real: Vec<u32> = Vec::with_capacity(plan.windows.len());
    let mut ids: Vec<Vec<Option<String>>> = Vec::with_capacity(plan.windows.len());
    let mut not_built: Vec<String> = Vec::new();
    for (wi, w) in plan.windows.iter().enumerate() {
        let (index, first_id) = if wi == 0 {
            let id0 = mux.display(&window_target(&session, first_index), "#{pane_id}").map_err(tmux_err)?;
            (first_index, id0)
        } else {
            b.add_window(w).map_err(tmux_err)?
        };
        let mut wids = vec![Some(first_id)];
        for p in &w.panes[1..] {
            let made = b.split(index, p).map_err(tmux_err)?;
            if made.is_none() {
                not_built.push(p.key.clone());
            }
            wids.push(made);
        }
        real.push(index);
        ids.push(wids);
    }
    for (w, &index) in plan.windows.iter().zip(&real) {
        b.layout(index, w.layout.as_deref(), err).map_err(tmux_err)?;
    }
    mux.unset_window_option(&session, first_index, "window-size").map_err(tmux_err)?;
    // D13: the panes that did not fit leave `panes[]`; fresh members among them leave `members` (D30).
    let mut au96 = Vec::new();
    if !not_built.is_empty() {
        for k in &not_built {
            let saved = if plan.fresh.contains_key(k) { None } else { last.members.get(k) };
            au96.push(not_built_line(k, saved));
        }
        last = store
            .update(LOCK_WAIT, &mut |c| {
                c.panes.retain(|p| !not_built.contains(&p.key));
                for k in &not_built {
                    if plan.fresh.contains_key(k) {
                        c.members.remove(k);
                    }
                }
                c.snapshot_seq += 1;
                Ok(())
            })
            .map_err(write_err)?;
        let _ = snapshot::after_write(&opts.paths, &Tmux::new(opts.tmux.clone(), last.tmux_socket.clone()), &id, snapshot::OP_WAIT);
    }
    // Step 12: active panes and zoom per window, then the active window.
    for (w, wids) in plan.windows.iter().zip(&ids) {
        let built: Vec<&String> = wids.iter().flatten().collect();
        let active = w.panes.iter().zip(wids).find_map(|(p, id)| id.as_ref().filter(|_| p.active)).or(built.first().copied());
        if let Some(a) = active {
            mux.select_pane(a).map_err(tmux_err)?;
            if w.zoomed && built.len() > 1 {
                mux.zoom(a).map_err(tmux_err)?;
            }
        }
    }
    let active_w = plan.windows.iter().position(|w| w.active).unwrap_or(0);
    mux.select_window(&session, real[active_w]).map_err(tmux_err)?;
    // D8: wait until the built panes report their planned folder.
    let expected: BTreeMap<String, PathBuf> = plan
        .windows
        .iter()
        .zip(&ids)
        .flat_map(|(w, wids)| w.panes.iter().zip(wids))
        .filter(|(_, id)| id.is_some())
        .filter_map(|(p, _)| fs::canonicalize(&p.cwd).ok().filter(|c| c.is_dir()).map(|c| (p.key.clone(), c)))
        .collect();
    let t0 = Instant::now();
    loop {
        let reached = match mux.list_panes(&session) {
            Ok(lines) => {
                let got: BTreeMap<String, PathBuf> =
                    lines.iter().filter_map(|l| parse_pane_line(l).ok()).map(|(_, p)| (p.key, p.cwd)).collect();
                expected.iter().all(|(k, c)| got.get(k) == Some(c))
            }
            Err(_) => true,
        };
        if reached || t0.elapsed() >= CWD_WAIT {
            break;
        }
        std::thread::sleep(CWD_POLL);
    }
    fire(store.dir(), "before_final_snapshot").map_err(tmux_err)?;
    let snap = Tmux::new(opts.tmux.clone(), last.tmux_socket.clone());
    if let Err(e) = snapshot::run(&opts.paths, &snap, &id, None, true, snapshot::OP_WAIT) {
        say(err, &format!("zygo: final snapshot not saved: {e:#}"));
    }
    fire(store.dir(), "after_final_snapshot").map_err(tmux_err)?;
    let done = store
        .update(LOCK_WAIT, &mut |c| {
            c.phase = Phase::Up;
            c.phase_since = now_secs();
            c.down_all = false;
            Ok(())
        })
        .map_err(write_err)?;
    if let Err(e) = snapshot::after_write(&opts.paths, &Tmux::new(opts.tmux.clone(), done.tmux_socket.clone()), &id, snapshot::OP_WAIT) {
        say(err, &format!("zygo: final snapshot not saved: {e:#}"));
    }
    // D23.
    let detached: Vec<String> = done
        .members
        .iter()
        .filter(|(k, m)| m.state == MemberState::Resumable && !planned_keys.contains(*k) && !not_built.contains(*k))
        .filter_map(|(k, m)| {
            m.agent_session.as_deref().map(|s| {
                format!(
                    "zygo: {} agent session {} is detached (its pane was closed); resume it by hand or run zygo ws forget-pane {k}",
                    kind_name(m),
                    esc(s)
                )
            })
        })
        .collect();
    for line in report.au98.iter().chain(&report.au70).chain(&report.au30).chain(&report.no_id).chain(&detached).chain(&au96) {
        say(err, line);
    }
    drop(op);
    // Step 13.
    Ok(if attach { attach_step(&mux, &name, &socket, opts.tmux_env.as_deref(), quiet, out, err) } else { 0 })
}

fn run(opts: &UpOpts, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match preflight(&opts.paths, &opts.tmux, opts.name.as_deref(), &opts.cwd) {
        Err(e) => {
            say(err, &e.message);
            e.code
        }
        Ok(t) => up_target(opts, t, out, err, true),
    }
}

/// `zygo up` for one workspace (W§8.1 steps 2-13), on the process stdout and stderr.
pub fn up(opts: &UpOpts) -> i32 {
    let (stdout, stderr) = (std::io::stdout(), std::io::stderr());
    run(opts, &mut stdout.lock(), &mut stderr.lock())
}

/// `zygo attach` (W§12, D21), on the process stdout and stderr.
pub fn attach(paths: &Paths, tmux: &Path, tmux_env: Option<&OsStr>, name: Option<&str>, cwd: &Path) -> i32 {
    let (stdout, stderr) = (std::io::stdout(), std::io::stderr());
    attach_to(paths, tmux, tmux_env, name, cwd, &mut stdout.lock(), &mut stderr.lock())
}

fn attach_to(
    paths: &Paths, tmux: &Path, tmux_env: Option<&OsStr>, name: Option<&str>, cwd: &Path, out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let t = match preflight(paths, tmux, name, cwd) {
        Ok(t) => t,
        Err(e) => {
            say(err, &e.message);
            return e.code;
        }
    };
    let down = format!("zygo: {} is down; run zygo up {}", t.name, t.name);
    let ws = match t.state {
        Err(text) => {
            say(err, &format!("zygo: {text}"));
            return 1;
        }
        Ok(None) => {
            say(err, &down);
            return 1;
        }
        Ok(Some(ws)) => ws,
    };
    let Some(socket) = ws.tmux_socket.clone() else {
        say(err, &down);
        return 1;
    };
    if let Err(text) = socket_is_private(&socket, true) {
        say(err, &format!("zygo: {text}"));
        return 1;
    }
    let mux = server(tmux, Some(&socket), tmux_env);
    match mux.session_exists(&ws.tmux_session) {
        SessionAnswer::Exists => {}
        SessionAnswer::Gone => {
            match Store::open(paths, &t.id).live_launcher(&ws) {
                Some(key) => say(err, &format!("zygo: {}", unreachable(&t.name, &key, Some(&socket)))),
                None => say(err, &down),
            }
            return 1;
        }
        SessionAnswer::Error { message } => {
            say(err, &format!("zygo: tmux: {message}"));
            return 1;
        }
    }
    if let Err(e) = owner_check(&mux, &ws.tmux_session, &t.id, &t.name) {
        say(err, &e.message);
        return e.code;
    }
    attach_step(&mux, &t.name, &socket, tmux_env, false, out, err)
}

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
use crate::identity::{main_worktree, new_pane_key, slug, unique_name, valid_name, workspace_id, worktrees, Worktree};
use crate::loadout::{self, LoadoutError, PaneKind as PlanKind, PlanPane};
use crate::mux::tmux::Tmux;
use crate::mux::{
    check_path, check_window_name, hook_command, parse_pane_line, session_target, window_target, HookScope, Multiplexer,
    MuxError, NewSession, NewWindow, SessionAnswer, Split, UnsafePath, MIN_VERSION,
};
use crate::repair::repair;
use crate::snapshot;
use crate::state::{
    list_all, read_state_capped, BuildPlan, StateBytes, MemberRecord, MemberState, Pane, PaneKind, Phase, StateError, Store, Window, Workspace,
};

/// The lock wait of every write of `up` (W§7.3).
const LOCK_WAIT: Duration = snapshot::OP_WAIT;
/// The bound of the git calls of `preflight` (finding 9) and of `ws forget-pane` (W8 D23).
/// Also used by zygo config check and show, and zygo loadout show (FIX-27).
pub const GIT_BOUND: Duration = Duration::from_secs(5);
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
    /// `before_plan_write` (before the step 8 write), `after_new_session`, `split` (before each `split_window`),
    /// `before_undo` (before the write of the B1 undo), `before_final_snapshot`, `after_final_snapshot`; the admin
    /// points of W8 D21: `forget_before_op_lock`, `forget_before_state` (under both locks, before anything is
    /// removed), `forget_before_cache` (after the state rename, before the cache removal; an error there is the cache
    /// error), `forget_before_tombstone_remove` (after the rename), `forget_pane_before_op_lock`,
    /// `forget_pane_before_cache`, `forget_pane_remove_results` and `forget_pane_remove_program` (right before the
    /// removal of that cache; an error there is that cache's error).
    /// An `Err(text)` of the hook becomes `MuxError::Failed { message: text }` at that point; at `before_undo` it is
    /// the undo's write error.
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

    /// [`super::up_all`] with the given writers.
    pub fn run_all_with(opts: &UpOpts, out: &mut dyn Write, err: &mut dyn Write) -> i32 { super::run_all(opts, out, err) }

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
    format!("tmux session {session} does not belong to this workspace; rename it or run zygo ws forget -- {}", esc(name))
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

/// Whether the member has an agent session id: `agent_session` or `pending.agent_session` (Round 3 R3-4). No undo and
/// no forget deletes such a record.
pub(crate) fn has_session_id(m: &MemberRecord) -> bool {
    m.agent_session.is_some() || m.pending.as_ref().is_some_and(|p| p.agent_session.is_some())
}

/// The line of AU67 for a `building` state that names no build plan (a file of an older zygo).
pub(crate) fn no_build_plan_line(name: &str) -> String {
    format!("zygo: {}: the state has no build plan (an older zygo wrote it); its layout and members stay", esc(name))
}

/// Takes back the plan of a build that did not finish (Round 3 R3-2), in the write that sets `phase = down`: a loadout
/// plan clears `windows[]` and `panes[]`, deletes the `fresh_members` records that have no agent session id and adds 1
/// to `snapshot_seq`; a restore plan keeps everything. Then `build_plan` and `fresh_members` are cleared. `false` when
/// no plan is saved (a file of an older zygo): everything stays, and the caller prints [`no_build_plan_line`].
pub(crate) fn take_back_plan(c: &mut Workspace) -> bool {
    let known = match c.build_plan {
        Some(BuildPlan::Loadout) => {
            c.windows.clear();
            c.panes.clear();
            for k in &c.fresh_members {
                if c.members.get(k).is_some_and(|m| !has_session_id(m)) {
                    c.members.remove(k);
                }
            }
            c.snapshot_seq += 1;
            true
        }
        Some(BuildPlan::Restore) => true,
        None => false,
    };
    c.build_plan = None;
    c.fresh_members.clear();
    known
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

/// The checkout of `cwd`, the worktree list and the main worktree (AU25, AU77, D4). Every git call ends by
/// `Instant::now() + bound` (W8 D23).
/// Also used by zygo config check and show, and zygo loadout show (FIX-27).
pub fn find_checkout(cwd: &Path, bound: Duration) -> Result<(Checkout, Vec<Worktree>, PathBuf), Exit> {
    let usage = |m: &str| Exit { code: 2, message: m.to_string() };
    let deadline = Instant::now() + bound;
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

/// The tmux version check of W§8.1 step 2 (code 2): tmux missing, older than 3.2, or a version that cannot be read
/// (W8 D4).
pub(crate) fn tmux_check(tmux: &Path) -> Result<(), Exit> {
    let usage = |m: String| Exit { code: 2, message: m };
    match Tmux::new(tmux.to_path_buf(), None).version() {
        Ok(v) if v >= MIN_VERSION => Ok(()),
        Ok((major, minor)) => Err(usage(format!("zygo: tmux {major}.{minor} is too old; workspaces need tmux 3.2 or later"))),
        Err(MuxError::Spawn { .. }) => Err(usage(NO_TMUX.into())),
        Err(e) => Err(usage(format!("zygo: cannot read the tmux version: {e}"))),
    }
}

/// W§8.1 steps 2-3 and the id of step 4 (D1, D4, D5).
pub fn preflight(paths: &Paths, tmux: &Path, name: Option<&str>, cwd: &Path) -> Result<Target, Exit> {
    tmux_check(tmux)?;
    let all = list_all(paths);
    if let Some(n) = name {
        let tree = |common_dir: &Path, main_root: &Path| {
            let deadline = Instant::now() + GIT_BOUND;
            let co = Checkout { top: main_root.to_path_buf(), common_dir: common_dir.to_path_buf(), main_root: main_root.to_path_buf(), deadline };
            worktrees(&co).unwrap_or_default()
        };
        if let Some(ws) = all.iter().find_map(|(_, r)| r.as_ref().ok().filter(|ws| ws.name == n)) {
            return Ok(Target {
                id: ws.id.clone(),
                name: ws.name.clone(),
                common_dir: ws.common_dir.clone(),
                main_root: ws.main_root.clone(),
                worktrees: tree(&ws.common_dir, &ws.main_root),
                state: Ok(Some(ws.clone())),
            });
        }
        // Round 2 M5: a state that cannot be read still names its workspace in its JSON `name`: a newer state exits 1
        // with the D1 text; an invalid one goes to the caller as `state = Err(<text naming the file>)` (W§10, D28).
        // Round 3 R3-5: an invalid state claims the name only when that name is valid, `common_dir` and `main_root`
        // are non-empty absolute paths and its folder is the W1 id of `common_dir`; two claimants exit 1.
        let text_of = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let claims = |id: &str, v: &serde_json::Value| {
            let (common_dir, main_root) = (text_of(v, "common_dir"), text_of(v, "main_root"));
            valid_name(n)
                && [&common_dir, &main_root].iter().all(|p| !p.is_empty() && Path::new(p.as_str()).is_absolute())
                && workspace_id(Path::new(&common_dir)) == id
        };
        let mut claimants: Vec<(&String, &StateError, PathBuf, serde_json::Value)> = Vec::new();
        for (id, r) in &all {
            let Err(e) = r else { continue };
            let file = paths.workspaces_dir().join(id).join("workspace.json");
            // FIX-17 D8: the one cap of AU84; a file larger than 1 MiB (or a link, or not a regular file) is not read,
            // so it matches no name.
            let raw = match read_state_capped(&file) {
                Ok(StateBytes::Bytes(b)) => Some(b),
                _ => None,
            };
            let Some(v) = raw.and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else {
                continue;
            };
            if v.get("name").and_then(|x| x.as_str()) != Some(n) {
                continue;
            }
            if matches!(e, StateError::Newer { .. }) || claims(id, &v) {
                claimants.push((id, e, file, v));
            }
        }
        match claimants.as_slice() {
            [] => return Err(fail(format!("zygo: no workspace named {}", esc(n)))),
            [(_, StateError::Newer { .. }, _, _)] => return Err(fail(NEWER.into())),
            [(id, e, _, v)] => {
                let (common_dir, main_root) = (PathBuf::from(text_of(v, "common_dir")), PathBuf::from(text_of(v, "main_root")));
                return Ok(Target {
                    id: (*id).clone(),
                    name: n.to_string(),
                    worktrees: tree(&common_dir, &main_root),
                    common_dir,
                    main_root,
                    state: Err(e.to_string()),
                });
            }
            many => {
                let files: Vec<String> = many.iter().map(|(_, _, f, _)| f.display().to_string()).collect();
                return Err(fail(format!("zygo: more than one workspace state has the name {}: {}", esc(n), files.join(", "))));
            }
        }
    }
    let (co, list, main_root) = find_checkout(cwd, GIT_BOUND)?;
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

/// `<zygo> ws run --ws-id <id> --pane <key>`, then `--shell` for a shell pane (AU15, AU46).
fn pane_argv(zygo: &Path, id: &str, key: &str, shell: bool) -> Vec<OsString> {
    let mut v: Vec<OsString> = vec![zygo.as_os_str().to_os_string()];
    v.extend(["ws", "run", "--ws-id", id, "--pane", key].iter().map(OsString::from));
    if shell {
        v.push("--shell".into());
    }
    v
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
    pub fn argv(&self, key: &str, shell: bool) -> Vec<OsString> { pane_argv(self.zygo, self.id, key, shell) }

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
    /// Bare members (no agent session id anywhere, no pending start) of panes restored as shells: step 8 deletes them
    /// (Round 2 M4, Round 3 R3-4).
    forget: Vec<String>,
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
        forget: vec![],
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
        forget: vec![],
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
                Some(m) => {
                    report.no_id.push(format!("zygo: pane {}: no agent session id saved; started a shell", p.key));
                    // Round 3 R3-4: only a bare record goes: no agent session id anywhere and no pending start.
                    if !has_session_id(m) && m.pending.is_none() {
                        out.forget.push(p.key.clone());
                    }
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

/// Round 2 M1: tmux reads an argument that ends with `;` as the end of a command, so no value that goes into a tmux
/// command line may end with `;`: the main worktree (the `-c` of new-session and new-window), each window name and
/// layout (Round 3 R3-6: from the loadout or the saved state), and each item of each pane's argument list (the zygo
/// path is the first). Checked before every write of this run except the step 5 repair; `repaired` says that the
/// repair wrote, so the text names "this plan".
fn check_separators(zygo: &Path, id: &str, main_root: &Path, plan: &Plan, repaired: bool) -> Result<(), Exit> {
    let what_stays = if repaired { "nothing of this plan was written" } else { "nothing was written" };
    let refuse = |what: String, value: String| {
        fail(format!("zygo: {what} {value:?} ends with `;`, which tmux reads as the end of a command; {what_stays}"))
    };
    let root = main_root.to_string_lossy();
    if root.ends_with(';') {
        return Err(refuse("the main worktree".into(), root.into_owned()));
    }
    for (w, idx) in plan.windows.iter().zip(&plan.indexes) {
        if let Some(n) = w.name.as_deref().filter(|n| n.ends_with(';')) {
            return Err(refuse(format!("window {idx}: name"), n.to_string()));
        }
        if let Some(l) = w.layout.as_deref().filter(|l| l.ends_with(';')) {
            return Err(refuse(format!("window {idx}: layout"), l.to_string()));
        }
        for p in &w.panes {
            for item in pane_argv(zygo, id, &p.key, p.shell) {
                let item = item.to_string_lossy().into_owned();
                if item.ends_with(';') {
                    return Err(refuse(format!("pane {}: the argument", p.key), item));
                }
            }
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
    // The state again under `op.lock` (implementation decision I1); a state that cannot be read: D28. With `--reset`
    // the rename to `workspace.json.bad` waits until the plan passed its checks (Round 3 R3-6).
    let mut bad: Option<PathBuf> = None;
    let mut ws = match store.read() {
        Ok(s) => s,
        Err(StateError::Newer { .. }) => return Err(fail(NEWER.into())),
        Err(e @ StateError::Invalid { .. }) if !opts.reset => return Err(fail(format!("zygo: {e}"))),
        Err(StateError::Invalid { path, .. }) => {
            bad = Some(path);
            None
        }
        Err(e) => return Err(fail(format!("zygo: {e}"))),
    };
    // Step 5.
    let was_building = ws.as_ref().is_some_and(|w| w.phase == Phase::Building);
    let repaired = ws.as_ref().is_some_and(|w| !matches!(w.phase, Phase::Up | Phase::Down));
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
                return Err(fail(format!("zygo: {name} is running; run zygo down -- {name} first")));
            }
            let socket = match saved_socket {
                Some(s) => s,
                None => mux6.socket_path(&session).map_err(tmux_err)?,
            };
            // Round 2 M2: a repair from `building` built panes; take the final snapshot as step 12 does.
            if was_building && ws.as_ref().is_some_and(|w| w.phase == Phase::Up) {
                let snap = Tmux::new(opts.tmux.clone(), ws.as_ref().and_then(|w| w.tmux_socket.clone()));
                if let Err(e) = snapshot::run(&opts.paths, &snap, &id, None, true, snapshot::OP_WAIT) {
                    say(err, &format!("zygo: final snapshot not saved: {e:#}"));
                }
                let after = read()?.and_then(|w| w.tmux_socket);
                if let Err(e) = snapshot::after_write(&opts.paths, &Tmux::new(opts.tmux.clone(), after), &id, snapshot::OP_WAIT) {
                    say(err, &format!("zygo: final snapshot not saved: {e:#}"));
                }
            }
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
    check_separators(&opts.zygo, &id, &target.main_root, &plan, repaired)?;
    if let Some(path) = &bad {
        let to = store.dir().join("workspace.json.bad");
        fs::rename(path, &to).map_err(|e| fail(format!("zygo: cannot rename {}: {e}", path.display())))?;
    }
    if opts.reset
        && let Some(cur) = &ws
    {
        for (k, m) in &cur.members {
            if let (MemberState::Running | MemberState::Resumable, Some(s)) = (m.state, m.agent_session.as_deref()) {
                say(err, &format!("zygo: --reset drops {} agent session {} (pane {k})", kind_name(m), esc(s)));
            }
        }
    }
    // Step 8. The server that step 9 uses (D18) goes into `tmux_socket` in this write, so the next `up` and its repair
    // ask the same server (Round 2 M3); a `$TMUX` socket gets the AU62 check before it is written.
    let saved_before = ws.as_ref().and_then(|w| w.tmux_socket.clone());
    if saved_before.is_none()
        && let Some(s) = opts.tmux_env.as_deref().and_then(env_socket)
    {
        socket_is_private(&s, false).map_err(|t| fail(format!("zygo: {t}")))?;
    }
    let chosen = server(&opts.tmux, saved_before.as_deref(), opts.tmux_env.as_deref()).socket().map(Path::to_path_buf);
    let planned_keys: BTreeSet<String> = plan.windows.iter().flat_map(|w| w.panes.iter().map(|p| p.key.clone())).collect();
    // Round 3 R3-8: whether this write set `tmux_socket`, decided in the closure on the state under `lock`.
    let mut stored_socket = false;
    // Round 3 R3-4: each record that this write deleted, whole, so the undo can put it back.
    let mut forgotten: BTreeMap<String, MemberRecord> = BTreeMap::new();
    fire(store.dir(), "before_plan_write").map_err(tmux_err)?;
    let written = store
        .update(LOCK_WAIT, &mut |cur| {
            stored_socket = false;
            forgotten.clear();
            cur.name = name.clone();
            cur.tmux_session = session.clone();
            cur.common_dir = target.common_dir.clone();
            cur.main_root = target.main_root.clone();
            cur.loadout = plan.loadout.clone();
            cur.phase = Phase::Building;
            cur.phase_since = now_secs();
            // Round 3 R3-2: the plan kind and the records that this write creates, for the undo and AU67.
            cur.build_plan = Some(if plan.from_loadout { BuildPlan::Loadout } else { BuildPlan::Restore });
            cur.fresh_members = if plan.from_loadout { plan.fresh.keys().cloned().collect() } else { Vec::new() };
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
                for k in &plan.forget {
                    // The record as it is under `lock`: one that got an id or a pending start since the read stays.
                    if cur.members.get(k).is_some_and(|m| !has_session_id(m) && m.pending.is_none())
                        && let Some(m) = cur.members.remove(k)
                    {
                        forgotten.insert(k.clone(), m);
                    }
                }
            }
            if cur.tmux_socket.is_none() && chosen.is_some() {
                cur.tmux_socket = chosen.clone();
                stored_socket = true;
            }
            cur.snapshot_seq += 1;
            Ok(())
        })
        .map_err(write_err)?;
    let _ = snapshot::after_write(&opts.paths, &Tmux::new(opts.tmux.clone(), written.tmux_socket.clone()), &id, snapshot::OP_WAIT);
    // Step 9.
    let first = &plan.windows[0];
    let first_pane = &first.panes[0];
    // B1 (Round 2): a failure that ends with phase `down` and no session of ours takes back what this `up` wrote, in
    // the same write: the plan by `build_plan` (Round 3 R3-2), the records that step 8 deleted (R3-4) and the socket
    // that step 8 stored (R3-8). Then the `snapshot.pending` check on the state it wrote (R3-7).
    let forgotten = &forgotten;
    let undo = |err: &mut dyn Write, code: i32, message: String| {
        let mut older = false;
        let wrote = fire(store.dir(), "before_undo").map_err(|e| e.to_string()).and_then(|()| {
            store
                .update(LOCK_WAIT, &mut |c| {
                    c.phase = Phase::Down;
                    c.phase_since = now_secs();
                    older = !take_back_plan(c);
                    for (k, m) in forgotten {
                        c.members.entry(k.clone()).or_insert_with(|| m.clone());
                    }
                    if stored_socket {
                        c.tmux_socket = None;
                    }
                    Ok(())
                })
                .map_err(|e| e.to_string())
        });
        match wrote {
            Ok(w) => {
                if older {
                    say(err, &no_build_plan_line(&name));
                }
                let _ = snapshot::after_write(&opts.paths, &Tmux::new(opts.tmux.clone(), w.tmux_socket.clone()), &id, snapshot::OP_WAIT);
            }
            Err(e) => say(err, &format!("zygo: state not cleaned: {e}")),
        }
        Exit { code, message }
    };
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
    // Round 3 R3-1: a failed `new-session` can still leave a session. Ours (`@zygo-ws` is the id): `phase` stays
    // `building` and the next `up` repairs it (AU67). None: the undo. Another owner's or none: AU63 and the undo. An
    // answer that tells neither keeps `building`; the next `up` asks again.
    let first_index = match mux9.new_session(&spec) {
        Ok(i) => i,
        Err(e) => {
            let message = format!("zygo: tmux: {e}");
            return Err(match mux9.session_exists(&session) {
                SessionAnswer::Gone => undo(err, 1, message),
                SessionAnswer::Exists => match mux9.show_session_option(&session, WS_OPTION) {
                    Ok(Some(v)) if v == id => fail(message),
                    Ok(_) => {
                        say(err, &message);
                        undo(err, 1, format!("zygo: {}", not_owned(&session, &name)))
                    }
                    Err(_) => fail(message),
                },
                SessionAnswer::Error { .. } => fail(message),
            });
        }
    };
    if new_server {
        say(err, NEW_SERVER);
    }
    fire(store.dir(), "after_new_session").map_err(tmux_err)?;
    mux9.resize_window(&session, first_index, first.width, first.height).map_err(tmux_err)?;
    if let Some(n) = &first.name {
        mux9.rename_window(&session, first_index, n).map_err(tmux_err)?;
    }
    let socket = mux9.socket_path(&session).map_err(tmux_err)?;
    let refuse = |err: &mut dyn Write, code: i32, message: String| {
        let _ = mux9.kill_session(&session);
        undo(err, code, message)
    };
    if check_path(&socket).is_err() {
        return Err(refuse(err, 2, UnsafePath.to_string()));
    }
    if let Err(t) = socket_is_private(&socket, false) {
        return Err(refuse(err, 1, format!("zygo: {t}")));
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
                c.fresh_members.retain(|k| !not_built.contains(k));
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
            c.build_plan = None;
            c.fresh_members.clear();
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

const ALL_FLAGS: &str = "zygo: up --all takes no workspace name, --loadout or --reset";
const NOTHING: &str = "zygo: up --all found no workspace to restore";

/// `zygo up --all` (W§8.1, AU52, AU80; W8 D2-D7), on the process stdout and stderr. It attaches to none.
pub fn up_all(opts: &UpOpts) -> i32 {
    let (stdout, stderr) = (std::io::stdout(), std::io::stderr());
    run_all(opts, &mut stdout.lock(), &mut stderr.lock())
}

/// What the D2 session check found for one candidate.
enum Found {
    /// No session: the workspace is restored.
    Chosen,
    /// The session exists: not restored, no line (D6).
    Running,
    /// Not restored: `zygo: <name> skipped: <reason>`.
    Skip(String),
}

fn run_all(opts: &UpOpts, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    // D4: refusals before any session check.
    if opts.name.is_some() || opts.loadout.is_some() || opts.reset {
        say(err, ALL_FLAGS);
        return 2;
    }
    if let Err(e) = check_path(&opts.zygo) {
        say(err, &e.to_string());
        return 2;
    }
    if let Err(e) = tmux_check(&opts.tmux) {
        say(err, &e.message);
        return e.code;
    }
    // D3: the states that cannot be read first, in id order (AU80).
    let mut skipped = false;
    let mut readable: Vec<Workspace> = Vec::new();
    for (id, r) in list_all(&opts.paths) {
        match r {
            Ok(ws) => readable.push(ws),
            Err(e) => {
                say(err, &format!("zygo: workspace {id} skipped: {e}"));
                skipped = true;
            }
        }
    }
    // D2: the candidates, in (name, id) order.
    readable.retain(|ws| ws.down_all || ws.phase == Phase::Up);
    readable.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    let mut chosen = false;
    for ws in readable {
        let name = ws.name.clone();
        let mut skip = |err: &mut dyn Write, reason: &str| {
            say(err, &format!("zygo: {name} skipped: {reason}"));
            skipped = true;
        };
        match session_check(opts, &ws) {
            Found::Chosen => {}
            Found::Running => continue,
            Found::Skip(reason) => {
                skip(err, &reason);
                continue;
            }
        }
        chosen = true;
        let target = match preflight(&opts.paths, &opts.tmux, Some(&name), &opts.cwd) {
            Ok(t) => t,
            Err(e) if e.code == 2 => {
                say(err, &e.message);
                return 2;
            }
            Err(e) => {
                skip(err, e.message.strip_prefix("zygo: ").unwrap_or(&e.message));
                continue;
            }
        };
        // D5: `preflight` by name gives the first readable state of that name in id order.
        if target.id != ws.id {
            skip(err, &format!("more than one workspace state has the name {name}"));
            continue;
        }
        // D7: `up_target` takes and releases its own `op.lock`.
        if up_target(opts, target, out, err, false) == 0 {
            say(out, &format!("zygo: {name} is up"));
        } else {
            say(err, &format!("zygo: {name} skipped"));
            skipped = true;
        }
    }
    if !chosen {
        say(err, NOTHING);
    }
    i32::from(skipped)
}

/// The D2 session check of one candidate: the AU62 check of its socket, then `has-session` on its server; a session
/// that exists goes to [`clear_down_all`] (D6).
fn session_check(opts: &UpOpts, ws: &Workspace) -> Found {
    if let Some(sock) = &ws.tmux_socket
        && let Err(t) = socket_is_private(sock, true)
    {
        return Found::Skip(t);
    }
    let mux = server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref());
    match mux.session_exists(&ws.tmux_session) {
        SessionAnswer::Gone => Found::Chosen,
        SessionAnswer::Error { message } => Found::Skip(format!("tmux: {message}")),
        SessionAnswer::Exists => match clear_down_all(opts, ws, &mux) {
            Ok(()) => Found::Running,
            Err(reason) => Found::Skip(reason),
        },
    }
}

/// D6: a session that exists is not restored. When it holds this workspace's `@zygo-ws` and `down_all = true`, the
/// workspace is up: `down_all` is cleared under `op.lock` after a second session and owner check, in one write under
/// `lock`; then the W§7.3 `snapshot.pending` check. A held `op.lock` changes nothing. `Err` holds the skip reason.
fn clear_down_all(opts: &UpOpts, ws: &Workspace, mux: &dyn Multiplexer) -> Result<(), String> {
    let ours = |mux: &dyn Multiplexer| match mux.show_session_option(&ws.tmux_session, WS_OPTION) {
        Ok(v) => Ok(v.as_deref() == Some(ws.id.as_str())),
        Err(e) => Err(format!("tmux: {e}")),
    };
    if !ours(mux)? || !ws.down_all {
        return Ok(());
    }
    let store = Store::open(&opts.paths, &ws.id);
    let _op = match store.op_lock() {
        Ok(l) => l,
        Err(StateError::OpHeld) => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if mux.session_exists(&ws.tmux_session) != SessionAnswer::Exists || !ours(mux)? {
        return Ok(());
    }
    let written = store
        .update(snapshot::OP_WAIT, &mut |cur| {
            if cur.down_all {
                cur.down_all = false;
            }
            Ok(())
        })
        .map_err(|e| e.to_string())?;
    let _ = snapshot::after_write(&opts.paths, &Tmux::new(opts.tmux.clone(), written.tmux_socket), &ws.id, snapshot::OP_WAIT);
    Ok(())
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
    let down = format!("zygo: {} is down; run zygo up -- {}", t.name, t.name);
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

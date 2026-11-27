//! `zygo up`: one workspace (owner: node W7, W§8.1) and `--all` (owner: node W8, AU80).
//!
//! The library reads no process environment: `$TMUX`, `ZYGO_SESSION` and the caller environment come in through
//! [`UpOpts`]. Only `zygo: <name> is up` and the AU100 attach line go to `out`; every other line goes to `err` (D2).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use zygo_config::repo_def::{apply_to_config, check_default, short, Definition, GitRunner, SystemGit, Use, REPOSITORY_LOADOUT};
use zygo_config::{Config, Severity, BUILTIN_LOADOUT};
use zygo_core::esc;
use zygo_core::paths::Paths;
use zygo_work::repo::{checkout, Checkout};

use crate::args::{bypass_flags, filter_from, has_user_info};
use crate::env::{cleaned, REMOVE_ALWAYS, REMOVE_INSIDE_AGENT};
use crate::identity::{
    first_user_worktree, main_worktree, new_pane_key, slug, unique_name, valid_name, valid_pane_key, valid_worktree, workspace_id, worktrees_with,
    ListError, Worktree,
};
use crate::loadout::{self, LoadoutError, PaneKind as PlanKind, PlanPane};
use crate::mail::{self, MAIL_LOCK_WAIT, mail_dir, not_pruned_line, store as mail_store};
use crate::pool;
use crate::mux::tmux::Tmux;
use crate::mux::{
    check_path, check_window_name, hook_command, parse_pane_line, session_target, window_target, HookScope, Multiplexer,
    MuxError, NewSession, NewWindow, SessionAnswer, Split, UnsafePath, MIN_VERSION,
};
use crate::repair::{check_saved_socket, repair, repair_failure};
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
/// The `Exit.message` of [`find_checkout`] when the folder is not in a git repository; its other errors keep their
/// own reason.
pub const NOT_A_REPO: &str = "zygo up needs a git repository";
const BARE: &str = "zygo up needs a worktree; this repository is bare";
const NEWER: &str = "zygo: workspace state is from a newer zygo; run zygo update";
const AU98: &str = "zygo: --server holds user info; this agent is not saved by the workspace";
const NEW_SERVER: &str =
    "zygo: started a new tmux server from inside a zygo agent; its panes have no provider keys from the environment";
const UP_ALL_NO_REPOSITORY: &str = "zygo: up --all reads no repository workspace files; using solo";
const LOADOUT_IGNORED: &str = "zygo: --loadout applies only with --reset or to a new workspace; restoring the saved layout";

/// The inputs of `zygo up` for one workspace. `tmux_env` is `$TMUX`, `zygo_session` is `ZYGO_SESSION` and `vars` is
/// the caller environment; only the entry point reads them from the process.
#[derive(Clone)]
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
    /// True when the user config file sets `[workspace] default_loadout` (workspace definition design R§10).
    pub default_loadout_set: bool,
    /// The git runner of the default-branch read (R§7.3); `None`: `SystemGit`. A test passes a counting runner.
    pub git: Option<Arc<dyn GitRunner + Send + Sync>>,
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
    /// The main worktree (AU77), the base of the pane folders (W§11). For a workspace with a saved state, by name or
    /// from any checkout of its repository: the saved `main_root` while it is a valid worktree
    /// ([`crate::identity::valid_worktree`]), else the first valid worktree of the list (FX-25 review round 2 D2).
    pub main_root: PathBuf,
    /// The saved `main_root` that is no longer a valid worktree, when `main_root` replaced it (D3): `zygo up` prints
    /// `zygo: the main worktree <old> is gone; panes start in <new>`.
    pub moved_from: Option<PathBuf>,
    /// The folder of the definition read (workspace definition design R§10, `zygo up` row): `Checkout.top` of the
    /// checkout found from the cwd; `main_root` for a workspace found by name.
    pub top: PathBuf,
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
    /// `before_undo` (before the write of the B1 undo), `before_final_snapshot`, `after_final_snapshot`, `release`
    /// (before each window of `Builder::release`; an error there is that window's error, FIX-29), `dead_lines` (before
    /// the pane read of `Builder::dead_lines`; an error there is that read's error), `dead_pane` (before the read of
    /// each pane of `Builder::dead_lines`; an error there is that pane's error; FIX-29 round 2), `add_window` (after
    /// the `new-window` of `Builder::add_window`, before the key; an error there is that window's error; FIX-29 round
    /// 3), `release_all` (before the window read of `Builder::release_all`; an error there is that read's error;
    /// FIX-29 round 3); the admin
    /// points of W8 D21: `forget_before_op_lock`, `forget_before_state` (under both locks, before anything is
    /// removed), `forget_before_cache` (after the state rename, before the cache removal; an error there is the cache
    /// error), `forget_before_tombstone_remove` (after the rename), `forget_pane_before_op_lock`,
    /// `forget_pane_before_cache`, `forget_pane_remove_results` and `forget_pane_remove_program` (right before the
    /// removal of that cache; an error there is that cache's error); the mail points of FX-13 (M§5 P1):
    /// `mail_delivery_after_read` (in `mail::Delivery::begin`, after the state read under `mail/lock`; an error there
    /// is the delivery's `Io` error) and `mail_read_before_dirs` (in `mail::store::read`, before it makes `cur/`; the
    /// state folder is the parent of `mail/`; an error there is the read's `Io` error).
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

    /// [`super::fresh_member`]: the record that `up` writes for a fresh loadout member (ML-3).
    pub fn fresh_member(agent: zygo_core::Agent, pp: &crate::loadout::PlanPane) -> crate::state::MemberRecord {
        super::fresh_member(agent, pp)
    }

    /// The `panes[]` entry that a loadout `up` writes for `pp` as pane 0 of window 0 (ML-3 review round 1).
    pub fn loadout_pane(pp: &crate::loadout::PlanPane) -> crate::state::Pane {
        let (kind, member) = super::plan_pane(pp);
        let b = super::BuildPane { key: "kaaaaaaaa".into(), shell: kind == crate::state::PaneKind::Shell, cwd: pp.cwd.clone(), active: true };
        super::new_pane(&b, 0, 0, kind, member.as_ref())
    }

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

/// The FIX-29 line of a pane whose command exited while `up` held it (W§8.1 step 12). `status` is
/// `#{pane_dead_status}`, empty when a signal ended it. `socket` holds no `'`: it is the socket read in W§8.1 step 9
/// (checked there) or a saved `tmux_socket`, which `repair::check_saved_socket` checked before any use (AU54).
pub(crate) fn exited_line(key: &str, status: &str, socket: &Path, pane: &str) -> String {
    let how = if status.is_empty() { "no exit status".to_string() } else { format!("exit {status}") };
    format!(
        "zygo: pane {key} exited during up ({how}); its dead pane stays: tmux -S '{}' respawn-pane -t {pane} starts it again",
        socket.display()
    )
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

/// FIX-30 (AU54): the socket that `mux` passes to tmux with `-S`, saved or from `$TMUX` (D18), must pass
/// [`check_path`] before the first tmux call on it; no socket (tmux's own choice) passes.
pub(crate) fn check_server(mux: &dyn Multiplexer) -> Result<(), UnsafePath> {
    mux.socket().map_or(Ok(()), |s| check_path(s).map(|_| ()))
}

/// The AU62 check (D17): the parent folder of `path` is a folder owned by the current uid with mode 0700, and the
/// socket, when it exists, is owned by the current uid. A missing parent is created with mode 0700 only when
/// `create_parent`. `Err` holds the line without the `zygo: ` prefix.
#[cfg(unix)]
pub fn socket_is_private(path: &Path, create_parent: bool) -> Result<(), String> {
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
pub fn socket_is_private(_path: &Path, _create_parent: bool) -> Result<(), String> { Ok(()) }

/// The checkout of `cwd`, the worktree list and the main worktree (AU25, AU77, D4). Every git call ends by
/// `Instant::now() + bound` (W8 D23).
/// Also used by zygo config check and show, and zygo loadout show (FIX-27).
/// Exits (W§8.1 step 3, FX-25): code 2 [`NOT_A_REPO`] only for git's "not a git repository" answer or an empty list
/// from the cwd, code 2 `BARE` for a list with no non-bare entry; code 1 with the reason for a failed or timed-out
/// `git worktree list` (`git worktree list failed[: <text>]`, `git worktree list timed out`), no program `git`
/// (`git not found`; another start error is `git worktree list failed: cannot start git: <error>`) and a main worktree
/// with no checkout (`no checkout at the main worktree <path>`; when the deadline stopped that read, `git worktree list
/// timed out`, FX-25 review round 2 F5). A code-1 message
/// has no `zygo:` prefix and is not escaped: each caller adds its prefix and escapes.
/// `cache` is zygo's workspaces cache folder (`Paths::workspace_cache_dir`): a worktree inside it (the mail-sync
/// worktree) is never the main worktree (D1, [`crate::identity::valid_worktree`]).
pub fn find_checkout(cwd: &Path, bound: Duration, cache: &Path) -> Result<(Checkout, Vec<Worktree>, PathBuf), Exit> {
    find_checkout_with(cwd, bound, OsStr::new("git"), cache)
}

/// [`find_checkout`] with `git` as the program of each `git worktree list` (`identity::worktrees_with`): the seam of
/// a test's failing or missing `git` (FX-25), so no test changes the `PATH` of its own process. The `rev-parse` calls
/// of `zygo_work::repo::checkout` keep the `git` of the `PATH`. Production calls go through [`find_checkout`].
#[doc(hidden)]
pub fn find_checkout_with(cwd: &Path, bound: Duration, git: &OsStr, cache: &Path) -> Result<(Checkout, Vec<Worktree>, PathBuf), Exit> {
    find_checkout_by(cwd, Bounds::all(bound), git, cache)
}

/// The deadlines of the git calls of [`find_checkout_by`]: the seam of a test that times out one call and not the
/// others (FX-25 review round 2 F5, F8). Production calls use [`Bounds::all`], one shared deadline.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// Every git call of the lookup ends by `Instant::now() + all`, taken once when the lookup starts.
    pub all: Duration,
    /// When set: the `rev-parse` of the main worktree (from a bare repository's own folder) ends by its own deadline,
    /// `Instant::now() + main` taken when that call starts.
    pub main: Option<Duration>,
    /// When set: the `git worktree list` of the found checkout ends by its own deadline, `Instant::now() + list` taken
    /// when that call starts.
    pub list: Option<Duration>,
}

impl Bounds {
    /// One deadline, `Instant::now() + bound` at the start, for every git call of the lookup.
    pub fn all(bound: Duration) -> Bounds { Bounds { all: bound, main: None, list: None } }
}

/// [`find_checkout_with`] with the deadlines of `bounds`. Production calls go through [`find_checkout`].
#[doc(hidden)]
pub fn find_checkout_by(cwd: &Path, bounds: Bounds, git: &OsStr, cache: &Path) -> Result<(Checkout, Vec<Worktree>, PathBuf), Exit> {
    let usage = |m: &str| Exit { code: 2, message: m.to_string() };
    let failed = |e: ListError| fail(e.to_string());
    let deadline = Instant::now() + bounds.all;
    let own = |bound: Option<Duration>| bound.map_or(deadline, |b| Instant::now() + b);
    let co = match checkout(cwd, deadline) {
        Some(c) => c,
        None => {
            // A bare repository's own folder: `rev-parse --show-toplevel` fails there (finding 8).
            let probe = Checkout { top: cwd.to_path_buf(), common_dir: cwd.to_path_buf(), main_root: cwd.to_path_buf(), deadline };
            let list = match worktrees_with(git, &probe) {
                Ok(list) => list,
                Err(ListError::NotARepository(_)) => return Err(usage(NOT_A_REPO)),
                Err(e) => return Err(failed(e)),
            };
            if list.is_empty() {
                return Err(usage(NOT_A_REPO));
            }
            let main = main_of(&list, cache)?;
            let main_deadline = own(bounds.main);
            match checkout(&main, main_deadline) {
                Some(c) => c,
                // F5: a read that the deadline stopped is the timeout, not a main worktree with no checkout.
                None if Instant::now() >= main_deadline => return Err(failed(ListError::TimedOut)),
                None => return Err(no_checkout_at(&main)),
            }
        }
    };
    let co = Checkout { deadline: own(bounds.list), ..co };
    // `rev-parse` found this checkout, so "not a git repository" here is a failed read (as `zygo_work::repo`), with
    // git's text (F6).
    let list = worktrees_with(git, &co).map_err(|e| match e {
        ListError::NotARepository(text) => failed(ListError::Failed(text)),
        e => failed(e),
    })?;
    let main = main_of(&list, cache)?;
    Ok((co, list, main))
}

/// The code-1 `Exit` of a main worktree with no checkout (W§8.1 step 3); no prefix, not escaped.
fn no_checkout_at(main: &Path) -> Exit { fail(format!("no checkout at the main worktree {}", main.display())) }

/// The main worktree of `list` (AU77, [`main_worktree`]: the first valid worktree, D1). With no entry that is neither
/// bare nor inside `cache`: code 2 `BARE`. With such entries, all marked `prunable`: code 1, no checkout at the first.
fn main_of(list: &[Worktree], cache: &Path) -> Result<PathBuf, Exit> {
    if let Some(main) = main_worktree(list, cache) {
        return Ok(main.path.clone());
    }
    match first_user_worktree(list, cache) {
        Some(first) => Err(no_checkout_at(&first.path)),
        None => Err(Exit { code: 2, message: BARE.to_string() }),
    }
}

/// The main worktree of a workspace (D2): the saved `main_root` while it is a valid worktree of `list`
/// ([`valid_worktree`]); else the first valid worktree ([`main_worktree`]), with the saved `main_root` that it replaces
/// (D3). None when `list` has no valid worktree.
fn keep_saved(saved: Option<&Path>, list: &[Worktree], cache: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    if let Some(s) = saved
        && list.iter().any(|w| w.path == s && valid_worktree(w, cache))
    {
        return Some((s.to_path_buf(), None));
    }
    let first = main_worktree(list, cache)?.path.clone();
    Some((first, saved.map(Path::to_path_buf)))
}

/// The D3 line of a saved `main_root` that is no longer a valid worktree; paths escaped.
fn moved_line(old: &Path, new: &Path) -> String {
    format!("zygo: the main worktree {} is gone; panes start in {}", esc(&old.to_string_lossy()), esc(&new.to_string_lossy()))
}

/// The code-1 `Exit` of [`find_checkout`] as `zygo up` prints it: `zygo: ` and the escaped message; code 2 as it is.
fn prefixed(e: Exit) -> Exit { if e.code == 1 { fail(format!("zygo: {}", esc(&e.message))) } else { e } }

/// The main worktree, the worktree list and the replaced `main_root` (D3) of a workspace found by name (W§8.1 step 3,
/// AU20), from the list that git gives in the saved `common_dir` ([`keep_saved`], D2). D4: with no valid worktree in
/// that list, exit 1 `zygo: no checkout at the main worktree <saved main_root>`; a list that fails or times out exits 1
/// with its reason, as from the cwd. Only a saved `common_dir` that does not exist keeps the saved `main_root` with no
/// list (the fallback of FX-25 review round 1, kept for states whose repository is not on this machine).
fn main_by_name(
    git: &OsStr, common_dir: &Path, main_root: &Path, cache: &Path,
) -> Result<(PathBuf, Vec<Worktree>, Option<PathBuf>), Exit> {
    if !common_dir.exists() {
        return Ok((main_root.to_path_buf(), Vec::new(), None));
    }
    let deadline = Instant::now() + GIT_BOUND;
    let co = Checkout { top: common_dir.to_path_buf(), common_dir: common_dir.to_path_buf(), main_root: main_root.to_path_buf(), deadline };
    let list = worktrees_with(git, &co).map_err(|e| prefixed(fail(e.to_string())))?;
    let (main, moved_from) = keep_saved(Some(main_root), &list, cache).ok_or_else(|| prefixed(no_checkout_at(main_root)))?;
    Ok((main, list, moved_from))
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
    preflight_with(paths, tmux, name, cwd, OsStr::new("git"))
}

/// [`preflight`] with `git` as the program of each `git worktree list` (see [`find_checkout_with`]; FX-25). Production
/// calls go through [`preflight`].
#[doc(hidden)]
pub fn preflight_with(paths: &Paths, tmux: &Path, name: Option<&str>, cwd: &Path, git: &OsStr) -> Result<Target, Exit> {
    tmux_check(tmux)?;
    let all = list_all(paths);
    if let Some(n) = name {
        if let Some(ws) = all.iter().find_map(|(_, r)| r.as_ref().ok().filter(|ws| ws.name == n)) {
            let (main_root, worktrees, moved_from) = main_by_name(git, &ws.common_dir, &ws.main_root, &paths.workspace_cache_dir())?;
            return Ok(Target {
                id: ws.id.clone(),
                name: ws.name.clone(),
                common_dir: ws.common_dir.clone(),
                top: main_root.clone(),
                main_root,
                moved_from,
                worktrees,
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
                let common_dir = PathBuf::from(text_of(v, "common_dir"));
                let (main_root, worktrees, moved_from) =
                    main_by_name(git, &common_dir, Path::new(&text_of(v, "main_root")), &paths.workspace_cache_dir())?;
                return Ok(Target {
                    id: (*id).clone(),
                    name: n.to_string(),
                    worktrees,
                    common_dir,
                    top: main_root.clone(),
                    main_root,
                    moved_from,
                    state: Err(e.to_string()),
                });
            }
            many => {
                let files: Vec<String> = many.iter().map(|(_, _, f, _)| f.display().to_string()).collect();
                return Err(fail(format!("zygo: more than one workspace state has the name {}: {}", esc(n), files.join(", "))));
            }
        }
    }
    let cache = paths.workspace_cache_dir();
    let (co, list, first) = find_checkout_with(cwd, GIT_BOUND, git, &cache).map_err(prefixed)?;
    let id = workspace_id(&co.common_dir);
    let taken: BTreeSet<String> =
        all.iter().filter(|(i, _)| *i != id).filter_map(|(_, r)| r.as_ref().ok().map(|w| w.name.clone())).collect();
    // A new workspace is named after its main worktree, the first valid one (AU2); a name, once given, does not change.
    let folder = first.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let fresh_name = || unique_name(&slug(&folder), &|n| taken.contains(n));
    let (name, state) = match Store::open(paths, &id).read() {
        Ok(Some(ws)) => (ws.name.clone(), Ok(Some(ws))),
        Ok(None) => (fresh_name(), Ok(None)),
        Err(StateError::Newer { .. }) => return Err(fail(NEWER.into())),
        Err(e @ StateError::Invalid { .. }) => (fresh_name(), Err(e.to_string())),
        Err(e) => return Err(fail(format!("zygo: {e}"))),
    };
    // D2: a saved `main_root` wins while it is valid, from any checkout of the repository (the same workspace id).
    let saved = state.as_ref().ok().and_then(|s| s.as_ref()).map(|ws| ws.main_root.as_path());
    let (main_root, moved_from) = keep_saved(saved, &list, &cache).unwrap_or((first, None));
    Ok(Target { id, name, common_dir: co.common_dir, main_root, moved_from, top: co.top, worktrees: list, state })
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
    /// FIX-29: the build holds each window (`remain-on-exit on`) until [`Builder::release`]; `repair` passes `false`.
    pub hold: bool,
}

impl Builder<'_> {
    /// `<zygo> ws run --ws-id <id> --pane <key>`, then `--shell` for a shell pane (AU15, AU46).
    pub fn argv(&self, key: &str, shell: bool) -> Vec<OsString> { pane_argv(self.zygo, self.id, key, shell) }

    fn hook(&self, event: &str) -> Result<String, MuxError> {
        hook_command(self.zygo, self.id, event).map_err(|e: UnsafePath| MuxError::Failed { message: e.to_string() })
    }

    /// The build hold (`on`, FIX-29) or `remain-on-exit off` (AU68), and the window hooks of W§7.4 on one window
    /// (fixtures 6.22n, 6.42).
    pub fn window_setup(&self, window: u32) -> Result<(), MuxError> {
        let remain = if self.hold { "on" } else { "off" };
        self.mux.set_window_option(&window_target(self.session, window), "remain-on-exit", remain)?;
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

    /// FIX-29, W§8.1 step 12: ends the build hold: `remain-on-exit off` on each window, best effort. A window that is
    /// gone prints nothing; any other error prints one line and the next window is tried. A dead pane stays dead.
    pub fn release(&self, windows: &[u32], name: &str, err: &mut dyn Write) {
        for &w in windows {
            let r = fire(self.dir, "release")
                .and_then(|()| self.mux.set_window_option(&window_target(self.session, w), "remain-on-exit", "off"));
            match r {
                Ok(()) | Err(MuxError::NoTarget { .. } | MuxError::Gone { .. }) => {}
                Err(e) => say(err, &format!("zygo: {}: could not release the build hold of window {w}: {e}", esc(name))),
            }
        }
    }

    /// FIX-29 rounds 2-3, W§8.1 step 6: [`Builder::release`] on each window of the session that holds at least one
    /// pane with a valid `@zygo-pane` (read in the same `list-panes` call), so a hold that a failed release left ends
    /// at the next `up` (AU68); a window with no zygo pane (the user's) is never changed. A gone session gives no line;
    /// any other error of the window read prints `zygo: <name>: could not release the build hold: <error>`.
    pub fn release_all(&self, name: &str, err: &mut dyn Write) {
        let rows = match fire(self.dir, "release_all").and_then(|()| self.mux.list_panes(self.session)) {
            Ok(rows) => rows,
            Err(MuxError::NoTarget { .. } | MuxError::Gone { .. }) => return,
            Err(e) => return say(err, &format!("zygo: {}: could not release the build hold: {e}", esc(name))),
        };
        let windows: BTreeSet<u32> = rows
            .iter()
            .filter_map(|r| parse_pane_line(r).ok())
            .filter(|(_, p)| valid_pane_key(&p.key))
            .map(|(w, _)| w.index)
            .collect();
        self.release(&windows.into_iter().collect::<Vec<_>>(), name, err);
    }

    /// FIX-29: one [`exited_line`] for each dead pane of the session that has a valid pane key, in `list-panes` order,
    /// best effort. A gone session or pane gives no line; any other error prints one line and goes on.
    pub fn dead_lines(&self, socket: &Path, name: &str, err: &mut dyn Write) -> Vec<String> {
        let rows = match fire(self.dir, "dead_lines").and_then(|()| self.mux.list_panes(self.session)) {
            Ok(rows) => rows,
            Err(MuxError::NoTarget { .. } | MuxError::Gone { .. }) => return Vec::new(),
            Err(e) => {
                say(err, &format!("zygo: {}: could not read the dead panes: {e}", esc(name)));
                return Vec::new();
            }
        };
        let mut lines = Vec::new();
        for row in rows {
            let Ok((_, p)) = parse_pane_line(&row) else { continue };
            if !valid_pane_key(&p.key) {
                continue;
            }
            match fire(self.dir, "dead_pane").and_then(|()| self.mux.display(&p.id, "#{pane_dead}|#{pane_dead_status}")) {
                Ok(v) => {
                    if let Some(status) = v.strip_prefix("1|") {
                        lines.push(exited_line(&p.key, status, socket, &p.id));
                    }
                }
                Err(MuxError::NoTarget { .. } | MuxError::Gone { .. }) => {}
                Err(e) => say(err, &format!("zygo: {}: could not read pane {}: {e}", esc(name), p.key)),
            }
        }
        lines
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
        let spec =
            NewWindow { session: self.session, name: w.name.as_deref(), start_dir: self.main_root, argv: &argv, hold: self.hold };
        let index = self.mux.new_window(&spec)?;
        fire(self.dir, "add_window")?;
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
    m.name = pp.name.clone();
    m.launch.profile = pp.profile.clone();
    m.launch.yolo = pp.yolo;
    m.launch.passthrough = pp.passthrough;
    m.launch.server = pp.server.clone();
    m.launch.compact = pp.compact.clone();
    m.launch.config_dir = pp.config_dir.clone();
    // R§9.5: a pool member's directory, set at plan time (the launcher callback writes the same value, AU92).
    if pp.pool {
        m.config_dir = pp.config_dir.as_ref().map(PathBuf::from);
    }
    m.launch.job = pp.job.clone();
    m.launch.definition = pp.definition.clone();
    let f = filter_from(agent, &pp.args, &pp.cwd);
    m.launch.args = f.kept;
    m.dropped_flags = f.dropped_flags;
    m
}

/// The kind and the fresh member record of one loadout pane (D30): a shell pane and an agent pane whose server has user
/// info (AU98) start a shell and get no record.
fn plan_pane(pp: &PlanPane) -> (PaneKind, Option<MemberRecord>) {
    match pp.kind {
        PlanKind::Agent(a) if !pp.server.as_deref().is_some_and(has_user_info) => (agent_kind(a), Some(fresh_member(a, pp))),
        _ => (PaneKind::Shell, None),
    }
}

/// The `panes[]` entry that a loadout `up` writes for `b` (W§7.2). The name comes from the pane's fresh record (M§6), so
/// a shell pane and an AU98 pane get none.
fn new_pane(b: &BuildPane, window: u32, index: u32, kind: PaneKind, fresh: Option<&MemberRecord>) -> Pane {
    Pane {
        key: b.key.clone(),
        window,
        index,
        active: b.active,
        cwd: b.cwd.clone(),
        kind,
        name: fresh.and_then(|m| m.name.clone()),
        ..Pane::default()
    }
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
    /// The R§9.5 pool line, printed once the step 8 write succeeds (a later refusal starts nothing).
    pool_line: Option<String>,
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

/// R§10 `up` row: the workspace files are not approved.
fn not_approved_line(full_ref: &str, digest: &str) -> String {
    format!(
        "zygo: this repository's workspace files ({}, digest {}) are not approved on this machine; nothing from them was applied; see zygo workspace approve",
        esc(full_ref),
        short(digest)
    )
}

/// R§10 `up` row: the workspace files are invalid (an invalid read has no digest).
fn invalid_line(full_ref: &str, why: &str) -> String {
    format!("zygo: this repository's workspace files ({}) are invalid ({}); nothing from them was applied", esc(full_ref), esc(why))
}

/// R§10 `up` row: the workspace files could not be read.
fn unread_line(why: &str) -> String {
    format!("zygo: this repository's workspace files are unread ({}); nothing from them was applied", esc(why))
}

/// R§9.4: the agent panes of the repository loadout against `[limits] max_members`, before any write.
fn check_max(plan: &loadout::Plan, max: u32) -> Result<(), Exit> {
    let n = plan.windows.iter().flat_map(|w| &w.panes).filter(|p| matches!(p.kind, PlanKind::Agent(_))).count();
    if n > max as usize {
        return Err(fail(format!(
            "zygo: this repository's workspace files start {n} agents; [limits] max_members is {max}; nothing started"
        )));
    }
    Ok(())
}

/// WD-10 F3, R§9.1: the `config check` rules of `job` and `pool` on the user loadout `name`, before any write (`zygo up`
/// does not run `config check`). A repository loadout is checked when its members are read.
fn check_jobs(cfg: &Config, name: &str) -> Result<(), Exit> {
    let Some(l) = cfg.loadout.get(name) else { return Ok(()) };
    let mut one = Config::default();
    one.loadout.insert(name.into(), l.clone());
    let bad = zygo_config::check(&one).into_iter().find(|v| {
        v.severity == Severity::Error && v.path.starts_with("loadout.") && (v.path.ends_with(".job") || v.path.ends_with(".pool"))
    });
    match bad {
        Some(v) => Err(fail(format!("zygo: {}; nothing started", esc(&v.to_string())))),
        None => Ok(()),
    }
}

/// R§9.5: true when a pane takes a pool directory and runs in passthrough at a fresh build: `pool = true` on a
/// claude-code pane whose server holds no user info. The rule of `apply_pool`, also used by the approve summary.
pub fn pool_member(pool: bool, claude_code: bool, server: Option<&str>) -> bool {
    pool && claude_code && !server.is_some_and(has_user_info)
}

/// R§9.5 "Local mode only": the reason `zygo up` refuses a pool with at least one member, or `None` when it does not.
pub fn pool_refused(config: &Config) -> Option<&'static str> {
    config.server.url.is_some().then_some("passthrough is local mode only, and [server] url is set")
}

/// R§9.5: the refusal of a `[workspace] pool` that `pool::expand` refuses with `why`, as `pool <text>: <why>`. `zygo up`
/// ends it with `; nothing started`; the approve summary with `; zygo up starts nothing`.
pub fn pool_unexpanded(text: &str, why: &str) -> String {
    format!("pool {}: {}", esc(text), esc(why))
}

/// R§9.5 at a fresh build: the pool members (`pool = true` on a claude-code pane that gets a member record) take the
/// directories of `[workspace] pool` in turn and run in passthrough. Checks the pool before any write; `Some` is the
/// line for the operator. No pool member, or no pool: the pool is not read.
fn apply_pool(opts: &UpOpts, plan: &mut loadout::Plan) -> Result<Option<String>, Exit> {
    let mut members: Vec<(usize, &mut PlanPane)> = plan
        .windows
        .iter_mut()
        .flat_map(|w| w.panes.iter_mut())
        .enumerate()
        .filter(|(_, pp)| {
            pool_member(pp.pool, pp.kind == PlanKind::Agent(zygo_core::Agent::ClaudeCode), pp.server.as_deref())
        })
        .collect();
    let Some(text) = opts.config.workspace.pool.as_deref().filter(|_| !members.is_empty()) else { return Ok(None) };
    let home = pool::home_in(|k| opts.vars.get(OsStr::new(k)).cloned());
    let dir = pool::expand(text, home.as_deref()).map_err(|r| fail(format!("zygo: {}; nothing started", pool_unexpanded(text, &r))))?;
    let shown = esc(&dir.to_string_lossy());
    if let Some(why) = pool_refused(&opts.config) {
        return Err(fail(format!("zygo: pool {shown}: {why}; nothing started")));
    }
    let dirs = pool::list(&dir).map_err(|r| fail(format!("zygo: pool {shown}: {}; nothing started", esc(&r))))?;
    let names: Vec<Option<String>> = members.iter().map(|(_, pp)| pp.name.clone()).collect();
    for ((_, pp), k) in members.iter_mut().zip(pool::assign(&names, dirs.len())) {
        let Some(d) = dirs[k].to_str() else {
            return Err(fail(format!("zygo: pool {shown}: a directory name is not UTF-8; nothing started")));
        };
        pp.config_dir = Some(d.into());
        pp.passthrough = true;
    }
    // D3: an unnamed member is `pane <n>`, `n` its 1-based ordinal in the plan.
    let label = |i: usize| match &members[i].1.name {
        Some(n) => esc(n),
        None => format!("pane {}", members[i].0 + 1),
    };
    let order = pool::order(&names);
    let labels: Vec<String> = order.iter().map(|&i| label(i)).collect();
    let mut line = format!("zygo: {} run in passthrough on pool {shown}", labels.join(", "));
    for &i in &order {
        if let Some(p) = &members[i].1.profile {
            line.push_str(&format!("; profile {} of {} does not apply in passthrough", esc(p), label(i)));
        }
    }
    Ok(Some(line))
}

/// R§10: true when the user config has `[loadout.repository]`, which replaces the repository loadout of the approved
/// `def`, so `zygo up` plans the user loadout. The decision of `choose`, also used by the approve summary.
pub fn loadout_shadowed(config: &Config, def: &Definition) -> bool {
    apply_to_config(&mut config.clone(), def).loadout_shadowed
}

/// The loadout of W§8.1 step 7, first case (R§10 `up` row): `--loadout`; else `[workspace] default_loadout` when the
/// user config file sets it; else `repository` when the default branch has an approved `.zygo/workspace.toml`; else
/// `solo`. The repository plan comes from one checked read (R§7.3) and each pane gets its pin (R§7.4). A choice by
/// `--loadout repository` or a user `default_loadout = "repository"` refuses where the default choice plans `solo`.
/// `repo_read` is false for `zygo up --all`, which makes no read.
fn choose(opts: &UpOpts, t: &Target, repo_read: bool, err: &mut dyn Write) -> Result<loadout::Plan, Exit> {
    let resolve_err = |e: LoadoutError| Exit {
        code: if matches!(e, LoadoutError::Unknown { .. }) { 2 } else { 1 },
        message: format!("zygo: {e}"),
    };
    let user = |name: &str| {
        let plan = loadout::resolve(&opts.config, Some(name), &t.main_root, &t.worktrees).map_err(resolve_err)?;
        check_jobs(&opts.config, name)?;
        Ok(plan)
    };
    let name = opts.loadout.as_deref().or(opts.default_loadout_set.then_some(opts.config.workspace.default_loadout.as_str()));
    let explicit = name.is_some();
    if let Some(n) = name
        && (n != REPOSITORY_LOADOUT || opts.config.loadout.contains_key(REPOSITORY_LOADOUT))
    {
        return user(n);
    }
    // R§10 `zygo up --all` row (AU10): no default-branch read; the user default, else `solo`. A user default
    // `repository` with no user table (the early return above takes one) plans `solo` with one line.
    if !repo_read {
        if name == Some(REPOSITORY_LOADOUT) {
            say(err, UP_ALL_NO_REPOSITORY);
            return user(BUILTIN_LOADOUT);
        }
        return user(name.unwrap_or(BUILTIN_LOADOUT));
    }
    let git: &dyn GitRunner = match &opts.git {
        Some(g) => g.as_ref(),
        None => &SystemGit,
    };
    let line = match check_default(&t.top, &t.id, &opts.paths.approvals_file, git) {
        Use::Approved { pin, def, .. } if def.workspace.is_some() => {
            if loadout_shadowed(&opts.config, &def) {
                return user(REPOSITORY_LOADOUT);
            }
            let mut cfg = opts.config.clone();
            apply_to_config(&mut cfg, &def);
            let plan = loadout::resolve_repository(&cfg.loadout[REPOSITORY_LOADOUT], &pin, &t.main_root, &t.worktrees)
                .map_err(resolve_err)?;
            check_max(&plan, opts.config.limits.max_members)?;
            return Ok(plan);
        }
        Use::Approved { full_ref, .. } if explicit => {
            format!("zygo: this repository's workspace files at {} have no .zygo/workspace.toml; nothing started", esc(&full_ref))
        }
        Use::NotApproved { full_ref, digest, def, .. } if explicit || def.workspace.is_some() => {
            not_approved_line(&full_ref, &digest)
        }
        Use::Invalid { full_ref, why, .. } => invalid_line(full_ref.as_deref().unwrap_or_default(), &why),
        Use::Unread { why } => unread_line(&why),
        Use::Absent { full_ref } if explicit => format!("zygo: this repository has no workspace files at {}", esc(&full_ref)),
        Use::Approved { .. } | Use::NotApproved { .. } | Use::Absent { .. } => return user(BUILTIN_LOADOUT),
    };
    if explicit {
        return Err(fail(line));
    }
    say(err, &line);
    user(BUILTIN_LOADOUT)
}

/// The loadout plan (W§8.1 step 7, first case).
fn loadout_plan(
    opts: &UpOpts, t: &Target, ws: Option<&Workspace>, repo_read: bool, report: &mut Report, err: &mut dyn Write,
) -> Result<Plan, Exit> {
    let mut plan = choose(opts, t, repo_read, err)?;
    for w in &plan.warnings {
        say(err, &format!("zygo: {w}"));
    }
    let pool_line = apply_pool(opts, &mut plan)?;
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
        pool_line,
    };
    for (wi, pw) in plan.windows.iter().enumerate() {
        let mut panes = Vec::with_capacity(pw.panes.len());
        for (pi, pp) in pw.panes.iter().enumerate() {
            let mut key = new_pane_key();
            while taken.contains(&key) {
                key = new_pane_key();
            }
            taken.insert(key.clone());
            let (kind, member) = plan_pane(pp);
            if member.is_none() && matches!(pp.kind, PlanKind::Agent(_)) {
                report.au98.push(AU98.into());
            }
            let shell = kind == PaneKind::Shell;
            out.kinds.insert(key.clone(), kind);
            if let Some(m) = member {
                out.fresh.insert(key.clone(), m);
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
        pool_line: None,
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

/// W§8.1 steps 4-12 for one target, and 13 when `attach` (crate-private, for W8). `repo_read` is false for `--all`:
/// its loadout choice makes no default-branch read (R§10).
pub(crate) fn up_target(
    opts: &UpOpts, target: Target, out: &mut dyn Write, err: &mut dyn Write, attach: bool, repo_read: bool,
) -> i32 {
    // D3: one line when the saved `main_root` is no longer a valid worktree and this `up` replaces it.
    if let Some(old) = &target.moved_from {
        say(err, &moved_line(old, &target.main_root));
    }
    match build(opts, target, out, err, attach, repo_read) {
        Ok(code) => code,
        Err(e) => {
            say(err, &e.message);
            e.code
        }
    }
}

fn build(opts: &UpOpts, target: Target, out: &mut dyn Write, err: &mut dyn Write, attach: bool, repo_read: bool) -> Result<i32, Exit> {
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
        // FIX-29 rounds 2-3: the AU54 check of the saved socket, in every phase, before any use (as step 9).
        check_saved_socket(cur).map_err(|e| Exit { code: 2, message: e.to_string() })?;
        if let Some(sock) = &cur.tmux_socket {
            socket_is_private(sock, true).map_err(|t| fail(format!("zygo: {t}")))?;
        }
        let mux = server(&opts.tmux, cur.tmux_socket.as_deref(), opts.tmux_env.as_deref());
        check_server(&mux).map_err(|e| Exit { code: 2, message: e.to_string() })?;
        repair(&store, &mux, &opts.zygo, out, err).map_err(|e| {
            let (code, message) = repair_failure(&e);
            Exit { code, message }
        })?;
        ws = read()?;
        let socket = ws.as_ref().and_then(|w| w.tmux_socket.clone());
        let _ = snapshot::after_write(&opts.paths, &Tmux::new(opts.tmux.clone(), socket), &id, snapshot::OP_WAIT);
    }
    // Step 6.
    let quiet = opts.no_attach || opts.zygo_session.is_some();
    let saved_socket = ws.as_ref().and_then(|w| w.tmux_socket.clone());
    let mux6 = server(&opts.tmux, saved_socket.as_deref(), opts.tmux_env.as_deref());
    // FIX-30 (AU54): with no saved socket this is the socket of `$TMUX`, before its first tmux call.
    check_server(&mux6).map_err(|e| Exit { code: 2, message: e.to_string() })?;
    match mux6.session_exists(&session) {
        SessionAnswer::Exists => {
            owner_check(&mux6, &session, &id, &name)?;
            if opts.reset || opts.loadout.is_some() {
                return Err(fail(format!("zygo: {name} is running; run zygo down -- {name} first")));
            }
            // FIX-29 round 2 (AU68): end any build hold that a failed release left, best effort.
            let b6 = Builder {
                mux: &mux6,
                session: &session,
                zygo: &opts.zygo,
                id: &id,
                main_root: &target.main_root,
                dir: store.dir(),
                hold: false,
            };
            b6.release_all(&name, err);
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
        _ => loadout_plan(opts, &target, ws.as_ref(), repo_read, &mut report, err)?,
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
    // ask the same server (Round 2 M3); a `$TMUX` socket gets the AU62 check before it is written. Its AU54 check ran
    // at step 6 (FIX-30).
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
                    .map(|(window, i, p)| {
                        let kind = plan.kinds.get(&p.key).copied().unwrap_or_default();
                        new_pane(p, window, i as u32, kind, plan.fresh.get(&p.key))
                    })
                    .collect();
                for (k, m) in &plan.fresh {
                    cur.members.insert(k.clone(), m.clone());
                }
            } else {
                for p in cur.panes.iter_mut() {
                    if let Some(k) = plan.kinds.get(&p.key) {
                        p.kind = *k;
                        // M§6: a pane restored as a shell (its record not resumed, or forgotten below) has no name.
                        if *k == PaneKind::Shell {
                            p.name = None;
                        }
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
    if let Some(l) = &plan.pool_line {
        say(err, l);
    }
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
                // M§5 P1: after the undo write, which put the `plan.forget` records back, the maildirs of the fresh
                // records that `take_back_plan` dropped go.
                if let Err(e) = mail::prune(&store, MAIL_LOCK_WAIT) {
                    say(err, &not_pruned_line(&e));
                }
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
    let b9 = Builder {
        mux: &mux9,
        session: &session,
        zygo: &opts.zygo,
        id: &id,
        main_root: &target.main_root,
        dir: store.dir(),
        hold: true,
    };
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
    let b = Builder {
        mux: &mux,
        session: &session,
        zygo: &opts.zygo,
        id: &id,
        main_root: &target.main_root,
        dir: store.dir(),
        hold: true,
    };
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
    // FIX-29: end the build hold before `phase = up`, so a stop during the hold leaves `building` (AU67); then one
    // line per pane that exited while held (W§8.1 step 12).
    b.release(&real, &name, err);
    let exited = b.dead_lines(&socket, &name, err);
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
    // M§5 P1: after `phase = up`, the maildirs of the records this `up` dropped (`--reset`, `plan.forget`, members
    // not built) go, under mail/lock and the state read there.
    if let Err(e) = mail::prune(&store, MAIL_LOCK_WAIT) {
        say(err, &not_pruned_line(&e));
    }
    // D23; M§9: the unread count when it is not zero.
    let mail = mail_dir(&opts.paths, &id);
    let detached: Vec<String> = done
        .members
        .iter()
        .filter(|(k, m)| m.state == MemberState::Resumable && !planned_keys.contains(*k) && !not_built.contains(*k))
        .filter_map(|(k, m)| {
            m.agent_session.as_deref().map(|s| {
                let line = format!(
                    "zygo: {} agent session {} is detached (its pane was closed); resume it by hand or run zygo ws forget-pane {k}",
                    kind_name(m),
                    esc(s)
                );
                match mail_store::unread(&mail, k) {
                    Ok(n) if n > 0 => format!("{line}; unread {n}"),
                    _ => line,
                }
            })
        })
        .collect();
    for line in report.au98.iter().chain(&report.au70).chain(&report.au30).chain(&report.no_id).chain(&detached).chain(&au96).chain(&exited) {
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
        Ok(t) => up_target(opts, t, out, err, true, true),
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
        match session_check(opts, &ws, err) {
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
        if up_target(opts, target, out, err, false, false) == 0 {
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
fn session_check(opts: &UpOpts, ws: &Workspace, err: &mut dyn Write) -> Found {
    // FIX-29 round 3 (AU54): the saved socket before any use.
    if let Err(e) = check_saved_socket(ws) {
        return Found::Skip(e.to_string().trim_start_matches("zygo: ").to_string());
    }
    if let Some(sock) = &ws.tmux_socket
        && let Err(t) = socket_is_private(sock, true)
    {
        return Found::Skip(t);
    }
    let mux = server(&opts.tmux, ws.tmux_socket.as_deref(), opts.tmux_env.as_deref());
    // FIX-30 (AU54): with no saved socket this is the socket of `$TMUX`, before its first tmux call.
    if let Err(e) = check_server(&mux) {
        return Found::Skip(e.to_string().trim_start_matches("zygo: ").to_string());
    }
    match mux.session_exists(&ws.tmux_session) {
        SessionAnswer::Gone => Found::Chosen,
        SessionAnswer::Error { message } => Found::Skip(format!("tmux: {message}")),
        SessionAnswer::Exists => match clear_down_all(opts, ws, &mux, err) {
            Ok(()) => Found::Running,
            Err(reason) => Found::Skip(reason),
        },
    }
}

/// D6: a session that exists is not restored. When it holds this workspace's `@zygo-ws`, under `op.lock` and after a
/// second session and owner check, the build hold that a failed release left ends ([`Builder::release_all`], AU68,
/// FIX-29 round 3); then, with `down_all = true`, the workspace is up: `down_all` is cleared in one write under
/// `lock`, then the W§7.3 `snapshot.pending` check. A held `op.lock` changes nothing; an `op.lock` that cannot be taken
/// for another reason is a skip only with `down_all = true`. `Err` holds the skip reason.
fn clear_down_all(opts: &UpOpts, ws: &Workspace, mux: &dyn Multiplexer, err: &mut dyn Write) -> Result<(), String> {
    let ours = |mux: &dyn Multiplexer| match mux.show_session_option(&ws.tmux_session, WS_OPTION) {
        Ok(v) => Ok(v.as_deref() == Some(ws.id.as_str())),
        Err(e) => Err(format!("tmux: {e}")),
    };
    if !ours(mux)? {
        return Ok(());
    }
    let store = Store::open(&opts.paths, &ws.id);
    let _op = match store.op_lock() {
        Ok(l) => l,
        Err(StateError::OpHeld) => return Ok(()),
        Err(e) if ws.down_all => return Err(e.to_string()),
        Err(_) => return Ok(()),
    };
    if mux.session_exists(&ws.tmux_session) != SessionAnswer::Exists || !ours(mux)? {
        return Ok(());
    }
    let b = Builder {
        mux,
        session: &ws.tmux_session,
        zygo: &opts.zygo,
        id: &ws.id,
        main_root: &ws.main_root,
        dir: store.dir(),
        hold: false,
    };
    b.release_all(&ws.name, err);
    if !ws.down_all {
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
    // FIX-29 round 3 (AU54): the saved socket before any use; the AU100 line quotes it.
    if let Err(e) = check_saved_socket(&ws) {
        say(err, &e.to_string());
        return 2;
    }
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

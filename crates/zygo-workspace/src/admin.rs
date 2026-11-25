//! `ls`, `workspace`, `ws forget`, `ws forget-pane`, `ws name`. Owner: node W8 (W§12); `ws name`: node ML-8 (M§6).
//!
//! The library reads no process environment: the tmux program, `$TMUX` and `ZYGO_WORKSPACE` come in as inputs, and
//! `forget` and `forget_pane` write their lines to the writers they are given.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail};
use zygo_core::esc;
use zygo_core::ids::valid_member_name;
use zygo_core::paths::Paths;

use crate::identity::{valid_name, valid_pane_key, valid_ws_id, workspace_id};
use crate::mail::names;
use crate::mux::tmux::Tmux;
use crate::mux::{check_path, session_target, Multiplexer, MuxError, SessionAnswer, BOUND, UNSAFE_PATH_MESSAGE};
use crate::repair::check_saved_socket;
use crate::snapshot;
use crate::state::{list_all, lock_file, read_state_capped, StateBytes, MemberRecord, MemberState, OpLock, PaneKind, Phase, StateError, Store, Workspace};
use crate::up::{check_server, find_checkout, fire, member_live, server, socket_is_private, Exit, GIT_BOUND};

/// The git bound of `status`, so `/zygo:workspace` fits the 4.5 s hook bound (D12, D23).
const STATUS_GIT_BOUND: Duration = Duration::from_secs(2);
/// `ls` makes no tmux call after this time from its start (D11).
const LIST_DEADLINE: Duration = Duration::from_secs(4);
const WS_OPTION: &str = "@zygo-ws";
const NEWER: &str = "zygo: workspace state is from a newer zygo; run zygo update";
const NO_STATUS: &str = "no workspace for this session or folder";
const NO_FOLDER: &str = "zygo: no workspace for this folder; run in the workspace's checkout or pass --name";
const NO_FOLDER_WS: &str = "zygo: no workspace for this folder; run in the workspace's checkout or pass --workspace";
/// The largest state file that W8 reads (Round 2 R6).
const STATE_MAX: u64 = 1024 * 1024;

/// What `symlink_metadata` says of a path; a link is never followed (Round 2 R1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Real,
    Link,
    Missing,
    /// A file or another entry that is not a folder, or a path that cannot be read.
    Other,
}

fn kind(path: &Path) -> Kind {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Kind::Link,
        Ok(m) if m.is_dir() => Kind::Real,
        Ok(_) => Kind::Other,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Kind::Missing,
        Err(_) => Kind::Other,
    }
}

/// The kind of the state folder of `id`.
fn state_kind(paths: &Paths, id: &str) -> Kind { kind(&paths.workspaces_dir().join(id)) }

/// Whether `workspace.json` of `dir` is larger than [`STATE_MAX`].
fn too_large(dir: &Path) -> bool { fs::metadata(dir.join("workspace.json")).is_ok_and(|m| m.len() > STATE_MAX) }

/// The `TooLarge` error of the state file of `dir` (FIX-17 Round 2 F1).
fn too_large_err(dir: &Path) -> StateError { StateError::TooLarge { path: dir.join("workspace.json") } }

/// [`Store::read`] with the cap of [`STATE_MAX`]: a larger file is `TooLarge` (Round 2 R6; FIX-17 Round 2 F1). The
/// store reads with the same cap, so this check agrees with it.
fn read_capped(store: &Store) -> Result<Option<Workspace>, StateError> {
    if too_large(store.dir()) {
        return Err(too_large_err(store.dir()));
    }
    store.read()
}

/// The JSON of `workspace.json` of `dir`, read with the store's cap in the read itself ([`read_state_capped`]), so a
/// file that grows after any earlier check is still not read past 1 MiB + 1 byte (FIX-17 Round 2 F2). A larger file is
/// `Err(TooLarge)`; a file that is missing, a link, not a regular file or not JSON is `Ok(None)`.
fn raw_json(dir: &Path) -> Result<Option<serde_json::Value>, StateError> {
    let _ = fire(dir, "raw_json_before_read");
    match read_state_capped(&dir.join("workspace.json")) {
        Ok(StateBytes::Bytes(b)) => Ok(serde_json::from_slice(&b).ok()),
        Ok(StateBytes::TooLarge) => Err(too_large_err(dir)),
        Ok(StateBytes::Link | StateBytes::NotFile) | Err(_) => Ok(None),
    }
}

/// The state column of `ls` (W§12, D8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListState {
    Up,
    Down,
    Building,
    Draining,
    Stopping,
    StoppedTmuxGone,
    Unreachable,
    RepoMissing,
    Unreadable,
    /// No answer from tmux: `no tmux`, `tmux timed out` or `tmux error`.
    Unknown(&'static str),
}

impl fmt::Display for ListState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListState::Up => f.write_str("up"),
            ListState::Down => f.write_str("down"),
            ListState::Building => f.write_str("building"),
            ListState::Draining => f.write_str("draining"),
            ListState::Stopping => f.write_str("stopping"),
            ListState::StoppedTmuxGone => f.write_str("stopped (tmux gone)"),
            ListState::Unreachable => f.write_str("unreachable"),
            ListState::RepoMissing => f.write_str("repo missing"),
            ListState::Unreadable => f.write_str("unreadable"),
            ListState::Unknown(reason) => write!(f, "unknown ({reason})"),
        }
    }
}

/// One line of `ls` (D10). A state that cannot be read has its id as `name` and no other value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    pub id: String,
    pub name: String,
    pub main_root: Option<PathBuf>,
    pub state: ListState,
    /// Running, saved and detached members (D9).
    pub agents: Option<(usize, usize, usize)>,
    /// Attached clients, only for a session of this workspace.
    pub clients: Option<u32>,
}

impl fmt::Display for ListRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let root = self.main_root.as_ref().map_or_else(|| "-".to_string(), |p| esc(&p.to_string_lossy()));
        let agents = self.agents.map_or_else(|| "-".to_string(), |(r, s, d)| format!("{r}/{s}/{d}"));
        let clients = self.clients.map_or_else(|| "-".to_string(), |n| n.to_string());
        write!(f, "{}  {root}  {}  agents {agents}  clients {clients}", esc(&self.name), self.state)
    }
}

/// Test hooks of `ls` (D11). Only the `test-hooks` feature builds them.
#[cfg(feature = "test-hooks")]
pub mod testing {
    use std::path::Path;
    use std::time::Instant;

    use zygo_core::paths::Paths;

    use super::ListRow;
    use crate::mux::Multiplexer;

    /// The `ls` rows with an injected multiplexer per socket and an injected clock.
    pub fn list_by(
        paths: &Paths, no_tmux: bool, mux_of: &dyn Fn(&Path) -> Box<dyn Multiplexer>, now: &dyn Fn() -> Instant,
    ) -> Vec<ListRow> {
        super::list_by(paths, no_tmux, mux_of, now)
    }
}

fn say(w: &mut dyn Write, line: &str) { let _ = writeln!(w, "{line}"); }

/// FIX-30: the error of `forget`: the AU54 refusal (exit 2, as `up`, `down` and `attach`) or any other line (exit 1).
enum ForgetError {
    Unsafe(String),
    Line(String),
}

impl From<String> for ForgetError {
    fn from(line: String) -> ForgetError { ForgetError::Line(line) }
}

impl ForgetError {
    fn text(&self) -> &str {
        match self {
            ForgetError::Unsafe(t) | ForgetError::Line(t) => t,
        }
    }
}

/// FIX-30: the exit code of a `forget` error: 2 for the AU54 refusal, else 1.
fn exit_code(e: &ForgetError) -> i32 {
    match e {
        ForgetError::Unsafe(_) => 2,
        ForgetError::Line(_) => 1,
    }
}

/// FIX-30 (AU54, W§12 `ws forget`): the refusal of a socket that fails AU54 right before the first tmux call. A saved
/// socket also refuses `up` and `down`, so the line names the state folder to remove by hand (escaped, C6); the socket
/// of `$TMUX` is not the state's fault, so its line is the AU54 line alone.
fn unsafe_socket(store: &Store, saved: bool) -> ForgetError {
    if saved {
        ForgetError::Unsafe(format!(
            "{UNSAFE_PATH_MESSAGE}; remove {} by hand to forget this workspace",
            esc(&store.dir().display().to_string())
        ))
    } else {
        ForgetError::Unsafe(UNSAFE_PATH_MESSAGE.to_string())
    }
}

/// `zygo ls` (W§12, D8-D11): one row per workspace, sorted by (name, id). It never fails. The tmux version probe
/// counts toward the 4 s deadline: the deadline runs from the start of `list`.
pub fn list(paths: &Paths, tmux: &Path) -> Vec<ListRow> {
    let start = Instant::now();
    let no_tmux = matches!(Tmux::new(tmux.to_path_buf(), None).version(), Err(MuxError::Spawn { .. }));
    let first = Cell::new(true);
    let now = || if first.replace(false) { start } else { Instant::now() };
    list_by(paths, no_tmux, &|s: &Path| Box::new(Tmux::new(tmux.to_path_buf(), Some(s.to_path_buf()))), &now)
}

/// What tmux said about the session of one row.
enum Seen {
    Unknown(&'static str),
    /// No server known, or a socket that fails AU62.
    Unreachable,
    Gone,
    /// The session holds this workspace's `@zygo-ws`; the attached clients when read.
    Ours(Option<u32>),
    /// The session exists with another `@zygo-ws` or none.
    Foreign,
}

/// The reason of a failed call (D11).
fn reason_of(e: &MuxError) -> &'static str {
    if matches!(e, MuxError::TimedOut) { "tmux timed out" } else { "tmux error" }
}

fn list_by(
    paths: &Paths, no_tmux: bool, mux_of: &dyn Fn(&Path) -> Box<dyn Multiplexer>, now: &dyn Fn() -> Instant,
) -> Vec<ListRow> {
    let start = now();
    let may_call = || now() + BOUND <= start + LIST_DEADLINE;
    let mut all = list_all(paths);
    let label = |id: &String, r: &Result<Workspace, StateError>| r.as_ref().map_or_else(|_| id.clone(), |w| w.name.clone());
    all.sort_by(|(ia, ra), (ib, rb)| (label(ia, ra), ia).cmp(&(label(ib, rb), ib)));
    let mut failed: BTreeMap<PathBuf, &'static str> = BTreeMap::new();
    let mut rows = Vec::new();
    for (id, r) in all {
        let ws = match r {
            Ok(ws) => ws,
            Err(_) => {
                rows.push(ListRow { id: id.clone(), name: id, main_root: None, state: ListState::Unreadable, agents: None, clients: None });
                continue;
            }
        };
        let store = Store::open(paths, &id);
        let agents = agents(&store, &ws);
        let repo_missing = !ws.main_root.is_dir() || !ws.common_dir.is_dir();
        let (state, clients) = if ws.phase == Phase::Down {
            (if repo_missing { ListState::RepoMissing } else { ListState::Down }, None)
        } else {
            let seen = ask(&ws, no_tmux, &mut failed, mux_of, &may_call);
            let clients = match seen {
                Seen::Ours(c) => c,
                _ => None,
            };
            let state = match ws.phase {
                Phase::Building => ListState::Building,
                Phase::Draining => ListState::Draining,
                Phase::Stopping => ListState::Stopping,
                Phase::Down | Phase::Up => match seen {
                    Seen::Unknown(reason) => ListState::Unknown(reason),
                    Seen::Unreachable | Seen::Foreign => ListState::Unreachable,
                    Seen::Ours(_) => ListState::Up,
                    Seen::Gone if store.live_launcher(&ws).is_some() => ListState::Unreachable,
                    Seen::Gone if repo_missing => ListState::RepoMissing,
                    Seen::Gone => ListState::StoppedTmuxGone,
                },
            };
            (state, clients)
        };
        rows.push(ListRow { id, name: ws.name.clone(), main_root: Some(ws.main_root.clone()), state, agents: Some(agents), clients });
    }
    rows
}

/// The session check of one row (D8, D11): no call without tmux, without a known server, on a socket that fails AU54,
/// on a failed socket, on a socket whose folder is missing or that fails AU62, or past the deadline.
fn ask(
    ws: &Workspace, no_tmux: bool, failed: &mut BTreeMap<PathBuf, &'static str>, mux_of: &dyn Fn(&Path) -> Box<dyn Multiplexer>,
    may_call: &dyn Fn() -> bool,
) -> Seen {
    if no_tmux {
        return Seen::Unknown("no tmux");
    }
    // FIX-30 (AU54): the saved socket before any use; `ls` never fails, so the row is `unreachable`, with no call.
    if check_saved_socket(ws).is_err() {
        return Seen::Unreachable;
    }
    let Some(sock) = ws.tmux_socket.as_deref() else {
        return Seen::Unreachable;
    };
    if let Some(reason) = failed.get(sock) {
        return Seen::Unknown(reason);
    }
    if parent_missing(sock) {
        return Seen::Gone;
    }
    if socket_is_private(sock, false).is_err() {
        return Seen::Unreachable;
    }
    if !may_call() {
        return Seen::Unknown("tmux timed out");
    }
    let mux = mux_of(sock);
    match mux.session_exists(&ws.tmux_session) {
        SessionAnswer::Gone => return Seen::Gone,
        SessionAnswer::Error { message } => {
            let reason = if message == MuxError::TimedOut.to_string() { "tmux timed out" } else { "tmux error" };
            failed.insert(sock.to_path_buf(), reason);
            return Seen::Unknown(reason);
        }
        SessionAnswer::Exists => {}
    }
    if !may_call() {
        return Seen::Unknown("tmux timed out");
    }
    match mux.show_session_option(&ws.tmux_session, WS_OPTION) {
        Ok(Some(v)) if v == ws.id => {}
        Ok(_) => return Seen::Foreign,
        Err(e) => {
            let reason = reason_of(&e);
            failed.insert(sock.to_path_buf(), reason);
            return Seen::Unknown(reason);
        }
    }
    if !may_call() {
        return Seen::Ours(None);
    }
    match mux.display(&session_target(&ws.tmux_session), "#{session_attached}") {
        Ok(s) => Seen::Ours(s.trim().parse().ok()),
        Err(e) => {
            failed.insert(sock.to_path_buf(), reason_of(&e));
            Seen::Ours(None)
        }
    }
}

/// Running, saved and detached members (D9).
fn agents(store: &Store, ws: &Workspace) -> (usize, usize, usize) {
    let (mut running, mut saved, mut detached) = (0, 0, 0);
    for (key, m) in &ws.members {
        if member_live(store, ws, key) {
            running += 1;
        } else if matches!(m.state, MemberState::Running | MemberState::Resumable) && m.agent_session.is_some() {
            if ws.panes.iter().any(|p| p.key == *key) {
                saved += 1;
            } else {
                detached += 1;
            }
        }
    }
    (running, saved, detached)
}

/// Whether the folder of the socket `sock` is missing: no server can listen there. An empty parent is not missing,
/// so the AU62 check refuses it.
fn parent_missing(sock: &Path) -> bool {
    match sock.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => matches!(fs::symlink_metadata(parent), Err(e) if e.kind() == std::io::ErrorKind::NotFound),
        None => false,
    }
}

fn phase_word(p: Phase) -> &'static str {
    match p {
        Phase::Building => "building",
        Phase::Up => "up",
        Phase::Draining => "draining",
        Phase::Stopping => "stopping",
        Phase::Down => "down",
    }
}

fn state_word(s: MemberState) -> &'static str {
    match s {
        MemberState::Running => "running",
        MemberState::Resumable => "resumable",
        MemberState::Ended => "ended",
    }
}

/// `zygo workspace` (W§12, D12, D13): the workspace of `ZYGO_WORKSPACE`, else of the checkout of `cwd`. The text has
/// no trailing new line; an `Err` text has no `zygo: ` prefix.
pub fn status(paths: &Paths, cwd: &Path, zygo_workspace: Option<&OsStr>) -> anyhow::Result<String> {
    let (id, by_env) = match zygo_workspace {
        Some(v) => {
            let id = v
                .to_str()
                .filter(|s| valid_ws_id(s))
                .ok_or_else(|| anyhow!("invalid workspace id {}", esc(&v.to_string_lossy())))?;
            (id.to_string(), true)
        }
        None => {
            let (co, _, _) = find_checkout(cwd, STATUS_GIT_BOUND).map_err(|_| anyhow!(NO_STATUS))?;
            (workspace_id(&co.common_dir), false)
        }
    };
    // Round 2 R1: a state folder that is a link is no state; it is not opened.
    let read = match state_kind(paths, &id) {
        Kind::Real => read_capped(&Store::open(paths, &id)),
        _ => Ok(None),
    };
    let ws = match read {
        Ok(Some(ws)) => ws,
        Ok(None) if by_env => bail!("no workspace with id {id}"),
        Ok(None) => bail!(NO_STATUS),
        Err(e) => bail!("{e}"),
    };
    let mut lines = vec![format!("workspace {}: {}", esc(&ws.name), phase_word(ws.phase))];
    for (key, m) in &ws.members {
        let session = m.agent_session.as_deref().map_or_else(|| "-".to_string(), esc);
        lines.push(format!("{key} {} {} {session}", m.launch.kind.as_str(), state_word(m.state)));
    }
    if ws.members.is_empty() {
        lines.push("no members".to_string());
    }
    Ok(lines.join("\n"))
}

/// Removes `path`: a real folder with everything in it, any other entry (a symbolic link, a file) as itself, so a link
/// never makes it remove what the link points to. A missing path is `Ok` (D16).
fn remove_tree(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
    }
}

/// The folder name a state directory is renamed to before it is removed (D16, D26).
fn tombstone(paths: &Paths, id: &str) -> PathBuf { paths.workspaces_dir().join(format!(".forget-{id}-{}", std::process::id())) }

/// The entries of `dir` with their names and whether each is a real folder (a link is not followed).
fn entries_of(dir: &Path) -> Vec<(String, PathBuf, bool)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            e.file_name().into_string().ok().map(|n| (n, e.path(), is_dir))
        })
        .collect()
}

/// Whether `dir` holds no `workspace.json`.
fn no_state(dir: &Path) -> bool {
    matches!(fs::symlink_metadata(dir.join("workspace.json")), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
}

/// D26 and Round 2 R2: removes the folders that an earlier `forget` left (`.forget-*`), the state directories with no
/// `workspace.json` whose `op.lock` and `lock` are free, and the results caches of ids that have no state folder (none,
/// or a link or file in its place) or a state folder with no `workspace.json` whose locks are free. A link is never
/// followed, opened or locked. Every error is ignored: the next `forget` tries again.
fn sweep(paths: &Paths) {
    sweep_states(paths);
    for (name, path, _) in entries_of(&paths.workspace_cache_dir()) {
        if !valid_ws_id(&name) {
            continue;
        }
        let state = paths.workspaces_dir().join(&name);
        match kind(&state) {
            Kind::Missing | Kind::Link => {
                let _ = remove_tree(&path);
            }
            Kind::Other if fs::symlink_metadata(&state).is_ok() => {
                let _ = remove_tree(&path);
            }
            Kind::Real if no_state(&state) => {
                let Ok(_op) = Store::open(paths, &name).op_lock() else {
                    continue;
                };
                let Ok(Some(_lock)) = lock_file(&state.join("lock"), Duration::ZERO) else {
                    continue;
                };
                if no_state(&state) {
                    let _ = remove_tree(&path);
                }
            }
            Kind::Real | Kind::Other => {}
        }
    }
}

/// The state-folder part of [`sweep`] (D26).
fn sweep_states(paths: &Paths) {
    for (name, path, is_dir) in entries_of(&paths.workspaces_dir()) {
        if name.starts_with(".forget-") {
            let _ = remove_tree(&path);
            continue;
        }
        if !is_dir || !valid_ws_id(&name) || !no_state(&path) {
            continue;
        }
        let Ok(_op) = Store::open(paths, &name).op_lock() else {
            continue;
        };
        let Ok(Some(_lock)) = lock_file(&path.join("lock"), Duration::ZERO) else {
            continue;
        };
        if !no_state(&path) {
            continue;
        }
        let tomb = tombstone(paths, &name);
        if fs::rename(&path, &tomb).is_ok() {
            let _ = remove_tree(&tomb);
        }
    }
}

/// The workspace that `forget` or `forget-pane` names (D14).
enum Found {
    Readable(Box<Workspace>),
    /// A state that does not parse, fails AU84 or is larger than 1 MiB, by its id.
    Invalid(String),
    /// `<workspaces_dir>/<id>` is a symbolic link, not a state folder (Round 2 R1).
    Link(String),
}

/// D14: the id first, then the readable states by name, then a newer state by its JSON `name`. `for_pane` gives the
/// `forget-pane` form of the line for a shared name.
fn find(paths: &Paths, arg: &str, for_pane: bool) -> Result<Found, Exit> {
    let fail = |message: String| Exit { code: 1, message };
    if valid_ws_id(arg) {
        match state_kind(paths, arg) {
            Kind::Link => return Ok(Found::Link(arg.to_string())),
            Kind::Real => match read_capped(&Store::open(paths, arg)) {
                Ok(Some(ws)) => return Ok(Found::Readable(Box::new(ws))),
                Ok(None) => {}
                Err(StateError::Newer { .. }) => return Err(fail(NEWER.into())),
                // FIX-17 Round 2 F1 (AU29): it may be from a newer zygo, so it is refused before any lock or write.
                Err(e @ StateError::TooLarge { .. }) => return Err(fail(format!("zygo: {e}"))),
                Err(_) => return Ok(Found::Invalid(arg.to_string())),
            },
            Kind::Missing | Kind::Other => {}
        }
    }
    let all = list_all(paths);
    // Round 2 R1: only a real state folder is a candidate (as `list_all` and `sweep` count them).
    let mut named: Vec<&Workspace> = all
        .iter()
        .filter_map(|(_, r)| r.as_ref().ok().filter(|w| w.name == arg))
        .filter(|w| state_kind(paths, &w.id) == Kind::Real)
        .collect();
    match named.len() {
        0 => {}
        1 => {
            let ws = named.remove(0);
            // The file grew after `list_all` read it (FIX-17 Round 2 F1: refused, as by id).
            let dir = paths.workspaces_dir().join(&ws.id);
            if too_large(&dir) {
                return Err(fail(format!("zygo: {}", too_large_err(&dir))));
            }
            return Ok(Found::Readable(Box::new(ws.clone())));
        }
        _ => {
            let tail = if for_pane {
                "run in its checkout".to_string()
            } else {
                let ids: Vec<&str> = named.iter().map(|w| w.id.as_str()).collect();
                format!("forget it by id: {}", ids.join(", "))
            };
            return Err(fail(format!("zygo: more than one workspace has the name {}; {tail}", esc(arg))));
        }
    }
    for (id, r) in &all {
        let Err(StateError::Newer { .. }) = r else { continue };
        let dir = paths.workspaces_dir().join(id);
        if kind(&dir) != Kind::Real {
            continue;
        }
        let name = raw_json(&dir).ok().flatten().and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string));
        if name.as_deref() == Some(arg) {
            return Err(fail(NEWER.into()));
        }
    }
    Err(fail(format!("zygo: no workspace named {}", esc(arg))))
}

/// `zygo ws forget <name>` (W§12, D14-D17, D26): exit 0 with `zygo: forgot <label>` on `out`, or exit 1 with one line on
/// `err` (exit 2 for the AU54 refusal, FIX-30). `tmux_env` is `$TMUX`.
pub fn forget(paths: &Paths, tmux: &Path, tmux_env: Option<&OsStr>, name: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    sweep(paths);
    match forget_run(paths, tmux, tmux_env, name) {
        Ok(line) => {
            say(out, &line);
            0
        }
        Err(e) => {
            say(err, e.text());
            exit_code(&e)
        }
    }
}

fn forget_run(paths: &Paths, tmux: &Path, tmux_env: Option<&OsStr>, arg: &str) -> Result<String, ForgetError> {
    let found = find(paths, arg, false).map_err(|e| e.message)?;
    let (id, first_label) = match &found {
        Found::Readable(ws) => (ws.id.clone(), ws.name.clone()),
        Found::Invalid(id) => (id.clone(), id.clone()),
        Found::Link(id) => return forget_link(paths, id).map_err(ForgetError::Line),
    };
    let store = Store::open(paths, &id);
    fire(store.dir(), "forget_before_op_lock").map_err(|e| format!("zygo: cannot forget {first_label}: {e}"))?;
    // Round 2 R1: `op_lock` creates and chmods its folder, so the folder is checked again right before it.
    match state_kind(paths, &id) {
        Kind::Real => {}
        Kind::Link => return forget_link(paths, &id).map_err(ForgetError::Line),
        Kind::Missing | Kind::Other => return Err(ForgetError::Line(format!("zygo: no workspace named {}", esc(arg)))),
    }
    let op = match store.op_lock() {
        Ok(l) => l,
        Err(StateError::OpHeld) => return Err(ForgetError::Line(format!("zygo: up or down is in progress for {first_label}"))),
        Err(e) => return Err(ForgetError::Line(format!("zygo: {e}"))),
    };
    // D15: the state again under `op.lock`.
    let label = match read_capped(&store) {
        Ok(None) => return Err(ForgetError::Line(format!("zygo: no workspace named {}", esc(arg)))),
        Err(StateError::Newer { .. }) => return Err(ForgetError::Line(NEWER.into())),
        Err(e @ StateError::TooLarge { .. }) => return Err(ForgetError::Line(format!("zygo: {e}"))),
        Ok(Some(ws)) => {
            check_readable(&store, &ws, tmux, tmux_env)?;
            ws.name
        }
        Err(_) => {
            check_invalid(&store, &id, tmux, tmux_env)?;
            id.clone()
        }
    };
    remove(paths, &store, &id, &label, op).map_err(ForgetError::Line)
}

/// Round 2 R1: `<workspaces_dir>/<id>` is a link: it is removed as a link (an unlink of the link itself, after one more
/// `symlink_metadata` right before), and nothing it points to is opened, locked, written or changed.
fn forget_link(paths: &Paths, id: &str) -> Result<String, String> {
    let link = paths.workspaces_dir().join(id);
    match kind(&link) {
        Kind::Link => fs::remove_file(&link).map_err(|e| format!("zygo: cannot forget {id}: {e}"))?,
        Kind::Missing => return Err(format!("zygo: no workspace named {id}")),
        Kind::Real | Kind::Other => {
            return Err(format!("zygo: cannot forget {id}: {} changed during the check; try again", link.display()));
        }
    }
    Ok(format!("zygo: {} was a link, not a state folder; removed the link", link.display()))
}

/// Whether no tmux can run here: then no session exists, and the session check is skipped (D15, D17).
fn no_tmux(tmux: &Path) -> bool { matches!(Tmux::new(tmux.to_path_buf(), None).version(), Err(MuxError::Spawn { .. })) }

/// D15: a live launcher, a socket that fails AU62, the workspace's own session, or a tmux error refuses; FIX-30: the
/// socket of the session check (saved or from `$TMUX`) that fails AU54 refuses with exit 2, right before that call.
fn check_readable(store: &Store, ws: &Workspace, tmux: &Path, tmux_env: Option<&OsStr>) -> Result<(), ForgetError> {
    if let Some(key) = store.live_launcher(ws) {
        return Err(ForgetError::Line(format!("zygo: {} has a live agent launcher (pane {key}); it is not forgotten", ws.name)));
    }
    // Round 2 R5: the AU62 check runs also when no tmux can run, as in `check_invalid`.
    if let Some(sock) = &ws.tmux_socket {
        if parent_missing(sock) {
            return Ok(());
        }
        socket_is_private(sock, false).map_err(|t| format!("zygo: {t}"))?;
    }
    if no_tmux(tmux) {
        return Ok(());
    }
    let mux = server(tmux, ws.tmux_socket.as_deref(), tmux_env);
    // FIX-30 (AU54): the socket of the calls below, saved or from `$TMUX`, right before the first tmux call.
    if check_server(&mux).is_err() {
        return Err(unsafe_socket(store, ws.tmux_socket.is_some()));
    }
    match mux.session_exists(&ws.tmux_session) {
        SessionAnswer::Gone => Ok(()),
        SessionAnswer::Error { message } => Err(ForgetError::Line(format!("zygo: tmux: {message}"))),
        SessionAnswer::Exists => match mux.show_session_option(&ws.tmux_session, WS_OPTION) {
            Ok(Some(v)) if v == ws.id => {
                Err(ForgetError::Line(format!("zygo: {} is running; run zygo down -- {} first", ws.name, ws.name)))
            }
            Ok(_) => Ok(()),
            Err(e) => Err(ForgetError::Line(format!("zygo: tmux: {e}"))),
        },
    }
}

/// D17: the checks of a state that cannot be read, from its raw JSON: name, socket, member launchers, session.
fn check_invalid(store: &Store, id: &str, tmux: &Path, tmux_env: Option<&OsStr>) -> Result<(), ForgetError> {
    let refuse = |reason: &str| {
        format!(
            "zygo: workspace {id} not forgotten: {reason}; its state cannot be read, so repair it with zygo up --reset in its checkout"
        )
    };
    // Round 2 R6, FIX-17 Round 2 F1, F2: one capped read; a file over 1 MiB may be from a newer zygo and is refused.
    let v = raw_json(store.dir()).map_err(|e| format!("zygo: {e}"))?.unwrap_or(serde_json::Value::Null);
    let Some(name) = v.get("name").and_then(|n| n.as_str()).filter(|n| valid_name(n)) else {
        return Err(ForgetError::Line(refuse("it has no valid name")));
    };
    let Some(sock) = v.get("tmux_socket").and_then(|s| s.as_str()).map(PathBuf::from).filter(|s| s.is_absolute()) else {
        return Err(ForgetError::Line(refuse("it has no usable tmux socket")));
    };
    let gone = parent_missing(&sock);
    if !gone && socket_is_private(&sock, false).is_err() {
        return Err(ForgetError::Line(refuse("its socket is not private")));
    }
    let members: BTreeMap<String, MemberRecord> = match v.get("members") {
        None => BTreeMap::new(),
        Some(m) => serde_json::from_value(m.clone()).map_err(|_| refuse("its members cannot be read"))?,
    };
    let mut probe = Workspace::new(id);
    probe.members = members;
    if let Some(key) = store.live_launcher(&probe) {
        return Err(ForgetError::Line(refuse(&format!("a member launcher is live (pane {})", esc(&key)))));
    }
    if gone || no_tmux(tmux) {
        return Ok(());
    }
    // FIX-30 (AU54): the raw socket is not a `Workspace`, so `check_path`, which `check_saved_socket` wraps; right before
    // the first tmux call.
    if check_path(&sock).is_err() {
        return Err(unsafe_socket(store, true));
    }
    match server(tmux, Some(&sock), tmux_env).session_exists(&format!("zygo-{name}")) {
        SessionAnswer::Gone => Ok(()),
        SessionAnswer::Exists => Err(ForgetError::Line(refuse("its session runs"))),
        SessionAnswer::Error { message } => Err(ForgetError::Line(refuse(&format!("tmux: {message}")))),
    }
}

/// D16, Round 2 R2 and M2 review round 3 finding 4: `lock` is taken (with `op.lock` held) before anything is deleted;
/// then, under both locks, the state directory is renamed out of the workspace list first, then the workspace cache is
/// removed, then the renamed folder. A writer of the cache (`ws run`) that reads the state after it made a folder
/// finds the state gone whenever the cache removal could have run before its folder, so it removes that folder
/// itself (W§12). A cache that cannot be removed is reported after the rest; the next `forget` sweeps it (D26).
fn remove(paths: &Paths, store: &Store, id: &str, label: &str, op: OpLock) -> Result<String, String> {
    let cannot = |e: &dyn fmt::Display| format!("zygo: cannot forget {label}: {e}");
    let lock = match lock_file(&store.dir().join("lock"), snapshot::OP_WAIT) {
        Ok(Some(f)) => f,
        Ok(None) => return Err(format!("zygo: workspace {label} is busy; try again")),
        Err(e) => return Err(cannot(&e)),
    };
    fire(store.dir(), "forget_before_state").map_err(|e| cannot(&e))?;
    let tomb = tombstone(paths, id);
    fs::rename(store.dir(), &tomb).map_err(|e| cannot(&e))?;
    let cache = paths.workspace_cache_dir().join(id);
    let cache_left = fire(store.dir(), "forget_before_cache")
        .map_err(|e| e.to_string())
        .and_then(|()| remove_tree(&cache).map_err(|e| e.to_string()))
        .err();
    drop(lock);
    drop(op);
    let gone = |e: &dyn fmt::Display| format!("zygo: {label} forgotten, but {} was not removed: {e}", tomb.display());
    fire(store.dir(), "forget_before_tombstone_remove").map_err(|e| gone(&e))?;
    remove_tree(&tomb).map_err(|e| gone(&e))?;
    if let Some(e) = cache_left {
        return Err(format!("zygo: {label} forgotten, but {} was not removed: {e}", cache.display()));
    }
    Ok(format!("zygo: forgot {label}"))
}

/// `zygo ws forget-pane <key>` (W§12, D18-D20): the workspace named `name`, else the one of the checkout of `cwd`.
/// Exit 0 with `zygo: forgot pane <key> of <name>` on `out`, or exit 1 with one line on `err`.
pub fn forget_pane(
    paths: &Paths, tmux: &Path, name: Option<&str>, cwd: &Path, key: &str, out: &mut dyn Write, err: &mut dyn Write,
) -> i32 {
    match forget_pane_run(paths, tmux, name, cwd, key) {
        Ok(line) => {
            say(out, &line);
            0
        }
        Err(line) => {
            say(err, &line);
            1
        }
    }
}

/// zygo ws name <key> <name> (M§6, W§7.3)
#[allow(clippy::too_many_arguments)]
pub fn rename_member(
    paths: &Paths, tmux: &Path, workspace: Option<&str>, cwd: &Path, key: &str, new_name: &str, out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match rename_run(paths, tmux, workspace, cwd, key, new_name) {
        Ok(line) => {
            say(out, &line);
            0
        }
        Err(line) => {
            say(err, &line);
            1
        }
    }
}

fn rename_run(
    paths: &Paths, tmux: &Path, workspace: Option<&str>, cwd: &Path, key: &str, new_name: &str,
) -> Result<String, String> {
    if !valid_pane_key(key) {
        return Err(format!("zygo: invalid pane key {}", esc(key)));
    }
    if !valid_member_name(new_name) {
        return Err(format!(
            "zygo: bad name `{}`: use ^[a-z][a-z0-9-]{{0,31}}$, not all, cli, zygo or a pane key",
            esc(new_name)
        ));
    }
    let ws = pane_workspace(paths, workspace, cwd, NO_FOLDER_WS)?;
    let label = ws.name;
    // W§7.3: the record and its `panes[]` entry in one write under `lock`; `mail/<key>/` stays (M§6).
    let written = Store::open(paths, &ws.id).update_existing(snapshot::OP_WAIT, &mut |c| {
        if !c.members.contains_key(key) {
            bail!("zygo: {label} has no member record for pane {key}");
        }
        if let Some(t) = names::members(c).into_iter().find(|t| t.key != key && t.name.as_deref() == Some(new_name)) {
            bail!("zygo: name {new_name} belongs to pane {}; nothing changed", t.key);
        }
        if let Some(m) = c.members.get_mut(key) {
            m.name = Some(new_name.to_string());
        }
        if let Some(p) = c.panes.iter_mut().find(|p| p.key == key) {
            p.name = Some(new_name.to_string());
        }
        Ok(())
    });
    match written {
        Ok(Some(w)) => {
            let mux = Tmux::new(tmux.to_path_buf(), w.tmux_socket.clone());
            let _ = snapshot::after_write(paths, &mux, &ws.id, snapshot::OP_WAIT);
            Ok(format!("zygo: pane {key} of {label} is now named {new_name}"))
        }
        Ok(None) => Err(format!("zygo: no workspace named {}", esc(&label))),
        Err(StateError::Refused(e)) => Err(e.to_string()),
        Err(StateError::Busy) => Err(format!("zygo: workspace {label} is busy; try again")),
        Err(e) => Err(format!("zygo: {e}")),
    }
}

/// The workspace of `ws forget-pane` and `ws name` (W§12): the one named `name`, else the one of the checkout of `cwd`;
/// `no_folder` is the line when `cwd` has none.
fn pane_workspace(paths: &Paths, name: Option<&str>, cwd: &Path, no_folder: &str) -> Result<Workspace, String> {
    let not_found = || name.map_or_else(|| no_folder.to_string(), |n| format!("zygo: no workspace named {}", esc(n)));
    let link = |id: &str| format!("zygo: the state folder of workspace {id} is a link; it is not changed");
    Ok(match name {
        Some(n) => match find(paths, n, true).map_err(|e| e.message)? {
            Found::Readable(ws) => *ws,
            Found::Invalid(id) => match read_capped(&Store::open(paths, &id)) {
                Ok(Some(ws)) => ws,
                Ok(None) => return Err(not_found()),
                Err(e) => return Err(format!("zygo: {e}")),
            },
            Found::Link(id) => return Err(link(&id)),
        },
        None => {
            let (co, _, _) = find_checkout(cwd, GIT_BOUND).map_err(|_| no_folder.to_string())?;
            let id = workspace_id(&co.common_dir);
            match state_kind(paths, &id) {
                Kind::Real => {}
                Kind::Link => return Err(link(&id)),
                Kind::Missing | Kind::Other => return Err(no_folder.into()),
            }
            match read_capped(&Store::open(paths, &id)) {
                Ok(Some(ws)) => ws,
                Ok(None) => return Err(no_folder.into()),
                Err(StateError::Newer { .. }) => return Err(NEWER.into()),
                Err(e) => return Err(format!("zygo: {e}")),
            }
        }
    })
}

fn forget_pane_run(paths: &Paths, tmux: &Path, name: Option<&str>, cwd: &Path, key: &str) -> Result<String, String> {
    if !valid_pane_key(key) {
        return Err(format!("zygo: invalid pane key {}", esc(key)));
    }
    let not_found = || name.map_or_else(|| NO_FOLDER.to_string(), |n| format!("zygo: no workspace named {}", esc(n)));
    let link = |id: &str| format!("zygo: the state folder of workspace {id} is a link; it is not changed");
    let ws = pane_workspace(paths, name, cwd, NO_FOLDER)?;
    let (id, label) = (ws.id.clone(), ws.name.clone());
    let cannot = |e: &dyn fmt::Display| format!("zygo: cannot forget pane {key} of {label}: {e}");
    let store = Store::open(paths, &id);
    fire(store.dir(), "forget_pane_before_op_lock").map_err(|e| cannot(&e))?;
    // Round 2 R1: `op_lock` creates and chmods its folder, so the folder is checked again right before it.
    match state_kind(paths, &id) {
        Kind::Real => {}
        Kind::Link => return Err(link(&id)),
        Kind::Missing | Kind::Other => return Err(not_found()),
    }
    let _op = match store.op_lock() {
        Ok(l) => l,
        Err(StateError::OpHeld) => return Err(format!("zygo: up or down is in progress for {label}")),
        Err(e) => return Err(format!("zygo: {e}")),
    };
    let cur = match read_capped(&store) {
        Ok(Some(ws)) => ws,
        Ok(None) => return Err(not_found()),
        Err(StateError::Newer { .. }) => return Err(NEWER.into()),
        Err(e) => return Err(format!("zygo: {e}")),
    };
    // Round 2 R4: a link in the middle of a cache path refuses before anything is written. The pane caches are
    // `results/<key>` (AU16) and `program/<key>` (AU61).
    let mut has_cache = false;
    for (sub, _) in PANE_CACHES {
        has_cache |= pane_cache(paths, &id, sub, key).map_err(|r| cannot(&r))?.is_some();
    }
    if cur.members.contains_key(key) {
        // D18-D20: the live check under `lock`, on the record as read there; the record and `panes[].kind` in one write.
        let mut live = false;
        let written = store.update(snapshot::OP_WAIT, &mut |c| {
            live = false;
            if !c.members.contains_key(key) {
                return Ok(());
            }
            if member_live(&store, c, key) {
                live = true;
                bail!("pane {key} has a live agent launcher");
            }
            c.members.remove(key);
            if let Some(p) = c.panes.iter_mut().find(|p| p.key == key)
                && (p.kind != PaneKind::Shell || p.name.is_some())
            {
                // M§6: the name goes with the record.
                p.kind = PaneKind::Shell;
                p.name = None;
                c.snapshot_seq += 1;
            }
            Ok(())
        });
        match written {
            Ok(w) => {
                let mux = Tmux::new(tmux.to_path_buf(), w.tmux_socket.clone());
                let _ = snapshot::after_write(paths, &mux, &id, snapshot::OP_WAIT);
            }
            Err(StateError::Refused(_)) if live => {
                return Err(format!("zygo: pane {key} of {label} has a live agent launcher; it is not forgotten"));
            }
            Err(StateError::Busy) => return Err(format!("zygo: workspace {label} is busy; try again")),
            Err(e) => return Err(format!("zygo: {e}")),
        }
    } else if !has_cache {
        return Err(format!("zygo: {label} has no member record for pane {key}"));
    }
    fire(store.dir(), "forget_pane_before_cache").map_err(|e| cannot(&e))?;
    // M2 review round 3 finding 9: every cache is tried, also after an error; the first error is reported (exit 1),
    // and a second run removes what is left (the record is gone, so only the caches remain).
    let mut first = None;
    for (sub, point) in PANE_CACHES {
        let removed = fire(store.dir(), point).map_err(|e| e.to_string()).and_then(|()| match pane_cache(paths, &id, sub, key)? {
            Some(entry) => remove_tree(&entry).map_err(|e| format!("{}: {e}", entry.display())),
            None => Ok(()),
        });
        if let Err(e) = removed {
            first.get_or_insert(cannot(&e));
        }
    }
    match first {
        Some(e) => Err(e),
        None => Ok(format!("zygo: forgot pane {key} of {label}")),
    }
}

/// The pane caches under `<workspace_cache_dir>/<id>/`, `results/<key>` (AU16) and `program/<key>` (AU61), each with
/// the test hook point that runs right before its removal.
const PANE_CACHES: [(&str, &str); 2] = [("results", "forget_pane_remove_results"), ("program", "forget_pane_remove_program")];

/// Round 2 R4: the pane cache `<workspace_cache_dir>/<id>/<sub>/<key>` of one pane, `sub` = `results` (AU16) or
/// `program` (AU61), each part below `workspace_cache_dir()` checked with `symlink_metadata`. `Ok(Some(path))` when
/// `<key>` exists (a link there is removed as a link); `Ok(None)` when a part is missing or is not a folder;
/// `Err(reason)` for a link at `<id>` or `<sub>` (it is not followed) or a part that cannot be read.
pub fn pane_cache(paths: &Paths, id: &str, sub: &str, key: &str) -> Result<Option<PathBuf>, String> {
    let mut at = paths.workspace_cache_dir();
    for part in [id, sub] {
        at = at.join(part);
        match fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_symlink() => return Err(format!("{} is a link; it is not followed", at.display())),
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{}: {e}", at.display())),
        }
    }
    let entry = at.join(key);
    match fs::symlink_metadata(&entry) {
        Ok(_) => Ok(Some(entry)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", entry.display())),
    }
}

/// Makes or opens `<workspace_cache_dir>/<id>/<sub>/<key>` for the member wrapper (M2 review rounds 2 and 3, findings 3
/// and 10): first the check of [`pane_cache`]; then, from an fd of `workspace_cache_dir()` (made when missing), each
/// part is made with mode 0700 when missing (`mkdirat`), opened with `O_NOFOLLOW | O_DIRECTORY` relative to the fd of
/// its parent, and set to mode 0700 through its own fd (`fchmod`), so a part that already exists with another mode
/// gets 0700 too and no link is followed at any part. A link at `<id>`, `<sub>` or `<key>` refuses with
/// `<path> is a link; it is not followed`. `Ok(path)` is the folder; nothing is deleted.
#[cfg(unix)]
pub fn pane_cache_create(paths: &Paths, id: &str, sub: &str, key: &str) -> Result<PathBuf, String> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::DirBuilderExt;
    pane_cache(paths, id, sub, key)?;
    let base = paths.workspace_cache_dir();
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    let name = |s: &OsStr, at: &Path| CString::new(s.as_bytes()).map_err(|e| format!("{}: {e}", at.display()));
    let last = |at: &Path| {
        let e = std::io::Error::last_os_error();
        if fs::symlink_metadata(at).is_ok_and(|m| m.file_type().is_symlink()) {
            format!("{} is a link; it is not followed", at.display())
        } else {
            format!("{}: {e}", at.display())
        }
    };
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
    let c = name(base.as_os_str(), &base)?;
    // SAFETY: `c` is a NUL-terminated path; the new descriptor is owned by `dir` at once.
    let fd = unsafe { libc::open(c.as_ptr(), flags) };
    if fd < 0 {
        return Err(last(&base));
    }
    // SAFETY: `fd` is a new descriptor that nothing else owns.
    let mut dir = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut at = base;
    for part in [id, sub, key] {
        at = at.join(part);
        let c = name(OsStr::new(part), &at)?;
        // SAFETY: `dir` is an open folder descriptor and `c` a NUL-terminated name in it.
        if unsafe { libc::mkdirat(dir.as_raw_fd(), c.as_ptr(), 0o700) } != 0
            && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
        {
            return Err(last(&at));
        }
        // SAFETY: as above; `O_NOFOLLOW` refuses a link at `part`.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), c.as_ptr(), flags | libc::O_NOFOLLOW) };
        if fd < 0 {
            return Err(last(&at));
        }
        // SAFETY: `fd` is a new descriptor that nothing else owns.
        let next = unsafe { OwnedFd::from_raw_fd(fd) };
        // SAFETY: `next` is an open descriptor of the folder `at`.
        if unsafe { libc::fchmod(next.as_raw_fd(), 0o700) } != 0 {
            return Err(format!("{}: {}", at.display(), std::io::Error::last_os_error()));
        }
        dir = next;
    }
    Ok(at)
}

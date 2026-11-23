//! `ls`, `workspace`, `ws forget`, `ws forget-pane`. Owner: node W8 (W§12).
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
use zygo_core::paths::Paths;

use crate::identity::{valid_name, valid_pane_key, valid_ws_id, workspace_id};
use crate::mux::tmux::Tmux;
use crate::mux::{session_target, Multiplexer, MuxError, SessionAnswer, BOUND};
use crate::snapshot;
use crate::state::{list_all, lock_file, MemberRecord, MemberState, OpLock, PaneKind, Phase, StateError, Store, Workspace};
use crate::up::{find_checkout, fire, member_live, server, socket_is_private, Exit, GIT_BOUND};

/// The git bound of `status`, so `/zygo:workspace` fits the 4.5 s hook bound (D12, D23).
const STATUS_GIT_BOUND: Duration = Duration::from_secs(2);
/// `ls` makes no tmux call after this time from its start (D11).
const LIST_DEADLINE: Duration = Duration::from_secs(4);
const WS_OPTION: &str = "@zygo-ws";
const NEWER: &str = "zygo: workspace state is from a newer zygo; run zygo update";
const NO_STATUS: &str = "no workspace for this session or folder";
const NO_FOLDER: &str = "zygo: no workspace for this folder; run in the workspace's checkout or pass --name";

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

/// The session check of one row (D8, D11): no call without tmux, without a known server, on a failed socket, on a
/// socket whose folder is missing or that fails AU62, or past the deadline.
fn ask(
    ws: &Workspace, no_tmux: bool, failed: &mut BTreeMap<PathBuf, &'static str>, mux_of: &dyn Fn(&Path) -> Box<dyn Multiplexer>,
    may_call: &dyn Fn() -> bool,
) -> Seen {
    if no_tmux {
        return Seen::Unknown("no tmux");
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
    let ws = match Store::open(paths, &id).read() {
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

/// D26: removes the folders that an earlier `forget` left (`.forget-*`) and the state directories with no
/// `workspace.json` whose `op.lock` and `lock` are free. Every error is ignored: the next `forget` tries again.
fn sweep(paths: &Paths) {
    let Ok(entries) = fs::read_dir(paths.workspaces_dir()) else {
        return;
    };
    let entries: Vec<(String, PathBuf, bool)> = entries
        .flatten()
        .filter_map(|e| {
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            e.file_name().into_string().ok().map(|n| (n, e.path(), is_dir))
        })
        .collect();
    for (name, path, is_dir) in entries {
        if name.starts_with(".forget-") {
            let _ = remove_tree(&path);
            continue;
        }
        let no_state = || matches!(fs::symlink_metadata(path.join("workspace.json")), Err(e) if e.kind() == std::io::ErrorKind::NotFound);
        if !is_dir || !valid_ws_id(&name) || !no_state() {
            continue;
        }
        let Ok(_op) = Store::open(paths, &name).op_lock() else {
            continue;
        };
        let Ok(Some(_lock)) = lock_file(&path.join("lock"), Duration::ZERO) else {
            continue;
        };
        if !no_state() {
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
    Readable(Workspace),
    /// A state that does not parse or fails AU84, by its id.
    Invalid(String),
}

/// D14: the id first, then the readable states by name, then a newer state by its JSON `name`. `for_pane` gives the
/// `forget-pane` form of the line for a shared name.
fn find(paths: &Paths, arg: &str, for_pane: bool) -> Result<Found, Exit> {
    let fail = |message: String| Exit { code: 1, message };
    if valid_ws_id(arg) && paths.workspaces_dir().join(arg).is_dir() {
        match Store::open(paths, arg).read() {
            Ok(Some(ws)) => return Ok(Found::Readable(ws)),
            Ok(None) => {}
            Err(StateError::Newer { .. }) => return Err(fail(NEWER.into())),
            Err(_) => return Ok(Found::Invalid(arg.to_string())),
        }
    }
    let all = list_all(paths);
    let mut named: Vec<&Workspace> = all.iter().filter_map(|(_, r)| r.as_ref().ok().filter(|w| w.name == arg)).collect();
    match named.len() {
        0 => {}
        1 => return Ok(Found::Readable(named.remove(0).clone())),
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
    for (_, r) in &all {
        let Err(StateError::Newer { path }) = r else { continue };
        let name = fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string));
        if name.as_deref() == Some(arg) {
            return Err(fail(NEWER.into()));
        }
    }
    Err(fail(format!("zygo: no workspace named {}", esc(arg))))
}

/// `zygo ws forget <name>` (W§12, D14-D17, D26): exit 0 with `zygo: forgot <label>` on `out`, or exit 1 with one line on
/// `err`. `tmux_env` is `$TMUX`.
pub fn forget(paths: &Paths, tmux: &Path, tmux_env: Option<&OsStr>, name: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    sweep(paths);
    match forget_run(paths, tmux, tmux_env, name) {
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

fn forget_run(paths: &Paths, tmux: &Path, tmux_env: Option<&OsStr>, arg: &str) -> Result<String, String> {
    let found = find(paths, arg, false).map_err(|e| e.message)?;
    let (id, first_label) = match &found {
        Found::Readable(ws) => (ws.id.clone(), ws.name.clone()),
        Found::Invalid(id) => (id.clone(), id.clone()),
    };
    let store = Store::open(paths, &id);
    fire(store.dir(), "forget_before_op_lock").map_err(|e| format!("zygo: cannot forget {first_label}: {e}"))?;
    let op = match store.op_lock() {
        Ok(l) => l,
        Err(StateError::OpHeld) => return Err(format!("zygo: up or down is in progress for {first_label}")),
        Err(e) => return Err(format!("zygo: {e}")),
    };
    // D15: the state again under `op.lock`.
    let label = match store.read() {
        Ok(None) => return Err(format!("zygo: no workspace named {}", esc(arg))),
        Err(StateError::Newer { .. }) => return Err(NEWER.into()),
        Ok(Some(ws)) => {
            check_readable(&store, &ws, tmux, tmux_env)?;
            ws.name
        }
        Err(_) => {
            check_invalid(&store, &id, tmux, tmux_env)?;
            id.clone()
        }
    };
    remove(paths, &store, &id, &label, op)
}

/// Whether no tmux can run here: then no session exists, and the session check is skipped (D15, D17).
fn no_tmux(tmux: &Path) -> bool { matches!(Tmux::new(tmux.to_path_buf(), None).version(), Err(MuxError::Spawn { .. })) }

/// D15: a live launcher, a socket that fails AU62, the workspace's own session, or a tmux error refuses.
fn check_readable(store: &Store, ws: &Workspace, tmux: &Path, tmux_env: Option<&OsStr>) -> Result<(), String> {
    if let Some(key) = store.live_launcher(ws) {
        return Err(format!("zygo: {} has a live agent launcher (pane {key}); it is not forgotten", ws.name));
    }
    if no_tmux(tmux) {
        return Ok(());
    }
    if let Some(sock) = &ws.tmux_socket {
        if parent_missing(sock) {
            return Ok(());
        }
        socket_is_private(sock, false).map_err(|t| format!("zygo: {t}"))?;
    }
    let mux = server(tmux, ws.tmux_socket.as_deref(), tmux_env);
    match mux.session_exists(&ws.tmux_session) {
        SessionAnswer::Gone => Ok(()),
        SessionAnswer::Error { message } => Err(format!("zygo: tmux: {message}")),
        SessionAnswer::Exists => match mux.show_session_option(&ws.tmux_session, WS_OPTION) {
            Ok(Some(v)) if v == ws.id => Err(format!("zygo: {} is running; run zygo down {} first", ws.name, ws.name)),
            Ok(_) => Ok(()),
            Err(e) => Err(format!("zygo: tmux: {e}")),
        },
    }
}

/// D17: the checks of a state that cannot be read, from its raw JSON: name, socket, member launchers, session.
fn check_invalid(store: &Store, id: &str, tmux: &Path, tmux_env: Option<&OsStr>) -> Result<(), String> {
    let refuse = |reason: &str| {
        format!(
            "zygo: workspace {id} not forgotten: {reason}; its state cannot be read, so repair it with zygo up --reset in its checkout"
        )
    };
    let v: serde_json::Value = fs::read(store.dir().join("workspace.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::Value::Null);
    let Some(name) = v.get("name").and_then(|n| n.as_str()).filter(|n| valid_name(n)) else {
        return Err(refuse("it has no valid name"));
    };
    let Some(sock) = v.get("tmux_socket").and_then(|s| s.as_str()).map(PathBuf::from).filter(|s| s.is_absolute()) else {
        return Err(refuse("it has no usable tmux socket"));
    };
    let gone = parent_missing(&sock);
    if !gone && socket_is_private(&sock, false).is_err() {
        return Err(refuse("its socket is not private"));
    }
    let members: BTreeMap<String, MemberRecord> = match v.get("members") {
        None => BTreeMap::new(),
        Some(m) => serde_json::from_value(m.clone()).map_err(|_| refuse("its members cannot be read"))?,
    };
    let mut probe = Workspace::new(id);
    probe.members = members;
    if let Some(key) = store.live_launcher(&probe) {
        return Err(refuse(&format!("a member launcher is live (pane {})", esc(&key))));
    }
    if gone || no_tmux(tmux) {
        return Ok(());
    }
    match server(tmux, Some(&sock), tmux_env).session_exists(&format!("zygo-{name}")) {
        SessionAnswer::Gone => Ok(()),
        SessionAnswer::Exists => Err(refuse("its session runs")),
        SessionAnswer::Error { message } => Err(refuse(&format!("tmux: {message}"))),
    }
}

/// D16: the results cache, then the state directory, renamed out of the workspace list under `op.lock` and `lock`,
/// then removed.
fn remove(paths: &Paths, store: &Store, id: &str, label: &str, op: OpLock) -> Result<String, String> {
    let cannot = |e: &dyn fmt::Display| format!("zygo: cannot forget {label}: {e}");
    let cache = paths.workspace_cache_dir().join(id);
    remove_tree(&cache).map_err(|e| cannot(&format!("{}: {e}", cache.display())))?;
    fire(store.dir(), "forget_before_state").map_err(|e| cannot(&e))?;
    let lock = match lock_file(&store.dir().join("lock"), snapshot::OP_WAIT) {
        Ok(Some(f)) => f,
        Ok(None) => return Err(format!("zygo: workspace {label} is busy; try again")),
        Err(e) => return Err(cannot(&e)),
    };
    let tomb = tombstone(paths, id);
    fs::rename(store.dir(), &tomb).map_err(|e| cannot(&e))?;
    drop(lock);
    drop(op);
    let gone = |e: &dyn fmt::Display| format!("zygo: {label} forgotten, but {} was not removed: {e}", tomb.display());
    fire(store.dir(), "forget_before_tombstone_remove").map_err(|e| gone(&e))?;
    remove_tree(&tomb).map_err(|e| gone(&e))?;
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

fn forget_pane_run(paths: &Paths, tmux: &Path, name: Option<&str>, cwd: &Path, key: &str) -> Result<String, String> {
    if !valid_pane_key(key) {
        return Err(format!("zygo: invalid pane key {}", esc(key)));
    }
    let not_found = || name.map_or_else(|| NO_FOLDER.to_string(), |n| format!("zygo: no workspace named {}", esc(n)));
    let ws = match name {
        Some(n) => match find(paths, n, true).map_err(|e| e.message)? {
            Found::Readable(ws) => ws,
            Found::Invalid(id) => match Store::open(paths, &id).read() {
                Ok(Some(ws)) => ws,
                Ok(None) => return Err(not_found()),
                Err(e) => return Err(format!("zygo: {e}")),
            },
        },
        None => {
            let (co, _, _) = find_checkout(cwd, GIT_BOUND).map_err(|_| NO_FOLDER.to_string())?;
            match Store::open(paths, &workspace_id(&co.common_dir)).read() {
                Ok(Some(ws)) => ws,
                Ok(None) => return Err(NO_FOLDER.into()),
                Err(StateError::Newer { .. }) => return Err(NEWER.into()),
                Err(e) => return Err(format!("zygo: {e}")),
            }
        }
    };
    let (id, label) = (ws.id.clone(), ws.name.clone());
    let cannot = |e: &dyn fmt::Display| format!("zygo: cannot forget pane {key} of {label}: {e}");
    let store = Store::open(paths, &id);
    fire(store.dir(), "forget_pane_before_op_lock").map_err(|e| cannot(&e))?;
    let _op = match store.op_lock() {
        Ok(l) => l,
        Err(StateError::OpHeld) => return Err(format!("zygo: up or down is in progress for {label}")),
        Err(e) => return Err(format!("zygo: {e}")),
    };
    let cur = match store.read() {
        Ok(Some(ws)) => ws,
        Ok(None) => return Err(not_found()),
        Err(StateError::Newer { .. }) => return Err(NEWER.into()),
        Err(e) => return Err(format!("zygo: {e}")),
    };
    let results = paths.workspace_cache_dir().join(&id).join("results").join(key);
    let has_cache = fs::symlink_metadata(&results).is_ok();
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
                && p.kind != PaneKind::Shell
            {
                p.kind = PaneKind::Shell;
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
    remove_tree(&results).map_err(|e| cannot(&format!("{}: {e}", results.display())))?;
    Ok(format!("zygo: forgot pane {key} of {label}"))
}

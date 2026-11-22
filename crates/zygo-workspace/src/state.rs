//! `workspace.json` types, atomic write, locks, live check. Owner: node W2 (W§7.1-W§7.3).
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use zygo_core::paths::Paths;
use zygo_core::session_dir::{self, SessionDir, write_atomic};

use crate::identity::{valid_name, valid_pane_key, valid_tmux_session, valid_ws_id};

/// The `version` this zygo reads and writes (AU29).
pub const VERSION: u32 = 1;
/// How long `op_lock` tries again before it reports the lock as held (AU83).
pub const OP_LOCK_RETRY: Duration = Duration::from_millis(200);
/// The largest size of `errors.log` (W§7.1).
pub const ERRORS_LOG_MAX: u64 = 64 * 1024;

/// The fields of a JSON object that this zygo does not know; a write puts them back (AU78).
type Extra = serde_json::Map<String, serde_json::Value>;

/// The content of `workspace.json` (W§7.2). It holds no secret and no content (W-C7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub common_dir: PathBuf,
    pub main_root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_socket: Option<PathBuf>,
    pub tmux_session: String,
    pub loadout: String,
    pub phase: Phase,
    pub phase_since: u64,
    #[serde(default)]
    pub down_all: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_at: Option<u64>,
    #[serde(default)]
    pub windows: Vec<Window>,
    #[serde(default)]
    pub panes: Vec<Pane>,
    #[serde(default)]
    pub members: BTreeMap<String, MemberRecord>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Window {
    pub index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub layout: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub zoomed: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Pane {
    pub key: String,
    pub window: u32,
    pub index: u32,
    #[serde(default)]
    pub active: bool,
    pub cwd: PathBuf,
    pub kind: PaneKind,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One member of `members`, by pane key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemberRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zygo_session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Pending>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<String>,
    pub state: MemberState,
    pub launch: Launch,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<u64>,
    /// The whole content of the member's `overrides.json`, so a key this zygo does not know is kept (AU78).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact: Option<CompactOverride>,
    #[serde(default)]
    pub interrupted: Vec<Subagent>,
    #[serde(default)]
    pub closed_units: Vec<String>,
    #[serde(default)]
    pub dropped_flags: Vec<String>,
    #[serde(default)]
    pub clean_stop: bool,
    #[serde(default)]
    pub note_pending: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A start that is not committed yet.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Pending {
    pub zygo_session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<String>,
    pub started_at: u64,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Launch {
    pub kind: zygo_core::Agent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default)]
    pub passthrough: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact: Option<String>,
    #[serde(default)]
    pub yolo: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Subagent {
    pub agent_id: String,
    pub agent_type: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Building,
    Up,
    Draining,
    Stopping,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemberState {
    Running,
    Resumable,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PaneKind {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "opencode")]
    OpenCode,
    #[default]
    #[serde(rename = "shell")]
    Shell,
}

/// The member's compaction override as the text `set <w>` or `cleared` (W§7.2); `<w>` is the `--compact` text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum CompactOverride {
    Set(String),
    Cleared,
}

impl From<CompactOverride> for String {
    fn from(c: CompactOverride) -> String {
        match c {
            CompactOverride::Set(w) => format!("set {w}"),
            CompactOverride::Cleared => "cleared".to_string(),
        }
    }
}

impl TryFrom<String> for CompactOverride {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        match s.strip_prefix("set ") {
            Some(w) if !w.is_empty() => Ok(CompactOverride::Set(w.to_string())),
            _ if s == "cleared" => Ok(CompactOverride::Cleared),
            _ => Err("invalid compact override".to_string()),
        }
    }
}

impl Workspace {
    /// A state with only its version and id: phase `down`, every other field empty, none or false.
    pub fn new(id: &str) -> Workspace {
        Workspace {
            version: VERSION,
            id: id.to_string(),
            name: String::new(),
            common_dir: PathBuf::new(),
            main_root: PathBuf::new(),
            tmux_socket: None,
            tmux_session: String::new(),
            loadout: String::new(),
            phase: Phase::Down,
            phase_since: 0,
            down_all: false,
            snapshot_at: None,
            windows: Vec::new(),
            panes: Vec::new(),
            members: BTreeMap::new(),
            extra: Extra::new(),
        }
    }
}

impl Launch {
    pub fn new(kind: zygo_core::Agent) -> Launch {
        Launch {
            kind,
            profile: None,
            passthrough: false,
            server: None,
            compact: None,
            yolo: false,
            config_dir: None,
            args: Vec::new(),
            extra: Extra::new(),
        }
    }
}

impl MemberRecord {
    /// A member in state `ended` with `Launch::new(kind)`; every other field is empty, none or false.
    pub fn new(kind: zygo_core::Agent) -> MemberRecord {
        MemberRecord {
            zygo_session: None,
            pending: None,
            agent_session: None,
            state: MemberState::Ended,
            launch: Launch::new(kind),
            config_dir: None,
            program: None,
            last_run_at: None,
            overrides: None,
            compact: None,
            interrupted: Vec::new(),
            closed_units: Vec::new(),
            dropped_flags: Vec::new(),
            clean_stop: false,
            note_pending: false,
            extra: Extra::new(),
        }
    }
}

#[derive(Debug)]
pub enum StateError {
    /// `workspace.json` has a `version` this zygo does not know (AU29).
    Newer { path: PathBuf },
    /// `workspace.json` does not parse, or a field fails its check (AU84).
    Invalid { path: PathBuf, reason: String },
    /// Another writer held `lock` for the whole bound.
    Busy,
    /// Another process held `op.lock` for the whole retry time (AU83).
    OpHeld,
    Io(std::io::Error),
    /// The closure given to `update` returned an error; nothing was written.
    Refused(anyhow::Error),
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateError::Newer { .. } => f.write_str("workspace state is from a newer zygo; run zygo update"),
            StateError::Invalid { path, reason } => write!(f, "{}: {reason}", path.display()),
            StateError::Busy => f.write_str("workspace state is locked"),
            StateError::OpHeld => f.write_str("up or down is in progress"),
            StateError::Io(e) => write!(f, "{e}"),
            StateError::Refused(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for StateError {}

impl From<std::io::Error> for StateError {
    fn from(e: std::io::Error) -> StateError {
        StateError::Io(e)
    }
}

/// The checks of AU84 on a state that belongs to the directory `id`; the text is the first check that fails.
fn validate(ws: &Workspace, id: &str) -> Result<(), String> {
    let reason = if ws.version != VERSION {
        "invalid version"
    } else if !valid_ws_id(&ws.id) || ws.id != id {
        "invalid id"
    } else if !valid_name(&ws.name) {
        "invalid name"
    } else if !valid_tmux_session(&ws.tmux_session, &ws.name) {
        "invalid tmux_session"
    } else if ws.panes.iter().any(|p| !valid_pane_key(&p.key)) || ws.members.keys().any(|k| !valid_pane_key(k)) {
        "invalid pane key"
    } else {
        return Ok(());
    };
    Err(reason.to_string())
}

const STATE_FILE: &str = "workspace.json";
const LOCK_FILE: &str = "lock";
const OP_LOCK_FILE: &str = "op.lock";
const ERRORS_FILE: &str = "errors.log";
/// The time between two tries for a lock that another holder has.
const LOCK_POLL: Duration = Duration::from_millis(5);
/// A temporary file older than this belongs to no live writer (W§7.3).
const TMP_MAX_AGE: Duration = Duration::from_secs(60);
/// The largest text of one `errors.log` line, in bytes.
const ERROR_LINE_MAX: usize = 1024;

/// The state of one workspace: the files in `<workspaces_dir>/<id>`.
#[derive(Debug, Clone)]
pub struct Store {
    id: String,
    dir: PathBuf,
    sessions_dir: PathBuf,
}

impl Store {
    /// Names the state directory of `ws_id`; it creates nothing and reads nothing. Each operation checks the id
    /// before it reads or writes a path made from it (AU84).
    pub fn open(paths: &Paths, ws_id: &str) -> Store {
        Store { id: ws_id.to_string(), dir: paths.workspaces_dir().join(ws_id), sessions_dir: paths.sessions_dir.clone() }
    }

    /// The state directory: `workspace.json`, `lock`, `op.lock`, `errors.log` and the files of other modules.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn state_file(&self) -> PathBuf {
        self.dir.join(STATE_FILE)
    }

    fn invalid_id(&self) -> StateError {
        StateError::Invalid { path: self.state_file(), reason: "invalid id".to_string() }
    }

    /// Reads `workspace.json` with no lock: a write replaces the file in one rename, so a reader gets a whole
    /// file (W§7.3). `Ok(None)` when no state exists.
    pub fn read(&self) -> Result<Option<Workspace>, StateError> {
        if !valid_ws_id(&self.id) {
            return Err(self.invalid_id());
        }
        let path = self.state_file();
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(StateError::Io(e)),
        };
        let value: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(e) => return Err(StateError::Invalid { path, reason: e.to_string() }),
        };
        // Before the typed parse: a newer form is never reported as invalid (AU29).
        if value.get("version").and_then(serde_json::Value::as_u64).is_some_and(|v| v > u64::from(VERSION)) {
            return Err(StateError::Newer { path });
        }
        let ws: Workspace = match serde_json::from_value(value) {
            Ok(ws) => ws,
            Err(e) => return Err(StateError::Invalid { path, reason: e.to_string() }),
        };
        match validate(&ws, &self.id) {
            Ok(()) => Ok(Some(ws)),
            Err(reason) => Err(StateError::Invalid { path, reason }),
        }
    }

    /// The phase as the file has it now, with no lock; `None` when the state is missing or cannot be read.
    pub fn phase_unlocked(&self) -> Option<Phase> {
        self.read().ok().flatten().map(|ws| ws.phase)
    }

    /// Makes the state directory (mode 0700 on unix) after the id check: no path is made from an id that is not
    /// valid.
    fn ensure_dir(&self) -> Result<(), StateError> {
        if !valid_ws_id(&self.id) {
            return Err(self.invalid_id());
        }
        SessionDir::new(self.dir.clone()).create().map_err(|e| StateError::Io(std::io::Error::other(e)))
    }

    /// Read-modify-write of `workspace.json` under `lock` (W§7.3): waits for the lock at most `bound`, reads the
    /// state (`Workspace::new` when none exists), applies `f`, checks the result (AU84) and replaces the file in one
    /// rename (W-C4). An error of the read, of `f` or of the check writes nothing. It runs no other program and
    /// waits for nothing but the lock.
    pub fn update(
        &self, bound: Duration, f: &mut dyn FnMut(&mut Workspace) -> anyhow::Result<()>,
    ) -> Result<Workspace, StateError> {
        self.ensure_dir()?;
        let Some(_lock) = lock_file(&self.dir.join(LOCK_FILE), bound)? else {
            return Err(StateError::Busy);
        };
        let mut ws = match self.read()? {
            Some(ws) => ws,
            None => Workspace::new(&self.id),
        };
        f(&mut ws).map_err(StateError::Refused)?;
        let path = self.state_file();
        if let Err(reason) = validate(&ws, &self.id) {
            return Err(StateError::Invalid { path, reason });
        }
        sweep_tmp(&self.dir);
        let bytes = serde_json::to_vec_pretty(&ws).map_err(|e| StateError::Io(std::io::Error::other(e)))?;
        write_atomic(&path, &bytes).map_err(|e| StateError::Io(std::io::Error::other(e)))?;
        Ok(ws)
    }

    /// Takes `op.lock`, the lock of `up` and `down`; tries again for `OP_LOCK_RETRY` before `OpHeld` (AU83).
    pub fn op_lock(&self) -> Result<OpLock, StateError> {
        self.ensure_dir()?;
        match lock_file(&self.dir.join(OP_LOCK_FILE), OP_LOCK_RETRY)? {
            Some(f) => Ok(OpLock { _file: f }),
            None => Err(StateError::OpHeld),
        }
    }

    /// `false` only while another holder has `op.lock`. It creates no file and no directory. When the lock is free,
    /// the check holds it until the return; `op_lock` tries again for that short hold (AU83).
    pub fn op_lock_free(&self) -> bool {
        if !valid_ws_id(&self.id) {
            return true;
        }
        match File::open(self.dir.join(OP_LOCK_FILE)) {
            Ok(f) => !matches!(fs4::FileExt::try_lock(&f), Err(fs4::TryLockError::WouldBlock)),
            Err(_) => true,
        }
    }

    /// The pane key of the first member, in key order, whose launcher is live: its `zygo_session` or its
    /// `pending.zygo_session` (a start that is not committed yet) names a live session directory (AU87 (2)). An id
    /// that is empty or has a byte outside `[A-Za-z0-9]` is not looked up, so no id names a path outside the
    /// sessions directory. It takes no lock of this store and writes nothing.
    pub fn live_launcher(&self, ws: &Workspace) -> Option<String> {
        if !valid_ws_id(&self.id) {
            return None;
        }
        let live = |id: &str| {
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric()) && session_dir::live(&self.sessions_dir.join(id))
        };
        ws.members.iter().find_map(|(key, m)| {
            let committed = m.zygo_session.as_deref();
            let pending = m.pending.as_ref().map(|p| p.zygo_session.as_str());
            committed.into_iter().chain(pending).any(live).then(|| key.clone())
        })
    }

    /// Adds the line `<unix seconds> <text>` to `errors.log`. The caller has no place to report a failure of the
    /// log, so every error is ignored.
    pub fn log_error(&self, line: &str) {
        if self.ensure_dir().is_err() {
            return;
        }
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let text = line.replace(['\n', '\r'], " ");
        let mut end = text.len().min(ERROR_LINE_MAX);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let entry = format!("{secs} {}\n", &text[..end]);
        let path = self.dir.join(ERRORS_FILE);
        let len = fs::metadata(&path).map_or(0, |m| m.len());
        if len + entry.len() as u64 <= ERRORS_LOG_MAX {
            if let Ok(mut f) = private().append(true).create(true).open(&path) {
                let _ = f.write_all(entry.as_bytes());
            }
            return;
        }
        // The file is full: keep its newest whole lines within half of the limit, so a rewrite occurs one time
        // per 32 KiB of lines and not at each line.
        let Ok(mut all) = fs::read(&path) else {
            return;
        };
        all.extend_from_slice(entry.as_bytes());
        let keep = (ERRORS_LOG_MAX / 2) as usize;
        let from = (0..all.len()).find(|&o| all.len() - o <= keep && (o == 0 || all[o - 1] == b'\n')).unwrap_or(all.len());
        let _ = write_atomic(&path, &all[from..]);
    }
}

/// `op.lock` held by this process; the lock ends when the value is dropped.
#[derive(Debug)]
pub struct OpLock {
    _file: File,
}

/// Open options for a file that only its owner can read: mode 0600 on unix. Windows has no file mode: the file
/// inherits the access list of its directory.
fn private() -> fs::OpenOptions {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut o = fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}

/// Removes the temporary files of `write_atomic` (`.tmp*`) in `dir` that are older than 1 min: a writer that was
/// killed before its rename left them (W§7.3). A younger file can belong to a live writer. Every error is ignored;
/// an entry with no readable time stays.
fn sweep_tmp(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(".tmp") {
            continue;
        }
        let age = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| now.duration_since(t).ok());
        if age.is_some_and(|a| a > TMP_MAX_AGE) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Opens (and creates) the lock file `path` and takes its lock: tries each 5 ms until `bound` ends. `Ok(None)` when
/// another holder kept the lock for the whole bound. The lock ends when the file is dropped.
fn lock_file(path: &Path, bound: Duration) -> std::io::Result<Option<File>> {
    let f = private().read(true).write(true).create(true).truncate(false).open(path)?;
    // `None`: the bound is past the end of the clock, so no deadline exists.
    let deadline = Instant::now().checked_add(bound);
    loop {
        match fs4::FileExt::try_lock(&f) {
            Ok(()) => return Ok(Some(f)),
            Err(fs4::TryLockError::Error(e)) => return Err(e),
            Err(fs4::TryLockError::WouldBlock) => {
                if deadline.is_some_and(|d| Instant::now() >= d) {
                    return Ok(None);
                }
                std::thread::sleep(LOCK_POLL);
            }
        }
    }
}

/// Each workspace of the state directory with the result of its read, in id order: a state that cannot be read
/// does not hide the others. A directory whose name is not a workspace id, and one with no `workspace.json`, is
/// not a workspace.
pub fn list_all(paths: &Paths) -> Vec<(String, Result<Workspace, StateError>)> {
    let Ok(entries) = fs::read_dir(paths.workspaces_dir()) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| valid_ws_id(name))
        .collect();
    ids.sort();
    ids.into_iter().filter_map(|id| Store::open(paths, &id).read().transpose().map(|r| (id, r))).collect()
}

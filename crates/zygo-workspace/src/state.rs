//! `workspace.json` types, atomic write, locks, live check. Owner: node W2 (W§7.1-W§7.3).
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zygo_core::paths::Paths;

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
}

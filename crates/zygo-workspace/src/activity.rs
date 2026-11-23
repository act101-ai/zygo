//! `activity.json` under `activity.lock`. Owner: node W9 (W§9.1, AU56).
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use zygo_core::session_dir::{write_atomic, SessionDir};

/// The schema version this zygo writes in `activity.json` (W§9.1).
pub const VERSION: u32 = 1;

/// How long a hook waits for `activity.lock` before it gives up and loses its change (AU56).
pub const LOCK_BOUND: Duration = Duration::from_secs(1);

/// Turn and subagent state of one workspace member, the contents of `activity.json` (W§9.1). Unknown fields are
/// dropped and missing fields take their default, so a file of a newer zygo reads its known fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Activity {
    /// Schema version (W§9.1); every write sets it to `VERSION`.
    pub version: u32,
    /// `true` from `UserPromptSubmit` until `Stop` or `StopFailure` (AU26 rule 1).
    pub busy: bool,
    /// Running subagents, `agent_id` to `agent_type` (AU26 rule 2). Hooks change it only through
    /// `Activity::subagent_start` and `Activity::subagent_stop`.
    pub running: BTreeMap<String, String>,
    /// Unix seconds of the last write (W§9.1).
    pub updated_at: u64,
    /// Tool calls of the main loop that started and did not end yet (W10 rounds 2 and 3): `PreToolUse` adds 1,
    /// `PostToolUse` and `PostToolUseFailure` take 1 away (never below 0), `UserPromptSubmit`, `Stop` and `SessionEnd`
    /// set 0 (H1). Above 0 the drain counts the member as busy whatever `busy` holds. Absent in the file when 0, so a
    /// file of an older zygo reads 0.
    #[serde(skip_serializing_if = "is_zero")]
    pub tools_running: u32,
}

fn is_zero(n: &u32) -> bool { *n == 0 }

impl Default for Activity {
    /// The state of a member with no `activity.json` (W§9.1): not busy, nothing running.
    fn default() -> Self {
        Activity { version: VERSION, busy: false, running: BTreeMap::new(), updated_at: 0, tools_running: 0 }
    }
}

impl Activity {
    /// Records a `SubagentStart` (AU26 rule 2, AU43): adds `agent_id` with its `agent_type`, or replaces the type of
    /// an id already in the set. An empty `agent_type` is kept as it is. `busy` does not change. The member hooks
    /// (H1) change `running` only through this method and `subagent_stop`.
    /// An empty `agent_id` names no subagent: the call does nothing.
    pub fn subagent_start(&mut self, agent_id: &str, agent_type: &str) {
        if agent_id.is_empty() {
            return;
        }
        self.running.insert(agent_id.to_string(), agent_type.to_string());
    }

    /// Records a `SubagentStop` (AU26 rule 2, AU43, PW7 7.5): removes `agent_id` and returns `true` when
    /// `subagent_start` added it; otherwise returns `false` and changes nothing. Internal agents of Claude Code send a
    /// `SubagentStop` with no `SubagentStart`, so such a stop must not touch the running set. `busy` does not change.
    /// The member hooks (H1) change `running` only through this method and `subagent_start`. An empty `agent_id`
    /// returns `false` and changes nothing.
    pub fn subagent_stop(&mut self, agent_id: &str) -> bool {
        !agent_id.is_empty() && self.running.remove(agent_id).is_some()
    }
}

/// The most bytes of `activity.json` that are read; a larger file is not parsed, and `update` writes no larger file.
const MAX_FILE: u64 = 1 << 20;

/// The result of one read of `activity.json`.
enum Load {
    Missing,
    ReadFailed(std::io::Error),
    TooLarge,
    Unparsable(String),
    /// A JSON object whose `version` is present and is not the integer 0 or 1: the file of a newer zygo. It holds
    /// the known fields, taken leniently, and the JSON text of `version`.
    Newer(Activity, String),
    Parsed(Activity),
}

fn load(path: &Path) -> Load {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Load::Missing,
        Err(e) => return Load::ReadFailed(e),
        Ok(f) => f,
    };
    let mut bytes = Vec::new();
    if let Err(e) = file.take(MAX_FILE + 1).read_to_end(&mut bytes) {
        return Load::ReadFailed(e);
    }
    if bytes.len() as u64 > MAX_FILE {
        return Load::TooLarge;
    }
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return Load::Unparsable(e.to_string()),
    };
    let serde_json::Value::Object(fields) = value else {
        return Load::Unparsable("not a JSON object".to_string());
    };
    match fields.get("version") {
        Some(v) if !matches!(v.as_u64(), Some(0 | 1)) => Load::Newer(lenient(&fields), v.to_string()),
        _ => match serde_json::from_value(serde_json::Value::Object(fields)) {
            Ok(a) => Load::Parsed(a),
            Err(e) => Load::Unparsable(e.to_string()),
        },
    }
}

/// The known fields of a newer file, whatever their types (round 2, B1): `version` when it is a u32, else
/// `u32::MAX` (newer than any zygo); `busy` when it is a bool, else `true`, so the drain does not count the member
/// as idle on a field it cannot read; each key of `running` with its type: the value when it is a string, the
/// string `type` member when the value is an object, else empty; `updated_at` when it is a number, else 0. A
/// `running` that is present but not an object gives an empty set and `busy = true`, so the drain never counts the
/// member as idle on subagents it cannot read; a missing `running` gives an empty set and leaves `busy` as read.
/// `tools_running` (W10 round 2) is the number when it is a u32 (larger numbers give `u32::MAX`), 0 when it is
/// absent, and 1 for any other value, so the drain never counts a running tool it cannot read as none.
fn lenient(fields: &serde_json::Map<String, serde_json::Value>) -> Activity {
    use serde_json::Value;
    let agent_type = |v: &Value| match v {
        Value::String(s) => s.clone(),
        Value::Object(o) => o.get("type").and_then(Value::as_str).unwrap_or_default().to_string(),
        _ => String::new(),
    };
    let running_unreadable = fields.get("running").is_some_and(|v| !v.is_object());
    Activity {
        version: fields.get("version").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(u32::MAX),
        busy: running_unreadable || fields.get("busy").and_then(Value::as_bool).unwrap_or(true),
        running: fields
            .get("running")
            .and_then(Value::as_object)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), agent_type(v))).collect())
            .unwrap_or_default(),
        updated_at: fields
            .get("updated_at")
            .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)))
            .unwrap_or(0),
        tools_running: match fields.get("tools_running") {
            None => 0,
            Some(v) => v.as_u64().map_or(1, |n| u32::try_from(n).unwrap_or(u32::MAX)),
        },
    }
}

/// Reads `activity.json` with no lock: every write is one rename, so the file is always whole (W§7.3). A missing
/// file is the default (W§9.1). These are errors, and the drain counts the member as busy (AU26): a file that
/// cannot be read or parsed or is not a JSON object (`<activity.json path>: <error>`), and a file larger than 1 MiB
/// (`<activity.json path>: activity.json larger than 1 MiB`). A file of a newer zygo gives its known fields, taken
/// leniently (see `lenient`).
pub fn read(dir: &SessionDir) -> anyhow::Result<Activity> {
    let path = dir.activity_json();
    match load(&path) {
        Load::Missing => Ok(Activity::default()),
        Load::ReadFailed(e) => Err(anyhow::anyhow!("{}: {e}", path.display())),
        Load::TooLarge => Err(anyhow::anyhow!("{}: activity.json larger than 1 MiB", path.display())),
        Load::Unparsable(e) => Err(anyhow::anyhow!("{}: {e}", path.display())),
        Load::Newer(a, _) | Load::Parsed(a) => Ok(a),
    }
}

/// The error kind of an `update` that met an existing `activity.json` it cannot read, parse or take as a JSON object,
/// or one larger than 1 MiB: a condition of the file, so every later `update` meets it again until the file changes
/// (H1 attempt-2 review round 3, S2 and S4). Its text is the error text that `update` documents. Each other error of
/// `update` (the lock, a newer zygo, a refused result, the write) is of a single call. Callers tell the two apart with
/// `anyhow::Error::is::<Unreadable>()`.
#[derive(Debug)]
pub struct Unreadable(String);

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unreadable {}

/// Read-modify-write of `activity.json` under the fs4 lock on `activity.lock` (AU56): waits for the lock at most
/// `bound` (hooks pass `LOCK_BOUND`), removes stale temp files, applies `f`, sets `version` and `updated_at`, and
/// replaces the file in one rename. A missing file starts from the default. A file that `read` cannot give (AU26:
/// the drain counts it as busy) is never replaced, because a write from the default would set `busy = false` in an
/// open turn (H1 attempt-2 review round 1, F1). These cases return an error and write nothing: the lock file cannot
/// be opened (`<activity.lock path>: open lock file`), the lock is not taken within `bound`
/// (`<activity.lock path>: lock not taken within <ms> ms`), the file cannot be read or parsed or is not a JSON
/// object (`<activity.json path>: <error>`), the file is larger than 1 MiB
/// (`<activity.json path>: activity.json larger than 1 MiB`), the file is a JSON object whose `version` is present
/// and is not the integer 0 or 1 (`<activity.json path>: written by a newer zygo (version <JSON text of version>)`),
/// or the result is larger than the read limit, so no reader could read it
/// (`<activity.json path>: the new activity is <n> bytes, more than the 1048576 bytes that every reader accepts;
/// not written`). The errors of a file that cannot be read or parsed, is not a JSON object or is larger than 1 MiB
/// are of the kind `Unreadable`.
pub fn update(dir: &SessionDir, bound: Duration, f: &mut dyn FnMut(&mut Activity)) -> anyhow::Result<()> {
    let lock = dir.activity_lock();
    let Some(_lock) = crate::state::lock_file(&lock, bound).with_context(|| format!("{}: open lock file", lock.display()))?
    else {
        anyhow::bail!("{}: lock not taken within {} ms", lock.display(), bound.as_millis());
    };
    crate::state::sweep_tmp(&dir.root);
    let path = dir.activity_json();
    let mut a = match load(&path) {
        Load::Missing => Activity::default(),
        Load::ReadFailed(e) => return Err(Unreadable(format!("{}: {e}", path.display())).into()),
        Load::TooLarge => return Err(Unreadable(format!("{}: activity.json larger than 1 MiB", path.display())).into()),
        Load::Unparsable(e) => return Err(Unreadable(format!("{}: {e}", path.display())).into()),
        Load::Newer(_, version) => anyhow::bail!("{}: written by a newer zygo (version {version})", path.display()),
        Load::Parsed(a) => a,
    };
    f(&mut a);
    a.version = VERSION;
    a.updated_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let bytes = serde_json::to_vec(&a)?;
    if bytes.len() as u64 > MAX_FILE {
        anyhow::bail!(
            "{}: the new activity is {} bytes, more than the {MAX_FILE} bytes that every reader accepts; not written",
            path.display(),
            bytes.len()
        );
    }
    write_atomic(&path, &bytes)
}

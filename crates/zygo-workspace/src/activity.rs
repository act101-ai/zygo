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
}

impl Default for Activity {
    /// The state of a member with no `activity.json` (W§9.1): not busy, nothing running.
    fn default() -> Self {
        Activity { version: VERSION, busy: false, running: BTreeMap::new(), updated_at: 0 }
    }
}

impl Activity {
    /// Records a `SubagentStart` (AU26 rule 2, AU43): adds `agent_id` with its `agent_type`, or replaces the type of
    /// an id already in the set. An empty `agent_type` is kept as it is. `busy` does not change. The member hooks
    /// (H1) change `running` only through this method and `subagent_stop`.
    pub fn subagent_start(&mut self, agent_id: &str, agent_type: &str) {
        self.running.insert(agent_id.to_string(), agent_type.to_string());
    }

    /// Records a `SubagentStop` (AU26 rule 2, AU43, PW7 7.5): removes `agent_id` and returns `true` when
    /// `subagent_start` added it; otherwise returns `false` and changes nothing. Internal agents of Claude Code send a
    /// `SubagentStop` with no `SubagentStart`, so such a stop must not touch the running set. `busy` does not change.
    /// The member hooks (H1) change `running` only through this method and `subagent_start`.
    pub fn subagent_stop(&mut self, agent_id: &str) -> bool {
        self.running.remove(agent_id).is_some()
    }
}

/// The result of one read of `activity.json`.
enum Load {
    Missing,
    ReadFailed(std::io::Error),
    Unparsable(serde_json::Error),
    Parsed(Activity),
}

fn load(path: &Path) -> Load {
    match std::fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Load::Missing,
        Err(e) => Load::ReadFailed(e),
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(a) => Load::Parsed(a),
            Err(e) => Load::Unparsable(e),
        },
    }
}

/// Reads `activity.json` with no lock: every write is one rename, so the file is always whole (W§7.3). A missing
/// file is the default (W§9.1). A file that cannot be read or parsed is an error with the text
/// `<activity.json path>: <error>`; the drain counts such a member as busy (AU26). A file of any `version` gives its
/// known fields.
pub fn read(dir: &SessionDir) -> anyhow::Result<Activity> {
    let path = dir.activity_json();
    match load(&path) {
        Load::Missing => Ok(Activity::default()),
        Load::ReadFailed(e) => Err(anyhow::anyhow!("{}: {e}", path.display())),
        Load::Unparsable(e) => Err(anyhow::anyhow!("{}: {e}", path.display())),
        Load::Parsed(a) => Ok(a),
    }
}

/// Read-modify-write of `activity.json` under the fs4 lock on `activity.lock` (AU56): waits for the lock at most
/// `bound` (hooks pass `LOCK_BOUND`), removes stale temp files, applies `f`, sets `version` and `updated_at`, and
/// replaces the file in one rename. A missing or unparsable file starts from the default, so a hook recovers from a
/// broken file. These cases return an error and write nothing: the lock file cannot be opened
/// (`<activity.lock path>: open lock file`), the lock is not taken within `bound`
/// (`<activity.lock path>: lock not taken within <ms> ms`), the file cannot be read (`<activity.json path>: <error>`),
/// or a newer zygo wrote it (`<activity.json path>: written by a newer zygo (version <n>)`).
pub fn update(dir: &SessionDir, bound: Duration, f: &mut dyn FnMut(&mut Activity)) -> anyhow::Result<()> {
    let lock = dir.activity_lock();
    let Some(_lock) = crate::state::lock_file(&lock, bound).with_context(|| format!("{}: open lock file", lock.display()))?
    else {
        anyhow::bail!("{}: lock not taken within {} ms", lock.display(), bound.as_millis());
    };
    crate::state::sweep_tmp(&dir.root);
    let path = dir.activity_json();
    let mut a = match load(&path) {
        Load::Missing | Load::Unparsable(_) => Activity::default(),
        Load::ReadFailed(e) => anyhow::bail!("{}: {e}", path.display()),
        Load::Parsed(a) if a.version > VERSION => {
            anyhow::bail!("{}: written by a newer zygo (version {})", path.display(), a.version)
        }
        Load::Parsed(a) => a,
    };
    f(&mut a);
    a.version = VERSION;
    a.updated_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    write_atomic(&path, &serde_json::to_vec(&a)?)
}

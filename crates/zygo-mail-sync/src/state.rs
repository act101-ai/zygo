//! The sync state files of one workspace (S§5, S§15): `mail/outbox/` (with `tmp/` and `refused/`), `mail/sync/`
//! (`identity`, `state.json`, `members.json`, `imported`, `incoming/`, `lock`, `again`, `last_start`) and the
//! worktree `<workspace_cache_dir>/<id>/mail-sync/`. Every write is atomic, files `0600`, folders `0700` (W-C4,
//! M-C4). No file holds a URL or mail content (W-C7, S-C2).
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ulid::Ulid;
use zygo_core::paths::Paths;

/// The paths of the sync state of one workspace (S§15; W§7.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPaths {
    /// `<workspaces_dir>/<id>/mail` (M-C2).
    pub mail: PathBuf,
    /// `mail/outbox/`: messages to remote addresses that wait for a push (S§5 Outbox).
    pub outbox: PathBuf,
    /// `mail/outbox/tmp/`: partial writes.
    pub outbox_tmp: PathBuf,
    /// `mail/outbox/refused/`: messages the secret pass refused (S§8.3 step 11).
    pub refused: PathBuf,
    /// `mail/sync/`.
    pub sync: PathBuf,
    /// `mail/sync/incoming/` (S§8.5).
    pub incoming: PathBuf,
    /// `mail/sync/imported`: the imported log (S§8.5).
    pub imported: PathBuf,
    /// `mail/sync/identity` (S§5 Identity, S§7.5).
    pub identity: PathBuf,
    /// `mail/sync/state.json` (S§8.3).
    pub state: PathBuf,
    /// `mail/sync/members.json` (S§7.3).
    pub members: PathBuf,
    /// `mail/sync/lock` (S§8.3 step 3).
    pub lock: PathBuf,
    /// `mail/sync/again` (S§8.3 step 2).
    pub again: PathBuf,
    /// `mail/sync/last_start` (S§8.4).
    pub last_start: PathBuf,
    /// `<workspace_cache_dir>/<id>/mail-sync`: the side-branch worktree (S§6.3).
    pub worktree: PathBuf,
}

impl SyncPaths {
    /// The sync paths of workspace `ws_id`. Pure: it creates and checks nothing.
    pub fn new(paths: &Paths, ws_id: &str) -> SyncPaths {
        let m = zygo_workspace::mail::mail_dir(paths, ws_id);
        let outbox = m.join("outbox");
        let sync = m.join("sync");
        SyncPaths {
            outbox_tmp: outbox.join("tmp"),
            refused: outbox.join("refused"),
            incoming: sync.join("incoming"),
            imported: sync.join("imported"),
            identity: sync.join("identity"),
            state: sync.join("state.json"),
            members: sync.join("members.json"),
            lock: sync.join("lock"),
            again: sync.join("again"),
            last_start: sync.join("last_start"),
            worktree: paths.workspace_cache_dir().join(ws_id).join("mail-sync"),
            mail: m,
            outbox,
            sync,
        }
    }
}

/// W-C4, M-C4: makes `path`'s folder (and its parents) `0700`, then writes `bytes` to a `0600` temp file in it,
/// syncs it and renames it over `path` (`zygo_core::session_dir::write_atomic`).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "write_atomic: the path has no folder",
            )
        })?;
    zygo_workspace::mail::ensure_mail_root(dir)?;
    zygo_core::session_dir::write_atomic(path, bytes).map_err(to_io)
}

/// The `io::Error` inside the core writer's `anyhow::Error` (a failed rename is a `tempfile::PersistError`).
fn to_io(e: anyhow::Error) -> io::Error {
    match e.downcast::<io::Error>() {
        Ok(e) => e,
        Err(e) => match e.downcast::<tempfile::PersistError>() {
            Ok(p) => p.error,
            Err(e) => io::Error::other(e),
        },
    }
}

/// Exactly 32 characters, each `0-9` or `a-f`.
fn valid_identity(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// S§5 Identity, S§7.5: the workspace's sync identity, `None` when the file does not exist yet. A file that is not
/// 32 lower-case hex characters (one trailing `\n` allowed) is `InvalidData`.
pub fn identity(sp: &SyncPaths) -> std::io::Result<Option<String>> {
    let text = match fs::read_to_string(&sp.identity) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let v = text.strip_suffix('\n').unwrap_or(&text);
    if valid_identity(v) {
        Ok(Some(v.to_string()))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mail/sync/identity is not 32 lower-case hex characters",
        ))
    }
}

/// S§5 Identity, S§7.5: the existing identity, or a new random one (128 bits, lower-case hex) written `0600`. It
/// is per workspace and not a secret; the first sync makes it. `--new-identity` deletes the file first.
pub fn make_identity(sp: &SyncPaths) -> std::io::Result<String> {
    if let Some(v) = identity(sp)? {
        return Ok(v);
    }
    let v = format!("{:032x}", rand::random::<u128>());
    write_atomic(&sp.identity, v.as_bytes())?;
    Ok(v)
}

/// S§8.2: the cached public check.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicCache {
    /// The SHA-256 hex of each checked URL without user info; never the URL (W-C7).
    pub url_sha256: Vec<String>,
    /// The verdict of the check.
    pub verdict: String,
    /// Unix seconds of the injected clock at the check.
    pub at: u64,
}

/// `mail/sync/state.json` (S§8.3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Unix seconds of the last sync that passed (S§8.3).
    pub last_ok: Option<u64>,
    /// The name of the check that failed last; a check name only (S§8.3, S-C2).
    pub last_error: Option<String>,
    /// The cached public check (S§8.2).
    pub public: Option<PublicCache>,
    /// Own file's branch path (`registry/<host>.json`, `ack/<host>.json`) → blob id last pushed (S§7.5).
    pub pushed: BTreeMap<String, String>,
    /// Own file's branch path → blob id about to be pushed (S§7.5).
    pub pending: BTreeMap<String, String>,
    /// The S§6.6 once-only note was shown.
    pub ci_note_shown: bool,
}

/// S§8.3: the state; the default for a missing or unparsable file.
pub fn load(sp: &SyncPaths) -> State {
    fs::read(&sp.state)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// S§8.3: writes the state atomically.
pub fn save(sp: &SyncPaths, s: &State) -> std::io::Result<()> {
    write_atomic(
        &sp.state,
        &serde_json::to_vec_pretty(s).map_err(io::Error::other)?,
    )
}

/// S§8.5: each ULID in the imported log. Only `\n`-ended lines count: the bytes after the last `\n` are a torn
/// entry from a crash and are ignored; a line that is not a ULID is skipped. No file: an empty set.
pub fn imported_load(sp: &SyncPaths) -> std::io::Result<HashSet<Ulid>> {
    let bytes = match fs::read(&sp.imported) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(e) => return Err(e),
    };
    let mut lines = bytes.split(|b| *b == b'\n');
    lines.next_back(); // after the last `\n`: empty, or a torn entry
    Ok(lines
        .filter_map(|l| {
            std::str::from_utf8(l)
                .ok()
                .and_then(|s| Ulid::from_string(s).ok())
        })
        .collect())
}

/// S§8.5 step 2: appends `<ULID>\n` (27 bytes) and syncs the file. When the file does not end with `\n` (a torn
/// entry), a `\n` goes first, so the new entry is a line of its own. The log is never trimmed (S§13 limit 6).
pub fn imported_append(sp: &SyncPaths, id: Ulid) -> std::io::Result<()> {
    zygo_workspace::mail::ensure_mail_root(&sp.sync)?;
    let mut o = OpenOptions::new();
    o.read(true).append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut f = o.open(&sp.imported)?;
    let mut line = String::with_capacity(28);
    if f.metadata()?.len() > 0 {
        let mut last = [0u8; 1];
        f.seek(SeekFrom::End(-1))?;
        f.read_exact(&mut last)?;
        if last[0] != b'\n' {
            line.push('\n');
        }
    }
    line.push_str(&id.to_string());
    line.push('\n');
    f.write_all(line.as_bytes())?;
    f.sync_all()
}

/// S§8.3 step 2: asks the lock holder for one more pass.
pub fn mark_again(sp: &SyncPaths) -> std::io::Result<()> {
    write_atomic(&sp.again, b"")
}

/// S§8.3 steps 4 and 17: clears the mark and tells whether it was set, in one call.
pub fn take_again(sp: &SyncPaths) -> std::io::Result<bool> {
    match fs::remove_file(&sp.again) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// S§8.4: the Unix seconds of the last sync start; `None` when the file is missing or not a number.
pub fn read_last_start(sp: &SyncPaths) -> Option<u64> {
    fs::read_to_string(&sp.last_start)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// S§8.4: writes the start time in its own file, atomically; the hook path is its only writer.
pub fn write_last_start(sp: &SyncPaths, unix: u64) -> std::io::Result<()> {
    write_atomic(&sp.last_start, unix.to_string().as_bytes())
}

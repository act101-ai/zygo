//! The sync state files of one workspace (S§5, S§15): `mail/outbox/` (with `tmp/` and `refused/`), `mail/sync/`
//! (`identity`, `state.json`, `members.json`, `imported`, `incoming/`, `lock`, `again`, `last_start`) and the
//! worktree `<workspace_cache_dir>/<id>/mail-sync/`. Every write is atomic, files `0600`, folders `0700` (W-C4,
//! M-C4); `mail/` and `mail/sync/` are checked at each write and a link at either is refused. A read refuses a
//! link and does not wait on a FIFO. No file holds a URL or mail content (W-C7, S-C2).
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, File, OpenOptions};
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
    /// The sync paths of workspace `ws_id`. An id that is not `^[0-9a-f]{16}$` (AU84) is `InvalidInput`, so no
    /// path leaves `workspaces_dir` or `workspace_cache_dir`. It creates nothing.
    pub fn new(paths: &Paths, ws_id: &str) -> std::io::Result<SyncPaths> {
        if !zygo_workspace::identity::valid_ws_id(ws_id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the workspace id is not 16 lower-case hex characters",
            ));
        }
        let m = zygo_workspace::mail::mail_dir(paths, ws_id);
        let outbox = m.join("outbox");
        let sync = m.join("sync");
        Ok(SyncPaths {
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
        })
    }
}

/// W-C4, M-C4: makes `path`'s folder `0700` (no parent: a missing parent is `NotFound`, MS-21 review R2-1), then writes `bytes` to a `0600` temp file in it,
/// syncs it and renames it over `path` (`zygo_core::session_dir::write_atomic`), then syncs the folder (unix).
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
    zygo_workspace::mail::ensure_mail_dir(dir)?;
    zygo_core::session_dir::write_atomic(path, bytes).map_err(to_io)?;
    sync_dir(dir)
}

/// Syncs folder `dir` (unix), so a rename or a new name in it lasts through a power loss: S§19 #14 counts on the
/// imported log and the state files after a crash. No test can show a power loss.
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// `write_atomic` for a file under `mail/sync/` or `mail/outbox/`: `mail/` itself is checked first (`0700`, no
/// link, M-C4), as `write_atomic` checks only the file's own folder.
fn write_file(sp: &SyncPaths, path: &Path, bytes: &[u8]) -> io::Result<()> {
    zygo_workspace::mail::ensure_mail_dir(&sp.mail)?;
    write_atomic(path, bytes)
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

/// Opens `path` for reading with no link follow and no wait on a FIFO. `Ok(None)` when it does not exist; a link
/// or anything that is not a regular file is `InvalidInput`. (`zygo_workspace::mail::store::open_regular` has a
/// size cap for messages and gives no error kind, so it does not fit here.)
fn open_regular(path: &Path) -> io::Result<Option<File>> {
    let refused = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        )
    };
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(not(unix))]
    match fs::symlink_metadata(path) {
        Ok(m) if !m.is_file() => return Err(refused()),
        _ => {}
    }
    let f = match o.open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        #[cfg(unix)]
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Err(refused()),
        Err(e) => return Err(e),
    };
    if !f.metadata()?.is_file() {
        return Err(refused());
    }
    Ok(Some(f))
}

/// The bytes of the regular file `path` (`open_regular`); `Ok(None)` when it does not exist.
pub(crate) fn read_regular(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let Some(mut f) = open_regular(path)? else {
        return Ok(None);
    };
    let mut b = Vec::new();
    f.read_to_end(&mut b)?;
    Ok(Some(b))
}

/// `s` without one line ending, `\r\n` or `\n`.
fn strip_eol(s: &[u8]) -> &[u8] {
    s.strip_suffix(b"\r\n")
        .or_else(|| s.strip_suffix(b"\n"))
        .unwrap_or(s)
}

/// Exactly 32 characters, each `0-9` or `a-f`.
pub(crate) fn valid_identity(s: &[u8]) -> bool {
    s.len() == 32 && s.iter().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// S§5 Identity, S§7.5: the workspace's sync identity, `None` when the file does not exist yet. A file that is not
/// 32 lower-case hex characters (one `\n` or `\r\n` ending allowed) is `InvalidData`; a link is refused.
pub fn identity(sp: &SyncPaths) -> std::io::Result<Option<String>> {
    let Some(b) = read_regular(&sp.identity)? else {
        return Ok(None);
    };
    let v = strip_eol(&b);
    if valid_identity(v) {
        Ok(Some(String::from_utf8_lossy(v).into_owned()))
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
    write_file(sp, &sp.identity, v.as_bytes())?;
    Ok(v)
}

/// S§8.2: the cached public check. A missing `url_sha256` or `at` takes its default, which makes the cache stale
/// (no URL hash matches, `at` 0), so the check runs again. A missing `verdict` is `""`, which `check_state`
/// refuses, so `load` refuses the file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PublicCache {
    /// The SHA-256 hex of each checked URL without user info; never the URL (W-C7).
    pub url_sha256: Vec<String>,
    /// The verdict of the check: one of `VERDICTS`.
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
    /// Own file's branch path (`registry/<host>.json`, `ack/<host>.json`) → blob id last pushed (S§7.5); only the
    /// paths of own hosts ([`forget_hosts`]).
    pub pushed: BTreeMap<String, String>,
    /// Own file's branch path → blob id about to be pushed (S§7.5).
    pub pending: BTreeMap<String, String>,
    /// The S§6.6 once-only note was shown.
    pub ci_note_shown: bool,
}

/// The verdicts of the public check (S§8.2 steps 2 and 4).
const VERDICTS: [&str; 4] = ["public", "private", "local", "unknown"];

/// The longest check name `save` takes.
const CHECK_NAME_MAX: usize = 64;

fn lower_hex(s: &str, lens: &[usize]) -> bool {
    lens.contains(&s.len()) && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// A check name: `^[a-z0-9-]{1,64}$`. The spec gives no closed list of check names (S§8.3, S-C2).
fn valid_check_name(s: &str) -> bool {
    (1..=CHECK_NAME_MAX).contains(&s.len())
        && s.bytes()
            .all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

/// The `<host>` of an own file's branch path `registry/<host>.json` or `ack/<host>.json`, as written; `None` for
/// any other form.
fn own_path_host(s: &str) -> Option<&str> {
    s.strip_prefix("registry/")
        .or_else(|| s.strip_prefix("ack/"))
        .and_then(|r| r.strip_suffix(".json"))
}

/// An own file's branch path: `registry/<host>.json` or `ack/<host>.json`, the host in the S§5 form
/// `^[a-z0-9][a-z0-9-]{0,31}$`.
fn valid_own_path(s: &str) -> bool {
    own_path_host(s).is_some_and(|h| {
        (1..=32).contains(&h.len())
            && h.bytes()
                .all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'-'))
            && !h.starts_with('-')
    })
}

/// W-C7, S-C2: every value of `s` is one the spec allows, so no URL, credential or content reaches the file.
fn check_state(s: &State) -> io::Result<()> {
    let bad = |what: &str| {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("state.json: {what} is not allowed"),
        ))
    };
    if s.last_error
        .as_deref()
        .is_some_and(|e| !valid_check_name(e))
    {
        return bad("last_error");
    }
    if let Some(p) = &s.public {
        if !VERDICTS.contains(&p.verdict.as_str()) {
            return bad("public.verdict");
        }
        if !p.url_sha256.iter().all(|h| lower_hex(h, &[64])) {
            return bad("public.url_sha256");
        }
    }
    // a git blob id: SHA-1 (40) or SHA-256 (64) hex
    for (what, m) in [("pushed", &s.pushed), ("pending", &s.pending)] {
        if !m
            .iter()
            .all(|(k, v)| valid_own_path(k) && lower_hex(v, &[40, 64]))
        {
            return bad(what);
        }
    }
    Ok(())
}

/// S§8.3: the state. A missing file is the default; a link, a read error or a file that is not a JSON object of
/// this form is an error, or a file that holds a value `check_state` refuses (S§8.3: one rule set at read and at
/// write), so the S§7.5 twin check never runs on a silent default.
pub fn load(sp: &SyncPaths) -> std::io::Result<State> {
    let Some(b) = read_regular(&sp.state)? else {
        return Ok(State::default());
    };
    let bad = |e: serde_json::Error| io::Error::new(io::ErrorKind::InvalidData, e);
    let v: serde_json::Value = serde_json::from_slice(&b).map_err(bad)?;
    if !v.is_object() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "state.json is not a JSON object",
        ));
    }
    let s: State = serde_json::from_value(v).map_err(bad)?;
    check_state(&s).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(s)
}

/// S§7.5, S-C2: removes from `pushed` and `pending` each key whose host is not in `hosts` and that `keep` does not
/// name, so the state holds no name of a host that is no longer an own host. The caller saves the state. Whether a
/// key went.
pub fn forget_hosts(s: &mut State, hosts: &BTreeSet<String>, keep: &BTreeSet<String>) -> bool {
    let before = s.pushed.len() + s.pending.len();
    let stays = |k: &String| keep.contains(k) || own_path_host(k).is_some_and(|h| hosts.contains(h));
    s.pushed.retain(|k, _| stays(k));
    s.pending.retain(|k, _| stays(k));
    s.pushed.len() + s.pending.len() != before
}

/// The caller holds `mail/sync/lock`: the pass under the lock is the only writer of `state.json` (S§15).
/// S§8.3: writes the state atomically. A value outside the spec (`check_state`) is `InvalidInput`, and nothing is
/// written.
pub fn save(sp: &SyncPaths, s: &State) -> std::io::Result<()> {
    check_state(s)?;
    write_file(
        sp,
        &sp.state,
        &serde_json::to_vec_pretty(s).map_err(io::Error::other)?,
    )
}

/// S§8.5: each ULID in the imported log. Only `\n`-ended lines count (a `\r` before the `\n` is allowed): the bytes
/// after the last `\n` are a torn entry from a crash and are ignored; a line that is not a ULID is skipped. No
/// file: an empty set. A link is refused.
pub fn imported_load(sp: &SyncPaths) -> std::io::Result<HashSet<Ulid>> {
    let Some(bytes) = read_regular(&sp.imported)? else {
        return Ok(HashSet::new());
    };
    let mut lines = bytes.split(|b| *b == b'\n');
    lines.next_back(); // after the last `\n`: empty, or a torn entry
    Ok(lines
        .filter_map(|l| {
            let l = l.strip_suffix(b"\r").unwrap_or(l);
            std::str::from_utf8(l)
                .ok()
                .and_then(|s| Ulid::from_string(s).ok())
        })
        .collect())
}

/// S§8.5 step 2: appends `<ULID>\n` (27 bytes) and syncs the file; the file is set to `0600` (unix). When the file
/// does not end with `\n` (a torn entry), a `\n` goes first, so the new entry is a line of its own. The log is
/// never trimmed (S§13 limit 6). `mail/` and `mail/sync/` are checked first.
pub fn imported_append(sp: &SyncPaths, id: Ulid) -> std::io::Result<()> {
    zygo_workspace::mail::ensure_mail_dir(&sp.mail)?;
    zygo_workspace::mail::ensure_mail_dir(&sp.sync)?;
    let mut o = OpenOptions::new();
    o.read(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let (mut f, created) = match o.clone().create_new(true).open(&sp.imported) {
        Ok(f) => (f, true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (o.open(&sp.imported)?, false),
        Err(e) => return Err(e),
    };
    let meta = f.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mail/sync/imported is not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o7777 != 0o600 {
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
    }
    let mut line = String::with_capacity(28);
    if meta.len() > 0 {
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
    f.sync_all()?;
    if created {
        // the new name in `sync/` must last too (S§19 #14)
        sync_dir(&sp.sync)?;
    }
    Ok(())
}

/// S§8.3 step 2: asks the lock holder for one more pass.
pub fn mark_again(sp: &SyncPaths) -> std::io::Result<()> {
    write_file(sp, &sp.again, b"")
}

/// S§8.3 steps 4 and 17: clears the mark and tells whether it was set, in one call.
pub fn take_again(sp: &SyncPaths) -> std::io::Result<bool> {
    match fs::remove_file(&sp.again) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// S§8.4: the Unix seconds of the last sync start; `None` when the file is missing, a link, not a regular file or
/// not a number.
pub fn read_last_start(sp: &SyncPaths) -> Option<u64> {
    let b = read_regular(&sp.last_start).ok()??;
    std::str::from_utf8(&b).ok()?.trim().parse::<u64>().ok()
}

/// S§8.4: writes the start time in its own file, atomically; `spawn::trigger` with a gap (the hooks and `zygo mail
/// watch`) is its only writer.
pub fn write_last_start(sp: &SyncPaths, unix: u64) -> std::io::Result<()> {
    write_file(sp, &sp.last_start, unix.to_string().as_bytes())
}

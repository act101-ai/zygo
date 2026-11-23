//! Snapshot: list-panes read, shrink rule, key assignment, pending rule. Owner: node W6 (W§7.4, W§7.5).
use std::collections::BTreeSet;
use std::fs;
use std::io::{ErrorKind, Read as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail};
use zygo_core::paths::Paths;
use zygo_core::session_dir::write_atomic;

use crate::identity::{new_pane_key, valid_pane_key, valid_ws_id};
use crate::mux::{parse_pane_line, Multiplexer, MuxError, PaneRow, SessionAnswer, WindowRow};
use crate::state::{lock_file, MemberState, Pane, PaneKind, Phase, StateError, Store, Window, Workspace};

/// The result of one snapshot (W§7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotOutcome {
    /// `windows[]` and `panes[]` were written (W§7.4 step 5).
    Written,
    /// Nothing was written; the reason says why.
    Skipped(SkipReason),
    /// The snapshot is in `snapshot.pending` and runs after the next write under `lock` (W§7.3).
    Pending,
}

/// Why a snapshot wrote nothing (W§7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The workspace id is not valid (AU84); no file was read or made and no tmux call ran.
    InvalidId,
    /// No `workspace.json` exists (W§7.4 step 1).
    NoState,
    /// The phase does not allow this snapshot (AU38; W§7.4 steps 1 and 4).
    Phase,
    /// `tmux_session` changed between the first read and the lock (W§7.4 step 4).
    Changed,
    /// `tmux_socket` is not set (W§7.4 step 1).
    NoSocket,
    /// The session or its server is gone (AU87; W§7.4 steps 2-4); no log line.
    Gone,
    /// One line was written to `errors.log` (W§7.4 steps 1, 2, 4 and 5).
    Logged,
    /// `after_write` found no `snapshot.pending` (W§7.3).
    NoPending,
}

/// The lock wait of tmux hooks, member events and the member wrapper (W§7.3).
pub const HOOK_WAIT: Duration = Duration::from_secs(1);
/// The lock wait of `up` and `down` (W§7.3).
pub const OP_WAIT: Duration = Duration::from_secs(5);

/// The wait before each further read of the shrink rule (AU89).
const SHRINK_WAIT: Duration = Duration::from_secs(2);
/// The most reads of the shrink rule, the first one included (AU89).
const SHRINK_READS: usize = 4;
/// The second try for `lock` after a snapshot set `snapshot.pending` (W§7.3).
const PROBE_WAIT: Duration = Duration::from_millis(100);
const PENDING_FILE: &str = "snapshot.pending";
/// The name of a taken `snapshot.pending`; the `.tmp*` sweep never matches it (W§7.3).
const TAKEN_PREFIX: &str = "snapshot.pending.taken-";
/// The name a recoverer claims a dead taker's file under; neither the recovery scan nor the `.tmp*` sweep matches it
/// (round 3 D1).
const RECOVER_PREFIX: &str = "snapshot.pending.recover-";
/// A taken file older than this belongs to no live taker (round 2 M1); the `.tmp*` sweep uses the same age (W§7.3).
const TAKEN_MAX_AGE: Duration = Duration::from_secs(60);
/// The most bytes read from a pending or taken file (round 2 M7); `force` has 5.
const FLAG_MAX: u64 = 64;
/// The W§7.1 name of the state lock.
const LOCK_FILE: &str = "lock";
const PANE_OPTION: &str = "@zygo-pane";
const WS_OPTION: &str = "@zygo-ws";

/// Test hooks (W6 round 2 M5). Only the `test-hooks` feature builds them.
#[cfg(feature = "test-hooks")]
pub mod testing {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    type Hook = Arc<dyn Fn() + Send + Sync>;

    /// The hooks by (point, state folder), so parallel tests do not meet.
    static HOOKS: Mutex<BTreeMap<(&'static str, PathBuf), Hook>> = Mutex::new(BTreeMap::new());

    const AFTER_QUEUE: &str = "after_queue";
    const AFTER_TAKE: &str = "after_take";

    fn set(point: &'static str, dir: &Path, hook: Option<Box<dyn Fn() + Send + Sync>>) {
        let mut hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
        match hook {
            Some(h) => {
                hooks.insert((point, dir.to_path_buf()), Arc::from(h));
            }
            None => {
                hooks.remove(&(point, dir.to_path_buf()));
            }
        }
    }

    /// Runs the hook of (`point`, `dir`), if one is set, outside the lock of the hook table.
    fn fire(point: &'static str, dir: &Path) {
        let hook = HOOKS.lock().unwrap_or_else(|e| e.into_inner()).get(&(point, dir.to_path_buf())).cloned();
        if let Some(h) = hook {
            h();
        }
    }

    /// Sets (`Some`) or clears (`None`) the hook that a snapshot of the state folder `dir` runs at the D14 point:
    /// after it set `snapshot.pending` on `Busy`, and before its probe of `lock`. A test uses it to order a lock
    /// release or a take before the probe without any timing.
    pub fn after_queue(dir: &Path, hook: Option<Box<dyn Fn() + Send + Sync>>) {
        set(AFTER_QUEUE, dir, hook);
    }

    /// Sets (`Some`) or clears (`None`) the hook that a take in the state folder `dir` runs between its rename of
    /// `snapshot.pending` and its read of the taken file (round 3 D1).
    pub fn after_take(dir: &Path, hook: Option<Box<dyn Fn() + Send + Sync>>) {
        set(AFTER_TAKE, dir, hook);
    }

    pub(super) fn run_after_queue(dir: &Path) {
        fire(AFTER_QUEUE, dir);
    }

    pub(super) fn run_after_take(dir: &Path) {
        fire(AFTER_TAKE, dir);
    }
}

type Rows = Vec<(WindowRow, PaneRow)>;

/// One checked `list-panes` read.
enum Read {
    Gone,
    Rows(Rows),
}

/// Why a pane got no key under the lock (D11).
enum KeyError {
    Gone,
    Closed,
    Invalid(String),
    Tmux(String),
}

/// What the closure given to `Store::update` found (D9).
enum Locked {
    Write,
    Skip(SkipReason),
    Conflict,
    /// A pane call under the lock failed (the pane closed, or another tmux error); the text is the log line that is
    /// written when `has-session` after the lock does not say gone (round 2 M6).
    Pane(String),
    Gone,
    Failed(String),
}

/// One pass of W§7.4 steps 1-5.
enum Attempt {
    Done(SnapshotOutcome),
    Conflict,
    Busy,
    Unstable,
}

/// Whether a snapshot may run in `phase` (AU38, D16).
fn gated(phase: Phase, force: bool) -> bool {
    match phase {
        Phase::Up | Phase::Draining => true,
        Phase::Building | Phase::Stopping => force,
        Phase::Down => false,
    }
}

/// One `errors.log` line (D17).
fn log(store: &Store, event: Option<&str>, text: &str) {
    match event {
        Some(e) => store.log_error(&format!("snapshot ({e}): {text}")),
        None => store.log_error(&format!("snapshot: {text}")),
    }
}

fn done(reason: SkipReason) -> Attempt {
    Attempt::Done(SnapshotOutcome::Skipped(reason))
}

/// The owner check of W§7.4 step 2 (AU63, D3, D4): `Ok(true)` when `@zygo-ws` names `ws_id`; `Ok(false)` when it
/// names another value, or is unset while the session exists; `Err(None)` when the session or server is gone;
/// `Err(Some(text))` for another tmux error.
fn owner(mux: &dyn Multiplexer, session: &str, ws_id: &str) -> Result<bool, Option<String>> {
    match mux.show_session_option(session, WS_OPTION) {
        Ok(Some(v)) => Ok(valid_ws_id(&v) && v == ws_id),
        Ok(None) => match mux.session_exists(session) {
            SessionAnswer::Gone => Err(None),
            SessionAnswer::Exists => Ok(false),
            SessionAnswer::Error { message } => Err(Some(format!("has-session failed: {message}"))),
        },
        Err(MuxError::Gone { .. }) => Err(None),
        Err(e) => Err(Some(format!("show-options {WS_OPTION} failed: {e}"))),
    }
}

/// One `list-panes` read, checked as W§7.4 step 2 says (D6, D7): rows sorted by window, then pane index; a pane that
/// a linked window lists again is kept once, in its first window; a set key must be valid and on one pane only.
/// `Err` holds the log text.
fn read_panes(mux: &dyn Multiplexer, session: &str) -> Result<Read, String> {
    let lines = match mux.list_panes(session) {
        Ok(l) => l,
        Err(MuxError::Gone { .. }) => return Ok(Read::Gone),
        Err(e) => return Err(format!("list-panes failed: {e}")),
    };
    if lines.is_empty() {
        return Err("list-panes gave no pane".to_string());
    }
    let mut rows = Rows::with_capacity(lines.len());
    for l in &lines {
        rows.push(parse_pane_line(l).map_err(|e| format!("{e}: {l:?}"))?);
    }
    rows.sort_by_key(|(w, p)| (w.index, p.index));
    let mut seen = BTreeSet::new();
    rows.retain(|(_, p)| seen.insert(p.id.clone()));
    let mut keys = BTreeSet::new();
    for (_, p) in &rows {
        if p.key.is_empty() {
            continue;
        }
        if !valid_pane_key(&p.key) {
            return Err(format!("invalid {PANE_OPTION} {:?} on pane {}", p.key, p.id));
        }
        if !keys.insert(p.key.as_str()) {
            return Err(format!("pane key {} is on two panes", p.key));
        }
    }
    Ok(Read::Rows(rows))
}

fn pane_ids(rows: &Rows) -> BTreeSet<String> {
    rows.iter().map(|(_, p)| p.id.clone()).collect()
}

/// `^%[0-9]+$`.
fn valid_pane_id(s: &str) -> bool {
    s.strip_prefix('%').is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

/// The pane kind of AU53: the member's agent when its record is `running` or `resumable`, else `shell`.
fn kind_of(ws: &Workspace, key: &str) -> PaneKind {
    match ws.members.get(key) {
        Some(m) if matches!(m.state, MemberState::Running | MemberState::Resumable) => match m.launch.kind {
            zygo_core::Agent::ClaudeCode => PaneKind::ClaudeCode,
            zygo_core::Agent::OpenCode => PaneKind::OpenCode,
        },
        _ => PaneKind::Shell,
    }
}

/// The key of `pane` under the lock (W§7.4 step 4, AU41, D11): reads `@zygo-pane` again and uses a valid value (set by
/// another writer meanwhile); when it is unset, makes a key that `taken` does not hold and sets it. The `bool` is true
/// when the key was set now. Each key it gives is added to `taken`.
fn key_pane(mux: &dyn Multiplexer, pane: &str, taken: &mut BTreeSet<String>) -> Result<(String, bool), KeyError> {
    match mux.show_pane_option(pane, PANE_OPTION) {
        Ok(Some(v)) if valid_pane_key(&v) => {
            taken.insert(v.clone());
            Ok((v, false))
        }
        Ok(Some(v)) => Err(KeyError::Invalid(format!("invalid {PANE_OPTION} {v:?} on pane {pane}"))),
        Ok(None) => {
            let mut key = new_pane_key();
            while taken.contains(&key) {
                key = new_pane_key();
            }
            match mux.set_pane_option(pane, PANE_OPTION, &key) {
                Ok(()) => {
                    taken.insert(key.clone());
                    Ok((key, true))
                }
                Err(MuxError::Gone { .. }) => Err(KeyError::Gone),
                Err(MuxError::NoTarget { .. }) => Err(KeyError::Closed),
                Err(e) => Err(KeyError::Tmux(format!("set-option {PANE_OPTION} on pane {pane} failed: {e}"))),
            }
        }
        Err(MuxError::Gone { .. }) => Err(KeyError::Gone),
        Err(e) => Err(KeyError::Tmux(format!("show-options {PANE_OPTION} on pane {pane} failed: {e}"))),
    }
}

fn key_text(e: KeyError, pane: &str) -> String {
    match e {
        KeyError::Gone => "the tmux session or server is gone".to_string(),
        KeyError::Closed => format!("pane {pane} does not exist"),
        KeyError::Invalid(t) | KeyError::Tmux(t) => t,
    }
}

/// W§7.4 step 5: replaces `windows[]` and `panes[]` by the read, sets `snapshot_at` and adds 1 to `snapshot_seq`.
/// `keys` has one key per row. Member records stay. A pane that has the key of a stored pane keeps that pane's
/// unknown fields (AU78). A window keeps the unknown fields of the stored window with its index only when that stored
/// window had a pane whose key is on a pane of this window now (round 3 D5): a new window at the index of a closed one
/// gets none.
fn apply(ws: &mut Workspace, rows: &Rows, keys: &[String]) {
    let mut windows: Vec<Window> = Vec::new();
    for (w, _) in rows {
        if windows.last().is_some_and(|x| x.index == w.index) {
            continue;
        }
        let now: BTreeSet<&str> =
            rows.iter().zip(keys).filter(|((x, _), _)| x.index == w.index).map(|(_, k)| k.as_str()).collect();
        let same = ws.panes.iter().any(|p| p.window == w.index && now.contains(p.key.as_str()));
        let extra = match ws.windows.iter().find(|x| x.index == w.index) {
            Some(old) if same => old.extra.clone(),
            _ => Default::default(),
        };
        windows.push(Window {
            index: w.index,
            name: (!w.automatic_rename).then(|| w.name.clone()),
            layout: w.layout.clone(),
            width: w.width,
            height: w.height,
            active: w.active,
            zoomed: w.zoomed,
            extra,
        });
    }
    let panes: Vec<Pane> = rows
        .iter()
        .zip(keys)
        .map(|((w, p), key)| Pane {
            key: key.clone(),
            window: w.index,
            index: p.index,
            active: p.active,
            cwd: p.cwd.clone(),
            kind: kind_of(ws, key),
            extra: ws.panes.iter().find(|x| x.key == *key).map(|x| x.extra.clone()).unwrap_or_default(),
        })
        .collect();
    ws.windows = windows;
    ws.panes = panes;
    ws.snapshot_at = Some(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()));
    ws.snapshot_seq += 1;
}

/// The work of W§7.4 step 4 and 5 under the lock: the checks, the keys, then the new layout.
fn under_lock(cur: &mut Workspace, mux: &dyn Multiplexer, rows: &Rows, session: &str, seq: u64, force: bool) -> Locked {
    if !gated(cur.phase, force) {
        return Locked::Skip(SkipReason::Phase);
    }
    if cur.tmux_session != session || cur.tmux_socket.as_deref() != mux.socket() {
        return Locked::Skip(SkipReason::Changed);
    }
    if cur.snapshot_seq != seq {
        return Locked::Conflict;
    }
    let mut taken: BTreeSet<String> = cur
        .panes
        .iter()
        .map(|p| p.key.clone())
        .chain(cur.members.keys().cloned())
        .chain(rows.iter().filter(|(_, p)| !p.key.is_empty()).map(|(_, p)| p.key.clone()))
        .collect();
    let mut keys = Vec::with_capacity(rows.len());
    let mut seen = BTreeSet::new();
    for (_, p) in rows {
        let key = if p.key.is_empty() {
            match key_pane(mux, &p.id, &mut taken) {
                Ok((k, _)) => k,
                Err(KeyError::Gone) => return Locked::Gone,
                Err(KeyError::Closed) => return Locked::Pane(format!("pane {} closed during the snapshot", p.id)),
                Err(KeyError::Tmux(t)) => return Locked::Pane(t),
                Err(KeyError::Invalid(t)) => return Locked::Failed(t),
            }
        } else {
            p.key.clone()
        };
        if !seen.insert(key.clone()) {
            return Locked::Failed(format!("pane key {key} is on two panes"));
        }
        keys.push(key);
    }
    apply(cur, rows, &keys);
    Locked::Write
}

/// Sets `snapshot.pending` (W§7.3, D14): `force` replaces any file with `force` in one rename; else an empty file is
/// made only when none exists. A state folder that is a link or not a folder is refused with nothing written (FIX-17
/// Round 2 F5).
fn queue(store: &Store, force: bool) -> anyhow::Result<()> {
    store.check_dir().map_err(|e| anyhow!("{e}"))?;
    let path = store.dir().join(PENDING_FILE);
    if force {
        return write_atomic(&path, b"force").map_err(|e| anyhow!("{}: {e:#}", path.display()));
    }
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    match o.open(&path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(anyhow!("{}: {e}", path.display())),
    }
}

/// [`queue`], with one log line when it fails.
fn queue_logged(store: &Store, event: Option<&str>, force: bool) -> anyhow::Result<()> {
    queue(store, force).inspect_err(|e| log(store, event, &format!("cannot set {PENDING_FILE}: {e:#}")))
}

/// A new taken name: [`TAKEN_PREFIX`] and 128 random bits in hex (round 2 M1).
fn taken_path(store: &Store) -> PathBuf {
    store.dir().join(format!("{TAKEN_PREFIX}{:032x}", rand::random::<u128>()))
}

/// The flag of a taken file (round 2 M7): `Ok(true)` when it holds exactly `force`. A file that is not a regular file
/// (a folder, a FIFO, a symlink, ...) is never opened and gives `Err`. The open does not follow a symlink and does not
/// block. The read stops after [`FLAG_MAX`] bytes.
fn read_flag(path: &Path) -> Result<bool, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if !meta.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let mut o = fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let f = o.open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut bytes = Vec::with_capacity(FLAG_MAX as usize);
    f.take(FLAG_MAX).read_to_end(&mut bytes).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(bytes == b"force")
}

/// Removes a taken entry of any type; `Err` holds the log text.
fn remove_entry(path: &Path) -> Result<(), String> {
    let removed = match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        _ => fs::remove_file(path),
    };
    removed.map_err(|e| format!("cannot remove {}: {e}", path.display()))
}

/// Sets the file time of `path` to now, without following a symlink and without opening the entry (round 3 D1).
/// Every error is ignored: the rename that follows reports a missing file.
fn touch(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        if let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) {
            // SAFETY: `c` is a valid NUL-terminated path; a null `times` means "now" for both times.
            unsafe {
                libc::utimensat(libc::AT_FDCWD, c.as_ptr(), std::ptr::null(), libc::AT_SYMLINK_NOFOLLOW);
            }
        }
    }
    #[cfg(not(unix))]
    {
        if let Ok(f) = fs::OpenOptions::new().write(true).open(path) {
            let _ = f.set_modified(SystemTime::now());
        }
    }
}

/// Queues again each taken file older than [`TAKEN_MAX_AGE`] (round 2 M1, round 3 D1). A taker sets the time of
/// `snapshot.pending` to now just before its rename, and reads and removes the taken file before it runs the
/// snapshot, so a taken file lives only for that read: one older than 60 s was left by a process that died. Each one
/// is claimed by a rename to `snapshot.pending.recover-<128-bit hex>` (so one recoverer gets it, and this scan never
/// matches the claimed name); its flag is read, the file is removed at once, then `snapshot.pending` is set with that
/// flag (OR-ed with a `force` already there, [`queue`]), and one log line is written.
fn recover_taken(store: &Store) {
    let Ok(entries) = fs::read_dir(store.dir()) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(TAKEN_PREFIX) {
            continue;
        }
        let age = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| now.duration_since(t).ok());
        if !age.is_some_and(|a| a > TAKEN_MAX_AGE) {
            continue;
        }
        let mine = store.dir().join(format!("{RECOVER_PREFIX}{:032x}", rand::random::<u128>()));
        if fs::rename(entry.path(), &mine).is_err() {
            continue; // another recoverer claimed it
        }
        let (force, read_err) = match read_flag(&mine) {
            Ok(f) => (f, None),
            Err(t) => (false, Some(t)),
        };
        let remove_err = remove_entry(&mine).err();
        let name = entry.file_name().to_string_lossy().into_owned();
        let mut text = match queue(store, force) {
            Ok(()) => format!("{name} was left by a dead taker; {PENDING_FILE} set again{}", if force { " with force" } else { "" }),
            Err(e) => format!("{name} was left by a dead taker; cannot set {PENDING_FILE}: {e:#}"),
        };
        for t in read_err.into_iter().chain(remove_err) {
            text.push_str("; ");
            text.push_str(&t);
        }
        log(store, None, &text);
    }
}

/// Takes `snapshot.pending` (W§7.3, D14): first queues again the taken files of dead takers ([`recover_taken`]); then
/// sets the time of `snapshot.pending` to now, renames it to a name that only this caller has, reads it
/// ([`read_flag`]) and removes it, all before the caller runs a snapshot (round 3 D1). `Some(true)` when it held
/// `force`; `Some(false)` when it was empty, or was not a regular file or could not be read (one log line); `None`
/// when there was no file to take, or when the state folder is a link or not a folder (nothing is taken, renamed or
/// logged; FIX-17 Round 2 F5).
fn take(store: &Store) -> Option<bool> {
    if store.check_dir().is_err() {
        return None;
    }
    recover_taken(store);
    let from = store.dir().join(PENDING_FILE);
    let to = taken_path(store);
    touch(&from);
    if let Err(e) = fs::rename(&from, &to) {
        if e.kind() != ErrorKind::NotFound {
            log(store, None, &format!("cannot take {PENDING_FILE}: {e}"));
        }
        return None;
    }
    #[cfg(feature = "test-hooks")]
    testing::run_after_take(store.dir());
    let flag = read_flag(&to);
    let removed = remove_entry(&to);
    let mut problems: Vec<String> = Vec::new();
    let force = match flag {
        Ok(f) => f,
        Err(t) => {
            problems.push(format!("{PENDING_FILE}: {t}"));
            false
        }
    };
    problems.extend(removed.err());
    if !problems.is_empty() {
        log(store, None, &problems.join("; "));
    }
    Some(force)
}

/// One pass of W§7.4 steps 1-5 (D2-D11). Each state read, the first one and every later one (a retry, the run after
/// the probe, a `snapshot.pending` check), must find the socket that `mux` is bound to: another `tmux_socket` gives
/// `Skipped(Changed)` before any tmux call (round 2 M4, round 3 D3). The check under the lock does the same
/// (round 3 D2).
fn attempt(store: &Store, mux: &dyn Multiplexer, event: Option<&str>, force: bool, wait: Duration) -> Attempt {
    // Step 1.
    let ws = match store.read() {
        Ok(Some(ws)) => ws,
        Ok(None) => return done(SkipReason::NoState),
        Err(e) => {
            log(store, event, &e.to_string());
            return done(SkipReason::Logged);
        }
    };
    if !gated(ws.phase, force) {
        return done(SkipReason::Phase);
    }
    let Some(state_socket) = ws.tmux_socket.as_deref() else {
        return done(SkipReason::NoSocket);
    };
    if mux.socket() != Some(state_socket) {
        return done(SkipReason::Changed);
    }
    let session = ws.tmux_session.clone();
    let seq = ws.snapshot_seq;
    // Step 2, done again before each read of step 3 (round 2 M3): the owner check, then one checked read.
    let read = || -> Result<Rows, Attempt> {
        match owner(mux, &session, &ws.id) {
            Ok(true) => {}
            Ok(false) => {
                log(store, event, &format!("tmux session {session} does not belong to workspace {} ({WS_OPTION})", ws.id));
                return Err(done(SkipReason::Logged));
            }
            Err(None) => return Err(done(SkipReason::Gone)),
            Err(Some(t)) => {
                log(store, event, &t);
                return Err(done(SkipReason::Logged));
            }
        }
        match read_panes(mux, &session) {
            Ok(Read::Rows(r)) => Ok(r),
            Ok(Read::Gone) => Err(done(SkipReason::Gone)),
            Err(t) => {
                log(store, event, &t);
                Err(done(SkipReason::Logged))
            }
        }
    };
    let mut rows = match read() {
        Ok(r) => r,
        Err(a) => return a,
    };
    // Step 3 (AU89).
    let shrunk = ws.panes.iter().any(|p| valid_pane_key(&p.key) && !rows.iter().any(|(_, r)| r.key == p.key));
    if shrunk {
        let mut stable = false;
        for _ in 1..SHRINK_READS {
            std::thread::sleep(SHRINK_WAIT);
            let next = match read() {
                Ok(r) => r,
                Err(a) => return a,
            };
            let same = pane_ids(&next) == pane_ids(&rows);
            rows = next;
            if same {
                stable = true;
                break;
            }
        }
        if !stable {
            return Attempt::Unstable;
        }
    }
    // Steps 4 and 5.
    let mut locked = Locked::Write;
    let result = store.update(wait, &mut |cur| {
        locked = under_lock(cur, mux, &rows, &session, seq, force);
        match locked {
            Locked::Write => Ok(()),
            _ => Err(anyhow!("snapshot not written")),
        }
    });
    match result {
        Ok(_) => Attempt::Done(SnapshotOutcome::Written),
        Err(StateError::Busy) => Attempt::Busy,
        Err(StateError::Refused(e)) => match locked {
            Locked::Skip(reason) => done(reason),
            Locked::Conflict => Attempt::Conflict,
            Locked::Gone => done(SkipReason::Gone),
            Locked::Pane(text) => match mux.session_exists(&session) {
                SessionAnswer::Gone => done(SkipReason::Gone),
                SessionAnswer::Exists => {
                    log(store, event, &text);
                    done(SkipReason::Logged)
                }
                SessionAnswer::Error { message } => {
                    log(store, event, &format!("{text}; has-session failed: {message}"));
                    done(SkipReason::Logged)
                }
            },
            Locked::Failed(t) => {
                log(store, event, &t);
                done(SkipReason::Logged)
            }
            Locked::Write => {
                log(store, event, &format!("{e:#}"));
                done(SkipReason::Logged)
            }
        },
        Err(e) => {
            log(store, event, &format!("workspace state not written: {e}"));
            done(SkipReason::Logged)
        }
    }
}

/// After a write: the `snapshot.pending` check of W§7.3. Its outcome does not change the write's outcome; a queue
/// that fails in it wrote its own log line.
fn written(store: &Store, mux: &dyn Multiplexer, wait: Duration) -> SnapshotOutcome {
    let _ = pending_check(store, mux, wait);
    SnapshotOutcome::Written
}

/// After `Busy` (D14): queue with the own flag, try `lock` once more for [`PROBE_WAIT`] without reading the state,
/// and on success take the file and run once more.
fn busy(store: &Store, mux: &dyn Multiplexer, event: Option<&str>, force: bool, wait: Duration) -> anyhow::Result<SnapshotOutcome> {
    queue_logged(store, event, force)?;
    #[cfg(feature = "test-hooks")]
    testing::run_after_queue(store.dir());
    match lock_file(&store.dir().join(LOCK_FILE), PROBE_WAIT) {
        Ok(Some(f)) => drop(f),
        Ok(None) => return Ok(SnapshotOutcome::Pending),
        Err(e) => {
            log(store, event, &format!("cannot open {LOCK_FILE}: {e}"));
            return Ok(SnapshotOutcome::Pending);
        }
    }
    let Some(taken) = take(store) else {
        return Ok(SnapshotOutcome::Pending);
    };
    let force = force || taken;
    match attempt(store, mux, event, force, wait) {
        Attempt::Done(SnapshotOutcome::Written) => Ok(written(store, mux, wait)),
        Attempt::Done(o) => Ok(o),
        Attempt::Busy | Attempt::Conflict | Attempt::Unstable => {
            queue_logged(store, event, force)?;
            Ok(SnapshotOutcome::Pending)
        }
    }
}

/// W§7.4 with the D8 retry: one more whole attempt after a `snapshot_seq` conflict; a second conflict or unstable
/// shrink reads queue the snapshot; `Busy` goes to [`busy`].
fn snapshot(store: &Store, mux: &dyn Multiplexer, event: Option<&str>, force: bool, wait: Duration) -> anyhow::Result<SnapshotOutcome> {
    let mut conflicts = 0;
    loop {
        match attempt(store, mux, event, force, wait) {
            Attempt::Done(SnapshotOutcome::Written) => return Ok(written(store, mux, wait)),
            Attempt::Done(o) => return Ok(o),
            Attempt::Conflict if conflicts == 0 => conflicts += 1,
            Attempt::Conflict | Attempt::Unstable => {
                queue_logged(store, event, force)?;
                return Ok(SnapshotOutcome::Pending);
            }
            Attempt::Busy => return busy(store, mux, event, force, wait),
        }
    }
}

/// The `snapshot.pending` check of W§7.3 (D15): no file gives `Skipped(NoPending)`; else one snapshot with the flag
/// that the file held.
fn pending_check(store: &Store, mux: &dyn Multiplexer, wait: Duration) -> anyhow::Result<SnapshotOutcome> {
    match take(store) {
        None => Ok(SnapshotOutcome::Skipped(SkipReason::NoPending)),
        Some(force) => snapshot(store, mux, None, force, wait),
    }
}

/// `zygo ws snapshot` (W§7.4 steps 1-5, then the W§7.3 `snapshot.pending` check). `mux` is built with the state's
/// `tmux_socket` (AU62). `wait` is the lock wait of W§7.3 ([`HOOK_WAIT`] or [`OP_WAIT`]). An invalid `ws_id` gives
/// `Ok(Skipped(InvalidId))` with no file and no tmux call (AU84). `Err` only when the snapshot had to be queued and
/// `snapshot.pending` could not be set (also logged).
pub fn run(
    paths: &Paths, mux: &dyn Multiplexer, ws_id: &str, event: Option<&str>, force: bool, wait: Duration,
) -> anyhow::Result<SnapshotOutcome> {
    if !valid_ws_id(ws_id) {
        return Ok(SnapshotOutcome::Skipped(SkipReason::InvalidId));
    }
    snapshot(&Store::open(paths, ws_id), mux, event, force, wait)
}

/// Takes `snapshot.pending` (W§7.3): `Some(true)` when it held `force`, `Some(false)` when it was empty or could not be
/// read (one log line), `None` when there was none or `ws_id` is not valid, and `None` with nothing taken when the
/// state folder is a link or not a folder: the signature has no error (FIX-17 Round 2 F5).
pub fn take_pending(paths: &Paths, ws_id: &str) -> Option<bool> {
    if !valid_ws_id(ws_id) {
        return None;
    }
    take(&Store::open(paths, ws_id))
}

/// Sets `snapshot.pending` (W§7.3): with `force`, `force` replaces any file in one rename; without, an empty file is
/// made only when none exists, so no order of writers removes a `force`. A state folder that is a link or not a folder
/// gives `Err` with the store's refusal and nothing written (FIX-17 Round 2 F5).
pub fn queue_pending(paths: &Paths, ws_id: &str, force: bool) -> anyhow::Result<()> {
    if !valid_ws_id(ws_id) {
        bail!("invalid workspace id {ws_id:?}");
    }
    queue(&Store::open(paths, ws_id), force)
}

/// The `snapshot.pending` check of W§7.3 that each writer runs after it releases `lock`: no file gives
/// `Skipped(NoPending)` with no tmux call; else one snapshot with `--force` when the file held `force`. A state folder
/// that is a link or not a folder gives `Err` with the store's refusal, with no file written and no tmux call
/// (FIX-17 Round 2 F5).
pub fn after_write(paths: &Paths, mux: &dyn Multiplexer, ws_id: &str, wait: Duration) -> anyhow::Result<SnapshotOutcome> {
    if !valid_ws_id(ws_id) {
        return Ok(SnapshotOutcome::Skipped(SkipReason::InvalidId));
    }
    let store = Store::open(paths, ws_id);
    store.check_dir().map_err(|e| anyhow!("{e}"))?;
    pending_check(&store, mux, wait)
}

/// The pane key of a hand-started member (W§7.4 step 4, AU41, AU79, D12): the pane id must be `%N`, the state must
/// exist in phase `up` or `draining`; then, under `lock` ([`OP_WAIT`]), a valid `@zygo-pane` is used, or a new key
/// is set. When the key was set now and the state write then fails, the rollback takes `lock` again, reads the state,
/// and sets `@zygo-pane` back to the empty value only when no `panes[]` entry holds the key (round 2 M2). After the
/// lock it runs the W§7.3 `snapshot.pending` check with [`HOOK_WAIT`]; that outcome does not change the result.
pub fn assign_key(store: &Store, mux: &dyn Multiplexer, pane: &str) -> anyhow::Result<String> {
    if !valid_pane_id(pane) {
        bail!("invalid pane id {pane:?}");
    }
    let Some(ws) = store.read()? else {
        bail!("no workspace state");
    };
    let allowed = |phase: Phase| matches!(phase, Phase::Up | Phase::Draining);
    if !allowed(ws.phase) {
        bail!("a member starts only in phase up or draining, not {}", format!("{:?}", ws.phase).to_lowercase());
    }
    let mut got: Option<(String, bool)> = None;
    let result = store.update(OP_WAIT, &mut |cur| {
        if !allowed(cur.phase) {
            bail!("a member starts only in phase up or draining, not {}", format!("{:?}", cur.phase).to_lowercase());
        }
        let mut taken: BTreeSet<String> = cur.panes.iter().map(|p| p.key.clone()).chain(cur.members.keys().cloned()).collect();
        match key_pane(mux, pane, &mut taken) {
            Ok(v) => {
                got = Some(v);
                Ok(())
            }
            Err(e) => Err(anyhow!(key_text(e, pane))),
        }
    });
    match result {
        Ok(_) => {
            let key = got.map(|(k, _)| k).ok_or_else(|| anyhow!("no key for pane {pane}"))?;
            let _ = pending_check(store, mux, HOOK_WAIT);
            Ok(key)
        }
        Err(e) => {
            if let Some((key, true)) = &got {
                rollback(store, mux, pane, key);
                // Round 3 D4: the rollback held `lock`, so it runs the check that every writer runs after it.
                let _ = pending_check(store, mux, HOOK_WAIT);
            }
            Err(anyhow!("cannot assign a key to pane {pane}: {e}"))
        }
    }
}

/// The rollback of [`assign_key`] (round 2 M2): under `lock` ([`OP_WAIT`]), reads the state again and clears
/// `@zygo-pane` of `pane` only when no `panes[]` entry holds `key`, so a key that a snapshot already wrote stays. A
/// state that cannot be read holds no key. When the lock cannot be taken the option stays: a set key that no entry
/// holds is used by the next snapshot. The lock is released when this returns.
///
/// Accepted limit (round 3 D4): a snapshot can read the pane's key from tmux before the rollback clears it, and write
/// that key to `panes[]` after the rollback releases `lock`. `panes[]` then holds a key that tmux no longer has. The
/// next snapshot finds that stored key missing from its read, treats it as a shrink (AU89) and gives the pane a new
/// key.
fn rollback(store: &Store, mux: &dyn Multiplexer, pane: &str, key: &str) {
    let Ok(Some(_lock)) = lock_file(&store.dir().join(LOCK_FILE), OP_WAIT) else {
        return;
    };
    let written = matches!(store.read(), Ok(Some(ws)) if ws.panes.iter().any(|p| p.key == key));
    if !written {
        let _ = mux.set_pane_option(pane, PANE_OPTION, "");
    }
}

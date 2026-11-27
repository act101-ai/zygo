//! The maildir of each member: mail/<pane key>/{tmp,new,cur}/<ULID>.json (M§5, M-C3, M-C4).
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{BODY_MAX, MAIL_LOCK_WAIT, MailError, Message, SUBJECT_MAX};
use crate::identity::valid_pane_key;

/// A file in `tmp/` older than this is a failed write; the sweep deletes it (M§10 "Write fails partway").
const TMP_MAX_AGE: Duration = Duration::from_secs(3600);

/// The largest message file read, 8 MiB (S§8.5: a 1 MiB body can grow to 6 MiB with JSON escapes).
pub const FILE_MAX: u64 = 8 * 1024 * 1024;

/// A message in `new/`, as `list` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub id: String,
    pub from: String,
    pub subject: String,
}

/// `^[0-9A-HJKMNP-TV-Z]{26}$` on the whole string (S§8.5 without `.json`).
fn valid_id(s: &str) -> bool {
    s.len() == 26 && s.bytes().all(|c| c.is_ascii_digit() || (c.is_ascii_uppercase() && !b"ILOU".contains(&c)))
}

/// The key check, then the join (AU84).
fn maildir(mail_dir: &Path, key: &str) -> Result<PathBuf, MailError> {
    if !valid_pane_key(key) {
        return Err(MailError::BadKey);
    }
    Ok(mail_dir.join(key))
}

fn file_name(id: &str) -> String {
    format!("{id}.json")
}

/// Makes the missing parts of `dir` and its parents, `0700` on unix, then sets `dir` itself to `0700` when it is
/// not (M-C4). A `dir` that is a symlink or not a folder is refused.
pub(crate) fn make_dir(dir: &Path) -> std::io::Result<()> {
    make_dir_with(dir, true)
}

/// [`make_dir`] that makes no parent: a missing parent is `NotFound` and nothing is made (mail sync design S§8.3
/// step 2, MS-21 review R2-1).
pub(crate) fn make_dir_in_place(dir: &Path) -> std::io::Result<()> {
    make_dir_with(dir, false)
}

fn make_dir_with(dir: &Path, recursive: bool) -> std::io::Result<()> {
    let mut b = DirBuilder::new();
    b.recursive(recursive);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    match b.create(dir) {
        Err(e) if e.kind() != ErrorKind::AlreadyExists => return Err(e),
        _ => {}
    }
    let meta = fs::symlink_metadata(dir)?;
    if !meta.is_dir() {
        return Err(std::io::Error::new(ErrorKind::InvalidInput, format!("{} is not a folder", dir.display())));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o7777 != 0o700 {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

/// `dir` checked with `symlink_metadata` (no link follow): `Ok(true)` for a real folder, `Ok(false)` when it is
/// missing; a link or an entry that is not a folder is `InvalidInput`.
pub(crate) fn real_dir(dir: &Path) -> std::io::Result<bool> {
    match fs::symlink_metadata(dir) {
        Ok(m) if m.file_type().is_symlink() => {
            Err(std::io::Error::new(ErrorKind::InvalidInput, format!("{} is a link; it is not followed", dir.display())))
        }
        Ok(m) if !m.is_dir() => {
            Err(std::io::Error::new(ErrorKind::InvalidInput, format!("{} is not a folder", dir.display())))
        }
        Ok(_) => Ok(true),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// `mail/`, `mail/<key>/` and its three folders, each `0700` (M-C4).
fn make_dirs(mail_dir: &Path, md: &Path) -> std::io::Result<()> {
    super::ensure_mail_root(mail_dir)?;
    make_dir(md)?;
    for s in ["tmp", "new", "cur"] {
        make_dir(&md.join(s))?;
    }
    Ok(())
}

/// Opens `path` for reading with no link follow and no wait on a FIFO; `None` unless it is a regular file of at
/// most `FILE_MAX` bytes.
fn open_regular(path: &Path) -> Option<File> {
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let f = o.open(path).ok()?;
    let meta = f.metadata().ok()?;
    (meta.is_file() && meta.len() <= FILE_MAX).then_some(f)
}

/// The message in `f`, read to at most `FILE_MAX` bytes; `None` when it cannot be read, is larger or does not parse.
fn parse(f: &File) -> Option<Message> {
    let mut bytes = Vec::new();
    f.take(FILE_MAX + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > FILE_MAX {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// The valid message ids of the regular files in `dir` (no symlink, folder or FIFO), oldest first (byte order is
/// ULID order); a missing `dir` has none.
fn ids(dir: &Path) -> std::io::Result<Vec<String>> {
    let entries = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut v = Vec::new();
    for e in entries {
        let e = e?;
        if !e.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let name = e.file_name();
        if let Some(id) = name.to_str().and_then(|n| n.strip_suffix(".json")).filter(|id| valid_id(id)) {
            v.push(id.to_string());
        }
    }
    v.sort();
    Ok(v)
}

fn sync_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

/// The M§5 file write: create-new, `0600`, the bytes unchanged, `fsync` of the file and its folder. It makes no
/// folder.
pub fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    sync_dir(path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")))
}

/// Moves `staged` (written with `write_synced` under `mail_dir`) into `mail/<key>/new/<id>.json` without reading
/// or rewriting its bytes (M§5; S§8.5 step 3). `Ok(false)`, and the staged file removed, when the id is already in
/// `new/` or `cur/`. A full `new/` returns the M§5 error and leaves the staged file in place. It takes no
/// `mail/lock`; production callers use [`super::Delivery::deliver_staged`] (M§5 P1).
pub fn deliver_staged(mail_dir: &Path, key: &str, id: &str, staged: &Path, max_unread: u32) -> Result<bool, MailError> {
    let md = maildir(mail_dir, key)?;
    if !valid_id(id) {
        return Err(MailError::BadId);
    }
    let (new, cur) = (md.join("new"), md.join("cur"));
    if ids(&new)?.len() >= max_unread as usize {
        return Err(MailError::Full { label: key.to_string(), max: max_unread });
    }
    if fs::symlink_metadata(cur.join(file_name(id))).is_ok() {
        fs::remove_file(staged)?;
        return Ok(false);
    }
    make_dirs(mail_dir, &md)?;
    // A link never replaces a file: an existing new/<id>.json stays as it is (lead's change; no check then rename).
    match fs::hard_link(staged, new.join(file_name(id))) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            fs::remove_file(staged)?;
            return Ok(false);
        }
        Err(e) => return Err(e.into()),
    }
    // The message is in new/ now: a failed fsync of the folder is not an error, or the sender would retry and the
    // message would arrive twice.
    let _ = sync_dir(&new);
    // A staged name left by a failed remove is a later duplicate or a swept tmp/ file.
    let _ = fs::remove_file(staged);
    Ok(true)
}

/// Sends `msg` to the maildir of `key`: write in `tmp/`, `fsync`, then into `new/` (M§5, M-C3). `label` names the
/// member in the full-maildir line. On a failed write the file stays in `tmp/` for the sweep (M§10).
pub fn deliver(mail_dir: &Path, key: &str, label: &str, msg: &Message, max_unread: u32) -> Result<(), MailError> {
    let md = maildir(mail_dir, key)?;
    if !valid_id(&msg.id) {
        return Err(MailError::BadId);
    }
    if msg.subject.len() > SUBJECT_MAX {
        return Err(MailError::SubjectTooLong);
    }
    if msg.body.len() > BODY_MAX {
        return Err(MailError::BodyTooLarge);
    }
    let full = || MailError::Full { label: label.to_string(), max: max_unread };
    if ids(&md.join("new"))?.len() >= max_unread as usize {
        return Err(full());
    }
    make_dirs(mail_dir, &md)?;
    let bytes = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
    let tmp = md.join("tmp").join(file_name(&msg.id));
    match write_synced(&tmp, &bytes) {
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return Err(MailError::Duplicate { id: msg.id.clone() }),
        r => r?,
    }
    match deliver_staged(mail_dir, key, &msg.id, &tmp, max_unread) {
        Ok(true) => Ok(()),
        Ok(false) => Err(MailError::Duplicate { id: msg.id.clone() }),
        Err(MailError::Full { .. }) => Err(full()),
        Err(e) => Err(e),
    }
}

/// Moves messages from `new/` to `cur/`, oldest first, and returns at most `max` that parse (M§5). The move is a
/// link that never replaces a file, then the removal of the `new/` name: a file gone before its link was taken by
/// another reader, and an id already in `cur/` is a duplicate, removed from `new/` and not returned. A moved file
/// gets the read time as its modification time, so the sweep ages it from its read. A moved file that cannot be
/// read or parsed stays in `cur/` for the sweep. An error before the first move is returned; after it, `read` stops
/// and returns what it has. `mail/`, `mail/<key>/`, `new/` and `cur/` are never followed as links: a link or an
/// entry that is not a folder at any of them is an `Io` error; a missing `mail/`, `mail/<key>/` or `new/` has no
/// messages and nothing is made; only `cur/` is made, when `new/` has a message, and an existing `cur/` is set to
/// `0700` then (M-C4). The list and the moves run under `mail/lock` held shared (M§5 P1), so no removal of
/// `mail/<key>/` runs between them; [`read_within`] with [`MAIL_LOCK_WAIT`].
pub fn read(mail_dir: &Path, key: &str, max: usize) -> Result<Vec<Message>, MailError> {
    read_within(mail_dir, key, max, MAIL_LOCK_WAIT)
}

/// [`read`], waiting at most `bound` for `mail/lock`: a lock held for the whole bound is `Busy`. A missing `mail/`
/// takes no lock and is not made.
pub fn read_within(mail_dir: &Path, key: &str, max: usize, bound: Duration) -> Result<Vec<Message>, MailError> {
    let md = maildir(mail_dir, key)?;
    let (new, cur) = (md.join("new"), md.join("cur"));
    let mut out = Vec::new();
    let mut moved = false;
    // No link is followed (M-C4): `mail/`, `mail/<key>/` and `new/` are checked without a follow; a missing one has
    // no messages and nothing is made.
    if !real_dir(mail_dir)? {
        return Ok(Vec::new());
    }
    let _lock = match crate::state::lock_file_as(&mail_dir.join("lock"), bound, true) {
        Ok(Some(f)) => f,
        Ok(None) => return Err(MailError::Busy),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    for dir in [md.as_path(), new.as_path()] {
        if !real_dir(dir)? {
            return Ok(Vec::new());
        }
    }
    let ids = ids(&new)?;
    if !ids.is_empty() {
        // M§5 P1: a read never makes mail/<key>/. Only cur/ is made, in a maildir that exists; a maildir removed since
        // the listing has nothing to return.
        let state_dir = mail_dir.parent().unwrap_or(mail_dir);
        crate::up::fire(state_dir, "mail_read_before_dirs").map_err(|e| std::io::Error::other(e.to_string()))?;
        match real_dir(&md) {
            Ok(false) => return Ok(Vec::new()),
            r => r.map(|_| ())?,
        }
        // A link at cur/ is refused; a missing cur/ is made, and an existing one is set to 0700 (M-C4).
        real_dir(&cur)?;
        match make_dir_in_place(&cur) {
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            r => r?,
        }
    }
    for id in ids {
        if out.len() >= max {
            break;
        }
        let (from, to) = (new.join(file_name(&id)), cur.join(file_name(&id)));
        match fs::hard_link(&from, &to) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&from);
                continue;
            }
            Err(_) if moved => break,
            Err(e) => return Err(e.into()),
        }
        moved = true;
        let _ = fs::remove_file(&from);
        let Some(f) = open_regular(&to) else { continue };
        let _ = f.set_modified(SystemTime::now());
        if let Some(m) = parse(&f) {
            out.push(m);
        }
    }
    Ok(out)
}

/// The messages in `new/`, oldest first; it moves nothing.
pub fn list(mail_dir: &Path, key: &str) -> Result<Vec<Summary>, MailError> {
    let md = maildir(mail_dir, key)?;
    let new = md.join("new");
    let mut out = Vec::new();
    for id in ids(&new)? {
        let Some(f) = open_regular(&new.join(file_name(&id))) else { continue };
        if let Some(m) = parse(&f) {
            out.push(Summary { id, from: m.from, subject: m.subject });
        }
    }
    Ok(out)
}

/// The number of message files in `new/`: regular files only.
pub fn unread(mail_dir: &Path, key: &str) -> Result<usize, MailError> {
    Ok(ids(&maildir(mail_dir, key)?.join("new"))?.len())
}

/// Deletes `cur/` files older than `keep` and `tmp/` files older than 1 h in each maildir; `new/` and folders that
/// are not pane keys (such as `sync/`) are never touched. A `cur/` or `tmp/` that is not a real folder is skipped,
/// and a symlink in one is deleted itself, never its target. Every error is ignored.
pub fn sweep(mail_dir: &Path, keep: Duration, now: SystemTime) {
    let Ok(entries) = fs::read_dir(mail_dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(key) = name.to_str().filter(|k| valid_pane_key(k)) else { continue };
        let md = mail_dir.join(key);
        if !fs::symlink_metadata(&md).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        for (s, age) in [("cur", keep), ("tmp", TMP_MAX_AGE)] {
            let dir = md.join(s);
            if !fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            let Ok(files) = fs::read_dir(&dir) else { continue };
            for f in files.flatten() {
                let p = f.path();
                let Ok(meta) = fs::symlink_metadata(&p) else { continue };
                let Ok(m) = meta.modified() else { continue };
                if (meta.is_file() || meta.is_symlink()) && now.duration_since(m).is_ok_and(|d| d > age) {
                    let _ = fs::remove_file(&p);
                }
            }
        }
    }
}

/// Deletes the maildir of `key`; a missing one is no error. It takes no `mail/lock` and reads no member record, so it
/// has no production caller: production removals use [`super::remove_maildir`] and [`super::prune`] (M§5 P1). It
/// stays `pub` and hidden because the integration tests (`tests/mail_store.rs`, `tests/mail_lifecycle.rs`) call it.
#[doc(hidden)]
pub fn remove_pane(mail_dir: &Path, key: &str) -> std::io::Result<()> {
    if !valid_pane_key(key) {
        return Err(std::io::Error::new(ErrorKind::InvalidInput, "invalid pane key"));
    }
    match fs::remove_dir_all(mail_dir.join(key)) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

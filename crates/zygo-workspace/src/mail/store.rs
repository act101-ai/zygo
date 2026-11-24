//! The maildir of each member: mail/<pane key>/{tmp,new,cur}/<ULID>.json (M§5, M-C3, M-C4).
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{BODY_MAX, MailError, Message, SUBJECT_MAX};
use crate::identity::valid_pane_key;

/// A file in `tmp/` older than this is a failed write; the sweep deletes it (M§10 "Write fails partway").
const TMP_MAX_AGE: Duration = Duration::from_secs(3600);

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

/// Makes the missing parts of `dir` and its parents, `0700` on unix (M-C4).
fn make_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(dir)
}

fn make_dirs(md: &Path) -> std::io::Result<()> {
    for s in ["tmp", "new", "cur"] {
        make_dir(&md.join(s))?;
    }
    Ok(())
}

/// The valid message ids in `dir`, oldest first (byte order is ULID order); a missing `dir` has none.
fn ids(dir: &Path) -> std::io::Result<Vec<String>> {
    let entries = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut v = Vec::new();
    for e in entries {
        let name = e?.file_name();
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
/// `new/` or `cur/`. A full `new/` returns the M§5 error and leaves the staged file in place.
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
    make_dirs(&md)?;
    // A link never replaces a file: an existing new/<id>.json stays as it is (lead's change; no check then rename).
    match fs::hard_link(staged, new.join(file_name(id))) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            fs::remove_file(staged)?;
            return Ok(false);
        }
        Err(e) => return Err(e.into()),
    }
    sync_dir(&new)?;
    // The message is in new/ now; a staged name left by a failed remove is a later duplicate or a swept tmp/ file.
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
    make_dirs(&md)?;
    let bytes = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
    let tmp = md.join("tmp").join(file_name(&msg.id));
    write_synced(&tmp, &bytes)?;
    match deliver_staged(mail_dir, key, &msg.id, &tmp, max_unread) {
        Ok(true) => Ok(()),
        Ok(false) => Err(MailError::Duplicate { id: msg.id.clone() }),
        Err(MailError::Full { .. }) => Err(full()),
        Err(e) => Err(e),
    }
}

/// Moves at most `max` messages from `new/` to `cur/`, oldest first, and returns those that parse (M§5). A file
/// gone before its rename was taken by another reader.
pub fn read(mail_dir: &Path, key: &str, max: usize) -> Result<Vec<Message>, MailError> {
    let md = maildir(mail_dir, key)?;
    let (new, cur) = (md.join("new"), md.join("cur"));
    let mut out = Vec::new();
    let mut moved = 0;
    for id in ids(&new)? {
        if moved >= max {
            break;
        }
        make_dir(&cur)?;
        let to = cur.join(file_name(&id));
        match fs::rename(new.join(file_name(&id)), &to) {
            Ok(()) => moved += 1,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        }
        if let Ok(m) = serde_json::from_slice::<Message>(&fs::read(&to)?) {
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
        let bytes = match fs::read(new.join(file_name(&id))) {
            Ok(b) => b,
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        if let Ok(m) = serde_json::from_slice::<Message>(&bytes) {
            out.push(Summary { id, from: m.from, subject: m.subject });
        }
    }
    Ok(out)
}

/// The number of message files in `new/`.
pub fn unread(mail_dir: &Path, key: &str) -> Result<usize, MailError> {
    Ok(ids(&maildir(mail_dir, key)?.join("new"))?.len())
}

/// Deletes `cur/` files older than `keep` and `tmp/` files older than 1 h in each maildir; `new/` and folders that
/// are not pane keys (such as `sync/`) are never touched. Every error is ignored.
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
            let Ok(files) = fs::read_dir(md.join(s)) else { continue };
            for f in files.flatten() {
                let p = f.path();
                let Ok(meta) = fs::symlink_metadata(&p) else { continue };
                let Ok(m) = meta.modified() else { continue };
                if meta.is_file() && now.duration_since(m).is_ok_and(|d| d > age) {
                    let _ = fs::remove_file(&p);
                }
            }
        }
    }
}

/// Deletes the maildir of `key` (`forget-pane`, M§5); a missing one is no error.
pub fn remove_pane(mail_dir: &Path, key: &str) -> std::io::Result<()> {
    if !valid_pane_key(key) {
        return Err(std::io::Error::new(ErrorKind::InvalidInput, "invalid pane key"));
    }
    match fs::remove_dir_all(mail_dir.join(key)) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

//! Workspace mail: the message file and the maildir store (M§5). Owner: nodes ML-2, ML-4, ML-6.
pub mod names;
pub mod store;
pub mod wake;

use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use zygo_core::paths::Paths;

use crate::identity::valid_pane_key;
use crate::state::{Store, Workspace, lock_file_as, lock_is_at};

/// The largest subject in bytes (M§5).
pub const SUBJECT_MAX: usize = 200;
/// The largest body in bytes, 1 MiB (M§5).
pub const BODY_MAX: usize = 1048576;
/// The longest wait for `mail/lock`, by a delivery or by a removal (M§5 P1).
pub const MAIL_LOCK_WAIT: Duration = Duration::from_secs(2);

/// The message file, version 1 (M§5). Field order is the file's key order; unknown fields are kept (AU78).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub version: u32,
    pub id: String,
    pub from: String,
    pub to: String,
    pub sent_at: String,
    pub subject: String,
    pub body: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Message {
    /// A new version 1 message with a ULID made from `now` and `sent_at` in whole UTC seconds. It checks the
    /// subject and body limits; it checks no name (names are ML-4's).
    pub fn new(from: &str, to: &str, subject: &str, body: &str, now: SystemTime) -> Result<Message, MailError> {
        check_limits(subject, body)?;
        let sent = OffsetDateTime::from(now).replace_nanosecond(0).map_err(std::io::Error::other)?;
        Ok(Message {
            version: 1,
            id: ulid::Ulid::from_datetime(now).to_string(),
            from: from.to_string(),
            to: to.to_string(),
            sent_at: sent.format(&Rfc3339).map_err(std::io::Error::other)?,
            subject: subject.to_string(),
            body: body.to_string(),
            extra: serde_json::Map::new(),
        })
    }
}

/// The subject and body limits (M§5).
fn check_limits(subject: &str, body: &str) -> Result<(), MailError> {
    if subject.len() > SUBJECT_MAX {
        return Err(MailError::SubjectTooLong);
    }
    if body.len() > BODY_MAX {
        return Err(MailError::BodyTooLarge);
    }
    Ok(())
}

/// A mail error; `Display` is the whole user line.
#[derive(Debug)]
pub enum MailError {
    SubjectTooLong,
    BodyTooLarge,
    BadKey,
    BadId,
    Full { label: String, max: u32 },
    Duplicate { id: String },
    UnknownTarget { known: Vec<String> },
    /// `mail/lock` stayed held for the whole bound (M§5 P1).
    Busy,
    /// The workspace has no state folder or no `workspace.json` (M§5 P1).
    NoWorkspace,
    /// The key has no member record in the read under `mail/lock` (M§5 P1).
    NoRecord { key: String },
    /// [`Delivery::begin`]: the state folder is a link or not a folder, or the state does not read as valid.
    BadState(std::io::Error),
    /// [`Delivery::begin`]: `mail/` cannot be made or checked: a link, an entry that is not a folder, or another error
    /// of its `mkdir` or check (FX-24 review R2 F2).
    BadMail(std::io::Error),
    Io(std::io::Error),
}

impl fmt::Display for MailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MailError::SubjectTooLong => write!(f, "zygo: subject is longer than {SUBJECT_MAX} bytes"),
            MailError::BodyTooLarge => write!(f, "zygo: body is larger than 1 MiB"),
            MailError::BadKey => write!(f, "zygo: invalid pane key"),
            MailError::BadId => write!(f, "zygo: invalid message id"),
            MailError::Full { label, max } => write!(f, "zygo: {label} has {max} unread messages"),
            MailError::Duplicate { id } => write!(f, "zygo: message {id} was already delivered"),
            MailError::UnknownTarget { known } if known.is_empty() => {
                write!(f, "zygo: unknown target; known names: none")
            }
            MailError::UnknownTarget { known } => {
                write!(f, "zygo: unknown target; known names: {}", known.join(", "))
            }
            MailError::Busy => write!(f, "zygo: mail is busy; try again"),
            MailError::NoWorkspace => write!(f, "zygo: the workspace has no state; nothing sent"),
            MailError::NoRecord { key } => write!(f, "zygo: pane {key} has no member record; nothing delivered"),
            MailError::BadState(e) | MailError::BadMail(e) | MailError::Io(e) => write!(f, "zygo: mail: {e}"),
        }
    }
}

impl std::error::Error for MailError {}

impl From<std::io::Error> for MailError {
    fn from(e: std::io::Error) -> MailError {
        MailError::Io(e)
    }
}

/// The mail folder of workspace `ws_id`: `<workspaces_dir>/<id>/mail` (M-C2). It creates and checks nothing.
pub fn mail_dir(paths: &Paths, ws_id: &str) -> PathBuf {
    crate::state::Store::open(paths, ws_id).dir().join("mail")
}

/// Makes `mail_dir` (and its missing parents) and sets it to `0700` (M-C4).
pub fn ensure_mail_root(mail_dir: &std::path::Path) -> std::io::Result<()> {
    store::make_dir(mail_dir)
}

/// [`ensure_mail_root`] that makes no parent: `dir`'s parent must exist, else `NotFound` and nothing is made. The
/// mail sync makes `mail/` and its folders with it, each in the one before it, so a sync that `zygo ws forget` ran
/// under never makes the workspace state folder `<workspaces_dir>/<id>/` again (mail sync design S§8.3 step 2,
/// MS-21 review R2-1).
pub fn ensure_mail_dir(dir: &std::path::Path) -> std::io::Result<()> {
    store::make_dir_in_place(dir)
}

/// A delivery under `mail/lock` held shared (M§5 P1): the member records are read under the lock, and the lock is held
/// until the value is dropped, so no removal of `mail/<key>/` runs between that read and the last link into `new/`.
/// Deliveries do not wait for each other.
pub struct Delivery {
    mail_dir: PathBuf,
    ws: Workspace,
    _lock: fs::File,
}

impl fmt::Debug for Delivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Delivery").field("mail_dir", &self.mail_dir).field("ws", &self.ws.id).finish_non_exhaustive()
    }
}

impl Delivery {
    /// Takes `mail/lock` of `store`'s workspace shared, waiting at most `bound`, checks that the locked file is still
    /// the file at `mail/lock` ([`lock_is_at`]), then reads the state under it. It makes `mail/` (`0700`) when the
    /// state folder exists, never the state folder itself. A missing state folder, `mail/` that cannot be made because
    /// the folder went away, a locked file that is no longer at `mail/lock` (a forget renamed the state folder while
    /// this waited; FX-24 review R2 F3), or no `workspace.json` is `NoWorkspace`; a lock held
    /// for the whole bound is `Busy`; a state folder that is a link or not a folder, and a state that does not read as
    /// valid, is `BadState`; every other error of the make or check of `mail/` (a link or an entry that is not a
    /// folder at `mail/`) is `BadMail` (FX-24 review R2 F2); every other error is `Io`.
    pub fn begin(store: &Store, bound: Duration) -> Result<Delivery, MailError> {
        match fs::symlink_metadata(store.dir()) {
            Ok(m) if m.file_type().is_symlink() => {
                let text = format!("{} is a link; it is not followed", store.dir().display());
                return Err(MailError::BadState(std::io::Error::new(ErrorKind::InvalidInput, text)));
            }
            Ok(m) if !m.is_dir() => {
                let text = format!("{} is not a folder", store.dir().display());
                return Err(MailError::BadState(std::io::Error::new(ErrorKind::InvalidInput, text)));
            }
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(MailError::NoWorkspace),
            Err(e) => return Err(e.into()),
        }
        let mail = store.dir().join("mail");
        match store::make_dir_in_place(&mail) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(MailError::NoWorkspace),
            Err(e) => return Err(MailError::BadMail(e)),
        }
        let lock = match lock_file_as(&mail.join("lock"), bound, true) {
            Ok(Some(f)) => f,
            Ok(None) => return Err(MailError::Busy),
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(MailError::NoWorkspace),
            Err(e) => return Err(e.into()),
        };
        crate::up::fire(store.dir(), "mail_delivery_after_lock").map_err(|e| std::io::Error::other(e.to_string()))?;
        // FX-24 review R2 F3: a forget renamed the state folder (with the file this delivery locked) while it waited;
        // the state at the path, if any, is another workspace's, and its `mail/lock` is not held.
        if !lock_is_at(&lock, &mail.join("lock")) {
            return Err(MailError::NoWorkspace);
        }
        let ws = match store.read() {
            Ok(Some(ws)) => ws,
            Ok(None) => return Err(MailError::NoWorkspace),
            Err(crate::state::StateError::Io(e)) => return Err(e.into()),
            Err(e) => return Err(MailError::BadState(std::io::Error::new(ErrorKind::InvalidData, e.to_string()))),
        };
        crate::up::fire(store.dir(), "mail_delivery_after_read").map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(Delivery { mail_dir: mail, ws, _lock: lock })
    }

    /// The state as read under the lock.
    pub fn workspace(&self) -> &Workspace {
        &self.ws
    }

    /// Releases the lock and returns the state read under it, for the work after the last link that needs the member
    /// records of the delivery (the wake, M§7.3).
    pub fn into_workspace(self) -> Workspace {
        self.ws
    }

    /// [`store::deliver`] to `key` when `key` has a member record in the read under the lock; else `NoRecord`, and
    /// nothing is made.
    pub fn deliver(&self, key: &str, label: &str, msg: &Message, max_unread: u32) -> Result<(), MailError> {
        if !self.ws.members.contains_key(key) {
            return Err(MailError::NoRecord { key: key.to_string() });
        }
        store::deliver(&self.mail_dir, key, label, msg, max_unread)
    }

    /// The M§5 P1 delivery of a staged file, as [`Delivery::deliver`]: [`store::deliver_staged`] to `key` when `key`
    /// has a member record in the read under the lock; else `NoRecord`, nothing is made, and the staged file stays for
    /// the caller.
    pub fn deliver_staged(&self, key: &str, id: &str, staged: &Path, max_unread: u32) -> Result<bool, MailError> {
        if !self.ws.members.contains_key(key) {
            return Err(MailError::NoRecord { key: key.to_string() });
        }
        store::deliver_staged(&self.mail_dir, key, id, staged, max_unread)
    }

    /// Makes `mail/<key>/` and its three folders (`0700`, M-C4) when `key` has a member record in the read under the
    /// lock; else `NoRecord`, and nothing is made. The mail sync makes it before it stages a file for `key` (mail sync
    /// design S§8.5 step 1), so a staged file is kept while its maildir exists (S§8.5 step 5).
    pub fn make_maildir(&self, key: &str) -> Result<(), MailError> {
        if !self.ws.members.contains_key(key) {
            return Err(MailError::NoRecord { key: key.to_string() });
        }
        store::make_maildir(&self.mail_dir, key)
    }
}

/// `mail/` of `store` checked with `symlink_metadata`; `None` when it is missing. A link or an entry that is not a
/// folder is `InvalidInput` and is not followed.
fn checked_mail(store: &Store) -> std::io::Result<Option<PathBuf>> {
    let mail = store.dir().join("mail");
    Ok(store::real_dir(&mail)?.then_some(mail))
}

/// Takes `mail/lock` exclusive (at most `bound`) and reads the state under it; `Ok(None)` when the state is gone, or
/// when the locked file is no longer the file at `mail/lock` (a forget renamed the state folder while this waited;
/// FX-24 review R2 F3). A lock held for the whole bound is `WouldBlock`.
fn removal_lock(store: &Store, mail: &Path, bound: Duration) -> std::io::Result<Option<(fs::File, Workspace)>> {
    let Some(lock) = lock_file_as(&mail.join("lock"), bound, false)? else {
        let text = format!("{}/lock is held by another process", mail.display());
        return Err(std::io::Error::new(ErrorKind::WouldBlock, text));
    };
    crate::up::fire(store.dir(), "mail_removal_after_lock").map_err(|e| std::io::Error::other(e.to_string()))?;
    if !lock_is_at(&lock, &mail.join("lock")) {
        return Ok(None);
    }
    match store.read() {
        Ok(Some(ws)) => Ok(Some((lock, ws))),
        Ok(None) => Ok(None),
        Err(e) => Err(std::io::Error::other(e.to_string())),
    }
}

/// Removes `path` itself: a folder with its contents, a link as a link, any other entry as a file. `Ok(false)` when
/// nothing was there. An error names the path.
fn remove_entry(path: &Path) -> std::io::Result<bool> {
    let named = |e: std::io::Error| std::io::Error::new(e.kind(), format!("{}: {e}", path.display()));
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(named(e)),
    };
    let r = if meta.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
    match r {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(named(e)),
    }
}

/// Removes `mail/<key>/` of `store`'s workspace when `key` has no member record (M§5 P1): it takes `mail/lock`
/// exclusive (at most `bound`) and reads the state under it. `Ok(true)` when something was removed; `Ok(false)` when
/// `mail/` or `mail/<key>` is missing, the key has a record, or the state is gone. A missing `mail/` takes no lock and
/// is not made. A key that fails `^k[a-z2-7]{8}$` and a `mail/` that is a link or not a folder are `InvalidInput`; a
/// lock held for the whole bound is `WouldBlock`, and the maildir stays for the next removal.
pub fn remove_maildir(store: &Store, key: &str, bound: Duration) -> std::io::Result<bool> {
    if !valid_pane_key(key) {
        return Err(std::io::Error::new(ErrorKind::InvalidInput, "invalid pane key"));
    }
    let Some(mail) = checked_mail(store)? else { return Ok(false) };
    let Some((_lock, ws)) = removal_lock(store, &mail, bound)? else { return Ok(false) };
    if ws.members.contains_key(key) {
        return Ok(false);
    }
    remove_entry(&mail.join(key))
}

/// Removes every `mail/<key>` of `store`'s workspace whose name is a pane key with no member record (M§5 P1), under
/// `mail/lock` exclusive (at most `bound`) and the state read under it, in name order. Every entry is tried; it
/// returns the removed keys, or the first error after trying all. Other names (`lock`, `sync/`, `outbox/`) are never
/// touched. A missing `mail/` takes no lock and is not made; errors as [`remove_maildir`].
pub fn prune(store: &Store, bound: Duration) -> std::io::Result<Vec<String>> {
    let Some(mail) = checked_mail(store)? else { return Ok(Vec::new()) };
    let Some((_lock, ws)) = removal_lock(store, &mail, bound)? else { return Ok(Vec::new()) };
    let mut keys: Vec<String> = fs::read_dir(&mail)?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|k| valid_pane_key(k) && !ws.members.contains_key(k))
        .collect();
    keys.sort();
    let (mut removed, mut first) = (Vec::new(), None);
    for k in keys {
        match remove_entry(&mail.join(&k)) {
            Ok(true) => removed.push(k),
            Ok(false) => {}
            Err(e) => {
                first.get_or_insert(e);
            }
        }
    }
    match first {
        Some(e) => Err(e),
        None => Ok(removed),
    }
}

/// The one line `up` and `repair` print when [`prune`] fails; the next removal tries again (M§5 P1).
pub(crate) fn not_pruned_line(e: &std::io::Error) -> String {
    format!("zygo: maildirs without a member record not removed: {e}")
}

/// One delivered message: its id, the pane key it went to, and its label: the target's name in the read under
/// `mail/lock` or, with none, its pane key (the message's `to`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub id: String,
    pub key: String,
    pub label: String,
}

/// One target `send` did not deliver to: its pane key and the error.
#[derive(Debug)]
pub struct Refused {
    pub key: String,
    pub error: MailError,
}

/// What one `send` did: the messages delivered and the targets refused, each in target (key) order, and the state
/// it read under `mail/lock`, on which it resolved the targets. A caller labels and wakes from this read, not from its
/// own older one.
#[derive(Debug)]
pub struct SendReport {
    pub sent: Vec<Sent>,
    pub refused: Vec<Refused>,
    pub workspace: Workspace,
}

/// Sends one message to each target of `to` (M§5, M§8). It checks the subject and body limits, then takes `mail/lock`
/// shared and reads the member records under it ([`Delivery`], M§5 P1): `ws` gives only the workspace id, and `to`
/// and the sender are resolved on that read, so a record removed after the caller's read gets no maildir; the report
/// carries that read and each label for the caller's lines and wake. It
/// `resolve`s `to` before it writes a file; those errors, and those of [`Delivery::begin`], are its only `Err`. Then it writes one file per target, with `from` from
/// `names::sender` and `to` the target's name or, with none, its pane key. A target whose message it cannot make or
/// deliver (for example `Full`, M§5 limit per target) goes into `refused`, and the send continues with the next
/// target. It writes no log line, so the subject and body reach only `mail/` (M-C2).
#[allow(clippy::too_many_arguments)]
pub fn send(
    paths: &Paths,
    ws: &Workspace,
    sender_key: Option<&str>,
    to: &str,
    subject: &str,
    body: &str,
    cfg: &zygo_config::MailConfig,
    now: SystemTime,
) -> Result<SendReport, MailError> {
    check_limits(subject, body)?;
    let d = Delivery::begin(&Store::open(paths, &ws.id), MAIL_LOCK_WAIT)?;
    let targets = names::resolve(d.workspace(), to, sender_key)?;
    let from = names::sender(d.workspace(), sender_key);
    let mut sent = Vec::with_capacity(targets.len());
    let mut refused = Vec::new();
    for t in targets {
        let label = t.name.unwrap_or_else(|| t.key.clone());
        let delivered = Message::new(&from, &label, subject, body, now)
            .and_then(|msg| d.deliver(&t.key, &label, &msg, cfg.max_unread).map(|()| msg.id));
        match delivered {
            Ok(id) => sent.push(Sent { id, key: t.key, label }),
            Err(error) => refused.push(Refused { key: t.key, error }),
        }
    }
    Ok(SendReport { sent, refused, workspace: d.ws })
}

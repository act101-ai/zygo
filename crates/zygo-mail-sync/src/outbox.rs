//! The outbox of remote mail (S§5 Outbox, S§7.2, S§8.6, S§9 last paragraph, S§15): the send-time rules and secret pass, the move of a refused message and its notice, the counts and the local sweep.
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ulid::Ulid;
use zygo_config::schema::{RedactionConfig, SyncOff, SyncSettings};
use zygo_core::ids::valid_member_name;
use zygo_redact::secret::DeadlinePolicy;
use zygo_workspace::mail::store::{write_synced, FILE_MAX};
use zygo_workspace::mail::{ensure_mail_dir, Delivery, MailError, Message, MAIL_LOCK_WAIT};
use zygo_workspace::up::fire_point;

use crate::address::{ulid_file, Address};
use crate::registry::read_cache;
use crate::secret::{self, Outcome};
use crate::state::{self, SyncPaths};

/// The age after which a partial write in `outbox/tmp/` is deleted (S§9: "older than 1 h (as M§10)").
const TMP_MAX_AGE: Duration = Duration::from_secs(3600);

/// Why a remote send is refused (S§7.2, S§8.6). `Io` holds a whole user line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    NoSenderName,
    SyncOff,
    Public,
    UnknownAddress(Vec<String>),
    Secret(Vec<String>),
    CheckFailed(&'static str),
    Io(String),
}

impl SendError {
    /// The whole user line of the refusal.
    pub fn line(&self) -> String {
        match self {
            SendError::NoSenderName => {
                "zygo: remote mail needs a sender name; give this member a name in the loadout or with --name".into()
            }
            SendError::SyncOff => "zygo: mail sync is off for this workspace".into(),
            SendError::Public => "zygo: the remote can be read without a login; mail sync refused".into(),
            SendError::UnknownAddress(k) if k.is_empty() => "zygo: unknown address; known addresses: none".into(),
            SendError::UnknownAddress(k) => format!("zygo: unknown address; known addresses: {}", k.join(", ")),
            SendError::Secret(ids) => format!("zygo: message not sent: it holds a secret ({})", ids.join(", ")),
            SendError::CheckFailed(c) => {
                format!("zygo: message not sent: the secret check could not run ({c}); send it again")
            }
            SendError::Io(l) => l.clone(),
        }
    }
}

/// An I/O error as its M§10 user line.
fn io_err(e: io::Error) -> SendError {
    SendError::Io(MailError::Io(e).to_string())
}

/// Puts one message to a remote address in `mail/outbox/`, with the default deadline policy of the secret pass
/// (S§8.6 "At send").
#[allow(clippy::too_many_arguments)]
pub fn send_remote(
    sp: &SyncPaths,
    settings: Result<&SyncSettings, &SyncOff>,
    from: Option<&str>,
    to: &Address,
    subject: &str,
    body: &str,
    redaction: &RedactionConfig,
    main_root: &Path,
    now: SystemTime,
) -> Result<Ulid, SendError> {
    send_remote_with(sp, settings, from, to, subject, body, redaction, main_root, now, &DeadlinePolicy::default())
}

/// `send_remote` with the deadline policy of the secret pass given. The rules run in the S§7.2 order (sender
/// name, sync off, public, unknown address), then the M§5 limits, then the secret pass; a refusal writes nothing.
/// The file is written in `outbox/tmp/` and linked into `outbox/` (M§5, M-C3).
///
/// `mail/lock` is held shared ([`Delivery::begin`]) from before the first outbox folder is made until the link (S§7.6
/// step 4, FX-28). Each refusal of `Delivery::begin` is `SendError::Io` with the `MailError` line (S§8.1 row 1):
/// - `Busy`, the lock not taken in `MAIL_LOCK_WAIT`: `zygo: mail is busy; try again`;
/// - `NoWorkspace`, no `workspace.json`, no state folder, or a locked file that is no longer `mail/lock` (a forget
///   renamed the state folder while this send waited): `zygo: the workspace has no state; nothing sent`;
/// - `BadState`, a corrupt `workspace.json` (`zygo: mail: <state>/workspace.json: <reason>`), a newer one (`zygo: mail:
///   workspace state is from a newer zygo; run zygo update`), or a state folder that is a link (`zygo: mail: <state>
///   is a link; it is not followed`);
/// - `BadMail`, a `mail/` that is a link or not a folder: `zygo: mail: <state>/mail is not a folder`;
/// - `Io`, a `mail/lock` that is not a regular file: `zygo: mail: <state>/mail/lock: is a link; it is not followed`,
///   `zygo: mail: <state>/mail/lock: is not a regular file`, or, for a folder, the text of the open error (`zygo:
///   mail: Is a directory (os error 21)` on Linux).
///
/// A refusal makes no state folder, no `mail/outbox/` folder and no outbox file. When the state folder is a real
/// folder and `mail/` is not a link, `Delivery::begin` makes `mail/` (`0700`) and `mail/lock` (`0600`) when they are
/// missing, before it reads the state, and they stay after a `NoWorkspace` or `BadState` refusal.
/// The hook point `remote_send_before_link` (test hooks only) runs under the lock, right before the link.
#[allow(clippy::too_many_arguments)]
pub fn send_remote_with(
    sp: &SyncPaths,
    settings: Result<&SyncSettings, &SyncOff>,
    from: Option<&str>,
    to: &Address,
    subject: &str,
    body: &str,
    redaction: &RedactionConfig,
    main_root: &Path,
    now: SystemTime,
    policy: &DeadlinePolicy,
) -> Result<Ulid, SendError> {
    let name = from.filter(|n| valid_member_name(n)).ok_or(SendError::NoSenderName)?;
    let s = settings.map_err(|_| SendError::SyncOff)?;
    // A state file that cannot be read leaves the public rule unknown: the send is refused (fail closed).
    let st = state::load(sp).map_err(io_err)?;
    if st.public.as_ref().is_some_and(|p| p.verdict == "public") {
        return Err(SendError::Public);
    }
    let cache = read_cache(sp);
    if !cache.knows(to) {
        return Err(SendError::UnknownAddress(cache.addresses()));
    }
    let msg = Message::new(&format!("{name}@{}", s.host), &format!("{}@{}", to.name, to.host), subject, body, now)
        .map_err(|e| SendError::Io(e.to_string()))?;
    let v = serde_json::to_value(&msg).map_err(|e| io_err(io::Error::other(e)))?;
    match secret::check(&v, redaction, main_root, policy) {
        Outcome::Clear => {}
        Outcome::Finding(ids) => return Err(SendError::Secret(ids)),
        Outcome::CouldNotRun(c) => return Err(SendError::CheckFailed(c)),
    }
    let id = msg.id.parse::<Ulid>().map_err(|e| io_err(io::Error::other(e)))?;
    let bytes = serde_json::to_vec(&msg).map_err(|e| io_err(io::Error::other(e)))?;
    // S§7.6 step 4 (FX-28): held to the end, across the folder makes, the write and the link.
    let _held = Delivery::begin(&sp.store, MAIL_LOCK_WAIT).map_err(|e| SendError::Io(e.to_string()))?;
    for d in [&sp.mail, &sp.outbox, &sp.outbox_tmp] {
        ensure_mail_dir(d).map_err(io_err)?;
    }
    let file = format!("{id}.json");
    let tmp = sp.outbox_tmp.join(&file);
    write_synced(&tmp, &bytes).map_err(io_err)?;
    // The test hook between the lock and the link (FX-28 review round 1 F1); nothing without `test-hooks`.
    fire_point(sp.store.dir(), "remote_send_before_link").map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io_err(io::Error::other(e))
    })?;
    publish(&tmp, &sp.outbox.join(&file)).map_err(io_err)?;
    // The message is in the outbox now: an error here would make the sender send it again. A left tmp/ name is a
    // second name of the same file, swept after 1 h.
    let _ = sync_dir(&sp.outbox);
    let _ = fs::remove_file(&tmp);
    Ok(id)
}

/// Links the written `tmp` name to `dst`; a link never replaces a file (M-C3). On a failed link the `tmp` name is
/// removed (best effort; else the sweep), so nothing is left in either folder.
fn publish(tmp: &Path, dst: &Path) -> io::Result<()> {
    fs::hard_link(tmp, dst).inspect_err(|_| {
        let _ = fs::remove_file(tmp);
    })
}

/// `fsync` of a folder (unix); elsewhere nothing.
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

/// The folder check of `ensure_mail_root` without making anything, on `dirs` (each in the one before it):
/// `Ok(false)` at the first one that does not exist, `InvalidInput` for one that is a link or not a folder.
fn real_dirs(dirs: &[&Path]) -> io::Result<bool> {
    for d in dirs {
        match fs::symlink_metadata(d) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{} is not a folder", d.display())))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

/// The message files of the last folder of `dirs`, after the folder check of each: regular files (not links) named
/// `<ULID>.json`, sorted by ULID. No folder is empty; a folder or entry that cannot be read is an error.
fn ulid_files(dirs: &[&Path]) -> io::Result<Vec<(Ulid, PathBuf)>> {
    if !real_dirs(dirs)? {
        return Ok(Vec::new());
    }
    let Some(dir) = dirs.last() else { return Ok(Vec::new()) };
    let mut v = Vec::new();
    for e in fs::read_dir(dir)? {
        let e = e?;
        if !e.file_type()?.is_file() {
            continue;
        }
        if let Some(u) = e.file_name().to_str().and_then(ulid_file) {
            v.push((u, e.path()));
        }
    }
    v.sort_by_key(|(u, _)| *u);
    Ok(v)
}

/// The messages in `mail/outbox/` that wait for a push, in ULID order.
pub fn pending(sp: &SyncPaths) -> io::Result<Vec<(Ulid, PathBuf)>> {
    ulid_files(&[&sp.mail, &sp.outbox])
}

/// The number of messages in `mail/outbox/`; no folder is 0, and one that cannot be read is an error (S§19 #20:
/// unsent mail is never taken for none).
pub fn count(sp: &SyncPaths) -> io::Result<usize> {
    pending(sp).map(|v| v.len())
}

/// The number of messages in `mail/outbox/refused/`; no folder is 0, and one that cannot be read is an error.
pub fn refused_count(sp: &SyncPaths) -> io::Result<usize> {
    ulid_files(&[&sp.mail, &sp.outbox, &sp.refused]).map(|v| v.len())
}

/// The bytes of the regular file `path`, read to at most `FILE_MAX + 1` bytes (M§5), with no link follow and no
/// wait on a FIFO. `Ok(None)` when it does not exist; a link or anything that is not a regular file is
/// `InvalidInput`.
fn read_capped(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let not_regular =
        || io::Error::new(io::ErrorKind::InvalidInput, format!("{} is not a regular file", path.display()));
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(not(unix))]
    if fs::symlink_metadata(path).is_ok_and(|m| !m.is_file()) {
        return Err(not_regular());
    }
    let f = match o.open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        #[cfg(unix)]
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Err(not_regular()),
        Err(e) => return Err(e),
    };
    if !f.metadata()?.is_file() {
        return Err(not_regular());
    }
    let mut b = Vec::new();
    f.take(FILE_MAX + 1).read_to_end(&mut b)?;
    Ok(Some(b))
}

/// Sets the modification time of the regular file `path` (no link follow).
fn set_mtime(path: &Path, t: SystemTime) -> io::Result<()> {
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    // Windows `SetFileTime` needs `FILE_WRITE_ATTRIBUTES`, which a read-only handle does not have (os error 5).
    #[cfg(not(unix))]
    o.write(true);
    o.open(path)?.set_modified(t)
}

/// Why the notice of a refusal (S§8.6) was not delivered.
#[derive(Debug)]
pub enum NoticeError {
    /// A later pass can deliver it (its delivery did not begin: a busy `mail/lock`, the lock I/O, a bad state or
    /// `mail/`, no workspace): the message stays in the outbox.
    Later(io::Error),
    /// No later pass can deliver it (FX-24 review R2 F1, R3 F3): the message moves to `refused/` with no notice. The
    /// text is the reason of the `errors.log` line: `full`, `no-record`, `maildir`, `io` or `moved`.
    Never(&'static str),
}

impl From<io::Error> for NoticeError {
    fn from(e: io::Error) -> NoticeError { NoticeError::Later(e) }
}

/// Calls `notify(sender, subject, body)` once when the sender is a member name (S§8.6 "At commit"; `cli` and `zygo`
/// have no inbox), then moves `outbox/<id>.json` to `outbox/refused/` without rewriting it. The order (FX-24 review
/// F5): notify; then M§5's move: link into `refused/`, set its time to `now` (the `refused/` age counts from the
/// refusal, S§9), `fsync` `refused/`, remove the `outbox/` name, `fsync` `outbox/`. A `NoticeError::Later` of `notify`
/// is returned as its error and the outbox file stays, for the next pass; a `NoticeError::Never` moves the file all
/// the same, and its reason is the `Ok` value (FX-24 review R2 F1: a notice that no pass can deliver must not keep
/// the message, nor log it again, on every pass). `Ok(None)`: the notice was delivered, or none was due. A move
/// that fails after a delivered notice leaves the file too, and the next pass sends a second notice (the accepted
/// cost). No source file is `NotFound`. A `refused/<id>.json` left by an earlier call that stopped partway is
/// finished when it holds the same bytes; other bytes are `AlreadyExists` before any notice, and the outbox file
/// stays. A file that does not parse, or is larger than `FILE_MAX`, is moved and gets no notice.
pub fn refuse(
    sp: &SyncPaths,
    id: Ulid,
    ids: &[String],
    notify: &mut dyn FnMut(&str, &str, &str) -> Result<(), NoticeError>,
    now: SystemTime,
) -> io::Result<Option<&'static str>> {
    let file = format!("{id}.json");
    let (src, dst) = (sp.outbox.join(&file), sp.refused.join(&file));
    let Some(bytes) = read_capped(&src)? else {
        return Err(io::ErrorKind::NotFound.into());
    };
    let whole = bytes.len() as u64 <= FILE_MAX;
    let msg = if whole { serde_json::from_slice::<Message>(&bytes).ok() } else { None };
    for d in [&sp.mail, &sp.outbox, &sp.refused] {
        ensure_mail_dir(d)?;
    }
    let another = || io::Error::new(io::ErrorKind::AlreadyExists, format!("{} holds another message", dst.display()));
    // A `refused/` name that holds another message stops the refusal before the notice, so no pass notifies for it.
    if read_capped(&dst)?.is_some_and(|d| !(whole && d == bytes)) {
        return Err(another());
    }
    // S§8.6 (FX-24 review F5): the notice first. A notice that a later pass can deliver (a busy `mail/lock`, an I/O
    // error) leaves the message in the outbox, and the next pass refuses it and sends the notice again. One that no
    // pass can deliver moves the message now (review R2 F1).
    let mut lost = None;
    if let Some(m) = &msg {
        let sender = m.from.split_once('@').map_or(m.from.as_str(), |(n, _)| n);
        if valid_member_name(sender) {
            let sent = notify(
                sender,
                &format!("not sent: {id}"),
                &format!(
                    "zygo did not send message {id} to {}: it holds a secret ({}). The message is kept on this machine in mail/outbox/refused/.",
                    m.to,
                    ids.join(", ")
                ),
            );
            match sent {
                Ok(()) => {}
                Err(NoticeError::Later(e)) => return Err(e),
                Err(NoticeError::Never(why)) => lost = Some(why),
            }
        }
    }
    match fs::hard_link(&src, &dst) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            // The link of an earlier call holds the same bytes; anything else is another message.
            if !(whole && read_capped(&dst)?.is_some_and(|d| d == bytes)) {
                return Err(another());
            }
        }
        Err(e) => return Err(e),
    }
    set_mtime(&dst, now)?;
    sync_dir(&sp.refused)?;
    fs::remove_file(&src)?;
    // The file is in refused/ now: a failed fsync here is no error.
    let _ = sync_dir(&sp.outbox);
    Ok(lost)
}

/// Deletes `outbox/refused/` files older than `keep` and `outbox/tmp/` files older than 1 h by `now` (S§9 last
/// paragraph). Queued `outbox/` files and `mail/sync/incoming/` are never touched. A `mail/`, `outbox/` or swept
/// folder that is a link or not a folder stops that folder's sweep, and a link in a swept folder is deleted itself,
/// never its target. Every error is ignored.
pub fn sweep(sp: &SyncPaths, keep: Duration, now: SystemTime) {
    for (dir, age) in [(&sp.refused, keep), (&sp.outbox_tmp, TMP_MAX_AGE)] {
        if !real_dirs(&[&sp.mail, &sp.outbox, dir]).unwrap_or(false) {
            continue;
        }
        let Ok(files) = fs::read_dir(dir) else { continue };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // a failed link into outbox/ leaves no tmp/ name
    fn failed_link_removes_the_tmp_name() {
        let t = tempfile::TempDir::new().unwrap();
        let (tmp, dst) = (t.path().join("tmp.json"), t.path().join("dst.json"));
        fs::write(&tmp, b"new").unwrap();
        fs::write(&dst, b"old").unwrap();
        assert_eq!(publish(&tmp, &dst).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert!(!tmp.exists());
        assert_eq!(fs::read(&dst).unwrap(), b"old");
    }
}

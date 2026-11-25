//! The outbox of remote mail (S§5 Outbox, S§7.2, S§8.6, S§9 last paragraph, S§15): the send-time rules and secret pass, the move of a refused message and its notice, the counts and the local sweep.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ulid::Ulid;
use zygo_config::schema::{RedactionConfig, SyncOff, SyncSettings};
use zygo_core::ids::valid_member_name;
use zygo_redact::secret::DeadlinePolicy;
use zygo_workspace::mail::store::write_synced;
use zygo_workspace::mail::{ensure_mail_root, MailError, Message};

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
    for d in [&sp.mail, &sp.outbox, &sp.outbox_tmp] {
        ensure_mail_root(d).map_err(io_err)?;
    }
    let file = format!("{id}.json");
    let tmp = sp.outbox_tmp.join(&file);
    write_synced(&tmp, &bytes).map_err(io_err)?;
    // A link never replaces a file (M-C3).
    if let Err(e) = fs::hard_link(&tmp, sp.outbox.join(&file)) {
        let _ = fs::remove_file(&tmp);
        return Err(io_err(e));
    }
    // The message is in the outbox now: an error here would make the sender send it again. A left tmp/ name is a
    // second name of the same file, swept after 1 h.
    let _ = sync_dir(&sp.outbox);
    let _ = fs::remove_file(&tmp);
    Ok(id)
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

/// The message files of `dir`: regular files (not links) named `<ULID>.json`, sorted by ULID. No folder is empty.
fn ulid_files(dir: &Path) -> io::Result<Vec<(Ulid, PathBuf)>> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut v = Vec::new();
    for e in entries {
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
    ulid_files(&sp.outbox)
}

/// The number of messages in `mail/outbox/`; an unreadable outbox counts 0.
pub fn count(sp: &SyncPaths) -> usize {
    pending(sp).map_or(0, |v| v.len())
}

/// The number of messages in `mail/outbox/refused/`; an unreadable folder counts 0.
pub fn refused_count(sp: &SyncPaths) -> usize {
    ulid_files(&sp.refused).map_or(0, |v| v.len())
}

/// Moves `outbox/<id>.json` to `outbox/refused/` without rewriting it, then calls `notify(sender, subject, body)`
/// once when the sender is a member name (S§8.6 "At commit"; `cli` and `zygo` have no inbox). No source file is
/// `NotFound`. A link left by an earlier call that stopped partway is finished. A file that does not parse is
/// moved and gets no notice.
pub fn refuse(
    sp: &SyncPaths,
    id: Ulid,
    ids: &[String],
    notify: &mut dyn FnMut(&str, &str, &str) -> io::Result<()>,
) -> io::Result<()> {
    let file = format!("{id}.json");
    let (src, dst) = (sp.outbox.join(&file), sp.refused.join(&file));
    let Some(bytes) = state::read_regular(&src)? else {
        return Err(io::ErrorKind::NotFound.into());
    };
    let msg = serde_json::from_slice::<Message>(&bytes).ok();
    for d in [&sp.mail, &sp.refused] {
        ensure_mail_root(d)?;
    }
    match fs::hard_link(&src, &dst) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {} // the link of an earlier call
        Err(e) => return Err(e),
    }
    fs::remove_file(&src)?;
    let _ = sync_dir(&sp.refused);
    let Some(m) = msg else { return Ok(()) };
    let sender = m.from.split_once('@').map_or(m.from.as_str(), |(n, _)| n);
    if !valid_member_name(sender) {
        return Ok(());
    }
    notify(
        sender,
        &format!("not sent: {id}"),
        &format!(
            "zygo did not send message {id} to {}: it holds a secret ({}). The message is kept on this machine in mail/outbox/refused/.",
            m.to,
            ids.join(", ")
        ),
    )
}

/// Deletes `outbox/refused/` files older than `keep` and `outbox/tmp/` files older than 1 h by `now` (S§9 last
/// paragraph). Queued `outbox/` files and `mail/sync/incoming/` are never touched. A folder that is not a real
/// folder is skipped, and a link in one is deleted itself, never its target. Every error is ignored.
pub fn sweep(sp: &SyncPaths, keep: Duration, now: SystemTime) {
    for (dir, age) in [(&sp.refused, keep), (&sp.outbox_tmp, TMP_MAX_AGE)] {
        if !fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) {
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

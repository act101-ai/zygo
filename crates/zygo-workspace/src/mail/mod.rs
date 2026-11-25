//! Workspace mail: the message file and the maildir store (M§5). Owner: nodes ML-2, ML-4.
pub mod names;
pub mod store;

use std::fmt;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use zygo_core::paths::Paths;

/// The largest subject in bytes (M§5).
pub const SUBJECT_MAX: usize = 200;
/// The largest body in bytes, 1 MiB (M§5).
pub const BODY_MAX: usize = 1048576;

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
            MailError::Io(e) => write!(f, "zygo: mail: {e}"),
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

/// One delivered message: its id and the pane key it went to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub id: String,
    pub key: String,
}

/// One target `send` did not deliver to: its pane key and the error.
#[derive(Debug)]
pub struct Refused {
    pub key: String,
    pub error: MailError,
}

/// What one `send` did: the messages delivered and the targets refused, each in target (key) order.
#[derive(Debug)]
pub struct SendReport {
    pub sent: Vec<Sent>,
    pub refused: Vec<Refused>,
}

/// Sends one message to each target of `to` (M§5, M§8). It checks the subject and body limits and `resolve`s `to`
/// before it writes a file; those errors are its only `Err`. Then it writes one file per target, with `from` from
/// `names::sender` and `to` the target's name or, with none, its pane key. A target whose message it cannot make or
/// deliver (for example `Full`, M§5 limit per target) goes into `refused`, and the send continues with the next
/// target. It writes no log line, so the subject and body reach only `mail/` (M-C2).
#[allow(clippy::too_many_arguments)]
pub fn send(
    paths: &Paths,
    ws: &crate::state::Workspace,
    sender_key: Option<&str>,
    to: &str,
    subject: &str,
    body: &str,
    cfg: &zygo_config::MailConfig,
    now: SystemTime,
) -> Result<SendReport, MailError> {
    check_limits(subject, body)?;
    let targets = names::resolve(ws, to, sender_key)?;
    let from = names::sender(ws, sender_key);
    let dir = mail_dir(paths, &ws.id);
    let mut report = SendReport {
        sent: Vec::with_capacity(targets.len()),
        refused: Vec::new(),
    };
    for t in targets {
        let label = t.name.as_deref().unwrap_or(&t.key);
        let delivered = Message::new(&from, label, subject, body, now)
            .and_then(|msg| store::deliver(&dir, &t.key, label, &msg, cfg.max_unread).map(|()| msg.id));
        match delivered {
            Ok(id) => report.sent.push(Sent { id, key: t.key }),
            Err(error) => report.refused.push(Refused { key: t.key, error }),
        }
    }
    Ok(report)
}

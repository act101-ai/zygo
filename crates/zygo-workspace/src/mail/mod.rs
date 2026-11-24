//! Workspace mail: the message file and the maildir store (M§5). Owner: node ML-2.
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
        if subject.len() > SUBJECT_MAX {
            return Err(MailError::SubjectTooLong);
        }
        if body.len() > BODY_MAX {
            return Err(MailError::BodyTooLarge);
        }
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

/// A mail error; `Display` is the whole user line.
#[derive(Debug)]
pub enum MailError {
    SubjectTooLong,
    BodyTooLarge,
    BadKey,
    BadId,
    Full { label: String, max: u32 },
    Duplicate { id: String },
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

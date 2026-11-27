//! The check at each use (R§7.3) and the pinned check (R§7.4): one read, one digest, one store lookup.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{digest, is_workspace_approved_in, parse, read_at, read_default, Definition, Files, GitRunner, Read};

/// The approved read that a session saves: the commit and the digest of the files read there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub commit: String,
    pub digest: String,
    /// AU78: fields this version does not know, kept through a read and a write.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Pin {
    /// A pin with no unknown fields.
    pub fn new(commit: String, digest: String) -> Pin {
        Pin { commit, digest, extra: serde_json::Map::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Use {
    Absent { full_ref: String },
    Unread { why: String },
    /// `workspace`: the read has `.zygo/workspace.toml` (as `Read::Invalid`; WD-16 review F4).
    Invalid { full_ref: Option<String>, commit: String, why: String, workspace: bool },
    NotApproved { full_ref: String, files: Files, digest: String, def: Definition },
    Approved { full_ref: String, pin: Pin, files: Files, def: Definition },
}

/// R§7.3: one read of the default branch; the digest and the definition both come from that one read.
pub fn check_default(top: &Path, ws_id: &str, approvals: &Path, git: &dyn GitRunner) -> Use {
    super::trap("check_default");
    let files = match read_default(top, git) {
        Read::Absent { full_ref } => return Use::Absent { full_ref: full_ref.unwrap_or_default() },
        Read::Unread { why } => return Use::Unread { why },
        Read::Invalid { full_ref, commit, why, workspace } => return Use::Invalid { full_ref, commit, why, workspace },
        Read::Gone { commit } => {
            return Use::Unread { why: format!("commit {commit} or one of its objects is missing") };
        }
        Read::Found { files } => files,
    };
    let def = match parse(&files) {
        Ok(def) => def,
        Err(why) => {
            let workspace = files.workspace.is_some();
            return Use::Invalid { full_ref: files.full_ref, commit: files.commit, why, workspace };
        }
    };
    let digest = digest(&files);
    let full_ref = files.full_ref.clone().unwrap_or_default();
    if is_workspace_approved_in(approvals, ws_id, &digest) {
        Use::Approved { full_ref, pin: Pin::new(files.commit.clone(), digest), files, def }
    } else {
        Use::NotApproved { full_ref, files, digest, def }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinnedUse {
    Approved { def: Definition },
    Gone,
    NotApproved,
    Unread { why: String },
}

/// R§7.4: one read at `pin.commit`; approved only when its digest is `pin.digest` and that pair is stored.
pub fn check_pinned(top: &Path, ws_id: &str, pin: &Pin, approvals: &Path, git: &dyn GitRunner) -> PinnedUse {
    super::trap("check_pinned");
    let files = match read_at(top, &pin.commit, git) {
        Read::Gone { .. } => return PinnedUse::Gone,
        Read::Unread { why } => return PinnedUse::Unread { why },
        Read::Absent { .. } | Read::Invalid { .. } => return PinnedUse::NotApproved,
        Read::Found { files } => files,
    };
    if digest(&files) != pin.digest || !is_workspace_approved_in(approvals, ws_id, &pin.digest) {
        return PinnedUse::NotApproved;
    }
    match parse(&files) {
        Ok(def) => PinnedUse::Approved { def },
        Err(_) => PinnedUse::NotApproved,
    }
}

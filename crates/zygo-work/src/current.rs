//! The current work item (§13.2): explicit setting first, then detection in config order.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zygo_config::WorkConfig;

use crate::repo::{self, Checkout};
use crate::sources::{self, Source};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItem {
    pub source: String,
    pub id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub status: Option<String>,
    pub check_hint: Option<String>,
    pub files_hint: Vec<String>,
    /// Reason the lookup failed; the id is kept.
    pub lookup_failed: Option<String>,
}

impl WorkItem {
    /// The §13.1 unit id: `<source>:<segment>`, each id character outside `[A-Za-z0-9._-]` is `-`.
    /// A segment over 48 characters keeps its first 39, then `-` and 8 hex of its SHA-256.
    pub fn unit(&self) -> String {
        let mut seg: String = self
            .id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '-' })
            .collect();
        if !seg.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            seg.insert(0, 'x');
        }
        if seg.len() > 48 {
            let h = hex::encode(Sha256::digest(seg.as_bytes()));
            seg = format!("{}-{}", &seg[..39], &h[..8]); // `seg` is ASCII here
        }
        format!("{}:{seg}", self.source)
    }
}

fn build(src: &dyn Source, co: &Checkout, id: String) -> WorkItem {
    let r = if src.looks_up() { Some(src.lookup(co, &id)) } else { None };
    let (item, failed) = match r {
        Some(Ok(i)) => (i, None),
        Some(Err(e)) => (Default::default(), Some(e)),
        None => (Default::default(), None),
    };
    WorkItem {
        source: src.name().into(),
        id,
        title: item.title,
        url: item.url,
        status: item.status,
        check_hint: item.check_hint,
        files_hint: item.files_hint,
        lookup_failed: failed,
    }
}

fn setting(co: &Checkout, notes: &mut Vec<String>) -> Option<(Box<dyn Source>, String)> {
    let path = co.common_dir.join("zygo-work");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                notes.push(format!("zygo: ignoring unreadable {}", path.display()));
            }
            return None;
        }
    };
    let parsed = text.strip_suffix('\n').unwrap_or(&text).split_once(' ').and_then(|(s, id)| {
        if id.is_empty() || id.chars().any(|c| c.is_control()) {
            return None;
        }
        Some((sources::by_name(s)?, id.to_string()))
    });
    if parsed.is_none() {
        notes.push(format!("zygo: ignoring unreadable {}", path.display()));
    }
    parsed
}

/// Like [`resolve`], plus the notes for the error sink. All child commands share one 2 s budget.
pub fn resolve_noted(repo: &Path, cfg: &WorkConfig) -> (Option<WorkItem>, Vec<String>) {
    resolve_noted_with_budget(repo, cfg, repo::BOUND)
}

/// [`resolve_noted`] with a different total budget. For tests only.
#[doc(hidden)]
pub fn resolve_noted_with_budget(repo: &Path, cfg: &WorkConfig, budget: Duration) -> (Option<WorkItem>, Vec<String>) {
    let mut notes = Vec::new();
    let Some(co) = repo::checkout(repo, Instant::now() + budget) else { return (None, notes) };
    if let Some((src, id)) = setting(&co, &mut notes) {
        return (Some(build(src.as_ref(), &co, id)), notes);
    }
    let branch = repo::branch(repo, co.deadline);
    for name in &cfg.sources {
        let Some(src) = sources::by_name(name) else { continue };
        if let Some(id) = src.detect(&co, branch.as_deref(), &mut notes) {
            return (Some(build(src.as_ref(), &co, id)), notes);
        }
    }
    (None, notes)
}

pub fn resolve(repo: &Path, cfg: &WorkConfig) -> Option<WorkItem> {
    resolve_noted(repo, cfg).0
}

/// A refusal caused by the caller's input or location; the CLI maps it to exit 2.
#[derive(Debug)]
pub enum UsageError {
    NotInRepository,
    NoSourceAccepts(String),
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsageError::NotInRepository => f.write_str("not in a git repository"),
            UsageError::NoSourceAccepts(arg) => write!(f, "no work source accepts `{arg}`"),
        }
    }
}

impl std::error::Error for UsageError {}

fn checkout(repo: &Path, budget: Duration) -> Result<Checkout> {
    Ok(repo::checkout(repo, Instant::now() + budget).ok_or(UsageError::NotInRepository)?)
}

pub fn set(repo: &Path, cfg: &WorkConfig, arg: &str) -> Result<WorkItem> {
    set_with_budget(repo, cfg, arg, repo::BOUND)
}

/// [`set`] with a different total budget. For tests only.
#[doc(hidden)]
pub fn set_with_budget(repo: &Path, cfg: &WorkConfig, arg: &str, budget: Duration) -> Result<WorkItem> {
    let co = checkout(repo, budget)?;
    for name in &cfg.sources {
        let Some(src) = sources::by_name(name) else { continue };
        let Some(id) = src.normalize(&co, arg) else { continue };
        let tmp = co.common_dir.join(format!("zygo-work.tmp.{}", std::process::id()));
        std::fs::write(&tmp, format!("{} {id}\n", src.name()))?;
        if let Err(e) = std::fs::rename(&tmp, co.common_dir.join("zygo-work")) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        return Ok(build(src.as_ref(), &co, id));
    }
    Err(UsageError::NoSourceAccepts(arg.to_string()).into())
}

pub fn clear(repo: &Path) -> Result<()> {
    clear_with_budget(repo, repo::BOUND)
}

/// [`clear`] with a different total budget. For tests only.
#[doc(hidden)]
pub fn clear_with_budget(repo: &Path, budget: Duration) -> Result<()> {
    let co = checkout(repo, budget)?;
    match std::fs::remove_file(co.common_dir.join("zygo-work")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

pub fn stderr_lines(item: Option<&WorkItem>, notes: &[String]) -> Vec<String> {
    let mut v = notes.to_vec();
    if let Some(i) = item
        && let Some(r) = &i.lookup_failed {
            v.push(format!("zygo: work item lookup failed for {}: {r}", i.id));
        }
    v
}

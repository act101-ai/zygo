//! One agent change of a record file: the D§6.6 agent rows (D§6.3, D§9.4 step 9, D-C4, D-C5).
use std::collections::BTreeSet;

use zygo_core::esc;

use crate::decisions::record::{name_kind, NameKind, Record, Status};

/// One version of a record file in one commit (D§9.4 step 9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Version {
    Absent,
    Malformed,
    Valid(Record),
}

/// The node range facts that one record change is checked against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RangeCtx {
    /// Full commit ids of the node range R (`git rev-list <base>..<head>`).
    pub range: BTreeSet<String>,
    /// Ids that a record added in R names in `supersedes`.
    pub superseding: BTreeSet<String>,
}

/// `None`: an allowed agent transition of D§6.6. `Some(problem)`: the first fault, in rule order.
pub fn agent_change(id: &str, old: &Version, new: &Version, ctx: &RangeCtx) -> Option<String> {
    match name_kind(&format!("{id}.md")) {
        NameKind::Other => return None,
        NameKind::Malformed => return Some(format!("{}: malformed record name", esc(id))),
        NameKind::Record(_) => {}
    }
    let n = match (old, new) {
        (Version::Absent, Version::Absent) => return None,
        (_, Version::Absent) => return Some(format!("{id}: record deleted")),
        (_, Version::Malformed) => return Some(format!("{id}: malformed record")),
        (Version::Malformed, _) => return Some(format!("{id}: changes a malformed record")),
        (_, Version::Valid(n)) => n,
    };
    let o = match old {
        Version::Valid(o) => Some(o),
        _ => None,
    };
    if o.iter().chain([&n]).any(|r| r.id != id) {
        return Some(format!("{id}: record id does not match its name"));
    }
    match o {
        None => created(id, n),
        Some(o) => changed(id, o, n, ctx),
    }
}

/// The first key whose flag is `true`.
fn first(keys: &[(&'static str, bool)]) -> Option<&'static str> {
    keys.iter().find(|(_, hit)| *hit).map(|(k, _)| *k)
}

/// D§6.6 row "none → `open`".
fn created(id: &str, n: &Record) -> Option<String> {
    if n.status != Status::Open {
        return Some(format!("{id}: new record with status {}", n.status.name()));
    }
    let operator = [
        ("reviewed", n.reviewed.is_some()),
        ("review_note", n.review_note.is_some()),
        ("promoted_to", n.promoted_to.is_some()),
        ("promotion", n.promotion.is_some()),
    ];
    if let Some(k) = first(&operator) {
        return Some(format!("{id}: new record writes operator key {k}"));
    }
    if let Some(k) = first(&[("fixed_by", n.fixed_by.is_some()), ("promoted_by", n.promoted_by.is_some())]) {
        return Some(format!("{id}: new record writes {k}"));
    }
    if n.supersedes.as_deref() == Some(id) {
        return Some(format!("{id}: supersedes itself"));
    }
    None
}

/// The first key outside `allowed` that differs between `o` and `n`, among the keys an agent row can write.
fn writes_outside(o: &Record, n: &Record, allowed: &[&str]) -> Option<&'static str> {
    let keys = [
        ("title", o.title != n.title),
        ("depends_on", o.depends_on != n.depends_on),
        ("reverse_cost", o.reverse_cost != n.reverse_cost),
        ("fixed_by", o.fixed_by != n.fixed_by),
        ("promoted_by", o.promoted_by != n.promoted_by),
        ("sections", o.body != n.body),
    ];
    keys.iter().find(|(k, hit)| *hit && !allowed.contains(k)).map(|(k, _)| *k)
}

/// The agent rows of D§6.6 for a record that is valid in both versions.
fn changed(id: &str, o: &Record, n: &Record, ctx: &RangeCtx) -> Option<String> {
    use Status::*;
    if o == n {
        return None;
    }
    if matches!(o.status, Fixed | Superseded) {
        return Some(format!("{id}: {} is final", o.status.name()));
    }
    let frozen = [
        ("kind", o.kind != n.kind),
        ("class", o.class != n.class),
        ("unit", o.unit != n.unit),
        ("round", o.round != n.round),
        ("role", o.role != n.role),
        ("model", o.model != n.model),
        ("commit", o.commit != n.commit),
        ("date", o.date != n.date),
        ("supersedes", o.supersedes != n.supersedes),
    ];
    if let Some(k) = first(&frozen) {
        return Some(format!("{id}: frozen key {k} changed"));
    }
    if matches!(n.status, Ratified | Overturned) && n.status != o.status {
        return Some(format!("{id}: agent writes status {}", n.status.name()));
    }
    let operator = [
        ("reviewed", o.reviewed != n.reviewed),
        ("review_note", o.review_note != n.review_note),
        ("promoted_to", o.promoted_to != n.promoted_to),
        ("promotion", o.promotion != n.promotion),
    ];
    if let Some(k) = first(&operator) {
        return Some(format!("{id}: agent writes operator key {k}"));
    }
    let allowed: &[&str] = match (o.status, n.status) {
        (Open, Open) => &["title", "depends_on", "reverse_cost", "sections"],
        (Open, Fixed) | (Overturned, Fixed) => &["fixed_by"],
        (Open, Superseded) => &[],
        _ => &["title", "depends_on", "reverse_cost", "fixed_by", "promoted_by", "sections"],
    };
    if let Some(k) = writes_outside(o, n, allowed) {
        return Some(format!("{id}: {} to {} writes {k}", o.status.name(), n.status.name()));
    }
    match (o.status, n.status) {
        (Open, Open) | (Open, Fixed) | (Open, Superseded) | (Overturned, Fixed) => {}
        (Ratified, Ratified) => {
            let adds_only = o.promoted_to.is_some()
                && o.promoted_by.is_none()
                && n.promoted_by.is_some()
                && Record { promoted_by: None, ..n.clone() } == *o;
            if !adds_only {
                return Some(format!("{id}: an agent changes a ratified record only to add promoted_by"));
            }
        }
        (from, to) => {
            return Some(format!("{id}: not an agent transition: {} to {}", from.name(), to.name()));
        }
    }
    if let Some(c) = &n.fixed_by {
        let fresh = n.fixed_by != o.fixed_by || (n.status == Fixed && o.status != Fixed);
        if fresh && !ctx.range.contains(c) {
            return Some(format!("{id}: fixed_by {c} is not in the node range"));
        }
    }
    if let Some(c) = &n.promoted_by {
        if n.promoted_by != o.promoted_by && !ctx.range.contains(c) {
            return Some(format!("{id}: promoted_by {c} is not in the node range"));
        }
    }
    if n.status == Superseded && n.supersedes.as_deref() == Some(id) {
        return Some(format!("{id}: supersedes itself"));
    }
    if n.status == Superseded && o.status != Superseded && !ctx.superseding.contains(id) {
        return Some(format!("{id}: superseded but no record added in the node range names it"));
    }
    None
}

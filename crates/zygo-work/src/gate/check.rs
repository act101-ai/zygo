//! The keep check of `work_verdict` (D§9.4): the dispositions against the node range. It stores nothing: the node
//! range and the touched records are computed per call (D-C3).
use std::collections::BTreeSet;

use zygo_core::esc;

use crate::decisions::record::{parse_record, Class, Kind, Status};
use crate::gate::git::{Blob, GitFail, VerdictGit};
use crate::gate::transition::Version;

/// The proof of one disposition (D§8.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proof {
    Fixed { commit: String },
    Rejected { reason: String },
    Recorded { record: String },
}

/// One `dispositions` entry of `work_verdict` (D§9.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disposition {
    pub finding: String,
    pub class: Class,
    pub proof: Proof,
}

/// The keep facts the check reads (D§9.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeepRequest {
    pub unit: String,
    pub round: u32,
    pub head: Option<String>,
    pub base: Option<String>,
    pub dispositions: Option<Vec<Disposition>>,
}

/// The most commits of the node range (D§9.4 step 4).
pub const RANGE_MAX: usize = 10_000;
/// The most record changes read in step 9 (D§9.4 step 9).
pub const RECORD_CHANGES_MAX: usize = 1_000;
/// The most problems shown in `<problems>` (D§9.3).
pub const PROBLEMS_SHOWN: usize = 10;
/// The most bytes of `<problems>` (D§9.3).
pub const PROBLEMS_BYTES: usize = 4_096;

/// The problems of one keep, in step order.
#[derive(Default)]
struct Problems(Vec<String>);

impl Problems {
    fn push(&mut self, p: String) {
        self.0.push(p);
    }

    /// A git failure, once per distinct text: the `--batch` process stays failed after its first error.
    fn git(&mut self, e: GitFail) {
        let p = e.to_string();
        if !self.0.contains(&p) {
            self.0.push(p);
        }
    }
}

/// The D§9.4 problems of one keep; empty when the keep passes.
pub fn check_keep(req: &KeepRequest, git: &mut dyn VerdictGit) -> Vec<String> {
    let mut p = Problems::default();
    // Step 1: head.
    let head = match &req.head {
        None => none(&mut p, "no head commit"),
        Some(h) => match git.commit(h) {
            Ok(Some(id)) => Some(id),
            Ok(None) => none(&mut p, "no head commit"),
            Err(e) => failed(&mut p, e),
        },
    };
    // Step 2: dispositions.
    if req.dispositions.is_none() {
        p.push("no dispositions".into());
    }
    let ds: &[Disposition] = req.dispositions.as_deref().unwrap_or(&[]);
    // Step 3: uniqueness.
    unique(ds, &mut p);
    let Some(head) = head else { return p.0 };
    // Step 4: node range.
    let Some((base, range)) = node_range(req, &head, git, &mut p) else { return p.0 };
    // Steps 5 to 7, each in entry order: one problem per entry.
    for d in ds {
        if let Proof::Fixed { commit } = &d.proof {
            match in_range(commit, &range, git) {
                Ok(true) => {}
                Ok(false) => p.push(format!("{}: commit {} is not in the node range", esc(&d.finding), esc(commit))),
                Err(e) => p.git(e),
            }
        }
    }
    for d in ds {
        if let Proof::Rejected { reason } = &d.proof {
            if reason.is_empty() {
                p.push(format!("{}: no reason", esc(&d.finding)));
            } else if d.class != Class::None {
                p.push(format!("{}: class {} needs a record for the operator", esc(&d.finding), d.class.name()));
            }
        }
    }
    for d in ds {
        if let Proof::Recorded { record } = &d.proof {
            match recorded_why(req, d.class, record, &head, &base, git) {
                Ok(None) => {}
                Ok(Some(why)) => p.push(format!("{}: {why}", esc(&d.finding))),
                Err(e) => p.git(e),
            }
        }
    }
    p.0
}

/// D§9.4 step 5: `commit` resolves and its full id is in R.
fn in_range(commit: &str, range: &BTreeSet<String>, git: &mut dyn VerdictGit) -> Result<bool, GitFail> {
    if commit.is_empty() {
        return Ok(false);
    }
    Ok(git.commit(commit)?.is_some_and(|c| range.contains(&c)))
}

/// The record file `name` at `rev` (D§9.4 steps 7 to 9).
fn version(git: &mut dyn VerdictGit, rev: &str, name: &str) -> Result<Version, GitFail> {
    Ok(match git.record_blob(rev, name)? {
        Blob::Missing => Version::Absent,
        Blob::NotRegular => Version::Malformed,
        Blob::Data(b) => {
            let stem = name.strip_suffix(".md").unwrap_or(name);
            parse_record(stem, &b).map_or(Version::Malformed, Version::Valid)
        }
    })
}

/// D§9.4 step 7: the first failing check of one `recorded` entry, in step order; `None` when it passes.
fn recorded_why(
    req: &KeepRequest,
    class: Class,
    record: &str,
    head: &str,
    base: &str,
    git: &mut dyn VerdictGit,
) -> Result<Option<String>, GitFail> {
    if record.is_empty() {
        return Ok(Some("no record".into()));
    }
    if class == Class::None && req.round < 3 {
        return Ok(Some("recorded with class none needs round 3 or later".into()));
    }
    let r = esc(record);
    let name = format!("{record}.md");
    let rec = match version(git, head, &name)? {
        Version::Absent => return Ok(Some(format!("{r} is not at head"))),
        Version::Malformed => return Ok(Some(format!("{r} is not a valid record at head"))),
        Version::Valid(rec) => rec,
    };
    if !matches!(version(git, base, &name)?, Version::Absent) {
        return Ok(Some(format!("{r} was not added in the node range")));
    }
    if rec.unit != req.unit {
        return Ok(Some(format!("{r} has unit {}, not {}", esc(&rec.unit), esc(&req.unit))));
    }
    if rec.class != class {
        return Ok(Some(format!("{r} has class {}, not {}", rec.class.name(), class.name())));
    }
    let kind_ok = matches!(rec.kind, Kind::AcceptedLimit | Kind::Compromise | Kind::OpenFinding)
        || (rec.kind == Kind::Decision && class != Class::None);
    if !kind_ok {
        return Ok(Some(format!("{r} of kind {} cannot record this finding", rec.kind.name())));
    }
    for dep in &rec.depends_on {
        if !matches!(version(git, head, &format!("{dep}.md"))?, Version::Valid(_)) {
            return Ok(Some(format!("{r} depends on {dep}, which is not a record at head")));
        }
    }
    let stop = class != Class::None;
    Ok(match rec.status {
        Status::Open if !stop => None,
        Status::Ratified if stop => None,
        st if !stop => Some(format!("{r} is {}, not open", st.name())),
        Status::Open => Some(format!("class {} stops for the operator: ratify or overturn {r}", class.name())),
        Status::Overturned => Some(format!("{r} overturned by the operator")),
        st => Some(format!("{r} is {}, not ratified", st.name())),
    })
}

/// D§9.4 step 3: one problem per repeated label and per repeated record id.
fn unique(ds: &[Disposition], p: &mut Problems) {
    let (mut labels, mut twice) = (BTreeSet::new(), BTreeSet::new());
    for d in ds {
        if !labels.insert(d.finding.as_str()) && twice.insert(d.finding.as_str()) {
            p.push(format!("finding {} has two dispositions", esc(&d.finding)));
        }
    }
    let (mut records, mut twice) = (BTreeSet::new(), BTreeSet::new());
    for d in ds {
        if let Proof::Recorded { record } = &d.proof {
            if !records.insert(record.as_str()) && twice.insert(record.as_str()) {
                p.push(format!("{} serves two findings", esc(record)));
            }
        }
    }
}

/// D§9.4 step 4: the full base id and the node range R; `None` (with its problem) when R is not defined.
fn node_range(
    req: &KeepRequest,
    head: &str,
    git: &mut dyn VerdictGit,
    p: &mut Problems,
) -> Option<(String, BTreeSet<String>)> {
    let base = match &req.base {
        Some(b) => {
            let b = match git.commit(b) {
                Ok(Some(b)) => b,
                Ok(None) => return none(p, "no base commit"),
                Err(e) => return failed(p, e),
            };
            match git.merge_base(&b, head) {
                Ok(Some(m)) if m == b => b,
                Ok(_) => return none(p, "base is not an ancestor of head"),
                Err(e) => return failed(p, e),
            }
        }
        None => match git.merge_base("HEAD", head) {
            Ok(Some(m)) if m == head => return none(p, "head is already in HEAD: call before the merge, or give base"),
            Ok(Some(m)) => m,
            Ok(None) => return none(p, "head has no common ancestor with HEAD: give base"),
            Err(e) => return failed(p, e),
        },
    };
    match git.rev_list(&base, head, RANGE_MAX) {
        Ok(Some(ids)) => Some((base, ids.into_iter().collect())),
        Ok(None) => none(p, "node range too large"),
        Err(e) => failed(p, e),
    }
}

fn none<T>(p: &mut Problems, text: &str) -> Option<T> {
    p.push(text.to_string());
    None
}

fn failed<T>(p: &mut Problems, e: GitFail) -> Option<T> {
    p.git(e);
    None
}

/// `<problems>` of D§9.3: at most [`PROBLEMS_SHOWN`] problems joined by `; `, then `; and <n> more`, at most
/// [`PROBLEMS_BYTES`] bytes.
pub fn format_problems(problems: &[String]) -> String {
    let n = problems.len();
    let tail = |shown: usize| if n > shown { format!("; and {} more", n - shown) } else { String::new() };
    let mut text = String::new();
    let mut shown = 0;
    for q in problems.iter().take(PROBLEMS_SHOWN) {
        let sep = if shown == 0 { 0 } else { 2 };
        if text.len() + sep + q.len() + tail(shown + 1).len() > PROBLEMS_BYTES {
            break;
        }
        if shown > 0 {
            text.push_str("; ");
        }
        text.push_str(q);
        shown += 1;
    }
    if shown == 0 && n > 0 {
        // Not even the first fits: cut it at a char boundary to the room its tail leaves.
        let mut cut = PROBLEMS_BYTES.saturating_sub(tail(1).len()).min(problems[0].len());
        while !problems[0].is_char_boundary(cut) {
            cut -= 1;
        }
        text.push_str(&problems[0][..cut]);
        shown = 1;
    }
    text + &tail(shown)
}

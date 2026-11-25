//! The keep check of `work_verdict` (D§9.4): the dispositions against the node range. It stores nothing: the node
//! range and the touched records are computed per call (D-C3).
use std::collections::BTreeSet;

use zygo_core::esc;

use crate::decisions::record::Class;
use crate::gate::git::{GitFail, VerdictGit};

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
    let Some((_base, _range)) = node_range(req, &head, git, &mut p) else { return p.0 };
    p.0
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

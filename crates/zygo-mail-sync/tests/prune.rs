//! MS-14: the prune (mail sync design S§9), step 16 of the sync (S§8.3), the S§14 prune rows, S§19 #14, #15,
//! #17, #18 and SP5. Local bare repositories, the injected clock, no wall-clock wait (S-C9).
#![cfg(unix)]
mod common;

use std::path::Path;

use common::{far, git, git_in, plain, Machine, Remote};
use zygo_mail_sync::prune::{self, COMMITS_DUE, OWN_ACK_DUE};

/// 2023-11-14T22:13:20Z.
const T0: u64 = 1_700_000_000;

/// `n` new commits on `refname` in `repo` with one `git fast-import` (one process): each has the tree of the one
/// before and that one as parent; the first starts at the current `refname`, or is a root when it does not exist.
/// Returns the new tip.
fn grow(repo: &Path, refname: &str, n: u64) -> String {
    let o = plain(repo).args(["rev-parse", "--verify", "-q", refname]).output().unwrap();
    let from = String::from_utf8(o.stdout).unwrap().trim_end().to_string();
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!("commit {refname}\ncommitter t <t@t.invalid> {T0} +0000\ndata 0\n"));
        if i == 0 && !from.is_empty() {
            s.push_str(&format!("from {from}\n"));
        }
        s.push('\n');
    }
    git_in(repo, &["fast-import", "--quiet"], s.as_bytes());
    git(repo, &["rev-parse", refname])
}

#[test] // S§9 When, S§7.4 Size
fn t1_spec_values() {
    assert_eq!((COMMITS_DUE, OWN_ACK_DUE), (1000, 100_000));
}

#[test] // S§9 When: 1000 commits or more, more than 100 000 own ack entries, or forced
fn t2_due_at_1000_commits_over_100000_entries_or_forced() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let c999 = grow(&m.root, "refs/heads/t", 999);
    assert_eq!(prune::due(&m.root, &c999, 0, false, far()), Ok(false));
    assert_eq!(prune::due(&m.root, &c999, 100_000, false, far()), Ok(false));
    assert_eq!(prune::due(&m.root, &c999, 100_001, false, far()), Ok(true));
    assert_eq!(prune::due(&m.root, &c999, 0, true, far()), Ok(true));
    let c1000 = grow(&m.root, "refs/heads/t", 1);
    assert_eq!(prune::due(&m.root, &c1000, 0, false, far()), Ok(true));
}

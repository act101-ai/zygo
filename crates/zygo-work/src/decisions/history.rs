//! The commits that name a record in a `Zygo-Decision:` trailer (D§6.7).
use std::path::Path;
use std::time::Instant;

use super::record::valid_id;
use crate::repo::{git_run, RunError, BOUND};

/// The newest `limit` commits of `top` with a line `Zygo-Decision: <id>` in the message: `(full id, subject)`, newest
/// first. `--basic-regexp` and `--encoding=UTF-8` override the user's `grep.patternType` and `i18n.logOutputEncoding`.
/// A HEAD with no commit yet gives no commits. The git calls share the 2 s bound; `Err` is the stderr line that says
/// why the commits are left out.
pub fn trailer_commits(top: &Path, id: &str, limit: usize) -> Result<Vec<(String, String)>, String> {
    if !valid_id(id) {
        return Err("zygo decision: commits left out: bad id".to_string());
    }
    let deadline = Instant::now() + BOUND;
    let n = limit.to_string();
    let grep = format!("--grep=^Zygo-Decision: {id}$");
    let args = ["log", "-n", &n, "--basic-regexp", "--encoding=UTF-8", &grep, "--format=%H%x09%s", "--"];
    let out = match git_run(top, &args, deadline) {
        Ok(out) => out,
        Err(RunError::Failed) if unborn_head(top, deadline) => return Ok(Vec::new()),
        Err(e) => return Err(format!("zygo decision: commits left out: git log {}", e.reason())),
    };
    Ok(out
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(h, s)| (h.to_string(), s.to_string()))
        .collect())
}

/// Whether HEAD of `top` has no commit yet: `top` is a repository (`git rev-parse --git-dir` succeeds) and
/// `git rev-parse --verify -q HEAD` fails. This does not depend on the wording of git's messages, which changes
/// between versions; a missing directory or a non-repository fails the first call, so its error is kept.
fn unborn_head(top: &Path, deadline: Instant) -> bool {
    git_run(top, &["rev-parse", "--git-dir"], deadline).is_ok()
        && git_run(top, &["rev-parse", "--verify", "-q", "HEAD"], deadline).is_err()
}

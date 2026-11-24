//! The commits that name a record in a `Zygo-Decision:` trailer (D§6.7).
use std::path::Path;
use std::time::Instant;

use super::record::valid_id;
use crate::repo::{git_run, BOUND};

/// The newest `limit` commits of `top` with a line `Zygo-Decision: <id>` in the message: `(full id, subject)`, newest
/// first. The `git log` runs under the 2 s bound; `Err` is the stderr line that says why the commits are left out.
pub fn trailer_commits(top: &Path, id: &str, limit: usize) -> Result<Vec<(String, String)>, String> {
    if !valid_id(id) {
        return Err("zygo decision: commits left out: bad id".to_string());
    }
    let n = limit.to_string();
    let grep = format!("--grep=^Zygo-Decision: {id}$");
    let out = git_run(top, &["log", "-n", &n, &grep, "--format=%H%x09%s", "--"], Instant::now() + BOUND)
        .map_err(|e| format!("zygo decision: commits left out: git log {}", e.reason()))?;
    Ok(out
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(h, s)| (h.to_string(), s.to_string()))
        .collect())
}

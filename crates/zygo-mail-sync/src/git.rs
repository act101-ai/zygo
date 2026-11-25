//! The git calls of the mail sync: the hardened builder plus the maintenance overrides (S-C4), run under the
//! bounded runner with a deadline (S§8.7).

use std::path::Path;
use std::process::Command;
use std::time::Instant;
use zygo_work::repo::{run_stream, run_whole, Flow, RunError, Streamed, Whole};

/// S-C4: `git_command` in `cwd` plus `-c gc.auto=0 -c maintenance.auto=false`.
pub fn sync_git(cwd: &Path) -> Command {
    let mut c = zygo_plugins::sync::git_command(cwd);
    c.args(["-c", "gc.auto=0", "-c", "maintenance.auto=false"]);
    c
}

/// S§8.7: `git <args>` in `cwd` through `sync_git`, whole mode, killed at `deadline`.
pub fn run(cwd: &Path, args: &[&str], deadline: Instant) -> Result<Whole, RunError> {
    let mut c = sync_git(cwd);
    c.args(args);
    run_whole(c, deadline)
}

/// S§8.7: `git <args>` in `cwd` through `sync_git`, stream mode: `each` gets each NUL-ended entry of stdout.
pub fn stream(cwd: &Path, args: &[&str], deadline: Instant, each: &mut dyn FnMut(&[u8]) -> Flow)
    -> Result<Streamed, RunError> {
    let mut c = sync_git(cwd);
    c.args(args);
    run_stream(c, deadline, each)
}

/// S§8.7: true when some non-empty needle occurs in `hay` under ASCII case folding (git runs with `LC_ALL=C`).
/// `hay` need not be UTF-8; an empty needle matches nothing.
pub fn has_text(hay: &[u8], needles: &[&str]) -> bool {
    needles.iter().map(|n| n.as_bytes()).filter(|n| !n.is_empty())
        .any(|n| hay.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n)))
}

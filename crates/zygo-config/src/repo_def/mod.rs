//! The workspace definition `.zygo/`: read from the git tree (R§6.1, R§7.4), parsed into typed shapes with the
//! allow-list validation (R§6.2-R§6.4), digested (R§6.5), the approval store (R§7.2), the check at each use
//! (R§7.3, R§7.4), the repository loadout (R§9.1-R§9.2), and its addition to the merged config (R§8).

mod apply;
mod check;
mod digest;
mod loadout;
mod read;
mod shape;
mod store;

pub use apply::{apply_to_config, Applied};
pub use check::{check_default, check_pinned, Pin, PinnedUse, Use};
pub use digest::{digest, short};
pub use loadout::{repository_loadout, REPOSITORY_LOADOUT};
pub use read::{
    read_at, read_default, Files, GitRunner, Read, SystemGit, LISTING_BYTES_MAX, LISTING_ENTRIES_MAX, PROFILES_MAX, PROFILE_MAX,
    WORKSPACE_MAX,
};
pub use shape::{parse, Definition, MemberDef, RepoProfile, WorkspaceFile};
pub use store::{approve_workspace_in, is_workspace_approved_in, Stored};

/// The environment variable of the test tripwire: the file that [`trap`] appends to.
#[cfg(feature = "test-hooks")]
pub const TRAP_ENV: &str = "ZYGO_TEST_TRAP_REPO_DEF";

/// The test tripwire (WD-19 review round 1): with the `test-hooks` feature and `ZYGO_TEST_TRAP_REPO_DEF` set, each
/// public entry that reads git objects or `.zygo/` (`read_default`, `read_at`, `check_default`, `check_pinned`,
/// `SystemGit::git`) appends the line `repo_def:<entry>` to the file it names, then continues. A write error is
/// ignored. A child process inherits the variable, so a detached child's read is written too. Without the feature it
/// does nothing.
#[cfg(feature = "test-hooks")]
fn trap(entry: &str) {
    use std::io::Write;
    let Some(file) = std::env::var_os(TRAP_ENV).filter(|v| !v.is_empty()) else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(f, "repo_def:{entry}");
    }
}

#[cfg(not(feature = "test-hooks"))]
#[inline(always)]
fn trap(_entry: &str) {}

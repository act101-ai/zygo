//! The workspace definition `.zygo/`: read from the git tree (R§6.1, R§7.4), parsed into typed shapes with the
//! allow-list validation (R§6.2-R§6.4), digested (R§6.5), the approval store (R§7.2), the check at each use
//! (R§7.3, R§7.4), and the repository loadout (R§9.1-R§9.2).

mod check;
mod digest;
mod loadout;
mod read;
mod shape;
mod store;

pub use check::{check_default, check_pinned, Pin, PinnedUse, Use};
pub use digest::{digest, short};
pub use loadout::{repository_loadout, REPOSITORY_LOADOUT};
pub use read::{read_at, read_default, Files, GitRunner, Read, SystemGit, PROFILES_MAX, PROFILE_MAX, WORKSPACE_MAX};
pub use shape::{parse, Definition, MemberDef, RepoProfile, WorkspaceFile};
pub use store::{approve_workspace_in, is_workspace_approved_in, Stored};

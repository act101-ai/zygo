//! The workspace definition `.zygo/`: read from the git tree (R§6.1, R§7.4), parsed into typed shapes with the
//! allow-list validation (R§6.2-R§6.4) and digested (R§6.5).

mod digest;
mod read;
mod shape;

pub use digest::{digest, short};
pub use read::{read_at, read_default, Files, GitRunner, Read, SystemGit, PROFILES_MAX, PROFILE_MAX, WORKSPACE_MAX};
pub use shape::{parse, Definition, MemberDef, RepoProfile, WorkspaceFile};

//! The workspace definition `.zygo/`, read from the git tree (R§6.1, R§7.4).

mod read;

pub use read::{read_at, read_default, Files, GitRunner, Read, SystemGit, PROFILES_MAX, PROFILE_MAX, WORKSPACE_MAX};

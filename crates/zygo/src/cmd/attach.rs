//! `zygo attach [<name>]` (W§12).

use zygo_core::paths::Paths;
use zygo_workspace::up;

use super::{cwd_or_exit, op5_exit, tmux_program};

#[derive(clap::Args)]
pub struct AttachArgs {
    /// Workspace name; default: the workspace of this folder
    pub name: Option<String>,
}

/// The OP5 check, the current folder (D10), then `up::attach`.
pub fn run(a: &AttachArgs) -> i32 {
    if let Some(code) = op5_exit() {
        return code;
    }
    let cwd = match cwd_or_exit() {
        Ok(d) => d,
        Err(code) => return code,
    };
    up::attach(&Paths::from_env(), &tmux_program(), std::env::var_os("TMUX").as_deref(), a.name.as_deref(), &cwd)
}

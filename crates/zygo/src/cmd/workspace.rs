//! `zygo workspace` and `/zygo:workspace` (W§12): the status of this session's or this folder's workspace.

use anyhow::anyhow;
use zygo_workspace::admin;

use super::verbs::VerbCtx;
use super::{cwd_error, op5, UsageError};

/// The OP5 check, then (D6) no `--save` and no argument, then `admin::status` with the current folder (D10) and
/// `ZYGO_WORKSPACE`.
pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    op5()?;
    if ctx.save || !args.is_empty() {
        return Err(UsageError("workspace takes no arguments".into()).into());
    }
    let cwd = std::env::current_dir().map_err(|e| anyhow!(cwd_error(&e)))?;
    admin::status(&ctx.paths, &cwd, std::env::var_os("ZYGO_WORKSPACE").as_deref())
}

#[cfg(all(test, windows))]
mod op5_slash_workspace {
    use super::verb;
    use crate::cmd::UsageError;

    #[test]
    fn op5_slash_workspace() {
        let d = tempfile::TempDir::new().unwrap();
        let mut c = crate::cmd::compact::tests::ctx(d.path(), None);
        c.slash = true;
        let e = verb(&c, &[]).unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some());
        assert_eq!(e.to_string(), "workspaces need tmux; run zygo in WSL");
    }
}

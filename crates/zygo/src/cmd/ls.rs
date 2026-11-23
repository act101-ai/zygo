//! `zygo ls` and `/zygo:ls` (W§12): one line per workspace, in the order of `admin::list`.

use zygo_workspace::admin;

use super::verbs::VerbCtx;
use super::{op5, tmux_program, UsageError};

/// The OP5 check, then (D6) no `--save` and no argument, then each `ListRow` with its `Display`, joined by `\n`, or
/// `no workspaces`. `admin::list` handles a missing tmux itself and never fails.
pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    op5()?;
    if ctx.save || !args.is_empty() {
        return Err(UsageError("ls takes no arguments".into()).into());
    }
    let rows = admin::list(&ctx.paths, &tmux_program());
    if rows.is_empty() {
        return Ok("no workspaces".to_string());
    }
    Ok(rows.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))
}

#[cfg(all(test, windows))]
mod op5_slash_ls {
    use super::verb;
    use crate::cmd::UsageError;

    #[test]
    fn op5_slash_ls() {
        let d = tempfile::TempDir::new().unwrap();
        let mut c = crate::cmd::compact::tests::ctx(d.path(), None);
        c.slash = true;
        let e = verb(&c, &[]).unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some());
        assert_eq!(e.to_string(), "workspaces need tmux; run zygo in WSL");
    }
}

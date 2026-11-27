//! `zygo workspace` and `/zygo:workspace` (W§12): the status of this session's or this folder's workspace; `zygo
//! workspace approve` (workspace definition design R§7.1): approve this repository's workspace files on this machine.

use std::io::{BufRead, Write};

use anyhow::anyhow;
use zygo_workspace::admin;

use super::verbs::VerbCtx;
use super::{cwd_error, op5, UsageError};

/// `approve` first, before the OP5 check (R§7.1 step 1: it runs on native Windows); else the OP5 check, then (D6) no
/// `--save` and no argument, then `admin::status` with the current folder (D10) and `ZYGO_WORKSPACE`.
pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    if args.first().is_some_and(|a| a == "approve") {
        return approve(ctx, &args[1..], &mut std::io::stdin().lock(), &mut std::io::stderr());
    }
    op5()?;
    if ctx.save || !args.is_empty() {
        return Err(UsageError("workspace takes no arguments".into()).into());
    }
    let cwd = std::env::current_dir().map_err(|e| anyhow!(cwd_error(&e)))?;
    admin::status(&ctx.paths, &cwd, std::env::var_os("ZYGO_WORKSPACE").as_deref())
}

/// The reply of `/zygo:workspace approve`, for every argument list (WD-15 plan call 2, the DR-3 form).
const SLASH_REPLY: &str = "run in your shell: zygo workspace approve";

/// `zygo workspace approve [--yes] [--digest <hex>]` (R§7.1). `args` are the tokens after `approve`. The slash form
/// returns `run in your shell: zygo workspace approve` and reads and writes nothing (R§7.1 step 2, R-C8); the CLI form
/// asks on `prompt` and reads the answer from `input`.
pub fn approve(ctx: &VerbCtx, args: &[String], input: &mut dyn BufRead, prompt: &mut dyn Write) -> anyhow::Result<String> {
    if ctx.slash {
        return Ok(SLASH_REPLY.into());
    }
    super::workspace_approve::run(ctx, args, input, prompt)
}

/// The R§7.1 step 6 listing of a file's bytes: the bytes as UTF-8 (an invalid sequence as U+FFFD), one line per `\n`
/// (a final `\n` adds no empty line); in each line printable ASCII (`0x20`-`0x7e`) is kept, a tab is `\t` and every
/// other character is `\u{<hex>}` in lower-case hex with no leading zero (R-C7, review focus 36).
pub fn listing(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.strip_suffix('\n').unwrap_or(&text).split('\n').map(line).collect::<Vec<_>>().join("\n")
}

/// One line of `listing`: the character rule, with no newline in `s` expected (a newline is `\u{a}`).
pub(crate) fn line(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' '..='~' => o.push(c),
            '\t' => o.push_str("\\t"),
            c => o.push_str(&format!("\\u{{{:x}}}", c as u32)),
        }
    }
    o
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

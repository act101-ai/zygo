//! `zygo down [--now] [--timeout <dur>] [--wait] [--all] [--] [<name>]` and `/zygo:down [--now]` (W§12, W§9.3).

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::anyhow;
use zygo_config::display::EscapedLines;
use zygo_config::Config;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_workspace::down::{self, DownOpts, DownTarget};

use super::verbs::VerbCtx;
use super::{
    cwd_error, cwd_or_exit, exe_error, exe_or_exit, op5, op5_exit, print_err, tmux_program, usable_session_id, user_config,
    user_config_or_exit, UsageError,
};

#[derive(clap::Args)]
pub struct DownArgs {
    /// Stop at once: no drain
    #[arg(long)]
    pub now: bool,
    /// The drain timeout: <n>s, <n>m or <n>h
    #[arg(long, value_name = "DUR")]
    pub timeout: Option<String>,
    /// Wait in the foreground until the down ends
    #[arg(long)]
    pub wait: bool,
    /// Stop every workspace that is up
    #[arg(long, conflicts_with = "name")]
    pub all: bool,
    /// Workspace name; default: the workspace of this folder
    pub name: Option<String>,
}

/// The CLI form: the OP5 check, the `--timeout` value (D4), the user config (D3), the folder and the binary (D10),
/// then `down::down`. The target is `--all`, else the name, else the folder; no environment overrides it.
pub fn run(a: &DownArgs) -> i32 {
    if let Some(code) = op5_exit() {
        return code;
    }
    let timeout = match a.timeout.as_deref() {
        None => None,
        Some(v) => match zygo_config::parse_duration(v) {
            Some(d) => Some(d),
            None => {
                print_err(&format!("zygo: bad --timeout value {}: use <n>s, <n>m or <n>h\n", zygo_core::esc(v)));
                return 2;
            }
        },
    };
    let paths = Paths::from_env();
    let config = match user_config_or_exit(&paths, "down") {
        Ok(c) => c,
        Err(code) => return code,
    };
    let cwd = match cwd_or_exit() {
        Ok(d) => d,
        Err(code) => return code,
    };
    let zygo = match exe_or_exit() {
        Ok(p) => p,
        Err(code) => return code,
    };
    down::down(&cli_opts(a, timeout, cwd, zygo, paths, config, &|k| std::env::var_os(k)))
}

/// The `DownOpts` of the CLI form; `env` gives `TMUX`, `TMUX_PANE` and `ZYGO_SESSION` (`std::env::var_os` outside
/// tests). An empty `ZYGO_SESSION` is no session id (D8), so it also does not detach.
fn cli_opts(
    a: &DownArgs,
    timeout: Option<Duration>,
    cwd: PathBuf,
    zygo: PathBuf,
    paths: Paths,
    config: Config,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> DownOpts {
    let target = if a.all {
        DownTarget::All
    } else {
        match &a.name {
            Some(n) => DownTarget::Name(n.clone()),
            None => DownTarget::Cwd,
        }
    };
    DownOpts {
        target,
        now: a.now,
        timeout,
        wait: a.wait,
        cwd,
        zygo,
        tmux: tmux_program(),
        tmux_env: env("TMUX"),
        tmux_pane: env("TMUX_PANE"),
        zygo_session: usable_session_id(env),
        proc_probe: down::proc_identity,
        paths,
        config,
    }
}

/// `/zygo:down [--now]` (D5, OP11, AU18): the workspace of this session's membership marker, never the cwd. The down
/// runs as a detached child, so the prompt hook's bound never ends it partway. `down::run` writes to two buffers,
/// never to the hook's stdout; the reply is their text without the `zygo: ` prefix of each line.
pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    op5()?;
    if ctx.save || args.iter().any(|a| a != "--now") {
        return Err(UsageError("usage: /zygo:down [--now]".into()).into());
    }
    let now = args.iter().any(|a| a == "--now");
    let cwd = std::env::current_dir().map_err(|e| anyhow!(cwd_error(&e)))?;
    let zygo = std::env::current_exe().map_err(|e| anyhow!(exe_error(&e)))?;
    let config = user_config(&ctx.paths).map_err(|e| anyhow!("down: {}", zygo_core::esc(&e.to_string())))?;
    let opts = slash_opts(ctx, now, cwd, zygo, config, &|k| std::env::var_os(k));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = down::run(&opts, &mut out, &mut err);
    let text = reply(&String::from_utf8_lossy(&out), &String::from_utf8_lossy(&err));
    if code == 0 { Ok(text) } else { Err(EscapedLines::new(text.lines()).into()) }
}

/// The `DownOpts` of the slash form (D5); `env` gives `TMUX`, `TMUX_PANE` and `ZYGO_SESSION` (`std::env::var_os`
/// outside tests). The target is the marker of the session folder (`None` without one), never the cwd; `wait` is
/// always `false` and `timeout` is `None`.
fn slash_opts(ctx: &VerbCtx, now: bool, cwd: PathBuf, zygo: PathBuf, config: Config, env: &dyn Fn(&str) -> Option<OsString>) -> DownOpts {
    let sd = ctx.session_dir.as_ref();
    let target = DownTarget::Session(sd.and_then(|d| SessionView::load(d).ok()).and_then(|v| v.workspace).map(|w| w.member.ws_id));
    DownOpts {
        target,
        now,
        timeout: None,
        wait: false,
        cwd,
        zygo,
        tmux: tmux_program(),
        tmux_env: env("TMUX"),
        tmux_pane: env("TMUX_PANE"),
        zygo_session: session_of(env, sd),
        proc_probe: down::proc_identity,
        paths: ctx.paths.clone(),
        config,
    }
}

/// The `zygo_session` of the slash form (D5, D8): `ZYGO_SESSION` when it is not empty, else the session id of the
/// session folder when that is not empty, else `None`.
fn session_of(env: &dyn Fn(&str) -> Option<OsString>, sd: Option<&SessionDir>) -> Option<OsString> {
    usable_session_id(env).or_else(|| sd.map(zygo_work::hooks::session_id).filter(|id| !id.is_empty()).map(OsString::from))
}

/// The D5 reply: `out`, then `err` (a `\n` between them when `out` does not end in one), trailing white space removed,
/// and a leading `zygo: ` removed from each line (the hook adds its own prefix).
fn reply(out: &str, err: &str) -> String {
    let mut all = out.to_string();
    if !all.is_empty() && !all.ends_with('\n') && !err.is_empty() {
        all.push('\n');
    }
    all.push_str(err);
    all.trim_end().lines().map(|l| l.strip_prefix("zygo: ").unwrap_or(l)).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{cli_opts, reply, session_of, slash_opts, DownArgs};
    use zygo_core::session_dir::SessionDir;
    use zygo_workspace::down::DownTarget;

    /// `env` with `ZYGO_SESSION` set to `v` and no other variable.
    fn with(v: &'static str) -> impl Fn(&str) -> Option<OsString> {
        move |k: &str| if k == "ZYGO_SESSION" { Some(OsString::from(v)) } else { None }
    }

    fn unset(_: &str) -> Option<OsString> {
        None
    }

    /// D5, D8 (F18): an empty `ZYGO_SESSION` counts as unset, so the session folder gives the id; an empty
    /// `session_id(sd)` also counts as no id. Kills: `session_of` passing `Some("")`.
    #[test]
    fn slash_down_counts_an_empty_zygo_session_as_unset() {
        let t = tempfile::tempdir().unwrap();
        let sd = SessionDir::new(t.path().join("s01"));
        assert_eq!(session_of(&with(""), Some(&sd)), Some("s01".into()));
        assert_eq!(session_of(&unset, Some(&sd)), Some("s01".into()));
        assert_eq!(session_of(&with("x"), Some(&sd)), Some("x".into()));
        assert_eq!(session_of(&with(""), None), None);
        // no view and no folder name: `session_id` gives ""
        assert_eq!(session_of(&unset, Some(&SessionDir::new(PathBuf::from("/")))), None);
    }

    /// D5 (F18): the slash form never waits, with or without a session folder. Kills: `wait` taken from anywhere but
    /// a fixed `false`.
    #[test]
    fn slash_down_always_runs_detached() {
        let t = tempfile::tempdir().unwrap();
        let mut ctx = crate::cmd::compact::tests::ctx(t.path(), None);
        ctx.slash = true;
        let make = |ctx: &crate::cmd::verbs::VerbCtx, now: bool| {
            slash_opts(ctx, now, t.path().into(), t.path().join("zygo"), zygo_config::Config::default(), &unset)
        };
        let o = make(&ctx, false);
        assert!(!o.wait);
        assert_eq!(o.timeout, None);
        assert_eq!(o.zygo_session, None);
        assert!(matches!(o.target, DownTarget::Session(None)));
        ctx.session_dir = Some(SessionDir::new(t.path().join("s01")));
        let o = make(&ctx, false);
        assert!(!o.wait);
        assert_eq!(o.zygo_session, Some("s01".into()));
        assert!(matches!(o.target, DownTarget::Session(None)), "no view file: no marker");
        let o = make(&ctx, true);
        assert!(o.now && !o.wait);
    }

    /// D8: an empty `ZYGO_SESSION` counts as unset in the CLI form too. Kills: the CLI form passing `Some("")`.
    #[test]
    fn cli_down_counts_an_empty_zygo_session_as_unset() {
        let t = tempfile::tempdir().unwrap();
        let a = DownArgs { now: false, timeout: None, wait: false, all: false, name: None };
        let make = |env: &dyn Fn(&str) -> Option<OsString>| {
            let paths = crate::cmd::compact::tests::paths(t.path());
            cli_opts(&a, None, t.path().into(), t.path().join("zygo"), paths, zygo_config::Config::default(), env)
        };
        assert_eq!(make(&with("")).zygo_session, None);
        assert_eq!(make(&unset).zygo_session, None);
        assert_eq!(make(&with("s01")).zygo_session, Some("s01".into()));
    }

    #[test]
    fn reply_joins_out_and_err_and_drops_the_prefix() {
        assert_eq!(reply("zygo: a\n", "zygo: b\n"), "a\nb");
        assert_eq!(reply("a", "b"), "a\nb");
        assert_eq!(reply("", "zygo: b\n\n"), "b");
        assert_eq!(reply("x zygo: y\n", ""), "x zygo: y");
    }
}

#[cfg(all(test, windows))]
mod op5_slash_down {
    use super::verb;
    use crate::cmd::UsageError;

    #[test]
    fn op5_slash_down() {
        let d = tempfile::TempDir::new().unwrap();
        let mut c = crate::cmd::compact::tests::ctx(d.path(), None);
        c.slash = true;
        let e = verb(&c, &[]).unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some());
        assert_eq!(e.to_string(), "workspaces need tmux; run zygo in WSL");
    }
}

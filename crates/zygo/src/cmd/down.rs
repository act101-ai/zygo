//! `zygo down [--now] [--timeout <dur>] [--wait] [--all] [--] [<name>]` and `/zygo:down [--now]` (W§12, W§9.3).

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::anyhow;
use zygo_config::display::EscapedLines;
use zygo_config::Config;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
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
    /// The drain timeout: <n>s, <n>m, <n>h or <n>d
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
                print_err(&format!("zygo: bad --timeout value {}: use <n>s, <n>m, <n>h or <n>d\n", zygo_core::esc(v)));
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
/// runs as a detached child when it has a session id (`session_of`: a `ZYGO_SESSION` that is not empty, else the
/// view's `session` when it is not empty, else the session folder's name when it is not empty), or when `$TMUX_PANE`
/// is a pane of the target workspace session; then the prompt hook's bound never ends it partway. With neither, it
/// runs in the foreground. `down::run` writes to two buffers, never to the hook's stdout; the reply is their text
/// without the `zygo: ` prefix of each line. The current folder is read only when `current_exe()` is relative: the
/// target never comes from it, and the library uses it only to make a relative zygo path absolute (D10).
pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    op5()?;
    if ctx.save || args.iter().any(|a| a != "--now") {
        return Err(UsageError(format!("usage: /zygo:down {}", zygo_core::verbs::DOWN_SLASH_ARGS)).into());
    }
    let now = args.iter().any(|a| a == "--now");
    let zygo = std::env::current_exe().map_err(|e| anyhow!(exe_error(&e)))?;
    // An absolute zygo path needs no folder; the library reads `cwd` for nothing else on a `Session` target.
    let cwd = if zygo.is_absolute() { PathBuf::new() } else { std::env::current_dir().map_err(|e| anyhow!(cwd_error(&e)))? };
    let config = user_config(&ctx.paths).map_err(|e| anyhow!("down: {}", zygo_core::esc(&e.to_string())))?;
    let opts = slash_opts(ctx, now, cwd, zygo, config, &|k| std::env::var_os(k));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = down::run(&opts, &mut out, &mut err);
    let text = reply(&String::from_utf8_lossy(&out), &String::from_utf8_lossy(&err));
    if code == 0 { Ok(text) } else { Err(EscapedLines::new(text.lines()).into()) }
}

/// The `DownOpts` of the slash form (D5); `env` gives `TMUX`, `TMUX_PANE`, `ZYGO_SESSION` and `ZYGO_WORKSPACE`
/// (`std::env::var_os` outside tests). The target is the marker of the session folder read by the hook rule of the
/// mirror (`verb_member`, AU105: no `ZYGO_WORKSPACE`, or a view that is a link, is no marker; `None` without one),
/// never the cwd; `wait` is always `false` and `timeout` is `None`.
fn slash_opts(ctx: &VerbCtx, now: bool, cwd: PathBuf, zygo: PathBuf, config: Config, env: &dyn Fn(&str) -> Option<OsString>) -> DownOpts {
    let sd = ctx.session_dir.as_ref();
    let target = DownTarget::Session(sd.and_then(|d| zygo_hooks::workspace::verb_member(d, &ctx.paths, Some(env))).map(|m| m.ws_id));
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

/// The `zygo_session` of the slash form (D5, D8): `ZYGO_SESSION` when it is not empty, else `session_id(sd)` (the
/// view's `session`, or the folder name when `view.json` does not load) when that is not empty, else the session
/// folder's name when that is not empty (a view whose `session` is `""`), else `None`.
fn session_of(env: &dyn Fn(&str) -> Option<OsString>, sd: Option<&SessionDir>) -> Option<OsString> {
    let folder_name = |d: &SessionDir| d.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    usable_session_id(env).or_else(|| {
        let d = sd?;
        let id = zygo_work::hooks::session_id(d);
        let id = if id.is_empty() { folder_name(d) } else { id };
        (!id.is_empty()).then(|| OsString::from(id))
    })
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
    use std::path::{Path, PathBuf};

    use super::{cli_opts, reply, session_of, slash_opts, DownArgs};
    use zygo_core::session_dir::SessionDir;
    use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
    use zygo_workspace::down::DownTarget;

    /// The session folder `<t>/<name>` with a `view.json` whose `session` is `session` and, for `Some(id)`, a
    /// membership marker of the workspace `id`.
    fn view_dir(t: &Path, name: &str, session: &str, ws_id: Option<&str>) -> SessionDir {
        let sd = SessionDir::new(t.join(name));
        std::fs::create_dir_all(&sd.root).unwrap();
        let workspace = ws_id.map(|id| ViewWorkspace {
            member: Member { ws_id: id.into(), pane: "kaaaaaaaa".into(), ..Default::default() },
            ..Default::default()
        });
        SessionView { session: session.into(), workspace, ..Default::default() }.save(&sd).unwrap();
        sd
    }

    /// `env` with `ZYGO_SESSION` set to `v` and no other variable.
    fn with(v: &'static str) -> impl Fn(&str) -> Option<OsString> {
        move |k: &str| if k == "ZYGO_SESSION" { Some(OsString::from(v)) } else { None }
    }

    fn unset(_: &str) -> Option<OsString> {
        None
    }

    /// `env` with the slash gate of a member (`ZYGO_WORKSPACE`) set and no other variable.
    fn member_env(k: &str) -> Option<OsString> {
        (k == "ZYGO_WORKSPACE").then(|| OsString::from("0123456789abcdef"))
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

    /// D5 (review round 1, fix 2): the target is the `ws_id` of the membership marker in `view.json`, and the slash
    /// form does not wait. Kills: a `target` that is always `Session(None)`.
    #[test]
    fn slash_down_targets_the_workspace_of_the_marker() {
        let t = tempfile::tempdir().unwrap();
        let mut ctx = crate::cmd::compact::tests::ctx(t.path(), None);
        ctx.slash = true;
        ctx.session_dir = Some(view_dir(t.path(), "s01", "s01", Some("0123456789abcdef")));
        let o = slash_opts(&ctx, false, t.path().into(), t.path().join("zygo"), zygo_config::Config::default(), &member_env);
        assert_eq!(o.target, DownTarget::Session(Some("0123456789abcdef".into())));
        assert!(!o.wait);
        assert_eq!(o.zygo_session, Some("s01".into()));
    }

    /// M4, decision 2026-11-23: `/zygo:down` reads its marker by the rule of the mirror (`verb_member`): no
    /// `ZYGO_WORKSPACE`, or a `view.json` that is a link, is no marker.
    #[cfg(unix)]
    #[test]
    fn slash_down_reads_the_marker_by_the_hook_rule() {
        let t = tempfile::tempdir().unwrap();
        let mut ctx = crate::cmd::compact::tests::ctx(t.path(), None);
        ctx.slash = true;
        ctx.session_dir = Some(view_dir(t.path(), "s01", "s01", Some("0123456789abcdef")));
        let make = |ctx: &crate::cmd::verbs::VerbCtx, env: &dyn Fn(&str) -> Option<OsString>| {
            slash_opts(ctx, false, t.path().into(), t.path().join("zygo"), zygo_config::Config::default(), env)
        };
        assert_eq!(make(&ctx, &unset).target, DownTarget::Session(None), "no gate");
        let real = view_dir(t.path(), "real", "s02", Some("0123456789abcdef"));
        let linked = SessionDir::new(t.path().join("s02"));
        std::fs::create_dir_all(&linked.root).unwrap();
        std::os::unix::fs::symlink(real.view_json(), linked.view_json()).unwrap();
        ctx.session_dir = Some(linked);
        assert_eq!(make(&ctx, &member_env).target, DownTarget::Session(None), "linked view");
    }

    /// D5 (review round 1, fix 5): a `view.json` whose `session` is empty gives the session folder's name, so the
    /// library still detaches; a `session` that is not empty wins over the folder name. Kills: a `session_of` that
    /// gives `None` for an empty view `session` (the down then runs in the foreground of the prompt hook when no pane
    /// matches).
    #[test]
    fn slash_down_takes_the_folder_name_when_the_view_session_is_empty() {
        let t = tempfile::tempdir().unwrap();
        let sd = view_dir(t.path(), "s02", "", Some("0123456789abcdef"));
        assert_eq!(session_of(&unset, Some(&sd)), Some("s02".into()));
        assert_eq!(session_of(&with(""), Some(&sd)), Some("s02".into()));
        assert_eq!(session_of(&with("x"), Some(&sd)), Some("x".into()));
        let sd = view_dir(t.path(), "s03", "v03", None);
        assert_eq!(session_of(&unset, Some(&sd)), Some("v03".into()));
    }

    /// D10 (review round 1, fix 6): `/zygo:down` reads the current folder only to make a relative zygo path absolute,
    /// and `current_exe()` is absolute, so a removed current folder does not fail it. The child test changes the cwd,
    /// so it runs in its own process. Kills: a `verb` that reads `current_dir()` on every path.
    #[cfg(unix)]
    #[test]
    fn slash_down_does_not_need_the_current_folder() {
        let o = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cmd::down::tests::slash_down_in_a_removed_folder", "--ignored", "--nocapture", "--test-threads=1"])
            .env("ZYGO_TEST_CHILD", "cmd::down::tests::slash_down_in_a_removed_folder")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        assert!(o.status.success(), "{text}");
        assert!(text.contains("test result: ok. 1 passed"), "{text}");
        assert!(text.contains("child ran: slash_down_in_a_removed_folder"), "{text}");
    }

    /// The child of `slash_down_does_not_need_the_current_folder`: its cwd is a folder that it removed. Outside its
    /// parent (`--include-ignored`) it does nothing.
    #[cfg(unix)]
    #[test]
    #[ignore = "child process of slash_down_does_not_need_the_current_folder"]
    fn slash_down_in_a_removed_folder() {
        if std::env::var("ZYGO_TEST_CHILD").as_deref() != Ok("cmd::down::tests::slash_down_in_a_removed_folder") {
            return;
        }
        let t = tempfile::tempdir().unwrap();
        let gone = t.path().join("gone");
        std::fs::create_dir(&gone).unwrap();
        std::env::set_current_dir(&gone).unwrap();
        std::fs::remove_dir(&gone).unwrap();
        assert!(std::env::current_dir().is_err(), "the current folder is still readable");
        let mut ctx = crate::cmd::compact::tests::ctx(t.path(), None);
        ctx.slash = true;
        let e = super::verb(&ctx, &[]).unwrap_err();
        assert_eq!(zygo_config::display::error_text(&e), "this session is not in a workspace");
        println!("child ran: slash_down_in_a_removed_folder");
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

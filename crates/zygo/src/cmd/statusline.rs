//! `zygo statusline [compact|verbose]` and `/zygo:statusline` (statusline HUD design H§8, AU1): session, `--save`.
//!
//! With a style, the verb changes `overrides.json` through the shared session writer (`set_session`), so the next
//! render of the HUD uses it; `--save` also writes `[statusline] style` to user config. With no argument it prints
//! the current style and its source. The verb is the same in a Claude Code session and in an OpenCode session.

use zygo_config::{Layer, Merged};
use zygo_core::overrides::Overrides;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_hooks::statusline::render::Style;

use super::verbs::{base, set_session, SessionChange, Setting, VerbCtx};
use super::UsageError;

/// The word of a style, as the verb, `overrides.json`, `view.json` and user config write it.
pub fn name(style: Style) -> &'static str {
    match style {
        Style::Compact => "compact",
        Style::Verbose => "verbose",
    }
}

/// The style of a session and its source, by the rule of the hook (`zygo_hooks::statusline::style`): `session` is
/// `overrides.json`, `launch` is `view.json` (the value the launcher wrote), else `default`.
fn of_session(dir: &SessionDir) -> (Style, &'static str) {
    let overrides = Overrides::load(dir);
    let view = SessionView::load(dir).ok();
    let source = if overrides.statusline.as_deref().and_then(Style::parse).is_some() {
        "session"
    } else if view.as_ref().and_then(|v| Style::parse(&v.statusline_style)).is_some() {
        "launch"
    } else {
        "default"
    };
    (zygo_hooks::statusline::style(&overrides, view.as_ref()), source)
}

/// The style that the next launch uses and its source: `user config`, else `default`. A value that is no style
/// counts as not set (`config check` reports it).
fn of_config(m: &Merged) -> (Style, &'static str) {
    match Style::parse(&m.config.statusline.style) {
        Some(style) if m.source_of("statusline.style") == Layer::User => (style, "user config"),
        Some(style) => (style, "default"),
        None => (Style::default(), "default"),
    }
}

/// The current style and its source: in a session, the session's; outside, the config's (`None` without a config).
pub fn current(ctx: &VerbCtx, m: Option<&Merged>) -> Option<(Style, &'static str)> {
    match &ctx.session_dir {
        Some(dir) => Some(of_session(dir)),
        None => m.map(of_config),
    }
}

pub fn run(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] => {
            // A session needs no config for the read form: its files hold the style.
            let (style, source) = match &ctx.session_dir {
                Some(dir) => of_session(dir),
                None => of_config(&base(ctx, "statusline", None, None)?),
            };
            Ok(format!("statusline {} ({source})", name(style)))
        }
        [word] => {
            let Some(style) = Style::parse(word) else { return Err(usage().into()) };
            // `[statusline]` is user config only (H§8), so the team layer is not read.
            let m = base(ctx, "statusline", None, None)?;
            let change = SessionChange { verb: "statusline", words: name(style).to_string(), setting: Setting::Statusline(style) };
            set_session(ctx, &m, change)
        }
        _ => Err(usage().into()),
    }
}

fn usage() -> UsageError {
    UsageError("zygo statusline: usage: statusline [compact|verbose]".into())
}

pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    run(ctx, args)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::cmd::compact::tests::ctx;
    use crate::cmd::verbs::tests::{err_text, ov, sd, usage_text, v};

    const USAGE: &str = "zygo statusline: usage: statusline [compact|verbose]";

    fn view(dir: &std::path::Path, style: &str) {
        let view = SessionView { session: "S1".into(), statusline_style: style.into(), ..Default::default() };
        view.save(&SessionDir::new(dir.to_path_buf())).unwrap();
    }

    #[test]
    fn sets_the_session_style() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        let c = ctx(d.path(), Some(&dir));
        assert_eq!(run(&c, &v(&["verbose"])).unwrap(), "statusline verbose for this session");
        assert_eq!(ov(&dir).statusline.as_deref(), Some("verbose"));
        assert_eq!(run(&c, &v(&["compact"])).unwrap(), "statusline compact for this session");
        assert_eq!(ov(&dir).statusline.as_deref(), Some("compact"));
        assert!(!d.path().join("config.toml").exists());
    }

    #[test]
    fn save_writes_user_config() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        let saved = VerbCtx { save: true, ..ctx(d.path(), Some(&dir)) };
        assert_eq!(run(&saved, &v(&["verbose"])).unwrap(), "statusline verbose for this session; saved to user config");
        assert_eq!(ov(&dir).statusline.as_deref(), Some("verbose"));
        assert_eq!(std::fs::read_to_string(d.path().join("config.toml")).unwrap(), "[statusline]\nstyle = \"verbose\"\n");
        let outside = VerbCtx { save: true, ..ctx(d.path(), None) };
        assert_eq!(run(&outside, &v(&["compact"])).unwrap(), "statusline compact saved to user config");
        assert_eq!(std::fs::read_to_string(d.path().join("config.toml")).unwrap(), "[statusline]\nstyle = \"compact\"\n");
        assert_eq!(ov(&dir).statusline.as_deref(), Some("verbose"), "a save outside a session leaves the session");
    }

    #[test]
    fn usage_errors_change_nothing() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        for c in [ctx(d.path(), Some(&dir)), ctx(d.path(), None), VerbCtx { save: true, ..ctx(d.path(), None) }] {
            for a in [&["wide"][..], &["Verbose"], &["verbose", "x"], &[""], &["compact", "verbose"]] {
                assert_eq!(usage_text(run(&c, &v(a))), USAGE, "{a:?}");
            }
        }
        assert_eq!(
            usage_text(run(&ctx(d.path(), None), &v(&["verbose"]))),
            "zygo statusline: not in a zygo session; add --save to change user config"
        );
        assert!(!dir.join("overrides.json").exists());
        assert!(!d.path().join("config.toml").exists());
    }

    #[test]
    fn shows_the_style_and_its_source() {
        let d = TempDir::new().unwrap();
        let outside = ctx(d.path(), None);
        assert_eq!(run(&outside, &[]).unwrap(), "statusline compact (default)");
        std::fs::write(d.path().join("config.toml"), "[statusline]\nstyle = \"verbose\"\n").unwrap();
        assert_eq!(run(&outside, &[]).unwrap(), "statusline verbose (user config)");
        let dir = sd(d.path());
        let c = ctx(d.path(), Some(&dir));
        // The user config does not reach a running session: only its own files do (AU1).
        assert_eq!(run(&c, &[]).unwrap(), "statusline compact (default)");
        view(&dir, "verbose");
        assert_eq!(run(&c, &[]).unwrap(), "statusline verbose (launch)");
        std::fs::write(dir.join("overrides.json"), r#"{"statusline":"compact"}"#).unwrap();
        assert_eq!(run(&c, &[]).unwrap(), "statusline compact (session)");
        // A value that is no style is ignored at its layer.
        std::fs::write(dir.join("overrides.json"), r#"{"statusline":"wide"}"#).unwrap();
        assert_eq!(run(&c, &[]).unwrap(), "statusline verbose (launch)");
        view(&dir, "wide");
        assert_eq!(run(&c, &[]).unwrap(), "statusline compact (default)");
        assert_eq!(current(&c, None), Some((Style::Compact, "default")));
        assert_eq!(current(&outside, None), None);
    }

    #[test]
    fn config_that_does_not_load_stops_a_change() {
        let d = TempDir::new().unwrap();
        std::fs::write(d.path().join("config.toml"), "[statusline\n").unwrap();
        let dir = sd(d.path());
        let c = ctx(d.path(), Some(&dir));
        assert!(err_text(run(&c, &v(&["verbose"]))).starts_with("zygo statusline: "));
        assert!(!dir.join("overrides.json").exists());
        assert_eq!(run(&c, &[]).unwrap(), "statusline compact (default)");
        assert!(err_text(run(&ctx(d.path(), None), &[])).starts_with("zygo statusline: "));
    }
}

//! `zygo help [<verb>]` and `/zygo:help` (§17.1): every verb with its arguments and current value.

use std::path::Path;

use zygo_config::{Merged, GUARDS};
use zygo_core::verbs::{Scope, VerbSpec, VERB_SPECS};
use zygo_hooks::statusline::short;
use zygo_plugins::check::esc;
use zygo_proxy::client::WindowArg;

use super::status::{hide_window, merged, repo_top, session_status};
use super::verbs::VerbCtx;
use super::UsageError;

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

fn scope_text(s: Scope) -> &'static str {
    match s {
        Scope::None => "none",
        Scope::Session => "session",
        Scope::SessionSave => "session, --save",
        Scope::Repository => "repository; session text at next launch",
        Scope::NextLaunch => "next launch",
        Scope::UserConfig => "user config",
    }
}

fn usage(s: &VerbSpec) -> String {
    if s.args.is_empty() { s.name.to_string() } else { format!("{} {}", s.name, s.args) }
}

/// The current value of a verb with a setting. `compact` asks the session; C7 (`hide`) keeps its number out.
fn value(ctx: &VerbCtx, name: &str, m: Option<&Merged>, hide: bool) -> Option<String> {
    if name == "compact" {
        let s = session_status(ctx)?.ok()?;
        return match (s.compact, hide) {
            (None, true) => None,
            (Some(_), true) => Some("override set".into()),
            (None, false) => Some("on".into()),
            (Some(WindowArg::Off), false) => Some("off".into()),
            (Some(WindowArg::Tokens(n)), false) => Some(short(n)),
        };
    }
    let c = &m?.config;
    Some(match name {
        "results" => format!("{}, threshold {}", on_off(c.features.results), c.results.trim_threshold_bytes),
        "redaction" => {
            format!("secrets {}, content {}", on_off(c.features.redaction.secrets), on_off(c.features.redaction.content))
        }
        "guards" => GUARDS.iter().map(|g| format!("{g} {}", c.guards.get(g).unwrap_or_default())).collect::<Vec<_>>().join(", "),
        "units" => on_off(c.features.work_units).to_string(),
        "profile" => esc(c.user.default_profile.as_deref().unwrap_or("none")),
        "compact-tool" => on_off(c.features.compact_tool).to_string(),
        _ => return None,
    })
}

/// A config that does not load leaves the values out.
pub fn run(ctx: &VerbCtx, args: &[String], repo: Option<&Path>, hide: bool) -> anyhow::Result<String> {
    let m = merged(ctx, repo).ok();
    if let Some(name) = args.first() {
        let Some(s) = VERB_SPECS.iter().find(|s| s.name == name) else {
            return Err(UsageError(format!("zygo help: unknown verb `{name}`")).into());
        };
        let mut out = format!("{}\n{}\nscope: {}", usage(s), s.summary, scope_text(s.scope));
        if let Some(v) = value(ctx, s.name, m.as_ref(), hide) {
            out.push_str(&format!("\nnow: {v}"));
        }
        return Ok(out);
    }
    let mut lines = vec!["zygo verbs: /zygo:<verb> in Claude Code, zygo <verb> in a shell".to_string()];
    for s in VERB_SPECS {
        let mut l = format!("{} — {}", usage(s), s.summary);
        if let Some(v) = value(ctx, s.name, m.as_ref(), hide) {
            l.push_str(&format!(" (now: {v})"));
        }
        lines.push(l);
    }
    Ok(lines.join("\n"))
}

pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    run(ctx, args, repo_top().as_deref(), hide_window(ctx))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use zygo_core::session_dir::SessionDir;
    use zygo_core::verbs::VERB_SPECS;

    use super::*;
    use crate::cmd::compact::tests::ctx;
    use crate::cmd::UsageError;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|t| t.to_string()).collect()
    }

    fn line<'a>(out: &'a str, name: &str) -> &'a str {
        out.lines().find(|l| l.split(' ').next() == Some(name)).unwrap_or_else(|| panic!("{name}: {out}"))
    }

    #[test]
    fn lists_every_verb() {
        let d = TempDir::new().unwrap();
        let out = run(&ctx(d.path(), None), &[], None, false).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "zygo verbs: /zygo:<verb> in Claude Code, zygo <verb> in a shell");
        assert_eq!(lines.len(), 1 + VERB_SPECS.len());
        for (i, s) in VERB_SPECS.iter().enumerate() {
            assert!(lines[i + 1].starts_with(s.name), "{}", lines[i + 1]);
        }
        assert_eq!(line(&out, "compact"), "compact <tokens|on|off> — Set the compaction window for the session");
    }

    #[test]
    fn shows_current_values() {
        let d = TempDir::new().unwrap();
        std::fs::write(d.path().join("config.toml"), "[features]\ncompact_tool = false\n[user]\ndefault_profile = \"p\"\n").unwrap();
        let out = run(&ctx(d.path(), None), &[], None, false).unwrap();
        assert_eq!(line(&out, "compact-tool"), "compact-tool on|off — The `compact` MCP tool (now: off)");
        assert!(line(&out, "profile").ends_with("(now: p)"), "{out}");
        assert!(line(&out, "units").ends_with("(now: on)"), "{out}");
        let sd = SessionDir::new(d.path().join("s"));
        sd.create().unwrap();
        std::fs::write(sd.overrides_json(), r#"{"work_units":false}"#).unwrap();
        let out = run(&ctx(d.path(), Some(&sd.root)), &[], None, false).unwrap();
        assert!(line(&out, "units").ends_with("(now: off)"), "{out}");
    }

    #[test]
    fn one_verb_and_unknown() {
        let d = TempDir::new().unwrap();
        let c = ctx(d.path(), None);
        assert_eq!(
            run(&c, &v(&["compact"]), None, false).unwrap(),
            "compact <tokens|on|off>\nSet the compaction window for the session\nscope: session"
        );
        let e = run(&c, &v(&["frob"]), None, false).unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some());
        assert_eq!(e.to_string(), "zygo help: unknown verb `frob`");
    }
}

//! `zygo redaction [content on|off]` and `/zygo:redaction` (§17.1): the secret pass is status only (C4).

use zygo_config::Refusal;

use super::verbs::{base, set_session, with_session, SessionChange, Setting, VerbCtx};
use super::UsageError;

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

/// D5: turning the secret pass off is refused before any other check, in or out of a session.
pub fn run(ctx: &VerbCtx, args: &[String], team: Option<toml::Table>) -> anyhow::Result<String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["off"] | ["secrets", "off"] => Err(Refusal::SecretsOff.into()),
        [] => {
            let (m, _) = with_session(base(ctx, "redaction", team, None)?, ctx.session_dir.as_ref());
            let r = &m.config.features.redaction;
            Ok(format!("secrets {} (status only), content {}", on_off(r.secrets), on_off(r.content)))
        }
        ["content", w @ ("on" | "off")] => {
            let m = base(ctx, "redaction", team, None)?;
            let setting = Setting::RedactionContent(*w == "on");
            set_session(ctx, &m, SessionChange { verb: "redaction", words: format!("content {w}"), setting })
        }
        _ => Err(UsageError("zygo redaction: usage: redaction [content on|off]".into()).into()),
    }
}

pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    run(ctx, args, super::verbs::team(ctx, "redaction")?)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use zygo_config::Refusal;

    use super::*;
    use crate::cmd::compact::tests::{ctx, proxy};
    use crate::cmd::verbs::tests::{err_text, ov, sd, usage_text, v};

    #[test]
    fn shows_both_passes() {
        let d = TempDir::new().unwrap();
        assert_eq!(run(&ctx(d.path(), None), &[], None).unwrap(), "secrets on (status only), content on");
        std::fs::write(d.path().join("config.toml"), "[features.redaction]\ncontent = false\n").unwrap();
        assert_eq!(run(&ctx(d.path(), None), &[], None).unwrap(), "secrets on (status only), content off");
        let dir = sd(d.path());
        std::fs::write(dir.join("overrides.json"), r#"{"redaction_content":true}"#).unwrap();
        assert_eq!(run(&ctx(d.path(), Some(&dir)), &[], None).unwrap(), "secrets on (status only), content on");
    }

    #[test]
    fn show_keeps_content_beside_a_refused_guard() {
        let d = TempDir::new().unwrap();
        std::fs::write(d.path().join("config.toml"), "[guards]\nmin_level = \"notify\"\n").unwrap();
        let dir = sd(d.path());
        std::fs::write(dir.join("overrides.json"), r#"{"redaction_content":false,"guards":{"claims":"monitor"}}"#).unwrap();
        assert_eq!(run(&ctx(d.path(), Some(&dir)), &[], None).unwrap(), "secrets on (status only), content off");
    }

    #[test]
    fn secrets_cannot_be_turned_off() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        for c in [ctx(d.path(), Some(&dir)), ctx(d.path(), None)] {
            for a in [&["off"][..], &["secrets", "off"]] {
                let e = run(&c, &v(a), None).unwrap_err();
                assert_eq!(e.downcast_ref::<Refusal>(), Some(&Refusal::SecretsOff), "{a:?}");
                assert_eq!(e.to_string(), "refused: secret redaction cannot be turned off in a session");
            }
            for a in [&["on"][..], &["secrets", "on"], &["content"], &["content", "maybe"]] {
                assert_eq!(usage_text(run(&c, &v(a), None)), "zygo redaction: usage: redaction [content on|off]", "{a:?}");
            }
        }
        assert!(!dir.join("overrides.json").exists());
    }

    #[test]
    fn content_off_patches_the_session_and_team_set_is_refused() {
        let d = TempDir::new().unwrap();
        let (h, id, dir) = proxy(d.path());
        let c = ctx(d.path(), Some(&dir));
        assert_eq!(run(&c, &v(&["content", "off"]), None).unwrap(), "redaction content off for this session");
        assert_eq!(h.session(&id).unwrap().settings().redaction_content, Some(false));
        assert_eq!(ov(&dir).redaction_content, Some(false));

        let (h2, id2, dir2) = proxy(d.path());
        let team: toml::Table = "[features.redaction]\ncontent = true\n".parse().unwrap();
        assert_eq!(
            err_text(run(&ctx(d.path(), Some(&dir2)), &v(&["content", "off"]), Some(team))),
            "refused: `features.redaction.content` is set by the team config"
        );
        assert_eq!(h2.session(&id2).unwrap().settings().redaction_content, None);
        assert!(!dir2.join("overrides.json").exists());
    }
}

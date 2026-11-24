//! `zygo guards [<guard> <level>]` and `/zygo:guards` (§17.1, §13.5): session, `--save`.

use std::path::Path;

use zygo_config::GUARDS;

use super::status::repo_top;
use super::verbs::{base, set_session, with_session, SessionChange, Setting, VerbCtx};
use super::UsageError;

/// D4: `[]` lists each guard's level with its layer, over the config with the session overrides.
pub fn run(ctx: &VerbCtx, args: &[String], team: Option<toml::Table>, repo: Option<&Path>) -> anyhow::Result<String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] => {
            let (m, lifted) = with_session(base(ctx, "guards", team, repo)?, ctx.session_dir.as_ref());
            let g = &m.config.guards;
            let source = |n: &str| {
                let key = if lifted.contains(n) { "guards.min_level".to_string() } else { format!("guards.{n}") };
                m.source_of(&key).name()
            };
            let mut lines: Vec<String> =
                GUARDS.iter().map(|n| format!("{n} {} ({})", g.get(n).unwrap_or_default(), source(n))).collect();
            lines.push(format!("min_level {} ({})", g.min_level, m.source_of("guards.min_level").name()));
            Ok(lines.join("\n"))
        }
        [guard, level] => {
            let m = base(ctx, "guards", team, repo)?;
            let setting = Setting::Guard { guard: guard.to_string(), level: level.to_string() };
            set_session(ctx, &m, SessionChange { verb: "guards", words: format!("{guard} {level}"), setting })
        }
        _ => Err(UsageError("zygo guards: usage: guards [<guard> <level>]".into()).into()),
    }
}

pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    run(ctx, args, super::verbs::team(ctx, "guards")?, repo_top().as_deref())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::TempDir;

    use super::*;
    use crate::cmd::compact::tests::ctx;
    use crate::cmd::verbs::tests::{err_text, ov, sd, usage_text, v};

    #[test]
    fn lists_levels_with_sources() {
        let d = TempDir::new().unwrap();
        std::fs::write(d.path().join("config.toml"), "[guards]\nclaims = \"notify\"\n").unwrap();
        assert_eq!(
            run(&ctx(d.path(), None), &[], None, None).unwrap(),
            "claims notify (user)\nscope monitor (builtin)\nnavigation monitor (builtin)\noutput monitor (builtin)\n\
             trailers notify (builtin)\nrepeat monitor (builtin)\ndispositions enforce (builtin)\nmin_level monitor (builtin)"
        );
        let dir = sd(d.path());
        std::fs::write(dir.join("overrides.json"), r#"{"guards":{"scope":"enforce"}}"#).unwrap();
        let out = run(&ctx(d.path(), Some(&dir)), &[], None, None).unwrap();
        assert_eq!(out.lines().nth(1), Some("scope enforce (session)"), "{out}");
    }

    #[test]
    fn each_session_level_applies_on_its_own() {
        let d = TempDir::new().unwrap();
        std::fs::write(d.path().join("config.toml"), "[guards]\nmin_level = \"notify\"\n").unwrap();
        let dir = sd(d.path());
        std::fs::write(dir.join("overrides.json"), r#"{"guards":{"claims":"monitor","scope":"enforce"}}"#).unwrap();
        let out = run(&ctx(d.path(), Some(&dir)), &[], None, None).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "claims notify (user)", "{out}");
        assert_eq!(lines[1], "scope enforce (session)", "{out}");
        assert_eq!(lines[7], "min_level notify (user)", "{out}");
    }

    #[test]
    fn set_and_floor() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        let c = ctx(d.path(), Some(&dir));
        assert_eq!(run(&c, &v(&["claims", "enforce"]), None, None).unwrap(), "guards claims enforce for this session");
        assert_eq!(ov(&dir).guards, BTreeMap::from([("claims".to_string(), "enforce".to_string())]));
        std::fs::write(d.path().join("config.toml"), "[guards]\nmin_level = \"notify\"\n").unwrap();
        assert_eq!(
            err_text(run(&c, &v(&["scope", "monitor"]), None, None)),
            "refused: guard scope cannot go below min_level notify"
        );
        for a in [&["claims"][..], &["claims", "enforce", "x"]] {
            assert_eq!(usage_text(run(&c, &v(a), None, None)), "zygo guards: usage: guards [<guard> <level>]", "{a:?}");
        }
    }

    #[test]
    fn team_and_unknown_refused() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        let c = ctx(d.path(), Some(&dir));
        let team = || Some("[guards]\nclaims = \"enforce\"\n".parse::<toml::Table>().unwrap());
        let out = run(&c, &[], team(), None).unwrap();
        assert_eq!(out.lines().next(), Some("claims enforce (team)"), "{out}");
        assert_eq!(
            err_text(run(&c, &v(&["claims", "notify"]), team(), None)),
            "refused: `guards.claims` is set by the team config"
        );
        assert_eq!(err_text(run(&c, &v(&["frob", "enforce"]), None, None)), "refused: unknown guard `frob`");
        assert_eq!(err_text(run(&c, &v(&["claims", "loud"]), None, None)), "refused: unknown level `loud`");
        assert!(!dir.join("overrides.json").exists());
    }

    #[test]
    fn team_floor_holds_over_user_config() {
        let d = TempDir::new().unwrap();
        std::fs::write(d.path().join("config.toml"), "[guards]\nmin_level = \"monitor\"\nclaims = \"monitor\"\n").unwrap();
        let dir = sd(d.path());
        let c = ctx(d.path(), Some(&dir));
        let team = || Some("[guards]\nmin_level = \"enforce\"\n".parse::<toml::Table>().unwrap());
        let out = run(&c, &[], team(), None).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "claims enforce (team)", "{out}");
        assert_eq!(lines[7], "min_level enforce (team)", "{out}");
        assert_eq!(
            err_text(run(&c, &v(&["scope", "notify"]), team(), None)),
            "refused: guard scope cannot go below min_level enforce"
        );
        assert!(!dir.join("overrides.json").exists());
    }
}

//! `zygo compact-tool on|off` and `/zygo:compact-tool` (§17.1, §10.3): next launch, always user config.

use zygo_config::Refusal;
use zygo_plugins::select::set_user_key;

use super::verbs::{base, VerbCtx};
use super::UsageError;

/// D6: `--save` is ignored; a team-set key is refused (`Overrides` has no field for it, so this is the
/// `Merged::refuse_team` check).
pub fn run(ctx: &VerbCtx, args: &[String], team: Option<toml::Table>) -> anyhow::Result<String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let [w @ ("on" | "off")] = args.as_slice() else {
        return Err(UsageError("zygo compact-tool: usage: compact-tool on|off".into()).into());
    };
    let key = "features.compact_tool";
    if base(ctx, "compact-tool", team, None)?.team_keys.contains(key) {
        return Err(Refusal::TeamSet { key: key.into() }.into());
    }
    set_user_key(&ctx.paths.config_file, &["features", "compact_tool"], *w == "on")
        .map_err(|e| anyhow::anyhow!("zygo compact-tool: saving to user config failed: {e}"))?;
    Ok(format!("compact-tool {w} (applies at next launch)"))
}

pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    run(ctx, args, super::verbs::team(ctx, "compact-tool")?)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::cmd::compact::tests::ctx;
    use crate::cmd::verbs::tests::{cfg, err_text, sd, usage_text, v};

    #[test]
    fn writes_user_config_for_the_next_launch() {
        let d = TempDir::new().unwrap();
        std::fs::write(d.path().join("config.toml"), "# keep\n").unwrap();
        let c = ctx(d.path(), None);
        assert_eq!(run(&c, &v(&["off"]), None).unwrap(), "compact-tool off (applies at next launch)");
        assert!(std::fs::read_to_string(d.path().join("config.toml")).unwrap().starts_with("# keep\n"));
        assert_eq!(cfg(d.path())["features"]["compact_tool"].as_bool(), Some(false));
        let dir = sd(d.path());
        let c = ctx(d.path(), Some(&dir));
        assert_eq!(run(&c, &v(&["on"]), None).unwrap(), "compact-tool on (applies at next launch)");
        assert_eq!(cfg(d.path())["features"]["compact_tool"].as_bool(), Some(true));
        assert!(!dir.join("overrides.json").exists());
        for a in [&[][..], &["maybe"]] {
            assert_eq!(usage_text(run(&c, &v(a), None)), "zygo compact-tool: usage: compact-tool on|off", "{a:?}");
        }
    }

    #[test]
    fn team_set_refused() {
        let d = TempDir::new().unwrap();
        let team: toml::Table = "[features]\ncompact_tool = true\n".parse().unwrap();
        assert_eq!(
            err_text(run(&ctx(d.path(), None), &v(&["off"]), Some(team))),
            "refused: `features.compact_tool` is set by the team config"
        );
        assert!(!d.path().join("config.toml").exists());
    }
}

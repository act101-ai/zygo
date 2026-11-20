//! `zygo results on|off|threshold <bytes>` and `/zygo:results` (§17.1, §10.1): session, `--save`.

use super::verbs::{base, set_session, SessionChange, Setting, VerbCtx};
use super::UsageError;

pub fn run(ctx: &VerbCtx, args: &[String], team: Option<toml::Table>) -> anyhow::Result<String> {
    let usage = || UsageError("zygo results: usage: results on|off|threshold <bytes>".into());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (words, setting) = match args.as_slice() {
        [w @ ("on" | "off")] => (w.to_string(), Setting::Results(*w == "on")),
        ["threshold", n] => {
            // F4: user config holds an i64, so a larger value is a usage error.
            let n = n.parse::<u64>().ok().filter(|n| i64::try_from(*n).is_ok()).ok_or_else(usage)?;
            (format!("threshold {n}"), Setting::Threshold(n))
        }
        _ => return Err(usage().into()),
    };
    let m = base(ctx, "results", team, None)?;
    set_session(ctx, &m, SessionChange { verb: "results", words, setting })
}

pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    run(ctx, args, super::verbs::team(ctx, "results")?)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::cmd::compact::tests::ctx;
    use crate::cmd::verbs::tests::{ov, sd, usage_text, v};

    #[test]
    fn on_off_and_threshold() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        let c = ctx(d.path(), Some(&dir));
        assert_eq!(run(&c, &v(&["off"]), None).unwrap(), "results off for this session");
        assert_eq!(ov(&dir).results, Some(false));
        assert_eq!(run(&c, &v(&["on"]), None).unwrap(), "results on for this session");
        assert_eq!(ov(&dir).results, Some(true));
        assert_eq!(run(&c, &v(&["threshold", "32768"]), None).unwrap(), "results threshold 32768 for this session");
        assert_eq!(ov(&dir).results_threshold, Some(32768));
    }

    #[test]
    fn usage_errors() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        let c = ctx(d.path(), Some(&dir));
        let over = (i64::MAX as u64 + 1).to_string();
        for a in [&[][..], &["maybe"], &["threshold"], &["threshold", "x"], &["threshold", "-1"], &["threshold", &over], &["off", "x"]] {
            assert_eq!(usage_text(run(&c, &v(a), None)), "zygo results: usage: results on|off|threshold <bytes>", "{a:?}");
        }
        assert!(!dir.join("overrides.json").exists());
        let saved = VerbCtx { save: true, ..ctx(d.path(), None) };
        assert_eq!(usage_text(run(&saved, &v(&["threshold", &over]), None)), "zygo results: usage: results on|off|threshold <bytes>");
        assert!(!d.path().join("config.toml").exists());
        let max = i64::MAX.to_string();
        assert_eq!(run(&saved, &v(&["threshold", &max]), None).unwrap(), format!("results threshold {max} saved to user config"));
    }
}

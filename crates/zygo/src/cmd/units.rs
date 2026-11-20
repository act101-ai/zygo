//! `zygo units on|off` and `/zygo:units` (§17.1, §13): session, `--save`.

use super::verbs::{base, set_session, SessionChange, Setting, VerbCtx};
use super::UsageError;

pub fn run(ctx: &VerbCtx, args: &[String], team: Option<toml::Table>) -> anyhow::Result<String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let [w @ ("on" | "off")] = args.as_slice() else {
        return Err(UsageError("zygo units: usage: units on|off".into()).into());
    };
    let m = base(ctx, "units", team, None)?;
    set_session(ctx, &m, SessionChange { verb: "units", words: w.to_string(), setting: Setting::Units(*w == "on") })
}

pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    run(ctx, args, super::verbs::team(ctx, "units")?)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::cmd::compact::tests::ctx;
    use crate::cmd::verbs::tests::{ov, sd, usage_text, v};

    #[test]
    fn on_off_and_usage() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        let c = ctx(d.path(), Some(&dir));
        assert_eq!(run(&c, &v(&["off"]), None).unwrap(), "units off for this session");
        assert_eq!(ov(&dir).work_units, Some(false));
        assert_eq!(run(&c, &v(&["on"]), None).unwrap(), "units on for this session");
        assert_eq!(ov(&dir).work_units, Some(true));
        for a in [&[][..], &["maybe"]] {
            assert_eq!(usage_text(run(&c, &v(a), None)), "zygo units: usage: units on|off", "{a:?}");
        }
    }
}

//! `zygo work [<id>|clear]` (§17.1): show, set or clear the current work item; `zygo work report` (§13.6).

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use zygo_config::{LoadContext, WorkConfig};
use zygo_work::current;

use super::verbs::VerbCtx;
use super::UsageError;

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Show,
    Clear,
    Set(String),
    Report { unit: Option<String>, since: Option<String> },
}

fn parse(args: &[String]) -> Result<Action> {
    match args {
        [] => Ok(Action::Show),
        [a] if a == "clear" => Ok(Action::Clear),
        [a, rest @ ..] if a == "report" => parse_report(rest),
        [a] => Ok(Action::Set(a.clone())),
        _ => Err(UsageError("zygo work: usage: work [<id>|clear]".into()).into()),
    }
}

fn parse_report(rest: &[String]) -> Result<Action> {
    let usage = || UsageError("zygo work: usage: work report [<unit>] [--since <date>]".into());
    let (mut unit, mut since) = (None, None);
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        let d = if a == "--since" {
            Some(it.next().ok_or_else(usage)?.clone())
        } else {
            a.strip_prefix("--since=").map(String::from)
        };
        match d {
            Some(d) if since.is_none() => since = Some(d),
            None if unit.is_none() && !a.starts_with('-') => unit = Some(a.clone()),
            _ => return Err(usage().into()),
        }
    }
    Ok(Action::Report { unit, since })
}

fn describe(item: Option<&current::WorkItem>) -> String {
    match item {
        None => "no current work item".into(),
        Some(i) => match &i.title {
            Some(t) => format!("{}:{} {t}", i.source, i.id),
            None => format!("{}:{}", i.source, i.id),
        },
    }
}

/// Maps a `set`/`clear` error: a typed refusal (outside a repository, no source accepts the
/// argument) is a usage error (exit 2).
fn map_err(e: anyhow::Error) -> anyhow::Error {
    let msg = format!("zygo work: {e}");
    if e.downcast_ref::<current::UsageError>().is_some() { UsageError(msg).into() } else { anyhow::anyhow!(msg) }
}

/// The repository top whose decision records `work report` counts: looked up only for the CLI form of `report`
/// (the slash form reads no record, D-C2).
fn records_root(slash: bool, args: &[String], find: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    if !slash && args.first().is_some_and(|a| a == "report") { find() } else { None }
}

fn run(
    cwd: &Path,
    cfg: &WorkConfig,
    db: &Path,
    records_in: Option<&Path>,
    args: &[String],
    err: &mut dyn Write,
) -> Result<String> {
    match parse(args)? {
        Action::Report { unit, since } => {
            let since = match since {
                Some(s) => Some(zygo_work::report::parse_since(&s).map_err(|_| {
                    UsageError(format!("zygo work report: --since takes a date YYYY-MM-DD, not `{s}`"))
                })?),
                None => None,
            };
            zygo_work::report::report(db, records_in, unit.as_deref(), since)
        }
        Action::Show => {
            let (item, notes) = current::resolve_noted(cwd, cfg);
            for l in current::stderr_lines(item.as_ref(), &notes) {
                let _ = writeln!(err, "{l}");
            }
            Ok(describe(item.as_ref()))
        }
        Action::Set(arg) => {
            let item = current::set(cwd, cfg, &arg).map_err(map_err)?;
            for l in current::stderr_lines(Some(&item), &[]) {
                let _ = writeln!(err, "{l}");
            }
            Ok(describe(Some(&item)))
        }
        Action::Clear => {
            current::clear(cwd).map_err(map_err)?;
            Ok("cleared".into())
        }
    }
}

pub fn verb(ctx: &VerbCtx, args: &[String]) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let merged = zygo_config::load(&LoadContext { paths: ctx.paths.clone(), repo: Some(cwd.clone()), team: None })
        .map_err(|e| anyhow::anyhow!("zygo work: {e}"))?;
    let records = records_root(ctx.slash, args, super::status::repo_top);
    run(&cwd, &merged.config.work, &ctx.paths.usage_db, records.as_deref(), args, &mut std::io::stderr())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn parse_table() {
        assert_eq!(parse(&[]).unwrap(), Action::Show);
        assert_eq!(parse(&v(&["clear"])).unwrap(), Action::Clear);
        assert_eq!(parse(&v(&["42-fix"])).unwrap(), Action::Set("42-fix".into()));
        assert_eq!(parse(&v(&["report", "--since", "1d"])).unwrap(), Action::Report { unit: None, since: Some("1d".into()) });
        assert_eq!(parse(&v(&["report"])).unwrap(), Action::Report { unit: None, since: None });
        assert_eq!(
            parse(&v(&["report", "beads:bd-a1b2", "--since=2026-11-18"])).unwrap(),
            Action::Report { unit: Some("beads:bd-a1b2".into()), since: Some("2026-11-18".into()) }
        );
        assert!(parse(&v(&["a", "b"])).unwrap_err().downcast_ref::<UsageError>().is_some());
        for bad in [v(&["report", "a", "b"]), v(&["report", "--since"])] {
            assert!(parse(&bad).unwrap_err().downcast_ref::<UsageError>().is_some(), "{bad:?}");
        }
    }

    #[test]
    fn outside_a_repository() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        let cfg = WorkConfig::default();
        let mut err = Vec::new();
        for a in [v(&["clear"]), v(&["x"])] {
            let e = run(d, &cfg, Path::new("unused.db"), None, &a, &mut err).unwrap_err();
            assert!(e.to_string().contains("zygo work: not in a git repository"));
            assert!(e.downcast_ref::<UsageError>().is_some());
        }
        assert_eq!(run(d, &cfg, Path::new("unused.db"), None, &[], &mut err).unwrap(), "no current work item");
        assert!(err.is_empty());
    }

    #[test]
    #[ignore = "timing: git checkout lookup inside the 2 s zygo-work repo BOUND"]
    fn no_source_accepting_the_argument_is_a_usage_error() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        let st = std::process::Command::new("git").args(["init", "-q"]).current_dir(d).status().unwrap();
        assert!(st.success());
        let cfg = WorkConfig { sources: vec!["github".into()] };
        let e = run(d, &cfg, Path::new("unused.db"), None, &v(&["a b"]), &mut Vec::new()).unwrap_err();
        assert!(e.to_string().contains("no work source accepts"), "{e}");
        assert!(e.downcast_ref::<UsageError>().is_some());
    }

    #[test]
    fn report_reads_the_store() {
        let t = tempfile::tempdir().unwrap();
        let db = t.path().join("usage.db");
        let cfg = WorkConfig::default();
        assert_eq!(run(t.path(), &cfg, &db, None, &v(&["report"]), &mut Vec::new()).unwrap(), "no work units");
        let top = t.path().join("repo");
        std::fs::create_dir_all(top.join("docs")).unwrap();
        std::fs::write(top.join("docs/decisions"), "x").unwrap();
        assert_eq!(
            run(t.path(), &cfg, &db, Some(&top), &v(&["report"]), &mut Vec::new()).unwrap(),
            "no work units\n\ndecision records: docs/decisions is not a directory"
        );
        let e = run(t.path(), &cfg, &db, None, &v(&["report", "--since", "1d"]), &mut Vec::new()).unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some());
        assert!(e.to_string().contains("--since takes a date YYYY-MM-DD"), "{e}");
    }

    #[test]
    fn records_only_for_the_cli_report() {
        // D§9.5, D-C2
        let top = || Some(PathBuf::from("/r"));
        assert_eq!(records_root(false, &v(&["report"]), top), Some(PathBuf::from("/r")));
        assert_eq!(records_root(false, &v(&["report", "beads:bd-a1b2"]), top), Some(PathBuf::from("/r")));
        assert_eq!(records_root(false, &v(&["report"]), || None), None); // outside a repository, or git failed
        assert_eq!(records_root(true, &v(&["report"]), || unreachable!()), None); // slash form: no lookup
        assert_eq!(records_root(false, &v(&["clear"]), || unreachable!()), None);
        assert_eq!(records_root(false, &[], || unreachable!()), None);
    }

    #[test]
    fn failed_lookup_goes_to_the_sink_as_one_line() {
        let item = current::WorkItem {
            source: "github".into(),
            id: "a/b#1".into(),
            title: None,
            url: None,
            status: None,
            check_hint: None,
            files_hint: vec![],
            lookup_failed: Some("timed out".into()),
        };
        let mut err = Vec::new();
        for l in current::stderr_lines(Some(&item), &[]) {
            writeln!(err, "{l}").unwrap();
        }
        let s = String::from_utf8(err).unwrap();
        assert_eq!(s.lines().count(), 1);
        assert!(s.ends_with('\n'));
    }
}

//! `zygo work [<id>|clear]` (§17.1): show, set or clear the current work item; `zygo work report` (§13.6).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

/// The bound of the records lookup (D§9.5).
const RECORDS_BOUND: Duration = Duration::from_secs(2);

/// The repository top whose decision records `work report` counts: looked up only in the CLI form (the slash form
/// reads no record, D-C2).
fn records_root(slash: bool, find: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    if slash { None } else { find() }
}

/// `git rev-parse --show-toplevel` at `cwd` until `deadline` (D§9.5), as `repo::git_cmd` builds a git call; `None`
/// outside a repository, on any git error and at the deadline.
fn show_toplevel(cwd: &Path, deadline: Instant) -> Option<PathBuf> {
    let mut c = zygo_config::repo_file::git_command_for(std::ffi::OsStr::new("git"));
    c.args(["-c", "core.fsmonitor=false", "-C"]).arg(cwd).args(["rev-parse", "--show-toplevel"]);
    c.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0");
    let out = zygo_work::repo::run_whole(c, deadline).ok().filter(|w| w.ok)?.out;
    let out = String::from_utf8(out).ok()?;
    let top = out.strip_suffix('\n').unwrap_or(&out);
    (!top.is_empty()).then(|| PathBuf::from(top))
}

fn run(
    cwd: &Path,
    cfg: &WorkConfig,
    db: &Path,
    records: impl FnOnce() -> Option<PathBuf>,
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
            let top = records();
            zygo_work::report::report(db, top.as_deref(), unit.as_deref(), since)
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
    verb_in(ctx, &std::env::current_dir()?, RECORDS_BOUND, args)
}

/// [`verb`] at `cwd`, with `bound` for the records lookup.
fn verb_in(ctx: &VerbCtx, cwd: &Path, bound: Duration, args: &[String]) -> Result<String> {
    let merged = zygo_config::load(&LoadContext { paths: ctx.paths.clone(), repo: Some(cwd.to_path_buf()), team: None })
        .map_err(|e| anyhow::anyhow!("zygo work: {e}"))?;
    let records = || records_root(ctx.slash, || show_toplevel(cwd, Instant::now() + bound));
    run(cwd, &merged.config.work, &ctx.paths.usage_db, records, args, &mut std::io::stderr())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_lookup() -> Option<PathBuf> {
        unreachable!("no records lookup")
    }

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
            let e = run(d, &cfg, Path::new("unused.db"), no_lookup, &a, &mut err).unwrap_err();
            assert!(e.to_string().contains("zygo work: not in a git repository"));
            assert!(e.downcast_ref::<UsageError>().is_some());
        }
        assert_eq!(run(d, &cfg, Path::new("unused.db"), no_lookup, &[], &mut err).unwrap(), "no current work item");
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
        let e = run(d, &cfg, Path::new("unused.db"), no_lookup, &v(&["a b"]), &mut Vec::new()).unwrap_err();
        assert!(e.to_string().contains("no work source accepts"), "{e}");
        assert!(e.downcast_ref::<UsageError>().is_some());
    }

    #[test]
    fn report_reads_the_store() {
        let t = tempfile::tempdir().unwrap();
        let db = t.path().join("usage.db");
        let cfg = WorkConfig::default();
        assert_eq!(run(t.path(), &cfg, &db, || None, &v(&["report"]), &mut Vec::new()).unwrap(), "no work units");
        let top = t.path().join("repo");
        std::fs::create_dir_all(top.join("docs")).unwrap();
        std::fs::write(top.join("docs/decisions"), "x").unwrap();
        assert_eq!(
            run(t.path(), &cfg, &db, || Some(top.clone()), &v(&["report"]), &mut Vec::new()).unwrap(),
            "no work units\n\ndecision records: docs/decisions is not a directory"
        );
        let e = run(t.path(), &cfg, &db, no_lookup, &v(&["report", "--since", "1d"]), &mut Vec::new()).unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some());
        assert!(e.to_string().contains("--since takes a date YYYY-MM-DD"), "{e}");
    }

    #[test]
    fn records_only_for_the_cli_report() {
        // D§9.5, D-C2
        assert_eq!(records_root(false, || Some(PathBuf::from("/r"))), Some(PathBuf::from("/r")));
        assert_eq!(records_root(false, || None), None); // outside a repository, or git failed
        assert_eq!(records_root(true, || unreachable!()), None); // slash form: no lookup
    }

    #[test]
    fn usage_error_starts_no_lookup() {
        let t = tempfile::tempdir().unwrap();
        let cfg = WorkConfig::default();
        for a in [v(&["report", "--since", "zz"]), v(&["report", "a", "b"]), v(&["report", "--since"])] {
            let e = run(t.path(), &cfg, &t.path().join("usage.db"), no_lookup, &a, &mut Vec::new()).unwrap_err();
            assert!(e.downcast_ref::<UsageError>().is_some(), "{a:?}: {e}");
        }
    }

    /// No test waits on the bound: an hour is never reached.
    const HOUR: Duration = Duration::from_secs(3600);

    fn git(dir: &Path, args: &[&str]) {
        const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let mut c = zygo_config::repo_file::git_command_for(std::ffi::OsStr::new("git"));
        let out = c.env("GIT_CONFIG_GLOBAL", NULL).env("GIT_CONFIG_NOSYSTEM", "1").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn show_toplevel_one_git_call() {
        let t = tempfile::tempdir().unwrap();
        let top = t.path().join("repo");
        std::fs::create_dir_all(top.join("a/b")).unwrap();
        assert_eq!(show_toplevel(&top, Instant::now() + HOUR), None); // outside a repository
        git(&top, &["init", "-q"]);
        let want = top.canonicalize().unwrap();
        assert_eq!(show_toplevel(&top.join("a/b"), Instant::now() + HOUR).map(|p| p.canonicalize().unwrap()), Some(want));
        assert_eq!(show_toplevel(&top, Instant::now() - Duration::from_millis(1)), None); // bound passed: no spawn
        assert_eq!(show_toplevel(&t.path().join("missing"), Instant::now() + HOUR), None); // git error
    }

    /// A git repository with one open record of `phase-3:N12`, and a store with one round of that unit.
    fn records_fixture(d: &Path) -> (VerbCtx, PathBuf) {
        let top = d.join("repo");
        let dir = top.join("docs/decisions");
        std::fs::create_dir_all(&dir).unwrap();
        git(&top, &["init", "-q"]);
        let id = "DDR-01JDQ2Y7Z8K4M5N6P7Q8R00001";
        let front = format!(
            "+++\nid = \"{id}\"\nkind = \"decision\"\nstatus = \"open\"\nclass = \"none\"\ntitle = \"T\"\nunit = \"phase-3:N12\"\nround = 1\nrole = \"code\"\nmodel = \"unknown\"\ncommit = \"6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556\"\ndate = \"2026-11-24\"\ndepends_on = []\nreverse_cost = \"low\"\n+++\n"
        );
        let body = "\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n";
        std::fs::write(dir.join(format!("{id}.md")), front + body).unwrap();
        let ctx = crate::cmd::compact::tests::ctx(&d.join("home"), None);
        std::fs::create_dir_all(ctx.paths.usage_db.parent().unwrap()).unwrap();
        let store = zygo_usage::Store::open(&ctx.paths.usage_db).unwrap();
        let row = zygo_usage::WorkUnitRow {
            unit: "phase-3:N12".into(),
            round: 1,
            session: "S1".into(),
            agent_id: "main".into(),
            began_at: 100,
            ..Default::default()
        };
        store.insert_work_unit(&row).unwrap();
        (ctx, top)
    }

    #[test]
    fn cli_report_reads_the_records_of_the_cwd_repository() {
        // D§9.5: the CLI form passes `ctx.slash` and the real finder
        let t = tempfile::tempdir().unwrap();
        let (ctx, top) = records_fixture(t.path());
        let out = verb_in(&ctx, &top, HOUR, &v(&["report"])).unwrap();
        assert!(out.contains("\n  decisions: 1 open, 1 total"), "{out}");
        assert!(out.ends_with("\n\ndecision records: 1 open, 1 total, deepest chain 1"), "{out}");
    }

    #[test]
    fn slash_report_reads_no_record() {
        // D-C2: the slash form (`run_verb` with `slash = true`) reads no record
        let t = tempfile::tempdir().unwrap();
        let (ctx, top) = records_fixture(t.path());
        let ctx = VerbCtx { slash: true, ..ctx };
        let out = verb_in(&ctx, &top, HOUR, &v(&["report"])).unwrap();
        assert!(out.starts_with("phase-3:N12\n"), "{out}");
        assert!(!out.contains("decisions:") && !out.contains("decision records:"), "{out}");
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

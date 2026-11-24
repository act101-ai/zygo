//! `zygo decision` (decision records design D§10.1): delegated decision records in `docs/decisions/`.
use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zygo_core::esc;
use zygo_work::decisions::graph::{count_in, dependents, inverse_of};
use zygo_work::decisions::history::trailer_commits;
use zygo_work::decisions::record::{parse_record, read_bytes, Kind, Record, Status, DIR};
use zygo_work::decisions::set::{read_dir, set_faults, DirError, RecordSet};
use zygo_work::repo;

use super::verbs::VerbCtx;

const USAGE: &str =
    "zygo decision: usage: zygo decision ls [--status <s>] [--kind <k>] [--unit <prefix>] [--pending] | show <id> | deps <id> | check";
/// The commits `show` lists at most.
const SHOW_COMMITS: usize = 50;

/// The streams of one `zygo decision` run.
pub struct DecisionIo<'a> {
    pub input: &'a mut dyn BufRead,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    /// Whether stdin is a terminal.
    pub tty: bool,
}

/// The repository facts `run_in` reads, as values so tests run no git.
pub struct Repo<'a> {
    /// The repository top of the current directory; `None` outside a git repository. Called after the arguments parse.
    pub top: &'a dyn Fn() -> Option<PathBuf>,
    /// `history::trailer_commits` outside tests.
    pub commits: &'a CommitsFn,
}

/// `(top, id, limit)` to the `(full id, subject)` commits, or the stderr line; see `history::trailer_commits`.
pub type CommitsFn = dyn Fn(&Path, &str, usize) -> Result<Vec<(String, String)>, String>;

/// The `ls` filters, joined by AND.
#[derive(Default)]
struct Filter {
    status: Option<Status>,
    kind: Option<Kind>,
    unit: Option<String>,
    pending: bool,
}

impl Filter {
    fn keeps(&self, r: &Record) -> bool {
        let pending = r.status == Status::Overturned
            || (r.status == Status::Ratified && r.promoted_to.is_some() && r.promoted_by.is_none());
        self.status.is_none_or(|s| r.status == s)
            && self.kind.is_none_or(|k| r.kind == k)
            && self.unit.as_ref().is_none_or(|u| r.unit.starts_with(u.as_str()))
            && (!self.pending || pending)
    }
}

enum Sub {
    Ls(Filter),
    Show(String),
    Deps(String),
    Check,
}

/// The subcommand of `args`; `None` for any argument error.
fn parse(args: &[String]) -> Option<Sub> {
    let (first, rest) = args.split_first()?;
    match (first.as_str(), rest) {
        ("ls", _) => {
            let mut f = Filter::default();
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--status" => f.status = Some(Status::parse(it.next()?)?),
                    "--kind" => f.kind = Some(Kind::parse(it.next()?)?),
                    "--unit" => f.unit = Some(it.next()?.clone()),
                    "--pending" => f.pending = true,
                    _ => return None,
                }
            }
            Some(Sub::Ls(f))
        }
        ("show", [id]) => Some(Sub::Show(id.clone())),
        ("deps", [id]) => Some(Sub::Deps(id.clone())),
        ("check", []) => Some(Sub::Check),
        _ => None,
    }
}

/// Writes `esc(line)` and a newline; write errors are ignored.
fn line(w: &mut dyn Write, s: &str) {
    let _ = writeln!(w, "{}", esc(s));
}

/// One `zygo decision: <name>: <faults>` line per malformed file.
fn malformed_lines(set: &RecordSet, io: &mut DecisionIo) {
    for (name, faults) in &set.malformed {
        line(io.err, &format!("zygo decision: {name}: {}", faults.join("; ")));
    }
}

/// The valid record `id` of `set`, or the refusal line and `None`.
fn find<'s>(set: &'s RecordSet, id: &str, io: &mut DecisionIo) -> Option<&'s Record> {
    let r = set.valid.iter().find(|r| r.id == id);
    if r.is_none() {
        line(io.err, &format!("zygo decision: {id} is not a valid record"));
    }
    r
}

/// One arm per subcommand; gives the exit code. `-h` or `--help` in any position prints the usage line to stdout.
pub fn run_in(repo: &Repo, _env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32 {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        line(io.out, USAGE);
        return 0;
    }
    let Some(sub) = parse(args) else {
        line(io.err, USAGE);
        return 2;
    };
    let Some(top) = (repo.top)() else {
        line(io.err, "zygo decision: not in a git repository");
        return 2;
    };
    let set = match read_dir(&top) {
        Ok(set) => set,
        Err(DirError::NotDirectory) => {
            line(io.err, "zygo decision: docs/decisions is not a directory");
            return 1;
        }
        Err(DirError::TooMany) => {
            line(io.err, "zygo decision: more than 10,000 files in docs/decisions/");
            return 1;
        }
    };
    match sub {
        Sub::Ls(f) => ls(&set, &f, io),
        Sub::Show(id) => show(repo, &top, &set, &id, io),
        Sub::Deps(id) => deps(&set, &id, io),
        Sub::Check => check(&set, io),
    }
}

fn ls(set: &RecordSet, f: &Filter, io: &mut DecisionIo) -> i32 {
    malformed_lines(set, io);
    for fault in set_faults(set) {
        line(io.err, &format!("zygo decision: {fault}"));
    }
    let inv = inverse_of(&set.valid, false);
    for r in set.valid.iter().filter(|r| f.keeps(r)) {
        let n = count_in(&inv, &r.id);
        line(io.out, &format!("{} {} {} {} {n} {}", r.id, r.kind.name(), r.status.name(), r.class.name(), r.title));
    }
    0
}

fn show(repo: &Repo, top: &Path, set: &RecordSet, id: &str, io: &mut DecisionIo) -> i32 {
    malformed_lines(set, io);
    let Some(r) = find(set, id, io) else { return 1 };
    // The record as written: read again with the lstat-first open, and printed only as `parse_record` accepts it.
    let bytes = read_bytes(&top.join(DIR).join(format!("{}.md", r.id))).ok().filter(|b| parse_record(&r.id, b).is_ok());
    let Some(bytes) = bytes else {
        line(io.err, &format!("zygo decision: {id} is not a valid record"));
        return 1;
    };
    let text = String::from_utf8_lossy(&bytes);
    for l in text.strip_suffix('\n').unwrap_or(&text).split('\n') {
        line(io.out, l);
    }
    let rows = dependents(&set.valid, &r.id).rows;
    let direct = rows.iter().filter(|(d, _)| *d == 1).count();
    line(io.out, &format!("dependents: {direct} direct, {} total", rows.len()));
    match (repo.commits)(top, &r.id, SHOW_COMMITS) {
        Ok(commits) => {
            line(io.out, &format!("commits: {}", commits.len()));
            for (hash, subject) in commits {
                line(io.out, &format!("{hash} {subject}"));
            }
        }
        Err(e) => line(io.err, &e),
    }
    0
}

fn deps(set: &RecordSet, id: &str, io: &mut DecisionIo) -> i32 {
    malformed_lines(set, io);
    let Some(r) = find(set, id, io) else { return 1 };
    let d = dependents(&set.valid, &r.id);
    for (depth, rid) in &d.rows {
        if let Some(x) = set.valid.iter().find(|x| &x.id == rid) {
            line(io.out, &format!("{depth} {rid} {} {} {}", x.kind.name(), x.status.name(), x.title));
        }
    }
    if let Some(c) = d.cycle_at {
        line(io.err, &format!("zygo decision: cycle at {c}"));
    }
    0
}

fn check(set: &RecordSet, io: &mut DecisionIo) -> i32 {
    let mut any = false;
    for (name, faults) in &set.malformed {
        for fault in faults {
            line(io.out, &format!("{name}: {fault}"));
            any = true;
        }
    }
    for fault in set_faults(set) {
        line(io.out, &fault);
        any = true;
    }
    i32::from(any)
}

/// [`run_in`] with the repository of `cwd` (2 s bound) and the real `git log`.
pub fn run_with(cwd: &Path, env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32 {
    let top = || repo::checkout(cwd, Instant::now() + Duration::from_secs(2)).map(|c| c.top);
    run_in(&Repo { top: &top, commits: &trailer_commits }, env, args, io)
}

/// The CLI arm: the process cwd, env and standard streams.
pub fn run(args: &[String]) -> i32 {
    let cwd = match super::cwd_or_exit() {
        Ok(cwd) => cwd,
        Err(code) => return code,
    };
    let stdin = std::io::stdin();
    let tty = std::io::IsTerminal::is_terminal(&stdin);
    let (mut input, mut out, mut err) = (stdin.lock(), std::io::stdout().lock(), std::io::stderr().lock());
    let mut io = DecisionIo { input: &mut input, out: &mut out, err: &mut err, tty };
    let code = run_with(&cwd, &|k| std::env::var_os(k), args, &mut io);
    let _ = io.out.flush();
    code
}

/// The slash form (D-C2): the reply names the shell command; it reads no file, since it runs inside the hook.
pub fn verb(_ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    Ok(format!("run in your shell: zygo decision {}", esc(&args.join(" "))).trim_end().to_string())
}

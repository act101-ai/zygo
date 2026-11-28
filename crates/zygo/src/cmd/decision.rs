//! `zygo decision` (decision records design D§10.1): delegated decision records in `docs/decisions/`.
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zygo_core::esc;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_work::decisions::edit::{allowed, apply, lock_path, EditError, Outcome, Promotion};
use zygo_work::decisions::graph::{count_in, dependents, inverse_of};
use zygo_work::decisions::history::trailer_commits;
use zygo_work::decisions::new::{self as new_rec, check_args, NewArgs};
use zygo_work::decisions::record::{parse_record, read_bytes, Class, Kind, Record, Status, DIR};
use zygo_work::decisions::review::Queue;
use zygo_work::decisions::set::{read_dir, set_faults, DirError, RecordSet};
use zygo_work::hooks::session_id;
use zygo_work::repo;

use super::verbs::VerbCtx;

const USAGE: &str = "zygo decision: usage: zygo decision new --kind <k> --title <t> --unit <u> --round <n> --role <r> \
    [--class <c>] [--depends-on <id>]... [--supersedes <id>] | ls [--status <s>] [--kind <k>] [--unit <prefix>] [--pending] \
    | show <id> | deps <id> | check | ratify <id> [--note <text>] [--promote adr|spec|rule --as <text>] [--yes] \
    | overturn <id> --reason <text> [--yes] | review [--kind <k>] [--unit <prefix>]";
/// The commits `show` lists at most.
const SHOW_COMMITS: usize = 50;
/// The commits `review` lists at most per record (D§11.2 step 3).
const REVIEW_COMMITS: usize = 20;
/// The longest `review_note` (the ratify note and the overturn reason), in bytes: `parse_record` refuses more
/// (crates/zygo-work/src/decisions/record.rs, `review_note`).
const NOTE_MAX: usize = 2000;
/// The longest `promotion`, in bytes: `parse_record` refuses more (record.rs, `promotion`).
const PROMOTION_MAX: usize = 500;

/// The streams of one `zygo decision` run.
pub struct DecisionIo<'a> {
    pub input: &'a mut dyn BufRead,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    /// Whether stdin and stdout are terminals.
    pub tty: bool,
}

/// The repository facts `run_in` reads, as values so tests run no git and read no clock.
pub struct Repo<'a> {
    /// The repository top of the current directory and its git common dir; `None` outside a git repository. Called
    /// after the arguments parse. `apply` locks `edit::lock_path` of the common dir, outside the work tree.
    pub checkout: &'a dyn Fn() -> Option<(PathBuf, PathBuf)>,
    /// `history::trailer_commits` outside tests.
    pub commits: &'a CommitsFn,
    /// `HEAD` of a top; `decisions::new::head` outside tests.
    pub head: &'a dyn Fn(&Path) -> Option<String>,
    /// Today, UTC, `YYYY-MM-DD`; `decisions::new::today_utc` outside tests.
    pub today: &'a dyn Fn() -> String,
    /// The usage store; `Paths::from_env().usage_db` outside tests.
    pub usage_db: &'a Path,
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
    New(NewArgs),
    Ls(Filter),
    Show(String),
    Deps(String),
    Check,
    Decide(DecideArgs),
    /// `review [--kind <k>] [--unit <prefix>]` (D§11.2).
    Review { kind: Option<Kind>, unit: Option<String> },
}

/// The arguments of `ratify` and `overturn` (D§10.1).
struct DecideArgs {
    /// `ratify` or `overturn`.
    verb: &'static str,
    id: String,
    outcome: Outcome,
    yes: bool,
}

/// The subcommand of `args`; `None` for any argument error.
fn parse(args: &[String]) -> Option<Sub> {
    let (first, rest) = args.split_first()?;
    match (first.as_str(), rest) {
        ("new", _) => parse_new(rest).map(Sub::New),
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
        ("ratify", [id, rest @ ..]) => parse_decide("ratify", id, rest).map(Sub::Decide),
        ("overturn", [id, rest @ ..]) => parse_decide("overturn", id, rest).map(Sub::Decide),
        ("review", _) => {
            let (mut kind, mut unit) = (None, None);
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--kind" => kind = Some(Kind::parse(it.next()?)?),
                    "--unit" => unit = Some(it.next()?.clone()),
                    _ => return None,
                }
            }
            Some(Sub::Review { kind, unit })
        }
        _ => None,
    }
}

/// The `new` flags: a later flag replaces an earlier one, `--depends-on` appends, `--class` defaults to `none`; `None`
/// for an unknown flag, a missing value, a bad `--kind`, `--class` or `--round`, or a missing required flag.
fn parse_new(rest: &[String]) -> Option<NewArgs> {
    let (mut kind, mut title, mut unit, mut round, mut role) = (None, None, None, None, None);
    let (mut class, mut depends_on, mut supersedes) = (Class::None, Vec::new(), None);
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        let v = it.next()?;
        match a.as_str() {
            "--kind" => kind = Some(Kind::parse(v)?),
            "--title" => title = Some(v.clone()),
            "--unit" => unit = Some(v.clone()),
            "--round" => round = Some(v.parse::<u32>().ok()?),
            "--role" => role = Some(v.clone()),
            "--class" => class = Class::parse(v)?,
            "--depends-on" => depends_on.push(v.clone()),
            "--supersedes" => supersedes = Some(v.clone()),
            _ => return None,
        }
    }
    Some(NewArgs { kind: kind?, title: title?, unit: unit?, round: round?, role: role?, class, depends_on, supersedes })
}

/// The `ratify` and `overturn` flags: a later flag replaces an earlier one; `None` for an unknown flag, a missing
/// value, a bad `--promote`, `--promote` without `--as` or the reverse, an empty `--as`, or a missing or empty
/// `--reason`.
fn parse_decide(verb: &'static str, id: &str, rest: &[String]) -> Option<DecideArgs> {
    let (mut note, mut promote, mut as_text, mut reason, mut yes) = (String::new(), None, None, None, false);
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match (verb, a.as_str()) {
            (_, "--yes") => yes = true,
            ("ratify", "--note") => note = it.next()?.clone(),
            ("ratify", "--promote") => promote = Some(Promotion::parse(it.next()?)?),
            ("ratify", "--as") => as_text = Some(it.next()?.clone()),
            ("overturn", "--reason") => reason = Some(it.next()?.clone()),
            _ => return None,
        }
    }
    let outcome = if verb == "ratify" {
        let promote = match (promote, as_text) {
            (None, None) => None,
            (Some(p), Some(t)) if !t.is_empty() => Some((p, t)),
            _ => return None,
        };
        Outcome::Ratify { note, promote }
    } else {
        Outcome::Overturn { reason: reason.filter(|r| !r.is_empty())? }
    };
    Some(DecideArgs { verb, id: id.to_string(), outcome, yes })
}

/// The session directory of `ZYGO_SESSION_DIR`; `None` when unset or empty.
fn session_dir(env: &dyn Fn(&str) -> Option<OsString>) -> Option<SessionDir> {
    env("ZYGO_SESSION_DIR").filter(|v| !v.is_empty()).map(|v| SessionDir::new(PathBuf::from(v)))
}

/// D§14 row 1: `docs` or `docs/decisions` is absent. Each part is checked with `lstat` in turn, as `read_dir` does, so a
/// link at `docs` is never followed: a link, a file, or an `lstat` error other than absent is not absent, and
/// `read_dir` reports it (D§6.8).
fn records_dir_absent(top: &Path) -> bool {
    let dir = top.join(DIR);
    for path in [dir.parent().unwrap_or(&dir), &dir] {
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return true,
            Ok(m) if m.file_type().is_dir() => {}
            _ => return false,
        }
    }
    false
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

/// Whether `args` hold `-h` or `--help` in any position but the value of a flag (`--title -h` is a title).
fn wants_help(args: &[String]) -> bool {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => return true,
            "--kind" | "--title" | "--unit" | "--round" | "--role" | "--class" | "--depends-on" | "--supersedes"
            | "--status" | "--note" | "--promote" | "--as" | "--reason" => {
                it.next();
            }
            _ => {}
        }
    }
    false
}

/// One arm per subcommand; gives the exit code. `-h` or `--help` in any position but a flag value prints the usage line
/// to stdout.
pub fn run_in(repo: &Repo, env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32 {
    if wants_help(args) {
        line(io.out, USAGE);
        return 0;
    }
    let Some(sub) = parse(args) else {
        line(io.err, USAGE);
        return 2;
    };
    if let Sub::New(a) = &sub {
        // The session legend; `None` outside a session or when the view does not load.
        let legend = session_dir(env)
            .and_then(|sd| SessionView::load(&sd).ok())
            .map(|v| v.legend.into_iter().map(|e| e.role).collect::<Vec<_>>());
        if let Err(e) = check_args(a, legend.as_deref()) {
            line(io.err, &e);
            return 2;
        }
    }
    // D§11.1, D-C5: the operator verbs never run in an agent session.
    let operator_verb = match &sub {
        Sub::Decide(a) => Some(a.verb),
        Sub::Review { .. } => Some("review"),
        _ => None,
    };
    // Set is enough, empty included: `session_dir` drops an empty value, so it is not asked here.
    if let Some(verb) = operator_verb
        && env("ZYGO_SESSION_DIR").is_some()
    {
        line(io.err, &format!("zygo decision {verb}: run it in your own shell, not in an agent session"));
        return 2;
    }
    // D§11.1: `review` asks on a terminal.
    if matches!(sub, Sub::Review { .. }) && !io.tty {
        line(io.err, "zygo decision review: needs a terminal; use ratify or overturn with --yes in scripts");
        return 2;
    }
    let Some((top, common_dir)) = (repo.checkout)() else {
        line(io.err, "zygo decision: not in a git repository");
        return 2;
    };
    // D§14 row 1: no records directory, nothing to review.
    if operator_verb.is_some() && records_dir_absent(&top) {
        line(io.out, "no open records");
        return 0;
    }
    let set = match read_dir(&top) {
        Ok(set) => set,
        Err(DirError::NotDirectory) => {
            line(io.err, "zygo decision: docs/decisions is not a directory");
            return 1;
        }
        Err(DirError::Unreadable) => {
            line(io.err, "zygo decision: cannot read docs/decisions");
            return 1;
        }
        Err(DirError::TooMany) => {
            line(io.err, "zygo decision: more than 10,000 files in docs/decisions/");
            return 1;
        }
    };
    match sub {
        Sub::New(a) => new(repo, env, &top, &set, &a, io),
        Sub::Ls(f) => ls(&set, &f, io),
        Sub::Show(id) => show(repo, &top, &set, &id, io),
        Sub::Deps(id) => deps(&set, &id, io),
        Sub::Check => check(&set, io),
        Sub::Decide(a) => decide(repo, &top, &lock_path(&common_dir), &set, &a, io),
        Sub::Review { kind, unit } => review(repo, &top, &lock_path(&common_dir), &set, kind, unit.as_deref(), io),
    }
}

/// D§10.2: each linked id a valid record, then `HEAD`, the model and the file; prints its path.
fn new(repo: &Repo, env: &dyn Fn(&str) -> Option<OsString>, top: &Path, set: &RecordSet, a: &NewArgs, io: &mut DecisionIo) -> i32 {
    for id in a.depends_on.iter().chain(&a.supersedes) {
        if find(set, id, io).is_none() {
            return 1;
        }
    }
    let Some(commit) = (repo.head)(top) else {
        line(io.err, "zygo decision: cannot read HEAD");
        return 1;
    };
    let model = match session_dir(env) {
        Some(sd) => new_rec::session_model(repo.usage_db, &session_id(&sd), &a.role),
        None => "unknown".to_string(),
    };
    match new_rec::create(top, a, &model, &commit, &(repo.today)()) {
        Ok(path) => {
            line(io.out, &path.display().to_string());
            0
        }
        Err(e) => {
            line(io.err, &e);
            1
        }
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
    print_lines(io.out, &bytes);
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

/// Each line of `bytes`, without one trailing CR, through `esc`.
fn print_lines(w: &mut dyn Write, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    for l in text.strip_suffix('\n').unwrap_or(&text).split('\n') {
        line(w, l.strip_suffix('\r').unwrap_or(l));
    }
}

/// D§11.3 `ratify` and `overturn`: the D§6.6 check on the record it shows, the record and the `[y/N]` prompt unless
/// `--yes`, then `edit::apply`, which re-reads the file and writes only when it is unchanged since it was shown; prints
/// its path.
fn decide(repo: &Repo, top: &Path, lock: &Path, set: &RecordSet, a: &DecideArgs, io: &mut DecisionIo) -> i32 {
    let Some(r) = find(set, &a.id, io) else { return 1 };
    let path = top.join(DIR).join(format!("{}.md", r.id));
    let Some((shown, now)) = read_bytes(&path).ok().and_then(|b| parse_record(&r.id, &b).ok().map(|now| (b, now))) else {
        line(io.err, &format!("zygo decision: {} is not a valid record", r.id));
        return 1;
    };
    if let Err(e) = allowed(&now, &a.outcome) {
        line(io.err, &e);
        return 1;
    }
    if !a.yes {
        print_lines(io.out, &shown);
        let _ = write!(io.out, "{} {}? [y/N] ", a.verb, esc(&r.id));
        let _ = io.out.flush();
        let mut answer = String::new();
        let yes = io.input.read_line(&mut answer).is_ok()
            && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes");
        if !yes {
            line(io.err, &format!("zygo decision: {} not changed", r.id));
            return 1;
        }
    }
    match apply(&path, lock, &shown, &a.outcome, &(repo.today)()) {
        Ok(()) => {
            line(io.out, &path.display().to_string());
            0
        }
        Err(EditError::Changed) => {
            line(io.err, &format!("{} changed since it was shown; skipped", r.id));
            1
        }
        Err(EditError::Refused(e) | EditError::Io(e)) => {
            line(io.err, &e);
            1
        }
    }
}

/// What the operator picks for one shown record.
enum Pick {
    Do(Outcome),
    Skip,
    Quit,
}

/// Prints `prompt` and reads one answer without its line end; `None` (after a newline) at EOF or a read error.
fn ask(io: &mut DecisionIo, prompt: &str) -> Option<String> {
    let _ = write!(io.out, "{prompt}");
    let _ = io.out.flush();
    let mut s = String::new();
    match io.input.read_line(&mut s) {
        Ok(n) if n > 0 => Some(s.trim_end_matches(['\n', '\r']).to_string()),
        _ => {
            let _ = writeln!(io.out);
            None
        }
    }
}

/// Asks `prompt` until `take` accepts the answer; `None` at EOF or a read error.
fn ask_until<T>(io: &mut DecisionIo, prompt: &str, take: impl Fn(String) -> Option<T>) -> Option<T> {
    loop {
        if let Some(v) = take(ask(io, prompt)?) {
            return Some(v);
        }
    }
}

/// Asks `prompt` until the answer has at most `max` bytes and, unless `empty_ok`, is not empty; a longer answer prints
/// `too long: at most <max> bytes` first. `None` at EOF or a read error.
fn ask_text(io: &mut DecisionIo, prompt: &str, max: usize, empty_ok: bool) -> Option<String> {
    loop {
        let a = ask(io, prompt)?;
        if a.len() > max {
            line(io.out, &format!("too long: at most {max} bytes"));
        } else if empty_ok || !a.is_empty() {
            return Some(a);
        }
    }
}

/// The D§11.3 outcome of one record: the action prompt, then its sub-prompts; an unknown action, a bad target, an
/// empty promotion text or reason, and a text over its record limit ask again; EOF quits.
fn choose(io: &mut DecisionIo) -> Pick {
    let picked = ask_until(io, "r ratify · p promote · o overturn · s skip · q quit ", |a| {
        matches!(a.trim(), "r" | "p" | "o" | "s" | "q").then(|| a.trim().to_string())
    });
    let outcome = match picked.as_deref() {
        Some("r") => ask_text(io, "note (empty for none): ", NOTE_MAX, true).map(|note| Outcome::Ratify { note, promote: None }),
        Some("p") => ask_until(io, "promote to adr, spec or rule: ", |t| Promotion::parse(t.trim())).and_then(|p| {
            ask_text(io, "promotion text: ", PROMOTION_MAX, false)
                .map(|t| Outcome::Ratify { note: String::new(), promote: Some((p, t)) })
        }),
        Some("o") => ask_text(io, "reason: ", NOTE_MAX, false).map(|reason| Outcome::Overturn { reason }),
        Some("s") => return Pick::Skip,
        _ => None,
    };
    outcome.map_or(Pick::Quit, Pick::Do)
}

/// The outcome counts and the written files of one `review`.
#[derive(Default)]
struct Tally {
    ratified: usize,
    promoted: usize,
    overturned: usize,
    skipped: usize,
    files: Vec<String>,
}

/// D§11.2 step 3: the header, the record, its `depends_on` statuses and its commits.
fn review_block(repo: &Repo, top: &Path, queue: &Queue, statuses: &HashMap<String, Status>, r: &Record, at: (usize, usize), io: &mut DecisionIo) {
    let (i, n) = at;
    let dependents = queue.dependents(&r.id);
    line(io.out, &format!("[{i}/{n}] {} {} class {} dependents {dependents}", r.id, r.kind.name(), r.class.name()));
    line(io.out, &r.title);
    line(io.out, &format!("unit {} round {} role {} model {} date {}", r.unit, r.round, r.role, r.model, r.date));
    if let Some(c) = queue.cause(&r.id) {
        line(io.out, &format!("depends on overturned {c}"));
    }
    for d in &r.depends_on {
        let status = match statuses.get(d) {
            Some(Status::Overturned) => "(overturned)",
            Some(s) => s.name(),
            None => "missing",
        };
        line(io.out, &format!("depends_on {d} {status}"));
    }
    print_lines(io.out, r.body.as_bytes());
    match (repo.commits)(top, &r.id, REVIEW_COMMITS) {
        Ok(commits) => {
            line(io.out, &format!("commits: {}", commits.len()));
            for (hash, subject) in commits {
                line(io.out, &format!("{hash} {subject}"));
            }
        }
        Err(e) => line(io.err, &e),
    }
}

/// D§11.2: each open record in dependents order, its outcome written (D§11.3, re-read first) before the next; at the
/// end or at `q` the counts, the written files and the commit line. An outcome that cannot be written ends the review
/// with exit 1; the outcomes before it stay written (D§14).
fn review(repo: &Repo, top: &Path, lock: &Path, set: &RecordSet, kind: Option<Kind>, unit: Option<&str>, io: &mut DecisionIo) -> i32 {
    for (name, _) in &set.malformed {
        line(io.err, &format!("zygo decision: {name}: cannot be reviewed; zygo decision check names the fault"));
    }
    let mut queue = Queue::new(&set.valid, kind, unit);
    let mut total = queue.remaining();
    if total == 0 {
        line(io.out, "no open records");
        return 0;
    }
    let mut statuses: HashMap<String, Status> = set.valid.iter().map(|r| (r.id.clone(), r.status)).collect();
    // The records with the statuses written in this review: `Queue::overturned` takes an `open` one as not decided.
    let mut records = set.valid.clone();
    let (mut tally, mut shown_count) = (Tally::default(), 0);
    let code = loop {
        let Some(id) = queue.next() else { break 0 };
        let path = top.join(DIR).join(format!("{id}.md"));
        let Some((shown, r)) = read_bytes(&path).ok().and_then(|b| parse_record(&id, &b).ok().map(|r| (b, r))) else {
            line(io.err, &format!("zygo decision: {id} is not a valid record"));
            continue;
        };
        if r.status != Status::Open {
            line(io.err, &format!("zygo decision: {id} is {}", r.status.name()));
            continue;
        }
        shown_count += 1;
        review_block(repo, top, &queue, &statuses, &r, (shown_count, total), io);
        let outcome = match choose(io) {
            Pick::Do(o) => o,
            Pick::Skip => {
                tally.skipped += 1;
                continue;
            }
            Pick::Quit => break 0,
        };
        match apply(&path, lock, &shown, &outcome, &(repo.today)()) {
            Ok(()) => {
                let status = match &outcome {
                    Outcome::Ratify { promote: Some(_), .. } => {
                        tally.promoted += 1;
                        Status::Ratified
                    }
                    Outcome::Ratify { promote: None, .. } => {
                        tally.ratified += 1;
                        Status::Ratified
                    }
                    Outcome::Overturn { .. } => {
                        tally.overturned += 1;
                        Status::Overturned
                    }
                };
                statuses.insert(id.clone(), status);
                if let Some(x) = records.iter_mut().find(|x| x.id == id) {
                    x.status = status;
                }
                tally.files.push(format!("{DIR}/{id}.md"));
                if status == Status::Overturned {
                    // A skipped dependent queued again adds one to the `N` of `[i/N]`.
                    let before = queue.remaining();
                    queue.overturned(&id, &records);
                    total += queue.remaining() - before;
                }
            }
            Err(EditError::Changed) => {
                line(io.err, &format!("{id} changed since it was shown; skipped"));
                tally.skipped += 1;
            }
            Err(EditError::Refused(e)) => {
                line(io.err, &e);
                tally.skipped += 1;
            }
            Err(EditError::Io(e)) => {
                line(io.err, &e);
                break 1;
            }
        }
    };
    let Tally { ratified, promoted, overturned, skipped, files } = tally;
    line(io.out, &format!("ratified {ratified}, promoted {promoted}, overturned {overturned}, skipped {skipped}"));
    for f in files {
        line(io.out, &f);
    }
    line(io.out, "commit these files; add a Zygo-Decision: line per id");
    code
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

/// [`run_in`] with the repository of `cwd` (2 s bound), the real `git log`, `HEAD`, clock and usage store.
pub fn run_with(cwd: &Path, env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32 {
    let checkout = || repo::checkout(cwd, Instant::now() + Duration::from_secs(2)).map(|c| (c.top, c.common_dir));
    let usage_db = Paths::from_env().usage_db;
    let repo = Repo { checkout: &checkout, commits: &trailer_commits, head: &new_rec::head, today: &new_rec::today_utc, usage_db: &usage_db };
    run_in(&repo, env, args, io)
}

/// The CLI arm: the process cwd, env and standard streams.
pub fn run(args: &[String]) -> i32 {
    let cwd = match super::cwd_or_exit() {
        Ok(cwd) => cwd,
        Err(code) => return code,
    };
    let stdin = std::io::stdin();
    let tty = std::io::IsTerminal::is_terminal(&stdin) && std::io::IsTerminal::is_terminal(&std::io::stdout());
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

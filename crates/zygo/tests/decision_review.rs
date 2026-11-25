//! DR-6: zygo decision review (D§11.1, D§11.2, D§11.3, D§10.1, D§14, D§17 #12, #13).
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use zygo::cmd::decision::{run_in, DecisionIo, Repo};
use zygo_work::decisions::record::{parse_record, Kind, Record};
use zygo_work::decisions::review::Queue;
use zygo_work::decisions::set::read_dir;

mod common;

const C40: &str = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556";
const BODY: &str = "\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n";
const TODAY: &str = "2026-12-01";
const RATIFIED: &str = "reviewed = \"2026-11-30\"\nreview_note = \"\"\n";
fn id(n: u32) -> String { format!("DDR-01JDQ2Y7Z8K4M5N6P7Q8R{n:05}") }
fn md(n: u32) -> String { format!("{}.md", id(n)) }
/// A valid record text; `title` is a TOML basic-string body, `extra` more front-matter lines.
#[allow(clippy::too_many_arguments)]
fn text(rid: &str, kind: &str, status: &str, title: &str, unit: &str, deps: &[u32], extra: &str, body: &str) -> String {
    let deps: Vec<String> = deps.iter().map(|d| id(*d)).collect();
    format!("+++\nid = \"{rid}\"\nkind = \"{kind}\"\nstatus = \"{status}\"\nclass = \"none\"\ntitle = \"{title}\"\nunit = \"{unit}\"\nround = 1\nrole = \"code\"\nmodel = \"unknown\"\ncommit = \"{C40}\"\ndate = \"2026-11-24\"\ndepends_on = {deps:?}\nreverse_cost = \"low\"\n{extra}+++\n{body}")
}
fn dir(top: &Path) -> PathBuf { let d = top.join("docs/decisions"); fs::create_dir_all(&d).unwrap(); d }
/// Fixture G row `n`: (kind, status, unit, depends_on, extra).
fn row(n: u32) -> (&'static str, &'static str, &'static str, &'static [u32], &'static str) {
    match n {
        1 => ("assumption", "open", "phase-3:N1", &[], ""),
        2 => ("decision", "open", "phase-3:N1", &[1], ""),
        3 => ("decision", "open", "phase-3:N1", &[2], ""),
        4 => ("decision", "open", "phase-3:N2", &[1], ""),
        5 => ("decision", "open", "phase-3:N1", &[], ""),
        6 => ("decision", "superseded", "phase-3:N1", &[5], ""),
        7 => ("decision", "ratified", "phase-3:N1", &[5], RATIFIED),
        8 => ("decision", "open", "phase-3:N1", &[6], ""),
        _ => unreachable!(),
    }
}
/// The fixture G text of row `n`, titled `T<n>`.
fn g_text(n: u32) -> String {
    let (kind, status, unit, deps, extra) = row(n);
    text(&id(n), kind, status, &format!("T{n}"), unit, deps, extra, BODY)
}
/// Writes the fixture G rows `ns` under `top`.
fn put_rows(top: &Path, ns: &[u32]) {
    for &n in ns {
        fs::write(dir(top).join(md(n)), g_text(n)).unwrap();
    }
}
/// Writes fixture G and gives its valid records.
fn graph(top: &Path) -> Vec<Record> {
    put_rows(top, &[1, 2, 3, 4, 5, 6, 7, 8]);
    read_dir(top).unwrap().valid
}
fn drain(q: &mut Queue) -> Vec<String> { std::iter::from_fn(|| q.next()).collect() }
fn ids(ns: &[u32]) -> Vec<String> { ns.iter().map(|n| id(*n)).collect() }

#[test] fn queue_orders_by_dependents_then_id() { // D§11.2 step 2
    let t = tempfile::tempdir().unwrap();
    let g = graph(t.path());
    let mut q = Queue::new(&g, None, None);
    assert_eq!(q.remaining(), 6);
    assert_eq!(q.dependents(&id(1)), 3);
    assert_eq!(q.dependents(&id(5)), 1);
    assert_eq!(drain(&mut q), ids(&[1, 2, 5, 3, 4, 8]));
}
#[test] fn queue_filters_by_kind_and_unit_and_counts_over_all_records() { // D§11.2 step 1
    let t = tempfile::tempdir().unwrap();
    let g = graph(t.path());
    let mut q = Queue::new(&g, Some(Kind::Assumption), None);
    assert_eq!(q.dependents(&id(1)), 3);
    assert_eq!(drain(&mut q), ids(&[1]));
    assert_eq!(drain(&mut Queue::new(&g, None, Some("phase-3:N2"))), ids(&[4]));
}
#[test] fn overturned_moves_the_open_dependents_to_the_front_in_step_2_order() { // D§11.2 step 4
    let t = tempfile::tempdir().unwrap();
    let g = graph(t.path());
    let mut q = Queue::new(&g, None, None);
    assert_eq!((q.next(), q.next()), (Some(id(1)), Some(id(2))));
    q.overturned(&id(2), &g);
    assert_eq!(q.next(), Some(id(3)));
    let mut q = Queue::new(&g, None, None);
    assert_eq!((q.next(), q.next()), (Some(id(1)), Some(id(2))));
    q.overturned(&id(1), &g);
    assert_eq!(q.cause(&id(3)), Some(id(1).as_str()));
    assert_eq!(q.cause(&id(5)), None);
    // 2 was returned and is still open in `g` (skipped): it is queued again, in step 2 order.
    assert_eq!(q.cause(&id(2)), Some(id(1).as_str()));
    assert_eq!(drain(&mut q), ids(&[2, 3, 4, 5, 8]));
    // 7 is not queued and 8 depends only on a superseded record: overturning 5 moves nothing.
    let mut q = Queue::new(&g, None, None);
    q.overturned(&id(5), &g);
    assert_eq!(q.cause(&id(8)), None);
    assert_eq!(drain(&mut q), ids(&[1, 2, 5, 3, 4, 8]));
}
/// Records 1 and 2, open, each depending on the other.
fn cycle(top: &Path) -> Vec<Record> {
    fs::write(dir(top).join(md(1)), text(&id(1), "decision", "open", "T1", "phase-3:N1", &[2], "", BODY)).unwrap();
    fs::write(dir(top).join(md(2)), text(&id(2), "decision", "open", "T2", "phase-3:N1", &[1], "", BODY)).unwrap();
    read_dir(top).unwrap().valid
}
#[test] fn overturned_on_a_cycle_ends_and_queues_each_id_once() { // D§11.2 step 5
    let t = tempfile::tempdir().unwrap();
    let g = cycle(t.path());
    let mut q = Queue::new(&g, None, None);
    assert_eq!((q.next(), q.next()), (Some(id(1)), Some(id(2))));
    q.overturned(&id(2), &g);
    q.overturned(&id(2), &g);
    assert_eq!(q.remaining(), 1);
    assert_eq!(q.cause(&id(1)), Some(id(2).as_str()));
    assert_eq!(drain(&mut q), ids(&[1]));
}

const USAGE: &str = "zygo decision: usage: zygo decision new --kind <k> --title <t> --unit <u> --round <n> --role <r> [--class <c>] [--depends-on <id>]... [--supersedes <id>] | ls [--status <s>] [--kind <k>] [--unit <prefix>] [--pending] | show <id> | deps <id> | check | ratify <id> [--note <text>] [--promote adr|spec|rule --as <text>] [--yes] | overturn <id> --reason <text> [--yes] | review [--kind <k>] [--unit <prefix>]\n";
const PROMPT: &str = "r ratify · p promote · o overturn · s skip · q quit";
const COMMIT_LINE: &str = "commit these files; add a Zygo-Decision: line per id\n";
const SESSION_LINE: &str = "zygo decision review: run it in your own shell, not in an agent session\n";
const TTY_LINE: &str = "zygo decision review: needs a terminal; use ratify or overturn with --yes in scripts\n";
#[derive(Debug)]
struct Ran { code: i32, out: String, err: String }
/// `run_in` with `top` (common dir `top/.git`), today TODAY, the terminal flag `tty`, stdin `input`, and `env` for
/// ZYGO_SESSION_DIR; no git, no clock, no process env. One commit carries the trailer of record 1.
fn go(top: &Path, env: &dyn Fn(&str) -> Option<OsString>, tty: bool, input: &mut dyn BufRead, args: &[&str]) -> Ran {
    let first = id(1);
    let list = move |_: &Path, rid: &str, limit: usize| {
        assert_eq!(limit, 20);
        Ok(if rid == first { vec![("a".repeat(40), "first".to_string())] } else { vec![] })
    };
    let checkout = || {
        let c = top.join(".git");
        fs::create_dir_all(&c).unwrap();
        Some((top.to_path_buf(), c))
    };
    let today = || TODAY.to_string();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut io = DecisionIo { input, out: &mut out, err: &mut err, tty };
    let code = run_in(&Repo { checkout: &checkout, commits: &list, head: &|_| None, today: &today, usage_db: Path::new("") }, env, &args, &mut io);
    Ran { code, out: String::from_utf8(out).unwrap(), err: String::from_utf8(err).unwrap() }
}
fn review(top: &Path, input: &str) -> Ran { go(top, &|_| None, true, &mut input.as_bytes(), &["review"]) }
/// The `[i/N]` header lines; a header can follow a prompt on its line, since prompts end with no newline.
fn headers(out: &str) -> Vec<&str> { out.lines().filter_map(header).collect() }
fn header(l: &str) -> Option<&str> {
    let at = l.rfind('[')?;
    l[at + 1..].starts_with(|c: char| c.is_ascii_digit()).then(|| &l[at..])
}
fn file(top: &Path, n: u32) -> String { fs::read_to_string(top.join("docs/decisions").join(md(n))).unwrap() }
fn record(top: &Path, n: u32) -> Record { parse_record(&id(n), file(top, n).as_bytes()).unwrap() }
fn ending(summary: &str, files: &[u32]) -> String {
    let files: String = files.iter().map(|n| format!("docs/decisions/{}\n", md(*n))).collect();
    format!("{summary}\n{files}{COMMIT_LINE}")
}
/// Stdin with one answer per read: it records the status of the record at `probe` at each answer, and writes `edit`
/// at its first read (an edit while a prompt waits). EOF when no answer is left.
struct Script { answers: VecDeque<String>, probe: PathBuf, seen: Vec<String>, edit: Option<(PathBuf, String)> }
impl Script {
    fn new(probe: PathBuf, answers: &[&str]) -> Script {
        Script { answers: answers.iter().map(|a| a.to_string()).collect(), probe, seen: vec![], edit: None }
    }
}
impl Read for Script {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if let Some((p, t)) = self.edit.take() {
            fs::write(p, t)?;
        }
        let Some(a) = self.answers.pop_front() else { return Ok(0) };
        let bytes = fs::read(&self.probe)?;
        let stem = self.probe.file_stem().unwrap().to_str().unwrap();
        self.seen.push(parse_record(stem, &bytes).unwrap().status.name().to_string());
        buf[..a.len()].copy_from_slice(a.as_bytes());
        Ok(a.len())
    }
}
fn all_files(top: &Path) -> Vec<String> { (1..=8).map(|n| file(top, n)).collect() }

#[test] fn review_exits_2_in_an_agent_session() { // D§11.1
    let t = tempfile::tempdir().unwrap();
    graph(t.path());
    let before = all_files(t.path());
    let session = t.path().join("session");
    let env = move |k: &str| (k == "ZYGO_SESSION_DIR").then(|| session.clone().into_os_string());
    let r = go(t.path(), &env, true, &mut "s\n".as_bytes(), &["review"]);
    assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (2, "", SESSION_LINE));
    assert_eq!(all_files(t.path()), before);
}
#[test] fn review_without_a_terminal_exits_2() { // D§11.1
    let t = tempfile::tempdir().unwrap();
    graph(t.path());
    let r = go(t.path(), &|_| None, false, &mut "s\n".as_bytes(), &["review"]);
    assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (2, "", TTY_LINE));
}
#[test] fn the_binary_refuses_without_a_terminal_and_in_a_session() { // D§11.1, D§17 #12
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut cmd, &home).current_dir(t.path()).args(["decision", "review"]).stdin(Stdio::null());
    let o = cmd.env_remove("ZYGO_SESSION_DIR").output().unwrap();
    assert_eq!((o.status.code(), String::from_utf8_lossy(&o.stderr).as_ref()), (Some(2), TTY_LINE));
    let o = cmd.env("ZYGO_SESSION_DIR", t.path().join("session")).output().unwrap();
    assert_eq!((o.status.code(), String::from_utf8_lossy(&o.stderr).as_ref()), (Some(2), SESSION_LINE));
}
#[test] fn review_shows_records_in_dependents_order() { // D§11.2 steps 2, 3, 6
    let t = tempfile::tempdir().unwrap();
    graph(t.path());
    let before = all_files(t.path());
    let r = review(t.path(), &"s\n".repeat(6));
    let want = [
        format!("[1/6] {} assumption class none dependents 3", id(1)),
        format!("[2/6] {} decision class none dependents 1", id(2)),
        format!("[3/6] {} decision class none dependents 1", id(5)),
        format!("[4/6] {} decision class none dependents 0", id(3)),
        format!("[5/6] {} decision class none dependents 0", id(4)),
        format!("[6/6] {} decision class none dependents 0", id(8)),
    ];
    assert_eq!(headers(&r.out), want.iter().map(String::as_str).collect::<Vec<_>>(), "{}", r.out);
    assert!(r.out.contains(&format!("depends_on {} open\n", id(1))));
    assert!(r.out.contains(&format!("depends_on {} superseded\n", id(6))));
    assert_eq!(r.out.matches(&format!("commits: 1\n{} first\n", "a".repeat(40))).count(), 1);
    assert_eq!(r.out.matches(PROMPT).count(), 6);
    assert!(r.out.ends_with(&ending("ratified 0, promoted 0, overturned 0, skipped 6", &[])), "{}", r.out);
    assert_eq!((r.code, r.err.as_str()), (0, ""));
    assert_eq!(all_files(t.path()), before);
}
#[test] fn after_an_overturn_its_open_dependents_come_next() { // D§11.2 step 5
    let t = tempfile::tempdir().unwrap();
    graph(t.path());
    let r = review(t.path(), &format!("o\nwrong\n{}", "s\n".repeat(5)));
    assert_eq!(r.code, 0, "{}", r.err);
    let hs: Vec<String> = headers(&r.out).iter().map(|h| h.split(' ').nth(1).unwrap().to_string()).collect();
    assert_eq!(hs, ids(&[1, 2, 3, 4, 5, 8]));
    let cause = format!("depends on overturned {}", id(1));
    assert_eq!(r.out.matches(&cause).count(), 3);
    let lines: Vec<&str> = r.out.lines().collect();
    for n in [2, 3, 4] {
        let at = lines.iter().position(|l| header(l).is_some_and(|h| h.contains(&id(n)))).unwrap();
        assert!(lines[at + 1..at + 4].contains(&cause.as_str()), "{n}: {:?}", &lines[at..at + 5]);
    }
    let block2 = &r.out[r.out.find(&format!("] {}", id(2))).unwrap()..r.out.find(&format!("] {}", id(3))).unwrap()];
    assert!(block2.contains(&format!("depends_on {} (overturned)\n", id(1))), "{block2}");
    let r1 = record(t.path(), 1);
    assert_eq!((r1.status.name(), r1.review_note.as_deref(), r1.reviewed.as_deref()), ("overturned", Some("wrong"), Some(TODAY)));
}
#[test] fn each_outcome_is_in_its_file_before_the_next_prompt() { // D§11.2 step 4, D§17 #12
    let t = tempfile::tempdir().unwrap();
    graph(t.path());
    let mut script = Script::new(t.path().join("docs/decisions").join(md(1)), &["r\n", "ok\n", "s\n"]);
    let r = go(t.path(), &|_| None, true, &mut BufReader::new(&mut script), &["review"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(script.seen, ["open", "open", "ratified"]);
    let r1 = record(t.path(), 1);
    assert_eq!((r1.status.name(), r1.review_note.as_deref()), ("ratified", Some("ok")));
    let again = review(t.path(), &"s\n".repeat(5));
    assert!(!headers(&again.out).iter().any(|h| h.contains(&id(1))), "{}", again.out);
    assert!(headers(&again.out)[0].starts_with(&format!("[1/5] {}", id(2))), "{}", again.out);
}
#[test] fn q_prints_counts_files_and_the_commit_line() { // D§11.2 step 6
    let t = tempfile::tempdir().unwrap();
    graph(t.path());
    let five = file(t.path(), 5);
    let r = review(t.path(), "r\n\np\nrule\nAdd it\nq\n");
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.ends_with(&ending("ratified 1, promoted 1, overturned 0, skipped 0", &[1, 2])), "{}", r.out);
    let r2 = record(t.path(), 2);
    assert_eq!((r2.promoted_to.as_deref(), r2.promotion.as_deref()), (Some("rule"), Some("Add it")));
    assert_eq!(file(t.path(), 5), five);
}
#[test] fn unknown_answers_ask_again_and_eof_quits() { // D§11.2 step 3, D§11.3 the reason is not empty
    let t = tempfile::tempdir().unwrap();
    graph(t.path());
    let r = review(t.path(), "x\no\n\nwrong\n");
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out.matches(PROMPT).count(), 3);
    assert_eq!(r.out.matches("reason: ").count(), 2);
    let r1 = record(t.path(), 1);
    assert_eq!((r1.status.name(), r1.review_note.as_deref()), ("overturned", Some("wrong")));
    assert!(r.out.ends_with(&ending("ratified 0, promoted 0, overturned 1, skipped 0", &[1])), "{}", r.out);
}
#[test] fn a_record_changed_while_shown_is_skipped() { // D§11.3, D§17 #13
    let t = tempfile::tempdir().unwrap();
    let p = dir(t.path()).join(md(1));
    fs::write(&p, text(&id(1), "decision", "open", "T", "phase-3:N1", &[], "", BODY)).unwrap();
    let edited = text(&id(1), "decision", "open", "T2", "phase-3:N1", &[], "", BODY);
    let mut script = Script::new(p.clone(), &["r\n", "\n"]);
    script.edit = Some((p.clone(), edited.clone()));
    let r = go(t.path(), &|_| None, true, &mut BufReader::new(&mut script), &["review"]);
    assert_eq!((r.code, r.err.as_str()), (0, format!("{} changed since it was shown; skipped\n", id(1)).as_str()));
    assert_eq!(fs::read_to_string(&p).unwrap(), edited);
    assert!(r.out.ends_with(&ending("ratified 0, promoted 0, overturned 0, skipped 1", &[])), "{}", r.out);
}
#[test] fn an_esc_in_a_title_prints_as_an_escape() { // D§10.1, D§17 #13
    let t = tempfile::tempdir().unwrap();
    fs::write(dir(t.path()).join(md(1)), text(&id(1), "decision", "open", "a\\u001bb", "phase-3:N1", &[], "", BODY)).unwrap();
    let r = review(t.path(), "q\n");
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.lines().any(|l| l == "a\\u{1b}b"), "{}", r.out);
    assert!(!r.out.contains('\u{1b}'));
}
#[test] fn failure_rows_of_d14() { // D§14 column `review, ratify, overturn`
    let no_open = |r: Ran| assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, "no open records\n", ""));
    let t = tempfile::tempdir().unwrap();
    no_open(review(t.path(), "")); // (a) no docs/decisions
    let t = tempfile::tempdir().unwrap();
    put_rows(t.path(), &[6, 7]);
    no_open(review(t.path(), "")); // (b) no open record
    let t = tempfile::tempdir().unwrap();
    graph(t.path());
    no_open(go(t.path(), &|_| None, true, &mut "".as_bytes(), &["review", "--kind", "compromise"])); // (c)
    let t = tempfile::tempdir().unwrap();
    fs::create_dir_all(t.path().join("docs")).unwrap();
    fs::write(t.path().join("docs/decisions"), "").unwrap();
    let r = review(t.path(), ""); // (d)
    assert_eq!((r.code, r.err.as_str()), (1, "zygo decision: docs/decisions is not a directory\n"));
    let t = tempfile::tempdir().unwrap();
    put_rows(t.path(), &[1]);
    let broken = text(&id(9), "decision", "open", "T", "phase-3:N1", &[], "", &BODY.replace("## Evidence", "## Proof"));
    fs::write(dir(t.path()).join(md(9)), broken).unwrap();
    let r = review(t.path(), "s\n"); // (e)
    assert_eq!(r.err, format!("zygo decision: {}: cannot be reviewed; zygo decision check names the fault\n", md(9)));
    assert_eq!(headers(&r.out).len(), 1, "{}", r.out);
    for args in [&["review", "--kind", "bogus"][..], &["review", "--unit"][..]] { // (f)
        let r = go(t.path(), &|_| None, true, &mut "".as_bytes(), args);
        assert_eq!((r.code, r.err.as_str()), (2, USAGE), "{args:?}");
    }
}
#[cfg(unix)]
#[test] fn a_write_failure_exits_1_and_keeps_earlier_outcomes() { // D§14 "the write of an outcome fails"
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 { eprintln!("skipped: root ignores directory modes"); return; }
    let t = tempfile::tempdir().unwrap();
    put_rows(t.path(), &[1, 5]);
    let five = file(t.path(), 5);
    assert_eq!(review(t.path(), "r\n\nq\n").code, 0);
    let d = dir(t.path());
    fs::set_permissions(&d, fs::Permissions::from_mode(0o500)).unwrap();
    let r = review(t.path(), "o\nx\n");
    fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(r.code, 1);
    assert!(headers(&r.out)[0].contains(&id(5)), "{}", r.out);
    assert!(r.err.starts_with("zygo decision: cannot write ") && r.err.contains("docs/decisions"), "{}", r.err);
    assert!(r.out.ends_with(&ending("ratified 0, promoted 0, overturned 0, skipped 0", &[])), "{}", r.out);
    assert_eq!(record(t.path(), 1).status.name(), "ratified");
    assert_eq!(file(t.path(), 5), five);
}

#[test] fn a_skipped_dependent_in_a_cycle_comes_back_after_an_overturn() { // D§11.2 step 5
    let t = tempfile::tempdir().unwrap();
    cycle(t.path());
    let r = review(t.path(), "s\no\nwrong\nq\n");
    assert_eq!(r.code, 0, "{}", r.err);
    let hs: Vec<String> = headers(&r.out).iter().map(|h| h.split(' ').nth(1).unwrap().to_string()).collect();
    assert_eq!(hs, ids(&[1, 2, 1]), "{}", r.out);
    let last = &r.out[r.out.rfind(&format!("] {}", id(1))).unwrap()..];
    assert!(last.contains(&format!("depends on overturned {}\n", id(2))), "{last}");
    assert!(r.out.ends_with(&ending("ratified 0, promoted 0, overturned 1, skipped 1", &[2])), "{}", r.out);
    assert_eq!(record(t.path(), 1).status.name(), "open");
}
#[test] fn a_text_over_its_record_limit_asks_again() { // D§11.3, record.rs review_note <= 2000, promotion <= 500
    let t = tempfile::tempdir().unwrap();
    put_rows(t.path(), &[1, 5]);
    let r = review(t.path(), &format!("p\nrule\n{}\nAdd it\no\n{}\nwrong\n", "x".repeat(501), "y".repeat(2001)));
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out.matches("too long: at most 500 bytes\n").count(), 1, "{}", r.out);
    assert_eq!(r.out.matches("too long: at most 2000 bytes\n").count(), 1, "{}", r.out);
    assert_eq!((r.out.matches("promotion text: ").count(), r.out.matches("reason: ").count()), (2, 2));
    let r1 = record(t.path(), 1);
    assert_eq!((r1.promoted_to.as_deref(), r1.promotion.as_deref()), (Some("rule"), Some("Add it")));
    let r5 = record(t.path(), 5);
    assert_eq!((r5.status.name(), r5.review_note.as_deref()), ("overturned", Some("wrong")));
    assert!(r.out.ends_with(&ending("ratified 0, promoted 1, overturned 1, skipped 0", &[1, 5])), "{}", r.out);
    // The ratify note has the same limit; 2000 bytes is accepted.
    let t = tempfile::tempdir().unwrap();
    put_rows(t.path(), &[1]);
    let r = review(t.path(), &format!("r\n{}\n{}\n", "z".repeat(2001), "z".repeat(2000)));
    assert_eq!((r.code, r.out.matches("too long: at most 2000 bytes\n").count()), (0, 1), "{}", r.err);
    assert_eq!(record(t.path(), 1).review_note.map(|n| n.len()), Some(2000));
}
#[test] fn a_bad_promote_target_asks_again() { // D§11.3
    let t = tempfile::tempdir().unwrap();
    put_rows(t.path(), &[1]);
    let r = review(t.path(), "p\nbogus\nrule\nAdd it\n");
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out.matches("promote to adr, spec or rule: ").count(), 2);
    assert_eq!(record(t.path(), 1).promoted_to.as_deref(), Some("rule"));
}
#[test] fn eof_at_a_sub_prompt_ends_the_review_and_keeps_earlier_outcomes() { // D§11.2 "a crash or q loses nothing"
    for tail in ["r\n", "p\n", "p\nrule\n", "o\n"] {
        let t = tempfile::tempdir().unwrap();
        put_rows(t.path(), &[1, 5]);
        let five = file(t.path(), 5);
        let r = review(t.path(), &format!("r\n\n{tail}"));
        assert_eq!((r.code, r.err.as_str()), (0, ""), "{tail:?}");
        assert!(r.out.ends_with(&ending("ratified 1, promoted 0, overturned 0, skipped 0", &[1])), "{tail:?}: {}", r.out);
        assert_eq!(record(t.path(), 1).status.name(), "ratified", "{tail:?}");
        assert_eq!(file(t.path(), 5), five, "{tail:?}");
    }
}
/// Stdin with one answer per read that makes `dir` read-only before it gives answer `at` (0-based).
#[cfg(unix)]
struct LockAt { answers: VecDeque<String>, at: usize, dir: PathBuf }
#[cfg(unix)]
impl Read for LockAt {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::os::unix::fs::PermissionsExt;
        if self.at == 0 {
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o500))?;
        }
        self.at = self.at.wrapping_sub(1);
        let Some(a) = self.answers.pop_front() else { return Ok(0) };
        buf[..a.len()].copy_from_slice(a.as_bytes());
        Ok(a.len())
    }
}
#[cfg(unix)]
#[test] fn a_write_failure_after_an_outcome_lists_that_outcome_and_exits_1() { // D§14 "the write of an outcome fails"
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 { eprintln!("skipped: root ignores directory modes"); return; }
    let t = tempfile::tempdir().unwrap();
    put_rows(t.path(), &[1, 5]);
    let five = file(t.path(), 5);
    let d = dir(t.path());
    let answers = ["r\n", "\n", "o\n", "x\n"].iter().map(|a| a.to_string()).collect();
    let mut stdin = LockAt { answers, at: 3, dir: d.clone() };
    let r = go(t.path(), &|_| None, true, &mut BufReader::new(&mut stdin), &["review"]);
    fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(r.code, 1, "{}", r.err);
    assert!(r.err.starts_with("zygo decision: cannot write ") && r.err.contains("docs/decisions"), "{}", r.err);
    assert!(r.out.ends_with(&ending("ratified 1, promoted 0, overturned 0, skipped 0", &[1])), "{}", r.out);
    assert_eq!(record(t.path(), 1).status.name(), "ratified");
    assert_eq!(file(t.path(), 5), five);
}

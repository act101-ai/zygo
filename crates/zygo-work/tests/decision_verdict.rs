//! DR-12: the keep check (D§9.4) over the node range, on a fixture repository with a linked node checkout. No test
//! waits on wall-clock time: real-git tests run with every deadline an hour away, and no developer git config is read.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zygo_work::decisions::record::Class;
use zygo_work::gate::check::{check_keep, format_problems, names_commit, Disposition, KeepRequest, Proof};
use zygo_work::gate::git::{BatchIo, Blob, GitFail, GitRunner, LogCommit, ProcessGit, ProcessRunner, VerdictGit};
use zygo_work::repo::{RunError, Whole};

// ---- copied from tests/decision_git.rs (private to that test crate) ----

const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

/// `git -C <dir> <args>` with no global or system config, a fixed identity and `LC_ALL=C`.
fn git_in(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(["-c", "commit.gpgsign=false"]).args(args);
    c.env("GIT_CONFIG_GLOBAL", NULL).env("GIT_CONFIG_NOSYSTEM", "1").env("LC_ALL", "C");
    c.env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t");
    c
}

/// A setup git call that must succeed.
fn git(dir: &Path, args: &[&str]) {
    let out = git_in(dir, args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// The trimmed stdout of a setup git call that must succeed.
fn out(dir: &Path, args: &[&str]) -> String {
    let o = git_in(dir, args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// The real runner with its clock one hour ahead, so every deadline is an hour away and machine load cannot fail a test.
struct Far(ProcessRunner);

impl GitRunner for Far {
    fn run(&mut self, cmd: Command, deadline: Instant) -> Result<Whole, RunError> { self.0.run(cmd, deadline) }
    fn batch(&mut self, cmd: Command, deadline: Instant) -> Result<Box<dyn BatchIo>, RunError> { self.0.batch(cmd, deadline) }
    fn now(&self) -> Instant { Instant::now() + Duration::from_secs(3600) }
}

/// [`Far`] with `HOME`, `XDG_CONFIG_HOME` (and `USERPROFILE`) set to an empty temp dir on each command, so no
/// developer `~/.gitconfig` reaches a real-git test.
struct Iso {
    far: Far,
    home: TempDir,
}

impl Iso {
    fn isolate(&self, mut cmd: Command) -> Command {
        cmd.env("HOME", self.home.path()).env("XDG_CONFIG_HOME", self.home.path());
        if cfg!(windows) {
            cmd.env("USERPROFILE", self.home.path());
        }
        cmd
    }
}

impl GitRunner for Iso {
    fn run(&mut self, cmd: Command, deadline: Instant) -> Result<Whole, RunError> {
        let cmd = self.isolate(cmd);
        self.far.run(cmd, deadline)
    }
    fn batch(&mut self, cmd: Command, deadline: Instant) -> Result<Box<dyn BatchIo>, RunError> {
        let cmd = self.isolate(cmd);
        self.far.batch(cmd, deadline)
    }
    fn now(&self) -> Instant { self.far.now() }
}

fn far_git(root: &Path) -> ProcessGit {
    ProcessGit::with_runner(root, Box::new(Iso { far: Far(ProcessRunner), home: TempDir::new().unwrap() }))
}

// ---- the keep fixture ----

const UNIT: &str = "phase3:DR-12";
const C40: &str = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556";
const BODY: &str = "\n## Finding\n\nF.\n\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n";
const AGENT: &str = "\n\nZygo-Session: s";

/// The record id `DDR-0…0<c>`; `c` is never I, L, O or U.
fn id(c: char) -> String { format!("DDR-{}{c}", "0".repeat(25)) }

fn path(c: char) -> String { format!("docs/decisions/{}.md", id(c)) }

/// Record `id(c)`: an open `accepted-limit`, class `none`, title `T`, unit UNIT, round 3, role `code`, model
/// `unknown`, commit C40, date 2026-11-24, reverse_cost `low`, `depends_on = []`, body BODY. Each `(key, value)` of
/// `set` replaces or adds a key; values are TOML strings, except `round` and `depends_on`, written as given.
fn record(c: char, set: &[(&str, &str)]) -> String {
    let q = |v: &str| format!("\"{v}\"");
    let mut keys: Vec<(String, String)> = [
        ("id", q(&id(c))),
        ("kind", q("accepted-limit")),
        ("status", q("open")),
        ("class", q("none")),
        ("title", q("T")),
        ("unit", q(UNIT)),
        ("round", "3".to_string()),
        ("role", q("code")),
        ("model", q("unknown")),
        ("commit", q(C40)),
        ("date", q("2026-11-24")),
        ("reverse_cost", q("low")),
        ("depends_on", "[]".to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    for (k, v) in set {
        let v = if matches!(*k, "round" | "depends_on") { v.to_string() } else { q(v) };
        match keys.iter_mut().find(|(key, _)| key == k) {
            Some(slot) => slot.1 = v,
            None => keys.push((k.to_string(), v)),
        }
    }
    let front: String = keys.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
    format!("+++\n{front}+++\n{BODY}")
}

struct Fx {
    _t: TempDir,
    w: PathBuf,
    n: PathBuf,
    root: String,
    base: String,
}

/// `w` on `main`: `root` (file `a`), then `base` (adds `docs/decisions/README.md` and `path('P')`). The linked
/// checkout `n` on `node` at `base`. `w` stays at `base`.
fn fixture() -> Fx {
    let t = TempDir::new().unwrap();
    git(t.path(), &["init", "-q", "-b", "main", "w"]);
    let w = t.path().join("w");
    fs::write(w.join("a"), "a\n").unwrap();
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "-q", "-m", "root"]);
    let root = out(&w, &["rev-parse", "HEAD"]);
    fs::create_dir_all(w.join("docs/decisions")).unwrap();
    fs::write(w.join("docs/decisions/README.md"), "readme\n").unwrap();
    fs::write(w.join(path('P')), record('P', &[])).unwrap();
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "-q", "-m", "base"]);
    let base = out(&w, &["rev-parse", "HEAD"]);
    git(&w, &["worktree", "add", "-q", "../n", "-b", "node"]);
    let n = t.path().join("n");
    Fx { _t: t, w, n, root, base }
}

/// In `n`: writes `Some(text)` or removes (`None`) each path, `git add -A`, commits with `msg`; returns the full id.
fn commit(n: &Path, msg: &str, files: &[(&str, Option<&str>)]) -> String {
    for (p, text) in files {
        let f = n.join(p);
        match text {
            Some(text) => {
                fs::create_dir_all(f.parent().unwrap()).unwrap();
                fs::write(&f, text).unwrap();
            }
            None => fs::remove_file(&f).unwrap(),
        }
    }
    git(n, &["add", "-A"]);
    git(n, &["commit", "-q", "-m", msg]);
    out(n, &["rev-parse", "HEAD"])
}

/// An agent commit in `fx.n` that writes `path(c)` = `record(c, set)`; returns the full id.
fn add(fx: &Fx, c: char, set: &[(&str, &str)]) -> String {
    commit(&fx.n, &format!("add{AGENT}"), &[(&path(c), Some(&record(c, set)))])
}

/// The usual `h`: an agent commit of `src.txt`.
fn code(fx: &Fx) -> String { commit(&fx.n, &format!("code{AGENT}"), &[("src.txt", Some("1\n"))]) }

fn req(round: u32, head: Option<&str>, base: Option<&str>, ds: Option<Vec<Disposition>>) -> KeepRequest {
    KeepRequest {
        unit: UNIT.to_string(),
        round,
        head: head.map(str::to_string),
        base: base.map(str::to_string),
        dispositions: ds,
    }
}

/// `check_keep` with `far_git(&fx.w)`: the main checkout reads the node's commits (PD1).
fn keep(fx: &Fx, round: u32, head: Option<&str>, base: Option<&str>, ds: Option<Vec<Disposition>>) -> Vec<String> {
    check_keep(&req(round, head, base, ds), &mut far_git(&fx.w))
}

fn fixed(l: &str, c: &str) -> Disposition {
    Disposition { finding: l.to_string(), class: Class::None, proof: Proof::Fixed { commit: c.to_string() } }
}

fn rejected(l: &str, class: Class) -> Disposition {
    Disposition { finding: l.to_string(), class, proof: Proof::Rejected { reason: "not a defect".to_string() } }
}

fn recorded(l: &str, class: Class, c: char) -> Disposition {
    Disposition { finding: l.to_string(), class, proof: Proof::Recorded { record: id(c) } }
}

fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

// ---- cycle 1: format_problems, steps 1 to 4 ----

#[test]
fn no_head() {
    let fx = fixture();
    assert_eq!(keep(&fx, 3, None, None, Some(vec![])), s(&["no head commit"]));
}

#[test]
fn no_dispositions() {
    let fx = fixture();
    let h = code(&fx);
    assert_eq!(keep(&fx, 3, Some(&h), None, None), s(&["no dispositions"]));
}

#[test]
fn duplicate_label() {
    let fx = fixture();
    let h = code(&fx);
    let ds = vec![rejected("R1-1", Class::None), rejected("R1-1", Class::None)];
    assert_eq!(keep(&fx, 1, Some(&h), None, Some(ds)), s(&["finding R1-1 has two dispositions"]));
}

#[test]
fn one_record_for_two_findings() {
    let fx = fixture();
    let h = add(&fx, 'R', &[]);
    let ds = vec![recorded("R3-1", Class::None, 'R'), recorded("R3-2", Class::None, 'R')];
    assert_eq!(keep(&fx, 3, Some(&h), None, Some(ds)), vec![format!("{} serves two findings", id('R'))]);
}

#[test]
fn acceptance_8_head_already_in_head_skips_steps_5_to_9() {
    let fx = fixture();
    let h = code(&fx);
    git(&fx.w, &["merge", "-q", "--ff-only", "node"]);
    // R1-1 is in the node range and would pass step 5; R1-2 is outside it and would fail step 5 if it ran.
    let ds = vec![fixed("R1-1", &h), fixed("R1-2", &fx.root)];
    let p = keep(&fx, 1, Some(&h), None, Some(ds));
    assert_eq!(p, s(&["head is already in HEAD: call before the merge, or give base"]));
    assert!(!p.iter().any(|x| x.contains("not in the node range")));
}

#[test]
fn format_problems_12_gives_10_then_and_2_more() {
    let p: Vec<String> = (0..12).map(|i| format!("p{i}")).collect();
    assert_eq!(format_problems(&p), "p0; p1; p2; p3; p4; p5; p6; p7; p8; p9; and 2 more");
    assert_eq!(format_problems(&[]), "");
}

#[test]
fn format_problems_stays_within_4_kib() {
    let p = vec!["x".repeat(1000); 12];
    assert_eq!(format_problems(&p), format!("{}; and 8 more", vec!["x".repeat(1000); 4].join("; ")));
    // 6,000 bytes of a 3-byte char: byte 4,096 is inside a char, so the cut steps back to 4,095.
    let one = format_problems(&["€".repeat(2000)]);
    assert_eq!(one, "€".repeat(1365));
    assert_eq!(one.len(), 4095);
}

// ---- cycle 2: steps 5 to 7 ----

#[test]
fn fixed_commit_that_is_an_ancestor_of_base() {
    let fx = fixture();
    let h = code(&fx);
    let ds = vec![fixed("R1-1", &fx.root)];
    assert_eq!(
        keep(&fx, 1, Some(&h), Some(&fx.base), Some(ds)),
        vec![format!("R1-1: commit {} is not in the node range", fx.root)]
    );
}

#[test]
fn recorded_in_round_2() {
    let fx = fixture();
    let h = add(&fx, 'R', &[]);
    assert_eq!(
        keep(&fx, 2, Some(&h), None, Some(vec![recorded("R2-1", Class::None, 'R')])),
        s(&["R2-1: recorded with class none needs round 3 or later"])
    );
}

#[test]
fn recorded_record_absent_at_head() {
    let fx = fixture();
    let h = code(&fx);
    assert_eq!(
        keep(&fx, 3, Some(&h), None, Some(vec![recorded("R3-1", Class::None, 'Z')])),
        vec![format!("R3-1: {} is not at head", id('Z'))]
    );
}

#[test]
fn recorded_record_present_at_base() {
    let fx = fixture();
    let h = code(&fx);
    assert_eq!(
        keep(&fx, 3, Some(&h), None, Some(vec![recorded("R3-1", Class::None, 'P')])),
        vec![format!("R3-1: {} was not added in the node range", id('P'))]
    );
}

#[test]
fn recorded_record_of_another_unit() {
    let fx = fixture();
    let h = add(&fx, 'R', &[("unit", "phase3:DR-99")]);
    assert_eq!(
        keep(&fx, 3, Some(&h), None, Some(vec![recorded("R3-1", Class::None, 'R')])),
        vec![format!("R3-1: {} has unit phase3:DR-99, not phase3:DR-12", id('R'))]
    );
}

#[test]
fn recorded_record_of_another_class() {
    let fx = fixture();
    let h = add(&fx, 'R', &[("kind", "decision")]);
    assert_eq!(
        keep(&fx, 3, Some(&h), None, Some(vec![recorded("R3-1", Class::Security, 'R')])),
        vec![format!("R3-1: {} has class none, not security", id('R'))]
    );
}

#[test]
fn rejected_entry_of_class_security() {
    let fx = fixture();
    let h = code(&fx);
    assert_eq!(
        keep(&fx, 1, Some(&h), None, Some(vec![rejected("R1-1", Class::Security)])),
        s(&["R1-1: class security needs a record for the operator"])
    );
}

#[test]
fn recorded_open_record_of_class_security() {
    let fx = fixture();
    let h = add(&fx, 'S', &[("kind", "decision"), ("class", "security")]);
    assert_eq!(
        keep(&fx, 3, Some(&h), None, Some(vec![recorded("R3-1", Class::Security, 'S')])),
        vec![format!("R3-1: class security stops for the operator: ratify or overturn {}", id('S'))]
    );
}

#[test]
fn stop_class_recorded_in_round_1_stops_for_the_operator_not_the_round_limit() {
    let fx = fixture();
    let h = add(&fx, 'S', &[("kind", "decision"), ("class", "security")]);
    let p = keep(&fx, 1, Some(&h), None, Some(vec![recorded("R1-1", Class::Security, 'S')]));
    assert_eq!(p, vec![format!("R1-1: class security stops for the operator: ratify or overturn {}", id('S'))]);
    assert!(!p.iter().any(|x| x.contains("round 3")));
}

// ---- cycle 3: steps 8 and 9 ----

/// The record `P` (present at `base`) set ratified by the operator keys.
fn ratified_p() -> String { record('P', &[("status", "ratified"), ("reviewed", "2026-11-25"), ("review_note", "")]) }

#[test]
fn new_open_contract_record_no_finding_names() {
    let fx = fixture();
    let h = add(&fx, 'N', &[("kind", "decision"), ("class", "contract")]);
    assert_eq!(
        keep(&fx, 1, Some(&h), None, Some(vec![])),
        vec![format!("{0}: class contract stops for the operator: ratify or overturn {0}", id('N'))]
    );
}

#[test]
fn agent_commit_sets_ratified() {
    let fx = fixture();
    let h = commit(&fx.n, &format!("ratify{AGENT}"), &[(&path('P'), Some(&ratified_p()))]);
    assert_eq!(keep(&fx, 1, Some(&h), None, Some(vec![])), vec![format!("{}: agent writes status ratified", id('P'))]);
}

#[test]
fn agent_commit_changes_class() {
    let fx = fixture();
    let h = commit(&fx.n, &format!("class{AGENT}"), &[(&path('P'), Some(&record('P', &[("class", "security")])))]);
    assert_eq!(keep(&fx, 1, Some(&h), None, Some(vec![])), vec![format!("{}: frozen key class changed", id('P'))]);
}

#[test]
fn agent_commit_sets_fixed_with_fixed_by_outside_the_range() {
    let fx = fixture();
    let p = record('P', &[("status", "fixed"), ("fixed_by", &fx.base)]);
    let h = commit(&fx.n, &format!("fix{AGENT}"), &[(&path('P'), Some(&p))]);
    assert_eq!(
        keep(&fx, 1, Some(&h), None, Some(vec![])),
        vec![format!("{}: fixed_by {} is not in the node range", id('P'), fx.base)]
    );
}

#[test]
fn agent_commit_deletes_a_record() {
    let fx = fixture();
    let h = commit(&fx.n, &format!("rm{AGENT}"), &[(&path('P'), None)]);
    assert_eq!(keep(&fx, 1, Some(&h), None, Some(vec![])), vec![format!("{}: record deleted", id('P'))]);
}

#[test]
fn zygo_session_line_in_a_middle_paragraph_is_an_agent_commit() {
    let fx = fixture();
    let h = commit(&fx.n, "ratify\n\nZygo-Session: s\n\nmore text", &[(&path('P'), Some(&ratified_p()))]);
    assert_eq!(keep(&fx, 1, Some(&h), None, Some(vec![])), vec![format!("{}: agent writes status ratified", id('P'))]);
}

#[test]
fn operator_ratify_or_overturn_without_zygo_session_passes_step_9() {
    let fx = fixture();
    let stop: [(&str, &str); 2] = [("kind", "decision"), ("class", "security")];
    add(&fx, 'S', &stop);
    let ratified = record('S', &[stop[0], stop[1], ("status", "ratified"), ("reviewed", "2026-11-25"), ("review_note", "")]);
    let h1 = commit(&fx.n, "ratify\n\nper the Zygo-Session: rule, no trailer", &[(&path('S'), Some(&ratified))]);
    let ds = || Some(vec![recorded("R1-1", Class::Security, 'S')]);
    assert_eq!(keep(&fx, 1, Some(&h1), None, ds()), Vec::<String>::new());
    let over = record('S', &[stop[0], stop[1], ("status", "overturned"), ("reviewed", "2026-11-25"), ("review_note", "No.")]);
    let h2 = commit(&fx.n, "overturn", &[(&path('S'), Some(&over))]);
    assert_eq!(keep(&fx, 1, Some(&h2), None, ds()), vec![format!("R1-1: {} overturned by the operator", id('S'))]);
}

/// [`ProcessGit`] whose `log` step times out.
struct FailLog(ProcessGit);

impl VerdictGit for FailLog {
    fn commit(&mut self, rev: &str) -> Result<Option<String>, GitFail> { self.0.commit(rev) }
    fn record_blob(&mut self, rev: &str, name: &str) -> Result<Blob, GitFail> { self.0.record_blob(rev, name) }
    fn merge_base(&mut self, a: &str, b: &str) -> Result<Option<String>, GitFail> { self.0.merge_base(a, b) }
    fn rev_list(&mut self, base: &str, head: &str, max: usize) -> Result<Option<Vec<String>>, GitFail> {
        self.0.rev_list(base, head, max)
    }
    fn log(&mut self, _base: &str, _head: &str) -> Result<Vec<LogCommit>, GitFail> {
        Err(GitFail::Failed { step: "git log", why: "timed out".into() })
    }
}

#[test]
fn log_failure_gives_cannot_check_git_log() {
    let fx = fixture();
    let h = code(&fx);
    let p = check_keep(&req(1, Some(&h), None, Some(vec![])), &mut FailLog(far_git(&fx.w)));
    assert_eq!(p, s(&["cannot check: git log timed out"]));
}

#[test]
fn acceptance_6_passes_with_no_problem() {
    let fx = fixture();
    let c1 = code(&fx);
    let h = add(&fx, 'R', &[]);
    let ds = vec![fixed("R3-1", &c1), rejected("R3-2", Class::None), recorded("R3-3", Class::None, 'R')];
    assert_eq!(keep(&fx, 3, Some(&h), None, Some(ds)), Vec::<String>::new());
    let fx2 = fixture();
    let h = code(&fx2);
    assert_eq!(keep(&fx2, 1, Some(&h), None, Some(vec![])), Vec::<String>::new());
}

// ---- review round 1 ----

fn rejected_no_reason(l: &str, class: Class) -> Disposition {
    Disposition { finding: l.to_string(), class, proof: Proof::Rejected { reason: String::new() } }
}

/// In `fx.w`: branch `side` from `base` with a commit of `side.txt`; `w` goes back to `main` at `base`.
fn side_branch(fx: &Fx) {
    git(&fx.w, &["checkout", "-q", "-b", "side"]);
    fs::write(fx.w.join("side.txt"), "s\n").unwrap();
    git(&fx.w, &["add", "-A"]);
    git(&fx.w, &["commit", "-q", "-m", "side"]);
    git(&fx.w, &["checkout", "-q", "main"]);
}

/// In `fx.n`: merges `side` without committing, writes each file, commits the merge with `msg`; returns the full id.
fn evil_merge(fx: &Fx, msg: &str, files: &[(&str, &str)]) -> String {
    side_branch(fx);
    git(&fx.n, &["merge", "-q", "--no-ff", "--no-commit", "side"]);
    commit(&fx.n, msg, &files.iter().map(|(p, t)| (*p, Some(*t))).collect::<Vec<_>>())
}

#[test]
fn record_added_in_a_merge_commit_stops_at_step_8() {
    let fx = fixture();
    code(&fx);
    let n = record('N', &[("kind", "decision"), ("class", "contract")]);
    let h = evil_merge(&fx, "merge side", &[(&path('N'), &n)]);
    assert_eq!(
        keep(&fx, 1, Some(&h), None, Some(vec![])),
        vec![format!("{0}: class contract stops for the operator: ratify or overturn {0}", id('N'))]
    );
}

#[test]
fn merge_commits_and_commits_without_zygo_session_are_not_read_by_step_9() {
    let fx = fixture();
    code(&fx);
    let h = evil_merge(&fx, &format!("merge side{AGENT}"), &[(&path('P'), &ratified_p())]);
    assert_eq!(keep(&fx, 1, Some(&h), None, Some(vec![])), Vec::<String>::new());
    let fx = fixture();
    let h = commit(&fx.n, "ratify", &[(&path('P'), Some(&ratified_p()))]);
    assert_eq!(keep(&fx, 1, Some(&h), None, Some(vec![])), Vec::<String>::new());
}

#[test]
fn indented_or_lower_case_zygo_session_line_is_an_agent_commit() {
    for msg in ["ratify\n\n    Zygo-Session: s", "ratify\n\nzygo-session: s"] {
        let fx = fixture();
        let h = commit(&fx.n, msg, &[(&path('P'), Some(&ratified_p()))]);
        assert_eq!(
            keep(&fx, 1, Some(&h), None, Some(vec![])),
            vec![format!("{}: agent writes status ratified", id('P'))],
            "{msg:?}"
        );
    }
}

#[test]
fn step_7_early_stop_keeps_the_step_8_stop_problem() {
    let fx = fixture();
    let h = add(&fx, 'N', &[("kind", "decision"), ("class", "contract")]);
    assert_eq!(
        keep(&fx, 2, Some(&h), None, Some(vec![recorded("R2-1", Class::None, 'N')])),
        vec![
            "R2-1: recorded with class none needs round 3 or later".to_string(),
            format!("{0}: class contract stops for the operator: ratify or overturn {0}", id('N')),
        ]
    );
}

#[test]
fn rejected_stop_class_without_reason_gives_both_problems() {
    let fx = fixture();
    let h = code(&fx);
    assert_eq!(
        keep(&fx, 1, Some(&h), None, Some(vec![rejected_no_reason("R1-1", Class::Security)])),
        s(&["R1-1: no reason", "R1-1: class security needs a record for the operator"])
    );
}

#[test]
fn label_with_a_control_character_is_escaped() {
    let fx = fixture();
    let h = code(&fx);
    assert_eq!(
        keep(&fx, 1, Some(&h), None, Some(vec![rejected("R1\u{7}1", Class::Security)])),
        s(&["R1\\u{07}1: class security needs a record for the operator"])
    );
}

#[test]
fn rename_of_a_record_is_a_deleted_record() {
    let fx = fixture();
    let h = commit(&fx.n, &format!("mv{AGENT}"), &[(&path('P'), None), (&path('Q'), Some(&record('Q', &[])))]);
    assert_eq!(keep(&fx, 1, Some(&h), None, Some(vec![])), vec![format!("{}: record deleted", id('P'))]);
}

#[test]
fn base_that_is_no_commit() {
    let fx = fixture();
    let h = code(&fx);
    assert_eq!(keep(&fx, 1, Some(&h), Some("nope"), Some(vec![])), s(&["no base commit"]));
}

#[test]
fn base_that_is_not_an_ancestor_of_head() {
    let fx = fixture();
    let h = code(&fx);
    fs::write(fx.w.join("b"), "b\n").unwrap();
    git(&fx.w, &["add", "-A"]);
    git(&fx.w, &["commit", "-q", "-m", "main"]);
    let b = out(&fx.w, &["rev-parse", "HEAD"]);
    assert_eq!(keep(&fx, 1, Some(&h), Some(&b), Some(vec![])), s(&["base is not an ancestor of head"]));
}

#[test]
fn head_with_no_common_ancestor_with_head_ref() {
    let fx = fixture();
    let o = out(&fx.w, &["commit-tree", &format!("{}^{{tree}}", fx.base), "-m", "o"]);
    assert_eq!(keep(&fx, 1, Some(&o), None, Some(vec![])), s(&["head has no common ancestor with HEAD: give base"]));
}

#[test]
fn one_checkout_with_base_runs_steps_7_to_9() {
    let fx = fixture();
    add(&fx, 'R', &[]);
    add(&fx, 'N', &[("kind", "decision"), ("class", "contract")]);
    let h = commit(&fx.n, &format!("ratify{AGENT}"), &[(&path('P'), Some(&ratified_p()))]);
    let ds = vec![recorded("R3-1", Class::None, 'R'), recorded("R3-2", Class::None, 'Z')];
    let p = check_keep(&req(3, Some(&h), Some(&fx.base), Some(ds)), &mut far_git(&fx.n));
    assert_eq!(p, vec![
        format!("R3-2: {} is not at head", id('Z')),
        format!("{0}: class contract stops for the operator: ratify or overturn {0}", id('N')),
        format!("{}: agent writes status ratified", id('P')),
    ]);
    // Without base, the node checkout's HEAD is head.
    let p = check_keep(&req(3, Some(&h), None, Some(vec![])), &mut far_git(&fx.n));
    assert_eq!(p, s(&["head is already in HEAD: call before the merge, or give base"]));
}

#[test]
fn acceptance_7_fixed_after_overturn_passes() {
    let fx = fixture();
    let stop: [(&str, &str); 2] = [("kind", "decision"), ("class", "security")];
    add(&fx, 'S', &stop);
    let review = [("reviewed", "2026-11-25"), ("review_note", "No.")];
    let over = record('S', &[stop[0], stop[1], ("status", "overturned"), review[0], review[1]]);
    commit(&fx.n, "overturn", &[(&path('S'), Some(&over))]);
    let f = commit(&fx.n, &format!("revert the subject{AGENT}"), &[("src.txt", Some("0\n"))]);
    let done = record('S', &[stop[0], stop[1], ("status", "fixed"), review[0], review[1], ("fixed_by", &f)]);
    let h = commit(&fx.n, &format!("fixed{AGENT}"), &[(&path('S'), Some(&done))]);
    let ds = vec![Disposition { finding: "R2-1".into(), class: Class::Security, proof: Proof::Fixed { commit: f.clone() } }];
    assert_eq!(keep(&fx, 2, Some(&h), None, Some(ds.clone())), Vec::<String>::new());
    let p = check_keep(&req(2, Some(&h), Some(&fx.base), Some(ds)), &mut far_git(&fx.n));
    assert_eq!(p, Vec::<String>::new());
}

// ---- a stub VerdictGit: the limits and git failures without real git ----

const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const H: &str = "cccccccccccccccccccccccccccccccccccccccc";
const P1: &str = "dddddddddddddddddddddddddddddddddddddddd";

/// Every rev is its own full id, every record is missing, the merge base is `B`, and `fail` names the step that fails.
#[derive(Default)]
struct Stub {
    fail: Option<&'static str>,
    too_large: bool,
    log: Vec<LogCommit>,
}

impl Stub {
    fn fails(&self, step: &'static str) -> Result<(), GitFail> {
        match self.fail {
            Some(f) if f == step => Err(GitFail::Failed { step, why: "timed out".into() }),
            _ => Ok(()),
        }
    }
}

impl VerdictGit for Stub {
    fn commit(&mut self, rev: &str) -> Result<Option<String>, GitFail> {
        self.fails("git cat-file")?;
        Ok(Some(rev.to_string()))
    }
    fn record_blob(&mut self, _rev: &str, _name: &str) -> Result<Blob, GitFail> {
        self.fails("git cat-file")?;
        Ok(Blob::Missing)
    }
    fn merge_base(&mut self, _a: &str, _b: &str) -> Result<Option<String>, GitFail> {
        self.fails("git merge-base")?;
        Ok(Some(B.to_string()))
    }
    fn rev_list(&mut self, _base: &str, head: &str, _max: usize) -> Result<Option<Vec<String>>, GitFail> {
        self.fails("git rev-list")?;
        Ok((!self.too_large).then(|| vec![head.to_string()]))
    }
    fn log(&mut self, _base: &str, _head: &str) -> Result<Vec<LogCommit>, GitFail> {
        Ok(self.log.clone())
    }
}

/// The record id number `i` in Crockford base 32.
fn nth_id(mut i: usize) -> String {
    const D: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut u = vec![b'0'; 26];
    for slot in u.iter_mut().rev() {
        *slot = D[i % 32];
        i /= 32;
    }
    format!("DDR-{}", String::from_utf8(u).unwrap())
}

/// One agent commit `H` (parent `P1`) that adds `records` record files and `others` other `.md` files.
fn stub_log(records: usize, others: usize) -> Vec<LogCommit> {
    let mut paths: Vec<(char, String)> = (0..records).map(|i| ('A', format!("docs/decisions/{}.md", nth_id(i)))).collect();
    paths.extend((0..others).map(|i| ('A', format!("docs/decisions/notes-{i}.md"))));
    vec![LogCommit { id: H.into(), parents: vec![P1.into()], message: "x\n\nZygo-Session: s\n".into(), paths }]
}

fn stub_keep(stub: &mut Stub) -> Vec<String> { check_keep(&req(1, Some(H), None, Some(vec![])), stub) }

#[test]
fn node_range_too_large() {
    assert_eq!(stub_keep(&mut Stub { too_large: true, ..Stub::default() }), s(&["node range too large"]));
}

#[test]
fn record_changes_over_1000_are_too_many() {
    assert_eq!(stub_keep(&mut Stub { log: stub_log(1000, 0), ..Stub::default() }), Vec::<String>::new());
    assert_eq!(stub_keep(&mut Stub { log: stub_log(1001, 0), ..Stub::default() }), s(&["too many record changes"]));
}

#[test]
fn other_names_do_not_count_toward_the_record_change_limit() {
    assert_eq!(stub_keep(&mut Stub { log: stub_log(1000, 5), ..Stub::default() }), Vec::<String>::new());
}

#[test]
fn git_failures_give_cannot_check() {
    for step in ["git cat-file", "git merge-base", "git rev-list"] {
        let p = stub_keep(&mut Stub { fail: Some(step), ..Stub::default() });
        assert_eq!(p, vec![format!("cannot check: {step} timed out")], "{step}");
    }
}

// ---- DR-13 review round 1: a ref cannot stand for a short id; a `fixed` entry without `commit` ----

const H40: &str = "4444444444444444444444444444444444444444";
const B40: &str = "2222222222222222222222222222222222222222";
const C40X: &str = "3333333333333333333333333333333333333333";
const OTHER: &str = "ffffffffffffffffffffffffffffffffffffffff";

/// A [`VerdictGit`] with no process: `commit` looks up `refs` (a short hex that a ref of the same name shadows
/// maps to OTHER); H40, B40, C40X resolve to themselves; merge_base gives B40; the range is [H40, C40X]; no paths.
struct Refs(Vec<(&'static str, &'static str)>);

impl VerdictGit for Refs {
    fn commit(&mut self, rev: &str) -> Result<Option<String>, GitFail> {
        let own = [H40, B40, C40X, OTHER].iter().find(|c| **c == rev).map(|c| c.to_string());
        Ok(self.0.iter().find(|(r, _)| *r == rev).map(|(_, id)| id.to_string()).or(own))
    }
    fn record_blob(&mut self, _rev: &str, _name: &str) -> Result<Blob, GitFail> { Ok(Blob::Missing) }
    fn merge_base(&mut self, _a: &str, _b: &str) -> Result<Option<String>, GitFail> { Ok(Some(B40.to_string())) }
    fn rev_list(&mut self, _base: &str, _head: &str, _max: usize) -> Result<Option<Vec<String>>, GitFail> {
        Ok(Some(vec![H40.to_string(), C40X.to_string()]))
    }
    fn log(&mut self, _base: &str, _head: &str) -> Result<Vec<LogCommit>, GitFail> { Ok(vec![]) }
}

#[test]
fn names_commit_compares_the_given_hex_as_a_prefix() {
    assert!(names_commit("4444444", H40));
    assert!(names_commit(H40, H40));
    assert!(!names_commit("b32729d", OTHER));
    assert!(!names_commit("44444445", H40));
}

#[test]
fn a_ref_named_like_a_short_head_is_refused() {
    let p = check_keep(&req(1, Some("b32729d"), None, Some(vec![])), &mut Refs(vec![("b32729d", OTHER)]));
    assert_eq!(p, s(&["cannot check: head b32729d is a ref name, not a commit id"]));
}

#[test]
fn a_ref_named_like_a_short_base_is_refused() {
    let p = check_keep(&req(1, Some(H40), Some("2222222"), Some(vec![])), &mut Refs(vec![("2222222", OTHER)]));
    assert_eq!(p, s(&["cannot check: base 2222222 is a ref name, not a commit id"]));
}

#[test]
fn a_ref_named_like_a_short_fixed_commit_is_refused() {
    let ds = vec![fixed("R1-1", "abc1234")];
    let p = check_keep(&req(1, Some(H40), None, Some(ds)), &mut Refs(vec![("abc1234", C40X)]));
    assert_eq!(p, s(&["R1-1: cannot check: commit abc1234 is a ref name, not a commit id"]));
}

#[test]
fn short_ids_that_resolve_to_themselves_pass() {
    let ds = vec![fixed("R1-1", "3333333")];
    let p = check_keep(&req(1, Some("4444444"), Some("2222222"), Some(ds)),
        &mut Refs(vec![("4444444", H40), ("2222222", B40), ("3333333", C40X)]));
    assert_eq!(p, Vec::<String>::new());
}

#[test]
fn fixed_without_commit_names_no_empty_value() {
    let p = check_keep(&req(1, Some(H40), None, Some(vec![fixed("R1-1", "")])), &mut Refs(vec![]));
    assert_eq!(p, s(&["R1-1: no commit"]));
}

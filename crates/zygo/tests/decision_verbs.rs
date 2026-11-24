//! DR-5: `zygo decision ratify` and `overturn` (D§11.1, D§11.3, D§10.1, D§6.6, D§6.2, D§14, D§17 #14).
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use zygo::cmd::decision::{run_in, DecisionIo, Repo};
use zygo_work::decisions::edit::{allowed, apply, EditError, Outcome, Promotion};
use zygo_work::decisions::record::parse_record;

const C40: &str = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556";
const ID: &str = "DDR-01JDQ2Y7Z8K4M5N6P7Q8R00001";
const TODAY: &str = "2026-12-01";
const BODY: &str = "\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n\n## Unknown section\n\nKept   \n";
/// D§17 #14: a comment above a key, a comment after a value, an unknown inline key, a closing comment, an unknown table, an unknown section.
fn rich(status: &str) -> String {
    format!("+++\nid = \"{ID}\"\n# kind is frozen\nkind = \"decision\"\nstatus = \"{status}\"  # set by the operator\nclass = \"none\"\ntitle = \"T\"\nunit = \"phase-3:N12\"\nround = 1\nrole = \"code\"\nmodel = \"unknown\"\ncommit = \"{C40}\"\ndate = \"2026-11-24\"\ndepends_on = []\nreverse_cost = \"low\"\nzz_unknown = {{ a = 1 }}   # kept\n# closing comment\n\n[extra]\nnote = \"kept\"\n+++\n{BODY}")
}
const PROMOTED: &str = "reviewed = \"2026-12-01\"\nreview_note = \"\"\npromoted_to = \"rule\"\npromotion = \"Add the rule\"\n";
/// `rich` after `ratify --promote rule --as "Add the rule"` on TODAY.
fn rich_promoted() -> String {
    rich("ratified").replace("zz_unknown = { a = 1 }   # kept\n", &format!("zz_unknown = {{ a = 1 }}   # kept\n{PROMOTED}"))
}
/// A valid record with `status` and `extra` root key lines before the closing `+++`.
fn plain(status: &str, extra: &str) -> String {
    format!("+++\nid = \"{ID}\"\nkind = \"decision\"\nstatus = \"{status}\"\nclass = \"none\"\ntitle = \"T\"\nunit = \"phase-3:N12\"\nround = 1\nrole = \"code\"\nmodel = \"unknown\"\ncommit = \"{C40}\"\ndate = \"2026-11-24\"\ndepends_on = []\nreverse_cost = \"low\"\n{extra}+++\n{BODY}")
}
const RATIFIED: &str = "reviewed = \"2026-11-30\"\nreview_note = \"\"\n";
fn dir(top: &Path) -> PathBuf { let d = top.join("docs/decisions"); fs::create_dir_all(&d).unwrap(); d }
fn put(top: &Path, text: &str) -> PathBuf { let p = dir(top).join(format!("{ID}.md")); fs::write(&p, text).unwrap(); p }
fn promote() -> Outcome { Outcome::Ratify { note: String::new(), promote: Some((Promotion::Rule, "Add the rule".into())) } }
fn overturn(reason: &str) -> Outcome { Outcome::Overturn { reason: reason.into() } }

#[test] fn apply_promote_keeps_unknown_keys_comments_and_body_byte_equal() { // D§17 #14, D§6.2, review focus 39
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &rich("open"));
    let shown = fs::read(&p).unwrap();
    assert_eq!(apply(&p, &shown, &promote(), TODAY), Ok(()));
    assert_eq!(fs::read_to_string(&p).unwrap(), rich_promoted());
    let r = parse_record(ID, &fs::read(&p).unwrap()).unwrap();
    assert_eq!((r.promoted_to.as_deref(), r.promotion.as_deref(), r.reviewed.as_deref()), (Some("rule"), Some("Add the rule"), Some(TODAY)));
}
#[test] fn apply_keeps_a_crlf_record_byte_equal_and_writes_crlf_lines() { // review focus 39
    let t = tempfile::tempdir().unwrap();
    let crlf = |s: String| s.replace('\n', "\r\n");
    let p = put(t.path(), &crlf(rich("open")));
    let shown = fs::read(&p).unwrap();
    assert_eq!(apply(&p, &shown, &promote(), TODAY), Ok(()));
    assert_eq!(fs::read_to_string(&p).unwrap(), crlf(rich_promoted()));
}
#[test] fn apply_returns_changed_and_leaves_the_file_when_it_changed_after_it_was_shown() { // D§11.3, review focus 39
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let shown = fs::read(&p).unwrap();
    let edited = plain("open", "").replace("title = \"T\"", "title = \"T2\"");
    fs::write(&p, &edited).unwrap();
    assert_eq!(apply(&p, &shown, &promote(), TODAY), Err(EditError::Changed));
    assert_eq!(fs::read_to_string(&p).unwrap(), edited);
}
#[test] fn apply_writes_a_new_file_and_renames_it() { // D§11.3 temporary file + rename
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let old = t.path().join("old-link");
    fs::hard_link(&p, &old).unwrap();
    assert_eq!(apply(&p, &fs::read(&p).unwrap(), &overturn("wrong"), TODAY), Ok(()));
    assert_eq!(fs::read_to_string(&old).unwrap(), plain("open", ""));          // the old inode is untouched: a rename, not a write in place
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("overturned", "reviewed = \"2026-12-01\"\nreview_note = \"wrong\"\n"));
    let mut names: Vec<String> = fs::read_dir(dir(t.path())).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    assert_eq!(names, vec![".lock".to_string(), format!("{ID}.md")]);         // no .tmp left; the lock file stays
    let r = run(t.path(), &["check"]);                                         // check and the listing ignore .lock
    assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, "", ""));
    let r = run(t.path(), &["ls"]);
    assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, format!("{ID} decision overturned none 0 T\n").as_str(), ""));
}
#[test] fn apply_overturn_of_a_ratified_record_replaces_the_operator_keys_in_place() { // D§6.6 row 3
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("ratified", RATIFIED));
    assert_eq!(apply(&p, &fs::read(&p).unwrap(), &overturn("wrong"), TODAY), Ok(()));
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("overturned", "reviewed = \"2026-12-01\"\nreview_note = \"wrong\"\n"));
}
#[test] fn apply_refuses_forbidden_transitions_and_changes_nothing() { // D§6.6, D§10.1
    let promoted = format!("{RATIFIED}promoted_to = \"rule\"\npromotion = \"P\"\n");
    let cases: Vec<(String, Outcome, String)> = vec![
        (plain("ratified", RATIFIED), promote(), format!("zygo decision: {ID} is ratified")),
        (plain("overturned", "reviewed = \"2026-11-30\"\nreview_note = \"no\"\n"), promote(), format!("zygo decision: {ID} is overturned")),
        (plain("fixed", &format!("fixed_by = \"{C40}\"\n")), overturn("x"), format!("zygo decision: {ID} is fixed")),
        (plain("superseded", ""), overturn("x"), format!("zygo decision: {ID} is superseded")),
        (plain("ratified", &promoted), overturn("x"), format!("zygo decision: {ID} is promoted; remove the promotion by hand first")),
    ];
    for (text, o, want) in cases {
        let t = tempfile::tempdir().unwrap();
        let p = put(t.path(), &text);
        assert_eq!(allowed(&parse_record(ID, text.as_bytes()).unwrap(), &o), Err(want.clone()));
        assert_eq!(apply(&p, text.as_bytes(), &o, TODAY), Err(EditError::Refused(want)));
        assert_eq!(fs::read_to_string(&p).unwrap(), text);
    }
}
#[test] fn apply_refuses_an_outcome_that_makes_the_record_malformed() { // D§6.3 review_note at most 2,000 bytes
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let r = apply(&p, &fs::read(&p).unwrap(), &overturn(&"x".repeat(2001)), TODAY);
    assert!(matches!(&r, Err(EditError::Refused(s)) if s.starts_with(&format!("zygo decision: {ID}: the outcome would make the record malformed: "))), "{r:?}");
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
}
#[cfg(unix)]
#[test] fn apply_write_failure_is_io_naming_the_file_and_changes_nothing() { // D§14 "the write of an outcome fails"
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 { eprintln!("skipped: root ignores directory modes"); return; }
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let d = dir(t.path());
    fs::set_permissions(&d, fs::Permissions::from_mode(0o500)).unwrap();
    let r = apply(&p, &fs::read(&p).unwrap(), &overturn("x"), TODAY);
    fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(&r, Err(EditError::Io(s)) if s.starts_with("zygo decision: cannot write ") && s.contains("docs/decisions")), "{r:?}");
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
}

#[test] fn apply_with_a_stale_shown_after_a_first_apply_returns_changed_and_keeps_the_first_outcome() { // D§11.3 two reviews, review focus 39
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let shown = fs::read(&p).unwrap();                                        // both writers were shown these bytes
    assert_eq!(apply(&p, &shown, &overturn("first"), TODAY), Ok(()));
    let first = fs::read_to_string(&p).unwrap();
    assert_eq!(apply(&p, &shown, &promote(), TODAY), Err(EditError::Changed)); // the writer that waited for the lock
    assert_eq!(fs::read_to_string(&p).unwrap(), first);
    assert_eq!(first, plain("overturned", "reviewed = \"2026-12-01\"\nreview_note = \"first\"\n"));
}
#[test] fn apply_refuses_a_front_matter_that_mixes_crlf_and_lf_and_changes_nothing() { // D§6.2 untouched lines byte-equal
    let text = plain("open", "");
    let (front, body) = text.split_at(text.len() - BODY.len());
    let lf_then_crlf = format!("+++\n{}{body}", front["+++\n".len()..].replace('\n', "\r\n"));
    let crlf_then_lf = format!("+++\r\n{}{body}", &front["+++\n".len()..]);
    for text in [lf_then_crlf, crlf_then_lf] {
        let t = tempfile::tempdir().unwrap();
        let p = put(t.path(), &text);
        assert!(parse_record(ID, text.as_bytes()).is_ok());
        let want = format!("zygo decision: {ID}: the front matter mixes CRLF and LF line endings; make them one kind first");
        for o in [promote(), overturn("x")] {
            assert_eq!(apply(&p, text.as_bytes(), &o, TODAY), Err(EditError::Refused(want.clone())), "{text:?}");
            assert_eq!(fs::read_to_string(&p).unwrap(), text);
        }
    }
}
#[test] fn apply_refuses_an_empty_promotion_text_or_reason_and_changes_nothing() { // D§11.3 "not empty"
    let empty_promotion = Outcome::Ratify { note: String::new(), promote: Some((Promotion::Rule, String::new())) };
    for (o, want) in [(empty_promotion, format!("zygo decision: {ID}: the promotion text is empty")),
                      (overturn(""), format!("zygo decision: {ID}: the reason is empty"))] {
        let t = tempfile::tempdir().unwrap();
        let p = put(t.path(), &plain("open", ""));
        assert_eq!(allowed(&parse_record(ID, plain("open", "").as_bytes()).unwrap(), &o), Err(want.clone()));
        assert_eq!(apply(&p, &fs::read(&p).unwrap(), &o, TODAY), Err(EditError::Refused(want)));
        assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
    }
}

const USAGE: &str = "zygo decision: usage: zygo decision new --kind <k> --title <t> --unit <u> --round <n> --role <r> [--class <c>] [--depends-on <id>]... [--supersedes <id>] | ls [--status <s>] [--kind <k>] [--unit <prefix>] [--pending] | show <id> | deps <id> | check | ratify <id> [--note <text>] [--promote adr|spec|rule --as <text>] [--yes] | overturn <id> --reason <text> [--yes]\n";
struct Ran { code: i32, out: String, err: String }
/// `run_in` with `top`, today TODAY, stdin `input`, and `env` for ZYGO_SESSION_DIR; no git, no clock, no process env.
fn go(top: &Path, env: &dyn Fn(&str) -> Option<OsString>, input: &mut dyn BufRead, args: &[&str]) -> Ran {
    let list = |_: &Path, _: &str, _: usize| Ok(vec![]);
    let (t, today) = (|| Some(top.to_path_buf()), || TODAY.to_string());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut io = DecisionIo { input, out: &mut out, err: &mut err, tty: false };
    let code = run_in(&Repo { top: &t, commits: &list, head: &|_| None, today: &today, usage_db: Path::new("") }, env, &args, &mut io);
    Ran { code, out: String::from_utf8(out).unwrap(), err: String::from_utf8(err).unwrap() }
}
fn run(top: &Path, args: &[&str]) -> Ran { go(top, &|_| None, &mut std::io::empty(), args) }
fn path_line(p: &Path) -> String { format!("{}\n", p.display()) }
/// Stdin that writes `bytes` to `path` at its first read, then answers `y`: an edit while the prompt waits.
struct EditOnRead { path: PathBuf, bytes: String, done: bool }
impl Read for EditOnRead {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done { return Ok(0); }
        fs::write(&self.path, &self.bytes)?;
        self.done = true;
        buf[..2].copy_from_slice(b"y\n");
        Ok(2)
    }
}

#[test] fn ratify_promote_rule_yes_writes_the_outcome_and_overturn_of_it_is_refused() { // D§17 #14
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &rich("open"));
    let r = run(t.path(), &["ratify", ID, "--promote", "rule", "--as", "Add the rule", "--yes"]);
    assert_eq!((r.code, r.out.clone(), r.err.as_str()), (0, path_line(&p), ""));
    assert_eq!(fs::read_to_string(&p).unwrap(), rich_promoted());
    let r = run(t.path(), &["overturn", ID, "--reason", "wrong", "--yes"]);
    assert_eq!((r.code, r.out.as_str(), r.err), (1, "", format!("zygo decision: {ID} is promoted; remove the promotion by hand first\n")));
    assert_eq!(fs::read_to_string(&p).unwrap(), rich_promoted());
}
#[test] fn ratify_and_overturn_write_their_keys() { // D§6.6 rows 1-3, D§11.3
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    assert_eq!(run(t.path(), &["ratify", ID, "--note", "ok", "--yes"]).code, 0);
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("ratified", "reviewed = \"2026-12-01\"\nreview_note = \"ok\"\n"));
    assert_eq!(run(t.path(), &["overturn", ID, "--reason", "wrong", "--yes"]).code, 0);   // ratified without promoted_to
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("overturned", "reviewed = \"2026-12-01\"\nreview_note = \"wrong\"\n"));
    put(t.path(), &plain("open", ""));
    assert_eq!(run(t.path(), &["ratify", ID, "--yes"]).code, 0);
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("ratified", "reviewed = \"2026-12-01\"\nreview_note = \"\"\n"));
}
#[test] fn operator_verbs_exit_2_in_an_agent_session_and_change_nothing() { // D§11.1, D-C5
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let sd = t.path().join("session");
    let env = |k: &str| (k == "ZYGO_SESSION_DIR").then(|| sd.clone().into_os_string());
    for (verb, args) in [("ratify", vec!["ratify", ID, "--yes"]), ("overturn", vec!["overturn", ID, "--reason", "x", "--yes"])] {
        let r = go(t.path(), &env, &mut std::io::empty(), &args);
        assert_eq!((r.code, r.out.as_str(), r.err), (2, "", format!("zygo decision {verb}: run it in your own shell, not in an agent session\n")));
        assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
    }
}
#[test] fn ratify_of_a_record_that_is_not_open_exits_1() { // D§10.1
    for (status, extra) in [("ratified", RATIFIED.to_string()), ("overturned", "reviewed = \"2026-11-30\"\nreview_note = \"no\"\n".into()),
                            ("fixed", format!("fixed_by = \"{C40}\"\n")), ("superseded", String::new())] {
        let t = tempfile::tempdir().unwrap();
        let p = put(t.path(), &plain(status, &extra));
        let r = run(t.path(), &["ratify", ID, "--yes"]);
        assert_eq!((r.code, r.out.as_str(), r.err), (1, "", format!("zygo decision: {ID} is {status}\n")));
        assert_eq!(fs::read_to_string(&p).unwrap(), plain(status, &extra));
        if status != "ratified" {
            let r = run(t.path(), &["overturn", ID, "--reason", "x", "--yes"]);
            assert_eq!((r.code, r.err), (1, format!("zygo decision: {ID} is {status}\n")));
        }
    }
}
#[test] fn argument_errors_exit_2_with_the_usage_line() { // D§10.1 `--promote` without `--as`; empty `--reason`
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    for a in [vec!["ratify"], vec!["ratify", ID, "--promote", "rule", "--yes"], vec!["ratify", ID, "--as", "t", "--yes"],
              vec!["ratify", ID, "--promote", "law", "--as", "t"], vec!["ratify", ID, "--promote", "rule", "--as", ""],
              vec!["ratify", ID, "--note"], vec!["ratify", ID, "--bogus"], vec!["overturn", ID, "--yes"],
              vec!["overturn", ID, "--reason", "", "--yes"], vec!["overturn", ID, "--reason"]] {
        let r = run(t.path(), &a);
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (2, "", USAGE), "{a:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
    }
}
#[test] fn without_yes_the_record_is_shown_and_only_y_writes() { // D§11.1 `[y/N]`
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let shown = format!("{}ratify {ID}? [y/N] ", plain("open", ""));
    for answer in ["n\n", "", "\n", "no\n"] {
        let r = go(t.path(), &|_| None, &mut answer.as_bytes(), &["ratify", ID]);
        assert_eq!((r.code, r.out.clone(), r.err), (1, shown.clone(), format!("zygo decision: {ID} not changed\n")), "{answer:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
    }
    let r = go(t.path(), &|_| None, &mut "y\n".as_bytes(), &["ratify", ID]);
    assert_eq!((r.code, r.out, r.err.as_str()), (0, format!("{shown}{}", path_line(&p)), ""));
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("ratified", "reviewed = \"2026-12-01\"\nreview_note = \"\"\n"));
}
#[test] fn a_record_changed_at_the_prompt_is_skipped_and_left_as_edited() { // D§11.3 re-read, D§14 two writers
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let edited = plain("open", "").replace("title = \"T\"", "title = \"T2\"");
    let mut input = BufReader::new(EditOnRead { path: p.clone(), bytes: edited.clone(), done: false });
    let r = go(t.path(), &|_| None, &mut input, &["overturn", ID, "--reason", "wrong"]);
    assert_eq!((r.code, r.err), (1, format!("{ID} changed since it was shown; skipped\n")));
    assert_eq!(fs::read_to_string(&p).unwrap(), edited);
}
#[test] fn failure_rows_of_d14() { // D§14 column `review, ratify, overturn`
    let t = tempfile::tempdir().unwrap();
    for a in [vec!["ratify", ID, "--yes"], vec!["overturn", ID, "--reason", "x", "--yes"]] {
        let r = run(t.path(), &a);                                             // docs/decisions absent
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, "no open records\n", ""), "{a:?}");
    }
    let t = tempfile::tempdir().unwrap();
    fs::create_dir_all(t.path().join("docs")).unwrap();
    fs::write(t.path().join("docs/decisions"), "").unwrap();                   // not a directory
    let r = run(t.path(), &["ratify", ID, "--yes"]);
    assert_eq!((r.code, r.err.as_str()), (1, "zygo decision: docs/decisions is not a directory\n"));
    let t = tempfile::tempdir().unwrap();
    let bad = plain("open", "").replace("\n## Evidence\n\nE.\n", "\n");          // malformed: missing section
    let p = put(t.path(), &bad);
    let r = run(t.path(), &["ratify", ID, "--yes"]);
    assert_eq!((r.code, r.err), (1, format!("zygo decision: {ID} is not a valid record\n")));
    assert_eq!(fs::read_to_string(&p).unwrap(), bad);
    let r = run(t.path(), &["overturn", "DDR-01JDQ2Y7Z8K4M5N6P7Q8R00009", "--reason", "x", "--yes"]);
    assert_eq!((r.code, r.err.as_str()), (1, "zygo decision: DDR-01JDQ2Y7Z8K4M5N6P7Q8R00009 is not a valid record\n"));
}
#[test] fn help_words_as_flag_values_are_values() { // D§10.1 `wants_help`
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    assert_eq!(run(t.path(), &["ratify", ID, "--note", "-h", "--yes"]).code, 0);
    assert_eq!(parse_record(ID, &fs::read(&p).unwrap()).unwrap().review_note.as_deref(), Some("-h"));
    let r = run(t.path(), &["ratify", ID, "-h"]);
    assert_eq!((r.code, r.out.as_str()), (0, USAGE));
}
#[cfg(unix)]
#[test] fn a_write_failure_exits_1_naming_the_file() { // D§14 "the write of an outcome fails"
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 { eprintln!("skipped: root ignores directory modes"); return; }
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let d = dir(t.path());
    fs::set_permissions(&d, fs::Permissions::from_mode(0o500)).unwrap();
    let r = run(t.path(), &["overturn", ID, "--reason", "x", "--yes"]);
    fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!((r.code, r.out.as_str()), (1, ""));
    assert!(r.err.starts_with("zygo decision: cannot write ") && r.err.contains("docs/decisions"), "{}", r.err);
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
}
#[test] fn the_prompt_of_a_crlf_record_holds_no_cr() { // D§11.1 the record is shown
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", "").replace('\n', "\r\n"));
    let r = go(t.path(), &|_| None, &mut "n\n".as_bytes(), &["ratify", ID]);
    assert_eq!((r.code, r.out.clone()), (1, format!("{}ratify {ID}? [y/N] ", plain("open", ""))));
    assert!(!r.out.contains('\r') && !r.out.contains("\\r"), "{:?}", r.out);
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", "").replace('\n', "\r\n"));
}

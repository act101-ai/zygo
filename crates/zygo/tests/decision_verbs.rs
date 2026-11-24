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
    let names: Vec<String> = fs::read_dir(dir(t.path())).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert_eq!(names, vec![format!("{ID}.md")]);                              // no .tmp left
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

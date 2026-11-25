//! DR-7: the record lines of `zygo work report` (D§9.5, D§6.8, D§17 #11).
use std::fs;
use std::path::{Path, PathBuf};
use zygo_usage::{RequestRow, Store, WorkUnitRow};
use zygo_work::report::report;

const C40: &str = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556";
const BODY: &str = "\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n";
const N12: &str = "phase-3:N12";

fn id(n: u32) -> String { format!("DDR-01JDQ2Y7Z8K4M5N6P7Q8R{n:05}") }
fn decisions(top: &Path) -> PathBuf { let d = top.join("docs/decisions"); fs::create_dir_all(&d).unwrap(); d }
fn put(dir: &Path, name: &str, text: &str) { fs::write(dir.join(name), text).unwrap(); }
/// A valid record file `<id(n)>.md` (as `crates/zygo-work/tests/decision_set.rs` `file`).
fn record(dir: &Path, n: u32, unit: &str, status: &str, deps: &[u32]) {
    let deps: Vec<String> = deps.iter().map(|d| id(*d)).collect();
    put(dir, &format!("{}.md", id(n)), &format!("+++\nid = \"{}\"\nkind = \"decision\"\nstatus = \"{status}\"\nclass = \"none\"\ntitle = \"T\"\nunit = \"{unit}\"\nround = 1\nrole = \"code\"\nmodel = \"unknown\"\ncommit = \"{C40}\"\ndate = \"2026-11-24\"\ndepends_on = {deps:?}\nreverse_cost = \"low\"\n+++\n{BODY}", id(n)));
}
/// One store: each unit one round by `main` in session S1 at `began`; one lead request at ts 150.
fn db(dir: &Path, units: &[(&str, i64)]) -> PathBuf {
    let db = dir.join("usage.db");
    let s = Store::open(&db).unwrap();
    for (unit, began) in units {
        s.insert_work_unit(&WorkUnitRow { unit: (*unit).into(), round: 1, session: "S1".into(), agent_id: "main".into(),
            began_at: *began, ..Default::default() }).unwrap();
    }
    s.insert_request(&RequestRow { ts: 150, session: "S1".into(), agent: "claude-code".into(), role_served: Some("lead".into()),
        model: "m1".into(), input_tokens: 7, output_tokens: 1, estimated_usd: 0.0625, ..Default::default() }).unwrap();
    db
}
/// D§17 #11: 3 valid records of N12 (2 open, a chain of 2), one malformed name, one ignored name.
fn fixture(top: &Path) {
    let d = decisions(top);
    record(&d, 1, N12, "open", &[]);
    record(&d, 2, N12, "open", &[1]);
    record(&d, 3, N12, "ratified", &[]);
    put(&d, "DDR-bad.md", "x");
    put(&d, "README.md", "x");
}
const N12_BLOCK: &str = concat!(
    "phase-3:N12\n",
    "  rounds: 1\n",
    "  round 1 main: check ran no, passed no; verdict none\n",
    "  guard events: none\n");
const N12_TOKENS: &str = "  lead m1: 1 requests, 7 in, 1 out, 0 cache read, 0 cache write, $0.0625";

#[test] fn fixture_repository_gives_both_lines() { // D§17 #11
    let t = tempfile::tempdir().unwrap();
    let top = t.path().join("repo"); fixture(&top);
    let db = db(t.path(), &[(N12, 100)]);
    assert_eq!(report(&db, Some(&top), None, None).unwrap(), format!(
        "{N12_BLOCK}  decisions: 2 open, 3 total\n{N12_TOKENS}\n\ndecision records: 2 open, 3 total, deepest chain 2, 1 malformed"));
    // The unit filter selects blocks only; the records line stays.
    assert_eq!(report(&db, Some(&top), Some("beads:bd-a1b2"), None).unwrap(),
        "no work units\n\ndecision records: 2 open, 3 total, deepest chain 2, 1 malformed");
}
#[test] fn no_repository_no_record_lines() { // D§9.5 bullet 4, D§17 #11; the slash form passes None
    let t = tempfile::tempdir().unwrap();
    let top = t.path().join("repo"); fixture(&top);
    let db = db(t.path(), &[(N12, 100)]);
    assert_eq!(report(&db, None, None, None).unwrap(), format!("{N12_BLOCK}{N12_TOKENS}"));
}
#[test] fn part_unit_counts_for_its_parent_block() { // D§9.5 bullet 1
    let t = tempfile::tempdir().unwrap();
    let top = t.path().join("repo"); let d = decisions(&top);
    record(&d, 1, "phase-3:N12:plan", "open", &[]);
    record(&d, 2, "phase-3:N1", "open", &[]);          // a prefix without `:` is another unit
    let db = db(t.path(), &[(N12, 100), ("beads:bd-a1b2", 300)]);
    assert_eq!(report(&db, Some(&top), None, None).unwrap(), format!(concat!(
        "{}  decisions: 1 open, 1 total\n{}\n\n",
        "beads:bd-a1b2\n  rounds: 1\n  round 1 main: check ran no, passed no; verdict none\n  guard events: none\n\n",
        "decision records: 2 open, 2 total, deepest chain 1"), N12_BLOCK, N12_TOKENS));
}
#[test] fn no_units_then_the_records_line() { // D§9.5 bullet 3
    let t = tempfile::tempdir().unwrap();
    let top = t.path().join("repo"); record(&decisions(&top), 1, N12, "open", &[]);
    let absent = t.path().join("absent.db");
    assert_eq!(report(&absent, Some(&top), None, None).unwrap(), "no work units\n\ndecision records: 1 open, 1 total, deepest chain 1");
    assert!(!absent.exists());
}
#[test] fn record_names_decide_the_records_line() { // D§9.5 bullet 2, D§6.8 names, D§14 absent
    let t = tempfile::tempdir().unwrap();
    let top = t.path().join("repo");
    let absent = t.path().join("absent.db");
    assert_eq!(report(&absent, Some(&top), None, None).unwrap(), "no work units");           // no docs/decisions
    let d = decisions(&top);
    put(&d, "README.md", "x"); put(&d, "DDR-bad.md", "x");
    assert_eq!(report(&absent, Some(&top), None, None).unwrap(), "no work units");           // no record name
    put(&d, &format!("{}.md", id(1)), "not a record");
    assert_eq!(report(&absent, Some(&top), None, None).unwrap(),
        "no work units\n\ndecision records: 0 open, 0 total, deepest chain 0, 2 malformed");
}
#[test] fn regular_file_is_not_a_directory() { // D§6.8, D§14
    let t = tempfile::tempdir().unwrap();
    let top = t.path().join("repo"); fs::create_dir_all(top.join("docs")).unwrap();
    put(&top, "docs/decisions", "x");
    assert_eq!(report(&t.path().join("absent.db"), Some(&top), None, None).unwrap(),
        "no work units\n\ndecision records: docs/decisions is not a directory");
}
#[test] fn over_ten_thousand_entries() { // D§6.8
    let t = tempfile::tempdir().unwrap();
    let top = t.path().join("repo"); let d = decisions(&top);
    for n in 0..=10_000 { put(&d, &format!("f{n:05}"), ""); }
    assert_eq!(report(&t.path().join("absent.db"), Some(&top), None, None).unwrap(),
        "no work units\n\ndecision records: over 10,000 files");
}
#[test] fn part_record_counts_in_its_own_block_and_its_parent() { // D§9.5 bullet 1: pins the current behaviour
    let t = tempfile::tempdir().unwrap();
    let top = t.path().join("repo"); let d = decisions(&top);
    record(&d, 1, "phase-3:N12:review", "open", &[]);
    let db = db(t.path(), &[(N12, 100), ("phase-3:N12:review", 200)]);
    assert_eq!(report(&db, Some(&top), None, None).unwrap(), format!(concat!(
        "{}  decisions: 1 open, 1 total\n{}\n\n",
        "phase-3:N12:review\n  rounds: 1\n  round 1 main: check ran no, passed no; verdict none\n  guard events: none\n",
        "  decisions: 1 open, 1 total\n\n",
        "decision records: 1 open, 1 total, deepest chain 1"), N12_BLOCK, N12_TOKENS));
}
#[cfg(unix)]
#[test] fn symlink_is_not_a_directory() { // D§6.8
    let t = tempfile::tempdir().unwrap();
    let top = t.path().join("repo"); fs::create_dir_all(top.join("docs")).unwrap();
    let real = t.path().join("real"); fs::create_dir_all(&real).unwrap();
    record(&real, 1, N12, "open", &[]);
    std::os::unix::fs::symlink(&real, top.join("docs/decisions")).unwrap();
    assert_eq!(report(&t.path().join("absent.db"), Some(&top), None, None).unwrap(),
        "no work units\n\ndecision records: docs/decisions is not a directory");
}

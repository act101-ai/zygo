//! DR-2: the record set, the set checks and the dependency graph (D§6.8, D§7, D§5, D§9.5, D§11.2).
use std::fs;
use std::path::{Path, PathBuf};
use zygo_work::decisions::graph::*;
use zygo_work::decisions::record::{Class, Kind, Record, Status};
use zygo_work::decisions::set::*;

const C40: &str = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556";
const BODY: &str = "\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n";

fn id(n: u32) -> String { format!("DDR-01JDQ2Y7Z8K4M5N6P7Q8R{n:05}") }
fn rec(n: u32, status: Status, deps: &[u32], supersedes: Option<u32>) -> Record {
    Record { id: id(n), kind: Kind::Decision, status, class: Class::None, title: "T".into(), unit: "phase-3:N12".into(),
             round: 1, role: "code".into(), model: "unknown".into(), commit: C40.into(), date: "2026-11-24".into(),
             depends_on: deps.iter().map(|d| id(*d)).collect(), reverse_cost: "low".into(), supersedes: supersedes.map(id),
             fixed_by: None, promoted_by: None, reviewed: None, review_note: None, promoted_to: None, promotion: None,
             body: BODY.into() }
}
fn open(n: u32, deps: &[u32]) -> Record { rec(n, Status::Open, deps, None) }

#[test] fn dependents_breadth_first_once_at_smallest_depth() { // D§7, D§17 #4
    let mut x = open(1, &[]); x.kind = Kind::Assumption;
    let rs = vec![x, open(2, &[1]), open(3, &[2]), open(4, &[3]), open(5, &[4, 3])]; // 5: depth 3 via 3, depth 4 via 4
    assert_eq!(dependents(&rs, &id(1)), Dependents { rows: vec![(1, id(2)), (2, id(3)), (3, id(4)), (3, id(5))], cycle_at: None });
    assert_eq!(dependents(&rs, &id(4)), Dependents { rows: vec![(1, id(5))], cycle_at: None });
    assert_eq!(dependents(&rs, &id(9)), Dependents { rows: vec![], cycle_at: None });
}
#[test] fn dependents_cut_cycle_sets_cycle_at() { // D§7
    let rs = vec![open(1, &[]), open(2, &[1, 3]), open(3, &[2])];
    assert_eq!(dependents(&rs, &id(1)), Dependents { rows: vec![(1, id(2)), (2, id(3))], cycle_at: Some(id(2)) });
    assert_eq!(dependents(&rs, &id(2)), Dependents { rows: vec![(1, id(3))], cycle_at: Some(id(2)) });
}
#[test] fn dependent_count_transitive_and_skip_superseded() { // D§5, D§11.2 step 2
    let rs = vec![open(1, &[]), open(2, &[1]), rec(3, Status::Superseded, &[1], None), open(4, &[3])];
    assert_eq!(dependent_count(&rs, &id(1), false), 3);
    assert_eq!(dependent_count(&rs, &id(1), true), 1);
    assert_eq!(dependent_count(&rs, &id(2), false), 0);
}
#[test] fn deepest_chain_cases() { // D§9.5
    assert_eq!(deepest_chain(&[]), 0);
    assert_eq!(deepest_chain(&[open(1, &[])]), 1);
    assert_eq!(deepest_chain(&[open(1, &[]), open(2, &[1])]), 2);
    assert_eq!(deepest_chain(&[open(1, &[]), open(2, &[1]), open(3, &[2]), open(4, &[1])]), 3);
    assert_eq!(deepest_chain(&[open(1, &[2]), open(2, &[1])]), 2); // cut at the first repeat
    assert_eq!(deepest_chain(&[open(1, &[9])]), 1);                // a missing target ends the path
}

fn file(rid: &str, deps: &[String]) -> String { // a valid record whose `id` is `rid`
    format!("+++\nid = \"{rid}\"\nkind = \"decision\"\nstatus = \"open\"\nclass = \"none\"\ntitle = \"T\"\nunit = \"phase-3:N12\"\nround = 1\nrole = \"code\"\nmodel = \"unknown\"\ncommit = \"{C40}\"\ndate = \"2026-11-24\"\ndepends_on = {deps:?}\nreverse_cost = \"low\"\n+++\n{BODY}")
}
fn put(dir: &Path, name: &str, text: &str) { fs::write(dir.join(name), text).unwrap(); }
fn decisions(top: &Path) -> PathBuf { let d = top.join("docs/decisions"); fs::create_dir_all(&d).unwrap(); d }
fn set(valid: Vec<Record>) -> RecordSet { RecordSet { valid, malformed: vec![] } }
fn md(n: u32) -> String { format!("{}.md", id(n)) }
fn lines(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }
#[cfg(unix)]
fn mkfifo(path: &Path) { // as crates/zygo-work/tests/decision_record.rs:52
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo {}", path.display());
}

#[test] fn absent_directory_is_empty_set() { // D§6.8, D§14
    let t = tempfile::tempdir().unwrap();
    assert_eq!(read_dir(t.path()), Ok(RecordSet::default()));
    fs::create_dir(t.path().join("docs")).unwrap();
    assert_eq!(read_dir(t.path()), Ok(RecordSet::default()));
}
#[test] fn regular_file_is_not_a_directory() { // D§6.8
    let t = tempfile::tempdir().unwrap();
    fs::create_dir(t.path().join("docs")).unwrap();
    put(t.path(), "docs/decisions", "x");
    assert_eq!(read_dir(t.path()), Err(DirError::NotDirectory));
}
#[cfg(unix)]
#[test] // review focus 42 (D§6.8)
fn linked_directory_is_not_a_directory() {
    let t = tempfile::tempdir().unwrap();
    let real = t.path().join("real"); fs::create_dir(&real).unwrap(); put(&real, &md(1), &file(&id(1), &[]));
    fs::create_dir(t.path().join("docs")).unwrap();
    std::os::unix::fs::symlink(&real, t.path().join("docs/decisions")).unwrap();
    assert_eq!(read_dir(t.path()), Err(DirError::NotDirectory));
}
#[test] fn reads_records_and_ignores_other_names() { // D§6.8, D§17 #3
    let t = tempfile::tempdir().unwrap(); let d = decisions(t.path());
    put(&d, &md(1), &file(&id(1), &[]));
    put(&d, &md(2), &file(&id(2), &[id(1)]));
    put(&d, "README.md", "# Decision records\n");
    put(&d, "x.tmp", "+++\n");
    put(&d, &format!("{}.tmp", id(3)), &file(&id(3), &[]));
    fs::create_dir(d.join("sub")).unwrap(); put(&d.join("sub"), &md(4), &file(&id(4), &[]));
    put(&d, "DDR-bad.md", &file(&id(5), &[]));
    put(&d, &md(6), &file(&id(7), &[]));          // id is not the file stem
    fs::create_dir(d.join(md(8))).unwrap();       // a directory with a record name
    let s = read_dir(t.path()).unwrap();
    assert_eq!(s.valid.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), vec![id(1), id(2)]);
    assert_eq!(s.valid[1].depends_on, vec![id(1)]);
    assert_eq!(s.malformed, vec![(md(6), lines(&["id does not equal the file stem"])),
                                 (md(8), lines(&["is not a regular file; not read"])),
                                 ("DDR-bad.md".to_string(), lines(&["bad record name"]))]);
}
#[cfg(unix)]
#[test] // review focus 42 (D§6.8): never followed, never opened (an open FIFO would block this test)
fn link_and_fifo_entries_are_malformed_and_not_opened() {
    let t = tempfile::tempdir().unwrap(); let d = decisions(t.path());
    put(t.path(), "target.md", &file(&id(1), &[]));
    std::os::unix::fs::symlink(t.path().join("target.md"), d.join(md(1))).unwrap();
    mkfifo(&d.join(md(2)));
    mkfifo(&d.join("x.fifo"));
    assert_eq!(read_dir(t.path()), Ok(RecordSet { valid: vec![], malformed: vec![
        (md(1), lines(&["is a link; not read"])), (md(2), lines(&["is not a regular file; not read"]))] }));
}
#[test] fn ten_thousand_entries_then_too_many() { // D§6.8
    let t = tempfile::tempdir().unwrap(); let d = decisions(t.path());
    for n in 0..10_000 { put(&d, &format!("f{n:05}"), ""); }
    assert_eq!(read_dir(t.path()), Ok(RecordSet::default()));
    put(&d, "f10000", "");
    assert_eq!(read_dir(t.path()), Err(DirError::TooMany));
}
#[test] fn set_faults_clean_set_and_malformed_files_give_none() { // D§6.8
    assert_eq!(set_faults(&set(vec![open(1, &[]), open(2, &[1]), rec(3, Status::Superseded, &[], None), rec(4, Status::Open, &[1], Some(3))])), Vec::<String>::new());
    assert_eq!(set_faults(&RecordSet { valid: vec![], malformed: vec![("DDR-bad.md".into(), lines(&["bad record name"]))] }), Vec::<String>::new());
}
#[test] fn set_faults_each_fault() { // D§6.8 set checks, D§17 #3
    assert_eq!(set_faults(&set(vec![open(1, &[]), open(1, &[])])), vec![format!("duplicate id {}", id(1))]);
    assert_eq!(set_faults(&set(vec![open(1, &[9]), rec(2, Status::Open, &[], Some(8))])),
               vec![format!("{}: depends_on {} is not a record", id(1), id(9)), format!("{}: supersedes {} is not a record", id(2), id(8))]);
    assert_eq!(set_faults(&set(vec![open(1, &[2]), open(2, &[1])])), vec![format!("depends_on cycle at {}", id(1))]);
    assert_eq!(set_faults(&set(vec![open(3, &[3])])), vec![format!("depends_on cycle at {}", id(3))]);
    assert_eq!(set_faults(&set(vec![open(1, &[]), rec(2, Status::Open, &[], Some(1))])),
               vec![format!("{}: supersedes {}, which is open, not superseded", id(2), id(1))]);
}
#[test] fn ten_thousand_record_chain_has_no_depth_limit() { // the directory limit, walked without recursion
    let mut rs: Vec<Record> = (0..10_000u32).map(|n| open(n, &(if n == 0 { vec![] } else { vec![n - 1] }))).collect();
    rs.reverse(); // the first DFS start is the deepest record
    assert_eq!(deepest_chain(&rs), 10_000);
    let d = dependents(&rs, &id(0));
    assert_eq!((d.rows.len(), d.rows.last().cloned(), d.cycle_at), (9_999, Some((9_999, id(9_999))), None));
    assert_eq!(dependent_count(&rs, &id(0), false), 9_999);
    assert_eq!(set_faults(&set(rs)), Vec::<String>::new());
}

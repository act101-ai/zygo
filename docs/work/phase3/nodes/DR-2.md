# DR-2 — Record set: directory reader, set checks and the dependency graph

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§6.8 (the directory, the 10,000-entry limit, the set checks), D§7, D§5 (Dependent), D§9.5 (deepest chain), D§11.2 step 2 (dependent count), D-C3
**Files:** `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/set.rs`, `crates/zygo-work/src/decisions/graph.rs`, `crates/zygo-work/tests/decision_set.rs`
**Consumes:** `Record` (DR-1), `read_record` (DR-1), `name_kind` (DR-1), `DIR` (DR-1), `Status` (DR-1)
**Produces:** `decisions::set::RecordSet` (struct) `decisions::set::RecordSet { pub valid: Vec<Record>, pub malformed: Vec<(String, Vec<String>)> }` (file name, fault lines); `decisions::set::DirError` (`NotDirectory | TooMany`); `decisions::set::read_dir(top: &Path) -> Result<RecordSet, DirError>` (absent directory → empty set; subdirectories and non-record names ignored; at most 10,000 entries considered); `decisions::set::set_faults(set: &RecordSet) -> Vec<String>` (duplicate id, missing `depends_on` or `supersedes` target, `depends_on` cycle, `supersedes` target not `superseded`); `decisions::graph::Dependents { pub rows: Vec<(u32, String)>, pub cycle_at: Option<String> }`; `decisions::graph::dependents(records: &[Record], id: &str) -> Dependents` (breadth first, each id once at its smallest depth, a cycle cut at the first repeat); `decisions::graph::dependent_count(records: &[Record], id: &str, skip_superseded: bool) -> usize` (transitive); `decisions::graph::deepest_chain(records: &[Record]) -> usize`
**Acceptance:** `cargo test -p zygo-work --locked --test decision_set` → all pass. The cases include a `depends_on` cycle, a missing `depends_on` id, a duplicate id, a `supersedes` target still `open`; `README.md`, `x.tmp` and a subdirectory ignored; 10,001 entries → `TooMany`; `docs/decisions` a regular file → `NotDirectory` (a link → `NotDirectory`, `#[cfg(unix)]`); an absent directory → empty set; an assumption with a 3-record chain and a fourth record depending on the second and third → rows with depths 1, 2, 3, 3 and the fourth record once; a cut cycle sets `cycle_at`; `deepest_chain` is 1 for one record with no `depends_on` and 2 for a two-record chain; `dependent_count` with `skip_superseded = true` leaves out a `superseded` dependent.

## Plan

Implementer notes. Paths are relative to the repository root. "The spec" is `docs/specs/2026-11-24-decision-records-design.md`; D§n is a section of it. This node reads `docs/decisions/` into a set, checks the set (D§6.8 set checks) and computes the derived values of D§7, D§9.5 and D§11.2 at each call (D-C3: no copy, no index). It prints nothing (DR-3, DR-7 print) and reads no clock.

Reused code (do not rebuild): `read_record` (`crates/zygo-work/src/decisions/record.rs:359`: `lstat` first, never opens a link, a FIFO or a directory, gives the fault lines); `name_kind` (`record.rs:130`); `DIR` (`record.rs:7`); `mkfifo` (copy from `crates/zygo-work/tests/decision_record.rs:52`); `BODY` (copy from `decision_record.rs:10`).

Consumes (`zygo_work::decisions::record`, as in code now):

```rust
pub const DIR: &str = "docs/decisions";
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Status { Open, Ratified, Overturned, Fixed, Superseded } // pub fn name(self) -> &'static str
#[derive(Debug, Clone, PartialEq, Eq)] pub struct Record { pub id: String, pub status: Status, pub depends_on: Vec<String>, pub supersedes: Option<String>, /* and the other D§6.3 fields */ }
#[derive(Debug, Clone, PartialEq, Eq)] pub enum NameKind { Record(String), Malformed, Other }
pub fn name_kind(file_name: &str) -> NameKind;
pub fn read_record(path: &Path) -> Result<Record, Vec<String>>;
```

Produces:

```rust
// crates/zygo-work/src/decisions/set.rs  (zygo_work::decisions::set)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordSet { pub valid: Vec<Record>, pub malformed: Vec<(String, Vec<String>)> } // (file name, fault lines)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirError { NotDirectory, TooMany }
pub fn read_dir(top: &Path) -> Result<RecordSet, DirError>;   // top = repository root; reads top.join(DIR)
pub fn set_faults(set: &RecordSet) -> Vec<String>;
// crates/zygo-work/src/decisions/graph.rs  (zygo_work::decisions::graph)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependents { pub rows: Vec<(u32, String)>, pub cycle_at: Option<String> } // (depth, id)
pub fn dependents(records: &[Record], id: &str) -> Dependents;
pub fn dependent_count(records: &[Record], id: &str, skip_superseded: bool) -> usize;
pub fn deepest_chain(records: &[Record]) -> usize;
```

Checked against the consumers `docs/work/phase3/nodes/DR-3.md` (`read_dir`, `set_faults`, `RecordSet`, `DirError`, `dependents`, `dependent_count`), `DR-4.md` and `DR-5.md` (`read_dir`), `DR-6.md` (`read_dir`, `dependent_count`), `DR-7.md` (`read_dir`, `RecordSet`, `DirError`, `deepest_chain`). Names and types match.

Fixed values: at most 10,000 directory entries (D§6.8); the malformed-name fault line `bad record name`; the set fault lines, in this order:
1. `duplicate id <id>` (one per repeat, in slice order);
2. per record in slice order: `<id>: depends_on <dep> is not a record` (in `depends_on` order), then `<id>: supersedes <t> is not a record` or `<id>: supersedes <t>, which is <status name>, not superseded`;
3. `depends_on cycle at <id>`, one per back edge, in the order of `back_edges`.

Readings of the spec that this plan fixes:
- "Names a record" (D§6.8 set checks) means a record in `set.valid`. A malformed file is not in the graph (D§7) and has its own fault lines.
- `read_dir` lists the names first and reads records after: `TooMany` comes before any record file is opened. A `lstat` error other than "not found", a failed open of the directory or a failed entry gives `NotDirectory` (the contract has two variants; an empty set here would let `check` pass over unread records).
- `skip_superseded = true` removes `superseded` records from the graph before the walk: such a record is not counted and not walked through ("over all valid records that are not superseded", D§11.2 step 2).
- `Dependents.rows` is sorted by depth, then id.
- No recursion in `graph.rs` or `set.rs`: a 10,000-record chain (the directory limit) must not overflow a test thread's stack.

### Steps

- [x] **1. Write the failing graph tests.** Create `crates/zygo-work/tests/decision_set.rs` with the header, helpers and four tests below.

```rust
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
```

```rust
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
```

- [x] **2. Run and see it fail.** `cargo test -p zygo-work --locked --test decision_set` → compile error `unresolved import zygo_work::decisions::graph` (and `set`). Comment out the `set` import line until step 6.

- [x] **3. Implement the graph.** Create `crates/zygo-work/src/decisions/graph.rs` with `Dependents`, `dependents`, `dependent_count` and `deepest_chain` as in Produces, and add `pub mod graph;` to `crates/zygo-work/src/decisions/mod.rs` (before `pub mod record;`). Private helpers, also used by `set.rs`:
  - `pub(crate) fn back_edges(next: &HashMap<&str, Vec<&str>>, starts: &[&str]) -> Vec<String>`: iterative three-color DFS from each start in order, children in listed order; each edge to a node on the current path adds that node's id.
  - `fn inverse(records: &[Record]) -> HashMap<&str, Vec<&str>>`: for each `d` in `r.depends_on`, an edge `d → r.id`; each child list sorted by id and deduplicated.
  - `dependents`: BFS over `inverse` from `id`, seen set starts with `id`, a new child gets its parent's depth + 1; rows sorted by (depth, id); `cycle_at = back_edges(&inverse, &[id]).into_iter().next()`.
  - `dependent_count`: filter out `Status::Superseded` when `skip_superseded`, then `dependents(..).rows.len()`.
  - `deepest_chain`: forward edges `r.id → d` only where `d` is in `records`; iterative post-order DFS with a memo of the longest path from each record (counted in records); an edge to a record on the current path counts 0; result is the largest memo value, 0 for no records.

- [x] **4. Run and see it pass.** `cargo test -p zygo-work --locked --test decision_set` → 4 passed.

- [x] **5. Commit.** `git add crates/zygo-work/src/decisions/graph.rs crates/zygo-work/src/decisions/mod.rs crates/zygo-work/tests/decision_set.rs`, message `feat(work): decision dependency graph (DR-2)` with the `Zygo-Session:` and `Zygo-Work:` trailers of the implementer's brief.

- [x] **6. Write the failing set tests.** Restore the `set` import and append these helpers and nine tests to `crates/zygo-work/tests/decision_set.rs`.

```rust
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
```

```rust
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
```

- [x] **7. Run and see it fail.** `cargo test -p zygo-work --locked --test decision_set` → compile error `unresolved import zygo_work::decisions::set`.

- [x] **8. Implement the set.** Create `crates/zygo-work/src/decisions/set.rs` with `RecordSet`, `DirError`, `read_dir` and `set_faults` as in Produces, `const MAX_ENTRIES: usize = 10_000;`, and add `pub mod set;` to `mod.rs` (after `pub mod record;`). `read_dir`:
  1. `dir = top.join(DIR)`; `fs::symlink_metadata(&dir)`: `NotFound` → `Ok(RecordSet::default())`; any other error, a link, or not a directory → `Err(NotDirectory)`.
  2. `fs::read_dir(&dir)`; an open error or an entry error → `Err(NotDirectory)`. Collect the file names; the 10,001st entry → `Err(TooMany)`. Do not open any file in this pass.
  3. For each name that is UTF-8 (others are ignored): `name_kind` → `Record(_)`: `read_record(&dir.join(name))` into `valid` or `(name, faults)` into `malformed`; `Malformed`: `(name, vec!["bad record name"])`; `Other`: ignore. Subdirectories are never entered (their names are `Other`, or a record name that `read_record` refuses).
  4. Sort `valid` by `id` and `malformed` by name.

  `set_faults`: the fixed lines above; known ids are the ids in `set.valid`; the cycle lines are `graph::back_edges` over the forward edges `r.id → d` (only `d` in the set), starts = the ids in slice order.

- [x] **9. Run and see it pass.** `cargo test -p zygo-work --locked --test decision_set` → 13 passed on unix (11 on Windows: the two `#[cfg(unix)]` tests compile to nothing).

- [x] **10. Commit.** `git add crates/zygo-work/src/decisions/set.rs crates/zygo-work/src/decisions/mod.rs crates/zygo-work/tests/decision_set.rs`, message `feat(work): decision record set and set checks (DR-2)` with the implementer's trailers.

- [x] **11. Acceptance.** `cargo test -p zygo-work --locked --test decision_set` → `test result: ok. 13 passed; 0 failed`.

- [x] **12. Floor.** `cargo test --workspace --locked --no-fail-fast` → every `test result:` line `ok`, exit 0.

- [x] **13. Review focus.** Line 42: `linked_directory_is_not_a_directory` and `link_and_fifo_entries_are_malformed_and_not_opened` pass (step 11). Line 75: `grep -nE 'SystemTime|Instant|time::|Command' crates/zygo-work/src/decisions/set.rs crates/zygo-work/src/decisions/graph.rs crates/zygo-work/tests/decision_set.rs` → no output (no clock, no spawned process).

### Review round 1

- F1 partly fixed: `dependent_count` now does only the visited-set walk over the inverse graph, with no cycle detection; the per-record O(N+E) walk stays, because D-C3 computes each count at each use (probe of 10,000 records × 64 `depends_on`, 200 calls, `--release`: 11.47 s before, 5.98 s after on the implementer's machine).
- F2 fixed: `deepest_chain` cuts cycles at the back edges of a DFS that starts at the records no record depends on, then at the others, each group in ascending id order, with children sorted; it then takes the memoized longest path of the acyclic graph that is left, so the result does not depend on input order.
- F3 fixed: `set_faults` removes duplicate lines and keeps the first occurrence, so the fixed order of kinds still holds (D§17 #3: one line each).
- F4 rejected: D§11.2 says only "cut at the first repeat"; a back edge from a fixed-order DFS is the exact, deterministic test for a cycle (a diamond is also a repeat in a BFS), and no consumer depends on which cycle id is named.
- F5 fixed: a non-UTF-8 name whose bytes start with `DDR-` and end with `.md` is malformed (`bad record name`, under its lossy name); any other non-UTF-8 name is ignored (D§6.8).
- F6 partly fixed: `docs` is also checked with lstat; a link there gives `NotDirectory`, and a missing `docs` still gives the empty set. Parts 2–4 rejected: the lstat/read_dir race needs a local writer who can already change any file; `docs` as a regular file gives the true `NotDirectory`; permission errors give `NotDirectory` (author's call 1).
- F7 partly fixed: tests added for the fixed order of set fault kinds and for a self-loop in `dependents`. The other gaps (TooMany before any open, removal between listing and read, permission error) are not observable without fault injection, and the plan's ordering covers them.

# DR-7 — `zygo work report`: the two record lines

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§9.5, D§6.8 (the two directory lines of the report), D§14 (column `zygo work report`), D§13 item 4, D-C2
**Files:** `crates/zygo-work/src/report.rs`, `crates/zygo/src/cmd/work.rs`, `crates/zygo-work/tests/collection.rs` (the existing report::report calls), `crates/zygo-work/tests/decision_report.rs`, `docs/specs/2026-11-18-zygo-design.md` (§13.6, one change-log line)
**Consumes:** `read_dir` (DR-2), `RecordSet` (DR-2), `DirError` (DR-2), `deepest_chain` (DR-2), `Record` (DR-1), `Status` (DR-1)
**Produces:** `zygo_work::report::report(db: &Path, records_in: Option<&Path>, unit: Option<&str>, since: Option<i64>) -> anyhow::Result<String>` (`records_in = None`: no record lines; the slash form of `work report` passes `None`)
**Acceptance:** `cargo test -p zygo-work --locked --test decision_report` → all pass. Covers spec Acceptance #11 (a fixture repository gives `  decisions: 2 open, 3 total` in the unit block, after the `guard events:` line, and `decision records: 2 open, 3 total, deepest chain 2, 1 malformed` after the last block; outside a repository no records line); a `:<part>` unit counts for its parent block; no units and one record → `no work units` then the records line; `docs/decisions` a regular file → `decision records: docs/decisions is not a directory`; 10,001 entries → `decision records: over 10,000 files`; `records_in = None` → no records line.

## Plan

Implementer notes. Paths are relative to the repository root. "The spec" is `docs/specs/2026-11-24-decision-records-design.md`; D§n is a section of it. "The base spec" is `docs/specs/2026-11-18-zygo-design.md`. This node adds the two record lines of D§9.5 to `zygo work report`. It reads `docs/decisions/` with the DR-2 reader at each call (D-C3) and writes nothing.

Reused code (do not rebuild):
- `read_dir` (`crates/zygo-work/src/decisions/set.rs:35`): absent directory → empty set; a file or link at `docs` or `docs/decisions` → `DirError::NotDirectory`; more than 10,000 entries of any name → `DirError::TooMany` (`set.rs:44-49`).
- `deepest_chain` (`crates/zygo-work/src/decisions/graph.rs:123`): the D§9.5 `<d>`; a cycle is cut, a missing target ends the path.
- `name_kind` (`crates/zygo-work/src/decisions/record.rs:130`): tells a record name (`NameKind::Record`) from a `DDR-*.md` name that does not match (`NameKind::Malformed`).
- `repo_top` (`crates/zygo/src/cmd/status.rs:21-24`): the top of the checkout at the process cwd, `repo::checkout` with a 2 s deadline; `None` outside a repository (`find_checkout` maps git's "not a repository" to `Ok(None)`, `crates/zygo-work/src/repo.rs:474-479`) and `None` when a git call fails or passes the bound (`repo.rs:492-494`). Already used by `guards.rs:37`, `doctor.rs:300`, `help.rs:89`.
- The unit filter of `report` (`crates/zygo-work/src/report.rs:26`): `u == unit` or `u` is `unit` + `:<part>`. The same test decides which records count for a block.

Consumes (as in the code now):

```rust
// zygo_work::decisions::set  (DR-2)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordSet { pub valid: Vec<Record>, pub malformed: Vec<(String, Vec<String>)> } // valid sorted by id; (file name, faults) sorted by name
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirError { NotDirectory, TooMany }
pub fn read_dir(top: &Path) -> Result<RecordSet, DirError>;
// zygo_work::decisions::graph  (DR-2)
pub fn deepest_chain(records: &[Record]) -> usize;
// zygo_work::decisions::record  (DR-1)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record { pub id: String, pub status: Status, pub unit: String, pub depends_on: Vec<String>, /* and the other D§6.3 fields */ }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status { Open, Ratified, Overturned, Fixed, Superseded }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameKind { Record(String), Malformed, Other }
pub fn name_kind(file_name: &str) -> NameKind;
// crates/zygo/src/cmd/status.rs
pub fn repo_top() -> Option<PathBuf>;
```

Produces:

```rust
// crates/zygo-work/src/report.rs  (zygo_work::report)
pub fn report(db: &Path, records_in: Option<&Path>, unit: Option<&str>, since: Option<i64>) -> anyhow::Result<String>;
// records_in = the repository top; None: no record lines. The slash form of `work report` passes None.
pub fn render(joined: &[Joined], records: &[Record]) -> String;   // gains `records`; no caller outside report.rs
```

Fixed values (D§9.5, D§6.8): `  decisions: <open> open, <total> total`; `decision records: <open> open, <total> total, deepest chain <d>` then `, <m> malformed` when `m > 0`; `decision records: docs/decisions is not a directory`; `decision records: over 10,000 files`; `no work units` (`report.rs:11`).

Readings of the spec that this plan fixes:
- Unit line: `<total>` is the number of valid records whose `unit` is the block's unit or the block's unit + `:<part>`; `<open>` is those with `status = open`. The line comes right after `  guard events:` and before the token lines; no such record → no line.
- Records line: `<total>` = `valid.len()`, `<open>` = valid records with `status = open`, `<d>` = `deepest_chain(&valid)`, `<m>` = `malformed.len()` (bad names and bad files). It prints when at least one directory entry is a record name in the D§6.8 sense: a valid record, or a malformed entry whose name is `NameKind::Record`. A `DDR-bad.md` alone (or `README.md`) gives no line.
- "After one empty line" holds also after `no work units`: the output is `no work units\n\n<records line>`.
- The records line covers the whole directory: the `<unit>` filter and `--since` select blocks only.
- The CLI looks for the repository only for `report` and only in the CLI form, so `zygo work`, `zygo work <id>` and `/zygo:work report` start no git process for records (D-C2: the slash form runs inside the `UserPromptSubmit` hook and reads no record).
- Every `read_dir` result maps to a line or to no line; `report` adds no new error path ("The report never fails because of a record").

### Steps

- [ ] **1. Write the failing library tests.** Create `crates/zygo-work/tests/decision_report.rs`:

```rust
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
```

- [ ] **2. See the tests fail.** `cargo test -p zygo-work --locked --test decision_report` → does not compile: `report` takes 3 arguments, 4 given.

- [ ] **3. Implement the record lines** in `crates/zygo-work/src/report.rs`:
  - imports: `use crate::decisions::graph::deepest_chain; use crate::decisions::record::{name_kind, NameKind, Record, Status}; use crate::decisions::set::{read_dir, DirError, RecordSet};`
  - `fn under(u: &str, unit: &str) -> bool` — `u == unit || u.strip_prefix(unit).is_some_and(|r| r.starts_with(':'))`; replace the closure body at `report.rs:26` with `under(&j.row.unit, u)`.
  - `fn records_line(read: &Result<RecordSet, DirError>) -> Option<String>`: `Err(NotDirectory)` → `decision records: docs/decisions is not a directory`; `Err(TooMany)` → `decision records: over 10,000 files`; `Ok(set)` with no valid record and no malformed name of `NameKind::Record(_)` → `None`; else the D§9.5 line with `, <m> malformed` only when `m > 0`.
  - `report(db, records_in, unit, since)`: `let read = records_in.map(read_dir);` `let valid: &[Record] = match &read { Some(Ok(s)) => &s.valid, _ => &[] };` The blocks part keeps its current code (absent db → `no work units` without creating it; `render(&joined, valid)` when not empty). Then, when `read.as_ref().and_then(records_line)` is `Some(l)`, return `format!("{body}\n\n{l}")`, else `body`. Update the doc comment: `records_in` is the repository top; `None` gives no record lines.
  - `render(joined, records)`: after the `guard events:` `writeln!` (`report.rs:82`), count `records.iter().filter(|r| under(&r.unit, unit))` (total) and those with `r.status == Status::Open` (open); when total > 0, `writeln!(out, "  decisions: {open} open, {total} total")`.

- [ ] **4. Update the existing callers** of `report::report` to the new signature, `records_in = None`, so their outputs stay byte-equal: `crates/zygo-work/tests/collection.rs:1108`, `:1123`, `:1136`, `:1141`, `:1142` (`report::report(&db, None, None)` → `report::report(&db, None, None, None)`, and so on).

- [ ] **5. Write the failing CLI test** in `crates/zygo/src/cmd/work.rs` `mod tests` (`use super::*` brings `PathBuf` once step 6 imports it):

```rust
#[test]
fn records_only_for_the_cli_report() { // D§9.5, D-C2
    let top = || Some(PathBuf::from("/r"));
    assert_eq!(records_root(false, &v(&["report"]), top), Some(PathBuf::from("/r")));
    assert_eq!(records_root(false, &v(&["report", "beads:bd-a1b2"]), top), Some(PathBuf::from("/r")));
    assert_eq!(records_root(false, &v(&["report"]), || None), None);   // outside a repository, or git failed
    assert_eq!(records_root(true, &v(&["report"]), || unreachable!()), None);  // slash form: no lookup
    assert_eq!(records_root(false, &v(&["clear"]), || unreachable!()), None);
    assert_eq!(records_root(false, &[], || unreachable!()), None);
}
```

  and in `report_reads_the_store` add, after the first assertion:

```rust
    let top = t.path().join("repo");
    std::fs::create_dir_all(top.join("docs")).unwrap();
    std::fs::write(top.join("docs/decisions"), "x").unwrap();
    assert_eq!(run(t.path(), &cfg, &db, Some(&top), &v(&["report"]), &mut Vec::new()).unwrap(),
               "no work units\n\ndecision records: docs/decisions is not a directory");
```

  Run `cargo test -p zygo --locked work::tests` → does not compile: no `records_root`, `run` takes 5 arguments.

- [ ] **6. Implement the CLI side** in `crates/zygo/src/cmd/work.rs`:
  - `fn records_root(slash: bool, args: &[String], find: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf>` — `find()` only when `!slash` and `args.first()` is `report`; else `None`.
  - `fn run(cwd: &Path, cfg: &WorkConfig, db: &Path, records_in: Option<&Path>, args: &[String], err: &mut dyn Write) -> Result<String>`; the `Action::Report` arm calls `zygo_work::report::report(db, records_in, unit.as_deref(), since)`.
  - `verb`: `let records = records_root(ctx.slash, args, super::status::repo_top);` and pass `records.as_deref()` to `run`. The slash path reaches `verb` with `ctx.slash = true` (`crates/zygo/src/cmd/mod.rs:515-517`); the CLI with `false` (`mod.rs:601`).
  - Add `None` as the `records_in` argument at the existing `run` calls in `mod tests` (`work.rs:138`, `:142`, `:154`, `:164`, `:165`).
  - Import `PathBuf` beside `Path` (`work.rs:4`).

- [ ] **7. See the tests pass.** `cargo test -p zygo-work --locked --test decision_report` → 7 passed. `cargo test -p zygo-work --locked --test collection` → all pass (report outputs unchanged). `cargo test -p zygo --locked work::tests` → all pass (the timing test stays ignored).

- [ ] **8. Write the base-spec change (D§13 item 4).** In `docs/specs/2026-11-18-zygo-design.md` §13.6 (line 692), replace the last sentence ("`zygo work report [<unit>] [--since]` prints per unit: … verdicts.") with this final text:

> `zygo work report [<unit>] [--since]` prints per unit: rounds, tokens and estimated cost by role and model, check ran/passed, guard events, verdicts. The CLI form also counts the delegated decision records in `docs/decisions/` of the working tree of the repository at the current directory (`git rev-parse --show-toplevel`, 2 s bound; decision records design D§6.8, D§9.5). A unit block that has at least one valid record of that unit, or of that unit with a `:<part>`, gains `  decisions: <open> open, <total> total` after its `guard events:` line. After the last block, or after `no work units`, after one empty line, when the directory holds at least one record name: `decision records: <open> open, <total> total, deepest chain <d>`, then `, <m> malformed` when `m > 0`; a `docs/decisions` that is a link or not a directory prints `decision records: docs/decisions is not a directory`, and more than 10,000 entries print `decision records: over 10,000 files`. Outside a repository, when the git call fails, and in the slash form (which reads no record, D-C2), there are no record lines. A record never makes the report fail.

  Append to the base spec's `## Change log` table:

  `| 2026-11-25 | §13.6 | decision records design D§13 item 4 (approved 2026-11-24; node DR-7) | `zygo work report` prints the `decisions:` line in each unit block and the `decision records:` line after the units (D§9.5); the slash form reads no record (D-C2). |`

  Check: `grep -n 'decision records: over 10,000 files' docs/specs/2026-11-18-zygo-design.md` → one hit in §13.6; `tail -1` of the file is the new row.

- [ ] **9. Verify.** `cargo test -p zygo-work --locked --test decision_report` → all pass; `cargo test -p zygo-work --locked --test collection` → all pass; `cargo build --workspace --locked` → builds with no warning from the changed files; `grep -rn 'report::report(' crates` → every call has 4 arguments.

- [ ] **10. Commit.** Stage `crates/zygo-work/src/report.rs`, `crates/zygo-work/tests/decision_report.rs`, `crates/zygo-work/tests/collection.rs`, `crates/zygo/src/cmd/work.rs`, `docs/specs/2026-11-18-zygo-design.md`; message `DR-7: record lines in zygo work report` with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:DR-7`, each on its own line. `git show --stat HEAD` → those 5 files only.

Author's checks:
- Spec coverage: D§9.5 bullet 1 (step 1 `fixture_repository_gives_both_lines`, `part_unit_counts_for_its_parent_block`), bullet 2 (`fixture_repository_gives_both_lines`, `record_names_decide_the_records_line`), the two D§6.8 directory lines (`regular_file_is_not_a_directory`, `over_ten_thousand_entries`), bullet 3 (`no_units_then_the_records_line`), bullet 4 (`no_repository_no_record_lines`, `records_only_for_the_cli_report`), D§14 absent directory (`record_names_decide_the_records_line`), D-C2 slash form (`records_only_for_the_cli_report`), D§13 item 4 (step 8), D§17 #11 (step 1).
- Interface match: Consumes copied from `set.rs:15-35`, `graph.rs:123`, `record.rs:43-67,121-130`, `status.rs:21`; Produces equals the node contract.
- Live code: every cited path and line was opened at `origin/phase-3` 46072866. RT-2 (merge c9e8c096) did not touch `report.rs` or `work.rs`; it changed `status.rs`, where `repo_top` stays at lines 21-24.

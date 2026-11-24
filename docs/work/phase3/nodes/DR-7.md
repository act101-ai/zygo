# DR-7 — `zygo work report`: the two record lines

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§9.5, D§6.8 (the two directory lines of the report), D§14 (column `zygo work report`), D§13 item 4, D-C2
**Files:** `crates/zygo-work/src/report.rs`, `crates/zygo/src/cmd/work.rs`, `crates/zygo-work/tests/collection.rs` (the existing report::report calls), `crates/zygo-work/tests/decision_report.rs`, `docs/specs/2026-11-18-zygo-design.md` (§13.6, one change-log line)
**Consumes:** `read_dir` (DR-2), `RecordSet` (DR-2), `DirError` (DR-2), `deepest_chain` (DR-2), `Record` (DR-1), `Status` (DR-1)
**Produces:** `zygo_work::report::report(db: &Path, records_in: Option<&Path>, unit: Option<&str>, since: Option<i64>) -> anyhow::Result<String>` (`records_in = None`: no record lines; the slash form of `work report` passes `None`)
**Acceptance:** `cargo test -p zygo-work --locked --test decision_report` → all pass. Covers spec Acceptance #11 (a fixture repository gives `  decisions: 2 open, 3 total` in the unit block, after the `guard events:` line, and `decision records: 2 open, 3 total, deepest chain 2, 1 malformed` after the last block; outside a repository no records line); a `:<part>` unit counts for its parent block; no units and one record → `no work units` then the records line; `docs/decisions` a regular file → `decision records: docs/decisions is not a directory`; 10,001 entries → `decision records: over 10,000 files`; `records_in = None` → no records line.

## Plan

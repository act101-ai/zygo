# OS-4 — Trim hook writes `result_id` and `shaper`, opens the store bounded

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§7.9 (row fields; `shaper = "size"` for a §10.1 view), O§7.11, O§10 row "Usage store busy past `HOOK_DB_BOUND`, or broken (hook)", O§16 item 15 (hook part)
**Files:** `crates/zygo-hooks/src/trim.rs`, `crates/zygo-hooks/tests/hooks_basic.rs`, `crates/zygo/tests/hook_trim.rs`, `crates/zygo-hooks/Cargo.toml`, `Cargo.lock`, `crates/zygo-usage/src/store.rs`
**Consumes:** `Store::put` (OS-1), `Stored.session` (OS-1), `TrimRow.result_id` (OS-2), `TrimRow.shaper` (OS-2), `Store::insert_trim` (OS-2), `Store::open_bounded` (OS-3), `HOOK_DB_BOUND` (OS-3)
**Produces:** `TrimOpts { db_bound: Duration }` (`Default`: `HOOK_DB_BOUND`), `trim::trim_in_with(input: &HookInput, dir: &SessionDir, usage_db: &Path, opts: &TrimOpts) -> anyhow::Result<HookOutput>`; `trim_in` keeps its signature and calls `trim_in_with` with the default
**Acceptance:** `test "$(cargo test -q -p zygo-hooks --test hooks_basic -- trim_row --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-hooks --test hooks_basic && cargo test -p zygo --test hook_trim` → every count holds (at least 3 named `trim_row…` in zygo-hooks `hooks_basic`; a count of 0 fails before any run), then all pass, including new tests: a trimmed Bash call writes a row whose `result_id` is the id in the marker, whose `shaper` is `size` and whose `command` is null; with a second connection holding the write lock and `db_bound = 0`, the hook returns the same view as with no lock and writes no row; on a database built from the `97d8767` schema the hook writes a seven-column row and adds no column

## Plan

### Scope defect (blocks execution until the lead rules)

The plan needs three files that are not in **Files**. Every one of them is forced by the spec or by **Acceptance**:

1. `crates/zygo/tests/hook_trim.rs`. `Store::open_bounded` creates no database file (`crates/zygo-usage/src/store.rs:191-194`; test `open_bounded_on_missing_path_is_an_error_and_creates_nothing`, `crates/zygo-usage/tests/store.rs:873-880`). The hook therefore no longer creates `usage.db` (O§6.5 item 3: the launcher and the CLI create it). Evidence: with only the open call in `trim.rs` changed to `open_bounded(usage_db, HOOK_DB_BOUND)` at 882a7bdd, `cargo test -p zygo --test hook_trim` fails 2 of 6:
   - `trim_binary_trims_bash_over_threshold` (`hook_trim.rs:35`, stderr `"zygo hook: usage store unavailable\n"` is not `""`; `:40` also asserts that `usage.db` is a file);
   - `trim_binary_work_log_failure_keeps_view_with_one_fixed_line` (`hook_trim.rs:70`, stderr has the usage line as well).
   
   **Acceptance** requires `hook_trim` to stay green, so the test must create the database first, as the launcher does.
2. `crates/zygo-hooks/Cargo.toml` and `Cargo.lock`. The new tests must hold a write lock in a second connection (O§8.4) and read `result_id`, `shaper` and the column list. No `zygo_usage` API exposes these (`crates/zygo-usage/src/query.rs` has no `trims` reader that returns them). `zygo-hooks` has no `rusqlite` dev-dependency (`crates/zygo-hooks/Cargo.toml:28-30`). CI runs `cargo test --workspace --locked` (`.github/workflows/ci.yml:45`), so `Cargo.lock` must be committed with the change.

Ruling needed: add `crates/zygo/tests/hook_trim.rs`, `crates/zygo-hooks/Cargo.toml` and `Cargo.lock` to **Files**. No spec-conformant plan keeps `hook_trim` green without editing it.

The same experiment also breaks 2 tests in `hooks_basic` (`trim_records_bytes_trimmed`, `trim_subagent_output_is_trimmed`), which read `bytes_trimmed` from a database that the hook used to create. These fixes are in **Files** (Step 2).

### Premise check (live code at 882a7bdd)

| Consumes | Location | Signature now |
|---|---|---|
| `Store::put` (OS-1) | `crates/zygo-results/src/store.rs:60` | `pub fn put(dir: &Path, tool: &str, session: Option<&str>, bytes: &[u8]) -> anyhow::Result<Stored>` |
| `Stored.session` (OS-1) | `crates/zygo-results/src/store.rs:53` | `pub session: Option<String>` |
| `TrimRow.result_id` (OS-2) | `crates/zygo-usage/src/schema.rs:145` | `pub result_id: Option<String>` (`#[serde(default)]`) |
| `TrimRow.shaper` (OS-2) | `crates/zygo-usage/src/schema.rs:151` | `pub shaper: Option<String>` (`#[serde(default)]`) |
| `Store::insert_trim` (OS-2) | `crates/zygo-usage/src/store.rs:310` | `pub fn insert_trim(&self, t: &TrimRow) -> anyhow::Result<()>`: ten columns; falls back to the seven old columns on "table trims has no column named" or "no such column" (`:317`) |
| `Store::open_bounded` (OS-3) | `crates/zygo-usage/src/store.rs:195` | `pub fn open_bounded(path: &Path, bound: Duration) -> anyhow::Result<Store>`: `READ_WRITE` without `CREATE`, `busy_timeout(bound)`, no retry, no schema or column change, error unless `user_version` = 1 |
| `HOOK_DB_BOUND` (OS-3) | `crates/zygo-usage/src/store.rs:69` | `pub const HOOK_DB_BOUND: Duration = Duration::from_millis(100);`, reached as `zygo_usage::store::HOOK_DB_BOUND` (`crates/zygo-usage/src/lib.rs:5`, `pub mod store`) |

All seven are present with the signatures that the node expects. There is no premise drift.

### Produces (exact)

- `crates/zygo-hooks/src/trim.rs`: `#[derive(Debug, Clone, Copy)] pub struct TrimOpts { pub db_bound: std::time::Duration }`, with `impl Default for TrimOpts` giving `TrimOpts { db_bound: zygo_usage::store::HOOK_DB_BOUND }`.
- `pub fn trim_in_with(input: &HookInput, dir: &SessionDir, usage_db: &Path, opts: &TrimOpts) -> anyhow::Result<HookOutput>`.
- `pub fn trim_in(input: &HookInput, dir: &SessionDir, usage_db: &Path) -> anyhow::Result<HookOutput>` is unchanged. Its body is `trim_in_with(input, dir, usage_db, &TrimOpts::default())`.

Other callers of `trim_in` keep their signature: `crates/zygo-launch/tests/opencode.rs:137,160` (these assert the view only, not a row) and `run` (`trim.rs:25`).

### Existing tests this node must keep green

- `cargo test -p zygo --test hook_trim` (`crates/zygo/tests/hook_trim.rs`, 6 tests). It runs the real `zygo hook trim` binary with `HOME` in a temporary directory and covers these cases:
  - a trimmed Bash call (view, 2 result files, `usage.db` exists, empty stderr);
  - small output passes;
  - store failure prints one fixed line;
  - usage failure (the data directory is a file) keeps the view and prints `zygo hook: usage store unavailable`;
  - work-log failure prints one fixed line;
  - `hook token`.
  
  Step 3 changes two of these tests to create the database first. The usage-failure test stays as it is, because `open_bounded` still fails on that path.
- `cargo test -p zygo-hooks --test hooks_basic`: 55 tests now. `trim_usage_store_failure_keeps_view` (`:339`, the database path is a directory) stays as it is.

### Steps

- [x] **Step 1: Add `rusqlite` as a dev-dependency of `zygo-hooks`.** This step depends on the ruling above. In `crates/zygo-hooks/Cargo.toml` under `[dev-dependencies]`, add `rusqlite.workspace = true` (workspace entry `Cargo.toml:57`, `0.43.0`, `bundled`). Run `cargo build -p zygo-hooks --tests` and check that `Cargo.lock` gains `"rusqlite"` in the `zygo-hooks` dependency list and nothing else.

- [x] **Step 2: Write the failing row tests and make `hooks_basic` create the database first.** All edits are in `crates/zygo-hooks/tests/hooks_basic.rs`.

  New helpers after `dir_names` (`:114`). No existing helper does this: the only id extraction is inline in `crates/zygo/tests/workspace_restore.rs:763`, in another crate.

  ```rust
  /// The usage database as the launcher leaves it (O§6.5 item 3: the hook's bounded open creates nothing).
  fn usage_ready(e: &Env) { drop(Usage::open(&db(e)).unwrap()); }
  /// The result id in a §10.1 trim marker.
  fn marker_id(view: &str) -> String { view.split("saved as result ").nth(1).unwrap().split('.').next().unwrap().to_string() }
  type RowCols = (String, String, i64, i64, Option<String>, Option<String>, Option<String>);
  fn trim_rows(e: &Env) -> Vec<RowCols> {
      let c = rusqlite::Connection::open(db(e)).unwrap();
      let mut q = c.prepare("SELECT session, tool, stored_bytes, view_bytes, result_id, command, shaper FROM trims ORDER BY rowid").unwrap();
      q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))).unwrap().map(Result::unwrap).collect()
  }
  fn trim_cols(e: &Env) -> Vec<String> {
      let c = rusqlite::Connection::open(db(e)).unwrap();
      let mut q = c.prepare("PRAGMA table_xinfo(trims)").unwrap();
      q.query_map([], |r| r.get(1)).unwrap().map(Result::unwrap).collect()
  }
  /// The `trims` table and its indexes as at `97d8767`: `git show 97d8767:crates/zygo-usage/src/schema.rs`, lines 35-43 and 63-64,
  /// copied byte for byte. The hook touches no other table. The full schema copy is `SCHEMA_97D8767` in `crates/zygo-usage/tests/store.rs:37`.
  const TRIMS_97D8767: &str = "CREATE TABLE IF NOT EXISTS trims (
      ts INTEGER NOT NULL,
      session TEXT NOT NULL,
      agent_id TEXT,
      tool TEXT NOT NULL,
      kind TEXT NOT NULL,
      stored_bytes INTEGER NOT NULL,
      view_bytes INTEGER NOT NULL
  );
  CREATE INDEX IF NOT EXISTS trims_session ON trims(session);
  CREATE INDEX IF NOT EXISTS trims_ts ON trims(ts);";
  const TRIM_SEVEN: [&str; 7] = ["ts", "session", "agent_id", "tool", "kind", "stored_bytes", "view_bytes"];
  ```

  Edit two existing tests: add `usage_ready(&e);` as the second line of `trim_records_bytes_trimmed` (`:134`) and of `trim_subagent_output_is_trimmed` (`:261`).

  Add the new tests after `trim_usage_store_failure_keeps_view` (`:339`):

  ```rust
  #[test] fn trim_row_bash_records_result_id_and_size_shaper() {      // O§7.9: result_id = stored id, shaper "size", command null
      let e = env(true); usage_ready(&e);
      let o = updated(&out(run_trim(&bash(&lines(5000)), &e)));
      let view = o["stdout"].as_str().unwrap();
      let id = marker_id(view);
      assert_eq!(result_ids(&e), [id.clone()]);
      assert_eq!(trim_rows(&e), [("sess-1".to_string(), "Bash".to_string(), 105000, view.len() as i64, Some(id), None, Some("size".to_string()))]);
  }
  #[test] fn trim_row_on_97d8767_database_writes_seven_columns() {    // O§6.5 items 3-4, O§16 item 15
      let e = env(true);
      std::fs::create_dir_all(db(&e).parent().unwrap()).unwrap();
      let c = rusqlite::Connection::open(db(&e)).unwrap();
      c.execute_batch(TRIMS_97D8767).unwrap();
      c.execute_batch("PRAGMA user_version = 1").unwrap();
      drop(c);
      let o = updated(&out(run_trim(&bash(&lines(5000)), &e)));
      assert!(o["stdout"].as_str().unwrap().contains("[zygo trimmed this output: 5000 lines, 105000 bytes;"));
      assert_eq!(trim_cols(&e), TRIM_SEVEN);                             // no column added
      let c = rusqlite::Connection::open(db(&e)).unwrap();
      let row: (String, String, i64, i64) = c.query_row("SELECT session, tool, stored_bytes, view_bytes FROM trims", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
      assert_eq!(row, ("sess-1".into(), "Bash".into(), 105000, o["stdout"].as_str().unwrap().len() as i64));
  }
  #[test] fn trim_row_missing_database_keeps_view_and_creates_nothing() {   // O§7.11, O§10 "broken (hook)"
      let e = env(true);
      let o = updated(&out(run_trim(&bash(&lines(5000)), &e)));
      assert!(o["stdout"].as_str().unwrap().contains("[zygo trimmed this output:"));
      assert_eq!(result_ids(&e).len(), 1);
      assert!(!db(&e).parent().unwrap().exists());
  }
  ```

- [x] **Step 3: Make `hook_trim` create the database first.** This step depends on the ruling above. In `crates/zygo/tests/hook_trim.rs`, add after `data_dir` (`:29`):

  ```rust
  /// The usage database as the launcher leaves it (O§6.5 item 3).
  fn usage_ready(d: &Path) { drop(zygo_usage::Store::open(&data_dir(d).join("usage.db")).unwrap()); }
  ```

  Call `usage_ready(d.path());` after `let sd = session(d.path());` in `trim_binary_trims_bash_over_threshold` (`:31`) and in `trim_binary_work_log_failure_keeps_view_with_one_fixed_line` (`:65`). `zygo-usage` is a normal dependency of `zygo` (`crates/zygo/Cargo.toml:15`). Keep every assertion.

- [x] **Step 4: Run the tests and see them fail.** Run `cargo test -p zygo-hooks --test hooks_basic -- trim_row`. Expect 3 failures:
  - `trim_row_bash_records_result_id_and_size_shaper`: `result_id` and `shaper` are `None`;
  - `trim_row_on_97d8767_database_writes_seven_columns`: the full `Store::open` adds `result_id`, `command` and `shaper`, so there are 10 columns;
  - `trim_row_missing_database_keeps_view_and_creates_nothing`: the full open creates `usage/`.
  
  Then run `cargo test -p zygo-hooks --test hooks_basic` and `cargo test -p zygo --test hook_trim`. Everything else passes, including the four tests edited in Steps 2-3.

- [x] **Step 5: Implement the row fields and the bounded open in `crates/zygo-hooks/src/trim.rs`, with the bound still fixed.**
  1. In the `TrimRow` literal (`:118-129`), set `result_id: Some(stored.id.clone())` and `shaper: Some("size".into())`. Keep `command: None`: the O§7.2 command name comes with the shaping pipeline in a later node.
  2. Replace `zygo_usage::Store::open(usage_db)` (`:131`) with `zygo_usage::Store::open_bounded(usage_db, zygo_usage::store::HOOK_DB_BOUND)`.
  3. Change the comment above it to `// C6, O§7.11: busy past the bound or broken: no row, the view stands.`
  4. Keep `note("usage store unavailable")` unchanged (O§10: one stderr line).

- [x] **Step 6: Run the tests and see them pass.** Run `cargo test -p zygo-hooks --test hooks_basic` and `cargo test -p zygo --test hook_trim`. All pass, including the 3 `trim_row…` tests.

- [x] **Step 7: Commit.**
  ```sh
  git add crates/zygo-hooks/src/trim.rs crates/zygo-hooks/tests/hooks_basic.rs crates/zygo/tests/hook_trim.rs crates/zygo-hooks/Cargo.toml Cargo.lock
  git commit -F - <<'MSG'
  OS-4: trim row result_id and shaper, bounded usage open

  Zygo-Session: 01M895N7W11GKNMZ31G5G3CAZZ
  Zygo-Work: phase3:OS-4
  MSG
  ```

- [x] **Step 8: Write the failing lock test.** In `crates/zygo-hooks/tests/hooks_basic.rs`, add `use std::time::Duration;` to the imports. Add this test after the other `trim_row…` tests:

  ```rust
  #[test] fn trim_row_not_written_under_write_lock_with_bound_zero() {   // O§7.11, O§10, O§16 item 15; O§8.4: no clock
      let e = env(true); usage_ready(&e);
      let input = parse(&bytes(&bash(&lines(5000)))).unwrap();
      let lock = rusqlite::Connection::open(db(&e)).unwrap();
      lock.execute_batch("BEGIN IMMEDIATE").unwrap();                     // a second connection holds the write lock
      let locked = out(trim::trim_in_with(&input, &e.dir, &db(&e), &trim::TrimOpts { db_bound: Duration::ZERO }));
      lock.execute_batch("ROLLBACK").unwrap();
      drop(lock);
      assert!(trim_rows(&e).is_empty());                                 // no row
      let free = out(trim::trim_in(&input, &e.dir, &db(&e)));
      let id_of = |s: &str| marker_id(updated(s)["stdout"].as_str().unwrap());
      let (a, b) = (id_of(&locked), id_of(&free));
      assert_ne!(a, b);
      assert_eq!(locked.replace(&a, &b), free);                          // same hook output as with no lock, up to the result id
      assert_eq!(trim_rows(&e).len(), 1);
      assert_eq!(trim_rows(&e)[0].4.as_deref(), Some(b.as_str()));
      assert_eq!(result_ids(&e).len(), 2);                               // both results stored
  }
  ```

  The decision does not depend on a wall-clock wait. With `busy_timeout(0)`, SQLite returns `SQLITE_BUSY` at once on the write. The reads of `open_bounded` (`user_version`) do not wait on a WAL writer.

- [x] **Step 9: Run the lock test and see it fail.** Run `cargo test -p zygo-hooks --test hooks_basic -- trim_row_not_written`. It fails to compile: `cannot find function trim_in_with in module trim` and `cannot find struct TrimOpts in module trim`.

- [x] **Step 10: Add `TrimOpts` and `trim_in_with` in `crates/zygo-hooks/src/trim.rs`.**
  1. Add `use std::time::Duration;`.
  2. Add `TrimOpts` and its `Default`, exactly as in **Produces**.
  3. Rename the body of `trim_in` to `pub fn trim_in_with(input: &HookInput, dir: &SessionDir, usage_db: &Path, opts: &TrimOpts) -> anyhow::Result<HookOutput>`, with the doc line of `trim_in`. In it, replace `zygo_usage::store::HOOK_DB_BOUND` with `opts.db_bound`.
  4. Add a new `pub fn trim_in(input: &HookInput, dir: &SessionDir, usage_db: &Path) -> anyhow::Result<HookOutput>` with the body `trim_in_with(input, dir, usage_db, &TrimOpts::default())`.

- [x] **Step 11: Run the lock test and see it pass.** Run `cargo test -p zygo-hooks --test hooks_basic -- trim_row_not_written`. It passes. Then run it 20 times as a stability check with no timing: `for i in $(seq 20); do cargo test -q -p zygo-hooks --test hooks_basic -- trim_row_not_written || break; done`. Every run passes.

- [x] **Step 12: Run the acceptance check and the lint.**
  ```sh
  test "$(cargo test -q -p zygo-hooks --test hooks_basic -- trim_row --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-hooks --test hooks_basic && cargo test -p zygo --test hook_trim
  ```
  The count is 4, then all pass. Also run `cargo test -p zygo-launch --test opencode` (it calls `trim_in` and asserts the view only) and `cargo clippy -p zygo-hooks -p zygo --tests -- -D warnings`. Both are clean.

- [x] **Step 13: Commit.**
  ```sh
  git add crates/zygo-hooks/src/trim.rs crates/zygo-hooks/tests/hooks_basic.rs
  git commit -F - <<'MSG'
  OS-4: TrimOpts bound and the write-lock test

  Zygo-Session: 01M895N7W11GKNMZ31G5G3CAZZ
  Zygo-Work: phase3:OS-4
  MSG
  ```


### Author's checks

- **Spec coverage.**
  - O§7.9: `result_id` = the stored id and `shaper = "size"` (Step 2, test 1; Step 5). `command` is null: Acceptance; the O§7.2 name belongs to the pipeline node.
  - O§7.11: bounded open, a bound parameter, no `ALTER` (Steps 5, 8 and 10; the 97d8767 test).
  - O§10 row "busy past `HOOK_DB_BOUND`, or broken": view kept, no row (lock test, missing-database test, existing `trim_usage_store_failure_keeps_view`). The one stderr line is asserted in-process by none, and in the binary by `trim_binary_usage_failure_keeps_view_with_one_fixed_line`, whose text is unchanged.
  - O§16 item 15, hook part: lock with bound 0, and the seven-column write on the 97d8767 schema (both tests).
- **Step scan.** Each step has one action and a checkable result. There are no placeholders.
- **Interface match.** `TrimOpts` and `trim_in_with` match **Produces** character for character. `trim_in` is unchanged.
- **Proportion.** One code change (about 15 lines in `trim.rs`), 4 new tests, 4 one-line test fixes. The 97d8767 copy holds only the `trims` DDL that the hook touches.
- **Live code.** All line numbers are at 882a7bdd. The scope defect was found by running the bounded open against `hook_trim` and `hooks_basic`, then reverting.
- **Not in scope.** The doc of `Store::open` (`crates/zygo-usage/src/store.rs:111-113`, "until then they call this open") becomes stale for the hook. It is in `zygo-usage`, outside **Files**. Leave it for OS-5 or the lead.

### Fix round 1

- F2, fixed: only the Bash row's `result_id` and `shaper` were asserted, but O§7.9 sets `shaper = "size"` for every trim. New test `trim_row_read_records_result_id_and_size_shaper` trims a Read output and asserts the row's `result_id` (the stored id) and `shaper = "size"`. It failed with a temporary non-Bash shaper of `"mutant"` (not committed) and passes on the committed code.
- F3, fixed: the doc of `Store::open` (`crates/zygo-usage/src/store.rs`) named an MCP read-back write that does not exist in `crates/zygo-mcp/src`. It now says only that `Store::open` is the full open (it creates and migrates) and that the trim hook uses `Store::open_bounded`.
- F1, dropped: a test that proves `opts.db_bound` reaches `open_bounded` must release the lock during the wait. That is a wall-clock ordering, and timing tests do not run in CI.
- F4, dropped: the node file is in scope by rule.

# OS-3 — Bounded usage-store open for the hook and the MCP server

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§7.11 "Bound", O-C10, O§6.5 item 3 (no `ALTER` in a bounded open), O§8.4 (busy store with the bound passed as 0)
**Files:** `crates/zygo-usage/src/store.rs`, `crates/zygo-usage/tests/store.rs`
**Consumes:** `Store::insert_trim` (OS-2), `Store::open_with_column_check` (OS-2)
**Produces:** `HOOK_DB_BOUND` (const), `HOOK_DB_BOUND: Duration = Duration::from_millis(100)`, `Store::open_bounded(path: &Path, bound: Duration) -> anyhow::Result<Store>` (busy timeout = `bound` for each statement, no retry loop, no `PRAGMA table_info`, no `ALTER`, creates no directory or file)
**Acceptance:** `test "$(cargo test -q -p zygo-usage --test store -- open_bounded --list 2>/dev/null | grep -c ': test$')" -ge 4 && cargo test -p zygo-usage --test store` → every count holds (at least 4 named `open_bounded…` in zygo-usage `store`; a count of 0 fails before any run), then all pass, including new tests: `open_bounded(.., Duration::ZERO)` gives `busy_timeout_ms() == 0` and `open_bounded(.., HOOK_DB_BOUND)` gives 100; with a second connection holding `BEGIN IMMEDIATE`, `insert_trim` through `open_bounded(.., Duration::ZERO)` returns a busy error (no clock read in any assertion); `open_bounded` on a seven-column database leaves `PRAGMA table_info(trims)` at seven columns and its `insert_trim` writes a row; `open_bounded` on a missing path is an error and creates no file

## Plan

### Premises checked against the live code

Branch `phase3/OS-3` at `1df2f158`, which is `origin/phase-3` with OS-2 merged (PR #49).

- `crates/zygo-usage/src/store.rs:1-7` imports `std::time::{Duration, Instant}`, `anyhow::{anyhow, Result}` and `rusqlite::{…, Connection, ErrorCode, OpenFlags, …}`; `Result` in `store.rs` is `anyhow::Result`. `Store` is `store.rs:11-13` (`pub(crate) conn: Connection`).
- `SCHEMA_VERSION` (= 1) is `store.rs:65`. `is_busy` (`store.rs:99-104`) matches `DatabaseBusy | DatabaseLocked`.
- The full open: `Store::open` (`store.rs:110`), `Store::open_with_column_check` (`store.rs:117`, `#[doc(hidden)]`), both through `open_full` (`store.rs:121-167`): `create_dir_all` of the parent, busy timeout 5 s, a 5 s retry loop with `sleep(20 ms)` around `journal_mode = WAL`, `user_version`, `execute_batch(SCHEMA)`, `add_trim_columns` (`PRAGMA table_xinfo` + `ALTER`), then `synchronous = NORMAL`.
- `Store::open_read_only` (`store.rs:172-185`) is the model for a no-create open: `Connection::open_with_flags(path, SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX)`, `busy_timeout(Duration::ZERO)`, rejects `user_version != SCHEMA_VERSION` with `"usage database schema version {version} is not {SCHEMA_VERSION}"`. It cannot write, so it is not the bounded open.
- `Store::busy_timeout_ms(&self) -> Result<i64>` (`store.rs:188-192`, `#[doc(hidden)]`) reads `PRAGMA busy_timeout`.
- `Store::insert_trim(&self, t: &TrimRow) -> Result<()>` (`store.rs:286-299`): ten columns through `prepare_cached(INSERT_TRIM)`; when the prepare fails with `table trims has no column named …` or `no such column`, the seven-column insert. It runs no `PRAGMA`. So the bounded open needs no column knowledge: the fallback is in the insert.
- `Store::insert_readback(&self, r: &ReadbackRow) -> Result<()>` (`store.rs:302-307`); OS-5 calls it through the bounded open.
- No `open_bounded` or `HOOK_DB_BOUND` exists in `crates/` (`grep -rn` prints nothing). The Acceptance count is 0 today.
- `crates/zygo-usage/src/lib.rs` has `pub mod store;` and `pub use store::Store;`; tests reach a new const as `store::HOOK_DB_BOUND` through `use zygo_usage::*;` (as `store::SCHEMA_VERSION`, `tests/store.rs:694`). `lib.rs` does not change.
- Test helpers in `crates/zygo-usage/tests/store.rs`: `Tmp` (`:7-16`, `Tmp::new()` creates its directory, `db()` = `<dir>/usage.db`), `cols(db, table)` (`:17-21`, `PRAGMA table_info` names), `count(db, table)` (`:22-24`), `trim(session, ts, stored, view)` (`:29-31`), `old_db(db)` (`:115-120`, the `97d8767` schema, `user_version` 1, one seven-column row, rollback journal), `TRIM_SEVEN` (`:111`). Existing model test: `open_sets_a_five_second_busy_timeout` (`:423-427`).
- SQLite behaviour, checked on this machine with a scratch database: with a second connection holding `BEGIN IMMEDIATE` on a WAL database, a third connection opened read-write with busy timeout 0 reads `PRAGMA user_version`, sets `PRAGMA synchronous`, and gets `SQLITE_BUSY` ("database is locked") on `INSERT`. A read-write open without the create flag on a missing path fails with "unable to open database file" and creates no file. rusqlite 0.43 (`Cargo.toml:57`, bundled) reports `SQLITE_BUSY` as `ErrorCode::DatabaseBusy`.

### Decisions

- The bounded open is `Connection::open_with_flags(path, SQLITE_OPEN_READ_WRITE | SQLITE_OPEN_NO_MUTEX)` (no `SQLITE_OPEN_CREATE`, no `create_dir_all`), then `busy_timeout(bound)`, then a `user_version` read that rejects any value other than `SCHEMA_VERSION` (as `open_read_only`), then `synchronous = NORMAL` (as `open_full`). Nothing else.
- It runs no statement that takes the write lock: no `journal_mode` change, no `execute_batch(SCHEMA)`, no `user_version` write, no `PRAGMA table_info`/`table_xinfo`, no `ALTER` (O§6.5 item 3, O§7.11). The full open of the launcher at launch makes the file, the schema and the columns. So the open itself never waits on a busy database; only each later statement waits, up to `bound`, through SQLite's busy handler.
- No retry loop and no clock read (O§7.11 "no retry loop"): `open_bounded` calls neither `Instant::now` nor `sleep`. The 5 s loop of `open_full` is not reused.
- A database that a `97d8767` binary made has no `readbacks` table; `insert_readback` through the bounded open then fails, and OS-5 writes no row (O§6.4, O-C1). The bounded open does not create the table.
- The const lives in `store.rs` and is reached as `zygo_usage::store::HOOK_DB_BOUND`; OS-4 and OS-5 import it from there.

### Review focus

Item 21 (graph `## Review focus`): "The hook adding a column (write lock on every Bash call); the seven-column fallback hiding an error other than 'no such column'; a bounded open that waits longer than `bound` — test in OS-2, OS-3". OS-3's part, each proved without a clock:

- a bounded open that waits longer than `bound`: the busy timeout equals `bound` (`open_bounded_sets_busy_timeout_to_the_bound`: 0 and 100), and with the write lock held and `bound` 0 the insert returns `SQLITE_BUSY` (`open_bounded_insert_is_busy_at_bound_zero_under_a_held_write_lock`); the open takes no write lock, so it cannot wait at all (`open_bounded_opens_under_a_held_write_lock`);
- the hook adding a column: `open_bounded_on_seven_column_database_adds_no_column` (seven columns and `PRAGMA schema_version` unchanged after the open and an insert);
- the fallback hiding another error: the busy error of the insert reaches the caller as `ErrorCode::DatabaseBusy`, not a seven-column write (same busy test: zero rows after the lock is released).

### Interfaces

Consumes (current signatures, `crates/zygo-usage/src/store.rs`):

- `pub fn insert_trim(&self, t: &TrimRow) -> Result<()>` (`store.rs:286`), `Result` = `anyhow::Result`.
- `#[doc(hidden)] pub fn open_with_column_check(path: &Path, report_missing: &[&str]) -> Result<Store>` (`store.rs:117`). This node does not call it; the full-open tests of OS-2 that use it stay unchanged, and `open_bounded` shares no code with `open_full`.
- Also used by the tests: `pub fn open(path: &Path) -> Result<Store>` (`store.rs:110`), `#[doc(hidden)] pub fn busy_timeout_ms(&self) -> Result<i64>` (`store.rs:188`).

Produces:

- `pub const HOOK_DB_BOUND: Duration = Duration::from_millis(100);` in `crates/zygo-usage/src/store.rs` (O-C10, O§7.11).
- `pub fn open_bounded(path: &Path, bound: Duration) -> anyhow::Result<Store>` on `impl Store` in `crates/zygo-usage/src/store.rs`: busy timeout = `bound` for each statement, no retry loop, no `PRAGMA table_info`, no `ALTER`, creates no directory or file; an error for a missing path or a `user_version` other than `SCHEMA_VERSION`.

### Steps

- [ ] 1. Write the failing tests at the end of `crates/zygo-usage/tests/store.rs`, under a comment line `// ---- OS-3: bounded open (O§7.11, O-C10) ----`. Add `use std::time::Duration;` at the top (extend the `std` import at `:1` with `time::{Duration, SystemTime, UNIX_EPOCH}`). Each name starts with `open_bounded_`.

  ```rust
  #[test]
  fn open_bounded_sets_busy_timeout_to_the_bound() {
      let t = Tmp::new();
      drop(Store::open(&t.db()).unwrap());
      assert_eq!(store::HOOK_DB_BOUND, Duration::from_millis(100));
      assert_eq!(Store::open_bounded(&t.db(), Duration::ZERO).unwrap().busy_timeout_ms().unwrap(), 0);
      assert_eq!(Store::open_bounded(&t.db(), store::HOOK_DB_BOUND).unwrap().busy_timeout_ms().unwrap(), 100);
  }

  #[test]
  fn open_bounded_opens_under_a_held_write_lock() {
      let t = Tmp::new();
      drop(Store::open(&t.db()).unwrap());
      let raw = Connection::open(t.db()).unwrap();
      raw.execute_batch("BEGIN IMMEDIATE").unwrap(); // another process holds the write lock
      // The open takes no write lock, so with bound 0 it still succeeds.
      assert!(Store::open_bounded(&t.db(), Duration::ZERO).is_ok());
      raw.execute_batch("ROLLBACK").unwrap();
  }

  #[test]
  fn open_bounded_insert_is_busy_at_bound_zero_under_a_held_write_lock() {
      let t = Tmp::new();
      drop(Store::open(&t.db()).unwrap());
      let s = Store::open_bounded(&t.db(), Duration::ZERO).unwrap();
      let raw = Connection::open(t.db()).unwrap();
      raw.execute_batch("BEGIN IMMEDIATE").unwrap();
      let e = s.insert_trim(&trim("S1", 5, 1000, 100)).err().expect("a held write lock with bound 0 must fail the insert");
      let code = e.downcast_ref::<rusqlite::Error>().and_then(|e| e.sqlite_error_code());
      assert_eq!(code, Some(rusqlite::ErrorCode::DatabaseBusy), "{e}");
      raw.execute_batch("ROLLBACK").unwrap();
      assert_eq!(count(&t.db(), "trims"), 0); // no row, also not through the seven-column fallback
  }

  #[test]
  fn open_bounded_on_seven_column_database_adds_no_column() {
      let t = Tmp::new();
      old_db(&t.db());
      let before = pragma(&t.db(), "schema_version");
      let s = Store::open_bounded(&t.db(), store::HOOK_DB_BOUND).unwrap();
      s.insert_trim(&trim("S2", 6, 2000, 200)).unwrap();
      drop(s);
      assert_eq!(cols(&t.db(), "trims"), TRIM_SEVEN);
      assert_eq!(pragma(&t.db(), "schema_version"), before);
      assert_eq!(count(&t.db(), "trims"), 2);
  }

  #[test]
  fn open_bounded_on_missing_path_is_an_error_and_creates_nothing() {
      let t = Tmp::new();
      assert!(Store::open_bounded(&t.db(), store::HOOK_DB_BOUND).is_err());
      assert!(!t.db().exists());
      let deep = t.0.join("no").join("such").join("usage.db");
      assert!(Store::open_bounded(&deep, store::HOOK_DB_BOUND).is_err());
      assert!(!t.0.join("no").exists());
  }

  #[test]
  fn open_bounded_rejects_another_schema_version() {
      let t = Tmp::new();
      drop(Store::open(&t.db()).unwrap());
      Connection::open(t.db()).unwrap().execute_batch("PRAGMA user_version = 2").unwrap();
      let e = Store::open_bounded(&t.db(), store::HOOK_DB_BOUND).err().expect("another schema version must fail").to_string();
      assert!(e.contains('2') && e.contains('1'), "{e}");
  }
  ```

  The helper `pragma(db, name)` exists at `tests/store.rs:121-123`. No test reads a clock or sleeps (Global constraints: no wall-clock test in CI; O§8.4).

- [ ] 2. Run `cargo test -p zygo-usage --test store -- open_bounded` → it fails to compile with "no function or associated item named `open_bounded`" and "cannot find value `HOOK_DB_BOUND` in module `store`".

- [ ] 3. In `crates/zygo-usage/src/store.rs`, after `SCHEMA_VERSION` (`:65`), add

  ```rust
  /// The longest wait of each usage-store statement in the trim hook and the MCP server's read-back write
  /// (O§7.11, O-C10).
  pub const HOOK_DB_BOUND: Duration = Duration::from_millis(100);
  ```

  and in `impl Store`, after `open_read_only` (`:172-185`), add

  ```rust
  /// The bounded open of the trim hook and the MCP server's read-back write (O§7.11): busy timeout `bound` for each
  /// statement and no retry loop; no directory, file, schema or column is created (O§6.5 item 3), so the open takes no
  /// write lock. A database of another schema version is an error.
  pub fn open_bounded(path: &Path, bound: Duration) -> Result<Store>
  ```

  The body is the sequence of the Decisions, first bullet: `Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)?`, `conn.busy_timeout(bound)?`, read `PRAGMA user_version` and return `anyhow!("usage database schema version {version} is not {SCHEMA_VERSION}")` when it is not `SCHEMA_VERSION`, `conn.pragma_update(None, "synchronous", "NORMAL")?`, `Ok(Store { conn })`. No `Instant`, no `sleep`, no `execute_batch`, no `journal_mode`.

- [ ] 4. Run `cargo test -p zygo-usage --test store -- open_bounded` → 6 passed, 0 failed.

- [ ] 5. Run the Acceptance check: `test "$(cargo test -q -p zygo-usage --test store -- open_bounded --list 2>/dev/null | grep -c ': test$')" -ge 4 && cargo test -p zygo-usage --test store` → the count is 6 (at least 4), then every test of `store` passes (the `#[ignore = "timing: …"]` tests stay ignored).

- [ ] 6. Run `cargo clippy --locked -p zygo-usage --all-targets -- -D warnings` → no warning; and `cargo test --locked -p zygo-usage` → all pass.

- [ ] 7. Commit `crates/zygo-usage/src/store.rs`, `crates/zygo-usage/tests/store.rs` and this node file (ticked steps) with message `OS-3: bounded usage-store open` and the trailer `Zygo-Session:` with the implementing session's id and the trailer `Zygo-Work: phase3:OS-3`, each on its own line at the end; no Claude attribution.

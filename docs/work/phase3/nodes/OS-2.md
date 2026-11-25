# OS-2 — Usage store: `readbacks` table and three additive `trims` columns

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§6.5 items 2-6, O-C8, O§10 rows "Database without the new columns" and "Two full opens add a column at once", O§16 item 15 (store part)
**Files:** `crates/zygo-usage/src/schema.rs`, `crates/zygo-usage/src/store.rs`, `crates/zygo-usage/src/query.rs`, `crates/zygo-usage/tests/store.rs`, `crates/zygo/tests/usage_doctor.rs`, `crates/zygo-hooks/src/trim.rs`
**Consumes:** —
**Produces:** `ReadbackRow { ts: i64, session: String, read_in: String, result_id: String, tool: String, bytes: u64 }`, `TrimRow.result_id: Option<String>`, `TrimRow.command: Option<String>`, `TrimRow.shaper: Option<String>` (each `#[serde(default)]`), `Store::open(path: &Path) -> anyhow::Result<Store>` (adds each missing `trims` column once per process; "duplicate column name" is success), `Store::open_with_column_check(path: &Path, report_missing: &[&str]) -> anyhow::Result<Store>` (`#[doc(hidden)]` seam for O§16 item 15), `Store::insert_trim(&self, t: &TrimRow) -> anyhow::Result<()>` (ten columns; seven-column fallback on "no such column"), `Store::insert_readback(&self, r: &ReadbackRow) -> anyhow::Result<()>`, `Store::prune(&self, retain_days: u32) -> anyhow::Result<()>` (also deletes old `readbacks` rows)
**Acceptance:** `test "$(cargo test -q -p zygo-usage --test store -- readbacks_schema --list 2>/dev/null | grep -c ': test$')" -ge 7 && cargo test -p zygo-usage --test store && cargo test -p zygo-usage --lib && cargo test -p zygo --test usage_doctor` → every count holds (at least 7 named `readbacks_schema…` in zygo-usage `store`; a count of 0 fails before any run), then all pass, including new tests: `readbacks` and its two indexes exist after `open`; `PRAGMA user_version` stays 1; a database built in the test from the `97d8767` schema text gets the three columns on `open` and its old rows read them as null; `open_with_column_check(.., &["shaper"])` on a database that already has the column succeeds; `insert_trim` on a seven-column database writes the seven columns; `prune` deletes old `readbacks` rows and keeps new ones; `open_read_only` still opens the migrated database

## Plan

**Premises checked against the live code (branch `phase3/OS-2` at `b36b0eb`, which is `origin/phase-3`):**

- `crates/zygo-usage/src/schema.rs` and `crates/zygo-usage/src/store.rs` are byte-equal to their `97d8767` form (`git show 97d8767:<path> | diff - <path>` prints nothing). OS-1 changed no file in `zygo-usage`; it produced `Stored.session` / `Meta.session` in `zygo-results`, which this node does not use.
- `SCHEMA` is `schema.rs:5-78`; its text is lines 6-77. `trims` is `schema.rs:35-43` (seven columns); `upstream_errors` and its two indexes are `schema.rs:67-77`, the model for the new table (O§6.5 item 2). `TrimRow` is `schema.rs:119-130`, `#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]`.
- `store.rs:6` imports `anyhow::{anyhow, Result}`, so `Result<T>` in `store.rs` is `anyhow::Result<T>`. `SCHEMA_VERSION` is `store.rs:62` (= 1). `is_busy` is `store.rs:66-71`. `Store::open` is `store.rs:74-115` (busy timeout 5 s, a retry loop around WAL + `user_version` + `execute_batch(SCHEMA)`). `open_read_only` is `store.rs:119-132` (rejects only `user_version != 1`). `insert_trim` is `store.rs:232-245` (seven columns). `prune` is `store.rs:272-284`; it reads "now" itself with `time::OffsetDateTime::now_utc()` at `:276`. There is no clock injection. The prune test therefore uses fixed `ts` values far on each side of any cutoff (1 and 4_102_444_800), not values near `now() - 90 days`.
- `lib.rs` re-exports `schema::*` and `store::Store`; `store` and `query` are `pub mod`, so the test reaches `store::SCHEMA_VERSION` and `query::BYTES_TRIMMED_SQL` through `use zygo_usage::*;` (`tests/store.rs:3`, `:487`).
- `TrimRow` struct literals that list every field (they break when the struct gets fields): `crates/zygo-usage/tests/store.rs:30` (helper `trim`), `crates/zygo-usage/src/query.rs:226-227` (lib test), `crates/zygo/tests/usage_doctor.rs:37` (helper `trim`), `crates/zygo-hooks/src/trim.rs:118-126`. Every other literal uses `..Default::default()` (`crates/zygo-hooks/tests/statusline_data.rs:227`, `:606`, `:608`; `crates/zygo-hooks/src/statusline/mod.rs:99`; `crates/zygo/tests/compact_cmd.rs:286-287`; `crates/zygo/src/cmd/status.rs:156`) and needs no change.
- Existing tests in `tests/store.rs` that this node changes because they pin the table list or the `trims` columns: `schema_creates_three_tables_in_wal_mode` (`:44`), `never_list_columns_are_exact` (`:54`), `upstream_errors_table_is_created_on_an_existing_database` (`:525`). Test helpers that exist and are reused: `Tmp` (`:7-16`, creates its directory), `cols` (`:17-21`), `count` (`:22-24`), `survivors` (`:186-190`, selects `session`), `tables` (`:499-503`).
- Readers today read only `stored_bytes` and `view_bytes` from `trims` (`query.rs:52`, `:184`) and never touch `readbacks`. No reader in this node reads the new columns; the O§6.5 item 5 reader checks belong to OS-7.
- Callers of the full open `Store::open` in one process: the launcher at `crates/zygo-launch/src/launcher.rs:311`, `:437`, `:800` and `session_dir.rs:79`; `zygo usage` at `crates/zygo/src/cmd/usage.rs:57`; the work hooks at `crates/zygo-work/src/hooks.rs:278`, `:549`; the trim hook at `crates/zygo-hooks/src/trim.rs:128` (OS-4 moves it to `open_bounded`).
- SQLite error texts, checked with `sqlite3` on this machine: `INSERT INTO trims (ts, result_id) …` on a table without `result_id` fails with `table trims has no column named result_id` (not "no such column"); `ALTER TABLE trims ADD COLUMN ts …` on an existing column fails with `duplicate column name: ts`; `SELECT result_id FROM trims` fails with `no such column: result_id`. rusqlite 0.43 (`Cargo.toml:55`, bundled SQLite from `libsqlite3-sys` 0.40) gives these as `rusqlite::Error::SqliteFailure(ffi::Error, Some(message))`. The bundled SQLite supports `ALTER TABLE … DROP COLUMN` (3.35 and later), which the tests use to make a seven-column table under an open `Store`.
- Baseline of the Acceptance count: `cargo test -q -p zygo-usage --test store -- readbacks_schema --list | grep -c ': test$'` prints `0` today.

**Decisions:**

- **Fresh databases get ten columns from `SCHEMA`.** The `CREATE TABLE IF NOT EXISTS trims` text gains `result_id TEXT, command TEXT, shaper TEXT` after `view_bytes`. A fresh database is made only by the full open (the bounded open of OS-3 creates no file), so this agrees with O§6.5 item 3 "added by the full open only". An existing seven-column table is not touched by `CREATE … IF NOT EXISTS`; the column check adds the columns there, in the same order.
- **"One time per process" is one time per process per database path.** A process-wide set of the paths that `Store::open` has already checked skips `PRAGMA table_info` on later opens of the same path (the launcher opens the store up to four times). A process-wide flag is not used: tests and the CLI open more than one database in one process, and a flag would skip the second database. The seam `open_with_column_check` always runs the check, so O§16 item 15 can inject a missing column on a path already checked.
- **The column check runs inside the existing retry loop** of `open`, after `execute_batch(SCHEMA)`, so an `ALTER` that meets `SQLITE_BUSY` gets the same 5 s retry as the schema text. It does not run when `user_version > SCHEMA_VERSION` (that open fails, as today).
- **Seven-column fallback matches only a missing column.** `insert_trim` falls back only when preparing the ten-column statement fails with a message that starts with `table trims has no column named ` (what SQLite says, see premises) or `no such column` (the spec's text, O§6.5 item 4). Every other error (busy, no such table, a NOT NULL failure at execute, a byte count above `i64::MAX`) is returned unchanged (review focus 21).
- **The trim hook writes `None` in the three new fields.** OS-4 sets `result_id` and `shaper` (O§7.9) and OS-28 sets `command`; this node only keeps `trim.rs` compiling.

**Review focus:** item 21 (graph `## Review focus`): "The hook adding a column (write lock on every Bash call); the seven-column fallback hiding an error other than 'no such column'; a bounded open that waits longer than `bound` — test in OS-2, OS-3". OS-2's part: the check runs `ALTER` only for a missing column, so a database that has the columns gets no schema write (`PRAGMA schema_version` unchanged, `readbacks_schema_column_check_writes_no_schema_when_columns_exist`); and the fallback does not hide another error (`readbacks_schema_insert_trim_fallback_keeps_other_errors`). The bounded open is OS-3.

**Consumes:** — (no other node's output).

**Produces** (the three that exist today, with their current signatures, then the new ones):

- Today: `pub fn open(path: &Path) -> Result<Store>` (`store.rs:74`), `pub fn insert_trim(&self, t: &TrimRow) -> Result<()>` (`store.rs:232`), `pub fn prune(&self, retain_days: u32) -> Result<()>` (`store.rs:272`), `pub struct TrimRow { pub ts: i64, pub session: String, pub agent_id: Option<String>, pub tool: String, pub kind: String, pub stored_bytes: u64, pub view_bytes: u64 }` (`schema.rs:121-130`). `Result` = `anyhow::Result`.
- After this node: `ReadbackRow { ts: i64, session: String, read_in: String, result_id: String, tool: String, bytes: u64 }`; `TrimRow.result_id: Option<String>`, `TrimRow.command: Option<String>`, `TrimRow.shaper: Option<String>` (each `#[serde(default)]`); `Store::open(path: &Path) -> anyhow::Result<Store>` (adds each missing `trims` column once per process per path; "duplicate column name" is success); `Store::open_with_column_check(path: &Path, report_missing: &[&str]) -> anyhow::Result<Store>` (`#[doc(hidden)]`); `Store::insert_trim(&self, t: &TrimRow) -> anyhow::Result<()>` (ten columns; seven-column fallback on a missing column); `Store::insert_readback(&self, r: &ReadbackRow) -> anyhow::Result<()>`; `Store::prune(&self, retain_days: u32) -> anyhow::Result<()>` (also deletes `readbacks` rows with `ts < cutoff`).

### Steps

Cycle A: the `readbacks` table, the row types, the ten-column insert, prune.

- [x] **A1 — test helpers.** In `crates/zygo-usage/tests/store.rs`:
  - In the helper `trim` (`:29-31`) add `result_id: None, command: None, shaper: None` to the literal.
  - After the helper `unit` (`:32-34`) add:

  ```rust
  /// The `SCHEMA` text of commit `97d8767`: lines 6-77 of `git show 97d8767:crates/zygo-usage/src/schema.rs`, copied
  /// byte for byte (from `CREATE TABLE IF NOT EXISTS requests (` to `… upstream_errors_ts ON upstream_errors(ts);`).
  const SCHEMA_97D8767: &str = "
  CREATE TABLE IF NOT EXISTS requests (
      ts INTEGER NOT NULL,
      session TEXT NOT NULL,
      token_label TEXT,
      agent TEXT NOT NULL,
      agent_version TEXT,
      agent_id TEXT,
      agent_type TEXT,
      request_class TEXT,
      compaction INTEGER NOT NULL,
      route TEXT NOT NULL,
      role_requested TEXT,
      role_served TEXT,
      model TEXT NOT NULL,
      provider TEXT NOT NULL,
      profile TEXT,
      passthrough INTEGER NOT NULL,
      status INTEGER NOT NULL,
      latency_ms INTEGER NOT NULL,
      input_tokens INTEGER NOT NULL,
      output_tokens INTEGER NOT NULL,
      cache_read_tokens INTEGER NOT NULL,
      cache_write_tokens INTEGER NOT NULL,
      estimated_usd REAL NOT NULL,
      adjustments TEXT NOT NULL,
      redactions TEXT NOT NULL,
      repo TEXT,
      work_item TEXT
  );
  CREATE TABLE IF NOT EXISTS trims (
      ts INTEGER NOT NULL,
      session TEXT NOT NULL,
      agent_id TEXT,
      tool TEXT NOT NULL,
      kind TEXT NOT NULL,
      stored_bytes INTEGER NOT NULL,
      view_bytes INTEGER NOT NULL
  );
  CREATE TABLE IF NOT EXISTS work_units (
      unit TEXT NOT NULL,
      round INTEGER NOT NULL,
      session TEXT NOT NULL,
      agent_id TEXT NOT NULL,
      agent_type TEXT,
      began_at INTEGER NOT NULL,
      ended_at INTEGER,
      tool_calls INTEGER NOT NULL,
      failed_calls INTEGER NOT NULL,
      check_ran INTEGER NOT NULL,
      check_passed INTEGER NOT NULL,
      guard_events TEXT NOT NULL,
      levels TEXT NOT NULL,
      verdict TEXT,
      PRIMARY KEY (session, agent_id, unit, round, began_at)
  );
  CREATE INDEX IF NOT EXISTS requests_session_ts ON requests(session, ts);
  CREATE INDEX IF NOT EXISTS requests_ts ON requests(ts);
  CREATE INDEX IF NOT EXISTS trims_session ON trims(session);
  CREATE INDEX IF NOT EXISTS trims_ts ON trims(ts);
  CREATE INDEX IF NOT EXISTS work_units_session ON work_units(session);
  CREATE TABLE IF NOT EXISTS sync_pending (session TEXT PRIMARY KEY);
  CREATE TABLE IF NOT EXISTS upstream_errors (
      ts INTEGER NOT NULL,
      session TEXT NOT NULL,
      request_class TEXT,
      model TEXT NOT NULL,
      status INTEGER NOT NULL,
      error_type TEXT NOT NULL,
      error_message TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS upstream_errors_session_ts ON upstream_errors(session, ts);
  CREATE INDEX IF NOT EXISTS upstream_errors_ts ON upstream_errors(ts);
  ";
  const TRIM_SEVEN: [&str; 7] = ["ts", "session", "agent_id", "tool", "kind", "stored_bytes", "view_bytes"];
  const TRIM_TEN: [&str; 10] = ["ts", "session", "agent_id", "tool", "kind", "stored_bytes", "view_bytes", "result_id", "command", "shaper"];
  const RID: &str = "01J00000000000000000000000";
  /// A database as the code at `97d8767` leaves it: its schema text, `user_version` 1, one seven-column `trims` row.
  fn old_db(db: &Path) {
      let c = Connection::open(db).unwrap();
      c.execute_batch(SCHEMA_97D8767).unwrap();
      c.execute_batch("PRAGMA user_version = 1").unwrap();
      c.execute("INSERT INTO trims (ts, session, agent_id, tool, kind, stored_bytes, view_bytes) VALUES (5, 'S1', NULL, 'Bash', 'head', 1000, 100)", []).unwrap();
  }
  fn pragma(db: &Path, name: &str) -> i64 {
      Connection::open(db).unwrap().query_row(&format!("PRAGMA {name}"), [], |r| r.get(0)).unwrap()
  }
  /// Makes `trims` a seven-column table again under an open `Store` (SQLite 3.35+ `DROP COLUMN`).
  fn drop_new_trim_columns(db: &Path) {
      let c = Connection::open(db).unwrap();
      for col in ["result_id", "command", "shaper"] { c.execute_batch(&format!("ALTER TABLE trims DROP COLUMN {col}")).unwrap(); }
  }
  fn readback(session: &str, ts: i64) -> ReadbackRow {
      ReadbackRow { ts, session: session.into(), read_in: session.into(), result_id: RID.into(), tool: "result_view".into(), bytes: 10 }
  }
  type TenCols = (i64, String, Option<String>, String, String, i64, i64, Option<String>, Option<String>, Option<String>);
  fn ten_cols(db: &Path, session: &str) -> TenCols {
      Connection::open(db).unwrap().query_row(
          "SELECT ts, session, agent_id, tool, kind, stored_bytes, view_bytes, result_id, command, shaper FROM trims WHERE session = ?1",
          [session], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?))).unwrap()
  }
  ```

  Check: `grep -c readbacks` on the const body is 0, and `git show 97d8767:crates/zygo-usage/src/schema.rs | sed -n 6,77p` equals the const body (compare by eye or with a one-off `diff`).

- [x] **A2 — update the three pinned tests.** In `tests/store.rs`:
  - `schema_creates_three_tables_in_wal_mode` (`:44`): `assert_eq!(names, ["readbacks", "requests", "sync_pending", "trims", "upstream_errors", "work_units"]);` (keep the comment, add `O§6.5: readbacks`).
  - `never_list_columns_are_exact` (`:54`): `assert_eq!(cols(&t.db(), "trims"), TRIM_TEN);`.
  - `upstream_errors_table_is_created_on_an_existing_database` (`:525`): `assert_eq!(tables(&t.db()), ["readbacks", "requests", "sync_pending", "trims", "work_units"]);`.

- [x] **A3 — failing tests, cycle A.** Append to `tests/store.rs` after the last test, under a `// ---- OS-2: readbacks and the additive trims columns (O§6.5) ----` line:

  ```rust
  #[test] // O§6.5 item 2: the table, its NOT NULL columns, its two indexes
  fn readbacks_schema_table_and_indexes_exist_after_open() {
      let t = Tmp::new();
      drop(Store::open(&t.db()).unwrap());
      assert_eq!(cols(&t.db(), "readbacks"), ["ts", "session", "read_in", "result_id", "tool", "bytes"]);
      let c = Connection::open(t.db()).unwrap();
      let notnull: Vec<i64> = c.prepare("PRAGMA table_info(readbacks)").unwrap().query_map([], |r| r.get(3)).unwrap().map(|r| r.unwrap()).collect();
      assert_eq!(notnull, [1, 1, 1, 1, 1, 1]);
      let idx: Vec<String> = c.prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'readbacks' ORDER BY name").unwrap()
          .query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
      assert_eq!(idx, ["readbacks_session", "readbacks_ts"]);
      let on = |i: &str| -> Vec<String> { c.prepare(&format!("PRAGMA index_info({i})")).unwrap().query_map([], |r| r.get(2)).unwrap().map(|r| r.unwrap()).collect() };
      assert_eq!(on("readbacks_session"), ["session"]);
      assert_eq!(on("readbacks_ts"), ["ts"]);
  }

  #[test] // O-C8, O§6.5 preamble: no schema version change, on a fresh and on a 97d8767 database
  fn readbacks_schema_user_version_stays_1() {
      assert_eq!(store::SCHEMA_VERSION, 1);
      let (fresh, old) = (Tmp::new(), Tmp::new());
      drop(Store::open(&fresh.db()).unwrap());
      old_db(&old.db());
      drop(Store::open(&old.db()).unwrap());
      assert_eq!(pragma(&fresh.db(), "user_version"), 1);
      assert_eq!(pragma(&old.db(), "user_version"), 1);
  }

  #[test] // O§6.5 item 4: ten columns written; item 4 `#[serde(default)]`: a row without the new keys reads them as None
  fn readbacks_schema_insert_trim_writes_ten_columns() {
      let t = Tmp::new();
      let s = Store::open(&t.db()).unwrap();
      let tr = TrimRow { agent_id: Some("a1".into()), result_id: Some(RID.into()), command: Some("git diff".into()), shaper: Some("diff".into()), ..trim("S1", 5, 1000, 100) };
      s.insert_trim(&tr).unwrap();
      assert_eq!(ten_cols(&t.db(), "S1"), (5, "S1".into(), Some("a1".into()), "Bash".into(), "head".into(), 1000, 100, Some(RID.into()), Some("git diff".into()), Some("diff".into())));
      let old: TrimRow = serde_json::from_str(r#"{"ts":5,"session":"S1","agent_id":null,"tool":"Bash","kind":"head","stored_bytes":1000,"view_bytes":100}"#).unwrap();
      assert_eq!(old, trim("S1", 5, 1000, 100));
  }

  #[test] // O§6.4, O§6.5 item 2: one row, every column
  fn readbacks_schema_insert_readback_round_trip() {
      let t = Tmp::new();
      let s = Store::open(&t.db()).unwrap();
      s.insert_readback(&ReadbackRow { ts: 7, session: "S1".into(), read_in: "S2".into(), result_id: RID.into(), tool: "result_view".into(), bytes: 4096 }).unwrap();
      let row: (i64, String, String, String, String, i64) = Connection::open(t.db()).unwrap()
          .query_row("SELECT ts, session, read_in, result_id, tool, bytes FROM readbacks", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).unwrap();
      assert_eq!(row, (7, "S1".into(), "S2".into(), RID.into(), "result_view".into(), 4096));
  }

  #[test] // O§6.5 item 6; fixed ts values far from any cutoff, so no clock margin is involved
  fn readbacks_schema_prune_deletes_old_readbacks_and_keeps_new() {
      const OLD_TS: i64 = 1; // 1970-01-01: older than every cutoff
      const NEW_TS: i64 = 4_102_444_800; // 2100-01-01T00:00:00Z: newer than every cutoff before 2100
      let t = Tmp::new();
      let s = Store::open(&t.db()).unwrap();
      s.insert_readback(&readback("old", OLD_TS)).unwrap();
      s.insert_readback(&readback("new", NEW_TS)).unwrap();
      s.prune(90).unwrap();
      assert_eq!(survivors(&t.db(), "readbacks"), ["new"]);
      s.prune(0).unwrap(); // 0 keeps everything
      assert_eq!(survivors(&t.db(), "readbacks"), ["new"]);
  }
  ```

- [x] **A4 — run and see it fail.** `cargo test -p zygo-usage --test store` → does not compile: `ReadbackRow` not found, no field `result_id`/`command`/`shaper` on `TrimRow`, no method `insert_readback`.

- [x] **A5 — schema.** In `crates/zygo-usage/src/schema.rs`:
  - `trims` (`:35-43`): after `view_bytes INTEGER NOT NULL` add `,` and the lines `    result_id TEXT,` / `    command TEXT,` / `    shaper TEXT` (nullable, in this order).
  - After `CREATE INDEX IF NOT EXISTS upstream_errors_ts ON upstream_errors(ts);` (`:77`), before the closing `";`, add the O§6.5 item 2 text exactly (SQL comments included):

    ```sql
    CREATE TABLE IF NOT EXISTS readbacks (
        ts INTEGER NOT NULL,
        session TEXT NOT NULL,   -- the trim's session (O§6.4 key)
        read_in TEXT NOT NULL,   -- the reading session (carry context)
        result_id TEXT NOT NULL,
        tool TEXT NOT NULL,      -- result_view | result_search | result_tree
        bytes INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS readbacks_session ON readbacks(session);
    CREATE INDEX IF NOT EXISTS readbacks_ts ON readbacks(ts);
    ```
  - `TrimRow` (`:121-130`): after `view_bytes` add, each with its doc line:
    `/// The stored result's id (O§7.9). #[serde(default)] pub result_id: Option<String>,` — `/// The O§7.2 command name; null for a tool other than Bash. #[serde(default)] pub command: Option<String>,` — `/// The last step that changed the text (O§7.9). #[serde(default)] pub shaper: Option<String>,` (attribute on its own line above each field).
  - After `TrimRow` add:

    ```rust
    /// One successful reply of `result_view`, `result_search` or `result_tree` (O§6.4). Every timestamp is Unix seconds.
    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    pub struct ReadbackRow {
        /// Unix seconds.
        pub ts: i64,
        /// The session whose hook stored the result (O§6.4 key).
        pub session: String,
        /// The reading session (the carry context).
        pub read_in: String,
        pub result_id: String,
        /// `result_view`, `result_search` or `result_tree`.
        pub tool: String,
        /// The reply's byte count.
        pub bytes: u64,
    }
    ```

- [x] **A6 — store: ten-column insert, `insert_readback`, prune.** In `crates/zygo-usage/src/store.rs`:
  - Replace the body of `insert_trim` (`:232-245`) with the ten-column insert (the fallback comes in B4):
    `INSERT INTO trims (ts, session, agent_id, tool, kind, stored_bytes, view_bytes, result_id, command, shaper) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)` with `t.ts, t.session, t.agent_id, t.tool, t.kind, i64::try_from(t.stored_bytes)?, i64::try_from(t.view_bytes)?, t.result_id, t.command, t.shaper` (via `rusqlite::params!`, as `insert_work_unit` does). Keep the statement text in a `const INSERT_TRIM: &str` next to `INSERT_REQUEST`.
  - After `insert_trim` add `pub fn insert_readback(&self, r: &ReadbackRow) -> Result<()>`: `prepare_cached("INSERT INTO readbacks (ts, session, read_in, result_id, tool, bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")` with `r.ts, r.session, r.read_in, r.result_id, r.tool, i64::try_from(r.bytes)?`. Doc: `/// One `readbacks` row (O§6.4). The MCP server writes it through the bounded open (OS-5).` No check of `tool`'s value (the spec sets none).
  - `prune` (`:272-284`): after the `trims` delete add `tx.execute("DELETE FROM readbacks WHERE ts < ?1", [cutoff])?;`. `replace_requests` stays as it is (O§6.5 item 6).

- [x] **A7 — the other `TrimRow` literals.** Add `result_id: None, command: None, shaper: None` to: `crates/zygo-usage/src/query.rs:226-227` (lib test `bytes_trimmed_runs_the_indexed_statement`); `crates/zygo/tests/usage_doctor.rs:37` (helper `trim`); `crates/zygo-hooks/src/trim.rs:118-126` (the hook's row; OS-4 and OS-28 fill them, O§7.9). Check: `cargo check --workspace --all-targets --locked` compiles with no error.

- [x] **A8 — run and see it pass.** `cargo test -p zygo-usage --test store -- readbacks_schema` → the 5 cycle-A tests pass; `cargo test -p zygo-usage --test store` → all pass (the three updated tests included); `cargo test -p zygo-usage --lib` → passes.

- [x] **A9 — commit.** `git add crates/zygo-usage/src/schema.rs crates/zygo-usage/src/store.rs crates/zygo-usage/src/query.rs crates/zygo-usage/tests/store.rs crates/zygo/tests/usage_doctor.rs crates/zygo-hooks/src/trim.rs` and commit `OS-2: readbacks table, ReadbackRow, ten-column trims insert, prune of readbacks` with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:OS-2`, each on its own line. No Claude attribution.

Cycle B: the column check on old databases, the seam, the seven-column fallback.

- [x] **B1 — failing tests, cycle B.** Append to `tests/store.rs`:

  ```rust
  #[test] // O§6.5 item 3; O§16 item 15 "a full open then adds the three columns"; old rows read the new columns as null
  fn readbacks_schema_old_database_gets_three_columns_and_old_rows_read_null() {
      let t = Tmp::new();
      old_db(&t.db());
      assert_eq!(cols(&t.db(), "trims"), TRIM_SEVEN);
      assert!(!tables(&t.db()).iter().any(|n| n == "readbacks"));
      drop(Store::open(&t.db()).unwrap());
      assert_eq!(cols(&t.db(), "trims"), TRIM_TEN);
      assert!(tables(&t.db()).iter().any(|n| n == "readbacks"));
      assert_eq!(ten_cols(&t.db(), "S1"), (5, "S1".into(), None, "Bash".into(), "head".into(), 1000, 100, None, None, None));
  }

  #[test] // O§10 "Two full opens add a column at once"; O§16 item 15: an injected missing column gets "duplicate column name" and succeeds
  fn readbacks_schema_injected_missing_column_is_success() {
      let t = Tmp::new();
      old_db(&t.db());
      drop(Store::open(&t.db()).unwrap()); // adds the three columns
      let s = Store::open_with_column_check(&t.db(), &["shaper"]).unwrap();
      assert_eq!(cols(&t.db(), "trims"), TRIM_TEN);
      s.insert_trim(&TrimRow { shaper: Some("size".into()), ..trim("S2", 6, 10, 1) }).unwrap();
      assert_eq!(ten_cols(&t.db(), "S2").9.as_deref(), Some("size"));
  }

  #[test] // O§6.5 item 4, O§10 "Database without the new columns (hook insert)"; item 3 "one time per process"
  fn readbacks_schema_insert_trim_on_seven_column_database_writes_seven_columns() {
      let t = Tmp::new();
      old_db(&t.db());
      drop(Store::open(&t.db()).unwrap());
      drop_new_trim_columns(&t.db());
      let s = Store::open(&t.db()).unwrap(); // this process checked this path already: no second check
      assert_eq!(cols(&t.db(), "trims"), TRIM_SEVEN);
      s.insert_trim(&TrimRow { result_id: Some(RID.into()), command: Some("cargo test".into()), shaper: Some("libtest".into()), ..trim("S2", 6, 2000, 200) }).unwrap();
      assert_eq!(count(&t.db(), "trims"), 2);
      let row: (i64, String, Option<String>, String, String, i64, i64) = Connection::open(t.db()).unwrap()
          .query_row("SELECT ts, session, agent_id, tool, kind, stored_bytes, view_bytes FROM trims WHERE session = 'S2'", [], |r|
              Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))).unwrap();
      assert_eq!(row, (6, "S2".into(), None, "Bash".into(), "head".into(), 2000, 200));
  }

  #[test] // review focus 21: no ALTER (no schema write) when the columns exist; the seam always checks
  fn readbacks_schema_column_check_writes_no_schema_when_columns_exist() {
      let t = Tmp::new();
      drop(Store::open(&t.db()).unwrap());
      let v = pragma(&t.db(), "schema_version");
      drop(Store::open_with_column_check(&t.db(), &[]).unwrap()); // the check runs; nothing is missing
      assert_eq!(pragma(&t.db(), "schema_version"), v);
      drop_new_trim_columns(&t.db());
      drop(Store::open_with_column_check(&t.db(), &[]).unwrap()); // the seam checks a path already checked
      assert_eq!(cols(&t.db(), "trims"), TRIM_TEN);
  }

  #[test] // review focus 21: the fallback hides no error other than a missing column
  fn readbacks_schema_insert_trim_fallback_keeps_other_errors() {
      let t = Tmp::new();
      let s = Store::open(&t.db()).unwrap();
      Connection::open(t.db()).unwrap().execute_batch("DROP TABLE trims").unwrap();
      let e = s.insert_trim(&trim("S1", 1, 10, 1)).err().expect("a missing table is not a missing column").to_string();
      assert!(e.contains("no such table: trims"), "{e}");
  }

  #[test] // O§6.5 preamble, O-C8: the read-only open of each binary still opens the database, before and after the full open
  fn readbacks_schema_open_read_only_opens_old_and_migrated_database() {
      let t = Tmp::new();
      old_db(&t.db());
      assert_eq!(Store::open_read_only(&t.db()).unwrap().bytes_trimmed("S1").unwrap(), 900);
      drop(Store::open(&t.db()).unwrap());
      assert_eq!(Store::open_read_only(&t.db()).unwrap().bytes_trimmed("S1").unwrap(), 900);
  }
  ```

- [x] **B2 — run and see it fail.** `cargo test -p zygo-usage --test store -- readbacks_schema` → does not compile: no function `open_with_column_check`. (With a stub that calls `open`, `…old_database_gets_three_columns…` fails on `TRIM_TEN` and `…seven_column_database…` fails with `table trims has no column named result_id`.)

- [x] **B3 — store: the column check and the seam.** In `crates/zygo-usage/src/store.rs`:
  - Add near `SCHEMA_VERSION`:

    ```rust
    /// The `trims` columns the full open adds to an older table (O§6.5 item 3), in order.
    const TRIM_COLUMNS: [(&str, &str); 3] = [("result_id", "TEXT"), ("command", "TEXT"), ("shaper", "TEXT")];
    /// The database paths whose `trims` columns this process has checked (O§6.5 item 3: one time per process).
    static TRIM_COLUMNS_CHECKED: std::sync::Mutex<std::collections::BTreeSet<std::path::PathBuf>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());

    /// The message of an SQLite failure, when there is one.
    fn sqlite_message(e: &rusqlite::Error) -> Option<&str> {
        match e { rusqlite::Error::SqliteFailure(_, Some(m)) => Some(m), _ => None }
    }
    ```
  - Add the check (algorithm fixed here):

    ```rust
    /// Adds each `trims` column of `TRIM_COLUMNS` that `PRAGMA table_info(trims)` lacks, plus each name in
    /// `report_missing` (the O§16 item 15 seam). "duplicate column name" means another process added it first: success.
    fn add_trim_columns(conn: &Connection, report_missing: &[&str]) -> rusqlite::Result<()> {
        let have: Vec<String> = conn.prepare("PRAGMA table_info(trims)")?
            .query_map([], |r| r.get(1))?.collect::<rusqlite::Result<_>>()?;
        for (name, ty) in TRIM_COLUMNS {
            if have.iter().any(|h| h == name) && !report_missing.contains(&name) {
                continue;
            }
            match conn.execute_batch(&format!("ALTER TABLE trims ADD COLUMN {name} {ty}")) {
                Err(e) if !sqlite_message(&e).is_some_and(|m| m.starts_with("duplicate column name")) => return Err(e),
                _ => {}
            }
        }
        Ok(())
    }
    ```
  - Rename the body of `open` into `fn open_full(path: &Path, column_check: Option<&[&str]>) -> Result<Store>`. In its `attempt` closure, after `conn.execute_batch(SCHEMA)?;` add `if let Some(report_missing) = column_check { add_trim_columns(&conn, report_missing)?; }` (inside the retry loop, so a busy `ALTER` is retried as the schema text is; not reached when `version > SCHEMA_VERSION`). After the loop breaks, when `column_check.is_some()`, insert `path.to_path_buf()` into `TRIM_COLUMNS_CHECKED` (lock with `.lock().unwrap_or_else(|e| e.into_inner())`).
  - `pub fn open(path: &Path) -> Result<Store>`: `let checked = TRIM_COLUMNS_CHECKED.lock().unwrap_or_else(|e| e.into_inner()).contains(path); Store::open_full(path, if checked { None } else { Some(&[] as &[&str]) })`. Doc: `/// The full open: creates the file and the schema, and adds each missing `trims` column one time per process for this path (O§6.5 item 3). The launcher and the CLI use it; the hook and the MCP server use `open_bounded` (OS-3).`
  - `#[doc(hidden)] pub fn open_with_column_check(path: &Path, report_missing: &[&str]) -> Result<Store>`: `Store::open_full(path, Some(report_missing))`. Doc: `/// `open`, with the column check always run and each name of `report_missing` treated as missing (O§16 item 15).`

- [x] **B4 — store: the seven-column fallback.** In `insert_trim`, convert the byte counts first (`let (stored, view) = (i64::try_from(t.stored_bytes)?, i64::try_from(t.view_bytes)?);`), then:

  ```rust
  match self.conn.prepare_cached(INSERT_TRIM) {
      Ok(mut st) => { st.execute(rusqlite::params![t.ts, t.session, t.agent_id, t.tool, t.kind, stored, view, t.result_id, t.command, t.shaper])?; }
      // O§6.5 item 4: a table no new full open has touched yet. Only a missing column falls back.
      Err(e) if sqlite_message(&e).is_some_and(|m| m.starts_with("table trims has no column named ") || m.starts_with("no such column")) => {
          self.conn.prepare_cached(INSERT_TRIM_SEVEN)?.execute(rusqlite::params![t.ts, t.session, t.agent_id, t.tool, t.kind, stored, view])?;
      }
      Err(e) => return Err(e.into()),
  }
  Ok(())
  ```
  with `const INSERT_TRIM_SEVEN: &str = "INSERT INTO trims (ts, session, agent_id, tool, kind, stored_bytes, view_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";` next to `INSERT_TRIM`. Doc on `insert_trim`: `/// Ten columns; on a table without the new columns, the seven old ones (O§6.5 item 4).`

- [x] **B5 — run and see it pass.** `cargo test -p zygo-usage --test store -- readbacks_schema` → 11 tests pass. Mutation spot checks (revert each after): (a) make the fallback guard `Err(_)` → `readbacks_schema_insert_trim_fallback_keeps_other_errors` fails, and (b) drop the `duplicate column name` arm (return every `ALTER` error) → `readbacks_schema_injected_missing_column_is_success` fails.

- [x] **B6 — commit.** Same six paths as A9; message `OS-2: add missing trims columns on the full open; seven-column insert fallback` with the two trailers, each on its own line. No Claude attribution.

Verification.

- [x] **V1 — Acceptance.** Run exactly:
  `test "$(cargo test -q -p zygo-usage --test store -- readbacks_schema --list 2>/dev/null | grep -c ': test$')" -ge 7 && cargo test -p zygo-usage --test store && cargo test -p zygo-usage --lib && cargo test -p zygo --test usage_doctor`
  Pass means: the count prints 11 (at least 7; it was 0 before this node), and the four test runs each end with `test result: ok` and 0 failed. A count below 7 fails before any run.
- [x] **V2 — floor, affected crates.** `cargo test --locked -p zygo-usage -p zygo-hooks -p zygo-launch -p zygo-work -p zygo-proxy -p zygo-mcp` and `cargo test --locked -p zygo --test usage_doctor --test compact_cmd --test hook_trim` → all pass (the other `TrimRow` users and every full-open caller). Then `cargo test --workspace --locked --no-fail-fast` → no new failure against `origin/phase-3`. Never run the zygo binary with `ZYGO_SESSION_DIR` set (`env -u ZYGO_SESSION_DIR` if needed).
- [x] **V3 — scope.** `git diff --name-only origin/phase-3...HEAD` lists only the six **Files** of this node plus this node file.

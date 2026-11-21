# HUD-1 — Usage store: Store::bytes_trimmed

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.4, AU4 (trimmed read on every call)
**Files:** `crates/zygo-usage/src/query.rs`, `crates/zygo-usage/tests/store.rs`
**Consumes:** —
**Produces:** `Store::bytes_trimmed(&self, session: &str) -> anyhow::Result<u64>`
**Acceptance:** `cargo test -p zygo-usage && cargo test --workspace --locked --no-run` → all pass, including `bytes_trimmed` equal to the `summary` trimmed value for the same session and a query plan that uses the session index

## Plan

Premises checked 2026-11-21 against the live code: `summary` sums trims at `crates/zygo-usage/src/query.rs:179-183` with `(?1 IS NULL OR session = ?1) AND (?2 IS NULL OR ts >= ?2)`; index `trims_session ON trims(session)` at `crates/zygo-usage/src/schema.rs:63`; `Store::open_read_only` at `crates/zygo-usage/src/store.rs:111`; `Store.conn` is `pub(crate)` (`store.rs:12`); `u64_at` at `query.rs:43`. rusqlite 0.43.0 has only the features `bundled`, `cache`, `hashlink`, `modern_sqlite` and `ffi-sqlite-wasm-rs` here (`cargo tree -p zygo-usage -e features -i rusqlite`), so `trace` and `hooks` are off; `Statement::get_status(StatementStatus)` has no feature gate (rusqlite `src/statement.rs:736`), and `CachedStatement` keeps its counters when it goes back to the cache (it only clears bindings, rusqlite `src/cache.rs:156`). A scratch probe on rusqlite 0.43.0 `bundled`, with the three trim rows of the step 1 unit test, gave: the cached spec statement after one `query_row` → `Run` 1, `FullscanStep` 0; the `summary` form → `FullscanStep` 2. Plans on the bundled SQLite 3.54.1 (`libsqlite3-sys` 0.40.0, `sqlite3.h:149`; the same plans on the sqlite3 3.53.4 CLI): the spec query → `SEARCH trims USING INDEX trims_session (session=?)`; the `summary` form → `SCAN trims`.

- [x] **1. Write the failing tests.** Append to `crates/zygo-usage/tests/store.rs` (it already has `Tmp`, `trim(session, ts, stored, view)`, `Connection` and `use zygo_usage::*;`):

  ```rust
  #[test]
  fn bytes_trimmed_equals_summary_for_the_session() { // H§7.4
      let t = Tmp::new();
      let w = Store::open(&t.db()).unwrap();
      w.insert_trim(&trim("A", 1, 1000, 100)).unwrap();
      w.insert_trim(&trim("A", 2, 500, 500)).unwrap();
      w.insert_trim(&trim("A", 3, 10, 50)).unwrap(); // view larger than stored counts 0
      w.insert_trim(&trim("A", 4, 700, 0)).unwrap();
      w.insert_trim(&trim("B", 5, 300, 0)).unwrap();
      let s = Store::open_read_only(&t.db()).unwrap(); // the HUD opens the store read-only
      assert_eq!(s.bytes_trimmed("A").unwrap(), 1600);
      assert_eq!(s.bytes_trimmed("A").unwrap(),
                 s.summary(&Filter { session: Some("A".into()), since: None }).unwrap().bytes_trimmed);
      assert_eq!(s.bytes_trimmed("B").unwrap(), 300);
      assert_eq!(s.bytes_trimmed("none").unwrap(), 0);
  }

  #[test]
  fn bytes_trimmed_query_uses_the_session_index() { // H§7.4: one indexed SUM
      let t = Tmp::new();
      drop(Store::open(&t.db()).unwrap()); // creates the schema and its indexes
      let c = Connection::open(t.db()).unwrap();
      let mut q = c.prepare(&format!("EXPLAIN QUERY PLAN {}", query::BYTES_TRIMMED_SQL)).unwrap();
      let plan: Vec<String> = q.query_map(["A"], |r| r.get::<_, String>(3)).unwrap().map(|r| r.unwrap()).collect();
      assert!(plan.iter().any(|d| d.starts_with("SEARCH trims USING INDEX trims_session")), "{plan:?}");
      assert!(!plan.iter().any(|d| d.starts_with("SCAN trims")), "{plan:?}");
  }
  ```

  Append to the end of `crates/zygo-usage/src/query.rs` this unit test. It reaches the `pub(crate)` `conn` and proves that `Store::bytes_trimmed` runs `BYTES_TRIMMED_SQL` through the statement cache and finds its rows with no full-scan step. A method that calls `summary`, `conn.query_row` or other SQL leaves the cached statement with `Run` 0; a method that runs a scanning text has `FullscanStep` > 0. The last part is the control: it proves that the counter sees the scan of today's `summary` form.

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use rusqlite::StatementStatus::{FullscanStep, Run};

      #[test]
      fn bytes_trimmed_runs_the_indexed_statement() { // H§7.4: one indexed SUM per call
          let conn = rusqlite::Connection::open_in_memory().unwrap();
          conn.execute_batch(SCHEMA).unwrap();
          let s = Store { conn };
          for (session, stored, view) in [("A", 1000, 100), ("B", 300, 0), ("C", 50, 0)] {
              s.insert_trim(&TrimRow { ts: 1, session: session.into(), agent_id: None, tool: "Bash".into(),
                                       kind: "head".into(), stored_bytes: stored, view_bytes: view }).unwrap();
          }
          assert_eq!(s.bytes_trimmed("A").unwrap(), 900);
          let st = s.conn.prepare_cached(BYTES_TRIMMED_SQL).unwrap();
          assert_eq!(st.get_status(Run), 1); // the method ran this exact text, once, through the cache
          assert_eq!(st.get_status(FullscanStep), 0); // by the index, not by a scan
          let mut scan = s.conn.prepare("SELECT COALESCE(SUM(MAX(stored_bytes - view_bytes, 0)), 0) FROM trims WHERE (?1 IS NULL OR session = ?1)").unwrap();
          scan.query_row(["A"], |r| r.get::<_, i64>(0)).unwrap();
          assert!(scan.get_status(FullscanStep) > 0); // control: the summary form scans
      }
  }
  ```

- [x] **2. Run them and see them fail.** `cargo test -p zygo-usage bytes_trimmed` → compile errors: no method `bytes_trimmed` on `Store` (E0599) and no value `BYTES_TRIMMED_SQL` (E0425), in both the `zygo-usage` lib test target and `tests/store.rs`. This is the only red state here that has no fake code: the method does not exist yet, and a wrong temporary body is a stub, which the project forbids. The behavioral discrimination is in the test itself: the control assertion in `bytes_trimmed_runs_the_indexed_statement` shows a scanning query fails the `FullscanStep` check, and a method that does not run the cached `BYTES_TRIMMED_SQL` fails the `Run == 1` check.

- [x] **3. Implement.** In `crates/zygo-usage/src/query.rs`, add above `impl Store` (`#[doc(hidden)]` as on `Store::busy_timeout_ms`, `store.rs:126`; the constant exists so the plan test checks the text the method runs):

  ```rust
  #[doc(hidden)]
  pub const BYTES_TRIMMED_SQL: &str =
      "SELECT COALESCE(SUM(MAX(stored_bytes - view_bytes, 0)), 0) FROM trims WHERE session = ?1";
  ```

  and inside `impl Store`, after `summary`:

  ```rust
  /// The session's trimmed bytes: one SUM on the `trims_session` index (H§7.4). Equal to
  /// `summary(&Filter { session: Some(session.to_string()), since: None })?.bytes_trimmed`.
  pub fn bytes_trimmed(&self, session: &str) -> Result<u64> {
      let mut q = self.conn.prepare_cached(BYTES_TRIMMED_SQL)?;
      Ok(q.query_row([session], |r| u64_at(r, 0))?)
  }
  ```

  The body uses `prepare_cached`, not `Connection::query_row`: the unit test reads the counters of the cached statement.

  Do not change `summary` or `lib.rs` (`pub mod query` already exports the constant; `Store` is already re-exported).

- [x] **4. Run them and see them pass.** `cargo test -p zygo-usage bytes_trimmed` → lib target `1 passed; 0 failed` (`query::tests::bytes_trimmed_runs_the_indexed_statement`) and `tests/store.rs` `2 passed; 0 failed`.

- [x] **5. Run the node's Acceptance check exactly as written.** `cargo test -p zygo-usage && cargo test --workspace --locked --no-run` → exit 0; every `zygo-usage` test passes, including `bytes_trimmed_equals_summary_for_the_session`, `bytes_trimmed_query_uses_the_session_index` and `query::tests::bytes_trimmed_runs_the_indexed_statement`; every workspace test target compiles; `Cargo.lock` is unchanged.

- [x] **6. Commit** `crates/zygo-usage/src/query.rs` and `crates/zygo-usage/tests/store.rs` with this message:

  ```
  feat(usage): add Store::bytes_trimmed, an indexed per-session trimmed-bytes read

  H§7.4: the status line reads the trimmed count on every call (AU4) with one
  SUM on the trims_session index, not through summary's unindexed filter form.

  Zygo-Session: <the executor's own zygo session id>
  Zygo-Work: HUD-1
  ```

### Interfaces

**Consumes** (existing, unchanged):
- `zygo_usage::Store::open(path: &Path) -> anyhow::Result<Store>` — `crates/zygo-usage/src/store.rs:66`
- `zygo_usage::Store::open_read_only(path: &Path) -> anyhow::Result<Store>` — `store.rs:111`
- `zygo_usage::Store::insert_trim(&self, t: &TrimRow) -> anyhow::Result<()>` — `store.rs:197`
- `zygo_usage::Store::summary(&self, f: &Filter) -> anyhow::Result<Summary>` — `query.rs:138`
- `fn u64_at(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<u64>` (private) — `query.rs:43`

**Produces:**
- `impl Store { pub fn bytes_trimmed(&self, session: &str) -> anyhow::Result<u64> }` — `crates/zygo-usage/src/query.rs`; consumed by HUD-9 (worker 3) and HUD-12.
- `#[doc(hidden)] pub const zygo_usage::query::BYTES_TRIMMED_SQL: &str` — test seam only; no other node uses it.

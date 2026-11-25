# OS-7 — Savings read: net figures, per-context carry, breakdown and ratio

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§6.2, O§6.3 step 1 and rule "OpenCode", O§6.4 "Context of the carry", O§6.5 item 5, O§6.6 (keys, ratio, filter), O§10 rows "No request rows for an event" and "A model with no price row", O§16 item 13
**Files:** `crates/zygo-usage/src/savings.rs`, `crates/zygo-usage/src/lib.rs`, `crates/zygo-usage/tests/savings.rs`
**Consumes:** `carry::carry` (OS-6), `Rates` (OS-6), `CtxRequest` (OS-6), `ReadbackRow` (OS-2), `TrimRow.result_id` (OS-2), `TrimRow.command` (OS-2), `TrimRow.shaper` (OS-2), `Store::insert_readback` (OS-2)
**Produces:** `Store::savings(&self, f: &Filter, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> anyhow::Result<Savings>`, `Savings { bytes_read_back: u64, kept_out_bytes: i64, kept_out_tokens: i64, spend_usd: f64, unpriced: u64, views: u64, readbacks: u64, by_key: Vec<SavingsKey> }`, `SavingsKey { tool: String, command: String, shaper: String, views: u64, readbacks: u64, kept_out_bytes: i64, spend_usd: f64 }` (`-` for null; sorted by `kept_out_bytes`, largest first)
**Acceptance:** `cargo test -p zygo-usage --test savings` → all pass, one test per O§16 item 13 bullet (worked example `spend_usd` = 0.1146 within 1e-9 and `kept_out_tokens == 9000`; growing small context; cut gives 0; subagent trim carried only by its own rows; `auxiliary` row carries nothing; OpenCode session counts its first row only; S1/S2 read-back: filter S1 counts it, filter S2 shows no negative figure, the carry uses S2's main-loop rows; doubled catalog rates double `spend_usd`), plus: a database built from the `97d8767` schema (no `readbacks`, seven `trims` columns) opened with `Store::open_read_only` gives `bytes_read_back == 0` and the key `Bash - -`; `unpriced` counts distinct rowids

## Plan

**Premises checked against the live code (branch `phase3/OS-7` at `390a6606`):**
- `crates/zygo-usage/src/` has `carry.rs`, `lib.rs`, `query.rs`, `schema.rs`, `store.rs`; no `savings` module. `lib.rs` re-exports `Filter` (`query.rs:9`) and `Carry, CtxRequest, Rates` (`carry.rs`). `Store.conn` is `pub(crate)` (`store.rs:12`), so `savings.rs` adds an `impl Store` block, as `query.rs` does.
- `carry::carry` (`carry.rs:37`) already does O§6.3 steps 2-4 and returns `usd_per_token` (rates ÷ 10^6) and the `unpriced` rowids. OS-7 reuses it; it does not price rows itself. `T_e` is applied here: `spend_e = bytes ÷ BYTES_PER_TOKEN × usd_per_token` (`zygo_core::tokens::BYTES_PER_TOKEN`, `crates/zygo-core/src/tokens.rs:4`, value 4).
- `TrimRow.result_id`, `.command`, `.shaper` (`schema.rs:145-151`), `ReadbackRow` (`schema.rs:156`), `Store::insert_readback` (`store.rs:302`), `Store::insert_trim` (`store.rs:286`, ten columns), `Store::open_read_only` (`store.rs:172`, checks only `user_version`) exist as the contract says.
- OpenCode's `requests.agent` value is `zygo_core::Agent::OpenCode.as_str()` = `"opencode"` (`crates/zygo-core/src/lib.rs:32`); reused, not retyped.
- `rusqlite` 0.43 (`Cargo.lock`) has `FromSql for u64` that refuses a negative value, so token columns read as `u64` with no new helper (`query.rs`'s private `u64_at` is not needed).
- The `97d8767` schema equals today's `SCHEMA` without the three `trims` columns and without the `readbacks` table and its two indexes (`git show 97d8767:crates/zygo-usage/src/schema.rs` lines 6-77 diffed against `schema.rs:5-90`: only those two hunks). The test builds that database by running `SCHEMA` and then dropping exactly those parts, instead of a second 70-line copy of the old text (`tests/store.rs` keeps one; a test binary cannot share it).

**Decisions (author, from the spec text):**
- The view to read-back ratio is not a field: `Produces` fixes `Savings`, and OS-8 computes `views ÷ readbacks` (O§6.6).
- `unpriced` = the number of distinct rowids over every event's `Carry.unpriced` (Acceptance; O§6.6 "requests had no price").
- OpenCode (O§6.3 rule): a context's session is OpenCode when any of its `requests` rows has `agent = 'opencode'`; then `first_only = true`. `Q_C` is the main-loop set of step 1 (its rows have no agent id).
- Carry context (O§6.4): a trim → `(trim.session, trim.agent_id)`. A read-back → `(read_in, trim.agent_id)` when `read_in == trim.session`, else `(read_in, None)`; no trim row with its `result_id` (or no `result_id` column) → `(read_in, None)`, key `- - -`.
- Readers use `PRAGMA table_info(trims)` and `sqlite_master`, as O§6.5 item 5 says. A missing column is selected as `NULL`.
- `by_key`: grouped in a `BTreeMap` on `(tool, command, shaper)`, then a stable sort by `kept_out_bytes` descending. Equal amounts stay in key order, so the output is deterministic.
- `spend_usd` and each key's `spend_usd` = Σ trim `spend_e` − Σ read-back `spend_e` (O§6.3 step 5). `kept_out_bytes` = Σ `max(stored − view, 0)` − Σ read-back bytes. `kept_out_tokens = signum(n) × ceil(|n| ÷ 4)` (O§6.2).
- All reads are in one deferred transaction (`unchecked_transaction`, as `summary`, `query.rs:146`), so all figures come from one snapshot. `Q_C` and the OpenCode test are cached per context. The filter selects events only; `Q_C` is read without it (O§6.6).

**Review focus (graph item 22, OS-7 part):** "a read-back keyed by the reading session" → `savings_readback_in_s2_of_s1_result`; "OpenCode subagent rows carrying a main-loop trim" → `savings_opencode_trim_counts_first_row_only`; "an unpriced request counted twice" → `savings_unpriced_counts_distinct_rowids`.

**Interfaces.**
Consumes (as in the code today):
```rust
// crates/zygo-usage/src/carry.rs
pub struct Rates { pub input: f64, pub cache_read: f64, pub write: f64 }
pub struct CtxRequest { pub rowid: i64, pub ts: i64, pub model: String, pub provider: String, pub input_tokens: u64,
    pub cache_read_tokens: u64, pub cache_write_tokens: u64, pub compaction: bool }
pub struct Carry { pub usd_per_token: f64, pub unpriced: Vec<i64> }
pub fn carry(event_ts: i64, q: &[CtxRequest], first_only: bool, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Carry
// crates/zygo-usage/src/schema.rs
pub struct ReadbackRow { pub ts: i64, pub session: String, pub read_in: String, pub result_id: String, pub tool: String, pub bytes: u64 }
pub struct TrimRow { /* … */ pub result_id: Option<String>, pub command: Option<String>, pub shaper: Option<String> }
// crates/zygo-usage/src/store.rs
pub fn insert_readback(&self, r: &ReadbackRow) -> Result<()>
```
Produces (`crates/zygo-usage/src/savings.rs`):
```rust
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Savings { pub bytes_read_back: u64, pub kept_out_bytes: i64, pub kept_out_tokens: i64, pub spend_usd: f64,
    pub unpriced: u64, pub views: u64, pub readbacks: u64, pub by_key: Vec<SavingsKey> }
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SavingsKey { pub tool: String, pub command: String, pub shaper: String, pub views: u64, pub readbacks: u64,
    pub kept_out_bytes: i64, pub spend_usd: f64 }   // command, shaper: "-" for null
impl Store {
    pub fn savings(&self, f: &Filter, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> anyhow::Result<Savings>
}
```

### Steps

- [x] **Step 1: failing tests.** Create `crates/zygo-usage/tests/savings.rs`. Rates: `m1` is the O§6.3 test row; `m2` is ten times it, so a wrong context gives a different figure.
  ```rust
  use std::path::PathBuf;
  use rusqlite::Connection;
  use zygo_core::Agent;
  use zygo_usage::*;

  const R: Rates = Rates { input: 4.0, cache_read: 0.4, write: 5.0 };
  const RID: &str = "01J00000000000000000000000";
  fn rates(m: &str, _: &str, _: u64) -> Option<Rates> {
      match m { "m1" => Some(R), "m2" => Some(Rates { input: 40.0, cache_read: 4.0, write: 50.0 }), _ => None }
  }
  struct Tmp(PathBuf);   // as tests/store.rs:7-16
  impl Tmp { fn new() -> Tmp { let d = std::env::temp_dir().join(format!("zygo-savings-{}", zygo_core::ids::ulid()));
      std::fs::create_dir_all(&d).unwrap(); Tmp(d) } fn db(&self) -> PathBuf { self.0.join("usage.db") } }
  impl Drop for Tmp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
  fn row(s: &str, ts: i64, agent_id: Option<&str>, class: Option<&str>, model: &str, cr: u64, cw: u64) -> RequestRow {
      RequestRow { ts, session: s.into(), agent: "claude-code".into(), route: "/anthropic/v1/messages".into(),
          agent_id: agent_id.map(Into::into), request_class: class.map(Into::into), compaction: class == Some("compaction"),
          model: model.into(), provider: "p1".into(), status: 200, input_tokens: 100, cache_read_tokens: cr,
          cache_write_tokens: cw, ..Default::default() }
  }
  fn req(st: &Store, s: &str, ts: i64, agent_id: Option<&str>, class: Option<&str>, model: &str, cr: u64, cw: u64) {
      st.insert_request(&row(s, ts, agent_id, class, model, cr, cw)).unwrap();
  }
  fn trim(s: &str, ts: i64, agent_id: Option<&str>, kept: u64, rid: Option<&str>) -> TrimRow {
      TrimRow { ts, session: s.into(), agent_id: agent_id.map(Into::into), tool: "Bash".into(), kind: "shaped".into(),
          stored_bytes: kept + 100, view_bytes: 100, result_id: rid.map(Into::into), command: Some("cargo test".into()),
          shaper: Some("libtest".into()) }
  }
  fn rb(s: &str, read_in: &str, ts: i64, bytes: u64, rid: &str) -> ReadbackRow {
      ReadbackRow { ts, session: s.into(), read_in: read_in.into(), result_id: rid.into(), tool: "result_view".into(), bytes }
  }
  fn sess(s: &str) -> Filter { Filter { session: Some(s.into()), since: None } }
  fn near(a: f64, b: f64) -> bool { (a - b).abs() < 1e-9 }
  /// Two growing main-loop rows (write, then read: 5.4 per Mtok with m1) and two subagent `a1` rows with `sub_model`.
  fn two_contexts(st: &Store, s: &str, ts: [i64; 2], main_model: &str, sub_model: &str) {
      for (t, cr) in [(ts[0], 1_000), (ts[1], 2_000)] {
          req(st, s, t, None, None, main_model, cr, 1_000);
          req(st, s, t, Some("a1"), None, sub_model, cr, 1_000);
      }
  }
  /// O§6.3 worked example in session "S": the rows of tests/carry.rs `example()`, the trim (40,000 bytes) at 100,
  /// the read-back (4,000 bytes) at 155.
  fn worked_example(st: &Store) {
      for (ts, cr, cw) in [(90, 10_000, 2_000), (110, 10_000, 12_000), (120, 22_000, 1_000), (130, 23_000, 1_000),
          (140, 24_000, 1_000), (150, 25_000, 1_000), (160, 0, 27_000), (170, 27_000, 500), (180, 27_500, 100)] {
          req(st, "S", ts, None, if ts == 170 { Some("compaction") } else { None }, "m1", cr, cw);
      }
      st.insert_trim(&trim("S", 100, None, 40_000, Some(RID))).unwrap();
      st.insert_readback(&rb("S", "S", 155, 4_000, RID)).unwrap();
  }
  fn key(tool: &str, command: &str, shaper: &str, views: u64, readbacks: u64, kept: i64, usd: f64) -> SavingsKey {
      SavingsKey { tool: tool.into(), command: command.into(), shaper: shaper.into(), views, readbacks, kept_out_bytes: kept, spend_usd: usd }
  }
  ```
  Tests (each opens `let t = Tmp::new(); let st = Store::open(&t.db()).unwrap();` and seeds the rows first). Tests 1-8 are the O§16 item 13 bullets; 9-14 are the rest of the Acceptance, the O§10 rows and O§6.4/O§6.6.
  1. `savings_worked_example_is_0_1146_and_9000_tokens`: `worked_example`; `v = st.savings(&Filter::default(), &rates)`:
     `assert!(near(v.spend_usd, 0.1146)); assert_eq!((v.kept_out_bytes, v.kept_out_tokens, v.bytes_read_back, v.views, v.readbacks, v.unpriced), (36_000, 9_000, 4_000, 1, 1, 0)); assert_eq!(v.by_key.len(), 1); assert_eq!((v.by_key[0].tool.as_str(), v.by_key[0].command.as_str(), v.by_key[0].shaper.as_str(), v.by_key[0].views, v.by_key[0].readbacks, v.by_key[0].kept_out_bytes), ("Bash", "cargo test", "libtest", 1, 1, 36_000)); assert!(near(v.by_key[0].spend_usd, 0.1146));`
  2. `savings_growing_context_reads_after_the_first_row`: main rows of session "G" `(ts, cr, cw)` = `(10, 100, 1_000), (20, 1_100, 2_000), (30, 3_100, 4_000), (40, 7_100, 8_000)`, m1; trim at 5, kept 4,000,000 (10^6 tokens): `assert!(near(v.spend_usd, 5.0 + 3.0 * 0.4));`
  3. `savings_cut_before_the_first_request_adds_0`: main rows of "C" `(10, 40_000, 10_000), (30, 0, 20_000), (40, 20_000, 1_000)`, m1; trim at 20, kept 40,000; filter `Filter { session: Some("C".into()), since: Some(20) }` (the row at 10 is outside the filter and still decides the cut): `assert_eq!(v.spend_usd, 0.0); assert_eq!(v.kept_out_tokens, 10_000);`
  4. `savings_subagent_trim_carried_by_own_rows_only`: `two_contexts(&st, "S", [10, 20], "m2", "m1")`; trim at 5, `agent_id` `a1`, kept 4,000,000: `assert!(near(v.spend_usd, 5.4));` (main-loop rows would give 54).
  5. `savings_auxiliary_row_carries_nothing`: main rows of "A": `(10, None, m1, 1_000, 1_000)`, `(20, Some("auxiliary"), m2, 0, 100)`, `(30, Some("main"), m1, 2_000, 1_000)`; trim at 5, kept 4,000,000: `assert!(near(v.spend_usd, 5.4));` (an included auxiliary row cuts at 200 < 2_100 ÷ 2 and gives 5.0).
  6. `savings_opencode_trim_counts_first_row_only`: the rows of case 2 in session "O", each inserted as `st.insert_request(&RequestRow { agent: Agent::OpenCode.as_str().into(), ..row("O", ts, None, None, "m1", cr, cw) })`; trim at 5, kept 4,000,000: `assert!(near(v.spend_usd, 5.0));`
  7. `savings_readback_in_s2_of_s1_result`: S1 subagent rows `req(&st, "S1", 110, Some("a1"), None, "m1", 1_000, 1_000); req(&st, "S1", 120, Some("a1"), None, "m1", 2_000, 1_000)`; S1 main rows `req(&st, "S1", 210, None, None, "m2", 1_000, 1_000); req(&st, "S1", 220, None, None, "m2", 2_000, 1_000)`; `two_contexts(&st, "S2", [210, 220], "m1", "m2")`; trim `("S1", 100, Some("a1"), 40_000, Some(RID))`; read-back `rb("S1", "S2", 200, 4_000, RID)`.
     `let v1 = st.savings(&sess("S1"), &rates).unwrap(); assert!(near(v1.spend_usd, 0.054 - 0.0054)); assert_eq!((v1.readbacks, v1.bytes_read_back, v1.kept_out_bytes, v1.kept_out_tokens), (1, 4_000, 36_000, 9_000));`
     `assert_eq!(st.savings(&sess("S2"), &rates).unwrap(), Savings::default());` (no negative figure: S2 has no event).
  8. `savings_doubled_rates_double_spend`: `worked_example`; `let double = |m: &str, _: &str, _: u64| (m == "m1").then_some(Rates { input: 8.0, cache_read: 0.8, write: 10.0 });` `assert!(near(v2.spend_usd, 0.2292)); assert!(near(v2.spend_usd, 2.0 * v1.spend_usd));`
  9. `savings_97d8767_database_reads_dash_keys`: on a fresh path, `Connection::open`, `execute_batch(SCHEMA)`, then `execute_batch("DROP TABLE readbacks; ALTER TABLE trims DROP COLUMN result_id; ALTER TABLE trims DROP COLUMN command; ALTER TABLE trims DROP COLUMN shaper; PRAGMA user_version = 1")`, then the seven-column insert of `tests/store.rs:119` (`(5, 'S1', NULL, 'Bash', 'head', 1000, 100)`); `let v = Store::open_read_only(&db).unwrap().savings(&Filter::default(), &rates).unwrap();`
     `assert_eq!(v.bytes_read_back, 0); assert_eq!(v.by_key, vec![key("Bash", "-", "-", 1, 0, 900, 0.0)]);`
  10. `savings_unpriced_counts_distinct_rowids`: main rows of "U" `(10, m1, 1_000, 1_000), (20, "unknown", 2_000, 1_000), (30, m1, 3_000, 1_000)`; trims at 5 and 6, each kept 4,000,000: `assert_eq!(v.unpriced, 1); assert!(near(v.spend_usd, 2.0 * 5.4));`
  11. `savings_no_request_rows_adds_0_keeps_tokens` (O§10): trim of "N" at 5, kept 40,000, no request: `assert_eq!((v.spend_usd, v.kept_out_tokens, v.unpriced), (0.0, 10_000, 0));`
  12. `savings_readback_without_trim_row_is_dash_key_in_main_loop`: `two_contexts(&st, "R", [10, 20], "m1", "m2")`; read-back `rb("R", "R", 5, 4_000, RID)`, no trim: `assert!(near(v.spend_usd, -0.0054)); assert_eq!((v.kept_out_bytes, v.kept_out_tokens), (-4_000, -1_000)); assert_eq!(v.by_key.len(), 1); assert_eq!((v.by_key[0].tool.as_str(), v.by_key[0].command.as_str(), v.by_key[0].shaper.as_str(), v.by_key[0].readbacks), ("-", "-", "-", 1));`
  13. `savings_readback_in_trim_session_carried_by_trim_context`: `two_contexts(&st, "S", [10, 20], "m2", "m1")`; trim `("S", 5, Some("a1"), 40_000, Some(RID))`; read-back `rb("S", "S", 15, 4_000, RID)`: `assert!(near(v.spend_usd, 0.054 - 0.005));` (the main loop would give 0.004).
  14. `savings_by_key_sorted_largest_first`: session "K", no request; trims at 1: `trim("K", 1, None, 100, None)` with `command: Some("git diff"), shaper: Some("diff")`; `trim("K", 1, None, 300, None)`; `trim("K", 1, None, 200, None)` with `tool: "Read", command: None, shaper: Some("size")`; read-back `rb("K", "K", 1, 50, RID)`:
      `assert_eq!(v.by_key, vec![key("Bash", "cargo test", "libtest", 1, 0, 300, 0.0), key("Read", "-", "size", 1, 0, 200, 0.0), key("Bash", "git diff", "diff", 1, 0, 100, 0.0), key("-", "-", "-", 0, 1, -50, 0.0)]);`
- [x] **Step 2: run and see it fail.** `env -u ZYGO_SESSION_DIR cargo test -p zygo-usage --locked --test savings` → compile error: unresolved `Savings`, `SavingsKey`, and no method `savings` on `Store`.
- [x] **Step 3: implement.** Create `crates/zygo-usage/src/savings.rs` with the two structs and `Store::savings` of `Interfaces`. In `lib.rs` add `pub mod savings;` and `pub use savings::{Savings, SavingsKey};`. Body, in one `self.conn.unchecked_transaction()`:
  1. `cols` = names from `PRAGMA table_info(trims)`. For each of `result_id`, `command`, `shaper`, put the name in the select if `cols` has it, else `NULL`. `has_rb` = `SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'readbacks'` is 1.
  2. Trims: `SELECT ts, session, agent_id, tool, MAX(stored_bytes - view_bytes, 0), {result_id}, {command}, {shaper} FROM trims WHERE (?1 IS NULL OR session = ?1) AND (?2 IS NULL OR ts >= ?2) ORDER BY ts, rowid` with `[f.session, f.since]`. Each row is an event: `+bytes`, context `(session, agent_id)`, key `(tool, command or "-", shaper or "-")`, `views += 1`.
  3. Read-backs, when `has_rb`: `SELECT ts, session, read_in, result_id, bytes FROM readbacks WHERE (?1 IS NULL OR session = ?1) AND (?2 IS NULL OR ts >= ?2) ORDER BY ts, rowid`. For each, when `cols` has `result_id`: `SELECT session, agent_id, tool, {command}, {shaper} FROM trims WHERE result_id = ?1 ORDER BY rowid LIMIT 1`. Context and key follow the O§6.4 decision above. Event: `−bytes`, `readbacks += 1`, `bytes_read_back += bytes`.
  4. `Q_C` per context, cached in a `HashMap<(String, Option<String>), (Vec<CtxRequest>, bool)>`: `SELECT rowid, ts, model, provider, input_tokens, cache_read_tokens, cache_write_tokens, compaction FROM requests WHERE session = ?1 AND status BETWEEN 200 AND 299 AND CASE WHEN ?2 IS NULL THEN agent_id IS NULL AND (request_class IS NULL OR request_class IN ('main', 'compaction')) ELSE agent_id = ?2 END ORDER BY ts, rowid`. The token columns are read as `u64`. The flag is `SELECT EXISTS (SELECT 1 FROM requests WHERE session = ?1 AND agent = ?2)` with `Agent::OpenCode.as_str()`.
  5. Per event: `c = carry::carry(ts, &q, first_only, rates)`; `spend_e = ±(|bytes| as f64 / BYTES_PER_TOKEN as f64) × c.usd_per_token`. Add `c.unpriced` to a `BTreeSet<i64>`. Add bytes and `spend_e` to the totals and to the key's entry.
  6. `kept_out_tokens = n.signum() * n.unsigned_abs().div_ceil(BYTES_PER_TOKEN) as i64`. `unpriced = set.len() as u64`. Sort `by_key` as the decision above says.
- [x] **Step 4: run and see it pass.** `env -u ZYGO_SESSION_DIR cargo test -p zygo-usage --locked --test savings` → 14 passed, 0 failed. Then `env -u ZYGO_SESSION_DIR cargo test -p zygo-usage --locked` → `carry`, `store` and unit tests also pass (no regression).
- [x] **Step 5: commit.** Stage only `crates/zygo-usage/src/savings.rs`, `crates/zygo-usage/src/lib.rs` and `crates/zygo-usage/tests/savings.rs`. Message `OS-7: savings read`, then a blank line and the `Zygo-Session:` and `Zygo-Work:` trailers from the implementing unit's brief. No other trailer.
- [x] **Step 6: verification.** `env -u ZYGO_SESSION_DIR cargo test -p zygo-usage --locked --test savings` → all 14 tests pass.

### Deviations from the plan

- **`u64` columns (Step 3.4).** The premise "rusqlite 0.43 has `FromSql for u64`" is wrong: that impl needs the `fallible_uint` feature, which the workspace does not enable, so `r.get::<u64>` does not compile. Fix: `query.rs`'s `u64_at` (a negative value is a conversion error, not a wrap) is now `pub(crate)` and `savings.rs` reads the three token columns with it. `crates/zygo-usage/src/query.rs` is therefore in the Step 5 commit too.
- **Trim select (Step 3.2).** The trims query does not select `result_id`: a trim event does not use it. Only the read-back lookup reads that column.
- **Test helper.** `req` takes eight parameters, so it carries `#[allow(clippy::too_many_arguments)]`.

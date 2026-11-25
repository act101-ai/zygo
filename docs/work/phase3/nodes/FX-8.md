# FX-8 — zygo usage reads summary and savings from one snapshot

**Spec:** — (found in the OS-8 review, F3). `crates/zygo/src/cmd/usage.rs` calls `st.summary(&f)?` and then `st.savings(&f, &price)?` (one line, `Ok((st.summary(&f)?, st.savings(&f, &price)?))`); each opens its own read transaction (`crates/zygo-usage/src/query.rs:146`, `crates/zygo-usage/src/savings.rs:142`). A trim or read-back written between the two makes `bytes trimmed` and the savings lines describe different row sets. O§6.6 prints them as one report.
**Files:** `crates/zygo-usage/src/query.rs`, `crates/zygo-usage/src/savings.rs`, `crates/zygo-usage/tests/report_snapshot.rs` (new), `crates/zygo/src/cmd/usage.rs` (plan: `crates/zygo-usage/src/lib.rs` dropped; `Summary` and `Savings` are already re-exported at lib.rs:9 and lib.rs:11)
**Consumes:** `Store::summary(&self, f: &Filter) -> Result<Summary>` (existing, crates/zygo-usage/src/query.rs:142), `Store::savings(&self, f: &Filter, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Result<Savings>` (OS-7, crates/zygo-usage/src/savings.rs:140), `Filter { session: Option<String>, since: Option<i64> }` (existing, query.rs), `Summary` (existing, query.rs), `Savings` (OS-7, savings.rs), `Rates { input: f64, cache_read: f64, write: f64 }` (OS-6, carry.rs)
**Produces:** `Store::report(&self, f: &Filter, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Result<(Summary, Savings)>` (crates/zygo-usage/src/savings.rs): both figures from one read transaction
**Acceptance:** `cargo test -p zygo-usage --test report_snapshot` → all pass: a trim row written by a second connection while the report runs (from inside the rates callback) shows in neither the summary nor the savings figures; and `cargo test -p zygo --test usage_doctor` still passes with no expected line changed.

## Plan

Premises checked against the live code at 8381a2ff:

- `Store::summary` (query.rs:142) opens `self.conn.unchecked_transaction()` at query.rs:146 and calls `tx.finish()` before it parses the guard-event JSON.
- `Store::savings` (savings.rs:140) opens `self.conn.unchecked_transaction()` at savings.rs:142 and drops it (rollback) at return. Its helpers `has_table`, `context`, `trim_events` and `readback_events` already take `&Transaction<'_>`.
- `rates` is `&dyn Fn(&str, &str, u64) -> Option<Rates>`. `savings` passes it to `carry::carry` (carry.rs:38), which calls it once for each carrying request of each event. `savings` reads all trim and read-back events before the first call (savings.rs:145-148).
- usage.rs:145 calls `Store::open_read_only(..).and_then(|st| Ok((st.summary(&f)?, st.savings(&f, &price)?)))`. `price` is a closure that `&price` coerces to the `dyn Fn`.
- No shared transaction helper exists. The two call sites above and `prune` (store.rs:363) each call `self.conn.unchecked_transaction()` directly. The plan adds no helper.
- `Store::open` sets WAL (store.rs:135). A probe on this commit showed this: a second `Store::open` connection can write a trim row from inside the rates callback while an `open_read_only` read transaction is open. The callback was called once. With `savings` then `summary` (two transactions), `kept_out_bytes` was 4,000,000 and `bytes_trimmed` was 4,001,000.

Test hook: the callback is the only point where code runs during the report. It runs inside the savings part, after its event reads. So `report` reads savings first and the summary second, in the same transaction. The summary's `trims` read then comes after the second connection's write, and only one snapshot keeps the write out. This order does not change any figure (the two parts are independent). It is also the order in which the two-transaction stub fails the test deterministically.

### Steps

- [x] 1. Create `crates/zygo-usage/tests/report_snapshot.rs` with this test:

```rust
use std::cell::Cell;
use std::path::PathBuf;
use zygo_usage::*;

const R: Rates = Rates { input: 4.0, cache_read: 0.4, write: 5.0 };
struct Tmp(PathBuf);
impl Drop for Tmp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
fn trim(ts: i64, kept: u64) -> TrimRow {
    TrimRow { ts, session: "S".into(), agent_id: None, tool: "Bash".into(), kind: "shaped".into(),
        stored_bytes: kept + 100, view_bytes: 100, result_id: None, command: Some("cargo test".into()),
        shaper: Some("libtest".into()) }
}

/// FX-8: a trim written by a second connection while `report` runs (from inside the rates callback) shows in
/// neither the summary nor the savings figures.
#[test]
fn report_reads_summary_and_savings_from_one_snapshot() {
    let t = Tmp(std::env::temp_dir().join(format!("zygo-report-{}", zygo_core::ids::ulid())));
    let db = t.0.join("usage.db");
    let w = Store::open(&db).unwrap();
    // One main-loop request at 10 (rate `write` = 5.0 per Mtok) carries the 4,000,000-byte trim at 5.
    w.insert_request(&RequestRow { ts: 10, session: "S".into(), agent: "claude-code".into(),
        route: "/anthropic/v1/messages".into(), model: "m1".into(), provider: "p1".into(), status: 200,
        input_tokens: 100, cache_write_tokens: 1_000, ..Default::default() }).unwrap();
    w.insert_trim(&trim(5, 4_000_000)).unwrap();
    let r = Store::open_read_only(&db).unwrap();
    let calls = Cell::new(0u32);
    let price = |m: &str, _: &str, _: u64| {
        if calls.replace(calls.get() + 1) == 0 {
            w.insert_trim(&trim(6, 1_000)).unwrap(); // second connection, committed mid-report
        }
        (m == "m1").then_some(R)
    };
    let (s, v) = r.report(&Filter::default(), &price).unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!((s.requests, s.input_tokens, s.cache_write_tokens), (1, 100, 1_000));
    assert_eq!(s.bytes_trimmed, 4_000_000);
    assert_eq!((v.kept_out_bytes, v.kept_out_tokens, v.views, v.readbacks, v.unpriced), (4_000_000, 1_000_000, 1, 0, 0));
    assert!((v.spend_usd - 5.0).abs() < 1e-9, "{}", v.spend_usd);
    assert_eq!(v.by_key.len(), 1);
    assert_eq!(v.by_key[0].kept_out_bytes, 4_000_000);
    // Control: the write committed; a new read sees it.
    assert_eq!(r.summary(&Filter::default()).unwrap().bytes_trimmed, 4_001_000);
}
```

  Check: the file compiles only after step 2 (`report` does not exist yet).

- [x] 2. In `crates/zygo-usage/src/savings.rs` `impl Store`, add the stub that reproduces today's two transactions in the planned order:

```rust
/// O§6.6: the summary and the savings of `f` from one read snapshot (FX-8).
pub fn report(&self, f: &Filter, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Result<(Summary, Savings)> {
    let v = self.savings(f, rates)?;
    Ok((self.summary(f)?, v))
}
```

  Change the import `query::{u64_at, Filter}` to `query::{u64_at, Filter, Summary}`. Run `cargo test -p zygo-usage --test report_snapshot`. Check: it FAILS at `assert_eq!(s.bytes_trimmed, 4_000_000)` with left `4001000`.

- [x] 3. In `crates/zygo-usage/src/query.rs`, move the body of `Store::summary` into a free function. The SQL and the result stay the same:

```rust
/// The `Summary` of `f`, read through `tx` (one snapshot).
pub(crate) fn summary_in(tx: &rusqlite::Transaction<'_>, f: &Filter) -> Result<Summary>
```

  The body is the current summary body from `tx.query_row(..)` to `Ok(sum)`, without `unchecked_transaction`. Remove `drop((groups, q)); tx.finish()?;` (the statements drop at the end of their scope). `Store::summary` becomes:

```rust
pub fn summary(&self, f: &Filter) -> Result<Summary> {
    // One deferred read transaction: every part reads the same snapshot.
    let tx = self.conn.unchecked_transaction()?;
    let sum = summary_in(&tx, f)?;
    tx.finish()?;
    Ok(sum)
}
```

  Check: `cargo test -p zygo-usage --test store` passes.

- [x] 4. In `crates/zygo-usage/src/savings.rs`, move the body of `Store::savings` after `unchecked_transaction` into:

```rust
/// O§6.6: the savings of `f`, read through `tx` (one snapshot); the filter selects events only.
fn savings_in(tx: &Transaction<'_>, f: &Filter, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Result<Savings>
```

  `Store::savings` becomes `let tx = self.conn.unchecked_transaction()?; savings_in(&tx, f, rates)`. Check: `cargo test -p zygo-usage --test savings` passes.

- [x] 5. Add `summary_in` to the `crate::query` import in savings.rs. Replace the body of `Store::report` (step 2) with one transaction, savings first:

```rust
// One deferred read transaction. Savings first: `rates` runs inside it, so the summary reads after
// the callback and only the shared snapshot keeps a concurrent write out of both.
let tx = self.conn.unchecked_transaction()?;
let v = savings_in(&tx, f, rates)?;
let s = summary_in(&tx, f)?;
tx.finish()?;
Ok((s, v))
```

  Run `cargo test -p zygo-usage --test report_snapshot`. Check: it passes.

- [x] 6. In `crates/zygo/src/cmd/usage.rs:145`, replace `.and_then(|st| Ok((st.summary(&f)?, st.savings(&f, &price)?)))` with `.and_then(|st| st.report(&f, &price))`. Check: `cargo test -p zygo --test usage_doctor` passes with no test file changed (`git diff --stat crates/zygo/tests` is empty).

- [x] 7. Run the regression set. Check: all pass, no warnings:
  `cargo test -p zygo-usage`; `cargo test -p zygo-hooks` (`summary` callers: statusline/mod.rs:58, tests/hooks_basic.rs:113); `cargo clippy -p zygo-usage -p zygo --all-targets -- -D warnings`.

- [x] 8. Commit the four files (`git add` each path; `git commit -m`), subject `FX-8: one read snapshot for zygo usage summary and savings`, with the trailers `Zygo-Session:` (the implementing session's id) and `Zygo-Work: phase3:FX-8:implement`, each on its own line. Check: `git show --stat HEAD` lists only query.rs, savings.rs, tests/report_snapshot.rs and cmd/usage.rs.

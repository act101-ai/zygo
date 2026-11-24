# OS-6 — Carry computation for one event over its context's requests

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§6.3 (steps 1-4, the rules, the worked example), O-C5
**Files:** `crates/zygo-usage/src/carry.rs`, `crates/zygo-usage/src/lib.rs`, `crates/zygo-usage/tests/carry.rs`
**Consumes:** —
**Produces:** `Rates { input: f64, cache_read: f64, write: f64 }` (USD per Mtok; `write` = `cache_write_5m`, absent: `cache_write`), `CtxRequest { rowid: i64, ts: i64, model: String, provider: String, input_tokens: u64, cache_read_tokens: u64, cache_write_tokens: u64, compaction: bool }`, `carry::carry(event_ts: i64, q: &[CtxRequest], first_only: bool, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Carry`, `Carry { usd_per_token: f64, unpriced: Vec<i64> }` (`q` is Q_C in `ts`, `rowid` order; the closure takes model, provider and prompt tokens; `unpriced` holds rowids)
**Acceptance:** `cargo test -p zygo-usage --test carry` → all pass, including: the O§6.3 worked example gives `usd_per_token × 10_000 = 0.12` and, for the read-back after request 5, `usd_per_token × 1_000 = 0.0054` (each within 1e-12); a context whose cache-write tokens exceed its cache-read tokens on each row and whose cache-read tokens never fall prices each later row at `cache_read`; a first row whose prompt is under half of the prompt of the row before the event gives an empty carry; a `compaction` row is taken and ends the carry; an event in the same second as request A starts at A; `first_only` takes one row; a row with no cache tokens is priced at `input`; a row whose closure returns `None` adds 0 and its rowid is in `unpriced`; doubled rates give twice `usd_per_token`

## Plan

**Premises checked against the live code (branch `phase3/OS-6` at `80d4381`):**

- `crates/zygo-usage/src/` has `lib.rs` (7 lines: `pub mod query; schema; store;` and three `pub use`), `query.rs`, `schema.rs`, `store.rs`. No rates, pricing or carry type exists in the crate (`grep -rn "carry\|Rates" crates/zygo-usage/src`: only a doc comment at `store.rs:214`). `RequestRow` (`schema.rs:88`) has no `rowid`, so `CtxRequest` is a new type; OS-7 fills it from `requests` (`rowid` and the columns `schema.rs:14-28`).
- Prompt rule: `crates/zygo-proxy/src/window.rs:89-91` is `input.saturating_add(cache_read).saturating_add(cache_write)`, as O§6.3 step 1 says. `carry.rs` repeats it (a private three-term `fn`; `zygo-usage` does not depend on `zygo-proxy`, and `zygo-proxy` depends on `zygo-usage`).
- Tier rule: `Prices::cost` is `crates/zygo-config/src/schema.rs:282-299` (doc `:276-281`); the spec cites `:280-289` — line drift only, the rule (all rates from `long_context` when prompt `> above`; 5-minute writes at `cache_write_5m`, else `cache_write`) is unchanged. `zygo-usage` does not depend on `zygo-config`; the contract puts the catalog lookup and the tier choice in the caller's closure, which gets the prompt. No `Cargo.toml` change.

**Decisions (author):**

- `Rates`, `CtxRequest`, `Carry` live in `carry.rs`; `lib.rs` adds `pub mod carry;` and `pub use carry::{Carry, CtxRequest, Rates};`, so OS-7 names them as `crate::Rates` etc. and calls `carry::carry`.
- `usd_per_token = Σ rates of R_e ÷ 10^6` (rates are USD per Mtok); OS-7 multiplies by `T_e`.
- Rate order per row: no cache tokens → `input` first, then first row of `R_e` → `write`, then expiry → `write`, else `cache_read` (O§6.3 step 3 lists the no-cache case first; the Acceptance says such a row "is priced at `input`").
- The cut on the first row has nothing to compare with when the event is before the first row of `Q_C`: no cut.
- "Less than half" is `prompt × 2 < prev_prompt`, in `u64` with `saturating_mul` (a saturated product is never less, so no false cut).
- `first_only` keeps the cut test on its one row (O§6.3 "OpenCode" changes only how many rows are taken).
- A `None` row still counts for the cut, the expiry compare of the next row and the compaction end; it only adds 0. Its rowid is pushed once (one loop visit per row).
- `Rates` is `Copy`; mapping `Prices` → `Rates` (`write = cache_write_5m.unwrap_or(cache_write)`, tier by prompt) is the caller's (OS-7's caller, the CLI); O-C5 holds because no rate constant is in `carry.rs`.

**Review focus (graph item 22, OS-6 part):** "the cut compared with the wrong previous row" → `cut_compares_with_the_row_just_before`; "an expiry on a growing context" → `growing_context_reads_after_the_first_row`; "an unpriced request counted twice" → `unpriced_row_adds_zero_and_is_listed_once`. The read-back key and the OpenCode subagent case are OS-7's.

**Interfaces.** Consumes: —. Produces (`crates/zygo-usage/src/carry.rs`):

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rates { pub input: f64, pub cache_read: f64, pub write: f64 }   // USD per Mtok; write = cache_write_5m, absent: cache_write
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CtxRequest { pub rowid: i64, pub ts: i64, pub model: String, pub provider: String, pub input_tokens: u64,
    pub cache_read_tokens: u64, pub cache_write_tokens: u64, pub compaction: bool }
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Carry { pub usd_per_token: f64, pub unpriced: Vec<i64> }
pub fn carry(event_ts: i64, q: &[CtxRequest], first_only: bool, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Carry
```

### Steps

- [x] **Step 1 — failing tests.** Create `crates/zygo-usage/tests/carry.rs`:

  ```rust
  use std::cell::RefCell;
  use zygo_usage::{carry::carry, Carry, CtxRequest, Rates};

  const R: Rates = Rates { input: 4.0, cache_read: 0.4, write: 5.0 };    // O§6.3 test row: cache_write_5m 5, cache_write 8
  fn flat(_: &str, _: &str, _: u64) -> Option<Rates> { Some(R) }
  fn row(rowid: i64, ts: i64, cr: u64, cw: u64) -> CtxRequest {
      CtxRequest { rowid, ts, model: "m1".into(), provider: "p1".into(), input_tokens: 100, cache_read_tokens: cr,
          cache_write_tokens: cw, ..Default::default() }
  }
  /// O§6.3 worked example: one row before the trim (ts 100), requests 1-7 at ts 110..170, 7 the compaction, one row after.
  fn example() -> Vec<CtxRequest> {
      let mut q = vec![row(1, 90, 10_000, 2_000), row(2, 110, 10_000, 12_000), row(3, 120, 22_000, 1_000),
          row(4, 130, 23_000, 1_000), row(5, 140, 24_000, 1_000), row(6, 150, 25_000, 1_000), row(7, 160, 0, 27_000),
          row(8, 170, 27_000, 500), row(9, 180, 27_500, 100)];
      q[7].compaction = true;
      q
  }
  fn near(a: f64, b: f64) -> bool { (a - b).abs() < 1e-12 }

  #[test]
  fn worked_example_trim_is_0_12() {
      let c = carry(100, &example(), false, &flat);
      assert!(near(c.usd_per_token * 10_000.0, 0.12), "{}", c.usd_per_token * 10_000.0);
      assert!(c.unpriced.is_empty());
  }

  #[test]
  fn worked_example_read_back_after_request_5_is_0_0054() {
      let c = carry(155, &example(), false, &flat);
      assert!(near(c.usd_per_token * 1_000.0, 0.0054), "{}", c.usd_per_token * 1_000.0);
  }

  #[test]
  fn growing_context_reads_after_the_first_row() {     // cache write > cache read on each row, cache read never falls
      let q = [row(1, 10, 100, 1_000), row(2, 20, 1_100, 2_000), row(3, 30, 3_100, 4_000), row(4, 40, 7_100, 8_000)];
      assert!(near(carry(5, &q, false, &flat).usd_per_token * 1e6, 5.0 + 3.0 * 0.4));
  }

  #[test]
  fn cut_before_the_first_row_gives_an_empty_carry() {
      let q = [row(1, 10, 40_000, 10_000), row(2, 30, 0, 20_000), row(3, 40, 20_000, 1_000)];   // 20_100 < 50_100 / 2
      assert_eq!(carry(20, &q, false, &flat), Carry::default());
  }

  #[test]
  fn cut_compares_with_the_row_just_before() {
      // before the event: prompts 100_100 then 40_100; first row 30_100 >= 40_100 / 2 (not cut against 100_100)
      // then 100_100, then 45_100 < 100_100 / 2 (cut against the row just before, not against the first row 30_100)
      let q = [row(1, 10, 90_000, 10_000), row(2, 20, 0, 40_000), row(3, 40, 0, 30_000), row(4, 50, 30_000, 70_000),
          row(5, 60, 45_000, 0)];
      assert!(near(carry(30, &q, false, &flat).usd_per_token * 1e6, 5.0 + 0.4));
  }

  #[test]
  fn compaction_row_is_taken_and_ends_the_carry() {
      let mut q = vec![row(1, 10, 1_000, 1_000), row(2, 20, 2_000, 1_000), row(3, 30, 3_000, 1_000)];
      q[0].compaction = true;
      assert!(near(carry(5, &q, false, &flat).usd_per_token * 1e6, 5.0));
  }

  #[test]
  fn event_in_the_same_second_as_a_starts_at_a() {
      let q = [row(1, 100, 1_000, 1_000), row(2, 110, 2_000, 1_000)];
      assert!(near(carry(100, &q, false, &flat).usd_per_token * 1e6, 5.0 + 0.4));
      assert!(near(carry(101, &q, false, &flat).usd_per_token * 1e6, 5.0));
  }

  #[test]
  fn first_only_takes_one_row() {
      let q = [row(1, 10, 100, 1_000), row(2, 20, 1_100, 2_000), row(3, 30, 3_100, 4_000)];
      assert!(near(carry(5, &q, true, &flat).usd_per_token * 1e6, 5.0));
  }

  #[test]
  fn row_with_no_cache_tokens_is_priced_at_input() {
      let q = [row(1, 10, 0, 0), row(2, 20, 0, 0)];
      assert!(near(carry(5, &q, false, &flat).usd_per_token * 1e6, 4.0 + 4.0));
  }

  #[test]
  fn unpriced_row_adds_zero_and_is_listed_once() {
      let mut q = vec![row(1, 10, 1_000, 1_000), row(42, 20, 2_000, 1_000), row(3, 30, 3_000, 1_000)];
      q[1].model = "unknown".into();
      let rates = |m: &str, _: &str, _: u64| (m == "m1").then_some(R);
      let c = carry(5, &q, false, &rates);
      assert!(near(c.usd_per_token * 1e6, 5.0 + 0.4));
      assert_eq!(c.unpriced, vec![42]);
  }

  #[test]
  fn doubled_rates_give_twice_usd_per_token() {     // O-C5: rates come only from the closure
      let double = |_: &str, _: &str, _: u64| Some(Rates { input: 8.0, cache_read: 0.8, write: 10.0 });
      let (one, two) = (carry(100, &example(), false, &flat), carry(100, &example(), false, &double));
      assert!(near(two.usd_per_token * 1e6, 2.0 * one.usd_per_token * 1e6));
  }

  #[test]
  fn closure_gets_model_provider_and_prompt_of_each_carrying_row() {
      let seen = RefCell::new(Vec::new());
      let rates = |m: &str, p: &str, prompt: u64| { seen.borrow_mut().push((m.to_string(), p.to_string(), prompt)); Some(R) };
      carry(155, &example(), false, &rates);
      assert_eq!(*seen.borrow(), vec![("m1".to_string(), "p1".to_string(), 27_100u64), ("m1".to_string(), "p1".to_string(), 27_600)]);
  }

  #[test]
  fn event_after_the_last_row_gives_an_empty_carry() {
      assert_eq!(carry(200, &example(), false, &flat), Carry::default());
      assert_eq!(carry(0, &[], false, &flat), Carry::default());
  }
  ```

  Result: the file exists with 13 tests.

- [x] **Step 2 — see it fail.** `cargo test -p zygo-usage --test carry` → compile error `E0432` (unresolved import `zygo_usage::carry`).

- [x] **Step 3 — implement.** Create `crates/zygo-usage/src/carry.rs` with the three types of **Interfaces** and:

  ```rust
  fn prompt(r: &CtxRequest) -> u64 { r.input_tokens.saturating_add(r.cache_read_tokens).saturating_add(r.cache_write_tokens) }

  /// O§6.3 steps 2-4 for one event: the sum of the carrying requests' rates in USD per token (`T_e` not applied).
  /// `q` is `Q_C` in `ts`, `rowid` order; `first_only` is the OpenCode rule.
  pub fn carry(event_ts: i64, q: &[CtxRequest], first_only: bool, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Carry {
      let mut c = Carry::default();
      let Some(start) = q.iter().position(|r| r.ts >= event_ts) else { return c };
      let mut sum = 0.0;
      for i in start..q.len() {
          let (r, prev) = (&q[i], i.checked_sub(1).map(|j| &q[j]));
          if prev.is_some_and(|p| prompt(r).saturating_mul(2) < prompt(p)) { break }
          match rates(&r.model, &r.provider, prompt(r)) {
              None => c.unpriced.push(r.rowid),
              Some(x) if r.cache_read_tokens == 0 && r.cache_write_tokens == 0 => sum += x.input,
              Some(x) if i == start || prev.is_some_and(|p| r.cache_read_tokens < p.cache_read_tokens) => sum += x.write,
              Some(x) => sum += x.cache_read,
          }
          if r.compaction || first_only { break }
      }
      c.usd_per_token = sum / 1e6;
      c
  }
  ```

  In `crates/zygo-usage/src/lib.rs`: add `pub mod carry;` before `pub mod query;` and `pub use carry::{Carry, CtxRequest, Rates};` after `pub use query::{Filter, RoleModel, Summary};`.

- [x] **Step 4 — see it pass.** `cargo test -p zygo-usage --test carry` → 13 passed, 0 failed.

- [x] **Step 5 — no regression.** `cargo test -p zygo-usage` → all pass (the `store` tests unchanged).

- [x] **Step 6 — commit.** `git add crates/zygo-usage/src/carry.rs crates/zygo-usage/src/lib.rs crates/zygo-usage/tests/carry.rs`; commit `OS-6: carry computation` with trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:OS-6:implement`; `git log -1` shows the three files only.

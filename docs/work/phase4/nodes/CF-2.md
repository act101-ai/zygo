# CF-2 — Conformance derived values: cell, tier and total score, bar, rollup, release state

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.4, K§6.2.6 (the example numbers; release state), K§6.2.7 last paragraph (release state text), K§11 items 5 and 6; K-D7, K-D8, K-D9, K-D18
**Files:** `crates/zygo-core/tests/conformance.rs`, `crates/zygo-core/tests/conformance/model.rs`, `crates/zygo-core/tests/conformance/derive.rs` (new), `crates/zygo-core/tests/conformance/fixtures/`
**Consumes:** `Registry::load` (CF-1), `GradeFile::load` (CF-1), `model::Client` (CF-1), `model::check` (CF-1)
**Produces:** `derive::cell(reg: &Registry, g: &GradeFile, capability: &str) -> Cell` (`Cell { grade: Grade, value: Option<f64>, missing: Vec<Unmet>, na: Vec<Unmet> }`), `derive::scores(reg: &Registry, g: &GradeFile) -> Scores` (`total`, `core`, `standard`, `extra`: `Option<u32>` percent, rounded down last), `derive::bar(reg: &Registry, g: &GradeFile) -> Bar` (`Bar { meets: bool, failing: Vec<String>, exceptions: Vec<String> }`), `derive::rollup(reg: &Registry, g: &GradeFile, supported: bool) -> Rollup`
**Acceptance:** `test "$(cargo test -q -p zygo-core --test conformance -- derive_ --list 2>/dev/null | grep -c ': test$')" -ge 9 && cargo test -p zygo-core --test conformance --locked` → at least 9 tests named `derive_…` (a count of 0 fails), then the whole `conformance` target passes on ubuntu, macOS and Windows CI: per-tier and total full / partial / none / n/a and met / applicable counts for the fixture clients; `below bar` with the capability id for an unmet core criterion with no exception; `meets bar` with a registry exception; tier score = mean of cell values; total = weighted mean rounded down last; a tier with no applicable cell is `n/a` and leaves the weights; the K§6.2.6 example gives 96, 75, 94, 89; an all-`n/a` cell is `n/a`; release state `supported`, `meets bar, not released`, `below bar`

## Planning notes

- `model.rs` is in Files because check item 8 moves from CF-1's minimal bar test to `derive::bar`; CF-1's item 8 case stays green.
- The release state is derived from `supported` and the bar only (K§6.2.6 item 2). The launch gate, the hidden help and `ZYGO_PREVIEW_CLIENT` are P4-6 (K§6.2.7, K§9), not this graph.

## Plan

Base: `phase-4--CF-1` at `20661a57` (in review). If CF-1 changes in a later round, re-check the Consumes below before step 1.

### Premise check (live code, `phase-4--CF-1` @ 20661a57)

- Holds. `crates/zygo-core/tests/conformance.rs:1-4` is the test crate root: `#[allow(dead_code)] #[path = "conformance/model.rs"] mod model;` and `use model::*;`. CF-2 adds `derive` the same way. Baseline: `cargo test -p zygo-core --test conformance --locked` → 34 passed; `derive_` tests: 0.
- `model.rs` defines no `Grade`, `Unmet`, `Cell`, `Bar` or `Rollup`. CF-1 kept the names `Cell` and `Grade` free for CF-2 (CF-1 plan, "Interfaces"). CF-2 defines them in `derive.rs`.
- Check item 8 (`model.rs`, `check`, comment `// 8. a supported client meets the bar`) calls the private `failing_core(reg, g)`, doc line `CF-2 replaces this with derive::bar(reg, g).failing`. `failing_core` accepts a criterion that is `met`, or that has a registry exception with any status. K§6.2.4 accepts an exception only for a `not-met` criterion. CF-2's `bar` follows K§6.2.4. The CF-1 tests stay green: `check_4_reason_and_core_na` (core `n/a`, no exception → items `{4, 8}`), `check_8_supported_client_below_bar`, `check_8_registry_exception_meets_bar`.
- `f64` floor gives wrong results for small inputs: three cells 1/1, 1/4, 2/5 have the exact mean 0.55 (55 %), but `f64` gives 54.99999999999999 → 54. K§6.2.4 says "rounded down … on unrounded values", so `scores` uses exact integer fractions (step 5).

### Interfaces

Consumes (exact, `crates/zygo-core/tests/conformance/model.rs` @ 20661a57):

```rust
impl Registry { pub fn load(path: &Path) -> Result<Registry, String>; pub fn parse(text: &str) -> Result<Registry, String>; }
impl GradeFile { pub fn load(path: &Path) -> Result<GradeFile, String>; pub fn parse(text: &str) -> Result<GradeFile, String>; }
pub fn check(root: &Path, reg: &Registry, grades: &[GradeFile], clients: &[Client]) -> Vec<Fault>;
pub struct Client { pub id: String, pub name: String, pub version: String, pub supported: bool }
pub struct Registry { pub capability: Vec<Capability>, #[serde(default)] pub bar_exception: Vec<BarException> }
pub struct Capability { pub id: String, pub name: String, pub tier: Tier, pub standard: String, pub criterion: Vec<Criterion> }
pub struct Criterion { pub id: String, pub text: String }
pub enum Tier { Core, Standard, Extra }   // Clone, Copy, PartialEq, Eq
pub struct BarException { pub client: String, pub criterion: String, pub reason: String, pub approved: Option<String> }
pub struct GradeFile { pub client: String, pub name: String, pub version: String, pub cell: Vec<CellUrl>, pub grade: Vec<GradeEntry> }
pub struct CellUrl { pub capability: String, pub url: String }
pub struct GradeEntry { pub criterion: String, pub status: Status, pub evidence: Vec<String>, pub reason: Option<String> }
pub enum Status { Met, NotMet, Na }       // Clone, Copy, PartialEq, Eq
```

Produces (new file `crates/zygo-core/tests/conformance/derive.rs`; each type derives `Debug, Clone, PartialEq`; `Grade` also `Copy, Eq`):

```rust
pub enum Grade { Full, Partial, None, Na }    // always written `Grade::None`; never glob-imported
pub struct Unmet { pub criterion: String, pub text: String, pub reason: Option<String> }
pub struct Cell { pub grade: Grade, pub value: Option<f64>, pub missing: Vec<Unmet>, pub na: Vec<Unmet> }
pub struct Scores { pub total: Option<u32>, pub core: Option<u32>, pub standard: Option<u32>, pub extra: Option<u32> }
pub struct Bar { pub meets: bool, pub failing: Vec<String>, pub exceptions: Vec<String> }
pub struct Counts { pub full: u32, pub partial: u32, pub none: u32, pub na: u32, pub met: u32, pub applicable: u32 }
pub struct Rollup { pub core: Counts, pub standard: Counts, pub extra: Counts, pub total: Counts, pub bar: Bar, pub release: &'static str }

pub fn cell(reg: &Registry, g: &GradeFile, capability: &str) -> Cell;   // panics on an id not in reg (a caller bug)
pub fn scores(reg: &Registry, g: &GradeFile) -> Scores;
pub fn bar(reg: &Registry, g: &GradeFile) -> Bar;
pub fn rollup(reg: &Registry, g: &GradeFile, supported: bool) -> Rollup;
```

`Unmet` holds the keys of a K§6.2.6 JSON `missing` entry (`criterion`, `text`, `reason`). `Counts` holds the keys of a JSON rollup row. CF-3 consumes the four functions and these types.

### Rules the code must hold (K§6.2.4, K§6.2.6 item 2, K§6.2.7 last paragraph, K-D7, K-D8, K-D9, K-D18)

- **Grade of a criterion in `g`:** the first `GradeEntry` with that `criterion`. A criterion with no grade counts as not met, with `reason: None`. (Check item 2 already faults it. A missing grade never raises a score.)
- **Cell (K-D7):** applicable = criteria of the capability that are not `n/a`; met = criteria that are `met`. `value` = `Some(met / applicable)` when applicable > 0, else `None`. `grade`: applicable = 0 → `Na`; met = applicable → `Full`; met = 0 → `None`; else `Partial`. `missing` = one `Unmet` per applicable criterion that is not met (its registry `text`, its grade's `reason`), in registry order. `na` = one `Unmet` per `n/a` criterion, in registry order.
- **Scores (K-D8):** tier score = mean of `met / applicable` over the tier's capabilities with applicable > 0. If the tier has no such capability, the tier is `None` and leaves the weights. Total = Σ(w × tier score) ÷ Σ w over the tiers that are not `None`; weights core 3, standard 2, extra 1. All tiers `None` → total `None`. Percent = ⌊100 × x⌋, computed once per shown score from the exact unrounded fraction. A tier score is never rounded before the total.
- **Bar (K-D9, K-D18):** a criterion of a core capability counts when it is `met`, or `not-met` with a registry `bar_exception` whose `(client, criterion)` = `(g.client, id)`. `failing` = the ids, in registry order, of each core capability with a criterion that does not count (`not-met` with no exception, `n/a`, or no grade). `meets` = `failing.is_empty()`. `exceptions` = the `criterion` of each registry `bar_exception` with `client == g.client`, in registry order.
- **Rollup:** per tier, over the tier's capabilities: `full`, `partial`, `none`, `na` = the count of cells of each grade; `met`, `applicable` = the sums of the criterion counts. `total` = the field-wise sum of the three tiers. `release` (K§6.2.6 item 2, K§6.2.7): `!bar.meets` → `"below bar"`; `bar.meets && supported` → `"supported"`; `bar.meets && !supported` → `"meets bar, not released"`.

### Expected values for the CF-1 fixtures (computed by hand)

Fixtures: `crates/zygo-core/tests/conformance/fixtures/registry.toml`, `alpha.toml`, `beta.toml` (A-1 core: A-1.1, A-1.2; A-3 standard: A-3.1; WA-2 extra: WA-2.1).

| client | A-1 (core) | A-3 (standard) | WA-2 (extra) | scores total / core / standard / extra |
|---|---|---|---|---|
| alpha | `Full`, 1.0 | `Full`, 1.0 | `Na`, `None` | 100 / 100 / 100 / n/a |
| beta | `Full`, 1.0 | `None`, 0.0 | `Full`, 1.0 | 66 / 100 / 0 / 100 |

Beta's total is (3 × 1 + 2 × 0 + 1 × 1) ÷ 6 = 0.666… → 66 (rounded down, not 67).

### Steps

- [ ] **1. Module and a synthetic-input helper.** Create `crates/zygo-core/tests/conformance/derive.rs` with the doc line `//! Derived values (K§6.2.4): cell, scores, bar, rollup and release state.`, `use crate::model::*;`, and the types and four signatures of "Interfaces", each body `todo!()`. In `conformance.rs`, after the `mod model;` block, add:

  ```rust
  #[allow(dead_code)] // CF-3 reads the rest
  #[path = "conformance/derive.rs"]
  mod derive;
  ```
  and `use derive::{Bar, Cell, Counts, Grade, Scores, Unmet};`. Add the test helper `fn synth(caps: &[(Tier, &[Status])]) -> (Registry, GradeFile)`. Capability `i` (1-based, list order) is `A-{i}`, name `c`, standard `s`. Its criterion `j` (1-based) is `A-{i}.{j}`, text `t{i}.{j}`. No bar exceptions. The grade file: client `synth`, name `Synth`, version `1.0.0`, one `CellUrl` per capability with url `https://example.com/{i}`, one grade per criterion with the given status, evidence empty, reason `Some("r{i}.{j}")` for `NotMet` and `Na`, `None` for `Met`. Check: `cargo test -p zygo-core --test conformance --no-run --locked` exits 0.

- [ ] **2. Cell tests (failing).** Add to `conformance.rs`:

  ```rust
  #[test]
  fn derive_cell_fixtures_full_none_na() {
      let (r, g, _) = base();
      assert_eq!(derive::cell(&r, &g[0], "A-1"), Cell { grade: Grade::Full, value: Some(1.0), missing: vec![], na: vec![] });
      assert_eq!(derive::cell(&r, &g[0], "WA-2"), Cell { grade: Grade::Na, value: None, missing: vec![],
          na: vec![Unmet { criterion: "WA-2.1".into(), text: "Turn start.".into(), reason: Some("Alpha has no turn events".into()) }] });
      assert_eq!(derive::cell(&r, &g[1], "A-3"), Cell { grade: Grade::None, value: Some(0.0),
          missing: vec![Unmet { criterion: "A-3.1".into(), text: "The picker shows the role list.".into(), reason: Some("Beta has no model picker".into()) }],
          na: vec![] });
  }

  #[test]
  fn derive_cell_partial_lists_missing() {
      let (r, mut g, _) = base();
      { let e = entry(&mut g[0], "A-1.2"); e.status = Status::NotMet; e.reason = Some("no key helper".into()); }
      assert_eq!(derive::cell(&r, &g[0], "A-1"), Cell { grade: Grade::Partial, value: Some(0.5),
          missing: vec![Unmet { criterion: "A-1.2".into(), text: "The credential is the session token.".into(), reason: Some("no key helper".into()) }],
          na: vec![] });
  }

  #[test]
  fn derive_cell_na_criteria_leave_value_and_ungraded_is_unmet() {
      let (r, g) = synth(&[(Tier::Extra, &[Status::Met, Status::Na, Status::NotMet][..]), (Tier::Extra, &[Status::Na, Status::Na][..])]);
      let c = derive::cell(&r, &g, "A-1");
      assert_eq!((c.grade, c.value, c.missing.len(), c.na.len()), (Grade::Partial, Some(0.5), 1, 1));
      let c = derive::cell(&r, &g, "A-2"); // an all-n/a cell is n/a
      assert_eq!((c.grade, c.value, c.na.len()), (Grade::Na, None, 2));
      let (r, mut g) = synth(&[(Tier::Standard, &[Status::Met, Status::Met][..])]);
      g.grade.retain(|e| e.criterion != "A-1.2");
      assert_eq!(derive::cell(&r, &g, "A-1").missing, vec![Unmet { criterion: "A-1.2".into(), text: "t1.2".into(), reason: None }]);
  }
  ```
  Run `cargo test -p zygo-core --test conformance derive_cell` → the 3 tests fail (panic `not yet implemented`).

- [ ] **3. Implement `derive::cell`** by the "Cell" rule. Run `cargo test -p zygo-core --test conformance derive_cell` → 3 passed.

- [ ] **4. Score tests (failing).** Add:

  ```rust
  fn pct(t: Option<u32>, c: Option<u32>, s: Option<u32>, e: Option<u32>) -> Scores {
      Scores { total: t, core: c, standard: s, extra: e }
  }

  #[test]
  fn derive_scores_fixtures() {
      let (r, g, _) = base();
      assert_eq!(derive::scores(&r, &g[0]), pct(Some(100), Some(100), Some(100), None));
      assert_eq!(derive::scores(&r, &g[1]), pct(Some(66), Some(100), Some(0), Some(100))); // 4/6 rounds down
  }

  #[test]
  fn derive_scores_na_tier_leaves_weights() {
      let (r, mut g, _) = base();
      { let e = entry(&mut g[0], "A-1.2"); e.status = Status::NotMet; e.reason = Some("no key helper".into()); }
      // (3 × 0.5 + 2 × 1) ÷ 5 = 0.7; an n/a extra tier counted as 0 with weight 1 would give 58
      assert_eq!(derive::scores(&r, &g[0]), pct(Some(70), Some(50), Some(100), None));
      let (r, g) = synth(&[(Tier::Extra, &[Status::Na][..])]);
      assert_eq!(derive::scores(&r, &g), pct(None, None, None, None));
  }

  #[test]
  fn derive_scores_mean_of_cells_exact_floor() {
      use Status::{Met as M, NotMet as N};
      // cell values 1/1, 1/4, 2/5: exact mean 0.55 → 55 (f64 arithmetic gives 54)
      let (r, g) = synth(&[(Tier::Standard, &[M][..]), (Tier::Standard, &[M, N, N, N][..]), (Tier::Standard, &[M, M, N, N, N][..])]);
      assert_eq!(derive::scores(&r, &g), pct(Some(55), None, Some(55), None));
  }

  /// K§6.2.6: core 7 full + 1 at 3/4 (A-1.4 not-met, excepted); standard 12 cells summing to 9;
  /// extra 5/6, 1, 1 and one all-n/a cell.
  fn spec_example() -> (Registry, GradeFile) {
      use Status::{Met as M, Na as X, NotMet as N};
      let mut caps: Vec<(Tier, &[Status])> = vec![(Tier::Core, &[M, M, M, N][..])];
      caps.extend([(Tier::Core, &[M, M][..]); 5]);
      caps.extend([(Tier::Core, &[M, M, M][..]); 2]);
      caps.extend([(Tier::Standard, &[M][..]); 9]);
      caps.extend([(Tier::Standard, &[N][..]); 3]);
      caps.extend([(Tier::Extra, &[M, M, M, M, M, N][..]), (Tier::Extra, &[M][..]), (Tier::Extra, &[M][..]), (Tier::Extra, &[X, X][..])]);
      let (mut r, g) = synth(&caps);
      r.bar_exception.push(BarException { client: "synth".into(), criterion: "A-1.4".into(), reason: "r".into(), approved: Some("operator".into()) });
      (r, g)
  }

  #[test]
  fn derive_scores_spec_example_rounds_last() {
      let (r, g) = spec_example();
      assert_eq!(derive::cell(&r, &g, "A-1").value, Some(0.75));
      // tiers rounded first would give (3 × 96 + 2 × 75 + 94) ÷ 6 = 88.67 → 88 (review focus 3)
      assert_eq!(derive::scores(&r, &g), pct(Some(89), Some(96), Some(75), Some(94)));
  }
  ```
  Run `cargo test -p zygo-core --test conformance derive_scores` → the 4 tests fail (`not yet implemented`).

- [ ] **5. Implement `derive::scores`** by the "Scores" rule. Use a private exact fraction: numerator and denominator `u128`, reduced by their gcd after each operation. Tier score = (Σ met_i/app_i) ÷ n; total = (Σ w × S) ÷ Σ w; percent = `(100 * num / den) as u32` (integer division, so ⌊·⌋). `scores` uses no `f64`. Run `cargo test -p zygo-core --test conformance derive_scores` → 4 passed.

- [ ] **6. Bar tests (failing).** Add:

  ```rust
  fn exception(client: &str, criterion: &str) -> BarException {
      BarException { client: client.into(), criterion: criterion.into(), reason: "r".into(), approved: Some("operator".into()) }
  }

  #[test]
  fn derive_bar_below_then_exception_meets() {
      let (mut r, mut g, _) = base();
      assert_eq!(derive::bar(&r, &g[0]), Bar { meets: true, failing: vec![], exceptions: vec![] });
      { let e = entry(&mut g[0], "A-1.2"); e.status = Status::NotMet; e.reason = Some("no key helper".into()); }
      assert_eq!(derive::bar(&r, &g[0]), Bar { meets: false, failing: vec!["A-1".into()], exceptions: vec![] });
      r.bar_exception.push(exception("alpha", "A-1.2"));
      assert_eq!(derive::bar(&r, &g[0]), Bar { meets: true, failing: vec![], exceptions: vec!["A-1.2".into()] });
      assert!(derive::bar(&r, &g[1]).exceptions.is_empty()); // another client's exception
  }

  #[test]
  fn derive_bar_core_na_or_ungraded_fails_even_with_exception() {
      let (mut r, g0, _) = base();
      r.bar_exception.push(exception("alpha", "A-1.2"));
      let mut g = g0.clone();
      { let e = entry(&mut g[0], "A-1.2"); e.status = Status::Na; e.reason = Some("x".into()); }
      assert_eq!(derive::bar(&r, &g[0]).failing, ["A-1"]); // an exception counts only for not-met (K§6.2.4)
      let mut g = g0.clone();
      g[0].grade.retain(|e| e.criterion != "A-1.1");
      assert_eq!(derive::bar(&r, &g[0]).failing, ["A-1"]);
      let mut g = g0.clone();
      entry(&mut g[0], "WA-2.1").status = Status::NotMet; // a non-core gap never fails the bar
      assert!(derive::bar(&r, &g[0]).meets);
  }
  ```
  Run `cargo test -p zygo-core --test conformance derive_bar` → the 2 tests fail (`not yet implemented`).

- [ ] **7. Implement `derive::bar`** by the "Bar" rule. Run `cargo test -p zygo-core --test conformance derive_bar` → 2 passed.

- [ ] **8. Replace CF-1's minimal bar rule in check item 8.** In `model.rs`, item 8 uses `let failing = crate::derive::bar(reg, g).failing;`. The fault text stays `client `<id>` is supported and below the bar: <ids joined by ", ">`. Delete the private `failing_core` and its doc line. Check: `grep -rn failing_core crates/zygo-core/tests/` prints nothing; `cargo test -p zygo-core --test conformance check_` → all `check_…` tests pass, among them `check_4_reason_and_core_na` (`{4, 8}`), `check_8_supported_client_below_bar` and `check_8_registry_exception_meets_bar`, with no change to their text.

- [ ] **9. Rollup and release tests (failing).** Add:

  ```rust
  fn n(full: u32, partial: u32, none: u32, na: u32, met: u32, applicable: u32) -> Counts {
      Counts { full, partial, none, na, met, applicable }
  }

  #[test]
  fn derive_rollup_fixtures() {
      let (r, g, _) = base();
      let a = derive::rollup(&r, &g[0], true);
      assert_eq!((a.core, a.standard, a.extra, a.total), (n(1, 0, 0, 0, 2, 2), n(1, 0, 0, 0, 1, 1), n(0, 0, 0, 1, 0, 0), n(2, 0, 0, 1, 3, 3)));
      let b = derive::rollup(&r, &g[1], true);
      assert_eq!((b.core, b.standard, b.extra, b.total), (n(1, 0, 0, 0, 2, 2), n(0, 0, 1, 0, 0, 1), n(1, 0, 0, 0, 1, 1), n(2, 0, 1, 0, 3, 4)));
      assert_eq!(b.bar, derive::bar(&r, &g[1]));
  }

  #[test]
  fn derive_rollup_spec_example() {
      let (r, g) = spec_example();
      let x = derive::rollup(&r, &g, true);
      assert_eq!((x.core, x.standard, x.extra, x.total), (n(7, 1, 0, 0, 19, 20), n(9, 0, 3, 0, 9, 12), n(2, 1, 0, 1, 7, 8), n(18, 2, 3, 1, 35, 40)));
      assert_eq!(x.bar, Bar { meets: true, failing: vec![], exceptions: vec!["A-1.4".into()] });
      assert_eq!(x.release, "supported");
  }

  #[test]
  fn derive_release_state() {
      let (r, mut g, _) = base();
      assert_eq!(derive::rollup(&r, &g[0], true).release, "supported");
      assert_eq!(derive::rollup(&r, &g[0], false).release, "meets bar, not released");
      { let e = entry(&mut g[0], "A-1.2"); e.status = Status::NotMet; e.reason = Some("no key helper".into()); }
      assert_eq!(derive::rollup(&r, &g[0], false).release, "below bar");
      assert_eq!(derive::rollup(&r, &g[0], true).release, "below bar");
  }
  ```
  Run `cargo test -p zygo-core --test conformance derive_r` → the 3 tests fail (`not yet implemented`).

- [ ] **10. Implement `derive::rollup`** by the "Rollup" rule, built from `cell` and `bar`. Run `cargo test -p zygo-core --test conformance derive_` → 12 passed.

- [ ] **11. Acceptance and commit.** Run `test "$(cargo test -q -p zygo-core --test conformance -- derive_ --list 2>/dev/null | grep -c ': test$')" -ge 9 && cargo test -p zygo-core --test conformance --locked` → exit 0 (12 `derive_` tests; the whole target passes). Run `cargo test --workspace --locked --no-fail-fast` → exit 0. Run `cargo test -p zygo-core --test conformance --no-run --locked 2>&1 | grep -c '^warning'` → `0`. Commit `feat(conformance): derived cell, scores, bar, rollup (CF-2)`, ending with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:CF-2`. Push. CI is green on ubuntu, macOS and Windows.

### Coverage map (Acceptance → test)

| Acceptance item | Test |
|---|---|
| per-tier and total full / partial / none / n/a and met / applicable | `derive_rollup_fixtures`, `derive_rollup_spec_example` |
| `below bar` with the capability id; `meets bar` with a registry exception | `derive_bar_below_then_exception_meets`, `derive_bar_core_na_or_ungraded_fails_even_with_exception`, `check_8_*` |
| tier score = mean of cell values | `derive_scores_mean_of_cells_exact_floor`, `derive_scores_fixtures` |
| total = weighted mean, rounded down last (review focus 3: 89, not 88) | `derive_scores_spec_example_rounds_last`, `derive_scores_fixtures` (66) |
| a tier with no applicable cell is `n/a` and leaves the weights | `derive_scores_na_tier_leaves_weights` |
| the K§6.2.6 example gives 96, 75, 94, 89 | `derive_scores_spec_example_rounds_last` |
| an all-`n/a` cell is `n/a` | `derive_cell_fixtures_full_none_na`, `derive_cell_na_criteria_leave_value_and_ungraded_is_unmet` |
| release state `supported`, `meets bar, not released`, `below bar` | `derive_release_state`, `derive_rollup_spec_example` |
| `partial` lists the missing text; `none` gives the reasons (K§6.2.4) | `derive_cell_partial_lists_missing`, `derive_cell_fixtures_full_none_na` |

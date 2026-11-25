# CF-2 — Conformance derived values: cell, tier and total score, bar, rollup, release state

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.4, K§6.2.6 (the example numbers; release state), K§6.2.7 last paragraph (release state text), K§11 items 5 and 6; K-D7, K-D8, K-D9, K-D18
**Files:** `crates/zygo-core/tests/conformance.rs`, `crates/zygo-core/tests/conformance/model.rs`, `crates/zygo-core/tests/conformance/derive.rs` (new), `crates/zygo-core/tests/conformance/fixtures/`
**Consumes:** `model::Registry::load(path: &Path) -> Result<Registry, String>` (CF-1), `model::GradeFile::load(path: &Path) -> Result<GradeFile, String>` (CF-1), `model::Client { id: String, name: String, version: String, supported: bool }` (CF-1), `model::resolve(root: &Path, grades: &[GradeFile]) -> Resolved` (CF-1), `Resolved { grade: Vec<ResolvedGrade> }` (CF-1), `model::check(root: &Path, reg: &Registry, grades: &[GradeFile], clients: &[Client], resolved: &Resolved) -> Vec<Fault>` (CF-1), `Fault { item: u8, text: String }` (CF-1)
**Produces:** `derive::cell(reg: &Registry, g: &GradeFile, capability: &str) -> Cell` (`Cell { grade: Grade, value: Option<f64>, missing: Vec<Unmet>, na: Vec<Unmet> }`), `derive::scores(reg: &Registry, g: &GradeFile) -> Scores` (`total`, `core`, `standard`, `extra`: `Option<u32>` percent, rounded down last), `derive::bar(reg: &Registry, g: &GradeFile) -> Bar` (`Bar { meets: bool, failing: Vec<String>, exceptions: Vec<String> }`), `derive::rollup(reg: &Registry, g: &GradeFile, supported: bool) -> Rollup`
**Acceptance:** `test "$(cargo test -q -p zygo-core --test conformance -- derive_ --list 2>/dev/null | grep -c ': test$')" -ge 9 && cargo test -p zygo-core --test conformance --locked` → at least 9 tests named `derive_…` (a count of 0 fails), then the whole `conformance` target passes on ubuntu, macOS and Windows CI: per-tier and total full / partial / none / n/a and met / applicable counts for the fixture clients; `below bar` with the capability id for an unmet core criterion with no exception; `meets bar` with a registry exception; tier score = mean of cell values; total = weighted mean rounded down last; a tier with no applicable cell is `n/a` and leaves the weights; the K§6.2.6 example gives 96, 75, 94, 89; an all-`n/a` cell is `n/a`; release state `supported`, `meets bar, not released`, `below bar`

## Planning notes

- `model.rs` is in Files because check item 8 moves from CF-1's minimal bar test to `derive::bar`; CF-1's item 8 case stays green.
- The release state is derived from `supported` and the bar only (K§6.2.6 item 2). The launch gate, the hidden help and `ZYGO_PREVIEW_CLIENT` are P4-6 (K§6.2.7, K§9), not this graph.

## Plan

Base: `phase-4` at `ca8103e8` (CF-1 merged, PR #87, `74562e3b`).

### Premise check (live code, `phase-4` @ ca8103e8)

- Holds. `crates/zygo-core/tests/conformance.rs:2-4` is the test crate's only module: `#[allow(dead_code)] #[path = "conformance/model.rs"] mod model;`, then `use model::*;` at `:9`. No other file includes `model.rs`. CF-2 adds `derive` the same way. Baseline: `cargo test -p zygo-core --test conformance --locked` → 65 passed; `derive_` tests: 0.
- Test helpers CF-2 reuses (`conformance.rs`): `base() -> (Registry, Vec<GradeFile>, Vec<Client>)` at `:27` (fixtures `registry.toml`, `alpha.toml`, `beta.toml`; clients alpha and beta, both supported); `entry(g: &mut GradeFile, criterion: &str) -> &mut GradeEntry` at `:41`; `items(&[Fault]) -> BTreeSet<u8>` at `:33`; `set(&[u8])` at `:37`; `repo()` at `:15`. A check call is `check(&repo(), &r, &g, &c, &resolve(&repo(), &g))`.
- `model.rs` defines no `Grade`, `Unmet`, `Cell`, `Scores`, `Bar`, `Counts` or `Rollup` (it has `CellUrl`, not `Cell`). CF-2 defines them in `derive.rs`.
- Check item 8 (`model.rs:1052-1059`, comment `// 8. a supported client meets the bar`) calls the private `failing_core(reg, g)` (`model.rs:1071-1082`, doc line `/// CF-2 replaces this with derive::bar(reg, g).failing`). `failing_core` counts a core criterion that is `met`, or that has a registry exception for the client with a non-blank `approved` (`!blank(&b.approved)`), whatever its status. K§6.2.4 and K-D18 count an exception only for a `not-met` criterion, and only an operator-approved one. CF-2's `bar` holds both: an exception counts when the criterion is `not-met` and `approved` is not blank.
- CF-1 tests that pin item 8 and stay green with no edit: `check_4_reason_and_core_na` (`:574`; core `n/a`, no exception → items `{4, 8}`), `check_6_bar_exception_faults` (`:603`; an exception with `approved` `None` or `" "` on a `not-met` core criterion → `{6, 8}`; an approved exception on the `met` A-1.1 → `{6}` only), `check_6_duplicate_or_reasonless_bar_exception` (`:631`), `check_8_supported_client_below_bar` (`:716`; fault text contains `A-1`), `check_8_registry_exception_meets_bar` (`:731`).
- `blank(s: &Option<String>) -> bool` (`model.rs:915`) is private to `model`, so `derive` cannot call it. Step 7 makes it `pub fn blank`; `derive` and item 8 use one rule for "approved".
- `f64` floor gives wrong results for small inputs: three cells 1/1, 1/4, 2/5 have the exact mean 0.55 (55 %), but `f64` gives 54.99999999999999 → 54 (checked in Python). K§6.2.4 says the rounding is "the last step, on unrounded values", so `scores` uses exact integer fractions (step 5).
- The K§6.2.6 numbers, recomputed with exact fractions: core (7 + 3/4) ÷ 8 = 31/32 → 96; standard 9/12 → 75; extra (5/6 + 1 + 1) ÷ 3 = 17/18 → 94; total (3 × 31/32 + 2 × 3/4 + 17/18) ÷ 6 = 0.8918 → 89. Tier percents rounded first: (3 × 96 + 2 × 75 + 94) ÷ 6 = 88.67 → 88 (review focus 3).

### Interfaces

Consumes (exact, `crates/zygo-core/tests/conformance/model.rs` @ ca8103e8):

```rust
impl Registry { pub fn load(path: &Path) -> Result<Registry, String>; pub fn parse(text: &str) -> Result<Registry, String>; }
impl GradeFile { pub fn load(path: &Path) -> Result<GradeFile, String>; pub fn parse(text: &str) -> Result<GradeFile, String>; }
pub fn resolve(root: &Path, grades: &[GradeFile]) -> Resolved;
pub fn check(root: &Path, reg: &Registry, grades: &[GradeFile], clients: &[Client], resolved: &Resolved) -> Vec<Fault>;
pub struct Resolved { pub grade: Vec<ResolvedGrade> }
pub struct Fault { pub item: u8, pub text: String }
pub struct Client { pub id: String, pub name: String, pub version: String, pub supported: bool }
pub struct Registry { pub capability: Vec<Capability>, pub bar_exception: Vec<BarException> }   // bar_exception: #[serde(default)]
pub struct Capability { pub id: String, pub name: String, pub tier: Tier, pub standard: String, pub criterion: Vec<Criterion> }
pub struct Criterion { pub id: String, pub text: String }
pub enum Tier { Core, Standard, Extra }   // Clone, Copy, PartialEq, Eq
pub struct BarException { pub client: String, pub criterion: String, pub reason: String, pub approved: Option<String> }
pub struct GradeFile { pub client: String, pub name: String, pub version: String, pub cell: Vec<CellUrl>, pub grade: Vec<GradeEntry> }
pub struct CellUrl { pub capability: String, pub url: String }
pub struct GradeEntry { pub criterion: String, pub status: Status, pub evidence: Vec<String>, pub reason: Option<String> }
pub enum Status { Met, NotMet, Na }       // Clone, Copy, PartialEq, Eq
fn blank(s: &Option<String>) -> bool;     // private today; step 7 makes it pub
```

`derive` reads statuses only. It calls neither `resolve` nor `evidence`: a `met` grade whose evidence does not count is check item 3's fault, not a derived value. The tests call `resolve` only to pass its result to `check` (step 8).

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

`Unmet` holds the keys of a K§6.2.6 JSON `missing` entry (`criterion`, `text`, `reason`). `Counts` holds the keys of a JSON rollup row. CF-3 consumes the four functions by these names (`nodes/CF-3.md` Consumes) and these types.

### Rules the code must hold (K§6.2.4, K§6.2.6 item 2, K§6.2.7 last paragraph, K-D7, K-D8, K-D9, K-D18)

- **Grade of a criterion in `g`:** the first `GradeEntry` with that `criterion`. A criterion with no grade counts as not met, with `reason: None`. (Check item 2 already faults it. A missing grade never raises a score.)
- **Cell (K-D7):** applicable = criteria of the capability that are not `n/a`; met = criteria that are `met`. `value` = `Some(met as f64 / applicable as f64)` when applicable > 0, else `None`. `grade`: applicable = 0 → `Na`; met = applicable → `Full`; met = 0 → `None`; else `Partial`. `missing` = one `Unmet` per applicable criterion that is not met (its registry `text`, its grade's `reason`), in registry order. `na` = one `Unmet` per `n/a` criterion, in registry order. `value` is for display only; `scores` never reads it.
- **Scores (K-D8):** tier score = mean of `met / applicable` over the tier's capabilities with applicable > 0. If the tier has no such capability, the tier is `None` and leaves the weights. Total = Σ(w × tier score) ÷ Σ w over the tiers that are not `None`; weights core 3, standard 2, extra 1. All tiers `None` → total `None`. Percent = ⌊100 × x⌋, computed once per shown score from the exact unrounded fraction. A tier score is never rounded before the total.
- **Bar (K-D9, K-D18):** an exception is approved when `!blank(&b.approved)`. A criterion of a core capability counts when it is `met`, or `not-met` with an approved registry `bar_exception` whose `(client, criterion)` = `(g.client, id)`. `failing` = the ids, in registry order, of each core capability with a criterion that does not count (`not-met` with no approved exception, `n/a`, or no grade). `meets` = `failing.is_empty()`. `exceptions` = the `criterion` of each approved registry `bar_exception` with `client == g.client`, in registry order.
- **Rollup:** per tier, over the tier's capabilities: `full`, `partial`, `none`, `na` = the count of cells of each grade; `met`, `applicable` = the sums of the criterion counts. `total` = the field-wise sum of the three tiers. `bar` = `bar(reg, g)`. `release` (K§6.2.6 item 2, K§6.2.7): `!bar.meets` → `"below bar"`; `bar.meets && supported` → `"supported"`; `bar.meets && !supported` → `"meets bar, not released"`.

### Expected values for the CF-1 fixtures (computed by hand)

Fixtures: `crates/zygo-core/tests/conformance/fixtures/registry.toml`, `alpha.toml`, `beta.toml` (A-1 core: A-1.1, A-1.2; A-3 standard: A-3.1; WA-2 extra: WA-2.1; no bar exception).

| client | A-1 (core) | A-3 (standard) | WA-2 (extra) | scores total / core / standard / extra |
|---|---|---|---|---|
| alpha | `Full`, 1.0 | `Full`, 1.0 | `Na`, `None` | 100 / 100 / 100 / n/a |
| beta | `Full`, 1.0 | `None`, 0.0 | `Full`, 1.0 | 66 / 100 / 0 / 100 |

Beta's total is (3 × 1 + 2 × 0 + 1 × 1) ÷ 6 = 2/3 → 66 (rounded down, not 67).

### Steps

- [x] **1. Module and a synthetic-input helper.** Create `crates/zygo-core/tests/conformance/derive.rs` with the doc line `//! Derived values (K§6.2.4): cell, scores, bar, rollup and release state.`, `use crate::model::*;`, and the types and four signatures of "Interfaces", each body `todo!()`. In `conformance.rs`, after the `mod model;` block (`:2-4`), add:

  ```rust
  #[allow(dead_code)] // CF-3 reads the rest
  #[path = "conformance/derive.rs"]
  mod derive;
  ```
  and, after `use model::*;`, `use derive::{Bar, Cell, Counts, Grade, Scores, Unmet};`. Add the test helper `fn synth(caps: &[(Tier, &[Status])]) -> (Registry, GradeFile)`. Capability `i` (1-based, list order) is `A-{i}`, name `c`, standard `s`. Its criterion `j` (1-based) is `A-{i}.{j}`, text `t{i}.{j}`. No bar exceptions. The grade file: client `synth`, name `Synth`, version `1.0.0`, one `CellUrl` per capability with url `https://example.com/{i}`, one grade per criterion with the given status, evidence empty, reason `Some("r{i}.{j}")` for `NotMet` and `Na`, `None` for `Met`. Check: `cargo test -p zygo-core --test conformance --no-run --locked` exits 0, and `cargo test -p zygo-core --test conformance --locked` → 65 passed.

  Done: `--no-run` exit 0; the target 65 passed (temporary unused warnings until steps 3-10).

- [x] **2. Cell tests (failing).** Add to `conformance.rs`:

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

  Done: failed first: 3 failed, each `not yet implemented` in `derive.rs`.

- [x] **3. Implement `derive::cell`** by the "Cell" rule. Run `cargo test -p zygo-core --test conformance derive_cell` → 3 passed.

  Done: 3 passed.

- [x] **4. Score tests (failing).** Add:

  ```rust
  fn pct(t: Option<u32>, c: Option<u32>, s: Option<u32>, e: Option<u32>) -> Scores {
      Scores { total: t, core: c, standard: s, extra: e }
  }

  #[test]
  fn derive_scores_fixtures() {
      let (r, g, _) = base();
      assert_eq!(derive::scores(&r, &g[0]), pct(Some(100), Some(100), Some(100), None));
      assert_eq!(derive::scores(&r, &g[1]), pct(Some(66), Some(100), Some(0), Some(100))); // 2/3 rounds down
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

  /// K§6.2.6: core 7 full + 1 at 3/4 (A-1.4 not-met, approved exception); standard 12 cells summing to 9;
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
      let s = derive::scores(&r, &g);
      assert_eq!(s, pct(Some(89), Some(96), Some(75), Some(94)));
      // review focus 3: the tier percents rounded before the total give 88, not 89
      let (c, st, e) = (s.core.unwrap(), s.standard.unwrap(), s.extra.unwrap());
      assert_eq!((3 * c + 2 * st + e) / 6, 88);
  }
  ```
  Run `cargo test -p zygo-core --test conformance derive_scores` → the 4 tests fail (`not yet implemented`).

  Done: failed first: 4 failed, each `not yet implemented`.

- [x] **5. Implement `derive::scores`** by the "Scores" rule. Use a private exact fraction `struct Frac { num: u128, den: u128 }` (den > 0), reduced by the gcd after each add, multiply and divide. Tier score = (Σ met_i/app_i) ÷ n over the tier's applicable cells; total = (Σ w × tier) ÷ Σ w; percent = `(100 * num / den) as u32` (integer division, so ⌊·⌋), applied once to each of the four fractions. `scores` reads no `f64` and never calls `cell(..).value`. Run `cargo test -p zygo-core --test conformance derive_scores` → 4 passed.

  Done: 4 passed (89 / 96 / 75 / 94; rounded first 88; exact mean 55).

- [x] **6. Bar tests (failing).** Add:

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
  fn derive_bar_unapproved_exception_does_not_count() {
      let (mut r, mut g, _) = base();
      { let e = entry(&mut g[0], "A-1.2"); e.status = Status::NotMet; e.reason = Some("no key helper".into()); }
      for approved in [None, Some(" ".to_string())] {
          r.bar_exception = vec![BarException { approved, ..exception("alpha", "A-1.2") }];
          assert_eq!(derive::bar(&r, &g[0]), Bar { meets: false, failing: vec!["A-1".into()], exceptions: vec![] });
      }
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
  Run `cargo test -p zygo-core --test conformance derive_bar` → the 3 tests fail (`not yet implemented`).

  Done: failed first: 3 failed, each `not yet implemented`.

- [x] **7. Implement `derive::bar`** by the "Bar" rule. In `model.rs:915` change `fn blank` to `pub fn blank` (doc line `/// True when the text is absent or only white space.`); `derive` calls `blank(&b.approved)`. Run `cargo test -p zygo-core --test conformance derive_bar` → 3 passed.

  Done: 3 passed.

- [x] **8. Replace CF-1's minimal bar rule in check item 8.** In `model.rs` item 8 (`:1052-1059`), replace `let failing = failing_core(reg, g);` with `let failing = crate::derive::bar(reg, g).failing;`. The fault text stays `client `<id>` is supported and below the bar: <ids joined by ", ">`. Delete `failing_core` and its doc line (`:1071-1082`). Check: `grep -rn failing_core crates/zygo-core/tests/` prints nothing; `cargo test -p zygo-core --test conformance check_` → all `check_…` tests pass with no change to their text, among them `check_4_reason_and_core_na` (`{4, 8}`), `check_6_bar_exception_faults` (`{6}` for the approved exception on `met` A-1.1; `{6, 8}` for the unapproved ones), `check_6_duplicate_or_reasonless_bar_exception`, `check_8_supported_client_below_bar` and `check_8_registry_exception_meets_bar`.

  Done: `grep -rn failing_core` prints nothing; `check_` → 50 passed, no test text changed.

- [x] **9. Rollup and release tests (failing).** Add:

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

  Done: failed first: 3 failed, each `not yet implemented`.

- [x] **10. Implement `derive::rollup`** by the "Rollup" rule, built from `cell` and `bar`. Run `cargo test -p zygo-core --test conformance derive_` → 13 passed.

  Done: `derive_` → 13 passed.

- [x] **11. Acceptance and commit.** Run `test "$(cargo test -q -p zygo-core --test conformance -- derive_ --list 2>/dev/null | grep -c ': test$')" -ge 9 && cargo test -p zygo-core --test conformance --locked` → exit 0 (13 `derive_` tests; the whole target, 78 tests, passes). Run `cargo test --workspace --locked --no-fail-fast` → exit 0. Run `cargo test -p zygo-core --test conformance --no-run --locked 2>&1 | grep -c '^warning'` → `0`. Commit `feat(conformance): derived cell, scores, bar, rollup (CF-2)`, ending with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:CF-2`. Push. CI is green on ubuntu, macOS and Windows.

  Done: acceptance exit 0: 13 `derive_` tests; `conformance` 78 passed. `cargo test -p zygo-core --locked`, `cargo build --workspace --locked` exit 0; warnings 0. Deviation: `cargo test --workspace --locked --no-fail-fast` exit 101 in this checkout only: `zygo --test guard_hook` `binary_guard_not_a_repository_writes_no_line` and `binary_guard_slow_rev_parse_says_unread_once` panic at `crates/zygo/tests/guard_hook.rs:72` ("no directory outside the temp roots") because the checkout is under `/private/tmp`; CF-2 does not touch the `zygo` crate; every other target passes; CI decides.

### Coverage map (Acceptance → test)

| Acceptance item | Test |
|---|---|
| per-tier and total full / partial / none / n/a and met / applicable | `derive_rollup_fixtures`, `derive_rollup_spec_example` |
| `below bar` with the capability id; `meets bar` with a registry exception | `derive_bar_below_then_exception_meets`, `derive_bar_unapproved_exception_does_not_count`, `derive_bar_core_na_or_ungraded_fails_even_with_exception`, `check_8_*`, `check_6_bar_exception_faults` |
| tier score = mean of cell values | `derive_scores_mean_of_cells_exact_floor`, `derive_scores_fixtures` |
| total = weighted mean, rounded down last (review focus 3: 89, not 88) | `derive_scores_spec_example_rounds_last` (asserts 89 and that rounding first gives 88), `derive_scores_fixtures` (66) |
| a tier with no applicable cell is `n/a` and leaves the weights | `derive_scores_na_tier_leaves_weights` |
| the K§6.2.6 example gives 96, 75, 94, 89 | `derive_scores_spec_example_rounds_last` |
| an all-`n/a` cell is `n/a` | `derive_cell_fixtures_full_none_na`, `derive_cell_na_criteria_leave_value_and_ungraded_is_unmet` |
| release state `supported`, `meets bar, not released`, `below bar` | `derive_release_state`, `derive_rollup_spec_example` |
| `partial` lists the missing text; `none` gives the reasons (K§6.2.4) | `derive_cell_partial_lists_missing`, `derive_cell_fixtures_full_none_na` |

## Round 2

Review findings of round 1, each fixed test first.

- **F1** (`derive.rs` `Frac::add`, `div`, `percent`: `u128` overflow on a large registry; a debug build panics and a release build gives a wrong score). Every multiply and add in `Frac` is now checked and gives `None` on overflow. `scores` then panics with `client `<id>`: score fraction overflow`, so every build fails closed. Each result is still reduced by its gcd. Test `derive_scores_overflow_fails_closed` (27 standard capabilities with the prime criteria counts 2..103, 1 met each). It failed first: the panic was `attempt to multiply with overflow`, not the expected text. It passes in debug and in release. Fix: `2a9f80c8`.
- **F2** (`derive.rs` `grade_of` read only the first of duplicate grades; check item 6 treats a criterion as met when any entry is met). `grade_of` now returns a `met` entry when any entry is `met`, else the first entry, so items 6 and 8 agree. Test `check_8_duplicate_grade_any_met_no_item_8` (alpha, A-1.2 not-met then met → items `{2}`). It failed first with `{2, 8}`. Fix: `fa390462`.
- **F3** (item 8 not pinned through `check` for a core criterion that has an approved exception). Tests `check_8_core_na_with_exception_is_below_bar` (items `{4, 8}`, text `core criterion `A-1.2` is n/a` and `below the bar: A-1`) and `check_8_core_ungraded_with_exception_is_below_bar` (items `{2, 8}`). These tests pin behaviour that round 1 already had, so they passed on their first run; no code change. Commit: `79a775fd`.
- **F4** (spec wording). In K§6.2.4 Bar, "failing core capability" now matches the bar sentence: a core criterion that is neither `met` nor `not-met` with a registry bar exception fails, so an ungraded or `n/a` core criterion fails. The change log has the new line. Commit: `bec8cb0d`.
- **N1** (minor; `grade_of` depended on the order of duplicate grades when none is `met`). `grade_of` now returns the first `met` entry, else the first `not-met` entry (its reason stays), else the first `n/a` entry. Tests `derive_duplicate_grades_order_independent` and `check_8_duplicate_grades_order_independent` (alpha, A-1.2 graded `[not-met, n/a]` and `[n/a, not-met]`, approved exception for alpha A-1.2). They failed first: the cells were `Partial` and `Full`, and the fault sets were `{2, 4}` and `{2, 4, 8}`. Now both orders give the same cell, the same bar (meets) and `{2, 4}`. Fix: `2442a311`.
- **N2** (minor; the `scores` doc comment). The comment now says that `scores` also panics when an intermediate product such as 100 × num does not fit in `u128`, even when the exact result would fit, so it fails closed. The change is to the comment only, so there is no test. Commit: `60c06395`.

Checks after round 2: the acceptance line exits 0 with 15 `derive_` tests, and `conformance` has 84 passed. `cargo test -p zygo-core --locked` passes. The `conformance` target builds with 0 warnings.

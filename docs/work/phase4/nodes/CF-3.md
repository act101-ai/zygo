# CF-3 — Conformance outputs: feature matrix, rollup and score in Markdown and JSON; write mode and check item 10

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.3 (the platforms recorded next to a cfg-limited test), K§6.2.5 item 10 and the write-mode paragraph, K§6.2.6, K§10 (text layout and key order are the builder's), K§11 items 2 (the platform shown in the document), 3 (generator side) and 4; K-D29, K-D30, K-D33
**Files:** `crates/zygo-core/tests/conformance.rs`, `crates/zygo-core/tests/conformance/render.rs` (new), `crates/zygo-core/tests/conformance/fixtures/`
**Consumes:** `derive::cell` (CF-2), `derive::scores` (CF-2), `derive::bar` (CF-2), `derive::rollup` (CF-2), `Registry::load` (CF-1), `GradeFile::load` (CF-1), `model::Client` (CF-1), `model::resolve` (CF-1), `Resolved` (CF-1), and the Fault type of model::check (CF-1) for item 10; resolve gives each met grade's evidence with its platforms, resolved once against the repository, so render reads no file
**Produces:** `render::markdown(reg: &Registry, grades: &[GradeFile], clients: &[Client], res: &Resolved) -> String`, `render::json(reg: &Registry, grades: &[GradeFile], clients: &[Client], res: &Resolved) -> String`, `render::compare_or_write(root: &Path, md: &str, json: &str) -> Result<(), Fault>` (writes both files when `ZYGO_WRITE_CONFORMANCE=1`; else compares after CR LF → LF; a difference is fault item 10). Platform text: an evidence item's `Evidence.platforms` (CF-1: a non-empty subset of `linux`, `macos`, `windows`, in that order) renders as `unix` when the set is `linux` and `macos`; as nothing (no limit) when it is all three; else as its names in that order
**Acceptance:** `test "$(cargo test -q -p zygo-core --test conformance -- render_ --list 2>/dev/null | grep -c ': test$')" -ge 8 && cargo test -p zygo-core --test conformance --locked` → at least 8 tests named `render_…` (a count of 0 fails), then the whole `conformance` target passes on ubuntu, macOS and Windows CI: matrix rows grouped core, standard, extra in registry order; the four cell forms of K§6.2.6 item 1; the JSON `matrix` holds the same grades and values as the Markdown; `unix` shown next to a `cfg(unix)` evidence; a planted difference in either file is fault item 10; CR LF and LF compare equal; a `|` in a text is escaped; the first Markdown line and the JSON `generated_by` name the generator; two renders are byte-equal; after the three outputs, the Markdown lists per client each unmet criterion with its reason, each `n/a` with its reason and the evidence of each met criterion

## Planning notes

- The real-tree run (reading `docs/conformance/` and `Agent::ALL`, writing `docs/conformance.md` and `.json`) is CF-4. This node tests on fixtures in a temporary root.

## Plan

Base: `phase-4` at `58582e07` (CF-1 PR #87, CF-2 PR #100 merged).

### Premise check (live code, `phase-4` @ 58582e07)

- Holds. `crates/zygo-core/tests/conformance.rs:2-7` declares `mod model;` and `mod derive;` by `#[path]`, each under `#[allow(dead_code)]` (derive's comment: `// CF-3 reads the rest`). `use model::*;` at `:12`, `use derive::{Bar, Cell, Counts, Grade, Scores, Unmet};` at `:13`. Baseline: `cargo test -p zygo-core --test conformance --locked` → 84 passed; tests named `render_`: 0.
- Consumes, as in the code (`model.rs`, `derive.rs` @ 58582e07): see "Interfaces". Each matches CF-3's Consumes line. One addition: `compare_or_write` returns `model::Fault`, which CF-3's Consumes did not name; it is now named (in prose, the type of `model::check`, CF-1).
- `Evidence.platforms: Vec<String>` (`model.rs:99-101`) is a subset of `linux`, `macos`, `windows` in that order (`TARGETS`, `model.rs:408`; `names`, `:415`), never empty: `evidence_in` returns `Err` when no target remains (`model.rs:476-478`). `Resolved::get(client, criterion)` (`model.rs:131`) gives the first resolved `met` grade by client id; each `ResolvedEvidence.result` is `Ok(Evidence)` only when the item counts.
- `Bar.exceptions` (`derive.rs:47`) holds criterion ids only: `bar` keeps the registry `bar_exception` entries with `client == g.client` and a non-blank `approved` (`derive.rs:185`, `model::blank` at `model.rs:916`). The rollup's exceptions line looks up the reason and approval in `reg.bar_exception` by `(g.client, criterion)` with the same non-blank rule.
- `rollup(reg, g, supported)` takes the flag; render passes `Client.supported`. `Cell.missing` holds each applicable criterion that is not met, with `reason: None` when ungraded (`derive.rs:77,88`); `Cell.na` each `n/a` criterion. Render takes the per-criterion status from these two lists (a criterion in neither is met), so it needs no private `grade_of`.
- Fixtures (`crates/zygo-core/tests/conformance/fixtures/`): `registry.toml` (A-1 Transport core: A-1.1, A-1.2; A-3 Model list standard: A-3.1; WA-2 Turn signals extra: WA-2.1), `alpha.toml`, `beta.toml`; the evidence repository `repo/`. Beta's WA-2.1 evidence `test:crates/demo/tests/ev.rs::unix_only` resolves to `["linux", "macos"]` (`conformance.rs:232-238`); `in_windows_mod` to `["windows"]`. Test helpers reused: `fx()` `:15`, `repo()` `:19`, `text(name)` `:23`, `client(id, name, version)` `:27`, `base()` `:31`, `entry(g, criterion)` `:45`, `synth(caps)` `:51`, `spec_example()` `:1083`, `exception(client, criterion)` `:1107` (reason `r`, approved `operator`). `tempfile` is a dependency of `zygo-core` (`Cargo.toml:12`); `serde` (with derive, used by `model.rs`) and `serde_json` are dependencies (`Cargo.toml:10-11`).
- Feature unification: `serde_json` gets `preserve_order` from `zygo-results` and `zygo-redact`, and `arbitrary_precision` from `zygo-plugins`, under `cargo test --workspace`, but not under `-p zygo-core`. So the JSON writer must not go through `serde_json::Value` or `serde_json::Map` (their key order and number text would differ between the two builds). Derived `Serialize` structs and `Serializer::collect_map` write fields in declaration and iteration order under both. Tests read numbers with `as_u64()` / `as_f64()`, never by `Value` equality with `json!`.
- K§11 items 6-7, CF-3's share: item 6's K§6.2.6 example numbers come out in the rendered score (Markdown row and JSON `score`), test `render_json_spec_example_values`; the derivation itself is CF-2's. Item 7 is CF-4's.

### Interfaces

Consumes (exact, @ 58582e07):

```rust
// model.rs
pub struct Registry { pub capability: Vec<Capability>, pub bar_exception: Vec<BarException> }
pub struct Capability { pub id: String, pub name: String, pub tier: Tier, pub standard: String, pub criterion: Vec<Criterion> }
pub struct Criterion { pub id: String, pub text: String }
pub enum Tier { Core, Standard, Extra }
pub struct BarException { pub client: String, pub criterion: String, pub reason: String, pub approved: Option<String> }
pub struct GradeFile { pub client: String, pub name: String, pub version: String, pub cell: Vec<CellUrl>, pub grade: Vec<GradeEntry> }
pub struct CellUrl { pub capability: String, pub url: String }
pub struct Client { pub id: String, pub name: String, pub version: String, pub supported: bool }
pub struct Evidence { pub platforms: Vec<String> }
pub struct Resolved { pub grade: Vec<ResolvedGrade> }
pub struct ResolvedGrade { pub file: usize, pub client: String, pub criterion: String, pub evidence: Vec<ResolvedEvidence> }
pub struct ResolvedEvidence { pub text: String, pub result: Result<Evidence, String> }
impl Resolved { pub fn get(&self, client: &str, criterion: &str) -> Option<&ResolvedGrade> }
pub struct Fault { pub item: u8, pub text: String }      // Debug, Clone, PartialEq
impl Registry { pub fn load(path: &Path) -> Result<Registry, String> }
impl GradeFile { pub fn load(path: &Path) -> Result<GradeFile, String> }
pub fn resolve(root: &Path, grades: &[GradeFile]) -> Resolved;
pub fn blank(s: &Option<String>) -> bool;
// derive.rs
pub enum Grade { Full, Partial, None, Na }
pub struct Unmet { pub criterion: String, pub text: String, pub reason: Option<String> }
pub struct Cell { pub grade: Grade, pub value: Option<f64>, pub missing: Vec<Unmet>, pub na: Vec<Unmet> }
pub struct Scores { pub total: Option<u32>, pub core: Option<u32>, pub standard: Option<u32>, pub extra: Option<u32> }
pub struct Bar { pub meets: bool, pub failing: Vec<String>, pub exceptions: Vec<String> }
pub struct Counts { pub full: u32, pub partial: u32, pub none: u32, pub na: u32, pub met: u32, pub applicable: u32 }
pub struct Rollup { pub core: Counts, pub standard: Counts, pub extra: Counts, pub total: Counts, pub bar: Bar, pub release: &'static str }
pub fn cell(reg: &Registry, g: &GradeFile, capability: &str) -> Cell;
pub fn scores(reg: &Registry, g: &GradeFile) -> Scores;
pub fn bar(reg: &Registry, g: &GradeFile) -> Bar;
pub fn rollup(reg: &Registry, g: &GradeFile, supported: bool) -> Rollup;
```

Produces (new file `crates/zygo-core/tests/conformance/render.rs`; CF-4 consumes the first three by these names):

```rust
pub fn markdown(reg: &Registry, grades: &[GradeFile], clients: &[Client], res: &Resolved) -> String;
pub fn json(reg: &Registry, grades: &[GradeFile], clients: &[Client], res: &Resolved) -> String;
pub fn compare_or_write(root: &Path, md: &str, json: &str) -> Result<(), Fault>;   // reads ZYGO_WRITE_CONFORMANCE
pub fn compare_or_write_mode(root: &Path, md: &str, json: &str, write: bool) -> Result<(), Fault>;   // tests call this
pub fn platform_text(platforms: &[String]) -> Option<String>;
pub const GENERATED_BY: &str = "crates/zygo-core/tests/conformance.rs (ZYGO_WRITE_CONFORMANCE=1); do not edit: a hand edit fails the check";
```

`GENERATED_BY` starts with the K§6.2.6 example text and adds the K§6.2.5 words "a hand edit fails the check", so the Markdown first line and the JSON key hold the same text (K§6.2.5 write-mode paragraph).

### Rules the code must hold (K§6.2.5 item 10 and input rules, K§6.2.6, K§10)

- **Clients.** Columns, rollups, score rows, JSON `clients` and criteria lists go in `clients` order (CF-4 passes `Agent::ALL` order). Each client uses the first grade file with `g.client == c.id`. A client with no grade file is left out of both outputs (check item 1 reports it). Name: `Client.name`; version: the grade file's `version` (the pinned one; item 7 holds them equal); supported: `Client.supported`.
- **Capability order.** Core, then standard, then extra; registry order inside a tier. The matrix rows, the JSON `matrix` keys and the criteria list use this order (criteria in registry order inside a capability).
- **Cell text (K§6.2.6 item 1).** From `derive::cell` and the grade file's `CellUrl` for the capability (empty URL if none; item 5 reports it): `Full` → `full <url>`; `Partial` → `partial (missing: <id> <text>; <id> <text>) <url>` over `missing`; `None` → `none (<id>: <reason>; …) <url>` over `missing`, with `no grade` for a `reason` of `None`; `Na` → `n/a (<reason>; <reason>)` over `na`.
- **Table escape (K§6.2.5 input rules).** Each text put in a table cell (capability `<id> <name>`, cell text, client name in a header or score row) has each `|` replaced by `\|`. Text outside a table is not changed.
- **Platform text.** `platform_text`: `["linux", "macos"]` → `Some("unix")`; all three → `None`; else `Some` of the names joined by `, ` in the given order; an empty list (never given: CF-1 `Evidence.platforms` is never empty) → `None`.
- **Criteria list** (after the three outputs, K§6.2.6): per client, each criterion: `- <id> not met: <reason>` (`no grade` for `None`) when in `cell.missing`; `- <id> n/a: <reason>` when in `cell.na`; else `- <id> met: ` and the evidence items of `res.get(client, id)` whose `result` is `Ok`, each as `` `<text>` `` plus ` (<platform text>)` when `platform_text` is `Some`, joined by `, `; `no evidence that counts` when there is none (check item 3 reports it).
- **Rollup text.** Per client: `### <name> <version>`, the table of `expected-conformance.md`, then three list lines: `- Bar: meets bar` or `- Bar: below bar: <failing ids joined by ", ">`; `- Bar exceptions: none` or `- Bar exceptions: ` + for each id of `Bar.exceptions`, `<id>: <reason> (approved: <approved>)` from the first `reg.bar_exception` entry with that client and criterion and `!blank(&approved)`, joined by `; `; `- Release: <Rollup.release>`.
- **Score text.** `<n>%` for `Some(n)`, `n/a` for `None`.
- **Markdown layout** is the golden file `fixtures/expected-conformance.md` (step 2): first line `Generated by <GENERATED_BY>.`; one blank line between blocks; the file ends with one `\n`.
- **JSON.** `serde_json::to_string_pretty` of derived `Serialize` structs, plus a final `\n`. Keys in this order (test `render_json_key_order`): top `generated_by` (= `GENERATED_BY`), `weights` (`core` 3, `standard` 2, `extra` 1), `clients`; client `id`, `name`, `version`, `supported`, `matrix`, `rollup`, `score`; `matrix` maps capability id → cell `tier` (`core`|`standard`|`extra`), `grade` (`full`|`partial`|`none`|`n/a`), `value` (number, `null` for n/a), `url`, `missing`, `na` (each entry `criterion`, `text`, `reason` with `null` for none); `rollup` `core`, `standard`, `extra`, `total` (each `full`, `partial`, `none`, `na`, `met`, `applicable`), `bar` (`verdict` `meets bar`|`below bar`, `failing`, `exceptions`), `release`; `score` `total`, `core`, `standard`, `extra` (integer, `null` for n/a). The `matrix` map is written with `Serializer::collect_map` in capability order. No `serde_json::Value` or `Map` on the write path.
- **Item 10.** `compare_or_write_mode`: paths `root/docs/conformance.md` and `root/docs/conformance.json`. `write == true`: `create_dir_all(root/docs)`, write both, `Ok(())`; an I/O error → `Err(Fault { item: 10, text: "<path>: <error>" })`. `write == false`: read each file as bytes; a file that cannot be read, is not UTF-8, or differs from the generated text after `\r\n` → `\n` on both sides is a problem `"<path>: differs from the generated text (run with ZYGO_WRITE_CONFORMANCE=1)"` or `"<path>: <error>"`; any problem → one `Err(Fault { item: 10, text })` with the problems joined by `; `. `compare_or_write` passes `write = std::env::var_os("ZYGO_WRITE_CONFORMANCE")` equal to `"1"`; no test calls it (the process environment is shared by parallel tests).

### Steps

- [ ] **1. Module stub.** Create `crates/zygo-core/tests/conformance/render.rs` with the doc line `//! The generator (K§6.2.6) and check item 10 (K§6.2.5): feature matrix, rollup and score in Markdown and JSON.`, `use crate::model::*;`, `use crate::derive;`, `GENERATED_BY` and the five signatures of "Interfaces", each body `todo!()`. In `conformance.rs` after the `mod derive;` block add:

  ```rust
  #[allow(dead_code)] // CF-4 calls render::compare_or_write
  #[path = "conformance/render.rs"]
  mod render;
  ```
  Check: `cargo test -p zygo-core --test conformance --locked` → 84 passed.

- [ ] **2. Golden file.** Create `crates/zygo-core/tests/conformance/fixtures/expected-conformance.md` with exactly this text (LF line ends, one final `\n`). It is the render of `render_input()` (step 3): `base()` with alpha A-1.2 `not-met` (`no key helper`) under the approved registry exception `exception("alpha", "A-1.2")`. Values computed by hand: alpha A-1 1/2 → core 50, standard 100, extra n/a, total (3 × 0.5 + 2 × 1) ÷ 5 = 70; beta 66 / 100 / 0 / 100 (`derive_scores_fixtures`); counts as `derive_rollup_fixtures` with alpha core now `0 1 0 0`, met 1 of 2.

  ````markdown
  Generated by crates/zygo-core/tests/conformance.rs (ZYGO_WRITE_CONFORMANCE=1); do not edit: a hand edit fails the check.

  # Conformance

  ## Feature matrix

  | Capability | Tier | Alpha | Beta |
  |---|---|---|---|
  | A-1 Transport | core | partial (missing: A-1.2 The credential is the session token.) https://example.com/alpha/transport | full https://example.com/beta/transport |
  | A-3 Model list | standard | full https://example.com/alpha/models | none (A-3.1: Beta has no model picker) https://example.com/beta/models |
  | WA-2 Turn signals | extra | n/a (Alpha has no turn events) | full https://example.com/beta/turns |

  ## Rollup

  ### Alpha 1.0.0

  | Tier | full | partial | none | n/a | met / applicable |
  |---|---|---|---|---|---|
  | core | 0 | 1 | 0 | 0 | 1 / 2 |
  | standard | 1 | 0 | 0 | 0 | 1 / 1 |
  | extra | 0 | 0 | 0 | 1 | 0 / 0 |
  | total | 1 | 1 | 0 | 1 | 2 / 3 |

  - Bar: meets bar
  - Bar exceptions: A-1.2: r (approved: operator)
  - Release: supported

  ### Beta 2.0.0

  | Tier | full | partial | none | n/a | met / applicable |
  |---|---|---|---|---|---|
  | core | 1 | 0 | 0 | 0 | 2 / 2 |
  | standard | 0 | 0 | 1 | 0 | 0 / 1 |
  | extra | 1 | 0 | 0 | 0 | 1 / 1 |
  | total | 2 | 0 | 1 | 0 | 3 / 4 |

  - Bar: meets bar
  - Bar exceptions: none
  - Release: supported

  ## Score

  | Client | Total | Core | Standard | Extra |
  |---|---|---|---|---|
  | Alpha | 70% | 50% | 100% | n/a |
  | Beta | 66% | 100% | 0% | 100% |

  ## Criteria

  ### Alpha

  - A-1.1 met: `test:crates/demo/tests/ev.rs::plain`
  - A-1.2 not met: no key helper
  - A-3.1 met: `test:crates/demo/tests/ev.rs::tokio_plain`
  - WA-2.1 n/a: Alpha has no turn events

  ### Beta

  - A-1.1 met: `fixture:fixtures/beta/capture.json`
  - A-1.2 met: `premise:docs/live-checks.md#L1`
  - A-3.1 not met: Beta has no model picker
  - WA-2.1 met: `test:crates/demo/tests/ev.rs::unix_only` (unix)
  ````
  Check: the file exists, has 62 lines (`wc -l` → 62; the block above, each line without its two-space list indent), and no `\r` (`grep -c $'\r'` → 0).

- [ ] **3. Markdown tests (failing).** Add to `conformance.rs`:

  ```rust
  type RenderIn = (Registry, Vec<GradeFile>, Vec<Client>, Resolved);

  /// `base()` with alpha A-1.2 not-met under an approved registry exception: the four cell forms.
  fn render_input() -> RenderIn {
      let (mut r, mut g, c) = base();
      {
          let e = entry(&mut g[0], "A-1.2");
          e.status = Status::NotMet;
          e.reason = Some("no key helper".into());
      }
      r.bar_exception.push(exception("alpha", "A-1.2"));
      let res = resolve(&repo(), &g);
      (r, g, c, res)
  }

  fn synth_input(caps: &[(Tier, &[Status])]) -> RenderIn {
      let (r, g) = synth(caps);
      let g = vec![g];
      let res = resolve(&repo(), &g);
      (r, g, vec![client("synth", "Synth", "1.0.0")], res)
  }

  fn md_of(x: &RenderIn) -> String {
      render::markdown(&x.0, &x.1, &x.2, &x.3)
  }

  fn json_of(x: &RenderIn) -> String {
      render::json(&x.0, &x.1, &x.2, &x.3)
  }

  fn row<'a>(md: &'a str, start: &str) -> &'a str {
      md.lines().find(|l| l.starts_with(start)).unwrap_or_else(|| panic!("no line `{start}`"))
  }

  #[test]
  fn render_markdown_golden_fixtures() {
      let x = render_input();
      assert_eq!(check(&repo(), &x.0, &x.1, &x.2, &x.3), vec![]); // valid input
      assert_eq!(md_of(&x), text("expected-conformance.md").replace("\r\n", "\n"));
  }

  #[test]
  fn render_markdown_four_cell_forms() {
      let md = md_of(&render_input());
      assert_eq!(row(&md, "| A-1 "), "| A-1 Transport | core | partial (missing: A-1.2 The credential is the session token.) https://example.com/alpha/transport | full https://example.com/beta/transport |");
      assert_eq!(row(&md, "| A-3 "), "| A-3 Model list | standard | full https://example.com/alpha/models | none (A-3.1: Beta has no model picker) https://example.com/beta/models |");
      assert_eq!(row(&md, "| WA-2 "), "| WA-2 Turn signals | extra | n/a (Alpha has no turn events) | full https://example.com/beta/turns |");
      use Status::{Met as M, Na as X, NotMet as N};
      let md = md_of(&synth_input(&[(Tier::Standard, &[M, N, N][..]), (Tier::Standard, &[N, N][..]), (Tier::Extra, &[X, X][..])]));
      assert_eq!(row(&md, "| A-1 "), "| A-1 c | standard | partial (missing: A-1.2 t1.2; A-1.3 t1.3) https://example.com/1 |");
      assert_eq!(row(&md, "| A-2 "), "| A-2 c | standard | none (A-2.1: r2.1; A-2.2: r2.2) https://example.com/2 |");
      assert_eq!(row(&md, "| A-3 "), "| A-3 c | extra | n/a (r3.1; r3.2) |");
      let mut x = synth_input(&[(Tier::Standard, &[N][..])]);
      x.1[0].grade.clear(); // ungraded
      let md = md_of(&x);
      assert_eq!(row(&md, "| A-1 "), "| A-1 c | standard | none (A-1.1: no grade) https://example.com/1 |");
      assert!(md.contains("- A-1.1 not met: no grade\n"));
  }

  #[test]
  fn render_markdown_rows_by_tier_columns_by_client() {
      use Status::Met as M;
      let md = md_of(&synth_input(&[(Tier::Extra, &[M][..]), (Tier::Core, &[M][..]), (Tier::Standard, &[M][..]), (Tier::Core, &[M][..])]));
      let pos = |id: &str| md.find(&format!("| {id} c |")).unwrap();
      assert!(pos("A-2") < pos("A-4") && pos("A-4") < pos("A-3") && pos("A-3") < pos("A-1"));
      assert!(md.contains("- A-1.1 met: no evidence that counts\n"));
      let mut x = render_input();
      x.2.reverse();
      let md = md_of(&x);
      assert!(md.lines().any(|l| l == "| Capability | Tier | Beta | Alpha |"));
      assert!(md.find("### Beta 2.0.0").unwrap() < md.find("### Alpha 1.0.0").unwrap());
      assert!(md.find("| Beta | 66%").unwrap() < md.find("| Alpha | 70%").unwrap());
  }

  #[test]
  fn render_markdown_platform_text_and_unix_shown() {
      let p = |v: &[&str]| render::platform_text(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
      assert_eq!(p(&["linux", "macos"]), Some("unix".to_string()));
      assert_eq!(p(&["linux", "macos", "windows"]), None);
      assert_eq!(p(&["windows"]), Some("windows".to_string()));
      assert_eq!(p(&["linux"]), Some("linux".to_string()));
      assert_eq!(p(&["macos", "windows"]), Some("macos, windows".to_string()));
      let md = md_of(&render_input());
      assert!(md.contains("- WA-2.1 met: `test:crates/demo/tests/ev.rs::unix_only` (unix)\n"));
      assert!(md.contains("- A-1.1 met: `test:crates/demo/tests/ev.rs::plain`\n"));
      let (r, mut g, c) = base();
      entry(&mut g[1], "WA-2.1").evidence = vec!["test:crates/demo/tests/ev.rs::in_windows_mod".into(), "fixture:fixtures/beta/capture.json".into()];
      let res = resolve(&repo(), &g);
      assert!(render::markdown(&r, &g, &c, &res).contains(
          "- WA-2.1 met: `test:crates/demo/tests/ev.rs::in_windows_mod` (windows), `fixture:fixtures/beta/capture.json`\n"
      ));
  }

  /// Review focus 4, first half: a `|` in a reason, URL, name or client name keeps the row one row.
  #[test]
  fn render_markdown_pipe_escaped_one_row() {
      let mut x = render_input();
      entry(&mut x.1[1], "A-3.1").reason = Some("a|b".into());
      x.1[1].cell.iter_mut().find(|c| c.capability == "A-3").unwrap().url = "https://example.com/x|y".into();
      x.0.capability[1].name = "Model|list".into();
      x.2[1].name = "Be|ta".into();
      let md = md_of(&x);
      let line = row(&md, "| A-3 ");
      assert_eq!(line, r"| A-3 Model\|list | standard | full https://example.com/alpha/models | none (A-3.1: a\|b) https://example.com/x\|y |");
      let unescaped = |l: &str| l.match_indices('|').filter(|(i, _)| !l[..*i].ends_with('\\')).count();
      assert_eq!(unescaped(line), 5);
      assert!(md.lines().any(|l| l == r"| Capability | Tier | Alpha | Be\|ta |"));
      assert!(md.lines().any(|l| l == r"| Be\|ta | 66% | 100% | 0% | 100% |"));
      assert!(md.contains("- A-3.1 not met: a|b\n")); // outside a table: as written
  }

  #[test]
  fn render_markdown_below_bar_rollup() {
      let (r, mut g, c) = base();
      {
          let e = entry(&mut g[0], "A-1.2");
          e.status = Status::NotMet;
          e.reason = Some("no key helper".into());
      }
      let res = resolve(&repo(), &g);
      let md = render::markdown(&r, &g, &c, &res);
      assert!(md.contains("### Alpha 1.0.0\n"));
      assert!(md.contains("- Bar: below bar: A-1\n- Bar exceptions: none\n- Release: below bar\n"));
  }
  ```
  Run `cargo test -p zygo-core --test conformance render_markdown` → the 6 tests fail (`not yet implemented`).

- [ ] **4. Implement `platform_text` and `markdown`** by "Rules" (Clients, Capability order, Cell text, Table escape, Platform text, Criteria list, Rollup text, Score text, Markdown layout). Run `cargo test -p zygo-core --test conformance render_markdown` → 6 passed.

- [ ] **5. JSON tests (failing).** Add:

  ```rust
  fn jv(s: &str) -> serde_json::Value {
      serde_json::from_str(s).unwrap()
  }

  /// The keys of pretty JSON in text order (one key per line).
  fn keys(json: &str) -> Vec<String> {
      json.lines().filter_map(|l| l.trim_start().strip_prefix('"')?.split_once("\": ").map(|(k, _)| k.to_string())).collect()
  }

  #[test]
  fn render_json_generated_by_matches_markdown_line() {
      let x = render_input();
      assert_eq!(render::GENERATED_BY, "crates/zygo-core/tests/conformance.rs (ZYGO_WRITE_CONFORMANCE=1); do not edit: a hand edit fails the check");
      assert_eq!(md_of(&x).lines().next(), Some(format!("Generated by {}.", render::GENERATED_BY).as_str()));
      assert_eq!(jv(&json_of(&x))["generated_by"].as_str(), Some(render::GENERATED_BY));
  }

  #[test]
  fn render_json_matrix_matches_markdown() {
      let x = render_input();
      let (md, v) = (md_of(&x), jv(&json_of(&x)));
      for (i, c) in x.2.iter().enumerate() {
          let j = &v["clients"][i];
          assert_eq!(j["id"].as_str(), Some(c.id.as_str()));
          for cap in &x.0.capability {
              let cell = derive::cell(&x.0, &x.1[i], &cap.id);
              let word = match cell.grade { Grade::Full => "full", Grade::Partial => "partial", Grade::None => "none", Grade::Na => "n/a" };
              let m = &j["matrix"][cap.id.as_str()];
              assert_eq!(m["grade"].as_str(), Some(word));
              assert_eq!(m["value"].as_f64(), cell.value);
              assert_eq!(m["missing"].as_array().unwrap().len(), cell.missing.len());
              assert_eq!(m["na"].as_array().unwrap().len(), cell.na.len());
              let md_cell = row(&md, &format!("| {} ", cap.id)).split(" | ").nth(2 + i).unwrap();
              assert!(md_cell.starts_with(&format!("{word} ")), "{md_cell}");
          }
      }
      let a = &v["clients"][0];
      assert_eq!(a["matrix"]["A-1"]["missing"][0]["reason"].as_str(), Some("no key helper"));
      assert_eq!(a["matrix"]["WA-2"]["na"][0]["criterion"].as_str(), Some("WA-2.1"));
      assert!(a["matrix"]["WA-2"]["value"].is_null() && a["score"]["extra"].is_null());
      assert_eq!(["total", "core", "standard"].map(|k| a["score"][k].as_u64()), [Some(70), Some(50), Some(100)]);
      assert_eq!(a["rollup"]["bar"]["exceptions"][0].as_str(), Some("A-1.2"));
  }

  /// K§6.2.6 machine-readable example, value by value.
  #[test]
  fn render_json_spec_example_values() {
      let (r, g) = spec_example();
      let x: RenderIn = { let g = vec![g]; let res = resolve(&repo(), &g); (r, g, vec![client("synth", "Synth", "1.0.0")], res) };
      let v = jv(&json_of(&x));
      assert!(v["generated_by"].as_str().unwrap().starts_with("crates/zygo-core/tests/conformance.rs (ZYGO_WRITE_CONFORMANCE=1); do not edit"));
      assert_eq!(["core", "standard", "extra"].map(|k| v["weights"][k].as_u64()), [Some(3), Some(2), Some(1)]);
      let c = &v["clients"][0];
      assert_eq!((c["id"].as_str(), c["name"].as_str(), c["version"].as_str(), c["supported"].as_bool()), (Some("synth"), Some("Synth"), Some("1.0.0"), Some(true)));
      let a1 = &c["matrix"]["A-1"];
      assert_eq!((a1["tier"].as_str(), a1["grade"].as_str(), a1["value"].as_f64(), a1["url"].as_str()), (Some("core"), Some("partial"), Some(0.75), Some("https://example.com/1")));
      let m = &a1["missing"];
      assert_eq!((m.as_array().unwrap().len(), a1["na"].as_array().unwrap().len()), (1, 0));
      assert_eq!((m[0]["criterion"].as_str(), m[0]["text"].as_str(), m[0]["reason"].as_str()), (Some("A-1.4"), Some("t1.4"), Some("r1.4")));
      let counts = |t: &str| ["full", "partial", "none", "na", "met", "applicable"].map(|k| c["rollup"][t][k].as_u64().unwrap());
      assert_eq!(counts("core"), [7, 1, 0, 0, 19, 20]);
      assert_eq!(counts("standard"), [9, 0, 3, 0, 9, 12]);
      assert_eq!(counts("extra"), [2, 1, 0, 1, 7, 8]);
      assert_eq!(counts("total"), [18, 2, 3, 1, 35, 40]);
      let bar = &c["rollup"]["bar"];
      assert_eq!(bar["verdict"].as_str(), Some("meets bar"));
      assert_eq!(bar["failing"].as_array().unwrap().len(), 0);
      assert_eq!(bar["exceptions"].as_array().unwrap().iter().map(|e| e.as_str().unwrap()).collect::<Vec<_>>(), ["A-1.4"]);
      assert_eq!(c["rollup"]["release"].as_str(), Some("supported"));
      assert_eq!(["total", "core", "standard", "extra"].map(|k| c["score"][k].as_u64()), [Some(89), Some(96), Some(75), Some(94)]);
      assert!(md_of(&x).lines().any(|l| l == "| Synth | 89% | 96% | 75% | 94% |"));
  }

  #[test]
  fn render_json_key_order() {
      let json = json_of(&synth_input(&[(Tier::Core, &[Status::Met][..])]));
      let counts = ["full", "partial", "none", "na", "met", "applicable"];
      let mut want = vec!["generated_by", "weights", "core", "standard", "extra", "clients", "id", "name", "version", "supported", "matrix", "A-1", "tier", "grade", "value", "url", "missing", "na", "rollup"];
      for t in ["core", "standard", "extra", "total"] {
          want.push(t);
          want.extend(counts);
      }
      want.extend(["bar", "verdict", "failing", "exceptions", "release", "score", "total", "core", "standard", "extra"]);
      assert_eq!(keys(&json), want);
      assert!(json.ends_with("}\n") && !json.ends_with("\n\n"));
  }

  #[test]
  fn render_json_matrix_in_tier_order() {
      use Status::Met as M;
      let json = json_of(&synth_input(&[(Tier::Extra, &[M][..]), (Tier::Core, &[M][..]), (Tier::Standard, &[M][..]), (Tier::Core, &[M][..])]));
      let order: Vec<String> = keys(&json).into_iter().filter(|k| k.starts_with("A-")).collect();
      assert_eq!(order, ["A-2", "A-4", "A-3", "A-1"]);
  }

  #[test]
  fn render_json_two_renders_byte_equal() {
      let (x, y) = (render_input(), render_input());
      assert_eq!(md_of(&x), md_of(&y));
      assert_eq!(json_of(&x), json_of(&y));
      assert_eq!(json_of(&x), json_of(&x));
  }

  #[test]
  fn render_json_client_without_grade_file_left_out() {
      let mut x = render_input();
      x.2.push(client("gamma", "Gamma", "3.0.0"));
      assert!(!md_of(&x).contains("Gamma"));
      assert_eq!(jv(&json_of(&x))["clients"].as_array().unwrap().len(), 2);
  }
  ```
  Run `cargo test -p zygo-core --test conformance render_json` → the 7 tests fail (`not yet implemented`).

- [ ] **6. Implement `json`** by "Rules" (JSON, Clients, Capability order). Run `cargo test -p zygo-core --test conformance render_json` → 7 passed.

- [ ] **7. Item 10 tests (failing).** Add:

  ```rust
  fn docs_root(md: &str, json: &str) -> tempfile::TempDir {
      let dir = tempfile::tempdir().unwrap();
      std::fs::create_dir(dir.path().join("docs")).unwrap();
      std::fs::write(dir.path().join("docs/conformance.md"), md).unwrap();
      std::fs::write(dir.path().join("docs/conformance.json"), json).unwrap();
      dir
  }

  /// Review focus 4, second half: a Windows checkout with CR LF compares equal.
  #[test]
  fn render_compare_crlf_equal() {
      let x = render_input();
      let (md, json) = (md_of(&x), json_of(&x));
      let dir = docs_root(&md.replace('\n', "\r\n"), &json.replace('\n', "\r\n"));
      assert_eq!(render::compare_or_write_mode(dir.path(), &md, &json, false), Ok(()));
      let dir = docs_root(&md, &json);
      assert_eq!(render::compare_or_write_mode(dir.path(), &md, &json, false), Ok(()));
  }

  #[test]
  fn render_compare_planted_difference_is_item_10() {
      let x = render_input();
      let (md, json) = (md_of(&x), json_of(&x));
      let planted = md.replacen("full https://example.com/beta/transport", "full https://example.com/beta/transporT", 1);
      for (m, j, path) in [(planted.as_str(), json.as_str(), "docs/conformance.md"), (md.as_str(), &json.replacen("\"meets bar\"", "\"below bar\"", 1)[..], "docs/conformance.json")] {
          let dir = docs_root(m, j);
          let f = render::compare_or_write_mode(dir.path(), &md, &json, false).unwrap_err();
          assert_eq!(f.item, 10);
          assert!(f.text.contains(path), "{}", f.text);
      }
      let dir = docs_root(&md, &json);
      std::fs::remove_file(dir.path().join("docs/conformance.json")).unwrap();
      let f = render::compare_or_write_mode(dir.path(), &md, &json, false).unwrap_err();
      assert!(f.item == 10 && f.text.contains("docs/conformance.json"));
  }

  #[test]
  fn render_compare_write_mode_writes_both() {
      let x = render_input();
      let (md, json) = (md_of(&x), json_of(&x));
      let dir = tempfile::tempdir().unwrap(); // no docs/ yet
      assert_eq!(render::compare_or_write_mode(dir.path(), &md, &json, true), Ok(()));
      assert_eq!(std::fs::read(dir.path().join("docs/conformance.md")).unwrap(), md.as_bytes());
      assert_eq!(std::fs::read(dir.path().join("docs/conformance.json")).unwrap(), json.as_bytes());
      assert_eq!(render::compare_or_write_mode(dir.path(), &md, &json, false), Ok(()));
      let file = dir.path().join("docs/conformance.md"); // a root that is a file cannot be written
      assert_eq!(render::compare_or_write_mode(&file, &md, &json, true).unwrap_err().item, 10);
  }
  ```
  Run `cargo test -p zygo-core --test conformance render_compare` → the 3 tests fail (`not yet implemented`).

- [ ] **8. Implement `compare_or_write_mode` and `compare_or_write`** by "Rules" (Item 10). Run `cargo test -p zygo-core --test conformance render_compare` → 3 passed.

- [ ] **9. Drop derive's dead-code allow.** In `conformance.rs`, delete the line `#[allow(dead_code)] // CF-3 reads the rest` above `mod derive;`. Check: `cargo test -p zygo-core --test conformance --no-run --locked 2>&1 | grep -c 'conformance/derive.rs'` → 0 (no warning in `derive.rs`), and `... | grep -c 'conformance/render.rs'` → 0.

- [ ] **10. Acceptance.** Run the node's Acceptance command: `test "$(cargo test -q -p zygo-core --test conformance -- render_ --list 2>/dev/null | grep -c ': test$')" -ge 8 && cargo test -p zygo-core --test conformance --locked` → exit 0; the count is 16; the target shows 100 passed (84 + 16). Then `cargo test --workspace --locked --no-fail-fast` → green (the `--workspace` build turns on `preserve_order` and `arbitrary_precision`; the JSON tests must pass there too).

- [ ] **11. Commit** `feat(conformance): CF-3 render matrix, rollup and score; check item 10`, with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:CF-3`, on `phase-4--CF-3`; push.

### Coverage map (Acceptance → test)

| Acceptance item | Test |
|---|---|
| rows grouped core, standard, extra in registry order | `render_markdown_rows_by_tier_columns_by_client`, `render_json_matrix_in_tier_order` |
| the four cell forms (K§6.2.6 item 1) | `render_markdown_four_cell_forms`, `render_markdown_golden_fixtures` |
| JSON `matrix` holds the same grades and values | `render_json_matrix_matches_markdown`, `render_json_spec_example_values` |
| `unix` next to a `cfg(unix)` evidence (K§11 item 2) | `render_markdown_platform_text_and_unix_shown` |
| planted difference in either file is item 10 | `render_compare_planted_difference_is_item_10` |
| CR LF and LF compare equal (review focus 4) | `render_compare_crlf_equal` |
| `|` escaped, table stays one row (review focus 4) | `render_markdown_pipe_escaped_one_row` |
| first Markdown line and `generated_by` name the generator | `render_json_generated_by_matches_markdown_line` |
| two renders byte-equal; fixed key order (K§6.2.6) | `render_json_two_renders_byte_equal`, `render_json_key_order` |
| rollup with bar, exceptions, release (K§6.2.6 item 2) | `render_markdown_golden_fixtures`, `render_markdown_below_bar_rollup` |
| score table (K§6.2.6 item 3; K§11 item 6 example numbers) | `render_markdown_golden_fixtures`, `render_json_spec_example_values` |
| criteria list after the outputs | `render_markdown_golden_fixtures` |
| write mode (K§6.2.5) | `render_compare_write_mode_writes_both` |

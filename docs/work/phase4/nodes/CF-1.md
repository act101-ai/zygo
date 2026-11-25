# CF-1 — Conformance input model and check: `Agent::ALL`, registry and grade parsing, evidence kinds, check items 1-9

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.1, K§6.2.2 (form only), K§6.2.3, K§6.2.5 items 1-9 and input rules, K§9 (P4-2), K§11 items 1 (items 1-9) and 2; K-D5, K-D6, K-D28
**Files:** `crates/zygo-core/src/lib.rs`, `crates/zygo-core/Cargo.toml`, `Cargo.toml`, `Cargo.lock`, `crates/zygo-core/tests/conformance.rs` (new), `crates/zygo-core/tests/conformance/model.rs` (new), `crates/zygo-core/tests/conformance/fixtures/` (new)
**Consumes:** —
**Produces:** `zygo_core::Agent::ALL: [Agent; 2]`, `model::Registry::load(path: &Path) -> Result<Registry, String>`, `model::GradeFile::load(path: &Path) -> Result<GradeFile, String>`, `model::Client { id: String, name: String, version: String, supported: bool }`, `model::evidence(root: &Path, e: &str) -> Result<Evidence, String>` (`Evidence.platforms: Vec<String>`), `model::check(root: &Path, reg: &Registry, grades: &[GradeFile], clients: &[Client]) -> Vec<Fault>` (`Fault { item: u8, text: String }`)
**Acceptance:** `test "$(cargo test -q -p zygo-core --test conformance -- check_ input_ --list 2>/dev/null | grep -c ': test$')" -ge 19 && cargo test -p zygo-core --test conformance --locked -- check_ input_` → at least 19 tests named `check_…` or `input_…` (a count of 0 fails before any run), then all pass on ubuntu, macOS and Windows CI: one planted-fault case per check item 1-9; the K§11 item 1 cases (an evidence test with `#[ignore]`, a `premise:` row whose result is `failed`, a premise id that is in the other file only, a `bar_exception` key in a grade file); the two K§11 item 2 cases (`#[cfg(unix)]` counts with platform `unix`; `#[tokio::test(flavor = "multi_thread")]` counts); at least 5 `input_` cases for the K§6.2.5 input rules

## Planning notes

- Layout (author): the test crate root is `tests/conformance.rs`; its modules live in `tests/conformance/*.rs` through `#[path]`, as `crates/zygo-launch/tests/premise.rs:4-5` does, so Cargo builds one test target. CF-2, CF-3 and CF-4 add their modules and test functions to this root.
- `Agent::ALL` goes in `crates/zygo-core/src/lib.rs` next to `enum Agent` (`lib.rs:20-27`); P4-6 moves it (K§9).
- `syn` is not a workspace dependency today (`Cargo.toml` `[workspace.dependencies]`); add it there (full syntax tree and visitor features) and as a `zygo-core` dev-dependency with `toml` (`Cargo.toml:66`).
- Check item 7 reads the built-in minimum from `crates/zygo-config/data/agents.toml` as a TOML file (author): `zygo-core` cannot depend on `zygo-config`, which depends on it.
- Check item 8 needs the bar rule, which is CF-2's `derive::bar`. CF-1 holds a minimal private bar test (each core criterion `met` or excepted) behind one function that CF-2 replaces; the item 8 case passes at CF-1's merge and at CF-2's.
- Fixtures are small hand-written registries and grade files under `tests/conformance/fixtures/`, plus planted test sources and premise files; no case reads the real `docs/conformance/` (that is CF-4).

## Plan

Base: `origin/phase-4` at `9595de88`. Premises checked at the base: `enum Agent` at `crates/zygo-core/src/lib.rs:20-26` (no `ALL` yet); `#[path]` modules at `crates/zygo-launch/tests/premise.rs:4-5`; `minimum_agent_versions_match_premises` at `crates/zygo-config/tests/builtin.rs:223`; `crates/zygo-config/data/agents.toml` holds `[claude-code] min_version` and `[opencode] min_version`; `toml = "1.2.1"` at `Cargo.toml:66`; `syn` is not a workspace dependency, and `Cargo.lock` already holds `syn 3.0.10` (it has the `full` and `visit` features); `tempfile` is already a normal dependency of `zygo-core`; `.gitattributes` checks out every text file as LF on every OS. No drift.

### Interfaces

Consumes: none.

Produces (all in the test crate `conformance`, module `model`, file `crates/zygo-core/tests/conformance/model.rs`, except `Agent::ALL`):

- `zygo_core::Agent::ALL: [Agent; 2]` = `[Agent::ClaudeCode, Agent::OpenCode]`
- `Registry::load(path: &Path) -> Result<Registry, String>`, `Registry::parse(text: &str) -> Result<Registry, String>`
- `GradeFile::load(path: &Path) -> Result<GradeFile, String>`, `GradeFile::parse(text: &str) -> Result<GradeFile, String>`
- `Client { pub id: String, pub name: String, pub version: String, pub supported: bool }`
- `evidence(root: &Path, e: &str) -> Result<Evidence, String>`, `Evidence { pub platforms: Vec<String> }`
- `check(root: &Path, reg: &Registry, grades: &[GradeFile], clients: &[Client]) -> Vec<Fault>`, `Fault { pub item: u8, pub text: String }`
- Added by this plan (CF-4 uses it to build the client list): `clients(root: &Path) -> Result<Vec<Client>, String>`

The model types, exact (each struct `#[derive(Debug, Clone, Deserialize)]` and `#[serde(deny_unknown_fields)]`; each enum also `Copy, PartialEq, Eq`; `Client`, `Evidence`, `Fault` derive `Debug, Clone, PartialEq` and no serde):

```rust
pub struct Registry { pub capability: Vec<Capability>, #[serde(default)] pub bar_exception: Vec<BarException> }
pub struct Capability { pub id: String, pub name: String, pub tier: Tier, pub standard: String, pub criterion: Vec<Criterion> }
pub struct Criterion { pub id: String, pub text: String }
#[serde(rename_all = "lowercase")] pub enum Tier { Core, Standard, Extra }
pub struct BarException { pub client: String, pub criterion: String, pub reason: String, pub approved: Option<String> }
pub struct GradeFile { pub client: String, pub name: String, pub version: String,
    #[serde(default)] pub cell: Vec<CellUrl>, #[serde(default)] pub grade: Vec<GradeEntry> }
pub struct CellUrl { pub capability: String, pub url: String }
pub struct GradeEntry { pub criterion: String, pub status: Status,
    #[serde(default)] pub evidence: Vec<String>, pub reason: Option<String> }
pub enum Status { #[serde(rename = "met")] Met, #[serde(rename = "not-met")] NotMet, #[serde(rename = "n/a")] Na }
```

The names `CellUrl`, `GradeEntry` and `Status` keep `Cell` and `Grade` free for CF-2's `derive::Cell { grade: Grade, … }`.

### Rules the code must hold (from K§6.2.3 and K§6.2.5)

- `parse` (both files) refuses, with an `Err` whose text names the bad value or key: a TOML error (this covers unknown keys, a `bar_exception` key in a grade file, an unknown `tier` or `status`); an id that does not match `^(A|WA)-[0-9]+(\.[0-9]+)?$` in `capability.id`, `criterion.id`, `bar_exception.criterion`, `cell.capability`, `grade.criterion`; a duplicate capability id or criterion id in the registry; a `grade.reason`, `bar_exception.reason` or `cell.url` that holds `\n` or `\r` or has more than 300 `char`s; an evidence string that `parse_evidence` refuses. `load` = read the file, then `parse`; its `Err` text starts with `<path>: `.
- `parse_evidence` (private) accepts exactly three forms. `test:<path>::<fn>` (split at the last `::`): `<path>` starts with `crates/` and ends with `.rs`, `<fn>` matches `[A-Za-z_][A-Za-z0-9_]*`. `fixture:<path>`: `<path>` starts with `fixtures/`. `premise:<file>#<id>`: `<file>` is exactly `docs/premises.md` or `docs/live-checks.md`; `<id>` is not empty and has no whitespace and no `|`. Each `<path>` is refused when it is empty, `Path::is_absolute()`, starts with `/`, or holds `\`, `:` or a `..` component (split on `/`). Any other kind prefix is refused.
- `evidence(root, e)`: `parse_evidence`, then `root.join(path)`; the canonical path must exist and `starts_with` the canonical `root`, else `Err` with `leaves the repository`. `test:` → parse the file with `syn::parse_file`; walk `Item::Fn` at file level and in inline `Item::Mod` bodies (`content: Some(..)`), recursively; a test function is an `ItemFn` named `<fn>` with an attribute whose path is `test` or `tokio::test` (any arguments). Exactly one test function of that name must exist (zero → `Err` `no test fn`, two or more → `Err` `ambiguous`). It is refused when it has an attribute whose path is `ignore` (any form) or a `cfg_attr(…)` whose tokens hold the ident `ignore`. `platforms` = the token text (`MetaList.tokens.to_string()`) of each `#[cfg(…)]` on each enclosing module, outermost first, then on the function, in source order, leaving out a bare `test`. `fixture:` → the un-resolved path is not a symbolic link (`symlink_metadata`), is a regular file of more than 2 bytes, and its text with space, tab, CR and LF removed is not `{}` or `[]` (the `nontrivial` rule, `scripts/check-premises.sh:20-27`); `platforms` empty. `premise:` → the file has exactly one line that starts (after leading spaces) with `| <id> |`; split that line on `|` not preceded by `\`; cell 6 (cell 0 is the empty text before the first `|`), trimmed, is exactly `verified` for `docs/premises.md` or `pass` for `docs/live-checks.md`; `platforms` empty.
- `check` returns the faults in item order. Each `Fault.text` names the client id and the criterion or capability id it concerns, where one exists. Items:
  1. a client of `clients` with no grade file, or with two or more; a grade file whose `client` is not an id of `clients`;
  2. per grade file: a registry criterion with no grade; a grade for a criterion not in the registry; a criterion graded twice;
  3. per grade file: a `met` grade for which no evidence item gives `Ok` from `evidence` (an empty list included); the text lists each item's `Err`;
  4. per grade file: a `not-met` or `n/a` grade whose `reason` is `None` or blank; an `n/a` grade of a criterion of a `core` capability;
  5. per grade file: a registry capability with no `cell` entry whose `url` starts with `https://`;
  6. per registry `bar_exception`: `client` not an id of `clients`; `criterion` `met` in that client's grade file; `criterion` not a criterion of a `core` capability; `approved` `None` or blank;
  7. per client with exactly one grade file `g`: `g.version != client.version`; in `root/docs/premises.md`, before the first line that starts with `##`, no line equal to `<g.name>: <g.version>`; in `root/docs/live-checks.md`, likewise no line equal to `<g.name>: <g.version> or later` (a `\r` at a line end is removed first; an unreadable file is a fault);
  8. per client with `supported` and exactly one grade file: `failing_core(reg, g)` is not empty; the text names each failing capability id;
  9. per client: `root/fixtures/<id>/` is missing or has no entry.
- `failing_core(reg: &Registry, g: &GradeFile) -> Vec<String>` (private, with the doc line `CF-2 replaces this with derive::bar(reg, g).failing`): the ids, in registry order, of each core capability with a criterion that has no `met` grade in `g` and no registry `bar_exception` for `(g.client, criterion)`.
- `clients(root)`: reads `root/crates/zygo-config/data/agents.toml` as a `toml::Table`; for each `a` of `Agent::ALL`, `Client { id: a.as_str(), name, version: table[id]["min_version"], supported: true }`, where `name` comes from a private exhaustive `match a` (`ClaudeCode` → `Claude Code`, `OpenCode` → `OpenCode`; P4-6 replaces it with the descriptor); a missing file, table or key is an `Err`.

### Steps

- [x] **1. Dependencies.** `Cargo.toml` `[workspace.dependencies]`, after `sha2`: `syn = { version = "3.0.10", features = ["full", "visit"] }`. `crates/zygo-core/Cargo.toml`, a new `[dev-dependencies]` table: `syn.workspace = true` and `toml.workspace = true`. Run `cargo test -p zygo-core --no-run` (no `--locked`, so `Cargo.lock` takes the two new edges). Check: `git diff Cargo.lock` adds only `"syn 3.0.10"` and `"toml"` to the `zygo-core` package's `dependencies` and adds no `[[package]]`; `cargo test -p zygo-core --no-run --locked` exits 0.

- [x] **2. Test crate root and the failing `Agent::ALL` test.** Create `crates/zygo-core/tests/conformance.rs`:

  ```rust
  //! The conformance check and generator (K§6.2.5). CF-1: the input model and check items 1-9 on fixture inputs.
  #[allow(dead_code)] // later nodes read the rest of the model
  #[path = "conformance/model.rs"]
  mod model;
  ```
  and `crates/zygo-core/tests/conformance/model.rs` holding the types of "Interfaces" and the function signatures of "Interfaces" plus `parse_evidence`, `failing_core`, each body `todo!()`. Add the test:

  ```rust
  #[test]
  fn check_agent_all() {
      assert_eq!(Agent::ALL, [Agent::ClaudeCode, Agent::OpenCode]);
      for a in Agent::ALL { assert_eq!(a.as_str().parse::<Agent>().unwrap(), a); }
  }
  ```
  Run `cargo test -p zygo-core --test conformance -- check_agent_all` → fails to compile: no `ALL` on `Agent`.

- [x] **3. `Agent::ALL`.** In `crates/zygo-core/src/lib.rs`, in `impl Agent` (after the enum at `:20-26`): `pub const ALL: [Agent; 2] = [Agent::ClaudeCode, Agent::OpenCode];` with the doc line `Every client, in id order (P4-6 moves this to client/mod.rs, K§9).` Run the step 2 command → passes. Commit `feat(core): Agent::ALL for the conformance check (CF-1)`.

- [x] **4. Fixtures.** Under `crates/zygo-core/tests/conformance/fixtures/` write these files, exactly (no `Cargo.toml` anywhere under them; Cargo compiles none of the `.rs` files, the check only parses them):

  `registry.toml`: three capabilities, in this order. `A-1`, name `Transport`, tier `core`, standard `Each model request goes to the proxy.`, criteria `A-1.1` (`Each model request goes to the proxy.`) and `A-1.2` (`The credential is the session token.`). `A-3`, name `Model list`, tier `standard`, standard `The picker shows the roles.`, criterion `A-3.1` (`The picker shows the role list.`). `WA-2`, name `Turn signals`, tier `extra`, standard `zygo sees each turn.`, criterion `WA-2.1` (`Turn start.`). No `bar_exception`. Write each criterion as its own `[[capability.criterion]]` table and each value as a basic string on one line `key = "value"`.

  `alpha.toml`: `client = "alpha"`, `name = "Alpha"`, `version = "1.0.0"`; `[[cell]]` for `A-1`, `A-3`, `WA-2` with `url = "https://example.com/alpha/transport"`, `…/alpha/models"`, `…/alpha/turns"`; `[[grade]]` in this order: `A-1.1` `met` `evidence = ["test:crates/demo/tests/ev.rs::plain"]`; `A-1.2` `met` `evidence = ["premise:docs/premises.md#Q1"]`; `A-3.1` `met` `evidence = ["test:crates/demo/tests/ev.rs::tokio_plain"]`; `WA-2.1` `n/a` `reason = "Alpha has no turn events"`.

  `beta.toml`: `client = "beta"`, `name = "Beta"`, `version = "2.0.0"`; cells as alpha with `/beta/`; grades: `A-1.1` `met` `["fixture:fixtures/beta/capture.json"]`; `A-1.2` `met` `["premise:docs/live-checks.md#L1"]`; `A-3.1` `not-met` `reason = "Beta has no model picker"`; `WA-2.1` `met` `["test:crates/demo/tests/ev.rs::unix_only"]`.

  `grade-with-bar-exception.toml`: the text of `alpha.toml` followed by `[[bar_exception]]` with `client = "alpha"`, `criterion = "A-1.2"`, `reason = "planted"`, `approved = "operator"`.

  `repo/docs/premises.md`:
  ```
  # Fixture premises

  Alpha: 1.0.0
  Beta: 2.0.0

  ## Results

  | Id | Item | Agent | Version | Date | Result | Evidence |
  |---|---|---|---|---|---|---|
  | Q1 | An item with a \| in its text | Alpha | 1.0.0 | 2026-11-25 | verified | `fixtures/alpha/a.json` |
  | Q2 | A failed item | Alpha | 1.0.0 | 2026-11-25 | failed | `fixtures/alpha/a.json` |
  | Q3 | An observed item | Alpha | 1.0.0 | 2026-11-25 | observed | `fixtures/alpha/a.json` |
  | Q9 | A row given twice | Alpha | 1.0.0 | 2026-11-25 | verified | `fixtures/alpha/a.json` |
  | Q9 | A row given twice | Alpha | 1.0.0 | 2026-11-25 | verified | `fixtures/alpha/a.json` |
  ```
  `repo/docs/live-checks.md`: the same layout with title `# Fixture live checks`, header lines `Alpha: 1.0.0 or later` and `Beta: 2.0.0 or later`, column `Check` in place of `Item`, and rows `| L1 | A passed check | Beta | 2.0.0 | 2026-11-25 | pass | `fixtures/beta/capture.json` |` and `| L2 | A check not run | Beta | 2.0.0 | 2026-11-25 | not run: no key | — |`.

  `repo/fixtures/alpha/a.json` = `{"a": 1}` + LF; `repo/fixtures/beta/capture.json` = `{"request": "fixture"}` + LF; `repo/fixtures/beta/empty.json` = `{ }` + LF; `repo/fixtures/beta/list.json` = `[` LF `]` LF; `repo/fixtures/beta/tiny.txt` = `ab` with no LF (`printf 'ab' >`, 2 bytes).

  `repo/crates/zygo-config/data/agents.toml`: `[claude-code]` `min_version = "9.1.0"`, `[opencode]` `min_version = "9.2.0"`.

  `repo/crates/demo/src/lib.rs`: `#[cfg(test)] mod tests { #[test] fn only_in_lib() {} }`.

  `repo/crates/demo/tests/ev.rs`, one item per entry: `#[test] fn plain() {}`; `#[tokio::test] async fn tokio_plain() {}`; `#[tokio::test(flavor = "multi_thread")] async fn tokio_multi() {}`; `#[test] #[ignore] fn ignored_plain() {}`; `#[test] #[ignore = "needs a live key"] fn ignored_reason() {}`; `#[test] #[cfg_attr(windows, ignore)] fn ignored_cfg_attr() {}`; `fn helper_not_test() {}`; `#[cfg(unix)] #[test] fn unix_only() {}`; `#[cfg(windows)] mod win { #[test] fn in_windows_mod() {} }`; `#[cfg(test)] mod inner { #[test] fn in_cfg_test_mod() {} }`; `mod a { #[test] fn dup() {} }`; `mod b { #[test] fn dup() {} }`. Each attribute on its own line.

  Check: `wc -c repo/fixtures/beta/tiny.txt` prints 2; `git status` shows only new files.

- [x] **5. Test helpers.** In `conformance.rs`, `use model::*;` and these helpers (bodies are the obvious ones):
  `fn fx() -> PathBuf` (`CARGO_MANIFEST_DIR/tests/conformance/fixtures`); `fn repo() -> PathBuf` (`fx()/repo`); `fn text(name: &str) -> String` (read `fx()/name`); `fn client(id: &str, name: &str, version: &str) -> Client` (`supported: true`); `fn base() -> (Registry, Vec<GradeFile>, Vec<Client>)` (`Registry::load(fx/registry.toml)`, `GradeFile::load` of `alpha.toml` then `beta.toml`, clients `alpha`/`Alpha`/`1.0.0` and `beta`/`Beta`/`2.0.0`, each `.unwrap()`); `fn items(f: &[Fault]) -> BTreeSet<u8>`; `fn set(v: &[u8]) -> BTreeSet<u8>`; `fn entry<'a>(g: &'a mut GradeFile, criterion: &str) -> &'a mut GradeEntry`; `fn with_evidence(ev: &[&str]) -> BTreeSet<u8>` (base, set alpha's `A-3.1` evidence to `ev`, return `items(&check(&repo(), …))`); `fn copy_repo() -> tempfile::TempDir` (recursive copy of `repo()` into a new temporary folder).

- [x] **6. Failing input-rule tests.** Each `parse` call below on fixture text with one change:

  ```rust
  #[test] fn input_unknown_key_refused() {
      let e = Registry::parse(&(text("registry.toml") + "\n[extra]\nx = 1\n")).unwrap_err(); assert!(e.contains("extra"));
      let e = Registry::parse(&text("registry.toml").replace("tier = \"core\"", "tier = \"core\"\ncolor = \"red\"")).unwrap_err(); assert!(e.contains("color"));
      let e = GradeFile::parse(&text("alpha.toml").replacen("status = \"met\"", "status = \"met\"\nweight = 2", 1)).unwrap_err(); assert!(e.contains("weight"));
  }
  #[test] fn input_bad_id_refused() {
      for bad in ["A-7.1x", "B-1", "A-1.2.3", "a-1", "A-", "WA-1."] {
          let q = format!("\"{bad}\"");
          assert!(Registry::parse(&text("registry.toml").replace("\"A-3.1\"", &q)).unwrap_err().contains(bad));
          assert!(GradeFile::parse(&text("alpha.toml").replace("\"A-3.1\"", &q)).unwrap_err().contains(bad));
          assert!(GradeFile::parse(&text("alpha.toml").replace("capability = \"A-3\"", &format!("capability = {q}"))).unwrap_err().contains(bad));
      }
      let ok = text("registry.toml").replace("\"A-3\"", "\"WA-30\"").replace("\"A-3.1\"", "\"WA-30.12\"");
      assert!(Registry::parse(&ok).is_ok());
  }
  #[test] fn input_evidence_path_absolute_or_dotdot_refused() {
      for bad in ["test:/crates/x.rs::f", "fixture:/etc/passwd", "fixture:C:/x.json", r"fixture:fixtures\beta\capture.json",
                  "fixture:fixtures/../crates/demo/tests/ev.rs", "test:crates/../../x.rs::f", "test:docs/x.rs::f",
                  "premise:docs/other.md#Q1", "doc:readme"] {
          let t = text("alpha.toml").replace("\"test:crates/demo/tests/ev.rs::plain\"", &format!("'{bad}'"));
          assert!(GradeFile::parse(&t).is_err(), "{bad}");
          assert!(evidence(&repo(), bad).is_err(), "{bad}");
      }
  }
  #[test] fn input_reason_or_url_line_break_refused() {
      for brk in ["\\n", "\\r"] {
          assert!(GradeFile::parse(&text("alpha.toml").replace("Alpha has no turn events", &format!("Alpha has{brk}no turn events"))).is_err());
          assert!(GradeFile::parse(&text("alpha.toml").replace("alpha/models", &format!("alpha/{brk}models"))).is_err());
      }
      let reg = text("registry.toml") + "\n[[bar_exception]]\nclient = \"alpha\"\ncriterion = \"A-1.2\"\nreason = \"one\\ntwo\"\napproved = \"operator\"\n";
      assert!(Registry::parse(&reg).is_err());
  }
  #[test] fn input_reason_or_url_over_300_refused() {
      let r = |n: usize| text("alpha.toml").replace("Alpha has no turn events", &"x".repeat(n));
      assert!(GradeFile::parse(&r(300)).is_ok());
      assert!(GradeFile::parse(&r(301)).is_err());
      let u = text("alpha.toml").replace("https://example.com/alpha/models", &format!("https://example.com/{}", "x".repeat(300)));
      assert!(GradeFile::parse(&u).is_err());
  }
  #[test] fn input_unknown_status_or_tier_refused() {
      assert!(GradeFile::parse(&text("alpha.toml").replacen("status = \"met\"", "status = \"partial\"", 1)).is_err());
      assert!(Registry::parse(&text("registry.toml").replace("tier = \"core\"", "tier = \"optional\"")).is_err());
  }
  #[test] fn input_duplicate_registry_id_refused() {
      let dup = "\n[[capability]]\nid = \"A-3\"\nname = \"Again\"\ntier = \"standard\"\nstandard = \"s\"\n\n[[capability.criterion]]\nid = \"A-3.9\"\ntext = \"t\"\n";
      assert!(Registry::parse(&(text("registry.toml") + dup)).unwrap_err().contains("A-3"));
      let dup = dup.replace("\"A-3\"", "\"A-4\"").replace("A-3.9", "A-3.1");
      assert!(Registry::parse(&(text("registry.toml") + &dup)).unwrap_err().contains("A-3.1"));
  }
  ```
  Run `cargo test -p zygo-core --test conformance -- input_` → the 7 tests fail (`not yet implemented`).

- [x] **7. `parse`, `load`, `parse_evidence`.** Implement them by the rules above, with a private `fn valid_id(id: &str) -> bool` for the id pattern (no new dependency). Run the step 6 command → 7 pass. Commit `feat(conformance): registry and grade-file model with the input rules (CF-1)`.

- [x] **8. Failing evidence tests (K§11 item 2).**

  ```rust
  #[test] fn check_cfg_unix_test_counts_with_platform() {
      let p = |f: &str| evidence(&repo(), &format!("test:crates/demo/tests/ev.rs::{f}")).unwrap().platforms;
      assert_eq!(p("unix_only"), ["unix"]);
      assert_eq!(p("in_windows_mod"), ["windows"]);
      assert!(p("in_cfg_test_mod").is_empty());
      assert!(p("plain").is_empty());
  }
  #[test] fn check_tokio_multi_thread_test_counts() {
      for f in ["tokio_multi", "tokio_plain"] {
          assert_eq!(evidence(&repo(), &format!("test:crates/demo/tests/ev.rs::{f}")), Ok(Evidence { platforms: vec![] }));
      }
  }
  #[cfg(unix)] #[test] fn input_evidence_link_leaving_repo_refused() {
      let (d, out) = (copy_repo(), tempfile::tempdir().unwrap());
      std::fs::write(out.path().join("x.json"), "{\"outside\": true}\n").unwrap();
      std::fs::copy(repo().join("crates/demo/tests/ev.rs"), out.path().join("ev.rs")).unwrap();
      std::os::unix::fs::symlink(out.path().join("x.json"), d.path().join("fixtures/beta/out.json")).unwrap();
      std::os::unix::fs::symlink(out.path().join("ev.rs"), d.path().join("crates/demo/tests/link.rs")).unwrap();
      assert!(evidence(d.path(), "fixture:fixtures/beta/out.json").unwrap_err().contains("leaves the repository"));
      assert!(evidence(d.path(), "test:crates/demo/tests/link.rs::plain").unwrap_err().contains("leaves the repository"));
  }
  #[cfg(unix)] #[test] fn check_3_symlink_fixture_not_evidence() {
      let d = copy_repo();
      std::os::unix::fs::symlink("capture.json", d.path().join("fixtures/beta/link.json")).unwrap();
      assert!(evidence(d.path(), "fixture:fixtures/beta/link.json").is_err());
      assert!(evidence(d.path(), "fixture:fixtures/beta/capture.json").is_ok());
  }
  ```
  Run `cargo test -p zygo-core --test conformance -- check_cfg check_tokio input_evidence_link check_3_symlink` → each fails.

- [x] **9. `evidence`.** Implement it by the rules above (`syn::parse_file`, recursion into inline modules with a stack of `cfg` texts). Run the step 8 command → passes (on Windows the two `#[cfg(unix)]` tests do not exist). Commit `feat(conformance): evidence kinds test, fixture and premise (CF-1)`.

- [ ] **10. Failing check tests (items 1-9; K§11 item 1).** Each case changes one thing in `base()` and asserts the exact item set:

  ```rust
  #[test] fn check_valid_fixtures_pass() { let (r, g, c) = base(); assert_eq!(check(&repo(), &r, &g, &c), vec![]); }
  #[test] fn check_1_client_without_grade_file() {
      let (r, g, mut c) = base(); c.push(client("gamma", "Gamma", "3.0.0"));
      let f = check(&repo(), &r, &g, &c);
      assert_eq!(items(&f), set(&[1, 9])); assert!(f[0].text.contains("gamma"));
  }
  #[test] fn check_1_grade_file_unknown_or_twice() {
      let (r, g, c) = base();
      let mut d = g.clone(); let mut x = g[1].clone(); x.client = "delta".into(); d.push(x);
      assert_eq!(items(&check(&repo(), &r, &d, &c)), set(&[1]));
      let mut d = g.clone(); d.push(g[0].clone());
      assert_eq!(items(&check(&repo(), &r, &d, &c)), set(&[1]));
  }
  #[test] fn check_2_missing_grade() { let (r, mut g, c) = base(); g[0].grade.retain(|e| e.criterion != "A-3.1");
      assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[2])); }
  #[test] fn check_2_unknown_criterion() { let (r, mut g, c) = base();
      let mut e = entry(&mut g[0], "A-1.1").clone(); e.criterion = "A-9.1".into(); g[0].grade.push(e);
      assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[2])); }
  #[test] fn check_2_duplicate_grade() { let (r, mut g, c) = base();
      let e = entry(&mut g[0], "A-3.1").clone(); g[0].grade.push(e);
      assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[2])); }
  #[test] fn check_3_ignored_test_not_evidence() {
      for f in ["ignored_plain", "ignored_reason", "ignored_cfg_attr"] {
          assert_eq!(with_evidence(&[&format!("test:crates/demo/tests/ev.rs::{f}")]), set(&[3]), "{f}"); } }
  #[test] fn check_3_missing_or_plain_function_not_evidence() {
      for e in ["test:crates/demo/tests/ev.rs::no_such_fn", "test:crates/demo/tests/ev.rs::helper_not_test",
                "test:crates/demo/tests/ev.rs::dup", "test:crates/demo/tests/none.rs::plain"] {
          assert_eq!(with_evidence(&[e]), set(&[3]), "{e}"); }
      assert_eq!(with_evidence(&[]), set(&[3]));
      assert_eq!(with_evidence(&["test:crates/demo/tests/ev.rs::no_such_fn", "test:crates/demo/tests/ev.rs::plain"]), set(&[]));
  }
  #[test] fn check_3_cfg_test_fn_in_other_file_not_evidence() {
      assert_eq!(with_evidence(&["test:crates/demo/tests/ev.rs::only_in_lib"]), set(&[3]));
      assert_eq!(with_evidence(&["test:crates/demo/src/lib.rs::only_in_lib"]), set(&[])); }
  #[test] fn check_3_premise_failed_or_observed_not_evidence() {
      assert_eq!(with_evidence(&["premise:docs/premises.md#Q2"]), set(&[3]));
      assert_eq!(with_evidence(&["premise:docs/premises.md#Q3"]), set(&[3]));
      assert_eq!(with_evidence(&["premise:docs/premises.md#Q1"]), set(&[])); }
  #[test] fn check_3_premise_id_in_other_file_only() {
      assert_eq!(with_evidence(&["premise:docs/premises.md#L1"]), set(&[3]));
      assert_eq!(with_evidence(&["premise:docs/live-checks.md#Q1"]), set(&[3])); }
  #[test] fn check_3_premise_not_run_absent_or_twice_not_evidence() {
      for e in ["premise:docs/live-checks.md#L2", "premise:docs/premises.md#Q8", "premise:docs/premises.md#Q9"] {
          assert_eq!(with_evidence(&[e]), set(&[3]), "{e}"); } }
  #[test] fn check_3_trivial_fixture_not_evidence() {
      for e in ["fixture:fixtures/beta/empty.json", "fixture:fixtures/beta/list.json", "fixture:fixtures/beta/tiny.txt",
                "fixture:fixtures/beta/missing.json", "fixture:fixtures/beta"] {
          assert_eq!(with_evidence(&[e]), set(&[3]), "{e}"); }
      assert_eq!(with_evidence(&["fixture:fixtures/beta/capture.json"]), set(&[])); }
  #[test] fn check_4_reason_and_core_na() {
      let (r, g0, c) = base();
      let mut g = g0.clone(); entry(&mut g[0], "WA-2.1").reason = None;
      assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[4]));
      let mut g = g0.clone(); entry(&mut g[1], "A-3.1").reason = Some(" ".into());
      assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[4]));
      let mut g = g0.clone(); { let e = entry(&mut g[0], "A-1.1"); e.status = Status::Na; e.reason = Some("x".into()); }
      assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[4, 8]));
  }
  #[test] fn check_5_cell_url() {
      let (r, g0, c) = base();
      let mut g = g0.clone(); g[0].cell.retain(|x| x.capability != "A-3");
      assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[5]));
      let mut g = g0.clone(); g[0].cell.iter_mut().find(|x| x.capability == "A-3").unwrap().url = "http://example.com".into();
      assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[5]));
  }
  #[test] fn check_6_bar_exception_faults() {
      let x = |client: &str, criterion: &str, approved: Option<&str>| BarException { client: client.into(),
          criterion: criterion.into(), reason: "r".into(), approved: approved.map(Into::into) };
      let (r0, g0, c) = base();
      for b in [x("gamma", "A-1.2", Some("operator")), x("alpha", "A-1.1", Some("operator")), x("beta", "A-3.1", Some("operator"))] {
          let mut r = r0.clone(); r.bar_exception.push(b);
          assert_eq!(items(&check(&repo(), &r, &g0, &c)), set(&[6])); }
      let mut g = g0.clone(); { let e = entry(&mut g[0], "A-1.2"); e.status = Status::NotMet; e.reason = Some("no helper".into()); }
      for a in [None, Some(" ")] {
          let mut r = r0.clone(); r.bar_exception.push(x("alpha", "A-1.2", a));
          assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[6])); }
  }
  #[test] fn check_6_bar_exception_key_in_grade_file() {
      assert!(GradeFile::load(&fx().join("grade-with-bar-exception.toml")).unwrap_err().contains("bar_exception")); }
  #[test] fn check_7_version_faults() {
      let (r, g, mut c) = base();
      c[0].version = "1.0.1".into();
      let f = check(&repo(), &r, &g, &c); assert_eq!(items(&f), set(&[7])); assert!(f[0].text.contains("alpha"));
      let (_, _, c) = base();
      for (file, from, to) in [("docs/premises.md", "Alpha: 1.0.0\n", "Alpha: 1.0.2\n"),
                               ("docs/live-checks.md", "Alpha: 1.0.0 or later", "Alpha: 1.0.0"),
                               ("docs/premises.md", "Beta: 2.0.0\n", "")] {
          let d = copy_repo(); let p = d.path().join(file);
          std::fs::write(&p, std::fs::read_to_string(&p).unwrap().replace(from, to)).unwrap();
          assert_eq!(items(&check(d.path(), &r, &g, &c)), set(&[7]), "{file} {from}"); }
  }
  #[test] fn check_8_supported_client_below_bar() {
      let (r, mut g, mut c) = base();
      { let e = entry(&mut g[0], "A-1.2"); e.status = Status::NotMet; e.reason = Some("no key helper".into()); }
      let f = check(&repo(), &r, &g, &c); assert_eq!(items(&f), set(&[8])); assert!(f[0].text.contains("A-1"));
      c[0].supported = false;
      assert_eq!(check(&repo(), &r, &g, &c), vec![]);
  }
  #[test] fn check_8_registry_exception_meets_bar() {
      let (mut r, mut g, c) = base();
      { let e = entry(&mut g[0], "A-1.2"); e.status = Status::NotMet; e.reason = Some("no key helper".into()); }
      r.bar_exception.push(BarException { client: "alpha".into(), criterion: "A-1.2".into(), reason: "r".into(), approved: Some("operator 2026-11-25".into()) });
      assert_eq!(check(&repo(), &r, &g, &c), vec![]);
  }
  #[test] fn check_9_fixtures_folder_missing_or_empty() {
      let (r, g, c) = base(); let d = copy_repo();
      std::fs::remove_dir_all(d.path().join("fixtures/alpha")).unwrap();
      assert_eq!(items(&check(d.path(), &r, &g, &c)), set(&[9]));
      std::fs::create_dir(d.path().join("fixtures/alpha")).unwrap();
      assert_eq!(items(&check(d.path(), &r, &g, &c)), set(&[9]));
  }
  ```
  Run `cargo test -p zygo-core --test conformance -- check_` → the tests of this step fail (`not yet implemented`); steps 2 and 8 tests still pass.

- [ ] **11. `check` and `failing_core`.** Implement them by the rules above. Run `cargo test -p zygo-core --test conformance -- check_` → all pass. Commit `feat(conformance): check items 1-9 on fixture inputs (CF-1)`.

- [ ] **12. Failing `clients` test.**

  ```rust
  #[test] fn check_clients_from_agent_all() {
      let c = clients(&repo()).unwrap();
      assert_eq!(c, vec![Client { id: "claude-code".into(), name: "Claude Code".into(), version: "9.1.0".into(), supported: true },
                         Client { id: "opencode".into(), name: "OpenCode".into(), version: "9.2.0".into(), supported: true }]);
      assert!(clients(tempfile::tempdir().unwrap().path()).is_err());
  }
  ```
  Run `cargo test -p zygo-core --test conformance -- check_clients` → fails.

- [ ] **13. `clients`.** Implement it by the rule above. Run the step 12 command → passes. Commit `feat(conformance): client list from Agent::ALL (CF-1)`.

- [ ] **14. Verification.** Run the Acceptance command:
  `test "$(cargo test -q -p zygo-core --test conformance -- check_ input_ --list 2>/dev/null | grep -c ': test$')" -ge 19 && cargo test -p zygo-core --test conformance --locked -- check_ input_`
  → exits 0; the count is 34 on Linux and macOS (25 `check_` and 7 `input_` tests, plus the 2 `#[cfg(unix)]` ones) and 32 on Windows. The symbolic-link cases of review focus 1 and 2 run only on unix (a Windows link needs a privilege the CI runner does not give). Then `cargo test -p zygo-core --locked` and `cargo test -p zygo-config --test builtin --locked -- --exact minimum_agent_versions_match_premises` → both exit 0 (K§11 item 7: `builtin.rs` is not changed). Then `cargo test --workspace --locked --no-fail-fast` → exits 0. Check `git diff --stat origin/phase-4` lists only the node's Files. No test sleeps, reads a clock or starts a process.

- [ ] **15. Push.** Every commit message ends with the `Zygo-Session: <session>` line of your run and the line `Zygo-Work: phase4:CF-1`; no Claude attribution. `git push -u origin <your node branch>`; CI is green on ubuntu, macOS and Windows.

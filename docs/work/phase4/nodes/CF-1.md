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

Written when the node starts.

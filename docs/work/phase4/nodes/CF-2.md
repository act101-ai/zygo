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

Written when the node starts.

# CF-3 — Conformance outputs: feature matrix, rollup and score in Markdown and JSON; write mode and check item 10

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.3 (the platforms recorded next to a cfg-limited test), K§6.2.5 item 10 and the write-mode paragraph, K§6.2.6, K§10 (text layout and key order are the builder's), K§11 items 2 (the platform shown in the document), 3 (generator side) and 4; K-D29, K-D30, K-D33
**Files:** `crates/zygo-core/tests/conformance.rs`, `crates/zygo-core/tests/conformance/render.rs` (new), `crates/zygo-core/tests/conformance/fixtures/`
**Consumes:** `derive::cell` (CF-2), `derive::scores` (CF-2), `derive::bar` (CF-2), `derive::rollup` (CF-2), `Registry::load` (CF-1), `GradeFile::load` (CF-1), `model::Client` (CF-1), `model::resolve` and `model::Resolved` (CF-1: each `met` grade's evidence with its platforms, resolved once against the repository, so render reads no file)
**Produces:** `render::markdown(reg: &Registry, grades: &[GradeFile], clients: &[Client], res: &Resolved) -> String`, `render::json(reg: &Registry, grades: &[GradeFile], clients: &[Client], res: &Resolved) -> String`, `render::compare_or_write(root: &Path, md: &str, json: &str) -> Result<(), Fault>` (writes both files when `ZYGO_WRITE_CONFORMANCE=1`; else compares after CR LF → LF; a difference is fault item 10)
**Acceptance:** `test "$(cargo test -q -p zygo-core --test conformance -- render_ --list 2>/dev/null | grep -c ': test$')" -ge 8 && cargo test -p zygo-core --test conformance --locked` → at least 8 tests named `render_…` (a count of 0 fails), then the whole `conformance` target passes on ubuntu, macOS and Windows CI: matrix rows grouped core, standard, extra in registry order; the four cell forms of K§6.2.6 item 1; the JSON `matrix` holds the same grades and values as the Markdown; `unix` shown next to a `cfg(unix)` evidence; a planted difference in either file is fault item 10; CR LF and LF compare equal; a `|` in a text is escaped; the first Markdown line and the JSON `generated_by` name the generator; two renders are byte-equal; after the three outputs, the Markdown lists per client each unmet criterion with its reason, each `n/a` with its reason and the evidence of each met criterion

## Planning notes

- The real-tree run (reading `docs/conformance/` and `Agent::ALL`, writing `docs/conformance.md` and `.json`) is CF-4. This node tests on fixtures in a temporary root.

## Plan

Written when the node starts.

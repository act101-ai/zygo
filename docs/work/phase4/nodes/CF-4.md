# CF-4 — Registry, Claude Code and OpenCode grade files, generated outputs, the tree test, CI paths, old scripts deleted

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.1, K§6.2.2 (the migration table and tiers), K§6.2.3 (grade form; bar exception A-1.4), K§6.2.5 (the check on the tree; script deletion), K§6.6 (owners), K§9 (P4-2), K§11 items 1 (on the tree), 3 and 7; K-D9, K-D10, K-D18, K-D26
**Files:** `docs/conformance/capabilities.toml` (new), `docs/conformance/claude-code.toml` (new), `docs/conformance/opencode.toml` (new), `docs/conformance.md`, `docs/conformance.json` (new), `crates/zygo-core/tests/conformance.rs`, `.github/workflows/ci.yml`, `scripts/check-conformance.sh` (deleted), `scripts/test-check-conformance.sh` (deleted)
**Consumes:** `model::check` (CF-1), `Registry::load` (CF-1), `GradeFile::load` (CF-1), `model::Client` (CF-1), `zygo_core::Agent::ALL` (CF-1), `render::markdown` (CF-3), `render::json` (CF-3), `render::compare_or_write` (CF-3)
**Produces:** `conformance_tree()` (the `#[test]` that runs the check and the generator on the repository), `docs/conformance/capabilities.toml`, `docs/conformance/claude-code.toml`, `docs/conformance/opencode.toml`
**Acceptance:** `cargo test -p zygo-core --test conformance --locked -- --exact conformance_tree && ZYGO_WRITE_CONFORMANCE=1 cargo test -p zygo-core --test conformance --locked -- --exact conformance_tree && git diff --exit-code docs/conformance.md docs/conformance.json && cargo test -p zygo-config --test builtin --locked -- --exact minimum_agent_versions_match_premises && git diff --exit-code origin/phase-4 -- crates/zygo-config/tests/builtin.rs && test ! -e scripts/check-conformance.sh && test ! -e scripts/test-check-conformance.sh && grep -q "docs/conformance" .github/workflows/ci.yml && grep -q "docs/live-checks.md" .github/workflows/ci.yml` → every command exits 0, and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI

## Planning notes

- The pull request stops for the operator before merge (K-D26): its body lists each derived cell that differs from the hand grade in today's `docs/conformance.md`, with its reason.
- A core criterion that grades `not-met` for Claude Code or OpenCode with no registry exception fails check item 8 (both count as supported until P4-6). The only approved exception is Claude Code A-1.4 (K-D18). Any other one is a new bar exception, which stops for the operator (K§6.2.1): the node goes `BLOCKED(operator: bar exception <client> <criterion>)` with the evidence. No grade is raised to avoid it (K§3).
- Evidence counts only by K§6.2.3. `docs/premises.md` rows P6a, Q7, PW6, PW7, PW15, PW15-bash and PW16 are `failed` at `ec17582b`, so they do not count. The K§6.2.3 example cites Q7 for A-1.2; that example shows the form, not the evidence.
- The hand notes of today's `docs/conformance.md` ("Windows shell for the generated commands", "Documentation caveat (OpenCode)", per-cell detail) do not survive generation. Their content goes into the `reason` of the grade or bar exception it explains, inside the 300-character rule; the A-1.4 exception may keep the K§6.2.3 example reason.
- CI paths (`.github/workflows/ci.yml:5-24`) skip `docs/**` except three files. Add `docs/conformance/**`, `docs/conformance.md`, `docs/conformance.json` and `docs/live-checks.md` to both lists, so a change of a grade, an output or a premise result runs the check in CI (K§11 item 3).
- `minimum_agent_versions_match_premises` (`crates/zygo-config/tests/builtin.rs:223`) stays unchanged (K§11 item 7).

## Plan

Written when the node starts.

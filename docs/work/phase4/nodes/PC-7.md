# PC-7 — `docs/premises.md` and `docs/live-checks.md`: `Codex` headers, rows PC1-PC16, LC-CX1, Codex method lines

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 (the table and the last paragraph), K§9 (P4-3), K§11 items 8, 9 and 10
**Files:** `docs/premises.md`, `docs/live-checks.md`
**Consumes:** `scripts/check-premises.sh` (PC-1), `fixtures/codex/requests/` (PC-3), `fixtures/codex/config/` (PC-4), `fixtures/codex/agents/` (PC-5), `fixtures/codex/workspace/` (PC-6)
**Produces:** `docs/premises.md` header `Codex: 0.172.0` and rows `PC1` … `PC16`, `docs/live-checks.md` header `Codex: 0.172.0 or later` and row `LC-CX1`
**Acceptance:** `N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(grep -cE '^\| PC([1-9]|1[0-6]) \|' docs/premises.md)" -eq 16 && grep -qx 'Codex: 0.172.0' docs/premises.md && grep -qx 'Codex: 0.172.0 or later' docs/live-checks.md && grep -q '^| LC-CX1 |' docs/live-checks.md && cargo test -p zygo-config --test builtin --locked -- --exact minimum_agent_versions_match_premises` → every command exits 0: 16 PC rows, each with a date, a result (`verified` or `failed`) and fixture paths that pass the `nontrivial` rule (K§11 item 9, checked by PC-1's script); the fixture scan passes (K§11 item 10)

## Planning notes

- Each row's date, result and paths come from the `- PCn: <result> <date>: <paths>` lines of "Observed" in `nodes/PC-3.md` to `nodes/PC-6.md`; PC1 to PC16 map to those nodes as their contracts list. The PC16 row states the scope recorded in PC-6 (the client default against the mock; the measurement under a zygo launch is P4-9 and P4-11).
- `LC-CX1` is owner-run: its row is `not run: owner-run (K§6.3.7 PC16)` until the operator runs it (`docs/live-checks.md` "Method": live checks run by hand only).
- The "Method" section of `docs/premises.md` gains the Codex lines: `CODEX_HOME`, `HTTPS_PROXY` at the mock, no login (K§6.3.7).
- Phase 3 adds a section to `docs/live-checks.md` (K§6.9). The two edits are in different places; a conflict at the phase 3 merge is resolved by keeping both.

## Plan

Written when the node starts.

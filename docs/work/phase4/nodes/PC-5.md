# PC-5 — Codex agents, compaction, MCP, status line and skills captures: PC6, PC8, PC9, PC11, PC15

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 rows PC6, PC8, PC9, PC11, PC15; K§6.4.5 (the subagent marker PC6 looks for); K§11 items 9 and 10 (for these fixtures)
**Files:** `scripts/premises/codex/session.sh` (new), `fixtures/codex/agents/` (new), `fixtures/codex/compaction/` (new), `fixtures/codex/mcp/` (new), `fixtures/codex/status/` (new), `fixtures/codex/skills/` (new)
**Consumes:** `run_codex` (PC-2), `codex_env` (PC-2)
**Produces:** `fixtures/codex/agents/` (PC6), `fixtures/codex/compaction/` (PC8), `fixtures/codex/mcp/` (PC9), `fixtures/codex/status/` (PC11), `fixtures/codex/skills/` (PC15), and in this node file under "Observed": the date, result and fixture paths of PC6, PC8, PC9, PC11, PC15
**Acceptance:** `sh scripts/premises/run-codex.sh session && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && for d in agents compaction mcp status skills; do test "$(find fixtures/codex/$d -type f -size +2c | wc -l)" -ge 1 || exit 1; done` → every command exits 0 and each of the five folders holds a nontrivial capture; each of PC6, PC8, PC9, PC11, PC15 has a dated result and fixture paths under "Observed" in this file

## Planning notes

- PC9 uses a probe MCP server (`scripts/premises/mcp_probe.py` precedent), not `zygo mcp`.
- PC-7 writes the rows from "Observed".

## Observed

Written when the node runs.

## Plan

Written when the node starts.

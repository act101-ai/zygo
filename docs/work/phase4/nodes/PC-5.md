# PC-5 — Codex agents, compaction, MCP, status line and skills captures: PC6, PC8, PC9, PC11, PC15

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 rows PC6, PC8, PC9, PC11, PC15; K§6.4.5 (the subagent marker PC6 looks for); K§11 items 9 and 10 (for these fixtures)
**Files:** `scripts/premises/codex/session.sh` (new), `fixtures/codex/agents/` (new), `fixtures/codex/compaction/` (new), `fixtures/codex/mcp/` (new), `fixtures/codex/status/` (new), `fixtures/codex/skills/` (new), `fixtures/codex/runs/session/` (new)
**Consumes:** `run_codex` (PC-2), `codex_env` (PC-2), `codex_record` (PC-2)
**Produces:** `fixtures/codex/agents/` (PC6), `fixtures/codex/compaction/` (PC8), `fixtures/codex/mcp/` (PC9), `fixtures/codex/status/` (PC11), `fixtures/codex/skills/` (PC15), and in this node file under "Observed" one line per id: `- PCn: <verified|failed> <YYYY-MM-DD>: <fixture paths>`
**Acceptance:** `sh scripts/premises/run-codex.sh session && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(for d in agents compaction mcp status skills; do find fixtures/codex/$d -type f -size +2c -print -quit; done | grep -c .)" -eq 5 && grep -qx 'home=scratch' fixtures/codex/runs/session/isolation.txt && grep -qx 'codex_home=scratch' fixtures/codex/runs/session/isolation.txt && test -f fixtures/codex/runs/session/connect.txt && test -z "$(grep -v ' refused$' fixtures/codex/runs/session/connect.txt)" && ! grep -q 'Written when the node runs' docs/work/phase4/nodes/PC-5.md && test "$(grep -cE '^- (PC6|PC8|PC9|PC11|PC15): (verified|failed) [0-9]{4}-[0-9]{2}-[0-9]{2}: fixtures/codex/' docs/work/phase4/nodes/PC-5.md)" -eq 5` → every command exits 0: each of the five folders holds a nontrivial capture; the run used scratch `HOME` and `CODEX_HOME`, and every `CONNECT` was refused; "Observed" has no placeholder and one dated result line for each of PC6, PC8, PC9, PC11, PC15

## Planning notes

- PC9 uses a probe MCP server (`scripts/premises/mcp_probe.py` precedent), not `zygo mcp`.
- PC8's raised usage comes from the PC-2 mock deliverable 3; every mock and scrub change this node needs is a PC-2 deliverable. PC-7 writes the rows from "Observed".

## Observed

Written when the node runs.

## Plan

Written when the node starts.

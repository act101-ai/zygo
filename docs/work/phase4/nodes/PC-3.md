# PC-3 — Codex proxy-facing captures: PC1, PC2, PC7, PC13, PC14

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 rows PC1, PC2, PC7, PC13, PC14; K§6.4.1 (the paths PC1 shows), K§6.4.4 (PC14), K§6.9 (P4-4 needs the captured Codex requests); K§11 items 9 and 10 (for these fixtures)
**Files:** `scripts/premises/codex/proxy.sh` (new), `fixtures/codex/requests/` (new), `fixtures/codex/transport/` (new), `fixtures/codex/models/` (new), `fixtures/codex/errors/` (new), `fixtures/codex/runs/proxy/` (new)
**Consumes:** `run_codex` (PC-2), `codex_env` (PC-2), `codex_record` (PC-2)
**Produces:** `fixtures/codex/requests/` (captured Codex `POST /v1/responses` bodies and headers, scrubbed: a first turn; a turn after a `function_call_output`; a turn after a `custom_tool_call_output` of `apply_patch`; a turn whose `input` carries a `reasoning` item), `fixtures/codex/models/` (the `/models` request and the shape Codex accepts, PC7), `fixtures/codex/errors/` (the context-too-long shape Codex recovers from, PC14), `fixtures/codex/transport/` (paths called, processes after exit, PC1), and in this node file under "Observed" one line per id: `- PCn: <verified|failed> <YYYY-MM-DD>: <fixture paths>`
**Acceptance:** `sh scripts/premises/run-codex.sh proxy && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(find fixtures/codex/requests -type f -name '*.json' -size +2c | wc -l)" -ge 4 && grep -qx 'home=scratch' fixtures/codex/runs/proxy/isolation.txt && grep -qx 'codex_home=scratch' fixtures/codex/runs/proxy/isolation.txt && test -f fixtures/codex/runs/proxy/connect.txt && test -z "$(grep -v ' refused$' fixtures/codex/runs/proxy/connect.txt)" && ! grep -q 'Written when the node runs' docs/work/phase4/nodes/PC-3.md && test "$(grep -cE '^- (PC1|PC2|PC7|PC13|PC14): (verified|failed) [0-9]{4}-[0-9]{2}-[0-9]{2}: fixtures/codex/' docs/work/phase4/nodes/PC-3.md)" -eq 5` → every command exits 0: at least 4 captured requests; the run used scratch `HOME` and `CODEX_HOME`, and every `CONNECT` was refused; "Observed" has no placeholder and one dated result line for each of PC1, PC2, PC7, PC13, PC14

## Planning notes

- Mock replies are authored (not model output), as the existing method says; they go under this node's own fixture folders. Every mock and scrub change this node needs is a PC-2 deliverable; this node edits neither file.
- PC13 (the user-role items Codex injects) is read from the same captured requests; the redaction span rule that uses it is P4-9 (K§6.4.2).
- PC-7 writes the `docs/premises.md` rows from "Observed"; this node does not edit `docs/premises.md`.

## Observed

Written when the node runs.

## Plan

Written when the node starts.

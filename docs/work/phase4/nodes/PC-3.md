# PC-3 — Codex proxy-facing captures: PC1, PC2, PC7, PC13, PC14

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 rows PC1, PC2, PC7, PC13, PC14; K§6.4.1 (the paths PC1 shows), K§6.4.4 (PC14), K§6.9 (P4-4 needs the captured Codex requests); K§11 items 9 and 10 (for these fixtures)
**Files:** `scripts/premises/codex/proxy.sh` (new), `fixtures/codex/requests/` (new), `fixtures/codex/transport/` (new), `fixtures/codex/models/` (new), `fixtures/codex/errors/` (new)
**Consumes:** `run_codex` (PC-2), `codex_env` (PC-2)
**Produces:** `fixtures/codex/requests/` (captured Codex `POST /v1/responses` bodies and headers, scrubbed: a first turn; a turn after a `function_call_output`; a turn after a `custom_tool_call_output` of `apply_patch`; a turn whose `input` carries a `reasoning` item), `fixtures/codex/models/` (the `/models` request and the shape Codex accepts, PC7), `fixtures/codex/errors/` (the context-too-long shape Codex recovers from, PC14), `fixtures/codex/transport/` (paths called, `CONNECT` records, processes after exit), and in this node file under "Observed": the date, result (`verified` or `failed`) and fixture paths of PC1, PC2, PC7, PC13, PC14
**Acceptance:** `sh scripts/premises/run-codex.sh proxy && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(find fixtures/codex/requests -type f -name '*.json' -size +2c | wc -l)" -ge 4` → every command exits 0 and at least 4 captured requests exist; each of PC1, PC2, PC7, PC13, PC14 has a dated result and fixture paths recorded under "Observed" in this file

## Planning notes

- Mock replies are authored (not model output), as the existing method says; they go under this node's own fixture folders.
- PC13 (the user-role items Codex injects) is read from the same captured requests; the redaction span rule that uses it is P4-9 (K§6.4.2).
- PC-7 writes the `docs/premises.md` rows from "Observed"; this node does not edit `docs/premises.md`.

## Observed

Written when the node runs.

## Plan

Written when the node starts.

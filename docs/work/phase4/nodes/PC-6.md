# PC-6 — Codex resume and update captures: PC10, PC16

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 rows PC10, PC16; K-D22 (the shared session store), K§6.5.4 (the fact PC16 measures; the fact itself is set in P4-9); K§11 items 9 and 10 (for these fixtures)
**Files:** `scripts/premises/codex/resume.sh` (new), `fixtures/codex/workspace/` (new), `fixtures/codex/update/` (new)
**Consumes:** `run_codex` (PC-2), `codex_env` (PC-2)
**Produces:** `fixtures/codex/workspace/` (PC10: `SessionStart` `session_id` against the rollout id; `codex resume <id>` with a new scratch `CODEX_HOME` whose `sessions` links to a shared scratch store), `fixtures/codex/update/` (PC16: `check_for_update_on_startup` at its default and the `CONNECT` record of the release-source request, refused by the mock), and in this node file under "Observed": the date, result and fixture paths of PC10 and PC16
**Acceptance:** `sh scripts/premises/run-codex.sh resume && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(find fixtures/codex/workspace fixtures/codex/update -type f -size +2c | wc -l)" -ge 2` → every command exits 0 and both folders hold a nontrivial capture; PC10 and PC16 have a dated result and fixture paths under "Observed" in this file

## Planning notes

- The release-source request of PC16 is refused at the mock's `CONNECT`; no request reaches the npm registry or another real host. The text of a real notice is LC-CX1, owner-run (PC-7 adds the row as `not run`).
- The shared store and its link are in the scratch tree; the link is a symbolic link (the Windows junction of K§6.3.1 is P4-9).
- PC-7 writes the rows from "Observed".

## Observed

Written when the node runs.

## Plan

Written when the node starts.

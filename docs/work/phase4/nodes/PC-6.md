# PC-6 — Codex resume and update captures: PC10, PC16

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 rows PC10, PC16; K-D22 (the shared session store), K-D25 (premises against the mock, with no zygo between), K§6.5.4 (the fact PC16 measures; the fact itself is set in P4-9); K§11 items 9 and 10 (for these fixtures)
**Files:** `scripts/premises/codex/resume.sh` (new), `fixtures/codex/workspace/` (new), `fixtures/codex/update/` (new), `fixtures/codex/runs/resume/` (new)
**Consumes:** `run_codex` (PC-2), `codex_env` (PC-2), `codex_record` (PC-2)
**Produces:** `fixtures/codex/workspace/` (PC10: `SessionStart` `session_id` against the rollout id; `codex resume <id>` with a new scratch `CODEX_HOME` whose `sessions` links to a shared scratch store), `fixtures/codex/update/` (PC16 as P4-3 can measure it: `check_for_update_on_startup` at its default and the `CONNECT` record of the release-source request, refused by the mock), and in this node file under "Observed" one line per id: `- PCn: <verified|failed> <YYYY-MM-DD>: <fixture paths>`
**Acceptance:** `sh scripts/premises/run-codex.sh resume && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(for d in workspace update; do find fixtures/codex/$d -type f -size +2c -print -quit; done | grep -c .)" -eq 2 && grep -qx 'home=scratch' fixtures/codex/runs/resume/isolation.txt && grep -qx 'codex_home=scratch' fixtures/codex/runs/resume/isolation.txt && test -f fixtures/codex/runs/resume/connect.txt && test -z "$(grep -v ' refused$' fixtures/codex/runs/resume/connect.txt)" && ! grep -q 'Written when the node runs' docs/work/phase4/nodes/PC-6.md && test "$(grep -cE '^- (PC10|PC16): (verified|failed) [0-9]{4}-[0-9]{2}-[0-9]{2}: fixtures/codex/' docs/work/phase4/nodes/PC-6.md)" -eq 2` → every command exits 0: each of the two folders holds a nontrivial capture; the run used scratch `HOME` and `CODEX_HOME`, and every `CONNECT` (the release-source request among them) was refused; "Observed" has no placeholder and one dated result line for each of PC10 and PC16

## Planning notes

- PC16 "under a zygo launch" (K§6.3.7) cannot run before the Codex client exists: there is no `zygo codex` in P4-3. Here PC16 measures the client's own default against the mock directly (K-D25). The measurement under a zygo launch is owned by P4-9 (the Codex launch keeps the key unset, K§6.3.1) and P4-11 (the `announces_own_updates` fact, K§11 item 52). The "Observed" line for PC16 states this scope.
- The release-source request of PC16 is refused at the mock's `CONNECT`; no request reaches the npm registry or another real host. The text of a real notice is LC-CX1, owner-run (PC-7 adds the row as `not run`).
- The shared store and its link are in the scratch tree; the link is a symbolic link (the Windows junction of K§6.3.1 is P4-9).
- The turn sequence before and after `codex resume` comes from the PC-2 mock deliverable 2; every mock and scrub change this node needs is a PC-2 deliverable. PC-7 writes the rows from "Observed".

## Observed

Written when the node runs.

## Plan

Written when the node starts.

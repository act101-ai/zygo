# PRB-2 — Premise docs: pins are minimums; live scratch dir removed on exit

**Spec:** Operator decision 2026-11-21 (graph Log): a pinned agent version is a minimum, not an exact version. Found in PRB-1 planning: `docs/premises.md:3-6` and `docs/live-checks.md:5-6` describe the Claude Code and OpenCode versions as exact pins; `scripts/premises/lib.sh` creates `N2_SCRATCH` with `mktemp -d` and nothing removes it, so each live run leaves a folder holding `results.txt`, and a reused `N2_SCRATCH` appends to an old `results.txt`.
**Files:** `docs/premises.md`, `docs/live-checks.md`, `scripts/premises/lib.sh`, `scripts/premises/lib_test.sh`
**Consumes:** —
**Produces:** —
**Acceptance:** `sh scripts/premises/lib_test.sh && sh scripts/check-premises.sh` → exit 0; `lib_test.sh` shows a scratch dir that lib.sh created is gone after the script exits (normal exit, error exit, and TERM), a caller-set `N2_SCRATCH` is kept, and a re-run starts with an empty `results.txt`; both docs state each version as a minimum ("2.1.328 or later", "1.18.16 or later")

## Plan

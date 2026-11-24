# SX-9 — Live check P3-15: the operator records the ST1-ST4 evidence by hand

**Spec:** docs/specs/2026-11-24-session-text-design.md §12.2, §12.1 (table ST1-ST4); T-C3, T-C6
**Files:** —
**Consumes:** `docs/live-checks.md#P3-15` (SX-8)
**Produces:** `fixtures/claude-code/live/st-hooks.jsonl`, `fixtures/claude-code/live/st-evidence.txt`, `fixtures/opencode/live/st-transform.jsonl` (written by the operator in step 6, not committed here), and the `claude --version` and `opencode --version` lines
**Acceptance:** `for f in fixtures/claude-code/live/st-hooks.jsonl fixtures/claude-code/live/st-evidence.txt fixtures/opencode/live/st-transform.jsonl; do test -s "$f" || exit 1; done` → exit 0; and the lead reads each of ST1, ST2, ST3 and ST4 as `verified` or `failed` by T§12.2 "Reading the result" (an ST1 or ST2 line with `no-transcript` is not decided, and the check is run again)

## Plan

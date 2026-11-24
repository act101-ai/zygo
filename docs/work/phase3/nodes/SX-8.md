# SX-8 — `docs/live-checks.md` section P3-15 (the operator's live check)

**Spec:** docs/specs/2026-11-24-session-text-design.md §12.2, §16 row `docs/live-checks.md`
**Files:** `docs/live-checks.md`
**Consumes:** —
**Produces:** `docs/live-checks.md#P3-15`
**Acceptance:** `awk '/^## P3-15/{f=1;next} /^## /{f=0} f' docs/live-checks.md | grep -cE 'CLAUDE_CODE_FORK_SUBAGENT=0|scrub.py|st-transform.jsonl|Reading the result'` → at least 4; and review → steps 1-8 and "Reading the result" of T§12.2 are copied without change in meaning (or the section points to T§12.2 for them), and the section names no operator user name

## Plan

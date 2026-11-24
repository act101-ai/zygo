# SX-8 — `docs/live-checks.md` section P3-15 (the operator's live check)

**Spec:** docs/specs/2026-11-24-session-text-design.md §12.2, §16 row `docs/live-checks.md`
**Files:** `docs/live-checks.md`
**Consumes:** —
**Produces:** `docs/live-checks.md#P3-15`
**Acceptance:** `N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh` → exit 0 (the script scans `docs/live-checks.md` for the account name read at run time; nothing personal is written in the repository). Review: steps 1-8 and "Reading the result" of T§12.2 are in the `## P3-15` section without change in meaning (or the section points to T§12.2 for them).

## Plan

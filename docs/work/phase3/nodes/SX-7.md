# SX-7 — Base-spec amendments for session text (§8.5, §8.6 item 7, §8.6 last paragraph, §8.7 N33, §10.6)

**Spec:** docs/specs/2026-11-24-session-text-design.md §13 (all bullets but the last), §8.5
**Files:** `docs/specs/2026-11-18-zygo-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `cargo test -p zygo-config --test builtin && sh scripts/check-premises.sh` → all pass, exit 0 (the base-spec sections that tests and the premise script read stay intact). Review of `git diff -- docs/specs/2026-11-18-zygo-design.md` against T§13: each of the five bullets (§8.5 hooks list, §8.6 item 7 parenthesis, §8.6 new last paragraph, §8.7 N33, §10.6 flag sentence) is present with the meaning T§13 gives, and no other base-spec text changes.

## Plan

# PC-9 — scripts/premises/ver_test.sh: its repo copy includes every evidence path the premise rows cite

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.5 (the premise check by data)
**Files:** `scripts/premises/ver_test.sh`
**Consumes:** `scripts/check-premises.sh` (PC-1)
**Produces:** `scripts/premises/ver_test.sh` whose K cases copy into their temporary tree every path that docs/premises.md rows cite as evidence (derived from the rows, not a fixed list), so K1 and K4 pass
**Acceptance:** `sh scripts/premises/ver_test.sh` → exit 0, last line `ver tests: ok (<n>)`

## Planning notes

- Since OC-1 (merged 53eabe25), K1 and K4 fail on phase-4 with `check-premises: PO1: evidence path scripts/premises/run-opencode-oc1.sh is missing`, because the test's copy omits that file (found by the PC-2 merge agent, 2026-11-25).
- CI does not run ver_test.sh.

## Plan


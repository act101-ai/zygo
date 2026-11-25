# PC-1 — `scripts/check-premises.sh` driven by data, with `scripts/test-check-premises.sh`

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.5 (last two paragraphs: the premise check by data), K§6.3.7 (last paragraph: the `Codex` header passes before the Codex client exists), K§9 (P4-3), K§11 item 8; K-D10
**Files:** `scripts/check-premises.sh`, `scripts/test-check-premises.sh` (new)
**Consumes:** —
**Produces:** `scripts/check-premises.sh` (names and versions from the `<Name>: <version>` header lines of `docs/premises.md` before the first `##`; every table row checked; the existing id lists still required), `scripts/test-check-premises.sh` (cases on temporary copies; last line `test-check-premises: ok (<n> cases)`)
**Acceptance:** `sh scripts/test-check-premises.sh && sh scripts/check-premises.sh` → both exit 0; the last line of the first is `test-check-premises: ok (<n> cases)` with n ≥ 8: a copy with a `Codex: 0.172.0` header and a well-formed `PC1` row passes; a copy with `Q7` removed fails; a copy with a `PW` id removed fails; a row whose agent has no header line fails; a Codex row below its header version fails (minimum rule); an `act-lite` row that differs from its header fails (exact rule); a `tmux` row `3.2a` passes (one letter); a row whose evidence path is trivial fails

## Planning notes

- Precedent for the test script: `scripts/test-check-conformance.sh` (copies of the documents into a temporary tree, one `fail` per case). That script is deleted by CF-4; this node copies its form, not the file.
- The header reads and `version_of` (`scripts/check-premises.sh:32-49`) and the name `case` of the version rule (`:81-85`) go, and the loop over the fixed lists `IDS`, `WIDS` (`:12-15`, loop at `:58`) becomes a loop over every table row while each listed id is still required; the rules per name stay: minimum for every agent name, exact for `act-lite`, minimum with one letter for `tmux` (K§6.2.5).
- A failed row's spec-note rule (`:102-121`) keys on the id prefix (`P*`, `Q*`, `PW*`). A `PC` id matches `P*` today; the data-driven check does not apply the base-spec §19 note rule to `PC` rows (author: K§6.3.7 names no note rule for PC rows, and the format adds none).
- No CI job runs these scripts (K§1 gap item 4); that does not change.

## Plan

Written when the node starts.

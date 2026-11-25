# PC-1 — `scripts/check-premises.sh` driven by data, with `scripts/test-check-premises.sh`

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.5 (last two paragraphs: the premise check by data), K§6.3.7 (last paragraph: the `Codex` header passes before the Codex client exists), K§9 (P4-3), K§11 item 8; K-D10
**Files:** `scripts/check-premises.sh`, `scripts/test-check-premises.sh` (new), `docs/specs/2026-11-20-workspaces-design.md` (read only: the W§17 rows of the PW rule)
**Consumes:** —
**Produces:** `scripts/check-premises.sh` (names and versions from the `<Name>: <version>` header lines of `docs/premises.md` before the first `##`; every table row checked; the existing id lists still required; the W§17 rule on the listed PW ids only; `fixtures/codex/config.schema.json` in the verbatim-schema exemption), `scripts/test-check-premises.sh` (cases on temporary copies; last line `test-check-premises: ok (<n> cases)`)
**Acceptance:** `sh scripts/test-check-premises.sh && sh scripts/check-premises.sh && test "$(sh scripts/test-check-premises.sh | grep -cE '^test-check-premises: ok \(([89]|[1-9][0-9]+) cases\)$')" -eq 1` → every command exits 0 on the real tree at the node's base, and the test script reports at least 8 cases: a copy with a `Codex: 0.172.0` header and a well-formed `PC1` row passes; a copy with `Q7` removed fails; a copy with `PW15` removed fails; a row whose agent has no header line fails; a Codex row below its header version fails (minimum rule); an `act-lite` row that differs from its header fails (exact rule); a `tmux` row `3.2a` passes (one letter); a row whose evidence path is trivial fails; the real `PW15-bash`, `PW15-tool` and `PW15-slash` rows pass the row checks with no W§17 row of their own

## Planning notes

- Precedent for the test script: `scripts/test-check-conformance.sh` (copies of the documents into a temporary tree, one `fail` per case). CF-4 deletes that script; this node copies its form, not the file.
- The header reads and `version_of` (`scripts/check-premises.sh:32-49`) and the name `case` of the version rule (`:81-85`) go, and the loop over the fixed lists `IDS`, `WIDS` (`:12-15`, loop at `:58`) becomes a loop over every table row while each listed id is still required. The rules per name stay: minimum for every agent name, exact for `act-lite`, minimum with one letter for `tmux` (K§6.2.5).
- PW rule (author, the simplest rule that keeps today's checks): the W§17 result and note rule (`:115-121`) applies to the ids of `WIDS` only, as today. `PW15-bash`, `PW15-tool` and `PW15-slash` (`docs/premises.md:42-44`) are not in `WIDS` and have no W§17 row of their own (W§17 has one `PW15` row, workspaces spec `:715`); they get the row checks (date, versions, result, evidence paths) and no W§17 lookup. Today they get no check at all.
- The failed-row note rule for the base spec (`:102-112`) keys on `P*` and `Q*`. A `PC` id matches `P*`; the data-driven check applies the §19 note rule to the ids of `IDS` only, so a `PC` row needs no base-spec note (K§6.3.7 names no note rule for PC rows).
- Scan exemption: `VERBATIM` (`:139`) holds one file today. It becomes a list with `fixtures/codex/config.schema.json` added, for the path and bearer rules only, as for the Claude Code schema. PC-2 depends on this node for it.
- No CI job runs these scripts (K§1 gap item 4); that does not change.

## Plan

Written when the node starts.

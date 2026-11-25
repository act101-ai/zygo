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

Base: `origin/phase-4` at 9595de88. Premise checked 2026-11-25 against the live code. The line numbers in the node notes hold: in `scripts/check-premises.sh`, the header reads are at `:31-49`, the version `case` at `:81-85`, the lists at `:13-15`, the loop at `:58`, the §19 note rule at `:102-114`, the W§17 rule at `:115-123`, `VERBATIM` at `:139` and the ok line at `:213`. K§6.2.5 cites `:32-44` and `:82`; that is a drift of one line and changes nothing. `docs/premises.md` has its header lines at `:5-8` (`Claude Code: 2.1.328`, `OpenCode: 1.18.16`, `act-lite: 1.2.3`, `tmux: 3.2`), its first `##` at `:10`, and 29 table rows at `:25-53`. Among them are `PW15-bash`, `PW15-tool` and `PW15-slash` (`:42-44`; `PW15-bash` is `failed`, and today no check reads these three rows). `fixtures/codex/` does not exist yet; the tests make it in their copies.

Safety: run only `scripts/check-premises.sh`, `scripts/test-check-premises.sh` and `scripts/premises/ver_test.sh` (it uses stub programs only). Do not run any `scripts/premises/run-*.sh`, `live*.sh` or `run-all.sh`, because they start agents. Never run an older version of a script (no `git archive`, no checkout of old commits).

### Interfaces

- Consumes: `ver_ge <have> <min>` from `scripts/premises/ver.sh` (it exists; it is not a node). Exit 0 when both are dotted numbers and `have >= min`, else 1.
- Produces:
  - `scripts/check-premises.sh` (no arguments): exit 0 with the last line `check-premises: ok (<row ids in table order, space-separated>)`, or exit 1 with one line `check-premises: <reason>`.
  - `scripts/test-check-premises.sh` (no arguments): exit 0 with the last line `test-check-premises: ok (<n> cases)`, or exit 1 with `test-check-premises: FAIL <case>: <why>` on stderr.
- Texts to keep (`scripts/premises/ver_test.sh:204-208` greps them):
  - `check-premises: ok`
  - `<id>: version '<v>' is below the header's '<want>' for <name>, or not a dotted number`
  - `<id>: version '<v>' differs from the header's '<want>' for act-lite`

### Steps

- [ ] 1. Write `scripts/test-check-premises.sh` (new, `#!/bin/sh`, `set -u`) in the form of `scripts/test-check-conformance.sh:1-47`: `HERE`, `ROOT`, `T=$(mktemp -d)`, `trap 'rm -rf "$T"' EXIT INT TERM`, and the globals `CASE`, `OUT`, `RC` and `N=0`. The first comment line is `# Tests for scripts/check-premises.sh on temporary copies (node PC-1). No agent, no network.` Helpers:
  - `fail <why>`: prints `test-check-premises: FAIL $CASE: <why>` and then `$OUT` to stderr, and exits 1.
  - `mk <name>`: makes `$T/<name>` and copies into it, at the same relative paths:
    - `scripts/check-premises.sh`;
    - `scripts/premises/` and `fixtures/` (each with `cp -RP`);
    - `docs/premises.md`, `docs/live-checks.md` and `docs/conformance.md` (each when present);
    - `docs/specs/2026-11-18-zygo-design.md` and `docs/specs/2026-11-20-workspaces-design.md`.

    It also runs `mkdir -p $T/<name>/fixtures/codex`, then prints the path.
  - `add_header <dir> <line>`: inserts `<line>` after the line that starts `tmux: ` in `<dir>/docs/premises.md`.
  - `add_row <dir> <row>`: inserts `<row>` after the line that starts `| PW16 |` in `<dir>/docs/premises.md`.
  - `run <dir>`: `OUT=$(sh "<dir>/scripts/check-premises.sh" 2>&1)`, `RC=$?`, `N=$((N + 1))`.
  - `expect_ok`: `RC` is 0, and the last line of `OUT` starts with `check-premises: ok (`.
  - `expect_fail <text>`: `RC` is 1, and `OUT` equals `check-premises: <text>`.
  - The last line of the script is `echo "test-check-premises: ok ($N cases)"`.

  The cases write two evidence files: `$D/fixtures/codex/pc1.txt` holds `pc1 evidence` (nontrivial), and `$D/fixtures/codex/empty.json` holds `{}` (trivial). In the table below, ROW(`<id>`, `<agent>`, `<version>`, `<result>`, `<path>`) means this line:

  ``| <id> | Test row | <agent> | <version> | 2026-11-25 | <result> | `<path>` |``

  The cases, in this order, each on its own copy:

  | # | Case | Change to the copy | Expect |
  |---|---|---|---|
  | 1 | real tree | none | `expect_ok`; `OUT` holds ` PW15-bash PW15-tool PW15-slash ` (the row checks ran on them with no W§17 row) |
  | 2 | Codex header and PC1 row | `add_header 'Codex: 0.172.0'`; `add_row` ROW(`PC1`, `Codex`, `0.172.0`, `verified`, `fixtures/codex/pc1.txt`) | `expect_ok`; `OUT` holds ` PC1)` |
  | 3 | a failed PC row needs no note | as 2, with the result `failed` | `expect_ok` |
  | 4 | Q7 removed | `sed '/^| Q7 |/d'` | `expect_fail "Q7: expected exactly one row, found 0"` |
  | 5 | PW15 removed | `sed '/^| PW15 |/d'` (it keeps `PW15-bash` and the other two) | `expect_fail "PW15: expected exactly one row, found 0"` |
  | 6 | an agent with no header line | `add_row` ROW(`PC1`, `Cursor`, `1.0.0`, `verified`, `fixtures/codex/pc1.txt`) | `expect_fail "PC1: agent 'Cursor' has no header line 'Cursor: <version>'"` |
  | 7 | Codex below its header (minimum rule) | `add_header 'Codex: 0.172.0'`; `add_row` ROW(`PC1`, `Codex`, `0.171.9`, `verified`, `fixtures/codex/pc1.txt`) | `expect_fail "PC1: version '0.171.9' is below the header's '0.172.0' for Codex, or not a dotted number"` |
  | 8 | Codex above its header | as 7, with the version `0.173.0` | `expect_ok` |
  | 9 | act-lite differs (exact rule) | change the `P6b` row version from `1.2.3` to `1.2.4` (sed script below) | `expect_fail "P6b: version '1.2.4' differs from the header's '1.2.3' for act-lite"` |
  | 10 | tmux with one letter | `add_row` ROW(`PC2`, `tmux`, `3.2a`, `verified`, `fixtures/codex/pc1.txt`) | `expect_ok` |
  | 11 | trivial evidence | `add_header 'Codex: 0.172.0'`; `add_row` ROW(`PC1`, `Codex`, `0.172.0`, `verified`, `fixtures/codex/empty.json`) | `expect_fail "PC1: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial"` |
  | 12 | PW15-bash gets the row checks | change the date cell of the `PW15-bash` row from `2026-11-23` to `2026-11-3` (sed script below) | `expect_fail "PW15-bash: date '2026-11-3' is not YYYY-MM-DD"` |
  | 13 | the Codex schema is verbatim | `printf '%s\n' '"/Users/alice/x"' > fixtures/codex/config.schema.json` | `expect_ok` |
  | 14 | the same text in another file | the same line in `fixtures/codex/other.json` | `RC` is 1, and `OUT` starts with `check-premises: forbidden content (macOS home path): fixtures/codex/other.json:` |

  The sed scripts, exactly (BRE; `|` is a plain character in a BRE):
  - case 9: `s/^\(| P6b | [^|]* | act-lite | \)1\.2\.3 |/\11.2.4 |/` (the same form as `scripts/premises/ver_test.sh:195`);
  - case 12: `s/^\(| PW15-bash | [^|]* | [^|]* | [^|]* | \)2026-11-23 |/\12026-11-3 |/`.

  Each `sed` writes to a temporary file and then moves it back, because BSD `sed -i` and GNU `sed -i` differ. After each change, use `grep -c` to check that the change took effect: the count of the old line is 0, or the count of the new line is 1. If the change did not take effect, call `fail "planted change did not apply"`.

- [ ] 2. Run `sh scripts/test-check-premises.sh` → exit 1 with `test-check-premises: FAIL real tree: …`, because today's ok line lists only the fixed ids. For reference, on today's script cases 1, 2, 3, 6, 7, 8, 11, 12 and 13 fail, and cases 4, 5, 9, 10 and 14 pass.

- [ ] 3. Change `scripts/check-premises.sh`:
  - Delete `header()`, `V_CC` … `V_TM`, their four `fail`s and `version_of()` (`:31-49`).
  - Add `HDR`: each `<Name>: <version>` line before the first `^## ` line of `$P`, stored as a `name<TAB>version` line. A header line matches the ERE `^[A-Za-z][A-Za-z0-9 ._-]*[A-Za-z0-9]: *[^ ]+ *$`. The prose line at `:3` does not match, because its name part holds a comma. Drop trailing spaces. When a name is given twice, the first one wins (as `head -n 1` does today).
  - Add `version_of <name>`: prints the version for that name from `HDR`, or nothing.
  - Add `ROWS`: for each line of `$P` that starts with `| `, except the line whose first cell is `Id`, the first cell with its spaces removed, in file order.
  - Before the row loop, add `for id in $IDS $WIDS` with the count check that is at `:59-60` today. The message is `"$id: expected exactly one row, found $n"`.
  - The row loop (`:58`) runs `for id in $ROWS` and keeps its own count check, which catches an unlisted id that appears twice.
  - Unknown agent (`:79`): change the message to `"$id: agent '$a' has no header line '$a: <version>'"`.
  - Version rule (`:81-85`):
    - `act-lite`: exact, with the same message as today.
    - `tmux`: no change.
    - Every other name (`*`): `ver_ge "$v" "$want"`, with the message `"$id: version '$v' is below the header's '$want' for $a, or not a dotted number"`.

    Change the comment at `:80` to say that the rule is chosen per header name (K§6.2.5).
  - §19 note rule (`:102-114`): run it only when `$id` is a word of `$IDS` (`case " $IDS " in *" $id "*)`). The inner `P*`, `Q*` and `P6a` logic stays. Remove the `PW*) ;;` arm.
  - W§17 rule (`:115-123`): run it only when `$id` is a word of `$WIDS`, in place of `case "$id" in PW*)`.
  - `VERBATIM` (`:139`): make it the space-separated list `fixtures/claude-code/settings-schema.json fixtures/codex/config.schema.json`. Add `VX`, the ERE `^(<path>|<path>):` built from that list, and use `"$VX"` in place of `"^$VERBATIM:"` at `:160`, `:162` and `:173`. In the comment at `:136-138`, name the Codex schema too (from the pinned `rust-v0.172.0` tag, K§6.3.7; exempt from the path and bearer rules only).
  - Ok line (`:213`): `check-premises: ok (<ROWS joined by one space>)`.
  - Header comment, line 2: add `; rows and agent versions are driven by data (K§6.2.5, node PC-1)`.

- [ ] 4. Run `sh scripts/test-check-premises.sh` → exit 0, last line `test-check-premises: ok (14 cases)`.

- [ ] 5. Run `sh scripts/check-premises.sh` on the real tree → exit 0, last line `check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7 PW6 PW11 PW15 PW15-bash PW15-tool PW15-slash PW1 PW2 PW3 PW4 PW5 PW7 PW9 PW10 PW16)`.

- [ ] 6. Regression check: `sh scripts/premises/ver_test.sh` → exit 0, last line `ver tests: ok (<n>)`. Its K cases grep the texts kept above, and it uses stub programs only.

- [ ] 7. Acceptance: `sh scripts/test-check-premises.sh && sh scripts/check-premises.sh && test "$(sh scripts/test-check-premises.sh | grep -cE '^test-check-premises: ok \(([89]|[1-9][0-9]+) cases\)$')" -eq 1` → exit 0.

- [ ] 8. Commit `scripts/check-premises.sh` and `scripts/test-check-premises.sh` with the message `feat(premises): check-premises driven by data (PC-1)`. End the message with `Zygo-Session: <session>` and `Zygo-Work: phase4:PC-1`, each on its own line. Add no Claude attribution.

### Author's checks (2026-11-25)

1. Spec coverage.
   - K§6.2.5, last paragraph: header names and versions → step 3 (`HDR`); the rule for each name → step 3 and cases 7-10; every row → `ROWS` and cases 1, 11 and 12; the listed ids are still required → cases 4 and 5.
   - K§6.3.7, last paragraph: the `Codex` header and the PC rows pass with no Codex client → cases 2 and 3.
   - K§11 item 8 → cases 2 and 4.
   - K-D10 (the script stays in shell and reads no client enum) → step 3.
   - The PW rule and the `VERBATIM` list come from the node notes → cases 1, 12, 13 and 14.
2. Step scan: each step is one action. Step 3 gives the data form, the ERE and the messages; it does not give the loops.
3. Interface match: PC-2 uses only the `VERBATIM` entry for `fixtures/codex/config.schema.json` (case 13). The `ver_ge` signature matches `scripts/premises/ver.sh`. The kept texts match `scripts/premises/ver_test.sh:204-208`.
4. Review focus 5: `Q7` removed (case 4), a `PW` id removed (case 5), an agent with no header line (case 6).
5. Proportion: the plan gives signatures, texts and cases. It leaves the bodies to the implementer.
6. Live code: every line cited above was read at 9595de88. No helper other than `header()` parses the header block, and this node replaces `header()`.

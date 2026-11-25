# PC-8 — check-premises.sh: the name scan exempts the verbatim schemas; an indented table row is not a premise row

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.5 (the premise check by data), K§11 item 10 (the fixture scan)
**Files:** `scripts/check-premises.sh`, `scripts/test-check-premises.sh`
**Consumes:** `scripts/check-premises.sh` (PC-1), `scripts/test-check-premises.sh` (PC-1)
**Produces:** `scripts/check-premises.sh` with (a) the forbidden-name scan (N2_FORBIDDEN_NAMES) skipping the same verbatim upstream schema files the script already exempts elsewhere (today fixtures/claude-code/settings-schema.json contains the upstream example path /Users/alice/…, so N2_FORBIDDEN_NAMES=alice sh scripts/check-premises.sh exits 1 on line 22 of that file); (b) a line indented four or more spaces (or a tab) not read as a Results table row or header line, as Markdown renders it as a code block. `scripts/test-check-premises.sh` gains one case for each, failing first.
**Acceptance:** `sh scripts/test-check-premises.sh && N2_FORBIDDEN_NAMES=alice sh scripts/check-premises.sh && sh scripts/check-premises.sh` → every command exits 0.

## Planning notes

- Found by the PC-1 round 3 implementer (2026-11-25); the behavior predates round 3.
- Premise check (a), 2026-11-25, on `origin/phase-4` ba67eedb:
  - `N2_FORBIDDEN_NAMES=alice sh scripts/check-premises.sh` → `check-premises: forbidden name from N2_FORBIDDEN_NAMES: fixtures/claude-code/settings-schema.json:22`, exit 1. `sh scripts/check-premises.sh` → `check-premises: ok (P1 … PW16)`, exit 0.
  - `grep -rnil alice fixtures docs/premises.md` → only `fixtures/claude-code/settings-schema.json` (line 22 `"Read(//Users/alice/secrets/**)"`, line 1970 `"//Users/alice/Documents"`).
  - The verbatim files are already one list: `VERBATIM` (`scripts/check-premises.sh:210`, `fixtures/claude-code/settings-schema.json fixtures/codex/config.schema.json`) and its ERE `VX` (`:212`, `^(<path>|<path>):`). `scan` (`:221-226`) drops hits that match its exempt ERE; the path and bearer rules pass `"$VX"` (`:233`, `:235`, `:246`). `name_scan` (`:254-261`) has no exempt ERE. The fix reuses `VX`; no second list.
  - Scope decision: the exemption applies to the N2_FORBIDDEN_NAMES call (`:262`) only, as the contract names. The host-name call (`:279`) keeps scanning the verbatim files.
- Premise check (b), 2026-11-25, on a copy of the tree with `fixtures/codex/empty.json` = `{}`:
  - The row of case 53 below (four blanks, then `| PC9 |`, evidence `fixtures/codex/empty.json`), inserted after the `| PW16 |` row → `check-premises: PC9: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial`, exit 1. The same line led by one tab → the same message, exit 1. So both are read as Results rows.
  - Cause: `TROWS` (`scripts/check-premises.sh:68-73`) takes a table line as `^[ \t]*\|` (`:71`, `:72`) and the separator as `^[ \t]*\|[-:| \t]*$` (`:70`).
  - Header lines already follow the rule: `HDR` (`:44`) takes only `^[A-Za-z]…:` at column 1, and the Results header (`:69`, `:75`) is `^\| Id \| Item \|` at column 1. No change there; case 56 pins it.
  - Live tree: `grep -cE '^(    |\t)' docs/premises.md` → 0; `grep -nE '^ {1,3}\|' docs/premises.md` → none. No row that must count is indented. `| PW16 |` (line 53) is the last Results row.
- Rule (b), exact: a table line starts with zero to three blanks (spaces) and then `|` (ERE `^ ? ? ?\|`, as `HDR` already uses at `:38`). A line that starts with four or more spaces, or with a tab, is not a table line (CommonMark indented code block): it is not a Results row, not another table's row, not counted by `rows_of`, and, inside the Results table, it ends that table (as any other non-table line does).

## Plan

Interfaces (as in the code now, `scripts/check-premises.sh` at ba67eedb):

- Consumes: `VERBATIM` (`:210`) and `VX` (`:212`), unchanged. `name_scan <label> <name> [<word characters>]` (`:254`). `TROWS` (`:68-73`), output "line<TAB>id<TAB>r|o".
- Produces: `name_scan <label> <name> [<word characters>] [<exempt ERE>]`: a hit line ("path:line:text") that matches the exempt ERE is dropped before the first hit is taken, as in `scan` (`:224`, `grep -v -E -- "${2:-^\$}"`). Messages unchanged. `TROWS` with the rule (b) above; its output form unchanged.

Test helper and cases, all in `scripts/test-check-premises.sh`, appended after case 50 ("a joint agent with no header for the second") and before the final `echo`. Numbering goes on from PC-1 (50 cases).

- Helper, after `run` (`:80-84`): `run_names <dir> <names>`: like `run`, but sets `N2_FORBIDDEN_NAMES` to `<names>` for the script: `OUT=$(N2_FORBIDDEN_NAMES="$2" sh "$1/scripts/check-premises.sh" 2>&1)`, `RC=$?`, `N=$((N + 1))`.

Cases (each starts `D=$(mk <dir>)`; the dir name is free):

- 51 `CASE="a forbidden name only in the verbatim schema"`. No plant: the copy's `fixtures/claude-code/settings-schema.json` holds `alice`. `run_names "$D" alice`; `expect_ok`. Before the fix: exit 1, `check-premises: forbidden name from N2_FORBIDDEN_NAMES: fixtures/claude-code/settings-schema.json:22`.
- 52 `CASE="a forbidden name in another fixture"` (guard: the exemption is for the listed files only).
  ```sh
  printf '%s\n' 'owner alice' > "$D/fixtures/codex/other.txt"
  run_names "$D" alice
  expect_fail "forbidden name from N2_FORBIDDEN_NAMES: fixtures/codex/other.txt:1"
  ```
- 53 `CASE="a row indented four blanks"`. Before the fix: exit 1, `check-premises: PC9: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial`.
  ```sh
  evidence "$D"
  add_row "$D" '    | PC9 | Test row | tmux | 3.2 | 2026-11-25 | verified | `fixtures/codex/empty.json` |'
  run "$D"
  expect_ok
  case "$OUT" in *' PC9'*) fail "ok line names PC9" ;; esac
  ```
- 54 `CASE="a row indented with a tab"`. As 53, with the row `"$(printf '\t| PC9 | Test row | tmux | 3.2 | 2026-11-25 | verified | `fixtures/codex/empty.json` |')"`. Before the fix: as 53.
- 55 `CASE="a row indented three blanks"` (guard: the boundary stays a row). As 53 with three blanks before `|`, then `expect_fail "PC9: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial"` in place of the last two lines.
- 56 `CASE="a header line indented four blanks"` (guard: header lines already follow the rule).
  ```sh
  evidence "$D"
  add_header "$D" '    Cursor: 1.0'
  add_row "$D" "$(row PC1 Cursor 1.0 verified fixtures/codex/pc1.txt)"
  run "$D"
  expect_fail "PC1: agent 'Cursor' $NOHDR"
  ```

Steps:

- [x] 1. Add `run_names` and cases 51-56 to `scripts/test-check-premises.sh` as in Cases above. Commit `test(premises): PC-8 failing cases for the verbatim name scan and indented rows`.
- [ ] 2. Run `sh scripts/test-check-premises.sh` → exit 1, first line `test-check-premises: FAIL a forbidden name only in the verbatim schema: exit 1, expected 0`. The harness stops at the first failure; the before-fix results of 53 and 54 are the ones recorded in Planning notes (b). Record the output under `## Results`.
- [ ] 3. In `scripts/check-premises.sh`, `name_scan` (`:251-261`): add the optional 4th argument `<exempt ERE>` and drop the matching hit lines before `head -n 1`, by the same form as `scan` (`grep -v -E -- "${4:-^\$}"`). Change the N2 call (`:262`) to `name_scan "forbidden name from N2_FORBIDDEN_NAMES" "$w" A-Za-z0-9_ "$VX"`. The host-name call (`:279`) is unchanged. Update the comments: `:251` (the new argument), and `:207-209` so the verbatim files are said to be exempt from the path and bearer rules and from the N2_FORBIDDEN_NAMES scan.
- [ ] 4. In `scripts/check-premises.sh`, `TROWS` (`:68-73`): replace `^[ \t]*\|` with `^ ? ? ?\|` at `:70` (separator: `/^ ? ? ?\|[-:| \t]*$/`), `:71` and `:72`. Update the comment `:64-67`: a table line starts with zero to three blanks and `|`; a line with four or more leading blanks or a leading tab is not a table line (Markdown renders it as code) and ends the Results table.
- [ ] 5. Run `sh scripts/test-check-premises.sh` → exit 0, last line `test-check-premises: ok (56 cases)`.
- [ ] 6. Regression: `sh scripts/premises/ver_test.sh` → exit 0, last line `ver tests: ok (<n>)`.
- [ ] 7. Acceptance, alone as one command: `sh scripts/test-check-premises.sh && N2_FORBIDDEN_NAMES=alice sh scripts/check-premises.sh && sh scripts/check-premises.sh` → exit 0; the second and third commands each end with `check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7 PW6 PW11 PW15 PW15-bash PW15-tool PW15-slash PW1 PW2 PW3 PW4 PW5 PW7 PW9 PW10 PW16)`.
- [ ] 8. Commit `fix(premises): PC-8 verbatim schemas exempt from the name scan; indented lines are not table rows` with `scripts/check-premises.sh`, `scripts/test-check-premises.sh` and this file (steps ticked, outputs under `## Results`). The message ends with these two lines, each on its own line:
  ```
  Zygo-Session: 01M895RK6NZRE14W4WWFAECF0B
  Zygo-Work: phase4:PC-8
  ```
  No Claude attribution. Push `phase-4--PC-8`.

Author's checks (§3.3), 2026-11-25:

1. Spec coverage: K§11 item 10 (no fixture holds a user name) is kept for every file but the upstream verbatim schemas → step 3, cases 51-52. K§6.2.5 (rows read from the table by data) with rule (b) → step 4, cases 53-56. Acceptance → step 7.
2. Step scan: each step makes one change or one run; step 3 and 4 give the exact call, regex and lines.
3. Interface match: `VERBATIM`/`VX` are reused, not copied; PC-2 still uses the `fixtures/codex/config.schema.json` entry. `TROWS` output form unchanged, so `ROWS`, `IDLIST` and the outside-row check (`:113`) read it as before.
4. Review focus: graph review focus 5 (PC-1) is not changed; cases 4-6 still run.
5. Proportion: the plan gives the call, the three regexes and the cases; it gives no function body.
6. Live code: every line cited above was read at ba67eedb.

## Results


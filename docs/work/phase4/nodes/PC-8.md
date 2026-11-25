# PC-8 — check-premises.sh: the name scan exempts the verbatim schemas; an indented table row is not a premise row

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.5 (the premise check by data), K§11 item 10 (the fixture scan)
**Files:** `scripts/check-premises.sh`, `scripts/test-check-premises.sh`, `scripts/premises/live-hud.sh`
**Consumes:** `scripts/check-premises.sh` (PC-1), `scripts/test-check-premises.sh` (PC-1)
**Produces:** `scripts/check-premises.sh` with (a) the forbidden-name scan (N2_FORBIDDEN_NAMES) skipping the same verbatim upstream schema files the script already exempts elsewhere (today fixtures/claude-code/settings-schema.json contains the upstream example path /Users/alice/…, so N2_FORBIDDEN_NAMES=alice sh scripts/check-premises.sh exits 1 on line 22 of that file); (b) a line indented four or more spaces (or a tab) not read as a Results table row or header line, as Markdown renders it as a code block; (c) after review round 1: a verbatim file keeps its exemptions only while its sha256 equals the `sha256:` line of its source file, the exemption matches the exact path only, and a symlink in the scan scope fails. `scripts/test-check-premises.sh` gains one case for each, failing first.
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
- Scope addition (c), found by the PC-2 implementer 2026-11-25, added by the lead during round 1: `scripts/premises/live-hud.sh:303` (`--selfcheck`) used a bare `mktemp -d`, which on macOS ignores `TMPDIR` (it prefers `_CS_DARWIN_USER_TEMP_DIR`). The fix is the template form that `scripts/premises/live-detach.sh:43` uses. live-hud.sh is an owner-run live check: it is not run here. Other bare `mktemp -d` uses stay out of scope: `scripts/premises/lib.sh:41` and the test harnesses `lib_test.sh:13`, `live-hud-probe_test.sh:11`, `selftest_test.sh:8`, `workspace_test.sh:7`, `ver_test.sh:12`; so the step 6a grep covers the live scripts `scripts/premises/live*.sh` without the `_test.sh` files.

## Plan

Interfaces (as in the code now, `scripts/check-premises.sh` at ba67eedb):

- Consumes: `VERBATIM` (`:210`) and `VX` (`:212`), unchanged. `name_scan <label> <name> [<word characters>]` (`:254`). `TROWS` (`:68-73`), output "line<TAB>id<TAB>r|o".
- Produces: `name_scan <label> <name> [<word characters>] [<exempt ERE>]`: a hit line ("path:line:text") that matches the exempt ERE is dropped before the first hit is taken, as in `scan` (`:224`, `grep -v -E -- "${2:-^\$}"`). Messages unchanged. `TROWS` with the rule (b) above; its output form unchanged.

Test helper and cases, all in `scripts/test-check-premises.sh`, appended after case 50 ("a joint agent with no header for the second") and before the final `echo`. Numbering goes on from PC-1 (50 cases).

- Helper, after `run` (`:80-84`): `run_names <dir> <names>`: like `run`, but sets `N2_FORBIDDEN_NAMES` to `<names>` for the script: `OUT=$(N2_FORBIDDEN_NAMES="$2" sh "$1/scripts/check-premises.sh" 2>&1)`, `RC=$?`, `N=$((N + 1))`.

Cases (each starts `D=$(mk <dir>)`; the dir name is free):

- 51 `CASE="a forbidden name only in the verbatim schema"`. No plant: the copy's `fixtures/claude-code/settings-schema.json` holds `alice`; the case asserts that first (`grep -qiw alice`, round 1 F3). `run_names "$D" alice`; `expect_ok`. Before the fix: exit 1, `check-premises: forbidden name from N2_FORBIDDEN_NAMES: fixtures/claude-code/settings-schema.json:22`.
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
- [x] 2. Run `sh scripts/test-check-premises.sh` → exit 1, first line `test-check-premises: FAIL a forbidden name only in the verbatim schema: exit 1, expected 0`. The harness stops at the first failure; the before-fix results of 53 and 54 are the ones recorded in Planning notes (b). Record the output under `## Results`.
- [x] 3. In `scripts/check-premises.sh`, `name_scan` (`:251-261`): add the optional 4th argument `<exempt ERE>` and drop the matching hit lines before `head -n 1`, by the same form as `scan` (`grep -v -E -- "${4:-^\$}"`). Change the N2 call (`:262`) to `name_scan "forbidden name from N2_FORBIDDEN_NAMES" "$w" A-Za-z0-9_ "$VX"`. The host-name call (`:279`) is unchanged. Update the comments: `:251` (the new argument), and `:207-209` so the verbatim files are said to be exempt from the path and bearer rules and from the N2_FORBIDDEN_NAMES scan.
- [x] 4. In `scripts/check-premises.sh`, `TROWS` (`:68-73`): replace `^[ \t]*\|` with `^ ? ? ?\|` at `:70` (separator: `/^ ? ? ?\|[-:| \t]*$/`), `:71` and `:72`. Update the comment `:64-67`: a table line starts with zero to three blanks and `|`; a line with four or more leading blanks or a leading tab is not a table line (Markdown renders it as code) and ends the Results table.
- [x] 5. Run `sh scripts/test-check-premises.sh` → exit 0, last line `test-check-premises: ok (56 cases)`.
- [x] 6. Regression: `sh scripts/premises/ver_test.sh` → exit 0, last line `ver tests: ok (<n>)`.
- [x] 6a. In `scripts/premises/live-hud.sh:303`, replace `PY=$(mktemp -d)` with `PY=$(mktemp -d "${TMPDIR:-/tmp}/tmp.XXXXXXXXXX")`. Check: `sh -n scripts/premises/live-hud.sh` → exit 0; `grep -nE 'mktemp -d *(\)|$)' scripts/premises/live*.sh | grep -v '_test\.sh:'` → no line (exit 1). Do not run live-hud.sh.
- [x] 7. Acceptance, alone as one command: `sh scripts/test-check-premises.sh && N2_FORBIDDEN_NAMES=alice sh scripts/check-premises.sh && sh scripts/check-premises.sh` → exit 0; the second and third commands each end with `check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7 PW6 PW11 PW15 PW15-bash PW15-tool PW15-slash PW1 PW2 PW3 PW4 PW5 PW7 PW9 PW10 PW16)`.
- [x] 8. Commit `fix(premises): PC-8 verbatim schemas exempt from the name scan; indented lines are not table rows` with `scripts/check-premises.sh`, `scripts/test-check-premises.sh`, `scripts/premises/live-hud.sh` and this file (steps ticked, outputs under `## Results`). The message ends with these two lines, each on its own line:
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

## Review round 1 findings

Round 1 review (PR #127, head 0227ac56), all confirmed by the lead. Round 2 work on `phase-4--PC-8`. Line numbers are `scripts/check-premises.sh` at 0227ac56 unless stated.

- F1 (important) `:212-214`, `:265`: the VERBATIM exemption covered the whole file and nothing checked it was still the upstream copy. Fix: for each VERBATIM file that exists, its source file is the same path with the final `.json` replaced by `.source.txt` (derived, not listed: `settings-schema.json` → `settings-schema.source.txt`, `config.schema.json` → `config.schema.source.txt`). The file's sha256 (`shasum -a 256`, else `sha256sum`; neither → fail) must equal the first `sha256: <64 hex>` line of the source file. Else: `verbatim file <f> has no source file <src>`, or `verbatim file <f>: sha256 <got> differs from <want|<none>> in <src>`. A mismatch fails the run, so the path, bearer and name exemptions apply only to a matching file. PC-1 case 13 ("the Codex schema is verbatim") now plants a matching `config.schema.source.txt` beside its hand-made schema. PC-2 (`origin/phase-4--PC-2`, `scripts/premises/codex/base.sh:26-28`) writes the source file in this form; its committed pair matches (52fb96b1…2bac).
- F2 (nit) `:214`: `VX` was `^(<path>|<path>):`, so `<path>:x` was exempt. Fix: `VX` is `^(<path>|<path>):[0-9]+:` (the `grep -n` hit form).
- F3 (nit): case 51 asserts the copied schema holds `alice`; the Codex VERBATIM entry has name-scan cases (59, 60); case 57 pins an indented line in the middle of the Results table.
- F4 (important, existing) `:7`, `:218`: `set -uf` stops `scripts/premises/live*.sh` from expanding, so the live scripts are not scanned. Not fixed in this round. With the glob expanded (`set +f` around the loop), the real tree fails on one line, in a file outside this node's Files, and that line is not a leak: `check-premises: forbidden content (e-mail address): scripts/premises/live-detach.sh:220:  git -C "$C" init -q && git -C "$C" -c user.name=zygo-live -c user.email=zygo-live@example.invalid commit -q --allow-empty -m init`. No other rule hits a live script (all rules, all hits listed, with `N2_FORBIDDEN_NAMES="$(id -un)"`). Per the brief, stopped and reported to the lead; the fix and its two cases (a name and an `sk-ant-` key appended to a live script) wait for a decision on that line (an exemption for `@example.invalid`, or a change to live-detach.sh).
- F5 (nit, existing) `:262`, `:226`: macOS `grep -r` does not follow symlinks. Fix: after `SCOPE` is built, `find $SCOPE -type l`; the first symlink fails: `<path> is a symlink; a symlink in the scan scope is not scanned and is not evidence`. The scope is the one F4 leaves today (fixtures, `scripts/premises/live/`, `docs/live-checks.md`); it gains the `live*.sh` files when F4 lands. Real tree: no symlink in it.

Cases 57-64 (after case 56, before the final `echo`); helper `sha <file>` (lower-case sha256 hex, `shasum -a 256` else `sha256sum`) after `run_names`:

| # | CASE | Plant | Expect | Before the fix |
|---|---|---|---|---|
| 57 | an indented line in the middle of the Results table | `    \| PC9 \| indented \|` after the `\| Q7 \|` row | fail `docs/premises.md:<line of PW6>: row 'PW6' is outside the Results table` | passes (pin of round 1 rule (b)) |
| 58 | a name appended to the verbatim schema | `owner alice` appended to `settings-schema.json`; `run_names alice` | fail `verbatim file …settings-schema.json: sha256 <new> differs from <old> in …settings-schema.source.txt` | exit 0 |
| 59 | a verbatim schema with no source file | `settings-schema.source.txt` removed | fail `verbatim file … has no source file …` | exit 0 |
| 60 | the Codex verbatim schema with a matching source hash | `fixtures/codex/config.schema.json` holding `owner alice`, source with its hash; `run_names alice` | ok | passes |
| 61 | the Codex verbatim schema with a mismatching source hash | as 60, source hash of another file | fail `verbatim file fixtures/codex/config.schema.json: sha256 … differs from … in fixtures/codex/config.schema.source.txt` | exit 0 |
| 62 | a path that only starts with a verbatim path | `fixtures/claude-code/settings-schema.json:x` holding `owner alice`; `run_names alice` | fail `forbidden name from N2_FORBIDDEN_NAMES: fixtures/claude-code/settings-schema.json:x` | exit 0 |
| 63 | a symlink under fixtures | `fixtures/codex/link.txt` → `../opencode/help.txt` | fail `fixtures/codex/link.txt is a symlink; …` | exit 0 |
| 64 | a symlink under the live scripts | `scripts/premises/live/link.txt` → a fixture | fail `scripts/premises/live/link.txt is a symlink; …` | exit 0 |

Steps (round 2):

- [x] R1. Add cases 57-64 and the case 51 assertion; run a non-stopping copy of the harness: 58, 59, 61, 62, 63, 64 fail as in the table; 57, 60 pass. Commit `test(premises): PC-8 round 2 failing cases for verbatim hashes, exact paths and symlinks`.
- [x] R2. In `scripts/check-premises.sh`: the hash check and `VX` (F1, F2), the symlink check (F5); comments updated. Case 13 gains its source file.
- [x] R3. Run the tests (sh and dash), `ver_test.sh`, the Acceptance, and `N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh`; record under Results.

## Results

Round 1, 2026-11-25, on `phase-4--PC-8`:

- Step 2 (after step 1, before the fix): `sh scripts/test-check-premises.sh` → exit 1:
  ```
  test-check-premises: FAIL a forbidden name only in the verbatim schema: exit 1, expected 0
  check-premises: forbidden name from N2_FORBIDDEN_NAMES: fixtures/claude-code/settings-schema.json:22
  ```
  After step 3 only: exit 1, `test-check-premises: FAIL a row indented four blanks: exit 1, expected 0` / `check-premises: PC9: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial` (cases 51-52 pass; 53 fails as planned).
- Step 5: `sh scripts/test-check-premises.sh` → exit 0, `test-check-premises: ok (56 cases)`. Under dash: `dash scripts/test-check-premises.sh` → exit 0, `test-check-premises: ok (56 cases)`.
- Step 6: `sh scripts/premises/ver_test.sh` → exit 0, `ver tests: ok (53)`.
- Step 6a: `sh -n scripts/premises/live-hud.sh` → exit 0; `grep -nE 'mktemp -d *(\)|$)' scripts/premises/live*.sh | grep -v '_test\.sh:'` → no line, exit 1.
- Step 7: Acceptance → exit 0:
  ```
  test-check-premises: ok (56 cases)
  check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7 PW6 PW11 PW15 PW15-bash PW15-tool PW15-slash PW1 PW2 PW3 PW4 PW5 PW7 PW9 PW10 PW16)
  check-premises: note: N2_FORBIDDEN_NAMES not set; name scan skipped
  check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7 PW6 PW11 PW15 PW15-bash PW15-tool PW15-slash PW1 PW2 PW3 PW4 PW5 PW7 PW9 PW10 PW16)
  ```

Round 2, 2026-11-25, on `phase-4--PC-8`:

- R1 (tests at 71212a21, before the fix), non-stopping copy of the harness: 58, 59, 61, 62, 63, 64 each exit 0 (expected 1). 57 and 60 pass.
- R3 (fix at 7f318ec5): `sh scripts/test-check-premises.sh` → exit 0, `test-check-premises: ok (64 cases)`; `dash scripts/test-check-premises.sh` → exit 0, `test-check-premises: ok (64 cases)`; `sh scripts/premises/ver_test.sh` → exit 0, `ver tests: ok (53)`.
- Acceptance → exit 0:
  ```
  test-check-premises: ok (64 cases)
  check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7 PW6 PW11 PW15 PW15-bash PW15-tool PW15-slash PW1 PW2 PW3 PW4 PW5 PW7 PW9 PW10 PW16)
  check-premises: note: N2_FORBIDDEN_NAMES not set; name scan skipped
  check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7 PW6 PW11 PW15 PW15-bash PW15-tool PW15-slash PW1 PW2 PW3 PW4 PW5 PW7 PW9 PW10 PW16)
  ```
- `N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh` → exit 0, `check-premises: ok (P1 … PW16)`.


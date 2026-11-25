# PC-9 — scripts/premises/ver_test.sh: its repo copy includes every evidence path the premise rows cite

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.5 (the premise check by data)
**Files:** `scripts/premises/ver_test.sh`, `scripts/check-premises.sh`, `scripts/test-check-premises.sh`
**Consumes:** `scripts/check-premises.sh` (PC-1)
**Produces:** `scripts/premises/ver_test.sh` whose K cases copy into their temporary tree every path that docs/premises.md rows cite as evidence (derived from the rows, not a fixed list), so K1 and K4 pass
**Acceptance:** `sh scripts/premises/ver_test.sh` → exit 0, last line `ver tests: ok (<n>)`

## Planning notes

- Since OC-1 (merged 53eabe25), K1 and K4 fail on phase-4 with `check-premises: PO1: evidence path scripts/premises/run-opencode-oc1.sh is missing`, because the test's copy omits that file (found by the PC-2 merge agent, 2026-11-25).
- CI does not run ver_test.sh.

## Plan

Capability: `mechanical` (one shell function and one test case in a test script; the plan decides both).

### Premise check (2026-11-25, against origin/phase-4 5cb6a387)

- Confirmed: `sh scripts/premises/ver_test.sh` on 5cb6a387 prints `FAIL: K1 … (exit 1: check-premises: PO1: evidence path scripts/premises/run-opencode-oc1.sh is missing, not a regular file, or trivial|)`, the same for K4, and `ver tests: 2 failed` (53 cases; 51 ok; the plan first said 43 cases, corrected in round 1). The planning note shortens the message; the live text is `scripts/check-premises.sh:165`.
- Cause: the K tree is a fixed copy, `scripts/premises/ver_test.sh:179-190` (check-premises.sh, ver.sh, the zygo spec, conformance.md, `fixtures/`). check-premises reads each Results row's evidence cells (`scripts/check-premises.sh:128`), takes every `` `(fixtures|docs|scripts)/…` `` path (`:161`), rejects an absolute or `..` path (`:164`) and needs each to be a regular, non-symlink, nontrivial file (`:165`; `nontrivial` at `:21-26`). Row PO1 (`docs/premises.md:54`, from OC-1) cites `scripts/premises/run-opencode-oc1.sh`, which the copy omits.
- Today the table lines of `docs/premises.md` cite one path outside `fixtures/`: `scripts/premises/run-opencode-oc1.sh`. `scripts/premises/sse.py` and `scripts/premises/scrub.py` exist and are cited only in the Method bullets (`docs/premises.md:12-16`), on no table line.
- Prototype (scratch tree = the fixed copy plus every path a table line cites): `check-premises: ok (P1 … PO1 PO2)`, exit 0. The real tree passes too: `sh scripts/check-premises.sh` → `check-premises: ok (…)`.
- Spec K§6.2.5 (`docs/specs/2026-11-25-client-model-design.md:302`): check-premises is driven by data from `docs/premises.md`. This node changes only its test; no spec text drifts, so no §5.4 rewrite.
- This node runs no agent binary (no claude, opencode or codex; the P cases use the `printf` stubs at `ver_test.sh:97-99`), touches no PC-2 harness file, needs no EXTRA_ENV name and no network. The Codex real-run rules of the PC-3..PC-6 plans do not apply. CI does not run `ver_test.sh` (no reference in `.github/workflows/`); the node adds no wall-clock assertion.

### Interfaces

- Consumes: `scripts/check-premises.sh` (PC-1), run as `sh "$K/scripts/check-premises.sh"` (it `cd`s to its script folder's parent); its evidence-path rule (the Results loop's `evidence path` checks, `in_repo` and `nontrivial`) and its table-line rule (`TROWS`), cited by name, not line number (round 2, F4).
- Produces (inside `scripts/premises/ver_test.sh`, used nowhere else):
  `kcopy <premises file> <source root> <dest>` (round 2 form) — for each path that matches `` `(fixtures|docs|scripts)/[^` ]+` `` (backticks removed) on a table line of `<premises file>` (a line matching `^ ? ? ?\|`, the `TROWS` rule of check-premises.sh), with no `..` component (the Results loop's `case`), whose directory resolves (`cd -P`, `pwd -P`) to the same path under the source root's real path, and for which `<source root>/<path>` is a regular file and not a symlink: `mkdir -p "<dest>/<dir of path>"`, then `cp "<source root>/<path>" "<dest>/<path>"`. Any other path is skipped silently (check-premises then reports it in the K tree as it would in the real tree). Returns 0; a failed `mkdir` or `cp` prints `kcopy: cannot copy <path>` on stderr and returns 1, and the K setup then fails every K1-K5 case with that message. A superset of the evidence cells (every table line, not only cells 8+ of Results rows) is intended: an extra copied file is a file the real tree has.

### Steps

- [x] 1. Baseline: run `sh scripts/premises/ver_test.sh 2>&1`. Pass: K1 and K4 print `FAIL` with `PO1: evidence path scripts/premises/run-opencode-oc1.sh is missing`; last line `ver tests: 2 failed`; exit 1.
- [x] 2. Write test K6 in `scripts/premises/ver_test.sh`, directly after the `kcase K5 …` line (`:208`). Name: `K6 kcopy copies the paths that table lines cite, and nothing outside the K tree`. If `$kmiss` is set: `no "$name" "$kmiss missing"`. Else write `$T/k6.md` with these three lines (`<base>` is `${REPO##*/}`):
  ```
  | X1 | i | Claude Code | 2.1.328 | 2026-11-25 | verified | `scripts/premises/sse.py` |
  | X2 | i | Claude Code | 2.1.328 | 2026-11-25 | verified | `scripts/../../<base>/scripts/premises/sse.py` |
  Method text `scripts/premises/scrub.py`
  ```
  call `kcopy "$T/k6.md"`, then assert, failing with the reason in parentheses:
  - `cmp -s "$REPO/scripts/premises/sse.py" "$K/scripts/premises/sse.py"` (`sse.py not copied`);
  - `[ ! -e "$T/${REPO##*/}" ]` (`a .. path was copied outside the K tree`);
  - `[ ! -e "$K/scripts/premises/scrub.py" ]` (`a path outside a table line was copied`);
  then `ok "$name"`.
- [x] 3. Run `sh scripts/premises/ver_test.sh 2>&1`. Pass: K6 prints `FAIL` (kcopy is not defined, or the reason `sse.py not copied`); K1 and K4 still `FAIL`; last line `ver tests: 3 failed`.
- [x] 4. Implement `kcopy` (Interfaces) in `scripts/premises/ver_test.sh` directly after `kmiss=""` (`:178`), above the K setup block that calls it, with a comment that cites `check-premises.sh:161-165`. In the block's `if [ -z "$kmiss" ]` branch after `cp -R "$REPO/fixtures" "$K/"`, add `kcopy "$REPO/docs/premises.md"`. Keep the fixed copies (check-premises needs them for itself, not as evidence). POSIX sh only (the file runs under `sh` and `dash`); read the path list with `while IFS= read -r`, not word splitting.
- [x] 5. Run the Acceptance check `sh scripts/premises/ver_test.sh 2>&1`. Pass: every case line is `ok:`, K1, K4 and K6 included; last line `ver tests: ok (54)`; exit 0.
- [x] 6. Run `dash scripts/premises/ver_test.sh 2>&1`. Pass: last line `ver tests: ok (54)`, exit 0.
- [x] 7. Run `sh scripts/check-premises.sh 2>&1`. Pass: last line `check-premises: ok (P1 … PO1 PO2)`, exit 0 (the node does not change it). Run `git diff --name-only origin/phase-4`. Pass: only `scripts/premises/ver_test.sh` and `docs/work/phase4/nodes/PC-9.md`.
- [x] 8. Commit `fix(premises): PC-9 ver_test K tree copies cited evidence paths` with trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:PC-9`; push the node branch.

## Review round 1 findings

Round 1 review (PR #136, head cdba2b8a), all confirmed by the lead. Round 2 work on `phase-4--PC-9`, after a merge of origin/phase-4 (PC-8, 73a4ff40; DONE chore 297b4613). The lead added `scripts/check-premises.sh` and `scripts/test-check-premises.sh` to Files (F1's root cause is there). Each case was written before its fix.

- F1 (important): an evidence path through a symlinked directory (`scripts/premises/ext/ev.sh`, `ext` → outside the repository) passed `[ -f ] && [ ! -L ]`, which tests only the last component, in kcopy and in check-premises.sh `nontrivial`. Fix: check-premises.sh gets `REAL=$(pwd -P)` and `in_repo <path>`: the path's directory, resolved with `cd -P`/`pwd -P`, must equal `$REAL/<directory>` (a missing directory passes; `nontrivial` then reports the path). The Results loop runs it before `nontrivial`: `<id>: evidence path <f> passes through a symlink or resolves outside the repository`. kcopy applies the same rule against its source root's real path and skips the path. Cases: test-check-premises.sh "an evidence path through a symlinked directory"; ver_test.sh K10.
- F2 (important): kcopy's symlink and regular-file guards were untested. Fix: kcopy takes `<source root> <dest>`; K7-K10 (`kguard`) build a scratch source tree `$T/ksrc` and cite a symlink file (K7), a directory (K8), a missing path (K9) and a file under a symlinked directory (K10); each needs kcopy to return 0 and leave its own folder empty. Mutation check: removing the realpath guard fails K10; removing `! -L` fails K7; removing `-f` fails K8 and K9.
- F3 (important): K6 ran kcopy into the shared `$K` that the setup had filled. Fix: `kfresh <case>` gives each kcopy case (K6-K12) its own empty folder `$T/<case>` and its own premises file `$T/<case>.md`; the assertions read only that folder. K6 failed (`sse.py not copied`) until kcopy took a destination.
- F4 (nit): the line cites `check-premises.sh:161-165` (kcopy comment) and `:161-167` (Interfaces) were stale after PC-8. Fix: both cite by name (the `TROWS` rule, the Results loop's evidence path checks, `in_repo`, `nontrivial`).
- F5 (nit): kcopy's table-line rule `^[[:blank:]]*\|` was wider than check-premises.sh's `TROWS` (zero to three blanks, no tab). Fix: kcopy uses `^ ? ? ?\|`. Case K11 (three blanks copied; four blanks and a tab not); it failed before the fix, and a mutation back to the wide rule fails it.
- F6 (nit): a failed copy was silent. Fix: a failed `mkdir` or `cp` prints `kcopy: cannot copy <path>` on stderr and returns 1; the K setup stores that message in `kmiss`, so K1-K5 fail with it. Case K12 (a read-only destination); it failed before the fix (`kcopy exit 0`).

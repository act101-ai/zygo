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

Capability: `mechanical` (one shell function and one test case in a test script; the plan decides both).

### Premise check (2026-11-25, against origin/phase-4 5cb6a387)

- Confirmed: `sh scripts/premises/ver_test.sh` on 5cb6a387 prints `FAIL: K1 … (exit 1: check-premises: PO1: evidence path scripts/premises/run-opencode-oc1.sh is missing, not a regular file, or trivial|)`, the same for K4, and `ver tests: 2 failed` (53 cases; 51 ok; the plan first said 43 cases, corrected in round 1). The planning note shortens the message; the live text is `scripts/check-premises.sh:165`.
- Cause: the K tree is a fixed copy, `scripts/premises/ver_test.sh:179-190` (check-premises.sh, ver.sh, the zygo spec, conformance.md, `fixtures/`). check-premises reads each Results row's evidence cells (`scripts/check-premises.sh:128`), takes every `` `(fixtures|docs|scripts)/…` `` path (`:161`), rejects an absolute or `..` path (`:164`) and needs each to be a regular, non-symlink, nontrivial file (`:165`; `nontrivial` at `:21-26`). Row PO1 (`docs/premises.md:54`, from OC-1) cites `scripts/premises/run-opencode-oc1.sh`, which the copy omits.
- Today the table lines of `docs/premises.md` cite one path outside `fixtures/`: `scripts/premises/run-opencode-oc1.sh`. `scripts/premises/sse.py` and `scripts/premises/scrub.py` exist and are cited only in the Method bullets (`docs/premises.md:12-16`), on no table line.
- Prototype (scratch tree = the fixed copy plus every path a table line cites): `check-premises: ok (P1 … PO1 PO2)`, exit 0. The real tree passes too: `sh scripts/check-premises.sh` → `check-premises: ok (…)`.
- Spec K§6.2.5 (`docs/specs/2026-11-25-client-model-design.md:302`): check-premises is driven by data from `docs/premises.md`. This node changes only its test; no spec text drifts, so no §5.4 rewrite.
- This node runs no agent binary (no claude, opencode or codex; the P cases use the `printf` stubs at `ver_test.sh:97-99`), touches no PC-2 harness file, needs no EXTRA_ENV name and no network. The Codex real-run rules of the PC-3..PC-6 plans do not apply. CI does not run `ver_test.sh` (no reference in `.github/workflows/`); the node adds no wall-clock assertion.

### Interfaces

- Consumes: `scripts/check-premises.sh` (PC-1), run as `sh "$K/scripts/check-premises.sh"` (it `cd`s to its script folder's parent, `:8`); its evidence-path rule `:161-167` as quoted above.
- Produces (inside `scripts/premises/ver_test.sh`, used nowhere else):
  `kcopy <premises file>` — for each path that matches `` `(fixtures|docs|scripts)/[^` ]+` `` (backticks removed) on a table line of `<premises file>` (a line matching `^[[:blank:]]*\|`), with no `..` component (the `case` of `check-premises.sh:164`), and for which `$REPO/<path>` is a regular file and not a symlink: `mkdir -p "$K/<dir of path>"`, then `cp "$REPO/<path>" "$K/<path>"`. Any other path is skipped silently (check-premises then reports it in the K tree as it would in the real tree). Always returns 0. A superset of the evidence cells (every table line, not only cells 8+ of Results rows) is intended: an extra copied file is a file the real tree has.

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

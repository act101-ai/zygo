# TF-2 — CI runs the premise test scripts

**Spec:** none (CI defect); PC-13 round-2 review G3; Global constraints "No CI test runs a real `claude`, `opencode` or `codex` binary" and "No test … may pass or fail by wall-clock time"
**Files:** `.github/workflows/ci.yml`, `scripts/premises/lib_test.sh` (CI-friendly waits and its "not run by CI" header line only)
**Consumes:** —
**Produces:** CI job `test` runs the five premise test scripts on `ubuntu-latest` and `macos-latest`
**Acceptance:** each command run alone from the repository root: `test "$(grep -oE 'scripts/(premises/scrub_test\.py|premises/rescrub_test\.py|test-check-premises\.sh|premises/lib_test\.sh|check-premises\.sh)' .github/workflows/ci.yml | sort -u | wc -l)" -eq 5` → exit 0 (ci.yml names each of the five scripts); `python3 -I scripts/premises/scrub_test.py` → exit 0; `python3 -I scripts/premises/rescrub_test.py` → exit 0; `sh scripts/test-check-premises.sh` → exit 0; `sh scripts/premises/lib_test.sh` → exit 0; `N2_FORBIDDEN_NAMES=zzcinameq sh scripts/check-premises.sh` → exit 0; then the CI run of the node's pull request is green on ubuntu, macOS and Windows, with the five new steps run on ubuntu and macOS. No real agent runs in CI (every script uses stubs or none); no network beyond what the scripts already use (127.0.0.1 only).

## Planning notes

- The defect: `.github/workflows/ci.yml:52-59` (job `test`) runs only `cargo test --workspace --locked --no-fail-fast`, `sh scripts/test-install.sh` and `pwsh scripts/test-install.ps1`. No workflow runs `scripts/premises/scrub_test.py`, `scripts/premises/rescrub_test.py`, `scripts/test-check-premises.sh`, `scripts/premises/lib_test.sh` or `scripts/check-premises.sh`. `test-check-premises.sh` broke on phase-4 after PC-12 (the real `Codex: 0.172.0` header, PC-13 planning notes) and nobody saw it until PC-13.
- Runs after PC-13: PC-13 repairs `test-check-premises.sh` and adds `rescrub_test.py` (on `origin/phase-4--PC-13`, not yet on `phase-4`). Before PC-13 merges, the CI step for `test-check-premises.sh` fails and `rescrub_test.py` does not exist.
- CI configuration changes are allowed. This node changes no registry standard text, tier or bar exception (K§6.2.1), and no boundary exception list, so it needs no operator stop.
- What each script needs in CI:
  - `scrub_test.py`: `python3` only (on every runner). It reads the host and user names at run time (`scrub_test.py:21-23`) and calls `scutil` only where it exists (`:183-190`, macOS). No network.
  - `rescrub_test.py` (PC-13): `python3` only; it builds a temporary fixtures tree and pops `N2_FORBIDDEN_NAMES` itself. No network.
  - `test-check-premises.sh`: POSIX `sh`, `awk`, `sed`, `grep`, `cp`, `mktemp`; it runs `check-premises.sh` on temporary copies and sets what each case needs. No network.
  - `check-premises.sh`: POSIX `sh`, `awk`, `grep`, and `shasum` or `sha256sum` (`check-premises.sh:263-266`); `N2_FORBIDDEN_NAMES` is optional (`:6-8`, `:395`). CI sets the fixed word `N2_FORBIDDEN_NAMES=zzcinameq`, not the runner user: the runner user on ubuntu and macOS is `runner`, and `fixtures/claude-code/options-2.1.331.txt:127` holds `--runner-declares`, a whole-word hit for the scan (word class `A-Za-z0-9_`, `:374`), so `$(id -un)` fails on CI. The fixed word runs the scan path; the host-name scan (`:375-394`) runs on the runner's own host names as it does locally.
  - `lib_test.sh`: no tmux, no real agent, no network beyond `127.0.0.1` (its mock and `HTTPS_PROXY` cases). It needs `cc` (it compiles the stub `codex`, `lib_test.sh:31-56`; gcc on ubuntu, clang on macOS), `perl` (`:120`, `:916`), `python3`, `git`, `shasum`, `file`, `ps`, `uname`, and `lsof` where there is no `/proc` (`:60-62`, macOS). `dash` is used when installed (`:13`; ubuntu has it, macOS falls back to `sh`). The root-only cases skip under root (`:214`, `:318`); runners are not root. Its header says "not run by CI" (`:7`).
  - `lib_test.sh` timing: `waitready` gives up after 50 polls 0.1 s apart (`:106-114`) and the case then fails with `NR=1` (`:127-131`, `:918`); three inline loops give up after 100 polls (`:994`, `:1046`, `:1131`). On a slow runner a case can fail by wall-clock time. The same rule as the Global constraint applies: wait on the ready file with no count limit; the CI job timeout is the bound.
- OS matrix (`ci.yml:43`, `ubuntu-latest`, `macos-latest`, `windows-latest`): the five steps run on `ubuntu-latest` and `macos-latest` and are skipped on `windows-latest` (`if: matrix.os != 'windows-latest'`, as `ci.yml:55`). Reasons: the three sh scripts are POSIX `sh` and `lib_test.sh` needs `cc`, `ps`, symbolic links and `/proc` or `lsof`; `check-premises.sh:42` refuses CRLF in `docs/premises.md`, which a Windows checkout with `core.autocrlf` gives; the premise harness that `scrub.py` serves runs on unix only (`scripts/premises/lib.sh`), so the two python tests skip Windows too. Running any of them on Windows is out of scope.
- CI paths (`ci.yml:3-32`): `docs/premises.md`, `docs/live-checks.md`, `docs/conformance.md` and `docs/specs/2026-11-18-zygo-design.md` already run CI. `check-premises.sh` also reads `docs/specs/2026-11-20-workspaces-design.md` (`check-premises.sh:16`), which does not run CI today; add it to both lists.
- Files overlap: no open node (PC-7, PC-13, RS-12, BS-1, CC-0 to CC-16) lists `.github/workflows/ci.yml` in Files; only CF-4 (DONE) did. `main` has no commit on `ci.yml` after `phase-4` (`git log origin/phase-4..origin/main -- .github/workflows/ci.yml` is empty), so CC-0's merge does not touch it. `lib_test.sh` is in no open node's Files. Edges: PC-13 → TF-2 → BS-1.

## Plan

Capability: `mechanical` (CI steps, one wait helper, no new test logic).

### Steps

- [x] 1. Confirm PC-13 is merged into `phase-4` (`scripts/premises/rescrub_test.py` exists; `sh scripts/test-check-premises.sh` exits 0). Run each of the five Acceptance script commands locally. Pass: each exits 0; note each final line.
- [x] 2. In `scripts/premises/lib_test.sh`, make `waitready` (`:106-114`) wait with no count limit, and remove the `$i -lt 100` limit from the three inline loops (`:994`, `:1046`, `:1131`). Keep each case's assertions and expected results. Change the header line 7 "not run by CI" to say CI runs it on ubuntu and macOS. Run `sh scripts/premises/lib_test.sh`. Pass: exit 0, the same case count as step 1.
- [x] 3. Add one CI step per script to job `test` in `.github/workflows/ci.yml`, after "Install script (sh)", each with `if: matrix.os != 'windows-latest'`, in this order (a failed step stops the later ones, so the run is fail-fast and names the failing script):
  - `Premise scrub tests`: `python3 -I scripts/premises/scrub_test.py`;
  - `Premise re-scrub tests`: `python3 -I scripts/premises/rescrub_test.py`;
  - `Premise check tests`: `sh scripts/test-check-premises.sh`;
  - `Premise harness tests`: `sh scripts/premises/lib_test.sh`;
  - `Premise check`: `sh scripts/check-premises.sh` with `env: N2_FORBIDDEN_NAMES: zzcinameq`.
- [x] 4. Add `- 'docs/specs/2026-11-20-workspaces-design.md'` to the `push` and the `pull_request` path lists of `ci.yml`, beside `docs/specs/2026-11-18-zygo-design.md`.
- [x] 5. Run the Acceptance `grep` check. Pass: exit 0 (5 names). Run `git diff origin/phase-4 -- .github/workflows/ci.yml scripts/premises/lib_test.sh`. Pass: only the steps, the path lines, the wait loops and the header line changed.
- [x] 6. Commit `ci: TF-2 run the premise test scripts` with trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:TF-2`; push the node branch; open the pull request. Pass: CI green on ubuntu, macOS and Windows; on ubuntu and macOS the five steps ran and passed; on Windows they show as skipped. Do not rerun CI without the operator's word.

### Execution notes

- Step 1 (base `e4fb2e37`, PC-13 merged): `scrub tests: ok (158)`; `rescrub tests: ok (172)`;
  `test-check-premises: ok (173 cases)`; `lib tests: ok (157)`; `check-premises: ok (P1 … PC18)`
  with `N2_FORBIDDEN_NAMES=zzcinameq`.
- Step 2: `lib tests: ok (157)` after the change, the same count. The callers of `waitready` keep
  their not-ready branches (`sigrun`, R5, B4, X34, X48, and the `NR` message "not ready in 5 s" in
  `res`); with no count limit these branches no longer run. They are left as they are, because the
  plan keeps each case's assertions unchanged and these lines are outside the plan's change list.
- Step 5: each of the five script names occurs once in `ci.yml` (`grep -c` per name → 1); the
  diff holds only the five steps, the two path lines, `waitready`, the three inline loops and the
  header line (rewrapped to the file's line width).

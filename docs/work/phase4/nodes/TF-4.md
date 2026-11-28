# TF-4 — Premise tests leave no `__pycache__` in the checkout

**Spec:** none (dev-host and CI hygiene defect), as TF-3 (TF-3 cites no spec section); the harness method: `docs/premises.md` "Method"; found by the dash test run (2026-11-27)
**Files:** `scripts/test-check-premises.sh` (case "PC-13 R1: rescrub_test.py", the `python3` line only), `.github/workflows/ci.yml` (step "Premise re-scrub tests" only), `scripts/premises/rescrub_test.py` (the usage line of the docstring only)
**Consumes:** —
**Produces:** no premise test run writes Python bytecode into the checkout
**Acceptance:** each command run alone from the repository root of a clean tree (`git status --porcelain` prints nothing before): `sh scripts/test-check-premises.sh` → exit 0; `python3 -I scripts/premises/scrub_test.py` → exit 0; then `git status --porcelain` → prints nothing.

## Planning notes

- The defect (2026-11-27, `phase-4` at `636f52a3`, Python 3.14.10): after the premise tests, `git status --porcelain` prints `?? scripts/premises/__pycache__/`, and the folder holds only `rescrub_codex.cpython-314.pyc`. `.gitignore` (`/target`, `/.worktrees/`, `.claude/`, `.DS_Store`) does not ignore it, so the tree is dirty and `git worktree remove` refuses.
- The cause, measured one command at a time on the main checkout with the folder removed first:
  - `python3 -I scripts/premises/scrub_test.py` → exit 0, no folder. It starts `scrub.py` only as a script (`subprocess` with `sys.executable -I SCRUB`) or through `runpy.run_path`, and neither writes bytecode for the file it runs.
  - `python3 -I scripts/premises/rescrub_test.py` → exit 0, folder with `rescrub_codex.cpython-314.pyc`. `rescrub_test.py:18-20` loads `rescrub_codex.py` with `importlib.util.spec_from_file_location` and `spec.loader.exec_module`, an import, which writes the bytecode cache.
  - `python3 -I scripts/premises/hud_probe_test.py` → exit 0, no folder: it loads `hud_probe.py` the same way but sets `sys.dont_write_bytecode = True` first (`hud_probe_test.py:14`, its round 2 finding 9).
  - `PYTHONDONTWRITEBYTECODE=1 python3 -I scripts/premises/rescrub_test.py` → folder written: `-I` implies `-E`, so Python ignores every `PYTHON*` variable. `python3 -I -B scripts/premises/rescrub_test.py` → no folder.
- The places that start `rescrub_test.py`: `scripts/test-check-premises.sh:1449` (case "PC-13 R1: rescrub_test.py", so the defect appears through `sh scripts/test-check-premises.sh`) and `.github/workflows/ci.yml:79` (step "Premise re-scrub tests"). No other script starts it. No other premise script imports a repo module: `lib_test.sh` starts `mock_upstream.py` and `https_static.py` through `runpy.run_path` (`nofqdn.py`, `lib_test.sh:1301-1317`), and its other `python3` lines run `-c` code or helpers it writes under `$T`.
- The fix (author's call): `-B` at each place that starts `rescrub_test.py` (`python3 -I -B`), and the same in the docstring usage line of `rescrub_test.py`, so a run as documented leaves nothing. `PYTHONDONTWRITEBYTECODE=1` is not used: it has no effect under `-I`. No `__pycache__/` line in `.gitignore`: Python in the tree is only the premise harness (`scripts/premises/*.py`) and three fixture files (`fixtures/act101-marketplace/…/work_graph.py`, `fixtures/claude-code/scan/*.py`) that no script runs; the project has no Python package (no `pyproject.toml`, `setup.py` or `requirements*.txt`). An ignore line would also hide a regression from this node's acceptance check.
- The check: the case's output is unchanged (`rescrub tests: ok (172)`); `-B` changes only whether bytecode is written.
- No registry standard text, tier, bar exception or boundary exception list changes; no operator stop.

## Plan

Capability: `mechanical` (one flag at two start sites and one doc line).

### Steps

- [ ] 1. Failing check first. In `.worktrees/phase4/TF-4` (branch `phase-4--TF-4` from `origin/phase-4`), with a clean tree, run `sh scripts/test-check-premises.sh` and `python3 -I scripts/premises/scrub_test.py`, then `git status --porcelain`. Pass: both runs exit 0 and `git status --porcelain` prints `?? scripts/premises/__pycache__/`. Remove the folder after.
- [ ] 2. The fix. `scripts/test-check-premises.sh:1449`: `python3 -I "$ROOT/scripts/premises/rescrub_test.py"` → `python3 -I -B "$ROOT/scripts/premises/rescrub_test.py"`. `.github/workflows/ci.yml:79`: `python3 -I scripts/premises/rescrub_test.py` → `python3 -I -B scripts/premises/rescrub_test.py`. `scripts/premises/rescrub_test.py:3`: the usage line names `python3 -I -B`. Pass: `git diff` shows only these three lines.
- [ ] 3. Acceptance. Each command alone from the worktree root, on a clean tree: `sh scripts/test-check-premises.sh`; `python3 -I scripts/premises/scrub_test.py`; `git status --porcelain`. Pass: both exit 0, the last line of the first is `test-check-premises: ok (<N> cases)`, and `git status --porcelain` prints nothing.
- [ ] 4. Commit `fix(premises): TF-4 no bytecode in the checkout` with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:TF-4`; no Claude attribution. Push the node branch (no force). Open the pull request into `phase-4`.

### Execution notes

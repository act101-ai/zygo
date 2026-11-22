# FIX-12 — Python premise tests leave `scripts/premises/__pycache__`

**Spec:** found by PRB-1. Defect: `scripts/premises/hud_probe_test.py` (from PRB-1) loads `hud_probe.py` with `importlib.util.spec_from_file_location` (line 17), so every run writes `scripts/premises/__pycache__/hud_probe.cpython-<ver>.pyc` (seen in `.worktrees/phase2/PRB-1`) and `git status` is not clean after the test. The repository `.gitignore` has no `__pycache__` entry (today: `/target`, `/.worktrees/`, `.claude/`, `.DS_Store`). Add the line `__pycache__/` to the repository `.gitignore`, so every Python bytecode folder in the repository is ignored; no test or script changes
**Files:** `.gitignore`
**Consumes:** —
**Produces:** —
**Acceptance:** `python3 scripts/premises/hud_probe_test.py && test -z "$(git status --short --untracked-files=all scripts/premises)"` → exit 0

## Plan

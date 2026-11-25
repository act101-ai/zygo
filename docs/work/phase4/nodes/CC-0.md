# CC-0 — Take `main` (with phase 3) into `phase-4`

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.9 (P4-6 row: "phase 3 merged into `main` and taken by `phase-4`"); `docs/roadmap/phase-4.md` "Plan of work" item 5
**Files:** `docs/live-checks.md` (merge conflict resolution only; no other edit)
**Consumes:** —
**Produces:** `phase_3_in_phase_4` (the merge commit on `phase-4` whose parents are the `phase-4` head and an `origin/main` that holds phase 3)
**Acceptance:** `git fetch origin && git merge-base --is-ancestor b7e9d45d origin/main && { ! git rev-parse -q --verify refs/remotes/origin/phase-3 >/dev/null || git merge-base --is-ancestor origin/phase-3 origin/main; } && git merge-base --is-ancestor origin/main HEAD && git merge-base --is-ancestor origin/phase-4 HEAD && test -d crates/zygo-mail-sync && sh scripts/check-premises.sh && cargo test --workspace --locked --no-fail-fast` → every command exits 0 (`b7e9d45d` is a `origin/phase-3` commit at plan time; the second test holds for any later phase 3 head while that branch exists), and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI

## Planning notes

- State `BLOCKED(operator: phase 3 merged into main)`: only the operator merges phase 3 into `main`. At plan time (2026-11-25) `origin/main` is `bf3726bd`; `origin/phase-3` is at `b6b4bdac` and holds `b7e9d45d`; neither is in `main`.
- Depends on every CF, PC, RS and BS node (coordinator ruling C2, 2026-11-25): the rows that start now are all done before the contract starts, so the merge base is one `phase-4` commit with no open stack above it.
- When the operator has merged phase 3, the controller sets the state to `TODO`. The node merges `origin/main` into `phase-4` as its own layer (Workflows (2)); a merge, not a rebase.
- `docs/live-checks.md`: phase 3 adds a section (K§6.9) and PC-7 adds the `Codex` header and LC-CX1. The merge keeps both edits. Any other conflict keeps the behavior of each side, and the pull request lists each resolved hunk.
- No client-contract work in this node. The inventory is CC-1.

## Test logic edits

None.

## Plan

Written when the node starts.

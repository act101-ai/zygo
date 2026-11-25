# CC-0 — Take `main` (with phase 3) into `phase-4`

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.9 (P4-6 row: "phase 3 merged into `main` and taken by `phase-4`"); `docs/roadmap/phase-4.md` "Plan of work" item 5
**Files:** — (a merge commit; conflict resolution only, no other edit)
**Consumes:** —
**Produces:** `phase_3_in_phase_4` (the merge commit on `phase-4` whose parents are the `phase-4` head and an `origin/main` that holds phase 3)
**Acceptance:** `git fetch origin && git merge-base --is-ancestor b7e9d45d origin/main && { ! git rev-parse -q --verify refs/remotes/origin/phase-3 >/dev/null || git merge-base --is-ancestor origin/phase-3 origin/main; } && git merge-base --is-ancestor origin/main HEAD && test -d crates/zygo-mail-sync && cargo test --workspace --locked --no-fail-fast` → every command exits 0 (`b7e9d45d` is the `origin/phase-3` head when this graph was planned; the second test holds for any later phase 3 head while that branch exists), and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI

## Planning notes

- State `BLOCKED(operator: phase 3 merged into main)`: only the operator merges phase 3 into `main`. At plan time (2026-11-25) `origin/main` is `bf3726bd` and `origin/phase-3` is `b7e9d45d`, which is not in `main`.
- When the operator has merged phase 3, the controller sets the state to `TODO`. The node merges `origin/main` into the top of the `phase-4` stack as its own layer (Workflows (2)). It is a merge, not a rebase, so the merged phase 4 layers keep their hashes.
- Phase 3 does not change `crates/zygo-proxy/src`, the conformance scripts or `docs/conformance.md` (K§6.9). It adds a section to `docs/live-checks.md`, which PC-7 also edits: a conflict there keeps both sides. Any other conflict keeps the behavior of each side, and the pull request lists each resolved hunk.
- No client-contract work in this node. The inventory is CC-1.

## Plan

Written when the node starts.

#!/bin/sh
# The per-node test-edit check of K-C7 (amended), docs/specs/2026-11-25-client-model-design.md.
# Usage: sh scripts/check-test-edits.sh <parent> <node file>
#   <parent>     the parent of the node's layer (the top of the stack below it); an ancestor of
#                HEAD, not HEAD itself
#   <node file>  the node file, read at HEAD; its "## Test logic edits" entries list each allowed
#                change
# In each test file and each test item that exists at <parent>, a removed or renamed test
# function, an added #[ignore], a changed cfg, cfg_attr or should_panic attribute or a changed
# multiset of literals at HEAD is an offence unless the node file lists that exact change; so is
# a changed doc-test block, a changed non-.rs file under a tests folder, or a change to the check.
# Exit 0: pass; 1: one offence per line on stdout; 2: usage, uncommitted changes, or the helper
# did not run (any other exit is mapped to 2).
# The helper is the crate zygo-test-edits (tools/test-edits). cargo keeps the caller's working
# folder, so the check reads the repository it is run in, from any of its folders.
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cargo run -q --locked --manifest-path "$ROOT/Cargo.toml" -p zygo-test-edits -- "$@"
rc=$?
case $rc in
    0 | 1 | 2) exit "$rc" ;;
esac
printf '%s\n' "check-test-edits: the helper exited $rc (build or run failure); exit 2" >&2
exit 2

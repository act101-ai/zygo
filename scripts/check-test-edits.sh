#!/bin/sh
# The per-node test-edit check of K-C7 (amended), docs/specs/2026-11-25-client-model-design.md.
# Usage: sh scripts/check-test-edits.sh <parent> <node file>
#   <parent>     the parent of the node's layer (the top of the stack below it)
#   <node file>  the node file; its "## Test logic edits" entries list each allowed change
# In each test file and each #[cfg(test)] item that exists at <parent>, a removed or renamed test
# function, an added #[ignore] or a changed multiset of literals at HEAD is an offence unless the
# node file lists that exact change. Exit 0: pass; 1: one offence per line on stdout; 2: usage.
# The helper is the crate zygo-test-edits (tools/test-edits). cargo keeps the caller's working
# folder, so the check reads the repository it is run in.
ROOT=$(cd "$(dirname "$0")/.." && pwd)
exec cargo run -q --locked --manifest-path "$ROOT/Cargo.toml" -p zygo-test-edits -- "$@"

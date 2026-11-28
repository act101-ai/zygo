#!/bin/sh
# The per-node test-edit check of K-C7 (amended), docs/specs/2026-11-25-client-model-design.md.
# Usage: sh scripts/check-test-edits.sh <parent> <node file>
#   <parent>     the parent of the node's layer: git merge-base HEAD <layer base branch>
#                (origin/phase-4 for a layer on the trunk), recomputed after every merge into the
#                node branch. A commit of the node's own branch is a wrong parent. The node writes
#                it as the first line of its "## Plan": "Parent: <40-hex>".
#   <node file>  the node file, read at HEAD; its "## Test logic edits" entries list each allowed
#                change
# In each test file and each test item that exists at <parent>, a removed or renamed test
# function, an added #[ignore], a changed cfg, cfg_attr or should_panic attribute anywhere on the
# test's path to its crate root, or a changed multiset of literals at HEAD is an offence unless
# the node file lists that exact change; so is a changed doc-test block, test-relevant manifest
# key, fixture, or file of the check itself.
# The checker does not judge itself: the helper (crate zygo-test-edits, tools/test-edits) is
# built from <parent>, cached by the tree id of tools/test-edits at <parent>, and run against
# this repository. The review gate runs the parent's copy of this script too:
#   git show <parent>:scripts/check-test-edits.sh | sh -s -- <parent> <node file>
# ZYGO_TEST_EDITS_IN_TREE=1 runs the helper of this working tree instead: the bootstrap, used only
# by node CC-2's own tests. ZYGO_TEST_EDITS_CACHE sets the cache folder (default
# ${XDG_CACHE_HOME:-$HOME/.cache}/zygo-test-edits).
# Exit 0: pass; 1: one offence per line on stdout; 2: usage, no checker at <parent>, uncommitted
# changes, or the helper did not build or run (any exit other than 0, 1 and 2 is mapped to 2).

usage() {
    printf '%s\n' "check-test-edits: $1" "usage: sh scripts/check-test-edits.sh <parent> <node file>" >&2
    exit 2
}

[ "$#" -eq 2 ] || usage "two arguments are required"
[ -n "$1" ] || usage "<parent> is empty"
git rev-parse --show-toplevel > /dev/null 2>&1 || usage "not inside a git repository"
PSHA=$(git rev-parse --verify --quiet "$1^{commit}") || usage "<parent> is not a commit: $1"

printf '%s\n' "check-test-edits: parent $PSHA" >&2
if BASE=$(git rev-parse --verify --quiet "refs/remotes/origin/phase-4^{commit}"); then
    MB=$(git merge-base HEAD "$BASE" 2> /dev/null)
    if [ "$MB" != "$PSHA" ]; then
        printf '%s\n' "check-test-edits: warning: <parent> is not git merge-base HEAD origin/phase-4 ($MB); a stacked layer may use a lower layer" >&2
    fi
fi

# run_helper <command...>: runs the helper and maps its exit.
run_helper() {
    "$@"
    rc=$?
    case $rc in
        0 | 1 | 2) exit "$rc" ;;
    esac
    printf '%s\n' "check-test-edits: the helper exited $rc (build or run failure); exit 2" >&2
    exit 2
}

if [ "${ZYGO_TEST_EDITS_IN_TREE:-}" = 1 ]; then
    ROOT=$(cd "$(dirname "$0")/.." && pwd)
    run_helper cargo run -q --locked --manifest-path "$ROOT/Cargo.toml" -p zygo-test-edits -- "$@"
fi

TREE=$(git rev-parse --verify --quiet "$PSHA:tools/test-edits") || usage "no checker at <parent>: $PSHA has no tools/test-edits"
CACHE=${ZYGO_TEST_EDITS_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zygo-test-edits}
BIN="$CACHE/bin/$TREE/zygo-test-edits"
if [ ! -x "$BIN" ]; then
    mkdir -p "$CACHE/bin/$TREE" || usage "cannot write the cache $CACHE"
    SRC=$(mktemp -d) || usage "mktemp failed"
    trap 'rm -rf "$SRC"' EXIT
    git archive --format=tar "$PSHA" | (cd "$SRC" && tar -xf -) || usage "git archive of $PSHA failed"
    printf '%s\n' "check-test-edits: building the checker of <parent> (tree $TREE)" >&2
    CARGO_TARGET_DIR="$CACHE/target" cargo build -q --release --locked --manifest-path "$SRC/Cargo.toml" -p zygo-test-edits || {
        printf '%s\n' "check-test-edits: the checker of <parent> does not build; exit 2" >&2
        exit 2
    }
    OUT="$CACHE/target/release/zygo-test-edits"
    [ -f "$OUT" ] || OUT="$OUT.exe"
    cp "$OUT" "$BIN.tmp.$$" && mv "$BIN.tmp.$$" "$BIN" || usage "cannot store the checker in $CACHE"
    rm -rf "$SRC"
    trap - EXIT
fi
run_helper "$BIN" "$@"

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
# built from <parent> and run against this repository. The review gate runs the parent's copy of
# this script too, from the top folder or a subfolder:
#   git show <parent>:scripts/check-test-edits.sh | sh -s -- <parent> <node file>
# Build cache: ZYGO_TEST_EDITS_CACHE (default ${XDG_CACHE_HOME:-$HOME/.cache}/zygo-test-edits).
# The key is a hash of the tree of tools/test-edits, the root Cargo.toml and Cargo.lock, the
# rust-toolchain file (if any) at <parent>, and "rustc -V". Each key has its own sources
# (src/<key>), its own cargo target folder (target/<key>) and its own lock (lock/<key>); each run
# does "cargo build --release --locked" there and runs the binary of that target folder.
# ZYGO_TEST_EDITS_IN_TREE=1 runs the helper of this working tree instead (cargo run in the top
# folder of the current repository, or in ZYGO_TEST_EDITS_ROOT if set). It is the bootstrap of
# node CC-2 only: it is refused when <parent> has a tools/test-edits.
# Exit 0: pass; 1: one offence per line on stdout; 2: usage, no checker at <parent>, a refused
# bootstrap, uncommitted changes, or the helper did not build or run (any exit other than 0, 1
# and 2 is mapped to 2).

usage() {
    printf '%s\n' "check-test-edits: $1" "usage: sh scripts/check-test-edits.sh <parent> <node file>" >&2
    exit 2
}

fail() {
    printf '%s\n' "check-test-edits: $1" >&2
    exit 2
}

[ "$#" -eq 2 ] || usage "two arguments are required"
[ -n "$1" ] || usage "<parent> is empty"
TOP=$(git rev-parse --show-toplevel 2> /dev/null) || usage "not inside a git repository"
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
    fail "the helper exited $rc (build or run failure); exit 2"
}

TREE=$(git rev-parse --verify --quiet "$PSHA:tools/test-edits")

if [ "${ZYGO_TEST_EDITS_IN_TREE:-}" = 1 ]; then
    [ -z "$TREE" ] || fail "ZYGO_TEST_EDITS_IN_TREE=1 is refused: <parent> $PSHA has a checker (tools/test-edits); run the checker of <parent>"
    ROOT=${ZYGO_TEST_EDITS_ROOT:-$TOP}
    [ -f "$ROOT/tools/test-edits/Cargo.toml" ] || fail "ZYGO_TEST_EDITS_IN_TREE=1: no tools/test-edits in $ROOT"
    run_helper cargo run -q --locked --manifest-path "$ROOT/Cargo.toml" -p zygo-test-edits -- "$@"
fi

[ -n "$TREE" ] || fail "no checker at <parent>: $PSHA has no tools/test-edits"
CACHE=${ZYGO_TEST_EDITS_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/zygo-test-edits}
mkdir -p "$CACHE/src" "$CACHE/target" "$CACHE/lock" || fail "cannot write the cache $CACHE"

# The sources of <parent>, extracted to a new folder; "rustc -V" runs there, so a rust-toolchain
# file of <parent> selects the toolchain as it does for the build.
NEW=$(mktemp -d "$CACHE/src/new.XXXXXX") || fail "cannot write the cache $CACHE"
LOCK=""
cleanup() {
    [ -z "$NEW" ] || rm -rf "$NEW"
    [ -z "$LOCK" ] || rm -rf "$LOCK"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
# (git archive in a subfolder archives only that subfolder: run it in the top folder.)
git -C "$TOP" archive --format=tar "$PSHA" | (cd "$NEW" && tar -xf -) || fail "git archive of $PSHA failed"
RUSTC=$(cd "$NEW" && rustc -V) || fail "rustc -V failed"
KEY=$(
    {
        printf 'tools/test-edits %s\n' "$TREE"
        for f in Cargo.toml Cargo.lock rust-toolchain rust-toolchain.toml; do
            printf '%s %s\n' "$f" "$(git rev-parse --verify --quiet "$PSHA:$f")"
        done
        printf 'rustc %s\n' "$RUSTC"
    } | git hash-object --stdin
) || fail "cannot hash the cache key"

# The lock of the key: mkdir is atomic. A lock whose owner is gone is removed.
L="$CACHE/lock/$KEY"
said=0
while ! mkdir "$L" 2> /dev/null; do
    owner=$(cat "$L/pid" 2> /dev/null)
    if [ -n "$owner" ] && ! kill -0 "$owner" 2> /dev/null; then
        printf '%s\n' "check-test-edits: removing the lock of exited process $owner" >&2
        rm -rf "$L"
        continue
    fi
    if [ "$said" -eq 0 ]; then
        printf '%s\n' "check-test-edits: waiting for the lock $L (process ${owner:-unknown})" >&2
        said=1
    fi
    sleep 1
done
LOCK=$L
printf '%s\n' "$$" > "$LOCK/pid"

SRC="$CACHE/src/$KEY"
rm -rf "$SRC" && mv "$NEW" "$SRC" || fail "cannot store the sources in $SRC"
NEW=""
TARGET="$CACHE/target/$KEY"
printf '%s\n' "check-test-edits: building the checker of <parent> (key $KEY)" >&2
(cd "$SRC" && CARGO_TARGET_DIR="$TARGET" cargo build -q --release --locked -p zygo-test-edits) ||
    fail "the checker of <parent> does not build; exit 2"
BIN="$TARGET/release/zygo-test-edits"
[ -f "$BIN" ] || BIN="$BIN.exe"
run_helper "$BIN" "$@"

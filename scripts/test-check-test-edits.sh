#!/bin/sh
# Tests for scripts/check-test-edits.sh (node CC-2, K-C7 amended). Each case is a planted commit
# pair in a throw-away repository under mktemp -d. No network, no wait, no time measure.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(dirname "$HERE")
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
trap 'rm -rf "$T"; exit 130' INT
trap 'rm -rf "$T"; exit 143' TERM

export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_NOSYSTEM=1
export GIT_AUTHOR_NAME=test
export GIT_AUTHOR_EMAIL=test@example.invalid
export GIT_COMMITTER_NAME=test
export GIT_COMMITTER_EMAIL=test@example.invalid

cargo build -q --locked --manifest-path "$ROOT/Cargo.toml" -p zygo-test-edits || {
    printf '%s\n' "test-check-test-edits: cargo build failed"
    exit 1
}

FAILED=0
CASE=""
R=""
PARENT=""

# The parent tree: crates/a/src/lib.rs (head, #[cfg(test)] mod tests, tail), crates/a/tests/t.rs,
# a fixture under crates/a/tests/fixtures, and node.md.
mkdir -p "$T/base"
cat > "$T/base/lib_head.rs" <<'EOF'
pub enum Agent {
    ClaudeCode,
}

/// Seven.
///
/// ```
/// assert_eq!(a::prod(), 7);
/// ```
pub fn prod() -> u32 {
    7
}
EOF
cat > "$T/base/tests_mod.rs" <<'EOF'
#[cfg(test)]
mod tests {
    use super::*;

    /// Doc text "d".
    #[test]
    fn adds() {
        assert_eq!(prod(), 7);
        let name = "claude-code";
        let msg = format!("{}x", name);
        assert!(!msg.is_empty());
    }

    #[test]
    fn kind() {
        let k = Agent::ClaudeCode;
        let _ = k;
    }

    #[test]
    fn neg() {
        assert_eq!(1 - 3, -2);
    }
}
EOF
cat > "$T/base/lib_tail.rs" <<'EOF'

#[cfg(all(test, unix))]
fn unix_helper() -> &'static str {
    "u"
}

#[cfg(feature = "test-hooks")]
fn hook() -> &'static str {
    "h"
}
EOF
cat > "$T/base/t.rs" <<'EOF'
#[test]
fn file_test() {
    assert_eq!(1 + 1, 2);
    assert_eq!("t", "t");
}

#[test]
fn other() {}
EOF

# new_repo <case>: $T/<case> with the parent tree committed; sets CASE, R, PARENT; node.md says None.
new_repo() {
    CASE=$1
    R="$T/$1"
    mkdir -p "$R/crates/a/src" "$R/crates/a/tests/fixtures"
    cat "$T/base/lib_head.rs" "$T/base/tests_mod.rs" "$T/base/lib_tail.rs" > "$R/crates/a/src/lib.rs"
    cp "$T/base/t.rs" "$R/crates/a/tests/t.rs"
    printf '{"k": "v"}\n' > "$R/crates/a/tests/fixtures/golden.json"
    write_node "None."
    git -C "$R" -c init.defaultBranch=main init -q
    git -C "$R" add -A
    git -C "$R" commit -q -m parent
    PARENT=$(git -C "$R" rev-parse HEAD)
}

write_node() {
    printf '# Node\n\n## Test logic edits\n\n%s\n\n## Plan\n' "$1" > "$R/node.md"
}

# entry <line>: node.md with <line> under "## Test logic edits", committed (the check reads it
# at HEAD).
entry() {
    write_node "$1"
    git -C "$R" add node.md
    git -C "$R" commit -q -m node
}

# sub <file under the repository> <perl substitution>
sub() {
    perl -0pi -e "$2" "$R/$1"
}

commit_change() {
    git -C "$R" add -A
    git -C "$R" commit -q -m change
}

# new_parent: commits the tree and makes it the parent.
new_parent() {
    commit_change
    PARENT=$(git -C "$R" rev-parse HEAD)
}

# expect <want exit> [<parent> [<node file> [<folder>]]]: runs the check in <folder> (default: the
# repository).
expect() {
    want=$1
    p=${2-$PARENT}
    n=${3-node.md}
    d=${4-$R}
    out=$(cd "$d" && sh "$ROOT/scripts/check-test-edits.sh" "$p" "$n" 2>&1)
    got=$?
    if [ "$got" -eq "$want" ]; then
        printf '%s\n' "ok $CASE"
    else
        printf '%s\n' "FAIL $CASE: exit $got, want $want"
        printf '%s\n' "$out"
        FAILED=1
    fi
}

LIB=crates/a/src/lib.rs
TF=crates/a/tests/t.rs

new_repo string-literal-changed
sub "$LIB" 's/"claude-code"/"opencode"/'
commit_change
expect 1

new_repo number-literal-changed
sub "$TF" 's/1 \+ 1, 2\)/1 + 1, 3)/'
commit_change
expect 1

new_repo neg-sign-flipped
sub "$LIB" 's/1 - 3, -2\)/1 - 3, 2)/'
commit_change
expect 1

new_repo test-fn-removed
sub "$TF" 's/\n#\[test\]\nfn other\(\) \{\}\n//'
commit_change
expect 1

new_repo test-fn-renamed
sub "$TF" 's/fn other\(\)/fn other_renamed()/'
commit_change
expect 1

new_repo ignore-added
sub "$TF" 's/\nfn other\(\)/\n#[ignore]\nfn other()/'
commit_change
expect 1

new_repo cfg-attr-ignore-added
sub "$TF" 's/\nfn other\(\)/\n#[cfg_attr(windows, ignore)]\nfn other()/'
commit_change
expect 1

new_repo cfg-any-on-test-fn
sub "$TF" 's/#\[test\]\nfn other/#[cfg(any())]\n#[test]\nfn other/'
commit_change
expect 1

new_repo cfg-windows-on-test-fn
sub "$LIB" 's/    #\[test\]\n    fn neg/    #[cfg(windows)]\n    #[test]\n    fn neg/'
commit_change
expect 1

new_repo mod-cfg-narrowed
sub "$LIB" 's/#\[cfg\(test\)\]\nmod tests/#[cfg(all(test, windows))]\nmod tests/'
commit_change
expect 1

new_repo file-inner-cfg
sub "$TF" 's/\A/#![cfg(windows)]\n/'
commit_change
expect 1

new_repo should-panic-added
sub "$TF" 's/#\[test\]\nfn other/#[test]\n#[should_panic]\nfn other/'
commit_change
expect 1

new_repo cfg-change-listed
sub "$TF" 's/#\[test\]\nfn other/#[cfg(unix)]\n#[test]\nfn other/'
commit_change
entry '- ignore crates/a/tests/t.rs::other — K§9 test logic edit'
expect 0

new_repo literal-delta-listed
sub "$LIB" 's/"claude-code"/"opencode"/'
commit_change
entry '- literals crates/a/src/lib.rs: +"opencode" x1, -"claude-code" x1 — K§9 test logic edit'
expect 0

new_repo literal-delta-wrong
sub "$LIB" 's/"claude-code"/"opencode"/'
commit_change
entry '- literals crates/a/src/lib.rs: +"opencode" x2, -"claude-code" x1 — K§9 test logic edit'
expect 1

new_repo removal-listed
sub "$TF" 's/\n#\[test\]\nfn other\(\) \{\}\n//'
commit_change
entry '- removed crates/a/tests/t.rs::other — K§9 test logic edit'
expect 0

new_repo rename-listed
sub "$TF" 's/fn other\(\)/fn other_renamed()/'
commit_change
entry '- renamed crates/a/tests/t.rs::other -> other_renamed — K§9 test logic edit'
expect 0

new_repo ignore-listed
sub "$TF" 's/\nfn other\(\)/\n#[ignore]\nfn other()/'
commit_change
entry '- ignore crates/a/tests/t.rs::other — K§9 test logic edit'
expect 0

new_repo entry-indented
sub "$TF" 's/\n#\[test\]\nfn other\(\) \{\}\n//'
commit_change
entry '  - removed crates/a/tests/t.rs::other — K§9 test logic edit'
expect 1

new_repo entry-star-bullet
sub "$TF" 's/\n#\[test\]\nfn other\(\) \{\}\n//'
commit_change
entry '* removed crates/a/tests/t.rs::other — K§9 test logic edit'
expect 1

new_repo type-path-rename
sub "$LIB" 's/let k = Agent::ClaudeCode;/let k = crate::client::Kind::ClaudeCode;/'
commit_change
expect 0

new_repo use-added
sub "$LIB" 's/    use super::\*;\n/    use super::*;\n    use std::collections::BTreeMap;\n/'
commit_change
expect 0

# The tests module moves to crates/a/src/b.rs with one identifier renamed.
move_tests_module() {
    cat "$T/base/lib_head.rs" > "$R/$LIB"
    printf '\nmod b;\n' >> "$R/$LIB"
    cat "$T/base/lib_tail.rs" >> "$R/$LIB"
    cp "$T/base/tests_mod.rs" "$R/crates/a/src/b.rs"
    sub crates/a/src/b.rs 's/\bname\b/client_name/g'
}

new_repo cfg-test-module-moved
move_tests_module
commit_change
expect 0

new_repo cfg-test-module-moved-literal-changed
move_tests_module
sub crates/a/src/b.rs 's/"claude-code"/"opencode"/'
commit_change
expect 1

new_repo literal-in-format-changed
sub "$LIB" 's/"\{\}x"/"{}y"/'
commit_change
expect 1

new_repo cfg-all-test-unix-literal-changed
sub "$LIB" 's/"u"/"v"/'
commit_change
expect 1

new_repo any-test-feature-mod-changed
printf '\n#[cfg(any(test, feature = "x"))]\nmod helpers {\n    pub fn h() -> u8 {\n        1\n    }\n}\n' >> "$R/$LIB"
new_parent
sub "$LIB" 's/u8 \{\n        1/u8 {\n        2/'
commit_change
expect 1

new_repo test-hooks-item-not-a-unit
sub "$LIB" 's/"h"/"i"/'
commit_change
expect 0

new_repo src-file-bare-test
printf '#[test]\nfn bare() {\n    assert_eq!(1, 1);\n}\n' > "$R/crates/a/src/bare.rs"
new_parent
sub crates/a/src/bare.rs 's/1, 1/1, 2/'
commit_change
expect 1

new_repo inner-cfg-test-src-file
printf '#![cfg(test)]\n#[test]\nfn inner() {\n    assert_eq!(1, 1);\n}\n' > "$R/crates/a/src/inner.rs"
new_parent
sub crates/a/src/inner.rs 's/1, 1/1, 2/'
commit_change
expect 1

# An out-of-line #[cfg(test)] mod more; with its body in crates/a/src/more.rs.
add_outline_module() {
    printf '\n#[cfg(test)]\nmod more;\n' >> "$R/$LIB"
    printf '#[test]\nfn m() {\n    assert_eq!(1, 1);\n}\n' > "$R/crates/a/src/more.rs"
    new_parent
}

new_repo outline-mod-at-parent-nochange
add_outline_module
sub "$LIB" 's/    7\n/    3 + 4\n/'
commit_change
expect 0

new_repo outline-mod-body-changed
add_outline_module
sub crates/a/src/more.rs 's/1, 1/1, 2/'
commit_change
expect 1

new_repo new-test-file-not-checked
printf '#[test]\nfn fresh() {\n    assert_eq!("n", "n");\n}\n' > "$R/crates/a/tests/new.rs"
commit_change
expect 0

new_repo test-file-renamed
git -C "$R" mv "$TF" crates/a/tests/t2.rs
commit_change
expect 0

new_repo outside-crates-tests
mkdir -p "$R/tools/x/tests"
printf '#[test]\nfn n() {\n    assert_eq!(1, 1);\n}\n' > "$R/tools/x/tests/n.rs"
new_parent
sub tools/x/tests/n.rs 's/1, 1/1, 2/'
commit_change
expect 1

new_repo doc-comment-edited
sub "$LIB" 's/Doc text "d"\./Doc text "e" 5./'
commit_change
expect 0

new_repo doctest-changed
sub "$LIB" 's/a::prod\(\), 7/a::prod(), 8/'
commit_change
expect 1

new_repo doctest-listed
sub "$LIB" 's/a::prod\(\), 7/a::prod(), 8/'
commit_change
entry '- doctest crates/a/src/lib.rs::prod — K§9 test logic edit'
expect 0

new_repo fixture-changed
printf '{"k": "w"}\n' > "$R/crates/a/tests/fixtures/golden.json"
commit_change
expect 1

new_repo fixture-listed
printf '{"k": "w"}\n' > "$R/crates/a/tests/fixtures/golden.json"
commit_change
entry '- fixture crates/a/tests/fixtures/golden.json — K§9 test logic edit'
expect 0

new_repo checker-changed
mkdir -p "$R/scripts"
printf '#!/bin/sh\n' > "$R/scripts/check-test-edits.sh"
new_parent
printf '#!/bin/sh\nexit 0\n' > "$R/scripts/check-test-edits.sh"
commit_change
expect 1

new_repo checker-listed
mkdir -p "$R/scripts"
printf '#!/bin/sh\n' > "$R/scripts/check-test-edits.sh"
new_parent
printf '#!/bin/sh\nexit 0\n' > "$R/scripts/check-test-edits.sh"
commit_change
entry '- checker scripts/check-test-edits.sh — K-C7 review'
expect 0

new_repo listed-entry-not-found
sub "$LIB" 's/    use super::\*;\n/    use super::*;\n    use std::collections::BTreeMap;\n/'
commit_change
entry '- removed crates/a/tests/t.rs::missing — K§9 test logic edit'
expect 1

new_repo subdir-run
sub "$TF" 's/1 \+ 1, 2\)/1 + 1, 3)/'
commit_change
expect 1 "$PARENT" ../../../node.md "$R/crates/a/src"

new_repo empty-parent
sub "$LIB" 's/"claude-code"/"opencode"/'
commit_change
expect 2 ""

new_repo parent-not-a-commit
sub "$LIB" 's/    7\n/    3 + 4\n/'
commit_change
expect 2 no-such-revision

new_repo parent-is-head
sub "$TF" 's/1 \+ 1, 2\)/1 + 1, 3)/'
commit_change
expect 2 HEAD

new_repo parent-not-ancestor
git -C "$R" checkout -q -b side
sub "$TF" 's/1 \+ 1, 2\)/1 + 1, 3)/'
commit_change
SIDE=$(git -C "$R" rev-parse HEAD)
git -C "$R" checkout -q main
sub "$LIB" 's/    7\n/    3 + 4\n/'
commit_change
expect 2 "$SIDE"

new_repo node-file-missing
sub "$LIB" 's/    7\n/    3 + 4\n/'
commit_change
expect 2 "$PARENT" missing.md

new_repo node-not-committed
sub "$LIB" 's/    7\n/    3 + 4\n/'
commit_change
write_node '- removed crates/a/tests/t.rs::other — K§9 test logic edit'
expect 2

new_repo dirty-tree
sub "$LIB" 's/    7\n/    3 + 4\n/'
commit_change
sub "$TF" 's/1 \+ 1, 2\)/1 + 1, 3)/'
expect 2

# The wrapper maps an exit other than 0 and 1 (here cargo's 101) to 2.
new_repo wrapper-maps-exit
sub "$LIB" 's/    7\n/    3 + 4\n/'
commit_change
mkdir -p "$T/fakebin"
printf '#!/bin/sh\nexit 101\n' > "$T/fakebin/cargo"
chmod +x "$T/fakebin/cargo"
out=$(cd "$R" && PATH="$T/fakebin:$PATH" sh "$ROOT/scripts/check-test-edits.sh" "$PARENT" node.md 2>&1)
got=$?
if [ "$got" -eq 2 ]; then
    printf '%s\n' "ok $CASE"
else
    printf '%s\n' "FAIL $CASE: exit $got, want 2"
    printf '%s\n' "$out"
    FAILED=1
fi

exit "$FAILED"

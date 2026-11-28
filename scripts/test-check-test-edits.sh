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
# The bootstrap (D1): CC-2's own tests run the in-tree helper of this checkout (the throw-away
# repositories have no tools/test-edits at their parent); the cases that test the parent-built
# helper clear this for their run.
export ZYGO_TEST_EDITS_IN_TREE=1
export ZYGO_TEST_EDITS_ROOT="$ROOT"
# Round 3 F2: the suite never writes the real build cache.
export ZYGO_TEST_EDITS_CACHE="$T/cache"

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

# Round 2 cases. mk <case>: an empty repository; wf <file> <line>...: writes the lines; par: commits
# the tree with node.md "None." and makes it the parent.
mk() {
    CASE=$1
    R="$T/$1"
    mkdir -p "$R"
    git -C "$R" -c init.defaultBranch=main init -q
}
wf() {
    f=$1
    shift
    mkdir -p "$(dirname "$R/$f")"
    printf '%s\n' "$@" > "$R/$f"
}
par() {
    write_node "None."
    new_parent
}

# expect_out <want exit> <text>: as expect, and the output must hold <text>.
expect_out() {
    want=$1
    text=$2
    out=$(cd "$R" && sh "$ROOT/scripts/check-test-edits.sh" "$PARENT" node.md 2>&1)
    got=$?
    case $out in
        *"$text"*) has=1 ;;
        *) has=0 ;;
    esac
    if [ "$got" -eq "$want" ] && [ "$has" -eq 1 ]; then
        printf '%s\n' "ok $CASE"
    else
        printf '%s\n' "FAIL $CASE: exit $got, want $want, text '$text' found: $has"
        printf '%s\n' "$out"
        FAILED=1
    fi
}

# D2: a fixture named by include_str! outside tests/, the root tests/ folder, the root fixtures/.
mk include-str-data-changed
wf crates/a/src/lib.rs '#[cfg(test)]' 'mod tests {' '    #[test]' '    fn g() { assert_eq!(include_str!("testdata/g.txt"), "v1"); }' '}'
wf crates/a/src/testdata/g.txt 'v1'
par
wf crates/a/src/testdata/g.txt 'v2'
commit_change
expect 1

mk root-tests-folder
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"' '[[test]]' 'name = "fc"' 'path = "../../tests/fc.rs"'
wf crates/a/src/lib.rs 'pub fn f() {}'
wf tests/fc.rs '#[test]' 'fn t() { assert_eq!("x", "x"); }' '#[test]' 'fn u() {}'
par
wf tests/fc.rs '#[test]' 'fn t() { assert_eq!("y", "y"); }'
commit_change
expect 1

mk root-fixtures-changed
wf crates/a/src/lib.rs 'pub fn f() {}'
wf fixtures/x/capture.json '{"a": 1}'
par
wf fixtures/x/capture.json '{"a": 2}'
commit_change
expect 1

# D3: the gate chain up to the crate root, and the manifest.
mk cfg-on-enclosing-prod-mod
wf crates/a/src/lib.rs 'pub mod a {' '    pub fn f() -> u8 { 1 }' '    #[cfg(test)]' '    mod tests {' '        #[test]' '        fn t() { assert_eq!(super::f(), 1); }' '    }' '}'
par
sub crates/a/src/lib.rs 's/^pub mod a/#[cfg(windows)]\npub mod a/'
commit_change
expect 1

mk inner-cfg-on-prod-file
wf crates/a/src/lib.rs 'pub mod a;'
wf crates/a/src/a.rs 'pub fn f() -> u8 { 1 }' '#[cfg(test)]' 'mod tests {' '    #[test]' '    fn t() { assert_eq!(super::f(), 1); }' '}'
par
sub crates/a/src/a.rs 's/^/#![cfg(windows)]\n/'
commit_change
expect 1

mk cfg-on-test-mod-decl
wf crates/a/tests/suite.rs '#[path = "suite/model.rs"]' 'mod model;'
wf crates/a/tests/suite/model.rs '#[test]' 'fn t() { assert_eq!(1 + 1, 2); }'
par
sub crates/a/tests/suite.rs 's/^/#[cfg(windows)]\n/'
commit_change
expect 1

mk cfg-on-prod-mod-decl
wf crates/a/src/lib.rs 'pub mod a;'
wf crates/a/src/a.rs 'pub fn f() -> u8 { 1 }' '#[cfg(test)]' 'mod tests {' '    #[test]' '    fn t() { assert_eq!(super::f(), 1); }' '}'
par
sub crates/a/src/lib.rs 's/^pub mod a;/#[cfg(windows)]\npub mod a;/'
commit_change
expect 1

mk manifest-autotests-off
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"'
wf crates/a/src/lib.rs 'pub fn f() {}'
wf crates/a/tests/t.rs '#[test]' 'fn t() { assert_eq!(1, 1); }'
par
printf 'autotests = false\n' >> "$R/crates/a/Cargo.toml"
commit_change
expect 1

mk manifest-listed
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"'
wf crates/a/src/lib.rs 'pub fn f() {}'
par
printf '[lib]\ndoctest = false\n' >> "$R/crates/a/Cargo.toml"
commit_change
entry '- manifest crates/a/Cargo.toml — K§9 test logic edit'
expect 0

mk manifest-other-key
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"'
wf crates/a/src/lib.rs 'pub fn f() {}'
par
sub crates/a/Cargo.toml 's/0\.1\.0/0.2.0/'
commit_change
expect 0

# Round 3 F5: include! names a fixture; so does concat!(env!("CARGO_MANIFEST_DIR"), "…").
mk include-bang-data-changed
wf crates/a/src/lib.rs 'pub fn f(x: i32) -> i32 { x }' '#[cfg(test)]' 'mod tests {' '    #[test]' '    fn g() { for (i, o) in include!("cases.in") { assert_eq!(super::f(i), o); } }' '}'
wf crates/a/src/cases.in '[(1, 1), (2, 2)]'
par
wf crates/a/src/cases.in '[(0i32, 0i32); 0]'
commit_change
expect 1

mk include-manifest-dir-changed
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"'
wf crates/a/src/lib.rs 'pub fn f() {}' '#[cfg(test)]' 'mod tests {' '    #[test]' '    fn g() { assert_eq!(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/data/g.txt")), "one"); }' '}'
wf crates/a/src/data/g.txt 'one'
par
wf crates/a/src/data/g.txt 'two'
commit_change
expect 1

mk include-manifest-dir-root-crate
wf Cargo.toml '[package]' 'name = "r"' 'version = "0.1.0"' '[[test]]' 'name = "t"' 'path = "tests/t.rs"'
wf tests/t.rs '#[test]' 'fn g() { assert_eq!(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/", "data/g.txt")), "one"); }'
wf data/g.txt 'one'
par
wf data/g.txt 'two'
commit_change
expect 1

# Round 3 F6: the manifest is read as TOML; dotted keys, the features that required-features
# needs, required-features itself, the test profile and the workspace members count.
mk manifest-dotted-lib-test
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"'
wf crates/a/src/lib.rs 'pub fn f() {}' '#[cfg(test)]' 'mod tests {' '    #[test]' '    fn a() { assert_eq!(1, 1); }' '}'
par
wf crates/a/Cargo.toml 'lib.test = false' '[package]' 'name = "a"' 'version = "0.1.0"'
commit_change
expect 1

mk manifest-features-default
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"' '[features]' 'default = ["slow"]' 'slow = []' '[[test]]' 'name = "t"' 'required-features = ["slow"]'
wf crates/a/src/lib.rs 'pub fn f() {}'
wf crates/a/tests/t.rs '#[test]' 'fn t() { assert_eq!(1, 1); }'
par
sub crates/a/Cargo.toml 's/default = \["slow"\]/default = []/'
commit_change
expect 1

mk manifest-features-no-required
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"' '[features]' 'default = ["slow"]' 'slow = []'
wf crates/a/src/lib.rs 'pub fn f() {}'
par
sub crates/a/Cargo.toml 's/default = \["slow"\]/default = []/'
commit_change
expect 0

mk manifest-bin-required-features
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"' '[features]' 'cli = []' '[[bin]]' 'name = "b"' 'path = "src/main.rs"'
wf crates/a/src/main.rs 'fn main() {}' '#[cfg(test)]' 'mod tests {' '    #[test]' '    fn t() { assert_eq!(1, 1); }' '}'
par
printf 'required-features = ["cli"]\n' >> "$R/crates/a/Cargo.toml"
commit_change
expect 1

mk manifest-profile-test
wf Cargo.toml '[workspace]' 'members = ["crates/a"]' 'resolver = "2"'
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"'
wf crates/a/src/lib.rs 'pub fn f(x: u8) -> u8 { debug_assert!(x < 10); x }'
wf crates/a/tests/t.rs '#[test]' '#[should_panic]' 'fn t() { a::f(11); }'
par
printf '\n[profile.test]\ndebug-assertions = false\n' >> "$R/Cargo.toml"
commit_change
expect 1

mk manifest-workspace-member-dropped
wf Cargo.toml '[workspace]' 'members = ["crates/a", "crates/b"]' 'resolver = "2"'
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"'
wf crates/a/src/lib.rs 'pub fn f() {}'
wf crates/a/tests/t.rs '#[test]' 'fn t() { assert_eq!(1, 1); }'
wf crates/b/Cargo.toml '[package]' 'name = "b"' 'version = "0.1.0"'
wf crates/b/src/lib.rs 'pub fn g() {}'
par
wf Cargo.toml '[workspace]' 'members = ["crates/b"]' 'exclude = ["crates/a"]' 'resolver = "2"'
commit_change
expect 1

mk manifest-workspace-glob-same
wf Cargo.toml '[workspace]' 'members = ["crates/*"]' 'resolver = "2"'
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"'
wf crates/a/src/lib.rs 'pub fn f() {}'
wf crates/b/Cargo.toml '[package]' 'name = "b"' 'version = "0.1.0"'
wf crates/b/src/lib.rs 'pub fn g() {}'
par
wf Cargo.toml '[workspace]' 'members = ["crates/a", "crates/b", "crates/c"]' 'resolver = "2"'
wf crates/c/Cargo.toml '[package]' 'name = "c"' 'version = "0.1.0"'
wf crates/c/src/lib.rs 'pub fn h() {}'
commit_change
expect 0

mk manifest-workspace-default-dropped
wf Cargo.toml '[workspace]' 'members = ["crates/a", "crates/b"]' 'default-members = ["crates/a", "crates/b"]' 'resolver = "2"'
wf crates/a/Cargo.toml '[package]' 'name = "a"' 'version = "0.1.0"'
wf crates/a/src/lib.rs 'pub fn f() {}'
wf crates/b/Cargo.toml '[package]' 'name = "b"' 'version = "0.1.0"'
wf crates/b/src/lib.rs 'pub fn g() {}'
par
sub Cargo.toml 's/default-members = \["crates\/a", "crates\/b"\]/default-members = ["crates\/b"]/'
commit_change
expect 1

# D4: attribute order and layout do not matter.
mk reorder-cfg-should-panic
wf crates/a/src/lib.rs '#[cfg(test)]' 'mod tests {' '    #[test]' '    #[cfg(unix)]' '    #[should_panic]' '    fn a() { panic!(); }' '}'
par
sub crates/a/src/lib.rs 's/    #\[cfg\(unix\)\]\n    #\[should_panic\]/    #[should_panic]\n    #[cfg(unix)]/'
commit_change
expect 0

mk reformat-cfg
wf crates/a/src/lib.rs '#[cfg(all(test,unix))]' 'mod tests {' '    #[test]' '    #[cfg_attr(windows,ignore = "x")]' '    fn a() {}' '}'
par
wf crates/a/src/lib.rs '#[cfg(all(' '    test,' '    unix' '))]' 'mod tests {' '    #[test]' '    #[cfg_attr(' '        windows,' '        ignore = "x"' '    )]' '    fn a() {}' '}'
commit_change
expect 0

# D6: a .rs file that does not parse is a fixture.
mk invalid-rs-fixture-listed
wf crates/a/src/lib.rs 'pub fn f() {}'
par
wf crates/a/tests/fixtures/broken/src/lib.rs 'fn ( {'
commit_change
entry '- fixture crates/a/tests/fixtures/broken/src/lib.rs — a parse-error fixture'
expect 0

mk invalid-rs-fixture-unlisted
wf crates/a/src/lib.rs 'pub fn f() {}'
par
wf crates/a/tests/fixtures/broken/src/lib.rs 'fn ( {'
commit_change
expect 1

# D7: src/bin/*.rs is a crate root; its child modules sit beside it.
mk bin-crate-root
wf crates/a/src/lib.rs 'pub fn f() {}'
wf crates/a/src/bin/tool.rs 'fn main() {}' '#[cfg(test)]' 'mod tests;'
wf crates/a/src/bin/tests.rs '#[test]' 'fn t() { assert_eq!(1, 1); }'
par
wf crates/a/src/lib.rs 'pub fn f() { let _ = 0; }'
commit_change
expect 0

# D8: an out-of-line module inside an out-of-line test module is test code.
mk nested-outline-helper
wf crates/a/src/lib.rs '#[cfg(test)]' 'mod tests;'
wf crates/a/src/tests.rs 'mod util;' '#[test]' 'fn t() { assert_eq!(util::v(), 3); }'
wf crates/a/src/tests/util.rs 'pub fn v() -> u8 { 3 }'
par
sub crates/a/src/tests/util.rs 's/\{ 3 \}/{ 4 }/'
commit_change
expect 1

# D9: an impl is named by its type, so inserting or swapping impls moves no doc test.
mk impl-index-shift
wf crates/a/src/lib.rs 'pub struct A;' 'impl A {' '    /// ```' '    /// assert_eq!(a::A::two(), 2);' '    /// ```' '    pub fn two() -> u8 { 2 }' '}'
par
wf crates/a/src/lib.rs 'pub struct A;' 'pub struct B;' 'impl B {' '    pub fn new() -> Self { B }' '}' 'impl A {' '    /// ```' '    /// assert_eq!(a::A::two(), 2);' '    /// ```' '    pub fn two() -> u8 { 2 }' '}'
commit_change
expect 0

mk impl-swap
wf crates/a/src/lib.rs 'pub struct A; pub struct B;' 'impl A {' '    /// ```' '    /// let _ = 1;' '    /// ```' '    pub fn f() {}' '}' 'impl B {' '    /// ```' '    /// let _ = 2;' '    /// ```' '    pub fn f() {}' '}'
par
wf crates/a/src/lib.rs 'pub struct A; pub struct B;' 'impl B {' '    /// ```' '    /// let _ = 2;' '    /// ```' '    pub fn f() {}' '}' 'impl A {' '    /// ```' '    /// let _ = 1;' '    /// ```' '    pub fn f() {}' '}'
commit_change
expect 0

# D10: two tests of one name under different cfgs are two tests.
mk dup-name-cfg-test-removed
wf crates/a/tests/t.rs '#[cfg(unix)]' '#[test]' 'fn a() { assert!(std::path::Path::new("/").is_absolute()); }' '#[cfg(windows)]' '#[test]' 'fn a() { assert!(std::path::Path::new("/").is_absolute()); }'
par
wf crates/a/tests/t.rs '#[cfg(unix)]' '#[test]' 'fn b() {}' '#[cfg(windows)]' '#[test]' 'fn a() { assert!(std::path::Path::new("/").is_absolute()); }' 'fn c() { let _ = "/"; }'
commit_change
expect 1

# D11: the section's lines.
for c in entry-numbered entry-plain-text-line; do
    new_repo "$c"
    sub "$TF" 's/\n#\[test\]\nfn other\(\) \{\}\n//'
    commit_change
    case $c in
        entry-numbered) entry '1. removed crates/a/tests/t.rs::other — K§9 test logic edit' ;;
        *) entry 'removed crates/a/tests/t.rs::other — K§9 test logic edit' ;;
    esac
    expect 1
done

new_repo entry-h1-after
sub "$TF" 's/\n#\[test\]\nfn other\(\) \{\}\n//'
commit_change
printf '# N\n\n## Test logic edits\n\nNone.\n\n# Appendix\n\n- removed crates/a/tests/t.rs::other — r\n' > "$R/node.md"
commit_change
expect 1

new_repo entry-section-twice
sub "$TF" 's/\n#\[test\]\nfn other\(\) \{\}\n//'
commit_change
printf '# N\n\n## Test logic edits\n\nNone.\n\n## Plan\n\nx\n\n## Test logic edits\n\n- removed crates/a/tests/t.rs::other — r\n' > "$R/node.md"
commit_change
expect 1

new_repo entry-heading-trailing-hash
sub "$TF" 's/\n#\[test\]\nfn other\(\) \{\}\n//'
commit_change
printf '# N\n\n## Test logic edits ##\n\n- removed crates/a/tests/t.rs::other — r\n' > "$R/node.md"
commit_change
expect 0

# D5: the parent used is printed; a parent that is not the merge base with origin/phase-4 warns
# and does not change the exit.
new_repo parent-printed
sub "$LIB" 's/    7\n/    3 + 4\n/'
commit_change
expect_out 0 "check-test-edits: parent $PARENT"

new_repo parent-warns-off-merge-base
sub "$LIB" 's/    7\n/    3 + 4\n/'
commit_change
git -C "$R" update-ref refs/remotes/origin/phase-4 HEAD
sub "$TF" 's/1 \+ 1, 2\)/1 + 1, 3)/'
commit_change
expect_out 1 "warning: <parent> is not git merge-base HEAD origin/phase-4"

# D1: the checker is built from <parent>, so a node's edit to the checker does not judge the
# node. These cases run the wrapper without the bootstrap.
new_repo no-checker-at-parent
sub "$LIB" 's/    7\n/    3 + 4\n/'
commit_change
out=$(cd "$R" && ZYGO_TEST_EDITS_IN_TREE= sh "$ROOT/scripts/check-test-edits.sh" "$PARENT" node.md 2>&1)
got=$?
case $out in
    *"no checker at <parent>"*) has=1 ;;
    *) has=0 ;;
esac
if [ "$got" -eq 2 ] && [ "$has" -eq 1 ]; then
    printf '%s\n' "ok $CASE"
else
    printf '%s\n' "FAIL $CASE: exit $got, want 2 with 'no checker at <parent>'"
    printf '%s\n' "$out"
    FAILED=1
fi

# judge <want exit> <text>: $got must be <want exit> and $out must hold <text>.
judge() {
    case $out in
        *"$2"*) has=1 ;;
        *) has=0 ;;
    esac
    if [ "$got" -eq "$1" ] && [ "$has" -eq 1 ]; then
        printf '%s\n' "ok $CASE"
    else
        printf '%s\n' "FAIL $CASE: exit $got, want $1 with '$2' (found: $has)"
        printf '%s\n' "$out"
        FAILED=1
    fi
}

# Round 3 F3: the bootstrap finds the repository from the current folder (the piped form has no
# script path), and it is refused when <parent> has a checker.
new_repo in-tree-root-from-cwd
sub "$LIB" 's/    7\n/    3 + 4\n/'
commit_change
out=$(cd "$R/crates/a/src" && ZYGO_TEST_EDITS_ROOT= sh -s -- "$PARENT" ../../../node.md < "$ROOT/scripts/check-test-edits.sh" 2>&1)
got=$?
judge 2 "ZYGO_TEST_EDITS_IN_TREE=1: no tools/test-edits in $(cd "$R" && pwd -P)"

# Round 3 F1, F2, F3: the parent-built checker in a copy of this repository (the tree of HEAD,
# with the working copies of the check's files and of Cargo.toml and Cargo.lock), so it builds
# with this repository's Cargo.lock. Commits A (the checker of this tree), B (A's checker turned
# into a pass-all that prints "I AM CHECKER B") and N (a node that edits a test literal) are made
# in one second: equal mtimes once extracted, as in the round 3 stale probe.
CASE=parent-built-setup
R="$T/big"
mkdir -p "$R"
git -C "$ROOT" archive --format=tar HEAD | (cd "$R" && tar -xf -)
rm -rf "$R/tools/test-edits"
cp -R "$ROOT/tools/test-edits" "$R/tools/test-edits"
for f in scripts/check-test-edits.sh scripts/test-check-test-edits.sh Cargo.toml Cargo.lock; do
    cp "$ROOT/$f" "$R/$f"
done
PROBE=crates/zygo-core/tests/core_basics.rs
printf '\n#[test]\nfn k_c7_probe() {\n    assert_eq!(1, 1);\n}\n' >> "$R/$PROBE"
write_node "None."
git -C "$R" -c init.defaultBranch=main init -q
git -C "$R" add -A
git -C "$R" commit -q -m A
A=$(git -C "$R" rev-parse HEAD)
sub tools/test-edits/src/main.rs 's/fn main\(\) -> ExitCode \{/fn main() -> ExitCode {\n    println!("I AM CHECKER B");\n    if std::env::args().count() > 0 {\n        return ExitCode::SUCCESS;\n    }/'
commit_change
B=$(git -C "$R" rev-parse HEAD)
sub "$PROBE" 's/assert_eq!\(1, 1\);\n\}\n\z/assert_eq!(2, 2);\n}\n/'
commit_change
N=$(git -C "$R" rev-parse HEAD)

# The node turns the helper into a pass-all and edits a test literal: the checker of A judges it.
CASE=self-judging-checker
out=$(cd "$R" && ZYGO_TEST_EDITS_IN_TREE= sh scripts/check-test-edits.sh "$A" node.md 2>&1)
got=$?
judge 1 "literals $PROBE"

# F1: after A, parent B runs B's checker (per-key target folder; no stale binary of A).
CASE=parent-key-own-build
out=$(cd "$R" && ZYGO_TEST_EDITS_IN_TREE= sh scripts/check-test-edits.sh "$B" node.md 2>&1)
got=$?
judge 0 "I AM CHECKER B"

# The lock of an exited process is removed.
CASE=stale-lock-removed
sh -c 'exit 0' &
dead=$!
wait "$dead"
for k in "$ZYGO_TEST_EDITS_CACHE/target"/*; do
    mkdir -p "$ZYGO_TEST_EDITS_CACHE/lock/${k##*/}"
    printf '%s\n' "$dead" > "$ZYGO_TEST_EDITS_CACHE/lock/${k##*/}/pid"
done
out=$(cd "$R" && ZYGO_TEST_EDITS_IN_TREE= sh scripts/check-test-edits.sh "$B" node.md 2>&1)
got=$?
judge 0 "removing the lock of exited process $dead"

# The round 3 race probe: with an empty cache, while the run with parent A is in its cargo
# build, a run with parent B builds and runs; the run of A then runs A's checker.
CASE=interleaved-parents
mkdir -p "$T/shim"
REAL_CARGO=$(command -v cargo)
cat > "$T/shim/cargo" <<EOF
#!/bin/sh
"$REAL_CARGO" "\$@"
rc=\$?
if [ ! -e "$T/shim/done" ]; then
    : > "$T/shim/done"
    (cd "$R" && ZYGO_TEST_EDITS_CACHE="$T/cache-race" ZYGO_TEST_EDITS_IN_TREE= sh scripts/check-test-edits.sh "$B" node.md > "$T/shim/b.out" 2>&1; echo "b exit \$?" >> "$T/shim/b.out")
fi
exit \$rc
EOF
chmod +x "$T/shim/cargo"
out=$(cd "$R" && PATH="$T/shim:$PATH" ZYGO_TEST_EDITS_CACHE="$T/cache-race" ZYGO_TEST_EDITS_IN_TREE= sh scripts/check-test-edits.sh "$A" node.md 2>&1)
got=$?
judge 1 "literals $PROBE"
out=$(cat "$T/shim/b.out")
got=0
judge 0 "I AM CHECKER B
b exit 0"

# F3: the bootstrap is refused when <parent> has a checker.
CASE=in-tree-refused
out=$(cd "$R" && sh scripts/check-test-edits.sh "$A" node.md 2>&1)
got=$?
judge 2 "ZYGO_TEST_EDITS_IN_TREE=1 is refused"

# The checker's own files are not units: a `checker` entry lists a change to them, its unit tests
# included. The parent's script runs piped, from the top folder and from a subfolder.
CASE=checker-own-tests-edited
git -C "$R" checkout -q -b own "$A"
sub tools/test-edits/src/main.rs 's/fn includes_are_recorded\(\)/fn includes_are_recorded_renamed()/'
commit_change
entry '- checker tools/test-edits/src/main.rs — K-C7 review of the check'
out=$(git -C "$R" show "$A:scripts/check-test-edits.sh" | (cd "$R" && ZYGO_TEST_EDITS_IN_TREE= sh -s -- "$A" node.md) 2>&1)
got=$?
judge 0 "check-test-edits: parent $A"
CASE=piped-from-subfolder
out=$(git -C "$R" show "$A:scripts/check-test-edits.sh" | (cd "$R/crates/zygo/src" && ZYGO_TEST_EDITS_IN_TREE= sh -s -- "$A" ../../../node.md) 2>&1)
got=$?
judge 0 "check-test-edits: parent $A"
CASE=piped-from-subfolder-offence
git -C "$R" checkout -q main
out=$(git -C "$R" show "$A:scripts/check-test-edits.sh" | (cd "$R/crates/zygo/src" && ZYGO_TEST_EDITS_IN_TREE= sh -s -- "$A" ../../../node.md) 2>&1)
got=$?
judge 1 "literals $PROBE"

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

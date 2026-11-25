#!/bin/sh
# Tests for scripts/check-premises.sh on temporary copies (node PC-1). No agent, no network.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(dirname "$HERE")
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT INT TERM

CASE=""
OUT=""
RC=0
N=0

fail() {
    echo "test-check-premises: FAIL $CASE: $1" >&2
    echo "$OUT" >&2
    exit 1
}

# mk <name>: a copy tree $T/<name> with the script, its helpers, the fixtures and the documents
# it reads, at the same relative paths, and an empty fixtures/codex; prints its path.
mk() {
    d="$T/$1"
    mkdir -p "$d/scripts" "$d/docs/specs"
    cp "$ROOT/scripts/check-premises.sh" "$d/scripts/"
    cp -RP "$ROOT/scripts/premises" "$d/scripts/"
    cp -RP "$ROOT/fixtures" "$d/"
    for f in docs/premises.md docs/live-checks.md docs/conformance.md; do
        [ -e "$ROOT/$f" ] && cp "$ROOT/$f" "$d/$f"
    done
    cp "$ROOT/docs/specs/2026-11-18-zygo-design.md" "$ROOT/docs/specs/2026-11-20-workspaces-design.md" "$d/docs/specs/"
    mkdir -p "$d/fixtures/codex"
    echo "$d"
}

# edit <file> <sed script>: applies the script through a temporary file (BSD and GNU sed -i differ).
edit() {
    sed "$2" "$1" > "$1.new" && mv "$1.new" "$1"
}

# add_header <dir> <line>: inserts the line after the header line that starts `tmux: `.
add_header() {
    awk -v l="$2" '{ print } /^tmux: / { print l }' "$1/docs/premises.md" > "$1/docs/premises.md.new" &&
        mv "$1/docs/premises.md.new" "$1/docs/premises.md"
    [ "$(grep -cxF -- "$2" "$1/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
}

# add_row <dir> <row>: inserts the row after the row that starts `| PW16 |`.
add_row() {
    awk -v l="$2" '{ print } /^\| PW16 \|/ { print l }' "$1/docs/premises.md" > "$1/docs/premises.md.new" &&
        mv "$1/docs/premises.md.new" "$1/docs/premises.md"
    [ "$(grep -cxF -- "$2" "$1/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
}

# row <id> <agent> <version> <result> <path>: a table row in the form of docs/premises.md.
row() {
    printf '| %s | Test row | %s | %s | 2026-11-25 | %s | `%s` |' "$1" "$2" "$3" "$4" "$5"
}

# evidence <dir>: a nontrivial and a trivial evidence file under fixtures/codex.
evidence() {
    echo 'pc1 evidence' > "$1/fixtures/codex/pc1.txt"
    echo '{}' > "$1/fixtures/codex/empty.json"
}

run() {
    OUT=$(sh "$1/scripts/check-premises.sh" 2>&1)
    RC=$?
    N=$((N + 1))
}

expect_ok() {
    [ "$RC" -eq 0 ] || fail "exit $RC, expected 0"
    case "$(printf '%s\n' "$OUT" | tail -n 1)" in
        'check-premises: ok ('*) ;;
        *) fail "last line does not start with 'check-premises: ok ('" ;;
    esac
}

expect_fail() {
    [ "$RC" -eq 1 ] || fail "exit $RC, expected 1"
    [ "$OUT" = "check-premises: $1" ] || fail "output is not 'check-premises: $1'"
}

CASE="real tree"
D=$(mk real)
run "$D"
expect_ok
case "$OUT" in *' PW15-bash PW15-tool PW15-slash '*) ;; *) fail "ok line lacks ' PW15-bash PW15-tool PW15-slash '" ;; esac

CASE="Codex header and PC1 row"
D=$(mk pc1)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC1 Codex 0.172.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_ok
case "$OUT" in *' PC1)'*) ;; *) fail "ok line lacks ' PC1)'" ;; esac

CASE="a failed PC row needs no note"
D=$(mk pc1failed)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC1 Codex 0.172.0 failed fixtures/codex/pc1.txt)"
run "$D"
expect_ok

CASE="Q7 removed"
D=$(mk noq7)
edit "$D/docs/premises.md" '/^| Q7 |/d'
[ "$(grep -c '^| Q7 |' "$D/docs/premises.md")" -eq 0 ] || fail "planted change did not apply"
run "$D"
expect_fail "Q7: expected exactly one row, found 0"

CASE="PW15 removed"
D=$(mk nopw15)
edit "$D/docs/premises.md" '/^| PW15 |/d'
[ "$(grep -c '^| PW15 |' "$D/docs/premises.md")" -eq 0 ] || fail "planted change did not apply"
run "$D"
expect_fail "PW15: expected exactly one row, found 0"

CASE="an agent with no header line"
D=$(mk cursor)
evidence "$D"
add_row "$D" "$(row PC1 Cursor 1.0.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: agent 'Cursor' has no header line 'Cursor: <version>'"

CASE="Codex below its header"
D=$(mk codexlow)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC1 Codex 0.171.9 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: version '0.171.9' is below the header's '0.172.0' for Codex, or not a dotted number"

CASE="Codex above its header"
D=$(mk codexhigh)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC1 Codex 0.173.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_ok

CASE="act-lite differs"
D=$(mk actlite)
edit "$D/docs/premises.md" 's/^\(| P6b | [^|]* | act-lite | \)1\.2\.3 |/\11.2.4 |/'
[ "$(grep -c '^| P6b | .* | act-lite | 1\.2\.4 |' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
run "$D"
expect_fail "P6b: version '1.2.4' differs from the header's '1.2.3' for act-lite"

CASE="tmux with one letter"
D=$(mk tmuxletter)
evidence "$D"
add_row "$D" "$(row PC2 tmux 3.2a verified fixtures/codex/pc1.txt)"
run "$D"
expect_ok

CASE="trivial evidence"
D=$(mk trivial)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC1 Codex 0.172.0 verified fixtures/codex/empty.json)"
run "$D"
expect_fail "PC1: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial"

CASE="PW15-bash gets the row checks"
D=$(mk pw15bash)
edit "$D/docs/premises.md" 's/^\(| PW15-bash | [^|]* | [^|]* | [^|]* | \)2026-11-23 |/\12026-11-3 |/'
[ "$(grep -c '^| PW15-bash | .* | 2026-11-3 |' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
run "$D"
expect_fail "PW15-bash: date '2026-11-3' is not YYYY-MM-DD"

CASE="the Codex schema is verbatim"
D=$(mk schema)
printf '%s\n' '"/Users/alice/x"' > "$D/fixtures/codex/config.schema.json"
run "$D"
expect_ok

CASE="the same text in another file"
D=$(mk other)
printf '%s\n' '"/Users/alice/x"' > "$D/fixtures/codex/other.json"
run "$D"
[ "$RC" -eq 1 ] || fail "exit $RC, expected 1"
case "$OUT" in
    'check-premises: forbidden content (macOS home path): fixtures/codex/other.json:'*) ;;
    *) fail "output does not start with 'check-premises: forbidden content (macOS home path): fixtures/codex/other.json:'" ;;
esac

echo "test-check-premises: ok ($N cases)"

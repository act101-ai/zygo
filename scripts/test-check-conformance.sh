#!/bin/sh
# Tests for scripts/check-conformance.sh on temporary copies of docs/conformance.md (node L2). No network.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(dirname "$HERE")
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT INT TERM

CASE=""
OUT=""
RC=0

fail() {
    echo "test-check-conformance: FAIL $CASE: $1" >&2
    echo "$OUT" >&2
    exit 1
}

# mk <name>: a copy tree $T/<name> with the script and both documents; prints its path.
mk() {
    mkdir -p "$T/$1/scripts" "$T/$1/docs"
    cp "$ROOT/scripts/check-conformance.sh" "$T/$1/scripts/"
    cp "$ROOT/docs/conformance.md" "$ROOT/docs/premises.md" "$T/$1/docs/"
    echo "$T/$1"
}

# before_evidence <file> <text file>: inserts the text before the line `## Evidence`.
before_evidence() {
    awk -v f="$2" '$0 == "## Evidence" { while ((getline l < f) > 0) print l; print ""; } { print }' "$1" > "$1.new" && mv "$1.new" "$1"
}

run() {
    OUT=$(sh "$1/scripts/check-conformance.sh" 2>&1)
    RC=$?
}

expect_fail() {
    [ "$RC" -eq 1 ] || fail "exit $RC, expected 1"
    [ "$OUT" = "check-conformance: $1" ] || fail "output is not 'check-conformance: $1'"
}

CASE="valid file"
D=$(mk valid)
run "$D"
[ "$RC" -eq 0 ] || fail "exit $RC, expected 0"
[ "$OUT" = "check-conformance: ok" ] || fail "output is not 'check-conformance: ok'"

CASE="extra Agent | X table"
D=$(mk extra)
printf '%s\n' '| Agent | X |' '|---|---|' '| Claude Code | full https://example.com/x |' '| OpenCode | full https://example.com/x |' > "$T/extra.txt"
before_evidence "$D/docs/conformance.md" "$T/extra.txt"
run "$D"
expect_fail "unknown table header: | Agent | X |"

CASE="duplicate WA header with bad rows"
D=$(mk dup)
printf '%s\n' '| Agent | WA-1 | WA-2 |' '|---|---|---|' '| Claude Code | maybe | maybe |' '| Cursor | none | none |' > "$T/dup.txt"
before_evidence "$D/docs/conformance.md" "$T/dup.txt"
run "$D"
expect_fail "duplicate table header: | Agent | WA-1 | WA-2 |"

CASE="data row outside a known table"
D=$(mk stray)
printf '%s\n' '| OpenCode | full https://opencode.ai/docs/cli/ | full https://opencode.ai/docs/plugins/ |' > "$T/stray.txt"
before_evidence "$D/docs/conformance.md" "$T/stray.txt"
run "$D"
expect_fail "table row outside a known table: | OpenCode | full https://opencode.ai/docs/cli/ | full https://opencode.ai/docs/plugins/ |"

CASE="known table with no separator row"
D=$(mk nosep)
awk '$0 == "| Agent | WA-1 | WA-2 |" { print; getline; next } { print }' "$ROOT/docs/conformance.md" > "$D/docs/conformance.md"
run "$D"
expect_fail "no separator row under | Agent | WA-1 | WA-2 |"

CASE="OpenCode WA cells name the evidence version"
ROW=$(awk '$0 == "| Agent | WA-1 | WA-2 |" { on = 1; next } on && /^\| OpenCode \|/ { print; exit }' "$ROOT/docs/conformance.md")
OUT=$ROW
[ -n "$ROW" ] || fail "no OpenCode row under the WA table"
N=$(printf '%s\n' "$ROW" | awk -F'|' '{ n = 0; for (i = 3; i <= 4; i++) if (index($i, "OpenCode 1.19.6, PW4; the pin 1.18.16 is a minimum; E2 re-checks")) n++; print n }')
[ "$N" -eq 2 ] || fail "$N of the 2 OpenCode WA cells name the evidence version"
grep -qx 'OpenCode: 1.18.16' "$ROOT/docs/conformance.md" || fail "the pin line 'OpenCode: 1.18.16' changed"

echo "test-check-conformance: ok"

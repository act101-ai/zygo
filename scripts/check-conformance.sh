#!/bin/sh
# Checks docs/conformance.md: one row per v1 agent, 21 capability cells, each marked
# full, partial (what is missing) or none (why), each with a documentation URL (node N39).
# Prints the first failing item and exits 1; exits 0 when all items pass.
set -uf
cd "$(dirname "$0")/.." || exit 1

C=docs/conformance.md
P=docs/premises.md

fail() { echo "check-conformance: $*"; exit 1; }

[ -s "$C" ] || fail "$C is missing or empty"
[ -s "$P" ] || fail "$P is missing or empty"

header() { sed -n "s/^$2: *\(.*[^ ]\) *\$/\1/p" "$1" | head -n 1; }
for A in 'Claude Code' 'OpenCode'; do
  VC=$(header "$C" "$A")
  VP=$(header "$P" "$A")
  [ -n "$VC" ] || fail "header line '$A: <version>' missing in $C"
  [ -n "$VP" ] || fail "header line '$A: <version>' missing in $P"
  [ "$VC" = "$VP" ] || fail "$A version '$VC' differs from $P ('$VP')"
done

HDR='| Agent | A-1 | A-2 | A-3 | A-4 | A-5 | A-6 | A-7 | A-8 | A-9 | A-10 | A-11 | A-12 | A-13 | A-14 | A-15 | A-16 | A-17 | A-18 | A-19 | A-20 | A-21 |'
grep -qxF "$HDR" "$C" || fail "table header row (Agent, A-1 ... A-21 in order) missing"

ROWS=$(grep -E '^\| [^|]+ \|' "$C" | grep -vxF "$HDR" | grep -v '^|[-| ]*|$')
[ "$(printf '%s\n' "$ROWS" | grep -c .)" -eq 2 ] || fail "expected exactly two data rows"
printf '%s\n' "$ROWS" | sed -n 1p | grep -q '^| Claude Code |' || fail "first data row is not 'Claude Code'"
printf '%s\n' "$ROWS" | sed -n 2p | grep -q '^| OpenCode |' || fail "second data row is not 'OpenCode'"

printf '%s\n' "$ROWS" | awk -F'|' '
  function trim(s) { gsub(/^[ \t]+|[ \t]+$/, "", s); return s }
  {
    agent = trim($2)
    if (NF != 24) { printf "check-conformance: %s row has %d fields, expected 22\n", agent, NF - 2; exit 1 }
    for (i = 3; i <= 23; i++) {
      c = trim($i)
      if (c !~ /^(full|partial \([^|)]+\)|none \([^|)]+\)) https:\/\/[^ ]+$/) {
        printf "check-conformance: %s A-%d cell is not <full|partial (..)|none (..)> <https-url>: %s\n", agent, i - 2, c
        exit 1
      }
    }
  }' || exit 1

echo "check-conformance: ok"

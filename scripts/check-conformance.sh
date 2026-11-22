#!/bin/sh
# Checks docs/conformance.md: one row per v1 agent in each table, the 21 capability cells A-1..A-21 and the
# 2 workspace cells WA-1, WA-2, each marked full, partial (what is missing) or none (why), each with a
# documentation URL (node N39; workspace cells: node L2).
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
WHDR='| Agent | WA-1 | WA-2 |'

# The data rows of the table whose header row is $1: the table rows right after it, without the separator row.
table_rows() {
  awk -v h="$1" '$0 == h { on = 1; next } on && /^\|/ { print; next } on { exit }' "$C" | grep -v '^|[-| ]*|$'
}

# check_table <header row> <cell count N> <id prefix ID>: two data rows (Claude Code, then OpenCode), each with
# cells ${ID}1 .. ${ID}N in the cell form.
check_table() {
  TH=$1 N=$2 ID=$3
  grep -qxF "$TH" "$C" || fail "table header row (Agent, ${ID}1 ... ${ID}${N} in order) missing"
  ROWS=$(table_rows "$TH")
  [ "$(printf '%s\n' "$ROWS" | grep -c .)" -eq 2 ] || fail "expected exactly two data rows under the ${ID}1 table"
  printf '%s\n' "$ROWS" | sed -n 1p | grep -q '^| Claude Code |' || fail "first data row under the ${ID}1 table is not 'Claude Code'"
  printf '%s\n' "$ROWS" | sed -n 2p | grep -q '^| OpenCode |' || fail "second data row under the ${ID}1 table is not 'OpenCode'"
  printf '%s\n' "$ROWS" | awk -F'|' -v n="$N" -v id="$ID" '
    function trim(s) { gsub(/^[ \t]+|[ \t]+$/, "", s); return s }
    {
      agent = trim($2)
      if (NF != n + 3) { printf "check-conformance: %s row has %d fields, expected %d\n", agent, NF - 2, n + 1; exit 1 }
      for (i = 3; i <= n + 2; i++) {
        c = trim($i)
        if (c !~ /^(full|partial \([^|)]+\)|none \([^|)]+\)) https:\/\/[^ ]+$/) {
          printf "check-conformance: %s %s%d cell is not <full|partial (..)|none (..)> <https-url>: %s\n", agent, id, i - 2, c
          exit 1
        }
      }
    }' || exit 1
}

check_table "$HDR" 21 A-
check_table "$WHDR" 2 WA-

echo "check-conformance: ok"

#!/bin/sh
# Tests for scripts/check-premises.sh on temporary copies (node PC-1). No agent, no network.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(dirname "$HERE")
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
trap 'rm -rf "$T"; exit 130' INT
trap 'rm -rf "$T"; exit 143' TERM

CASE=""
OUT=""
RC=0
N=0

fail() {
    printf '%s\n' "test-check-premises: FAIL $CASE: $1" >&2
    printf '%s\n' "$OUT" >&2
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

# add_header <dir> <line>: inserts the line after the header line that starts `tmux: `. The line goes
# through the environment (not awk -v), so its backslashes stay as written.
add_header() {
    L="$2" awk '{ print } /^tmux: / { print ENVIRON["L"] }' "$1/docs/premises.md" > "$1/docs/premises.md.new" &&
        mv "$1/docs/premises.md.new" "$1/docs/premises.md"
    [ "$(grep -cxF -- "$2" "$1/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
}

# add_lines <dir> <line>...: inserts the lines, in order, after the header line that starts `tmux: `.
add_lines() {
    d=$1
    shift
    L=$(printf '%s\n' "$@") awk '{ print } /^tmux: / { print ENVIRON["L"] }' "$d/docs/premises.md" > "$d/docs/premises.md.new" &&
        mv "$d/docs/premises.md.new" "$d/docs/premises.md"
}

# add_row <dir> <row>: inserts the row after the row that starts `| PW16 |`. The row goes through
# the environment (not awk -v), so its backslashes stay as written.
add_row() {
    L="$2" awk '{ print } /^\| PW16 \|/ { print ENVIRON["L"] }' "$1/docs/premises.md" > "$1/docs/premises.md.new" &&
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

# run <dir>: runs the copy's script; N2_FORBIDDEN_NAMES is cleared, so no case inherits it from the caller.
run() {
    OUT=$(unset N2_FORBIDDEN_NAMES; sh "$1/scripts/check-premises.sh" 2>&1)
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
expect_fail "PC1: agent 'Cursor' has no header line 'Cursor: <version>' before the first '## ' of docs/premises.md"

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

# lnof <dir> <line>: the line number of the exact line in <dir>/docs/premises.md.
lnof() {
    grep -nxF -- "$2" "$1/docs/premises.md" | cut -d: -f1
}

ID_FORM="is empty or not of the form [A-Za-z0-9][A-Za-z0-9-]*"

CASE="a row with an empty Id"
D=$(mk emptyid)
evidence "$D"
R="$(row '' tmux 3.2 verified fixtures/codex/pc1.txt)"
add_row "$D" "$R"
run "$D"
expect_fail "docs/premises.md:$(lnof "$D" "$R"): row id '' $ID_FORM"

CASE="a row with no blank after the first bar"
D=$(mk nobar)
evidence "$D"
add_row "$D" '|PC9 | Test row | tmux | 3.2 | 2026-11-25 | verified | `fixtures/codex/empty.json` |'
run "$D"
expect_fail "PC9: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial"

CASE="a row with a blank before the first bar"
D=$(mk lead)
evidence "$D"
add_row "$D" ' | PC9 | Test row | tmux | 3.2 | 2026-11-25 | verified | `fixtures/codex/empty.json` |'
run "$D"
expect_fail "PC9: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial"

CASE="an Id with an inner space"
D=$(mk innerspace)
evidence "$D"
R="$(row 'P 1' tmux 3.2 verified fixtures/codex/pc1.txt)"
add_row "$D" "$R"
run "$D"
expect_fail "docs/premises.md:$(lnof "$D" "$R"): row id 'P 1' $ID_FORM"

CASE="a failed P row with no base-spec note (P1 has one; P2 has none)"
D=$(mk p1failed)
edit "$D/docs/premises.md" 's/^\(| P2 | [^|]* | [^|]* | [^|]* | [^|]* | \)verified |/\1failed |/'
[ "$(grep -c '^| P2 | .* | failed |' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
run "$D"
expect_fail "P2: failed, but spec §19 has no dated note *(Note YYYY-MM-DD, N2, P2: …)*"

CASE="a W§17 result that differs"
D=$(mk pw11)
edit "$D/docs/premises.md" 's/^\(| PW11 | [^|]* | [^|]* | [^|]* | [^|]* | \)verified |/\1failed |/'
[ "$(grep -c '^| PW11 | .* | failed |' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
run "$D"
expect_fail "PW11: the W§17 row has no 'Result 2026-11-22: failed'"

CASE="a duplicate id"
D=$(mk dup)
edit "$D/docs/premises.md" '/^| PW15-bash |/p'
[ "$(grep -c '^| PW15-bash |' "$D/docs/premises.md")" -eq 2 ] || fail "planted change did not apply"
run "$D"
expect_fail "PW15-bash: expected exactly one row, found 2"

CASE="a header line after the first ##"
D=$(mk latehdr)
evidence "$D"
add_row "$D" "$(row PC1 Codex 0.172.0 verified fixtures/codex/pc1.txt)"
printf '\n%s\n' 'Codex: 0.172.0' >> "$D/docs/premises.md"
run "$D"
expect_fail "PC1: agent 'Codex' has no header line 'Codex: <version>' before the first '## ' of docs/premises.md"

CASE="a header value with extra words"
D=$(mk hdrwords)
edit "$D/docs/premises.md" 's/^Claude Code: 2\.1\.328$/Claude Code: 2.1.328 or later/'
[ "$(grep -cx 'Claude Code: 2\.1\.328 or later' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
run "$D"
expect_fail "P1: header 'Claude Code: 2.1.328 or later': header version must be a bare dotted version"

CASE="another table after the Results table"
D=$(mk table2)
printf '\n%s\n' '| Name | Value |' '|---|---|' '| foo | bar |' >> "$D/docs/premises.md"
run "$D"
expect_ok

CASE="CRLF line endings"
D=$(mk crlf)
awk '{ printf "%s\r\n", $0 }' "$D/docs/premises.md" > "$D/docs/premises.md.new" && mv "$D/docs/premises.md.new" "$D/docs/premises.md"
[ "$(grep -c "$(printf '\r')\$" "$D/docs/premises.md")" -gt 0 ] || fail "planted change did not apply"
run "$D"
expect_fail "docs/premises.md has CRLF line endings"

CASE="a duplicate of a required id in a second table"
D=$(mk dupq7)
printf '\n%s\n' '| Name | Value |' '|---|---|' '| Q7 | x |' >> "$D/docs/premises.md"
[ "$(grep -c '^| Q7 |' "$D/docs/premises.md")" -eq 2 ] || fail "planted change did not apply"
run "$D"
expect_fail "Q7: expected exactly one row, found 2"

CASE="a results-like row in a later table"
D=$(mk late)
printf '\n%s\n' '| Name | Value |' '|---|---|' '| PC5 | x |' >> "$D/docs/premises.md"
run "$D"
expect_fail "docs/premises.md:$(lnof "$D" '| PC5 | x |'): row 'PC5' is outside the Results table"

CASE="an empty Agent cell"
D=$(mk noagent)
evidence "$D"
add_row "$D" "$(row PC1 '' 3.2 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: the Agent cell is empty"

CASE="an empty Version cell"
D=$(mk nover)
evidence "$D"
add_row "$D" "$(row PC1 tmux '' verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: the Version cell is empty"

CASE="empty Agent and Version cells"
D=$(mk noagentver)
evidence "$D"
add_row "$D" "$(row PC1 '' '' verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: the Agent cell is empty"

CASE="a result that is not verified or failed"
D=$(mk badresult)
evidence "$D"
add_row "$D" "$(row PC1 tmux 3.2 passed fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: result 'passed' is not verified or failed"

# hdr_twice <dir> <first> <second>: inserts both header lines after the `tmux: ` line, in that order.
hdr_twice() {
    awk -v a="$2" -v b="$3" '{ print } /^tmux: / { print a; print b }' "$1/docs/premises.md" > "$1/docs/premises.md.new" &&
        mv "$1/docs/premises.md.new" "$1/docs/premises.md"
    [ "$(grep -c '^Codex: ' "$1/docs/premises.md")" -eq 2 ] || fail "planted change did not apply"
}

CASE="the same header line twice"
D=$(mk hdrsame)
evidence "$D"
hdr_twice "$D" 'Codex: 0.172.0' 'Codex: 0.172.0'
add_row "$D" "$(row PC1 Codex 0.172.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_ok

CASE="a second header line with another version"
D=$(mk hdrconflict)
evidence "$D"
hdr_twice "$D" 'Codex: 0.172.0' 'Codex: 0.100.0'
add_row "$D" "$(row PC1 Codex 0.172.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "docs/premises.md:$(grep -nx 'Codex: 0\.100\.0' "$D/docs/premises.md" | cut -d: -f1): header 'Codex: 0.100.0' differs from the first header line 'Codex: 0.172.0'"

CASE="extra blanks at the ends of cells"
D=$(mk blanks)
evidence "$D"
add_row "$D" '| PC1   | Test row |   tmux   |   3.2   |   2026-11-25   |   verified   | `fixtures/codex/pc1.txt` |'
run "$D"
expect_ok

CASE="a header value with blanks at its end"
D=$(mk hdrblanks)
evidence "$D"
add_header "$D" 'Codex: 0.172.0   '
add_row "$D" "$(row PC1 Codex 0.172.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_ok

NOHDR="has no header line 'Cursor: <version>' before the first '## ' of docs/premises.md"

CASE="a date cell with a backslash sequence"
D=$(mk datebs)
evidence "$D"
add_row "$D" '| PC1 | Test row | tmux | 3.2 | 2026-11-2\0065 | verified | `fixtures/codex/pc1.txt` |'
run "$D"
expect_fail "PC1: date '2026-11-2\0065' is not YYYY-MM-DD"

CASE="a result cell with a backslash sequence"
D=$(mk resultbs)
evidence "$D"
add_row "$D" '| PC1 | Test row | tmux | 3.2 | 2026-11-25 | verifie\0144 | `fixtures/codex/pc1.txt` |'
run "$D"
expect_fail "PC1: result 'verifie\0144' is not verified or failed"

CASE="a version cell with a backslash sequence"
D=$(mk verbs)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC1 Codex '0.172.\060' verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: version '0.172.\060' is below the header's '0.172.0' for Codex, or not a dotted number"

CASE="an Item cell with a backslash n"
D=$(mk itembn)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" '| PC1 | Codex writes `a\nb` | Codex | 0.172.0 | 2026-11-25 | verified | `fixtures/codex/pc1.txt` |'
run "$D"
expect_ok

CASE="an Item cell with a backslash c"
D=$(mk itembc)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" '| PC1 | Codex reads `\c` | Codex | 0.172.0 | 2026-11-25 | verified | `fixtures/codex/pc1.txt` |'
run "$D"
expect_ok

CASE="a header value with a tab"
D=$(mk hdrtab)
evidence "$D"
H=$(printf 'Codex: 0.172.0\tjunk')
add_header "$D" "$H"
add_row "$D" "$(row PC1 Codex 0.172.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "docs/premises.md:$(lnof "$D" "$H"): the value of a header line holds a tab"

CASE="a header line in a backtick fence"
D=$(mk hdrfence)
evidence "$D"
add_lines "$D" '```text' 'Cursor: 1.0' '```'
[ "$(grep -cx 'Cursor: 1\.0' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
add_row "$D" "$(row PC1 Cursor 1.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: agent 'Cursor' $NOHDR"

CASE="a header line in a tilde fence"
D=$(mk hdrtilde)
evidence "$D"
add_lines "$D" '~~~' 'Cursor: 1.0' '~~~'
[ "$(grep -cx 'Cursor: 1\.0' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
add_row "$D" "$(row PC1 Cursor 1.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: agent 'Cursor' $NOHDR"

CASE="a header line in an HTML comment"
D=$(mk hdrcomment)
evidence "$D"
add_lines "$D" '<!--' 'Cursor: 1.0' '-->'
[ "$(grep -cx 'Cursor: 1\.0' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
add_row "$D" "$(row PC1 Cursor 1.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: agent 'Cursor' $NOHDR"

CASE="no ## line"
D=$(mk noh2)
edit "$D/docs/premises.md" 's/^## /### /'
[ "$(grep -c '^## ' "$D/docs/premises.md")" -eq 0 ] || fail "planted change did not apply"
run "$D"
expect_fail "docs/premises.md has no '## ' line (the header lines end at the first one)"

CASE="a second Results table"
D=$(mk results2)
printf '\n%s\n' '| Id | Item | Agent |' '|---|---|---|' '| foo1 | x | nobody |' >> "$D/docs/premises.md"
run "$D"
expect_fail "docs/premises.md:$(lnof "$D" '| Id | Item | Agent |'): a second Results table header row '| Id | Item |'"

CASE="an escaped pipe in a cell"
D=$(mk escpipe)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" '| PC1 | Codex pipes a \| b | Codex | 0.172.0 | 2026-11-25 | verified | `fixtures/codex/pc1.txt` |'
run "$D"
expect_ok

CASE="a tab at the end of a cell"
D=$(mk celltab)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(printf '| PC1 | Test row | Codex\t| 0.172.0 | 2026-11-25 | verified | `fixtures/codex/pc1.txt` |')"
run "$D"
expect_ok

CASE="an evidence path with a .. component"
D=$(mk dotdot)
add_row "$D" "$(row PC1 tmux 3.2 verified docs/../docs/premises.md)"
run "$D"
expect_fail "PC1: evidence path docs/../docs/premises.md is absolute or holds a '..' component"

CASE="a joint agent with no header for the second"
D=$(mk joint)
evidence "$D"
add_row "$D" "$(row PC1 'Claude Code + Cursor' '2.1.328 + 1.0' verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: agent 'Cursor' $NOHDR"

echo "test-check-premises: ok ($N cases)"

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
# it reads, at the same relative paths, and fixtures/codex (made when the tree has none); prints its path.
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
# through the environment (not awk -v), so its backslashes stay as written. PC-13: docs/premises.md has
# a real `Codex:` header since PC-12, so the planted line first replaces the copy's header line of the
# same name (the text up to the first `:`, then `: `) before the first `## `.
add_header() {
    L="$2" N="${2%%:*}: " awk 'index($0, ENVIRON["N"]) == 1 && !h { next } /^## / { h = 1 } { print } /^tmux: / { print ENVIRON["L"] }' "$1/docs/premises.md" > "$1/docs/premises.md.new" &&
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

# add_row <dir> <row>: inserts the row after the last row of the Results table (from the header row
# "| Id | Item |" to the first line that is not a table line). The row goes through the environment
# (not awk -v), so its backslashes stay as written.
add_row() {
    L="$2" awk 'NR == FNR { if ($0 ~ /^\| Id \| Item \|/) t = 1; else if (t && $0 ~ /^ ? ? ?\|/) last = FNR; else t = 0; next }
        { print } FNR == last { print ENVIRON["L"] }' "$1/docs/premises.md" "$1/docs/premises.md" > "$1/docs/premises.md.new" &&
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

# sha <file>: its sha256 in lower-case hex.
sha() {
    { if command -v shasum >/dev/null 2>&1; then shasum -a 256; else sha256sum; fi; } <"$1" | awk '{ print tolower($1) }'
}

# SPIN, CPIN: the sha256 values check-premises.sh pins for the settings schema and the Codex
# config.schema.json (CPIN is the value PC-2 pins in scripts/premises/codex/base.sh).
SPIN=f26f028ad94a1e2a28e9dad1f80832ab196a8e080bd3a7d7a32e6428a15e0611
CPIN=52fb96b1ff79a51bf9ce31695a4a22d4e80f57591d91bad5747b657f4e612bac

# pin_codex <dir> <file>: a case plants its own Codex schema in place of the real one, so in the
# copy's script the pinned Codex hash becomes the file's sha256, and the copy's Codex source file
# gives the same hash.
pin_codex() {
    h=$(sha "$2")
    edit "$1/scripts/check-premises.sh" "s/$CPIN/$h/"
    grep -q "$h" "$1/scripts/check-premises.sh" || fail "planted pin did not apply"
    printf 'source: test\nsha256: %s\n' "$h" > "$1/fixtures/codex/config.schema.source.txt"
}

# run <dir>: runs the copy's script; N2_FORBIDDEN_NAMES is cleared, so no case inherits it from the caller.
run() {
    OUT=$(unset N2_FORBIDDEN_NAMES; sh "$1/scripts/check-premises.sh" 2>&1)
    RC=$?
    N=$((N + 1))
}

# run_names <dir> <names>: as run, with N2_FORBIDDEN_NAMES set to the names for the script.
run_names() {
    OUT=$(N2_FORBIDDEN_NAMES="$2" sh "$1/scripts/check-premises.sh" 2>&1)
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

# expect_fail_prefix <text>: exit 1 and one output line that starts "check-premises: <text>" (the
# rest of the line may hold bytes that differ between shells, such as a NUL from a binary file).
expect_fail_prefix() {
    [ "$RC" -eq 1 ] || fail "exit $RC, expected 1"
    [ "$(printf '%s\n' "$OUT" | wc -l | tr -d ' ')" -eq 1 ] || fail "output is not one line"
    case "$OUT" in "check-premises: $1"*) ;; *) fail "output does not start with 'check-premises: $1'" ;; esac
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
pin_codex "$D" "$D/fixtures/codex/config.schema.json"
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
add_row "$D" "$(row PC1 Cursor 1.0 verified fixtures/codex/pc1.txt)"
printf '\n%s\n' 'Cursor: 1.0' >> "$D/docs/premises.md"
run "$D"
expect_fail "PC1: agent 'Cursor' has no header line 'Cursor: <version>' before the first '## ' of docs/premises.md"

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
# PC-13: it first drops the copy's real `Codex: ` header lines before the first `## ` (since PC-12).
hdr_twice() {
    awk -v a="$2" -v b="$3" '/^Codex: / && !h { next } /^## / { h = 1 } { print } /^tmux: / { print a; print b }' "$1/docs/premises.md" > "$1/docs/premises.md.new" &&
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

# PC-9 round 2 (F1): the last component is a regular file, but a directory on the path is a symlink
# to a folder outside the tree; scripts/premises/ext is outside the forbidden-content scan scope.
CASE="an evidence path through a symlinked directory"
D=$(mk symdir)
mkdir -p "$T/symdir_out"
echo 'outside evidence' > "$T/symdir_out/ev.sh"
ln -s "$T/symdir_out" "$D/scripts/premises/ext"
add_row "$D" "$(row PC1 tmux 3.2 verified scripts/premises/ext/ev.sh)"
run "$D"
INREPO='is not a plain path inside the repository (a symlink, `.`/`//` part, or unreadable directory)'
expect_fail "PC1: evidence path scripts/premises/ext/ev.sh $INREPO"

# PC-9 round 3 (G4): a '.' or '//' part in the directory is refused too, with the same message.
CASE="an evidence path with a . part"
D=$(mk dotpart)
evidence "$D"
add_row "$D" "$(row PC1 tmux 3.2 verified fixtures/./codex/pc1.txt)"
run "$D"
expect_fail "PC1: evidence path fixtures/./codex/pc1.txt $INREPO"

CASE="an evidence path with a // part"
D=$(mk slashpart)
evidence "$D"
add_row "$D" "$(row PC1 tmux 3.2 verified fixtures//codex/pc1.txt)"
run "$D"
expect_fail "PC1: evidence path fixtures//codex/pc1.txt $INREPO"

CASE="a joint agent with no header for the second"
D=$(mk joint)
evidence "$D"
add_row "$D" "$(row PC1 'Claude Code + Cursor' '2.1.328 + 1.0' verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: agent 'Cursor' $NOHDR"

CASE="a forbidden name only in the verbatim schema"
D=$(mk names_verbatim)
grep -qiw alice "$D/fixtures/claude-code/settings-schema.json" || fail "the copied schema does not hold alice"
run_names "$D" alice
expect_ok

CASE="a forbidden name in another fixture"
D=$(mk names_other)
printf '%s\n' 'owner alice' > "$D/fixtures/codex/other.txt"
run_names "$D" alice
expect_fail "forbidden name from N2_FORBIDDEN_NAMES: fixtures/codex/other.txt:1"

CASE="a row indented four blanks"
D=$(mk indent4)
evidence "$D"
add_row "$D" '    | PC9 | Test row | tmux | 3.2 | 2026-11-25 | verified | `fixtures/codex/empty.json` |'
run "$D"
expect_ok
case "$OUT" in *' PC9'*) fail "ok line names PC9" ;; esac

CASE="a row indented with a tab"
D=$(mk indenttab)
evidence "$D"
add_row "$D" "$(printf '\t| PC9 | Test row | tmux | 3.2 | 2026-11-25 | verified | `fixtures/codex/empty.json` |')"
run "$D"
expect_ok
case "$OUT" in *' PC9'*) fail "ok line names PC9" ;; esac

CASE="a row indented three blanks"
D=$(mk indent3)
evidence "$D"
add_row "$D" '   | PC9 | Test row | tmux | 3.2 | 2026-11-25 | verified | `fixtures/codex/empty.json` |'
run "$D"
expect_fail "PC9: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial"

CASE="a header line indented four blanks"
D=$(mk hdrindent4)
evidence "$D"
add_header "$D" '    Cursor: 1.0'
add_row "$D" "$(row PC1 Cursor 1.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC1: agent 'Cursor' $NOHDR"

# Review round 1 (PC-8).
CASE="an indented line in the middle of the Results table"
D=$(mk indentmid)
L='    | PC9 | indented |' awk '{ print } /^\| Q7 \|/ { print ENVIRON["L"] }' "$D/docs/premises.md" > "$D/docs/premises.md.new" &&
    mv "$D/docs/premises.md.new" "$D/docs/premises.md"
[ "$(grep -c '^    | PC9 |' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
run "$D"
expect_fail "docs/premises.md:$(lnof "$D" "$(grep '^| PW6 |' "$D/docs/premises.md")"): row 'PW6' is outside the Results table"

SCH=fixtures/claude-code/settings-schema.json
SRC=fixtures/claude-code/settings-schema.source.txt
CSCH=fixtures/codex/config.schema.json
CSRC=fixtures/codex/config.schema.source.txt

CASE="a name appended to the verbatim schema"
D=$(mk verb_append)
printf '%s\n' 'owner alice' >> "$D/$SCH"
run_names "$D" alice
expect_fail "verbatim file $SCH (sha256 $(sha "$D/$SCH")) and its source file $SRC (sha256 $SPIN) must both give the pinned sha256 $SPIN"

CASE="a verbatim schema with no source file"
D=$(mk verb_nosrc)
rm "$D/$SRC"
run "$D"
expect_fail "verbatim file $SCH has no source file $SRC"

CASE="the Codex verbatim schema with a matching source hash"
D=$(mk verb_codex)
printf '%s\n' '{ "description": "owner alice" }' > "$D/$CSCH"
pin_codex "$D" "$D/$CSCH"
run_names "$D" alice
expect_ok

CASE="the Codex verbatim schema with a mismatching source hash"
D=$(mk verb_codex_bad)
printf '%s\n' '{ "description": "owner alice" }' > "$D/$CSCH"
pin_codex "$D" "$D/$CSCH"
OTHER=$(sha "$D/$SRC")
printf 'source: test\nsha256: %s\n' "$OTHER" > "$D/$CSRC"
run_names "$D" alice
expect_fail "verbatim file $CSCH (sha256 $(sha "$D/$CSCH")) and its source file $CSRC (sha256 $OTHER) must both give the pinned sha256 $(sha "$D/$CSCH")"

CASE="a path that only starts with a verbatim path"
D=$(mk verb_prefix)
printf '%s\n' 'owner alice' > "$D/$SCH:x"
run_names "$D" alice
expect_fail "a path in the scan scope holds ':': $SCH:x"

CASE="a symlink under fixtures"
D=$(mk link_fixture)
ln -s ../opencode/help.txt "$D/fixtures/codex/link.txt"
run "$D"
expect_fail "fixtures/codex/link.txt is a symlink; a symlink in the scan scope is not scanned and is not evidence"

CASE="a symlink under the live scripts"
D=$(mk link_live)
ln -s ../../../fixtures/opencode/help.txt "$D/scripts/premises/live/link.txt"
run "$D"
expect_fail "scripts/premises/live/link.txt is a symlink; a symlink in the scan scope is not scanned and is not evidence"

LIVE=scripts/premises/live-claude.sh
CASE="a forbidden name in a live script"
D=$(mk live_name)
printf '%s\n' '# owner alice' >> "$D/$LIVE"
run_names "$D" alice
expect_fail "forbidden name from N2_FORBIDDEN_NAMES: $LIVE:$(wc -l <"$D/$LIVE" | tr -d ' ')"

CASE="an API key in a live script"
D=$(mk live_key)
printf '%s\n' '# key sk-ant-test' >> "$D/$LIVE"
run "$D"
expect_fail "forbidden content (API key): $LIVE:$(wc -l <"$D/$LIVE" | tr -d ' '):# key sk-ant-test"

CASE="an address at example.invalid"
D=$(mk mail_invalid)
printf '%s\n' 'user.email=x@example.invalid, <y@example.invalid>' 'z@example.invalid' > "$D/fixtures/codex/mail.txt"
run "$D"
expect_ok

CASE="an address at example.invalidz.com"
D=$(mk mail_invalidz)
printf '%s\n' 'x@example.invalidz.com' > "$D/fixtures/codex/mail.txt"
run "$D"
expect_fail "forbidden content (e-mail address): fixtures/codex/mail.txt:1:x@example.invalidz.com"

CASE="an address at sub.example.invalid.evil.com"
D=$(mk mail_evil)
printf '%s\n' 'x@sub.example.invalid.evil.com' > "$D/fixtures/codex/mail.txt"
run "$D"
expect_fail "forbidden content (e-mail address): fixtures/codex/mail.txt:1:x@sub.example.invalid.evil.com"

CASE="a real address beside one at example.invalid"
D=$(mk mail_mixed)
printf '%s\n' 'x@example.invalid y@example.com' > "$D/fixtures/codex/mail.txt"
run "$D"
expect_fail "forbidden content (e-mail address): fixtures/codex/mail.txt:1:x@example.invalid y@example.com"

# Review round 2 (PC-8).
CASE="G1: the settings schema and its source edited consistently"
D=$(mk g1_both)
printf '%s\n' 'owner alice /Users/alice/x' >> "$D/$SCH"
NEW=$(sha "$D/$SCH")
edit "$D/$SRC" "s/^sha256: .*/sha256: $NEW/"
grep -qx "sha256: $NEW" "$D/$SRC" || fail "planted change did not apply"
run_names "$D" alice
expect_fail "verbatim file $SCH (sha256 $NEW) and its source file $SRC (sha256 $NEW) must both give the pinned sha256 $SPIN"

CASE="G1: the Codex schema and its source edited consistently"
D=$(mk g1_codex_both)
printf '%s\n' '{ "description": "owner alice" }' > "$D/$CSCH"
NEW=$(sha "$D/$CSCH")
printf 'source: test\nsha256: %s\n' "$NEW" > "$D/$CSRC"
run_names "$D" alice
expect_fail "verbatim file $CSCH (sha256 $NEW) and its source file $CSRC (sha256 $NEW) must both give the pinned sha256 $CPIN"

CASE="G1: the Codex source gives the pin, the file differs"
D=$(mk g1_codex_src)
printf '%s\n' '{ "description": "x" }' > "$D/$CSCH"
printf 'source: test\nsha256: %s\n' "$CPIN" > "$D/$CSRC"
run "$D"
expect_fail "verbatim file $CSCH (sha256 $(sha "$D/$CSCH")) and its source file $CSRC (sha256 $CPIN) must both give the pinned sha256 $CPIN"

CASE="G1: the settings schema matches its pin"
D=$(mk g1_pin)
[ "$(sha "$D/$SCH")" = "$SPIN" ] || fail "the copied schema does not match its pin"
run_names "$D" alice
expect_ok

CASE="G2: a fixture named like a grep hit of a verbatim file"
D=$(mk g2_colon)
printf '%s\n' 'owner alice /Users/alice/x Bearer abc' > "$D/$SCH:1:x"
run_names "$D" alice
expect_fail "a path in the scan scope holds ':': $SCH:1:x"

CASE="G3: a live script with a blank in its name"
D=$(mk g3_live)
printf '%s\n' '# key sk-ant-test' > "$D/scripts/premises/live foo.sh"
run "$D"
expect_fail "a path in the scan scope holds whitespace: 'scripts/premises/live foo.sh'"

CASE="G3: a fixture with a blank in its name"
D=$(mk g3_fixture)
printf '%s\n' '# key sk-ant-test' > "$D/fixtures/codex/a b.txt"
run "$D"
expect_fail "a path in the scan scope holds whitespace: 'fixtures/codex/a b.txt'"

MAILF=fixtures/codex/mail.txt
CASE="G4: a percent-hack address at example.invalid"
D=$(mk g4_pct)
printf '%s\n' 'bob%corp.com@example.invalid' > "$D/$MAILF"
run "$D"
expect_fail "forbidden content (e-mail address): $MAILF:1:bob%corp.com@example.invalid"

CASE="G4: a bang-path address at example.invalid"
D=$(mk g4_bang)
printf '%s\n' 'corp.com!bob@example.invalid' > "$D/$MAILF"
run "$D"
expect_fail "forbidden content (e-mail address): $MAILF:1:corp.com!bob@example.invalid"

CASE="G4: an address in the local part of one at example.invalid"
D=$(mk g4_at)
printf '%s\n' 'bob@corp.com@example.invalid' > "$D/$MAILF"
run "$D"
expect_fail "forbidden content (e-mail address): $MAILF:1:bob@corp.com@example.invalid"

CASE="G4: an address at example.invalid followed by @"
D=$(mk g4_after)
printf '%s\n' 'x@example.invalid@corp.com' > "$D/$MAILF"
run "$D"
expect_fail "forbidden content (e-mail address): $MAILF:1:x@example.invalid@corp.com"

CASE="G4: the live scripts' git identity and adjacent exempt addresses"
D=$(mk g4_ok)
printf '%s\n' 'user.email=zygo-live@example.invalid' 'a@example.invalid b@example.invalid c@example.invalid' > "$D/$MAILF"
run "$D"
expect_ok

CASE="G7: the exempt domain in upper case"
D=$(mk g7_upper_mail)
printf '%s\n' 'x@EXAMPLE.INVALID' > "$D/$MAILF"
run "$D"
expect_fail "forbidden content (e-mail address): $MAILF:1:x@EXAMPLE.INVALID"

CASE="G5: a directory at the Codex verbatim path"
D=$(mk g5_dir)
rm -f "$D/$CSCH"
mkdir "$D/$CSCH"
printf '%s\n' 'owner alice' > "$D/$CSCH/a"
printf 'sha256: %s\n' "$CPIN" > "$D/$CSRC"
run_names "$D" alice
expect_fail "verbatim file $CSCH is not a regular file"

CASE="G5: an empty directory at the Codex verbatim path"
D=$(mk g5_emptydir)
rm -f "$D/$CSCH"
mkdir "$D/$CSCH"
printf 'sha256: %s\n' "$(printf '' | { if command -v shasum >/dev/null 2>&1; then shasum -a 256; else sha256sum; fi; } | cut -d' ' -f1)" > "$D/$CSRC"
run "$D"
expect_fail "verbatim file $CSCH is not a regular file"

CASE="G6: a dangling symlink at the Codex verbatim path"
D=$(mk g6_dangle)
rm -f "$D/$CSCH"
ln -s "$T/none.json" "$D/$CSCH"
printf 'sha256: %s\n' "$CPIN" > "$D/$CSRC"
run "$D"
expect_fail "$CSCH is a symlink; a symlink in the scan scope is not scanned and is not evidence"

# The settings schema is Q1's evidence, so the evidence check (which refuses a symlink) fails first.
CASE="G6: a symlink at the settings schema path"
D=$(mk g6_link)
cp "$D/$SCH" "$T/s.json"
rm "$D/$SCH"
ln -s "$T/s.json" "$D/$SCH"
run "$D"
expect_fail "Q1: evidence path $SCH is missing, not a regular file, or trivial"

CASE="G7: no sha256 tool"
D=$(mk g7_nosha)
NOSHA="$T/nosha"
mkdir -p "$NOSHA"
OLDIFS=$IFS
IFS=:
for dir in $PATH; do
    [ -d "$dir" ] || continue
    for x in "$dir"/*; do
        b=${x##*/}
        case $b in shasum* | sha256sum) continue ;; esac
        [ -x "$x" ] && [ ! -e "$NOSHA/$b" ] && ln -s "$x" "$NOSHA/$b"
    done
done
IFS=$OLDIFS
SHBIN=$(command -v sh)
OUT=$(unset N2_FORBIDDEN_NAMES; PATH="$NOSHA" "$SHBIN" "$D/scripts/check-premises.sh" 2>&1)
RC=$?
N=$((N + 1))
expect_fail "no sha256 tool (need shasum or sha256sum)"

CASE="G7: a source file with no sha256 line"
D=$(mk g7_noline)
edit "$D/$SRC" '/^sha256:/d'
run "$D"
expect_fail "verbatim file $SCH (sha256 $SPIN) and its source file $SRC (sha256 <none>) must both give the pinned sha256 $SPIN"

CASE="G7: the source hash in upper case"
D=$(mk g7_upper)
edit "$D/$SRC" "s/^sha256: .*/sha256: $(printf '%s' "$SPIN" | tr 'a-f' 'A-F')/"
grep -q '^sha256: F26F' "$D/$SRC" || fail "planted change did not apply"
run "$D"
expect_ok

CASE="G7: the source hash line with CRLF"
D=$(mk g7_crlf)
edit "$D/$SRC" "s/^sha256: .*/sha256: $SPIN$(printf '\r')/"
grep -q "$(printf '\r')" "$D/$SRC" || fail "planted change did not apply"
run "$D"
expect_ok

CASE="G7: the source hash line with trailing blanks"
D=$(mk g7_trail)
edit "$D/$SRC" "s/^sha256: .*/sha256: $SPIN   /"
grep -q "^sha256: $SPIN   \$" "$D/$SRC" || fail "planted change did not apply"
run "$D"
expect_ok

ZERO=0000000000000000000000000000000000000000000000000000000000000000
CASE="G7: the first of two sha256 lines is right"
D=$(mk g7_first)
L="sha256: $ZERO" awk '{ print } /^sha256:/ { print ENVIRON["L"] }' "$D/$SRC" > "$D/$SRC.new" && mv "$D/$SRC.new" "$D/$SRC"
[ "$(grep -c '^sha256:' "$D/$SRC")" -eq 2 ] || fail "planted change did not apply"
run "$D"
expect_ok

CASE="G7: the first of two sha256 lines is wrong"
D=$(mk g7_second)
L="sha256: $ZERO" awk '/^sha256:/ { print ENVIRON["L"] } { print }' "$D/$SRC" > "$D/$SRC.new" && mv "$D/$SRC.new" "$D/$SRC"
run "$D"
expect_fail "verbatim file $SCH (sha256 $SPIN) and its source file $SRC (sha256 $ZERO) must both give the pinned sha256 $SPIN"

CASE="G7: a symlink matched by the live*.sh glob"
D=$(mk g7_liveglob)
ln -s ../../docs/premises.md "$D/scripts/premises/live-zz.sh"
run "$D"
expect_fail "scripts/premises/live-zz.sh is a symlink; a symlink in the scan scope is not scanned and is not evidence"

# grep reads every file as text (-a), so a hit in a binary file is a line with its content, not a
# "Binary file … matches" message (stdout on BSD grep, stderr on GNU grep).
BINF=fixtures/codex/bin.dat
CASE="H1: a binary file with an e-mail address"
D=$(mk h1_mail)
printf 'a\0b bob@corp.com\n' > "$D/$BINF"
run "$D"
expect_fail_prefix "forbidden content (e-mail address): $BINF:1:a"

CASE="H1: a binary file with an API key"
D=$(mk h1_key)
printf 'a\0b sk-ant-x\n' > "$D/$BINF"
run "$D"
expect_fail_prefix "forbidden content (API key): $BINF:1:a"

CASE="H1: a binary file with a forbidden name"
D=$(mk h1_name)
printf 'a\0b alice\n' > "$D/$BINF"
run_names "$D" alice
expect_fail "forbidden name from N2_FORBIDDEN_NAMES: $BINF:1"

# The exemption is the exact path, also for a rule that matches any case: a case variant of the
# Codex verbatim path is scanned. The real schema is removed first. On a case-insensitive file system
# the variant is the verbatim file, so its hash is pinned here (pin_codex) and the hash check passes; on a case-sensitive one the hash
# check does not see it. Either way only the exact-path exemption decides the result.
CASE="H2: a case variant of the Codex verbatim path with a Windows home path"
D=$(mk h2_case)
UCSCH=fixtures/codex/CONFIG.schema.json
rm -f "$D/$CSCH"   # else a case-insensitive file system keeps the real file's lower-case name
printf '%s\n' 'C:\Users\bob' > "$D/$UCSCH"
pin_codex "$D" "$D/$UCSCH"
run "$D"
expect_fail 'forbidden content (Windows home path): fixtures/codex/CONFIG.schema.json:1:C:\Users\bob'

echo "test-check-premises: ok ($N cases)"

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

# TCP_TIMES=1 (off by default; a report, not a test): mk and each check-premises.sh run print
# "test-check-premises: time <seconds> <step> <case>" on stderr (mk runs in $(...), so the next
# "other" step includes it).
TCP_T0=""
tick() {
    [ "${TCP_TIMES:-}" = 1 ] || return 0
    t=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
    [ -z "$TCP_T0" ] || awk -v a="$TCP_T0" -v b="$t" -v s="$1" -v c="$CASE" 'BEGIN { printf "test-check-premises: time %.3f %s %s\n", b - a, s, c }' >&2
    TCP_T0=$t
}

fail() {
    printf '%s\n' "test-check-premises: FAIL $CASE: $1" >&2
    printf '%s\n' "$OUT" >&2
    exit 1
}

# mk <name>: a copy tree $T/<name> with the script, its helpers, the fixtures and the documents
# it reads, at the same relative paths, and fixtures/codex (made when the tree has none); prints its path.
mk() {
    tick other
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
    tick mk
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

# SOH: the byte \001. A shell variable cannot hold a NUL, so a case plants a SOH and soh_to_nul
# changes it to a NUL (TF-3).
SOH=$(printf '\001')

# soh_to_nul <file>: changes each SOH byte of the file to a NUL byte; the file must then hold a NUL.
soh_to_nul() {
    tr '\001' '\000' <"$1" > "$1.new" && mv "$1.new" "$1"
    [ "$(tr -cd '\000' <"$1" | wc -c | tr -d ' ')" -gt 0 ] || fail "planted NUL did not apply"
}

# add_nul <file> <ERE> <line>: inserts the line (a SOH in it becomes a NUL) after the first line of
# the file that matches the ERE.
add_nul() {
    L="$3" R="$2" awk '{ print } !d && $0 ~ ENVIRON["R"] { print ENVIRON["L"]; d = 1 }' "$1" > "$1.new" &&
        mv "$1.new" "$1"
    soh_to_nul "$1"
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
    tick other
    OUT=$(unset N2_FORBIDDEN_NAMES; sh "$1/scripts/check-premises.sh" 2>&1)
    RC=$?
    N=$((N + 1))
    tick check
}

# run_names <dir> <names> [<shell>]: as run, with N2_FORBIDDEN_NAMES set to the names for the script,
# run by the shell (default sh).
run_names() {
    tick other
    OUT=$(N2_FORBIDDEN_NAMES="$2" "${3:-sh}" "$1/scripts/check-premises.sh" 2>&1)
    RC=$?
    N=$((N + 1))
    tick check
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

# run_split <dir> <names> <shell> [nopipe]: as run_names, but OUT is stdout only and ERR is stderr
# only (TF-3 round 2). nopipe: the script starts with SIGPIPE ignored (as on the macOS CI runner), so
# a process that writes into a pipe whose reader has exited gets EPIPE and may print an error on
# stderr in place of a silent death.
ERR=""
run_split() {
    tick other
    if [ "${4:-}" = nopipe ]; then
        OUT=$( (trap '' PIPE; export N2_FORBIDDEN_NAMES="$2"; exec "$3" "$1/scripts/check-premises.sh") 2>"$T/err")
    else
        OUT=$( (export N2_FORBIDDEN_NAMES="$2"; exec "$3" "$1/scripts/check-premises.sh") 2>"$T/err")
    fi
    RC=$?
    ERR=$(cat "$T/err")
    N=$((N + 1))
    tick check
}

# expect_no_stderr: ERR (from run_split) is empty.
expect_no_stderr() {
    [ -z "$ERR" ] || fail "the script wrote to stderr: $ERR"
}

# expect_ok_alone: after run_split, exit 0, stderr empty and the last stdout line the ok line (a note
# line, such as the one for a host name shorter than 4 characters, may come before it).
expect_ok_alone() {
    expect_no_stderr
    expect_ok
}

# expect_fail_alone <text>: after run_split, stderr empty and stdout exactly "check-premises: <text>".
expect_fail_alone() {
    expect_no_stderr
    expect_fail "$1"
}

CASE="real tree"
D=$(mk real)
run "$D"
expect_ok
case "$OUT" in *' PW15-bash PW15-tool PW15-slash '*) ;; *) fail "ok line lacks ' PW15-bash PW15-tool PW15-slash '" ;; esac

CASE="Codex header and PC99 row"
D=$(mk pc1)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC99 Codex 0.172.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_ok
case "$OUT" in *' PC99)'*) ;; *) fail "ok line lacks ' PC99)'" ;; esac

CASE="a failed PC row needs no note"
D=$(mk pc1failed)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC99 Codex 0.172.0 failed fixtures/codex/pc1.txt)"
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
add_row "$D" "$(row PC99 Cursor 1.0.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC99: agent 'Cursor' has no header line 'Cursor: <version>' before the first '## ' of docs/premises.md"

CASE="Codex below its header"
D=$(mk codexlow)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC99 Codex 0.171.9 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC99: version '0.171.9' is below the header's '0.172.0' for Codex, or not a dotted number"

CASE="Codex above its header"
D=$(mk codexhigh)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC99 Codex 0.173.0 verified fixtures/codex/pc1.txt)"
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
add_row "$D" "$(row PC98 tmux 3.2a verified fixtures/codex/pc1.txt)"
run "$D"
expect_ok

CASE="trivial evidence"
D=$(mk trivial)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC99 Codex 0.172.0 verified fixtures/codex/empty.json)"
run "$D"
expect_fail "PC99: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial"

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
add_row "$D" '|PC97 | Test row | tmux | 3.2 | 2026-11-25 | verified | `fixtures/codex/empty.json` |'
run "$D"
expect_fail "PC97: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial"

CASE="a row with a blank before the first bar"
D=$(mk lead)
evidence "$D"
add_row "$D" ' | PC97 | Test row | tmux | 3.2 | 2026-11-25 | verified | `fixtures/codex/empty.json` |'
run "$D"
expect_fail "PC97: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial"

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
add_row "$D" "$(row PC99 Cursor 1.0 verified fixtures/codex/pc1.txt)"
printf '\n%s\n' 'Cursor: 1.0' >> "$D/docs/premises.md"
run "$D"
expect_fail "PC99: agent 'Cursor' has no header line 'Cursor: <version>' before the first '## ' of docs/premises.md"

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
printf '\n%s\n' '| Name | Value |' '|---|---|' '| PC95 | x |' >> "$D/docs/premises.md"
run "$D"
expect_fail "docs/premises.md:$(lnof "$D" '| PC95 | x |'): row 'PC95' is outside the Results table"

CASE="an empty Agent cell"
D=$(mk noagent)
evidence "$D"
add_row "$D" "$(row PC99 '' 3.2 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC99: the Agent cell is empty"

CASE="an empty Version cell"
D=$(mk nover)
evidence "$D"
add_row "$D" "$(row PC99 tmux '' verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC99: the Version cell is empty"

CASE="empty Agent and Version cells"
D=$(mk noagentver)
evidence "$D"
add_row "$D" "$(row PC99 '' '' verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC99: the Agent cell is empty"

CASE="a result that is not verified or failed"
D=$(mk badresult)
evidence "$D"
add_row "$D" "$(row PC99 tmux 3.2 passed fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC99: result 'passed' is not verified or failed"

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
add_row "$D" "$(row PC99 Codex 0.172.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_ok

CASE="a second header line with another version"
D=$(mk hdrconflict)
evidence "$D"
hdr_twice "$D" 'Codex: 0.172.0' 'Codex: 0.100.0'
add_row "$D" "$(row PC99 Codex 0.172.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "docs/premises.md:$(grep -nx 'Codex: 0\.100\.0' "$D/docs/premises.md" | cut -d: -f1): header 'Codex: 0.100.0' differs from the first header line 'Codex: 0.172.0'"

CASE="extra blanks at the ends of cells"
D=$(mk blanks)
evidence "$D"
add_row "$D" '| PC99   | Test row |   tmux   |   3.2   |   2026-11-25   |   verified   | `fixtures/codex/pc1.txt` |'
run "$D"
expect_ok

CASE="a header value with blanks at its end"
D=$(mk hdrblanks)
evidence "$D"
add_header "$D" 'Codex: 0.172.0   '
add_row "$D" "$(row PC99 Codex 0.172.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_ok

NOHDR="has no header line 'Cursor: <version>' before the first '## ' of docs/premises.md"

CASE="a date cell with a backslash sequence"
D=$(mk datebs)
evidence "$D"
add_row "$D" '| PC99 | Test row | tmux | 3.2 | 2026-11-2\0065 | verified | `fixtures/codex/pc1.txt` |'
run "$D"
expect_fail "PC99: date '2026-11-2\0065' is not YYYY-MM-DD"

CASE="a result cell with a backslash sequence"
D=$(mk resultbs)
evidence "$D"
add_row "$D" '| PC99 | Test row | tmux | 3.2 | 2026-11-25 | verifie\0144 | `fixtures/codex/pc1.txt` |'
run "$D"
expect_fail "PC99: result 'verifie\0144' is not verified or failed"

CASE="a version cell with a backslash sequence"
D=$(mk verbs)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC99 Codex '0.172.\060' verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC99: version '0.172.\060' is below the header's '0.172.0' for Codex, or not a dotted number"

CASE="an Item cell with a backslash n"
D=$(mk itembn)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" '| PC99 | Codex writes `a\nb` | Codex | 0.172.0 | 2026-11-25 | verified | `fixtures/codex/pc1.txt` |'
run "$D"
expect_ok

CASE="an Item cell with a backslash c"
D=$(mk itembc)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" '| PC99 | Codex reads `\c` | Codex | 0.172.0 | 2026-11-25 | verified | `fixtures/codex/pc1.txt` |'
run "$D"
expect_ok

CASE="a header value with a tab"
D=$(mk hdrtab)
evidence "$D"
H=$(printf 'Codex: 0.172.0\tjunk')
add_header "$D" "$H"
add_row "$D" "$(row PC99 Codex 0.172.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "docs/premises.md:$(lnof "$D" "$H"): the value of a header line holds a tab"

CASE="a header line in a backtick fence"
D=$(mk hdrfence)
evidence "$D"
add_lines "$D" '```text' 'Cursor: 1.0' '```'
[ "$(grep -cx 'Cursor: 1\.0' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
add_row "$D" "$(row PC99 Cursor 1.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC99: agent 'Cursor' $NOHDR"

CASE="a header line in a tilde fence"
D=$(mk hdrtilde)
evidence "$D"
add_lines "$D" '~~~' 'Cursor: 1.0' '~~~'
[ "$(grep -cx 'Cursor: 1\.0' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
add_row "$D" "$(row PC99 Cursor 1.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC99: agent 'Cursor' $NOHDR"

CASE="a header line in an HTML comment"
D=$(mk hdrcomment)
evidence "$D"
add_lines "$D" '<!--' 'Cursor: 1.0' '-->'
[ "$(grep -cx 'Cursor: 1\.0' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
add_row "$D" "$(row PC99 Cursor 1.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC99: agent 'Cursor' $NOHDR"

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
add_row "$D" '| PC99 | Codex pipes a \| b | Codex | 0.172.0 | 2026-11-25 | verified | `fixtures/codex/pc1.txt` |'
run "$D"
expect_ok

CASE="a tab at the end of a cell"
D=$(mk celltab)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(printf '| PC99 | Test row | Codex\t| 0.172.0 | 2026-11-25 | verified | `fixtures/codex/pc1.txt` |')"
run "$D"
expect_ok

CASE="an evidence path with a .. component"
D=$(mk dotdot)
add_row "$D" "$(row PC99 tmux 3.2 verified docs/../docs/premises.md)"
run "$D"
expect_fail "PC99: evidence path docs/../docs/premises.md is absolute or holds a '..' component"

# PC-9 round 2 (F1): the last component is a regular file, but a directory on the path is a symlink
# to a folder outside the tree; scripts/premises/ext is outside the forbidden-content scan scope.
CASE="an evidence path through a symlinked directory"
D=$(mk symdir)
mkdir -p "$T/symdir_out"
echo 'outside evidence' > "$T/symdir_out/ev.sh"
ln -s "$T/symdir_out" "$D/scripts/premises/ext"
add_row "$D" "$(row PC99 tmux 3.2 verified scripts/premises/ext/ev.sh)"
run "$D"
INREPO='is not a plain path inside the repository (a symlink, `.`/`//` part, or unreadable directory)'
expect_fail "PC99: evidence path scripts/premises/ext/ev.sh $INREPO"

# PC-9 round 3 (G4): a '.' or '//' part in the directory is refused too, with the same message.
CASE="an evidence path with a . part"
D=$(mk dotpart)
evidence "$D"
add_row "$D" "$(row PC99 tmux 3.2 verified fixtures/./codex/pc1.txt)"
run "$D"
expect_fail "PC99: evidence path fixtures/./codex/pc1.txt $INREPO"

CASE="an evidence path with a // part"
D=$(mk slashpart)
evidence "$D"
add_row "$D" "$(row PC99 tmux 3.2 verified fixtures//codex/pc1.txt)"
run "$D"
expect_fail "PC99: evidence path fixtures//codex/pc1.txt $INREPO"

CASE="a joint agent with no header for the second"
D=$(mk joint)
evidence "$D"
add_row "$D" "$(row PC99 'Claude Code + Cursor' '2.1.328 + 1.0' verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC99: agent 'Cursor' $NOHDR"

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
add_row "$D" '    | PC97 | Test row | tmux | 3.2 | 2026-11-25 | verified | `fixtures/codex/empty.json` |'
run "$D"
expect_ok
case "$OUT" in *' PC97'*) fail "ok line names PC97" ;; esac

CASE="a row indented with a tab"
D=$(mk indenttab)
evidence "$D"
add_row "$D" "$(printf '\t| PC97 | Test row | tmux | 3.2 | 2026-11-25 | verified | `fixtures/codex/empty.json` |')"
run "$D"
expect_ok
case "$OUT" in *' PC97'*) fail "ok line names PC97" ;; esac

CASE="a row indented three blanks"
D=$(mk indent3)
evidence "$D"
add_row "$D" '   | PC97 | Test row | tmux | 3.2 | 2026-11-25 | verified | `fixtures/codex/empty.json` |'
run "$D"
expect_fail "PC97: evidence path fixtures/codex/empty.json is missing, not a regular file, or trivial"

CASE="a header line indented four blanks"
D=$(mk hdrindent4)
evidence "$D"
add_header "$D" '    Cursor: 1.0'
add_row "$D" "$(row PC99 Cursor 1.0 verified fixtures/codex/pc1.txt)"
run "$D"
expect_fail "PC99: agent 'Cursor' $NOHDR"

# Review round 1 (PC-8).
CASE="an indented line in the middle of the Results table"
D=$(mk indentmid)
L='    | PC97 | indented |' awk '{ print } /^\| Q7 \|/ { print ENVIRON["L"] }' "$D/docs/premises.md" > "$D/docs/premises.md.new" &&
    mv "$D/docs/premises.md.new" "$D/docs/premises.md"
[ "$(grep -c '^    | PC97 |' "$D/docs/premises.md")" -eq 1 ] || fail "planted change did not apply"
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

# bash >=4.4 writes a warning to stderr when a command substitution reads a NUL byte; under bash the
# scan of a binary file gives only the failure line (TF-3). The e-mail and API key cases guard the
# tr that scan has now: the NUL is a blank in the message (with no tr, read drops it: "ab").
CASE="TF-3: under bash, a binary file with an e-mail address"
D=$(mk tf3_mail)
printf 'a\0b bob@corp.com\n' > "$D/$BINF"
run_names "$D" '' bash
expect_fail "forbidden content (e-mail address): $BINF:1:a b bob@corp.com"

CASE="TF-3: under bash, a binary file with an API key"
D=$(mk tf3_key)
printf 'a\0b sk-ant-x\n' > "$D/$BINF"
run_names "$D" '' bash
expect_fail "forbidden content (API key): $BINF:1:a b sk-ant-x"

CASE="TF-3: under bash, a binary file with a forbidden name"
D=$(mk tf3_name)
printf 'a\0b alice\n' > "$D/$BINF"
run_names "$D" alice bash
expect_fail "forbidden name from N2_FORBIDDEN_NAMES: $BINF:1"

# TF-3: every command substitution that reads file content gives the shell no NUL byte, so bash gives
# no warning on stderr. Round 2: where the value feeds a decision, the NUL is deleted from the output
# of the command that reads the file, as every shell did before TF-3, so a NUL changes no result. Each
# case runs under bash with stdout and stderr apart (run_split).
CASE="TF-3: under bash, an N38 line with a NUL inside a forbidden name's word"
D=$(mk tf3_notes)
printf 'N38 note: x\0alice\n' >> "$D/docs/conformance.md"
run_split "$D" alice bash
expect_ok_alone

CASE="TF-3: under bash, a required file with a NUL"
D=$(mk tf3_required)
printf 'x\0\n' > "$D/fixtures/opencode/help.txt"
run_split "$D" '' bash
expect_ok_alone

CASE="TF-3: under bash, a header line with a NUL"
D=$(mk tf3_header)
add_nul "$D/docs/premises.md" '^tmux: ' "Zz-test: a${SOH}b"
run_split "$D" '' bash
expect_ok_alone

CASE="TF-3: under bash, a table line with a NUL in its first cell"
D=$(mk tf3_table)
add_nul "$D/docs/premises.md" '^tmux: ' "| a${SOH}b | x |"
run_split "$D" '' bash
expect_ok_alone

CASE="TF-3: under bash, a Results row with a NUL"
D=$(mk tf3_row)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC99 Codex 0.172.0 verified fixtures/codex/pc1.txt | sed "s/Test row/Test${SOH}row/")"
soh_to_nul "$D/docs/premises.md"
run_split "$D" '' bash
expect_ok_alone

CASE="TF-3: under bash, spec section 19 with a NUL"
D=$(mk tf3_s19)
add_nul "$D/docs/specs/2026-11-18-zygo-design.md" '^## 19\. ' "x${SOH}y"
run_split "$D" '' bash
expect_ok_alone

CASE="TF-3: under bash, spec section 19a with a NUL"
D=$(mk tf3_s19a)
add_nul "$D/docs/specs/2026-11-18-zygo-design.md" '^## 19a\. ' "x${SOH}y"
run_split "$D" '' bash
expect_ok_alone

CASE="TF-3: under bash, spec section 10.5 with a NUL"
D=$(mk tf3_s105)
add_nul "$D/docs/specs/2026-11-18-zygo-design.md" '^### 10\.5 ' "x${SOH}y"
run_split "$D" '' bash
expect_ok_alone

# TF-3 round 2 R2-F1, R2-F2, R2-F3: a NUL that a blank in its place would make significant. Before
# TF-3 awk read the NUL as a character (so "<NUL>```" is no fence and "<NUL>| P1 |" no table line)
# and the shell deleted it from the output (so "sk-<NUL>ant-" is "sk-ant-"); the result stays.
CASE="TF-3 R2-F1: under bash, a fence line that starts with a NUL"
D=$(mk tf3_fence)
hl=$(($(grep -n '^tmux: ' "$D/docs/premises.md" | cut -d: -f1) + 2))
add_nul "$D/docs/premises.md" '^tmux: ' "$(printf '%s```\nCodex: 0.1.0\n%s```' "$SOH" "$SOH")"
run_split "$D" '' bash
expect_fail_alone "docs/premises.md:$hl: header 'Codex: 0.1.0' differs from the first header line 'Codex: 0.172.0'"

CASE="TF-3 R2-F2: under bash, a NUL before a table line's first bar"
D=$(mk tf3_tablebar)
add_nul "$D/docs/premises.md" '^tmux: ' "${SOH}| P1 | x |"
run_split "$D" '' bash
expect_ok_alone

CASE="TF-3 R2-F3: under bash, an N38 line with a NUL inside an API key"
D=$(mk tf3_noteskey)
printf 'N38 note: sk-\0ant-x\n' >> "$D/docs/conformance.md"
run_split "$D" '' bash
expect_fail_alone "forbidden content (API key): docs/conformance.md:$(wc -l <"$D/docs/conformance.md" | tr -d ' '):N38 note: sk-ant-x"

# TF-3 round 2 R2-F4: expect_ok_alone reads stderr on its own, so a note line on stdout (here the one
# for a host name shorter than 4 characters, from a hostname stub) does not fail it.
CASE="TF-3 R2-F4: under bash, a host name shorter than 4 characters"
D=$(mk tf3_shorthost)
mkdir -p "$T/stubbin"
printf '#!/bin/sh\necho box\n' > "$T/stubbin/hostname"
chmod +x "$T/stubbin/hostname"
OLDPATH=$PATH
PATH="$T/stubbin:$PATH"
run_split "$D" '' bash
PATH=$OLDPATH
expect_ok_alone
case "$OUT" in *'check-premises: note: a host name is shorter than 4 characters or localhost; host-name scan skipped for it'*) ;;
    *) fail "the hostname stub did not apply" ;; esac

# TF-3 round 2, macOS CI: with SIGPIPE ignored, a process that writes into a pipe whose reader has
# exited gets EPIPE and prints an error (BSD tr: "tr: stdout: Broken pipe"; GNU tr: "tr: write error:
# Broken pipe"). Each reader after a tr reads to its end. The header scan stops at the first "## " line
# (a file of more than a pipe buffer after it); the name scan keeps only the first of many hit lines.
CASE="TF-3 R2 EPIPE: under bash with SIGPIPE ignored, a large docs/premises.md"
D=$(mk tf3_pipehdr)
awk 'BEGIN { for (i = 0; i < 20000; i++) print "padding text after the header lines, to fill a pipe buffer" }' >> "$D/docs/premises.md"
run_split "$D" '' bash nopipe
expect_ok_alone

CASE="TF-3 R2 EPIPE: under bash with SIGPIPE ignored, many name scan hit lines"
D=$(mk tf3_pipename)
awk 'BEGIN { for (i = 0; i < 20000; i++) print "zzpipeq" }' > "$D/fixtures/codex/many.txt"
run_split "$D" zzpipeq bash nopipe
expect_fail_alone "forbidden name from N2_FORBIDDEN_NAMES: fixtures/codex/many.txt:1"

# The hit loops of scan, cx_scan and env_scan keep only the first hit line: 4000 hit lines (more than
# a pipe buffer of output) of a rule with no drop script give the failure line and nothing on stderr.
CASE="TF-3 R2 EPIPE: under bash with SIGPIPE ignored, many scan hit lines"
D=$(mk tf3_pipescan)
awk 'BEGIN { for (i = 0; i < 4000; i++) print "sk-ant-x" }' > "$D/fixtures/codex/many.txt"
run_split "$D" '' bash nopipe
expect_fail_alone "forbidden content (API key): fixtures/codex/many.txt:1:sk-ant-x"

CASE="TF-3 R2 EPIPE: under bash with SIGPIPE ignored, many cx_scan hit lines"
D=$(mk tf3_pipecx)
awk 'BEGIN { for (i = 0; i < 4000; i++) print "__CFx" }' > "$D/fixtures/codex/many.txt"
run_split "$D" '' bash nopipe
expect_fail_alone "forbidden content (macOS env name): fixtures/codex/many.txt:1:__CFx"

CASE="TF-3 R2 EPIPE: under bash with SIGPIPE ignored, many env_scan hit lines"
D=$(mk tf3_pipeenv)
awk 'BEGIN { for (i = 0; i < 4000; i++) print "/cc-socks/1.sock" }' > "$D/fixtures/claude-code/many.txt"
run_split "$D" '' bash nopipe
expect_fail_alone "forbidden content (socket pid): fixtures/claude-code/many.txt:1:/cc-socks/1.sock"

# TF-3 round 2: cells uses the byte \001 as the placeholder of an escaped "|", so a row that already
# holds a \001 fails closed.
CASE="TF-3 R2: a Results row with a SOH byte"
D=$(mk tf3_rowsoh)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC99 Codex 0.172.0 verified fixtures/codex/pc1.txt | sed "s/Test row/Test${SOH}row/")"
rl=$(grep -n '^| PC99 |' "$D/docs/premises.md" | cut -d: -f1)
run_split "$D" '' bash
expect_fail_alone "docs/premises.md:$rl: row 'PC99' holds a SOH byte (\\001), the placeholder of an escaped '|'"

# TF-3 round 3 helpers.
# pad_after <file> <ERE> <n> <line>: inserts the line n times after the first line that matches the ERE.
pad_after() {
    R="$2" L="$4" awk -v n="$3" '{ print } !d && $0 ~ ENVIRON["R"] { for (i = 0; i < n; i++) print ENVIRON["L"]; d = 1 }' "$1" > "$1.new" &&
        mv "$1.new" "$1"
}
# pad_before <file> <ERE> <n> <line>: inserts the line n times before the first line that matches the ERE.
pad_before() {
    R="$2" L="$4" awk -v n="$3" '!d && $0 ~ ENVIRON["R"] { for (i = 0; i < n; i++) print ENVIRON["L"]; d = 1 } { print }' "$1" > "$1.new" &&
        mv "$1.new" "$1"
}
# sec_sub <file> <start ERE> <end ERE> <text> <new text>: in each line from the line that matches the
# start ERE to the line before the one that matches the end ERE, the first <text> becomes <new text>
# (a SOH in it then becomes a NUL).
sec_sub() {
    A="$2" B="$3" X="$4" Y="$5" awk '$0 ~ ENVIRON["A"] { f = 1 } $0 ~ ENVIRON["B"] { f = 0 }
        f && (i = index($0, ENVIRON["X"])) { $0 = substr($0, 1, i - 1) ENVIRON["Y"] substr($0, i + length(ENVIRON["X"])) } { print }' "$1" > "$1.new" &&
        mv "$1.new" "$1"
    soh_to_nul "$1"
}
# AWK_NUL: 1 when awk reads a NUL as a character (gawk, mawk); empty when a NUL ends the line for it
# (BSD awk). The spec section cases expect the pre-TF-3 result of the host's awk.
AWK_NUL=""
[ "$(printf 'a\000b\n' | awk '{ print length($0) }')" != 3 ] || AWK_NUL=1
ZSPEC=docs/specs/2026-11-18-zygo-design.md

# TF-3 round 3 R3-1: after the first hit line, the hit loops only read to the end (no drop script),
# so a rule with a drop script and many hit lines is as fast as one hit; the failure line stays.
CASE="TF-3 R3-1: under bash with SIGPIPE ignored, many e-mail hit lines"
D=$(mk tf3_pipemail)
awk 'BEGIN { for (i = 0; i < 2000; i++) print "bob@corp.com" }' > "$D/fixtures/codex/many.txt"
run_split "$D" '' bash nopipe
expect_fail_alone "forbidden content (e-mail address): fixtures/codex/many.txt:1:bob@corp.com"

# TF-3 round 3 R3-3: a NUL deleted (the pre-TF-3 result) and a NUL made a blank give different
# results at each site.
CASE="TF-3 R3-3: under bash, a NUL inside a header version"
D=$(mk tf3_hdrver)
add_header "$D" "Codex: 0.17${SOH}2.0"
soh_to_nul "$D/docs/premises.md"
run_split "$D" '' bash
expect_ok_alone

CASE="TF-3 R3-3: under bash, a NUL inside a Results row id"
D=$(mk tf3_rowid)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row "PC${SOH}99" Codex 0.172.0 verified fixtures/codex/pc1.txt)"
soh_to_nul "$D/docs/premises.md"
run_split "$D" '' bash
expect_ok_alone

CASE="TF-3 R3-3: under bash, a NUL inside a Results row version"
D=$(mk tf3_rowver)
evidence "$D"
add_header "$D" 'Codex: 0.172.0'
add_row "$D" "$(row PC99 Codex "0.172${SOH}.0" verified fixtures/codex/pc1.txt)"
soh_to_nul "$D/docs/premises.md"
run_split "$D" '' bash
expect_ok_alone

# spec_nul_case <name> <start ERE> <end ERE> <note text> <failure text>: a NUL inside the dated note
# of a failed row in that spec section.
spec_nul_case() {
    CASE="TF-3 R3-3: under bash, a NUL inside the $1 note"
    D=$(mk "tf3_note$1")
    sec_sub "$D/$ZSPEC" "$2" "$3" "$4" "$(printf '%s' "$4" | sed "s/N2/N${SOH}2/")"
    run_split "$D" '' bash
    if [ -n "$AWK_NUL" ]; then expect_ok_alone; else expect_fail_alone "$5"; fi
}
spec_nul_case 19 '^## 19\. ' '^## 19a\. ' 'N2, P6a:' 'P6a: failed, but spec §19 has no dated note *(Note YYYY-MM-DD, N2, P6a: …)*'
spec_nul_case 19a '^## 19a\. ' '^## 20\. ' 'N2, Q7:' 'Q7: failed, but spec §19a has no dated note *(Note YYYY-MM-DD, N2, Q7: …)*'
spec_nul_case 10.5 '^### 10\.5 ' '^### 10\.6 ' 'N2, P6a:' 'P6a: failed, but spec §10.5 has no dated note amending the language list'

# TF-3 round 3 R3-4, R3-5: every reader fed by a pipe reads to its end, so with SIGPIPE ignored no
# writer gets EPIPE when its output is larger than a pipe buffer; the result stays.
CASE="TF-3 R3-4: under bash with SIGPIPE ignored, a differing header before many header lines"
D=$(mk tf3_pipedup)
hl=$(($(grep -n '^tmux: ' "$D/docs/premises.md" | cut -d: -f1) + 1))
pad_after "$D/docs/premises.md" '^tmux: ' 3000 'Zz-padding-header-with-a-long-name-to-fill-a-pipe-buffer: 1'
add_lines "$D" 'Codex: 0.1.0'
run_split "$D" '' bash nopipe
expect_fail_alone "docs/premises.md:$hl: header 'Codex: 0.1.0' differs from the first header line 'Codex: 0.172.0'"

CASE="TF-3 R3-4: under bash with SIGPIPE ignored, an agent header before many header lines"
D=$(mk tf3_pipever)
pad_after "$D/docs/premises.md" '^tmux: ' 3000 'Zz-padding-header-with-a-long-name-to-fill-a-pipe-buffer: 1'
run_split "$D" '' bash nopipe
expect_ok_alone

CASE="TF-3 R3-4: under bash with SIGPIPE ignored, a row outside the Results table before many table lines"
D=$(mk tf3_pipeout)
hl=$(($(grep -n '^tmux: ' "$D/docs/premises.md" | cut -d: -f1) + 1))
pad_after "$D/docs/premises.md" '^tmux: ' 3000 '| zz-padding-table-line-with-a-long-first-cell-to-fill-a-pipe-buffer | x |'
add_lines "$D" '| PX1 | x |'
run_split "$D" '' bash nopipe
expect_fail_alone "docs/premises.md:$hl: row 'PX1' is outside the Results table"

# spec_pad_case <name> <ERE of the next heading>: many lines at the end of that spec section, after
# the dated note of its failed row.
spec_pad_case() {
    CASE="TF-3 R3-4: under bash with SIGPIPE ignored, a large spec section $1"
    D=$(mk "tf3_pipesec$1")
    pad_before "$D/$ZSPEC" "$2" 4000 'padding text at the end of the section, to fill a pipe buffer'
    run_split "$D" '' bash nopipe
    expect_ok_alone
}
spec_pad_case 19 '^## 19a\. '
spec_pad_case 19a '^## 20\. '
spec_pad_case 10.5 '^### 10\.6 '

# find_pad_case <name> <kind> <name prefix> <failure prefix>: 1000 entries of that kind (file or
# symlink) with long names in fixtures/codex/pad, so the find output is larger than a pipe buffer.
# find's stderr goes to /dev/null in the script (a scope path may be missing), so an EPIPE of find
# is not seen on stderr; these cases keep the result (the first path find prints).
find_pad_case() {
    CASE="TF-3 R3-4: under bash with SIGPIPE ignored, many paths $1"
    D=$(mk "tf3_pipefind$2")
    mkdir -p "$D/fixtures/codex/pad"
    i=0
    while [ "$i" -lt 1000 ]; do
        f="$D/fixtures/codex/pad/$3-padding-name-long-enough-to-fill-a-pipe-buffer-with-few-entries-of-the-find-output-$i"
        if [ "$2" = link ]; then ln -s pc1.txt "$f"; else : > "$f"; fi
        i=$((i + 1))
    done
    run_split "$D" '' bash nopipe
    expect_no_stderr
    expect_fail_prefix "$4"
}
find_pad_case 'with whitespace' space 'a b' "a path in the scan scope holds whitespace: 'fixtures/codex/pad/a b-padding-name-"
find_pad_case "with ':'" colon 'a:b' 'a path in the scan scope holds '"':'"': fixtures/codex/pad/a:b-padding-name-'
find_pad_case 'that are symlinks' link 'ab' 'fixtures/codex/pad/ab-padding-name-'

CASE="TF-3 R3-5: under bash with SIGPIPE ignored, many sha256 lines in a verbatim source file"
D=$(mk tf3_pipesha)
awk -v h="$CPIN" 'BEGIN { for (i = 0; i < 2000; i++) print "sha256: " h }' >> "$D/fixtures/codex/config.schema.source.txt"
run_split "$D" '' bash nopipe
expect_ok_alone

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

# PC-13: the Claude Code environment block lines in fixtures/claude-code and fixtures/opencode.
CASE="PC-13: a shell name in a Claude Code environment block"
D=$(mk pc13shell)
printf '%s\n' 'x\n - Shell: zsh\n' > "$D/fixtures/claude-code/plant.txt"
run "$D"
expect_fail 'forbidden content (shell name): fixtures/claude-code/plant.txt:1:x\n - Shell: zsh\n'

CASE="PC-13: an OS version in an OpenCode fixture"
D=$(mk pc13os)
printf '%s\n' ' - OS Version: Darwin 25.6.0' > "$D/fixtures/opencode/plant.txt"
run "$D"
expect_fail 'forbidden content (OS version): fixtures/opencode/plant.txt:1: - OS Version: Darwin 25.6.0'

CASE="PC-13: masked lines, prose and a shell key pass"
D=$(mk pc13ok)
printf '%s\n' 'x\n - Shell: (omitted)\n - OS Version: (os omitted)\n' 'The Shell: zsh is the default' '"shell": "/bin/zsh"' > "$D/fixtures/claude-code/plant.txt"
run "$D"
expect_ok

# PC-13 review round 1 F5, F6, F8: the forms of scrub.py ENV_SHELL and ENV_OS, the exact placeholders
# only, and an escaped backslash then "n" is not a newline. F3: the Stainless arch and runtime version.
TAB=$(printf '\t')
# env_case <name> <planted line> <failure text after "forbidden content ">: one planted line in
# fixtures/claude-code, and the failure it must give.
env_case() {
    CASE="PC-13 R1: $1"
    D=$(mk "pc13r1_$N")
    printf '%s\n' "$2" > "$D/fixtures/claude-code/plant.txt"
    run "$D"
    expect_fail "forbidden content ($3): fixtures/claude-code/plant.txt:1:$2"
}
env_case "lower case shell" 'x\n - shell: zsh\n' 'shell name'
env_case "upper case OS version" ' - OS VERSION: Darwin 25.6.0' 'OS version'
env_case "a tab after the colon" " - Shell:${TAB}zsh" 'shell name'
env_case "an escaped tab after the colon" 'x\n - OS Version:\tDarwin 25.6.0\n' 'OS version'
env_case "no dash at a line start" 'Shell: zsh' 'shell name'
env_case "no dash after an escaped newline" 'x\nOS Version: Linux 6.8.0' 'OS version'
env_case "a shell name in parentheses" 'x\n - Shell: (zsh)\n' 'shell name'
env_case "the OS placeholder on the shell line" 'x\n - Shell: (os omitted)\n' 'shell name'
env_case "an escaped backslash then an escaped newline" 'x\\\n - Shell: zsh' 'shell name'
env_case "Stainless arch" '    "x-stainless-arch": "arm64",' 'Stainless arch'
env_case "Stainless runtime version" '{\"X-Stainless-Runtime-Version\":\"v26.12.1\"}' 'Stainless runtime version'

CASE="PC-13 R1: placeholders in any form and the OS family pass"
D=$(mk pc13r1ok)
printf '%s\n' "shell:${TAB}(omitted)" 'x\n - shell:\t(omitted)\nOS VERSION: (os omitted)\n' \
    '    "x-stainless-arch": "(arch omitted)",' '{\"x-stainless-runtime-version\":\"(version omitted)\"}' \
    '    "x-stainless-os": "MacOS",' > "$D/fixtures/claude-code/plant.txt"
run "$D"
expect_ok

# PC-13 review round 2. G1 (fail closed): any run of backslashes then n or r, a carriage return or a
# \u000a escape starts a line; the reviewer's .sse partial_json and .summary.json forms fail.
CR=$(printf '\r')
NBSP=$(printf '\302\240')
# env_file <name> <file> <planted line> <failure text after "forbidden content ">: as env_case, in the
# named file under fixtures/claude-code.
env_file() {
    CASE="PC-13 R2: $1"
    D=$(mk "pc13r2_$N")
    printf '%s\n' "$3" > "$D/fixtures/claude-code/$2"
    run "$D"
    expect_fail "forbidden content ($4): $(printf '%s\n' "fixtures/claude-code/$2:1:$3" | cut -c1-160)"
}
env_file "G1 .sse partial_json" plant.sse 'data: {"delta":{"type":"input_json_delta","partial_json":"{\"text\": \"a\\n - Shell: zsh\"}"}}' 'shell name'
env_file "G1 .summary.json" plant.summary.json '{"summary": "a\\n - OS Version: Darwin 25.6.0"}' 'OS version'
env_case "R2 G1 escaped backslash then n" 'x\\n - Shell: zsh' 'shell name'
env_case "R2 G1 \\u000a escape" 'x\u000A - Shell: zsh' 'shell name'
env_case "R2 G1 escaped carriage return" 'x\r - OS Version: Darwin 25.6.0' 'OS version'
env_case "R2 G1 carriage return" "x${CR} - Shell: zsh" 'shell name'
# G2: the dashless form with a capital first letter and a host value fails.
env_case "R2 G2 dashless shell path" 'Shell: /opt/homebrew/bin/fish' 'shell name'
env_case "R2 G2 dashless upper case key" 'x\nSHELL: zsh' 'shell name'
env_case "R2 G2 dashless OS value" 'OS Version: macOS 15.1' 'OS version'
env_case "R2 G2 dash form, any value" ' - Shell: anything at all' 'shell name'
# G5: the key's own placeholder, case-sensitive, as the whole value.
env_case "R2 G5 placeholder in upper case" ' - Shell: (OMITTED)' 'shell name'
env_case "R2 G5 placeholder then text" ' - OS Version: (os omitted) Darwin' 'OS version'
env_case "R2 G5 arch placeholder on the OS line" ' - OS Version: (arch omitted)' 'OS version'
env_case "R2 G5 Stainless placeholder then text" '"x-stainless-arch": "(arch omitted)x"' 'Stainless arch'
env_case "R2 G5 Stainless wrong placeholder" '"x-stainless-runtime-version": "(arch omitted)"' 'Stainless runtime version'
env_case "R2 G5 Stainless placeholder in upper case" 'x-stainless-arch: (ARCH OMITTED)' 'Stainless arch'
env_case "R2 G5 CRLF leak" "x\\n - Shell: zsh${CR}" 'shell name'
# G7: the messaging socket pid.
env_case "R2 G7 socket pid" '"/tmp/cc-socks/95709.sock"' 'socket pid'
# G8: quoted values and no-break spaces.
env_case "R2 G8 quoted value" ' - Shell: "zsh"' 'shell name'
env_case "R2 G8 escaped quoted value" 'x\n - Shell: \"zsh\"' 'shell name'
env_case "R2 G8 dashless quoted value" 'Shell: "bash"' 'shell name'
env_case "R2 G8 no-break space" " - Shell:${NBSP}zsh" 'shell name'
env_case "R2 G8 escaped no-break space" ' - OS Version: Darwin 25.6.0' 'OS version'

CASE="PC-13 R2: prose, a YAML key, placeholders in every form and CRLF pass"
D=$(mk pc13r2ok)
printf '%s\n' 'Shell: run make first' 'OS version: check it is 14 or later' '  shell: bash' 'shell: zsh' \
    "x\\n - Shell: (omitted)${CR}" " - OS Version: (os omitted)${CR}" ' - Shell: "(omitted)"' \
    'x\n - Shell: \"(omitted)\"\n - OS Version: \"(os omitted)\"' " - Shell:${NBSP}(omitted)" \
    "x-stainless-arch: (arch omitted)${CR}" '{\"x-stainless-arch\":\"(arch omitted)\"}' \
    '"/tmp/cc-socks/<pid>.sock"' 'Shell: zshrc is read' > "$D/fixtures/claude-code/plant.txt"
run "$D"
expect_ok

# G4: a 1 MiB run of backslashes, then an environment line with no "n", finishes (no time asserted).
CASE="PC-13 R2: G4 a 1 MiB backslash run"
D=$(mk pc13r2long)
awk 'BEGIN { s = "\\"; while (length(s) < 1048576) s = s s; print s " - Shell: zsh" }' > "$D/fixtures/claude-code/plant.txt"
run "$D"
expect_ok

# PC-13 review round 3. H1: the reviewer's Stainless inputs fail; their scrub passes (a fixed point).
env_case "R3 H1 comma-separated header pair" 'x-stainless-arch=arm64, x-stainless-os=MacOS' 'Stainless arch'
env_case "R3 H1 trailing blank" 'x-stainless-arch: arm64 ' 'Stainless arch'
env_case "R3 H1 quoted value with a blank" '"x-stainless-arch": "arm64 extra"' 'Stainless arch'
env_case "R3 H1 runtime version then a comma" 'x-stainless-runtime-version: v26.1.0, node' 'Stainless runtime version'
CASE="PC-13 R3: H1 the scrub of the reviewer's inputs passes"
D=$(mk pc13r3h1)
printf '%s\n' 'x-stainless-arch=arm64, x-stainless-os=MacOS' 'x-stainless-arch: arm64 ' \
    '"x-stainless-arch": "arm64 extra"' 'x-stainless-runtime-version: v26.1.0, node' > "$T/h1.in"
SCRUB_FIXED=1 SCRUB_USER=user SCRUB_HOSTS= python3 -I "$ROOT/scripts/premises/scrub.py" "$T" "$T/h1.in" \
    > "$D/fixtures/claude-code/plant.txt" || fail "scrub.py failed"
grep -qE 'arm64|MacOS|node' "$D/fixtures/claude-code/plant.txt" && fail "the scrub left a value"
run "$D"
expect_ok
# The placeholder then a value, a "," or a blank fails (only a quote, a backslash, CR or the line end
# may follow it).
env_case "R3 H1 placeholder then a value" 'x-stainless-arch: (arch omitted) arm64' 'Stainless arch'
env_case "R3 H1 placeholder then a comma" 'x-stainless-runtime-version: (version omitted), node' 'Stainless runtime version'
env_case "R3 H1 placeholder then a blank" 'x-stainless-arch: (arch omitted) ' 'Stainless arch'
# H3: more dashless host values.
env_case "R3 H3 Windows_NT" 'OS Version: Windows_NT 10.0.26100' 'OS version'
env_case "R3 H3 Windows then a blank" 'OS Version: Windows Server 2022' 'OS version'
env_case "R3 H3 Ubuntu" 'OS Version: Ubuntu 24.04' 'OS version'
env_case "R3 H3 Debian" 'OS Version: Debian 12' 'OS version'
env_case "R3 H3 Fedora" 'OS Version: Fedora 40' 'OS version'
env_case "R3 H3 FreeBSD" 'OS Version: FreeBSD 14.1' 'OS version'
env_case "R3 H3 OpenBSD" 'OS Version: OpenBSD 7.5' 'OS version'
env_case "R3 H3 NixOS" 'OS Version: NixOS 24.05' 'OS version'
env_case "R3 H3 nu" 'Shell: nu' 'shell name'
env_case "R3 H3 mksh" 'Shell: mksh' 'shell name'
env_case "R3 H3 csh" 'Shell: csh' 'shell name'
env_case "R3 H3 xonsh" 'Shell: xonsh' 'shell name'
env_case "R3 H3 linuxbrew path" 'Shell: /home/linuxbrew/.linuxbrew/bin/zsh' 'shell name'
env_case "R3 H3 NixOS system path" 'Shell: /run/current-system/sw/bin/bash' 'shell name'
env_case "R3 H3 Nix store path" 'Shell: /nix/store/0a1b2c3d4e5f6g7h8i9j0k1l2m3n4p5q-zsh-5.9/bin/zsh' 'shell name'
env_case "R3 H3 Windows bash.exe" 'Shell: C:\Program Files\Git\bin\bash.exe' 'shell name'
env_case "R3 H3 Windows pwsh.exe" 'Shell: C:\Program Files\PowerShell\7\pwsh.exe' 'shell name'
env_case "R3 H3 Windows powershell.exe" 'Shell: C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe' 'shell name'
env_case "R3 H3 Windows cmd.exe" 'Shell: C:\Windows\System32\cmd.exe' 'shell name'
env_case "R3 H3 Windows path, escaped backslashes" 'x\nShell: C:\\Program Files\\Git\\bin\\bash.exe\n' 'shell name'
# H5: a socket path with escaped slashes.
env_case "R3 H5 escaped socket path" '"\/tmp\/cc-socks\/95709.sock"' 'socket pid'

CASE="PC-13 R3: prose, an escaped socket placeholder and a non-shell .exe pass"
D=$(mk pc13r3ok)
printf '%s\n' 'OS Version: Ubuntu is fine' 'OS Version: Windowsill' 'Shell: nushell is new' \
    '"\/tmp\/cc-socks\/<pid>.sock"' 'Shell: C:\tools\bash.exe.bak' > "$D/fixtures/claude-code/plant.txt"
run "$D"
expect_ok

# PC-13 review round 1 F4: the rescrub_codex.py tests (same(), the folders and verbatim lists, ENV_LEAKS).
CASE="PC-13 R1: rescrub_test.py"
OUT=$(python3 -I "$ROOT/scripts/premises/rescrub_test.py" 2>&1)
RC=$?
N=$((N + 1))
[ "$RC" -eq 0 ] || fail "exit $RC, expected 0"
case "$(printf '%s\n' "$OUT" | tail -n 1)" in
    'rescrub tests: ok ('*) ;;
    *) fail "last line does not start with 'rescrub tests: ok ('" ;;
esac

echo "test-check-premises: ok ($N cases)"

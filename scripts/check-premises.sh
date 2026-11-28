#!/bin/sh
# Checks docs/premises.md, the premise fixtures and the spec notes (node N2); rows and agent versions are driven by data (K§6.2.5, node PC-1).
# Prints the first failing item and exits 1; exits 0 when all items pass.
# It also scans for forbidden content (N38): fixtures/, the live scripts (scripts/premises/live*.sh),
# scripts/premises/live/, docs/live-checks.md and the spec and conformance lines naming N38.
# N2_FORBIDDEN_NAMES (optional, space-separated, given at run time, never written here): words
# that must not appear as whole words (any case) in that scope or docs/premises.md, such as the
# operator's user name.
set -uf
cd "$(dirname "$0")/.." || exit 1
. scripts/premises/ver.sh   # ver_ge (node PRB-2)

P=docs/premises.md
SPEC=docs/specs/2026-11-18-zygo-design.md
IDS="P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7"
WSPEC=docs/specs/2026-11-20-workspaces-design.md
WIDS="PW6 PW11 PW15 PW1 PW2 PW3 PW4 PW5 PW7 PW9 PW10 PW16"
WSKIP=""

fail() { printf '%s\n' "check-premises: $*"; exit 1; }

# nontrivial <path>: a regular file (not a symlink) of more than 2 bytes that is not {} or [] (the rule
# of K§6.3.7, docs/specs/2026-11-25-client-model-design.md, "fixture:<path>"; a file of only NULs or
# only blanks passes, as it did before TF-3). NUL bytes are deleted with the blanks, as the shell did
# in the command substitution before TF-3, so no shell sees a NUL there (bash >=4.4 warns on stderr).
nontrivial() {
  [ -f "$1" ] && [ ! -L "$1" ] || return 1
  [ "$(wc -c <"$1" | tr -d ' ')" -gt 2 ] || return 1
  case "$(tr -d ' \t\r\n\000' <"$1")" in '{}'|'[]') return 1 ;; esac
  return 0
}

REAL=$(pwd -P)
# in_repo <path>: true when the directory of the repository-relative <path> resolves to the same path
# under the repository's real root (no component is a symlink, '.' or empty, and the directory is
# readable), or does not exist (nontrivial then
# reports the path).
in_repo() {
  edir=$(dirname "$1")
  [ -d "$edir" ] || return 0
  [ "$(cd -P "$REAL/$edir" 2>/dev/null && pwd -P)" = "$REAL/$edir" ]
}

[ -s "$P" ] || fail "$P is missing or empty"
! grep -q "$(printf '\r')" "$P" || fail "$P has CRLF line endings"
[ -s "$SPEC" ] || fail "$SPEC is missing"
grep -q '^## ' "$P" || fail "$P has no '## ' line (the header lines end at the first one)"

# HDR: one "name<TAB>value<TAB>line" line per "<Name>: <value>" header line before the first "## ",
# the value trimmed of blanks and tabs at its ends. Lines in a fenced code block (``` or ~~~, up to
# three blanks before it) or an HTML comment (<!-- to -->) are skipped. A value that still holds a
# tab fails (it would split the record). A row's agent needs a value that is a bare dotted version.
# Here and in every command substitution below whose value feeds a decision, the command reads the
# file itself and tr -d deletes each NUL from its output: the shell did the same in the substitution
# before TF-3, so a NUL changes no result (a blank in its place would: "<NUL>```" would be a fence),
# and no shell sees a NUL there (bash >=4.4 warns on stderr). No tr writes into a reader that stops
# early (awk exit), so with SIGPIPE ignored no tr gets EPIPE (TF-3 round 2). The value of a header
# line that holds a tab gives HDR as that line's number alone; a header name never starts with a digit.
HDR=$(awk '
  { s = $0; sub(/^ ? ? ?/, "", s); f3 = substr(s, 1, 3) }
  fence != "" { if (f3 == fence) fence = ""; next }
  cmt { if (index($0, "-->")) cmt = 0; next }
  f3 == "```" || f3 == "~~~" { fence = f3; next }
  index($0, "<!--") { if (!index(substr($0, index($0, "<!--") + 4), "-->")) cmt = 1; next }
  /^## / { exit }
  /^[A-Za-z][A-Za-z0-9 ._-]*[A-Za-z0-9]:/ {
    i = index($0, ":"); v = substr($0, i + 1); gsub(/^[ \t]+|[ \t]+$/, "", v)
    if (index(v, "\t")) { bad = NR; exit }
    out = out substr($0, 1, i - 1) "\t" v "\t" NR "\n"
  }
  END { if (bad) { print bad; exit } printf "%s", out }' "$P" | tr -d '\000')
case $HDR in [0-9]*) fail "$P:$HDR: the value of a header line holds a tab" ;; esac
# A name given again must have the same value as its first header line. Here and in every awk or
# grep below that reads a pipe, the reader reads to the end and keeps the first match (no exit, no
# grep -q), so with SIGPIPE ignored the writer gets no EPIPE (TF-3 round 3).
dup=$(printf '%s\n' "$HDR" | awk -F'\t' 'd == "" && $1 in v && v[$1] != $2 { d = $3 "\t" $1 ": " $2 "\t" $1 ": " v[$1] } !($1 in v) { v[$1] = $2 } END { if (d != "") print d }')
TAB=$(printf '\t')
SOH=$(printf '\001')
if [ -n "$dup" ]; then
  IFS=$TAB read -r ln h1 h0 <<EOF_DUP
$dup
EOF_DUP
  fail "$P:$ln: header '$h1' differs from the first header line '$h0'"
fi

# version_of <name>: the header value for that name (the first line wins), or nothing.
version_of() { printf '%s\n' "$HDR" | N="$1" awk -F'\t' '!f && $1 == ENVIRON["N"] { v = $2; f = 1 } END { if (f) print v }'; }

# TROWS: "line<TAB>id<TAB>r|o" for every table line of the file (it starts with zero to three
# blanks and "|"; a line with four or more leading blanks or a leading tab is not a table line, as
# Markdown renders it as code), the first cell trimmed at its ends, in file order; r marks a row of
# the Results table (its header row starts "| Id | Item |"; it ends at the first line that is not a
# table line), o any other table line. The Results header and separator rows are left out.
TROWS=$(awk -F'|' '
  !t && /^\| Id \| Item \|/ { t = 1; next }
  t == 1 { t = 2; if ($0 ~ /^ ? ? ?\|[-:| \t]*$/) next }
  t == 2 && $0 !~ /^ ? ? ?\|/ { t = 3 }
  /^ ? ? ?\|/ { id = $2; gsub(/^[ \t]+|[ \t]+$/, "", id); print NR "\t" id "\t" (t == 2 ? "r" : "o") }
' "$P" | tr -d '\000')
# The file has one Results table: a second header row "| Id | Item |" fails.
res2=$(awk '/^\| Id \| Item \|/ { if (n++) { print NR; exit } }' "$P")
[ -z "$res2" ] || fail "$P:$res2: a second Results table header row '| Id | Item |'"
# ROWS: "line<TAB>id" for every row of the Results table.
ROWS=$(printf '%s\n' "$TROWS" | awk -F'\t' '$3 == "r" { print $1 "\t" $2 }')
[ -n "$ROWS" ] || fail "$P has no Results table (a header row '| Id | Item |' and its rows)"
while IFS=$TAB read -r ln id; do
  printf '%s\n' "$id" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9-]*$' ||
    fail "$P:$ln: row id '$id' is empty or not of the form [A-Za-z0-9][A-Za-z0-9-]*"
done <<EOF_ROWS
$ROWS
EOF_ROWS
IDLIST=$(printf '%s\n' "$TROWS" | cut -f2)
# rows_of <id>: how many table lines of the whole file have that id as their first cell.
rows_of() { printf '%s\n' "$IDLIST" | grep -cxF -- "$1"; }
CHECKED=""

trim() { sed 's/^[[:blank:]]*//; s/[[:blank:]]*$//'; }
# cells <from> [<to>]: cells <from> to <to> (default: the last cell) of $row, joined by "|". An
# escaped "\|" is a literal "|" inside a cell, not a cell border.
cells() {
  printf '%s\n' "$row" | awk -F'|' -v a="$1" -v b="${2:-}" '{
    p = "\001"; gsub(/\\[|]/, p); if (b == "") b = NF - 1
    s = ""; for (i = a; i <= b; i++) s = s (i > a ? "|" : "") $i
    gsub(p, "|", s); print s }'
}

# Spec sections: §19 is from "## 19." to "## 19a."; §19a is from "## 19a." to "## 20.".
S19=$(awk '/^## 19\. /{f=1;next} /^## 19a\. /{f=0} f' "$SPEC" | tr -d '\000')
S19A=$(awk '/^## 19a\. /{f=1;next} /^## 20\. /{f=0} f' "$SPEC" | tr -d '\000')
S105=$(awk '/^### 10\.5 /{f=1;next} /^### 10\.6 /{f=0} f' "$SPEC" | tr -d '\000')

for id in $IDS $WIDS; do
  n=$(rows_of "$id")
  [ "$n" -eq 1 ] || fail "$id: expected exactly one row, found $n"
done

# A table line outside the Results table whose first cell has the form of a premise id
# (capital letters, then a digit: P1, P6a, Q7, PW15-bash, PC1) is a Results row in the wrong place.
out=$(printf '%s\n' "$TROWS" | awk -F'\t' 'o == "" && $3 == "o" && $2 ~ /^[A-Z]+[0-9][A-Za-z0-9-]*$/ { o = $1 "\t" $2 } END { if (o != "") print o }')
if [ -n "$out" ]; then
  fail "$P:${out%%"$TAB"*}: row '${out#*"$TAB"}' is outside the Results table"
fi

# Each entry is "line:id"; an id holds no blank and no colon (checked above).
for e in $(printf '%s\n' "$ROWS" | tr '\t' ':'); do
  ln=${e%%:*} id=${e#*:}
  n=$(rows_of "$id")
  [ "$n" -eq 1 ] || fail "$id: expected exactly one row, found $n"
  row=$(sed -n "${ln}p" "$P" | tr -d '\000')
  # cells uses \001 as the placeholder of an escaped "|", so a row that holds one fails closed.
  case $row in *"$SOH"*) fail "$P:$ln: row '$id' holds a SOH byte (\\001), the placeholder of an escaped '|'" ;; esac
  agent=$(cells 4 4 | trim)
  ver=$(cells 5 5 | trim)
  date=$(cells 6 6 | trim)
  result=$(cells 7 7 | trim)
  evidence=$(cells 8)

  printf '%s\n' "$date" | grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' || fail "$id: date '$date' is not YYYY-MM-DD"

  # Agent and Version may each list several values joined by " + ", in the same order.
  [ -n "$agent" ] || fail "$id: the Agent cell is empty"
  [ -n "$ver" ] || fail "$id: the Version cell is empty"
  na=$(printf '%s\n' "$agent" | awk -F' [+] ' '{print NF}')
  nv=$(printf '%s\n' "$ver" | awk -F' [+] ' '{print NF}')
  [ "$na" -eq "$nv" ] || fail "$id: $na agent(s) but $nv version(s)"
  i=1
  while [ "$i" -le "$na" ]; do
    a=$(printf '%s\n' "$agent" | awk -F' [+] ' -v i="$i" '{print $i}' | trim)
    v=$(printf '%s\n' "$ver" | awk -F' [+] ' -v i="$i" '{print $i}' | trim)
    want=$(version_of "$a")
    [ -n "$want" ] || fail "$id: agent '$a' has no header line '$a: <version>' before the first '## ' of $P"
    printf '%s\n' "$want" | grep -Eq '^[0-9]+(\.[0-9]+)*$' ||
      fail "$id: header '$a: $want': header version must be a bare dotted version"
    # The rule is chosen per header name (K§6.2.5): an agent version is a minimum (operator,
    # 2026-11-21); act-lite stays exact; tmux is a minimum with at most one letter after it.
    case "$a" in
      'act-lite') [ "$v" = "$want" ] || fail "$id: version '$v' differs from the header's '$want' for $a" ;;
      'tmux') ver_ge "${v%[a-z]}" "$want" || fail "$id: version '$v' is below the header's '$want' for tmux, or not a dotted number with at most one letter after it" ;;
      *) ver_ge "$v" "$want" || fail "$id: version '$v' is below the header's '$want' for $a, or not a dotted number" ;;
    esac
    i=$((i + 1))
  done

  case "$result" in
    verified|failed) ;;
    *) fail "$id: result '$result' is not verified or failed" ;;
  esac

  paths=$(printf '%s\n' "$evidence" | grep -oE '`(fixtures|docs|scripts)/[^` ]+`' | tr -d '`')
  [ -n "$paths" ] || fail "$id: evidence names no path"
  while IFS= read -r f; do
    case "$f" in /* | .. | ../* | */.. | */../*) fail "$id: evidence path $f is absolute or holds a '..' component" ;; esac
    in_repo "$f" || fail "$id: evidence path $f is not a plain path inside the repository (a symlink, \`.\`/\`//\` part, or unreadable directory)"
    nontrivial "$f" || fail "$id: evidence path $f is missing, not a regular file, or trivial"
  done <<EOF_PATHS
$paths
EOF_PATHS

  case " $IDS " in *" $id "*)
  if [ "$result" = failed ]; then
    pat="\*\(Note [0-9]{4}-[0-9]{2}-[0-9]{2}, N2, $id: [^)]*\)\*"
    case "$id" in
      P*) [ "$(printf '%s\n' "$S19" | grep -Ec "$pat")" -gt 0 ] || fail "$id: failed, but spec §19 has no dated note *(Note YYYY-MM-DD, N2, $id: …)*" ;;
      Q*) [ "$(printf '%s\n' "$S19A" | grep -Ec "$pat")" -gt 0 ] || fail "$id: failed, but spec §19a has no dated note *(Note YYYY-MM-DD, N2, $id: …)*" ;;
    esac
    if [ "$id" = P6a ]; then
      [ "$(printf '%s\n' "$S105" | grep -Ec "$pat")" -gt 0 ] || fail "P6a: failed, but spec §10.5 has no dated note amending the language list"
      nontrivial fixtures/act-lite/README.md || fail "P6a: failed, but fixtures/act-lite/README.md is missing or trivial"
      nontrivial fixtures/act-lite/languages.json || fail "P6a: failed, but fixtures/act-lite/languages.json is missing or trivial"
    fi
  fi ;;
  esac
  case " $WIDS " in *" $id "*)
    if [ -s "$WSPEC" ]; then
      grep -Eq "^\| $id \|.*Result $date: $result([ .;,]|\$)" "$WSPEC" || fail "$id: the W§17 row has no 'Result $date: $result'"
      [ "$result" != failed ] || grep -Eq "\*\(Note $date, [A-Z0-9-]+, $id: [^)]*\)\*" "$WSPEC" ||
        fail "$id: failed, but the workspaces spec has no dated note *(Note $date, <node>, $id: …)*"
    else
      WSKIP=1
    fi ;;
  esac
  CHECKED="${CHECKED:+$CHECKED }$id"
done

for f in fixtures/claude-code/help.txt fixtures/opencode/help.txt \
  fixtures/claude-code/tool-output/Bash.json fixtures/claude-code/tool-output/Read.json \
  fixtures/claude-code/tool-output/Grep.json fixtures/claude-code/tool-output/Glob.json \
  fixtures/claude-code/tool-output/WebFetch.json fixtures/claude-code/settings-schema.json \
  fixtures/opencode/config-precedence.json fixtures/opencode/load-result.json; do
  nontrivial "$f" || fail "required file $f is missing, not a regular file, or trivial"
done
nontrivial fixtures/act-lite/status.json || nontrivial fixtures/act-lite/languages.json ||
  fail "required file fixtures/act-lite/status.json or fixtures/act-lite/languages.json is missing"

# Forbidden content (credentials, machine paths, e-mail addresses, names).
# Scope (N38): fixtures/, the live scripts (scripts/premises/live*.sh), scripts/premises/live/,
# docs/live-checks.md (each when it exists), and the lines of the spec and docs/conformance.md that
# name N38 (NOTES; scan and name_scan read them too). The live scripts' helpers (lib.sh,
# workspace_lib.sh) are not scanned here; tests/forbidden_content.rs (every tracked file) covers them. Every grep
# reads files as text (-a), so a hit in a binary file is a line with its content.
# A path in the scope that holds whitespace would be split and not scanned, and one that holds
# ':' could pass for a grep hit line of another file, so either fails.
SCOPE=fixtures
set +f   # the live*.sh glob needs globbing (set -f at the top)
for f in scripts/premises/live scripts/premises/live*.sh docs/live-checks.md; do
  case $f in *[[:space:]]*) fail "a path in the scan scope holds whitespace: '$f'" ;; esac
  [ -e "$f" ] && SCOPE="$SCOPE $f"
done
set -f
# shellcheck disable=SC2086
bad=$(find $SCOPE -name '*[[:space:]]*' 2>/dev/null | sed -n 1p)
[ -z "$bad" ] || fail "a path in the scan scope holds whitespace: '$bad'"
# shellcheck disable=SC2086
bad=$(find $SCOPE -name '*:*' 2>/dev/null | sed -n 1p)
[ -z "$bad" ] || fail "a path in the scan scope holds ':': $bad"
# grep -r does not follow symlinks on macOS, so a symlink in the scope is neither scanned nor evidence.
# shellcheck disable=SC2086
lnk=$(find $SCOPE -type l 2>/dev/null | sed -n 1p)
[ -z "$lnk" ] || fail "$lnk is a symlink; a symlink in the scan scope is not scanned and is not evidence"
# settings-schema.json is the upstream schema kept verbatim (its source is beside it); so is the
# Codex config.schema.json (from the pinned rust-v0.172.0 tag, K§6.3.7). Their own example texts
# ("//Users/alice/…", "Bearer token") are exempt from the path and bearer rules and from the
# N2_FORBIDDEN_NAMES scan only, and only while each is still the upstream copy: a verbatim file
# that exists must be a regular file whose sha256 equals the value pinned below, and the first
# "sha256: <hex>" line of its source file (the same path with the final .json replaced by
# .source.txt) must give the same value. The pins live in this script so that a new upstream copy
# needs a reviewed change here; a fetch that rewrites the file and its source line together fails.
VERBATIM="fixtures/claude-code/settings-schema.json fixtures/codex/config.schema.json"
# pin_of <verbatim file>: its pinned sha256 (the Codex value is the one PC-2 pins in codex/base.sh).
pin_of() {
  case $1 in
    fixtures/claude-code/settings-schema.json) echo f26f028ad94a1e2a28e9dad1f80832ab196a8e080bd3a7d7a32e6428a15e0611 ;;
    fixtures/codex/config.schema.json) echo 52fb96b1ff79a51bf9ce31695a4a22d4e80f57591d91bad5747b657f4e612bac ;;
  esac
}
# SHA256: shasum -a 256, else sha256sum. sha256_of <file>: its sha256 in lower-case hex.
if command -v shasum >/dev/null 2>&1; then SHA256="shasum -a 256"
elif command -v sha256sum >/dev/null 2>&1; then SHA256=sha256sum
else fail "no sha256 tool (need shasum or sha256sum)"
fi
# shellcheck disable=SC2086
sha256_of() { $SHA256 <"$1" | awk '{ print tolower($1) }'; }
for f in $VERBATIM; do
  [ -e "$f" ] || [ -L "$f" ] || continue
  [ -f "$f" ] && [ ! -L "$f" ] || fail "verbatim file $f is not a regular file"
  src=${f%.json}.source.txt
  [ -f "$src" ] || fail "verbatim file $f has no source file $src"
  pin=$(pin_of "$f")
  want=$(sed -n 's/^sha256:[[:space:]]*\([0-9A-Fa-f]\{64\}\)[[:space:]]*$/\1/p' "$src" | sed -n 1p | tr 'A-F' 'a-f')
  got=$(sha256_of "$f")
  [ "$got" = "$pin" ] && [ "$want" = "$pin" ] ||
    fail "verbatim file $f (sha256 $got) and its source file $src (sha256 ${want:-<none>}) must both give the pinned sha256 $pin"
done
# VX: the ERE ^(<path>|<path>):<digits>: for the list (a grep -n hit line of exactly that file), dots
# escaped. It is always applied case-sensitively, so only the exact path is exempt.
VX="^($(printf '%s' "$VERBATIM" | sed 's/[.]/\\./g; s/ /|/g')):[0-9]+:"
# NOTES: the N38 lines, each NUL deleted, as the shell did before TF-3 (scan and name_scan match on
# NOTES, so a blank in place of a NUL would change a hit: "sk-<NUL>ant-" is "sk-ant-"; TF-3 round 2).
NOTES=$( (grep -anH 'N38' "$SPEC" docs/conformance.md 2>/dev/null | tr -d '\000') || true)
# scan <ERE> <exempt ERE> <label> [i] [<drop sed -E script>]: the first line that matches and is not
# exempt fails; i: the ERE matches any case (the exempt ERE never does). The exempt ERE drops the whole
# hit line, so it is only the VX line prefix (a verbatim file); an exempt form inside a line is a drop
# script, which keeps the rest of the line under the check (TF-5 round 1 F1). grep and sed run with
# LC_ALL=C, byte by byte, so a byte that is not UTF-8 cannot stop BSD sed; a drop sed that fails
# leaves the line as it was, so the line fails (TF-5 round 2 F2). <drop sed -E script>:
# applied to a hit line, which fails only when the ERE still matches the result; the message shows
# the line as written. A NUL in a hit line becomes a blank first (read stops at a NUL); both are
# outside every character class of the rules and of the drop script. After the first hit line the
# loop (also in cx_scan and env_scan) only reads the rest with cat, with no drop script: the commands
# before it get no EPIPE when SIGPIPE is ignored (bash: "printf: write error: Broken pipe"; TF-3 round
# 2), and many hit lines cost no more than one (round 3). read takes one byte at a time from a pipe,
# so cat gets every byte after the hit line.
scan() {
  o=-aE; [ "${4:-}" != i ] || o=-aiE
  # shellcheck disable=SC2086
  hit=$( (LC_ALL=C grep -rn $o -- "$1" $SCOPE 2>/dev/null; printf '%s\n' "$NOTES" | LC_ALL=C grep $o -- "$1") |
    LC_ALL=C grep -av -E -- "${2:-^\$}" | tr '\000' ' ' | while IFS= read -r ln; do
      if [ -n "${5:-}" ]; then
        d=$(printf '%s\n' "$ln" | LC_ALL=C sed -E "$5") || d=$ln
        printf '%s\n' "$d" | LC_ALL=C grep -q $o -- "$1" || continue
      fi
      printf '%s\n' "$ln"
      cat >/dev/null
      break
    done)
  [ -z "$hit" ] || fail "forbidden content ($3): $(printf '%s\n' "$hit" | cut -c1-160)"
}
# L: the left edge of a key or id: line start, a non-word character, or a JSON backslash escape
# (\n, \t, \r, \b, \f, \uXXXX) right before it in nested JSON text.
L='(^|[^A-Za-z0-9_]|\\[nrtbf]|\\u[0-9A-Fa-f]{4})'
# XK: an x-api-key name and its separator, each quote bare, backslash-escaped once or more (JSON in
# JSON: \\\"; TF-5 round 2) or absent. The name is spelled in any case with bracket classes, because
# sed -E (the drop script XKX) is case-exact. The rule's value starts with any number of
# backslashes and then a character that is not a quote, a backslash or a blank, so "\abc" is a value
# and an escaped empty string (\"\") is not (TF-5 round 2).
XK='[xX]-[aA][pP][iI]-[kK][eE][yY](\\*["'"'"'])?[[:space:]]*[:=][[:space:]]*(\\*["'"'"'])?'
# XKX: drops each omitted x-api-key value, "<XK>[omitted]" followed by a value end of scrub.py (a
# blank, ", ', "," or \; kept) or the line end, so a hit line fails when any other value is left on
# it, in any case (TF-5 round 1 F1, round 2 F1). An exempt ERE would drop the whole line, and a
# value beside it.
XKX='s/('"$XK"')\[omitted\]([[:space:]"'"'"',\\]|$)/\4/g'
scan 'sk-ant-' '' 'API key'
scan 'Bearer [A-Za-z0-9]' "$VX" 'bearer token'
scan "$XK"'\\*[^"'"'"'\\[:space:]]' '' 'x-api-key value' i "$XKX"
scan '/Users/' "$VX" 'macOS home path'
scan '/private/var/' '' 'temporary path'
scan '/var/folders/' '' 'temporary path'
# An address at exactly the reserved domain example.invalid (RFC 2606; the live scripts' git test
# identity) is not a leak (K§11 item 10). Only "<local>@example.invalid" is dropped: <local> is
# [A-Za-z0-9._-]+ after the line start or a character outside [A-Za-z0-9._%!+@-] (so a percent hack
# or bang path such as bob%corp.com@… or corp.com!bob@… is not exempt), and the domain is followed
# by the line end or a character outside [A-Za-z0-9.@-]. The drop repeats until nothing matches
# (each drop takes the character after it, which may start the next address).
MAILX=$(printf '%s\n' ':a' 's/(^|[^A-Za-z0-9._%!+@-])[A-Za-z0-9._-]+@example\.invalid([^A-Za-z0-9.@-]|$)/\1\2/' 'ta')
scan '[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}' '' 'e-mail address' '' "$MAILX"
scan 'sk-ant-o' '' 'claude.ai login token'
scan "${L}sk-[A-Za-z0-9_-]{20,}" '' 'API key'
scan 'sk-(proj|svcacct|admin)-' '' 'OpenAI key'
scan 'sk-[A-Za-z0-9]*\*{3,}' '' 'masked key'
scan "${L}org-[A-Za-z0-9]{8,}" '' 'organization id'
scan "${L}proj_[A-Za-z0-9]{8,}" '' 'project id'
scan 'zyg_[A-Za-z0-9]' '' 'zygo token'
scan '[A-Za-z]:(\\\\|\\|/)Users(\\\\|\\|/)' "$VX" 'Windows home path' i
scan "${L}gh[pousr]_[A-Za-z0-9]{20,}" '' 'GitHub token'
scan 'github_pat_[A-Za-z0-9_]{20,}' '' 'GitHub token'
scan 'eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.' '' 'JWT'
# cx_scan <ERE> <label> [i] [<drop sed -E script>]: as scan, over fixtures/codex only (PC-11 leak
# classes of Codex captures); the verbatim files are exempt. As in scan, grep and sed run with
# LC_ALL=C and a drop sed that fails leaves the line a hit (TF-5 round 2).
cx_scan() {
  o=-aE; [ "${3:-}" != i ] || o=-aiE
  # shellcheck disable=SC2086
  hit=$(LC_ALL=C grep -rn $o -- "$1" fixtures/codex 2>/dev/null | LC_ALL=C grep -av -E -- "$VX" | tr '\000' ' ' |
    while IFS= read -r ln; do
      if [ -n "${4:-}" ]; then
        d=$(printf '%s\n' "$ln" | LC_ALL=C sed -E "$4") || d=$ln
        printf '%s\n' "$d" | LC_ALL=C grep -q $o -- "$1" || continue
      fi
      printf '%s\n' "$ln"
      cat >/dev/null
      break
    done)
  [ -z "$hit" ] || fail "forbidden content ($2): $(printf '%s\n' "$hit" | cut -c1-160)"
}
# TAG <shell> and ECX <environment_context> in their three bracket forms (plain, <..>, &lt;..&gt;).
# The shell rules have the scope of scrub.py (review round 2 G1): the <shell> tag after an
# environment_context opening on the same line (a captured block is one JSON string line;
# rescrub_codex.py --check also reads blocks over several lines), and a login shell path at the start
# of an exec "command" or "cmd" field. Prose that names a shell passes.
TAG='(<|\\u003[cC]|&lt;)shell(>|\\u003[eE]|&gt;)'
ECX='(<|\\u003[cC]|&lt;)environment_context(>|\\u003[eE]|&gt;)'
CMD='\\?"(command|cmd)\\?"[[:space:]]*:[[:space:]]*\\?"(/[^[:space:]"'"'"'\\/]+)*/(zsh|bash|fish|sh|dash|ksh|mksh|tcsh|csh) -lc'
# /private/tmp followed by "/" or the end of the path (as scrub.py); seatbelt as a JSON key or value.
cx_scan '/private/tmp([^A-Za-z0-9_.-]|$)' 'macOS temporary path'
cx_scan '\\?"seatbelt\\?"' 'macOS sandbox name' i
cx_scan "$ECX.*$TAG" 'login shell tag' '' "s/$TAG\\(omitted\\)//g"
cx_scan "$CMD" 'login shell command'
cx_scan '\\?"platform(Os|Family)\\?"[[:space:]]*:[[:space:]]*\\?"' 'OS name' '' 's/\\?"platform(Os|Family)\\?"[[:space:]]*:[[:space:]]*\\?"\(os omitted\)//g'
cx_scan '(msg|fco|ctco)_[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}' 'Codex item id' i
cx_scan '__CF' 'macOS env name'
cx_scan 'marketplace-(macos|linux|windows)-[A-Za-z0-9_-]' 'marketplace OS name'
# A time-bearing ULID after a lower-case prefix (resp_, msg_, fc_; scrub.py ULID); a placeholder of
# time 0 is dropped. A bare ULID has no rule (Codex ids carry a prefix; ordinary hashes would match).
cx_scan "${L}[a-z]{1,8}_[0-7][0-9A-HJKMNP-TV-Z]{25}([^0-9A-Za-z]|\$)" 'time-bearing id' '' 's/[a-z]{1,8}_0{10}[0-9A-HJKMNP-TV-Z]{16}//g'
# env_scan <ERE> <label> [<drop sed -E script>]: as cx_scan, case-sensitive, over fixtures/claude-code
# and fixtures/opencode (PC-13). The drop script is applied to the hit's text (without its "path:line:"
# prefix, so ^ is the line start); the hit fails only when the ERE still matches the result. grep and
# sed run with LC_ALL=C, byte by byte, so the time stays linear on long lines (review round 2 G4). A
# drop sed that fails leaves the text a hit (TF-5 round 2). The verbatim files are exempt.
env_scan() {
  hit=$(LC_ALL=C grep -rn -aE -- "$1" fixtures/claude-code fixtures/opencode 2>/dev/null | LC_ALL=C grep -av -E -- "$VX" | tr '\000' ' ' |
    while IFS= read -r ln; do
      if [ -n "${3:-}" ]; then
        d=$(printf '%s\n' "${ln#*:*:}" | LC_ALL=C sed -E "$3") || d=${ln#*:*:}
        printf '%s\n' "$d" | LC_ALL=C grep -qaE -- "$1" || continue
      fi
      printf '%s\n' "$ln"
      cat >/dev/null
      break
    done)
  [ -z "$hit" ] || fail "forbidden content ($2): $(printf '%s\n' "$hit" | cut -c1-160)"
}
# ci <text>: an ERE for the text in any case ([Ss][Hh]...); the text holds no ERE syntax but . / ( ) | -.
ci() {
  printf '%s' "$1" | awk '{ for (i = 1; i <= length($0); i++) { c = substr($0, i, 1); u = toupper(c); l = tolower(c)
    printf "%s", (u != l ? "[" u l "]" : c) } }'
}
CR=$(printf '\r')
NBSP=$(printf '\302\240')
# The Claude Code environment block lines, the forms of scrub.py ENV_SHELL and ENV_OS. A line start
# (ENL, review round 2 G1, fail closed): the line start, a carriage return, one or more backslashes
# then n or r, or a \u000a or \u000d escape. Blanks or escaped tabs may follow it (EIN). After the
# colon: blanks, escaped tabs, no-break spaces (G8; EWS), and a quote, plain or \"-escaped (EQ). The
# dash form, the key in any case, fails for any value but the key's own placeholder as the whole value
# (then a quote, a backslash, a CR or the line end; F6, G5); the dashless form only with a capital
# first letter and a host value (G2): a known shell name or its path, or Darwin, Linux, Windows or
# macOS then a version digit.
ENL='(^|'"$CR"'|\\+[nr]|\\[uU]000[aAdD])'
EIN='([[:blank:]]|\\t)*'
EWS='([[:blank:]]|\\t|'"$NBSP"'|\\u00[aA]0)*'
EQ='(\\?["'"'"'])?'
EEND='(["'"'"'\\'"$CR"']|$)'
ESH=$(ci shell)
EOS=$(ci 'os version')
# Review round 3 H3: the host values of scrub.py SHELL_VAL and OS_VAL (Linuxbrew, NixOS and Nix store
# shell paths, a Windows path to a shell .exe, plain or with escaped backslashes; Windows_NT, "Windows "
# and Linux and BSD distributions).
SHD="$(ci '((/usr(/local)?)?/bin/|/opt/homebrew/bin/|/home/linuxbrew/\.linuxbrew/bin/|/run/current-system/sw/bin/|/nix/store/')"'[0-9A-Za-z]+-[^/[:space:]"'"'"'\\]+/'"$(ci 'bin/)')"
SHW='[A-Za-z]:(\\{1,2}[^\\"'"'$CR"']+)*\\{1,2}'"$(ci '(bash|pwsh|powershell|cmd)')"'\.[Ee][Xx][Ee]'
SHV="($SHD?$(ci '(zsh|bash|fish|sh|tcsh|csh|ksh|mksh|dash|nu|xonsh|pwsh|powershell|cmd)(\.exe)?')|$SHW)"'([^A-Za-z0-9_./-]|$)'
OSV="($(ci '(darwin|linux|windows|macos|ubuntu|debian|fedora|freebsd|openbsd|nixos)')[[:blank:]]*[0-9]|$(ci windows)[_ ])"
EVAL='[^\\"'"'"'[:space:]]'
# The drop keeps the character after the placeholder (group 3: EWS and EQ are groups 1 and 2), which
# may start the next line.
env_scan "$ENL$EIN-$EWS$ESH:$EWS$EQ$EVAL" 'shell name' "s/$ESH:$EWS$EQ\\(omitted\\)$EEND/\\3/g"
env_scan "${ENL}${EIN}S$(ci hell):$EWS$EQ$SHV" 'shell name'
env_scan "$ENL$EIN-$EWS$EOS:$EWS$EQ$EVAL" 'OS version' "s/$EOS:$EWS$EQ\\(os omitted\\)$EEND/\\3/g"
env_scan "${ENL}${EIN}O$(ci 's version'):$EWS$EQ$OSV" 'OS version'
# The Stainless SDK's arch and runtime version headers (scrub.py STAINLESS_RE; F3), any case; only the
# key's own placeholder as the whole value is dropped (G5; group 3: XSA and XSR hold groups 1 and 2).
# x-stainless-os, the OS family, is kept.
XSA="$(ci x-stainless-arch)"'(\\?["'"'"'])?[[:space:]]*[:=][[:space:]]*(\\?["'"'"'])?'
XSR="$(ci x-stainless-runtime-version)"'(\\?["'"'"'])?[[:space:]]*[:=][[:space:]]*(\\?["'"'"'])?'
XSV='[^"'"'"'\\[:space:],]'
env_scan "$XSA$XSV" 'Stainless arch' "s/$XSA"'\(arch omitted\)'"$EEND/\\3/g"
env_scan "$XSR$XSV" 'Stainless runtime version' "s/$XSR"'\(version omitted\)'"$EEND/\\3/g"
# The pid of a Claude Code messaging socket (scrub.py CC_SOCK; review round 2 G7; round 3 H5: also
# with escaped slashes).
env_scan '/cc-socks\\*/[0-9]+\.sock' 'socket pid'

# name_scan <label> <name> [<word characters>] [<exempt ERE>]: the name as a whole word (any case;
# it may hold spaces), also right after a JSON backslash escape. Word characters: A-Za-z0-9_ unless
# given (host names: A-Za-z0-9_-, as in scrub.py). A hit line ("path:line:text") that matches the
# exempt ERE is dropped, as in scan. The message names the file and line, never the name. A NUL in a
# hit line becomes a blank first, as in scan, so no shell sees a NUL in the command substitution
# (bash >=4.4 warns on stderr; TF-3); grep matched the raw bytes, and the message keeps only the
# "path:line" part, so the blank changes no result. sed -n 1p, not head, keeps the first hit line: it
# reads to the end, so with SIGPIPE ignored tr and grep get no EPIPE (TF-3 round 2).
name_scan() {
  e=$(printf '%s' "$2" | sed 's/[][\.*^$+?(){}|/]/\\&/g')
  wc=${3:-A-Za-z0-9_}
  rx="(^|[^$wc]|\\\\[nrtbf])$e([^$wc]|\$)"
  # shellcheck disable=SC2086
  hit=$( (grep -rnaiE -- "$rx" $SCOPE "$P" 2>/dev/null; printf '%s\n' "$NOTES" | grep -aiE -- "$rx") | grep -av -E -- "${4:-^\$}" | tr '\000' ' ' | sed -n 1p)
  [ -z "$hit" ] || fail "$1: $(printf '%s\n' "$hit" | cut -d: -f1-2)"
}
for w in ${N2_FORBIDDEN_NAMES:-}; do name_scan "forbidden name from N2_FORBIDDEN_NAMES" "$w" A-Za-z0-9_ "$VX"; done
# The machine's host names (from the OS at run time, full and short): hostname, and on macOS
# scutil LocalHostName and ComputerName. A name shorter than 4 characters or localhost is skipped.
host_names() {
  { hostname 2>/dev/null || true
    if command -v scutil >/dev/null 2>&1; then
      scutil --get LocalHostName 2>/dev/null || true
      scutil --get ComputerName 2>/dev/null || true
    fi
  } | while IFS= read -r h; do [ -z "$h" ] || printf '%s\n%s\n' "$h" "${h%%.*}"; done | sort -u
}
while IFS= read -r h; do
  [ -n "$h" ] || continue
  if [ ${#h} -lt 4 ] || [ "$(printf '%s' "$h" | tr 'A-Z' 'a-z')" = localhost ]; then
    echo "check-premises: note: a host name is shorter than 4 characters or localhost; host-name scan skipped for it"
    continue
  fi
  name_scan "host name" "$h" A-Za-z0-9_-
done <<EOF_HOSTS
$(host_names)
EOF_HOSTS
[ -n "${N2_FORBIDDEN_NAMES:-}" ] || echo "check-premises: note: N2_FORBIDDEN_NAMES not set; name scan skipped"

[ -z "$WSKIP" ] || echo "check-premises: note: $WSPEC not found; W§17 result check skipped"
printf '%s\n' "check-premises: ok ($CHECKED)"
exit 0

#!/bin/sh
# Checks docs/premises.md, the premise fixtures and the spec notes (node N2).
# Prints the first failing item and exits 1; exits 0 when all items pass.
# N2_FORBIDDEN_NAMES (optional, space-separated, given at run time, never written here): words
# that must not appear as whole words (any case) in fixtures/ or docs/premises.md, such as the
# operator's user name.
set -uf
cd "$(dirname "$0")/.." || exit 1
. scripts/premises/ver.sh   # ver_ge (node PRB-2)

P=docs/premises.md
SPEC=docs/specs/2026-11-18-zygo-design.md
IDS="P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7"

fail() { echo "check-premises: $*"; exit 1; }

# nontrivial <path>: a regular file (not a symlink) of more than 2 bytes that is not {} or [].
nontrivial() {
  [ -f "$1" ] && [ ! -L "$1" ] || return 1
  [ "$(wc -c <"$1" | tr -d ' ')" -gt 2 ] || return 1
  case "$(tr -d ' \t\r\n' <"$1")" in '{}'|'[]') return 1 ;; esac
  return 0
}

[ -s "$P" ] || fail "$P is missing or empty"
[ -s "$SPEC" ] || fail "$SPEC is missing"

header() { sed -n "s/^$1: *\(.*[^ ]\) *\$/\1/p" "$P" | head -n 1; }
V_CC=$(header 'Claude Code')
V_OC=$(header 'OpenCode')
V_AL=$(header 'act-lite')
[ -n "$V_CC" ] || fail "header line 'Claude Code: <version>' missing"
[ -n "$V_OC" ] || fail "header line 'OpenCode: <version>' missing"
[ -n "$V_AL" ] || fail "header line 'act-lite: <version>' missing"

version_of() {
  case "$1" in
    'Claude Code') echo "$V_CC" ;;
    'OpenCode') echo "$V_OC" ;;
    'act-lite') echo "$V_AL" ;;
    *) echo "" ;;
  esac
}

trim() { sed 's/^ *//; s/ *$//'; }

# Spec sections: §19 is from "## 19." to "## 19a."; §19a is from "## 19a." to "## 20.".
S19=$(awk '/^## 19\. /{f=1;next} /^## 19a\. /{f=0} f' "$SPEC")
S19A=$(awk '/^## 19a\. /{f=1;next} /^## 20\. /{f=0} f' "$SPEC")
S105=$(awk '/^### 10\.5 /{f=1;next} /^### 10\.6 /{f=0} f' "$SPEC")

for id in $IDS; do
  n=$(grep -c "^| $id |" "$P")
  [ "$n" -eq 1 ] || fail "$id: expected exactly one row, found $n"
  row=$(grep "^| $id |" "$P")
  agent=$(echo "$row" | awk -F'|' '{print $4}' | trim)
  ver=$(echo "$row" | awk -F'|' '{print $5}' | trim)
  date=$(echo "$row" | awk -F'|' '{print $6}' | trim)
  result=$(echo "$row" | awk -F'|' '{print $7}' | trim)
  evidence=$(echo "$row" | awk -F'|' '{for(i=8;i<NF;i++) printf "%s|", $i}')

  echo "$date" | grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' || fail "$id: date '$date' is not YYYY-MM-DD"

  # Agent and Version may each list several values joined by " + ", in the same order.
  na=$(echo "$agent" | awk -F' [+] ' '{print NF}')
  nv=$(echo "$ver" | awk -F' [+] ' '{print NF}')
  [ "$na" -eq "$nv" ] || fail "$id: $na agent(s) but $nv version(s)"
  i=1
  while [ "$i" -le "$na" ]; do
    a=$(echo "$agent" | awk -F' [+] ' -v i="$i" '{print $i}' | trim)
    v=$(echo "$ver" | awk -F' [+] ' -v i="$i" '{print $i}' | trim)
    want=$(version_of "$a")
    [ -n "$want" ] || fail "$id: unknown agent '$a'"
    # A pinned agent version is a minimum (operator, 2026-11-21); act-lite stays exact.
    case "$a" in
      'Claude Code' | 'OpenCode') ver_ge "$v" "$want" || fail "$id: version '$v' is below the header's '$want' for $a, or not a dotted number" ;;
      *) [ "$v" = "$want" ] || fail "$id: version '$v' differs from the header's '$want' for $a" ;;
    esac
    i=$((i + 1))
  done

  case "$result" in
    verified|failed) ;;
    *) fail "$id: result '$result' is not verified or failed" ;;
  esac

  paths=$(echo "$evidence" | grep -oE '`(fixtures|docs|scripts)/[^` ]+`' | tr -d '`')
  [ -n "$paths" ] || fail "$id: evidence names no path"
  while IFS= read -r f; do
    nontrivial "$f" || fail "$id: evidence path $f is missing, not a regular file, or trivial"
  done <<EOF_PATHS
$paths
EOF_PATHS

  if [ "$result" = failed ]; then
    pat="\*\(Note [0-9]{4}-[0-9]{2}-[0-9]{2}, N2, $id: [^)]*\)\*"
    case "$id" in
      P*) echo "$S19" | grep -Eq "$pat" || fail "$id: failed, but spec §19 has no dated note *(Note YYYY-MM-DD, N2, $id: …)*" ;;
      Q*) echo "$S19A" | grep -Eq "$pat" || fail "$id: failed, but spec §19a has no dated note *(Note YYYY-MM-DD, N2, $id: …)*" ;;
    esac
    if [ "$id" = P6a ]; then
      echo "$S105" | grep -Eq "$pat" || fail "P6a: failed, but spec §10.5 has no dated note amending the language list"
      nontrivial fixtures/act-lite/README.md || fail "P6a: failed, but fixtures/act-lite/README.md is missing or trivial"
      nontrivial fixtures/act-lite/languages.json || fail "P6a: failed, but fixtures/act-lite/languages.json is missing or trivial"
    fi
  fi
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

# Forbidden content under fixtures/: credentials, machine paths, e-mail addresses.
# settings-schema.json is the upstream schema kept verbatim (its source is beside it); its own
# example texts ("//Users/alice/…", "Bearer token") are exempt from the path and bearer rules only.
VERBATIM=fixtures/claude-code/settings-schema.json
# Scope (N38): fixtures, the live scripts and their helpers, docs/live-checks.md (each when it
# exists); scan_notes applies the same patterns to the spec and conformance lines naming N38.
SCOPE=fixtures
for f in scripts/premises/live scripts/premises/live*.sh docs/live-checks.md; do
  [ -e "$f" ] && SCOPE="$SCOPE $f"
done
NOTES=$( (grep -nH 'N38' "$SPEC" docs/conformance.md 2>/dev/null) || true)
# scan <ERE> <exempt ERE> <label> [i]: the first line that matches and is not exempt fails; i: any case.
scan() {
  o=-E; [ "${4:-}" != i ] || o=-iE
  # shellcheck disable=SC2086
  hit=$( (grep -rn $o -- "$1" $SCOPE 2>/dev/null; echo "$NOTES" | grep $o -- "$1") | grep -v $o -- "${2:-^\$}" | head -n 1)
  [ -z "$hit" ] || fail "forbidden content ($3): $(printf '%s\n' "$hit" | head -n 1 | cut -c1-160)"
}
# L: the left edge of a key or id: line start, a non-word character, or a JSON backslash escape
# (\n, \t, \r, \b, \f, \uXXXX) right before it in nested JSON text.
L='(^|[^A-Za-z0-9_]|\\[nrtbf]|\\u[0-9A-Fa-f]{4})'
# XK: an x-api-key name and its separator, quoted, backslash-quoted or bare.
XK='x-api-key(\\?["'"'"'])?[[:space:]]*[:=][[:space:]]*(\\?["'"'"'])?'
scan 'sk-ant-' '' 'API key'
scan 'Bearer [A-Za-z0-9]' "^$VERBATIM:" 'bearer token'
scan "$XK"'[^"'"'"'\\[:space:]]' "$XK"'\[omitted\]' 'x-api-key value' i
scan '/Users/' "^$VERBATIM:" 'macOS home path'
scan '/private/var/' '' 'temporary path'
scan '/var/folders/' '' 'temporary path'
scan '[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}' '' 'e-mail address'
scan 'sk-ant-o' '' 'claude.ai login token'
scan "${L}sk-[A-Za-z0-9_-]{20,}" '' 'API key'
scan 'sk-(proj|svcacct|admin)-' '' 'OpenAI key'
scan 'sk-[A-Za-z0-9]*\*{3,}' '' 'masked key'
scan "${L}org-[A-Za-z0-9]{8,}" '' 'organization id'
scan "${L}proj_[A-Za-z0-9]{8,}" '' 'project id'
scan 'zyg_[A-Za-z0-9]' '' 'zygo token'
scan '[A-Za-z]:(\\\\|\\|/)Users(\\\\|\\|/)' "^$VERBATIM:" 'Windows home path' i
scan "${L}gh[pousr]_[A-Za-z0-9]{20,}" '' 'GitHub token'
scan 'github_pat_[A-Za-z0-9_]{20,}' '' 'GitHub token'
scan 'eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.' '' 'JWT'

# name_scan <label> <name> [<word characters>]: the name as a whole word (any case; it may hold
# spaces), also right after a JSON backslash escape. Word characters: A-Za-z0-9_ unless given (host
# names: A-Za-z0-9_-, as in scrub.py). The message names the file and line, never the name.
name_scan() {
  e=$(printf '%s' "$2" | sed 's/[][\.*^$+?(){}|/]/\\&/g')
  wc=${3:-A-Za-z0-9_}
  rx="(^|[^$wc]|\\\\[nrtbf])$e([^$wc]|\$)"
  # shellcheck disable=SC2086
  hit=$( (grep -rniE -- "$rx" $SCOPE "$P" 2>/dev/null; echo "$NOTES" | grep -iE -- "$rx") | head -n 1)
  [ -z "$hit" ] || fail "$1: $(printf '%s\n' "$hit" | head -n 1 | cut -d: -f1-2)"
}
for w in ${N2_FORBIDDEN_NAMES:-}; do name_scan "forbidden name from N2_FORBIDDEN_NAMES" "$w"; done
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

echo "check-premises: ok (${IDS})"
exit 0

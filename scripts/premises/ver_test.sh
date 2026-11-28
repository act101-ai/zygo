#!/bin/sh
# Tests of ver_ge (scripts/premises/ver.sh), the agent version rule of pinned_agents
# (scripts/premises/live.sh) and the agent row rule of scripts/check-premises.sh (node PRB-2:
# a pinned agent version is a minimum; act-lite stays exact), and its SP rows and `git:` header
# (node MS-25).
# Usage: sh scripts/premises/ver_test.sh   ->  "ver tests: ok (<n>)", exit 0
# No agent, no network; not run by CI. Stub claude and opencode programs print fixed text.
set -u
unset N2_SCRATCH N2_SCRATCH_OWN
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
SH=$(command -v dash || command -v sh)
T=$(mktemp -d)
trap 'chmod -R u+rwx "$T"; rm -rf "$T"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
n=0 bad=0
ok() { n=$((n + 1)); printf 'ok: %s\n' "$1"; }
no() { n=$((n + 1)); bad=$((bad + 1)); printf 'FAIL: %s (%s)\n' "$1" "$2"; }
NL='
'

# --- V: ver_ge exit status --------------------------------------------------------------------
# vcase <case> <want 0|1> <have> <min>
vcase() {
  name="$1 ver_ge '$(printf '%s' "$3" | tr '\n' '|')' '$4' -> $2"
  if [ ! -f "$HERE/ver.sh" ]; then no "$name" "ver.sh missing"; return; fi
  ( . "$HERE/ver.sh"; ver_ge "$3" "$4" )
  rc=$?
  if [ "$rc" -eq "$2" ]; then ok "$name"; else no "$name" "exit $rc"; fi
}
vcase V1 0 2.1.330 2.1.328
vcase V2 0 2.1.328 2.1.328
vcase V3 0 2.2 2.1.328
vcase V4 0 2.1.0 2.1
vcase V5 0 2.1 2.1.0
vcase V6 0 2.1.1000 2.1.328
vcase V7 0 10.0.0 9.9.9
vcase V8 0 1.18.16 1.18.16
vcase V9 0 2.01 2.1
vcase V10 0 2.1 2.01
vcase V11 0 9007199254740993 9007199254740992
vcase V12 1 2.1.327 2.1.328
vcase V13 1 1.15.7 1.18.16
vcase V14 1 1.9.0 1.18.16
vcase V15 1 "" 2.1.328
vcase V16 1 "2.1.330 (Claude Code)" 2.1.328
vcase V17 1 v2.1.330 2.1.328
vcase V18 1 2..1 2.1
vcase V19 1 2.1.330 ""
vcase V20 1 "2.1.330${NL}2.1.331" 2.1.328
vcase V21 1 2.1.99999999999999999 2.1.100000000000000000
vcase V22 1 9007199254740992 9007199254740993
vcase V23 1 .1 1
vcase V24 1 1. 1
vcase V25 1 1..2 1
# V26-V28 (round 2): a 32-digit field compares; a longer field fails closed, and at once.
D32=12345678901234567890123456789012
vcase V26 0 "$D32" "$D32"
vcase V27 1 "1${D32}" 1
BIG=$(printf '%20000s' '' | tr ' ' 1)
name="V28 ver_ge <20000-digit field> <same> -> 1 within 2 s"
if [ ! -f "$HERE/ver.sh" ]; then no "$name" "ver.sh missing"
else
  rm -f "$T/v26.rc"
  ( ( . "$HERE/ver.sh"; ver_ge "$BIG" "$BIG" ); echo $? >"$T/v26.rc" ) &
  bp=$!
  i=0
  while [ ! -s "$T/v26.rc" ] && [ "$i" -lt 20 ]; do i=$((i + 1)); sleep 0.1; done
  if [ ! -s "$T/v26.rc" ]; then
    pkill -KILL -P "$bp" 2>/dev/null; kill -s KILL "$bp" 2>/dev/null; wait "$bp" 2>/dev/null
    no "$name" "not done in 2 s"
  elif [ "$(cat "$T/v26.rc")" = 1 ]; then ok "$name"
  else no "$name" "exit $(cat "$T/v26.rc")"; fi
fi

# --- X: ver_ge changes no variable of the caller; sourcing ver.sh needs nothing on PATH ----------
if [ ! -f "$HERE/ver.sh" ]; then
  no "X1 ver_ge keeps the caller's short variables" "ver.sh missing"
  no "X2 ver.sh sources with an empty PATH" "ver.sh missing"
else
  out=$( . "$HERE/ver.sh"; a=keep b=keep c=keep d=keep i=keep m=keep n=keep p=keep q=keep v=keep x=keep
    ver_ge 2.1.330 2.1.328; ver_ge 2.01.9 2.1.10 || true
    echo "$a $b $c $d $i $m $n $p $q $v $x" )
  want="keep keep keep keep keep keep keep keep keep keep keep"
  if [ "$out" = "$want" ]; then ok "X1 ver_ge keeps the caller's short variables"
  else no "X1 ver_ge keeps the caller's short variables" "got: $out"; fi
  out=$(env PATH= "$SH" -c ". '$HERE/ver.sh'; command -v ver_ge >/dev/null && echo ok" 2>&1)
  rc=$?
  if [ "$rc" -eq 0 ] && [ "$out" = ok ]; then ok "X2 ver.sh sources with an empty PATH"
  else no "X2 ver.sh sources with an empty PATH" "exit $rc, output: $out"; fi
fi

# --- P: pinned_agents --------------------------------------------------------------------------
PREMISES_DIR=$HERE REPO_DIR=$REPO N2_SCRATCH=$T/s
CLAUDE_BIN=$T/claude OPENCODE_BIN=$T/opencode
mkdir -p "$N2_SCRATCH"
printf '#!/bin/sh\ncat '"'"'%s/cc.txt'"'"'\n' "$T" >"$CLAUDE_BIN"
printf '#!/bin/sh\ncat '"'"'%s/oc.txt'"'"'\n' "$T" >"$OPENCODE_BIN"
chmod +x "$CLAUDE_BIN" "$OPENCODE_BIN"
pdefaults() { printf '%s\n' '2.1.330 (Claude Code)' >"$T/cc.txt"; printf '%s\n' '1.18.16' >"$T/oc.txt"; }
# prun: run pinned_agents with cc.txt and oc.txt as they are; sets rc, output in $T/p.out.
prun() {
  ( . "$HERE/live.sh"; pinned_agents; echo "$LIVE_VER $LIVE_CC_VER $LIVE_OC_VER"
    sh -c 'echo "$LIVE_CC_VER $LIVE_OC_VER"' ) >"$T/p.out" 2>&1
  rc=$?
}
# pcheck <name> <want rc> <fixed text the output must hold | =first line exactly>
pcheck() {
  if [ "$rc" -ne "$2" ]; then no "$1" "exit $rc: $(head -n 3 "$T/p.out" | tr '\n' '|')"; return 1; fi
  case "$3" in
    =*) l=$(head -n 1 "$T/p.out")
      [ "$l" = "${3#=}" ] || { no "$1" "first line: $l"; return 1; } ;;
    *) grep -qF -- "$3" "$T/p.out" || { no "$1" "output lacks: $3; has: $(head -n 3 "$T/p.out" | tr '\n' '|')"; return 1; } ;;
  esac
  return 0
}
pdefaults; prun
if pcheck "P1 at or above both pins, versions exported" 0 "=2.1.330 2.1.330 1.18.16"; then
  if [ "$(cat "$T/p.out")" = "2.1.330 2.1.330 1.18.16${NL}2.1.330 1.18.16" ]; then ok "P1 at or above both pins, versions exported"
  else no "P1 at or above both pins, versions exported" "output: $(tr '\n' '|' <"$T/p.out")"; fi
fi

pdefaults; printf '%s\n' '2.1.328 (Claude Code)' >"$T/cc.txt"; printf '%s\n' '1.18.20 (OpenCode)' >"$T/oc.txt"; prun
pcheck "P2 the pin and a later opencode with a suffix word" 0 "=2.1.328 2.1.328 1.18.20" && ok "P2 the pin and a later opencode with a suffix word"

pdefaults; printf '%s\n' '2.1.327 (Claude Code)' >"$T/cc.txt"; prun
pcheck "P3 claude below the pin" 1 "live: claude is 2.1.327 (Claude Code); the minimum is 2.1.328" && ok "P3 claude below the pin"

pdefaults; printf '%s\n' '1.15.7' >"$T/oc.txt"; prun
pcheck "P4 opencode below the pin" 1 "live: opencode is 1.15.7; the minimum is 1.18.16" && ok "P4 opencode below the pin"

pdefaults; : >"$T/cc.txt"; prun
pcheck "P5 empty claude output" 1 "the minimum is 2.1.328" && ok "P5 empty claude output"

pdefaults; printf '%s\n' '2.1.328 (Claude Code)' 'extra' >"$T/cc.txt"; prun
if pcheck "P6 two-line claude output" 1 "live: claude is 2.1.328 (Claude Code); the minimum is 2.1.328"; then
  if grep -qF extra "$T/p.out"; then no "P6 two-line claude output" "output shows the second line"
  else ok "P6 two-line claude output"; fi
fi

pdefaults; printf '%s\n' 'unknown' >"$T/oc.txt"; prun
pcheck "P7 non-numeric opencode output" 1 "the minimum is 1.18.16" && ok "P7 non-numeric opencode output"

pdefaults; printf '\t 2.1.330 (Claude Code) \r\n' >"$T/cc.txt"; printf '1.18.16\r\n' >"$T/oc.txt"; prun
pcheck "P8 leading blanks, trailing blank and CR removed" 0 "=2.1.330 2.1.330 1.18.16" && ok "P8 leading blanks, trailing blank and CR removed"

pdefaults; printf '%s\n' '1.19.0-beta' >"$T/oc.txt"; prun
pcheck "P9 prerelease above the pin" 0 "=2.1.330 2.1.330 1.19.0-beta" && ok "P9 prerelease above the pin"

pdefaults; printf '%s\n' '1.18.16-beta' >"$T/oc.txt"; prun
pcheck "P10 prerelease of the pin" 1 "live: opencode is 1.18.16-beta; the minimum is 1.18.16" && ok "P10 prerelease of the pin"

pdefaults; printf '2.1.330\r\nextra\n' >"$T/cc.txt"; prun
pcheck "P11 CR line then a second line" 1 "live: claude is 2.1.330; the minimum is 2.1.328" && ok "P11 CR line then a second line"

pdefaults; printf '%s\n' '2.1.328+build.5 (Claude Code)' >"$T/cc.txt"; prun
pcheck "P12 build suffix ignored" 0 "=2.1.328 2.1.328 1.18.16" && ok "P12 build suffix ignored"

pdefaults; printf '2.1.0\033[31mX\r\n' >"$T/cc.txt"; prun
if pcheck "P13 control characters removed from the message" 1 "live: claude is 2.1.0[31mX; the minimum is 2.1.328"; then
  if [ "$(tr -d '\033\r' <"$T/p.out" | wc -c)" = "$(wc -c <"$T/p.out")" ]; then ok "P13 control characters removed from the message"
  else no "P13 control characters removed from the message" "output holds ESC or CR"; fi
fi

# P14 (round 2): a C1 control (U+009B, CSI, UTF-8 C2 9B) and DEL are removed from the message.
pdefaults; printf '2.1.0\302\233X\177Y\n' >"$T/cc.txt"; prun
if pcheck "P14 C1 controls and DEL removed from the message" 1 "live: claude is 2.1.0XY; the minimum is 2.1.328"; then
  if [ "$(LC_ALL=C tr -d '\177-\377' <"$T/p.out" | wc -c)" = "$(wc -c <"$T/p.out")" ]; then ok "P14 C1 controls and DEL removed from the message"
  else no "P14 C1 controls and DEL removed from the message" "output holds a byte 0x7F-0xFF"; fi
fi

# P15 (round 2): an empty prerelease is not SemVer and fails.
pdefaults; printf '%s\n' '1.19.0-' >"$T/oc.txt"; prun
pcheck "P15 empty prerelease" 1 "live: opencode is 1.19.0-; the minimum is 1.18.16" && ok "P15 empty prerelease"

# --- K: check-premises.sh agent rows at or above the header; act-lite exact ---------------------
K=$T/k
kmiss=""
MS=docs/specs/2026-11-24-mail-sync-design.md
for f in scripts/check-premises.sh scripts/premises/ver.sh docs/specs/2026-11-18-zygo-design.md docs/conformance.md \
  "$MS" docs/live-checks.md; do
  [ -f "$REPO/$f" ] || { kmiss=$f; break; }
done
for d in fixtures docs/work crates/zygo-mail-sync; do
  [ -n "$kmiss" ] || [ -d "$REPO/$d" ] || kmiss=$d
done
if [ -z "$kmiss" ]; then
  mkdir -p "$K/scripts/premises" "$K/docs/specs" "$K/crates"
  cp "$REPO/scripts/check-premises.sh" "$K/scripts/"
  cp -R "$HERE/." "$K/scripts/premises/"   # ver.sh, and the live scripts that rows cite (SP1)
  cp "$REPO/docs/specs/2026-11-18-zygo-design.md" "$REPO/$MS" "$K/docs/specs/"
  cp "$REPO/docs/conformance.md" "$REPO/docs/live-checks.md" "$K/docs/"
  cp -R "$REPO/fixtures" "$K/"
  cp -R "$REPO/docs/work" "$K/docs/"   # node records that SP rows cite (node MS-25)
  mkdir -p "$K/crates/zygo-mail-sync"
  cp -R "$REPO/crates/zygo-mail-sync/src" "$REPO/crates/zygo-mail-sync/tests" "$K/crates/zygo-mail-sync/"
fi
# kcase <case> <id> <agent> <old regex> <V> <want rc> <fixed text>
kcase() {
  name="$1 $2 $3 version $5 -> exit $6"
  if [ -n "$kmiss" ]; then no "$name" "$kmiss missing"; return; fi
  sed "s/^\(| $2 | [^|]* | $3 | \)$4 |/\1$5 |/" "$REPO/docs/premises.md" >"$K/docs/premises.md"
  c=$(grep -c "^| $2 | .* | $3 | $5 |" "$K/docs/premises.md")
  [ "$c" -eq 1 ] || { no "$name" "planted row count $c"; return; }
  sh "$K/scripts/check-premises.sh" >"$T/k.out" 2>&1
  rc=$?
  if [ "$rc" -ne "$6" ]; then no "$name" "exit $rc: $(head -n 3 "$T/k.out" | tr '\n' '|')"; return; fi
  grep -qF -- "$7" "$T/k.out" || { no "$name" "output lacks: $7; has: $(head -n 3 "$T/k.out" | tr '\n' '|')"; return; }
  ok "$name"
}
kcase K1 P1 'Claude Code' '2\.1\.328' 2.1.330 0 "check-premises: ok"
kcase K2 P1 'Claude Code' '2\.1\.328' 2.1.327 1 "P1: version '2.1.327' is below the header's '2.1.328' for Claude Code"
kcase K3 P1 'Claude Code' '2\.1\.328' 2.1.33a 1 "P1: version '2.1.33a' is below the header's '2.1.328'"
kcase K4 P4 OpenCode '1\.18\.16' 1.18.20 0 "check-premises: ok"
kcase K5 P6b act-lite '1\.2\.3' 1.2.4 1 "P6b: version '1.2.4' differs from the header's '1.2.3' for act-lite"
# Node MS-25: git rows at or above the `git:` header (the pinned version, a minimum).
kcase K6 SP2 git '2\.50\.1' 2.50.0 1 "SP2: version '2.50.0' is below the header's '2.50.1' for git"
kcase K7 SP3 git '2\.56\.1' 2.60.0 0 "check-premises: ok"

# --- K (node MS-25): SP rows, the git header, cited tests, S§20 items, failed-row notes ------------
# kplant <case> <premises.md sed> <mail-sync spec sed, or ''> <want rc> <fixed text>
kplant() {
  name="$1 -> exit $4"
  if [ -n "$kmiss" ]; then no "$name" "$kmiss missing"; return; fi
  sed "$2" "$REPO/docs/premises.md" >"$K/docs/premises.md"
  if cmp -s "$K/docs/premises.md" "$REPO/docs/premises.md"; then no "$name" "the premises plant changed nothing"; return; fi
  cp "$REPO/$MS" "$K/$MS"
  if [ -n "$3" ]; then
    sed "$3" "$REPO/$MS" >"$K/$MS"
    if cmp -s "$K/$MS" "$REPO/$MS"; then no "$name" "the spec plant changed nothing"; return; fi
  fi
  sh "$K/scripts/check-premises.sh" >"$T/k.out" 2>&1
  rc=$?
  cp "$REPO/$MS" "$K/$MS"
  if [ "$rc" -ne "$4" ]; then no "$name" "exit $rc: $(head -n 3 "$T/k.out" | tr '\n' '|')"; return; fi
  grep -qF -- "$5" "$T/k.out" || { no "$name" "output lacks: $5; has: $(head -n 3 "$T/k.out" | tr '\n' '|')"; return; }
  ok "$name"
}
SP6F='s/^\(| SP6 | .*| 2026-11-27 | \)verified |/\1failed |/'
NOTE='s/^\(- \*\*SP6\*\* .*\)$/\1 *(Note 2026-11-27, MS-99, SP6: planted note)*/'
kplant "K8 git header missing" '/^git: /d' '' 1 "check-premises: header line 'git: <version>' missing"
kplant "K9 SP1 row missing" '/^| SP1 |/d' '' 1 "check-premises: SP1: expected exactly one row, found 0"
kplant "K10 SP4 result pending" 's/^\(| SP4 | .*| 2026-11-27 | \)verified |/\1pending |/' '' 1 \
  "check-premises: SP4: result 'pending' is not verified or failed"
kplant "K11 SP5 cites a test that the file does not define" \
  's/prune\.rs::sp5_plain_fetch_after_a_prune/prune.rs::sp5_no_such_test/' '' 1 \
  "check-premises: SP5: evidence test sp5_no_such_test is not defined in crates/zygo-mail-sync/tests/prune.rs"
kplant "K12 SP3 without its S§20 item" 's/^\(| SP3 | .*\) |$/\1 Planted. |/' 's/^- \*\*SP3\*\* /- SP3 /' 1 \
  "check-premises: SP3: the mail-sync spec S§20 has no item **SP3**"
kplant "K13 SP6 failed, no spec note" "$SP6F" '' 1 \
  "check-premises: SP6: failed, but the mail-sync spec has no dated note *(Note YYYY-MM-DD, <node>, SP6: …)*"
kplant "K14 SP6 failed, the row does not name the note's node" "$SP6F" "$NOTE" 1 \
  "check-premises: SP6: failed, but the evidence does not name MS-99, the node of the spec note"
kplant "K15 SP6 failed, the row names the note's node" "$SP6F;"'s/^\(| SP6 | .*\) |$/\1 Answered by MS-99. |/' "$NOTE" 0 \
  "check-premises: ok"

# --- L (node PRB-1): a LIVE line names the agent version that pinned_agents found, never the pin ----
# The fallback of each LIVE_VER site in the live scripts is LIVE_CC_VER or LIVE_OC_VER (both set by
# pinned_agents), not PIN_CC or PIN_OC (a minimum, operator 2026-11-21).
lhits=""
for f in "$HERE"/live.sh "$HERE"/live-*.sh; do   # round 2: live.sh's own default too
  if grep -qE ':-\$PIN_(CC|OC)\}|LIVE_VER=\$PIN_(CC|OC)([^A-Za-z0-9_]|$)' "$f"; then lhits="$lhits ${f##*/}"; fi
done
if [ -z "$lhits" ]; then ok "L1 no LIVE_VER site falls back to PIN_CC or PIN_OC"
else no "L1 no LIVE_VER site falls back to PIN_CC or PIN_OC" "in:$lhits"; fi
# L2 (round 2): N14-S runs the claude-curl.sh stand-in, which reports LIVE_PIN_CC as its version; the
# N14-S line names that version (the variable passed as LIVE_PIN_CC), not the installed claude's.
sv=$(sed -n 's/.*LIVE_VARS="LIVE_PIN_CC=\$\([A-Za-z_]*\) .*/\1/p' "$HERE/live-strict.sh")
pre=$(awk '/^result N14-S /{print prev; exit} {prev=$0}' "$HERE/live-strict.sh")
if [ -n "$sv" ] && [ "$pre" = "LIVE_VER=\$$sv" ]; then ok "L2 the N14-S line names the stand-in's version"
else no "L2 the N14-S line names the stand-in's version" "stand-in gets \$$sv; line before result N14-S: $pre"; fi
# L3 (round 2): a result before pinned_agents names no version, never the pin.
out=$( (unset LIVE_VER; . "$HERE/live.sh"; result X1 pass one) 2>&1)
pin=$(sed -n 's/^Claude Code: *//p' "$REPO/docs/premises.md" | head -n 1)
if [ "$out" = "LIVE X1 pass (agent ?): one" ]; then ok "L3 a result before pinned_agents names no version"
else no "L3 a result before pinned_agents names no version" "$out (pin $pin)"; fi
if [ "$bad" -eq 0 ]; then echo "ver tests: ok ($n)"; exit 0; fi
echo "ver tests: $bad failed"
exit 1

#!/bin/sh
# Test of live-selftest.sh (node FIX-7): its private tmux server leaves no socket file, after a whole
# run and after TERM while the server runs. No agent (the self-test uses stubs).
# Usage: sh scripts/premises/selftest_test.sh   ->  "selftest tests: ok (2)", exit 0
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
SOCKDIR="${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)"
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
n=0 bad=0
ok() { n=$((n + 1)); printf 'ok: %s\n' "$1"; }
no() { n=$((n + 1)); bad=$((bad + 1)); printf 'FAIL: %s\n' "$1"; }
# 1: a whole run. exec keeps the PID, so the socket name is n38selftest<pid>.
sh -c 'echo "$$" >"$1"; exec sh "$2"' sh "$T/pid1" "$HERE/live-selftest.sh" >"$T/run1.out" 2>&1
rc=$?
p=$(cat "$T/pid1")
if [ "$rc" -eq 0 ] && [ ! -e "$SOCKDIR/n38selftest$p" ]; then ok "whole run: exit 0, no socket n38selftest$p"
else no "whole run: exit $rc; socket n38selftest$p exists: $([ -e "$SOCKDIR/n38selftest$p" ] && echo yes || echo no)"; fi
# 2: TERM while the private server runs (part T6).
sh -c 'echo "$$" >"$1"; exec sh "$2"' sh "$T/pid2" "$HERE/live-selftest.sh" >"$T/run2.out" 2>&1 &
bp=$!
i=0; while [ ! -s "$T/pid2" ] && [ $i -lt 50 ]; do i=$((i + 1)); sleep 0.1; done
p=$(cat "$T/pid2")
i=0; while [ ! -e "$SOCKDIR/n38selftest$p" ] && [ $i -lt 1500 ]; do i=$((i + 1)); sleep 0.2; done
if [ ! -e "$SOCKDIR/n38selftest$p" ]; then
  kill "$bp" 2>/dev/null; wait "$bp"; no "TERM: the socket n38selftest$p did not appear in 300 s"
else
  kill -s TERM "$p"; wait "$bp"; rc=$?
  if [ "$rc" -eq 143 ] && [ ! -e "$SOCKDIR/n38selftest$p" ]; then ok "TERM: exit 143, no socket n38selftest$p"
  else no "TERM: exit $rc; socket n38selftest$p exists: $([ -e "$SOCKDIR/n38selftest$p" ] && echo yes || echo no)"; fi
fi
if [ "$bad" -eq 0 ]; then echo "selftest tests: ok ($n)"; exit 0; fi
echo "selftest tests: $bad of $n failed"; exit 1

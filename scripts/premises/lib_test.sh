#!/bin/sh
# Tests of the scratch dir that scripts/premises/lib.sh makes (node PRB-2): removed after exit 0,
# kept and named after an error or a signal, never inherited and never exported; with live.sh's
# live_traps and live_end, and under run-all.sh.
# Usage: sh scripts/premises/lib_test.sh   ->  "lib tests: ok (<n>)", exit 0
# No agent, no network; not run by CI. Each child runs with PATH = a dir of stub claude and
# opencode programs and links to the few tools lib.sh needs, and TMPDIR = a dir of its own.
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

# mkbin <dir> <with claude: 0|1>
mkbin() {
  mkdir -p "$1"
  for t in mktemp rm mkdir chmod dirname sleep cat cp sed head; do ln -s "$(command -v "$t")" "$1/$t"; done
  ln -s "$SH" "$1/sh"
  printf '#!/bin/sh\necho 0\n' >"$1/opencode"
  chmod +x "$1/opencode"
  if [ "$2" = 1 ]; then printf '#!/bin/sh\necho 0\n' >"$1/claude"; chmod +x "$1/claude"; fi
}
mkbin "$T/bin" 1
mkbin "$T/nocc" 0

# prep <case> <own|caller>: TMPDIR=$T/tmp-<case> always; caller: N2_SCRATCH=$T/keep-<case> (with
# a file mine). Sets XENV (extra env words; no path here holds a blank) and resets CP and NR.
prep() {
  mkdir -p "$T/tmp-$1"
  XENV="" CP=$T/bin NR=0
  if [ "$2" = caller ]; then
    mkdir -p "$T/keep-$1"
    : >"$T/keep-$1/mine"
    XENV="N2_SCRATCH=$T/keep-$1"
  fi
}
# child <case> <live: 0|1> <action>: write $T/<case>.sh.
child() {
  {
    echo "PREMISES_DIR='$HERE'"
    echo ". '$HERE/lib.sh'"
    if [ "$2" = 1 ]; then echo ". '$HERE/live.sh'"; echo "live_traps"; fi
    echo "echo \"\$N2_SCRATCH\" >'$T/$1.dir'"
    printf '%s\n' "$3"
  } >"$T/$1.sh"
}
# run <case>: run $T/<case>.sh in the foreground; sets rc.
run() {
  # shellcheck disable=SC2086
  env PATH="$CP" TMPDIR="$T/tmp-$1" $XENV "$SH" "$T/$1.sh" >"$T/$1.out" 2>"$T/$1.err"
  rc=$?
}
# sigact <case>: the action of a signal case.
sigact() { printf '%s' "echo \$\$ >'$T/$1.pid'; sleep 30 & echo \$! >'$T/$1.sleep'; : >'$T/$1.ready'; wait"; }
# waitready <file>: at most 50 polls, 0.1 s apart.
waitready() {
  i=0
  while [ ! -f "$1" ]; do
    i=$((i + 1))
    [ "$i" -le 50 ] || return 1
    sleep 0.1
  done
}
# sigrun <case> <signal>: run $T/<case>.sh in the background, signal it when ready; sets rc and NR.
sigrun() {
  # shellcheck disable=SC2086
  env PATH="$CP" TMPDIR="$T/tmp-$1" $XENV "$SH" "$T/$1.sh" >"$T/$1.out" 2>"$T/$1.err" &
  bp=$!
  if waitready "$T/$1.ready"; then
    kill -s "$2" "$(cat "$T/$1.pid")"
    wait "$bp"
    rc=$?
  else
    NR=1
    kill -s KILL "$bp" 2>/dev/null
    wait "$bp"
    rc=$?
  fi
  [ ! -f "$T/$1.sleep" ] || kill "$(cat "$T/$1.sleep")" 2>/dev/null
}
# res <name> <want rc> <check command...>: one ok or FAIL line.
res() {
  name=$1 want=$2
  shift 2
  why=""
  if [ "$NR" = 1 ]; then no "$name" "not ready in 5 s"; return; fi
  if [ "$rc" -ne "$want" ]; then no "$name" "exit $rc, want $want; stderr: $(tr '\n' '|' <"$ERR")"; return; fi
  if "$@"; then ok "$name"; else no "$name" "$why"; fi
}
# own_ok <case> <dir>: dir under $T/tmp-<case>/ and the first stderr line names it.
own_ok() {
  D=$2
  case "$D" in "$T/tmp-$1/"?*) ;; *) why="dir '$D' is not under $T/tmp-$1/"; return 1 ;; esac
  l=$(head -n 1 "$ERR")
  [ "$l" = "premises: scratch $D (removed on exit 0, kept otherwise; set N2_SCRATCH to keep it)" ] ||
    { why="first stderr line: $l"; return 1; }
}
dir_of() { cat "$T/$1.dir" 2>/dev/null; }
# removed <case> [<dir>]
removed() {
  own_ok "$1" "${2:-$(dir_of "$1")}" || return 1
  [ ! -e "$D" ] || { why="$D still exists"; return 1; }
  [ -z "$(ls -A "$T/tmp-$1")" ] || { why="$T/tmp-$1 is not empty"; return 1; }
  [ "$(wc -l <"$ERR" | tr -d ' ')" = 1 ] || { why="stderr: $(tr '\n' '|' <"$ERR")"; return 1; }
}
# kept_n <case> <n> [<dir>]
kept_n() {
  own_ok "$1" "${3:-$(dir_of "$1")}" || return 1
  [ -d "$D" ] || { why="$D is gone"; return 1; }
  l=$(sed -n '$p' "$ERR")
  [ "$l" = "premises: kept $D (exit $2)" ] || { why="last stderr line: $l"; return 1; }
}
# kept <case>: the caller's dir and its file are there, and stderr is empty.
kept() {
  [ -f "$T/keep-$1/mine" ] || { why="$T/keep-$1/mine is gone"; return 1; }
  [ ! -s "$ERR" ] || { why="stderr: $(tr '\n' '|' <"$ERR")"; return 1; }
}

# --- L: lib.sh alone ---------------------------------------------------------------------------
c=L1; prep $c own; ERR=$T/$c.err; child $c 0 'exit 0'; run $c
res "L1 own dir removed after exit 0" 0 removed $c

c=L2; prep $c own; ERR=$T/$c.err; child $c 0 'false'; run $c
res "L2 own dir kept after an error" 1 kept_n $c 1

c=L3; prep $c own; ERR=$T/$c.err; CP=$T/nocc; child $c 0 'exit 0'; run $c
l3() {
  grep -qxF 'premises: claude is not on PATH' "$T/L3.out" || { why="stdout: $(tr '\n' '|' <"$T/L3.out")"; return 1; }
  [ "$(ls -A "$T/tmp-L3" | wc -l | tr -d ' ')" = 1 ] || { why="$T/tmp-L3 does not hold exactly one entry"; return 1; }
  kept_n L3 1 "$T/tmp-L3/$(ls -A "$T/tmp-L3")"
}
res "L3 own dir kept when claude is not on PATH" 1 l3

c=L4; prep $c own; ERR=$T/$c.err; child $c 0 "$(sigact $c)"; sigrun $c TERM
res "L4 own dir kept after TERM" 143 kept_n $c 143

c=L5; prep $c own; ERR=$T/$c.err; child $c 0 "$(sigact $c)"; sigrun $c HUP
res "L5 own dir kept after HUP" 129 kept_n $c 129

c=L6; prep $c caller; ERR=$T/$c.err; child $c 0 'exit 0'; run $c
res "L6 caller dir kept after exit 0" 0 kept $c

c=L7; prep $c caller; ERR=$T/$c.err; child $c 0 "$(sigact $c)"; sigrun $c TERM
res "L7 caller dir kept after TERM" 143 kept $c

c=L8; prep $c own; ERR=$T/$c.err
child $c 0 'mkdir "$N2_SCRATCH/ro"; : >"$N2_SCRATCH/ro/f"; chmod 500 "$N2_SCRATCH/ro"; exit 0'; run $c
res "L8 own dir with a read-only subdir removed" 0 removed $c

if [ "$(id -u)" = 0 ]; then
  ok "L9 skipped (root)"
else
  c=L9; prep $c own; ERR=$T/$c.err; child $c 0 'chmod 500 "$TMPDIR"; exit 0'; run $c
  l9() {
    D=$(dir_of L9)
    own_ok L9 "$D" || return 1
    l=$(sed -n '$p' "$ERR")
    [ "$l" = "premises: could not remove $D" ] || { why="last stderr line: $l"; return 1; }
    [ -d "$D" ] || { why="$D is gone"; return 1; }
  }
  res "L9 failed removal named, status kept" 0 l9
  chmod 700 "$T/tmp-L9"
fi

c=L10; prep $c own; ERR=$T/$c.err
child $c 0 ". '$HERE/lib.sh'; [ \"\$N2_SCRATCH_OWN\" = \"\$N2_SCRATCH\" ] && [ \"\$N2_SCRATCH\" = \"\$(cat '$T/L10.dir')\" ] && echo same >'$T/L10.same'; exit 0"
run $c
l10() {
  [ "$(cat "$T/L10.same" 2>/dev/null)" = same ] || { why="the second source changed the ownership"; return 1; }
  removed L10
}
res "L10 a second source keeps the first ownership" 0 l10

c=L11; prep $c caller; ERR=$T/$c.err; echo 'LIVE OLD' >"$T/keep-$c/results.txt"
child $c 0 "echo 'LIVE NEW' >>\"\$N2_SCRATCH/results.txt\""; run $c
l11() {
  [ "$(cat "$T/keep-L11/results.txt")" = "LIVE OLD
LIVE NEW" ] || { why="results.txt: $(tr '\n' '|' <"$T/keep-L11/results.txt")"; return 1; }
}
res "L11 results.txt is not truncated" 0 l11

# --- E: ownership is never inherited and never exported ------------------------------------------
c=E1; prep $c caller; ERR=$T/$c.err; XENV="$XENV N2_SCRATCH_OWN=$T/keep-$c"
child $c 0 "sh -c 'echo \"\${N2_SCRATCH_OWN:-none}\"'; exit 0"; run $c
e1() {
  [ "$(cat "$T/E1.out")" = none ] || { why="stdout: $(tr '\n' '|' <"$T/E1.out")"; return 1; }
  kept E1
}
res "E1 an inherited N2_SCRATCH_OWN is dropped, caller dir kept" 0 e1

c=E2; prep $c own; ERR=$T/$c.err
child $c 0 "sh -c 'echo \"\${N2_SCRATCH_OWN:-none}\"'; exit 0"; run $c
e2() {
  [ "$(cat "$T/E2.out")" = none ] || { why="stdout: $(tr '\n' '|' <"$T/E2.out")"; return 1; }
  removed E2
}
res "E2 N2_SCRATCH_OWN is not exported, own dir removed" 0 e2

# --- C: with live.sh (live_traps, live_end) -----------------------------------------------------
c=C1; prep $c own; ERR=$T/$c.err; child $c 1 'mkdir -p "$N2_SCRATCH/live-$$"; exit 0'; run $c
res "C1 live_end then own dir removed after exit 0" 0 removed $c

c=C2; prep $c own; ERR=$T/$c.err; child $c 1 "$(sigact $c)"; sigrun $c TERM
c2() {
  kept_n C2 143 || return 1
  [ ! -e "$D/live-$(cat "$T/C2.pid")" ] || { why="$D/live-<pid> is left"; return 1; }
}
res "C2 live_end then own dir kept after TERM" 143 c2

c=C3; prep $c caller; ERR=$T/$c.err
child $c 1 "mkdir -p \"\$N2_SCRATCH/live-\$\$\"; echo \$\$ >'$T/C3.pid'; exit 0"; run $c
c3() {
  kept C3 || return 1
  [ ! -e "$T/keep-C3/live-$(cat "$T/C3.pid")" ] || { why="live-<pid> is left"; return 1; }
}
res "C3 live_end removes live-<pid>, caller dir kept" 0 c3

# --- R: run-all.sh ------------------------------------------------------------------------------
# rall <case>: a copy of run-all.sh and lib.sh with a stub run-<id>.sh for each id it runs.
rall() {
  mkdir -p "$T/$1-pd"
  cp "$HERE/run-all.sh" "$HERE/lib.sh" "$T/$1-pd/"
  for id in smoke P1 P1-interactive P2 P3 Q1 Q4 Q6 Q7-claude Q5 opencode P6; do
    printf '%s\n' "echo \"\$N2_SCRATCH\" >>'$T/$1.seen'" >"$T/$1-pd/run-$id.sh"
  done
}
rrun() {
  # shellcheck disable=SC2086
  env PATH="$CP" TMPDIR="$T/tmp-$1" $XENV "$SH" "$T/$1-pd/run-all.sh" >"$T/$1.out" 2>"$T/$1.err"
  rc=$?
}
# seen <case> <n> <dir>: .seen has n lines, each <dir>.
seen() {
  [ "$(wc -l <"$T/$1.seen" | tr -d ' ')" = "$2" ] || { why=".seen: $(tr '\n' '|' <"$T/$1.seen")"; return 1; }
  [ -z "$(grep -vxF -- "$3" "$T/$1.seen")" ] || { why=".seen: $(tr '\n' '|' <"$T/$1.seen")"; return 1; }
}
first_seen() { head -n 1 "$T/$1.seen" 2>/dev/null; }

c=R1; prep $c own; ERR=$T/$c.err; rall $c; rrun $c
r1() { D1=$(first_seen R1); seen R1 12 "$D1" && removed R1 "$D1"; }
res "R1 run-all own dir removed after exit 0" 0 r1

c=R2; prep $c own; ERR=$T/$c.err; rall $c; echo 'exit 3' >>"$T/$c-pd/run-P2.sh"; rrun $c
r2() { D1=$(first_seen R2); seen R2 4 "$D1" && kept_n R2 3 "$D1"; }
res "R2 run-all own dir kept after a failed child" 3 r2

c=R3; prep $c caller; ERR=$T/$c.err; rall $c; rrun $c
r3() { seen R3 12 "$T/keep-R3" && kept R3; }
res "R3 run-all caller dir kept" 0 r3

c=R4; prep $c own; ERR=$T/$c.err; rall $c
printf '%s\n' ". \"\$(dirname \"\$0\")/lib.sh\"; echo \"\$N2_SCRATCH\" >>'$T/R4.seen'; : >\"\$N2_SCRATCH/child-mark\"" >"$T/$c-pd/run-smoke.sh"
printf '%s\n' "[ -f \"\$N2_SCRATCH/child-mark\" ] && echo kept >'$T/R4.kept'" >>"$T/$c-pd/run-P1.sh"
rrun $c
r4() {
  [ "$(cat "$T/R4.kept" 2>/dev/null)" = kept ] || { why="a child sourcing lib.sh removed the dir"; return 1; }
  D1=$(first_seen R4); seen R4 12 "$D1" && removed R4 "$D1"
}
res "R4 a child that sources lib.sh keeps run-all's dir" 0 r4

c=R5; prep $c own; ERR=$T/$c.err; rall $c
printf '%s\n' ": >'$T/R5.ready'; sleep 30" >"$T/$c-pd/run-smoke.sh"
perl -e 'setpgrp(0, 0); exec @ARGV or die' env PATH="$CP" TMPDIR="$T/tmp-$c" "$SH" "$T/$c-pd/run-all.sh" >"$T/$c.out" 2>"$T/$c.err" &
bp=$!
if waitready "$T/R5.ready"; then
  kill -s TERM -- "-$bp"
  wait "$bp"
  rc=$?
else
  NR=1
  kill -s KILL -- "-$bp" 2>/dev/null
  wait "$bp"
  rc=$?
fi
r5() {
  [ "$(ls -A "$T/tmp-R5" | wc -l | tr -d ' ')" = 1 ] || { why="$T/tmp-R5 does not hold exactly one entry"; return 1; }
  kept_n R5 143 "$T/tmp-R5/$(ls -A "$T/tmp-R5")"
}
res "R5 run-all own dir kept after TERM to the process group" 143 r5

if [ "$bad" -eq 0 ]; then echo "lib tests: ok ($n)"; exit 0; fi
echo "lib tests: $bad failed"
exit 1

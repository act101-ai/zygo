#!/bin/sh
# Tests of the scratch dir that scripts/premises/lib.sh makes (node PRB-2): removed after exit 0,
# kept and named after an error or a signal, never inherited and never exported; with live.sh's
# live_traps and live_end, and under run-all.sh; the Codex helpers and run-codex.sh (node PC-2).
# Usage: sh scripts/premises/lib_test.sh   ->  "lib tests: ok (<n>)", exit 0
# No agent, no network; not run by CI. Each child runs with PATH = a dir of stub claude, opencode
# and codex programs and links to the few tools lib.sh needs, and TMPDIR = a dir of its own.
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

# mkcx <file> <version>: a stub codex. --version prints "codex-cli <version>" (fixed here: env -i
# drops any variable); any other call writes its environment, $0 and arguments under $HOME and
# prints "stub". Perl, because a shell adds PWD, SHLVL and _ to the environment it reports.
mkcx() {
  sed "s/@VER@/$2/" >"$1" <<'PL'
#!/usr/bin/perl
if (@ARGV == 1 && $ARGV[0] eq "--version") { print "codex-cli @VER@\n"; exit 0; }
my $h = $ENV{HOME};
open(my $f, ">", "$h/stub-env.txt") or die "stub: $!"; print $f map { "$_=$ENV{$_}\n" } sort keys %ENV; close $f;
open($f, ">", "$h/stub-exe.txt") or die "stub: $!"; print $f "$0\n"; close $f;
open($f, ">", "$h/stub-args.txt") or die "stub: $!"; print $f "@ARGV\n"; close $f;
print "stub\n";
PL
  chmod +x "$1"
}
# mkbin <dir> <with claude: 0|1> [<stub codex version>]
mkbin() {
  mkdir -p "$1"
  for t in mktemp rm mkdir chmod dirname sleep cat cp sed head env grep tee python3 git cut tr ls; do ln -s "$(command -v "$t")" "$1/$t"; done
  ln -s "$SH" "$1/sh"
  printf '#!/bin/sh\necho 0\n' >"$1/opencode"
  chmod +x "$1/opencode"
  if [ "$2" = 1 ]; then printf '#!/bin/sh\necho 0\n' >"$1/claude"; chmod +x "$1/claude"; fi
  mkcx "$1/codex" "${3:-0.172.0}"
}
mkbin "$T/bin" 1
mkbin "$T/nocc" 0
mkbin "$T/nocx" 1
rm "$T/nocx/codex"

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
# perl resets INT to its default first: a background command of a non-interactive shell starts with
# INT ignored, and a shell cannot trap a signal that was ignored when it started.
sigrun() {
  # shellcheck disable=SC2086
  perl -e '$SIG{INT} = "DEFAULT"; exec @ARGV or die' \
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
# piperun <case>: run $T/<case>.sh with stdout to a pipe whose reader has exited; sets rc.
piperun() {
  # shellcheck disable=SC2086
  { env PATH="$CP" TMPDIR="$T/tmp-$1" $XENV "$SH" "$T/$1.sh" 2>"$T/$1.err"; echo $? >"$T/$1.rc"; } | true
  rc=$(cat "$T/$1.rc")
}
# PIPE_ACT: write to stdout until the closed pipe stops the script.
PIPE_ACT='while :; do echo x; done'
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
# Round 2: the PATH checks run before mktemp, so a failed check makes no dir and prints no scratch line.
l3() {
  grep -qxF 'premises: claude is not on PATH' "$T/L3.out" || { why="stdout: $(tr '\n' '|' <"$T/L3.out")"; return 1; }
  [ -z "$(ls -A "$T/tmp-L3")" ] || { why="$T/tmp-L3 holds: $(ls -A "$T/tmp-L3")"; return 1; }
  [ ! -s "$ERR" ] || { why="stderr: $(tr '\n' '|' <"$ERR")"; return 1; }
}
res "L3 no dir made when claude is not on PATH" 1 l3

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

# Round 2: live.sh's result writes results.txt twice; the earlier line and both new lines stay.
c=L11; prep $c caller; ERR=$T/$c.err; echo 'LIVE OLD' >"$T/keep-$c/results.txt"
child $c 1 'result X1 pass one; result X2 pass two; exit 0'; run $c
l11() {
  [ "$(cat "$T/keep-L11/results.txt")" = "LIVE OLD
LIVE X1 pass (agent ?): one
LIVE X2 pass (agent ?): two" ] || { why="results.txt: $(tr '\n' '|' <"$T/keep-L11/results.txt")"; return 1; }
  kept L11
}
res "L11 results.txt is not truncated by two result calls" 0 l11

c=L12; prep $c own; ERR=$T/$c.err; child $c 0 "$(sigact $c)"; sigrun $c INT
res "L12 own dir kept after INT" 130 kept_n $c 130

c=L13; prep $c own; ERR=$T/$c.err; child $c 0 "$PIPE_ACT"; piperun $c
res "L13 own dir kept after SIGPIPE" 141 kept_n $c 141

# --- E: ownership is never inherited and never exported ------------------------------------------
# Round 2: env shows an exported N2_SCRATCH_OWN even when it is empty (${…:-none} did not).
envact() { printf '%s' "env | grep '^N2_SCRATCH_OWN=' >'$T/$1.env' || :; exit 0"; }
noenv() {
  [ -f "$T/$1.env" ] || { why="the child did not run env"; return 1; }
  [ ! -s "$T/$1.env" ] || { why="a child sees: $(tr '\n' '|' <"$T/$1.env")"; return 1; }
}
c=E1; prep $c caller; ERR=$T/$c.err; XENV="$XENV N2_SCRATCH_OWN=$T/keep-$c"
child $c 0 "$(envact $c)"; run $c
e1() { noenv E1 && kept E1; }
res "E1 an inherited N2_SCRATCH_OWN is dropped, caller dir kept" 0 e1

c=E2; prep $c own; ERR=$T/$c.err
child $c 0 "$(envact $c)"; run $c
e2() { noenv E2 && removed E2; }
res "E2 N2_SCRATCH_OWN is not exported, own dir removed" 0 e2

c=E3; prep $c own; ERR=$T/$c.err; mkdir -p "$T/other-$c"; : >"$T/other-$c/mine"
XENV="N2_SCRATCH_OWN=$T/other-$c"
child $c 0 'exit 0'; run $c
e3() {
  [ -f "$T/other-E3/mine" ] || { why="the inherited N2_SCRATCH_OWN dir was removed"; return 1; }
  removed E3
}
res "E3 an inherited N2_SCRATCH_OWN dir is kept, only the own dir removed" 0 e3

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
child $c 1 "mkdir -p \"\$N2_SCRATCH/live-\$\$\"; : >\"\$N2_SCRATCH/live-\$\$/capture\"; : >\"\$N2_SCRATCH/live-\$\$/pg-1\"; echo \$\$ >'$T/C3.pid'; exit 0"; run $c
c3() {
  kept C3 || return 1
  L=$T/keep-C3/live-$(cat "$T/C3.pid")
  [ -f "$L/capture" ] || { why="live-<pid>/capture is gone"; return 1; }
  [ ! -e "$L/pg-1" ] || { why="live-<pid>/pg-1 is left"; return 1; }
}
res "C3 live_end keeps live-<pid> in a caller dir, without its pg-* files" 0 c3

c=C6; prep $c caller; ERR=$T/$c.err
child $c 1 "mkdir -p \"\$N2_SCRATCH/live-\$\$\"; : >\"\$N2_SCRATCH/live-\$\$/capture\"; $(sigact C6)"; sigrun $c TERM
c6() {
  kept C6 || return 1
  [ -f "$T/keep-C6/live-$(cat "$T/C6.pid")/capture" ] || { why="live-<pid>/capture is gone"; return 1; }
}
res "C6 live_end keeps live-<pid> in a caller dir after TERM" 143 c6

c=C4; prep $c own; ERR=$T/$c.err; child $c 1 "$PIPE_ACT"; piperun $c
res "C4 live_end then own dir kept after SIGPIPE" 141 kept_n $c 141

c=C5; prep $c own; ERR=$T/$c.err
child $c 1 'mkdir -p "$N2_SCRATCH/live-$$/ro"; : >"$N2_SCRATCH/live-$$/ro/f"; chmod 500 "$N2_SCRATCH/live-$$/ro"; exit 0'
run $c
res "C5 live_end removes a live-<pid> with a read-only subdir, then the own dir" 0 removed $c

if [ "$(id -u)" = 0 ]; then
  ok "C7 skipped (root)"
else
  # FIX-7: a caller dir keeps live-<pid>, so the failed removal is shown in a dir lib.sh made;
  # n2_scratch_end then makes that dir writable again and removes it.
  c=C7; prep $c own; ERR=$T/$c.err
  child $c 1 "mkdir -p \"\$N2_SCRATCH/live-\$\$\"; echo \$\$ >'$T/C7.pid'; chmod 500 \"\$N2_SCRATCH\"; exit 0"
  run $c
  c7() {
    D=$(dir_of C7)
    own_ok C7 "$D" || return 1
    l=$(sed -n '$p' "$ERR")
    [ "$l" = "premises: could not remove $D/live-$(cat "$T/C7.pid")" ] || { why="stderr: $(tr '\n' '|' <"$ERR")"; return 1; }
    [ ! -e "$D" ] || { why="$D still exists"; return 1; }
  }
  res "C7 live_end names a live-<pid> it could not remove in an own dir, status kept" 0 c7
fi

# --- R: run-all.sh ------------------------------------------------------------------------------
# rall <case>: a copy of run-all.sh and lib.sh with a stub run-<id>.sh for each id it runs.
rall() {
  mkdir -p "$T/$1-pd"
  cp "$HERE/run-all.sh" "$HERE/lib.sh" "$HERE/ver.sh" "$T/$1-pd/"
  for id in smoke P1 P1-interactive P2 P3 Q1 Q4 Q6 Q7-claude Q5 opencode P6 codex; do
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
r1() { D1=$(first_seen R1); seen R1 13 "$D1" && removed R1 "$D1"; }
res "R1 run-all own dir removed after exit 0" 0 r1

c=R2; prep $c own; ERR=$T/$c.err; rall $c; echo 'exit 3' >>"$T/$c-pd/run-P2.sh"; rrun $c
r2() { D1=$(first_seen R2); seen R2 4 "$D1" && kept_n R2 3 "$D1"; }
res "R2 run-all own dir kept after a failed child" 3 r2

c=R3; prep $c caller; ERR=$T/$c.err; rall $c; rrun $c
r3() { seen R3 13 "$T/keep-R3" && kept R3; }
res "R3 run-all caller dir kept" 0 r3

c=R4; prep $c own; ERR=$T/$c.err; rall $c
printf '%s\n' ". \"\$(dirname \"\$0\")/lib.sh\"; echo \"\$N2_SCRATCH\" >>'$T/R4.seen'; : >\"\$N2_SCRATCH/child-mark\"" >"$T/$c-pd/run-smoke.sh"
printf '%s\n' "[ -f \"\$N2_SCRATCH/child-mark\" ] && echo kept >'$T/R4.kept'" >>"$T/$c-pd/run-P1.sh"
rrun $c
r4() {
  [ "$(cat "$T/R4.kept" 2>/dev/null)" = kept ] || { why="a child sourcing lib.sh removed the dir"; return 1; }
  D1=$(first_seen R4); seen R4 13 "$D1" && removed R4 "$D1"
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

# --- X: Codex runs (node PC-2): codex_env, run_codex, codex_record --------------------------------
# Each X case: N2_AGENTS=codex, CODEX_FX=$T/fx-<case>, a stub codex (mkcx); no real agent runs.
DK="dummy-""key"
CX_NAMES="ALL_PROXY CODEX_HOME HOME HTTPS_PROXY HTTP_PROXY MOCK_API_KEY PATH TERM TMPDIR XDG_CACHE_HOME XDG_CONFIG_HOME XDG_DATA_HOME XDG_STATE_HOME "
mkbin "$T/cxonly" 0
rm "$T/cxonly/opencode"
mkbin "$T/cx171" 1 0.171.9
mkbin "$T/cx173" 1 0.173.0
cat >"$T/connect.py" <<'PY'
import socket, sys
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=30)
s.sendall(b"CONNECT example.invalid:443 HTTP/1.1\r\nHost: example.invalid:443\r\n\r\n")
while s.recv(4096):
    pass
PY
# cxprep <case> <own|caller>: prep, then the Codex words in XENV.
cxprep() { prep "$1" "$2"; ERR=$T/$1.err; XENV="$XENV N2_AGENTS=codex CODEX_FX=$T/fx-$1"; }
# MOCKREC <case>: action text that records the mock's pid, so a failed case's mock is stopped here.
mockrec() { printf '%s' "echo \"\$MOCK_PID\" >'$T/$1.mock'"; }
killmock() { [ ! -f "$T/$1.mock" ] || kill "$(cat "$T/$1.mock")" 2>/dev/null; }
# out_has <case> <line>: stdout holds the line.
out_has() { grep -qxF -- "$2" "$T/$1.out" || { why="stdout: $(tr '\n' '|' <"$T/$1.out")"; return 1; }; }
# no_stub_env <dir>...: no stub-env.txt under any dir.
no_stub_env() {
  for d in "$@"; do
    [ -z "$(find "$d" -name stub-env.txt 2>/dev/null)" ] || { why="stub-env.txt under $d"; return 1; }
  done
}
# envval <file> <name>: the value of NAME in a stub-env.txt.
envval() { sed -n "s/^$2=//p" "$1"; }
# isolation_is <case> <version>
isolation_is() {
  f=$T/fx-$1/runs/base/isolation.txt
  [ "$(cat "$f" 2>/dev/null)" = "home=scratch
codex_home=scratch
codex_version=$2" ] || { why="isolation.txt: $(tr '\n' '|' <"$f" 2>/dev/null)"; return 1; }
}

c=X1; cxprep $c own; CP=$T/cxonly; child $c 0 'exit 0'; run $c
res "X1 a codex-only run needs only codex on PATH" 0 removed $c

c=X2; cxprep $c own; CP=$T/nocx; child $c 0 'exit 0'; run $c
x2() {
  out_has X2 'premises: codex is not on PATH' || return 1
  [ -z "$(ls -A "$T/tmp-X2")" ] || { why="$T/tmp-X2 holds: $(ls -A "$T/tmp-X2")"; return 1; }
}
res "X2 no dir made when codex is not on PATH" 1 x2

X3ACT="new_run x; start_mock; $(mockrec X3); run_codex exec hi; python3 -I '$T/connect.py' \"\$(cat \"\$RUN/port\")\"; stop_mock; codex_record base; exit 0"
c=X3; cxprep $c caller; child $c 0 "$X3ACT"; run $c; killmock $c
x3() {
  E=$T/keep-X3/x/home/stub-env.txt
  [ -f "$E" ] || { why="no $E"; return 1; }
  names=$(cut -d= -f1 "$E" | LC_ALL=C sort | tr '\n' ' ')
  [ "$names" = "$CX_NAMES" ] || { why="names: $names"; return 1; }
  [ "$(envval "$E" HOME)" = "$T/keep-X3/x/home" ] || { why="HOME=$(envval "$E" HOME)"; return 1; }
  [ "$(envval "$E" CODEX_HOME)" = "$T/keep-X3/x/home/.codex" ] || { why="CODEX_HOME=$(envval "$E" CODEX_HOME)"; return 1; }
  P=$(envval "$E" HTTPS_PROXY)
  printf '%s\n' "$P" | grep -Eqx 'http://127\.0\.0\.1:[0-9]+' || { why="HTTPS_PROXY=$P"; return 1; }
  [ "$(envval "$E" HTTP_PROXY)" = "$P" ] && [ "$(envval "$E" ALL_PROXY)" = "$P" ] || { why="HTTP_PROXY or ALL_PROXY is not $P"; return 1; }
  [ "$(envval "$E" MOCK_API_KEY)" = "$DK" ] || { why="MOCK_API_KEY is not the dummy"; return 1; }
  ! grep -qF -- "$HOME" "$E" || { why="a value holds the tester's HOME"; return 1; }
  C=$T/keep-X3/x/home/.codex/config.toml
  grep -qxF "base_url = \"$P/v1\"" "$C" && grep -qxF 'env_key = "MOCK_API_KEY"' "$C" || { why="config.toml: $(tr '\n' '|' <"$C")"; return 1; }
  isolation_is X3 0.172.0 || return 1
  [ "$(cat "$T/fx-X3/runs/base/connect.txt")" = "example.invalid:443 refused" ] || { why="connect.txt: $(tr '\n' '|' <"$T/fx-X3/runs/base/connect.txt")"; return 1; }
}
res "X3 codex runs isolated, the mock as proxy and base_url, isolation and CONNECT recorded" 0 x3

c=X4; cxprep $c caller; CP=$T/cx171; child $c 0 "new_run x; start_mock; $(mockrec X4); run_codex exec hi"; run $c; killmock $c
x4() { out_has X4 'premises: codex 0.171.9 is below the minimum 0.172.0' && no_stub_env "$T/keep-X4"; }
res "X4 a codex below the minimum does not run" 1 x4

c=X5; cxprep $c caller; CP=$T/cx173
child $c 0 "new_run x; start_mock; $(mockrec X5); run_codex exec hi; stop_mock; codex_record base; exit 0"; run $c; killmock $c
res "X5 a later codex runs and its version is recorded" 0 isolation_is X5 0.173.0

c=X6; cxprep $c caller; child $c 0 "new_run x; start_mock; $(mockrec X6); H='$T/elsewhere-X6'; run_codex exec hi"; run $c; killmock $c
x6() { out_has X6 'premises: codex HOME is not under the run folder' && no_stub_env "$T/keep-X6" "$T/elsewhere-X6"; }
res "X6 HOME outside the run folder stops before codex starts" 1 x6

c=X7; cxprep $c caller; child $c 0 "new_run x; start_mock; $(mockrec X7); CX_HOME='$T/elsewhere-X7'; run_codex exec hi"; run $c; killmock $c
x7() { out_has X7 'premises: CODEX_HOME is not under the run folder' && no_stub_env "$T/keep-X7" "$T/elsewhere-X7"; }
res "X7 CODEX_HOME outside the run folder stops before codex starts" 1 x7

# X8: codex on PATH is a link into the owner's ~/.codex package; the harness runs a scratch copy.
c=X8; cxprep $c caller
PK=$T/hX8/.codex/packages/standalone
mkdir -p "$PK/releases/v/bin" "$T/hX8/.local/bin"
mkcx "$PK/releases/v/bin/codex" 0.172.0
: >"$PK/releases/v/codex-package.json"
ln -s releases/v "$PK/current"
ln -s "$PK/current/bin/codex" "$T/hX8/.local/bin/codex"
mkbin "$T/cx8" 1
rm "$T/cx8/codex"
CP=$T/hX8/.local/bin:$T/cx8; XENV="$XENV HOME=$T/hX8"
child $c 0 "new_run x; start_mock; $(mockrec X8); run_codex exec hi; stop_mock; codex_record base; exit 0"; run $c; killmock $c
x8() {
  E=$T/keep-X8/x/home/stub-env.txt
  [ "$(cat "$T/keep-X8/x/home/stub-exe.txt" 2>/dev/null)" = "$T/keep-X8/codex-pkg/bin/codex" ] ||
    { why="stub-exe.txt: $(cat "$T/keep-X8/x/home/stub-exe.txt" 2>/dev/null)"; return 1; }
  [ -f "$T/keep-X8/codex-pkg/codex-package.json" ] || { why="no codex-package.json in the copy"; return 1; }
  python3 -I -c 'import os, sys
b = os.path.realpath(sys.argv[2])
sys.exit(any(os.path.realpath(p) == b or os.path.realpath(p).startswith(b + "/") for p in sys.argv[1].split(":")))' \
    "$(envval "$E" PATH)" "$T/hX8/.codex" || { why="a PATH entry is under ~/.codex: $(envval "$E" PATH)"; return 1; }
}
res "X8 a codex under ~/.codex runs from a scratch copy of its package" 0 x8

c=X9; cxprep $c caller; mkdir -p "$T/hX9/.codex/scr"; XENV="N2_AGENTS=codex CODEX_FX=$T/fx-$c HOME=$T/hX9 N2_SCRATCH=$T/hX9/.codex/scr"
child $c 0 "new_run x; start_mock; $(mockrec X9); run_codex exec hi"; run $c; killmock $c
x9() { out_has X9 'premises: the run folder is under ~/.codex' && no_stub_env "$T/hX9"; }
res "X9 a run folder under ~/.codex stops before codex starts" 1 x9

c=X10; cxprep $c caller; child $c 0 "new_run x; start_mock; $(mockrec X10); EXTRA_ENV='HOME=$T/elsewhere-X10'; run_codex exec hi"; run $c; killmock $c
x10() { out_has X10 'premises: EXTRA_ENV sets HOME, CODEX_HOME or PATH' && no_stub_env "$T/keep-X10"; }
res "X10 EXTRA_ENV may not set HOME, CODEX_HOME or PATH" 1 x10

c=X11; cxprep $c caller; child $c 0 'new_run x; run_codex exec hi'; run $c
x11() { out_has X11 'premises: codex_env needs start_mock first' && no_stub_env "$T/keep-X11"; }
res "X11 run_codex needs start_mock first" 1 x11

c=X12; cxprep $c caller; child $c 0 'new_run x; codex_record base'; run $c
x12() {
  out_has X12 'premises: codex_record: no codex run in this run folder' || return 1
  [ ! -e "$T/fx-X12/runs/base" ] || { why="$T/fx-X12/runs/base was made"; return 1; }
}
res "X12 codex_record needs a codex run in this run folder" 1 x12

c=X13; cxprep $c caller
child $c 0 "new_run x; start_mock; $(mockrec X13); run_codex exec hi; stop_mock; codex_record ../x"; run $c; killmock $c
x13() { out_has X13 "premises: bad part name '../x'" && [ ! -e "$T/fx-X13" ] || { why="${why:-$T/fx-X13 was made}"; return 1; }; }
res "X13 codex_record refuses a bad part name" 1 x13

# --- K: run-codex.sh --------------------------------------------------------------------------
mkdir -p "$T/K-pd/codex"
cp "$HERE/run-codex.sh" "$HERE/lib.sh" "$HERE/ver.sh" "$T/K-pd/"
for p in a b; do printf '%s\n' "echo $p >>'$T/K.seen'" >"$T/K-pd/codex/$p.sh"; done
# krun <case> [<part>]: run the copied run-codex.sh; sets rc.
krun() {
  rm -f "$T/K.seen"
  c=$1; shift
  # shellcheck disable=SC2086
  env PATH="$CP" TMPDIR="$T/tmp-$c" $XENV "$SH" "$T/K-pd/run-codex.sh" "$@" >"$T/$c.out" 2>"$T/$c.err"
  rc=$?
}
kseen() { [ "$(cat "$T/K.seen" 2>/dev/null | tr '\n' ' ')" = "$1" ] || { why="K.seen: $(tr '\n' ' ' <"$T/K.seen" 2>/dev/null)"; return 1; }; }

c=K1; prep $c caller; ERR=$T/$c.err; krun $c
res "K1 run-codex runs every part in name order" 0 kseen "a b "
c=K2; prep $c caller; ERR=$T/$c.err; krun $c b
res "K2 run-codex runs one part" 0 kseen "b "
c=K3; prep $c caller; ERR=$T/$c.err; krun $c zz
res "K3 run-codex names a missing part" 1 out_has K3 'run-codex: no part zz'
c=K4; prep $c caller; ERR=$T/$c.err; krun $c ../a
res "K4 run-codex refuses a bad part name" 1 out_has K4 "run-codex: bad part name '../a'"

if [ "$bad" -eq 0 ]; then echo "lib tests: ok ($n)"; exit 0; fi
echo "lib tests: $bad failed"
exit 1

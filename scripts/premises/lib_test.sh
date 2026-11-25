#!/bin/sh
# Tests of the scratch dir that scripts/premises/lib.sh makes (node PRB-2): removed after exit 0,
# kept and named after an error or a signal, never inherited and never exported; with live.sh's
# live_traps and live_end, and under run-all.sh; the Codex helpers and run-codex.sh (node PC-2).
# Usage: sh scripts/premises/lib_test.sh   ->  "lib tests: ok (<n>)", exit 0
# LIB_TEST_SH: the shell the children run under (default dash when installed, else sh).
# No agent, no network; not run by CI. Each child runs with PATH = a dir of stub claude, opencode
# and codex programs and links to the few tools lib.sh needs, and TMPDIR = a dir of its own.
set -u
unset N2_SCRATCH N2_SCRATCH_OWN
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
SH=${LIB_TEST_SH:-$(command -v dash || command -v sh)}
T=$(mktemp -d)
# PC-10: lib.sh's process scans and daemon folders see only this test's folder (N2_TEST_ROOT), so
# the host's own Codex daemon or another session's run does not change a result. The stub daemons
# a case starts (pids in $T/*.dpid) are stopped on exit.
export N2_TEST_ROOT="$T"
trap 'for p in $(cat "$T"/*.dpid 2>/dev/null); do kill "$p" 2>/dev/null; done; chmod -R u+rwx "$T"; rm -rf "$T"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
n=0 bad=0
ok() { n=$((n + 1)); printf 'ok: %s\n' "$1"; }
no() { n=$((n + 1)); bad=$((bad + 1)); printf 'FAIL: %s (%s)\n' "$1" "$2"; }

# mkcx <file> <version>: a stub codex, a compiled program (lib.sh refuses a #! script, F3).
# --version prints "codex-cli <version>" (fixed at build time: env -i drops any variable); any other
# call writes its environment, argv[0] and arguments under $HOME and prints "stub". Compiled, because
# a shell adds PWD, SHLVL and _ to the environment it reports.
cat >"$T/stub.c" <<'C'
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
extern char **environ;
static int cmp(const void *a, const void *b) { return strcmp(*(char *const *)a, *(char *const *)b); }
static FILE *out(const char *name) {
  char p[4096]; const char *h = getenv("HOME");
  snprintf(p, sizeof p, "%s/%s", h ? h : ".", name);
  FILE *f = fopen(p, "w"); if (!f) { perror("stub"); exit(2); } return f;
}
int main(int argc, char **argv) {
  if (argc == 2 && strcmp(argv[1], "--version") == 0) { printf("codex-cli %s\n", VER); return 0; }
  int n = 0; while (environ[n]) n++;
  qsort(environ, n, sizeof *environ, cmp);
  FILE *f = out("stub-env.txt"); for (int i = 0; i < n; i++) fprintf(f, "%s\n", environ[i]); fclose(f);
  f = out("stub-exe.txt"); fprintf(f, "%s\n", argv[0]); fclose(f);
  f = out("stub-args.txt"); for (int i = 1; i < argc; i++) fprintf(f, "%s%s", i > 1 ? " " : "", argv[i]); fprintf(f, "\n"); fclose(f);
  printf("stub\n"); return 0;
}
C
mkcx() { cc -DVER="\"$2\"" -o "$1" "$T/stub.c" || { echo "lib tests: cc failed"; exit 1; }; }
# mkbin <dir> <with claude: 0|1> [<stub codex version>]
mkbin() {
  mkdir -p "$1"
  for t in mktemp rm mkdir chmod dirname sleep cat cp mv rmdir sed head env grep tee python3 git cut tr ls shasum date file uname ps; do ln -s "$(command -v "$t")" "$1/$t"; done
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
# a file mine). Sets XENV (extra env words; no path here holds a blank) and resets PRE (lines the
# child runs before it sources lib.sh: a runner sets N2_AGENTS itself), CP and NR.
prep() {
  mkdir -p "$T/tmp-$1"
  XENV="" PRE="" CP=$T/bin NR=0
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
    [ -z "$PRE" ] || printf '%s\n' "$PRE"
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
CX_NAMES="ALL_PROXY CODEX_HOME HOME HTTPS_PROXY HTTP_PROXY MOCK_API_KEY PATH TERM TMPDIR TZ XDG_CACHE_HOME XDG_CONFIG_HOME XDG_DATA_HOME XDG_STATE_HOME "
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
cxprep() { prep "$1" "$2"; ERR=$T/$1.err; PRE="N2_AGENTS=codex"; XENV="$XENV CODEX_FX=$T/fx-$1"; }
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
# cxcopy <run folder>: the one codex copy folder (codex-pkg.XXXXXX) in it, else exit 1.
cxcopy() { [ "$(ls -d "$1"/codex-pkg.* 2>/dev/null | wc -l | tr -d ' ')" = 1 ] && ls -d "$1"/codex-pkg.*; }
# isolation_is <case> <version> [<run versions, default: the version>]
isolation_is() {
  f=$T/fx-$1/runs/base/isolation.txt
  [ "$(cat "$f" 2>/dev/null)" = "home=scratch
codex_home=scratch
codex_version=$2
run_versions=${3:-$2}
codex_exe=scratch
env_names=${CX_NAMES% }
proxy=mock" ] || { why="isolation.txt: $(tr '\n' '|' <"$f" 2>/dev/null)"; return 1; }
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
c=X3; cxprep $c caller; XENV="$XENV TERM=dumb TZ=America/Los_Angeles"; child $c 0 "$X3ACT"; run $c; killmock $c
x3() {
  E=$T/keep-X3/x/home/stub-env.txt
  [ -f "$E" ] || { why="no $E"; return 1; }
  names=$(cut -d= -f1 "$E" | LC_ALL=C sort | tr '\n' ' ')
  [ "$names" = "$CX_NAMES" ] || { why="names: $names"; return 1; }
  [ "$(envval "$E" HOME)" = "$T/keep-X3/x/home" ] || { why="HOME=$(envval "$E" HOME)"; return 1; }
  [ "$(envval "$E" CODEX_HOME)" = "$T/keep-X3/x/home/.codex" ] || { why="CODEX_HOME=$(envval "$E" CODEX_HOME)"; return 1; }
  [ "$(envval "$E" TERM)" = xterm-256color ] || { why="TERM=$(envval "$E" TERM) (the caller's TERM is dumb)"; return 1; }
  [ "$(envval "$E" TZ)" = UTC ] || { why="TZ=$(envval "$E" TZ) (the caller's TZ is America/Los_Angeles)"; return 1; }
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
  K=$(cxcopy "$T/keep-X8/x") || { why="not one codex-pkg.* folder"; return 1; }
  [ "$(cat "$T/keep-X8/x/home/stub-exe.txt" 2>/dev/null)" = "$K/p/bin/codex" ] ||
    { why="stub-exe.txt: $(cat "$T/keep-X8/x/home/stub-exe.txt" 2>/dev/null)"; return 1; }
  [ -f "$K/p/codex-package.json" ] || { why="no codex-package.json in the copy"; return 1; }
  python3 -I -c 'import os, sys
b = os.path.realpath(sys.argv[2])
sys.exit(any(os.path.realpath(p) == b or os.path.realpath(p).startswith(b + "/") for p in sys.argv[1].split(":")))' \
    "$(envval "$E" PATH)" "$T/hX8/.codex" || { why="a PATH entry is under ~/.codex: $(envval "$E" PATH)"; return 1; }
}
res "X8 a codex under ~/.codex runs from a scratch copy of its package" 0 x8

c=X9; cxprep $c caller; mkdir -p "$T/hX9/.codex/scr"; XENV="CODEX_FX=$T/fx-$c HOME=$T/hX9 N2_SCRATCH=$T/hX9/.codex/scr"
child $c 0 "new_run x; start_mock; $(mockrec X9); run_codex exec hi"; run $c; killmock $c
x9() { out_has X9 'premises: the run folder is under ~/.codex' && no_stub_env "$T/hX9"; }
res "X9 a run folder under ~/.codex stops before codex starts" 1 x9

c=X10; cxprep $c caller; child $c 0 "new_run x; start_mock; $(mockrec X10); EXTRA_ENV='HOME=$T/elsewhere-X10'; run_codex exec hi"; run $c; killmock $c
x10() { out_has X10 'premises: EXTRA_ENV may not set HOME' && no_stub_env "$T/keep-X10"; }
res "X10 EXTRA_ENV may not set HOME" 1 x10

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

# --- Review round 1 (F1-F11) -------------------------------------------------------------------
# X14 (F1): EXTRA_ENV may not set a name codex_env sets, a proxy or CA name in either case, any
# XDG_* or CODEX_* name, TMPDIR, TZ (PC-10). The check comes before the mock check, so no mock runs.
for w in CODEX_HOME=x PATH=x TERM=x TZ=x tz=x TMPDIR=x MOCK_API_KEY=x HTTPS_PROXY= HTTP_PROXY= ALL_PROXY= \
  https_proxy=x http_proxy=x all_proxy=x 'NO_PROXY=*' 'no_proxy=*' SSL_CERT_FILE=x SSL_CERT_DIR=x \
  CURL_CA_BUNDLE=x REQUESTS_CA_BUNDLE=x XDG_CONFIG_HOME=x XDG_RUNTIME_DIR=x CODEX_SQLITE_HOME=x; do
  c=X14; cxprep $c caller; child $c 0 "new_run x; EXTRA_ENV='$w'; run_codex exec hi"; run $c
  x14() { out_has X14 "premises: EXTRA_ENV may not set ${w%%=*}" && no_stub_env "$T/keep-X14"; }
  res "X14 EXTRA_ENV may not set ${w%%=*}" 1 x14
  rm -rf "$T/keep-X14"
done
c=X14b; cxprep $c caller; child $c 0 "new_run x; EXTRA_ENV='RUST_LOG=1 justaword'; run_codex exec hi"; run $c
x14b() { out_has X14b "premises: EXTRA_ENV word 'justaword' is not NAME=value" && no_stub_env "$T/keep-X14b"; }
res "X14b EXTRA_ENV words are NAME=value" 1 x14b

# X15 (F1, G2): an allowed name with a plain value runs; a value that names a path is refused
# (review round 2: no /, \, ~ or .., so no path at all: absolute, relative or prefixed).
c=X15; cxprep $c caller; XENV="$XENV HOME=$T"
child $c 0 "new_run x; start_mock; $(mockrec X15); EXTRA_ENV='RUST_LOG=codex_core=debug,info'; run_codex exec hi; EXTRA_ENV='RUST_LOG=/usr:$T/other-X15'; run_codex exec hi"
run $c; killmock $c
x15() {
  out_has X15 "premises: EXTRA_ENV RUST_LOG value may hold only A-Za-z0-9_.:@%+,=- and no ..: '/usr:$T/other-X15'" || return 1
  E=$T/keep-X15/x/home/stub-env.txt
  [ "$(envval "$E" RUST_LOG)" = "codex_core=debug,info" ] || { why="first run: RUST_LOG=$(envval "$E" RUST_LOG)"; return 1; }
}
res "X15 an EXTRA_ENV value with a path is refused, a plain value runs" 1 x15
# X15b (F1, G2): relative, prefixed, home, backslash and .. values are refused before codex starts.
for v in '../../../owner/.codex' 'x@/owner/.codex' '~x' 'a\b' 'a..b'; do
  c=X15b; cxprep $c caller; mkdir -p "$T/hX15b"; XENV="$XENV HOME=$T/hX15b"
  child $c 0 "new_run x; EXTRA_ENV='RUST_LOG=$v'; run_codex exec hi"; run $c
  res "X15b EXTRA_ENV value '$v' is refused" 1 out_has X15b "premises: EXTRA_ENV RUST_LOG value may hold only A-Za-z0-9_.:@%+,=- and no ..: '$v'"
  rm -rf "$T/keep-X15b"
done
# X15c (G2): a name that is not on the allow-list (EXTRA_ENV_ALLOW in lib.sh) is refused.
for w in ZDOTDIR=x SHELL=zsh OK1=1; do
  c=X15c; cxprep $c caller; child $c 0 "new_run x; EXTRA_ENV='$w'; run_codex exec hi"; run $c
  res "X15c EXTRA_ENV name ${w%%=*} is not on the allow-list" 1 out_has X15c "premises: EXTRA_ENV name ${w%%=*} is not in EXTRA_ENV_ALLOW"
  rm -rf "$T/keep-X15c"
done

# X16, X17 (F2): codex_record derives each line from what the runs used ($RUN/cx-used.d): a HOME outside
# the run folder or a proxy that is not the mock is written as such, and codex_record exits 1.
c=X16; cxprep $c caller
child $c 0 "new_run x; start_mock; $(mockrec X16); run_codex exec hi; stop_mock; for f in \"\$RUN\"/cx-used.d/*; do sed 's| HOME=[^ ]*| HOME=$T/elsewhere-X16|' \"\$f\" >\"\$RUN/u\"; mv \"\$RUN/u\" \"\$f\"; done; codex_record base"
run $c; killmock $c
x16() {
  out_has X16 "premises: codex_record: HOME $T/elsewhere-X16 is not under the run folder" || return 1
  grep -qx 'home=outside-run' "$T/fx-X16/runs/base/isolation.txt" || { why="isolation.txt: $(tr '\n' '|' <"$T/fx-X16/runs/base/isolation.txt")"; return 1; }
}
res "X16 codex_record records a HOME outside the run folder and exits 1" 1 x16
c=X17; cxprep $c caller
child $c 0 "new_run x; start_mock; $(mockrec X17); run_codex exec hi; stop_mock; for f in \"\$RUN\"/cx-used.d/*; do sed 's| HTTP_PROXY=[^ ]*| HTTP_PROXY=http://127.0.0.1:1|' \"\$f\" >\"\$RUN/u\"; mv \"\$RUN/u\" \"\$f\"; done; codex_record base"
run $c; killmock $c
x17() {
  out_has X17 "premises: codex_record: HTTP_PROXY is not the mock" || return 1
  grep -qx 'proxy=other' "$T/fx-X17/runs/base/isolation.txt" || { why="isolation.txt: $(tr '\n' '|' <"$T/fx-X17/runs/base/isolation.txt")"; return 1; }
}
res "X17 codex_record records a proxy that is not the mock and exits 1" 1 x17

# X18 (F3, G1): the codex that runs must be a native program (Mach-O, ELF or PE magic bytes); a
# script is refused before anything runs, with or without #!, after a BOM or a blank line.
mkbin "$T/cx18" 1; rm "$T/cx18/codex"
for k in shebang plain bom blank text mz-text elf-text; do
  case $k in
    shebang) printf '#!/bin/sh\nexec "$HOME/.codex/bin/codex" "$@"\n' ;;
    plain) printf 'exec "$HOME/.codex/bin/codex" "$@"\n' ;;
    bom) printf '\357\273\277#!/bin/sh\nexec "$HOME/.codex/bin/codex" "$@"\n' ;;
    blank) printf '\n#!/bin/sh\nexec "$HOME/.codex/bin/codex" "$@"\n' ;;
    text) printf 'MZ' | cut -c1 ;;
    # H1: macOS answers ENOEXEC for these and env runs them with /bin/sh.
    mz-text) printf 'MZ=1\n: >"$HOME/ran-by-sh"\necho "codex-cli 0.172.0"\n' ;;
    elf-text) printf '\177ELF\n: >"$HOME/ran-by-sh"\necho "codex-cli 0.172.0"\n' ;;
  esac >"$T/cx18/codex"
  chmod +x "$T/cx18/codex"
  c=X18; cxprep $c caller; CP=$T/cx18; child $c 0 "new_run x; start_mock; $(mockrec X18); run_codex exec hi"; run $c; killmock $c
  x18() {
    out_has X18 "premises: codex at $T/cx18/codex is not a native program (Mach-O, ELF or PE)" && no_stub_env "$T/keep-X18" || return 1
    [ -z "$(find "$T/keep-X18" -name ran-by-sh)" ] || { why="the text ran under sh"; return 1; }
  }
  res "X18 a codex that is not a native program is refused ($k)" 1 x18
  rm -rf "$T/keep-X18"
done
# X18b (H1): the magic bytes alone are not enough. `file -b` must name an executable of the host's
# own format (Mach-O on Darwin, ELF on Linux, PE32 on Windows): ELF and PE headers that are not
# whole programs are refused before anything runs.
for k in elf pe; do
  case $k in elf) printf '\177ELF\002\001\001' ;; pe) printf 'MZ\220\000' ;; esac >"$T/cx18/codex"
  c=X18b; cxprep $c caller; CP=$T/cx18; child $c 0 "new_run x; start_mock; $(mockrec X18b); run_codex exec hi"; run $c; killmock $c
  res "X18b $k magic bytes alone are refused" 1 out_has X18b "premises: codex at $T/cx18/codex is not a native program (Mach-O, ELF or PE)"
  rm -rf "$T/keep-X18b"
done
# X18c (H1): the compiled stub (a native executable of this host) passes n2_native.
c=X18c; cxprep $c caller; child $c 0 'if n2_native "$CODEX_BIN"; then echo native=0; fi; exit 0'; run $c
res "X18c a compiled program of this host passes the native check" 0 out_has X18c "native=0"
# X18d (H1): a native program of the other host format is refused (a stub uname names the other
# system: Linux on Darwin, Darwin elsewhere).
mkbin "$T/cx18d" 1
case "$(uname -s)" in Darwin) _o=Linux ;; *) _o=Darwin ;; esac
rm "$T/cx18d/uname"; printf '#!/bin/sh\necho %s\n' "$_o" >"$T/cx18d/uname"; chmod +x "$T/cx18d/uname"
c=X18d; cxprep $c caller; CP=$T/cx18d; child $c 0 "new_run x; start_mock; $(mockrec X18d); run_codex exec hi"; run $c; killmock $c
x18d() { out_has X18d "premises: codex at $T/cx18d/codex is not a native program (Mach-O, ELF or PE)" && no_stub_env "$T/keep-X18d"; }
res "X18d a native program of another host format ($_o) is refused" 1 x18d

# X19 (F3, G9): a codex outside ~/.codex also runs from a copy in the run folder; PATH's first entry is
# that copy (each run folder has its own: no copy is shared between runs or processes).
c=X19; cxprep $c caller
child $c 0 "new_run x; start_mock; $(mockrec X19); run_codex exec hi; stop_mock; exit 0"; run $c; killmock $c
x19() {
  E=$T/keep-X19/x/home/stub-env.txt
  K=$(cxcopy "$T/keep-X19/x") || { why="not one codex-pkg.* folder"; return 1; }
  [ "$(cat "$T/keep-X19/x/home/stub-exe.txt" 2>/dev/null)" = "$K/p/codex" ] ||
    { why="stub-exe.txt: $(cat "$T/keep-X19/x/home/stub-exe.txt" 2>/dev/null)"; return 1; }
  [ "$(envval "$E" PATH | cut -d: -f1)" = "$K/p" ] || { why="PATH=$(envval "$E" PATH)"; return 1; }
}
res "X19 a codex outside ~/.codex runs from a copy in the run folder, first on PATH" 0 x19

# X20 (F4): a package whose copy holds a link out of the copy is refused and the copy removed.
c=X20; cxprep $c caller
PK=$T/hX20/.codex/packages/standalone/releases/v
mkdir -p "$PK/bin" "$T/hX20/.local/bin"; mkcx "$PK/bin/codex" 0.172.0
ln -s "$T/hX20/.codex" "$PK/owner"; ln -s bin/codex "$PK/codex"
ln -s "$PK/bin/codex" "$T/hX20/.local/bin/codex"
CP=$T/hX20/.local/bin:$T/cx8; XENV="$XENV HOME=$T/hX20"
child $c 0 "new_run x; start_mock; $(mockrec X20); run_codex exec hi"; run $c; killmock $c
x20() {
  out_has X20 "premises: the codex package copy holds a link out of it: owner" || return 1
  [ -z "$(ls -A "$T/keep-X20/x" | grep '^codex-pkg')" ] || { why="left: $(ls -A "$T/keep-X20/x" | tr '\n' ' ')"; return 1; }
  no_stub_env "$T/keep-X20" "$T/hX20"
}
res "X20 a package copy with a link out of it is refused and removed" 1 x20

# X21-X23 (F5): a run that fails after start_mock leaves no mock running (start_mock's EXIT trap).
# alive <pid file>: 0 when the process named in it still runs.
alive() { [ -s "$1" ] && kill -0 "$(cat "$1")" 2>/dev/null; }
c=X21; cxprep $c caller; child $c 0 "new_run x; start_mock; $(mockrec X21); H='$T/elsewhere-X21'; run_codex exec hi"; run $c
x21() { [ -s "$T/X21.mock" ] || { why="no pid"; return 1; }; ! alive "$T/X21.mock" || { why="mock $(cat "$T/X21.mock") still runs"; return 1; }; }
res "X21 codex_env failing after start_mock stops the mock (caller dir)" 1 x21
killmock X21
c=X22; cxprep $c own; child $c 0 "new_run x; start_mock; $(mockrec X22); H='$T/elsewhere-X22'; run_codex exec hi"; run $c
x22() { ! alive "$T/X22.mock" || { why="mock still runs"; return 1; }; kept_n X22 1; }
res "X22 codex_env failing after start_mock stops the mock, own dir kept" 1 x22
killmock X22
c=X23; cxprep $c caller
child $c 0 "trap 'echo prev \$? >\"$T/X23.prev\"' EXIT; new_run x; start_mock; $(mockrec X23); H='$T/elsewhere-X23'; run_codex exec hi"; run $c
x23() {
  ! alive "$T/X23.mock" || { why="mock still runs"; return 1; }
  [ "$(cat "$T/X23.prev" 2>/dev/null)" = "prev 1" ] || { why="earlier EXIT trap: $(cat "$T/X23.prev" 2>/dev/null)"; return 1; }
}
res "X23 start_mock chains an earlier EXIT trap" 1 x23
killmock X23

# X24 (F6): an existing config.toml whose base_url is not the running mock is refused.
c=X24; cxprep $c caller
child $c 0 "new_run x; mkdir -p \"\$H/.codex\"; echo 'base_url = \"http://127.0.0.1:1/v1\"' >\"\$H/.codex/config.toml\"; start_mock; $(mockrec X24); run_codex exec hi"
run $c; killmock $c
x24() {
  P=$(cat "$T/keep-X24/x/port" 2>/dev/null)
  out_has X24 "premises: config.toml base_url is 'http://127.0.0.1:1/v1', not 'http://127.0.0.1:$P/v1'" && no_stub_env "$T/keep-X24"
}
res "X24 a config.toml base_url that is not the mock is refused" 1 x24
c=X24b; cxprep $c caller
child $c 0 "new_run x; start_mock; $(mockrec X24b); run_codex exec hi; stop_mock; start_mock; run_codex exec hi"
run $c; killmock $c
x24b() { grep -q "^premises: config.toml base_url is 'http://127.0.0.1:[0-9]*/v1', not 'http://127.0.0.1:[0-9]*/v1'\$" "$T/X24b.out" || { why="stdout: $(tr '\n' '|' <"$T/X24b.out")"; return 1; }; }
res "X24b a mock restart in one run is refused (old base_url)" 1 x24b

# X25 (F9, H7): TERM is the constant xterm-256color; the caller's TERM (here with a blank) is not used.
c=X25; cxprep $c caller; child $c 0 "new_run x; start_mock; $(mockrec X25); TERM='xterm 256'; run_codex exec hi; stop_mock; exit 0"; run $c; killmock $c
x25() { [ "$(envval "$T/keep-X25/x/home/stub-env.txt" TERM)" = xterm-256color ] || { why="TERM=$(envval "$T/keep-X25/x/home/stub-env.txt" TERM)"; return 1; }; }
res "X25 codex gets TERM=xterm-256color, not the caller's TERM" 0 x25

# X26 (F10, G5): a codex-pkg left in N2_SCRATCH (here a script) is never used: each codex_env
# copies the program fresh into the run folder.
c=X26; cxprep $c caller; mkdir -p "$T/keep-X26/codex-pkg"
printf '#!/bin/sh\n: >"$HOME/X26-script-ran"\n' >"$T/keep-X26/codex-pkg/codex"; chmod +x "$T/keep-X26/codex-pkg/codex"
child $c 0 "new_run x; start_mock; $(mockrec X26); run_codex exec hi; stop_mock; exit 0"; run $c; killmock $c
x26() {
  K=$(cxcopy "$T/keep-X26/x") || { why="not one codex-pkg.* folder"; return 1; }
  [ "$(cat "$T/keep-X26/x/home/stub-exe.txt" 2>/dev/null)" = "$K/p/codex" ] || { why="codex did not run from the run folder's copy"; return 1; }
  [ ! -e "$T/keep-X26/x/home/X26-script-ran" ] || { why="the old codex-pkg ran"; return 1; }
}
res "X26 an old codex-pkg in N2_SCRATCH is not used" 0 x26
# X26b (G5): a copy changed after a run (here replaced by a script) is copied fresh for the next run.
c=X26b; cxprep $c caller; XENV="$XENV HOME=$T/hX8"; CP=$T/hX8/.local/bin:$T/cx8
child $c 0 "new_run x; start_mock; $(mockrec X26b); run_codex exec a; printf '#!/bin/sh\\n: >\"\$HOME/X26b-script-ran\"\\n' >\"\$CODEX_EXE\"; run_codex exec b; stop_mock; exit 0"
run $c; killmock $c
x26b() {
  [ "$(cat "$T/keep-X26b/x/home/stub-args.txt" 2>/dev/null)" = "exec b" ] || { why="the second run did not run the stub: $(cat "$T/keep-X26b/x/home/stub-args.txt" 2>/dev/null)"; return 1; }
  [ ! -e "$T/keep-X26b/x/home/X26b-script-ran" ] || { why="the changed copy ran"; return 1; }
  [ "$(ls -d "$T/keep-X26b/x"/codex-pkg.* | wc -l | tr -d ' ')" = 2 ] || { why="not one copy folder per run"; return 1; }
  for K in "$T/keep-X26b/x"/codex-pkg.*; do [ -f "$K/p/codex-package.json" ] || { why="the package copy $K is not complete"; return 1; }; done
}
res "X26b each run_codex copies the package fresh" 0 x26b

# X27 (F11): N2_AGENTS from the environment is refused; only the runner's own setting counts.
c=X27; prep $c own; ERR=$T/$c.err; XENV="N2_AGENTS=codex"; CP=$T/cxonly; child $c 0 'exit 0'; run $c
x27() {
  out_has X27 'premises: N2_AGENTS comes from the environment; only the runner sets it (unset N2_AGENTS)' || return 1
  [ -z "$(ls -A "$T/tmp-X27")" ] || { why="$T/tmp-X27 holds: $(ls -A "$T/tmp-X27")"; return 1; }
}
res "X27 an exported N2_AGENTS is refused" 1 x27

# --- Review round 2 (G1-G10) -------------------------------------------------------------------
# X28 (G3): the owner's ~/.codex in another letter case is still the owner's ~/.codex (the checks
# that keep a path out of it compare case-folded; macOS folders ignore case).
c=X28; cxprep $c caller; mkdir -p "$T/hX28/.codex/scr"; XENV="CODEX_FX=$T/fx-$c HOME=$T/hX28 N2_SCRATCH=$T/HX28/.CODEX/scr"
child $c 0 "new_run x; start_mock; $(mockrec X28); run_codex exec hi"; run $c; killmock $c
x28() { out_has X28 'premises: the run folder is under ~/.codex' && no_stub_env "$T/hX28"; }
res "X28 an N2_SCRATCH under an upper-cased ~/.codex is refused" 1 x28
c=X28b; cxprep $c caller; mkdir -p "$T/hX28b/.codex"; XENV="$XENV HOME=$T/hX28b"
child $c 0 "new_run x; start_mock; $(mockrec X28b); RUN='$T/HX28B/.CODEX/run'; mkdir -p \"\$RUN/home\"; H=\$RUN/home; CX_HOME=\$H/.codex; run_codex exec hi"
run $c; killmock $c
x28b() { out_has X28b 'premises: the run folder is under ~/.codex' && no_stub_env "$T/hX28b"; }
res "X28b a run folder under an upper-cased ~/.codex is refused" 1 x28b

# X29 (G4): a run_codex inside $(...) or a pipeline (a subshell) is in the record too: each run
# appends its line to $RUN/cx-used, and codex_record reads that file.
c=X29; cxprep $c caller
child $c 0 "new_run x; start_mock; $(mockrec X29); EXTRA_ENV=RUST_LOG=trace; out=\$(run_codex exec a); EXTRA_ENV=RUST_BACKTRACE=1; run_codex exec b | cat >/dev/null; EXTRA_ENV=; stop_mock; codex_record base; exit 0"
run $c; killmock $c
x29() {
  f=$T/fx-X29/runs/base/isolation.txt
  grep -qx 'codex_version=0.172.0' "$f" 2>/dev/null || { why="isolation.txt: $(tr '\n' '|' <"$f" 2>/dev/null)"; return 1; }
  grep -qx "env_names=ALL_PROXY CODEX_HOME HOME HTTPS_PROXY HTTP_PROXY MOCK_API_KEY PATH RUST_BACKTRACE RUST_LOG TERM TMPDIR TZ XDG_CACHE_HOME XDG_CONFIG_HOME XDG_DATA_HOME XDG_STATE_HOME" "$f" ||
    { why="isolation.txt: $(tr '\n' '|' <"$f")"; return 1; }
}
res "X29 runs in \$(...) and in a pipeline are recorded" 0 x29

# X30 (G6): a second start_mock while the first mock runs is refused; the first mock is stopped.
c=X30; cxprep $c caller; child $c 0 "new_run x; start_mock; $(mockrec X30); start_mock; echo second >'$T/X30.second'"; run $c
x30() {
  grep -q '^premises: start_mock: the mock (pid [0-9]*) still runs; stop_mock first$' "$T/X30.out" || { why="stdout: $(tr '\n' '|' <"$T/X30.out")"; return 1; }
  [ ! -e "$T/X30.second" ] || { why="the second start_mock went on"; return 1; }
  ! alive "$T/X30.mock" || { why="the first mock still runs"; return 1; }
}
res "X30 a second start_mock is refused and the first mock stopped" 1 x30
killmock X30

# X21sh (G10): the mock trap path under /bin/sh (the other cases run under LIB_TEST_SH or dash).
c=X21sh; cxprep $c caller; child $c 0 "new_run x; start_mock; $(mockrec X21sh); H='$T/elsewhere-X21sh'; run_codex exec hi"
_sh=$SH; SH=/bin/sh; run $c; SH=$_sh
x21sh() { [ -s "$T/X21sh.mock" ] || { why="no pid"; return 1; }; ! alive "$T/X21sh.mock" || { why="mock still runs"; return 1; }; }
res "X21sh codex_env failing after start_mock stops the mock under /bin/sh" 1 x21sh
killmock X21sh

# --- Review round 3 (H1-H7) -------------------------------------------------------------------
# X28c (H5): the owner's ~/.codex spelled in NFD is the same folder as in NFC (APFS ignores the
# normalization form): n2_under_fold compares NFC, case-folded real paths.
NFC=$(printf 'Jos\303\251') NFD=$(printf 'Jose\314\201')
mkdir -p "$T/h$NFC/.codex/packages"
c=X28c; cxprep $c caller
child $c 0 "if n2_under_fold '$T/h$NFD/.codex/packages/x' '$T/h$NFC/.codex'; then echo nfd=0; fi; if n2_under_fold '$T/h$NFC/.codex/packages/x' '$T/h$NFD/.codex'; then echo nfc=0; fi; exit 0"; run $c
x28c() { out_has X28c "nfd=0" && out_has X28c "nfc=0"; }
res "X28c a path under ~/.codex in NFD is under the NFC spelling (and back)" 0 x28c

# X31 (H2): the version is checked on every codex_env (each copies the program fresh), each record
# holds its own version, and codex_record exits 1 when the records hold more than one version.
# The install changes between two runs (a link on PATH swapped to another program).
mkdir -p "$T/cx31/bin" "$T/cx31/v172" "$T/cx31/v173" "$T/cx31/v171"
mkcx "$T/cx31/v172/codex" 0.172.0; mkcx "$T/cx31/v173/codex" 0.173.0; mkcx "$T/cx31/v171/codex" 0.171.9
mkbin "$T/cx31p" 1; rm "$T/cx31p/codex"
LN=$(command -v ln)
c=X31; cxprep $c caller; ln -sfn "$T/cx31/v172/codex" "$T/cx31/bin/codex"; CP=$T/cx31/bin:$T/cx31p
child $c 0 "new_run x; start_mock; $(mockrec X31); run_codex exec a; $LN -sfn '$T/cx31/v173/codex' '$T/cx31/bin/codex'; run_codex exec b; stop_mock; codex_record base"
run $c; killmock $c
x31() {
  out_has X31 "premises: codex_record: more than one codex version: 0.172.0,0.173.0" || return 1
  grep -qx 'run_versions=0.172.0,0.173.0' "$T/fx-X31/runs/base/isolation.txt" || { why="isolation.txt: $(tr '\n' '|' <"$T/fx-X31/runs/base/isolation.txt")"; return 1; }
}
res "X31 a codex that changes between runs is recorded and codex_record exits 1" 1 x31
c=X31b; cxprep $c caller; ln -sfn "$T/cx31/v172/codex" "$T/cx31/bin/codex"; CP=$T/cx31/bin:$T/cx31p
child $c 0 "new_run x; start_mock; $(mockrec X31b); run_codex exec a; $LN -sfn '$T/cx31/v171/codex' '$T/cx31/bin/codex'; run_codex exec b"
run $c; killmock $c
x31b() { out_has X31b 'premises: codex 0.171.9 is below the minimum 0.172.0' && [ "$(cat "$T/keep-X31b/x/home/stub-args.txt")" = "exec a" ] || { why="${why:-the second run ran}"; return 1; }; }
res "X31b a codex below the minimum at the second run does not run" 1 x31b
c=X31c; cxprep $c caller
child $c 0 "new_run x; start_mock; $(mockrec X31c); run_codex exec hi; stop_mock; for f in \"\$RUN\"/cx-used.d/*; do sed 's|^0.172.0 |0.100.0 |' \"\$f\" >\"\$RUN/u\"; mv \"\$RUN/u\" \"\$f\"; done; codex_record base"
run $c; killmock $c
res "X31c a record with a version below the minimum makes codex_record exit 1" 1 out_has X31c "premises: codex_record: codex 0.100.0 is below the minimum 0.172.0"

# X32 (H4): two run_codex at once in one run folder (records longer than one pipe write): each
# writes its own record file and runs its own copy (no copy is removed by the other). No timing is
# asserted.
LONG=$(python3 -I -c 'print("a" * 1400)')
c=X32; cxprep $c caller
child $c 0 "new_run x; start_mock; $(mockrec X32); EXTRA_ENV=RUST_LOG=$LONG; run_codex exec a & _a=\$!; run_codex exec b & _b=\$!; wait \$_a; wait \$_b; stop_mock; codex_record base; exit 0"
run $c; killmock $c
x32() {
  D=$T/keep-X32/x/cx-used.d
  [ "$(ls "$D" 2>/dev/null | wc -l | tr -d ' ')" = 2 ] || { why="records: $(ls "$D" 2>/dev/null | tr '\n' ' ')"; return 1; }
  for f in "$D"/*; do
    [ "$(wc -l <"$f" | tr -d ' ')" = 1 ] || { why="$f does not hold one line"; return 1; }
    grep -q "^0\.172\.0 http://127\.0\.0\.1:[0-9]* [^ ]*/codex-pkg\.[^ /]*/p/codex env -i .* RUST_LOG=$LONG\$" "$f" || { why="$f: $(cut -c1-200 "$f")"; return 1; }
    [ -f "$(cut -d' ' -f3 "$f")" ] || { why="the copy of $f was removed"; return 1; }
  done
  [ "$(cut -d' ' -f3 "$D"/* | sort -u | wc -l | tr -d ' ')" = 2 ] || { why="the two runs share a copy"; return 1; }
  grep -qx 'run_versions=0.172.0,0.172.0' "$T/fx-X32/runs/base/isolation.txt" || { why="isolation.txt: $(tr '\n' '|' <"$T/fx-X32/runs/base/isolation.txt" 2>/dev/null)"; return 1; }
}
res "X32 two concurrent run_codex keep both records and copies intact" 0 x32

# X33 (H6): the EXIT trap of a chain of three holds no earlier trap twice (the old chain copied the
# earlier trap into both branches of an if: 4 copies of the first at the third level); the chain
# runs newest first and the first trap gets the script's status.
c=X33; cxprep $c caller
child $c 0 "new_run x; trap 'echo \"base \$?\" >>\"$T/X33.log\"' EXIT; n2_trap_chain 'echo a >>\"$T/X33.log\"'; n2_trap_chain 'echo b >>\"$T/X33.log\"'; n2_trap_chain 'echo c >>\"$T/X33.log\"'; trap >'$T/X33.traps'; exit 3"
run $c
x33() {
  [ "$(tr '\n' ' ' <"$T/X33.log" 2>/dev/null)" = "c b a base 3 " ] || { why="log: $(tr '\n' ' ' <"$T/X33.log" 2>/dev/null)"; return 1; }
  for w in 'echo a' 'echo b' 'base'; do
    k=$(grep 'EXIT$' "$T/X33.traps" | grep -o "$w" | wc -l | tr -d ' ')
    [ "$k" -le 1 ] || { why="'$w' is in the EXIT trap $k times"; return 1; }
  done
}
res "X33 a chain of three holds no earlier trap twice" 3 x33

# --- B: codex/base.sh schema pin (F8) -------------------------------------------------------------
# A copy of the part with lib.sh, ver.sh, scrub.py and mock_upstream.py; CODEX_FX holds a schema.
mkdir -p "$T/B-pd/codex"
cp "$HERE/lib.sh" "$HERE/ver.sh" "$HERE/scrub.py" "$HERE/mock_upstream.py" "$T/B-pd/"
cp "$HERE/codex/base.sh" "$T/B-pd/codex/"
SCHEMA=$REPO/fixtures/codex/config.schema.json SRC=$REPO/fixtures/codex/config.schema.source.txt
PIN=52fb96b1ff79a51bf9ce31695a4a22d4e80f57591d91bad5747b657f4e612bac
# bprep <case>: CODEX_FX=$T/fx-<case> with the real schema and source file.
bprep() { prep "$1" caller; ERR=$T/$1.err; mkdir -p "$T/fx-$1"; cp "$SCHEMA" "$SRC" "$T/fx-$1/"; XENV="$XENV CODEX_FX=$T/fx-$1"; }
brun() {
  # shellcheck disable=SC2086
  env PATH="$CP" TMPDIR="$T/tmp-$1" $XENV "$SH" "$T/B-pd/codex/base.sh" >"$T/$1.out" 2>"$T/$1.err"
  rc=$?
}
c=B1; bprep $c; brun $c
b1() { out_has B1 'codex base: ok (codex 0.172.0)' && isolation_is B1 0.172.0 0.172.0,0.172.0,0.172.0,0.172.0; }
res "B1 base passes with the pinned schema" 0 b1
c=B2; bprep $c; printf 'x' >>"$T/fx-$c/config.schema.json"; brun $c
b2() {
  grep -q "^premises: config.schema.json sha256 is [0-9a-f]*, not the pinned $PIN\$" "$T/B2.out" || { why="stdout: $(tr '\n' '|' <"$T/B2.out")"; return 1; }
  [ ! -e "$T/fx-B2/runs" ] || { why="runs/ was written"; return 1; }
}
res "B2 a schema fixture that is not the pinned file fails" 1 b2
c=B3; bprep $c; sed 's/^sha256: .*/sha256: 00/' "$SRC" >"$T/fx-$c/config.schema.source.txt"; brun $c
res "B3 a source file with another sha256 fails" 1 out_has B3 "premises: config.schema.source.txt sha256 is '00', not the pinned $PIN"

# B4, B5 (G7): the schema fetch goes to a temporary file in the run folder, removed on a signal or a
# failed check; the fixture folder never holds a partial file. A local server stands in for the URL.
cat >"$T/srv.py" <<'PY'
import socket, sys, time
mode, port_file, ready = sys.argv[1:4]
s = socket.socket(); s.bind(("127.0.0.1", 0)); s.listen(5)
open(port_file, "w").write(str(s.getsockname()[1]))
c, _ = s.accept(); c.recv(4096)
if mode == "hang":
    c.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\n\r\n{\"partial\": ")
    open(ready, "w").close()
    time.sleep(30)
else:
    c.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
    c.close()
PY
# fprep <case> <hang|bad>: an empty CODEX_FX, a server, and a copy of base.sh that fetches from it.
fprep() {
  prep "$1" caller; ERR=$T/$1.err; mkdir -p "$T/fx-$1" "$T/$1-pd/codex"; XENV="$XENV CODEX_FX=$T/fx-$1"
  cp "$T/B-pd/lib.sh" "$T/B-pd/ver.sh" "$T/B-pd/scrub.py" "$T/B-pd/mock_upstream.py" "$T/$1-pd/"
  python3 -I "$T/srv.py" "$2" "$T/$1.port" "$T/$1.ready" & SRV=$!
  waitready "$T/$1.port"; sleep 0.1
  sed "s|^URL=.*|URL=http://127.0.0.1:$(cat "$T/$1.port")/s|" "$T/B-pd/codex/base.sh" >"$T/$1-pd/codex/base.sh"
}
# no_partial <case>: no file in CODEX_FX and no schema temporary in the run folder.
no_partial() {
  [ -z "$(ls -A "$T/fx-$1")" ] || { why="CODEX_FX holds: $(ls -A "$T/fx-$1" | tr '\n' ' ')"; return 1; }
  [ -z "$(ls -A "$T/keep-$1/codex-base" 2>/dev/null | grep '^schema')" ] || { why="run folder holds: $(ls -A "$T/keep-$1/codex-base" | tr '\n' ' ')"; return 1; }
}
c=B4; fprep $c hang
# shellcheck disable=SC2086
perl -e 'setpgrp(0, 0); exec @ARGV or die' env PATH="$CP" TMPDIR="$T/tmp-$c" $XENV "$SH" "$T/$c-pd/codex/base.sh" >"$T/$c.out" 2>"$T/$c.err" &
bp=$!
if waitready "$T/$c.ready"; then sleep 0.3; kill -s TERM -- "-$bp"; wait "$bp"; rc=$?; else NR=1; kill -s KILL -- "-$bp" 2>/dev/null; wait "$bp"; rc=$?; fi
kill "$SRV" 2>/dev/null; wait "$SRV" 2>/dev/null
res "B4 a TERM during the schema fetch leaves no partial file" 143 no_partial B4
c=B5; fprep $c bad
# shellcheck disable=SC2086
env PATH="$CP" TMPDIR="$T/tmp-$c" $XENV "$SH" "$T/$c-pd/codex/base.sh" >"$T/$c.out" 2>"$T/$c.err"; rc=$?
kill "$SRV" 2>/dev/null; wait "$SRV" 2>/dev/null
b5() { grep -q "^premises: the fetched schema sha256 is [0-9a-f]*, not the pinned $PIN\$" "$T/B5.out" || { why="stdout: $(tr '\n' '|' <"$T/B5.out")"; return 1; }; no_partial B5; }
res "B5 a fetched schema that is not the pinned file leaves no file" 1 b5

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

# --- PC-10: Codex daemons ------------------------------------------------------------------------
# A stub daemon: a compiled program that creates the file $N2D_READY and then waits until it is
# killed. A Codex TUI run can start "codex app-server --listen unix:// --managed-daemon" that
# outlives the run, with its socket in /private/tmp/codex-daemon-<uid> (or /tmp/...).
cat >"$T/dmn.c" <<'C'
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
int main(void) {
  const char *r = getenv("N2D_READY"); FILE *f = r ? fopen(r, "w") : NULL; if (f) fclose(f);
  for (;;) pause();
}
C
mkdir -p "$T/dmn"
cc -o "$T/dmn/codex" "$T/dmn.c" || { echo "lib tests: cc failed"; exit 1; }
# X34: a codex app-server outside the run folder: run_codex refuses before any codex starts, names
# its pid, and leaves it running.
N2D_READY=$T/X34.ready "$T/dmn/codex" app-server --listen unix:// --managed-daemon &
echo $! >"$T/X34.dpid"
waitready "$T/X34.ready" || echo "lib tests: X34 stub daemon not ready"
c=X34; cxprep $c caller; child $c 0 "new_run x; start_mock; $(mockrec X34); run_codex exec hi"; run $c; killmock $c
x34() {
  dpid=$(cat "$T/X34.dpid")
  out_has X34 "premises: a codex app-server runs outside this run folder (pid $dpid, $T/dmn/codex); codex not started; it is left running" || return 1
  no_stub_env "$T/keep-X34" || return 1
  kill -0 "$dpid" 2>/dev/null || { why="the stub daemon was stopped"; return 1; }
}
res "X34 a codex app-server outside the run folder: run_codex refuses and leaves it running" 1 x34
kill "$(cat "$T/X34.dpid")" 2>/dev/null; rm -f "$T/X34.dpid"
# X35: a socket in the per-uid daemon folder (/private/tmp and /tmp forms, under N2_TEST_ROOT).
for f in private/tmp tmp; do
  c=X35; cxprep $c caller; rm -rf "$T/private" "$T/tmp"
  D=$T/$f/codex-daemon-$(id -u)
  mkdir -p "$D"
  python3 -I -c 'import socket, sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' "$D/s"
  child $c 0 "new_run x; start_mock; $(mockrec X35); run_codex exec hi"; run $c; killmock $c
  x35() { out_has X35 "premises: $D holds a socket (a Codex daemon); codex not started" && no_stub_env "$T/keep-X35"; }
  res "X35 a socket in $f/codex-daemon-<uid>: run_codex refuses" 1 x35
  rm -rf "$T/private" "$T/tmp" "$T/keep-X35"
done
# X36: a process of the run folder still runs after the part (a daemon left by a TUI run):
# codex_record exits 1 and names its pid.
c=X36; cxprep $c caller
child $c 0 "new_run x; start_mock; $(mockrec X36); run_codex exec hi; mkdir -p \"\$RUN/pk\"; cp '$T/dmn/codex' \"\$RUN/pk/codex\"; N2D_READY='$T/X36.ready' \"\$RUN/pk/codex\" app-server --managed-daemon & echo \$! >'$T/X36.dpid'; i=0; while [ ! -f '$T/X36.ready' ] && [ \$i -lt 100 ]; do i=\$((i + 1)); sleep 0.1; done; stop_mock; codex_record base"
run $c; killmock $c
x36() { out_has X36 "premises: codex_record: a process of this run folder still runs (pid $(cat "$T/X36.dpid"), $T/keep-X36/x/pk/codex); stop it (TUI runs pass --no-daemon)"; }
res "X36 a process of the run folder left running: codex_record exits 1 and names it" 1 x36
kill "$(cat "$T/X36.dpid")" 2>/dev/null; rm -f "$T/X36.dpid"

# X37 (PC-10): start_mock hands MOCK_ALT_KEY (a shell variable, built at run time) to the mock,
# which accepts it and records credential "alt"; without it the same request aborts the mock.
ALT="dummy-""alt-key"
cat >"$T/alt.py" <<'PY'
import http.client, sys, urllib.parse
u = urllib.parse.urlsplit(sys.argv[1])
c = http.client.HTTPConnection(u.hostname, u.port, timeout=30)
c.request("POST", "/v1/responses", body=b'{"model":"m","input":"x"}',
          headers={"authorization": "Bearer " + sys.argv[2], "content-type": "application/json"})
print(c.getresponse().status)
PY
c=X37; cxprep $c caller
child $c 0 "new_run x; MOCK_ALT_KEY='$ALT'; start_mock; $(mockrec X37); python3 -I '$T/alt.py' \"\$MOCK_URL\" '$ALT' >'$T/X37.status'; stop_mock"
run $c; killmock $c
x37() {
  [ "$(cat "$T/X37.status")" = 200 ] || { why="status $(cat "$T/X37.status")"; return 1; }
  f=$T/keep-X37/x/rec/0001.json
  python3 -I -c 'import json, sys; sys.exit(json.load(open(sys.argv[1])).get("credential") != "alt")' "$f" ||
    { why="record: $(tr '\n' ' ' <"$f" 2>/dev/null | cut -c1-200)"; return 1; }
  ! grep -qF "$ALT" "$f" || { why="the record holds the alt key"; return 1; }
}
res "X37 start_mock passes MOCK_ALT_KEY; the mock accepts it and records credential alt" 0 x37
c=X37b; cxprep $c caller
child $c 0 "new_run x; start_mock; $(mockrec X37b); python3 -I '$T/alt.py' \"\$MOCK_URL\" '$ALT' >'$T/X37b.status'; stop_mock"
run $c; killmock $c
x37b() { [ "$(cat "$T/X37b.status")" = 401 ] && [ -f "$T/keep-X37b/x/rec/ABORT" ] || { why="status $(cat "$T/X37b.status"), no ABORT"; return 1; }; }
res "X37b without MOCK_ALT_KEY the alt key aborts the mock (check_abort: exit 3)" 3 x37b

if [ "$bad" -eq 0 ]; then echo "lib tests: ok ($n)"; exit 0; fi
echo "lib tests: $bad failed"
exit 1

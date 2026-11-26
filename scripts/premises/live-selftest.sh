#!/bin/sh
# Self-tests of the live-check harness (node N38, review rounds 3 and 4). Local only: no agent
# runs, no key, no login, no network beyond loopback. Every test token is built at run time from
# two parts; every tree it changes is a copy under its own temporary dir, deleted on exit.
# Usage: dash scripts/premises/live-selftest.sh   ->  "live selftest: ok (<n>)", exit 0
# Needs: dash or sh, perl, python3, curl, tmux, jq. No real agent: stub claude, opencode and zygo
# programs come first on PATH (ZYGO_BIN is the stub too), MOCK_URL is a dead loopback port, and the
# test stops before its first case unless `command -v` finds each stub (U4).
# Run it only from the checkout whose scripts it tests. No review or test may run this self-test
# against older versions of the scripts (no git archive, no checkout of old commits): an older
# version has no stubs and can start the real agent with the real HOME and login.
# - U4: every agent program started during the test is a stub (each writes a marker naming its
#   own path); a tmux_claude launch reaches the stub; no wrapped sleep is left at the end.
# - T1, U5: SIGINT, SIGTERM or SIGHUP to the script's process group during a LIVE_TIMEOUT-wrapped
#   command leaves no process of the wrapped command's own group, and the caller-set scratch dir
#   keeps live-<pid> without its pg-* files (FIX-7);
#   live_traps removes a stale pg-* file; live_end leaves a group whose leader is not the
#   recorded wrapper's child.
# - T3, U2, U3: live-passthrough.sh --scan-only in a copied tree, on a made-up HOME: passes on a
#   clean store (and writes the store-scan fixture); fails, without writing it, on a missing dir, a
#   dir with no file, an unreadable file, a stale token file, a link whose target is outside the
#   zygo dirs (named, nothing under it listed); follows a link inside them; with a session id, a
#   live session's token file is not counted, a stale one and the run's own one are, and so is one
#   whose url is file:, an option word, a dead port behind a live http_proxy, or not loopback
#   (that one is never fetched).
# - T4, T5, T8: check-premises.sh in a copied tree fails on each planted line (a key, id or
#   token right after a JSON escape; x-api-key forms, any case; a lower-case Windows path; host
#   names from hostname and scutil, also with spaces; a forbidden name after an escape) and skips
#   a host name shorter than 4 characters with a note; U5: "-" and "_" are part of a host name.
# - T6: tmux targets are exact and sessions this script did not start are left alone (a private
#   tmux server with no config file, through a tmux shim on PATH; only that server is killed, and its socket file is removed).
# - ML16 (node ML-16): live-mail.sh with no argument exits 2; --pm and --m12 in a copied tree, with stubs that answer --version with the pins, a tmux shim that logs each call, MOCK_URL at a dead port and a made-up HOME: one LIVE line per id, the fixtures written in the copy, only stubs started, every tmux call on a private server (-S, -f /dev/null), no process, server, socket file or private folder left; usage (exit 2, no stub called) for no argument, --pm --m12, --foo, -- --pm; a caller-exported PREMISES_DIR is not written; a PREMISES_DIR with a space or a double quote is refused; PM1 1.1 passes on the hook inputs of the stub claude; --m12-judge gives the 12.2/12.3 verdicts of fixed transcripts.
# - FX20 (node FX-20): live-detach.sh --zygo in a copied tree, on the stub claude and a stub zygo whose up fails: exit 0, one LIVE PW15 line; the record head line "launch zygo 9.8.7, real down" and the LIVE PW15 text "launch zygo 9.8.7 real down, tmux <version>" name zygo once; only stubs ran, every tmux call on a private server, nothing left; the repository's fixtures/tmux/pw15-*.txt unchanged.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
SH=$(command -v dash || command -v sh)
T=$(mktemp -d "${TMPDIR:-/tmp}/live-selftest.XXXXXX")
T=$(cd "$T" && pwd -P)
SOCK="n38selftest$$"
TMUX_REAL=$(command -v tmux || true)
LPID="" XPID=""
cleanup() {
  [ -z "$LPID" ] || kill "$LPID" 2>/dev/null
  [ -z "$XPID" ] || kill -s KILL -- "-$XPID" 2>/dev/null
  pkill -KILL -f "sleep 4711$$" 2>/dev/null
  [ -z "$TMUX_REAL" ] || "$TMUX_REAL" -L "$SOCK" kill-server 2>/dev/null
  rm -f "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/$SOCK" 2>/dev/null
  chmod -R u+rwx "$T" 2>/dev/null
  rm -rf "$T"
}
trap 'cleanup' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP
n=0 bad=0
ok() { n=$((n + 1)); printf 'ok: %s\n' "$1"; }
no() { n=$((n + 1)); bad=$((bad + 1)); printf 'FAIL: %s\n' "$1"; }
# expect <name> <0|fail> <rc> [<file> <fixed text that must be in it>]
expect() {
  if { [ "$2" = 0 ] && [ "$3" -eq 0 ]; } || { [ "$2" = fail ] && [ "$3" -ne 0 ]; }; then
    if [ -n "${4:-}" ] && ! grep -qF -- "$5" "$4"; then no "$1 (output lacks: $5)"; else ok "$1"; fi
  else
    no "$1 (exit $3)"; sed 's/^/    /' "${4:-/dev/null}"
  fi
}
A="abcDEF123ghiJKL456mnoPQR789stu"
RND=$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')
P_LOGIN="sk-ant""-o"
P_SESSION="zyg""_s_"

# --- U4: stub agents first on PATH; nothing reaches a provider -----------------------------------
STUB="$T/stubs" MARK="$T/markers"
mkdir -p "$STUB" "$MARK"
for a in claude opencode zygo; do
  printf '#!/bin/sh\necho "$0" >"%s/%s.$$"\nexec sleep 30\n' "$MARK" "$a" >"$STUB/$a"
  chmod +x "$STUB/$a"
done
PATH="$STUB:$PATH" MOCK_URL="http://127.0.0.1:1" ZYGO_BIN="$STUB/zygo"
export PATH MOCK_URL ZYGO_BIN
for a in claude opencode zygo; do
  [ "$(command -v "$a")" = "$STUB/$a" ] || { echo "live selftest: $a is not the stub; stopped before any case"; exit 1; }
done
ok "U4 claude, opencode and zygo resolve to the stubs"
rc=0
N2_SCRATCH="$T/u4" PREMISES_DIR="$HERE" "$SH" -c ". '$HERE/lib.sh'; . '$HERE/live.sh'; echo \"\$CLAUDE_BIN \$OPENCODE_BIN \$ZYGO_BIN\"" >"$T/u4.out" 2>&1 || rc=$?
[ "$rc" -eq 0 ] && [ "$(cat "$T/u4.out")" = "$STUB/claude $STUB/opencode $STUB/zygo" ] &&
  ok "U4 lib.sh and live.sh take the stubs" || { echo "live selftest: lib.sh or live.sh does not take the stubs; stopped"; exit 1; }

# --- T1: the timeout wrapper's process group ------------------------------------------------
t1() { # <signal name> <exit wanted>
  d="$T/t1-$1"; mkdir -p "$d"
  cat >"$d/child.sh" <<EOF
N2_SCRATCH='$d'; PREMISES_DIR='$HERE'
. '$HERE/lib.sh'
. '$HERE/live.sh'
live_traps
live_run t1
LIVE_TIMEOUT=300
live_exec sleep 4711$$
EOF
  # The child runs in its own process group with SIGINT at its default (a background job of a
  # non-interactive shell starts with SIGINT ignored, which a trap could not catch).
  perl -e '$SIG{INT} = "DEFAULT"; setpgrp(0, 0); exec @ARGV or die' "$SH" "$d/child.sh" &
  c=$!
  i=0 pg=""
  while [ $i -lt 100 ]; do
    f=$(ls "$d/live-$c"/pg-* 2>/dev/null | head -n 1)
    [ -n "$f" ] && read -r _ pg <"$f" && [ -n "$pg" ] && break
    i=$((i + 1)); sleep 0.1
  done
  if [ -z "$pg" ]; then
    no "T1 $1: no wrapper group recorded"; kill -s KILL -- "-$c" 2>/dev/null; pkill -KILL -f "sleep 4711$$" 2>/dev/null
    return
  fi
  kill -s "$1" -- "-$c"
  rc=0; wait "$c" || rc=$?
  sleep 0.5
  if kill -s 0 -- "-$pg" 2>/dev/null; then
    no "T1 $1: a process of the wrapped group is alive"; kill -s KILL -- "-$pg" 2>/dev/null; pkill -KILL -f "sleep 4711$$" 2>/dev/null
  elif [ ! -d "$d/live-$c/t1" ]; then no "T1 $1: the caller dir lost live-<pid>/t1"
  elif [ -n "$(ls "$d/live-$c"/pg-* 2>/dev/null)" ]; then no "T1 $1: a pg-* file is left"
  elif [ "$rc" -ne "$2" ]; then no "T1 $1: exit $rc, wanted $2"
  else ok "T1 $1 during a wrapped command: group killed, live-<pid> kept without pg-* files, exit $rc"; fi
}
t1 INT 130
t1 TERM 143
t1 HUP 129
# U5: a stale pg-* file is removed by live_traps; live_end leaves a group whose leader is not the
# child of the recorded wrapper PID (here: a group this test started, recorded with the child's PID).
perl -e 'setpgrp(0, 0); exec "sleep", "47120" or die' &
XPID=$!
d="$T/t5"; mkdir -p "$d"
cat >"$d/child.sh" <<EOF
N2_SCRATCH='$d'; PREMISES_DIR='$HERE'
mkdir -p "\$N2_SCRATCH/live-\$\$"; echo "1 1" >"\$N2_SCRATCH/live-\$\$/pg-stale"
. '$HERE/lib.sh'
. '$HERE/live.sh'
live_traps
[ ! -e "\$N2_SCRATCH/live-\$\$/pg-stale" ] || exit 7
live_run t5
echo "\$\$ $XPID" >"\$N2_SCRATCH/live-\$\$/pg-\$\$"
exit 0
EOF
rc=0; "$SH" "$d/child.sh" >"$T/t5.out" 2>&1 || rc=$?
sleep 0.3
if [ "$rc" -eq 7 ]; then no "U5 live_traps left a stale pg file"
elif [ "$rc" -ne 0 ]; then no "U5 live_end exit $rc, wanted 0"
elif ! kill -s 0 -- "-$XPID" 2>/dev/null; then no "U5 live_end killed a group it did not start"
else ok "U5 stale pg file removed; a group not led by the wrapper's child is left alone"; fi
kill -s KILL -- "-$XPID" 2>/dev/null; wait "$XPID" 2>/dev/null; XPID=""

# --- T3: --scan-only ---------------------------------------------------------------------------
mkdir -p "$T/tree/scripts" "$T/tree/docs" "$T/outside"
cp -R "$HERE" "$T/tree/scripts/premises"
cp "$REPO/docs/premises.md" "$T/tree/docs/"
TH="$T/home"
DATA="$TH/.local/share/zygo" CACHE="$TH/.cache/zygo"
mkhome() {
  rm -rf "$TH"; mkdir -p "$DATA/plugins" "$CACHE/sessions/cur"
  printf 'SQLite format 3 rows\n' >"$DATA/usage.db"; : >"$DATA/usage.db-wal"
  printf 'snapshot\n' >"$DATA/plugins/manifest.json"
  printf 'one log line\n' >"$CACHE/sessions/cur/session.log"
}
# A curl shim logs every URL the scan fetches ($T/curl.log), then runs the real curl.
mkdir -p "$T/curlshim"
printf '#!/bin/sh\nprintf "%%s\\n" "$*" >>"%s"\nexec "%s" "$@"\n' "$T/curl.log" "$(command -v curl)" >"$T/curlshim/curl"
chmod +x "$T/curlshim/curl"
FXF="$T/tree/fixtures/claude-code/live/28-store-scan.txt"
SCAN_ENV=""
scan_only() { # [<session id>] -> rc; output in $T/scan.out; env words in SCAN_ENV
  rm -rf "$T/s3"; mkdir -p "$T/s3"; rm -f "$FXF"
  rc=0
  # shellcheck disable=SC2086
  env PATH="$T/curlshim:$PATH" $SCAN_ENV HOME="$TH" N2_SCRATCH="$T/s3" "$SH" "$T/tree/scripts/premises/live-passthrough.sh" --scan-only "$@" >"$T/scan.out" 2>&1 || rc=$?
  if grep -qF -- "$RND" "$T/scan.out" "$FXF" 2>/dev/null; then
    no "T3: a token value is in the scan output"
  fi
  if [ "$rc" -ne 0 ] && [ -e "$FXF" ]; then no "U3: the store-scan fixture was written by a failed scan"; fi
  if [ "$rc" -eq 0 ] && [ ! -s "$FXF" ]; then no "U3: a passing scan wrote no store-scan fixture"; fi
}
mkhome; scan_only; expect "T3 clean store passes" 0 "$rc" "$T/scan.out" "read errors: 0"
mkhome; rm -rf "$DATA"; scan_only; expect "T3 missing data dir fails" fail "$rc" "$T/scan.out" "(missing)"
mkhome; rm -f "$CACHE/sessions/cur/session.log"; scan_only; expect "T3 cache dir with no file fails" fail "$rc" "$T/scan.out" "(no file)"
mkhome; printf 'x\n' >"$DATA/locked"; chmod 000 "$DATA/locked"; scan_only
expect "T3 unreadable file fails" fail "$rc" "$T/scan.out" "read errors: 1"; chmod 600 "$DATA/locked"
mkhome; mkdir -p "$CACHE/sessions/rx"; printf 'k=%s\n' "$P_LOGIN$RND" >"$CACHE/sessions/rx/held"; chmod 444 "$CACHE/sessions/rx"; scan_only
expect "U3 a dir that lists but cannot be searched fails" fail "$rc" "$T/scan.out" "read errors: 1"; chmod 755 "$CACHE/sessions/rx"
mkhome; printf 'k=%s\n' "$P_LOGIN$RND" >"$T/outside/held"; ln -s "$T/outside/held" "$DATA/plugins/link"; scan_only
expect "U3 a link to a file outside the zygo dirs is refused" fail "$rc" "$T/scan.out" "~/.local/share/zygo/plugins/link"
mkdir -p "$T/outside/dir/sub"; printf 'k=%s\n' "$P_LOGIN$RND" >"$T/outside/dir/sub/zzoutsidename"
mkhome; ln -s "$T/outside/dir" "$DATA/plugins/outdir"; scan_only
expect "U3 a link to a dir outside the zygo dirs fails" fail "$rc" "$T/scan.out" "refused links (target outside the zygo dirs, not read): 1"
if grep -qE 'zzoutsidename|/sub' "$T/scan.out"; then no "U3 a name under the refused link is listed"
elif grep -qF "login prefix hits: 0" "$T/scan.out"; then ok "U3 nothing under the refused link is listed or read"
else no "U3 the refused link was read"; fi
mkhome; printf 'k=%s\n' "$P_LOGIN$RND" >"$CACHE/sessions/cur/held"; ln -s "$CACHE/sessions/cur/held" "$DATA/plugins/inlink"; scan_only
expect "U3 a link inside the zygo dirs is followed" fail "$rc" "$T/scan.out" "~/.local/share/zygo/plugins/inlink"
mkhome; mkdir -p "$CACHE/sessions/old"; printf '%s' "$P_SESSION$RND" >"$CACHE/sessions/old/token"; scan_only
expect "T3 stale token file fails (no session id)" fail "$rc" "$T/scan.out" "sessions/old/token"
# A live session: its url answers (a loopback listener); a stale one: its url does not.
# The listener answers 200 to every GET (also a proxied one) and logs each request path.
python3 -I -c 'import http.server, sys
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        open(sys.argv[2], "a").write(self.path + "\n"); self.send_response(200); self.end_headers()
    def log_message(self, *a): pass
s = http.server.HTTPServer(("127.0.0.1", 0), H)
open(sys.argv[1], "w").write(str(s.server_port)); s.serve_forever()' "$T/port" "$T/listener.log" 2>/dev/null &
LPID=$!
i=0; while [ ! -s "$T/port" ] && [ $i -lt 50 ]; do i=$((i + 1)); sleep 0.1; done
mkhome; mkdir -p "$CACHE/sessions/live1"; printf '%s' "$P_SESSION$RND" >"$CACHE/sessions/live1/token"
printf 'http://127.0.0.1:%s/' "$(cat "$T/port")" >"$CACHE/sessions/live1/url"; scan_only cur
expect "T3 live session's token file not counted (session id given)" 0 "$rc" "$T/scan.out" "sessions/live1/token"
# U2: a url that is not exactly http://<loopback>:<port>[/] counts as not live.
u2() { # <name> <url> [<env words>]
  mkhome; mkdir -p "$CACHE/sessions/u2"; printf '%s' "$P_SESSION$RND" >"$CACHE/sessions/u2/token"
  printf '%s' "$2" >"$CACHE/sessions/u2/url"
  : >"$T/curl.log"; : >"$T/listener.log"
  SCAN_ENV=${3:-}; scan_only cur; SCAN_ENV=""
  expect "U2 url $1: token file counted" fail "$rc" "$T/scan.out" "sessions/u2/token"
}
u2 "file:" "file://$CACHE/sessions/u2/token"
u2 "--version" "--version"
P=$(cat "$T/port")
u2 "dead port, live http_proxy" "http://127.0.0.1:1/" "http_proxy=http://127.0.0.1:$P HTTP_PROXY=http://127.0.0.1:$P ALL_PROXY=http://127.0.0.1:$P"
if [ -s "$T/listener.log" ]; then no "U2 the probe went through http_proxy"; else ok "U2 the probe does not use http_proxy"; fi
u2 "not loopback" "http://192.0.2.10:$P/"
if grep -qF 192.0.2.10 "$T/curl.log"; then no "U2 a non-loopback url was fetched"; else ok "U2 a non-loopback url is never fetched"; fi
mkhome; mkdir -p "$CACHE/sessions/dead1"; printf '%s' "$P_SESSION$RND" >"$CACHE/sessions/dead1/token"
printf 'http://127.0.0.1:1' >"$CACHE/sessions/dead1/url"; scan_only cur
expect "T3 stale session's token file fails (session id given)" fail "$rc" "$T/scan.out" "sessions/dead1/token"
mkhome; printf '%s' "$P_SESSION$RND" >"$CACHE/sessions/cur/token"
printf 'http://127.0.0.1:%s' "$(cat "$T/port")" >"$CACHE/sessions/cur/url"; scan_only cur
expect "T3 the run's own token file fails" fail "$rc" "$T/scan.out" "sessions/cur/token"
kill "$LPID" 2>/dev/null; LPID=""

# --- T4, T5, T8: check-premises.sh scans ---------------------------------------------------------
C="$T/ctree"
mkdir -p "$C/scripts" "$C/docs/specs" "$T/shim"
cp "$REPO/scripts/check-premises.sh" "$C/scripts/"
cp -R "$REPO/fixtures" "$C/"
cp -R "$HERE" "$C/scripts/premises"
cp "$REPO/docs/premises.md" "$REPO/docs/live-checks.md" "$REPO/docs/conformance.md" "$C/docs/"
cp "$REPO"/docs/specs/2026-11-18-zygo-design.md "$C/docs/specs/"
PLANT="$C/fixtures/zz-selftest.txt"
cp_run() { # <line> [<env words>] -> rc, output in $T/cp.out
  printf '%s\n' "$1" >"$PLANT"; shift
  rc=0
  env "$@" sh "$C/scripts/check-premises.sh" >"$T/cp.out" 2>&1 || rc=$?
  rm -f "$PLANT"
}
cp_run "nothing here"; expect "T4 copied tree passes" 0 "$rc" "$T/cp.out" "check-premises: ok"
K="sk""-${A%?????}"   # a legacy-form key, 25 characters after the prefix
XN="x-api""-key"
for c in \
  "API key|{\"m\":\"a\\n$K\"}" \
  "API key|a\\t$K" \
  "API key|a\\u0022$K" \
  "organization id|a\\norg""-${A%??????????????????}" \
  "project id|a\\tproj""_${A%??????????????????}" \
  "GitHub token|a\\ngh""p_$A" \
  "x-api-key value|{\\\"$XN\\\":\\\"$A\\\"}" \
  "x-api-key value|{'$XN': '$A'}" \
  "x-api-key value|X-API""-KEY: $A" \
  "Windows home path|c:""\\users\\name\\x"; do
  cp_run "${c#*|}"; expect "T4/T5/T8 scan: ${c%%|*} ($(printf '%s' "${c#*|}" | sed "s/$A/<A>/; s/${A%?????}/<A>/" | cut -c1-28)...)" fail "$rc" "$T/cp.out" "(${c%%|*})"
done
cp_run "$(printf '%s: [omitted]' "X-Api""-Key")"; expect "T5 scan: an omitted x-api-key value passes" 0 "$rc" "$T/cp.out" "check-premises: ok"
cp_run "a\\nzzforbidq b" N2_FORBIDDEN_NAMES=zzforbidq
expect "T4 scan: a forbidden name after an escape" fail "$rc" "$T/cp.out" "forbidden name from N2_FORBIDDEN_NAMES"
# Host names from shims: hostname and scutil (LocalHostName, ComputerName with spaces).
shims() { # <hostname output> <LocalHostName> <ComputerName>
  printf '#!/bin/sh\necho "%s"\n' "$1" >"$T/shim/hostname"
  printf '#!/bin/sh\ncase "$2" in LocalHostName) echo "%s" ;; ComputerName) echo "%s" ;; esac\n' "$2" "$3" >"$T/shim/scutil"
  chmod +x "$T/shim/hostname" "$T/shim/scutil"
}
shims zzhostq.example.net zzlocalq "Zz Computer Q"
for c in "on ZZHOSTQ." "zzhostq.example.net" "a zzlocalq b" "a\\nZz Computer Q b"; do
  cp_run "$c" PATH="$T/shim:$PATH"
  expect "T8 scan: host name ($c)" fail "$rc" "$T/cp.out" "check-premises: host name:"
  if grep -qiE 'zzhostq|zzlocalq|computer q' "$T/cp.out"; then no "T8 scan: the message names the host"; fi
done
# U5: "-" and "_" are part of a host name (the same edges as scrub.py).
cp_run "zzhostq_x and x-zzhostq and zzlocalq-1" PATH="$T/shim:$PATH"
expect "U5 scan: a host name inside a longer name with - or _ passes" 0 "$rc" "$T/cp.out" "check-premises: ok"
shims abc "" ""
cp_run "abc here" PATH="$T/shim:$PATH"
expect "T8 scan: a host name under 4 characters is skipped with a note" 0 "$rc" "$T/cp.out" "shorter than 4 characters or localhost"

# --- T6: exact tmux targets and session ownership (private server) ------------------------------
if [ -z "$TMUX_REAL" ]; then
  no "T6: tmux not found"
else
  mkdir -p "$T/tbin"
  printf '#!/bin/sh\nexec "%s" -L "%s" -f /dev/null "$@"\n' "$TMUX_REAL" "$SOCK" >"$T/tbin/tmux"; chmod +x "$T/tbin/tmux"
  TM="$T/tbin/tmux"
  "$TM" new-session -d -s n38own "sleep 120"
  "$TM" new-session -d -s n24x "sh -c 'echo marker-n24x; sleep 120'"
  mkdir -p "$T/t6"
  t6() { # <shell code after sourcing lib.sh and live.sh> -> rc, output in $T/t6.out
    rc=0
    PATH="$T/tbin:$PATH" N2_SCRATCH="$T/t6" PREMISES_DIR="$HERE" "$SH" -c ". '$HERE/lib.sh'; . '$HERE/live.sh'; new_run t6; $1" >"$T/t6.out" 2>&1 || rc=$?
  }
  t6 'tmux_claude n38own'; expect "T6 tmux_claude leaves a session it did not start" fail "$rc" "$T/t6.out" "exists; not started"
  t6 'tmux_zygo n38own claude'; expect "T6 tmux_zygo leaves a session it did not start" fail "$rc" "$T/t6.out" "exists; not started"
  if "$TM" has-session -t =n38own 2>/dev/null; then ok "T6 that session is still alive"; else no "T6 that session was killed"; fi
  t6 'tmux_send n24 hi'; expect "T6 tmux_send n24 does not reach n24x" fail "$rc"
  if "$TM" capture-pane -p -t =n24x: | grep -q hi; then no "T6 text reached n24x"; fi
  t6 'tmux_wait_text n24 marker-n24x 1'; expect "T6 tmux_wait_text n24 does not read n24x" fail "$rc"
  t6 'tmux_wait_text n24x marker-n24x 5'; expect "T6 tmux_wait_text n24x reads n24x" 0 "$rc"
  t6 'tmux_own n38mine; tmux new-session -d -s n38mine "sleep 120"; tmux_own n38mine; tmux has-session -t =n38mine && exit 9; echo restarted-ok'
  expect "T6 a session this script started is killed and started again" 0 "$rc" "$T/t6.out" "restarted-ok"
  # U4: the incident path (tmux_claude on a fresh session) reaches the stub, on the private server.
  t6 'tmux_claude n38stub'
  i=0; while ! ls "$MARK"/claude.* >/dev/null 2>&1 && [ $i -lt 50 ]; do i=$((i + 1)); sleep 0.1; done
  if ls "$MARK"/claude.* >/dev/null 2>&1; then ok "U4 tmux_claude starts the stub claude"; else no "U4 tmux_claude did not reach the stub"; fi
  "$TM" kill-session -t =n38stub 2>/dev/null
fi

# --- ML16: live-mail.sh on stubs -----------------------------------------------------------------
# A copied tree, stubs that answer --version with the pins (so pinned_agents passes), a tmux shim that
# logs each call, MOCK_URL at a dead port, HOME made up, CLAUDE_CONFIG_DIR, TMUX and ZYGO_SESSION_DIR unset.
MSTUB="$T/stubs-mail" MT="$T/mtree" MH="$T/mhome" TLOG="$T/mail-tmux.log"
mkdir -p "$MSTUB" "$MT/scripts" "$MT/docs" "$MH" "$T/mtbin"
cp -R "$HERE" "$MT/scripts/premises"
cp "$REPO/docs/premises.md" "$MT/docs/"
PCC=$(sed -n 's/^Claude Code: *//p' "$REPO/docs/premises.md" | head -n 1)
POC=$(sed -n 's/^OpenCode: *//p' "$REPO/docs/premises.md" | head -n 1)
# Each stub writes its marker on every call, --version included (so a usage exit can show that no stub ran).
# The claude stub, started with --mcp-config and --settings (PM1 only), runs the Stop hook command of the
# settings file with a SessionStart input and two Stop inputs (stop_hook_active false), as Claude Code would.
cat >"$MSTUB/claude" <<EOF
#!/bin/sh
echo "\$0" >"$MARK/claude.\$\$"
case "\${1:-}" in --version) echo "$PCC (Claude Code)"; exit 0 ;; esac
st="" mc=0 pv=""
for a; do [ "\$pv" = --settings ] && st=\$a; [ "\$a" = --mcp-config ] && mc=1; pv=\$a; done
if [ "\$mc" = 1 ] && [ -n "\$st" ]; then
  cmd=\$(python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1]))["hooks"]["Stop"][0]["hooks"][0]["command"])' "\$st")
  echo '{"hook_event_name": "SessionStart", "source": "startup"}' | sh -c "\$cmd" >/dev/null
  echo '{"hook_event_name": "Stop", "stop_hook_active": false}' | sh -c "\$cmd" >/dev/null
  echo '{"hook_event_name": "Stop", "stop_hook_active": true}' | sh -c "\$cmd" >/dev/null
fi
exec sleep 30
EOF
printf '#!/bin/sh\necho "$0" >"%s/opencode.$$"\ncase "${1:-}" in --version) echo "%s"; exit 0 ;; esac\nexec sleep 30\n' "$MARK" "$POC" >"$MSTUB/opencode"
printf '#!/bin/sh\necho "$0" >"%s/zygo.$$"\ncase "${1:-}" in --version) echo "zygo 9.8.7"; exit 0 ;; esac\nexit 1\n' "$MARK" >"$MSTUB/zygo"
: >"$TLOG"
printf '#!/bin/sh\nprintf "%%s\\n" "$*" >>"%s"\nexec "%s" "$@"\n' "$TLOG" "$TMUX_REAL" >"$T/mtbin/tmux"
chmod +x "$MSTUB/claude" "$MSTUB/opencode" "$MSTUB/zygo" "$T/mtbin/tmux"
MPATH="$MSTUB:$T/mtbin:$PATH"
mguard=0
[ -n "$TMUX_REAL" ] || mguard=1
for a in claude opencode zygo; do [ "$(PATH="$MPATH"; command -v "$a")" = "$MSTUB/$a" ] || mguard=1; done
[ "$(PATH="$MPATH"; command -v tmux)" = "$T/mtbin/tmux" ] || mguard=1
# mrun <tag> [<args>]: live-mail.sh on the stubs -> rc; output in $T/m-<tag>.out; marker list before in $T/m-<tag>.before.
# PREMISES_DIR is the copied tree (MPD when set), never a value the caller exported (lib.sh keeps a set one).
mrun() {
  _t=$1; shift
  ls "$MARK" >"$T/m-$_t.before"
  : >"$TLOG"
  mkdir -p "$T/mscr-$_t"
  rc=0
  env -u CLAUDE_CONFIG_DIR -u TMUX -u ZYGO_SESSION_DIR PATH="$MPATH" HOME="$MH" MOCK_URL="http://127.0.0.1:1" ZYGO_BIN="$MSTUB/zygo" \
    PREMISES_DIR="${MPD:-$MT/scripts/premises}" N2_SCRATCH="$T/mscr-$_t" PW_TEST=1 PW_TICKS=1 "$SH" "$MT/scripts/premises/live-mail.sh" "$@" >"$T/m-$_t.out" 2>&1 || rc=$?
}
# mnew <tag> <name prefix>: the markers that came after mrun <tag> started
mnew() { ls "$MARK" | grep -vxF -f "$T/m-$1.before" | grep "^$2\\." ; }
# mstubs <tag> <agent>: at least one new marker of <agent>, each naming a program in $MSTUB
mstubs() {
  _f=$(mnew "$1" "$2"); [ -n "$_f" ] || return 1
  for _m in $_f; do case "$(cat "$MARK/$_m")" in "$MSTUB"/*) ;; *) return 1 ;; esac; done
}
# mtmux: every logged tmux call is -V or starts with "-S <path> -f /dev/null "; no path is in the default socket folder
mtmux() {
  [ -s "$TLOG" ] || return 1
  _pre="${TMUX_TMPDIR:-/tmp}/tmux-"
  while IFS= read -r _l; do
    case "$_l" in
      -V) ;;
      "-S "*)
        _r=${_l#-S }; _p=${_r%% *}; _r=${_r#"$_p" }
        case "$_r" in "-f /dev/null "*) ;; *) return 1 ;; esac
        case "$_p" in "$_pre"*) return 1 ;; esac ;;
      *) return 1 ;;
    esac
  done <"$TLOG"
}
# mleft <tag>: one line for each thing left: a process of the run's scratch folder, a live stub, a tmux
# server that answers (killed here), a socket file of a logged -S path or its private folder. Empty: nothing left.
mleft() {
  pgrep -f "$T/mscr-$1" >/dev/null 2>&1 && echo "a process of the scratch folder"
  for _m in $(mnew "$1" '[a-z]*'); do kill -0 "${_m##*.}" 2>/dev/null && echo "stub $_m"; done
  for _p in $(sed -n 's/^-S \([^ ]*\) .*/\1/p' "$TLOG" | sort -u); do
    if "$TMUX_REAL" -S "$_p" -f /dev/null has-session 2>/dev/null; then echo "tmux server $_p"; "$TMUX_REAL" -S "$_p" -f /dev/null kill-server 2>/dev/null; fi
    [ ! -e "$_p" ] || echo "socket file $_p"
    [ ! -e "${_p%/*}" ] || echo "private folder ${_p%/*}"
  done
}
# mcase <name> <0 = pass> [<output file>]: one ok or FAIL line; a FAIL shows the output
mcase() { if [ "$2" -eq 0 ]; then ok "$1"; else no "$1"; [ -z "${3:-}" ] || sed 's/^/    /' "$3"; fi; }
if [ "$mguard" -ne 0 ]; then
  no "ML16: stubs or tmux shim do not resolve; live-mail.sh not run"
else
  # A caller-exported PREMISES_DIR names another copy of the tree; no run may write in it.
  LK="$T/leak"; mkdir -p "$LK/scripts" "$LK/docs"; cp -R "$HERE" "$LK/scripts/premises"; cp "$REPO/docs/premises.md" "$LK/docs/"
  find "$LK" | sort >"$T/leak.before"
  PREMISES_DIR="$LK/scripts/premises"; export PREMISES_DIR
  # musage <tag> <name> [<args>]: usage, exit 2, no stub called (not even --version), no tmux call, scratch folder empty
  musage() {
    _u=$1 _nm=$2; shift 2
    mrun "$_u" "$@"
    t=1; [ "$rc" -eq 2 ] && grep -qF 'usage: live-mail.sh --pm | --m12' "$T/m-$_u.out" && [ ! -s "$TLOG" ] &&
      [ -z "$(mnew "$_u" '[a-z]*')" ] && [ -z "$(ls -A "$T/mscr-$_u")" ] && t=0
    mcase "ML16 $_nm: usage, exit 2, nothing started (exit $rc)" "$t" "$T/m-$_u.out"
  }
  musage usage "no argument"
  musage u-both "--pm --m12" --pm --m12
  musage u-foo "--foo" --foo
  musage u-dd "-- --pm" -- --pm
  # PREMISES_DIR with a space or a double quote: refused with a live: line before any tmux server starts
  for q in 'sp ace' 'q"t'; do
    QD="$T/$q"; mkdir -p "$QD/scripts" "$QD/docs"; cp -R "$HERE" "$QD/scripts/premises"; cp "$REPO/docs/premises.md" "$QD/docs/"
    MPD="$QD/scripts/premises"; mrun pq --m12; unset MPD
    t=1; [ "$rc" -eq 1 ] && grep -q '^live: PREMISES_DIR ' "$T/m-pq.out" && ! grep -q '^-S ' "$TLOG" && [ ! -e "$QD/fixtures" ] && t=0
    mcase "ML16 PREMISES_DIR holding $(printf '%s' "$q" | sed 's/[a-z]//g; s/^ $/a space/; s/^"$/a double quote/'): refused, exit 1 (exit $rc)" "$t" "$T/m-pq.out"
  done
  mrun pm --pm
  o="$T/m-pm.out"
  t=1; [ "$rc" -eq 0 ] && [ "$(grep -c '^LIVE PM[123] failed ' "$o")" -eq 3 ] &&
    grep -q '^LIVE PM1 failed ' "$o" && grep -q '^LIVE PM2 failed ' "$o" && grep -q '^LIVE PM3 failed ' "$o" && t=0
  mcase "ML16 --pm: exit $rc, one LIVE line for each of PM1, PM2, PM3 (failed on stubs)" "$t" "$o"
  t=1; mstubs pm claude && mstubs pm opencode && t=0
  mcase "ML16 --pm: the agents started were the stub claude and opencode" "$t"
  t=1; mtmux && t=0
  mcase "ML16 --pm: every tmux call is on a private server (-S, -f /dev/null)" "$t" "$TLOG"
  l=$(mleft pm); t=1; [ -z "$l" ] && t=0
  mcase "ML16 --pm: no process, tmux server or socket file left${l:+ ($(printf '%s' "$l" | tr '\n' ';'))}" "$t"
  t=0
  for f in claude-code/mail/pm1.txt claude-code/mail/pm2.txt claude-code/mail/pm3.txt opencode/mail/pm2.txt opencode/mail/pm3.txt; do
    [ -s "$MT/fixtures/$f" ] || t=1
  done
  mcase "ML16 --pm: the five fixtures are written in the copied tree" "$t"
  t=1; grep -q '^1[.]1 ok ' "$MT/fixtures/claude-code/mail/pm1.txt" 2>/dev/null && t=0
  mcase "ML16 --pm: PM1 1.1 ok from the hook inputs of the stub claude (the check can pass)" "$t" "$MT/fixtures/claude-code/mail/pm1.txt"
  mrun m12 --m12
  o="$T/m-m12.out"
  t=1; [ "$rc" -eq 0 ] && [ "$(grep -c '^LIVE ' "$o")" -eq 1 ] && grep -q '^LIVE M12 failed ' "$o" && t=0
  mcase "ML16 --m12: exit $rc, one LIVE M12 line (failed on stubs)" "$t" "$o"
  t=1; mstubs m12 zygo && t=0
  mcase "ML16 --m12: zygo ran as the stub" "$t"
  t=1; mtmux && t=0
  mcase "ML16 --m12: every tmux call is on a private server (-S, -f /dev/null)" "$t" "$TLOG"
  l=$(mleft m12); t=1; [ -z "$l" ] && t=0
  mcase "ML16 --m12: no process, tmux server or socket file left${l:+ ($(printf '%s' "$l" | tr '\n' ';'))}" "$t"
  t=1; [ -s "$MT/fixtures/claude-code/mail/m12.txt" ] && t=0
  mcase "ML16 --m12: the fixture is written in the copied tree" "$t"
  h=$(sed -n 1p "$MT/fixtures/claude-code/mail/m12.txt" 2>/dev/null)
  t=1; printf '%s\n' "$h" | grep -qF ', zygo 9.8.7, ' && [ "$(printf '%s\n' "$h" | grep -o zygo | wc -l | tr -d ' ')" -eq 1 ] && t=0
  mcase "FX18 --m12: the M12 head line holds \", zygo <version>,\" with the word zygo once" "$t" "$MT/fixtures/claude-code/mail/m12.txt"
  task=$(sed -n "s/^M12_TASK='\\(.*\\)'\$/\\1/p" "$HERE/live-mail.sh" 2>/dev/null)
  t=1; [ -n "$task" ] && ! printf '%s\n' "$task" | grep -qiE 'mail|message|send|inbox|poll|watch' && t=0
  mcase "ML16 the M12 task names no mail" "$t"
  find "$LK" | sort >"$T/leak.after"
  t=1; cmp -s "$T/leak.before" "$T/leak.after" && t=0
  mcase "ML16 an exported PREMISES_DIR: no file written in that tree" "$t"
  unset PREMISES_DIR
  # The 12.2/12.3 verdicts of live-mail.sh --m12-judge on fixed transcripts (no tmux, no agent).
  JT="$T/judge"; mkdir -p "$JT"
  tu() { printf '{"type": "user", "message": {"content": %s}}\n' "$1"; }
  ta() { printf '{"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "mcp__zygo__%s", "input": %s}]}}\n' "$1" "$2"; }
  tr_() { printf '{"type": "user", "message": {"content": [{"type": "tool_result", "content": "x"}]}}\n'; }
  { tu "\"$task\""; ta mail_list '{}'; tr_; ta mail_send '{"to": ["beta"], "subject": "s", "body": "b"}'; tr_; tu '"next"'; ta mail_read '{}'; } >"$JT/before.jsonl"
  { tu "\"$task\""; ta mail_send '{"to": "beta", "subject": "s", "body": "b"}'; tr_; ta mail_read '{}'; tr_; } >"$JT/after.jsonl"
  # mjudge <tag> <transcript>: the record lines in $JT/<tag>/M12.txt; rc
  mjudge() {
    mkdir -p "$JT/$1"; rc=0
    env -u PREMISES_DIR PATH="$MPATH" "$SH" "$MT/scripts/premises/live-mail.sh" --m12-judge "$2" "$JT/$1" >"$JT/$1.out" 2>&1 || rc=$?
    cat "$JT/$1.out" >>"$JT/$1/M12.txt" 2>/dev/null
  }
  mjudge before "$JT/before.jsonl"; J="$JT/before/M12.txt"
  t=1; [ "$rc" -eq 0 ] && grep -q '^12[.]2 ok .*beta among them: yes' "$J" && grep -q '^12[.]3 ok .*after the last mail_send of the turn: 0$' "$J" && t=0
  mcase "ML16 judge: mail_list before mail_send to beta: 12.2 ok, 12.3 ok with 0 polls" "$t" "$J"
  mjudge after "$JT/after.jsonl"; J="$JT/after/M12.txt"
  t=1; [ "$rc" -eq 0 ] && grep -q '^12[.]2 ok ' "$J" && grep -q '^12[.]3 FAIL .*after the last mail_send of the turn: 1$' "$J" && t=0
  mcase "ML16 judge: mail_read after mail_send: 12.3 FAIL with 1 poll" "$t" "$J"
  mjudge none "$JT/missing.jsonl"; J="$JT/none/M12.txt"
  t=1; [ "$rc" -eq 0 ] && grep -q '^12[.]2 FAIL .*found in its transcript: false' "$J" && grep -q '^12[.]3 FAIL ' "$J" && t=0
  mcase "ML16 judge: no transcript: 12.2 and 12.3 FAIL" "$t" "$J"
fi

# --- FX20: live-detach.sh --zygo on stubs: the PW15 record line names zygo once ------------------
# A copied tree; a zygo stub whose `up` exits 1 (no agent pane starts) and whose other calls exit 0;
# a setsid shim for the PATH check only (the --zygo path never calls setsid).
DT="$T/dtree"
mkdir -p "$DT/scripts" "$DT/docs" "$T/dtbin" "$T/mscr-pw15"
cp -R "$HERE" "$DT/scripts/premises"; cp "$REPO/docs/premises.md" "$DT/docs/"
printf '#!/bin/sh\necho "$0" >"%s/zygo.$$"\ncase "${1:-}" in --version) echo "zygo 9.8.7"; exit 0 ;; up) exit 1 ;; esac\nexit 0\n' "$MARK" >"$MSTUB/zygo-pw15"
printf '#!/bin/sh\nexec "$@"\n' >"$T/dtbin/setsid"
chmod +x "$MSTUB/zygo-pw15" "$T/dtbin/setsid"
if [ "$mguard" -ne 0 ] || ! command -v jq >/dev/null 2>&1 || ! command -v perl >/dev/null 2>&1; then
  no "FX20: stubs, tmux shim, jq or perl not found; live-detach.sh not run"
else
  cksum "$REPO"/fixtures/tmux/pw15-*.txt >"$T/pw15.before"
  ls "$MARK" >"$T/m-pw15.before"; : >"$TLOG"
  rc=0
  env -u CLAUDE_CONFIG_DIR -u TMUX -u ZYGO_SESSION_DIR PATH="$T/dtbin:$MPATH" HOME="$MH" PREMISES_DIR="$DT/scripts/premises" \
    N2_SCRATCH="$T/mscr-pw15" "$SH" "$DT/scripts/premises/live-detach.sh" --zygo "$MSTUB/zygo-pw15" >"$T/m-pw15.out" 2>&1 || rc=$?
  o="$T/m-pw15.out"
  t=1; [ "$rc" -eq 0 ] && [ "$(grep -c '^LIVE PW15 ' "$o")" -eq 1 ] && grep -q '^LIVE PW15 failed ' "$o" && t=0
  mcase "FX20 --zygo: exit $rc, one LIVE PW15 line (failed on stubs)" "$t" "$o"
  h=$(sed -n 1p "$DT/fixtures/tmux/pw15-down.txt" 2>/dev/null)
  t=1; [ "$h" = "launch zygo 9.8.7, real down" ] && t=0
  mcase "FX20 --zygo: the PW15 record head line is \"launch zygo 9.8.7, real down\" (zygo once)" "$t" "$DT/fixtures/tmux/pw15-down.txt"
  l=$(sed -n 's/^LIVE PW15 [a-z]* ([^)]*): \([^;]*\);.*/\1/p' "$o")
  t=1; case "$l" in "launch zygo 9.8.7 real down, tmux "*) t=0 ;; esac
  mcase "FX20 --zygo: the LIVE PW15 text starts \"launch zygo 9.8.7 real down, tmux \" (zygo once; got: $l)" "$t"
  t=1; mstubs pw15 zygo && mstubs pw15 claude && mtmux && [ -z "$(mleft pw15)" ] && t=0
  mcase "FX20 --zygo: only stubs ran, every tmux call on a private server, nothing left" "$t" "$TLOG"
  cksum "$REPO"/fixtures/tmux/pw15-*.txt >"$T/pw15.after"
  t=1; cmp -s "$T/pw15.before" "$T/pw15.after" && t=0
  mcase "FX20 the repository's fixtures/tmux/pw15-*.txt are unchanged" "$t"
fi

# --- U4: every agent program that ran was a stub; no wrapped sleep is left ------------------------
nm=0 real=0
for m in "$MARK"/*; do
  [ -f "$m" ] || continue
  nm=$((nm + 1))
  case "$(cat "$m")" in "$STUB"/* | "$MSTUB"/*) ;; *) real=$((real + 1)) ;; esac
done
if [ "$real" -eq 0 ]; then ok "U4 every agent started was a stub ($nm marker(s))"; else no "U4 $real marker(s) not from a stub"; fi
if pgrep -f "sleep 4711$$" >/dev/null 2>&1; then no "U4 a wrapped sleep is left"; pkill -KILL -f "sleep 4711$$"; else ok "U4 no wrapped sleep is left"; fi

if [ "$bad" -eq 0 ]; then echo "live selftest: ok ($n)"; exit 0; fi
echo "live selftest: $bad of $n failed"; exit 1

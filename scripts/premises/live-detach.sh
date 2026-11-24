#!/bin/sh
# Live premise PW15 (node P1, workspaces spec W§17, AU65): a process that the agent's Bash tool
# starts with `setsid` and redirected output keeps running after the tool call, after the agent's
# exit and after the pane's end, without and with the agent's sandbox.
# Usage: sh scripts/premises/live-detach.sh [--zygo <path>]
#   No argument: the stand-in run of node P1 (fixtures/tmux/pw15.txt).
#   --zygo <path>: the real check of node E2 (W§17 PW15 re-check): the real zygo down started from the Bash tool, the workspace_down tool and /zygo:down, each with the sandbox off and on; fixture fixtures/tmux/pw15-down.txt.
#   P15_KEEP=<dir> (environment, optional): copy each session's raw output files to <dir>/<off|on>
#   for a diagnosis. These files are not scrubbed: never commit them.
# Put the tmux to check first on PATH (the minimum is 3.2, AU7). Run by hand only.
#
# What this run does to the operator's machine:
# - It uses the real HOME and no CLAUDE_CONFIG_DIR, so Claude Code uses the operator's stored login.
#   `--setting-sources project` leaves the operator's user settings, hooks and plugins out;
#   `--strict-mcp-config` leaves the operator's MCP servers out; `--no-session-persistence` writes
#   no transcript. Claude Code can still update its own state file under the real HOME.
#   DISABLE_AUTOUPDATER=1 stops a self-update. Variables of an enclosing agent session are dropped
#   (env -i).
# - It starts two agent sessions in print mode (one without the sandbox, one with it). Each one is
#   one turn with one tool call. The run spends the operator's claude.ai usage on these two turns.
# - The agent can run exactly one Bash command with no prompt: `sh ./start-down.sh`, by one allow
#   rule. `--tools Bash` leaves no other built-in tool, and `--permission-prompts none` denies every
#   other call. No permission bypass is used.
# - The working directory, every file the agent's command writes, and the tmux socket are in
#   scratch. tmux runs as a server of its own (`-S <socket in a private folder>`, `-f /dev/null`);
#   the default tmux server is never called.
# - The command starts one sleeper per session with `setsid`. The script stops each sleeper by a
#   `stop` file; a sleeper that no `stop` file reaches ends by itself after 120 s.
# - With --zygo: six agent sessions in print mode, each a member of a new workspace (one shell pane) on the private server; zygo writes its state, session folders and usage rows under the real HOME, and each workspace is forgotten at the end of its run. Bash and tool runs are one turn with one tool call; a /zygo:down run makes no model request (the hook blocks the prompt).
ZYGO=""
case "$#" in
  0) ;;
  2)
    [ "$1" = --zygo ] || { echo "usage: live-detach.sh [--zygo <path>]"; exit 2; }
    ZYGO=$(cd "$(dirname "$2")" 2>/dev/null && pwd)/$(basename "$2")
    { [ -f "$ZYGO" ] && [ -x "$ZYGO" ]; } || { echo "live-detach: --zygo $2 is not an executable file"; exit 2; } ;;
  *) echo "usage: live-detach.sh [--zygo <path>]"; exit 2 ;;
esac
zp_stop_extra() {
  [ -n "${R:-}" ] || return 0
  for d in "$R/off" "$R/on"; do [ -d "$d" ] && : >"$d/stop"; done 2>/dev/null || true
}
ZP_PRIV=$(mktemp -d "${TMPDIR:-/tmp}/zp1.XXXXXX")
trap 'rm -rf "$ZP_PRIV"' EXIT
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps
SOCK="$ZP_PRIV/s"
TMUX_BIN=""
tm() { LC_ALL=C "$TMUX_BIN" -S "$SOCK" -f /dev/null "$@"; }
# zp_end <status>: stop the sleepers (live-detach.sh), kill the private server, remove the private directory, then live_end.
zp_end() { trap - EXIT INT TERM HUP PIPE; zp_stop_extra; [ -z "$TMUX_BIN" ] || tm kill-server 2>/dev/null || true; rm -rf "$ZP_PRIV"; live_end "$1"; }
trap 'zp_end $?' EXIT; trap 'zp_end 129' HUP; trap 'zp_end 130' INT; trap 'zp_end 141' PIPE; trap 'zp_end 143' TERM
TMUX_BIN=$(command -v tmux) || { echo "live: tmux is not on PATH"; exit 1; }
TV=$("$TMUX_BIN" -V | sed -n 's/^tmux //p')
ver_ge "${TV%[a-z]}" 3.2 || { echo "live: tmux is ${TV:-unknown}; the minimum is 3.2 (AU7); put tmux 3.2 or later first on PATH"; exit 1; }
[ ${#SOCK} -le 100 ] || { echo "live: the socket path is longer than 100 characters; set TMPDIR to a short directory"; exit 1; }
echo "live: tmux $TV"
# wait_for <seconds> <command...>: poll every 0.1 s; 0 when the command succeeds in time, else 1.
wait_for() { _w=$(($1 * 10)); shift; while ! "$@" >/dev/null 2>&1; do _w=$((_w - 1)); [ "$_w" -gt 0 ] || return 1; sleep 0.1; done; }
# srv_stop: kill the private server, wait at most 5 s until it does not answer, remove the socket file.
srv_stop() { tm kill-server 2>/dev/null || true; _s=50; while tm list-sessions >/dev/null 2>&1 && [ "$_s" -gt 0 ]; do _s=$((_s - 1)); sleep 0.1; done; rm -f "$SOCK"; }
FX="$REPO_DIR/fixtures/tmux"; mkdir -p "$FX"

for n in setsid jq perl; do
  command -v "$n" >/dev/null 2>&1 || { echo "live: $n is not on PATH"; exit 1; }
done
v=$("$CLAUDE_BIN" --version 2>/dev/null || true)
LIVE_CC_VER=$(agent_ver "$v")
agent_ok "$LIVE_CC_VER" "$PIN_CC" || { echo "live: claude is $(agent_show "$v"); the minimum is $PIN_CC (docs/premises.md)"; exit 1; }
LIVE_VER=$LIVE_CC_VER

live_run p15
LAUNCH=plain
FXFILE=pw15.txt
if [ -n "$ZYGO" ]; then
  ZV=$(agent_show "$("$ZYGO" --version 2>/dev/null || true)")
fi
case "$HOME$PATH$CLAUDE_BIN$RUN$ZYGO" in
  *"'"*) echo "live: a path holds a single quote; this script cannot use it"; exit 1 ;;
esac
MNT_SELF=$(readlink /proc/self/ns/mnt 2>/dev/null || echo none)
U=$(id -un)
LAUNCHWORDS="'$CLAUDE_BIN'"
EXTRA=""

# variant <off|on>: one agent session; writes one line to $RUN/pw15.txt and sets VR (ok or the reason).
variant() {
  v=$1
  C="$R/$v"
  mkdir "$C" "$C/.claude"
  rc="?"; turns="?"; den="?"; ran=no; started=no; a1=no; a2=no; insb=no; ow="?"; VR=""
  if [ "$v" = on ] && [ "$MNT_SELF" != none ]; then
    if ! command -v bwrap >/dev/null 2>&1 || ! command -v socat >/dev/null 2>&1; then
      VR="sandbox not available (bwrap or socat is not on PATH)"
      vline; return 0
    fi
  fi
  # The stand-in for the detached `zygo down --wait`: one line per second, at most 120, until ./stop exists.
  cat >"$C/sleeper.sh" <<'EOF'
#!/bin/sh
echo started >marker.head
i=0
while [ "$i" -lt 120 ] && [ ! -e stop ]; do date +%s >>marker; i=$((i + 1)); sleep 1; done
: >stopped
EOF
  # The stand-in for `zygo down` (W§9.3 step 2). It records the sandbox state itself (start.head),
  # so the record has it also when the sleeper ends at once.
  cat >"$C/start-down.sh" <<EOF
#!/bin/sh
{
  echo "mnt=\$(readlink /proc/self/ns/mnt 2>/dev/null || echo none)"
  if ( : >'$RUN/outside-$v' ) 2>/dev/null; then echo "outside_write=yes"; else echo "outside_write=no"; fi
} >start.head
setsid /bin/sh ./sleeper.sh </dev/null >down.log 2>&1 &
echo "down started; end your turn"
EOF
  if [ "$v" = off ]; then
    echo '{"sandbox":{"enabled":false},"permissions":{"allow":["Bash(sh ./start-down.sh)"]}}' >"$C/.claude/settings.json"
  else
    echo '{"sandbox":{"enabled":true,"allowUnsandboxedCommands":false,"autoAllowBashIfSandboxed":false},"permissions":{"allow":["Bash(sh ./start-down.sh)"]}}' >"$C/.claude/settings.json"
  fi
  cat >"$RUN/agent-$v.sh" <<EOF
#!/bin/sh
cd '$C' || exit 1
env -i PATH='$PATH' HOME='$HOME' TERM=xterm-256color USER='$U' LOGNAME='$U' SHELL=/bin/sh LANG=C.UTF-8 DISABLE_AUTOUPDATER=1 $EXTRA \\
  $LAUNCHWORDS -p 'Run this Bash command one time, exactly as written, then answer with the one word done: sh ./start-down.sh' \\
  --output-format json --tools Bash --allowedTools 'Bash(sh ./start-down.sh)' --permission-prompts none \\
  --setting-sources project --strict-mcp-config --no-session-persistence \\
  </dev/null >'$RUN/out-$v.json' 2>'$RUN/err-$v.txt'
echo \$? >'$RUN/rc-$v.tmp'; mv '$RUN/rc-$v.tmp' '$RUN/rc-$v'
exec sleep 300
EOF
  if ! tm new-session -d -s "zp15$v" -x 200 -y 50 -c "$C" -- /bin/sh "$RUN/agent-$v.sh" 2>"$RUN/tmux-$v.err"; then
    VR="tmux did not start the pane"
    vline; return 0
  fi
  # Wait 1: the agent's exit.
  if ! wait_for 180 test -s "$RUN/rc-$v"; then
    tm kill-session -t "=zp15$v" 2>/dev/null || true
    vstop
    VR="timeout 180 s: the agent did not exit"
    vline; return 0
  fi
  t_exit=$(date +%s)
  rc=$(cat "$RUN/rc-$v")
  J=$(jq -r '[.is_error, .num_turns, ((.permission_denials // []) | length)] | @tsv' "$RUN/out-$v.json" 2>/dev/null || echo "? ? ?")
  # shellcheck disable=SC2086
  set -- $J
  turns=${2:-?}; den=${3:-?}
  [ ! -s "$C/start.head" ] || ow=$(sed -n 's/^outside_write=//p' "$C/start.head")
  [ ! -s "$C/marker.head" ] || started=yes
  # Test 1: the agent ran the command.
  if [ "$rc" != 0 ] || [ ! -s "$C/start.head" ]; then
    tm kill-session -t "=zp15$v" 2>/dev/null || true
    vstop
    VR="the agent did not run the command (exit $rc; is_error, turns, denials: $(printf '%s' "$J" | tr '\t' ' '))"
    vline; return 0
  fi
  ran=yes
  # Test 2: alive after the tool call and the agent's exit.
  sleep 5
  a=$(tail -n 1 "$C/marker" 2>/dev/null || true)
  if [ "${a:-0}" -ge $((t_exit + 3)) ]; then a1=yes; else VR="the sleeper stopped after the tool call or the agent's exit"; fi
  # The pane's end.
  t_kill=$(date +%s)
  tm kill-session -t "=zp15$v" 2>/dev/null || true
  if ! wait_for 5 sh -c '! LC_ALL=C "$0" -S "$1" -f /dev/null has-session -t "$2"' "$TMUX_BIN" "$SOCK" "=zp15$v"; then
    [ -n "$VR" ] || VR="timeout 5 s: the pane's session did not end"
  fi
  # Test 3: alive after the pane's end.
  sleep 5
  b=$(tail -n 1 "$C/marker" 2>/dev/null || true)
  if [ "${b:-0}" -ge $((t_kill + 3)) ]; then a2=yes; else [ -n "$VR" ] || VR="the sleeper stopped at the pane's end"; fi
  # Test 4: the sandbox state is the wanted one.
  mnt=$(sed -n 's/^mnt=//p' "$C/start.head")
  if [ "$MNT_SELF" != none ]; then
    [ "$mnt" = "$MNT_SELF" ] || insb=yes
  else
    [ "$ow" != no ] || insb=yes
  fi
  if [ "$v" = off ] && [ "$insb" = yes ]; then [ -n "$VR" ] || VR="the sandbox was in effect"; fi
  if [ "$v" = on ] && [ "$insb" = no ]; then [ -n "$VR" ] || VR="the sandbox was not in effect"; fi
  vstop
  vline
}
# vstop: end this variant's sleeper (at most 3 s wait for its `stopped` file). With P15_KEEP=<dir>
# (for a diagnosis; never a fixture) the session's raw files are copied to <dir>/<variant> first.
vstop() {
  : >"$C/stop"
  [ ! -s "$C/marker.head" ] || wait_for 3 test -e "$C/stopped" || true
  [ -n "${P15_KEEP:-}" ] || return 0
  mkdir -p "$P15_KEEP/$v"
  ls -la "$C" >"$P15_KEEP/$v/ls.txt" 2>&1 || true
  for f in "$RUN/out-$v.json" "$RUN/err-$v.txt" "$RUN/agent-$v.sh" "$C/down.log" "$C/start.head" "$C/marker.head" "$C/marker"; do
    [ ! -e "$f" ] || cp "$f" "$P15_KEEP/$v/" 2>/dev/null || true
  done
}
# vline: the variant's record line; VR becomes ok when no reason is set.
vline() {
  if [ -z "$VR" ]; then w=ok; VR=ok; x=""; else w=FAIL; x="; $VR"; fi
  printf '15.%s %s exit %s; turns %s; denials %s; ran %s; sleeper started %s; alive after agent exit %s; alive after pane end %s; in sandbox %s; outside write %s%s\n' \
    "$v" "$w" "$rc" "$turns" "$den" "$ran" "$started" "$a1" "$a2" "$insb" "$ow" "$x" | sed "s#$SOCK#SOCKET#g; s#$ZP_PRIV#PRIV#g" >>"$RUN/pw15.txt"
}

# --- real mode (node E2) ---------------------------------------------------------------------
CCD=$(printenv CLAUDE_CONFIG_DIR || true)
renv() { env -i PATH="$PATH" HOME="$HOME" TERM=xterm-256color USER="$U" LOGNAME="$U" SHELL=/bin/sh LANG=C.UTF-8 \
  DISABLE_AUTOUPDATER=1 ZYGO_NO_UPDATE_CHECK=1 ZYGO_CONFIG="$RUN/zygo.toml" ${CCD:+CLAUDE_CONFIG_DIR="$CCD"} "$@"; }
wsphase() { (cd "$C" && renv "$ZYGO" workspace) 2>/dev/null | sed -n '1s/^\(zygo: \)\{0,1\}workspace [^:]*: //p'; }
is_down() { [ "$(wsphase)" = down ]; }
# real <bash|tool|slash> <off|on>: one workspace, one member agent in print mode; one record line; sets VR.
real() {
  p=$1 v=$2 name="$1-$2" C="$R/$1-$2" VR=""
  rc="?" started=no drc="-" guard=no insb="-" ph="?" logend=no after=no gone=no prem="-" where="-"
  if [ "$v" = on ] && [ "$MNT_SELF" != none ] && { ! command -v bwrap >/dev/null 2>&1 || ! command -v socat >/dev/null 2>&1; }; then
    VR="sandbox not available (bwrap or socat is not on PATH)"; rline; return 0
  fi
  mkdir -p "$C/.claude"
  git -C "$C" init -q && git -C "$C" -c user.name=zygo-live -c user.email=zygo-live@example.invalid commit -q --allow-empty -m init
  case "$p" in
    bash) allow='Bash(sh ./down.sh)'; prompt='Run this Bash command one time, exactly as written, then answer with the one word done: sh ./down.sh' ;;
    tool) allow='mcp__zygo__workspace_down'; prompt='Call the tool mcp__zygo__workspace_down one time with no arguments, then answer with the one word done.' ;;
    *) allow='Bash(true)'; prompt='/zygo:down' ;;
  esac
  sb=false; [ "$v" = on ] && sb=true
  # P16 (slash runs only): a project hook keeps each UserPromptSubmit input, so the record shows whether the
  # print-mode prompt reached the hooks as its raw text. It prints nothing, so it decides nothing.
  ups=""; [ "$p" = slash ] && ups=',"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"cat >>./ups.jsonl; echo >>./ups.jsonl"}]}]}'
  printf '{"sandbox":{"enabled":%s,"allowUnsandboxedCommands":false,"autoAllowBashIfSandboxed":false},"permissions":{"allow":["%s"]}%s}\n' "$sb" "$allow" "$ups" >"$C/.claude/settings.json"
  cat >"$C/down.sh" <<EOF
#!/bin/sh
echo "mnt=\$(readlink /proc/self/ns/mnt 2>/dev/null || echo none)" >down.head
'$ZYGO' down -- '$name' >down.out 2>&1
echo \$? >down.rc
cat down.out
EOF
  cat >"$RUN/agent-$name.sh" <<EOF
#!/bin/sh
cd '$C' || exit 1
'$ZYGO' claude --passthrough -- -p '$prompt' --output-format stream-json --verbose --tools Bash --allowedTools '$allow' \\
  --permission-prompts none --setting-sources project --strict-mcp-config --no-session-persistence \\
  </dev/null >'$RUN/out-$name.jsonl' 2>'$RUN/err-$name.txt'
echo \$? >'$RUN/rc-$name.tmp'; mv '$RUN/rc-$name.tmp' '$RUN/rc-$name'
exec sleep 300
EOF
  renv "$TMUX_BIN" -S "$SOCK" -f /dev/null new-session -d -s hold -x 200 -y 50 /bin/sleep 3600 || { VR="tmux did not start"; rline; return 0; }
  SP=$(tm display-message -p '#{pid}')
  if ! (cd "$C" && renv TMUX="$SOCK,$SP,0" "$ZYGO" up --loadout pw15 --no-attach) >"$RUN/up-$name.txt" 2>&1; then
    VR="zygo up failed: $(sed -n 1p "$RUN/up-$name.txt")"
    renv "$ZYGO" ws forget -- "$name" >>"$RUN/clean-$name.txt" 2>&1; srv_stop; rline; return 0
  fi
  WS=$(tm show-environment -t "=zygo-$name:" ZYGO_WORKSPACE | sed -n 's/^ZYGO_WORKSPACE=//p')
  LOG="$HOME/.local/state/zygo/workspaces/$WS/down.log"
  PANE=$(tm split-window -d -P -F '#{pane_id}' -t "=zygo-$name:" -c "$C" -- /bin/sh "$RUN/agent-$name.sh")
  if ! wait_for 180 test -s "$RUN/rc-$name"; then VR="timeout 180 s: the agent did not exit"; rclean; rline; return 0; fi
  rc=$(cat "$RUN/rc-$name")
  grep -qF 'down started; end your turn' "$RUN/out-$name.jsonl" "$RUN/err-$name.txt" 2>/dev/null && started=yes
  if [ "$p" = slash ]; then
    prem=no; jq -e -s 'any(.[]; .prompt == "/zygo:down")' "$C/ups.jsonl" >/dev/null 2>&1 && prem=yes
    # Where the block reason shows: the stream-json line types that hold it, else the stderr file, else none.
    where=$(jq -r -s '[.[] | select(tostring | contains("down started; end your turn")) | .type] | unique | join(",")' "$RUN/out-$name.jsonl" 2>/dev/null)
    if [ -z "$where" ]; then where=none; grep -qF 'down started; end your turn' "$RUN/err-$name.txt" 2>/dev/null && where=stderr; fi
  fi
  if [ "$p" = bash ]; then
    drc=$(cat "$C/down.rc" 2>/dev/null || echo -)
    grep -qE 'cannot run detached from a sandboxed shell|cannot reach the tmux server of' "$C/down.out" 2>/dev/null && guard=yes
    mnt=$(sed -n 's/^mnt=//p' "$C/down.head" 2>/dev/null); insb=no; [ "$MNT_SELF" = none ] || [ "$mnt" = "$MNT_SELF" ] || insb=yes
  fi
  [ "$started" = no ] || wait_for 120 is_down || true
  ph=$(wsphase)
  grep -q "^zygo: $name is down (" "$LOG" 2>/dev/null && logend=yes
  [ -e "$LOG" ] && ! [ "$RUN/rc-$name" -nt "$LOG" ] && after=yes
  tm has-session -t "=zygo-$name:" 2>/dev/null || gone=yes
  out=no; [ "$started" = yes ] && [ "$ph" = down ] && [ "$logend" = yes ] && [ "$after" = yes ] && [ "$gone" = yes ] && out=yes
  if [ "$p" = bash ] && [ "$v" = on ]; then
    if [ "$out" = yes ]; then :
    elif [ "$started" = no ] && [ "$drc" = 1 ] && [ "$guard" = yes ] && [ "$ph" = up ] && [ ! -s "$LOG" ]; then VR="guard"
    elif [ "$started" = yes ]; then VR="down started without a running process"
    else VR="no outlived down and no AU18 line (down exit $drc)"; fi
  elif [ "$p" = slash ] && [ "$prem" = no ]; then VR="premise false: the print-mode prompt did not reach UserPromptSubmit as /zygo:down (out-$name.jsonl, err-$name.txt and $name/ups.jsonl kept)"
  elif [ "$out" = no ]; then VR="down did not outlive the call (started $started, phase $ph, log end $logend)"; fi
  [ "$p" != bash ] || [ "$v" != off ] || [ "$insb" = no ] || VR="${VR:+$VR; }the sandbox was in effect"
  rclean; rline
}
# rclean: stop the workspace if it still runs, forget it, stop the private server.
rclean() {
  [ "$(wsphase)" = down ] || (cd "$C" && renv TMUX="$SOCK,$SP,0" "$ZYGO" down --now --wait -- "$name") >>"$RUN/clean-$name.txt" 2>&1
  renv "$ZYGO" ws forget -- "$name" >>"$RUN/clean-$name.txt" 2>&1
  srv_stop
}
rline() {
  if [ -z "$VR" ]; then w=ok; x=""; VR=ok; elif [ "$VR" = guard ]; then w=ok; x="; AU18 guard, nothing started"; else w=FAIL; x="; $VR"; fi
  printf '15.%s %s exit %s; started line %s; down exit %s; AU18 line %s; in sandbox %s; phase %s; log end %s; log not older than agent exit %s; session gone %s; prompt reached the hook as text %s; block reason in %s%s\n' \
    "$name" "$w" "$rc" "$started" "$drc" "$guard" "$insb" "$ph" "$logend" "$after" "$gone" "$prem" "$where" "$x" | sed "s#$SOCK#SOCKET#g; s#$ZP_PRIV#PRIV#g" >>"$RUN/pw15.txt"
}
real_all() {
  command -v jq >/dev/null 2>&1 || { echo "live-detach: --zygo needs jq on PATH" >&2; exit 2; }
  printf '[features]\nworkspace_tool = true\n\n[loadout.pw15]\n[[loadout.pw15.window]]\n[[loadout.pw15.window.pane]]\nkind = "shell"\n' >"$RUN/zygo.toml"
  printf '%s\n' "launch zygo $ZV, real down" "claude $LIVE_CC_VER" "tmux $TV" >"$RUN/pw15.txt"
  # 27a: the slash premise (P16) first, with one run; then the other five.
  for pv in "slash off" "slash on" "bash off" "bash on" "tool off" "tool on"; do
    set -- $pv; real "$1" "$2"; eval "R_$1$2=\$VR; W_$1$2=\$where"
  done
  case "$R_slashoff" in
    "premise false"*) result PW15-slash-premise failed "$R_slashoff" ;;
    *) result PW15-slash-premise verified "print-mode /zygo:down reached UserPromptSubmit as its text; block reason in $W_slashoff" ;;
  esac
  scrub "$RUN/pw15.txt" >"$FX/pw15-down.txt"
  sum="bash off: $R_bashoff; bash on: $R_bashon; tool off: $R_tooloff; tool on: $R_toolon; slash off: $R_slashoff; slash on: $R_slashon; fixtures/tmux/pw15-down.txt"
  if [ "$R_bashoff" = ok ] && [ "$R_bashon" = ok ]; then result PW15 verified "launch zygo $ZV real down, tmux $TV; $sum"
  else result PW15 failed "launch zygo $ZV real down, tmux $TV; $sum"; fi
  # A slash run whose premise is false (P16) is no product failure: the fallback then rests on the other runs,
  # and the PW15-slash-premise line above carries the slash result.
  so=$R_slashoff sn=$R_slashon
  case "$so" in "premise false"*) so=ok ;; esac; case "$sn" in "premise false"*) sn=ok ;; esac
  fb=failed; case "$R_bashon" in ok | guard) [ "$R_bashoff$R_tooloff$R_toolon$so$sn" = okokokokok ] && fb=verified ;; esac
  result PW15-fallback "$fb" "the workspace_down tool and /zygo:down outlive the call with the sandbox off and on; the Bash tool outlives it or gets the AU18 line; $sum"
  live_done "$FX"; exit 0
}

if [ -n "$ZYGO" ]; then real_all; fi
printf '%s\n' "launch $LAUNCH" "claude $LIVE_CC_VER" "tmux $TV" >"$RUN/pw15.txt"
variant off; R_OFF=$VR
variant on; R_ON=$VR

scrub "$RUN/pw15.txt" >"$FX/$FXFILE"
if [ "$R_OFF" = ok ] && [ "$R_ON" = ok ]; then
  result PW15 verified "launch $LAUNCH, tmux $TV; no sandbox: ok; sandbox: ok; stand-in sleeper, no op.lock, no drain; fixtures/tmux/$FXFILE"
else
  result PW15 failed "launch $LAUNCH, tmux $TV; no sandbox: $R_OFF; sandbox: $R_ON; stand-in sleeper, no op.lock, no drain; fixtures/tmux/$FXFILE"
fi
srv_stop
live_done "$FX"

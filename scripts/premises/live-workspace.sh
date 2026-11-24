#!/bin/sh
# Live premises PW1, PW2, PW3, PW4, PW5, PW7, PW9, PW10, PW16 (node P2; workspaces spec W§17):
# what the pinned Claude Code and OpenCode do with session ids, hooks, resume and SIGTERM.
# Usage: sh scripts/premises/live-workspace.sh [c1 c2 c5 c5l c7 oc]
#   No argument: every part, one LIVE line for each of the nine ids, exit 0. The fixtures are written
#   to the scratch folder and replace the committed ones only at the end of the full run.
#   With part names: only those parts, for work on the script; fixtures stay in the scratch folder
#   and the LIVE lines are not results (a sub-check of a part that did not run is a FAIL).
#   N2_SCRATCH=<folder> (lib.sh): the run folders (hook logs, recorded requests, pane texts,
#   temporary HOMEs with the dummy key only) stay in <folder>/live-<pid>, for work on the script.
# Run by hand only, with claude and opencode on PATH (the caller adds the OpenCode folder).
#
# What this run does to the machine:
# - Parts c1, c2, c5, c7 (Claude Code) and oc (OpenCode) use a temporary HOME and config folder in
#   the scratch folder, the dummy key and the mock upstream of lib.sh: no login is read, no model
#   request leaves the machine, nothing under the real HOME is written. OpenCode fetches its
#   provider package from the package registry into the temporary HOME.
# - Part c5l (one sub-check of PW5) runs one print-mode turn of Claude Code on the stored login of
#   the real HOME, with the caller's CLAUDE_CONFIG_DIR (else the default config folder):
#   --setting-sources project leaves the user's settings, hooks and plugins out, --strict-mcp-config
#   leaves the MCP servers out, --no-session-persistence writes no transcript. It spends the
#   operator's usage on about two model requests. Claude Code 2.1.330 still makes an empty
#   projects/<scratch repository>/memory folder in that config folder and can update its own state
#   file; note 5.6 records what it found (read only; this script changes and removes nothing there).
# - Every tmux command uses a server of its own (-S <socket in a private folder>, -f /dev/null).
#   The server, every agent and helper process and the private folder are removed on every exit.
# - PW3 needs a second Claude Code program at or above the pinned version and below the one on
#   PATH: PW3_OLD_CLAUDE=<path>, else the highest such version file beside the program on PATH.
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
. "$PREMISES_DIR/workspace_lib.sh"   # clean, sub, note, rest, finish
live_traps
set +e   # a failed command is a result here; every wait and every test handles its own status
ZP_PRIV="" SOCK="" TMUX_BIN="" BASE=""
tm() { LC_ALL=C "$TMUX_BIN" -S "$SOCK" -f /dev/null "$@"; }
# zp_end <status>: the private server, every process this script started, the private folder; then live_end.
zp_end() {
  trap - EXIT INT TERM HUP PIPE
  [ -z "$SOCK" ] || tm kill-server 2>/dev/null
  for _p in $LIVE_PIDS; do kill "$_p" 2>/dev/null; done
  [ -z "$LIVE_PIDS" ] || sleep 1
  for _p in $LIVE_PIDS; do kill -KILL "$_p" 2>/dev/null; done
  [ -z "$ZP_PRIV" ] || rm -rf "$ZP_PRIV"
  live_end "$1"
}
trap 'zp_end $?' EXIT; trap 'zp_end 129' HUP; trap 'zp_end 130' INT; trap 'zp_end 141' PIPE; trap 'zp_end 143' TERM

PARTS="c1 c2 c5 c5l c7 oc"
for _a in "$@"; do
  case " $PARTS " in *" $_a "*) ;; *) echo "usage: live-workspace.sh [c1 c2 c5 c5l c7 oc]"; exit 2 ;; esac
done
TMUX_BIN=$(command -v tmux) || { echo "live: tmux is not on PATH"; exit 1; }
for _t in jq python3 perl; do command -v "$_t" >/dev/null || { echo "live: $_t is not on PATH"; exit 1; }; done
pinned_agents
case "$N2_SCRATCH$CLAUDE_BIN$OPENCODE_BIN$TOOL_PATH$HOME" in
  *"'"* | *" "*) echo "live: a path holds a single quote or a space; this script cannot use it"; exit 1 ;;
esac
ZP_PRIV=$(mktemp -d "${TMPDIR:-/tmp}/zp2.XXXXXX") || exit 1
SOCK="$ZP_PRIV/s"
[ ${#SOCK} -le 100 ] || { echo "live: the socket path is longer than 100 characters; set TMPDIR to a short directory"; exit 1; }
echo "live: Claude Code $LIVE_CC_VER, OpenCode $LIVE_OC_VER, $("$TMUX_BIN" -V)"

BASE="$N2_SCRATCH/live-$$"
REC="$BASE/ws-rec"; mkdir -p "$REC"
# Fixtures are written to the scratch folder; only a full run copies them over the committed ones, at its end.
FXC="$BASE/fx-claude-code"; FXO="$BASE/fx-opencode"
OUTC="$REPO_DIR/fixtures/claude-code/workspace"; OUTO="$REPO_DIR/fixtures/opencode/workspace"
[ $# -eq 0 ] || { OUTC=$FXC; OUTO=$FXO; echo "live: partial run ($*): fixtures stay in the scratch folder; the LIVE lines are not results"; }
mkdir -p "$FXC" "$FXO"
EXTRA_ENV="DISABLE_AUTOUPDATER=1"
PW_TICKS=$(pw_ticks)   # polls per second of a wait bound (workspace_lib.sh: 10 unless PW_TEST=1)
DENY="zygo: workspace is draining for down; finish your turn"
BLOCK="zygo: workspace is stopping"
FULL='{"input_tokens":1000,"cache_creation_input_tokens":0,"cache_read_input_tokens":999000,"output_tokens":5}'
AGENTS='{"pw-sub":{"description":"probe subagent","prompt":"Do what the message says."}}'
MAIN='"no_header": ["x-claude-code-agent-id"], "extra": {"body": "\"tools\": \\[\\{"}'
SUBH='"header": {"x-claude-code-agent-id": "."}'

# --- helpers ---------------------------------------------------------------------------------
# wait_for <seconds> <command...>: poll every 0.1 s; 0 when the command succeeds in time, else 1.
wait_for() { _w=$(($1 * PW_TICKS)); shift; while ! "$@" >/dev/null 2>&1; do _w=$((_w - 1)); [ "$_w" -gt 0 ] || return 1; sleep 0.1; done; }
nap() { _n=$(($1 * PW_TICKS)); while [ "$_n" -gt 0 ]; do sleep 0.1; _n=$((_n - 1)); done; }
gone() { ! kill -0 "$1" 2>/dev/null; }
unpid() { LIVE_PIDS=$(printf '%s\n' $LIVE_PIDS | grep -vx "$1" | tr '\n' ' '); }
uuid() { python3 -I -c 'import uuid; print(uuid.uuid4())'; }
yn() { if [ "$1" -eq 0 ]; then echo yes; else echo no; fi; }
# pnote <PW id> <sub id> <detail>: note (workspace_lib.sh), and the same as one stdout line "note <PW id> <sub id> <detail>".
pnote() { note "$@"; printf 'note %s %s %s\n' "$1" "$2" "$(clean "$3")"; }
# pw4_rest <reason>: the notes 4.7, 4.8 and 4.9 when part oc ends before them (node E2).
pw4_rest() { for n in 4.7 4.8 4.9; do pnote PW4 "$n" "not run: $1"; done; }
WA="$PREMISES_DIR/workspace_analyze.py"
# steps <prefix> <steps json>: SSE files and mock rules of one scripted sequence (steps.py)
steps() {
  mkdir -p "$RUN/sse"
  printf '%s\n' "$2" >"$RUN/steps-$1.json"
  python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" "$1" "$RUN/rules.json" "$RUN/steps-$1.json"
}
# cc_settings <tag> <allow rules json>: hook log $RUN/hooks-<tag> (HK), rule file (RS), settings file (ST)
cc_settings() {
  HK="$RUN/hooks-$1"; RS="$RUN/responses-$1.json"; ST="$RUN/settings-$1.json"
  mkdir -p "$HK"; echo '[]' >"$RS"
  hook_settings "$HK" "$RS" | jq --argjson a "$2" '.hooks.StopFailure = .hooks.Stop | . + {permissions: {allow: $a, defaultMode: "default"}}' >"$ST"
}
nhook() { ls "$1" 2>/dev/null | grep -Ec "^[0-9]+-($2)\\.json\$"; }                    # <dir> <name regex>: count
has_hooks() { [ "$(nhook "$1" "$2")" -ge "$3" ]; }                                      # <dir> <name regex> <count>
hookfile() { ls "$1" 2>/dev/null | grep -E "^[0-9]+-($2)\\.json\$" | sed -n "${3:-1}p"; } # <dir> <name regex> [n]: n-th name
hval() { jq -r "$2 // \"absent\"" "$1" 2>/dev/null || echo unreadable; }                 # <file> <jq path>
ends() { nhook "$HK" 'Stop-main|StopFailure-main'; }
ends_gt() { [ "$(ends)" -gt "$1" ]; }
# typed_stops <count>: at least <count> SubagentStop inputs of a dispatched subagent (non-empty agent_type) in $HK
# (Claude Code 2.1.330 runs a dispatched agent in the background, so the lead's Stop can come before it ends)
typed_stops() { [ "$(jq -s '[.[] | select(.hook_event_name == "SubagentStop" and (.agent_type // "") != "")] | length' "$HK"/[0-9]*.json 2>/dev/null)" -ge "$1" ]; }
# cc_pane <session> <dir> <claude args>: Claude Code on the mock in a pane of the private server; CCPID = its process id
cc_pane() {
  _s=$1 _d=$2; shift 2
  CCPID=""   # never a pid of an earlier pane
  _q=""; for _x in "$@"; do _q="$_q '$(printf '%s' "$_x" | sed "s/'/'\\\\''/g")'"; done
  echo "cd '$_d' && exec $(agent_env) $EXTRA_ENV '$CLAUDE_BIN'$_q" >"$RUN/pane-$_s.sh"
  tm new-session -d -s "$_s" -x 200 -y 50 -c "$_d" -- /bin/sh "$RUN/pane-$_s.sh" || return 1
  CCPID=$(tm display-message -p -t "=$_s:" '#{pane_pid}')
  case "$CCPID" in "" | *[!0-9]*) CCPID=""; tm kill-session -t "=$_s" 2>/dev/null; return 1 ;; esac
  LIVE_PIDS="$LIVE_PIDS $CCPID"; CCS=$_s
}
PTR=$(printf '\342\235\257')   # U+276F, the pointer of a Claude Code menu
TRUST_YES="Yes, I trust"       # the accept option of the folder trust question
trust_line() { tm capture-pane -p -t "=$1:" 2>/dev/null | grep -F "$TRUST_YES" | tail -n 1; }
# cc_trust <session>: one key step on the folder trust question (the method of live-hud.sh: Claude Code
# 2.1.330 starts with the pointer on "No, exit"). Pointer not on the accept line: Down. Pointer on it
# in two readings 0.5 s apart: Enter. No question in the pane: nothing.
cc_trust() {
  case "$(trust_line "$1")" in
    "") ;;
    *"$PTR"*) sleep 0.5; case "$(trust_line "$1")" in *"$PTR"*) tm send-keys -t "=$1:" Enter; sleep 1 ;; esac ;;
    *) tm send-keys -t "=$1:" Down; sleep 1 ;;
  esac
}
# cc_ready <seconds>: the first SessionStart input of the main loop of the pane of cc_pane is in $HK (polls
# every 0.5 s; answers the folder trust question with yes)
cc_ready() {
  _c=$(($1 * PW_TICKS / 5))
  until has_hooks "$HK" SessionStart-main 1; do
    cc_trust "$CCS"; _c=$((_c - 1)); [ "$_c" -gt 0 ] || return 1; sleep 0.5
  done
}
# pane_end <session> <pid>: keep the pane text in the scratch folder, end the pane and its program
pane_end() {
  tm capture-pane -p -t "=$1:" >"$RUN/pane-$1.txt" 2>/dev/null
  tm kill-session -t "=$1" 2>/dev/null
  [ -n "$2" ] || return 0
  kill "$2" 2>/dev/null; wait_for 5 gone "$2" || kill -KILL "$2" 2>/dev/null
  unpid "$2"
}
say() { tm send-keys -t "=$1:" -l "$2"; sleep 0.5; tm send-keys -t "=$1:" Enter; }
# turn <session> <text> <seconds>: type one prompt; 0 when a Stop or StopFailure of the main loop follows in time
turn() { _e=$(ends); say "$1" "$2"; wait_for "$3" ends_gt "$_e"; }
# cc_print <tag> <seconds> <dir> <program> <args>: one bounded print-mode run on the mock (live_exec: isolated environment)
cc_print() {
  _t=$1 _s=$2 _d=$3; shift 3
  LIVE_VARS="ANTHROPIC_BASE_URL=$MOCK_URL ANTHROPIC_API_KEY=$DUMMY_KEY CLAUDE_CODE_GATEWAY_HINT_HEADERS=1"; LIVE_TIMEOUT=$_s
  live_exec /bin/sh -c 'cd "$1" && shift && exec "$@"' sh "$_d" "$@" </dev/null >"$RUN/out-$_t.txt" 2>"$RUN/err-$_t.txt"; _rc=$?
  LIVE_VARS=""; LIVE_TIMEOUT=""
  return $_rc
}
said() { { sed -n 1p "$RUN/out-$1.txt"; sed -n 1p "$RUN/err-$1.txt"; } 2>/dev/null | tr '\n' ' ' | cut -c1-120; }  # <tag>: first output lines
req_has() { [ -n "$(body_has "$1" "$RUN"/rec/[0-9]*.json 2>/dev/null)" ]; }               # <regex>: a recorded request holds it
# req_both <regex a> <regex b>: 0 when one recorded request holds both
req_both() { _f=$(body_has "$1" "$RUN"/rec/[0-9]*.json 2>/dev/null); [ -n "$_f" ] && [ -n "$(body_has "$2" $_f)" ]; }
# control <marker>: one request with no credential to the mock (recorded and answered 401; no agent involved)
control() {
  python3 -I -c 'import json, sys, urllib.request
r = urllib.request.Request(sys.argv[1] + "/v1/messages", method="POST", headers={"content-type": "application/json"},
    data=json.dumps({"model": "control", "max_tokens": 1, "messages": [{"role": "user", "content": sys.argv[2]}]}).encode())
try:
    urllib.request.urlopen(r, timeout=5)
except urllib.error.HTTPError as e:
    sys.exit(0 if e.code == 401 else 1)' "$MOCK_URL" "$1"
}
# pub <file> <fixture name> [<fixture dir>]: scrubbed copy; each agent id becomes agent-id-<n>
pub() {
  [ -f "$1" ] || return 0
  scrub "$1" | python3 -I -c 'import re, sys
t = sys.stdin.read()
ids = []
# agent_id values, and Claude Code agent ids in other keys (the "id" in background_tasks of a Stop input)
for m in re.finditer(r"\"agent_id\": \"([^\"]+)\"|\b(a[0-9a-f]{16})\b", t):
    a = m.group(1) or m.group(2)
    if a not in ids:
        ids.append(a)
for i, a in enumerate(ids):
    t = t.replace(a, "agent-id-%d" % (i + 1))
# no time in a fixture: the seconds since the last answer of a resumed session become 0
t = re.sub(r"(\"seconds_since_last_response\": )[0-9.]+", r"\g<1>0", t)
sys.stdout.write(t)' >"${3:-$FXC}/$2"
}

# --- c1: PW1, PW9 (Claude Code), PW10, PW16, PW3 ----------------------------------------------
part_c1() {
  ALL1="PW1:1.1 PW1:1.2 PW1:1.3 PW1:1.4 PW9:9.1 PW9:9.2 PW9:9.3 PW10:10.1 PW10:10.2 PW16:16.1 PW16:16.2 PW3:3.1 PW3:3.2"
  live_run c1; start_mock
  cc_settings a '[]'
  SID=$(uuid)
  cc_pane zc1 "$R" --tools "" --settings "$ST" --session-id "$SID" || { rest "tmux did not start the pane" $ALL1; stop_mock; return; }
  if ! cc_ready 60; then
    pane_end zc1 "$CCPID"; rest "timeout 60 s: no SessionStart hook after the start" $ALL1; stop_mock; return
  fi
  f="$HK/$(hookfile "$HK" SessionStart-main 1)"
  t=1; [ "$(hval "$f" .session_id)" = "$SID" ] && t=0
  sub PW1 1.1 $t "start with --session-id: SessionStart source $(hval "$f" .source); session_id is the given id: $(yn $t)"
  pub "$f" SessionStart-preassigned.json
  nap 3
  if ! turn zc1 "tracer-pw1-a first turn" 60; then
    pane_end zc1 "$CCPID"; rest "timeout 60 s: no Stop hook after the first prompt" PW1:1.2 PW1:1.3 PW1:1.4 PW9:9.1 PW9:9.2 PW9:9.3 PW10:10.1 PW10:10.2 PW16:16.1 PW16:16.2
  else
    # PW9, Claude Code: SIGTERM to the idle agent (the launcher forwards SIGTERM to this process)
    kill -TERM "$CCPID"
    t=1; wait_for 15 gone "$CCPID" && t=0
    sub PW9 9.1 $t "Claude Code, idle, SIGTERM: process ended within 15 s: $(yn $t)"
    pane_end zc1 "$CCPID"
    tr=$(find "$H/.claude/projects" -name "$SID.jsonl" 2>/dev/null | sed -n 1p)
    t=1; [ -n "$tr" ] && grep -q 'tracer-pw1-a' "$tr" && t=0
    sub PW9 9.2 $t "Claude Code: transcript file named by the session id holds the first prompt: $(yn $t)"
    # Resume in a new pane; the SessionStart hook of this launch returns a note as additional context.
    cc_settings r '[]'
    echo '[{"event": "SessionStart", "output": {"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": "tracer-pw10-note"}}}]' >"$RS"
    if ! cc_pane zc1r "$R" --tools "" --settings "$ST" --resume "$SID"; then
      rest "tmux did not start the resume pane" PW1:1.2 PW1:1.3 PW1:1.4 PW9:9.3 PW10:10.1 PW10:10.2
    elif ! cc_ready 60; then
      pane_end zc1r "$CCPID"; rest "timeout 60 s: no SessionStart hook after --resume" PW1:1.2 PW1:1.3 PW1:1.4 PW9:9.3 PW10:10.1 PW10:10.2
    else
      f="$HK/$(hookfile "$HK" SessionStart-main 1)"
      src=$(hval "$f" .source)
      t=1; [ "$src" = resume ] && t=0
      sub PW10 10.1 $t "SessionStart of the resumed session: source $src"
      t=1; [ "$(hval "$f" .session_id)" = "$SID" ] && t=0
      sub PW1 1.2 $t "after --resume without --fork-session: SessionStart session_id is the first id: $(yn $t)"
      pub "$f" SessionStart-resume.json
      nap 3
      if ! turn zc1r "tracer-pw1-b second turn" 60; then
        rest "timeout 60 s: no Stop hook after the prompt in the resumed session" PW1:1.3 PW1:1.4 PW9:9.3 PW10:10.2
      else
        t=1; req_both 'tracer-pw1-b' 'tracer-pw1-a' && t=0
        sub PW1 1.3 $t "the request of the second prompt holds the first prompt (same conversation): $(yn $t)"
        sub PW9 9.3 $t "Claude Code: the conversation saved at SIGTERM was resumed and continued: $(yn $t)"
        seen=$(jq -r .session_id "$HK"/[0-9]*.json 2>/dev/null | sort -u | tr '\n' ' ')
        t=1; [ "$seen" = "$SID " ] && t=0
        sub PW1 1.4 $t "every hook input of the resumed session ($(nhook "$HK" '.*') inputs) has the first id: $(yn $t)"
        t=1; req_both 'tracer-pw1-b' 'tracer-pw10-note' && t=0
        sub PW10 10.2 $t "additional context returned by the resume SessionStart hook is in the next request: $(yn $t)"
      fi
      # Fallback data for PW9 (not part of the verdict): /exit typed into the idle agent.
      say zc1r "/exit"
      t=1; wait_for 15 gone "$CCPID" && t=0
      note PW9 9.4 "Claude Code, idle, /exit by send-keys: process ended within 15 s: $(yn $t)"
      pane_end zc1r "$CCPID"
    fi
    # PW16: resume by id from another cwd, then with another config folder (print mode).
    cc_settings p '[]'
    mkdir -p "$RUN/cwd2" "$RUN/cfg2"
    cc_print 16a 90 "$RUN/cwd2" "$CLAUDE_BIN" -p "tracer-pw16-a other cwd" --tools "" --settings "$ST" --resume "$SID"; rc=$?
    t=1; req_both 'tracer-pw16-a' 'tracer-pw1-a' && t=0
    sub PW16 16.1 $t "another cwd, same config folder: exit $rc; conversation found: $(yn $t); output: $(said 16a)"
    cp "$H/.claude/.claude.json" "$RUN/cfg2/.claude.json"
    LIVE_CC_DIR="$RUN/cfg2"
    cc_print 16b 90 "$R" "$CLAUDE_BIN" -p "tracer-pw16-b other config" --tools "" --settings "$ST" --resume "$SID"; rc=$?
    LIVE_CC_DIR=""
    t=1; req_both 'tracer-pw16-b' 'tracer-pw1-a' && t=0
    sub PW16 16.2 $t "same cwd, another CLAUDE_CONFIG_DIR: exit $rc; conversation found: $(yn $t); output: $(said 16b)"
  fi
  # PW3: a conversation started by an older Claude Code, resumed by the one on PATH (print mode).
  OLD=${PW3_OLD_CLAUDE:-}
  if [ -z "$OLD" ]; then
    cur=$(python3 -I -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$CLAUDE_BIN"); best=""
    for c in "$(dirname "$cur")"/*; do
      [ -f "$c" ] && [ -x "$c" ] || continue
      v=$(basename "$c")
      agent_ok "$v" "$PIN_CC" || continue
      ver_ge "$v" "$LIVE_CC_VER" && continue
      if [ -z "$best" ] || ver_ge "$v" "$best"; then best=$v; OLD=$c; fi
    done
  fi
  OLDV=""; [ -z "$OLD" ] || OLDV=$(agent_ver "$("$OLD" --version 2>/dev/null)")
  if [ -z "$OLDV" ] || ! agent_ok "$OLDV" "$PIN_CC" || ver_ge "$OLDV" "$LIVE_CC_VER"; then
    rest "no second Claude Code at or above $PIN_CC and below $LIVE_CC_VER (set PW3_OLD_CLAUDE)" PW3:3.1 PW3:3.2
  else
    cc_settings u '[]'
    SID3=$(uuid)
    cc_print 3a 90 "$R" "$OLD" -p "tracer-pw3-a old version" --tools "" --settings "$ST" --session-id "$SID3"; rc=$?
    f=$(body_has 'tracer-pw3-a' "$RUN"/rec/[0-9]*.json 2>/dev/null | sed -n 1p)
    ua1=$([ -z "$f" ] || jq -r '.headers["user-agent"] // "absent"' "$f" | cut -c1-60)
    t=1; [ "$rc" -eq 0 ] && [ -n "$f" ] && t=0
    sub PW3 3.1 $t "first launch on Claude Code $OLDV with --session-id: exit $rc; request seen: $(yn $t); user-agent: ${ua1:-none}"
    cc_print 3b 90 "$R" "$CLAUDE_BIN" -p "tracer-pw3-b new version" --tools "" --settings "$ST" --resume "$SID3"; rc=$?
    f=$(body_has 'tracer-pw3-b' "$RUN"/rec/[0-9]*.json 2>/dev/null | sed -n 1p)
    ua2=$([ -z "$f" ] || jq -r '.headers["user-agent"] // "absent"' "$f" | cut -c1-60)
    t=1; req_both 'tracer-pw3-b' 'tracer-pw3-a' && t=0
    sub PW3 3.2 $t "resume on Claude Code $LIVE_CC_VER: exit $rc; conversation found: $(yn $t); user-agent: ${ua2:-none}; output: $(said 3b)"
  fi
  stop_mock
}

# --- c2: PW2 -----------------------------------------------------------------------------------
part_c2() {
  ALL2="PW2:2.1 PW2:2.2 PW2:2.3"
  live_run c2
  steps pw2-sub '{"trigger": "tracer-pw2-sub", '"$SUBH"', "steps": [
    {"tool": "Bash", "input": {"command": "true"}, "usage": '"$FULL"', "iterations": true}]}'
  steps pw2 '{"trigger": "tracer-pw2-lead go", '"$MAIN"', "steps": [
    {"tool": "Agent", "input": {"subagent_type": "pw-sub", "prompt": "tracer-pw2-sub run one command", "description": "pw2", "run_in_background": false}},
    {"text": "pw2 lead done"}]}'
  start_mock "$RUN/rules.json"
  cc_settings a '["Bash(true)"]'
  SID=$(uuid)
  cc_pane zc2 "$R" --tools "Bash,Agent" --agents "$AGENTS" --settings "$ST" --session-id "$SID" || { rest "tmux did not start the pane" $ALL2; stop_mock; return; }
  if ! cc_ready 60; then
    pane_end zc2 "$CCPID"; rest "timeout 60 s: no SessionStart hook after the start" $ALL2; stop_mock; return
  fi
  f1="$HK/$(hookfile "$HK" SessionStart-main 1)"
  s1=$(hval "$f1" .session_id)
  t=1; [ "$s1" = "$SID" ] && t=0
  sub PW2 2.1 $t "SessionStart input has session_id: $(yn $t) (source $(hval "$f1" .source))"
  nap 3
  if ! turn zc2 "tracer-pw2-a first turn" 60; then
    pane_end zc2 "$CCPID"; rest "timeout 60 s: no Stop hook after the first prompt" PW2:2.2 PW2:2.3; stop_mock; return
  fi
  say zc2 "/clear"
  if ! wait_for 30 has_hooks "$HK" SessionStart-main 2; then
    sub PW2 2.2 1 "timeout 30 s: no second SessionStart of the main loop after /clear"
  else
    f2="$HK/$(hookfile "$HK" SessionStart-main 2)"
    s2=$(hval "$f2" .session_id)
    t=1; printf '%s\n' "$s2" | grep -Eq '^[0-9a-f]{8}-([0-9a-f]{4}-){3}[0-9a-f]{12}$' && [ "$s2" != "$s1" ] && t=0
    sub PW2 2.2 $t "SessionStart fires again after /clear: yes; source $(hval "$f2" .source); new session_id: $(yn $t)"
    pub "$f2" SessionStart-clear.json
  fi
  nap 2
  if ! turn zc2 "tracer-pw2-lead go" 120; then
    sub PW2 2.3 1 "timeout 120 s: no Stop hook after the turn with the subagent"
  else
    wait_for 120 typed_stops 1 || note PW2 2.5 "timeout 120 s: no SubagentStop input of the dispatched subagent"
    fs="$HK/$(hookfile "$HK" Stop-main 2)"
    if [ -n "${s2:-}" ]; then
      t=1; [ "$(hval "$fs" .session_id)" = "$s2" ] && t=0
      note PW2 2.4 "the next Stop input after /clear has the new session_id: $(yn $t)"
      pub "$fs" Stop-after-clear.json
    fi
    # SessionStart inputs between the subagent's SubagentStart and its SubagentStop belong to the subagent.
    f1=$(rec_2_3 "$(python3 -I "$WA" pw2-sub "$HK")")
    [ "$f1" = none ] || pub "$HK/$f1" SessionStart-sub.json
  fi
  pane_end zc2 "$CCPID"; stop_mock
}

# --- c5: PW5 on the mock -----------------------------------------------------------------------
part_c5() {
  ALL5="PW5:5.1 PW5:5.2 PW5:5.3 PW5:5.4"
  live_run c5
  steps pw5 '{"trigger": "tracer-pw5-lead go", '"$MAIN"', "steps": [
    {"tool": "Agent", "input": {"subagent_type": "pw-sub", "prompt": "tracer-pw5-sub say hi", "description": "pw5", "run_in_background": false}},
    {"text": "pw5 lead done"}]}'
  start_mock "$RUN/rules.json"
  cc_settings a '[]'
  jq -n --arg r "$DENY" '[{event: "PreToolUse", tool: "^Agent$", output: {hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $r}}}]' >"$RS"
  cc_pane zc5 "$R" --tools "Agent" --agents "$AGENTS" --settings "$ST" || { rest "tmux did not start the pane" $ALL5; stop_mock; return; }
  if ! cc_ready 60; then
    pane_end zc5 "$CCPID"; rest "timeout 60 s: no SessionStart hook after the start" $ALL5; stop_mock; return
  fi
  nap 3
  t=1; turn zc5 "tracer-pw5-lead go" 60 && t=0
  ended=$t
  pre=$(nhook "$HK" PreToolUse-Agent-main); ss=$(nhook "$HK" 'SubagentStart-.*')
  subreq=$(jq -r 'select(.headers["x-claude-code-agent-id"] != null) | .seq' "$RUN"/rec/[0-9]*.json 2>/dev/null | grep -c .)
  t=1; [ "$pre" -ge 1 ] && [ "$ss" -eq 0 ] && [ "$subreq" -eq 0 ] && t=0
  sub PW5 5.1 $t "PreToolUse deny on Agent: PreToolUse inputs $pre; SubagentStart inputs $ss; subagent requests $subreq; dispatch blocked: $(yn $t)"
  # The lead's next request: the first one that holds the tool result of the denied call and is not
  # Claude Code's prompt-suggestion request (its last user message starts with "[SUGGESTION MODE:").
  f=$(python3 -I "$WA" lead-next "$RUN/rec" toolu_pw5_00_agent)
  t=1; [ -n "$f" ] && [ -n "$(body_has 'workspace is draining for down; finish your turn' "$f")" ] && t=0
  sub PW5 5.2 $t "the next request of the lead (the first one with the tool result, not the prompt-suggestion request) holds the deny reason: $(yn $t)"
  sub PW5 5.3 $ended "the turn ended after the deny (Stop within 60 s; scripted lead on the mock): $(yn $ended)"
  pub "$HK/$(hookfile "$HK" PreToolUse-Agent-main 1)" PreToolUse-Agent-denied.json
  # The fixture says what that tool result holds, without its text (no harness text in a fixture).
  if [ -n "$f" ]; then python3 -I "$WA" tool-result "$f" toolu_pw5_00_agent "$DENY" >"$RUN/deny-next.json"; pub "$RUN/deny-next.json" deny-next-tool-result.json; fi
  # A typed prompt while UserPromptSubmit blocks. A control request before and one after the bounded
  # wait show that the mock was alive and recorded requests during it.
  jq -n --arg r "$BLOCK" '[{event: "UserPromptSubmit", output: {decision: "block", reason: $r}}]' >"$RS"
  k1=1; control tracer-pw5-control-1 && wait_for 10 req_has 'tracer-pw5-control-1' && k1=0
  u0=$(nhook "$HK" UserPromptSubmit-main); e0=$(ends)
  say zc5 "tracer-pw5-block typed"
  if [ "$k1" -ne 0 ]; then
    sub PW5 5.4 1 "not reached: the mock did not record the control request before the typed prompt"
  elif ! wait_for 30 has_hooks "$HK" UserPromptSubmit-main $((u0 + 1)); then
    sub PW5 5.4 1 "timeout 30 s: no UserPromptSubmit hook for the typed prompt"
  else
    # Bounded wait: a request with the typed prompt within 10 s is a FAIL at once.
    seen=1; wait_for 10 req_has 'tracer-pw5-block' && seen=0
    k2=1; control tracer-pw5-control-2 && wait_for 10 req_has 'tracer-pw5-control-2' && k2=0
    t=1; [ "$seen" -ne 0 ] && [ "$k2" -eq 0 ] && t=0
    sub PW5 5.4 $t "UserPromptSubmit block: control request before the prompt recorded: yes; a request with the typed prompt within 10 s: $(yn $seen); control request after the wait recorded: $(yn $k2); Stop or StopFailure inputs after it: $(($(ends) - e0))"
    pub "$HK/$(hookfile "$HK" UserPromptSubmit-main $((u0 + 1)))" UserPromptSubmit-blocked.json
  fi
  pane_end zc5 "$CCPID"; stop_mock
}

# --- c5l: PW5, one turn of a real lead on the stored login ----------------------------------------
part_c5l() {
  live_run c5l
  HK="$RUN/hooks-l"; RS="$RUN/responses-l.json"; ST="$RUN/settings-l.json"; mkdir -p "$HK"
  jq -n --arg r "$DENY" '[{event: "PreToolUse", tool: "^Agent$", output: {hookSpecificOutput: {hookEventName: "PreToolUse", permissionDecision: "deny", permissionDecisionReason: $r}}}]' >"$RS"
  hook_settings "$HK" "$RS" | jq '.hooks.StopFailure = .hooks.Stop' >"$ST"
  U=$(id -un)
  # The caller's own Claude Code config folder (its CLAUDE_CONFIG_DIR, else the default one), so that the
  # turn uses the same login and state as the caller's session and no other config folder.
  CCD=${CLAUDE_CONFIG_DIR:-$HOME/.claude}
  pj=$(python3 -I -c 'import re, sys; print("-" + re.sub(r"[^A-Za-z0-9]", "-", sys.argv[1]).lstrip("-"))' "$R")
  (cd "$R" && exec env -i PATH="$PATH" HOME="$HOME" CLAUDE_CONFIG_DIR="$CCD" TERM=xterm-256color USER="$U" LOGNAME="$U" SHELL=/bin/sh LANG=C.UTF-8 DISABLE_AUTOUPDATER=1 \
    "$CLAUDE_BIN" -p 'Use the Agent tool one time: start a subagent with the prompt "Reply with the word hi." Then answer in one line.' \
    --output-format json --tools Agent --permission-prompts none --setting-sources project --settings "$ST" \
    --strict-mcp-config --no-session-persistence </dev/null >"$RUN/out-l.json" 2>"$RUN/err-l.txt") &
  LP=$!; LIVE_PIDS="$LIVE_PIDS $LP"
  to=0; wait_for 180 gone "$LP" || { to=1; kill "$LP" 2>/dev/null; nap 2; kill -KILL "$LP" 2>/dev/null; }
  wait "$LP" 2>/dev/null; rc=$?; unpid "$LP"
  pre=$(nhook "$HK" PreToolUse-Agent-main); ss=$(nhook "$HK" 'SubagentStart-.*'); st=$(nhook "$HK" Stop-main)
  turns=$(jq -r '.num_turns // "?"' "$RUN/out-l.json" 2>/dev/null); err=$(jq -r 'if has("is_error") then .is_error else "?" end' "$RUN/out-l.json" 2>/dev/null)
  # The final answer names the deny reason (the word stem "drain" of the reason, any case).
  named=1; jq -e '(.result // "") | ascii_downcase | contains("drain")' "$RUN/out-l.json" >/dev/null 2>&1 && named=0
  t=1; [ "$to" -eq 0 ] && [ "$rc" -eq 0 ] && [ "$err" = false ] && [ "$named" -eq 0 ] && [ "$pre" -ge 1 ] && [ "$ss" -eq 0 ] && [ "$st" -ge 1 ] && t=0
  why=""
  [ "$to" -eq 0 ] || why="; timeout 180 s: the turn did not end"
  [ "$pre" -ge 1 ] || why="$why; the lead made no dispatch (no PreToolUse input for Agent)"
  sub PW5 5.5 $t "real lead on the stored login, one print-mode turn: exit $rc; is_error ${err:-?}; turns ${turns:-?}; denied dispatches $pre; SubagentStart inputs $ss; Stop inputs $st; the answer names the deny reason: $(yn $named); the lead read the reason and ended its turn: $(yn $t)$why"
  # Side effect on the caller's config folder (read only; nothing is changed or removed).
  e1=1; [ -d "$CCD/projects/$pj" ] && e1=0
  e2=1; for _j in "$CCD/.claude.json" "$HOME/.claude.json"; do jq -e --arg r "$R" '(.projects // {}) | has($r)' "$_j" >/dev/null 2>&1 && e2=0; done
  note PW5 5.6 "side effect of the turn on the caller's config folder: a projects folder for the scratch repository: $(yn $e1); a projects entry for it in the config file: $(yn $e2)"
}

# --- c7: PW7 -----------------------------------------------------------------------------------
part_c7() {
  ALL7="PW7:7.1 PW7:7.2 PW7:7.3 PW7:7.4 PW7:7.5"
  live_run c7
  steps pw7-fgs '{"trigger": "tracer-pw7-fgsub", '"$SUBH"', "steps": [{"text": "pw7 fg sub done"}]}'
  steps pw7-bgs '{"trigger": "tracer-pw7-bgsub", '"$SUBH"', "steps": [
    {"tool": "Bash", "input": {"command": "sleep 3"}}, {"text": "pw7 bg sub done"}]}'
  steps pw7-fg '{"trigger": "tracer-pw7-fg go", '"$MAIN"', "steps": [
    {"tool": "Agent", "input": {"subagent_type": "pw-sub", "prompt": "tracer-pw7-fgsub say hi", "description": "pw7 fg", "run_in_background": false}},
    {"text": "pw7 fg done"}]}'
  steps pw7-bg '{"trigger": "tracer-pw7-bg go", '"$MAIN"', "steps": [
    {"tool": "Agent", "input": {"subagent_type": "pw-sub", "prompt": "tracer-pw7-bgsub run one command", "description": "pw7 bg", "run_in_background": true}},
    {"text": "pw7 bg launched"}]}'
  steps pw7-full '{"trigger": "tracer-pw7-full go", '"$MAIN"', "steps": [
    {"text": "pw7 full window", "usage": '"$FULL"', "iterations": true}]}'
  # Last in the rule file: an Esc turn sends no more request, and its tool id is still in the next turn's
  # request; the rules of the later turns come first so that they match that request.
  steps pw7-esc '{"trigger": "tracer-pw7-esc go", '"$MAIN"', "steps": [
    {"tool": "Bash", "input": {"command": "timeout 30 tail -f /dev/null"}}, {"text": "pw7 esc not interrupted"}]}'
  start_mock "$RUN/rules.json"
  cc_settings a '["Bash(timeout 30 tail -f /dev/null)", "Bash(sleep 3)"]'
  cc_pane zc7 "$R" --tools "Bash,Agent" --agents "$AGENTS" --settings "$ST" || { rest "tmux did not start the pane" $ALL7 PW7:7.6; stop_mock; return; }
  if ! cc_ready 60; then
    pane_end zc7 "$CCPID"; rest "timeout 60 s: no SessionStart hook after the start" $ALL7
  else
    nap 3
    t=1; turn zc7 "tracer-pw7-a plain turn" 60 && t=0
    sub PW7 7.1 $t "a turn that ends by itself: Stop or StopFailure within 60 s: $(yn $t)"
    # A turn ended by Esc while a Bash command runs.
    e0=$(ends); n0=$(nhook "$HK" '.*')
    say zc7 "tracer-pw7-esc go"
    if ! wait_for 60 has_hooks "$HK" PreToolUse-Bash-main 1; then
      sub PW7 7.2 1 "timeout 60 s: the Bash command of the Esc turn did not start"
    else
      nap 1; tm send-keys -t "=zc7:" Escape
      t=1; wait_for 15 ends_gt "$e0" && t=0
      ended=$t
      after=$(ls "$HK" | grep -E '^[0-9]+-.*\.json$' | sed -n "$((n0 + 1)),\$p" | sed 's/^[0-9]*-//; s/\.json$//' | tr '\n' ' ')
      cont=$(jq -r 'select(.reply == "pw7-esc-01") | .seq' "$RUN"/rec/[0-9]*.json 2>/dev/null | grep -c .)
      [ "$cont" -eq 0 ] || t=1
      sub PW7 7.2 $t "a turn ended by Esc: Stop or StopFailure within 15 s: $(yn $ended); hook inputs of the turn: ${after:-none}; requests after the interrupted command (0 = the Esc ended the turn): $cont"
      [ "$ended" -ne 0 ] || pub "$HK/$(hookfile "$HK" 'Stop-main|StopFailure-main' $((e0 + 1)))" turn-end-interrupt.json
    fi
    nap 2
    turn zc7 "tracer-pw7-fg go" 90 || note PW7 7.7 "timeout 90 s: no Stop after the turn with the foreground subagent"
    turn zc7 "tracer-pw7-bg go" 60 || note PW7 7.8 "timeout 60 s: no Stop after the turn that started the background subagent"
    wait_for 60 typed_stops 2 || note PW7 7.9 "timeout 60 s: fewer than two SubagentStop inputs of dispatched subagents"
    nap 5
    turn zc7 "tracer-pw7-full go" 60 || note PW7 7.10 "timeout 60 s: no Stop after the full-window turn"
    turn zc7 "tracer-pw7-after turn" 90 || note PW7 7.11 "timeout 90 s: no Stop after the turn that follows the full window"
    nap 3
    r=$(python3 -I "$WA" pw7 "$HK")
    rec_7_345 "$r"
    ls1=$(jv "$r" .last_start none); ls2=$(jv "$r" .last_stop none)
    [ "$ls1" = none ] || pub "$HK/$ls1" SubagentStart-last-dispatched.json
    [ "$ls2" = none ] || pub "$HK/$ls2" SubagentStop-last-dispatched.json
    pane_end zc7 "$CCPID"
  fi
  stop_mock
  # A turn ended by an API error: an upstream of this script that answers every request with HTTP 400.
  live_run c7e
  python3 -I - "$RUN/err-port" >"$RUN/err-server.log" 2>&1 <<'PY' &
import json, os, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def any(self):
        n = int(self.headers.get("content-length") or 0)
        if n:
            self.rfile.read(n)
        d = json.dumps({"type": "error", "error": {"type": "invalid_request_error", "message": "pw7 scripted error"}}).encode()
        self.send_response(400)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(d)))
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(d)

    do_POST = do_GET = do_HEAD = do_PUT = do_DELETE = any

s = ThreadingHTTPServer(("127.0.0.1", 0), H)
with open(sys.argv[1] + ".tmp", "w") as f:
    f.write(str(s.server_address[1]))
os.rename(sys.argv[1] + ".tmp", sys.argv[1])
s.serve_forever()
PY
  EP=$!; LIVE_PIDS="$LIVE_PIDS $EP"
  if ! wait_for 10 test -s "$RUN/err-port"; then
    sub PW7 7.6 1 "timeout 10 s: the error upstream did not start"
  else
    MOCK_URL="http://127.0.0.1:$(cat "$RUN/err-port")"
    cc_settings a '[]'
    if ! cc_pane zc7e "$R" --tools "" --settings "$ST"; then
      sub PW7 7.6 1 "not reached: tmux did not start the pane"
    elif ! cc_ready 60; then
      sub PW7 7.6 1 "timeout 60 s: no SessionStart hook after the start"
    else
      nap 3
      t=1; turn zc7e "tracer-pw7-err turn" 60 && t=0
      evn=$(hookfile "$HK" 'Stop-main|StopFailure-main' 1 | sed 's/^[0-9]*-//; s/-main\.json$//')
      sub PW7 7.6 $t "a turn ended by an API error (HTTP 400): Stop or StopFailure within 60 s: $(yn $t); event: ${evn:-none}"
      [ -z "$evn" ] || pub "$HK/$(hookfile "$HK" 'Stop-main|StopFailure-main' 1)" turn-end-api-error.json
    fi
    pane_end zc7e "$CCPID"
  fi
  kill "$EP" 2>/dev/null; unpid "$EP"
}

# --- oc: PW4, PW9 (OpenCode) ---------------------------------------------------------------------
# oc <tag> <seconds> <opencode args>: one bounded OpenCode command, isolated HOME, plugin log $RUN/plog-<tag>
oc() {
  _t=$1 _s=$2; shift 2
  LIVE_VARS="PW_OC_LOG=$RUN/plog-$_t${OC_BLOCK:+ PW_OC_BLOCK=$OC_BLOCK}"; LIVE_TIMEOUT=$_s
  live_exec "$OPENCODE_BIN" "$@" </dev/null >"$RUN/out-$_t.txt" 2>"$RUN/err-$_t.txt"; _rc=$?
  LIVE_VARS=""; LIVE_TIMEOUT=""
  return $_rc
}
ocq() { jq -r -s "$2" "$RUN/plog-$1/events.jsonl" 2>/dev/null; }   # <tag> <jq program over the list of log lines>
IDLE='select(.hook == "event" and (.type == "session.idle" or (.type == "session.status" and .status == "idle")))'
oc_sid() { ocq "$1" '[.[] | select((.sessionID | type) == "string" and (.sessionID | startswith("ses_")))][0].sessionID // empty'; }
oc_idle() { [ "$(ocq "$1" "[.[] | $IDLE] | length")" -gt 0 ]; }
part_oc() {
  ALLO="PW4:4.1 PW4:4.2 PW4:4.3 PW4:4.4 PW9:9.5 PW9:9.6 PW9:9.7"
  live_run oc
  OCD="$H/.config/opencode"; mkdir -p "$OCD/plugins"
  cat >"$OCD/plugins/zygo-ws-probe.js" <<'JS'
// Probe plugin of live-workspace.sh (node P2): one JSON line per plugin hook call in $PW_OC_LOG/events.jsonl.
import { appendFileSync, mkdirSync } from "node:fs";

const dir = process.env.PW_OC_LOG;
const put = (o) => {
  try {
    mkdirSync(dir, { recursive: true });
    appendFileSync(`${dir}/events.jsonl`, JSON.stringify({ t: Date.now(), ...o }) + "\n");
  } catch {}
};
const sid = (e) => {
  const p = e.properties || {};
  const i = p.info || {};
  return p.sessionID || i.sessionID || (String(e.type).startsWith("session.") ? i.id : undefined) || (p.part || {}).sessionID || null;
};
const status = (e) => {
  const s = (e.properties || {}).status;
  return s && typeof s === "object" ? s.type : s;
};
export const ZygoWsProbe = async () => {
  put({ hook: "init" });
  return {
    event: async ({ event }) => put({ hook: "event", type: event.type, sessionID: sid(event), status: status(event) }),
    "chat.message": async (input) => put({ hook: "chat.message", sessionID: input && input.sessionID }),
    "tool.execute.before": async (input, output) => {
      put({ hook: "tool.execute.before", tool: input.tool, sessionID: input.sessionID, argKeys: Object.keys((output && output.args) || {}).sort() });
      if (process.env.PW_OC_BLOCK && process.env.PW_OC_BLOCK === input.tool) throw new Error("zygo: workspace is draining for down; finish your turn");
    },
    "tool.execute.after": async (input) => put({ hook: "tool.execute.after", tool: input.tool, sessionID: input.sessionID }),
  };
};
JS
  OCX='"extra": {"body": "\"tools\""}'
  steps pw4 '{"trigger": "tracer-pw4-task go", '"$OCX"', "steps": [
    {"tool": "task", "input": {"description": "pw4", "prompt": "tracer-pw4-sub say hi", "subagent_type": "pw-sub"}},
    {"text": "pw4 lead done"}]}'
  steps pw4b '{"trigger": "tracer-pw4-block go", '"$OCX"', "steps": [
    {"tool": "task", "input": {"description": "pw4b", "prompt": "tracer-pw4-bsub say hi", "subagent_type": "pw-sub"}},
    {"text": "pw4 block lead done"}]}'
  start_mock "$RUN/rules.json"
  jq -n --arg u "$MOCK_URL/v1" --arg k "$DUMMY_KEY" '{model: "zygo/lead",
    provider: {zygo: {npm: "@ai-sdk/anthropic", options: {baseURL: $u, apiKey: $k}, models: {lead: {limit: {context: 200000, output: 4000}}}}},
    agent: {"pw-sub": {mode: "subagent", description: "probe subagent", prompt: "Do what the message says.", model: "zygo/lead"}},
    permission: {edit: "deny", bash: "deny", task: "allow"}}' >"$OCD/opencode.json"
  OC_BLOCK=""
  # Run a: one turn; the plugin's view of the session id and of the idle event.
  oc a 180 run "tracer-pw4-a first turn"; rc=$?
  OSID=$(oc_sid a)
  oc la 60 session list --format json
  t=1; [ -n "$OSID" ] && grep -qF "$OSID" "$RUN/out-la.txt" && t=0
  first=$(ocq a '[.[] | select((.sessionID | type) == "string" and (.sessionID | startswith("ses_")))][0] | "\(.hook) \(.type // .tool // "")"')
  sub PW4 4.1 $t "opencode run, exit $rc: the plugin saw a sessionID that opencode session list shows: $(yn $t); first hook that carries it: ${first:-none}"
  if [ -z "$OSID" ]; then
    rest "the plugin saw no sessionID in run a (output: $(said a))" PW4:4.2
  else
    oc b 180 run -s "$OSID" "tracer-pw4-b second turn"; rc=$?
    same=$(ocq b '[.[] | select((.sessionID | type) == "string") | .sessionID] | unique | join(" ")')
    t=1; req_both 'tracer-pw4-b' 'tracer-pw4-a' && [ "$same" = "$OSID" ] && t=0
    sub PW4 4.2 $t "opencode run -s <id>, exit $rc: the request of the second prompt holds the first prompt and the plugin saw only the first id: $(yn $t)"
  fi
  idle=$(ocq a "[.[] | $IDLE | .type] | unique | join(\",\")")
  t=1; oc_idle a && t=0
  sub PW4 4.3 $t "idle event after the turn of run a: $(yn $t); event types: ${idle:-none}"
  ocq a '[.[] | select(.hook == "event") | .type] | unique | .[]' >"$RUN/oc-event-types.txt"
  pub "$RUN/oc-event-types.txt" plugin-event-types.txt "$FXO"
  # Run c: the task tool; the plugin sees it before the subagent's first request.
  oc c 180 run "tracer-pw4-task go"; rc=$?
  r=$(python3 -I - "$RUN/plog-c/events.jsonl" "$RUN/rec" tracer-pw4-sub "$RUN/before-task.json" <<'PY'
import json, os, sys
log, rec, mark, out = sys.argv[1:5]
before = None
if os.path.exists(log):
    for line in open(log):
        e = json.loads(line)
        if e.get("hook") == "tool.execute.before" and e.get("tool") == "task":
            before = e
            break
first = None
for f in sorted(os.listdir(rec)):
    if not f[0].isdigit():
        continue
    b = json.load(open(os.path.join(rec, f))).get("body")
    if isinstance(b, dict) and b.get("messages") and mark in json.dumps(b["messages"][0]):
        first = os.path.getmtime(os.path.join(rec, f)) * 1000
        break
if before:
    json.dump({k: v for k, v in before.items() if k != "t"}, open(out, "w"), indent=2)
print("yes" if before else "no", "yes" if first else "no", "yes" if before and first and before["t"] <= first else "no")
PY
)
  set -- ${r:-no no no}
  t=1; [ "$1" = yes ] && [ "$2" = yes ] && [ "$3" = yes ] && t=0
  sub PW4 4.4 $t "task tool, exit $rc: tool.execute.before saw task: $1; the subagent ran: $2; the hook came before the subagent's first request: $3"
  pub "$RUN/before-task.json" tool.execute.before-task.json "$FXO"
  # Run d (not part of the verdict): a throw in tool.execute.before for task.
  OC_BLOCK=task
  oc d 180 run "tracer-pw4-block go"; rc=$?
  OC_BLOCK=""
  ran=$(python3 -I -c 'import json, os, sys
n = 0
for f in sorted(os.listdir(sys.argv[1])):
    if f[0].isdigit():
        b = json.load(open(os.path.join(sys.argv[1], f))).get("body")
        if isinstance(b, dict) and b.get("messages") and "tracer-pw4-bsub" in json.dumps(b["messages"][0]):
            n += 1
print(n)' "$RUN/rec")
  t2=1; req_both 'toolu_pw4b_00_task' 'workspace is draining for down; finish your turn' && t2=0
  note PW4 4.5 "a throw in tool.execute.before for task, exit $rc: subagent requests $ran; the next request of the lead holds the thrown text: $(yn $t2)"
  # PW9, OpenCode: the TUI in a pane, one turn, SIGTERM to the idle agent, then the session is listed and resumed.
  echo "cd '$R' && exec env -i PATH='$TOOL_PATH' TERM=xterm-256color HOME='$H' XDG_CONFIG_HOME='$H/.config' XDG_DATA_HOME='$H/.local/share' XDG_CACHE_HOME='$H/.cache' XDG_STATE_HOME='$H/.local/state' OPENCODE_DISABLE_AUTOUPDATE=1 PW_OC_LOG='$RUN/plog-t' '$OPENCODE_BIN'" >"$RUN/pane-zo9.sh"
  if ! tm new-session -d -s zo9 -x 200 -y 50 -c "$R" -- /bin/sh "$RUN/pane-zo9.sh"; then
    rest "tmux did not start the pane" PW9:9.5 PW9:9.6 PW9:9.7; pw4_rest "tmux did not start the pane"; stop_mock; return
  fi
  OCPID=$(tm display-message -p -t "=zo9:" '#{pane_pid}'); LIVE_PIDS="$LIVE_PIDS $OCPID"
  if ! wait_for 60 test -s "$RUN/plog-t/events.jsonl"; then
    pane_end zo9 "$OCPID"; rest "timeout 60 s: the OpenCode TUI did not load the plugin" PW9:9.5 PW9:9.6 PW9:9.7; pw4_rest "timeout 60 s: the OpenCode TUI did not load the plugin"; stop_mock; return
  fi
  nap 5
  say zo9 "tracer-pw9-oc first turn"
  if ! wait_for 90 req_has 'tracer-pw9-oc first'; then
    pane_end zo9 "$OCPID"; rest "timeout 90 s: no request with the prompt typed into the OpenCode TUI" PW9:9.5 PW9:9.6 PW9:9.7; pw4_rest "timeout 90 s: no request with the prompt typed into the OpenCode TUI"; stop_mock; return
  fi
  t=1; wait_for 30 oc_idle t && t=0
  TSID=$(oc_sid t)
  note PW4 4.6 "in the TUI the plugin saw a sessionID: $(yn "$([ -n "$TSID" ]; echo $?)"); idle event within 30 s of the request: $(yn $t)"
  # 4.7 (node E2): the TUI's typed prompt gives a session.status busy event of the main session before its first request.
  r=$(python3 -I - "$RUN/plog-t/events.jsonl" "$RUN/rec" 'tracer-pw9-oc first' "$TSID" <<'PY'
import json, os, sys
log, rec, mark, sid = sys.argv[1:5]
busy = None
if os.path.exists(log):
    for line in open(log):
        e = json.loads(line)
        if e.get("hook") == "event" and e.get("type") == "session.status" and e.get("status") == "busy" and sid and e.get("sessionID") == sid:
            busy = e["t"]
            break
first = None
for f in sorted(os.listdir(rec)):
    if not f[0].isdigit():
        continue
    p = os.path.join(rec, f)
    b = json.load(open(p)).get("body")
    if isinstance(b, dict) and mark in json.dumps(b.get("messages", [])):
        first = os.path.getmtime(p) * 1000
        break
print("yes" if busy else "no", "yes" if first else "no", "yes" if busy and first and busy <= first else "no")
PY
)
  set -- ${r:-no no no}
  if [ -z "$TSID" ]; then pnote PW4 4.7 "not run: the plugin saw no session id in the TUI"; else
  pnote PW4 4.7 "TUI typed prompt: a session.status busy event of the main session: $1; a request with the prompt: $2; the busy event came before that request (request file time): $3"; fi
  [ "$t" -eq 0 ] || nap 5
  kill -TERM "$OCPID"
  t=1; wait_for 15 gone "$OCPID" && t=0
  sub PW9 9.5 $t "OpenCode TUI, idle, SIGTERM: process ended within 15 s: $(yn $t)"
  pane_end zo9 "$OCPID"
  oc lt 60 session list --format json
  [ -n "$TSID" ] || TSID=$(jq -r 'map(select((.title // "") | test("tracer-pw9"))) | .[0].id // empty' "$RUN/out-lt.txt" 2>/dev/null)
  t=1; [ -n "$TSID" ] && grep -qF "$TSID" "$RUN/out-lt.txt" && t=0
  sub PW9 9.6 $t "OpenCode: opencode session list shows the session after SIGTERM: $(yn $t)"
  if [ -z "$TSID" ]; then
    rest "no session id of the TUI session" PW9:9.7
  else
    oc r 180 run -s "$TSID" "tracer-pw9-ocb second turn"; rc=$?
    t=1; req_both 'tracer-pw9-ocb' 'tracer-pw9-oc first' && t=0
    sub PW9 9.7 $t "OpenCode: opencode run -s <id>, exit $rc: the conversation saved at SIGTERM was resumed and continued: $(yn $t)"
  fi
  # 4.8 (node E2): a TUI started with --session=<id> resumes that session; the plugin's first session id is <id>.
  if [ -z "$TSID" ]; then
    pnote PW4 4.8 "not run: no session id of the TUI session"; pnote PW4 4.9 "not run: no session id of the TUI session"
  else
    tuienv="cd '$R' && exec env -i PATH='$TOOL_PATH' TERM=xterm-256color HOME='$H' XDG_CONFIG_HOME='$H/.config' XDG_DATA_HOME='$H/.local/share' XDG_CACHE_HOME='$H/.cache' XDG_STATE_HOME='$H/.local/state' OPENCODE_DISABLE_AUTOUPDATE=1"
    echo "$tuienv PW_OC_LOG='$RUN/plog-t8' '$OPENCODE_BIN' --session=$TSID" >"$RUN/pane-zo8.sh"
    t=1 f=no
    if tm new-session -d -s zo8 -x 200 -y 50 -c "$R" -- /bin/sh "$RUN/pane-zo8.sh"; then
      P8=$(tm display-message -p -t "=zo8:" '#{pane_id}'); PID8=$(tm display-message -p -t "$P8" '#{pane_pid}'); LIVE_PIDS="$LIVE_PIDS $PID8"
      if wait_for 60 test -s "$RUN/plog-t8/events.jsonl"; then
        nap 5
        tm send-keys -t "$P8" -l "tracer-pw4-8 resumed turn"; sleep 0.5; tm send-keys -t "$P8" Enter
        wait_for 90 req_both 'tracer-pw4-8' 'tracer-pw9-oc first' && t=0
        [ "$(oc_sid t8)" = "$TSID" ] && f=yes
      fi
      pane_end zo8 "$PID8"
    fi
    pnote PW4 4.8 "TUI started with --session=<id>: the request of a new prompt holds the earlier prompt: $(yn $t); the plugin's first session id is <id>: $f"
    # 4.9 (node E2, AU27 known limit 1): a new TUI, /sessions, Enter: a switch to the most recent session (the 4.8 one).
    echo "$tuienv PW_OC_LOG='$RUN/plog-t9' '$OPENCODE_BIN'" >"$RUN/pane-zo9b.sh"
    sw=no up=no types=""
    if tm new-session -d -s zo9b -x 200 -y 50 -c "$R" -- /bin/sh "$RUN/pane-zo9b.sh"; then
      P9=$(tm display-message -p -t "=zo9b:" '#{pane_id}'); PID9=$(tm display-message -p -t "$P9" '#{pane_pid}'); LIVE_PIDS="$LIVE_PIDS $PID9"
      if wait_for 60 test -s "$RUN/plog-t9/events.jsonl"; then
        nap 5
        tm send-keys -t "$P9" -l "/sessions"; sleep 0.5; tm send-keys -t "$P9" Enter; nap 3; tm send-keys -t "$P9" Enter
        wait_for 15 sh -c 'LC_ALL=C "$0" -S "$1" -f /dev/null capture-pane -p -t "$2" | grep -qF "tracer-pw4-8 resumed turn"' "$TMUX_BIN" "$SOCK" "$P9" && sw=yes
        nap 3
        up=$(ocq t9 "[.[] | select(.hook == \"event\" and .type == \"session.updated\" and .sessionID == \"$TSID\")] | length")
        [ "${up:-0}" -gt 0 ] 2>/dev/null && up=yes || up=no
        types=$(ocq t9 "[.[] | select(.hook == \"event\" and .sessionID == \"$TSID\") | .type] | unique | join(\",\")")
      fi
      pane_end zo9b "$PID9"
    fi
    pnote PW4 4.9 "TUI switch to an existing session (/sessions, Enter): the switch was seen in the pane: $sw; a session.updated event whose info.id is that session: $up; event types with that session id: ${types:-none}"
  fi
  stop_mock
}

# --- run the parts, then one LIVE line per id ------------------------------------------------------
for p in ${*:-$PARTS}; do "part_$p"; done

# finish (workspace_lib.sh): every sub-check of the list needs an ok or FAIL line; the fixture is
# written in the scratch folder; the LIVE line names the path that the fixture has after the run.
CC="Claude Code $LIVE_CC_VER"; OC="OpenCode $LIVE_OC_VER"
finish PW1 "$LIVE_CC_VER" "$FXC/pw1.txt" "$CC" "1.1 1.2 1.3 1.4" "$OUTC/pw1.txt"
finish PW2 "$LIVE_CC_VER" "$FXC/pw2.txt" "$CC" "2.1 2.2 2.3" "$OUTC/pw2.txt"
finish PW3 "$LIVE_CC_VER" "$FXC/pw3.txt" "$CC" "3.1 3.2" "$OUTC/pw3.txt"
finish PW4 "$LIVE_OC_VER" "$FXO/pw4.txt" "$OC" "4.1 4.2 4.3 4.4" "$OUTO/pw4.txt"
finish PW5 "$LIVE_CC_VER" "$FXC/pw5.txt" "$CC" "5.1 5.2 5.3 5.4 5.5" "$OUTC/pw5.txt"
finish PW7 "$LIVE_CC_VER" "$FXC/pw7.txt" "$CC" "7.1 7.2 7.3 7.4 7.5 7.6" "$OUTC/pw7.txt"
finish PW9 "$LIVE_CC_VER + $LIVE_OC_VER" "$FXC/pw9.txt" "$CC + $OC" "9.1 9.2 9.3 9.5 9.6 9.7" "$OUTC/pw9.txt"
finish PW10 "$LIVE_CC_VER" "$FXC/pw10.txt" "$CC" "10.1 10.2" "$OUTC/pw10.txt"
finish PW16 "$LIVE_CC_VER" "$FXC/pw16.txt" "$CC" "16.1 16.2" "$OUTC/pw16.txt"
tm kill-server 2>/dev/null
# A full run that came this far replaces the committed fixtures with the new ones.
if [ $# -eq 0 ]; then
  rm -rf "$OUTC" "$OUTO" && mkdir -p "$OUTC" "$OUTO" && cp "$FXC"/* "$OUTC"/ && cp "$FXO"/* "$OUTO"/ ||
    { echo "live: the fixtures could not be copied to the repository"; exit 1; }
fi
live_done "${OUTC%/claude-code/workspace}"

#!/bin/sh
# Live premises PM1, PM2, PM3 and the first half of acceptance #12 (node ML-16; workspace mail spec
# M§13 #12, M§14): what the pinned Claude Code and OpenCode do with a Stop block, a typed wake line and
# a per-turn mail line, and whether a workspace member sends mail with no prompt about mail.
# Usage: sh scripts/premises/live-mail.sh --pm | --m12
#   --pm: PM1, PM2 and PM3; one LIVE line for each id, exit 0. Fixtures fixtures/claude-code/mail/pm1.txt,
#   pm2.txt, pm3.txt and fixtures/opencode/mail/pm2.txt, pm3.txt.
#   --m12: the first half of M§13 #12 (two members; alpha gets a task that names beta, no prompt about
#   mail); one LIVE M12 line, exit 0. Fixture fixtures/claude-code/mail/m12.txt.
#   Fixtures are written to the scratch folder and copied file by file into the repository at the end of
#   the run, a failed run included. N2_SCRATCH=<folder> (lib.sh): the run folders stay in <folder>/live-<pid>.
# Run by hand only, by the operator, with claude and opencode on PATH (the caller adds the OpenCode
# folder); --m12 also needs ZYGO_BIN (default target/release/zygo).
#
# What this run does to the machine:
# - --pm: a temporary HOME and config folder in the scratch folder, the dummy key and the mock upstream
#   of lib.sh: no login is read, no model request leaves the machine, nothing under the real HOME is
#   written. OpenCode fetches its provider package from the package registry into the temporary HOME.
# - --m12: the real HOME and the stored claude.ai login of Claude Code (as live-detach.sh --zygo), with
#   the caller's CLAUDE_CONFIG_DIR when set. zygo starts a workspace with two interactive passthrough
#   members (alpha and beta; --setting-sources project, --strict-mcp-config) that spend the operator's
#   usage on a few turns: alpha runs the task turn, beta is woken by the mail and runs a turn. The folder
#   trust question is answered yes for one scratch folder, which writes that answer into the caller's
#   Claude Code config. zygo writes its state, session folders and usage rows under the real HOME; the
#   workspace is stopped and removed by zygo down and zygo ws forget at the end. The alpha transcript
#   is read only.
# - Every tmux command uses a server of its own (-S <socket in a private folder>, -f /dev/null); the
#   default tmux server is never called. The server, every agent and helper process and the private
#   folder are removed on every exit.
case "$#:${1:-}" in
  1:--pm) MODE=pm ;;
  1:--m12) MODE=m12 ;;
  *) echo "usage: live-mail.sh --pm | --m12"; exit 2 ;;
esac
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
. "$PREMISES_DIR/workspace_lib.sh"   # clean, sub, note, rest, pw_ticks, jv, finish
live_traps
set +e   # a failed command is a result here; every wait and every test handles its own status
ZP_PRIV="" SOCK="" TMUX_BIN="" BASE="" STARTED=0 C="" SP="" name="" CCD="" U=""
tm() { LC_ALL=C "$TMUX_BIN" -S "$SOCK" -f /dev/null "$@"; }
# zm_end <status>: the workspace of --m12 when one was started, the private server, every process this
# script started, the private folder; then live_end.
zm_end() {
  trap - EXIT INT TERM HUP PIPE
  [ "$STARTED" -eq 0 ] || rclean
  [ -z "$SOCK" ] || tm kill-server 2>/dev/null
  for _p in $LIVE_PIDS; do kill "$_p" 2>/dev/null; done
  [ -z "$LIVE_PIDS" ] || sleep 1
  for _p in $LIVE_PIDS; do kill -KILL "$_p" 2>/dev/null; done
  [ -z "$ZP_PRIV" ] || rm -rf "$ZP_PRIV"
  live_end "$1"
}
trap 'zm_end $?' EXIT; trap 'zm_end 129' HUP; trap 'zm_end 130' INT; trap 'zm_end 141' PIPE; trap 'zm_end 143' TERM

TMUX_BIN=$(command -v tmux) || { echo "live: tmux is not on PATH"; exit 1; }
TVL=$("$TMUX_BIN" -V)
TV=$(printf '%s\n' "$TVL" | sed -n 's/^tmux //p')
ver_ge "${TV%[a-z]}" 3.2 || { echo "live: tmux is ${TV:-unknown}; the minimum is 3.2 (AU7); put tmux 3.2 or later first on PATH"; exit 1; }
for _t in jq python3 perl; do command -v "$_t" >/dev/null || { echo "live: $_t is not on PATH"; exit 1; }; done
pinned_agents
case "$N2_SCRATCH$CLAUDE_BIN$OPENCODE_BIN$TOOL_PATH$HOME$ZYGO_BIN" in
  *"'"* | *" "*) echo "live: a path holds a single quote or a space; this script cannot use it"; exit 1 ;;
esac
if [ "$MODE" = m12 ]; then
  { [ -f "$ZYGO_BIN" ] && [ -x "$ZYGO_BIN" ]; } || { echo "live: ZYGO_BIN is not an executable file; build zygo or set ZYGO_BIN"; exit 1; }
fi
ZP_PRIV=$(mktemp -d "${TMPDIR:-/tmp}/zm.XXXXXX") || exit 1
SOCK="$ZP_PRIV/s"
[ ${#SOCK} -le 100 ] || { echo "live: the socket path is longer than 100 characters; set TMPDIR to a short directory"; exit 1; }
echo "live: Claude Code $LIVE_CC_VER, OpenCode $LIVE_OC_VER, $TVL"

BASE="$N2_SCRATCH/live-$$"
REC="$BASE/ml-rec"; mkdir -p "$REC"
FXC="$BASE/fx-claude-code"; FXO="$BASE/fx-opencode"; mkdir -p "$FXC" "$FXO"
OUTC="$REPO_DIR/fixtures/claude-code/mail"; OUTO="$REPO_DIR/fixtures/opencode/mail"
PW_TICKS=$(pw_ticks)   # polls per second of a wait bound (workspace_lib.sh: 10 unless PW_TEST=1)
EXTRA_ENV="DISABLE_AUTOUPDATER=1"
MAIN='"no_header": ["x-claude-code-agent-id"], "extra": {"body": "\"tools\": \\[\\{"}'
WAKE="zygo: you have mail; read it with mail_read"   # M§7.3, M-C5
M12_TASK='Tell beta that the build is green.'

# --- helpers (bodies of live-workspace.sh; trust_line and cc_trust take a tmux target) ---------
# wait_for <seconds> <command...>: poll every 0.1 s; 0 when the command succeeds in time, else 1.
wait_for() { _w=$(($1 * PW_TICKS)); shift; while ! "$@" >/dev/null 2>&1; do _w=$((_w - 1)); [ "$_w" -gt 0 ] || return 1; sleep 0.1; done; }
nap() { _n=$(($1 * PW_TICKS)); while [ "$_n" -gt 0 ]; do sleep 0.1; _n=$((_n - 1)); done; }
gone() { ! kill -0 "$1" 2>/dev/null; }
unpid() { LIVE_PIDS=$(printf '%s\n' $LIVE_PIDS | grep -vx "$1" | tr '\n' ' '); }
yn() { if [ "$1" -eq 0 ]; then echo yes; else echo no; fi; }
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
ends() { nhook "$HK" 'Stop-main|StopFailure-main'; }
ends_gt() { [ "$(ends)" -gt "$1" ]; }
# cc_pane <session> <dir> <claude args>: Claude Code on the mock in a pane of the private server; CCPID = its process id
CCPID="" CCS=""
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
trust_line() { tm capture-pane -p -t "$1" 2>/dev/null | grep -F "$TRUST_YES" | tail -n 1; }   # <target>
# cc_trust <target> (=<session>: or a pane id): one key step on the folder trust question. Pointer not on
# the accept line: Down. Pointer on it in two readings 0.5 s apart: Enter. No question in the pane: nothing.
cc_trust() {
  case "$(trust_line "$1")" in
    "") ;;
    *"$PTR"*) sleep 0.5; case "$(trust_line "$1")" in *"$PTR"*) tm send-keys -t "$1" Enter; sleep 1 ;; esac ;;
    *) tm send-keys -t "$1" Down; sleep 1 ;;
  esac
}
# cc_ready <seconds>: the first SessionStart input of the main loop of the pane of cc_pane is in $HK (polls
# every 0.5 s; answers the folder trust question with yes)
cc_ready() {
  _c=$(($1 * PW_TICKS / 5))
  until has_hooks "$HK" SessionStart-main 1; do
    cc_trust "=$CCS:"; _c=$((_c - 1)); [ "$_c" -gt 0 ] || return 1; sleep 0.5
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
req_has() { [ -n "$(body_has "$1" "$RUN"/rec/[0-9]*.json 2>/dev/null)" ]; }               # <regex>: a recorded request holds it
# req_both <regex a> <regex b>: 0 when one recorded request holds both
req_both() { _f=$(body_has "$1" "$RUN"/rec/[0-9]*.json 2>/dev/null); [ -n "$_f" ] && [ -n "$(body_has "$2" $_f)" ]; }
# sha <hook input file>: its stop_hook_active value as text (false stays false; hval's // would make it absent)
sha() { jq -r '.stop_hook_active | tostring' "$1" 2>/dev/null || echo unreadable; }

# --- PM1: a Stop block runs one more turn, and that turn can call an MCP tool ------------------
part_pm1() {
  ALL1="PM1:1.1 PM1:1.2 PM1:1.3"
  live_run pm1
  steps pm1b '{"trigger": "new messages; read them with mail_read", '"$MAIN"', "steps": [{"tool": "mcp__probe__probe_tool", "input": {}}, {"text": "tracer-pm1 after block done"}]}'
  start_mock "$RUN/rules.json"
  printf '{"mcpServers":{"probe":{"command":"%s","args":["-I","%s/mcp_probe.py"]}}}\n' "$(command -v python3)" "$PREMISES_DIR" >"$RUN/mcp.json"
  cc_settings b '["mcp__probe__probe_tool"]'
  echo '[{"event": "Stop", "who": "main", "times": 1, "output": {"decision": "block", "reason": "zygo: 1 new messages; read them with mail_read"}}]' >"$RS"
  cc_pane zm1 "$R" --settings "$ST" --mcp-config "$RUN/mcp.json" --strict-mcp-config || { rest "tmux did not start the pane" $ALL1; stop_mock; return; }
  if ! cc_ready 60; then
    pane_end zm1 "$CCPID"; rest "timeout 60 s: no SessionStart hook after the start" $ALL1; stop_mock; return
  fi
  nap 3
  say zm1 "tracer-pm1 go"
  if ! wait_for 60 has_hooks "$HK" Stop-main 1; then
    pane_end zm1 "$CCPID"; rest "timeout 60 s: no Stop hook after the first prompt" $ALL1; stop_mock; return
  fi
  f="$HK/$(hookfile "$HK" Stop-main 1)"; o="${f%.json}.out"
  wait_for 5 test -s "$o"
  s1=$(sha "$f")
  b=1; grep -qF '"decision": "block"' "$o" 2>/dev/null && b=0
  t=1; [ "$s1" = false ] && [ "$b" -eq 0 ] && t=0
  sub PM1 1.1 $t "first Stop input of the main loop: stop_hook_active $s1; the hook answered decision block: $(yn $b)"
  t=1; wait_for 60 req_has 'new messages; read them with mail_read' && t=0
  sub PM1 1.2 $t "a request after the block holds the block reason (Claude Code ran one more turn): $(yn $t)"
  p=1; wait_for 60 has_hooks "$HK" 'PostToolUse-mcp__probe__probe_tool-main' 1 && p=0
  r=1; [ "$p" -eq 0 ] && wait_for 60 req_has 'tracer-mcp-probe-result' && r=0
  t=1; [ "$p" -eq 0 ] && [ "$r" -eq 0 ] && t=0
  sub PM1 1.3 $t "that turn called the MCP tool: PostToolUse input for mcp__probe__probe_tool: $(yn $p); a request holds the tool result: $(yn $r)"
  if wait_for 60 has_hooks "$HK" Stop-main 2; then
    note PM1 1.4 "second Stop input of the main loop: stop_hook_active $(sha "$HK/$(hookfile "$HK" Stop-main 2)")"
  else
    note PM1 1.4 "timeout 60 s: no second Stop input of the main loop"
  fi
  pane_end zm1 "$CCPID"; stop_mock
}

# --- PM2, PM3 (Claude Code): the typed wake line, the SessionStart and UserPromptSubmit lines ------
part_cc23() {
  ALLC="PM2:2.1 PM2:2.2 PM3:3.1 PM3:3.2"
  live_run cc23; start_mock
  cc_settings p '[]'
  echo '[{"event": "SessionStart", "who": "main", "output": {"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": "zygo: 2 new messages; read them with mail_read"}}}, {"event": "UserPromptSubmit", "who": "main", "output": {"hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "additionalContext": "zygo: 3 new messages; read them with mail_read"}}}]' >"$RS"
  cc_pane zm2 "$R" --tools "" --settings "$ST" || { rest "tmux did not start the pane" $ALLC; stop_mock; return; }
  if ! cc_ready 60; then
    pane_end zm2 "$CCPID"; rest "timeout 60 s: no SessionStart hook after the start" $ALLC; stop_mock; return
  fi
  nap 3
  turn zm2 "$WAKE" 60 || note PM2 2.4 "timeout 60 s: no Stop hook after the wake line"
  f=$(hookfile "$HK" UserPromptSubmit-main 1)
  t=1; [ -n "$f" ] && jq -e --arg w "$WAKE" '.prompt == $w' "$HK/$f" >/dev/null 2>&1 && t=0
  sub PM2 2.1 $t "Claude Code, idle: the wake line typed with send-keys -l, then Enter: UserPromptSubmit input seen: $(yn "$([ -n "$f" ]; echo $?)"); its prompt is the wake line byte for byte: $(yn $t)"
  t=1; req_has "$WAKE" && t=0
  sub PM2 2.2 $t "a recorded request holds the wake line: $(yn $t)"
  t=1; req_both "$WAKE" 'zygo: 2 new messages' && t=0
  sub PM3 3.1 $t "one request holds the wake line and the SessionStart additionalContext line: $(yn $t)"
  t=1; req_both "$WAKE" 'zygo: 3 new messages' && t=0
  sub PM3 3.2 $t "one request holds the wake line and the UserPromptSubmit additionalContext line: $(yn $t)"
  pane_end zm2 "$CCPID"; stop_mock
}

# --- PM2, PM3 (OpenCode): the typed wake line, the per-request system line ----------------------
part_oc23() {
  ALLO="PM2:2.3 PM3:3.3"
  live_run oc23
  OCD="$H/.config/opencode"; mkdir -p "$OCD/plugins"
  cat >"$OCD/plugins/zygo-mail-probe.js" <<'JS'
// Probe plugin of live-mail.sh (node ML-16): one JSON line per hook call in $PM_OC_LOG/events.jsonl;
// pushes the mail line of M§7.2 into the system prompt of every request.
import { appendFileSync, mkdirSync } from "node:fs";

const dir = process.env.PM_OC_LOG;
const put = (o) => {
  try {
    mkdirSync(dir, { recursive: true });
    appendFileSync(`${dir}/events.jsonl`, JSON.stringify({ t: Date.now(), ...o }) + "\n");
  } catch {}
};
export const ZygoMailProbe = async () => {
  put({ hook: "init" });
  return {
    "experimental.chat.system.transform": async (input, output) => {
      output.system.push("zygo: 4 new messages; read them with mail_read");
      put({ hook: "system.transform" });
    },
  };
};
JS
  start_mock
  jq -n --arg u "$MOCK_URL/v1" --arg k "$DUMMY_KEY" '{model: "zygo/lead",
    provider: {zygo: {npm: "@ai-sdk/anthropic", options: {baseURL: $u, apiKey: $k}, models: {lead: {limit: {context: 200000, output: 4000}}}}},
    permission: {edit: "deny", bash: "deny", task: "deny"}}' >"$OCD/opencode.json"
  echo "cd '$R' && exec env -i PATH='$TOOL_PATH' TERM=xterm-256color HOME='$H' XDG_CONFIG_HOME='$H/.config' XDG_DATA_HOME='$H/.local/share' XDG_CACHE_HOME='$H/.cache' XDG_STATE_HOME='$H/.local/state' OPENCODE_DISABLE_AUTOUPDATE=1 PM_OC_LOG='$RUN/plog' '$OPENCODE_BIN'" >"$RUN/pane-zo2.sh"
  if ! tm new-session -d -s zo2 -x 200 -y 50 -c "$R" -- /bin/sh "$RUN/pane-zo2.sh"; then
    rest "tmux did not start the pane" $ALLO; stop_mock; return
  fi
  OCPID=$(tm display-message -p -t "=zo2:" '#{pane_pid}'); LIVE_PIDS="$LIVE_PIDS $OCPID"
  if ! wait_for 60 test -s "$RUN/plog/events.jsonl"; then
    pane_end zo2 "$OCPID"; rest "timeout 60 s: the OpenCode TUI did not load the plugin" $ALLO; stop_mock; return
  fi
  nap 5
  say zo2 "$WAKE"
  t=1; wait_for 90 req_has "$WAKE" && t=0
  sub PM2 2.3 $t "OpenCode TUI, idle: the wake line typed with send-keys -l, then Enter: a request holds it within 90 s: $(yn $t)"
  t=1; req_both "$WAKE" 'zygo: 4 new messages' && t=0
  sub PM3 3.3 $t "one request holds the wake line and the line pushed in experimental.chat.system.transform: $(yn $t)"
  pane_end zo2 "$OCPID"; stop_mock
}

# --- M12: a member mails another one with no prompt about mail (M§13 #12, first half) ------------
renv() { env -i PATH="$PATH" HOME="$HOME" TERM=xterm-256color USER="$U" LOGNAME="$U" SHELL=/bin/sh LANG=C.UTF-8 \
  DISABLE_AUTOUPDATER=1 ZYGO_NO_UPDATE_CHECK=1 ZYGO_CONFIG="$RUN/zygo.toml" ${CCD:+CLAUDE_CONFIG_DIR="$CCD"} "$@"; }
wsphase() { (cd "$C" && renv "$ZYGO_BIN" workspace) 2>/dev/null | sed -n '1s/^\(zygo: \)\{0,1\}workspace [^:]*: //p'; }
# rclean: stop the workspace if it still runs, forget it, stop the private server.
rclean() {
  STARTED=0
  [ "$(wsphase)" = down ] || (cd "$C" && renv TMUX="$SOCK,$SP,0" "$ZYGO_BIN" down --now --wait -- "$name") >>"$RUN/clean.txt" 2>&1
  renv "$ZYGO_BIN" ws forget -- "$name" >>"$RUN/clean.txt" 2>&1
  tm kill-server 2>/dev/null
}
mval() { jq -r --arg k "$1" ".members[\$k].$2 // empty" "$WJ" 2>/dev/null; }   # <pane key> <field>
m12_ready() { [ -n "$(mval "$KA" agent_session)" ] && [ -n "$(mval "$KB" agent_session)" ]; }
m12_busy() { [ "$(jq -r '.busy | tostring' "$AJ" 2>/dev/null)" = "$1" ]; }       # <true|false>
part_m12() {
  ALLM="M12:12.1 M12:12.2 M12:12.3"
  U=$(id -un)
  CCD=$(printenv CLAUDE_CONFIG_DIR || true)
  live_run m12
  C="$RUN/zm12" name=zm12
  mkdir -p "$C"
  git -C "$C" init -q && git -C "$C" -c user.name=t -c user.email=t@t commit -q --allow-empty -m init
  cat >"$RUN/zygo.toml" <<'T'
[loadout.m12]
[[loadout.m12.window]]
[[loadout.m12.window.pane]]
kind = "claude-code"
name = "alpha"
passthrough = true
args = ["--setting-sources", "project", "--strict-mcp-config"]
[[loadout.m12.window.pane]]
kind = "claude-code"
name = "beta"
passthrough = true
args = ["--setting-sources", "project", "--strict-mcp-config"]
T
  renv "$TMUX_BIN" -S "$SOCK" -f /dev/null new-session -d -s hold -x 200 -y 50 /bin/sleep 3600 || { rest "tmux did not start the hold session" $ALLM; return; }
  SP=$(tm display-message -p '#{pid}')
  STARTED=1
  (cd "$C" && renv TMUX="$SOCK,$SP,0" "$ZYGO_BIN" up --loadout m12 --no-attach) >"$RUN/up.txt" 2>&1; urc=$?
  if [ "$urc" -ne 0 ]; then rest "zygo up exit $urc" $ALLM; rclean; return; fi
  WS=$(tm show-environment -t "=zygo-$name:" ZYGO_WORKSPACE 2>/dev/null | sed -n 's/^ZYGO_WORKSPACE=//p')
  WJ="$HOME/.local/state/zygo/workspaces/$WS/workspace.json"
  KA=$(jq -r '[.panes[] | select(.kind == "claude-code" and .name == "alpha") | .key] | if length == 1 then .[0] else empty end' "$WJ" 2>/dev/null)
  KB=$(jq -r '[.panes[] | select(.kind == "claude-code" and .name == "beta") | .key] | if length == 1 then .[0] else empty end' "$WJ" 2>/dev/null)
  if [ -z "$WS" ] || [ -z "$KA" ] || [ -z "$KB" ]; then
    rest "zygo up exit 0, but workspace.json has no claude-code pane alpha and beta (workspace id found: $(yn "$([ -n "$WS" ]; echo $?)"))" $ALLM; rclean; return
  fi
  PANES=$(tm list-panes -s -t "=zygo-$name:" -F '#{pane_id} #{@zygo-pane}')
  PA=$(printf '%s\n' "$PANES" | awk -v k="$KA" '$2 == k { print $1; exit }')
  PB=$(printf '%s\n' "$PANES" | awk -v k="$KB" '$2 == k { print $1; exit }')
  if [ -z "$PA" ] || [ -z "$PB" ]; then rest "no tmux pane with the pane key of alpha or beta" $ALLM; rclean; return; fi
  _c=$((120 * PW_TICKS / 5))
  until m12_ready; do
    cc_trust "$PA"; cc_trust "$PB"; _c=$((_c - 1)); [ "$_c" -gt 0 ] || break; sleep 0.5
  done
  if ! m12_ready; then rest "timeout 120 s: alpha and beta do not both have an agent session" $ALLM; rclean; return; fi
  sub M12 12.1 0 "zygo up exit 0; workspace.json has the claude-code panes alpha and beta; both members have an agent session within 120 s"
  nap 5
  tm send-keys -t "$PA" -l "$M12_TASK"; sleep 0.5; tm send-keys -t "$PA" Enter
  AJ="$HOME/.cache/zygo/sessions/$(mval "$KA" zygo_session)/activity.json"
  b1=1; wait_for 60 m12_busy true && b1=0
  b2=1; [ "$b1" -eq 0 ] && wait_for 300 m12_busy false && b2=0
  AS=$(mval "$KA" agent_session)
  CDIR=$(mval "$KA" config_dir); [ -n "$CDIR" ] || CDIR=${CCD:-$HOME/.claude}
  TR=$(find "$CDIR/projects" -name "$AS.jsonl" 2>/dev/null | sed -n 1p)
  # The task turn of alpha's transcript (read only): tool names, mail_send recipients, poll calls. No subject or body (M-C2).
  r=$(python3 -I - "$TR" "$M12_TASK" <<'PY'
import json, sys
path, task = sys.argv[1:3]
out = {"found": False, "tools": [], "send_to": [], "polls": 0}
try:
    lines = open(path).read().splitlines() if path else []
except OSError:
    lines = []

def items(c):
    return [i for i in c if isinstance(i, dict)] if isinstance(c, list) else []

def holds_task(c):
    if isinstance(c, str):
        return task in c
    return any(i.get("type") == "text" and task in (i.get("text") or "") for i in items(c))

inside = False
for line in lines:
    try:
        e = json.loads(line)
    except ValueError:
        continue
    if not isinstance(e, dict):
        continue
    c = (e.get("message") or {}).get("content")
    if e.get("type") == "user":
        if not inside:
            if holds_task(c):
                inside = True
                out["found"] = True
        elif not any(i.get("type") == "tool_result" for i in items(c)):
            break  # the next prompt or Stop-hook feedback ends the task turn
        continue
    if inside and e.get("type") == "assistant":
        for i in items(c):
            if i.get("type") != "tool_use":
                continue
            n = i.get("name") or ""
            inp = i.get("input") or {}
            out["tools"].append(n)
            if n == "mcp__zygo__mail_send":
                to = inp.get("to") if isinstance(inp, dict) else None
                out["send_to"].extend(str(x) for x in (to if isinstance(to, list) else [to] if to is not None else []))
            if n in ("mcp__zygo__mail_read", "mcp__zygo__mail_list") or "zygo mail watch" in json.dumps(inp):
                out["polls"] += 1
print(json.dumps(out))
PY
)
  found=$(jv "$r" .found false); to=$(jv "$r" '.send_to | join(" ")' ""); polls=$(jv "$r" .polls "?")
  t=1; [ "$found" = true ] && printf '%s\n' $to | grep -qx beta && t=0
  sub M12 12.2 $t "alpha's task turn found in its transcript: $found; mail_send recipients: ${to:-none}; beta among them: $(yn $t)"
  t=1; [ "$b1" -eq 0 ] && [ "$b2" -eq 0 ] && [ "$polls" = 0 ] && t=0
  sub M12 12.3 $t "alpha busy within 60 s: $(yn $b1); idle again within 300 s: $(yn $b2); mail_read, mail_list or zygo mail watch calls in the turn: $polls"
  note M12 12.4 "tool calls of the task turn, in order: $(jv "$r" '.tools | join(", ")' none)"
  rclean
}

# --- run, then one LIVE line per id; fixtures copied file by file (the two modes share the folders) ---
cpfx() { mkdir -p "$1" && cp "$2" "$1"/ || { echo "live: the fixtures could not be copied to the repository"; exit 1; }; }
CC="Claude Code $LIVE_CC_VER"; OC="OpenCode $LIVE_OC_VER"
if [ "$MODE" = pm ]; then
  part_pm1; part_cc23; part_oc23
  finish PM1 "$LIVE_CC_VER" "$FXC/pm1.txt" "$CC" "1.1 1.2 1.3" "$OUTC/pm1.txt"
  finish PM2 "$LIVE_CC_VER + $LIVE_OC_VER" "$FXC/pm2.txt" "$CC + $OC" "2.1 2.2 2.3" "$OUTC/pm2.txt"
  finish PM3 "$LIVE_CC_VER + $LIVE_OC_VER" "$FXC/pm3.txt" "$CC + $OC" "3.1 3.2 3.3" "$OUTC/pm3.txt"
  # oc_fx <id> <file> <sub id regex>: the OpenCode sub-check of the record as the OpenCode fixture
  oc_fx() { { echo "$OC"; grep -E "^$3 " "$REC/$1.txt"; } >"$REC/$1-oc.out"; scrub "$REC/$1-oc.out" >"$FXO/$2"; }
  oc_fx PM2 pm2.txt '2[.]3'; oc_fx PM3 pm3.txt '3[.]3'
  tm kill-server 2>/dev/null
  for f in pm1.txt pm2.txt pm3.txt; do cpfx "$OUTC" "$FXC/$f"; done
  for f in pm2.txt pm3.txt; do cpfx "$OUTO" "$FXO/$f"; done
else
  part_m12
  ZV=$(renv "$ZYGO_BIN" --version 2>/dev/null | sed -n 1p)
  finish M12 "$LIVE_CC_VER" "$FXC/m12.txt" "$CC, zygo ${ZV:-?}, $TVL" "12.1 12.2 12.3" "$OUTC/m12.txt"
  tm kill-server 2>/dev/null
  cpfx "$OUTC" "$FXC/m12.txt"
fi
live_done "$REPO_DIR/fixtures"

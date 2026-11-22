#!/bin/sh
# Offline tests of live-workspace.sh's record helpers (workspace_lib.sh) and analyses
# (workspace_analyze.py), on stub data; no agent, no tmux, no network. Usage: sh scripts/premises/workspace_test.sh
set -u
D=$(cd "$(dirname "$0")" && pwd)
REPO_DIR=$(cd "$D/../.." && pwd)
T=$(mktemp -d) || exit 1
trap 'rm -rf "$T"' EXIT
SOCK="" N2_SCRATCH=""
scrub() { cat "$1"; }                         # stub of lib.sh: no run folder here
result() { echo "LIVE $1 $2 (agent ${LIVE_VER:-?}): $3"; }   # stub of live.sh
. "$D/workspace_lib.sh"
WA="$D/workspace_analyze.py"
n=0 bad=0
ok() { n=$((n + 1)); }
no() { n=$((n + 1)); bad=$((bad + 1)); echo "FAIL: $*"; }
check() { _name=$1; shift; if "$@"; then ok; else no "$_name"; fi; }
has() { case "$1" in *"$2"*) return 0 ;; esac; return 1; }
fresh() { REC="$T/rec-$1"; rm -rf "$REC"; mkdir -p "$REC"; }
# hook <dir> <n> <name> <json>: one saved hook input
hook() { mkdir -p "$1"; printf '%s\n' "$4" >"$1/$(printf '%04d' "$2")-$3.json"; }

# --- sub, note, rest, finish ---------------------------------------------------------------------
fresh a
out=$(finish PW1 v "$T/a.txt" head "1.1 1.2")
check "finish: no lines is failed" has "$out" "LIVE PW1 failed"
check "finish: no lines counts both sub-checks" has "$out" "0 of 2 sub-checks passed; failed: 1.1 1.2"
check "finish: the missing text" has "$(cat "$T/a.txt")" "1.1 FAIL not reached: no result line"
fresh b
sub PW1 1.1 0 "x"; sub PW1 1.2 0 "y"; note PW1 1.3 "z"
out=$(finish PW1 v "$T/b.txt" head "1.1 1.2")
check "finish: complete is verified" has "$out" "LIVE PW1 verified (agent v): 2 of 2 sub-checks passed"
check "finish: fixture has the head line" [ "$(sed -n 1p "$T/b.txt")" = head ]
check "finish: a note is kept and not counted" has "$(cat "$T/b.txt")" "1.3 note z"
fresh c
sub PW1 1.1 0 "x"
out=$(finish PW1 v "$T/c.txt" head "1.1 1.2")
check "finish: one missing sub-check is failed" has "$out" "1 of 2 sub-checks passed; failed: 1.2; first: not reached: no result line"
fresh d
sub PW1 1.1 0 "x"; sub PW1 1.2 1 "bad thing"
out=$(finish PW1 v "$T/d.txt" head "1.1 1.2")
check "finish: a FAIL line is failed" has "$out" "1 of 2 sub-checks passed; failed: 1.2; first: bad thing"
fresh e
rest "why" PW1:1.1 PW1:1.2
out=$(finish PW1 v "$T/e.txt" head "1.1 1.2")
check "rest: each sub-check FAIL with the reason" has "$out" "0 of 2 sub-checks passed; failed: 1.1 1.2; first: not reached: why"
fresh f
sub PW1 1.10 0 "x"
out=$(finish PW1 v "$T/f.txt" head "1.1")
check "finish: 1.10 does not count as 1.1" has "$out" "failed: 1.1;"
out=$(finish PW1 v "$T/g.txt" head "1.1" "$REPO_DIR/fixtures/x/pw1.txt")
check "finish: prints the path relative to the repository" has "$out" "; fixtures/x/pw1.txt"
SOCK=/s/sock N2_SCRATCH=/scr
check "clean: socket and scratch" [ "$(clean "a /s/sock b /scr/c
d")" = "a SOCKET b SCRATCH/c d" ]
SOCK="" N2_SCRATCH=""

# --- pw_ticks --------------------------------------------------------------------------------------
check "pw_ticks: default" [ "$(PW_TICKS=3 pw_ticks)" = 10 ]
check "pw_ticks: test mode" [ "$(PW_TEST=1 PW_TICKS=3 pw_ticks)" = 3 ]
check "pw_ticks: zero ignored" [ "$(PW_TEST=1 PW_TICKS=0 pw_ticks)" = 10 ]
check "pw_ticks: text ignored" [ "$(PW_TEST=1 PW_TICKS='1;x' pw_ticks)" = 10 ]
check "pw_ticks: negative ignored" [ "$(PW_TEST=1 PW_TICKS=-2 pw_ticks)" = 10 ]

# --- 2.3: SessionStart inside the subagent -----------------------------------------------------------
fresh p2e
f=$(rec_2_3 "$(python3 -I "$WA" pw2-sub "$T/none")")
out=$(finish PW2 v "$T/p2e.txt" head "2.3")
check "2.3: empty data is failed" has "$out" "LIVE PW2 failed"
check "2.3: empty data publishes nothing" [ "$f" = none ]
H2="$T/h2"
hook "$H2" 1 SessionStart-main '{"hook_event_name": "SessionStart", "source": "startup"}'
hook "$H2" 2 SubagentStart-sub '{"hook_event_name": "SubagentStart", "agent_id": "a1", "agent_type": "pw-sub"}'
hook "$H2" 3 SessionStart-sub '{"hook_event_name": "SessionStart", "source": "compact", "agent_id": "a1"}'
hook "$H2" 4 SubagentStop-sub '{"hook_event_name": "SubagentStop", "agent_id": "a1", "agent_type": "pw-sub"}'
fresh p2c
f=$(rec_2_3 "$(python3 -I "$WA" pw2-sub "$H2")")
out=$(finish PW2 v "$T/p2c.txt" head "2.3")
check "2.3: complete data is verified" has "$out" "LIVE PW2 verified"
check "2.3: the file to publish" [ "$f" = 0003-SessionStart-sub.json ]
hook "$H2" 3 SessionStart-sub '{"hook_event_name": "SessionStart", "source": "compact"}'
fresh p2b
f=$(rec_2_3 "$(python3 -I "$WA" pw2-sub "$H2")")
check "2.3: a SessionStart without agent_id is failed" has "$(finish PW2 v "$T/p2b.txt" head "2.3")" "LIVE PW2 failed"

# --- 7.3-7.5: turn ends and subagent pairs -------------------------------------------------------------
fresh p7e
rec_7_345 "$(python3 -I "$WA" pw7 "$T/none")"
check "7.3-7.5: empty data is failed" has "$(finish PW7 v "$T/p7e.txt" head "7.3 7.4 7.5")" "0 of 3 sub-checks passed; failed: 7.3 7.4 7.5"
H7="$T/h7"; i=0
for k in 1 2 3 4 5 6; do
  i=$((i + 1)); hook "$H7" $i UserPromptSubmit-main "{\"hook_event_name\": \"UserPromptSubmit\", \"prompt\": \"p$k\"}"
  if [ $k = 2 ]; then
    i=$((i + 1)); hook "$H7" $i SubagentStart-sub '{"hook_event_name": "SubagentStart", "agent_id": "f", "agent_type": "pw-sub"}'
    i=$((i + 1)); hook "$H7" $i SubagentStop-sub '{"hook_event_name": "SubagentStop", "agent_id": "f", "agent_type": "pw-sub"}'
  fi
  if [ $k = 3 ]; then
    i=$((i + 1)); hook "$H7" $i SubagentStart-sub '{"hook_event_name": "SubagentStart", "agent_id": "b", "agent_type": "pw-sub"}'
  fi
  if [ $k = 5 ]; then
    i=$((i + 1)); hook "$H7" $i PreCompact-main '{"hook_event_name": "PreCompact"}'
    i=$((i + 1)); hook "$H7" $i SubagentStart-sub '{"hook_event_name": "SubagentStart", "agent_id": "c", "agent_type": ""}'
    i=$((i + 1)); hook "$H7" $i SubagentStop-sub '{"hook_event_name": "SubagentStop", "agent_id": "c", "agent_type": ""}'
    i=$((i + 1)); hook "$H7" $i PostCompact-main '{"hook_event_name": "PostCompact"}'
  fi
  i=$((i + 1)); hook "$H7" $i Stop-main '{"hook_event_name": "Stop"}'
done
i=$((i + 1)); hook "$H7" $i SubagentStop-sub '{"hook_event_name": "SubagentStop", "agent_id": "b", "agent_type": "pw-sub"}'
r=$(python3 -I "$WA" pw7 "$H7")
fresh p7c
rec_7_345 "$r"
out=$(finish PW7 v "$T/p7c.txt" head "7.3 7.4 7.5")
check "7.3-7.5: complete data is verified" has "$out" "LIVE PW7 verified (agent v): 3 of 3 sub-checks passed"
check "7.5: the compaction agent is counted" has "$(cat "$T/p7c.txt")" "ids seen 1, of which 1 sent SubagentStop between PreCompact and PostCompact"
check "7.4: the last dispatched agent's stop file" [ "$(jv "$r" .last_stop none)" = "$(printf '%04d' $i)-SubagentStop-sub.json" ]
# An Esc turn (no Stop) and a stop-only internal agent.
hook "$H7" 90 UserPromptSubmit-main '{"hook_event_name": "UserPromptSubmit", "prompt": "esc turn"}'
hook "$H7" 91 SubagentStop-sub '{"hook_event_name": "SubagentStop", "agent_id": "s", "agent_type": ""}'
hook "$H7" 92 UserPromptSubmit-main '{"hook_event_name": "UserPromptSubmit", "prompt": "next"}'
hook "$H7" 93 Stop-main '{"hook_event_name": "Stop"}'
fresh p7b
rec_7_345 "$(python3 -I "$WA" pw7 "$H7")"
out=$(finish PW7 v "$T/p7b.txt" head "7.3 7.4 7.5")
check "7.3, 7.5: an open turn and a stop-only agent are failed" has "$out" "1 of 3 sub-checks passed; failed: 7.3 7.5"
check "7.3: names the prompt of the open turn" has "$(cat "$T/p7b.txt")" "before the next prompt: 1 (prompts: esc turn)"
check "7.5: names the stop-only kind" has "$(cat "$T/p7b.txt")" "ids without both SubagentStart and SubagentStop: 1 (stop-only)"

# --- 5.2: the lead's next request --------------------------------------------------------------------
RQ="$T/rq"; mkdir -p "$RQ"
check "lead-next: empty folder gives nothing" [ -z "$(python3 -I "$WA" lead-next "$T/none" toolu_x)" ]
TR='{"type": "tool_result", "tool_use_id": "toolu_x", "is_error": true, "content": "REASON"}'
printf '%s\n' '{"seq": 1, "body": {"messages": [{"role": "user", "content": "go"}]}}' >"$RQ/0001.json"
printf '%s\n' "{\"seq\": 2, \"body\": {\"messages\": [{\"role\": \"user\", \"content\": \"go\"}, {\"role\": \"assistant\", \"content\": []}, {\"role\": \"user\", \"content\": [$TR]}, {\"role\": \"assistant\", \"content\": []}, {\"role\": \"user\", \"content\": \"[SUGGESTION MODE: x]\"}]}}" >"$RQ/0002.json"
check "lead-next: the suggestion request is not taken" [ -z "$(python3 -I "$WA" lead-next "$RQ" toolu_x)" ]
printf '%s\n' "{\"seq\": 3, \"body\": {\"messages\": [{\"role\": \"user\", \"content\": \"go\"}, {\"role\": \"assistant\", \"content\": []}, {\"role\": \"user\", \"content\": [$TR]}]}}" >"$RQ/0003.json"
printf '%s\n' "{\"seq\": 4, \"body\": {\"messages\": [{\"role\": \"user\", \"content\": [$TR]}, {\"role\": \"user\", \"content\": \"later\"}]}}" >"$RQ/0004.json"
check "lead-next: the first lead request" [ "$(python3 -I "$WA" lead-next "$RQ" toolu_x)" = "$RQ/0003.json" ]
tr=$(python3 -I "$WA" tool-result "$RQ/0003.json" toolu_x REASON)
check "tool-result: holds the reason" [ "$(jv "$tr" .holds_reason x)" = true ]
check "tool-result: reason only" [ "$(jv "$tr" .reason_only x)" = true ]
hasnt() { ! has "$1" "$2"; }
check "tool-result: no text of the result" hasnt "$tr" '"content"'
tr=$(python3 -I "$WA" tool-result "$RQ/0001.json" toolu_x REASON)
check "tool-result: none gives false" [ "$(printf '%s' "$tr" | jq -r .holds_reason)" = false ]

[ "$bad" -eq 0 ] || { echo "workspace tests: $bad of $n failed"; exit 1; }
echo "workspace tests: ok ($n)"

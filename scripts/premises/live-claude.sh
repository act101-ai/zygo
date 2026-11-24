#!/bin/sh
# Live checks on the pinned Claude Code through zygo, mock upstream (node N38):
# P-N24-1, P-N25-1, P-N28-1, N15-1M, A-20, 40. No key; the mock answers only the dummy key.
# Usage: sh scripts/premises/live-claude.sh [n24 n25 n28 n15 n15c a20]  (default: all)
# n15c: the N15-1M control run, --model design (no [1m] suffix), recorded only.
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps
pinned_agents
FX="$REPO_DIR/fixtures/claude-code/live"
mkdir -p "$FX"
# Main-loop requests carry tools; Claude Code's own auxiliary requests (titles, quota) do not.
TX='"extra": {"body": "\"tools\": \\[\\{"}'

# steps <prefix> <steps json>: SSE files and mock rules for one scripted sequence.
steps() {
  mkdir -p "$RUN/sse"
  printf '%s\n' "$2" >"$RUN/steps-$1.json"
  python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" "$1" "$RUN/rules.json" "$RUN/steps-$1.json"
}
# settings [<responses.json>]: the run's user settings: hook logs, Bash allowed without a prompt.
settings() {
  hook_settings "$RUN/hooks" ${1:+"$1"} | jq '. + {permissions: {allow: ["Bash"], defaultMode: "default"}}' >"$H/.claude/settings.json"
}
first_hook() { ls "$RUN/hooks" 2>/dev/null | grep -E "$1" | head -n 1; }
wait_hook() { # <regex> <seconds>
  i=0
  until [ -n "$(first_hook "$1")" ]; do
    i=$((i + 1)); [ $i -lt $(($2 * 2)) ] || return 1
    sleep 0.5
  done
}
wait_body() { # <regex> <seconds>
  i=0
  until [ -n "$(body_has "$1" "$RUN"/rec/[0-9]*.json 2>/dev/null)" ]; do
    i=$((i + 1)); [ $i -lt $(($2 * 2)) ] || return 1
    check_abort; sleep 0.5
  done
}
set_ver() { v=$(usage_version); LIVE_VER=${v:-$LIVE_CC_VER}; }

part_n24() {
  # (a) print mode: a Bash command with a non-zero exit.
  live_run n24a
  steps n24a '{"trigger": "tracer-n24 go", '"$TX"', "steps": [
    {"tool": "Bash", "input": {"command": "exit 3"}}, {"text": "n24 done"}]}'
  start_mock "$RUN/rules.json"; mock_config "$RUN/zygo.toml"; settings
  run_zygo claude -- -p "tracer-n24 go" </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock; set_ver
  fa=$(first_hook 'PostToolUseFailure-Bash-main')
  [ -n "$fa" ] && scrub "$RUN/hooks/$fa" >"$FX/p-n24-1-exit.json"
  ev_a=$(ls "$RUN/hooks" | grep -E 'PostToolUse.*-Bash-' | sed 's/^[0-9]*-//; s/\.json$//' | tr '\n' ' ')
  # (b) interactive: an interrupted command.
  live_run n24b
  steps n24b '{"trigger": "tracer-n24i go", '"$TX"', "steps": [
    {"tool": "Bash", "input": {"command": "python3 -c \"import time; time.sleep(120)\""}}, {"text": "n24i done"}]}'
  start_mock "$RUN/rules.json"; mock_config "$RUN/zygo.toml"; settings
  tmux_zygo n24 claude -- --model lead
  tmux_wait_text n24 'zygo ◇' 60 || true
  tmux_send n24 "tracer-n24i go"
  ib=""
  if wait_hook 'PreToolUse-Bash' 60; then
    sleep 2
    tmux send-keys -t =n24: Escape
    wait_hook 'PostToolUse.*-Bash' 30 || true
    fb=$(first_hook 'PostToolUse.*-Bash')
    if [ -n "$fb" ]; then
      scrub "$RUN/hooks/$fb" >"$FX/p-n24-1-interrupt.json"
      ib=$(jq -r '.tool_response.interrupted // "absent"' "$RUN/hooks/$fb")
      ib="${fb#*-} with tool_response.interrupted=$ib"
    else
      ib="no PostToolUse* hook within 30 s of the interrupt"
      tmux capture-pane -p -t =n24: >"$RUN/pane-b.txt" 2>/dev/null || true
      python3 -I - "$RUN/hooks" "$RUN/pane-b.txt" >"$RUN/interrupt.json" <<'PY2'
import json, os, sys
ev = sorted(f[5:-5] for f in os.listdir(sys.argv[1]) if f[0].isdigit() and f.endswith(".json"))
pane = [l.strip() for l in open(sys.argv[2], errors="replace") if "Interrupted" in l or "Bash(" in l]
print(json.dumps({"hook_events": ev, "pane": pane}, indent=2, ensure_ascii=False))
PY2
      scrub "$RUN/interrupt.json" >"$FX/p-n24-1-interrupt.json"
    fi
  else
    ib="the Bash call did not start"
  fi
  tmux capture-pane -p -t =n24: >"$RUN/pane.txt" 2>/dev/null || true
  tmux kill-session -t =n24 2>/dev/null || true
  stop_mock
  if [ -n "$fa" ] && echo "$ib" | grep -q 'interrupted=true'; then r=pass; else r=fail; fi
  result P-N24-1 "$r" "(a) exit 3 fired: ${ev_a:-none}; (b) interrupt: $ib"
}

part_n25() {
  live_run n25
  steps n25-sub '{"trigger": "tracer-n25-sub", '"$TX"', "steps": [
    {"tool": "Bash", "input": {"command": "true"}}, {"text": "n25 sub done"}]}'
  steps n25 '{"trigger": "tracer-n25 go", '"$TX"', "steps": [
    {"tool": "Agent", "input": {"subagent_type": "zygo-code", "prompt": "tracer-n25-sub run one command", "description": "n25", "run_in_background": false}},
    {"text": "n25 lead done"}]}'
  cat >"$RUN/responses.json" <<'J'
[{"event": "SubagentStop", "times": 1,
  "output": {"hookSpecificOutput": {"hookEventName": "SubagentStop", "additionalContext": "tracer-n25-context"}}}]
J
  start_mock "$RUN/rules.json"; mock_config "$RUN/zygo.toml"; settings "$RUN/responses.json"
  run_zygo claude -- -p "tracer-n25 go" </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock; set_ver
  stop_hook=$(first_hook 'SubagentStop')
  f=$(body_has 'tracer-n25-context' "$RUN"/rec/[0-9]*.json | head -n 1)
  if [ -n "$f" ]; then
    who=$(jq -r 'if ((.body.messages[0] | tostring) | test("tracer-n25-sub")) then "subagent" else "lead" end' "$f")
    scrub "$f" >"$FX/p-n25-1.json"
    result P-N25-1 pass "SubagentStop additionalContext reached the $who (request $(basename "$f"))"
  else
    python3 -I - "$RUN/rec" >"$RUN/last2.json" <<'PY'
import json, os, sys
d = sys.argv[1]
fs = sorted(f for f in os.listdir(d) if f[0].isdigit())[-2:]
print(json.dumps([json.load(open(os.path.join(d, f))) for f in fs]))
PY
    scrub "$RUN/last2.json" >"$FX/p-n25-1.json"
    result P-N25-1 fail "SubagentStop hook ${stop_hook:+fired ($stop_hook) }returned additionalContext; no later request carries it"
  fi
}

part_n28() {
  live_run n28
  start_mock; mock_config "$RUN/zygo.toml"; settings
  tmux_zygo n28 claude --compact 777k
  tmux_wait_text n28 'zygo ◇' 30 || true
  tmux_send n28 "tracer-n28 say hi"
  wait_body 'tracer-n28' 60 || true
  sleep 3
  tmux capture-pane -p -t =n28: >"$RUN/pane.txt" 2>/dev/null || true
  tmux kill-session -t =n28 2>/dev/null || true
  stop_mock; set_ver
  scrub "$RUN/pane.txt" >"$FX/p-n28-1-pane.txt"
  leak=$(body_has '777k|777000|zygo ◇|◇' "$RUN"/rec/[0-9]*.json)
  n=$(body_has 'tracer-n28' "$RUN"/rec/[0-9]*.json | wc -l | tr -d ' ')
  if grep -qF 'zygo ◇' "$RUN/pane.txt" && grep -qF 'compact 777k' "$RUN/pane.txt" && [ -z "$leak" ] && [ "$n" -gt 0 ]; then
    result P-N28-1 pass "status line shown (zygo ◇ … compact 777k); its text in none of $(ls "$RUN"/rec/[0-9]*.json | wc -l | tr -d ' ') requests"
  else
    result P-N28-1 fail "pane has zygo ◇: $(grep -cF 'zygo ◇' "$RUN/pane.txt"); tracer requests: $n; requests holding status text: ${leak:-none}"
  fi
}

# n15_run <session> <model arg>: /context for <model arg>, role design on a 200k model; pane in $RUN/pane.txt.
n15_run() {
  live_run "$1"
  start_mock; mock_config "$RUN/zygo.toml"
  cat >>"$RUN/zygo.toml" <<T
[[model]]
id = "claude-haiku-4-5"

  [model.facts]
  reasoning = "switchable"
  reasoning_default_on = true
  effort_levels = ["low", "medium", "high", "xhigh", "max"]
  max_output_tokens = 128000
  context_window = 200000
  sampling = false
  vision = true
  structured_output = "native"
  forced_tool_choice = true
  verified_on = 2026-11-18
  source = "scripts/premises/live.sh: mock upstream"

  [[model.provider]]
  name = "mock"
  base_url = "$MOCK_URL"
  dialects = ["anthropic"]
  upstream_model = "claude-haiku-4-5"
  key = "env:ZYGO_LIVE_MOCK_KEY"
  enabled = true
T
  sed -i.bak '/^\[role\.design\]$/{n;s/.*/model = "claude-haiku-4-5"/;}' "$RUN/zygo.toml"
  settings
  tmux_zygo "$1" claude -- --model "$2"
  tmux_wait_text "$1" 'zygo ◇' 30 || true
  tmux_send "$1" /context
  sleep 5
  tmux_wait_text "$1" 'tokens' 20 || true
  tmux capture-pane -p -t "=$1:" -S -100 >"$RUN/pane.txt" 2>/dev/null || true
  tmux kill-session -t "=$1" 2>/dev/null || true
  stop_mock; set_ver
}

part_n15() {
  n15_run n15 'design[1m]'
  scrub "$RUN/pane.txt" >"$FX/n15-1m-context.txt"
  shown=$(grep -oE '[0-9.,]+[kKmM]? */ *[0-9.,]+[kKmM]? *tokens' "$RUN/pane.txt" | head -n 1)
  case "$shown" in
    */*200[kK]*|*/*200,000*) result N15-1M pass "/context for design[1m] (behavesAs 200k): $shown" ;;
    *) result N15-1M fail "/context for design[1m] (behavesAs 200k): ${shown:-no token line found}" ;;
  esac
}

part_n15c() {
  n15_run n15c design
  scrub "$RUN/pane.txt" >"$FX/n15-control-context.txt"
  shown=$(grep -oE '[0-9.,]+[kKmM]? */ *[0-9.,]+[kKmM]? *tokens' "$RUN/pane.txt" | head -n 1)
  result N15-CTL recorded "/context for design (no [1m]; behavesAs 200k): ${shown:-no token line found}"
}

part_a20() {
  live_run a20
  mkdir -p "$RUN/bin"
  ln -sf "$PREMISES_DIR/live/claude-argv.sh" "$RUN/bin/claude"
  OLD_PATH=$TOOL_PATH; TOOL_PATH="$RUN/bin:$TOOL_PATH"
  steps a20-rev '{"trigger": "tracer-a20-review", '"$TX"', "steps": [{"text": "a20 review done"}]}'
  steps a20-code '{"trigger": "tracer-a20-code", '"$TX"', "steps": [{"text": "a20 code done"}]}'
  steps a20 '{"trigger": "tracer-a20 go", '"$TX"', "steps": [
    {"tool": "Agent", "input": {"subagent_type": "zygo-review", "prompt": "tracer-a20-review look", "description": "a20 review", "run_in_background": false}},
    {"tool": "Agent", "input": {"subagent_type": "zygo-code", "prompt": "tracer-a20-code look", "description": "a20 code", "run_in_background": false}},
    {"text": "a20 lead done"}]}'
  start_mock "$RUN/rules.json"; mock_config "$RUN/zygo.toml"; settings
  LIVE_VARS="LIVE_ARGV_OUT=$RUN/agents.json LIVE_REAL_CLAUDE=$CLAUDE_BIN" \
    run_zygo claude -- -p "tracer-a20 go" </dev/null >"$RUN/out.txt" 2>&1 || true
  TOOL_PATH=$OLD_PATH
  stop_mock; set_ver
  python3 -I - "$RUN/agents.json" "$RUN/rec" >"$RUN/a20.json" <<'PY'
import json, os, sys
agents = json.load(open(sys.argv[1])) if os.path.exists(sys.argv[1]) else {}
rev = dict(agents.get("zygo-review", {}))
if "prompt" in rev:
    rev["prompt"] = "[omitted]"
def tools(tag):
    out = set()
    for f in sorted(os.listdir(sys.argv[2])):
        if not f[0].isdigit():
            continue
        b = json.load(open(os.path.join(sys.argv[2], f))).get("body")
        if isinstance(b, dict) and b.get("messages") and tag in json.dumps(b["messages"][0]):
            out.update(t.get("name") for t in b.get("tools") or [])
    return sorted(out)
print(json.dumps({"agents": {"zygo-review": rev}, "review_tools": tools("tracer-a20-review"),
                  "code_tools": tools("tracer-a20-code")}, indent=2))
PY
  scrub "$RUN/a20.json" >"$FX/a20-tools.json"
  ok=$(jq -r '(.agents["zygo-review"] | has("disallowedTools")) and (.review_tools | length > 0)
    and ([.review_tools[] | select(. == "Edit" or . == "Write" or . == "NotebookEdit")] | length == 0)
    and (.code_tools | index("Edit") != null)' "$RUN/a20.json")
  if [ "$ok" = true ]; then r=pass; else r=fail; fi
  result A-20 "$r" "zygo-review disallowedTools=$(jq -c '.agents["zygo-review"].disallowedTools' "$RUN/a20.json"); review tools: $(jq -c .review_tools "$RUN/a20.json"); code has Edit: $(jq '.code_tools | index("Edit") != null' "$RUN/a20.json")"
  # 40: the delivered snapshot's skill listed as <plugin>:<skill>, from the same records.
  python3 -I - "$RUN/rec" >"$RUN/40.json" <<'PY'
import json, os, re, sys
names = set()
for f in sorted(os.listdir(sys.argv[1])):
    if f[0].isdigit():
        t = json.dumps(json.load(open(os.path.join(sys.argv[1], f))), ensure_ascii=False)
        names.update(re.findall(r"(?<![A-Za-z0-9-])zygo:[a-z0-9-]+", t))
print(json.dumps(sorted(names)))
PY
  cp "$RUN/40.json" "$FX/40-skills.json"
  if jq -e 'index("zygo:zygo-cli") != null' "$RUN/40.json" >/dev/null; then
    result 40 pass "skill listed as zygo:zygo-cli in $(body_has 'zygo:zygo-cli' "$RUN"/rec/[0-9]*.json | wc -l | tr -d ' ') request(s); names: $(cat "$RUN/40.json")"
  else
    result 40 fail "zygo:zygo-cli in no request; zygo: names seen: $(cat "$RUN/40.json")"
  fi
}

for p in ${*:-n24 n25 n28 n15 n15c a20}; do "part_$p"; done
live_done "$FX"

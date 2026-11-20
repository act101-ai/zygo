#!/bin/sh
# Q1: which event can continue a subagent (claims guard, spec §13.4)? A SubagentStop hook
# returns {"decision":"block","reason":...} once; verified when the same subagent (same
# x-claude-code-agent-id) sends another request instead of returning to the lead.
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/claude-code"
mkdir -p "$FX/sse" "$FX/requests" "$FX/hooks"
new_run Q1
cat >"$RUN/steps-main.json" <<J
{"trigger": "tracer-q1-main", "no_header": ["x-claude-code-agent-id"], "steps": [
 {"tool": "Agent", "input": {"description": "q1 subagent", "prompt": "tracer-q1-sub report back", "subagent_type": "claude", "run_in_background": false}},
 {"text": "q1 main done"}]}
J
cat >"$RUN/steps-sub.json" <<J
{"trigger": "tracer-q1-sub", "header": {"x-claude-code-agent-id": "."}, "steps": [{"text": "q1 sub first report"}]}
J
mkdir -p "$RUN/sse"
python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" q1-sub "$RUN/rules.json" "$RUN/steps-sub.json"
python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" q1-main "$RUN/rules.json" "$RUN/steps-main.json"
cat >"$RUN/responses.json" <<'J'
[{"event": "SubagentStop", "who": "sub", "times": 1,
  "output": {"decision": "block", "reason": "zygo: tracer-q1-continue run the acceptance check and quote its output."}}]
J
hook_settings "$RUN/hooks" "$RUN/responses.json" >"$RUN/settings.json"
start_mock "$RUN/rules.json"
run_claude -p "tracer-q1-main dispatch" --settings "$RUN/settings.json" --dangerously-skip-permissions </dev/null >"$RUN/out.txt" 2>&1 || true
stop_mock
publish_sse "$FX/sse"
python3 -I "$PREMISES_DIR/summarize.py" "$RUN/rec" >"$RUN/summary.json"
scrub "$RUN/summary.json" >"$FX/requests/q1-run.summary.json"
f=$(grep -l 'tracer-q1-continue' "$RUN"/rec/[0-9]*.json | head -n 1 || true)
[ -n "$f" ] && scrub "$f" >"$FX/requests/q1-continued-subagent-request.json"
for h in "$RUN"/hooks/*SubagentStop*.json; do
  out="${h%.json}.out"
  [ -e "$out" ] || continue
  scrub "$h" >"$FX/hooks/SubagentStop-sub-blocked.json"
  cp "$out" "$RUN/block-output.json"
done
jq -c '.[] | [.seq,.reply,.hint_headers["x-claude-code-agent-id"],.hint_headers["x-claude-code-request-class"],.messages,.last_message[0:120]]' "$FX/requests/q1-run.summary.json"
ls "$RUN/hooks"

# Q1 and P3 with a zygo-style subagent: `--agents` zygo-code, SubagentStop registered with the
# matcher ^zygo- (spec §13.3) that blocks once; a built-in claude subagent is dispatched too, so
# the matcher's filtering shows. Every event also logs to hooks-all (matcher "*").
new_run Q1-zygo
cat >"$RUN/steps-main.json" <<J
{"trigger": "tracer-q1z-main", "no_header": ["x-claude-code-agent-id"], "steps": [
 {"tool": "Agent", "input": {"description": "q1 zygo", "prompt": "tracer-q1z-zygo run one command", "subagent_type": "zygo-code", "run_in_background": false}},
 {"tool": "Agent", "input": {"description": "q1 builtin", "prompt": "tracer-q1z-builtin report", "subagent_type": "claude", "run_in_background": false}},
 {"text": "q1z main done"}]}
J
cat >"$RUN/steps-zygo.json" <<J
{"trigger": "tracer-q1z-zygo", "header": {"x-claude-code-agent-id": "."}, "steps": [
 {"tool": "Bash", "input": {"command": "echo tracer-q1z-check"}},
 {"text": "q1z zygo-code first report"}]}
J
echo '{"trigger": "tracer-q1z-builtin", "header": {"x-claude-code-agent-id": "."}, "steps": [{"text": "q1z builtin report"}]}' >"$RUN/steps-builtin.json"
mkdir -p "$RUN/sse"
for s in zygo builtin main; do
  python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" "q1z-$s" "$RUN/rules.json" "$RUN/steps-$s.json"
done
cat >"$RUN/responses.json" <<'J'
[{"event": "SubagentStop", "times": 1,
  "output": {"decision": "block", "reason": "zygo: tracer-q1z-continue run the acceptance check `echo tracer-q1z-check` alone and quote its output."}}]
J
python3 -I - "$PREMISES_DIR" "$RUN" >"$RUN/settings.json" <<'PY'
import json, sys
pd, run = sys.argv[1:3]
log_all = [{"type": "command", "command": "python3 -I %s/hook_log.py %s/hooks-all" % (pd, run)}]
zygo = [{"type": "command", "command": "python3 -I %s/hook_log.py %s/hooks-zygo %s/responses.json" % (pd, run, run)}]
hooks = {e: [{"matcher": "*", "hooks": log_all}] for e in ["PreToolUse", "PostToolUse", "SubagentStart"]}
hooks["SubagentStop"] = [{"matcher": "*", "hooks": log_all}, {"matcher": "^zygo-", "hooks": zygo}]
print(json.dumps({"disableAllHooks": False, "hooks": hooks}))
PY
AGENTS='{"zygo-code":{"description":"code role","prompt":"tracer-q1z-agent-prompt","model":"claude-opus-5-5"}}'
start_mock "$RUN/rules.json"
run_claude -p "tracer-q1z-main dispatch" --settings "$RUN/settings.json" --agents "$AGENTS" --dangerously-skip-permissions </dev/null >"$RUN/out.txt" 2>&1 || true
stop_mock
publish_sse "$FX/sse"
python3 -I "$PREMISES_DIR/summarize.py" "$RUN/rec" >"$RUN/summary.json"
scrub "$RUN/summary.json" >"$FX/requests/q1-zygo-run.summary.json"
python3 -I - "$RUN" >"$RUN/q1-zygo.json" <<'PY'
import json, os, sys
run = sys.argv[1]
def hooks(d):
    d = os.path.join(run, d)
    return [json.load(open(os.path.join(d, f))) for f in sorted(os.listdir(d)) if f[0].isdigit() and f.endswith(".json")] if os.path.isdir(d) else []
recs = [json.load(open(os.path.join(run, "rec", f))) for f in sorted(os.listdir(run + "/rec")) if f[0].isdigit()]
zh = hooks("hooks-zygo")
allh = hooks("hooks-all")
zid = {h["agent_id"] for h in allh if h.get("agent_type") == "zygo-code"}
zreqs = [r for r in recs if r["headers"].get("x-claude-code-agent-id") in zid]
print(json.dumps({
  "SubagentStop inputs, matcher *": [[h.get("agent_type"), h["agent_id"]] for h in allh if h["hook_event_name"] == "SubagentStop"],
  "SubagentStop inputs, matcher ^zygo-": [[h.get("agent_type"), h["agent_id"]] for h in zh],
  "zygo-code hook agent_id values": sorted(zid),
  "x-claude-code-agent-id of the zygo-code requests": sorted({r["headers"]["x-claude-code-agent-id"] for r in zreqs}),
  "x-claude-code-agent-type of the zygo-code requests": sorted({r["headers"].get("x-claude-code-agent-type") for r in zreqs}),
  "zygo-code requests": len(zreqs),
  "zygo-code request after the block holds the reason": any("tracer-q1z-continue" in json.dumps(r["body"]) for r in zreqs),
  "main-loop requests with an agent id header": sum(1 for r in recs if r["headers"].get("x-claude-code-request-class") == "main" and "x-claude-code-agent-id" in r["headers"]),
}, indent=1))
PY
scrub "$RUN/q1-zygo.json" >"$FX/requests/q1-zygo-compare.json"
for h in "$RUN"/hooks-zygo/*SubagentStop*.json; do
  [ -e "${h%.json}.out" ] && scrub "$h" >"$FX/hooks/SubagentStop-zygo-code-sub.json"
done
cat "$FX/requests/q1-zygo-compare.json"
echo "Q1 done"

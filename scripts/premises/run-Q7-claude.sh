#!/bin/sh
# Q7 (Claude Code): every setting key, env row and flag zygo writes (spec §8.4, §8.5), each shown
# in help.txt or settings-schema.json and with its effect in a capture.
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/claude-code"
OUT="$FX/q7"
rm -rf "$OUT"; mkdir -p "$OUT" "$FX/sse"
new_run Q7
# Plugin snapshot (--plugin-dir), MCP config (--mcp-config), agents (--agents).
PL="$RUN/plugins/zygo-probe"
mkdir -p "$PL/.claude-plugin" "$PL/skills/probe-skill" "$PL/commands"
echo '{"name":"zygo-probe","version":"0.0.1","description":"N2 probe plugin"}' >"$PL/.claude-plugin/plugin.json"
printf -- '---\nname: probe-skill\ndescription: tracer-q7-plugin-skill description\n---\nProbe skill body.\n' >"$PL/skills/probe-skill/SKILL.md"
printf -- '---\ndescription: tracer-q7-plugin-command\n---\nProbe command body.\n' >"$PL/commands/probe.md"
printf '{"mcpServers":{"probe":{"command":"python3","args":["-I","%s"]}}}\n' "$PREMISES_DIR/mcp_probe.py" >"$RUN/mcp.json"
AGENTS='{"zygo-code":{"description":"tracer-q7-agent-description","prompt":"tracer-q7-agent-prompt","model":"code","disallowedTools":["Edit","Write","NotebookEdit"],"maxTurns":2}}'
python3 -I - "$RUN" "$DUMMY_KEY" "$PREMISES_DIR" <<'PY'
import json, sys
run, key, pd = sys.argv[1:4]
hook = [{"type": "command", "command": "python3 -I %s/hook_log.py %s/hooks" % (pd, run)}]
s = {
  "apiKeyHelper": "printf %s " + key,
  "model": "lead",
  "availableModels": ["lead", "code", "read", "design"],
  "modelPicker": {"options": [{"model": m, "label": m, "description": "role " + m} for m in ["lead", "code", "read", "design"]]},
  "statusLine": {"type": "command", "command": "python3 -I %s/hook_log.py %s/statusline" % (pd, run), "refreshInterval": 2},
  "disableAllHooks": False,
  "hooks": {e: [{"matcher": "*", "hooks": hook}] for e in ["PreToolUse", "PostToolUse", "PostToolUseFailure"]},
  "env": {"ZYGO_Q7_ENV": "tracer-q7-env",
          "ANTHROPIC_CUSTOM_HEADERS": "X-Zygo-Session: tracer-q7-session\nX-Zygo-Repo: example.com/tracer/repo"},
}
for e in ["SessionStart", "UserPromptSubmit", "PreCompact", "SubagentStop"]:
    s["hooks"][e] = [{"hooks": hook}]
json.dump(s, open(run + "/settings.json", "w"), indent=1)
PY
cat >"$RUN/steps-main.json" <<J
{"trigger": "tracer-q7-a1", "no_header": ["x-claude-code-agent-id"], "steps": [
 {"tool": "Bash", "input": {"command": "echo \$ZYGO_Q7_ENV"}},
 {"tool": "Agent", "input": {"description": "q7 code", "prompt": "tracer-q7-sub three commands", "subagent_type": "zygo-code", "run_in_background": false}},
 {"tool": "Agent", "input": {"description": "q7 builtin", "prompt": "tracer-q7-builtin answer", "subagent_type": "claude", "run_in_background": false}},
 {"text": "q7 main done"}]}
J
cat >"$RUN/steps-sub.json" <<J
{"trigger": "tracer-q7-sub", "header": {"x-claude-code-agent-id": "."}, "steps": [
 {"tool": "Bash", "input": {"command": "echo sub-turn-1"}},
 {"tool": "Bash", "input": {"command": "echo sub-turn-2"}},
 {"tool": "Bash", "input": {"command": "echo sub-turn-3"}},
 {"text": "q7 sub done"}]}
J
mkdir -p "$RUN/sse"
python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" q7-sub "$RUN/rules.json" "$RUN/steps-sub.json"
echo '{"trigger": "tracer-q7-builtin", "header": {"x-claude-code-agent-id": "."}, "steps": [{"text": "q7 builtin done"}]}' >"$RUN/steps-builtin.json"
python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" q7-builtin "$RUN/rules.json" "$RUN/steps-builtin.json"
python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" q7-main "$RUN/rules.json" "$RUN/steps-main.json"
start_mock "$RUN/rules.json"
ALIASES="ANTHROPIC_DEFAULT_FABLE_MODEL=design[1m] ANTHROPIC_DEFAULT_OPUS_MODEL=lead ANTHROPIC_DEFAULT_SONNET_MODEL=code ANTHROPIC_DEFAULT_HAIKU_MODEL=read CLAUDE_CODE_SUBAGENT_MODEL=code DISABLE_LOGIN_COMMAND=1 DISABLE_LOGOUT_COMMAND=1"
FLAGS="--settings $RUN/settings.json --agents $AGENTS --plugin-dir $RUN/plugins --mcp-config $RUN/mcp.json --dangerously-skip-permissions"

# A1: the full session config, no ANTHROPIC_API_KEY (apiKeyHelper supplies the key).
EXTRA_ENV="$ALIASES"
printf '%s\n' "tracer-q7-a1 go" >"$RUN/msgs"
NO_API_KEY=1
run_claude_stream "$RUN/a1.jsonl" "$RUN/msgs" $FLAGS
cp -R "$RUN/rec" "$RUN/rec-a1"
# A1b: control for DISABLE_LOGIN_COMMAND / DISABLE_LOGOUT_COMMAND (same flags, without them).
NO_API_KEY=""; EXTRA_ENV=""
printf '%s
' "tracer-q7-control" >"$RUN/msgs-control"
run_claude_stream "$RUN/a1-control.jsonl" "$RUN/msgs-control" --settings "$RUN/settings.json"
NO_API_KEY=1; EXTRA_ENV="$ALIASES"
# A2: alias env rows: --model opus|sonnet|haiku|fable.
for a in opus sonnet haiku fable; do
  run_claude -p "tracer-q7-alias-$a" --model "$a" --settings "$RUN/settings.json" </dev/null >"$RUN/a2-$a.txt" 2>&1 || true
done
# A3: availableModels: a model outside the list.
run_claude -p "tracer-q7-outside" --model claude-haiku-4-5 --settings "$RUN/settings.json" </dev/null >"$RUN/a3.txt" 2>&1 || true
# A4: without CLAUDE_CODE_GATEWAY_HINT_HEADERS.
EXTRA_ENV=""; NO_API_KEY=""; NO_HINT=1
run_claude -p "tracer-q7-nohint" </dev/null >"$RUN/a4.txt" 2>&1 || true
NO_HINT=""
# A5: disableAllHooks true in --settings: no hook runs.
python3 -I -c 'import json,sys; s=json.load(open(sys.argv[1])); s["disableAllHooks"]=True; json.dump(s,open(sys.argv[2],"w"))' "$RUN/settings.json" "$RUN/settings-disabled.json"
n_before=$(ls "$RUN/hooks" | grep -c '^[0-9]' || true)
run_claude -p "tracer-q7-hooks-off" --settings "$RUN/settings-disabled.json" </dev/null >"$RUN/a5.txt" 2>&1 || true
n_after=$(ls "$RUN/hooks" | grep -c '^[0-9]' || true)
# A6: managed settings (CLAUDE_CODE_MANAGED_SETTINGS_PATH) with allowManagedHooksOnly: true.
echo '{"allowManagedHooksOnly": true}' >"$RUN/managed-settings.json"
EXTRA_ENV="CLAUDE_CODE_MANAGED_SETTINGS_PATH=$RUN/managed-settings.json"
run_claude -p "tracer-q7-managed" --settings "$RUN/settings.json" </dev/null >"$RUN/a6.txt" 2>&1 || true
n_managed=$(ls "$RUN/hooks" | grep -c '^[0-9]' || true)
# A7: statusLine with refreshInterval 2, interactive, 11 s idle.
EXTRA_ENV="$ALIASES"
NO_API_KEY=1
tmux_claude n2q7 --settings "$RUN/settings.json"
sleep 16
tmux capture-pane -p -t =n2q7: >"$RUN/a7-screen.txt" || true
NO_API_KEY=""
tmux kill-session -t =n2q7 2>/dev/null || true
# A8: DISABLE_LOGIN_COMMAND / DISABLE_LOGOUT_COMMAND: the interactive "/log" suggestions.
NO_API_KEY=""
for v in set unset; do
  EXTRA_ENV=""; [ $v = set ] && EXTRA_ENV="DISABLE_LOGIN_COMMAND=1 DISABLE_LOGOUT_COMMAND=1"
  tmux_claude n2q7b
  sleep 6
  tmux send-keys -t =n2q7b: -l "/log"
  sleep 2
  tmux capture-pane -p -t =n2q7b: | grep -E '^ *(❯ )?/(login|logout|debug)' >"$RUN/a8-$v.txt" || true
  tmux kill-session -t =n2q7b 2>/dev/null || true
done
stop_mock
publish_sse "$FX/sse"

python3 -I - "$RUN" "$n_before" "$n_after" "$n_managed" >"$RUN/q7-claude.json" <<'PY'
import json, os, sys
run, n_before, n_after, n_managed = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])
def recs(d):
    rs = [json.load(open(os.path.join(d, f))) for f in sorted(os.listdir(d)) if f[0].isdigit()]
    return [r for r in rs if r["method"] == "POST" and isinstance(r.get("body"), dict)]
a1 = recs(run + "/rec-a1")
allr = recs(run + "/rec")
def blob(r): return json.dumps(r.get("body"))
main = [r for r in a1 if r["headers"].get("x-claude-code-request-class") == "main"]
sub = [r for r in a1 if "tracer-q7-sub" in blob(r) and "x-claude-code-agent-id" in r["headers"]]
builtin = [r for r in a1 if "tracer-q7-builtin" in blob(r) and "x-claude-code-agent-id" in r["headers"]]
hook_types = sorted({json.load(open(os.path.join(run, "hooks", f))).get("agent_type") or "" for f in os.listdir(run + "/hooks") if f[0].isdigit() and f.endswith("-sub.json")})
init_c = next((json.loads(l) for l in open(run + "/a1-control.jsonl") if '"subtype":"init"' in l.replace(" ", "")), {})
init = next((json.loads(l) for l in open(run + "/a1.jsonl") if '"subtype":"init"' in l.replace(" ", "")), {})
def first(tr):
    return next((r for r in allr if tr in blob(r)), None)
alias = {}
for a in ["opus", "sonnet", "haiku", "fable"]:
    r = first("tracer-q7-alias-" + a)
    alias[a] = r and {"model": r["body"]["model"], "anthropic-beta": r["headers"].get("anthropic-beta")}
out_r = first("tracer-q7-outside")
nohint = first("tracer-q7-nohint")
sl = sorted(f for f in os.listdir(run + "/statusline") if f[0].isdigit()) if os.path.isdir(run + "/statusline") else []
sl_in = json.load(open(run + "/statusline/" + sl[0])) if sl else None
res = {
 "apiKeyHelper": {"ANTHROPIC_API_KEY_set": False, "mock_accepted_requests": len(a1),
                   "credential_headers_of_first_request": a1[0]["credential_headers"] if a1 else None},
 "model": {"main_request_models": sorted({r["body"]["model"] for r in main})},
 "availableModels": {"--model claude-haiku-4-5 outside the list -> request model": out_r and out_r["body"]["model"],
                     "output": open(run + "/a3.txt").read()[:400]},
 "env": {"ZYGO_Q7_ENV in a Bash tool_result": any("tracer-q7-env" in json.dumps(m) for r in main for m in r["body"]["messages"] if m.get("role") == "user")},
 "ANTHROPIC_CUSTOM_HEADERS (set in settings env)": {"x-zygo-session": main[0]["headers"].get("x-zygo-session") if main else None,
                                                    "x-zygo-repo": main[0]["headers"].get("x-zygo-repo") if main else None},
 "aliases (ANTHROPIC_DEFAULT_*_MODEL)": alias,
 "--agents model": {"zygo-code request models": sorted({r["body"]["model"] for r in sub})},
 "CLAUDE_CODE_SUBAGENT_MODEL": {"built-in claude subagent request models": sorted({r["body"]["model"] for r in builtin})},
 "subagent identity": {"x-claude-code-agent-type of zygo-code requests": sorted({r["headers"].get("x-claude-code-agent-type") for r in sub}),
                       "x-claude-code-agent-type of built-in claude requests": sorted({r["headers"].get("x-claude-code-agent-type") for r in builtin}),
                       "hook agent_type values in subagent hook inputs": hook_types},
 "--agents": {"init agents": init.get("agents"),
              "subagent requests": len(sub),
              "main request tool names": sorted({t["name"] for r in main for t in r["body"].get("tools") or []}),
              "subagent tool names": sorted({t["name"] for r in sub for t in r["body"].get("tools") or []}),
              "prompt text in subagent system": any("tracer-q7-agent-prompt" in json.dumps(r["body"].get("system")) for r in sub),
              "description in main request": any("tracer-q7-agent-description" in blob(r) for r in main),
              "maxTurns=2: requests the subagent sent (3 tool turns scripted)": len(sub),
              "maxTurns=2: text the lead got back": next((json.dumps(m)[:300] for r in a1 for m in r["body"]["messages"] if "toolu_q7_main_01_agent" in json.dumps(m) and "tool_result" in json.dumps(m)), None)},
 "--plugin-dir": {"init plugins": init.get("plugins"),
                  "plugin skill in main request": any("probe-skill" in blob(r) for r in main),
                  "plugin command in init slash_commands": [c for c in init.get("slash_commands", []) if "probe" in c]},
 "--mcp-config": {"init mcp_servers": init.get("mcp_servers"),
                  "probe tool named in main request": any("mcp__probe__probe_tool" in blob(r) for r in main)},
 "DISABLE_LOGIN_COMMAND / DISABLE_LOGOUT_COMMAND": {"interactive '/log' suggestions, set": open(run + "/a8-set.txt").read().splitlines(),
                                                    "interactive '/log' suggestions, unset": open(run + "/a8-unset.txt").read().splitlines(),"set: login or logout in init slash_commands": [c for c in init.get("slash_commands", []) if c in ("login", "logout")],
                                                    "unset (control): login or logout in init slash_commands": [c for c in init_c.get("slash_commands", []) if c in ("login", "logout")]},
 "CLAUDE_CODE_GATEWAY_HINT_HEADERS": {"with (A1 main) x-claude-code-* headers": sorted(k for k in main[0]["headers"] if k.startswith("x-claude-code-")) if main else None,
                                      "without": nohint and sorted(k for k in nohint["headers"] if k.startswith("x-claude-code-"))},
 "hooks": {"events logged in A1": sorted({f.split("-")[1] for f in os.listdir(run + "/hooks") if f[0].isdigit() and f.endswith(".json")})},
 "disableAllHooks": {"false (A1-A4): hook inputs logged": n_before, "true (A5): new hook inputs": n_after - n_before},
 "allowManagedHooksOnly": {"note": "managed-only key; CLAUDE_CODE_MANAGED_SETTINGS_PATH is not honored by this build (A6 run: --settings hooks still ran), so the effect needs the system managed-settings file (administrator rights), which N2 does not write",
                           "new hook inputs from --settings hooks with the A6 file": n_managed - n_after},
 "statusLine refreshInterval 2": {"statusline runs in about 16 s": len(sl), "first input keys": sorted(sl_in) if sl_in else None},
}
print(json.dumps(res, indent=1))
PY
scrub "$RUN/q7-claude.json" >"$OUT/q7-claude.json"
init_line=$(grep -m1 '"subtype":"init"' "$RUN/a1.jsonl" || true)
echo "$init_line" >"$RUN/init.json"; scrub "$RUN/init.json" >"$OUT/a1-init.json"
cat "$OUT/q7-claude.json"
echo "Q7 claude done"

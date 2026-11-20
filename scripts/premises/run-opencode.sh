#!/bin/sh
# P4, P5, Q2 and the OpenCode half of Q7. Three config layers set the same keys differently:
# user (~/.config/opencode/opencode.json), project (.opencode/opencode.json) and inline
# (OPENCODE_CONFIG_CONTENT); OPENCODE_CONFIG_DIR holds a skill and a plugin. Observed through
# `opencode debug config`, `agent list`, `mcp list`, `debug skill` and an `opencode run` capture.
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/opencode"
rm -rf "$FX/plugin-hooks" "$FX/requests" "$FX/sse"
mkdir -p "$FX/plugin-hooks" "$FX/requests" "$FX/sse"
new_run OC
start_mock
OCDIR="$RUN/ocdir"
mkdir -p "$OCDIR/plugins" "$OCDIR/skills/zygo-probe-skill" "$H/.config/opencode" "$R/.opencode"
cp "$PREMISES_DIR/opencode/zygo-probe.js" "$OCDIR/plugins/zygo.js"
printf -- '---\nname: zygo-probe-skill\ndescription: tracer-oc-skill description\n---\nProbe skill body.\n' >"$OCDIR/skills/zygo-probe-skill/SKILL.md"
MCP_CMD="[\"python3\",\"-I\",\"$PREMISES_DIR/mcp_probe.py\"]"
layer() { # <name> <model> <context> <edit> [<more keys>]: one config layer
  cat <<J
{"\$schema": "https://opencode.ai/config.json",
 "model": "zygo/$2",
 "provider": {"zygo": {"npm": "@ai-sdk/anthropic",
   "options": {"baseURL": "$MOCK_URL/v1", "apiKey": "$DUMMY_KEY", "headers": {"X-Layer": "$1", "X-Layer-$1": "1"}},
   "models": {"$2": {"limit": {"context": $3, "output": 4000}}, "lead": {"limit": {"context": $3, "output": 4000}}}}},
 "agent": {"build": {"model": "zygo/$2"}},
 "permission": {"edit": "$4", "bash": "allow", "$1-only": "allow"},
 "mcp": {"$1-mcp": {"type": "local", "command": $MCP_CMD, "enabled": true}}${5:-}}
J
}
layer user user-model 111111 allow >"$H/.config/opencode/opencode.json"
layer project project-model 222222 ask >"$R/.opencode/opencode.json"
# P4: a same-name mcp.zygo in the project config (an extra "project" argument marks it).
python3 -I -c 'import json,sys
p=sys.argv[1]; c=json.load(open(p)); c["mcp"]["zygo"]={"type":"local","command":json.loads(sys.argv[2])+["project"],"enabled":True}
json.dump(c,open(p,"w"))' "$R/.opencode/opencode.json" "$MCP_CMD"
INLINE=$(layer inline lead 333333 deny ',
 "agent": {"build": {"model": "zygo/lead", "permission": {"edit": "deny"}},
           "zygo-code": {"mode": "subagent", "description": "tracer-oc-agent-description", "prompt": "tracer-oc-agent-prompt", "model": "zygo/lead", "permission": {"edit": "deny"}},
           "zygo-steps": {"mode": "primary", "description": "turn cap probe", "model": "zygo/lead", "steps": 2}},
 "command": {"zygo-cmd": {"description": "tracer-oc-command description", "template": "tracer-oc-command body: $ARGUMENTS"}},
 "mcp": {"zygo": {"type": "local", "command": '"$MCP_CMD"', "enabled": true}}' | python3 -I -c 'import json,sys
t=sys.stdin.read()
# duplicate "agent"/"mcp" keys: the later one (inline-only form) wins in json.loads; re-dump compactly
print(json.dumps(json.loads(t)))')
echo "$INLINE" >"$RUN/inline.json"

oc() { # <args>: OpenCode isolated, in the repo, with the inline config and config dir
  (cd "$R" && env -i PATH="$TOOL_PATH" TERM=xterm-256color HOME="$H" XDG_CONFIG_HOME="$H/.config" \
    XDG_DATA_HOME="$H/.local/share" XDG_CACHE_HOME="$H/.cache" XDG_STATE_HOME="$H/.local/state" \
    OPENCODE_DISABLE_AUTOUPDATE=1 N2_OC_LOG="$RUN/plugin-log" \
    OPENCODE_CONFIG_DIR="$OCDIR" OPENCODE_CONFIG_CONTENT="$INLINE" "$OPENCODE_BIN" "$@" </dev/null)
  check_abort
}
oc debug config >"$RUN/debug-config.json" 2>"$RUN/debug-config.err" || true
oc agent list >"$RUN/agent-list.txt" 2>&1 || true
oc mcp list >"$RUN/mcp-list.txt" 2>&1 || true
oc debug skill >"$RUN/debug-skill.txt" 2>&1 || true

# The run: bash tool call, then a text reply whose usage is over every layer's limit.context.
OVER='{"input_tokens":400000,"output_tokens":5}'
cat >"$RUN/steps.json" <<J
{"trigger": "tracer-oc-run", "extra": {"body": "\"tools\""}, "steps": [
 {"tool": "bash", "input": {"command": "echo tracer-oc-bash-original", "description": "echo"}},
 {"text": "oc done", "usage": $OVER}]}
J
mkdir -p "$RUN/sse"
python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" oc "$RUN/rules.json" "$RUN/steps.json"
# A-20: an agent with steps = 2 (turn cap) asked to run two tool calls.
cat >"$RUN/steps-cap.json" <<J
{"trigger": "tracer-oc-steps", "extra": {"body": "\"tools\""}, "steps": [
 {"tool": "bash", "input": {"command": "echo tracer-oc-steps-1", "description": "echo"}},
 {"tool": "bash", "input": {"command": "echo tracer-oc-steps-2", "description": "echo"}},
 {"text": "oc steps done"}]}
J
python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" ocs "$RUN/rules.json" "$RUN/steps-cap.json"
stop_mock; start_mock "$RUN/rules.json"
INLINE=$(echo "$INLINE" | sed "s|http://127.0.0.1:[0-9]*/v1|$MOCK_URL/v1|g")
sed -i '' "s|http://127.0.0.1:[0-9]*/v1|$MOCK_URL/v1|g" "$H/.config/opencode/opencode.json" "$R/.opencode/opencode.json"
oc_bounded() { # <seconds> <out> <args>: oc with a time bound (a stuck run is stopped)
  s=$1 o=$2; shift 2
  oc "$@" >"$o" 2>&1 &
  p=$!; i=0
  while kill -0 $p 2>/dev/null && [ $i -lt $s ]; do sleep 1; i=$((i + 1)); done
  pkill -P $p 2>/dev/null || true; kill $p 2>/dev/null || true; wait $p 2>/dev/null || true
}
oc_bounded 90 "$RUN/run.out" run --print-logs "tracer-oc-run use bash once"
oc_bounded 60 "$RUN/run-cmd.out" run --print-logs --command zygo-cmd "tracer-oc-cmd-args"
oc_bounded 60 "$RUN/run-steps.out" run --print-logs --agent zygo-steps "tracer-oc-steps run two commands"
stop_mock
publish_sse "$FX/sse"

python3 -I - "$RUN" "$PREMISES_DIR" >"$RUN/result.json" <<'PY'
import json, os, re, sys
run = sys.argv[1]
cfg = json.load(open(run + "/debug-config.json"))
recs = [json.load(open(os.path.join(run, "rec", f))) for f in sorted(os.listdir(run + "/rec")) if f[0].isdigit()]
posts = [r for r in recs if r["method"] == "POST" and isinstance(r.get("body"), dict)]
def has(r, s): return s in json.dumps(r["body"])
main = [r for r in posts if r["body"].get("tools")]
z = cfg.get("provider", {}).get("zygo", {})
prec = {
 "model": cfg.get("model"),
 "provider.zygo.options.headers": z.get("options", {}).get("headers"),
 "provider.zygo.models (keys)": sorted(z.get("models", {})),
 "provider.zygo.models.lead.limit.context": z.get("models", {}).get("lead", {}).get("limit", {}).get("context"),
 "agent.build": cfg.get("agent", {}).get("build"),
 "permission": cfg.get("permission"),
 "mcp (keys)": sorted(cfg.get("mcp", {})),
 "mcp.zygo.command (project adds a \"project\" argument)": cfg.get("mcp", {}).get("zygo", {}).get("command"),
}
layers = {"user": {"model": "zygo/user-model", "context": 111111, "edit": "allow"},
          "project": {"model": "zygo/project-model", "context": 222222, "edit": "ask"},
          "inline": {"model": "zygo/lead", "context": 333333, "edit": "deny"}}
def which(v, key):
    return [n for n, l in layers.items() if l[key] == v]
perm = cfg.get("permission")
pv = perm.get("edit") if isinstance(perm, dict) else perm
winner = {
 "model": which(cfg.get("model"), "model"),
 "provider.zygo.models.lead.limit.context": which(prec["provider.zygo.models.lead.limit.context"], "context"),
 "agent.build.model": which((cfg.get("agent", {}).get("build") or {}).get("model"), "model"),
 "permission.edit": which(pv, "edit"),
}
result = {
 "P4 config precedence (opencode debug config)": {"resolved": prec, "winner_per_key": winner,
   "objects": {"provider.zygo.options.headers": "merged" if len((prec["provider.zygo.options.headers"] or {})) > 2 else "replaced or single",
               "mcp": "merged" if len(prec["mcp (keys)"]) > 1 else "replaced or single",
               "permission": "merged" if isinstance(perm, dict) and len(perm) > 3 else "replaced or single"}},
 "run requests": [{"seq": r["seq"], "model": r["body"].get("model"), "x-layer": r["headers"].get("x-layer"),
                   "x-layer-headers": sorted(k for k in r["headers"] if k.startswith("x-layer-")),
                   "tools": len(r["body"].get("tools") or []), "reply": r["reply"], "messages": len(r["body"].get("messages") or []),
                   "system has plugin context text": "tracer-oc-session-context" in json.dumps(r["body"].get("system")),
                   "max_tokens": r["body"].get("max_tokens")} for r in posts],
 "P5 loading": {
   "agent list has zygo-code": "zygo-code" in open(run + "/agent-list.txt").read(),
   "mcp list": [l.strip() for l in open(run + "/mcp-list.txt").read().splitlines() if l.strip() and ("mcp" in l or "zygo" in l)][:10],
   "debug skill has zygo-probe-skill": "zygo-probe-skill" in open(run + "/debug-skill.txt").read(),
   "debug config command keys": sorted(cfg.get("command", {})),
   "debug config agent keys": sorted(cfg.get("agent", {})),
   "debug config plugin": cfg.get("plugin"),
   "run: tool names in main request": sorted({t.get("name") for r in main for t in r["body"].get("tools") or []}),
   "run: zygo-code named in a main request": any(has(r, "tracer-oc-agent-description") for r in main),
   "run: skill named in a main request": any(has(r, "zygo-probe-skill") for r in main),
   "run --command zygo-cmd: expanded template seen": any(has(r, "tracer-oc-command body: tracer-oc-cmd-args") for r in posts),
   "plugin loaded (plugin-init logged)": os.path.exists(run + "/plugin-log/plugin-init.json"),
 },
 "A-20 turn cap (agent steps = 2, two tool calls scripted)": [{"seq": r["seq"], "reply": r["reply"], "tools": len(r["body"].get("tools") or []),
     "tool_choice": r["body"].get("tool_choice"),
     "last message": json.dumps(r["body"]["messages"][-1])[:300]} for r in posts if "tracer-oc-steps" in json.dumps(r["body"]) and "title generator" not in json.dumps(r["body"].get("system") or "")[:200]],
 "plugin hooks": {
   "logged": sorted(f[:-5] for f in os.listdir(run + "/plugin-log")) if os.path.isdir(run + "/plugin-log") else [],
   "tool.execute.after replacement seen in next request": any(has(r, "OC-REPLACED-bash") for r in posts),
   "original bash output seen in a request": any(has(r, "tracer-oc-bash-original\\n") or has(r, "tracer-oc-bash-original\"") for r in posts if not has(r, "\"command\": \"echo tracer-oc-bash-original")),
   "session context text in system": any("tracer-oc-session-context" in json.dumps(r["body"].get("system")) for r in posts),
   "pre-compaction hook text (output.context) seen in the compaction request": any("tracer-oc-compaction-context" in json.dumps(r["body"]) and "summarization assistant" in json.dumps(r["body"].get("system") or "")[:400] for r in posts),
   "compaction requests (summarization system prompt) after a reply whose usage is over limit.context": [r["seq"] for r in posts if "summarization assistant" in json.dumps(r["body"].get("system") or "")[:400]],
 },
}
print(json.dumps(result, indent=1))
PY
scrub "$RUN/result.json" >"$RUN/result-scrubbed.json"
python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); json.dump(d["P4 config precedence (opencode debug config)"], open(sys.argv[2],"w"), indent=1)' "$RUN/result-scrubbed.json" "$FX/config-precedence.json"
python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); json.dump({k: d[k] for k in ("P5 loading", "plugin hooks", "A-20 turn cap (agent steps = 2, two tool calls scripted)", "run requests")}, open(sys.argv[2],"w"), indent=1)' "$RUN/result-scrubbed.json" "$FX/load-result.json"
scrub "$RUN/debug-config.json" | sed "s/$DUMMY_KEY/[omitted]/g" >"$FX/debug-config.json"
for f in agent-list.txt mcp-list.txt debug-skill.txt; do scrub "$RUN/$f" >"$FX/$f"; done
scrub "$RUN/inline.json" | sed "s/$DUMMY_KEY/[omitted]/g" >"$FX/inline-config.json"
[ -d "$RUN/plugin-log" ] && for f in "$RUN"/plugin-log/*.json; do scrub "$f" >"$FX/plugin-hooks/$(basename "$f")"; done
for f in "$RUN"/rec/[0-9]*.json; do
  if jq -e '.body.tools' "$f" >/dev/null 2>&1; then scrub "$f" >"$FX/requests/run-$(basename "$f")"; fi
done
# Q7 (OpenCode): each key zygo writes in the config schema; OPENCODE_* env names in the binary.
URL=https://opencode.ai/config.json
curl -sSL --max-time 30 -o "$RUN/oc-schema.json" "$URL"
python3 -I - "$RUN/oc-schema.json" "$URL" "$(date +%Y-%m-%d)" "$FX/config-schema.json" >"$RUN/q7-oc.json" <<'PY'
import json, sys
s = json.load(open(sys.argv[1]))
json.dump({"source": sys.argv[2], "retrieved": sys.argv[3], "schema": s}, open(sys.argv[4], "w"), indent=1)
defs = s.get("$defs") or s.get("definitions") or {}
def find(obj, key, path="", out=None):
    out = [] if out is None else out
    if isinstance(obj, dict):
        for k, v in obj.items():
            if k == key:
                out.append(path + "/" + k)
            find(v, key, path + "/" + k, out)
    elif isinstance(obj, list):
        for i, v in enumerate(obj):
            find(v, key, path + "/" + str(i), out)
    return out
props = s.get("properties", {})
res = {k: k in props for k in ("provider", "model", "agent", "permission", "mcp")}
res["limit.context (paths in schema)"] = [p for p in find(s, "context") if "/limit/" in p][:5]
res["agent mode/prompt/model/permission in AgentConfig"] = sorted(k for k in ("mode", "prompt", "model", "permission", "description") if k in (defs.get("AgentConfig", {}).get("properties") or {}))
res["McpLocalConfig keys"] = sorted((defs.get("McpLocalConfig", {}).get("properties") or {}).keys())
res["provider npm/options in ProviderConfig"] = sorted(k for k in ("npm", "options", "models") if k in (defs.get("ProviderConfig", {}).get("properties") or {}))
print(json.dumps(res, indent=1))
PY
B=$(python3 -I -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$OPENCODE_BIN")
strings -n 6 "$B" | grep -oE 'OPENCODE_CONFIG_(CONTENT|DIR)|trigger\("(tool\.execute\.(before|after)|experimental\.chat\.system\.transform|experimental\.session\.compacting)"' | sort | uniq -c >"$RUN/oc-binary-names.txt"
{ echo "# OpenCode $("$OPENCODE_BIN" --version) executable: env names and plugin hook triggers (strings | grep -o | uniq -c)"; cat "$RUN/oc-binary-names.txt"; } >"$FX/binary-names.txt"
scrub "$RUN/q7-oc.json" >"$FX/q7-opencode-schema.json"
cat "$FX/q7-opencode-schema.json" "$FX/binary-names.txt"
cat "$RUN/result-scrubbed.json"
echo "opencode done"

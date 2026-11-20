#!/bin/sh
# P3 (hook agent_id vs x-claude-code-agent-id) and the hook / tool-output fixtures (Q4 shapes):
# the main loop and a subagent each call Bash, Read (text, image, PDF, notebook), Grep, Glob,
# WebFetch, Write, Edit, NotebookEdit and a failing Read; each then gets a full-window usage
# so PreCompact fires for both. Every hook event logs its stdin.
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/claude-code"
mkdir -p "$FX/sse" "$FX/requests" "$FX/hooks" "$FX/tool-output"
new_run P3
rm -rf "$FX/hooks" "$FX/tool-output"; mkdir -p "$FX/hooks" "$FX/tool-output"
python3 -I "$PREMISES_DIR/make_files.py" "$R"
git -C "$R" add -A && git -C "$R" -c user.name=t -c user.email=t@t commit -qm files
start_web "$R"
FULL='{"input_tokens":1000,"cache_creation_input_tokens":0,"cache_read_input_tokens":999000,"output_tokens":5}'
steps() { # <who> <trigger> <last step json>
  cat <<J
{"trigger": "$2", $( [ "$1" = sub ] && echo '"header": {"x-claude-code-agent-id": "."},' || echo '"no_header": ["x-claude-code-agent-id"],' )
 "steps": [
  {"tool": "Bash", "input": {"command": "echo tracer-bash-out-$1"}},
  {"tool": "Read", "input": {"file_path": "$R/README.txt"}},
  {"tool": "Read", "input": {"file_path": "$R/img.png"}},
  {"tool": "Read", "input": {"file_path": "$R/doc.pdf"}},
  {"tool": "Read", "input": {"file_path": "$R/nb.ipynb"}},
  {"tool": "Grep", "input": {"pattern": "tracer", "path": "$R"}},
  {"tool": "Glob", "input": {"pattern": "*.txt", "path": "$R"}},
  {"tool": "WebFetch", "input": {"url": "$WEB_URL/page.html", "prompt": "Give the heading."}},
  {"tool": "Write", "input": {"file_path": "$R/new-$1.txt", "content": "alpha\\n"}},
  {"tool": "Edit", "input": {"file_path": "$R/new-$1.txt", "old_string": "alpha", "new_string": "beta"}},
  {"tool": "NotebookEdit", "input": {"notebook_path": "$R/nb.ipynb", "cell_id": "c1", "new_source": "print('tracer $1')", "edit_mode": "replace"}},
  {"tool": "Read", "input": {"file_path": "$R/missing-$1.txt"}},
  $3
 ]}
J
}
steps main "tracer-p3-main" '{"tool": "Agent", "input": {"description": "p3 subagent", "prompt": "tracer-p3-sub run the tools", "subagent_type": "claude", "run_in_background": false}},
  {"text": "p3 main full window", "usage": '"$FULL"', "iterations": true}' >"$RUN/steps-main.json"
steps sub "tracer-p3-sub" '{"tool": "Bash", "input": {"command": "echo tracer-sub-last"}, "usage": '"$FULL"', "iterations": true}' >"$RUN/steps-sub.json"
mkdir -p "$RUN/sse"
python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" p3-sub "$RUN/rules.json" "$RUN/steps-sub.json"
python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" p3-main "$RUN/rules.json" "$RUN/steps-main.json"
hook_settings "$RUN/hooks" >"$RUN/settings.json"
printf '%s\n' "tracer-p3-main run the tools" "tracer-p3-second turn" >"$RUN/msgs"
start_mock "$RUN/rules.json"
run_claude_stream "$RUN/out.jsonl" "$RUN/msgs" --settings "$RUN/settings.json" --dangerously-skip-permissions \
  --tools "Bash,Read,Grep,Glob,WebFetch,Write,Edit,NotebookEdit,Agent"
stop_mock
publish_sse "$FX/sse"
stop_web

# Fixtures: first hook input per event / tool (Read split by file kind) / main|sub.
python3 -I - "$RUN/hooks" "$RUN/fx-hooks" "$RUN/fx-tool-output" <<'PY'
import json, os, sys
src, hd, td = sys.argv[1:4]
os.makedirs(hd, exist_ok=True); os.makedirs(td, exist_ok=True)
kinds = {".png": "image", ".pdf": "pdf", ".ipynb": "notebook"}
seen = set()
for f in sorted(os.listdir(src)):
    if not (f[0].isdigit() and f.endswith(".json")):
        continue
    d = json.load(open(os.path.join(src, f)))
    ev, tool = d.get("hook_event_name"), d.get("tool_name")
    who = "sub" if "agent_id" in d else "main"
    name = ev + ("-" + tool if tool else "")
    if "agent_id" in d and not d.get("agent_type"):
        name += "-internal"  # Claude Code's own agents (compaction), not a dispatched subagent
    if tool == "Read":
        k = kinds.get(os.path.splitext((d.get("tool_input") or {}).get("file_path", ""))[1])
        if k:
            name += "-" + k
    key = name + "-" + who
    if key in seen:
        continue
    seen.add(key)
    json.dump(d, open(os.path.join(hd, key + ".json"), "w"), indent=2)
    if ev == "PostToolUse" and who == "main":
        out = name.split("-", 1)[1]
        json.dump(d.get("tool_response"), open(os.path.join(td, out + ".json"), "w"), indent=2)
PY
for f in "$RUN"/fx-hooks/*.json; do scrub "$f" >"$FX/hooks/$(basename "$f")"; done
for f in "$RUN"/fx-tool-output/*.json; do scrub "$f" >"$FX/tool-output/$(basename "$f")"; done
python3 -I "$PREMISES_DIR/summarize.py" "$RUN/rec" >"$RUN/summary.json"
scrub "$RUN/summary.json" >"$FX/requests/p3-run.summary.json"

# P3 comparison: hook agent_id values vs request header values.
python3 -I - "$RUN/hooks" "$RUN/rec" >"$RUN/p3-compare.json" <<'PY'
import json, os, sys
hd, rd = sys.argv[1:3]
hook_ids, hook_main, internal = set(), 0, set()
for f in sorted(os.listdir(hd)):
    if f[0].isdigit() and f.endswith(".json"):
        d = json.load(open(os.path.join(hd, f)))
        if "agent_id" in d and d.get("agent_type"):
            hook_ids.add(d["agent_id"])
        elif "agent_id" in d:
            internal.add((d["hook_event_name"], d["agent_id"]))
        else:
            hook_main += 1
hdr_ids, main_reqs, main_with_id, sub_reqs = set(), 0, 0, 0
for f in sorted(os.listdir(rd)):
    if f[0].isdigit() and f.endswith(".json"):
        r = json.load(open(os.path.join(rd, f)))
        h = r["headers"]
        cls = h.get("x-claude-code-request-class")
        if "x-claude-code-agent-id" in h:
            hdr_ids.add(h["x-claude-code-agent-id"]); sub_reqs += 1
            if cls == "main":
                main_with_id += 1
        elif cls == "main":
            main_reqs += 1
print(json.dumps({"hook_agent_ids": sorted(hook_ids), "header_agent_ids": sorted(hdr_ids),
                  "equal": hook_ids == hdr_ids and len(hdr_ids) == 1,
                  "subagent_requests_with_header": sub_reqs, "main_requests": main_reqs,
                  "main_requests_with_agent_id_header": main_with_id,
                  "hook_inputs_without_agent_id": hook_main,
                  "hook_inputs_with_agent_id_and_empty_agent_type": sorted(internal),
                  "empty_agent_type_ids_seen_in_request_headers": sorted(i for _, i in internal if i in hdr_ids)}, indent=1))
PY
cp "$RUN/p3-compare.json" "$FX/requests/p3-compare.json"
cat "$RUN/p3-compare.json"
ls "$FX/hooks" "$FX/tool-output"
echo "P3 done"

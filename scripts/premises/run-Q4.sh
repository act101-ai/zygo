#!/bin/sh
# Q4: updatedToolOutput per hooked tool (Bash, Read, Grep, Glob, WebFetch). A PostToolUse hook
# returns the tool's own tool_response with its text field replaced; verified per tool when the
# next captured request shows the replaced text in that tool's tool_result.
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/claude-code"
mkdir -p "$FX/sse" "$FX/requests" "$FX/hooks" "$FX/tool-output"
new_run Q4
python3 -I "$PREMISES_DIR/make_files.py" "$R"
cp "$R/README.txt" "$R/README2.txt"
start_web "$R"
cat >"$RUN/steps.json" <<J
{"trigger": "tracer-q4", "steps": [
 {"tool": "Bash", "input": {"command": "echo tracer-q4-original"}},
 {"tool": "Read", "input": {"file_path": "$R/README.txt"}},
 {"tool": "Read", "input": {"file_path": "$R/README2.txt"}},
 {"tool": "Grep", "input": {"pattern": "tracer", "path": "$R"}},
 {"tool": "Grep", "input": {"pattern": "readme", "path": "$R"}},
 {"tool": "Grep", "input": {"pattern": "tracer", "path": "$R/README.txt", "output_mode": "content"}},
 {"tool": "Glob", "input": {"pattern": "*.txt", "path": "$R"}},
 {"tool": "WebFetch", "input": {"url": "$WEB_URL/page.html", "prompt": "Give the heading."}},
 {"text": "q4 done"}]}
J
mkdir -p "$RUN/sse"
python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" q4 "$RUN/rules.json" "$RUN/steps.json"
cat >"$RUN/responses.json" <<'J'
[{"event": "PostToolUse", "tool": "^Bash$", "rewrite": [[["stdout"], "Q4-REPLACED-Bash"]]},
 {"event": "PostToolUse", "tool": "^Read$", "times": 1, "case": "Read, text only (counts left stale)",
  "rewrite": [[["file", "content"], "Q4-REPLACED-Read line one\nQ4-REPLACED-Read line two"]]},
 {"event": "PostToolUse", "tool": "^Read$", "times": 1, "case": "Read, text and counts",
  "rewrite": [[["file", "content"], "Q4-REPLACED-Read line one\nQ4-REPLACED-Read line two"], [["file", "numLines"], 2], [["file", "totalLines"], 2]]},
 {"event": "PostToolUse", "tool": "^Grep$", "times": 1, "case": "Grep files, names only (numFiles left stale)",
  "rewrite": [[["filenames"], ["Q4-REPLACED-Grep-files.txt"]]]},
 {"event": "PostToolUse", "tool": "^Grep$", "times": 1, "case": "Grep files, names and numFiles",
  "rewrite": [[["filenames"], ["Q4-REPLACED-Grep-files.txt"]], [["numFiles"], 1]]},
 {"event": "PostToolUse", "tool": "^Grep$", "times": 1, "case": "Grep content, text and numLines",
  "rewrite": [[["content"], "Q4-REPLACED-Grep-content"], [["numLines"], 1]]},
 {"event": "PostToolUse", "tool": "^Glob$", "case": "Glob, names and numFiles",
  "rewrite": [[["filenames"], ["Q4-REPLACED-Glob.txt"]], [["numFiles"], 1]]},
 {"event": "PostToolUse", "tool": "^WebFetch$", "rewrite": [[["result"], "Q4-REPLACED-WebFetch"]]}]
J
hook_settings "$RUN/hooks" "$RUN/responses.json" >"$RUN/settings.json"
start_mock "$RUN/rules.json"
run_claude -p "tracer-q4 run the tools" --settings "$RUN/settings.json" --dangerously-skip-permissions \
  --tools "Bash,Read,Grep,Glob,WebFetch" </dev/null >"$RUN/out.txt" 2>&1 || true
stop_mock
publish_sse "$FX/sse"
stop_web
# Result per tool: the tool_result the model got next, and whether it holds the replacement.
python3 -I - "$RUN/rec" "$RUN/hooks" >"$RUN/q4-result.json" <<'PY'
import json, os, sys
rd, hd = sys.argv[1:3]
reqs = [json.load(open(os.path.join(rd, f))) for f in sorted(os.listdir(rd)) if f[0].isdigit()]
outs = {}
for f in sorted(os.listdir(hd)):
    if f.endswith(".out"):
        hin = json.load(open(os.path.join(hd, f[:-4] + ".json")))
        outs[hin["tool_use_id"]] = (hin["tool_name"], json.load(open(os.path.join(hd, f))))
res = []
for tid, (tool, out) in outs.items():
    seen = None
    for r in reqs:
        for m in (r.get("body") or {}).get("messages") or []:
            if isinstance(m.get("content"), list):
                for b in m["content"]:
                    if isinstance(b, dict) and b.get("type") == "tool_result" and b.get("tool_use_id") == tid:
                        seen = b.get("content")
        if seen is not None:
            break
    res.append({"tool": tool, "tool_use_id": tid,
                "hook_output": out,
                "tool_result_seen_by_model": seen,
                "replaced": "Q4-REPLACED" in json.dumps(seen)})
print(json.dumps(res, indent=1))
PY
scrub "$RUN/q4-result.json" >"$FX/requests/q4-result.json"
for h in "$RUN"/hooks/*PostToolUse-Grep*.json; do
  m=$(jq -r '.tool_input.output_mode // "files"' "$h")
  [ "$m" = content ] && scrub "$h" >"$FX/hooks/PostToolUse-Grep-content-main.json" && jq '.tool_response' "$h" >"$RUN/grep-content.json" && scrub "$RUN/grep-content.json" >"$FX/tool-output/Grep-content.json"
done
grep -l 'hook_error_during_execution\|does not match' "$RUN/out.txt" || true
jq -c '.[] | [.tool, .replaced, .tool_result_seen_by_model]' "$FX/requests/q4-result.json" | cut -c1-300
echo "Q4 done"

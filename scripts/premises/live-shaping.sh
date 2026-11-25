#!/bin/sh
# Live premises PO1, PO2, PO3 and PO4 (node OS-22; output-shaping spec O§17): what the pinned Claude Code
# and OpenCode give a hook for a failed and a passing Bash call, what the model then sees, and how much
# text an agent adds around a result_view reply.
# Usage: sh scripts/premises/live-shaping.sh po1|po2|po3|po4
#   po1 (Claude Code): a Bash call that prints out-line and err-line and exits 3, three times; the
#     PostToolUseFailure hook answers nothing, updatedToolOutput as a string, then as a Bash-shaped object.
#     Writes fixtures/claude-code/hooks/PostToolUseFailure-Bash-output-main.json,
#     fixtures/claude-code/requests/po1-result.json and po1-request-<none|string|object>.json.
#   po2 (Claude Code): the same lines with exit 0; a PostToolUse rewrite sets stdout to a tracer and stderr
#     to "". Writes fixtures/claude-code/hooks/PostToolUse-Bash-stderr-main.json,
#     fixtures/claude-code/requests/po2-result.json and po2-request.json.
#   po3 (OpenCode): the same lines with exit 0; a probe plugin records tool.execute.after. Writes
#     fixtures/opencode/plugin-hooks/tool.execute.after-bash-stderr.json and
#     fixtures/opencode/requests/po3-request.json.
#   po4 (both agents): two result_view calls (lines 1-400 and 1-40) to a stdio MCP server named zygo.
#     Writes fixtures/claude-code/requests/po4-readback.json, fixtures/opencode/requests/po4-readback.json
#     and po4-request-<lines>.json beside each.
#   po1, po2 and po3 also write their scripted SSE replies under fixtures/<agent>/sse. Every file is
#   scrubbed (scripts/premises/scrub.py) and each path written is printed.
# Run by hand only, by the operator, with claude and opencode on PATH; never from a test or CI (O-C7).
#
# What this run does to the machine: a temporary HOME and config folder in the scratch folder, the dummy
# key and the mock upstream of lib.sh: no login is read, no model request leaves the machine, nothing
# under the real HOME is written. OpenCode fetches its provider package from the package registry into
# the temporary HOME.
case "$#:${1:-}" in
  1:po1 | 1:po2 | 1:po3 | 1:po4) MODE=$1 ;;
  *) echo "usage: live-shaping.sh po1|po2|po3|po4"; exit 2 ;;
esac
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps
for _t in jq python3; do command -v "$_t" >/dev/null || { echo "live: $_t is not on PATH"; exit 1; }; done
pinned_agents
# hook_settings puts PREMISES_DIR unquoted into the hook command; LIVE_VARS words take no space.
case "$N2_SCRATCH$CLAUDE_BIN$OPENCODE_BIN$TOOL_PATH$PREMISES_DIR" in
  *"'"* | *" "*) echo "live: a path holds a single quote or a space; this script cannot use it"; exit 1 ;;
esac
FXC="$REPO_DIR/fixtures/claude-code"
FXO="$REPO_DIR/fixtures/opencode"
mkdir -p "$FXC/hooks" "$FXC/requests" "$FXC/sse" "$FXO/plugin-hooks" "$FXO/requests" "$FXO/sse"
echo "live: Claude Code $LIVE_CC_VER, OpenCode $LIVE_OC_VER"

# --- helpers --------------------------------------------------------------------------------
# steps <prefix> <steps json>: SSE files and mock rules of one scripted sequence (steps.py)
steps() {
  mkdir -p "$RUN/sse"
  printf '%s\n' "$2" >"$RUN/steps-$1.json"
  python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" "$1" "$RUN/rules.json" "$RUN/steps-$1.json"
}
# wrote <src> <dest>: a scrubbed copy of <src> at <dest>, and its path; a missing <src> writes nothing
wrote() {
  if [ -z "$1" ] || [ ! -f "$1" ]; then echo "live: no ${1##*/}"; return 0; fi
  scrub "$1" >"$2"
  echo "wrote ${2#"$REPO_DIR"/}"
}
# hook_files <dir> <name>: the hook input files <dir>/<nnnn>-<name>.json, in name order
hook_files() { ls "$1" 2>/dev/null | grep -E "^[0-9]+-$2\\.json\$" | sed "s|^|$1/|" || true; }
# seen_json <out> <tool_use_id>...: per id, the first recorded request that holds its tool_result block
seen_json() {
  python3 -I - "$RUN/rec" "$@" <<'PY'
import glob, json, os, sys
rd, out, ids = sys.argv[1], sys.argv[2], sys.argv[3:]
recs = []
for p in sorted(glob.glob(os.path.join(rd, "[0-9]*.json"))):
    try:
        recs.append((p, json.load(open(p))))
    except (OSError, ValueError):
        continue

def block(r, tid):
    body = r.get("body") if isinstance(r, dict) else None
    msgs = body.get("messages") if isinstance(body, dict) else None
    for m in msgs if isinstance(msgs, list) else []:
        c = m.get("content") if isinstance(m, dict) else None
        for b in c if isinstance(c, list) else []:
            if isinstance(b, dict) and b.get("type") == "tool_result" and b.get("tool_use_id") == tid:
                return b
    return None

res = []
for tid in ids:
    o = {"tool_use_id": tid, "request_file": None, "is_error": None, "tool_result_seen_by_model": None, "text": None}
    for p, r in recs:
        b = block(r, tid)
        if b is None:
            continue
        c = b.get("content")
        if isinstance(c, str):
            t = c
        else:
            t = "\n".join(i.get("text", "") for i in (c if isinstance(c, list) else [])
                          if isinstance(i, dict) and i.get("type") == "text")
        o.update(request_file=p, is_error=b.get("is_error"), tool_result_seen_by_model=c, text=t)
        break
    res.append(o)
json.dump(res, open(out, "w"), indent=1)
PY
}

# --- OpenCode helpers -----------------------------------------------------------------------
# oc_setup [<mcp json>]: the probe plugin and opencode.json (provider on the mock) in the run's HOME
oc_setup() {
  _ocd="$H/.config/opencode"
  mkdir -p "$_ocd/plugins"
  cat >"$_ocd/plugins/po-probe.js" <<'JS'
// Probe plugin of live-shaping.sh (node OS-22): on tool.execute.after, writes {input, output} (deep
// copies, before any change) to $N2_OC_LOG/tool.execute.after-<tool>.json; changes nothing.
import { mkdirSync, writeFileSync } from "node:fs";

const dir = process.env.N2_OC_LOG;
const copy = (v) => JSON.parse(JSON.stringify(v ?? null));
export const PoProbe = async () => ({
  "tool.execute.after": async (input, output) => {
    const rec = { input: copy(input), output: copy(output) };
    try {
      mkdirSync(dir, { recursive: true });
      writeFileSync(`${dir}/tool.execute.after-${input.tool}.json`, JSON.stringify(rec, null, 2));
    } catch {}
  },
});
JS
  jq -n --arg u "$MOCK_URL/v1" --arg k "$DUMMY_KEY" --argjson m "${1:-null}" '{model: "zygo/lead",
    provider: {zygo: {npm: "@ai-sdk/anthropic", options: {baseURL: $u, apiKey: $k}, models: {lead: {limit: {context: 200000, output: 4000}}}}},
    permission: {edit: "deny", bash: "allow", task: "deny"}} + (if $m == null then {} else {mcp: $m} end)' >"$_ocd/opencode.json"
}
# oc_run <prompt>: one opencode run in the isolated environment, bounded to 120 s
oc_run() {
  LIVE_TIMEOUT=120 LIVE_VARS="N2_OC_LOG=$RUN/plog" live_exec "$OPENCODE_BIN" run --print-logs "$1" </dev/null >"$RUN/oc.out" 2>&1 || true
  check_abort
}

# The Bash commands of the runs: out-line on stdout, err-line on stderr; PO1 exits 3, the others 0.
CMD_FAIL="printf 'out-line\\n'; printf 'err-line\\n' >&2; exit 3"
CMD_BOTH="printf 'out-line\\n'; printf 'err-line\\n' >&2"

# --- PO1 (Claude Code): the PostToolUseFailure input of a failed Bash call, and what replaces its text --
part_po1() {
  live_run po1
  steps po1 "$(jq -nc --arg c "$CMD_FAIL" '{trigger: "tracer-po1",
    steps: ([range(3) | {tool: "Bash", input: {command: $c}}] + [{text: "po1 done"}])}')"
  cat >"$RUN/responses.json" <<'J'
[{"event": "PostToolUseFailure", "tool": "^Bash$", "times": 1, "case": "none", "output": {}},
 {"event": "PostToolUseFailure", "tool": "^Bash$", "times": 1, "case": "string",
  "output": {"hookSpecificOutput": {"hookEventName": "PostToolUseFailure", "updatedToolOutput": "PO1-REPLACED-string"}}},
 {"event": "PostToolUseFailure", "tool": "^Bash$", "times": 1, "case": "object",
  "output": {"hookSpecificOutput": {"hookEventName": "PostToolUseFailure", "updatedToolOutput": {"stdout": "PO1-REPLACED-object", "stderr": "", "interrupted": false, "isImage": false, "noOutputExpected": false}}}}]
J
  hook_settings "$RUN/hooks" "$RUN/responses.json" >"$RUN/settings.json"
  start_mock "$RUN/rules.json"
  run_claude -p "tracer-po1 run the command" --settings "$RUN/settings.json" --dangerously-skip-permissions \
    --tools Bash </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock
  publish_sse "$FXC/sse"
  _hk=$(hook_files "$RUN/hooks" PostToolUseFailure-Bash-main)
  wrote "$(printf '%s\n' "$_hk" | sed -n 1p)" "$FXC/hooks/PostToolUseFailure-Bash-output-main.json"
  _ids=$(for _f in $_hk; do jq -r '.tool_use_id // empty' "$_f" 2>/dev/null || true; done)
  # shellcheck disable=SC2086
  seen_json "$RUN/po1-seen.json" $_ids
  # shellcheck disable=SC2086
  python3 -I - "$RUN/po1-seen.json" "$RUN/po1-result.json" $_hk <<'PY'
import json, sys
seen_f, out, hooks = sys.argv[1], sys.argv[2], sys.argv[3:]
cases = ["none", "string", "object"]  # the rules of responses.json, one use each, in order

def load(p):
    try:
        return json.load(open(p))
    except (OSError, ValueError):
        return None

seen = {s.get("tool_use_id"): s for s in (load(seen_f) or []) if isinstance(s, dict)}
calls = []
for i, h in enumerate(hooks):
    inp = load(h) or {}
    s = seen.get(inp.get("tool_use_id"), {})
    c = {"case": cases[i] if i < len(cases) else "extra-%d" % i, "tool_use_id": inp.get("tool_use_id"),
         "hook_output": load(h[:-5] + ".out")}
    for k in ("request_file", "is_error", "tool_result_seen_by_model", "text"):
        c[k] = s.get(k)
    c["replaced"] = "PO1-REPLACED" in (c["text"] or "")
    calls.append(c)
err = (load(hooks[0]) or {}).get("error") if hooks else None
es = err if isinstance(err, str) else ("" if err is None else json.dumps(err))
res = {"error_field": err, "out_line_at": es.find("out-line"), "err_line_at": es.find("err-line"), "calls": calls}
json.dump(res, open(out, "w"), indent=1)
PY
  wrote "$RUN/po1-result.json" "$FXC/requests/po1-result.json"
  jq -r '.calls[] | "\(.case) \(.request_file // "")"' "$RUN/po1-result.json" | while read -r _c _f; do
    if [ -n "$_f" ]; then wrote "$_f" "$FXC/requests/po1-request-$_c.json" </dev/null; fi
  done
  _v=$(jq -r '(.out_line_at >= 0 and .err_line_at >= 0) as $both
    | ([.calls[] | select(.replaced) | .case] | join(", ")) as $r
    | (if $both and $r != "" then "pass" else "fail" end)
      + " error field: " + (if $both then (if .out_line_at < .err_line_at then "out-line before err-line" else "err-line before out-line" end)
        else "out-line at \(.out_line_at), err-line at \(.err_line_at)" end)
      + "; cases that replace the text the model sees: " + (if $r == "" then "none" else $r end)
      + "; failed calls hooked: \(.calls | length)"' "$RUN/po1-result.json")
  result PO1 "${_v%% *}" "${_v#* }"
}

# --- PO2 (Claude Code): updatedToolOutput with stderr "" removes the stderr the model sees ----------
part_po2() {
  live_run po2
  steps po2 "$(jq -nc --arg c "$CMD_BOTH" '{trigger: "tracer-po2",
    steps: [{tool: "Bash", input: {command: $c}}, {text: "po2 done"}]}')"
  cat >"$RUN/responses.json" <<'J'
[{"event": "PostToolUse", "tool": "^Bash$", "rewrite": [[["stdout"], "PO2-REPLACED-stdout"], [["stderr"], ""]]}]
J
  hook_settings "$RUN/hooks" "$RUN/responses.json" >"$RUN/settings.json"
  start_mock "$RUN/rules.json"
  run_claude -p "tracer-po2 run the command" --settings "$RUN/settings.json" --dangerously-skip-permissions \
    --tools Bash </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock
  publish_sse "$FXC/sse"
  _hk=$(hook_files "$RUN/hooks" PostToolUse-Bash-main | sed -n 1p)
  wrote "$_hk" "$FXC/hooks/PostToolUse-Bash-stderr-main.json"
  seen_json "$RUN/po2-seen.json" toolu_po2_00_bash
  python3 -I - "$RUN/po2-seen.json" "$RUN/po2-result.json" "$_hk" <<'PY'
import json, sys
seen_f, out, hook = sys.argv[1:4]

def load(p):
    try:
        return json.load(open(p))
    except (OSError, ValueError):
        return None

seen = load(seen_f) or []
s = seen[0] if seen and isinstance(seen[0], dict) else {}
res = {"hook_output": load(hook[:-5] + ".out") if hook else None}
for k in ("tool_use_id", "request_file", "is_error", "tool_result_seen_by_model", "text"):
    res[k] = s.get(k)
res["replaced"] = "PO2-REPLACED-stdout" in (res["text"] or "")
res["err_line_seen"] = "err-line" in (res["text"] or "")
json.dump(res, open(out, "w"), indent=1)
PY
  wrote "$RUN/po2-result.json" "$FXC/requests/po2-result.json"
  wrote "$(jq -r '.request_file // ""' "$RUN/po2-result.json")" "$FXC/requests/po2-request.json"
  _v=$(jq -r '(if .replaced and (.err_line_seen | not) then "pass" else "fail" end)
    + " the model saw the stdout tracer: \(if .replaced then "yes" else "no" end); the model saw err-line: \(if .err_line_seen then "yes" else "no" end)"' "$RUN/po2-result.json")
  result PO2 "${_v%% *}" "${_v#* }"
}

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

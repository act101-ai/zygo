#!/bin/sh
# Live premises PO1, PO2, PO3 and PO4 (node OS-22; output-shaping spec O§17): what the pinned Claude Code
# and OpenCode give a hook for a failed and a passing Bash call, what the model then sees, and how much
# text an agent adds around a result_view reply.
# Usage: sh scripts/premises/live-shaping.sh po1|po2|po3|po4
#   po1 (Claude Code): three Bash calls that print out-line and err-line and exit 3; the
#     PostToolUseFailure hook answers nothing, updatedToolOutput as a string, then as a Bash-shaped object.
#     Then a 4th Bash call, `cd scratch && cargo test`, on a crate with 2 failing tests; the hook answers
#     nothing, and the LIVE PO1 line reports its outcome. Writes
#     fixtures/claude-code/hooks/PostToolUseFailure-Bash-output-main.json (OS-23).
#   po2 (Claude Code): the same lines with exit 0; a PostToolUse rewrite sets stdout to a tracer and stderr
#     to "". Writes fixtures/claude-code/requests/po2-result.json (OS-24).
#   po3 (OpenCode): the same lines with exit 0; a probe plugin records tool.execute.after. Writes
#     fixtures/opencode/plugin-hooks/tool.execute.after-bash-stderr.json (OS-25).
#   po4 (both agents): two result_view calls (lines 1-400 and 1-40) to a stdio MCP server named zygo.
#     Writes fixtures/claude-code/requests/po4-readback.json and fixtures/opencode/requests/po4-readback.json
#     (OS-26).
#   Each file written is scrubbed (scripts/premises/scrub.py), and its path is printed; a file the run did
#   not produce is removed. Every other capture (result records, hook inputs, request bodies, SSE replies)
#   stays in the run folder in the scratch folder; their paths are printed at the end.
# Run by hand only, by the operator, with claude and opencode on PATH; never from a test or CI (O-C7).
# po1 also needs rustup on PATH with the toolchain of rust-toolchain.toml (`rustup which cargo` in the repo
# root finds its cargo).
#
# What this run does to the machine: a temporary HOME and config folder in the scratch folder, the dummy
# key and the mock upstream of lib.sh: no login is read, no model request leaves the machine, nothing
# under the real HOME is written. OpenCode fetches its provider package from the package registry into
# the temporary HOME. po1 puts the bin folder of that toolchain first on the isolated PATH.
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
mkdir -p "$FXC/hooks" "$FXC/requests" "$FXO/plugin-hooks" "$FXO/requests"
KEPT=""   # captures left in the scratch folder (kept), printed at the end
echo "live: Claude Code $LIVE_CC_VER, OpenCode $LIVE_OC_VER"

# --- helpers --------------------------------------------------------------------------------
# steps <prefix> <steps json>: SSE files and mock rules of one scripted sequence (steps.py)
steps() {
  mkdir -p "$RUN/sse"
  printf '%s\n' "$2" >"$RUN/steps-$1.json"
  python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" "$1" "$RUN/rules.json" "$RUN/steps-$1.json"
}
# wrote <src> <dest>: a scrubbed copy of <src> at <dest>, and its path; a missing <src> removes <dest>
wrote() {
  if [ -z "$1" ] || [ ! -f "$1" ]; then rm -f "$2"; echo "live: no ${1##*/}; removed ${2#"$REPO_DIR"/}"; return 0; fi
  scrub "$1" >"$2"
  echo "wrote ${2#"$REPO_DIR"/}"
}
# kept <path>...: captures that stay in the scratch folder; an empty word is skipped
kept() { for _k in "$@"; do [ -z "$_k" ] || KEPT="$KEPT $_k"; done; }
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
# po1_crate <dir>: <dir>/scratch, a crate whose cargo test passes 120 tests and fails 2 (the
# failing-nocapture recipe of fixtures/shaping/libtest)
po1_crate() {
  mkdir -p "$1/scratch/src"
  printf '%s\n' '[package]' 'name = "scratch"' 'version = "0.1.0"' 'edition = "2024"' '' '[dependencies]' \
    >"$1/scratch/Cargo.toml"
  {
    printf '%s\n' 'pub fn add(a: u64, b: u64) -> u64 {' '    a + b' '}' '' '#[cfg(test)]' 'mod tests {' \
      '    use super::add;' ''
    _n=1
    while [ "$_n" -le 120 ]; do
      printf '    #[test]\n    fn adds_%03d() {\n        assert_eq!(add(%d, %d), %d);\n    }\n' \
        "$_n" "$_n" "$_n" "$((_n * 2))"
      _n=$((_n + 1))
    done
    printf '%s\n' '' '    #[test]' '    fn sums_a_ledger() {' '        let rows = [3u64, 4, 5];' \
      '        for (i, r) in rows.iter().enumerate() {' '            println!("ledger row {i}: {r}");' '        }' \
      '        assert_eq!(rows.iter().sum::<u64>(), 13, "ledger total");' '    }' '' '    #[test]' \
      '    fn add_exceeds_its_inputs() {' '        assert!(add(1, 1) > 2, "add(1, 1) must exceed 2");' '    }' '}'
  } >"$1/scratch/src/lib.rs"
}
# po1_verdict <po1-result.json>: the pass|fail text of the LIVE PO1 line; the cargo-test call is not
# part of the verdict
po1_verdict() {
  jq -r '(.out_line_at >= 0 and .err_line_at >= 0) as $both
    | ([.calls[] | select(.replaced and .case != "cargo-test") | .case] | join(", ")) as $r
    | (if $both and $r != "" then "pass" else "fail" end)
      + " error field: " + (if $both then (if .out_line_at < .err_line_at then "out-line before err-line" else "err-line before out-line" end)
        else "out-line at \(.out_line_at), err-line at \(.err_line_at)" end)
      + "; cases that replace the text the model sees: " + (if $r == "" then "none" else $r end)
      + "; failed calls hooked: \(.calls | length)"' "$1"
}
# po1_cargo <po1-cargo-test-hook.json>: the cargo-test clause of the LIVE PO1 line (a report, not a rule):
# not hooked, or hooked with the first line of the field error and whether it holds a test result line
po1_cargo() {
  if [ ! -f "$1" ]; then echo "cargo-test call: not hooked"; return 0; fi
  jq -r '(.error // "" | if type == "string" then . else tojson end) as $e
    | "cargo-test call: hooked, " + ($e | split("\n")[0])
      + ", test result " + (if ($e | contains("test result:")) then "present" else "absent" end)' "$1" \
    || echo "cargo-test call: hooked, hook input not readable"
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
  jq -n --arg u "$MOCK_URL/v1" --arg k "$DUMMY_KEY" --argjson m "${1:-null}" '{model: "zygo/lead", small_model: "zygo/lead",
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
CMD_CARGO="cd scratch && cargo test"
CMD_BOTH="printf 'out-line\\n'; printf 'err-line\\n' >&2"

# --- PO1 (Claude Code): the PostToolUseFailure input of a failed Bash call, and what replaces its text --
part_po1() {
  live_run po1
  steps po1 "$(jq -nc --arg c "$CMD_FAIL" --arg k "$CMD_CARGO" '{trigger: "tracer-po1", no_header: ["x-claude-code-agent-id"], extra: {body: "\"tools\": \\[\\{"},
    steps: ([range(3) | {tool: "Bash", input: {command: $c}}] + [{tool: "Bash", input: {command: $k}}] + [{text: "po1 done"}])}')"
  cat >"$RUN/responses.json" <<'J'
[{"event": "PostToolUseFailure", "tool": "^Bash$", "times": 1, "case": "none", "output": {}},
 {"event": "PostToolUseFailure", "tool": "^Bash$", "times": 1, "case": "string",
  "output": {"hookSpecificOutput": {"hookEventName": "PostToolUseFailure", "updatedToolOutput": "PO1-REPLACED-string"}}},
 {"event": "PostToolUseFailure", "tool": "^Bash$", "times": 1, "case": "object",
  "output": {"hookSpecificOutput": {"hookEventName": "PostToolUseFailure", "updatedToolOutput": {"stdout": "PO1-REPLACED-object", "stderr": "", "interrupted": false, "isImage": false, "noOutputExpected": false}}}},
 {"event": "PostToolUseFailure", "tool": "^Bash$", "times": 1, "case": "cargo-test", "output": {}}]
J
  # cargo for the cargo-test call: the isolated PATH (TOOL_PATH, lib.sh) holds no cargo
  # rustup which reads rust-toolchain.toml of the current folder: run it in the repo root
  _cg=$(cd "$REPO_DIR" && rustup which cargo 2>/dev/null) ||
    { echo "live: rustup which cargo found no cargo for rust-toolchain.toml; rustup must be on PATH" >&2; exit 1; }
  case "$(dirname "$_cg")" in
    *"'"* | *" "*) echo "live: a path holds a single quote or a space; this script cannot use it"; exit 1 ;;
  esac
  TOOL_PATH="$(dirname "$_cg"):$TOOL_PATH"
  echo "live: cargo $_cg"
  "$_cg" --version >&2
  hook_settings "$RUN/hooks" "$RUN/responses.json" >"$RUN/settings.json"
  start_mock "$RUN/rules.json"
  po1_crate "$R"
  run_claude -p "tracer-po1 run the command" --settings "$RUN/settings.json" --dangerously-skip-permissions \
    --tools Bash </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock
  kept "$RUN/sse" "$RUN/hooks"
  _hk=$(hook_files "$RUN/hooks" PostToolUseFailure-Bash-main)
  # the raw hook input of the cargo-test call, scrubbed, stays in the run folder (never under fixtures/)
  _ck=""
  for _f in $_hk; do
    [ "$(jq -r '.tool_use_id // empty' "$_f" 2>/dev/null || true)" = toolu_po1_03_bash ] && { _ck=$_f; break; }
  done
  if [ -n "$_ck" ]; then
    scrub "$_ck" >"$RUN/po1-cargo-test-hook.json"
    kept "$RUN/po1-cargo-test-hook.json"
  else
    echo "live: no PostToolUseFailure input for toolu_po1_03_bash"
  fi
  wrote "$(printf '%s\n' "$_hk" | sed -n 1p)" "$FXC/hooks/PostToolUseFailure-Bash-output-main.json"
  _ids=$(for _f in $_hk; do jq -r '.tool_use_id // empty' "$_f" 2>/dev/null || true; done)
  # shellcheck disable=SC2086
  seen_json "$RUN/po1-seen.json" $_ids
  # shellcheck disable=SC2086
  python3 -I - "$RUN/po1-seen.json" "$RUN/po1-result.json" $_hk <<'PY'
import json, sys
seen_f, out, hooks = sys.argv[1], sys.argv[2], sys.argv[3:]
cases = ["none", "string", "object", "cargo-test"]  # the rules of responses.json, one use each, in order

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
  # shellcheck disable=SC2046
  kept "$RUN/po1-result.json" $(jq -r '.calls[].request_file // empty' "$RUN/po1-result.json")
  _v=$(po1_verdict "$RUN/po1-result.json")
  result PO1 "${_v%% *}" "${_v#* }; $(po1_cargo "$RUN/po1-cargo-test-hook.json")"
}

# --- PO2 (Claude Code): updatedToolOutput with stderr "" removes the stderr the model sees ----------
part_po2() {
  live_run po2
  steps po2 "$(jq -nc --arg c "$CMD_BOTH" '{trigger: "tracer-po2", no_header: ["x-claude-code-agent-id"], extra: {body: "\"tools\": \\[\\{"},
    steps: [{tool: "Bash", input: {command: $c}}, {text: "po2 done"}]}')"
  cat >"$RUN/responses.json" <<'J'
[{"event": "PostToolUse", "tool": "^Bash$", "rewrite": [[["stdout"], "PO2-REPLACED-stdout"], [["stderr"], ""]]}]
J
  hook_settings "$RUN/hooks" "$RUN/responses.json" >"$RUN/settings.json"
  start_mock "$RUN/rules.json"
  run_claude -p "tracer-po2 run the command" --settings "$RUN/settings.json" --dangerously-skip-permissions \
    --tools Bash </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock
  _hk=$(hook_files "$RUN/hooks" PostToolUse-Bash-main | sed -n 1p)
  kept "$RUN/sse" "$_hk"
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
  kept "$(jq -r '.request_file // ""' "$RUN/po2-result.json")"
  _v=$(jq -r '(if .replaced and (.err_line_seen | not) then "pass" else "fail" end)
    + " the model saw the stdout tracer: \(if .replaced then "yes" else "no" end); the model saw err-line: \(if .err_line_seen then "yes" else "no" end)"' "$RUN/po2-result.json")
  result PO2 "${_v%% *}" "${_v#* }"
}

# --- PO3 (OpenCode): tool.execute.after for bash gives stdout and stderr together in output.output ----
part_po3() {
  # shellcheck disable=SC2034  # read by result (live.sh)
  LIVE_VER=$LIVE_OC_VER   # the LIVE line names the OpenCode version
  live_run po3
  steps po3 "$(jq -nc --arg c "$CMD_BOTH" '{trigger: "tracer-po3", extra: {body: "\"tools\""},
    steps: [{tool: "bash", input: {command: $c, description: "po3"}}, {text: "po3 done"}]}')"
  start_mock "$RUN/rules.json"
  oc_setup
  oc_run "tracer-po3 run the command"
  stop_mock
  wrote "$RUN/plog/tool.execute.after-bash.json" "$FXO/plugin-hooks/tool.execute.after-bash-stderr.json"
  seen_json "$RUN/po3-seen.json" toolu_po3_00_bash
  kept "$RUN/sse" "$RUN/po3-seen.json" "$(jq -r '.[0].request_file // ""' "$RUN/po3-seen.json")"
  _v=$(jq -r '(.output.output // "" | if type == "string" then . else tojson end) as $o
    | ($o | index("out-line")) as $a | ($o | index("err-line")) as $b
    | (if $a != null and $b != null then "pass" else "fail" end)
      + " output.output: " + (if $a != null and $b != null then (if $a < $b then "out-line before err-line" else "err-line before out-line" end)
        else "out-line \(if $a == null then "absent" else "present" end), err-line \(if $b == null then "absent" else "present" end)" end)' \
    "$RUN/plog/tool.execute.after-bash.json" 2>/dev/null || echo "fail no tool.execute.after record for bash")
  result PO3 "${_v%% *}" "${_v#* }"
}

# --- PO4 (both agents): the text the model sees for a result_view reply -----------------------------
# rv_mcp: $RUN/rv_mcp.py, a stdio MCP server named zygo with one tool, result_view
rv_mcp() {
  cat >"$RUN/rv_mcp.py" <<'PY'
"""Stdio MCP server of live-shaping.sh PO4: serverInfo zygo, one tool result_view. Its reply has the
shape of crates/zygo-mcp/src/server.rs:132; the text is the lines po4-line-NNN from the first to the
last number of "lines", joined with newlines, no final newline."""
import json
import sys


def view(lines):
    try:
        a, b = (int(x) for x in str(lines).split("-", 1))
    except ValueError:
        a, b = 1, 0
    return "\n".join("po4-line-%03d" % n for n in range(a, b + 1))


for line in sys.stdin:
    try:
        msg = json.loads(line)
    except ValueError:
        continue
    mid, method = msg.get("id"), msg.get("method")
    if mid is None:
        continue
    if method == "initialize":
        res = {"protocolVersion": msg.get("params", {}).get("protocolVersion", "2024-11-05"),
               "capabilities": {"tools": {}}, "serverInfo": {"name": "zygo", "version": "0.0.1"}}
    elif method == "tools/list":
        res = {"tools": [{"name": "result_view", "description": "Returns lines of a stored result.",
                          "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "lines": {"type": "string"}},
                                          "required": ["id"]}}]}
    elif method == "tools/call":
        args = (msg.get("params") or {}).get("arguments") or {}
        res = {"content": [{"type": "text", "text": view(args.get("lines", ""))}], "isError": False}
    else:
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "not found"}}) + "\n")
        sys.stdout.flush()
        continue
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": mid, "result": res}) + "\n")
    sys.stdout.flush()
PY
}
# po4_judge <id> <readback dest> <tool_use_id of lines 1-400> <tool_use_id of lines 1-40>: the readback
# record at <readback dest>, the request bodies kept in the scratch folder, one LIVE line
po4_judge() {
  seen_json "$RUN/po4-seen.json" "$3" "$4"
  python3 -I - "$RUN/po4-seen.json" "$RUN/po4-readback.json" <<'PY'
import json, sys
seen_f, out = sys.argv[1:3]


def view(a, b):
    return "\n".join("po4-line-%03d" % n for n in range(a, b + 1))


try:
    seen = json.load(open(seen_f))
except (OSError, ValueError):
    seen = []
calls = []
for (lines, t), s in zip((("1-400", view(1, 400)), ("1-40", view(1, 40))), seen):
    text = s.get("text") or ""
    i = text.find(t)
    c = {"lines": lines, "reply_bytes": len(t.encode()), "seen_bytes": len(text.encode()), "reply_found": i >= 0,
         "prefix": text[:i] if i >= 0 else None, "suffix": text[i + len(t):] if i >= 0 else None}
    c["wrapper_bytes"] = c["seen_bytes"] - c["reply_bytes"]
    c.update(s)
    calls.append(c)
json.dump(calls, open(out, "w"), indent=1)
PY
  wrote "$RUN/po4-readback.json" "$2"
  # shellcheck disable=SC2046
  kept "$RUN/po4-readback.json" $(jq -r '.[].request_file // empty' "$RUN/po4-readback.json")
  _v=$(jq -r '(length == 2 and all(.[]; .reply_found) and .[0].wrapper_bytes == .[1].wrapper_bytes) as $ok
    | (if $ok then "pass" else "fail" end) + " "
      + ([.[] | "lines \(.lines): reply \(.reply_bytes) bytes, seen \(.seen_bytes) bytes, reply found \(if .reply_found then "yes" else "no" end), wrapper \(.wrapper_bytes) bytes"]
         | if length == 0 then "no tool result seen" else join("; ") end)' "$RUN/po4-readback.json")
  result "$1" "${_v%% *}" "${_v#* }"
}
part_po4() {
  live_run po4-cc
  rv_mcp
  steps po4c "$(jq -nc '{trigger: "tracer-po4", no_header: ["x-claude-code-agent-id"], extra: {body: "\"tools\": \\[\\{"}, steps: [
    {tool: "mcp__zygo__result_view", input: {id: "01PO4000000000000000000000", lines: "1-400"}},
    {tool: "mcp__zygo__result_view", input: {id: "01PO4000000000000000000000", lines: "1-40"}},
    {text: "po4 done"}]}')"
  # python3 through the isolated TOOL_PATH, as the OpenCode half starts it
  jq -n --arg s "$RUN/rv_mcp.py" '{mcpServers: {zygo: {command: "python3", args: ["-I", $s]}}}' >"$RUN/mcp.json"
  start_mock "$RUN/rules.json"
  run_claude -p "tracer-po4 read the result" --mcp-config "$RUN/mcp.json" --strict-mcp-config \
    --dangerously-skip-permissions </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock
  po4_judge PO4-CC "$FXC/requests/po4-readback.json" toolu_po4c_00_mcp__zygo__result_view toolu_po4c_01_mcp__zygo__result_view

  # shellcheck disable=SC2034  # read by result (live.sh)
  LIVE_VER=$LIVE_OC_VER   # the PO4-OC line names the OpenCode version
  live_run po4-oc
  rv_mcp
  steps po4o "$(jq -nc '{trigger: "tracer-po4", extra: {body: "\"tools\""}, steps: [
    {tool: "zygo_result_view", input: {id: "01PO4000000000000000000000", lines: "1-400"}},
    {tool: "zygo_result_view", input: {id: "01PO4000000000000000000000", lines: "1-40"}},
    {text: "po4 done"}]}')"
  start_mock "$RUN/rules.json"
  oc_setup "$(jq -nc --arg s "$RUN/rv_mcp.py" '{zygo: {type: "local", command: ["python3", "-I", $s], enabled: true}}')"
  oc_run "tracer-po4 read the result"
  stop_mock
  po4_judge PO4-OC "$FXO/requests/po4-readback.json" toolu_po4o_00_zygo_result_view toolu_po4o_01_zygo_result_view
}

case $MODE in
  po1) part_po1 ;;
  po2) part_po2 ;;
  po3) part_po3 ;;
  po4) part_po4 ;;
esac
for _k in $KEPT; do echo "kept $_k"; done   # scratch captures (paths only)
live_done "$REPO_DIR/fixtures"

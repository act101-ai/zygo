#!/bin/sh
# Codex encoder part (node PC-12): does Codex accept the Responses event sequence of zygo's own encoder
# (method K§6.3.7)?
#   PC17  K§6.4.3 stream obligations 1-5 and 7 (ping dropped): the extra output_item.added,
#         content_part, *.delta and *.done events and a response.completed whose output is []; per
#         case one served shape, each completing the turn, running its tools and coming back in the
#         next request as sent (field by field; the only key Codex may add is "content": null on a
#         reasoning item): t text; st text after a dropped server tool (obligation 3,
#         server_tool_dropped); f function_call; p custom_tool_call (the freeform apply_patch of
#         catalog-patch.json); r reasoning with the zygo mark + function_call; m text + function_call
#         (the message item echo); rt redacted_thinking (reasoning, summary [], mark only) +
#         function_call; ru reasoning with no encrypted_content (a mark over MARK_MAX,
#         reasoning_unmarked) + function_call; ff two function_calls in one response
#   PC18  K§6.4.3 stream obligations 5, 6 and 9 (each response.failed end), with request_max_retries =
#         stream_max_retries = 0: i response.incomplete max_output_tokens; ic response.incomplete
#         content_filter (stop_reason refusal); x response.failed after an upstream error; xt
#         response.failed after a half-open function_call (no tool runs). Each must end the turn with
#         the reason in Codex's turn.failed message
#   Recorded, not graded: i-def and x-def, the i and x shapes with Codex's default retries (no retry
#         keys, as a zygo launch): the number of upstream requests and how the turn ends
# The served SSE is what the encoder at HEAD makes, at run time: crates/zygo-proxy/examples/
# responses_encode.rs reads authored Messages SSE ($RUN/anthropic) and writes $RUN/sse; the served
# files are checked against the encoder's documented sequence before Codex starts (a miss: exit 1).
# result.txt records the commit and the dirty state of the tree the example was built from.
# Writes fixtures/codex/encoder/ (anthropic/, responses/, requests/, exec/, result.txt; anthropic/
# unmarked.sse is 3.2 MB and is left out: its size and sha256 are in result.txt) and
# fixtures/codex/runs/encoder/ (isolation.txt, connect.txt, observed.txt), built in $RUN/fx and
# swapped in at the end (the old set is restored on any failure). A premise that fails is a result
# (exit 0); exit 1 is a harness failure or a case with no captured request.
# Needs, besides run-codex.sh's list: cargo (the example is built offline from this checkout), git.
PREMISES_DIR=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck disable=SC2034 # lib.sh reads N2_AGENTS (unexported on purpose)
N2_AGENTS=codex
# shellcheck disable=SC1091 # lib.sh is linted on its own
. "$PREMISES_DIR/lib.sh"
command -v cargo >/dev/null 2>&1 || { echo "codex encoder: cargo is not on PATH"; exit 1; }
mkdir -p "$CODEX_FX"
new_run codex-encoder
mkdir -p "$RUN/json" "$RUN/anthropic" "$RUN/sse" "$RUN/out" "$RUN/fx"
cp "$CODEX_FX/models/catalog-patch.json" "$RUN/json/catalog-patch.json" || exit 1

# Authored inputs: the client request (Codex's own body with custom:apply_patch), the Messages SSE
# per reply, and the mock rules (first match wins).
python3 -I - "$RUN" "$CODEX_FX/requests/after-custom-tool-call-output.json" <<'PY'
import hashlib, json, os, sys
run, src = sys.argv[1:3]
with open(src) as f:
    body = json.load(f)["body"]
with open(os.path.join(run, "json", "client-request.json"), "w") as f:
    json.dump(body, f)
    f.write("\n")
PATCH = "*** Begin Patch\n*** Add File: pc12.txt\n+pc12 patch line\n*** End Patch\n"
def start(i, block):
    return {"type": "content_block_start", "index": i, "content_block": block}
def delta(i, d):
    return {"type": "content_block_delta", "index": i, "delta": d}
def stop(i):
    return {"type": "content_block_stop", "index": i}
def text(i, *parts):
    return [start(i, {"type": "text", "text": ""})] + [delta(i, {"type": "text_delta", "text": p}) for p in parts] + [stop(i)]
def tool(i, tid, name, *parts):
    return ([start(i, {"type": "tool_use", "id": tid, "name": name, "input": {}})]
            + [delta(i, {"type": "input_json_delta", "partial_json": p}) for p in parts] + [stop(i)])
def call(i, c):
    return tool(i, "toolu_pc12_" + c, "exec_command", '{"cmd": "echo ', 'pc12-tracer-%s"}' % c)
def thinking(i, think, sigs):
    return ([start(i, {"type": "thinking", "thinking": "", "signature": ""}),
             delta(i, {"type": "thinking_delta", "thinking": think})]
            + [delta(i, {"type": "signature_delta", "signature": s}) for s in sigs] + [stop(i)])
def message(name, blocks, stop_reason):
    ev = [{"type": "message_start", "message": {
        "id": "msg_pc12_" + name, "type": "message", "role": "assistant", "model": "pc12-up", "content": [],
        "stop_reason": None, "stop_sequence": None, "usage": {"input_tokens": 10, "output_tokens": 1}}},
        {"type": "ping"}] + blocks
    if stop_reason is not None:
        ev += [{"type": "message_delta", "delta": {"stop_reason": stop_reason, "stop_sequence": None},
                "usage": {"output_tokens": 5}}, {"type": "message_stop"}]
    return ev
ERROR = {"type": "error", "error": {"type": "api_error", "message": "pc12 upstream failure"}}
# unmarked: a 3,200,000-byte signature (50 deltas of 64,000) makes the mark (base64 of the block) pass
# MARK_MAX (4 MiB) while the held bytes stay under HOLD_MAX (4 MiB); the summary stays short.
inputs = {
    "text": message("text", text(0, "pc12 text ", "reply"), "end_turn"),
    "server": message("server", [start(0, {"type": "server_tool_use", "id": "srvtoolu_pc12", "name": "web_search", "input": {}}),
                                 delta(0, {"type": "input_json_delta", "partial_json": '{"query": "pc12"}'}), stop(0),
                                 start(1, {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_pc12", "content": []}),
                                 stop(1)] + text(2, "pc12 after server tool"), "end_turn"),
    "fc": message("fc", call(0, "f"), "tool_use"),
    "patch": message("patch", tool(0, "toolu_pc12_p", "apply_patch", json.dumps({"input": PATCH})), "tool_use"),
    "reason": message("reason", thinking(0, "pc12 thinking", ["pc12-signature"]) + call(1, "r"), "tool_use"),
    "msgcall": message("msgcall", text(0, "pc12 before call") + call(1, "m"), "tool_use"),
    "redacted": message("redacted", [start(0, {"type": "redacted_thinking", "data": "pc12-redacted-data"}), stop(0)]
                        + call(1, "rt"), "tool_use"),
    "unmarked": message("unmarked", thinking(0, "pc12 thinking ru", ["S" * 64000] * 50) + call(1, "ru"), "tool_use"),
    "twofc": message("twofc", call(0, "ff1") + call(1, "ff2"), "tool_use"),
    "incomplete": message("incomplete", text(0, "pc12 cut"), "max_tokens"),
    "refusal": message("refusal", text(0, "pc12 refused"), "refusal"),
    # No block stop, no message_delta, no message_stop: the upstream error event ends the stream.
    "failed": message("failed", text(0, "pc12 partial")[:2], None) + [ERROR],
    "halfcall": message("halfcall", tool(0, "toolu_pc12_xt", "exec_command", '{"cmd": "echo pc12-tr')[:2], None) + [ERROR],
}
for c in ("f", "p", "r", "m", "rt", "ru", "ff"):
    inputs["final-" + c] = message("final-" + c, text(0, "pc12 done " + c), "end_turn")
for name, ev in inputs.items():
    with open(os.path.join(run, "anthropic", name + ".sse"), "w") as f:
        f.write("".join("event: %s\ndata: %s\n\n" % (e["type"], json.dumps(e)) for e in ev))
with open(os.path.join(run, "anthropic", "unmarked.sse"), "rb") as f:
    data = f.read()
with open(os.path.join(run, "encoder-src.txt"), "a") as f:
    f.write("unmarked_input_bytes=%d sha256=%s\n" % (len(data), hashlib.sha256(data).hexdigest()))
# Rules: each case's prompt is pc12-turn-<case>-end (no case's prompt is part of another's).
FIRST = [("t", "text"), ("st", "server"), ("f", "fc"), ("p", "patch"), ("r", "reason"), ("m", "msgcall"),
         ("rt", "redacted"), ("ru", "unmarked"), ("ff", "twofc"), ("i", "incomplete"), ("x", "failed"),
         ("ic", "refusal"), ("xt", "halfcall"), ("i-def", "incomplete"), ("x-def", "failed")]
rules = []
for c in ("f", "p", "r", "m", "rt", "ru", "ff"):
    out = "custom_tool_call_output" if c == "p" else "function_call_output"
    rules.append({"name": c + "-2", "body": 'pc12-turn-%s-end.*"type": "%s"' % (c, out), "sse": "sse/final-%s.sse" % c})
rules += [{"name": c, "body": "pc12-turn-%s-end" % c, "sse": "sse/%s.sse" % s} for c, s in FIRST]
with open(os.path.join(run, "rules.json"), "w") as f:
    json.dump([dict(r, path="/v1/responses") for r in rules], f, indent=1)
    f.write("\n")
PY

# The tree the example is built from: its commit and whether crates/ or Cargo.* differ from it.
{
  echo "encoder_commit=$(git -C "$REPO_DIR" rev-parse HEAD)"
  if [ -z "$(git -C "$REPO_DIR" status --porcelain -- crates Cargo.toml Cargo.lock)" ]; then
    echo "encoder_tree=clean"
  else
    echo "encoder_tree=dirty"
  fi
} >"$RUN/encoder-src.head"
cat "$RUN/encoder-src.head" "$RUN/encoder-src.txt" >"$RUN/encoder-src.new" && mv "$RUN/encoder-src.new" "$RUN/encoder-src.txt"

# The served files: the encoder's own bytes for each input; client model {{MODEL}} (the mock puts the
# request's model there). The example's stderr (one adjustment code per line) is kept beside each.
for _pc12_f in "$RUN"/anthropic/*.sse; do
  _pc12_n=$(basename "$_pc12_f" .sse)
  cargo run --offline --locked -q --manifest-path "$REPO_DIR/Cargo.toml" -p zygo-proxy --example responses_encode -- \
    "$RUN/json/client-request.json" pc12-ant pc12-up '{{MODEL}}' \
    <"$_pc12_f" >"$RUN/sse/$_pc12_n.sse" 2>"$RUN/sse/$_pc12_n.adj" ||
    { echo "codex encoder: responses_encode failed on $_pc12_n.sse:"; cat "$RUN/sse/$_pc12_n.adj"; exit 1; }
done

# The served-file check: each file is the encoder's documented sequence (responses_client.rs, Encoder).
python3 -I - "$RUN" <<'PY' || exit 1
import json, os, sys
run = sys.argv[1]
PATCH = "*** Begin Patch\n*** Add File: pc12.txt\n+pc12 patch line\n*** End Patch\n"
TERMINAL = ("response.completed", "response.incomplete", "response.failed")
TEXT = ["response.created", "response.in_progress", "response.output_item.added", "response.content_part.added",
        "response.output_text.delta", "response.output_text.delta", "response.output_text.done",
        "response.content_part.done", "response.output_item.done", "response.completed"]
C, I, F = "response.completed", "response.incomplete", "response.failed"
RF = ["reasoning", "function_call"]
# name: (output_item.done item types, terminal, incomplete reason, adjustment codes, call ids)
EXPECT = {
    "text": (["message"], C, None, [], []), "server": (["message"], C, None, ["server_tool_dropped"], []),
    "fc": (["function_call"], C, None, [], ["toolu_pc12_f"]),
    "patch": (["custom_tool_call"], C, None, [], ["toolu_pc12_p"]),
    "reason": (RF, C, None, [], ["toolu_pc12_r"]),
    "msgcall": (["message", "function_call"], C, None, [], ["toolu_pc12_m"]),
    "redacted": (RF, C, None, [], ["toolu_pc12_rt"]),
    "unmarked": (RF, C, None, ["reasoning_unmarked:1"], ["toolu_pc12_ru"]),
    "twofc": (["function_call", "function_call"], C, None, [], ["toolu_pc12_ff1", "toolu_pc12_ff2"]),
    "incomplete": (["message"], I, "max_output_tokens", [], []),
    "refusal": (["message"], I, "content_filter", [], []),
    "failed": ([], F, None, [], []), "halfcall": ([], F, None, [], []),
}
for c in ("f", "p", "r", "m", "rt", "ru", "ff"):
    EXPECT["final-" + c] = (["message"], C, None, [], [])
class Miss(Exception):
    pass
def events(name):
    with open(os.path.join(run, "sse", name + ".sse")) as f:
        text = f.read()
    if not text.endswith("\n\n"):
        raise Miss("does not end with a blank line")
    out = []
    for n, block in enumerate(text[:-2].split("\n\n")):
        lines = block.split("\n")
        if len(lines) != 2 or not lines[0].startswith("event: ") or not lines[1].startswith("data: "):
            raise Miss("event %d is not one event and one data line" % n)
        e = lines[0][len("event: "):]
        try:
            d = json.loads(lines[1][len("data: "):])
        except ValueError:
            raise Miss("event %d data is not JSON" % n)
        if not isinstance(d, dict) or d.get("type") != e:
            raise Miss("event %d data.type is not %s" % (n, e))
        if d.get("sequence_number") != n:
            raise Miss("event %d sequence_number is %r" % (n, d.get("sequence_number")))
        out.append(d)
    return out
def check(name):
    types, term, reason, adj, calls = EXPECT[name]
    ev = events(name)
    names = [d["type"] for d in ev]
    if names[:2] != ["response.created", "response.in_progress"]:
        raise Miss("starts %s" % ",".join(names[:2]))
    if "response.output_item.added" not in names:
        raise Miss("no response.output_item.added")
    terms = [n for n in names if n in TERMINAL]
    if terms != [term] or names[-1] != term:
        raise Miss("terminal events %s, not one %s at the end" % (",".join(terms) or "none", term))
    resp = ev[-1].get("response")
    if not isinstance(resp, dict):
        raise Miss("terminal event has no response")
    if term != F and resp.get("output") != []:
        raise Miss("terminal response.output is not []")
    if reason is not None and (resp.get("incomplete_details") or {}).get("reason") != reason:
        raise Miss("incomplete_details.reason is not %s" % reason)
    if term == F and not (resp.get("error") or {}).get("message"):
        raise Miss("response.failed has no error.message")
    if name == "text" and names != TEXT:
        raise Miss("events %s" % ",".join(names))
    items = [d["item"] for d in ev if d["type"] == "response.output_item.done" and isinstance(d.get("item"), dict)]
    if [it.get("type") for it in items] != types:
        raise Miss("output_item.done types %s, not %s" % (",".join(it.get("type", "-") for it in items), ",".join(types)))
    got = [it.get("call_id") for it in items if it.get("type") in ("function_call", "custom_tool_call")]
    if got != calls:
        raise Miss("call ids %s, not %s" % (got, calls))
    with open(os.path.join(run, "sse", name + ".adj")) as f:
        codes = [l for l in f.read().splitlines() if l]
    if codes != adj:
        raise Miss("adjustments %s, not %s" % (codes, adj))
    rs = [it for it in items if it.get("type") == "reasoning"]
    if name == "patch" and items[0].get("input") != PATCH:
        raise Miss("custom_tool_call input is not the patch text")
    if name in ("reason", "redacted") and not str(rs[0].get("encrypted_content") or "").startswith("zygo1t:"):
        raise Miss("reasoning item has no encrypted_content zygo1t:...")
    if name == "reason" and not rs[0].get("summary"):
        raise Miss("reasoning item has no summary")
    if name == "redacted" and rs[0].get("summary") != []:
        raise Miss("redacted reasoning summary is not []")
    if name == "unmarked" and ("encrypted_content" in rs[0] or not rs[0].get("summary")):
        raise Miss("unmarked reasoning item has encrypted_content or no summary")
    if name == "halfcall" and not any(d["type"] == "response.output_item.added" and d["item"].get("type") == "function_call"
                                      for d in ev):
        raise Miss("no function_call output_item.added")
for name in EXPECT:
    try:
        check(name)
    except (Miss, OSError) as x:
        print("codex encoder: served %s.sse: %s" % (name, x))
        sys.exit(1)
PY

# Codex's configs, both with the catalog that offers the freeform apply_patch: $H/.codex with no retry
# (the graded cases) and $RUN/cx-def with Codex's default retries (no retry keys, as a zygo launch).
start_mock "$RUN/rules.json"
# pc12_config <dir> <retries: 0|default>
pc12_config() {
  mkdir -p "$1"
  {
    echo 'model = "gpt-mock"'
    echo 'model_provider = "mock"'
    echo "model_catalog_json = \"$RUN/json/catalog-patch.json\""
    echo
    echo '[model_providers.mock]'
    echo 'name = "mock"'
    echo "base_url = \"$MOCK_URL/v1\""
    echo 'wire_api = "responses"'
    if [ "$2" = 0 ]; then
      echo 'request_max_retries = 0'
      echo 'stream_max_retries = 0'
    fi
    echo 'env_key = "MOCK_API_KEY"'
  } >"$1/config.toml"
}
pc12_config "$H/.codex" 0
pc12_config "$RUN/cx-def" default

# pc12_ps <case>: the processes of this run's Codex copies (or the tracer commands) after a case.
# ps writes to a file first, so no filter's own command line can match.
pc12_ps() {
  ps -A -o pid=,ppid=,command= >"$RUN/ps.txt" 2>/dev/null || :
  if grep -F -e "$RUN/codex-pkg" -e pc12-tracer "$RUN/ps.txt" >"$RUN/ps-hit.txt"; then
    { echo "$1:"; cat "$RUN/ps-hit.txt"; } >>"$RUN/processes.txt"
  else
    echo "$1: none" >>"$RUN/processes.txt"
  fi
}
# pc12_run <case> <CODEX_HOME>: one codex exec turn; output, status (never fatal) and processes kept.
# The harness's own refusal ("premises: ..." from run_codex or codex_env, in a subshell) ends the part.
pc12_run() {
  _pc12_rc=0
  # shellcheck disable=SC2034 # lib.sh's codex_env reads CX_HOME
  (CX_HOME=$2 && run_codex exec --json -s workspace-write --enable apply_patch_freeform "pc12-turn-$1-end") \
    </dev/null >"$RUN/exec-$1.out" 2>"$RUN/exec-$1.err" || _pc12_rc=$?
  if grep '^premises: ' "$RUN/exec-$1.out"; then exit 1; fi
  check_abort
  echo "$_pc12_rc" >"$RUN/exec-$1.rc"
  pc12_ps "$1"
}
for _pc12_c in t st f p r m rt ru ff i x ic xt; do pc12_run "$_pc12_c" "$H/.codex"; done
for _pc12_c in i-def x-def; do pc12_run "$_pc12_c" "$RUN/cx-def"; done
stop_mock

# The isolation record goes to the run folder's fixture tree too (moved at the end).
_pc12_fx=$CODEX_FX
CODEX_FX=$RUN/fx
codex_record encoder
CODEX_FX=$_pc12_fx

# pc12_verdict begin
# The verdict: per case, the requests, what Codex printed, the echo of the sent items and the reason of
# a failed turn; PC17 and PC18. Writes $RUN/out/encoder/ (result.txt, requests/, exec/) and
# $RUN/out/runs/encoder/observed.txt. A case with no request: the two "codex encoder:" lines, exit 1.
_pc12_vs=0
python3 -I - "$RUN" "$RUN/out" "$(TZ=UTC date +%Y-%m-%d)" <<'PY' || _pc12_vs=$?
import glob, json, os, re, sys
run, out, day = sys.argv[1:4]
PC17 = ["t", "st", "f", "p", "r", "m", "rt", "ru", "ff"]
PC18 = ["i", "x", "ic", "xt"]
RECORDED = ["i-def", "x-def"]
CASES = PC17 + PC18 + RECORDED
TEXT = {"t": ["pc12 text reply"], "st": ["pc12 after server tool"]}
TYPES = {"f": ["function_call"], "p": ["custom_tool_call"], "r": ["reasoning", "function_call"],
         "m": ["message", "function_call"], "rt": ["reasoning", "function_call"], "ru": ["reasoning", "function_call"],
         "ff": ["function_call", "function_call"]}
MSGS = {"m": ["pc12 before call", "pc12 done m"]}
REASON = {"i": "max_output_tokens", "i-def": "max_output_tokens", "ic": "content_filter"}
def rd(p, default=""):
    try:
        with open(p, errors="replace") as f:
            return f.read()
    except OSError:
        return default
def put(rel, text):
    p = os.path.join(out, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w") as f:
        f.write(text)
with open(os.path.join(run, "rules.json")) as f:
    SERVED = {r["name"]: r["sse"] for r in json.load(f)}
recs = []
for p in glob.glob(os.path.join(run, "rec", "[0-9][0-9][0-9][0-9].json")):
    with open(p) as f:
        recs.append(json.load(f))
recs.sort(key=lambda r: r["seq"])
def reqs(c):
    return [r for r in recs if re.search(r"/v1/responses(\?|$)", r.get("path") or "")
            and "pc12-turn-%s-end" % c in json.dumps(r.get("body"))]
def served(rec):
    """The events of the file the mock served to this record."""
    name = SERVED.get(rec.get("reply"))
    ev = []
    for block in rd(os.path.join(run, name)).split("\n\n") if name else []:
        for line in block.split("\n"):
            if line.startswith("data: "):
                try:
                    ev.append(json.loads(line[len("data: "):]))
                except ValueError:
                    pass
    return [e for e in ev if isinstance(e, dict)]
def sent(rec):
    return [e["item"] for e in served(rec) if e.get("type") == "response.output_item.done" and isinstance(e.get("item"), dict)]
def lines(c):
    ls = []
    for line in rd(os.path.join(run, "exec-%s.out" % c)).splitlines():
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if isinstance(d, dict):
            ls.append(d)
    return ls
def items(rec):
    b = rec.get("body")
    i = b.get("input") if isinstance(b, dict) else None
    return [x for x in i if isinstance(x, dict)] if isinstance(i, list) else []
CALLS = ("function_call", "custom_tool_call")
def echo(item, rec):
    """(echoed, extra keys) for the input entry that echoes item: same type and call_id (calls) or id; every key
    of item with an equal value; the only extra key allowed is "content": null on a reasoning item."""
    key = "call_id" if item.get("type") in CALLS else "id"
    for x in items(rec):
        if x.get("type") == item.get("type") and x.get(key) == item.get(key):
            extra = sorted(set(x) - set(item))
            same = all(k in x and x[k] == v for k, v in item.items())
            allowed = item.get("type") == "reasoning" and extra in ([], ["content"]) and x.get("content") is None
            return same and (not extra or allowed), extra
    return False, []
def output_of(rec, ty, call_id):
    return [x.get("output") for x in items(rec) if x.get("type") == ty and x.get("call_id") == call_id]
res, ok, missing = {}, {}, []
for c in CASES:
    rs = reqs(c)
    ls = lines(c)
    last = ls[-1].get("type", "none") if ls else "none"
    msgs = [d["item"].get("text") for d in ls if d.get("type") == "item.completed"
            and isinstance(d.get("item"), dict) and d["item"].get("type") == "agent_message"]
    rc = rd(os.path.join(run, "exec-%s.rc" % c), "none").strip() or "none"
    for n, r in enumerate(rs, 1):
        put("encoder/requests/%s-%d.json" % (c, n), json.dumps(r, indent=2) + "\n")
    put("encoder/exec/%s.jsonl" % c, rd(os.path.join(run, "exec-%s.out" % c)))
    if not rs:
        missing.append(c)
    good, ech, extra, reason = False, "-", "-", None
    if c in TEXT:
        good = rc == "0" and last == "turn.completed" and len(rs) == 1 and msgs == TEXT[c]
    elif c in TYPES and rs:
        its = sent(rs[0])
        echoes = [echo(it, rs[1]) for it in its] if len(rs) == 2 else []
        ech = "yes" if its and len(echoes) == len(its) and all(e[0] for e in echoes) else "no"
        extra = ",".join(sorted({k for e in echoes for k in e[1]})) or "-"
        good = (rc == "0" and last == "turn.completed" and len(rs) == 2 and ech == "yes"
                and [it.get("type") for it in its] == TYPES[c]
                and (msgs == MSGS[c] if c in MSGS else "pc12 done %s" % c in msgs))
        for it in its:
            if it.get("type") not in CALLS or len(rs) != 2:
                continue
            cid = it.get("call_id", "")
            if it["type"] == "custom_tool_call":
                outs = output_of(rs[1], "custom_tool_call_output", cid)
                good = good and bool(outs) and not any("unsupported custom tool call" in json.dumps(o) for o in outs)
                good = good and rd(os.path.join(run, "repo", "pc12.txt"), None) == "pc12 patch line\n"
            else:
                outs = output_of(rs[1], "function_call_output", cid)
                good = good and any("pc12-tracer-" + cid[len("toolu_pc12_"):] in json.dumps(o) for o in outs)
    elif c in PC18 + RECORDED:
        tf = [d for d in ls if d.get("type") == "turn.failed"]
        reason = (tf[-1].get("error") or {}).get("message") if tf else None
        want = REASON.get(c)
        if want is None:
            want = next(((e.get("response") or {}).get("error", {}).get("message") for e in (served(rs[0]) if rs else [])
                         if e.get("type") == "response.failed"), None)
        good = last == "turn.failed" and isinstance(reason, str) and bool(want) and want in reason
        if c == "xt":
            good = good and not any(isinstance(d.get("item"), dict) and d["item"].get("type") == "command_execution"
                                    for d in ls)
    ok[c] = good
    res[c] = "%s: ok=%s rc=%s requests=%d last=%s msgs=%s echo=%s extra=%s reason=%s" % (
        c, "yes" if good else "no", rc, len(rs), last, json.dumps(msgs), ech, extra,
        json.dumps(reason) if reason is not None else "-")
pc17 = "verified" if all(ok[c] for c in PC17) else "failed"
pc18 = "verified" if all(ok[c] for c in PC18) else "failed"
iso = rd(os.path.join(run, "fx", "runs", "encoder", "isolation.txt"))
ver = next((l.split("=", 1)[1] for l in iso.splitlines() if l.startswith("codex_version=")), "none") or "none"
src = [l for l in rd(os.path.join(run, "encoder-src.txt")).splitlines() if l]
put("encoder/result.txt", "\n".join([res[c] for c in CASES] + ["PC17=" + pc17, "PC18=" + pc18,
                                                               "codex_version=" + ver] + src) + "\n")
def obs(pc, result, paths):
    return "- %s: %s %s: %s" % (pc, result, day, " ".join("fixtures/codex/encoder/" + p for p in paths))
put("runs/encoder/observed.txt", "\n".join([
    obs("PC17", pc17, ["result.txt"] + ["responses/%s.sse" % n for n in
                                        ("text", "server", "fc", "patch", "reason", "msgcall", "redacted", "unmarked", "twofc")]
        + ["requests/%s-2.json" % c for c in ("f", "p", "r", "m", "rt", "ru", "ff")] + ["exec/t.jsonl", "exec/st.jsonl"]),
    obs("PC18", pc18, ["result.txt"] + ["responses/%s.sse" % n for n in ("incomplete", "refusal", "failed", "halfcall")]
        + ["exec/%s.jsonl" % c for c in ("i", "x", "ic", "xt", "i-def", "x-def")])]) + "\n")
if missing:
    print("codex encoder: fixtures not replaced; this run's are in %s/fx" % run)
    print("codex encoder: missing cases: %s" % " ".join(missing))
    sys.exit(1)
PY
# pc12_verdict end

# Build the fixtures in $RUN/fx, each file through scrub (a failed scrub: exit 1, before the swap).
# anthropic/unmarked.sse (3.2 MB) is left out; result.txt holds its size and sha256.
: >"$RUN/fx-list.txt"
(cd "$RUN/out" && find . -type f) | sed 's|^\./||' | while read -r _pc12_f; do echo "out/$_pc12_f $_pc12_f"; done >>"$RUN/fx-list.txt"
for _pc12_f in "$RUN"/anthropic/*.sse; do
  [ "${_pc12_f##*/}" = unmarked.sse ] || echo "anthropic/${_pc12_f##*/} encoder/anthropic/${_pc12_f##*/}" >>"$RUN/fx-list.txt"
done
for _pc12_f in "$RUN"/sse/*.sse; do echo "sse/${_pc12_f##*/} encoder/responses/${_pc12_f##*/}" >>"$RUN/fx-list.txt"; done
while read -r _pc12_src _pc12_dst; do
  mkdir -p "$RUN/fx/$(dirname "$_pc12_dst")" || exit 1
  scrub "$RUN/$_pc12_src" >"$RUN/fx/$_pc12_dst" || { echo "codex encoder: scrub failed on $_pc12_src; fixtures not replaced"; exit 1; }
done <"$RUN/fx-list.txt"
[ "$_pc12_vs" = 0 ] || exit 1

# pc12_swap: replace the two fixture folders as one step (as proxy.sh's p3_swap). The old ones are
# moved aside into $RUN/old, the new ones moved in from $RUN/fx, then the old ones deleted. On any
# failure the new ones moved in so far are removed and the old set is moved back; return 1.
PC12_DIRS="encoder runs/encoder"
pc12_swap() {
  mkdir -p "$RUN/old/runs" "$CODEX_FX/runs" || return 1
  _pc12_ok=1 _pc12_aside="" _pc12_in=""
  for _pc12_d in $PC12_DIRS; do
    [ -e "$CODEX_FX/$_pc12_d" ] || continue
    if mv "$CODEX_FX/$_pc12_d" "$RUN/old/$_pc12_d"; then _pc12_aside="$_pc12_aside $_pc12_d"; else _pc12_ok=0; break; fi
  done
  if [ "$_pc12_ok" = 1 ]; then
    for _pc12_d in $PC12_DIRS; do
      if mv "$RUN/fx/$_pc12_d" "$CODEX_FX/$_pc12_d"; then _pc12_in="$_pc12_in $_pc12_d"; else _pc12_ok=0; break; fi
    done
  fi
  if [ "$_pc12_ok" = 1 ]; then
    rm -rf "$RUN/old"
    return 0
  fi
  for _pc12_d in $_pc12_in; do rm -rf "${CODEX_FX:?}/$_pc12_d"; done
  for _pc12_d in $_pc12_aside; do
    mv "$RUN/old/$_pc12_d" "$CODEX_FX/$_pc12_d" || echo "codex encoder: could not restore $_pc12_d; it is in $RUN/old/$_pc12_d"
  done
  echo "codex encoder: fixture swap failed; the old fixtures are back"
  return 1
}
pc12_swap || exit 1
_pc12_v=$(sed -n 's/^codex_version=//p' "$CODEX_FX/runs/encoder/isolation.txt")
echo "codex encoder: ok (codex $_pc12_v) $(sed -n 's/^\(PC1[78]=.*\)$/\1/p' "$CODEX_FX/encoder/result.txt" | tr '\n' ' ' | sed 's/ $//')"

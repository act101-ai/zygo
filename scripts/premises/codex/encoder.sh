#!/bin/sh
# Codex encoder part (node PC-12): does Codex accept the Responses event sequence of zygo's own encoder
# (method K§6.3.7)?
#   PC17  K§6.4.3 stream obligations 1-5 and 7 (ping dropped): the extra output_item.added,
#         content_part, *.delta and *.done events and a response.completed whose output is []; per
#         case one served shape, each completing the turn, running its tools and coming back exactly
#         once in the next request as sent (field by field; Codex may add only "content" and
#         "encrypted_content" on a reasoning item, each null and absent from the sent item): t text; st
#         text after a dropped server tool (obligation 3, server_tool_dropped); f function_call; p
#         custom_tool_call (the freeform apply_patch of catalog-patch.json); r reasoning with the zygo
#         mark + function_call; m text + function_call (the message item echo); rt redacted_thinking
#         (reasoning, summary [], mark only) + function_call; ru reasoning with no encrypted_content (a
#         mark over MARK_MAX, reasoning_unmarked) + function_call; ff two function_calls in one response
#   PC18  K§6.4.3 stream obligations 5 and 6, with request_max_retries = stream_max_retries = 0: i
#         response.incomplete max_output_tokens; x response.failed after an upstream error; xt
#         response.failed after a half-open function_call (no tool runs). Each: exactly 1 request, a
#         non-zero exit, and Codex's turn.failed message equal to the expected text (PC12_REASON in the
#         verdict). Obligation 9's endings (upstream ... too long) are not served
#   PC19  K§6.4.3 stream obligation 5, with Codex's default retries (no retry keys, as a zygo launch):
#         rfz, the encoder's ending for stop_reason refusal (responses/refusal.sse, the cand-c ending:
#         a second message item whose output_text is the refusal text, then response.completed). Turn
#         1 makes exactly 1 request, completes, and shows the refusal text once; the resumed next turn
#         makes 1 request, completes, and its request holds turn 1's prompt and the refusal text
#   Recorded, not graded ("recorded" lines in result.txt): i-def, x-def, xt-def, the i, x and xt
#         shapes with Codex's default retries (no retry keys, as a zygo launch): the number of
#         upstream requests and how the turn ends
#   Recorded (RS-12), with Codex's default retries: rfa-rfd, candidate endings for stop_reason refusal,
#         each made from the encoder's own refusal-base.sse (a text reply), so only the ending differs
#         (responses/cand-a..d.sse): rfa response.incomplete content_filter; rfb a second message item
#         with a refusal part, then response.completed; rfc the same with an output_text part; rfd
#         response.failed, code invalid_prompt. Each case is resumed for a next turn (exec resume
#         --last pc12-next-<case>-end). Per case: the turn-1 request count, whether Codex shows the
#         refusal text once, and whether the next turn completes with the refused turn in its request;
#         refusal_choice = the first of rfb, rfc, rfd that passes all three (else none)
# The served SSE is what the encoder at HEAD makes, at run time: crates/zygo-proxy/examples/
# responses_encode.rs reads authored Messages SSE ($RUN/anthropic) and writes $RUN/sse; the served
# files are checked against the encoder's documented sequence before Codex starts (a miss: exit 1).
# result.txt records the commit of the tree the example was built from (encoder_commit), the last
# commit of this part (part_commit) and whether either differs from its commit (encoder_tree).
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
    "refusal-base": message("refusal-base", text(0, "pc12 refused"), "end_turn"),
    "next": message("next", text(0, "pc12 next reply"), "end_turn"),
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
         ("xt", "halfcall"), ("i-def", "incomplete"), ("x-def", "failed"), ("xt-def", "halfcall"),
         ("rfa", "cand-a"), ("rfb", "cand-b"), ("rfc", "cand-c"), ("rfd", "cand-d"), ("rfz", "refusal")]
# A refusal case's next turn (its body also holds the case's turn-1 prompt) is matched first.
rules = [{"name": c + "-n", "body": "pc12-next-%s-end" % c, "sse": "sse/next.sse"}
         for c in ("rfa", "rfb", "rfc", "rfd", "rfz")]
for c in ("f", "p", "r", "m", "rt", "ru", "ff"):
    out = "custom_tool_call_output" if c == "p" else "function_call_output"
    rules.append({"name": c + "-2", "body": 'pc12-turn-%s-end.*"type": "%s"' % (c, out), "sse": "sse/final-%s.sse" % c})
rules += [{"name": c, "body": "pc12-turn-%s-end" % c, "sse": "sse/%s.sse" % s} for c, s in FIRST]
with open(os.path.join(run, "rules.json"), "w") as f:
    json.dump([dict(r, path="/v1/responses") for r in rules], f, indent=1)
    f.write("\n")
PY

# The tree the example is built from: its commit, this part's last commit, and whether crates/,
# Cargo.*, rust-toolchain.toml or this part differ from the checkout's HEAD.
_pc12_spec="crates Cargo.toml Cargo.lock scripts/premises/codex/encoder.sh"
[ ! -e "$REPO_DIR/rust-toolchain.toml" ] || _pc12_spec="$_pc12_spec rust-toolchain.toml"
{
  echo "encoder_commit=$(git -C "$REPO_DIR" rev-parse HEAD)"
  echo "part_commit=$(git -C "$REPO_DIR" log -1 --format=%H -- scripts/premises/codex/encoder.sh)"
  # shellcheck disable=SC2086 # a list of paths
  if [ -z "$(git -C "$REPO_DIR" status --porcelain -- $_pc12_spec)" ]; then
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

# pc12_candidates begin
# The refusal candidates (RS-12): sse/cand-<x>.sse from the encoder's own refusal-base.sse (U = the
# ULID of its resp_<U>), sequence_number renumbered from 0; each added event has output_index 1 and
# item_id msg_<U>_1. a and d replace the terminal event; b and c add a message item before it.
python3 -I - "$RUN" <<'PY' || exit 1
import json, os, sys
run = sys.argv[1]
T = "The upstream model refused this request (stop_reason: refusal)."
with open(os.path.join(run, "sse", "refusal-base.sse")) as f:
    base = [json.loads(l[len("data: "):]) for l in f.read().split("\n") if l.startswith("data: ")]
last = base[-1]
if last.get("type") != "response.completed":
    print("codex encoder: refusal-base.sse does not end with response.completed")
    sys.exit(1)
u = last["response"]["id"][len("resp_"):]
iid = "msg_%s_1" % u
def item(status, content):
    return {"type": "message", "id": iid, "role": "assistant", "status": status, "content": content}
def added_item(empty, full, delta_type, done_type, done_key):
    at = {"item_id": iid, "output_index": 1, "content_index": 0}
    return [dict(output_index=1, item=item("in_progress", []), type="response.output_item.added"),
            dict(at, part=empty, type="response.content_part.added"),
            dict(at, delta=T, type=delta_type),
            dict(at, **{done_key: T, "type": done_type}),
            dict(at, part=full, type="response.content_part.done"),
            dict(output_index=1, item=item("completed", [full]), type="response.output_item.done")]
def terminal(name, **fields):
    resp = dict(last["response"], **fields)
    return {"response": resp, "type": name}
cands = {
    "a": base[:-1] + [terminal("response.incomplete", status="incomplete", incomplete_details={"reason": "content_filter"})],
    "b": base[:-1] + added_item({"type": "refusal", "refusal": ""}, {"type": "refusal", "refusal": T},
                                "response.refusal.delta", "response.refusal.done", "refusal") + [last],
    "c": base[:-1] + added_item({"type": "output_text", "text": "", "annotations": []},
                                {"type": "output_text", "text": T, "annotations": []},
                                "response.output_text.delta", "response.output_text.done", "text") + [last],
    "d": base[:-1] + [terminal("response.failed", status="failed", error={"code": "invalid_prompt", "message": T})],
}
for x, ev in cands.items():
    with open(os.path.join(run, "sse", "cand-%s.sse" % x), "w") as f:
        for n, e in enumerate(ev):
            e = {k: v for k, v in e.items() if k != "sequence_number"}
            e["sequence_number"] = n
            f.write("event: %s\ndata: %s\n\n" % (e["type"], json.dumps(e, separators=(",", ":"))))
    open(os.path.join(run, "sse", "cand-%s.adj" % x), "w").close()
PY
# pc12_candidates end

# pc12_served begin
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
    "refusal": (["message", "message"], C, None, [], []),
    "failed": ([], F, None, [], []), "halfcall": ([], F, None, [], []),
    "refusal-base": (["message"], C, None, [], []), "next": (["message"], C, None, [], []),
    "cand-a": (["message"], I, "content_filter", [], []),
    "cand-b": (["message", "message"], C, None, [], []), "cand-c": (["message", "message"], C, None, [], []),
    "cand-d": (["message"], F, None, [], []),
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
    # Obligation 4: output_index counts the added items from 0; every event of an item carries its output_index
    # and its item id (item.id on output_item.*, item_id on the others).
    ids = {}
    for n, d in enumerate(ev):
        if d["type"] == "response.output_item.added":
            if d.get("output_index") != len(ids) or not isinstance(d.get("item"), dict) or not d["item"].get("id"):
                raise Miss("event %d: output_item.added output_index %r, not %d, or no item id" % (n, d.get("output_index"), len(ids)))
            ids[len(ids)] = d["item"]["id"]
        elif "output_index" in d or "item_id" in d or d["type"] == "response.output_item.done":
            oi = d.get("output_index")
            if oi not in ids:
                raise Miss("event %d (%s): output_index %r has no output_item.added before it" % (n, d["type"], oi))
            got = d["item"].get("id") if d["type"] == "response.output_item.done" else d.get("item_id")
            if got != ids[oi]:
                raise Miss("event %d (%s): item id %r, not %r of output_index %d" % (n, d["type"], got, ids[oi], oi))
for name in EXPECT:
    try:
        check(name)
    except (Miss, OSError) as x:
        print("codex encoder: served %s.sse: %s" % (name, x))
        sys.exit(1)
# The encoder's refusal ending (RS-12) is the chosen candidate's, but for ids and sequence numbers.
def norm(v):
    if isinstance(v, dict):
        return {k: norm(x) for k, x in v.items() if k not in ("id", "item_id", "sequence_number")}
    return [norm(x) for x in v] if isinstance(v, list) else v
if norm(events("refusal")) != norm(events("cand-c")):
    print("codex encoder: served refusal.sse: not the cand-c ending")
    sys.exit(1)
PY
# pc12_served end

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
# pc12_exec <name> <CODEX_HOME> <codex args>: one codex exec turn; output (exec-<name>.*), status
# (never fatal) and processes kept. The harness's own refusal ("premises: ..." from run_codex or
# codex_env, in a subshell) ends the part.
pc12_exec() {
  _pc12_rc=0 _pc12_name=$1 _pc12_home=$2
  shift 2
  # shellcheck disable=SC2034 # lib.sh's codex_env reads CX_HOME
  (CX_HOME=$_pc12_home && run_codex exec --json -s workspace-write --enable apply_patch_freeform "$@") \
    </dev/null >"$RUN/exec-$_pc12_name.out" 2>"$RUN/exec-$_pc12_name.err" || _pc12_rc=$?
  if grep '^premises: ' "$RUN/exec-$_pc12_name.out"; then exit 1; fi
  check_abort
  echo "$_pc12_rc" >"$RUN/exec-$_pc12_name.rc"
  pc12_ps "$_pc12_name"
}
# pc12_run <case> <CODEX_HOME>: the case's turn, prompt pc12-turn-<case>-end.
pc12_run() { pc12_exec "$1" "$2" "pc12-turn-$1-end"; }
# pc12_run2 <case>: the case's next turn in $RUN/cx-def, resuming its session (the newest there).
pc12_run2() { pc12_exec "$1-n" "$RUN/cx-def" resume --last "pc12-next-$1-end"; }
for _pc12_c in t st f p r m rt ru ff i x xt; do pc12_run "$_pc12_c" "$H/.codex"; done
for _pc12_c in i-def x-def xt-def; do pc12_run "$_pc12_c" "$RUN/cx-def"; done
for _pc12_c in rfa rfb rfc rfd rfz; do
  pc12_run "$_pc12_c" "$RUN/cx-def"
  pc12_run2 "$_pc12_c"
done
stop_mock

# The isolation record goes to the run folder's fixture tree too (moved at the end).
_pc12_fx=$CODEX_FX
CODEX_FX=$RUN/fx
codex_record encoder
CODEX_FX=$_pc12_fx

# pc12_verdict begin
# The verdict: per case, the requests, what Codex printed, the echo of the sent items and the reason of
# a failed turn; PC17, PC18 and PC19. Writes $RUN/out/encoder/ (result.txt, requests/, exec/) and
# $RUN/out/runs/encoder/observed.txt. A case with no request: the two "codex encoder:" lines, exit 1.
_pc12_vs=0
python3 -I - "$RUN" "$RUN/out" "$(TZ=UTC date +%Y-%m-%d)" <<'PY' || _pc12_vs=$?
import glob, json, os, re, sys, traceback
def crashed(ty, value, tb):
    first = (str(value).splitlines() or [""])[0]
    print("codex encoder: verdict failed: %s: %s" % (ty.__name__, first))
    traceback.print_exception(ty, value, tb)
sys.excepthook = crashed
run, out, day = sys.argv[1:4]
PC17 = ["t", "st", "f", "p", "r", "m", "rt", "ru", "ff"]
PC18 = ["i", "x", "xt"]
RECORDED = ["i-def", "x-def", "xt-def"]
# The refusal candidates (RS-12), recorded and not graded; refusal_choice is the first of rfb, rfc, rfd that passes.
CAND = ["rfa", "rfb", "rfc", "rfd"]
# PC19: the encoder's own refusal ending (the cand-c form), graded as a candidate that passes and completes its turn.
PC19 = ["rfz"]
# The cases whose refusal text is a message item (shown as an agent_message, sent back in the next turn).
TEXT_END = ("rfb", "rfc", "rfz")
REFUSAL_TEXT = "The upstream model refused this request (stop_reason: refusal)."
CASES = PC17 + PC18 + RECORDED + CAND + PC19
TEXT = {"t": ["pc12 text reply"], "st": ["pc12 after server tool"]}
TYPES = {"f": ["function_call"], "p": ["custom_tool_call"], "r": ["reasoning", "function_call"],
         "m": ["message", "function_call"], "rt": ["reasoning", "function_call"], "ru": ["reasoning", "function_call"],
         "ff": ["function_call", "function_call"]}
MSGS = {"m": ["pc12 before call", "pc12 done m"]}
# PC12_REASON: Codex 0.172.0's exact turn.failed message per ending (the round 2 real runs); the graded cases
# must equal it.
PC12_REASON = {"i": "Incomplete response returned, reason: max_output_tokens",
               "x": "stream disconnected before completion: pc12 upstream failure",
               "xt": "stream disconnected before completion: pc12 upstream failure"}
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
def responses(marker):
    return [r for r in recs if re.search(r"/v1/responses(\?|$)", r.get("path") or "")
            and marker in json.dumps(r.get("body"))]
def reqs(c):
    """The case's turn-1 requests: a next turn's body also holds the turn-1 prompt and is left out."""
    return [r for r in responses("pc12-turn-%s-end" % c) if "pc12-next-%s-end" % c not in json.dumps(r.get("body"))]
def reqs2(c):
    return responses("pc12-next-%s-end" % c)
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
    of item with an equal value; the only extra keys allowed are "content" and "encrypted_content" on a reasoning
    item, each with the value null and only when item lacks it (absent and null mean the same, lead's decision). The
    entry must be the only one with that type and call_id or id (a duplicate or a second, altered copy fails)."""
    key = "call_id" if item.get("type") in CALLS else "id"
    found = [x for x in items(rec) if x.get("type") == item.get("type") and x.get(key) == item.get(key)]
    if not found:
        return False, []
    x = found[0]
    extra = sorted(set(x) - set(item))
    same = all(k in x and x[k] == v for k, v in item.items())
    allowed = item.get("type") == "reasoning" and all(
        k in ("content", "encrypted_content") and x[k] is None for k in extra)
    return len(found) == 1 and same and (not extra or allowed), extra
def output_of(rec, ty, call_id):
    return [x.get("output") for x in items(rec) if x.get("type") == ty and x.get("call_id") == call_id]
def turn(name):
    """(exec lines, last line type, agent_message texts, exit status) of exec-<name>.*."""
    ls = lines(name)
    last = ls[-1].get("type", "none") if ls else "none"
    msgs = [d["item"].get("text") for d in ls if d.get("type") == "item.completed"
            and isinstance(d.get("item"), dict) and d["item"].get("type") == "agent_message"]
    rc = rd(os.path.join(run, "exec-%s.rc" % name), "none").strip() or "none"
    return ls, last, msgs, rc
def failed_reason(ls):
    tf = [d for d in ls if d.get("type") == "turn.failed"]
    return (tf[-1].get("error") or {}).get("message") if tf else None
def hist(rec, prompt):
    """The types of the input entries after the one holding prompt; a message as message:<part types joined by +>."""
    its = items(rec)
    at = next((n for n, x in enumerate(its) if prompt in json.dumps(x)), None)
    out = []
    for x in its[at + 1:] if at is not None else []:
        t = x.get("type", "-")
        if t == "message":
            parts = x.get("content") if isinstance(x.get("content"), list) else []
            t += ":" + "+".join(p.get("type", "-") if isinstance(p, dict) else "-" for p in parts)
        out.append(t)
    return ",".join(out) or "-"
def candidate(c, rs, msgs, ls):
    """(passes, line tail) of a refusal candidate: turn 1 once, the refusal shown once, the next turn works."""
    reason = failed_reason(ls)
    if c in TEXT_END:
        shown = sum(REFUSAL_TEXT in (m or "") for m in msgs) == 1
    elif c == "rfd":
        shown = REFUSAL_TEXT in (reason or "")
    else:
        shown = "content_filter" in (reason or "")
    rs2 = reqs2(c)
    ls2, last2, msgs2, rc2 = turn(c + "-n")
    if rs2:
        put("encoder/requests/%s-next.json" % c, json.dumps(rs2[0], indent=2) + "\n")
    put("encoder/exec/%s-n.jsonl" % c, rd(os.path.join(run, "exec-%s-n.out" % c)))
    nextok = (rc2 == "0" and last2 == "turn.completed" and len(rs2) == 1 and "pc12 next reply" in msgs2
              and "pc12-turn-%s-end" % c in json.dumps(items(rs2[0])))
    if c in TEXT_END:
        nextok = nextok and REFUSAL_TEXT in json.dumps(items(rs2[0]))
    ech, hs = "-", "-"
    if rs2:
        hs = hist(rs2[0], "pc12-turn-%s-end" % c)
        its = sent(rs[0]) if rs else []
        if c in TEXT_END and len(its) == 2:
            ech = "yes" if echo(its[1], rs2[0])[0] else "no"
    yn = lambda b: "yes" if b else "no"
    tail = "shown=%s reason=%s next: rc=%s requests=%d last=%s msgs=%s ok=%s echo=%s hist=%s" % (
        yn(shown), json.dumps(reason) if reason is not None else "-", rc2, len(rs2), last2, json.dumps(msgs2),
        yn(nextok), ech, hs)
    return len(rs) == 1 and shown and nextok, tail
res, ok, missing = {}, {}, []
for c in CASES:
    rs = reqs(c)
    ls, last, msgs, rc = turn(c)
    for n, r in enumerate(rs, 1):
        put("encoder/requests/%s-%d.json" % (c, n), json.dumps(r, indent=2) + "\n")
    put("encoder/exec/%s.jsonl" % c, rd(os.path.join(run, "exec-%s.out" % c)))
    if not rs:
        missing.append(c)
    if c in CAND + PC19:
        ok[c], tail = candidate(c, rs, msgs, ls)
        if c in PC19:
            ok[c] = ok[c] and rc == "0" and last == "turn.completed"
        mark = "ok=" + ("yes" if ok[c] else "no") if c in PC19 else "recorded"
        res[c] = "%s: %s rc=%s requests=%d last=%s msgs=%s %s" % (c, mark, rc, len(rs), last, json.dumps(msgs), tail)
        continue
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
        reason = failed_reason(ls)
        want = PC12_REASON.get(c.replace("-def", ""))
        good = last == "turn.failed" and reason == want and rc not in ("0", "none") and len(rs) == 1
        if c.startswith("xt"):
            good = good and not any(isinstance(d.get("item"), dict) and d["item"].get("type") == "command_execution"
                                    for d in ls)
    ok[c] = good
    res[c] = "%s: %s rc=%s requests=%d last=%s msgs=%s echo=%s extra=%s reason=%s" % (
        c, "recorded" if c in RECORDED else "ok=" + ("yes" if good else "no"), rc, len(rs), last, json.dumps(msgs), ech,
        extra, json.dumps(reason) if reason is not None else "-")
pc17 = "verified" if all(ok[c] for c in PC17) else "failed"
pc18 = "verified" if all(ok[c] for c in PC18) else "failed"
pc19 = "verified" if all(ok[c] for c in PC19) else "failed"
iso = rd(os.path.join(run, "fx", "runs", "encoder", "isolation.txt"))
ver = next((l.split("=", 1)[1] for l in iso.splitlines() if l.startswith("codex_version=")), "none") or "none"
src = [l for l in rd(os.path.join(run, "encoder-src.txt")).splitlines() if l]
choice = next((c for c in ("rfb", "rfc", "rfd") if ok[c]), "none")
if choice != "rfc":
    print("codex encoder: refusal_choice=%s, not rfc (the encoder's refusal ending, RS-12)" % choice)
put("encoder/result.txt", "\n".join([res[c] for c in CASES] + ["refusal_choice=" + choice, "PC17=" + pc17, "PC18=" + pc18,
                                                               "PC19=" + pc19, "codex_version=" + ver] + src) + "\n")
def obs(pc, result, paths):
    return "- %s: %s %s: %s" % (pc, result, day, " ".join("fixtures/codex/encoder/" + p for p in paths))
put("runs/encoder/observed.txt", "\n".join([
    obs("PC17", pc17, ["result.txt"] + ["responses/%s.sse" % n for n in
                                        ("text", "server", "fc", "patch", "reason", "msgcall", "redacted", "unmarked", "twofc")]
        + ["requests/%s-2.json" % c for c in ("f", "p", "r", "m", "rt", "ru", "ff")] + ["exec/t.jsonl", "exec/st.jsonl"]),
    obs("PC18", pc18, ["result.txt"] + ["responses/%s.sse" % n for n in ("incomplete", "failed", "halfcall")]
        + ["exec/%s.jsonl" % c for c in ("i", "x", "xt", "i-def", "x-def", "xt-def")]),
    obs("PC19", pc19, ["result.txt"] + ["responses/%s.sse" % n for n in
                                        ("refusal", "refusal-base", "next", "cand-a", "cand-b", "cand-c", "cand-d")]
        + ["exec/rfz.jsonl", "exec/rfz-n.jsonl", "requests/rfz-next.json"]
        + ["exec/rf%s%s.jsonl" % (x, n) for x in "abcd" for n in ("", "-n")])]) + "\n")
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
echo "codex encoder: ok (codex $_pc12_v) $(sed -n 's/^\(PC1[789]=.*\)$/\1/p' "$CODEX_FX/encoder/result.txt" | tr '\n' ' ' | sed 's/ $//')"

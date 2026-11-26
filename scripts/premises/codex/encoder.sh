#!/bin/sh
# Codex encoder part (node PC-12): does Codex accept the Responses event sequence of zygo's own encoder
# (spec K§6.4.3 stream obligations 1-7, method K§6.3.7)?
#   PC17  text, function_call, custom_tool_call (the freeform apply_patch of catalog-patch.json) and a
#         reasoning item with the zygo mark (encrypted_content zygo1t:...), each sent with the extra
#         output_item.added, content_part, *.delta and *.done events and a response.completed whose
#         output is []: the turn completes, the tool runs, and the item comes back in the next request
#         as sent (field by field; keys Codex adds are recorded as extra)
#   PC18  response.incomplete (max_output_tokens) and response.failed (after an upstream error): the
#         turn fails and Codex shows the reason
# The served SSE is what the encoder at HEAD makes, at run time: crates/zygo-proxy/examples/
# responses_encode.rs reads authored Messages SSE ($RUN/anthropic) and writes $RUN/sse; the served
# files are checked against the encoder's documented sequence before Codex starts (a miss: exit 1).
# Writes fixtures/codex/encoder/ (anthropic/, responses/, requests/, exec/, result.txt) and
# fixtures/codex/runs/encoder/ (isolation.txt, connect.txt, observed.txt), built in $RUN/fx and
# swapped in at the end (the old set is restored on any failure). A premise that fails is a result
# (exit 0); exit 1 is a harness failure or a case with no captured request.
# Needs, besides run-codex.sh's list: cargo (the example is built offline from this checkout).
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
import json, os, sys
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
def thinking(i):
    return [start(i, {"type": "thinking", "thinking": "", "signature": ""}),
            delta(i, {"type": "thinking_delta", "thinking": "pc12 thinking"}),
            delta(i, {"type": "signature_delta", "signature": "pc12-signature"}), stop(i)]
def message(name, blocks, stop_reason):
    ev = [{"type": "message_start", "message": {
        "id": "msg_pc12_" + name, "type": "message", "role": "assistant", "model": "pc12-up", "content": [],
        "stop_reason": None, "stop_sequence": None, "usage": {"input_tokens": 10, "output_tokens": 1}}},
        {"type": "ping"}] + blocks
    if stop_reason is not None:
        ev += [{"type": "message_delta", "delta": {"stop_reason": stop_reason, "stop_sequence": None},
                "usage": {"output_tokens": 5}}, {"type": "message_stop"}]
    return ev
inputs = {
    "text": message("text", text(0, "pc12 text ", "reply"), "end_turn"),
    "fc": message("fc", tool(0, "toolu_pc12_f", "exec_command", '{"cmd": "echo ', 'pc12-tracer-f"}'), "tool_use"),
    "patch": message("patch", tool(0, "toolu_pc12_p", "apply_patch", json.dumps({"input": PATCH})), "tool_use"),
    "reason": message("reason", thinking(0) + tool(1, "toolu_pc12_r", "exec_command", '{"cmd": "echo pc12-tracer-r"}'),
                      "tool_use"),
    "final-f": message("final-f", text(0, "pc12 done f"), "end_turn"),
    "final-p": message("final-p", text(0, "pc12 done p"), "end_turn"),
    "final-r": message("final-r", text(0, "pc12 done r"), "end_turn"),
    "incomplete": message("incomplete", text(0, "pc12 cut"), "max_tokens"),
    # No block stop, no message_delta, no message_stop: the upstream error event ends the stream.
    "failed": message("failed", text(0, "pc12 partial")[:2], None)
              + [{"type": "error", "error": {"type": "api_error", "message": "pc12 upstream failure"}}],
}
for name, ev in inputs.items():
    with open(os.path.join(run, "anthropic", name + ".sse"), "w") as f:
        f.write("".join("event: %s\ndata: %s\n\n" % (e["type"], json.dumps(e)) for e in ev))
R = "/v1/responses"
rules = [("f2", 'pc12-turn-f.*"type": "function_call_output"', "final-f"),
         ("p2", 'pc12-turn-p.*"type": "custom_tool_call_output"', "final-p"),
         ("r2", 'pc12-turn-r.*"type": "function_call_output"', "final-r"),
         ("t", "pc12-turn-t", "text"), ("f", "pc12-turn-f", "fc"), ("p", "pc12-turn-p", "patch"),
         ("r", "pc12-turn-r", "reason"), ("i", "pc12-turn-i", "incomplete"), ("x", "pc12-turn-x", "failed")]
with open(os.path.join(run, "rules.json"), "w") as f:
    json.dump([{"name": n, "path": R, "body": b, "sse": "sse/%s.sse" % s} for n, b, s in rules], f, indent=1)
    f.write("\n")
PY

# The served files: the encoder's own bytes for each input; client model {{MODEL}} (the mock puts the
# request's model there). The example's stderr (one adjustment code per line) is kept beside each.
for _pc12_n in text fc patch reason final-f final-p final-r incomplete failed; do
  cargo run --offline --locked -q --manifest-path "$REPO_DIR/Cargo.toml" -p zygo-proxy --example responses_encode -- \
    "$RUN/json/client-request.json" pc12-ant pc12-up '{{MODEL}}' \
    <"$RUN/anthropic/$_pc12_n.sse" >"$RUN/sse/$_pc12_n.sse" 2>"$RUN/sse/$_pc12_n.adj" ||
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
END = {"incomplete": "response.incomplete", "failed": "response.failed"}
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
def done(ev, ty):
    return [d["item"] for d in ev if d["type"] == "response.output_item.done"
            and isinstance(d.get("item"), dict) and d["item"].get("type") == ty]
def check(name):
    ev = events(name)
    names = [d["type"] for d in ev]
    if names[:2] != ["response.created", "response.in_progress"]:
        raise Miss("starts %s" % ",".join(names[:2]))
    if "response.output_item.added" not in names:
        raise Miss("no response.output_item.added")
    terms = [n for n in names if n in TERMINAL]
    want = END.get(name, "response.completed")
    if terms != [want] or names[-1] != want:
        raise Miss("terminal events %s, not one %s at the end" % (",".join(terms) or "none", want))
    resp = ev[-1].get("response")
    if not isinstance(resp, dict):
        raise Miss("terminal event has no response")
    if want != "response.failed" and resp.get("output") != []:
        raise Miss("terminal response.output is not []")
    if name == "incomplete" and (resp.get("incomplete_details") or {}).get("reason") != "max_output_tokens":
        raise Miss("incomplete_details.reason is not max_output_tokens")
    if name == "text" and names != TEXT:
        raise Miss("events %s" % ",".join(names))
    if name == "fc":
        fcs = done(ev, "function_call")
        if len(fcs) != 1 or fcs[0].get("call_id") != "toolu_pc12_f":
            raise Miss("not one function_call toolu_pc12_f")
    if name == "patch":
        cs = done(ev, "custom_tool_call")
        if len(cs) != 1 or cs[0].get("call_id") != "toolu_pc12_p" or cs[0].get("input") != PATCH:
            raise Miss("not one custom_tool_call toolu_pc12_p with the patch text")
    if name == "reason":
        rs = done(ev, "reasoning")
        if len(rs) != 1 or not str(rs[0].get("encrypted_content") or "").startswith("zygo1t:"):
            raise Miss("not one reasoning item with encrypted_content zygo1t:...")
        if [d.get("call_id") for d in done(ev, "function_call")] != ["toolu_pc12_r"]:
            raise Miss("not one function_call toolu_pc12_r")
for name in ("text", "fc", "patch", "reason", "final-f", "final-p", "final-r", "incomplete", "failed"):
    try:
        check(name)
    except (Miss, OSError) as x:
        print("codex encoder: served %s.sse: %s" % (name, x))
        sys.exit(1)
PY

# Codex's config: one CODEX_HOME for every case, with the catalog that offers the freeform apply_patch.
mkdir -p "$CX_HOME"
start_mock "$RUN/rules.json"
{
  echo 'model = "gpt-mock"'
  echo 'model_provider = "mock"'
  echo "model_catalog_json = \"$RUN/json/catalog-patch.json\""
  echo
  echo '[model_providers.mock]'
  echo 'name = "mock"'
  echo "base_url = \"$MOCK_URL/v1\""
  echo 'wire_api = "responses"'
  echo 'request_max_retries = 0'
  echo 'stream_max_retries = 0'
  echo 'env_key = "MOCK_API_KEY"'
} >"$CX_HOME/config.toml"

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
# pc12_run <case>: one codex exec turn; output, status (never fatal) and processes kept. The harness's
# own refusal ("premises: ..." from run_codex or codex_env, in a subshell) ends the part.
pc12_run() {
  _pc12_rc=0
  (run_codex exec --json -s workspace-write --enable apply_patch_freeform "pc12-turn-$1") \
    </dev/null >"$RUN/exec-$1.out" 2>"$RUN/exec-$1.err" || _pc12_rc=$?
  if grep '^premises: ' "$RUN/exec-$1.out"; then exit 1; fi
  check_abort
  echo "$_pc12_rc" >"$RUN/exec-$1.rc"
  pc12_ps "$1"
}
for _pc12_c in t f p r i x; do pc12_run "$_pc12_c"; done
stop_mock

# The isolation record goes to the run folder's fixture tree too (moved at the end).
_pc12_fx=$CODEX_FX
CODEX_FX=$RUN/fx
codex_record encoder
CODEX_FX=$_pc12_fx

# pc12_verdict begin
# The verdict: per case, the requests, what Codex printed and the echo of the sent items; PC17 and
# PC18. Writes $RUN/out/encoder/ (result.txt, requests/, exec/) and $RUN/out/runs/encoder/observed.txt.
# A case with no request: the two "codex encoder:" lines and exit 1.
_pc12_vs=0
python3 -I - "$RUN" "$RUN/out" "$(TZ=UTC date +%Y-%m-%d)" <<'PY' || _pc12_vs=$?
import glob, json, os, re, sys
run, out, day = sys.argv[1:4]
CASES = "tfprix"
SERVED = {"t": "text", "f": "fc", "p": "patch", "r": "reason", "i": "incomplete", "x": "failed",
          "f2": "final-f", "p2": "final-p", "r2": "final-r"}
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
recs = []
for p in glob.glob(os.path.join(run, "rec", "[0-9][0-9][0-9][0-9].json")):
    with open(p) as f:
        recs.append(json.load(f))
recs.sort(key=lambda r: r["seq"])
def reqs(c):
    return [r for r in recs if re.search(r"/v1/responses(\?|$)", r.get("path") or "")
            and "pc12-turn-" + c in json.dumps(r.get("body"))]
def served(rec):
    """The events of the file the mock served to this record."""
    name = SERVED.get(rec.get("reply"))
    ev = []
    for block in rd(os.path.join(run, "sse", "%s.sse" % name)).split("\n\n") if name else []:
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
def echo(item, rec):
    """The extra keys of the input entry that echoes item (same type, call_id or id, every key equal), or None."""
    key = "id" if item.get("type") == "reasoning" else "call_id"
    for x in items(rec):
        if x.get("type") == item.get("type") and x.get(key) == item.get(key):
            if all(k in x and x[k] == v for k, v in item.items()):
                return sorted(set(x) - set(item))
    return None
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
    outerr = rd(os.path.join(run, "exec-%s.out" % c)) + rd(os.path.join(run, "exec-%s.err" % c))
    for n, r in enumerate(rs, 1):
        put("encoder/requests/%s-%d.json" % (c, n), json.dumps(r, indent=2) + "\n")
    put("encoder/exec/%s.jsonl" % c, rd(os.path.join(run, "exec-%s.out" % c)))
    if not rs:
        missing.append(c)
    good, ech, extra = False, "-", "-"
    if c == "t":
        good = rc == "0" and last == "turn.completed" and len(rs) == 1 and msgs == ["pc12 text reply"]
    elif c in "fpr" and rs:
        calls = [it for it in sent(rs[0]) if it.get("type") != "message"]
        echoes = [echo(it, rs[1]) for it in calls] if len(rs) == 2 else []
        ech = "yes" if calls and len(echoes) == len(calls) and all(e is not None for e in echoes) else "no"
        extra = ",".join(sorted({k for e in echoes if e for k in e})) or "-"
        good = rc == "0" and last == "turn.completed" and len(rs) == 2 and ech == "yes" and "pc12 done %s" % c in msgs
        tid = "toolu_pc12_" + c
        if c == "p":
            want = ["custom_tool_call"]
            outs = output_of(rs[1], "custom_tool_call_output", tid) if len(rs) == 2 else []
            good = good and bool(outs) and not any("unsupported custom tool call" in json.dumps(o) for o in outs)
            good = good and rd(os.path.join(run, "repo", "pc12.txt"), None) == "pc12 patch line\n"
        else:
            want = ["function_call"] if c == "f" else ["reasoning", "function_call"]
            outs = output_of(rs[1], "function_call_output", tid) if len(rs) == 2 else []
            good = good and any("pc12-tracer-" + c in json.dumps(o) for o in outs)
        good = good and [it.get("type") for it in calls] == want
    elif c == "i":
        good = last == "turn.failed" and "max_output_tokens" in outerr
    elif c == "x":
        msg = next((e.get("response", {}).get("error", {}).get("message") for e in (served(rs[0]) if rs else [])
                    if e.get("type") == "response.failed"), None)
        good = last == "turn.failed" and bool(msg) and msg in outerr
    ok[c] = good
    res[c] = "%s: ok=%s rc=%s requests=%d last=%s msgs=%s echo=%s extra=%s" % (
        c, "yes" if good else "no", rc, len(rs), last, json.dumps(msgs), ech, extra)
pc17 = "verified" if all(ok[c] for c in "tfpr") else "failed"
pc18 = "verified" if all(ok[c] for c in "ix") else "failed"
iso = rd(os.path.join(run, "fx", "runs", "encoder", "isolation.txt"))
ver = next((l.split("=", 1)[1] for l in iso.splitlines() if l.startswith("codex_version=")), "none") or "none"
put("encoder/result.txt", "\n".join([res[c] for c in CASES] + ["PC17=" + pc17, "PC18=" + pc18,
                                                               "codex_version=" + ver]) + "\n")
def obs(pc, result, paths):
    return "- %s: %s %s: %s" % (pc, result, day, " ".join("fixtures/codex/encoder/" + p for p in paths))
put("runs/encoder/observed.txt", "\n".join([
    obs("PC17", pc17, ["result.txt", "responses/text.sse", "responses/fc.sse", "responses/patch.sse",
                       "responses/reason.sse", "requests/f-2.json", "requests/p-2.json", "requests/r-2.json",
                       "exec/t.jsonl"]),
    obs("PC18", pc18, ["result.txt", "responses/incomplete.sse", "responses/failed.sse", "exec/i.jsonl",
                       "exec/x.jsonl"])]) + "\n")
if missing:
    print("codex encoder: fixtures not replaced; this run's are in %s/fx" % run)
    print("codex encoder: missing cases: %s" % " ".join(missing))
    sys.exit(1)
PY
# pc12_verdict end

# Build the fixtures in $RUN/fx, each file through scrub.
(cd "$RUN/out" && find . -type f) | while read -r _pc12_f; do
  _pc12_f=${_pc12_f#./}
  mkdir -p "$RUN/fx/$(dirname "$_pc12_f")"
  scrub "$RUN/out/$_pc12_f" >"$RUN/fx/$_pc12_f"
done
mkdir -p "$RUN/fx/encoder/anthropic" "$RUN/fx/encoder/responses"
for _pc12_f in "$RUN"/anthropic/*.sse; do scrub "$_pc12_f" >"$RUN/fx/encoder/anthropic/${_pc12_f##*/}"; done
for _pc12_f in "$RUN"/sse/*.sse; do scrub "$_pc12_f" >"$RUN/fx/encoder/responses/${_pc12_f##*/}"; done
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

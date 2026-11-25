#!/bin/sh
# Codex proxy-facing part (node PC-3): what Codex sends to a Responses provider at the mock.
#   PC1   transport: paths called, the key-helper bearer (provider auth command), other hosts
#         (refused CONNECT lines), processes left after each run  -> fixtures/codex/transport/
#   PC2   provider http_headers and env_http_headers on each request -> requests/http-headers.txt
#   PC7   the /models request and the list shapes Codex accepts (app-server model/list, the list
#         the picker shows)                                         -> fixtures/codex/models/
#   PC13  the items Codex puts before the user prompt (AGENTS.md tracer) -> requests/injected-items.txt
#   PC14  the context-too-long shapes (HTTP 400, response.failed)   -> fixtures/codex/errors/
# The four turn records (first turn, after function_call_output, after custom_tool_call_output,
# a turn with a reasoning item) are fixtures/codex/requests/*.json. Every mock reply is authored
# here (never model output); every fixture is a scrubbed copy, written whole on each run. Runs the
# isolation record (codex_record proxy) and the five "Observed" lines (runs/proxy/observed.txt).
PREMISES_DIR=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck disable=SC2034 # lib.sh reads N2_AGENTS (unexported on purpose)
N2_AGENTS=codex
# shellcheck disable=SC1091 # lib.sh is linted on its own
. "$PREMISES_DIR/lib.sh"
mkdir -p "$CODEX_FX"
new_run codex-proxy
echo 'pc3-agents-tracer: project instructions for the PC13 capture.' >"$R/AGENTS.md"
mkdir -p "$RUN/json" "$RUN/sse" "$RUN/transport" "$RUN/out"

# Authored files and the mock rules (first match wins).
python3 -I - "$RUN" <<'PY'
import json, os, sys
run = sys.argv[1]
def w(rel, obj):
    with open(os.path.join(run, rel), "w") as f:
        f.write(obj if isinstance(obj, str) else json.dumps(obj) + "\n")
def m(slug):
    return {"slug": slug, "display_name": "PC3 " + slug, "description": "authored for PC-3",
            "supported_reasoning_levels": [{"effort": "low", "description": "low"}],
            "default_reasoning_level": "low", "shell_type": "shell_command", "visibility": "list",
            "supported_in_api": True, "priority": 1, "support_verbosity": False,
            "truncation_policy": {"mode": "bytes", "limit": 10000}, "experimental_supported_tools": [],
            "base_instructions": "PC3 base instructions."}
w("json/context-400.json", {"error": {"message": "pc3 input is over the limit", "type": "invalid_request_error",
                                      "param": "input", "code": "context_length_exceeded"}})
w("json/catalog-shape.json", {"models": [m("gpt-mock"), m("gpt-mock-listed")]})
w("json/catalog-file.json", {"models": [m("gpt-mock"), m("gpt-mock-file")]})
w("json/openai-shape.json", {"object": "list", "data": [{"id": "gpt-mock-listed", "object": "model",
                                                         "created": 0, "owned_by": "pc3"}]})
resp = {"id": "resp_pc3_e2", "object": "response", "model": "{{MODEL}}"}
w("sse/context-failed.sse", "".join("event: %s\ndata: %s\n\n" % (d["type"], json.dumps(d)) for d in [
    {"type": "response.created", "response": dict(resp, status="in_progress", output=[])},
    {"type": "response.failed", "response": dict(resp, status="failed", output=[], error={
        "code": "context_length_exceeded", "message": "pc3 input is over the limit"})}]))
R = "/v1/responses"
w("rules.json", [
    {"name": "e1", "path": R, "body": "pc3-turn-e1", "times": 1, "status": 400, "json": "json/context-400.json"},
    {"name": "e2", "path": R, "body": "pc3-turn-e2", "times": 1, "sse": "sse/context-failed.sse"},
    {"name": "final", "path": R, "body": '"type": "custom_tool_call_output"', "sse": "sse/turn-final.sse"},
    {"name": "patch", "path": R, "body": '"type": "function_call_output"', "sse": "sse/turn-patch.sse"},
    {"name": "fc", "path": R, "body": "pc3-turn-b", "sse": "sse/turn-fc.sse"},
    {"name": "models-b", "path": "/v1/models", "header": {"x-pc3-variant": "^b$"}, "json": "json/catalog-shape.json"},
    {"name": "models-c", "path": "/v1/models", "header": {"x-pc3-variant": "^c$"}, "json": "json/openai-shape.json"}])
PY
start_mock "$RUN/rules.json"

# The key helper (provider auth command): logs each run, prints the dummy key.
printf '%s\n' '#!/bin/sh' "echo ran >>'$RUN/key-helper.ran'" "echo '$DUMMY_KEY'" >"$RUN/key-helper.sh"
chmod 755 "$RUN/key-helper.sh"

# cx_config <variant> <dir> [helper|catalog]: <dir>/config.toml, written whole.
cx_config() {
  mkdir -p "$2"
  {
    echo 'model = "gpt-mock"'
    echo 'model_provider = "mock"'
    [ "${3:-}" != catalog ] || echo "model_catalog_json = \"$RUN/json/catalog-file.json\""
    echo
    echo '[model_providers.mock]'
    echo 'name = "mock"'
    echo "base_url = \"$MOCK_URL/v1\""
    echo 'wire_api = "responses"'
    echo 'request_max_retries = 0'
    echo 'stream_max_retries = 0'
    echo "http_headers = { \"X-Pc3-Static\" = \"pc3-static-value\", \"X-Pc3-Variant\" = \"$1\" }"
    echo 'env_http_headers = { "X-Pc3-Env" = "TERM", "X-Pc3-Unset" = "PC3_UNSET_VAR" }'
    if [ "${3:-}" = helper ]; then
      echo
      echo '[model_providers.mock.auth]'
      echo "command = \"$RUN/key-helper.sh\""
    else
      echo 'env_key = "MOCK_API_KEY"'
    fi
  } >"$2/config.toml"
}

# ps_note <label>: the processes of this run's Codex copies (or the tracer command) after a run.
# ps writes to a file first, so no filter's own command line can match.
ps_note() {
  ps -A -o pid=,ppid=,command= >"$RUN/ps.txt" 2>/dev/null || :
  if grep -F -e "$RUN/codex-pkg" -e pc3-tracer "$RUN/ps.txt" >"$RUN/ps-hit.txt"; then
    { echo "$1:"; cat "$RUN/ps-hit.txt"; } >>"$RUN/processes.txt"
  else
    echo "$1: none" >>"$RUN/processes.txt"
  fi
}

# cx_run <label> <codex args>: one run; output, status (never fatal) and processes kept.
cx_run() {
  _p3_x=$1
  shift
  _p3_rc=0
  run_codex "$@" </dev/null >"$RUN/exec-$_p3_x.out" 2>"$RUN/exec-$_p3_x.err" || _p3_rc=$?
  echo "$_p3_rc" >"$RUN/exec-$_p3_x.rc"
  ps_note "$_p3_x"
}

# The app-server driver: initialize, then model/list over the FIFO; stops early when the shell
# writes <done> (the server has exited, so no answer can come).
cat >"$RUN/rpc.py" <<'PY'
import json, os, sys, time
fifo, out, done = sys.argv[1:4]
def got(n):
    for _ in range(300):
        fin = os.path.exists(done)
        try:
            with open(out, errors="replace") as f:
                for line in f:
                    try:
                        d = json.loads(line)
                    except ValueError:
                        continue
                    if isinstance(d, dict) and d.get("id") == n:
                        return True
        except OSError:
            pass
        if fin:
            return False
        time.sleep(0.2)
    return False
try:
    with open(fifo, "w") as w:
        def send(o):
            w.write(json.dumps(o) + "\n")
            w.flush()
        send({"id": 1, "method": "initialize", "params": {"clientInfo": {
            "name": "zygo-premise", "title": "zygo premise", "version": "0.0.0"}}})
        ok = got(1)
        if ok:
            send({"method": "initialized"})
            send({"id": 2, "method": "model/list", "params": {}})
            ok = got(2)
    sys.exit(0 if ok else 1)
except BrokenPipeError:
    sys.exit(1)
PY

# models_run <variant>: codex app-server with the variant's config; the driver's status is kept.
# b and c use the key helper: in the first real run (0.172.0) only the provider with an auth
# command sent GET /v1/models; the env_key providers sent none.
models_run() {
  _p3_v=$1
  CX_HOME=$RUN/cx-models-$_p3_v
  if [ "$_p3_v" = d ]; then cx_config "$_p3_v" "$CX_HOME" catalog; else cx_config "$_p3_v" "$CX_HOME" helper; fi
  mkfifo "$RUN/rpc-$_p3_v.in"
  python3 -I "$RUN/rpc.py" "$RUN/rpc-$_p3_v.in" "$RUN/rpc-$_p3_v.out" "$RUN/rpc-$_p3_v.done" &
  _p3_dpid=$!
  _p3_rc=0
  run_codex app-server <"$RUN/rpc-$_p3_v.in" >"$RUN/rpc-$_p3_v.out" 2>"$RUN/rpc-$_p3_v.err" || _p3_rc=$?
  echo "$_p3_rc" >"$RUN/exec-models-$_p3_v.rc"
  : >"$RUN/rpc-$_p3_v.done"
  _p3_drc=0
  wait "$_p3_dpid" || _p3_drc=$?
  echo "$_p3_drc" >"$RUN/rpc-$_p3_v.status"
  ps_note "models-$_p3_v"
}

# Common turn flags (word-split on purpose).
T="exec --json -s workspace-write --enable apply_patch_freeform"

# a (probe): the tools Codex offers; the turn replies name its shell tool.
cx_config t "$H/.codex"
# shellcheck disable=SC2086 # T is a list of words
cx_run a $T pc3-turn-a
python3 -I - "$RUN" <<'PY'
import glob, json, os, sys
run = sys.argv[1]
tools = []
for p in sorted(glob.glob(os.path.join(run, "rec", "[0-9]*.json"))):
    rec = json.load(open(p))
    if isinstance(rec.get("body"), dict) and "pc3-turn-a" in json.dumps(rec["body"]):
        tools = [t for t in rec["body"].get("tools") or [] if isinstance(t, dict)]
        break
ARGS = {"exec_command": {"cmd": "echo pc3-tracer"}, "shell_command": {"command": "echo pc3-tracer"},
        "shell": {"command": ["echo", "pc3-tracer"]}}
shell = next((t["name"] for t in tools if t.get("type") == "function" and t.get("name") in ARGS), None)
patch = next((t.get("type") for t in tools if t.get("name") == "apply_patch"), None)
with open(os.path.join(run, "transport", "tools.txt"), "w") as f:
    f.write("tools=%s\nshell_tool=%s\napply_patch=%s\n" % (
        ",".join("%s:%s" % (t.get("type", "-"), t.get("name", "-")) for t in tools), shell or "none",
        patch if patch in ("custom", "function") else "none"))
USAGE = {"input_tokens": 10, "output_tokens": 1, "total_tokens": 11}
def sse(rid, items):
    resp = {"id": rid, "object": "response", "model": "{{MODEL}}"}
    ev = [{"type": "response.created", "response": dict(resp, status="in_progress", output=[])}]
    ev += [{"type": "response.output_item.done", "output_index": i, "item": it} for i, it in enumerate(items)]
    ev.append({"type": "response.completed", "response": dict(resp, status="completed", output=items, usage=USAGE)})
    return "".join("event: %s\ndata: %s\n\n" % (e["type"], json.dumps(e)) for e in ev)
def reasoning(n):
    return {"type": "reasoning", "id": "rs_pc3_%d" % n,
            "summary": [{"type": "summary_text", "text": "pc3 reasoning %s" % ("one" if n == 1 else "two")}],
            "encrypted_content": "pc3-opaque-reasoning-%d" % n}
replies = {
    "turn-fc": sse("resp_pc3_fc", [{"type": "function_call", "id": "fc_pc3_1", "call_id": "call_pc3_1",
                                    "name": shell or "pc3-no-shell-tool",
                                    "arguments": json.dumps(ARGS.get(shell, {})), "status": "completed"}]),
    "turn-patch": sse("resp_pc3_patch", [reasoning(1), {
        "type": "custom_tool_call", "id": "ctc_pc3_1", "call_id": "call_pc3_2", "name": "apply_patch",
        "input": "*** Begin Patch\n*** Add File: pc3.txt\n+pc3 patch line\n*** End Patch\n", "status": "completed"}]),
    "turn-final": sse("resp_pc3_final", [reasoning(2), {
        "type": "message", "id": "msg_pc3_final", "role": "assistant", "status": "completed",
        "content": [{"type": "output_text", "text": "pc3 done", "annotations": []}]}]),
}
for name, text in replies.items():
    with open(os.path.join(run, "sse", name + ".sse"), "w") as f:
        f.write(text)
PY

# b (turns), c (resume): one CODEX_HOME with a.
# shellcheck disable=SC2086 # T is a list of words
cx_run b $T pc3-turn-b
# shellcheck disable=SC2086 # T is a list of words
cx_run c $T resume --last pc3-turn-c

# h (key helper): no env_key; the bearer comes from the auth command.
CX_HOME=$RUN/cx-helper
cx_config h "$CX_HOME" helper
cx_run h exec --json pc3-turn-h

# e1, e2 (context too long: HTTP 400, then response.failed).
for _p3_e in e1 e2; do
  CX_HOME=$RUN/cx-$_p3_e
  cx_config "$_p3_e" "$CX_HOME"
  cx_run "$_p3_e" exec --json "pc3-turn-$_p3_e"
done

# b, c, d model lists (d: model_catalog_json).
for _p3_m in b c d; do models_run "$_p3_m"; done

stop_mock
codex_record proxy

rm -rf "$CODEX_FX/requests" "$CODEX_FX/transport" "$CODEX_FX/models" "$CODEX_FX/errors"
python3 -I - "$RUN" "$RUN/out" "$(date +%Y-%m-%d)" <<'PY'
import glob, json, os, re, shutil, sys
run, out, day = sys.argv[1:4]
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
    recs.append(json.load(open(p)))
recs.sort(key=lambda r: r["seq"])
def var(r):
    return (r.get("headers") or {}).get("x-pc3-variant", "-")
def items(r):
    b = r.get("body")
    i = b.get("input") if isinstance(b, dict) else None
    return [x for x in i if isinstance(x, dict)] if isinstance(i, list) else []
def has_type(r, t, call_id=None):
    return any(x.get("type") == t and (call_id is None or x.get("call_id") == call_id) for x in items(r))
def user_msg(r, word):
    return any(x.get("role") == "user" and x.get("type", "message") == "message"
               and word in json.dumps(x.get("content")) for x in items(r))
def any_msg(r, word):
    return any(x.get("type", "message") == "message" and word in json.dumps(x.get("content")) for x in items(r))
turns = [r for r in recs if r.get("method") == "POST" and re.search(r"/v1/responses(\?|$)", r.get("path", ""))]
CLASSES = [
    ("first-turn", lambda r: user_msg(r, "pc3-turn-b") and not has_type(r, "function_call_output")),
    ("after-function-call-output", lambda r: has_type(r, "function_call_output", "call_pc3_1")
     and not has_type(r, "custom_tool_call_output")),
    ("after-custom-tool-call-output", lambda r: has_type(r, "custom_tool_call_output", "call_pc3_2")
     and not any_msg(r, "pc3-turn-c")),
    ("turn-with-reasoning", lambda r: user_msg(r, "pc3-turn-c") and has_type(r, "reasoning")),
]
found, missing = {}, []
for name, test in CLASSES:
    r = next((r for r in turns if test(r)), None)
    if r is None:
        missing.append(name)
    else:
        found[name] = r
        put("requests/%s.json" % name, json.dumps(r, indent=2) + "\n")

# PC13: the items before the prompt in the first turn.
def text_of(x):
    c = x.get("content")
    if isinstance(c, list):
        t = "\n".join(p.get("text", "") for p in c if isinstance(p, dict) and isinstance(p.get("text"), str))
        if t:
            return t
    return json.dumps(x)
# The run paths get scrub's names before a line is cut: a cut path no longer matches scrub's rule.
RP = []
for sub, repl in (("repo", "/repo"), ("home", "/home/user"), ("", "/tmp/run")):
    p = os.path.realpath(os.path.join(run, sub)) if sub else os.path.realpath(run)
    RP += [(p, repl)] + [("/" + p[len("/private/"):], repl)] * p.startswith("/private/")
def local(t):
    for a, b in RP:
        t = t.replace(a, b)
    return t
lines, tracer, before = [], "none", 0
ft = found.get("first-turn")
if ft is None:
    lines.append("first-turn: none")
else:
    its = items(ft)
    k = next((i for i, x in enumerate(its) if "pc3-turn-b" in json.dumps(x)), len(its))
    before = k
    for i, x in enumerate(its[:k]):
        first = (local(text_of(x)).splitlines() or [""])[0][:120]
        lines.append("%d %s %s %s" % (i, x.get("type", "-"), x.get("role", "-"), first))
        if tracer == "none" and "pc3-agents-tracer" in json.dumps(x):
            tracer = str(i)
lines.append("agents_tracer_in=%s" % tracer)
put("requests/injected-items.txt", "\n".join(lines) + "\n")

# PC2: the provider headers on every request.
hl, sok, eok, uab = [], 0, 0, 0
for r in recs:
    h = r.get("headers") or {}
    st, en = h.get("x-pc3-static"), h.get("x-pc3-env")
    sok += st == "pc3-static-value"
    eok += en == "xterm-256color"
    uab += "x-pc3-unset" not in h
    hl.append("%s %s %s variant=%s static=%s env=%s unset=%s" % (
        r["seq"], r.get("method"), r.get("path"), var(r), st if st is not None else "absent",
        en if en is not None else "absent", "present" if "x-pc3-unset" in h else "absent"))
hl.append("requests=%d static_ok=%d env_ok=%d unset_absent=%d" % (len(recs), sok, eok, uab))
put("requests/http-headers.txt", "\n".join(hl) + "\n")

# PC1: transport.
counts = {}
for r in recs:
    path, _, q = (r.get("path") or "").partition("?")
    keys = ",".join(sorted({kv.partition("=")[0] for kv in q.split("&") if kv})) or "-"
    k = (r.get("method"), path, keys)
    counts[k] = counts.get(k, 0) + 1
put("transport/paths.txt", "".join("%d %s %s %s\n" % ((n,) + k) for k, n in sorted(counts.items())) or "none\n")
hosts = {}
for line in rd(os.path.join(run, "rec", "connect.txt")).splitlines():
    if line.strip():
        hosts[line] = hosts.get(line, 0) + 1
put("transport/other-hosts.txt", "".join("%d %s\n" % (n, l) for l, n in sorted(hosts.items())) or "none\n")
put("transport/processes.txt", rd(os.path.join(run, "processes.txt"), "none\n"))
put("transport/tools.txt", rd(os.path.join(run, "transport", "tools.txt"), "none\n"))
helper_runs = len(rd(os.path.join(run, "key-helper.ran")).splitlines())
hrecs = [r for r in recs if var(r) == "h"]
with_bearer = sum(1 for r in hrecs if r.get("credential_headers") == ["authorization"]
                  and r.get("reply") != "401-no-credential")
no_cred = sum(1 for r in recs if r.get("reply") == "401-no-credential")
put("transport/key-helper.txt", "helper_runs=%d\nrequests=%d\nwith_bearer=%d\nno_credential=%d\n"
    % (helper_runs, len(hrecs), with_bearer, no_cred))

# PC7: models.
mrecs = [r for r in recs if r.get("method") == "GET" and "/models" in (r.get("path") or "")]
if mrecs:
    put("models/request.json", json.dumps(mrecs[0], indent=2) + "\n")
for n in ("catalog-shape.json", "openai-shape.json", "catalog-file.json"):
    put("models/" + n, rd(os.path.join(run, "json", n)))
ml = ["models_requests=%d %s" % (len(mrecs), " ".join(sorted({r["path"] for r in mrecs})) or "-")]
listed = {}
for v, slug in (("b", "gpt-mock-listed"), ("c", "gpt-mock-listed"), ("d", "gpt-mock-file")):
    text = rd(os.path.join(run, "rpc-%s.out" % v))
    put("models/picker-%s.jsonl" % v, text)
    ok = False
    for line in text.splitlines():
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if isinstance(d, dict) and d.get("id") == 2 and "result" in d and slug in json.dumps(d["result"]):
            ok = True
    listed[v] = ok
    ml.append("%s: models_request=%d listed=%s driver=%s" % (
        v, sum(1 for r in mrecs if var(r) == v), "yes" if ok else "no",
        rd(os.path.join(run, "rpc-%s.status" % v), "none").strip() or "none"))
put("models/result.txt", "\n".join(ml) + "\n")

# PC14: errors.
put("errors/context-400.json", rd(os.path.join(run, "json", "context-400.json")))
el, words = [], False
for e in ("e1", "e2"):
    o, er = rd(os.path.join(run, "exec-%s.out" % e)), rd(os.path.join(run, "exec-%s.err" % e))
    put("errors/%s-exec.txt" % e, o + er)
    cw = "context window" in (o + er).lower()
    words = words or cw
    el.append("%s: exit=%s codex_words=%s requests=%d" % (
        e, rd(os.path.join(run, "exec-%s.rc" % e), "none").strip() or "none", "yes" if cw else "no",
        sum(1 for r in recs if var(r) == e)))
put("errors/result.txt", "\n".join(el) + "\n")

# The five "Observed" lines.
def line(pc, ok, paths):
    return "- %s: %s %s: %s" % (pc, "verified" if ok else "failed", day,
                                " ".join("fixtures/codex/" + p for p in paths))
pc7 = ["models/result.txt", "models/catalog-shape.json", "models/openai-shape.json", "models/catalog-file.json"]
if mrecs:
    pc7.append("models/request.json")
obs = [
    line("PC1", helper_runs >= 1 and len(hrecs) >= 1 and with_bearer == len(hrecs) and no_cred == 0,
         ["transport/paths.txt", "transport/key-helper.txt", "transport/other-hosts.txt", "transport/processes.txt"]),
    line("PC2", len(recs) > 0 and sok == eok == uab == len(recs), ["requests/http-headers.txt"]),
    line("PC7", len(mrecs) >= 1 and (listed["b"] or listed["c"]) and listed["d"], pc7),
    line("PC13", before >= 1 and tracer != "none", ["requests/injected-items.txt", "requests/first-turn.json"]),
    line("PC14", words, ["errors/result.txt", "errors/context-400.json", "errors/context-failed.sse",
                         "errors/e1-exec.txt", "errors/e2-exec.txt"]),
]
put("runs/proxy/observed.txt", "\n".join(obs) + "\n")
with open(os.path.join(run, "missing.txt"), "w") as f:
    f.write(" ".join(missing))
PY

# Publish: every analysis file through scrub; the turn replies through publish_sse.
(cd "$RUN/out" && find . -type f) | while read -r _p3_f; do
  _p3_f=${_p3_f#./}
  mkdir -p "$CODEX_FX/$(dirname "$_p3_f")"
  scrub "$RUN/out/$_p3_f" >"$CODEX_FX/$_p3_f"
done
scrub "$RUN/sse/context-failed.sse" >"$CODEX_FX/errors/context-failed.sse"
publish_sse "$CODEX_FX/transport/replies"
rm -f "$CODEX_FX/transport/replies/context-failed.sse"

_p3_missing=$(cat "$RUN/missing.txt")
if [ -n "$_p3_missing" ]; then
  echo "codex proxy: missing request classes: $_p3_missing"
  exit 1
fi
echo "codex proxy: ok (codex $CODEX_VERSION)"

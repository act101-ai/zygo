#!/bin/sh
# Codex proxy-facing part (node PC-3): what Codex sends to a Responses provider at the mock.
#   PC1   transport: paths called, the key-helper bearer (provider auth command, run h), other
#         hosts (CONNECT lines, each must be refused), processes left after each run
#                                                                   -> fixtures/codex/transport/
#   PC2   provider http_headers and env_http_headers on each request -> requests/http-headers.txt
#   PC7   the /models request and the list shapes Codex accepts (app-server model/list, the list
#         the picker shows; provider-served and bundled models told apart) -> fixtures/codex/models/
#   PC13  the parts Codex puts before the user prompt (environment context, AGENTS.md)
#                                                                   -> requests/injected-items.txt
#   PC14  the context-too-long shapes (HTTP 400, response.failed)   -> fixtures/codex/errors/
# The four turn records (first turn, after function_call_output, after a custom_tool_call_output
# of the offered freeform apply_patch, a turn with a reasoning item) are
# fixtures/codex/requests/*.json. Runs a, b, c use no model catalog (Codex's own base prompt and
# reasoning for gpt-mock): first turn, after function_call_output, turn with reasoning. Run p alone
# uses catalog-patch.json, which turns on the freeform apply_patch tool and copies run b's base
# prompt and (no) reasoning effort; requests/authored.txt states this per record. Every mock reply
# is authored here (never model output); every fixture is a scrubbed copy. All fixtures are built
# in the run folder ($RUN/fx) and replace the fixture folders only at the end, after every class
# and verdict is computed, by a swap that restores the old set on any failure.
PREMISES_DIR=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck disable=SC2034 # lib.sh reads N2_AGENTS (unexported on purpose)
N2_AGENTS=codex
# shellcheck disable=SC1091 # lib.sh is linted on its own
. "$PREMISES_DIR/lib.sh"
mkdir -p "$CODEX_FX"
new_run codex-proxy
echo 'pc3-agents-tracer: project instructions for the PC13 capture.' >"$R/AGENTS.md"
mkdir -p "$RUN/json" "$RUN/sse" "$RUN/transport" "$RUN/out" "$RUN/fx"

# Authored files and the mock rules (first match wins).
python3 -I - "$RUN" <<'PY'
import json, os, sys
run = sys.argv[1]
def w(rel, obj):
    with open(os.path.join(run, rel), "w") as f:
        f.write(obj if isinstance(obj, str) else json.dumps(obj) + "\n")
def m(slug, **extra):
    return dict({"slug": slug, "display_name": "PC3 " + slug, "description": "authored for PC-3",
                 "supported_reasoning_levels": [{"effort": "low", "description": "low"}],
                 "default_reasoning_level": "low", "shell_type": "shell_command", "visibility": "list",
                 "supported_in_api": True, "priority": 1, "support_verbosity": False,
                 "truncation_policy": {"mode": "bytes", "limit": 10000}, "experimental_supported_tools": [],
                 "base_instructions": "PC3 base instructions."}, **extra)
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
    {"name": "p", "path": R, "body": "pc3-turn-p", "sse": "sse/turn-patch.sse"},
    {"name": "fc", "path": R, "body": "pc3-turn-b", "sse": "sse/turn-fc.sse"},
    {"name": "models-b", "path": "/v1/models", "header": {"x-pc3-variant": "^b$"}, "json": "json/catalog-shape.json"},
    {"name": "models-c", "path": "/v1/models", "header": {"x-pc3-variant": "^c$"}, "json": "json/openai-shape.json"}])
PY
# The key helper's token (PC-10's MOCK_ALT_KEY): built at run time, a shell variable (never exported,
# never in the repo); the mock accepts it beside the dummy key and records "credential": "alt".
MOCK_ALT_KEY="pc3-alt-$(od -An -N12 -tx1 /dev/urandom | tr -d ' \n')"
start_mock "$RUN/rules.json"

# cx_config <variant> <dir> [helper|catalog|patch]: <dir>/config.toml, written whole. helper: the
# bearer comes from $RUN/key-helper-<variant>.sh (no env_key; mode 700, removed at the end and by
# the traps), which logs its variant to $RUN/key-helper.ran and prints MOCK_ALT_KEY. catalog:
# model_catalog_json = catalog-file.json (model list d); patch: catalog-patch.json (run p).
cx_config() {
  mkdir -p "$2"
  if [ "${3:-}" = helper ]; then
    (umask 077 && printf '%s\n' '#!/bin/sh' "echo $1 >>'$RUN/key-helper.ran'" "echo '$MOCK_ALT_KEY'" >"$RUN/key-helper-$1.sh")
    chmod 700 "$RUN/key-helper-$1.sh"
  fi
  {
    echo 'model = "gpt-mock"'
    echo 'model_provider = "mock"'
    [ "${3:-}" != catalog ] || echo "model_catalog_json = \"$RUN/json/catalog-file.json\""
    [ "${3:-}" != patch ] || echo "model_catalog_json = \"$RUN/json/catalog-patch.json\""
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
      echo "command = \"$RUN/key-helper-$1.sh\""
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
  (run_codex "$@") </dev/null >"$RUN/exec-$_p3_x.out" 2>"$RUN/exec-$_p3_x.err" || _p3_rc=$?
  p3_refused "$RUN/exec-$_p3_x.out"
  echo "$_p3_rc" >"$RUN/exec-$_p3_x.rc"
  ps_note "$_p3_x"
}

# The app-server driver: initialize, then model/list over the FIFO. It opens the FIFO without
# blocking (retried while no reader, so it never waits on a server that never started) and stops
# early once <done> exists (the server has exited: no answer can come). Bounds: 60 s to open,
# 60 s per answer.
cat >"$RUN/rpc.py" <<'PY'
import errno, json, os, sys, time
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
fd = None
for _ in range(300):
    try:
        fd = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
        break
    except OSError as e:
        if e.errno != errno.ENXIO or os.path.exists(done):
            sys.exit(1)
        time.sleep(0.2)
if fd is None:
    sys.exit(1)
os.set_blocking(fd, True)
try:
    with os.fdopen(fd, "w") as w:
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

# p3_tree <pid>: <pid> and all its descendants (pgrep -P, depth first), one per line.
p3_tree() {
  echo "$1"
  for _p3_c in $(pgrep -P "$1" 2>/dev/null); do p3_tree "$_p3_c"; done
}
# p3_kill_job <pid>: kill a job and every process under it (a subshell still in codex_env or
# run_codex, and their children), then this run's Codex copies.
p3_kill_job() {
  _p3_pids=$(p3_tree "$1")
  # shellcheck disable=SC2086 # a list of pids
  kill $_p3_pids 2>/dev/null || :
  pkill -f "$RUN/codex-pkg" 2>/dev/null || :
}
# p3_cleanup: the traps stop a running driver and app-server job and remove the key helpers.
_p3_dpid="" _p3_spid=""
p3_cleanup() {
  [ -z "$_p3_dpid" ] || kill "$_p3_dpid" 2>/dev/null || :
  [ -z "$_p3_spid" ] || p3_kill_job "$_p3_spid"
  rm -f "$RUN"/key-helper-*.sh
}
n2_trap_chain p3_cleanup
# p3_refused <file>: a run's stdout holds the harness's own refusal (run_codex and codex_env print
# "premises: ..." and exit; each run is a subshell, so that exit ends only the run): print it and
# exit 1. Then check_abort (the mock's ABORT file).
p3_refused() {
  if grep '^premises: ' "$1"; then exit 1; fi
  check_abort
}

# models_run <variant>: codex app-server with the variant's config, as a background job the shell
# waits on for at most 150 s (then the job, every process under it and this run's Codex copies
# are killed and the status is "bound"). The job writes $RUN/exec-models-<v>.rc when the server exits; that file is also
# the driver's <done>. The driver's status goes to $RUN/rpc-<v>.status.
models_run() {
  _p3_v=$1
  CX_HOME=$RUN/cx-models-$_p3_v
  if [ "$_p3_v" = d ]; then cx_config "$_p3_v" "$CX_HOME" catalog; else cx_config "$_p3_v" "$CX_HOME" helper; fi
  mkfifo "$RUN/rpc-$_p3_v.in"
  _p3_done=$RUN/exec-models-$_p3_v.rc
  python3 -I "$RUN/rpc.py" "$RUN/rpc-$_p3_v.in" "$RUN/rpc-$_p3_v.out" "$_p3_done" &
  _p3_dpid=$!
  {
    _p3_src=0
    (run_codex app-server) <"$RUN/rpc-$_p3_v.in" >"$RUN/rpc-$_p3_v.out" 2>"$RUN/rpc-$_p3_v.err" || _p3_src=$?
    echo "$_p3_src" >"$_p3_done"
  } &
  _p3_spid=$!
  _p3_i=0
  while [ ! -e "$_p3_done" ]; do
    _p3_i=$((_p3_i + 1))
    if [ "$_p3_i" -ge 750 ]; then
      p3_kill_job "$_p3_spid"
      echo bound >"$_p3_done"
      break
    fi
    sleep 0.2
  done
  wait "$_p3_spid" 2>/dev/null || :
  _p3_spid=""
  _p3_drc=0
  wait "$_p3_dpid" || _p3_drc=$?
  _p3_dpid=""
  echo "$_p3_drc" >"$RUN/rpc-$_p3_v.status"
  p3_refused "$RUN/rpc-$_p3_v.out"
  ps_note "models-$_p3_v"
}

# Common turn flags (word-split on purpose).
T="exec --json -s workspace-write --enable apply_patch_freeform"

# a (probe): the tools Codex offers (no catalog); the turn replies name its shell tool.
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

# b (turns), c (resume): one CODEX_HOME with a, no catalog.
# shellcheck disable=SC2086 # T is a list of words
cx_run b $T pc3-turn-b
# shellcheck disable=SC2086 # T is a list of words
cx_run c $T resume --last pc3-turn-c

# catalog-patch.json for run p: one gpt-mock entry. Authored: apply_patch_tool_type "freeform"
# (ModelInfo.apply_patch_tool_type, ApplyPatchToolType snake_case, codex-rs/protocol/src/
# openai_models.rs; the field that offers the freeform apply_patch tool, confirmed by the round 2
# real run). Copied from run b's first request, so the request keeps Codex's own values:
# base_instructions = the text of its first developer item (Codex's base prompt for gpt-mock);
# default_reasoning_level = null and supported_reasoning_levels = [] (run b sends no reasoning
# effort). The other fields are required ModelInfo fields (step 2.2) that a Responses request does
# not carry (names, priority, visibility, shell_type, truncation_policy, ...); run p's tools are
# compared with run a's in tools.txt.
python3 -I - "$RUN" <<'PY'
import glob, json, os, sys
run = sys.argv[1]
base = ""
for p in sorted(glob.glob(os.path.join(run, "rec", "[0-9]*.json"))):
    rec = json.load(open(p))
    body = rec.get("body")
    if isinstance(body, dict) and "pc3-turn-b" in json.dumps(body.get("input")):
        for x in body.get("input") or []:
            if isinstance(x, dict) and x.get("role") == "developer":
                parts = [c for c in x.get("content") or [] if isinstance(c, dict)]
                base = parts[0].get("text", "") if parts else ""
                break
        break
entry = {"slug": "gpt-mock", "display_name": "PC3 gpt-mock", "description": "authored for PC-3",
         "supported_reasoning_levels": [], "default_reasoning_level": None, "shell_type": "shell_command",
         "visibility": "list", "supported_in_api": True, "priority": 1, "support_verbosity": False,
         "truncation_policy": {"mode": "bytes", "limit": 10000}, "experimental_supported_tools": [],
         "base_instructions": base, "apply_patch_tool_type": "freeform"}
with open(os.path.join(run, "json", "catalog-patch.json"), "w") as f:
    json.dump({"models": [entry]}, f)
    f.write("\n")
PY

# p (apply_patch): its own CODEX_HOME with catalog-patch.json; the authored reply calls apply_patch.
CX_HOME=$RUN/cx-patch
cx_config p "$CX_HOME" patch
# shellcheck disable=SC2086 # T is a list of words
cx_run p $T pc3-turn-p

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

# b, c, d model lists (b, c: the key helper, since only a provider with an auth command sent
# GET /v1/models in the first real run; d: model_catalog_json).
for _p3_m in b c d; do models_run "$_p3_m"; done

stop_mock
# The isolation record goes to the run folder's fixture tree too (moved at the end).
_p3_fx=$CODEX_FX
CODEX_FX=$RUN/fx
codex_record proxy
CODEX_FX=$_p3_fx
# Each run was a subshell: the version comes from the isolation record (one version, or it failed).
CODEX_VERSION=$(sed -n 's/^codex_version=//p' "$RUN/fx/runs/proxy/isolation.txt")

python3 -I - "$RUN" "$RUN/out" "$(TZ=UTC date +%Y-%m-%d)" <<'PY'
import glob, json, os, re, sys
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
def tools(r):
    b = r.get("body")
    t = b.get("tools") if isinstance(b, dict) else None
    return [x for x in t if isinstance(x, dict)] if isinstance(t, list) else []
def has_type(r, t, call_id=None):
    return any(x.get("type") == t and (call_id is None or x.get("call_id") == call_id) for x in items(r))
def user_msg(r, word):
    return any(x.get("role") == "user" and x.get("type", "message") == "message"
               and word in json.dumps(x.get("content")) for x in items(r))
def any_msg(r, word):
    return any(x.get("type", "message") == "message" and word in json.dumps(x.get("content")) for x in items(r))
def patch_applied(r):
    # The offered tool is a custom (freeform) apply_patch, and Codex ran it: its output is not
    # Codex's rejection of an unknown custom tool.
    offered = any(t.get("type") == "custom" and t.get("name") == "apply_patch" for t in tools(r))
    outs = [x for x in items(r) if x.get("type") == "custom_tool_call_output" and x.get("call_id") == "call_pc3_2"]
    return offered and bool(outs) and not any("unsupported custom tool call" in json.dumps(x.get("output"))
                                              for x in outs)
turns = [r for r in recs if r.get("method") == "POST" and re.search(r"/v1/responses(\?|$)", r.get("path", ""))]
# Runs a, b, c (variant t) have no catalog; run p (variant p) alone has catalog-patch.json.
CLASSES = [
    ("first-turn", "b", lambda r: var(r) == "t" and user_msg(r, "pc3-turn-b")
     and not has_type(r, "function_call_output")),
    ("after-function-call-output", "b", lambda r: var(r) == "t" and has_type(r, "function_call_output", "call_pc3_1")
     and not has_type(r, "custom_tool_call_output")),
    ("after-custom-tool-call-output", "p", lambda r: var(r) == "p" and patch_applied(r)),
    ("turn-with-reasoning", "c", lambda r: var(r) == "t" and user_msg(r, "pc3-turn-c") and has_type(r, "reasoning")),
]
found, missing = {}, []
for name, _, test in CLASSES:
    r = next((r for r in turns if test(r)), None)
    if r is None:
        missing.append(name)
    else:
        found[name] = r
        put("requests/%s.json" % name, json.dumps(r, indent=2) + "\n")

# requests/authored.txt: per record, the run, whether a model catalog was used and which fields
# it set; for run p, whether the base prompt and reasoning equal run b's (Codex's own).
def first_dev(r):
    for x in items(r):
        if x.get("role") == "developer":
            parts = [c for c in x.get("content") or [] if isinstance(c, dict)]
            return parts[0].get("text") if parts else None
    return None
def reasoning_of(r):
    b = r.get("body")
    return b.get("reasoning") if isinstance(b, dict) else None
al = []
for name, run_id, _ in CLASSES:
    if name not in found:
        al.append("%s: none" % name)
    elif run_id != "p":
        al.append("%s: run=%s catalog=none" % (name, run_id))
    else:
        pr, ft0 = found[name], found.get("first-turn")
        same_b = "yes" if ft0 is not None and first_dev(pr) == first_dev(ft0) else "no"
        same_r = "yes" if ft0 is not None and reasoning_of(pr) == reasoning_of(ft0) else "no"
        al.append("%s: run=p catalog=models/catalog-patch.json set=apply_patch_tool_type:freeform "
                  "copied_from_run_b=base_instructions,default_reasoning_level:null,supported_reasoning_levels:[] "
                  "required_not_in_request=slug,display_name,description,shell_type,visibility,supported_in_api,"
                  "priority,support_verbosity,truncation_policy,experimental_supported_tools "
                  "base_instructions_as_run_b=%s reasoning_as_run_b=%s" % (name, same_b, same_r))
put("requests/authored.txt", "\n".join(al) + "\n")

# PC13: every part of every item before the prompt in the first turn.
# The run paths get scrub's names before a line is cut: a cut path no longer matches scrub's rule.
RP = []
for sub, repl in (("repo", "/repo"), ("home", "/home/user"), ("", "/tmp/run")):
    p = os.path.realpath(os.path.join(run, sub)) if sub else os.path.realpath(run)
    RP += [(p, repl)] + [("/" + p[len("/private/"):], repl)] * p.startswith("/private/")
def local(t):
    for a, b in RP:
        t = t.replace(a, b)
    return t
lines, tracer, envctx, before = [], "none", "none", 0
ft = found.get("first-turn")
if ft is None:
    lines.append("first-turn: none")
else:
    its = items(ft)
    k = next((i for i, x in enumerate(its) if "pc3-turn-b" in json.dumps(x)), len(its))
    before = k
    for i, x in enumerate(its[:k]):
        c = x.get("content")
        parts = [p for p in c if isinstance(p, dict)] if isinstance(c, list) else [x]
        for j, p in enumerate(parts):
            text = p.get("text") if isinstance(p.get("text"), str) else json.dumps(p)
            first = (local(text).splitlines() or [""])[0][:120]
            lines.append("%d.%d %s %s %s %s" % (i, j, x.get("type", "-"), x.get("role", "-"),
                                                p.get("type", "-") if p is not x else "-", first))
            # PC13 counts the parts of user-role items only (what Codex injects as the user).
            if x.get("role") == "user" and tracer == "none" and "pc3-agents-tracer" in text:
                tracer = "%d.%d" % (i, j)
            if x.get("role") == "user" and envctx == "none" and "<environment_context>" in text:
                envctx = "%d.%d" % (i, j)
lines.append("environment_context_in=%s" % envctx)
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
clines = [l for l in rd(os.path.join(run, "rec", "connect.txt")).splitlines() if l.strip()]
for line in clines:
    hosts[line] = hosts.get(line, 0) + 1
put("transport/other-hosts.txt", "".join("%d %s\n" % (n, l) for l, n in sorted(hosts.items())) or "none\n")
refused = sum(1 for l in clines if l.endswith(" refused"))
put("transport/processes.txt", rd(os.path.join(run, "processes.txt"), "none\n"))
# tools.txt: run a's lines (no catalog), then run p's tools (catalog-patch.json).
prec = next((r for r in turns if var(r) == "p"), None)
ptools = tools(prec) if prec is not None else []
ptype = next((t.get("type") for t in ptools if t.get("name") == "apply_patch"), None)
put("transport/tools.txt", rd(os.path.join(run, "transport", "tools.txt"), "none\n")
    + "p_tools=%s\np_apply_patch=%s\n" % (",".join("%s:%s" % (t.get("type", "-"), t.get("name", "-")) for t in ptools),
                                          ptype if ptype in ("custom", "function") else "none"))
ran = [l.strip() for l in rd(os.path.join(run, "key-helper.ran")).splitlines() if l.strip()]
helper_runs = sum(1 for l in ran if l == "h")
hrecs = [r for r in recs if var(r) == "h"]
with_token = sum(1 for r in hrecs if r.get("credential_headers") == ["authorization"]
                 and r.get("credential") == "alt" and r.get("reply") != "401-no-credential")
no_cred = sum(1 for r in recs if r.get("reply") == "401-no-credential")
put("transport/key-helper.txt", "helper_runs=%d\nhelper_runs_other=%d\nrequests=%d\nwith_helper_token=%d\nno_credential=%d\n"
    "token=MOCK_ALT_KEY, built at run time (records: credential=alt; the env_key runs: credential=primary)\n"
    % (helper_runs, len(ran) - helper_runs, len(hrecs), with_token, no_cred))
put("transport/result.txt", "connect_lines=%d refused=%d hosts=%s\n" % (
    len(clines), refused, ",".join(sorted({l.split(" ")[0] for l in clines})) or "-"))

# PC7: models. Picker runs are b, c, d; the exec runs' /models requests are counted apart.
PICK = ("b", "c", "d")
mall = [r for r in recs if r.get("method") == "GET" and "/models" in (r.get("path") or "")]
mrecs = [r for r in mall if var(r) in PICK]
if mrecs:
    put("models/request.json", json.dumps(mrecs[0], indent=2) + "\n")
for n in ("catalog-shape.json", "openai-shape.json", "catalog-file.json", "catalog-patch.json"):
    put("models/" + n, rd(os.path.join(run, "json", n)))
def ids_of(n):
    try:
        d = json.loads(rd(os.path.join(run, "json", n)))
    except ValueError:
        return set()
    return {x.get("slug") or x.get("id") for x in d.get("models") or d.get("data") or [] if isinstance(x, dict)}
ml = ["models_requests=%d %s" % (len(mrecs), " ".join(sorted({r["path"] for r in mrecs})) or "-"),
      "exec_models_requests=%d" % (len(mall) - len(mrecs))]
listed = {}
for v, slug, srcf in (("b", "gpt-mock-listed", "catalog-shape.json"), ("c", "gpt-mock-listed", "openai-shape.json"),
                      ("d", "gpt-mock-file", "catalog-file.json")):
    text = rd(os.path.join(run, "rpc-%s.out" % v))
    put("models/picker-%s.jsonl" % v, text)
    ok, names = False, []
    for line in text.splitlines():
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if isinstance(d, dict) and d.get("id") == 2 and "result" in d:
            ok = ok or slug in json.dumps(d["result"])
            data = d["result"].get("data") if isinstance(d["result"], dict) else None
            names = [x.get("id") or x.get("model") for x in data or [] if isinstance(x, dict)]
    listed[v] = ok
    served = ids_of(srcf)
    # served= ids the provider's /models reply named (b, c); file= ids from the catalog file (d).
    ml.append("%s: models_request=%d listed=%s driver=%s %s=%s bundled=%s" % (
        v, sum(1 for r in mrecs if var(r) == v), "yes" if ok else "no",
        rd(os.path.join(run, "rpc-%s.status" % v), "none").strip() or "none", "file" if v == "d" else "served",
        ",".join(x for x in names if x in served) or "-", ",".join(x for x in names if x not in served) or "-"))
put("models/result.txt", "\n".join(ml) + "\n")

# PC14: errors.
put("errors/context-400.json", rd(os.path.join(run, "json", "context-400.json")))
el, e_ok = [], True
for e in ("e1", "e2"):
    o, er = rd(os.path.join(run, "exec-%s.out" % e)), rd(os.path.join(run, "exec-%s.err" % e))
    put("errors/%s-exec.txt" % e, o + er)
    cw = "context window" in (o + er).lower()
    nreq = sum(1 for r in recs if var(r) == e)
    e_ok = e_ok and cw and nreq == 1
    el.append("%s: exit=%s codex_words=%s requests=%d" % (
        e, rd(os.path.join(run, "exec-%s.rc" % e), "none").strip() or "none", "yes" if cw else "no", nreq))
put("errors/result.txt", "\n".join(el) + "\n")

# The five "Observed" lines.
def line(pc, ok, paths):
    return "- %s: %s %s: %s" % (pc, "verified" if ok else "failed", day,
                                " ".join("fixtures/codex/" + p for p in paths))
pc7 = ["models/result.txt", "models/picker-b.jsonl", "models/picker-c.jsonl", "models/picker-d.jsonl",
       "models/catalog-shape.json", "models/openai-shape.json", "models/catalog-file.json"]
if mrecs:
    pc7.append("models/request.json")
obs = [
    line("PC1", helper_runs >= 1 and len(hrecs) >= 1 and with_token == len(hrecs) and no_cred == 0
         and refused == len(clines),
         ["transport/result.txt", "transport/paths.txt", "transport/key-helper.txt", "transport/other-hosts.txt",
          "transport/processes.txt"]),
    line("PC2", len(recs) > 0 and sok == eok == uab == len(recs), ["requests/http-headers.txt"]),
    line("PC7", len(mrecs) >= 1 and (listed["b"] or listed["c"]) and listed["d"], pc7),
    line("PC13", before >= 1 and tracer != "none" and envctx != "none",
         ["requests/injected-items.txt", "requests/first-turn.json"]),
    line("PC14", e_ok, ["errors/result.txt", "errors/context-400.json", "errors/context-failed.sse",
                        "errors/e1-exec.txt", "errors/e2-exec.txt"]),
]
put("runs/proxy/observed.txt", "\n".join(obs) + "\n")
with open(os.path.join(run, "missing.txt"), "w") as f:
    f.write(" ".join(missing))
PY

# Build the fixtures in $RUN/fx: every analysis file through scrub; the turn replies through
# publish_sse (context-failed.sse only under errors/).
(cd "$RUN/out" && find . -type f) | while read -r _p3_f; do
  _p3_f=${_p3_f#./}
  mkdir -p "$RUN/fx/$(dirname "$_p3_f")"
  scrub "$RUN/out/$_p3_f" >"$RUN/fx/$_p3_f"
done
scrub "$RUN/sse/context-failed.sse" >"$RUN/fx/errors/context-failed.sse"
publish_sse "$RUN/fx/transport/replies"
rm -f "$RUN/fx/transport/replies/context-failed.sse"

# The key helpers are no longer needed (the traps remove them too, on any exit).
rm -f "$RUN"/key-helper-*.sh

# p3_swap begin
# p3_swap: replace the five fixture folders as one step. The old ones are moved aside into
# $RUN/old, the new ones moved in from $RUN/fx, then the old ones deleted. On any failure the new
# ones moved in so far are removed and the old set is moved back; return 1.
P3_DIRS="requests transport models errors runs/proxy"
p3_swap() {
  mkdir -p "$RUN/old/runs" "$CODEX_FX/runs" || return 1
  _p3_ok=1 _p3_aside="" _p3_in=""
  for _p3_d in $P3_DIRS; do
    [ -e "$CODEX_FX/$_p3_d" ] || continue
    if mv "$CODEX_FX/$_p3_d" "$RUN/old/$_p3_d"; then _p3_aside="$_p3_aside $_p3_d"; else _p3_ok=0; break; fi
  done
  if [ "$_p3_ok" = 1 ]; then
    for _p3_d in $P3_DIRS; do
      if mv "$RUN/fx/$_p3_d" "$CODEX_FX/$_p3_d"; then _p3_in="$_p3_in $_p3_d"; else _p3_ok=0; break; fi
    done
  fi
  if [ "$_p3_ok" = 1 ]; then
    rm -rf "$RUN/old"
    return 0
  fi
  for _p3_d in $_p3_in; do rm -rf "${CODEX_FX:?}/$_p3_d"; done
  for _p3_d in $_p3_aside; do
    mv "$RUN/old/$_p3_d" "$CODEX_FX/$_p3_d" || echo "codex proxy: could not restore $_p3_d; it is in $RUN/old/$_p3_d"
  done
  echo "codex proxy: fixture swap failed; the old fixtures are back"
  return 1
}
# p3_swap end

_p3_missing=$(cat "$RUN/missing.txt")
if [ -n "$_p3_missing" ]; then
  echo "codex proxy: fixtures not replaced; this run's are in $RUN/fx"
  echo "codex proxy: missing request classes: $_p3_missing"
  exit 1
fi
# Every class and verdict is in: replace the fixture folders.
p3_swap || exit 1
echo "codex proxy: ok (codex $CODEX_VERSION)"

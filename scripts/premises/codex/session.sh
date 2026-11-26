#!/bin/sh
# Codex session part (node PC-5): one mock, then the captures of K§6.3.7 PC6 (agent role files, the
# model each subagent sends, the subagent marker of K§6.4.5), PC8 (auto compaction against a raised
# usage), PC9 (the model-visible name of an MCP tool), PC15 (skill folders) and PC11 (the TUI status
# line, in a pane of a private tmux server). Every Codex start goes through run_codex.
# Writes fixtures/codex/agents/, compaction/, mcp/, skills/, status/ and runs/session/.
# Usage: sh scripts/premises/run-codex.sh session
# Needs: codex (as run-codex.sh), python3, tmux 3.2 or later.
PREMISES_DIR=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck disable=SC2034 # lib.sh reads N2_AGENTS
N2_AGENTS=codex
# shellcheck disable=SC1091
. "$PREMISES_DIR/lib.sh"
new_run codex-session
for _d in agents compaction mcp status skills; do rm -rf "${CODEX_FX:?}/$_d"; done
mkdir -p "$RUN/out" "$RUN/sse" "$RUN/cap" "$RUN/mcp"
cat >"$RUN/rules.json" <<'JSON'
[
 {"name": "pc6-spawn-a", "path": "/v1/responses$", "model": "^gpt-mock$", "body": "tracer-pc6-main-a", "sse": "sse/pc6-spawn-a.sse", "times": 1},
 {"name": "pc6-spawn-b", "path": "/v1/responses$", "model": "^gpt-mock$", "body": "tracer-pc6-main-b", "sse": "sse/pc6-spawn-b.sse", "times": 1},
 {"name": "pc6-spawn-c", "path": "/v1/responses$", "model": "^gpt-mock$", "body": "tracer-pc6-main-c", "sse": "sse/pc6-spawn-c.sse", "times": 1},
 {"name": "pc6-wait-a", "path": "/v1/responses$", "model": "^gpt-mock$", "body": "tracer-pc6-main-a", "sse": "sse/pc6-wait.sse", "times": 1},
 {"name": "pc6-wait-b", "path": "/v1/responses$", "model": "^gpt-mock$", "body": "tracer-pc6-main-b", "sse": "sse/pc6-wait.sse", "times": 1},
 {"name": "pc6-wait-c", "path": "/v1/responses$", "model": "^gpt-mock$", "body": "tracer-pc6-main-c", "sse": "sse/pc6-wait.sse", "times": 1},
 {"name": "pc8-high", "path": "/v1/responses$", "body": "tracer-pc8-high-turn1", "sse": "sse/pc8-text.sse", "times": 1,
  "usage": {"input_tokens": 150000, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 150010}},
 {"name": "pc8-live-low", "path": "/v1/responses$", "body": "tracer-pc8-live-low-turn1", "sse": "sse/pc8-live-call.sse", "times": 1},
 {"name": "pc8-live-high", "path": "/v1/responses$", "body": "tracer-pc8-live-high-turn1", "sse": "sse/pc8-live-call.sse", "times": 1,
  "usage": {"input_tokens": 150000, "input_tokens_details": {"cached_tokens": 0}, "output_tokens": 10, "output_tokens_details": {"reasoning_tokens": 0}, "total_tokens": 150010}},
 {"name": "pc9-call", "path": "/v1/responses$", "body": "tracer-pc9-call", "sse": "sse/pc9-call.sse", "times": 1},
 {"name": "pc9-done", "path": "/v1/responses$", "body": "tracer-pc9-call", "sse": "sse/pc9-done.sse"},
 {"name": "pc11-reply", "path": "/v1/responses$", "body": "tracer-pc11-turn", "sse": "sse/pc11-text.sse"}
]
JSON

# pc5.py: the Responses SSE builders and the readers of the captured requests (stdlib only).
cat >"$RUN/pc5.py" <<'PY'
import json, os, re, sys


def sse(events):
    return "".join("event: %s\ndata: %s\n\n" % (e, json.dumps(d)) for e, d in events)


def write(path, text):
    with open(path, "w") as f:
        f.write(text)


def recs(d):
    """The records of a cap folder, in sequence order; each gets _seq (its file number)."""
    out = []
    if not d or not os.path.isdir(d):
        return out
    for n in sorted(os.listdir(d)):
        if re.fullmatch(r"[0-9]+\.json", n):
            with open(os.path.join(d, n)) as f:
                r = json.load(f)
            r["_seq"] = n[:-5]
            out.append(r)
    return out


def body(r):
    b = r.get("body")
    return b if isinstance(b, dict) else {}


def first_request(d):
    for r in recs(d):
        if r.get("method") == "POST" and str(r.get("path", "")).endswith("/v1/responses"):
            return r
    return None


def user_texts(b):
    out = []
    for it in b.get("input") or []:
        if not isinstance(it, dict) or it.get("role") != "user":
            continue
        c = it.get("content")
        if isinstance(c, str):
            out.append(c)
        elif isinstance(c, list):
            out += [p["text"] for p in c if isinstance(p, dict) and isinstance(p.get("text"), str)]
    return out


def lines(path, rows):
    write(path, "".join(r + "\n" for r in rows))


def pairs(args):
    return [a.split("=", 1) for a in args]


def sse_text(out, text):
    rid = "resp_pc5"
    item = {"type": "message", "id": "msg_pc5", "role": "assistant", "status": "completed",
            "content": [{"type": "output_text", "text": text, "annotations": []}]}
    write(out, sse([
        ("response.created", {"type": "response.created", "response": {
            "id": rid, "object": "response", "model": "{{MODEL}}", "status": "in_progress", "output": []}}),
        ("response.output_text.delta", {"type": "response.output_text.delta", "item_id": item["id"],
                                        "output_index": 0, "content_index": 0, "delta": text}),
        ("response.output_item.done", {"type": "response.output_item.done", "output_index": 0, "item": item}),
        ("response.completed", {"type": "response.completed", "response": {
            "id": rid, "object": "response", "model": "{{MODEL}}", "status": "completed", "output": [item],
            "usage": {"input_tokens": 10, "output_tokens": 1, "total_tokens": 11}}}),
    ]))


def sse_call(out, name, arguments, call_id):
    """<name> is a tool name, or <namespace>/<name> for a tool inside a namespace tool."""
    rid = "resp_pc5_" + call_id
    ns, _, name = name.rpartition("/")
    item = {"type": "function_call", "id": "fc_" + call_id, "call_id": call_id, "name": name,
            "arguments": arguments, "status": "completed"}
    if ns:
        item["namespace"] = ns
    write(out, sse([
        ("response.created", {"type": "response.created", "response": {
            "id": rid, "object": "response", "model": "{{MODEL}}", "status": "in_progress", "output": []}}),
        ("response.output_item.added", {"type": "response.output_item.added", "output_index": 0, "item": item}),
        ("response.output_item.done", {"type": "response.output_item.done", "output_index": 0, "item": item}),
        ("response.completed", {"type": "response.completed", "response": {
            "id": rid, "object": "response", "model": "{{MODEL}}", "status": "completed", "output": [item],
            "usage": {"input_tokens": 10, "output_tokens": 1, "total_tokens": 11}}}),
    ]))


def tools(cap, out):
    res = []

    def flat(ts, ns):
        for t in ts:
            if not isinstance(t, dict):
                continue
            e = {"name": t.get("name"), "type": t.get("type"), "description": t.get("description"),
                 "parameters": t.get("parameters")}
            if ns is not None:
                e["namespace"] = ns
            res.append(e)
            if isinstance(t.get("tools"), list):
                flat(t["tools"], t.get("name"))

    r = first_request(cap)
    if r is not None:
        flat(body(r).get("tools") or [], None)
    write(out, json.dumps(res, indent=2) + "\n")


def load(path):
    with open(path) as f:
        return json.load(f)


def full(e):
    """A tool's call name: <name>, or <namespace>/<name> inside a namespace tool (Codex 0.172 puts
    spawn_agent in multi_agent_v1 and an MCP server's tools in mcp__<server>)."""
    return "%s/%s" % (e["namespace"], e["name"]) if e.get("namespace") else e["name"]


def pick_spawn(path):
    for e in load(path):
        props = (e.get("parameters") or {}).get("properties") or {}
        if (e.get("type") == "function" and str(e.get("name") or "").startswith("spawn")
                and "agent_type" in props and "message" in props):
            print(full(e))
            return


def pick_mcp(path):
    for e in load(path):
        n = str(e.get("name") or "")
        if e.get("type") == "function" and "zygo" in full(e) and n.endswith("compact"):
            print(full(e))
            return


def agents(models_out, marker_out, runs):
    models, marker = [], []
    for run, cap in pairs(runs):
        first = {}
        for r in recs(cap):
            b = body(r)
            texts = user_texts(b)
            if any(re.search(r"tracer-pc6-task-[abc]", t) for t in texts):
                kind = "sub"
            elif any(re.search(r"tracer-pc6-main-[abc]", t) for t in texts):
                kind = "main"
            else:
                kind = "other"
            first.setdefault(kind, r)
            dev = list(dict.fromkeys(re.findall(r"tracer-pc6-[a-z]+-dev", json.dumps(b))))
            models.append("%s %s %s %s dev=%s" % (run, r["_seq"], b.get("model") or "none", kind,
                                                 ",".join(dev) or "none"))
        sub = first.get("sub")
        models.append("%s: sub_model=%s" % (run, (body(sub).get("model") or "none") if sub else "none"))
        main = first.get("main")
        if not (main and sub):
            marker.append("%s marker-candidates: no sub request" % run)
            continue
        skip = ("authorization", "x-api-key", "content-length")
        mh = {k: v for k, v in (main.get("headers") or {}).items() if k not in skip}
        sh = {k: v for k, v in (sub.get("headers") or {}).items() if k not in skip}
        only = []
        for k in sorted(set(mh) | set(sh)):
            if mh.get(k) != sh.get(k):
                marker.append("%s header %s: main=%s sub=%s" % (run, k, mh.get(k, "absent"), sh.get(k, "absent")))
                if k not in mh:
                    only.append(k)
        mb, sb = body(main), body(sub)
        for k in sorted(set(mb) ^ set(sb)):
            marker.append("%s body-key %s: main=%s sub=%s" % (run, k, "present" if k in mb else "absent",
                                                             "present" if k in sb else "absent"))
            if k not in mb:
                only.append(k)
        marker.append("%s marker-candidates: %s" % (run, " ".join(only) or "none"))
    lines(models_out, models)
    lines(marker_out, marker)


def meta(r):
    """The x-codex-turn-metadata header as a dict ({} when absent or not JSON)."""
    try:
        m = json.loads((r.get("headers") or {}).get("x-codex-turn-metadata") or "{}")
    except ValueError:
        return {}
    return m if isinstance(m, dict) else {}


def compaction(out, caps):
    """Per record: the request kind and history from the turn metadata, the compact_prompt tracer,
    the turn tracer, and whether the input carries a tool output. Per capture: compaction requests
    by request_kind (or a /responses/compact path) and by the tracer; the condition (cold_resume
    when a request resumes the session, else live); and whether a compaction came before the next
    request: turn 2 on a cold resume; in a live session the first request after the first one that
    is not a compaction and has the first one's turn_id (the follow-up; after a compaction its input
    is the compacted history, so it need not carry the tool output: next_tool_output says)."""
    rows = []
    for capture, cap in pairs(caps):
        n = n_prompt = 0
        first_comp = nxt = None
        rs = recs(cap)
        turn0 = meta(rs[0]).get("turn_id") if rs else None
        cold = any(meta(r).get("history_initialization") == "cold_resume" for r in rs)
        nxt_tool = "none"
        for i, r in enumerate(rs):
            b = body(r)
            m = meta(r)
            prompt = "tracer-pc8-compact-prompt" in json.dumps(b)
            texts = user_texts(b)
            t = re.search(r"tracer-pc8-(?:live-)?(?:low|high)-(turn1|turn2)", texts[-1]) if texts else None
            items = b.get("input") if isinstance(b.get("input"), list) else []
            types = [(it.get("type") or ("message" if "role" in it else "?")) if isinstance(it, dict) else "?"
                     for it in items]
            tool_out = "function_call_output" in types
            kind = m.get("request_kind") or "none"
            comp = kind == "compaction" or "/responses/compact" in str(r.get("path", ""))
            rows.append("%s %s %s %s model=%s request_kind=%s history=%s compact_prompt=%s turn=%s tool_output=%s input_items=%d item_types=%s" % (
                capture, r["_seq"], r.get("method"), r.get("path"), b.get("model") or "none", kind,
                m.get("history_initialization") or "none", "yes" if prompt else "no", t.group(1) if t else "none",
                "yes" if tool_out else "no", len(items), ",".join(types) or "none"))
            n += comp
            n_prompt += prompt
            if comp and first_comp is None:
                first_comp = i
            if nxt is None and i > 0 and not comp and (
                    (t and t.group(1) == "turn2") if cold else (turn0 is None or m.get("turn_id") == turn0)):
                nxt = i
                nxt_tool = "yes" if tool_out else "no"
        before = "yes" if first_comp is not None and nxt is not None and first_comp < nxt else "no"
        rows.append("%s: compaction_requests=%d compact_prompt_requests=%d condition=%s next_request=%s next_tool_output=%s compacted_before_next=%s" % (
            capture, n, n_prompt, "cold_resume" if cold else ("live" if rs else "none"),
            ("turn2" if cold else "followup") if nxt is not None else "absent", nxt_tool, before))
    lines(out, rows)


def roles(tools_json, out):
    """The roles the spawn tool's agent_type description lists: <role>: model=<model or inherited>
    description=<first line>."""
    rows = []
    for e in load(tools_json):
        props = (e.get("parameters") or {}).get("properties") or {}
        if not str(e.get("name") or "").startswith("spawn") or "agent_type" not in props:
            continue
        d = str(props["agent_type"].get("description") or "")
        for name, text in re.findall(r"(?m)^([A-Za-z0-9_.-]+): \{\n(.*?)\n\}", d, re.S):
            mm = re.search(r"model is set to `([^`]+)`", text)
            rows.append("%s: model=%s description=%s" % (name, mm.group(1) if mm else "inherited",
                                                        text.strip().splitlines()[0] if text.strip() else ""))
        break
    lines(out, rows)


def toml_pub(src, out):
    """A scrubbed config.toml with a table repeated with the same text dropped (scrub can map two
    project paths to one), then checked: tomllib when present, else no [table] header twice."""
    with open(src) as f:
        text = f.read()
    blocks, cur = [], []
    for line in text.splitlines(True):
        if re.match(r"\s*\[", line) and cur:
            blocks.append(cur)
            cur = []
        cur.append(line)
    blocks.append(cur)
    seen, keep = set(), []
    for bl in blocks:
        key = "".join(bl).strip()
        if re.match(r"\[[^\[]", key) and key in seen:
            continue
        seen.add(key)
        keep.append("".join(bl))
    res = "".join(keep)
    try:
        import tomllib
        tomllib.loads(res)
    except ImportError:
        heads = re.findall(r"(?m)^\s*\[([^\[\]]+)\]\s*$", res)
        dup = sorted({h for h in heads if heads.count(h) > 1})
        if dup:
            sys.exit("session: %s: a repeated table: %s" % (out, ", ".join(dup)))
    except ValueError as e:
        sys.exit("session: %s does not parse: %s" % (out, e))
    write(out, res)


def mcp(out, tools_json, name, cap):
    rs = recs(cap) if cap else []
    found = any("tracer-mcp-probe-result" in json.dumps(r.get("body")) for r in rs)
    lines(out, ["tool=%s" % (name or "none"), "call_output=%s" % ("found" if found else "absent"),
                "requests=%d" % len(rs)])


def skills(out, cap):
    r = first_request(cap)
    text = json.dumps(body(r)) if r is not None else ""
    # A whole token: pc15-home is not seen inside tracer-pc15-home-desc.
    seen = lambda s: "seen" if re.search(r"(?<![A-Za-z0-9_-])%s(?![A-Za-z0-9_-])" % re.escape(s), text) else "absent"
    lines(out, ["%s: name=%s description=%s body=%s" % (loc, seen("pc15-" + loc), seen("tracer-pc15-%s-desc" % loc),
                                                         seen("tracer-pc15-%s-body" % loc))
                for loc in ("home", "repo", "path")])


def status_row(pane, exit_file):
    """(status row, exit): the last non-blank row of the pane, or the row above the key-hint footer
    ("? for shortcuts") when the last row is that footer; the TUI's exit status, or running."""
    with open(pane, encoding="utf-8", errors="replace") as f:
        rows = [l for l in f.read().splitlines() if l.strip()]
    if len(rows) > 1 and "? for shortcuts" in rows[-1]:
        rows.pop()
    try:
        with open(exit_file) as f:
            ex = f.read().strip()
    except OSError:
        ex = "running"
    return (rows[-1].strip() if rows else ""), ex


def status(out, pane, exit_file, control_pane, control_exit):
    """The configured run (tui.status_line = ["model-with-reasoning"]) against the control run (no
    status_line key: Codex's default items)."""
    row, ex = status_row(pane, exit_file)
    crow, cex = status_row(control_pane, control_exit)
    shown = lambda r, s: "yes" if s in r else "no"
    lines(out, ["configured=model-with-reasoning", "status_row=%s" % row,
                "model_shown=%s" % shown(row, "gpt-mock"), "dir_shown=%s" % shown(row, "/repo"),
                "tui_exit=%s" % ex, "control=default", "control_row=%s" % crow,
                "control_model_shown=%s" % shown(crow, "gpt-mock"), "control_dir_shown=%s" % shown(crow, "/repo"),
                "control_tui_exit=%s" % cex, "differs=%s" % ("yes" if row != crow else "no")])


cmd, a = sys.argv[1], sys.argv[2:]
if cmd == "sse-text":
    sse_text(*a)
elif cmd == "sse-call":
    sse_call(*a)
elif cmd == "tools":
    tools(*a)
elif cmd == "pick-spawn":
    pick_spawn(*a)
elif cmd == "pick-mcp":
    pick_mcp(*a)
elif cmd == "agents":
    agents(a[0], a[1], a[2:])
elif cmd == "compaction":
    compaction(a[0], a[1:])
elif cmd == "mcp":
    mcp(*a)
elif cmd == "skills":
    skills(*a)
elif cmd == "status":
    status(*a)
elif cmd == "roles":
    roles(*a)
elif cmd == "toml-pub":
    toml_pub(*a)
else:
    sys.exit("pc5.py: unknown command " + cmd)
PY

# cx_home <name> <top-level lines> <table lines>: CX_HOME=$RUN/cx-<name> and its config.toml: the
# top-level lines, the 8 lines codex_env writes (same bytes), the repo trusted under its path and
# its real path, then the tables. An empty argument adds nothing.
cx_home() {
  CX_HOME=$RUN/cx-$1
  mkdir -p "$CX_HOME"
  _rr=$(n2_realpath "$R")
  {
    [ -z "$2" ] || printf '%s\n' "$2"
    printf '%s\n' 'model = "gpt-mock"' 'model_provider = "mock"' '' \
      '[model_providers.mock]' 'name = "mock"' "base_url = \"$MOCK_URL/v1\"" 'env_key = "MOCK_API_KEY"' \
      'wire_api = "responses"'
    printf '\n[projects."%s"]\ntrust_level = "trusted"\n' "$R"
    [ "$_rr" = "$R" ] || printf '\n[projects."%s"]\ntrust_level = "trusted"\n' "$_rr"
    [ -z "$3" ] || printf '\n%s\n' "$3"
  } >"$CX_HOME/config.toml"
}
# exec_run <name> <codex args...>: one Codex run, output in $RUN/out/<name>.{jsonl,err,rc}. A
# non-zero Codex status never ends the part; an isolation failure (codex_env exits) does, and the
# trap then shows what that run printed.
# The trap runs inside exec_run's redirections, so it writes to fd 9, the part's own stderr.
EXEC_CUR=""
exec 9>&2
exec_show() { [ -z "$EXEC_CUR" ] || cat "$RUN/out/$EXEC_CUR.jsonl" "$RUN/out/$EXEC_CUR.err" 2>/dev/null >&9 || true; }
n2_trap_chain exec_show
exec_run() {
  EXEC_CUR=$1; shift
  _rc=0
  run_codex "$@" </dev/null >"$RUN/out/$EXEC_CUR.jsonl" 2>"$RUN/out/$EXEC_CUR.err" || _rc=$?
  echo "$_rc" >"$RUN/out/$EXEC_CUR.rc"
  EXEC_CUR=""
}
# take <capture>: move every request record so far to $RUN/cap/<capture>/.
take() {
  mkdir -p "$RUN/cap/$1"
  for _f in "$RUN"/rec/[0-9]*.json; do [ -e "$_f" ] || continue; mv "$_f" "$RUN/cap/$1/"; done
}
# pub <src> <dest>: a scrubbed copy of <src> as the fixture <dest>.
pub() { mkdir -p "$(dirname "$2")"; scrub "$1" >"$2"; }
# pub_reqs <capture> <fixture dir> [<prefix>]: each record of the capture as requests/<prefix>-<seq>.json.
pub_reqs() {
  for _f in "$RUN/cap/$1"/[0-9]*.json; do
    [ -e "$_f" ] || continue
    _s=${_f##*/}
    pub "$_f" "$2/requests/${3:-$1}-$_s"
  done
}
py() { python3 -I "$RUN/pc5.py" "$@"; }
# pub_config <config.toml> <dest>: pub, with a table that scrub made a repeat of another dropped
# (two project paths become one /repo); the copy must parse (toml-pub).
pub_config() { pub "$1" "$RUN/pub-config.tmp"; mkdir -p "$(dirname "$2")"; py toml-pub "$RUN/pub-config.tmp" "$2"; }
# skill_md <file> <loc>: a SKILL.md for pc15-<loc>.
skill_md() {
  mkdir -p "$(dirname "$1")"
  printf '%s\n' '---' "name: pc15-$2" "description: tracer-pc15-$2-desc" '---' "tracer-pc15-$2-body" >"$1"
}

start_mock "$RUN/rules.json"

# --- PC6: agent role files, the model each subagent sends, the subagent marker ---
FX=$CODEX_FX/agents
C6=$RUN/cx-pc6
cx_home pc6 "" "[agents.pc6-probe]
description = \"tracer-pc6-probe-desc\"
config_file = \"$C6/roles/pc6-probe.toml\"

[agents.explorer]
description = \"tracer-pc6-explorer-desc\"
config_file = \"$C6/roles/explorer.toml\""
mkdir -p "$C6/roles" "$C6/agents"
printf '%s\n' 'model = "gpt-mock-pc6"' 'developer_instructions = "tracer-pc6-probe-dev"' >"$C6/roles/pc6-probe.toml"
printf '%s\n' 'model = "gpt-mock-explorer"' 'developer_instructions = "tracer-pc6-explorer-dev"' >"$C6/roles/explorer.toml"
printf '%s\n' 'name = "pc6-disc"' 'description = "tracer-pc6-disc-desc"' 'model = "gpt-mock-disc"' \
  'developer_instructions = "tracer-pc6-disc-dev"' >"$C6/agents/pc6-disc.toml"
exec_run pc6-probe exec --json tracer-pc6-probe
take pc6-probe
py tools "$RUN/cap/pc6-probe" "$RUN/pc6-tools.json"
name=$(py pick-spawn "$RUN/pc6-tools.json")
pc6_runs="pc6-probe"
if [ -z "$name" ]; then
  echo 'spawn tool: none found' >"$RUN/pc6-skip.txt"
else
  # The main agent's turn after the spawn: a sandboxed sleep, so codex exec does not end (and
  # stop the subagent) before the subagent sends its first request. wait_agent needs the agent
  # id, which is a fresh time-based UUID.
  py sse-call "$RUN/sse/pc6-wait.sse" exec_command '{"cmd":"sleep 15","yield_time_ms":20000}' call_pc6_wait
  for _xr in a:pc6-probe b:explorer c:pc6-disc; do
    _x=${_xr%%:*} _role=${_xr#*:}
    py sse-call "$RUN/sse/pc6-spawn-$_x.sse" "$name" "{\"agent_type\":\"$_role\",\"message\":\"tracer-pc6-task-$_x\"}" "call_pc6_$_x"
    exec_run "pc6-$_x" exec --json "tracer-pc6-main-$_x"
    take "pc6-$_x"
    pc6_runs="$pc6_runs pc6-$_x"
  done
fi
set --
for _r in $pc6_runs; do set -- "$@" "$_r=$RUN/cap/$_r"; done
py agents "$RUN/pc6-models.txt" "$RUN/pc6-marker.txt" "$@"
py roles "$RUN/pc6-tools.json" "$RUN/pc6-roles.txt"
pub_config "$C6/config.toml" "$FX/config.toml"
pub "$RUN/pc6-roles.txt" "$FX/roles.txt"
pub "$C6/roles/pc6-probe.toml" "$FX/role-pc6-probe.toml"
pub "$C6/roles/explorer.toml" "$FX/role-explorer.toml"
pub "$C6/agents/pc6-disc.toml" "$FX/pc6-disc.toml"
pub "$RUN/pc6-tools.json" "$FX/tools.json"
pub "$RUN/pc6-models.txt" "$FX/models.txt"
pub "$RUN/pc6-marker.txt" "$FX/marker.txt"
[ ! -e "$RUN/pc6-skip.txt" ] || pub "$RUN/pc6-skip.txt" "$FX/skip.txt"
for _r in $pc6_runs; do
  pub "$RUN/out/$_r.jsonl" "$FX/exec-$_r.jsonl"
  pub_reqs "$_r" "$FX"
done

# --- PC8: auto compaction, low against a raised usage, on a cold resume and in a live session ---
FX=$CODEX_FX/compaction
PC8_TOP='model_context_window = 200000
model_auto_compact_token_limit = 100000
compact_prompt = "tracer-pc8-compact-prompt"'
py sse-text "$RUN/sse/pc8-text.sse" tracer-pc8-reply
# Cold resume: turn 2 is a new process (exec resume --last).
for _c in low high; do
  cx_home "pc8-$_c" "$PC8_TOP" ""
  exec_run "pc8-$_c-1" exec --json "tracer-pc8-$_c-turn1"
  exec_run "pc8-$_c-2" exec resume --last "tracer-pc8-$_c-turn2"
  take "pc8-$_c"
done
# Live session: one process; turn 1's first reply is a shell call (with the raised usage in the
# high case), so Codex's next request in the same process carries the tool output.
py sse-call "$RUN/sse/pc8-live-call.sse" exec_command '{"cmd":"echo tracer-pc8-live-cmd"}' call_pc8_live
for _c in low high; do
  cx_home "pc8-live-$_c" "$PC8_TOP" ""
  exec_run "pc8-live-$_c" exec --json "tracer-pc8-live-$_c-turn1"
  take "pc8-live-$_c"
done
py compaction "$RUN/compaction.txt" "pc8-low=$RUN/cap/pc8-low" "pc8-high=$RUN/cap/pc8-high" \
  "pc8-live-low=$RUN/cap/pc8-live-low" "pc8-live-high=$RUN/cap/pc8-live-high"
pub_config "$RUN/cx-pc8-high/config.toml" "$FX/config.toml"
pub "$RUN/compaction.txt" "$FX/compaction.txt"
for _r in pc8-low pc8-high pc8-live-low pc8-live-high; do pub_reqs "$_r" "$FX"; done
for _r in pc8-live-low pc8-live-high; do pub "$RUN/out/$_r.jsonl" "$FX/exec-$_r.jsonl"; done

# --- PC9: an MCP server named zygo with a tool named compact ---
FX=$CODEX_FX/mcp
sed 's/probe_tool/compact/g' "$PREMISES_DIR/mcp_probe.py" >"$RUN/mcp/zygo_probe.py"
cx_home pc9 "" "[mcp_servers.zygo]
command = \"python3\"
args = [\"-I\", \"$RUN/mcp/zygo_probe.py\"]
default_tools_approval_mode = \"approve\""
exec_run pc9-probe exec --json tracer-pc9-probe
take pc9-probe
py tools "$RUN/cap/pc9-probe" "$RUN/pc9-tools.json"
name=$(py pick-mcp "$RUN/pc9-tools.json")
pc9_runs="pc9-probe" pc9_cap=""
if [ -n "$name" ]; then
  py sse-call "$RUN/sse/pc9-call.sse" "$name" '{}' call_pc9_1
  py sse-text "$RUN/sse/pc9-done.sse" tracer-pc9-done
  exec_run pc9-call exec --json tracer-pc9-call
  take pc9-call
  pc9_runs="$pc9_runs pc9-call" pc9_cap=$RUN/cap/pc9-call
fi
py mcp "$RUN/mcp.txt" "$RUN/pc9-tools.json" "$name" "$pc9_cap"
pub_config "$RUN/cx-pc9/config.toml" "$FX/config.toml"
pub "$RUN/pc9-tools.json" "$FX/tools.json"
pub "$RUN/mcp.txt" "$FX/mcp.txt"
for _r in $pc9_runs; do
  pub "$RUN/out/$_r.jsonl" "$FX/exec-$_r.jsonl"
  pub_reqs "$_r" "$FX"
done

# --- PC15: skills in CODEX_HOME, in the repo and on a configured path ---
FX=$CODEX_FX/skills
cx_home pc15 "" "[[skills.config]]
path = \"$RUN/skills-extra/pc15-path/SKILL.md\"
enabled = true"
skill_md "$CX_HOME/skills/pc15-home/SKILL.md" home
skill_md "$R/.agents/skills/pc15-repo/SKILL.md" repo
skill_md "$RUN/skills-extra/pc15-path/SKILL.md" path
exec_run pc15 exec --json tracer-pc15-turn
take pc15
py skills "$RUN/skills.txt" "$RUN/cap/pc15"
pub_config "$CX_HOME/config.toml" "$FX/config.toml"
pub "$CX_HOME/skills/pc15-home/SKILL.md" "$FX/SKILL-home.md"
pub "$R/.agents/skills/pc15-repo/SKILL.md" "$FX/SKILL-repo.md"
pub "$RUN/skills-extra/pc15-path/SKILL.md" "$FX/SKILL-path.md"
rm -rf "$R/.agents"
pub "$RUN/skills.txt" "$FX/skills.txt"
for _f in "$RUN/cap/pc15"/[0-9]*.json; do
  [ -e "$_f" ] || continue
  case "$(python3 -I -c 'import json, sys; r = json.load(open(sys.argv[1])); print(r.get("method"), r.get("path"))' "$_f")" in
    "POST "*/v1/responses) pub "$_f" "$FX/request.json"; break ;;
  esac
done

# --- PC11: the TUI status line, in a pane of a private tmux server ---
FX=$CODEX_FX/status
TMUX_BIN=$(command -v tmux) || { echo "session: tmux is not on PATH"; exit 1; }
TV=$("$TMUX_BIN" -V | sed -n 's/^tmux //p')
ver_ge "${TV%[a-z]}" 3.2 || { echo "session: tmux ${TV:-unknown} is below 3.2"; exit 1; }
SOCK=$RUN/s
[ ${#SOCK} -le 100 ] || { echo "session: the tmux socket path is longer than 100 characters; set N2_SCRATCH or TMPDIR to a short directory"; exit 1; }
tm() { LC_ALL=C "$TMUX_BIN" -S "$SOCK" -f /dev/null "$@"; }
tm_kill() { tm kill-server 2>/dev/null || true; }
n2_trap_chain tm_kill
py sse-text "$RUN/sse/pc11-text.sse" tracer-pc11-reply
# q <NAME> <value>: NAME='value'; a value that holds a quote ends the part.
q() { case "$2" in *"'"*) echo "session: $1 holds a quote"; exit 1 ;; esac; printf "%s='%s'\n" "$1" "$2"; }
# pane_wait <text> <polls>: up to <polls> captures of session $TS's pane, 0.5 s apart, into
# $RUN/pane.txt; 0 when one holds <text>, 1 when the TUI ended, a capture failed or the polls ran out.
pane_wait() {
  _i=0
  while [ "$_i" -lt "$2" ]; do
    _i=$((_i + 1))
    tm capture-pane -p -t "=$TS:" >"$RUN/pane.tmp" 2>/dev/null || return 1
    mv "$RUN/pane.tmp" "$RUN/pane.txt"
    sleep 0.5
    ! grep -qF -- "$1" "$RUN/pane.txt" || return 0
    [ ! -e "$RUN/$TS-exit" ] || return 1
  done
  return 1
}
# keep <file>: the last capture as <file> (none yet: nothing).
keep() { [ ! -e "$RUN/pane.txt" ] || cp "$RUN/pane.txt" "$1"; }
# key <key>: one key to the pane, then 0.5 s. send <text>: the text as typed, then Enter.
key() { tm send-keys -t "=$TS:" "$1" 2>/dev/null || true; sleep 0.5; }
send() { tm send-keys -t "=$TS:" -l "$1" 2>/dev/null || true; sleep 0.5; key Enter; }
# tui_run <tag> <full: 1|0>: the TUI with this CX_HOME in session <tag> of the private server; one
# turn (tracer-pc11-turn), captured as $RUN/<tag>-start.txt and <tag>-turn.txt; full: then
# /statusline (<tag>-statusline.txt) and the warnings view, F2 (<tag>-warnings.txt). Then quit,
# kill the server, take the records as capture <tag>. A refusal ends the part (exit 1).
tui_run() {
  TS=$1
  {
    q PREMISES_DIR "$PREMISES_DIR"
    echo 'N2_AGENTS=codex'
    q N2_SCRATCH "$N2_SCRATCH"
    # shellcheck disable=SC2016
    echo '. "$PREMISES_DIR/lib.sh"'
    q RUN "$RUN"; q H "$H"; q R "$R"; q CX_HOME "$CX_HOME"; q MOCK_URL "$MOCK_URL"; q MOCK_PID "$MOCK_PID"
    # A refusal (codex_env or run_codex exits before Codex ends) leaves <tag>-refused and its
    # "premises:" lines in <tag>-pre.txt or the pane; the part then prints them and exits 1.
    echo "trap '[ -e \"\$RUN/$TS-exit\" ] || : >\"\$RUN/$TS-refused\"' EXIT"
    # --no-daemon: else the TUI starts a managed app-server daemon (from CODEX_HOME/packages, with
    # a socket under /tmp/codex-daemon-<uid>) that outlives the run.
    echo 'set -- --no-daemon'
    # The checks run_codex makes, first with their output in a file (the pane closes on a refusal).
    echo "{ n2_daemon_flag \"\$@\"; codex_env; n2_procs daemon || exit 1; } >\"\$RUN/$TS-pre.txt\" 2>&1"
    echo "rc=0; run_codex \"\$@\" || rc=\$?; echo \"\$rc\" >\"\$RUN/$TS-exit\""
  } >"$RUN/$TS-tui.sh"
  rm -f "$RUN/$TS-exit" "$RUN/$TS-refused" "$RUN/pane.txt"
  tm new-session -d -s "$TS" -x 250 -y 50 /bin/sh "$RUN/$TS-tui.sh"
  pane_wait gpt-mock 60 || true
  keep "$RUN/$TS-start.txt"
  send tracer-pc11-turn
  pane_wait tracer-pc11-reply 120 || true
  keep "$RUN/$TS-turn.txt"
  if [ "$2" = 1 ]; then
    send /statusline
    pane_wait tracer-none 4 || true
    keep "$RUN/$TS-statusline.txt"
    key Escape
    key F2
    pane_wait tracer-none 4 || true
    keep "$RUN/$TS-warnings.txt"
  fi
  key Escape; key C-c; key C-c
  _i=0
  while [ ! -e "$RUN/$TS-exit" ] && [ "$_i" -lt 20 ]; do _i=$((_i + 1)); sleep 0.5; done
  tm_kill
  if [ -e "$RUN/$TS-refused" ]; then
    echo "session: $TS: the TUI run was refused before Codex ended:"
    cat "$RUN/$TS-pre.txt" 2>/dev/null || true
    grep -h '^premises:' "$RUN/pane.txt" "$RUN/$TS"-*.txt 2>/dev/null | sort -u || true
    exit 1
  fi
  take "$TS"
}
# pub_pane <capture file> <dest>: pub, or "(no capture)" when the file was never written; a
# scrubbed capture that still holds a machine path ends the part.
pub_pane() {
  if [ -e "$1" ]; then pub "$1" "$2"; else echo '(no capture)' >"$2"; fi
  ! grep -qE '/Users/|/var/folders/|/private/var/' "$2" || { echo "session: pc11: a pane capture holds a machine path"; exit 1; }
}
# The configured run, then the control run (no status_line key: Codex's default items).
cx_home pc11 'check_for_update_on_startup = false' '[tui]
status_line = ["model-with-reasoning"]
show_tooltips = false'
tui_run pc11 1
cx_home pc11-default 'check_for_update_on_startup = false' '[tui]
show_tooltips = false'
tui_run pc11-default 0
pub_config "$RUN/cx-pc11/config.toml" "$FX/config.toml"
pub_config "$RUN/cx-pc11-default/config.toml" "$FX/config-default.toml"
for _p in start turn statusline; do pub_pane "$RUN/pc11-$_p.txt" "$FX/pane-$_p.txt"; done
pub_pane "$RUN/pc11-warnings.txt" "$FX/warnings.txt"
pub_pane "$RUN/pc11-default-turn.txt" "$FX/pane-default-turn.txt"
pub_reqs pc11 "$FX"
pub_reqs pc11-default "$FX"
py status "$FX/status.txt" "$FX/pane-turn.txt" "$RUN/pc11-exit" "$FX/pane-default-turn.txt" "$RUN/pc11-default-exit"

stop_mock
codex_record session
echo "codex session: ok (codex $CODEX_VERSION)"

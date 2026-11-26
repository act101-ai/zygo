#!/bin/sh
# Codex config, trust and hook part (node PC-4, K§6.3.7 rows PC3, PC4, PC5, PC12 and the
# --strict-config run). Run it as: sh scripts/premises/run-codex.sh config
#   PC3  config sources: CODEX_HOME against ~/.codex (a planted file in the scratch HOME), -c against
#        a project .codex/config.toml per key, project agent files and hooks, -C/--cd.
#   PC4  hook trust: where Codex keeps it, the bypass flag, --trust, records written before launch.
#   PC5  one hook payload per event, the tool names (shell, apply_patch, MCP, spawn) and the hook
#        output shapes Codex accepts (context, deny, block, result replacement).
#   PC12 shell_environment_policy in tool shells (variable names only, never values).
#   The --strict-config run of a planted config with the keys K§6.3.1 item 2 says zygo writes.
# Writes fixtures/codex/config/, trust/, hooks/ (all through scrub, moved in only at the end) and
# runs/config/ (codex_record). One run folder, one mock in two phases: phase A (runs with no tool
# call: PC3 without tools, PC4, the strict runs, pc5-probe), then a restart with rules built from the
# tools pc5-probe's first request offers (phase B: every run with a tool call and the PC5 output
# shapes). Each scenario has its prompt marker zp-<scenario> and its CODEX_HOME $RUN/cxh-<scenario>.
# A Codex run that fails is data; only a harness refusal, ABORT or a missing tool exits non-zero.
PREMISES_DIR=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck disable=SC2034 # read by lib.sh
N2_AGENTS=codex
# shellcheck disable=SC1091 # lib.sh is checked on its own
. "$PREMISES_DIR/lib.sh"
for _t in python3 shasum; do command -v "$_t" >/dev/null || { echo "premises: $_t is not on PATH"; exit 1; }; done
# Paths go into TOML strings and sh -c texts unquoted: no blank, quote or backslash.
n2_plain PREMISES_DIR "$PREMISES_DIR"
new_run codex-config
mkdir -p "$RUN/fx/config" "$RUN/fx/trust" "$RUN/fx/hooks" "$RUN/sum/config" "$RUN/sum/trust" \
  "$RUN/sum/hooks" "$RUN/out" "$RUN/proj" "$RUN/sse" "$RUN/hk-rules" "$RUN/x" "$RUN/skills/zp-skill" "$RUN/other"
printf '%s\n' '---' 'name: zp-skill' 'description: zp-skill-desc' '---' 'A planted skill (node PC-4).' \
  >"$RUN/skills/zp-skill/SKILL.md"
git -C "$RUN/other" init -q
git -C "$RUN/other" -c user.name=t -c user.email=t@t commit -q --allow-empty -m init

# cx.py: the part's Python (TOML writing, SSE authoring, tool selection, judging and summaries).
cat >"$RUN/cx.py" <<'PY'
import difflib, hashlib, itertools, json, os, re, sqlite3, sys, urllib.parse

RUN = sys.argv[1]
CMD = sys.argv[2]
A = sys.argv[3:]
q = json.dumps
# MATCHERS: (label, matcher) groups of pc5-match on PreToolUse and PostToolUse (F4): the hook's
# tool name, the model's tool name, an alternation, a regex, a name that matches nothing, "*".
MATCHERS = [("exact", "Bash"), ("model", "exec_command"), ("alt", "Bash|Write"), ("regex", "^Ba.*"),
            ("none", "NoSuchTool"), ("star", "*")]
EVENTS = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PermissionRequest", "PostToolUse",
          "PreModelRequest", "PostModelRequest", "PreCompact", "PostCompact", "SubagentStart",
          "SubagentStop", "Stop", "Interrupt", "SessionEnd"]


def rd(p, default=""):
    try:
        with open(p) as f:
            return f.read()
    except OSError:
        return default


def wjson(p, obj):
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w") as f:
        json.dump(obj, f, indent=2, sort_keys=False)
        f.write("\n")


def recs(s):
    """The records of scenario s, in order (from out/<s>.seq: <folder> <first> <last>)."""
    t = rd(os.path.join(RUN, "out", s + ".seq")).split()
    if len(t) != 3:
        return []
    out = []
    for i in range(int(t[1]), int(t[2]) + 1):
        p = os.path.join(RUN, t[0], "%04d.json" % i)
        if not os.path.exists(p) and t[0] == "rec-a":
            # Phase A, before the restart: the records are still in rec/.
            p = os.path.join(RUN, "rec", "%04d.json" % i)
        if os.path.exists(p):
            out.append(json.load(open(p)))
    return out


def is_resp(r):
    return r.get("method") == "POST" and re.search(r"/v1/responses(\?|$)", r.get("path", ""))


def resp(s):
    return [r for r in recs(s) if is_resp(r)]


def btext(r):
    return json.dumps(r.get("body"))


def first_text(s):
    rs = resp(s)
    return btext(rs[0]) if rs else ""


def tool_outputs(r):
    """The text of the tool output items in a request body."""
    out = []
    body = r.get("body") if isinstance(r.get("body"), dict) else {}
    for it in body.get("input") or []:
        if not isinstance(it, dict) or it.get("type") not in ("function_call_output", "custom_tool_call_output"):
            continue
        o = it.get("output")
        if isinstance(o, list):
            o = "\n".join(x.get("text", "") for x in o if isinstance(x, dict))
        elif not isinstance(o, str):
            o = json.dumps(o)
        try:
            d = json.loads(o)
            if isinstance(d, dict) and isinstance(d.get("output"), str):
                o = d["output"]
        except ValueError:
            pass
        out.append(o)
    return "\n".join(out)


def rc(s):
    return rd(os.path.join(RUN, "out", s + ".rc")).strip()


def ran(s):
    return os.path.exists(os.path.join(RUN, "out", s + ".rc"))


def lines(p):
    return [l.strip() for l in rd(p).splitlines() if l.strip()]


def skipped():
    out = {}
    for l in lines(os.path.join(RUN, "out", "skipped.txt")):
        s, _, why = l.partition(" ")
        out[s] = why
    return out


def why_not(s):
    return skipped().get(s) or ("not run" if not ran(s) else "no request recorded (exit %s)" % rc(s))


# --- writers ---------------------------------------------------------------------------------
def cmd_home():
    out, mode, extra, url, key, untrusted, feat = A[:7]
    paths = A[7:]
    text = rd(extra) if extra else ""
    top, tables, in_t = [], [], False
    for line in text.splitlines():
        if re.match(r"\s*\[", line):
            in_t = True
        (tables if in_t else top).append(line)
    names = {m.group(1) for m in (re.match(r"\s*([A-Za-z0-9_-]+)\s*=", l) for l in top) if m}
    L = []
    if "model" not in names:
        L.append('model = "gpt-mock"')
    if "model_provider" not in names:
        L.append('model_provider = "mock"')
    L += top
    L += ["", "[model_providers.mock]", 'name = "mock"', "base_url = %s" % q(url + "/v1"), 'wire_api = "responses"']
    if mode == "env":
        L.append('env_key = "MOCK_API_KEY"')
    else:
        L.append('auth = { command = "/bin/sh", args = ["-c", %s] }' % q("echo home >>%s/auth-who; printf %%s %s" % (RUN, key)))
    seen = []
    for p in paths + [os.path.realpath(p) for p in paths]:
        if p not in seen:
            seen.append(p)
    for p in seen if untrusted != "none" else []:
        L += ["", "[projects.%s]" % q(p), 'trust_level = "%s"' % ("untrusted" if untrusted == "1" else "trusted")]
    if feat:
        L += ["", "[features]", "%s = true" % feat]
    L += [""] + tables
    with open(out, "w") as f:
        f.write("\n".join(L) + "\n")


def cmd_sse():
    path, kind = A[0], A[1]
    if kind == "text":
        item = {"type": "message", "id": "msg_zp", "role": "assistant", "status": "completed",
                "content": [{"type": "output_text", "text": A[2], "annotations": []}]}
        added = dict(item, status="in_progress", content=[])
    elif kind == "call":
        item = {"type": "function_call", "id": "fc_zp", "status": "completed", "call_id": A[3],
                "name": A[2], "arguments": A[4]}
        if len(A) > 5 and A[5]:
            item["namespace"] = A[5]
        added = dict(item, status="in_progress")
    elif kind == "custom":
        item = {"type": "custom_tool_call", "id": "ctc_zp", "status": "completed", "call_id": A[3],
                "name": A[2], "input": A[4]}
        added = dict(item, status="in_progress")
    else:
        sys.exit("cx_sse: kind %s" % kind)
    resp_ = {"id": "resp_zp", "object": "response", "model": "{{MODEL}}"}
    ev = [("response.created", {"type": "response.created", "response": dict(resp_, status="in_progress", output=[])}),
          ("response.output_item.added", {"type": "response.output_item.added", "output_index": 0, "item": added})]
    if kind == "text":
        ev.append(("response.output_text.delta", {"type": "response.output_text.delta", "item_id": item["id"],
                                                  "output_index": 0, "content_index": 0, "delta": A[2]}))
    ev += [("response.output_item.done", {"type": "response.output_item.done", "output_index": 0, "item": item}),
           ("response.completed", {"type": "response.completed", "response": dict(
               resp_, status="completed", output=[item],
               usage={"input_tokens": 10, "output_tokens": 1, "total_tokens": 11})})]
    with open(path, "w") as f:
        f.write("".join("event: %s\ndata: %s\n\n" % (e, json.dumps(d)) for e, d in ev))


def cmd_rule():
    rules_p, s, sse = A[:3]
    rules = json.load(open(rules_p))
    body = A[4] if len(A) > 4 and A[4] else "zp-%s(?![-A-Za-z0-9])" % re.escape(s)
    r = {"name": s, "path": r"/v1/responses", "body": body, "sse": sse, "times": 1}
    if len(A) > 3 and A[3]:
        r["usage"] = json.loads(A[3])
    rules.append(r)
    wjson(rules_p, rules)


def cmd_shellargs():
    arg, arr, text = A[:3]
    d = {arg: ["/bin/sh", "-c", text] if arr == "1" else text}
    if len(A) > 3 and A[3]:
        d["yield_time_ms"] = int(A[3])
    print(json.dumps(d))


def cmd_pc5toml():
    out, s, prem = A[:3]
    ev, rules = (A[3], A[4]) if len(A) > 4 else ("", "")
    match = len(A) > 5 and A[5] == "match"
    approve = not (len(A) > 6 and A[6] == "noapprove")
    L = []
    for e in EVENTS:
        c = "python3 -I %s/hook_log.py %s/hk/%s/%s" % (prem, RUN, s, e)
        if e == ev:
            c += " " + rules
        L += ["[[hooks.%s]]" % e, "[[hooks.%s.hooks]]" % e, 'type = "command"', "command = %s" % q(c), ""]
        if match and e in ("PreToolUse", "PostToolUse"):
            for label, m in MATCHERS:
                c = "python3 -I %s/hook_log.py %s/hk/%s/%s-m-%s" % (prem, RUN, s, e, label)
                L += ["[[hooks.%s]]" % e, "matcher = %s" % q(m), "[[hooks.%s.hooks]]" % e, 'type = "command"',
                      "command = %s" % q(c), ""]
    # default_tools_approval_mode: under approval_policy "never" Codex refuses an MCP call that needs
    # approval ("MCP tool call requires approval, but approval policy is never"; real run 2026-11-26).
    L += ["[mcp_servers.probe]", 'command = "python3"', "args = %s" % q(["-I", prem + "/mcp_probe.py"])]
    L += ['default_tools_approval_mode = "approve"', ""] if approve else [""]
    with open(out, "w") as f:
        f.write("\n".join(L))


def tool_names(body):
    """Every tool (not a namespace) in body.tools, each with "_ns": its namespace name or ""."""
    found = []

    def walk(o, ns):
        if isinstance(o, dict):
            if o.get("type") == "namespace":
                for v in o.get("tools") or []:
                    walk(v, o.get("name", ""))
                return
            if isinstance(o.get("name"), str):
                found.append(dict(o, _ns=ns))
        elif isinstance(o, list):
            for v in o:
                walk(v, ns)
    walk(body.get("tools") or [], "")
    return found


def cmd_tools():
    out = A[0]
    first = None
    for p in A[1:]:
        r = json.load(open(p))
        if is_resp(r) and isinstance(r.get("body"), dict):
            first = r
            break
    v = dict(SHELL_TOOL="", SHELL_ARG="", SHELL_ARRAY="", SHELL_YIELD="", PATCH_TOOL="", PATCH_KIND="", SPAWN_TOOL="",
             SPAWN_ARG="", SPAWN_NS="", MCP_TOOL="", MCP_NS="")
    tools = tool_names(first["body"]) if first else []
    by = {}
    for t in tools:
        by.setdefault(t["name"], t)
    props = lambda t: (t.get("parameters") or {}).get("properties") or {}
    for n in ("exec_command", "shell_command", "shell"):
        if n in by:
            v["SHELL_TOOL"] = n
            p = props(by[n])
            v["SHELL_ARG"] = "cmd" if "cmd" in p else "command"
            v["SHELL_ARRAY"] = "1" if (p.get(v["SHELL_ARG"]) or {}).get("type") == "array" else ""
            v["SHELL_YIELD"] = "1" if "yield_time_ms" in p else ""
            break
    if "apply_patch" in by:
        v["PATCH_TOOL"] = "apply_patch"
        v["PATCH_KIND"] = "custom" if by["apply_patch"].get("type") == "custom" else "function"
    for n, t in by.items():
        if "spawn" in n:
            v["SPAWN_TOOL"] = n
            v["SPAWN_ARG"] = next((a for a in ("message", "prompt", "task", "input") if a in props(t)), "")
            v["SPAWN_NS"] = t["_ns"]
            break
    v["MCP_TOOL"] = next((n for n in by if "probe_tool" in n), "")
    v["MCP_NS"] = by[v["MCP_TOOL"]]["_ns"] if v["MCP_TOOL"] else ""
    v["ALL_TOOLS"] = ",".join(sorted(("%s/%s" % (t["_ns"], n)) if t["_ns"] else n for n, t in by.items()))
    with open(out, "w") as f:
        for k, x in v.items():
            if not re.fullmatch(r"[A-Za-z0-9_.,:/-]*", x):
                x = ""
            f.write("%s='%s'\n" % (k, x))


def state_tables(text):
    """The [hooks.state...] tables of a config.toml, verbatim, and their keys."""
    out, keys, cur = [], [], None
    for line in text.splitlines():
        m = re.match(r"\s*\[\s*hooks\s*\.\s*state(\s*\.\s*(.+?))?\s*\]\s*$", line)
        if m:
            cur = True
            out.append(line)
            if m.group(2):
                k = m.group(2).strip()
                keys.append(json.loads(k) if k.startswith('"') else k.strip("'"))
            continue
        if re.match(r"\s*\[", line):
            cur = None
            continue
        if cur:
            out.append(line)
            m = re.match(r'\s*("(?:[^"\\]|\\.)*"|[A-Za-z0-9_-]+)\s*=\s*\{', line)
            if m and not out[-2].strip().startswith("[hooks.state."):
                k = m.group(1)
                keys.append(json.loads(k) if k.startswith('"') else k)
    return [l for l in out], keys


def cmd_form():
    for s in A:
        t, keys = state_tables(rd(os.path.join(RUN, "out", s + ".config.toml")))
        if keys:
            with open(os.path.join(RUN, "sum", "hooks-state.toml"), "w") as f:
                f.write("\n" + "\n".join(t) + "\n")
            print(s)
            return


# --- strict config -------------------------------------------------------------------------------
DENY = ["ANTHROPIC_*", "OPENAI_*", "OPENROUTER_*", "CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY", "ZYGO_TEAM_TOKEN", "CLAUDECODE",
        "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_BRIDGE_SESSION_ID"]


def strict_keys(url, prem, key, rcv):
    """(id, dotted key, table or "", TOML line, value as shown) in file order."""
    return [
        ("model_provider", "model_provider", "", 'model_provider = "zygo"', '"zygo"'),
        ("model", "model", "", 'model = "code"', '"code"'),
        ("model_context_window", "model_context_window", "", "model_context_window = 200000", "200000"),
        ("model_auto_compact_token_limit", "model_auto_compact_token_limit", "", "model_auto_compact_token_limit = 180000", "180000"),
        ("developer_instructions", "developer_instructions", "", 'developer_instructions = "zp-dev"', '"zp-dev"'),
        ("compact_prompt", "compact_prompt", "", 'compact_prompt = "zp-compact"', '"zp-compact"'),
        ("status_line", "tui.status_line", "tui", 'status_line = ["model-with-reasoning", "current-dir"]', '["model-with-reasoning", "current-dir"]'),
        ("skills", "skills.config", "skills", "config = [{ path = %s, enabled = true }]" % q(RUN + "/skills/zp-skill/SKILL.md"),
         "[{ path = %s, enabled = true }]" % q(RUN + "/skills/zp-skill/SKILL.md")),
        ("inherit", "shell_environment_policy.inherit", "shell_environment_policy", 'inherit = "all"', '"all"'),
        ("exclude", "shell_environment_policy.exclude", "shell_environment_policy", "exclude = %s" % q(DENY), q(DENY)),
        ("mcp_command", "mcp_servers.zygo.command", "mcp_servers.zygo", 'command = "python3"', '"python3"'),
        ("mcp_args", "mcp_servers.zygo.args", "mcp_servers.zygo", "args = %s" % q(["-I", prem + "/mcp_probe.py"]), q(["-I", prem + "/mcp_probe.py"])),
        ("mcp_approval", "mcp_servers.zygo.default_tools_approval_mode", "mcp_servers.zygo",
         'default_tools_approval_mode = "approve"', '"approve"'),
        ("name", "model_providers.zygo.name", "model_providers.zygo", 'name = "zygo"', '"zygo"'),
        ("wire_api", "model_providers.zygo.wire_api", "model_providers.zygo", 'wire_api = "responses"', '"responses"'),
        ("base_url", "model_providers.zygo.base_url", "model_providers.zygo", "base_url = %s" % q(url + "/v1"), q(url + "/v1")),
        ("auth", "model_providers.zygo.auth", "model_providers.zygo", 'auth = { command = "printf", args = ["%%s", %s] }' % q(key),
         '{ command = "printf", args = [omitted] }'),
        ("http_headers", "model_providers.zygo.http_headers", "model_providers.zygo",
         'http_headers = { X-Zygo-Session = "zp-session", X-Zygo-Agent-Version = "0.0.0", X-Zygo-Repo = "zp-repo", X-Zygo-Work = "zp-work" }',
         '{ X-Zygo-Session = "zp-session", X-Zygo-Agent-Version = "0.0.0", X-Zygo-Repo = "zp-repo", X-Zygo-Work = "zp-work" }'),
        ("supports_websockets", "model_providers.zygo.supports_websockets", "model_providers.zygo", "supports_websockets = false", "false"),
        ("remote_compaction", "model_providers.zygo.capabilities.remote_compaction", "model_providers.zygo",
         "capabilities = { remote_compaction = %s }" % rcv, rcv),
    ]


ALIASES = {"remote_compaction": ["capabilities", "remote_compaction", "RemoteCompactionSupport"],
           "skills": ["skills", "config"], "status_line": ["status_line", "tui"], "mcp_command": ["command"],
           "mcp_args": ["args"], "hooks_state": ["state", "trusted_hash", "trusted_content_hash"]}


def cmd_strict_gen():
    out, removed, rcv, url, prem, key, with_state = A[:7]
    removed = removed.split()
    keys = [k for k in strict_keys(url, prem, key, rcv) if k[0] not in removed]
    L, tables = [], {}
    for k in keys:
        if k[2]:
            tables.setdefault(k[2], []).append(k[3])
        else:
            L.append(k[3])
    for t, ls in tables.items():
        L += ["", "[%s]" % t] + ls
    if with_state == "1" and "hooks_state" not in removed:
        L.append(rd(os.path.join(RUN, "sum", "hooks-state.toml")))
    with open(out, "w") as f:
        f.write("\n".join(L) + "\n")


LOC = re.compile(r"config\.toml:(\d+):(\d+): (.*)$")


def first_err(s):
    """The first stderr line that locates a config error (from "config.toml:"), else the first line."""
    ls = lines(os.path.join(RUN, "out", s + ".err"))
    for l in ls:
        m = LOC.search(l)
        if m:
            return l[m.start():]
    return ls[0] if ls else "no stderr (exit %s)" % rc(s)


def key_name(k):
    """(table, key name) of a strict key, from its TOML line (values play no part)."""
    return (k[2], k[3].split("=", 1)[0].strip())


def key_at(s, keys):
    """The keys on the config.toml line a stderr location names, read from the file the run used
    (out/<s>.toml): a key line gives its key, a table header the keys of that table, a line in a
    [hooks.state...] table hooks_state. None when no location or the line holds no listed key."""
    for l in lines(os.path.join(RUN, "out", s + ".err")):
        m = LOC.search(l)
        if not m:
            continue
        fl = rd(os.path.join(RUN, "out", s + ".toml")).splitlines()
        n = int(m.group(1))
        if not 0 < n <= len(fl):
            return None
        table = ""
        for i, line in enumerate(fl[:n]):
            h = re.match(r"\s*\[\s*([^\]]+?)\s*\]\s*$", line)
            if h:
                table = h.group(1)
                if i == n - 1:
                    if table.startswith("hooks.state"):
                        return [k for k in keys if k[0] == "hooks_state"] or None
                    return [k for k in keys if k[2] == table] or None
        if table.startswith("hooks.state"):
            return [k for k in keys if k[0] == "hooks_state"] or None
        name = fl[n - 1].split("=", 1)[0].strip()
        return [k for k in keys if k[0] != "hooks_state" and key_name(k) == (table, name)] or None
    return None


def key_hit(line, keys):
    """The key an error line names: a whole dotted key (longest first), else a quoted alias. Only a
    line that says error, unknown or invalid counts (not the banner's "model: <name>")."""
    if not re.search(r"error|unknown|invalid", line, re.I):
        return None
    for k in sorted(keys, key=lambda k: -len(k[1])):
        if re.search(r"(?<![A-Za-z0-9_.])%s(?![A-Za-z0-9_])" % re.escape(k[1]), line):
            return k
    for k in keys:
        names = ALIASES.get(k[0], [k[1].split(".")[-1]])
        if any(re.search(r"[`'\"]%s[`'\"]" % re.escape(n), line) for n in names):
            return k
    return None


def cmd_strict_judge():
    s, removed, rcv, with_state = A[:4]
    keys = strict_keys("", "", "", rcv)
    if with_state == "1":
        keys.append(("hooks_state", "hooks.state", "", "", "[omitted]"))
    keys = [k for k in keys if k[0] not in removed.split()]
    v = {"run": s, "removed": removed.split(), "remote_compaction": rcv}
    if rc(s) == "0" and resp(s):
        v["verdict"] = "accepted"
    elif rc(s) == "0":
        v["verdict"] = "accepted-no-request"
    else:
        errs = lines(os.path.join(RUN, "out", s + ".err"))
        ks = key_at(s, keys)
        if not ks:
            ks = next(([key_hit(l, keys)] for l in errs if key_hit(l, keys)), None)
        if ks:
            v.update(verdict="key", key=",".join(k[0] for k in ks), line=first_err(s))
        else:
            v.update(verdict="other", line=first_err(s) if errs else "no request recorded (exit %s)" % rc(s))
    with open(os.path.join(RUN, "sum", "strict.jsonl"), "a") as f:
        f.write(json.dumps(v) + "\n")
    print(v["verdict"] + (" " + v["key"] if v["verdict"] == "key" else ""))


def control_line(s):
    if rc(s) == "0":
        return "accepted" if resp(s) else "accepted: no request"
    return "refused: " + first_err(s)


def strict_report(version, with_state, url, prem):
    runs = [json.loads(l) for l in lines(os.path.join(RUN, "sum", "strict.jsonl"))]
    out = ["codex %s --strict-config" % version,
           "control zp_unknown_key: " + control_line("strict-control"),
           "control zp_unknown_key without --strict-config: " + control_line("strict-control-off")]
    last = runs[-1] if runs else None
    keys = strict_keys(url, prem, "", "false")
    if with_state:
        keys.append(("hooks_state", "hooks.state", "", "", "[omitted]"))
    unsupported = None
    for k in keys:
        refused = [r for r in runs if r["verdict"] == "key" and k[0] in r["key"].split(",")]
        if k[0] == "remote_compaction":
            first = [r for r in refused if r["remote_compaction"] == "false"]
            second = [r for r in runs if r["remote_compaction"] != "false" and "remote_compaction" not in r["removed"]]
            if second:
                sr = [r for r in second if r["verdict"] == "key" and "remote_compaction" in r["key"].split(",")]
                unsupported = "refused: " + sr[0]["line"] if sr else (
                    "accepted" if second[-1]["verdict"] == "accepted" else
                    "accepted: no request" if second[-1]["verdict"] == "accepted-no-request" else
                    "undecided (config: refused: %s)" % second[-1]["line"])
            refused = first
        val = k[4] if k[0] != "remote_compaction" else "false"
        if refused:
            verdict = "refused: " + refused[0]["line"]
        elif last and last["verdict"] == "accepted":
            verdict = "accepted"
        elif last and last["verdict"] == "accepted-no-request":
            verdict = "accepted: no request"
        elif last:
            verdict = "undecided (config: refused: %s)" % last.get("line", "")
        else:
            verdict = "undecided (no strict run)"
        out.append("%s = %s: %s" % (k[1], val, verdict))
    if unsupported is not None:
        out.append('capabilities.remote_compaction = "unsupported": ' + unsupported)
    if last and last["verdict"] == "other":
        out.append("config: refused: " + last["line"])
    with open(os.path.join(RUN, "sum", "config", "strict-config.txt"), "w") as f:
        f.write("\n".join(out) + "\n")


# --- summaries -------------------------------------------------------------------------------
def who(s, f):
    return lines(os.path.join(RUN, "out", "%s.%s" % (s, f)))


def side(ls):
    return "cli" if "cli" in ls else "project" if "project" in ls else "none"


def compaction(s):
    rs = resp(s)
    for i, r in enumerate(rs):
        t = btext(r)
        if "compaction_trigger" in t:
            return "trigger", t
        if i > 0 and re.search(r"zp-compact-(cli|project|window)", t):
            return "local", t
    return None, ""


# WS: the Responses WebSocket log target as a whole token (not feature.responses_websockets).
WS = re.compile(r"\bresponses_websocket\b")


def shown_fns():
    def path_side(tag):
        def f(s):
            rs = resp(s)
            p = rs[0]["path"] if rs else ""
            return "cli" if "/zp-%s-cli/" % tag in p else "project" if "/zp-%s-project/" % tag in p else "none"
        return f

    def marker(cli, proj, absent=None):
        def f(s):
            t = first_text(s)
            if not t:
                return "none"
            return "cli" if cli in t else "project" if proj and proj in t else absent or "none"
        return f

    def model(s):
        rs = resp(s)
        m = rs[0]["body"].get("model") if rs and isinstance(rs[0].get("body"), dict) else None
        return {"zp-model-cli": "cli", "zp-model-project": "project"}.get(m, "none")

    def header(s):
        rs = resp(s)
        return {"cli": "cli", "project": "project"}.get(rs[0]["headers"].get("x-zp-who") if rs else None, "none")

    def ws(s):
        # Codex tunnels the WebSocket through HTTPS_PROXY (a refused CONNECT to the mock's port) and
        # logs "responses_websocket" on stderr; a GET with upgrade: websocket would be recorded.
        if not recs(s):
            return "none"
        err = rd(os.path.join(RUN, "out", s + ".err"))
        get = any(r.get("method") == "GET" and "websocket" in (r.get("headers") or {}).get("upgrade", "").lower()
                  for r in recs(s))
        return "cli" if get or WS.search(err) else "none"

    def env(s):
        rs = resp(s)
        t = tool_outputs(rs[1]) if len(rs) > 1 else ""
        return "cli" if "ZP_WHO=cli" in t else "project" if "ZP_WHO=project" in t else "none"

    def threshold(s):
        # Only the cli side holds the small value: a compaction shows it; none is no evidence.
        return "cli" if resp(s) and compaction(s)[0] else "none"

    def cprompt(s):
        kind, t = compaction(s)
        return "cli" if "zp-compact-cli" in t else "project" if "zp-compact-project" in t else "none"

    def remote(s):
        kind, _ = compaction(s)
        return {"trigger": "cli", "local": "project"}.get(kind, "none")

    return {
        ("pc3-url", "model_providers.mock.base_url"): path_side("url"),
        ("pc3-prov", "model_provider"): path_side("prov"),
        ("pc3-auth", "model_providers.mock.auth"): lambda s: side(who(s, "auth-who")),
        ("pc3-main", "model"): model,
        ("pc3-main", "model_providers.mock.http_headers"): header,
        ("pc3-main", "developer_instructions"): marker("zp-dev-cli", "zp-dev-project"),
        ("pc3-main", "skills.config"): marker("zp-skill-desc", None),
        ("pc3-main", "mcp_servers.zygo"): lambda s: side(who(s, "mcp-who")),
        ("pc3-main", "model_providers.mock.supports_websockets"): ws,
        ("pc3-main", "model_providers.mock.wire_api"): None,
        ("pc3-main", "tui.status_line"): None,
        ("pc3-env", "shell_environment_policy.set"): env,
        ("pc3-compact", "model_auto_compact_token_limit"): threshold,
        ("pc3-compact", "compact_prompt"): cprompt,
        ("pc3-window", "model_context_window"): threshold,
        ("pc3-window", "model_providers.mock.capabilities.remote_compaction"): remote,
    }


NOT_SHOWN = {"model_providers.mock.wire_api": "one valid value, WireApi",
             "tui.status_line": "exec renders no status line; PC11 is PC-5's"}


def pc3():
    ok = lambda b, why: ("verified", "") if b else ("failed", why)
    rs = resp("pc3-home")
    m = rs[0]["body"].get("model") if rs and isinstance(rs[0].get("body"), dict) else None
    r, w = ok(m == "zp-model-codexhome", "model seen: %s" % (m or why_not("pc3-home")))
    home = {"model_seen": m, "dotcodex_model": "zp-model-dotcodex", "codex_home_model": "zp-model-codexhome",
            "result": r, "why": w or "the CODEX_HOME config won over the scratch ~/.codex config"}
    wjson(os.path.join(RUN, "sum", "config", "codex-home.json"), home)
    t = first_text("pc3-cd")
    cd = {"other_marker": "zp-cd-other" in t, "cwd_marker": "zp-cd-cwd" in t}
    r, w = ok(cd["other_marker"] and not cd["cwd_marker"],
              "-C did not select the other project's config" if t else why_not("pc3-cd"))
    cd.update(result=r, why=w or "-C/--cd selects the project config of its folder")
    wjson(os.path.join(RUN, "sum", "config", "cd.json"), cd)
    keys, bad = {}, []
    for (g, k), f in shown_fns().items():
        if f is None:
            keys[k] = {"cli_only": False, "project_only": False, "both": "none", "verdict": "not-shown (%s)" % NOT_SHOWN[k]}
            continue
        cli, proj, both = f(g + "-cli"), f(g + "-proj"), f(g + "-both")
        e = {"group": g, "cli_only": cli == "cli", "project_only": proj == "project", "both": both}
        if not e["cli_only"]:
            reason = skipped().get(g + "-cli") or ("value not seen" if resp(g + "-cli") else why_not(g + "-cli"))
            e["verdict"] = "not-shown (%s)" % reason
        elif both in ("cli", "project"):
            e["verdict"] = both
        else:
            e["verdict"] = "not-shown (no evidence in %s-both)" % g
        if e["cli_only"] and e["verdict"] != "cli":
            bad.append(k)
        keys[k] = e
    if ran("pc3-compact-v2"):
        kind, _ = compaction("pc3-compact-v2")
        keys["model_providers.mock.capabilities.remote_compaction"]["v2_run"] = kind or "none"
    shown = [k for k, e in keys.items() if e["cli_only"]]
    if bad:
        r, w = "failed", "cli did not win for: " + ", ".join(bad)
    elif not shown:
        r, w = "failed", "no key showed its cli value"
    else:
        r, w = "verified", "the -c value wins for every key whose cli value shows"
    prec = {"keys": keys, "result": r, "why": w}
    # files-override.json: a measure (its result says only whether the runs were measured).
    t = first_text("pc3-agents")
    agents = ("both" if "zp-agent-home" in t and "zp-agent-project" in t else "home" if "zp-agent-home" in t
              else "project" if "zp-agent-project" in t else "none")
    hk = who("pc3-hooks", "hooks-who")
    hooks = "both" if "home" in hk and "project" in hk else hk[0] if hk else "none"
    measured = bool(t) and bool(resp("pc3-hooks"))
    wjson(os.path.join(RUN, "sum", "config", "files-override.json"), {
        "agents": agents, "hooks": hooks, "hooks_lines": hk,
        "result": "verified" if measured else "failed",
        "why": "measured (no pass value): agent descriptions in the first request, hook markers written"
               if measured else "not measured: " + (why_not("pc3-agents") if not t else why_not("pc3-hooks"))})
    all_ok = home["result"] == cd["result"] == r == "verified"
    prec["pc3_result"] = "verified" if all_ok else "failed"
    prec["pc3_why"] = ("codex-home.json, cd.json and precedence.json are verified" if all_ok else
                       "failed: " + ", ".join(n for n, x in (("codex-home.json", home["result"]), ("cd.json", cd["result"]),
                                                            ("precedence.json", r)) if x != "verified"))
    wjson(os.path.join(RUN, "sum", "config", "precedence.json"), prec)


PC4 = ["pc4-none", "pc4-bypass", "pc4-untrusted", "pc4-trust", "pc4-trust-new", "pc4-log", "pc4-feature-default",
       "pc4-feature-legacy", "pc4-tui", "pc4-tui-after", "pc4-ctl-neg", "pc4-ctl-pos"]
SOURCES = ("home-inline", "home-json", "project-inline", "project-json")


def trust_log_lines(paths):
    """Lines about hook trust or approval (never a feedback_tags dump)."""
    out = []
    for p in paths:
        for l in rd(p).splitlines():
            if "feedback_tags" in l:
                continue
            if re.search("hook", l, re.I) and re.search(r"trust|approv", l, re.I):
                out.append(l)
    return out


def sqlite_dump(db, needles):
    """Every table of a SQLite file (opened read-only), and each column value that mentions hook,
    trust, approv or a needle (at most 50 per table, 300 characters each)."""
    out = ["db %s" % os.path.basename(db)]
    pat = re.compile("|".join(["hook", "trust", "approv"] + [re.escape(n) for n in needles if n]), re.I)
    con = None
    for mode in ("ro", "ro&immutable=1"):
        try:
            con = sqlite3.connect("file:%s?mode=%s" % (urllib.parse.quote(db), mode), uri=True)
            tables = [r[0] for r in con.execute("select name from sqlite_master where type='table' order by name")]
            break
        except sqlite3.Error as x:
            err = x
            con = None
    if con is None:
        return out + ["  error: %s" % err]
    for t in tables:
        out.append("table %s" % t)
        qt = '"%s"' % t.replace('"', '""')
        try:
            cols = [c[1] for c in con.execute("pragma table_info(%s)" % qt)]
            n = 0
            for row in con.execute("select * from %s" % qt):
                for c, v in zip(cols, row):
                    if v is None:
                        continue
                    v = v.decode("utf-8", "replace") if isinstance(v, bytes) else str(v)
                    if "feedback_tags" in v or not pat.search(v):
                        continue
                    out.append("  %s.%s: %s" % (t, c, v[:300].replace("\n", " ")))
                    n += 1
                if n >= 50:
                    out.append("  (more rows not shown)")
                    break
        except sqlite3.Error as x:
            out.append("  error: %s" % x)
    con.close()
    return out


def log_rows(db):
    """'level | target | body' of each row of a logs_2.sqlite logs table, read-only (mode=ro, else
    immutable=1: a database with a -wal file and no -shm cannot be opened ro); [] when absent."""
    if not os.path.exists(db):
        return []
    for mode in ("ro", "ro&immutable=1"):
        try:
            con = sqlite3.connect("file:%s?mode=%s" % (urllib.parse.quote(db), mode), uri=True)
            rows = [" | ".join(str(x) for x in r if x is not None)
                    for r in con.execute("select level, target, feedback_log_body from logs order by id")]
            con.close()
            return rows
        except sqlite3.Error as x:
            err = x
    return ["error: %s" % err]


def tree(d, skip_git=False):
    """'<size> <path>' for each file under d (relative paths, sorted; .git/ left out when asked)."""
    out = []
    for dp, dirs, fs in os.walk(d):
        if skip_git and ".git" in dirs:
            dirs.remove(".git")
        for f in fs:
            p = os.path.join(dp, f)
            try:
                out.append((os.path.relpath(p, d), os.lstat(p).st_size))
            except OSError:
                pass
    return ["%d %s" % (n, r) for r, n in sorted(out)] or ["(empty)"]


def cmd_evidence():
    """evidence <scenario> <CODEX_HOME> <repo> <HOME> <marker file>: sum/trust/evidence-<s>.txt."""
    sc, cxh, rdir, hdir, marker = A[:5]
    cmds = ["echo %s >>%s" % (x, marker) for x in SOURCES]
    needles = cmds + [hashlib.sha256(c.encode()).hexdigest() for c in cmds]
    out = ["== %s: CODEX_HOME/config.toml, before the run -> after (unified diff)" % sc]
    before = rd(os.path.join(RUN, "out", sc + ".config.before")).splitlines()
    after = rd(os.path.join(cxh, "config.toml")).splitlines()
    out += list(difflib.unified_diff(before, after, "before", "after", lineterm="")) or ["(no change)"]
    out.append("== SQLite under CODEX_HOME (every table; values that mention hook, trust, approv, a hook command or its sha256)")
    dbs = sorted(os.path.join(dp, f) for dp, _, fs in os.walk(cxh) for f in fs if f.endswith(".sqlite"))
    for db in dbs:
        out += sqlite_dump(db, needles)
    if not dbs:
        out.append("(no .sqlite file)")
    out.append("== CODEX_HOME files (size path)")
    out += tree(cxh)
    out.append("== repo files (size path; .git/ left out)")
    out += tree(rdir, skip_git=True)
    out.append("== HOME files (size path)")
    out += tree(hdir)
    with open(os.path.join(RUN, "sum", "trust", "evidence-%s.txt" % sc), "w") as f:
        f.write("\n".join(out) + "\n")


TUI_PROMPT = "Hooks need review"
TUI_OPTION = "Trust all and continue"


def cmd_tui_choice():
    """tui-choice <pane file>: the digit of the option labelled exactly TUI_OPTION on a pane that shows
    TUI_PROMPT; else a driver-failed verdict (never a K-D13 verdict)."""
    t = rd(A[0])
    if TUI_PROMPT not in t:
        print("driver-failed: prompt not recognised")
        return
    for d, label in re.findall(r"(?m)^\W*(\d)\.\s+(.+?)\s*$", t):
        if label == TUI_OPTION:
            print(d)
            return
    print("driver-failed: no option labelled '%s'" % TUI_OPTION)


# --- G3: what trusted_hash is computed over ----------------------------------------------------
def toml_handlers(text):
    """{(event, group, handler): (matcher, handler dict)} of a config.toml's [[hooks.<E>]] groups (the
    shapes this part writes: matcher, type, command, timeout)."""
    out, ev, g, h, matcher, cur, counts = {}, None, -1, -1, None, None, {}
    for line in text.splitlines():
        m = re.match(r"\s*\[\[hooks\.([A-Za-z]+)\]\]\s*$", line)
        if m:
            ev = m.group(1)
            counts[ev] = counts.get(ev, -1) + 1
            g, h, matcher, cur = counts[ev], -1, None, None
            continue
        if re.match(r"\s*\[\[hooks\.[A-Za-z]+\.hooks\]\]\s*$", line) and ev:
            h += 1
            cur = {}
            out[(ev, g, h)] = (matcher, cur)
            continue
        if re.match(r"\s*\[", line):
            ev, cur = None, None
            continue
        kv = re.match(r'\s*([A-Za-z_]+)\s*=\s*(.+?)\s*$', line)
        if kv and ev:
            v = json.loads(kv.group(2)) if kv.group(2)[:1] in '"0123456789-tf' or kv.group(2) in ("true", "false") else kv.group(2)
            if cur is None and kv.group(1) == "matcher":
                matcher = v
            elif cur is not None:
                cur[kv.group(1)] = v
    return out


def json_handlers(text):
    out = {}
    for ev, groups in (json.loads(text).get("hooks") or {}).items():
        for g, grp in enumerate(groups):
            for h, hd in enumerate(grp.get("hooks") or []):
                out[(ev, g, h)] = (grp.get("matcher"), hd)
    return out


def snake(e):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", e).lower()


def hash_records(cxh, sources):
    """[(key, hash, source file, event, group, handler index, matcher, handler)] for each hooks.state
    record of CODEX_HOME's config.toml whose source is in <sources> (real path -> file to read)."""
    cfg = rd(os.path.join(cxh, "config.toml"))
    out = []
    for key, hx in re.findall(r'\[hooks\.state\."([^"]+)"\]\s*\ntrusted_hash = "sha256:([0-9a-f]+)"', cfg):
        m = re.match(r"(.+):([a-z_]+):(\d+):(\d+)$", key)
        if not m:
            continue
        f, ev_s, g, h = m.group(1), m.group(2), int(m.group(3)), int(m.group(4))
        text = rd(sources.get(f, f))
        hs = json_handlers(text) if f.endswith(".json") else toml_handlers(text)
        hit = next(((ev, mt, hd) for (ev, gg, hh), (mt, hd) in hs.items() if snake(ev) == ev_s and gg == g and hh == h), None)
        if hit:
            out.append((key, hx, f, hit[0], g, h, hit[1], hit[2]))
    return out


DEFAULTS = {"timeout": None, "async": False, "statusMessage": None, "commandWindows": None, "additionalContextLimit": None}
SNAKE = {"statusMessage": "status_message", "commandWindows": "command_windows", "additionalContextLimit": "additional_context_limit"}


def hash_candidates(rec, file_bytes):
    """(template tag, text) pairs for one record; a tag names the template, never a value."""
    key, _, f, ev, g, h, matcher, hd = rec
    cmd = hd.get("command", "")
    yield "command", cmd
    yield "key", key
    yield "file-bytes", file_bytes
    for sep in ("\n", ":", "\0", "|", ""):
        yield "key%rcommand" % sep, key + sep + cmd
        yield "command%rkey" % sep, cmd + sep + key
        for evn in (ev, snake(ev)):
            yield "event%s%rcommand" % ("" if evn == ev else "-snake", sep), evn + sep + cmd
    present = [k for k in hd if k not in ("type", "command")]
    opt = [k for k in DEFAULTS if k not in hd]
    for n in range(len(opt) + 1):
        for extra in itertools.combinations(opt, n):
            fields = ["type", "command"] + present + list(extra)
            for camel in (True, False):
                nm = (lambda x: x) if camel else (lambda x: SNAKE.get(x, x))
                orders = itertools.permutations(fields) if len(fields) <= 5 else [tuple(fields), tuple(sorted(fields))]
                for order in orders:
                    d = {nm(k): (hd[k] if k in hd else DEFAULTS[k]) for k in order}
                    for sep in ((",", ":"), (", ", ": ")):
                        tag = "handler[%s|%s|%s]" % ("/".join(order), "camel" if camel else "snake", "compact" if sep[0] == "," else "spaced")
                        yield tag, json.dumps(d, separators=sep)
                        for gt, gd in (("group", {"hooks": [d]}), ("group+matcher", {"matcher": matcher, "hooks": [d]}),
                                       ("matcher+group", {"hooks": [d], "matcher": matcher})):
                            yield gt + tag, json.dumps(gd, separators=sep)
                        for evn in (ev, snake(ev)):
                            for ek in ("event", "event_name", "hook_event_name"):
                                yield "%s=%s+matcher+%s" % (ek, "camel" if evn == ev else "snake", tag), \
                                    json.dumps(dict({ek: evn, "matcher": matcher}, **d), separators=sep)


def sha(t):
    return hashlib.sha256(t if isinstance(t, bytes) else t.encode()).hexdigest()


def find_rule(recs, files):
    """The template tags that give every record's hash (empty when none)."""
    common = None
    for rec in recs:
        tags = {tag for tag, t in hash_candidates(rec, files.get(rec[2], b"")) if sha(t) == rec[1]}
        common = tags if common is None else common & tags
    return sorted(common or [])


def cmd_hashrule():
    """hashrule <scenario> <CODEX_HOME> <real path=file>...: sum/trust/hash-rule.json; with a rule,
    x/ctl-pos.state (the positive control's record) for $RUN/cxh-pc4-ctl-pos/config.toml."""
    sc, cxh = A[:2]
    sources = dict(a.split("=", 1) for a in A[2:])
    recs = hash_records(cxh, sources)
    files = {}
    for r in recs:
        try:
            files[r[2]] = open(sources.get(r[2], r[2]), "rb").read()
        except OSError:
            files[r[2]] = b""
    rule = find_rule(recs, files) if recs else []
    same = {}
    for r in recs:
        same.setdefault(r[7].get("command"), []).append(r)
    dep = []
    for cmd_, rs in same.items():
        if len(rs) > 1:
            for r in rs:
                dep.append({"key": r[0].rsplit("/", 1)[-1], "event": r[3], "group": r[4], "matcher": r[6],
                            "handler": r[7], "hash": r[1]})
    n = sum(1 for r in recs[:1] for _ in hash_candidates(r, files.get(r[2], b"")))
    out = {"scenario": sc, "records": [{"key": r[0], "trusted_hash": "sha256:" + r[1], "event": r[3], "group": r[4],
                                        "handler_index": r[5], "matcher": r[6], "handler": r[7]} for r in recs],
           "candidates_per_record": n, "rule": rule[0] if rule else None, "rules_matching_all": rule[:10],
           "same_command_records": dep}
    if rule:
        ctl = os.path.realpath(os.path.join(RUN, "cxh-pc4-ctl-pos")) + "/config.toml"
        hd = {"type": "command", "command": "echo ctl-pos >>%s/trust-pc4-ctl-pos" % RUN}
        rec = (ctl + ":session_start:0:0", "", ctl, "SessionStart", 0, 0, None, hd)
        text = dict(hash_candidates(rec, b""))[rule[0]]
        with open(os.path.join(RUN, "x", "ctl-pos.state"), "w") as fh:
            fh.write('\n[hooks.state."%s"]\ntrusted_hash = "sha256:%s"\n' % (rec[0], sha(text)))
        out["positive_control_record"] = rec[0] + " sha256 by rule " + rule[0]
    wjson(os.path.join(RUN, "sum", "trust", "hash-rule.json"), out)
    print(rule[0] if rule else "")


def cmd_variants():
    """variants <scenario> <home hooks.json> <project hooks.json>: add groups with one shared command:
    home: SessionStart g1 (no matcher), g2 (matcher zpm), g3 (timeout 30), UserPromptSubmit g0;
    project: SessionStart g1. Equal hashes for equal inputs show what the hash covers."""
    sc, home, proj = A[:3]
    var = "echo zp-var >>%s/var-%s" % (RUN, sc)
    hd = {"type": "command", "command": var}
    d = json.load(open(home))
    d["hooks"]["SessionStart"] += [{"hooks": [hd]}, {"matcher": "zpm", "hooks": [hd]}, {"hooks": [dict(hd, timeout=30)]}]
    d["hooks"]["UserPromptSubmit"] = [{"hooks": [hd]}]
    json.dump(d, open(home, "w"))
    d = json.load(open(proj))
    d["hooks"]["SessionStart"] += [{"hooks": [hd]}]
    json.dump(d, open(proj, "w"))


def cmd_tuilog():
    """tuilog <scenario> <CODEX_HOME>: trust/tui-log-lines.txt: the TUI's stderr, CODEX_HOME/log and
    logs_2.sqlite lines about config writes, trust, hooks, MCP URLs or a loopback address (no
    feedback_tags line; at most 200)."""
    sc, cxh = A[:2]
    pat = re.compile(r"config/batchWrite|config/write|config/value|hooks/|trust|/mcp\b|127\.0\.0\.1:\d+|listen", re.I)
    out = []
    for p in [os.path.join(RUN, sc + "-stderr.txt")] + sorted(
            os.path.join(dp, f) for dp, _, fs in os.walk(os.path.join(cxh, "log")) for f in fs):
        out += ["%s: %s" % (os.path.basename(p), l[:400]) for l in rd(p).splitlines() if pat.search(l) and "feedback_tags" not in l]
    for t in log_rows(os.path.join(cxh, "logs_2.sqlite")):
        if pat.search(t) and "feedback_tags" not in t:
            out.append("logs_2.sqlite: " + t[:400])
    with open(os.path.join(RUN, "sum", "trust", "tui-log-lines.txt"), "w") as f:
        f.write("\n".join(out[:200] or ["none: no TUI stderr or log line about config writes, trust, hooks or a loopback URL"]) + "\n")


def pc4():
    scen, where = {}, []
    for s in PC4 + ["pc4-prewritten"]:
        if not ran(s):
            scen[s] = {"ran": False, "why": skipped().get(s) or ("not-run: no record form" if s == "pc4-prewritten" else "not run")}
            continue
        _, keys = state_tables(rd(os.path.join(RUN, "out", s + ".config.toml")))
        scen[s] = {"ran": True, "exit": rc(s), "request": bool(resp(s)),
                   "sources_run": sorted(set(lines(os.path.join(RUN, "trust-" + s)))), "hooks_state_keys": keys,
                   "evidence": "evidence-%s.txt" % s}
        if keys:
            where.append("%s: CODEX_HOME/config.toml [hooks.state.\"<key>\"]" % s)
    pre = scen["pc4-prewritten"]
    if not pre["ran"]:
        honored = "not-run"
    else:
        src = rd(os.path.join(RUN, "out", "pc4-prewritten.src")).strip()
        pre["form_from"] = src
        honored = bool(set(pre["sources_run"]) & set(SOURCES))
    bypass = sorted(set(scen["pc4-bypass"].get("sources_run", [])) - set(scen["pc4-none"].get("sources_run", [])))
    driver = rd(os.path.join(RUN, "out", "pc4-tui.driver")).strip()
    hr = json.loads(rd(os.path.join(RUN, "sum", "trust", "hash-rule.json"), "{}"))
    neg = lines(os.path.join(RUN, "trust-pc4-ctl-neg")) if ran("pc4-ctl-neg") else None
    pos = lines(os.path.join(RUN, "trust-pc4-ctl-pos")) if ran("pc4-ctl-pos") else None
    controls = {"negative (wrong hash, must not run)": ("ran: %s" % neg if neg else "did not run") if neg is not None else why_not("pc4-ctl-neg"),
                "positive (record computed by the rule for a command Codex never saw, must run)":
                    ("ran" if pos else "did not run") if pos is not None else "not-run: no hash rule found"}
    if driver:
        r, w = driver, "the TUI driver did not see the prompt or the option; no K-D13 verdict"
    elif honored is True and hr.get("rule") and neg == [] and pos:
        r, w = "verified", "replayed records are honored; trusted_hash = sha256 of %s; a wrong hash does not run; a computed record runs" % hr["rule"]
    elif honored is True:
        r, w = "partial: replay only", "records Codex wrote are honored when written before launch, but %s" % (
            "the hash rule is not found, so zygo cannot compute a record for its own command" if not hr.get("rule") else
            "the controls did not behave (%s)" % controls)
    elif honored == "not-run":
        r, w = "failed", "no record form found (K-D13 takes branch 2 or 3)" if any(x["ran"] for x in scen.values()) else \
            "no PC4 run reached Codex"
    else:
        r, w = "failed", "prewritten records were not honored (K-D13 takes branch 2 or 3)"
    wjson(os.path.join(RUN, "sum", "trust", "trust.json"), {
        "scenarios": scen, "where_kept": where or "none written", "bypass_effect": bypass,
        "feature_flag": {"none": scen["pc4-feature-default"].get("sources_run"),
                         "codex_hooks": scen["pc4-feature-legacy"].get("sources_run"),
                         "hooks": scen["pc4-bypass"].get("sources_run"),
                         "warnings": sorted({l for s in ("pc4-feature-default", "pc4-feature-legacy", "pc4-bypass")
                                             for l in lines(os.path.join(RUN, "out", s + ".err")) if "deprecated" in l.lower()}),
                         "note": "each run: --dangerously-bypass-hook-trust; every other scenario sets [features] hooks = true"},
        "trust_flag_on_project_without_entry": scen["pc4-trust-new"].get("sources_run"),
        "hash_rule": hr.get("rule") or "not found (%s candidates per record)" % hr.get("candidates_per_record", 0),
        "controls": controls,
        "tui": {"driver": driver or "prompt '%s' seen, option '%s' chosen" % (TUI_PROMPT, TUI_OPTION),
                "keys_sent": lines(os.path.join(RUN, "out", "pc4-tui.keys")) or "none",
                "sources_run_in_tui": scen["pc4-tui"].get("sources_run"),
                "sources_run_in_exec_after": scen["pc4-tui-after"].get("sources_run"),
                "captures": sorted(f for f in os.listdir(os.path.join(RUN, "sum", "trust")) if f.startswith("tui-"))},
        "prewritten_honored": honored, "result": r, "why": w})
    out = []
    for s in PC4 + ["pc4-prewritten"]:
        if not ran(s):
            continue
        out.append("== %s files" % s)
        out += lines(os.path.join(RUN, "out", s + ".files"))
        out.append("== %s config.toml after the run (whole file)" % s)
        out += rd(os.path.join(RUN, "out", s + ".config.toml")).splitlines()
    with open(os.path.join(RUN, "sum", "trust", "codex-home-after.txt"), "w") as f:
        f.write("\n".join(out or ["no PC4 run"]) + "\n")
    logs = []
    for s in PC4 + ["pc4-prewritten"]:
        src = [os.path.join(RUN, "out", s + ".err")]
        for d, _, fs in os.walk(os.path.join(RUN, "out", s + ".log")):
            src += [os.path.join(d, f) for f in sorted(fs)]
        logs += ["%s: %s" % (s, l) for l in trust_log_lines(src)]
    with open(os.path.join(RUN, "sum", "trust", "log-lines.txt"), "w") as f:
        f.write("\n".join(logs[:200] or ["none: no PC4 stderr or log line is about hook trust or approval"]) + "\n")


REP_CLASSES = ("shell", "mcp")
# REP_SHAPES: the PostToolUse replacement outputs tried on each class (F5).
REP_SHAPES = {"str": {"updatedToolOutput": "zp-replaced"},
              "obj": {"updatedToolOutput": {"content": [{"type": "text", "text": "zp-replaced"}]}},
              "mcp": {"updatedMCPToolOutput": {"content": [{"type": "text", "text": "zp-replaced"}]}}}
REP = ["pc5-rep-%s-%s" % (c, x) for c in REP_CLASSES for x in REP_SHAPES]
PC5_CLASS = [("pc5-shell", "shell"), ("pc5-patch", "patch"), ("pc5-mcp", "mcp"), ("pc5-sub", "spawn")] + \
    [(x, x.split("-")[2]) for x in REP]
PC5_ORDER = ["pc5-probe", "pc5-shell", "pc5-patch", "pc5-mcp", "pc5-mcp-noapprove", "pc5-sub", "pc5-match", "pc5-ctx-start", "pc5-ctx-prompt",
             "pc5-deny", "pc5-deny-block", "pc5-stop-block", "pc5-post-block"] + REP
SHAPES = dict({
    "pc5-ctx-start": ("context", "SessionStart"), "pc5-ctx-prompt": ("context", "UserPromptSubmit"),
    "pc5-deny": ("deny", "PreToolUse"), "pc5-deny-block": ("deny", "PreToolUse"),
    "pc5-stop-block": ("block", "Stop"), "pc5-post-block": ("block", "PostToolUse")},
    **{x: ("replacement", "PostToolUse") for x in REP})


def matcher_fired(s, e):
    """label -> True when the matcher group's hook ran in scenario s for event e."""
    return {label: bool(payloads(s, "%s-m-%s" % (e, label))) for label, _ in MATCHERS}


def cmd_matchers():
    s, e = A[:2]
    print(" ".join("%s=%s" % (l, "fired" if v else "not-fired") for l, v in sorted(matcher_fired(s, e).items())))


def codex_reason(s):
    """Codex's own stderr and log lines about a PostToolUse output it did not accept (at most 5)."""
    pat = re.compile(r"PostToolUse|updated(MCP)?ToolOutput|hookSpecificOutput", re.I)
    bad = re.compile(r"fail|error|invalid|unsupported|unknown|reject|ignor", re.I)
    out = [l for l in lines(os.path.join(RUN, "out", s + ".err")) if pat.search(l) and bad.search(l) and "feedback_tags" not in l]
    for t in log_rows(os.path.join(RUN, "cxh-" + s, "logs_2.sqlite")):
        if pat.search(t) and bad.search(t) and "feedback_tags" not in t:
            out.append(t)
    return [re.sub(r"^\S+Z\s+", "", l)[:300] for l in out[:5]]


def hook_status(s, e):
    """Codex's own status lines for event e on stderr ("hook: <Event> Completed|Failed|...")."""
    return sorted(set(re.findall(r"hook: %s (\w+)" % e, rd(os.path.join(RUN, "out", s + ".err")))))


def payloads(s, e):
    d = os.path.join(RUN, "hk", s, e)
    try:
        return [os.path.join(d, f) for f in sorted(os.listdir(d)) if f.endswith(".json") and f[0].isdigit()]
    except OSError:
        return []


def pc5(tools_env, r_dir):
    env = dict(l.split("=", 1) for l in lines(tools_env) if "=" in l)
    env = {k: v.strip("'") for k, v in env.items()}
    out_files, events = [], {}
    for e in EVENTS:
        if e in ("PreToolUse", "PostToolUse"):
            done = set()
            for s, c in PC5_CLASS:
                ps = payloads(s, e)
                if ps and c not in done:
                    done.add(c)
                    out_files.append(("%s-%s.json" % (e, c), ps[0]))
            seen = [s for s in PC5_ORDER if payloads(s, e)]
        else:
            first = next((payloads(s, e)[0] for s in PC5_ORDER if payloads(s, e)), None)
            if first:
                out_files.append(("%s.json" % e, first))
            seen = [s for s in PC5_ORDER if payloads(s, e)]
        events[e] = seen or ("not triggered (exec has no interrupt)" if e == "Interrupt" else "not seen")
    with open(os.path.join(RUN, "sum", "hooks", ".payloads"), "w") as f:
        for n, p in out_files:
            f.write("%s %s\n" % (n, p))
    seen_names = {}
    for s, c in PC5_CLASS:
        names = set(seen_names.get(c, []))
        for e in ("PreToolUse", "PostToolUse"):
            for p in payloads(s, e):
                n = json.load(open(p)).get("tool_name")
                if n:
                    names.add(n)
        seen_names[c] = sorted(names)
    nf = lambda v, c: v or "not-shown: no %s tool in the request" % c
    tools = {"shell": nf(env.get("SHELL_TOOL"), "shell"), "shell_arg": env.get("SHELL_ARG", ""),
             "shell_array": env.get("SHELL_ARRAY") == "1",
             "patch": nf(env.get("PATCH_TOOL"), "patch"), "patch_kind": env.get("PATCH_KIND", ""),
             "spawn": nf(env.get("SPAWN_TOOL"), "spawn"), "spawn_arg": env.get("SPAWN_ARG", ""),
             "spawn_namespace": env.get("SPAWN_NS", ""), "mcp": nf(env.get("MCP_TOOL"), "mcp"),
             "mcp_namespace": env.get("MCP_NS", ""), "all_tools": [t for t in env.get("ALL_TOOLS", "").split(",") if t],
             "hook_tool_names": seen_names}
    tools["result"] = "verified" if env.get("SHELL_TOOL") else "failed"
    tools["why"] = "tool names read from pc5-probe's first request" if env.get("SHELL_TOOL") else \
        "not-shown: no shell tool in the request"
    wjson(os.path.join(RUN, "sum", "hooks", "tools.json"), tools)
    outs, kinds = {}, {}
    for s, (kind, ev) in SHAPES.items():
        sent = rd(os.path.join(RUN, "hk-rules", s + ".json"))
        rs = resp(s)
        fired = [p for p in payloads(s, ev) if os.path.exists(p[:-5] + ".out")]
        o = {"kind": kind, "event": ev, "output": json.loads(sent)[0]["output"] if sent else None,
             "hook_output_sent": bool(fired), "requests": len(rs)}
        if not ran(s):
            o.update(accepted=False, evidence=why_not(s))
        elif s in ("pc5-ctx-start", "pc5-ctx-prompt"):
            m = "zp-ctx-start" if s == "pc5-ctx-start" else "zp-ctx-prompt"
            o.update(accepted=bool(rs) and m in btext(rs[0]), evidence="%s in request 1: %s" % (m, bool(rs) and m in btext(rs[0])))
        elif s in ("pc5-deny", "pc5-deny-block"):
            m, f = ("zp-deny", "zp-ran-deny") if s == "pc5-deny" else ("zp-deny2", "zp-ran-deny2")
            ran_ = os.path.exists(os.path.join(r_dir, f))
            hit = len(rs) > 1 and re.search(re.escape(m) + r"(?![0-9])", btext(rs[1])) is not None
            o.update(accepted=(not ran_) and hit, evidence="command ran: %s; %s in request 2: %s" % (ran_, m, hit))
        elif s == "pc5-stop-block":
            idx = [i + 1 for i, r in enumerate(rs) if i > 0 and "zp-block" in btext(r)]
            o.update(accepted=bool(idx), evidence="zp-block in request(s) %s" % idx)
        elif s in REP:
            orig = "zp-orig" if "-shell-" in s else "tracer-mcp-probe-result"
            t = tool_outputs(rs[1]) if len(rs) > 1 else ""
            o.update(accepted="zp-replaced" in t and orig not in t, codex_status=hook_status(s, ev), codex_reason=codex_reason(s),
                     evidence="request 2 tool output holds zp-replaced: %s, %s: %s; hook status %s" % (
                         "zp-replaced" in t, orig, orig in t, hook_status(s, ev) or "none"))
        else:
            hit = len(rs) > 1 and "zp-post" in btext(rs[1])
            o.update(accepted=hit, evidence="zp-post in request 2: %s" % hit)
        outs[s] = o
        kinds[kind] = kinds.get(kind, False) or o["accepted"]
    replacement = {c: {x: (outs["pc5-rep-%s-%s" % (c, x)]["accepted"] if ran("pc5-rep-%s-%s" % (c, x))
                           else why_not("pc5-rep-%s-%s" % (c, x))) for x in REP_SHAPES} for c in REP_CLASSES}
    matchers = {e: {"%s (%s)" % (label, m): matcher_fired("pc5-match", e)[label] for label, m in MATCHERS}
                for e in ("PreToolUse", "PostToolUse")} if ran("pc5-match") else why_not("pc5-match")
    if isinstance(matchers, dict):
        for e in matchers:
            matchers[e]["no matcher"] = bool(payloads("pc5-match", e))
    na = "pc5-mcp-noapprove"
    nat = tool_outputs(resp(na)[1]) if len(resp(na)) > 1 else ""
    mcp_noapprove = {"PermissionRequest_payload": bool(payloads(na, "PermissionRequest")),
                     "tool_output": nat[:300] if ran(na) else why_not(na),
                     "refusal_text_seen": "MCP tool call requires approval, but approval policy is never" in nat} \
        if ran(na) else why_not(na)
    subagent = {"attempts": [x for x in ("pc5-sub",) if ran(x)],
                "parent_kept_alive": "a %s call of sleep 15 after the spawn (rule pc5-sub-wait)" % env.get("SHELL_TOOL", "")
                if os.path.exists(os.path.join(RUN, "sse", "pc5-sub-wait.sse")) else "no",
                "parent_requests": len(resp("pc5-sub")),
                "SubagentStart": bool(payloads("pc5-sub", "SubagentStart")),
                "SubagentStop": bool(payloads("pc5-sub", "SubagentStop"))}
    need = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "SubagentStart", "SubagentStop"]
    missing = [e for e in need if not isinstance(events[e], list)] + \
              ["%s shape" % k for k in ("context", "deny", "block", "replacement") if not kinds.get(k)]
    wjson(os.path.join(RUN, "sum", "hooks", "outputs.json"), {
        "events": events, "shapes": outs, "replacement": replacement,
        "replacement_note": "each shape is the hookSpecificOutput of a PostToolUse output", "matchers": matchers,
        "mcp_without_approval_mode": mcp_noapprove, "subagent": subagent,
        "result": "failed" if missing else "verified",
        "why": "missing: " + ", ".join(missing) if missing else "every needed event has a payload and each kind an accepted shape"})


PC12 = ["pc12-default", "pc12-all", "pc12-exclude", "pc12-case", "pc12-core", "pc12-none"]


def pc12_grade(ex, de, case, all_):
    """(result, why) from the variable names of pc12-exclude, -default, -case and -all (None: not measured)."""
    if None in (ex, de, case, all_):
        return "failed", "not measured: " + ", ".join(n for n, v in (("pc12-exclude", ex), ("pc12-default", de),
                                                                     ("pc12-case", case), ("pc12-all", all_)) if v is None)
    bad = []
    if "MOCK_API_KEY" in ex or "HTTPS_PROXY" in ex:
        bad.append("pc12-exclude keeps an excluded name")
    if "ZP_SET" not in ex:
        bad.append("pc12-exclude lacks ZP_SET")
    if "MOCK_API_KEY" not in de:
        bad.append("pc12-default lacks MOCK_API_KEY")
    if "MOCK_API_KEY" not in all_:
        bad.append("the control pc12-all (inherit all, empty exclude) lacks MOCK_API_KEY")
    if bad:
        return "failed", "; ".join(bad)
    return "verified", "exclude and set apply in tool shells; the default and the inherit-all control pass MOCK_API_KEY; " \
        "pc12-case: exclude %s letter case" % ("ignores" if "MOCK_API_KEY" not in case else "keeps")


def cmd_pc12grade():
    v = [None if x == "-" else x.split() for x in A[:4]]
    print(pc12_grade(*v)[0])


def pc12():
    out = {}
    for s in PC12:
        rs = resp(s)
        t = tool_outputs(rs[1]) if len(rs) > 1 else None
        if t is None:
            out[s] = {"names": None, "why": why_not(s) if not rs else "no tool output in request 2"}
            continue
        # __CF_USER_TEXT_ENCODING: set by macOS for every process, not by Codex's policy.
        out[s] = {"names": sorted(set(re.findall(r"(?m)^([A-Za-z_][A-Za-z0-9_]*)=", t)) - {"__CF_USER_TEXT_ENCODING"})}
        if s == "pc12-exclude":
            out[s]["zp_set"] = "ZP_SET=zp-set" in t
    case = out["pc12-case"].get("names")
    if case is not None:
        out["pc12-case"]["exclude_ignores_case"] = "MOCK_API_KEY" not in case
    out["result"], out["why"] = pc12_grade(*(out[x].get("names") for x in ("pc12-exclude", "pc12-default", "pc12-case", "pc12-all")))
    wjson(os.path.join(RUN, "sum", "config", "shell-env.json"), out)


def network():
    """config/network.txt: per scenario, the refused CONNECT lines of its window (host counts) and
    its responses_websocket stderr lines (time stamps cut, first 3)."""
    conn = rd(os.path.join(RUN, "rec", "connect.txt")).splitlines()
    out, total = [], {}
    for s in lines(os.path.join(RUN, "out", "order.txt")):
        w = rd(os.path.join(RUN, "out", s + ".conn")).split()
        part = conn[int(w[0]):int(w[1])] if len(w) == 2 else []
        hosts = {}
        for l in part:
            h = l.rsplit(" refused", 1)[0]
            hosts[h] = hosts.get(h, 0) + 1
            total[h] = total.get(h, 0) + 1
        wsl = [re.sub(r"^\S+Z\s+", "", l) for l in lines(os.path.join(RUN, "out", s + ".err"))
               if WS.search(l) and "feedback_tags" not in l]
        out.append("== %s: connect %s; responses_websocket lines %d" % (
            s, ", ".join("%s x%d" % (h, n) for h, n in sorted(hosts.items())) or "none", len(wsl)))
        out += ["  " + l for l in wsl[:3]]
    out.insert(0, "all refused CONNECT lines: " + (", ".join("%s x%d" % (h, n) for h, n in sorted(total.items())) or "none"))
    with open(os.path.join(RUN, "sum", "config", "network.txt"), "w") as f:
        f.write("\n".join(out) + "\n")


def cmd_analyze():
    version, tools_env, r_dir, with_state, url, prem = A[:6]
    pc3()
    pc4()
    pc5(tools_env, r_dir)
    pc12()
    network()
    strict_report(version, with_state == "1", url, prem)


{"home": cmd_home, "sse": cmd_sse, "rule": cmd_rule, "shellargs": cmd_shellargs, "pc5toml": cmd_pc5toml,
 "tools": cmd_tools, "form": cmd_form, "strict-gen": cmd_strict_gen, "strict-judge": cmd_strict_judge,
 "analyze": cmd_analyze, "evidence": cmd_evidence, "tui-choice": cmd_tui_choice, "hashrule": cmd_hashrule, "variants": cmd_variants,
 "tuilog": cmd_tuilog,
 # entry points of the synthetic tests
 "shown": lambda: print(shown_fns()[(A[0], A[1])](A[2])), "control": lambda: print(control_line(A[0])),
 "logfilter": lambda: print("\n".join(trust_log_lines(A))), "sqlite-dump": lambda: print("\n".join(sqlite_dump(A[0], A[1:]))),
 "matchers": cmd_matchers, "pc12grade": cmd_pc12grade}[CMD]()
PY
cxpy() { python3 -I "$RUN/cx.py" "$RUN" "$@"; }

# CX_FEAT: the [features] flag cx_home sets to true (empty: none; pc4-feature-* measure the flag).
CX_FEAT=hooks
# cx_fresh: no project .codex folder of an earlier scenario is left in R or $RUN/other.
cx_fresh() { rm -rf "$R/.codex" "$RUN/other/.codex"; }
# CX_UNTRUSTED: "" trusts the projects, 1 writes trust_level = "untrusted", none writes no projects table.
# cx_home <scenario> env|auth [<extra toml file>]: a fresh CODEX_HOME $RUN/cxh-<scenario> (and fresh
# project folders) with config.toml at the mock: one base_url line, "$MOCK_URL/v1".
cx_home() {
  cx_fresh
  CX_HOME=$RUN/cxh-$1
  rm -rf "$CX_HOME"
  mkdir -p "$CX_HOME"
  cxpy home "$CX_HOME/config.toml" "$2" "${3:-}" "$MOCK_URL" "$DUMMY_KEY" "${CX_UNTRUSTED:-}" "$CX_FEAT" "$R" "$RR" "$RUN/other"
  [ "$(grep -c 'base_url' "$CX_HOME/config.toml")" = 1 ] || { echo "premises: $CX_HOME/config.toml has not one base_url line"; exit 1; }
}
# cx_project <dir> <toml file>: the project config of <dir>.
cx_project() { mkdir -p "$1/.codex"; cp "$2" "$1/.codex/config.toml"; }
cx_count() {
  _c=0
  for _f in "$RUN"/rec/[0-9]*.json; do [ ! -e "$_f" ] || _c=$((_c + 1)); done
  echo "$_c"
}
# cx_conn: the line count of the mock's connect.txt (0 when missing).
cx_conn() { if [ -e "$RUN/rec/connect.txt" ]; then wc -l <"$RUN/rec/connect.txt" | tr -d ' '; else echo 0; fi; }
# cx_pkg_rm <CODEX_EXE>: remove that run's package copy, the first folder of the path under $RUN.
cx_pkg_rm() {
  case "$1" in
    "$RUN"/codex-pkg.*/p/*) _pk=${1#"$RUN"/}; rm -rf "${RUN:?}/${_pk%%/*}" ;;
  esac
}
# cx_run <scenario> <codex exec args...>: one codex exec with prompt zp-<scenario>; out/<s>.out,
# .err, .rc, .seq (<record folder> <first> <last>); the side files a run writes (auth-who, mcp-who,
# hooks-who) move to out/<s>.<name>; project folders are kept in proj/<s>/; the package copy is removed.
CX_REC=rec-a
cx_run() {
  _s=$1
  shift
  _b=$(cx_count) _cb=$(cx_conn)
  rm -f "$RUN/auth-who" "$RUN/mcp-who" "$RUN/hooks-who"
  rm -f "$RUN/out/$_s.done"
  # A subshell: a harness refusal (n2_daemon_flag, codex_env, the daemon scan, check_abort) exits
  # it before .done is written, and its message is in .out/.err; Codex's own status is data.
  (
    _rc=0
    run_codex exec --skip-git-repo-check -s workspace-write -c 'approval_policy="never"' "$@" "zp-$_s" || _rc=$?
    echo "$_rc $CODEX_VERSION $CODEX_EXE" >"$RUN/out/$_s.done"
  ) </dev/null >"$RUN/out/$_s.out" 2>"$RUN/out/$_s.err" || true
  if [ ! -s "$RUN/out/$_s.done" ]; then
    echo "premises: $_s: the harness refused or stopped the run:"
    cat "$RUN/out/$_s.out" "$RUN/out/$_s.err"
    check_abort
    exit 1
  fi
  read -r _rc CODEX_VERSION CODEX_EXE <"$RUN/out/$_s.done"
  echo "$_rc" >"$RUN/out/$_s.rc"
  echo "$CX_REC $((_b + 1)) $(cx_count)" >"$RUN/out/$_s.seq"
  echo "$_cb $(cx_conn)" >"$RUN/out/$_s.conn"
  echo "$_s" >>"$RUN/out/order.txt"
  cx_pkg_rm "$CODEX_EXE"
  for _f in auth-who mcp-who hooks-who; do [ ! -e "$RUN/$_f" ] || mv "$RUN/$_f" "$RUN/out/$_s.$_f"; done
  mkdir -p "$RUN/proj/$_s"
  [ ! -d "$R/.codex" ] || cp -R "$R/.codex" "$RUN/proj/$_s/repo.codex"
  [ ! -d "$RUN/other/.codex" ] || cp -R "$RUN/other/.codex" "$RUN/proj/$_s/other.codex"
  echo "codex config: $_s exit $_rc, requests $(($(cx_count) - _b))"
}
# cx_reqs <scenario>: the paths of the scenario's records, in order.
cx_reqs() {
  read -r _d _a _z <"$RUN/out/$1.seq"
  while [ "$_a" -le "$_z" ]; do printf '%s/%s/%04d.json\n' "$RUN" "$_d" "$_a"; _a=$((_a + 1)); done
}
# cx_sse <file> text <text> | call <name> <call_id> <arguments json> [<namespace>] | custom <name> <call_id> <input>
cx_sse() { _o=$1 _k=$2; shift 2; cxpy sse "$_o" "$_k" "$@"; }
# cx_tools: tools.env from pc5-probe's first request (SHELL_TOOL, SHELL_ARG, SHELL_ARRAY, PATCH_TOOL,
# PATCH_KIND, SPAWN_TOOL, SPAWN_ARG, MCP_TOOL; empty when not offered).
cx_tools() {
  # shellcheck disable=SC2046
  cxpy tools "$RUN/tools.env" $(cx_reqs pc5-probe)
  # shellcheck disable=SC1091 # written at run time by cx.py tools
  . "$RUN/tools.env"
}
# shell_args <command text>: the arguments JSON for SHELL_TOOL.
shell_args() { cxpy shellargs "$SHELL_ARG" "$SHELL_ARRAY" "$1"; }
# b_need <tool> <class> <scenario>: 0 when the tool is known; else the scenario is recorded not-shown.
b_need() {
  [ -z "$1" ] || return 0
  echo "$3 not-shown: no $2 tool in the request" >>"$RUN/out/skipped.txt"
  return 1
}
# b_shell <scenario> <command text> [<usage json>]: phase B rule: the first reply is a SHELL_TOOL call.
b_shell() {
  b_need "$SHELL_TOOL" shell "$1" || return 0
  cx_sse "$RUN/sse/$1.sse" call "$SHELL_TOOL" "call_$(echo "$1" | tr - _)" "$(shell_args "$2")"
  cxpy rule "$RUN/rules.json" "$1" "$RUN/sse/$1.sse" "${3:-}"
}
b_ok() { [ -e "$RUN/sse/$1.sse" ]; }
# b_mcp <scenario>: phase B rule: the first reply is an MCP_TOOL call (in its namespace).
b_mcp() {
  b_need "$MCP_TOOL" mcp "$1" || return 0
  cx_sse "$RUN/sse/$1.sse" call "$MCP_TOOL" "call_$(echo "$1" | tr - _)" '{}' "$MCP_NS"
  cxpy rule "$RUN/rules.json" "$1" "$RUN/sse/$1.sse"
}
# pc3_group <group> <mode> <project toml> <cli -c args...>: runs <group>-cli, -proj and -both.
pc3_group() {
  _g=$1 _m=$2 _p=$3
  shift 3
  cx_home "$_g-cli" "$_m"
  cx_run "$_g-cli" "$@"
  cx_home "$_g-proj" "$_m"
  cx_project "$R" "$_p"
  cx_run "$_g-proj"
  cx_home "$_g-both" "$_m"
  cx_project "$R" "$_p"
  cx_run "$_g-both" "$@"
}
# hook_cmd <text>: a hooks.json SessionStart hook running <text>.
hook_json() { printf '{"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "%s"}]}]}}\n' "$1"; }
hook_toml() { printf '\n[[hooks.SessionStart]]\n[[hooks.SessionStart.hooks]]\ntype = "command"\ncommand = "%s"\n' "$1"; }
# pc4_sources <marker scenario>: the four hook sources, each writing its name to $RUN/trust-<scenario>.
pc4_sources() {
  _mk=$RUN/trust-$1
  hook_toml "echo home-inline >>$_mk" >>"$CX_HOME/config.toml"
  hook_json "echo home-json >>$_mk" >"$CX_HOME/hooks.json"
  mkdir -p "$R/.codex"
  hook_toml "echo project-inline >>$_mk" >"$R/.codex/config.toml"
  hook_json "echo project-json >>$_mk" >"$R/.codex/hooks.json"
}
# pc4_before <scenario>: CODEX_HOME's config.toml as it is before the run (for the evidence diff).
pc4_before() { cp "$CX_HOME/config.toml" "$RUN/out/$1.config.before"; }
# pc4_after <scenario> [<marker scenario>]: keep the CODEX_HOME config.toml, its file list and its log
# folder; write trust/evidence-<scenario>.txt (config diff, SQLite tables and hook rows, the trees
# of CODEX_HOME, the repo and HOME).
pc4_after() {
  cp "$CX_HOME/config.toml" "$RUN/out/$1.config.toml"
  (cd "$CX_HOME" && find . -type f | sed 's|^\./||' | LC_ALL=C sort) >"$RUN/out/$1.files"
  [ ! -d "$CX_HOME/log" ] || cp -R "$CX_HOME/log" "$RUN/out/$1.log"
  cxpy evidence "$1" "$CX_HOME" "$R" "$H" "$RUN/trust-${2:-$1}"
}
pc4_run() {
  _s4=$1
  shift
  cx_home "$_s4" env
  pc4_sources "$_s4"
  rm -f "$RUN/trust-$_s4"
  pc4_before "$_s4"
  cx_run "$_s4" "$@"
  pc4_after "$_s4"
}
# --- The TUI in a pane of a private tmux server (F3: interactive hook approval) ---
TMUX_BIN=$(command -v tmux) || { echo "premises: tmux is not on PATH"; exit 1; }
SOCK=$RUN/s
[ ${#SOCK} -le 100 ] || { echo "premises: the tmux socket path is longer than 100 characters; set N2_SCRATCH or TMPDIR to a short folder"; exit 1; }
tm() { LC_ALL=C "$TMUX_BIN" -S "$SOCK" -f /dev/null "$@"; }
tm_kill() { tm kill-server 2>/dev/null || true; }
n2_trap_chain tm_kill
# pane_wait <ERE> <polls>: up to <polls> captures of session $TS's pane, 0.5 s apart, into
# $RUN/pane.txt; 0 when one matches <ERE> (any case), 1 when the TUI ended or the polls ran out.
pane_wait() {
  _i=0
  while [ "$_i" -lt "$2" ]; do
    _i=$((_i + 1))
    tm capture-pane -p -t "=$TS:" >"$RUN/pane.tmp" 2>/dev/null || return 1
    mv "$RUN/pane.tmp" "$RUN/pane.txt"
    sleep 0.5
    ! grep -qiE -- "$1" "$RUN/pane.txt" || return 0
    [ ! -e "$RUN/$TS-exit" ] || return 1
  done
  return 1
}
# keep <name>: the last capture as trust/tui-<name>.txt (published through scrub); none: a note.
keep() { if [ -e "$RUN/pane.txt" ]; then cp "$RUN/pane.txt" "$RUN/sum/trust/tui-$1.txt"; else echo '(no capture)' >"$RUN/sum/trust/tui-$1.txt"; fi; }
key() { tm send-keys -t "=$TS:" "$1" 2>/dev/null || true; sleep 0.5; }
send() { tm send-keys -t "=$TS:" -l "$1" 2>/dev/null || true; sleep 0.5; key Enter; }
# tui_hooks <scenario>: the TUI (run_codex --no-daemon, no bypass flag) with the current CX_HOME:
# capture the start; while the pane shows a hook prompt (cx.py tui-choice), keep it and send the
# approving key (at most 3); one turn (zp-<scenario>, reply zp-tui-reply); /hooks; quit. Keys go to
# out/<scenario>.keys; .rc, .seq, .conn as cx_run. A harness refusal ends the part (exit 1).
tui_hooks() {
  TS=$1
  {
    printf "PREMISES_DIR='%s'\nN2_AGENTS=codex\nN2_SCRATCH='%s'\n" "$PREMISES_DIR" "$N2_SCRATCH"
    # shellcheck disable=SC2016
    echo '. "$PREMISES_DIR/lib.sh"'
    printf "RUN='%s'; H='%s'; R='%s'; CX_HOME='%s'; MOCK_URL='%s'; MOCK_PID='%s'\n" "$RUN" "$H" "$R" "$CX_HOME" "$MOCK_URL" "$MOCK_PID"
    echo "trap '[ -e \"\$RUN/$TS-exit\" ] || : >\"\$RUN/$TS-refused\"' EXIT"
    # --no-daemon: a TUI run must not start the shared daemon (n2_daemon_flag).
    echo "set -- --no-daemon -c 'approval_policy=\"never\"'"
    echo "{ n2_daemon_flag \"\$@\"; codex_env; n2_procs daemon || exit 1; } >\"\$RUN/$TS-pre.txt\" 2>&1"
    # G5: RUST_LOG (on EXTRA_ENV_ALLOW) and the TUI's own stderr in a file.
    echo "EXTRA_ENV=RUST_LOG=debug"
    echo "rc=0; run_codex \"\$@\" 2>\"\$RUN/$TS-stderr.txt\" || rc=\$?; echo \"\$rc\" >\"\$RUN/$TS-exit\""
  } >"$RUN/$TS-tui.sh"
  rm -f "$RUN/$TS-exit" "$RUN/$TS-refused" "$RUN/pane.txt"
  : >"$RUN/out/$TS.keys"
  _b=$(cx_count) _cb=$(cx_conn)
  tm new-session -d -s "$TS" -x 220 -y 50 /bin/sh "$RUN/$TS-tui.sh"
  rm -f "$RUN/out/$TS.driver"
  pane_wait 'Hooks need review' 60 || true
  keep start
  _key=$(cxpy tui-choice "$RUN/pane.txt")
  case "$_key" in
    driver-failed*)
      # No K-D13 verdict from this run: the screen is kept as tui-driver-failed.txt.
      echo "$_key" >"$RUN/out/$TS.driver"
      keep driver-failed ;;
    *)
      keep prompt-1
      # A digit only moves the selection ("enter confirm" on the prompt): Enter confirms it.
      echo "prompt 1: option $_key ('Trust all and continue'), then Enter" >>"$RUN/out/$TS.keys"
      key "$_key"
      key Enter
      pane_wait 'gpt-mock' 10 || true
      keep after-key-1 ;;
  esac
  send "zp-$TS"
  pane_wait zp-tui-reply 120 || true
  keep turn
  send /hooks
  pane_wait hook 6 || true
  keep hooks-command
  key Escape; key Escape; key C-c; key C-c
  _i=0
  while [ ! -e "$RUN/$TS-exit" ] && [ "$_i" -lt 20 ]; do _i=$((_i + 1)); sleep 0.5; done
  tm_kill
  if [ -e "$RUN/$TS-refused" ] || [ ! -e "$RUN/$TS-exit" ]; then
    echo "premises: $TS: the TUI run was refused or did not end:"
    cat "$RUN/$TS-pre.txt" 2>/dev/null || true
    grep -h '^premises:' "$RUN/pane.txt" 2>/dev/null || true
    check_abort
    exit 1
  fi
  cp "$RUN/$TS-exit" "$RUN/out/$TS.rc"
  cp "$RUN/$TS-pre.txt" "$RUN/out/$TS.err"
  echo "$CX_REC $((_b + 1)) $(cx_count)" >"$RUN/out/$TS.seq"
  echo "$_cb $(cx_conn)" >"$RUN/out/$TS.conn"
  echo "$TS" >>"$RUN/out/order.txt"
  # The pane's codex_env check and run_codex each made a package copy.
  for _p in "$RUN"/codex-pkg.*; do [ ! -e "$_p" ] || rm -rf "$_p"; done
  echo "codex config: $TS (TUI) exit $(cat "$RUN/$TS-exit"), requests $(($(cx_count) - _b))"
}

# pc5_home <scenario> [<event> <hook rules file>]: CODEX_HOME with all fourteen events logged and the
# probe MCP server.
pc5_home() {
  cxpy pc5toml "$RUN/x/$1.toml" "$1" "$PREMISES_DIR" ${2:+"$2" "$3"}
  cx_home "$1" env "$RUN/x/$1.toml"
}
# pc5_home_match <scenario>: as pc5_home, plus the MATCHERS groups on PreToolUse and PostToolUse.
# pc5_home_noapp <scenario>: as pc5_home, the probe MCP server without default_tools_approval_mode.
pc5_home_noapp() {
  cxpy pc5toml "$RUN/x/$1.toml" "$1" "$PREMISES_DIR" "" "" "" noapprove
  cx_home "$1" env "$RUN/x/$1.toml"
}
pc5_home_match() {
  cxpy pc5toml "$RUN/x/$1.toml" "$1" "$PREMISES_DIR" "" "" match
  cx_home "$1" env "$RUN/x/$1.toml"
}
# hook_rule <scenario> <event> <output json>: hook_log.py rules file, one use.
hook_rule() { printf '[{"event": "%s", "times": 1, "output": %s}]\n' "$2" "$3" >"$RUN/hk-rules/$1.json"; }
# features.apply_patch_freeform: the pinned schema's apply_patch feature. Real runs (2026-11-26):
# for gpt-mock (no model metadata) Codex offers no apply_patch with or without it; with Codex's
# own default model (gpt-6.2-sol) the request carries no tools field at all, so the probe stays
# on gpt-mock.
pc5_run() { cx_run "$1" --dangerously-bypass-hook-trust -c features.apply_patch_freeform=true; }

# ================================ phase A: no tool calls ================================
printf '[]\n' >"$RUN/rules.json"
cx_sse "$RUN/sse/pc4-tui.sse" text zp-tui-reply
cxpy rule "$RUN/rules.json" pc4-tui "$RUN/sse/pc4-tui.sse"
start_mock "$RUN/rules.json"

# PC3: CODEX_HOME against ~/.codex (the scratch HOME's planted ~/.codex, never the owner's).
mkdir -p "$H/.codex"
printf '%s\n' 'model = "zp-model-dotcodex"' 'model_provider = "mock"' '' '[model_providers.mock]' 'name = "mock"' \
  "base_url = \"$MOCK_URL/v1\"" 'env_key = "MOCK_API_KEY"' 'wire_api = "responses"' >"$H/.codex/config.toml"
printf 'model = "zp-model-codexhome"\n' >"$RUN/x/pc3-home.toml"
cx_home pc3-home env "$RUN/x/pc3-home.toml"
cx_run pc3-home

# PC3: -C/--cd.
cx_home pc3-cd env
printf 'developer_instructions = "zp-cd-cwd"\n' >"$RUN/x/pc3-cd-cwd.toml"
printf 'developer_instructions = "zp-cd-other"\n' >"$RUN/x/pc3-cd-other.toml"
cx_project "$R" "$RUN/x/pc3-cd-cwd.toml"
cx_project "$RUN/other" "$RUN/x/pc3-cd-other.toml"
cx_run pc3-cd -C "$RUN/other"

# PC3 precedence, phase A groups.
printf '[model_providers.mock]\nbase_url = "%s"\n' "$MOCK_URL/zp-url-project/v1" >"$RUN/x/pc3-url.toml"
pc3_group pc3-url env "$RUN/x/pc3-url.toml" -c "model_providers.mock.base_url=\"$MOCK_URL/zp-url-cli/v1\""

printf 'model_provider = "zpproj"\n\n[model_providers.zpproj]\nname = "zpproj"\nbase_url = "%s"\nenv_key = "MOCK_API_KEY"\nwire_api = "responses"\n' \
  "$MOCK_URL/zp-prov-project/v1" >"$RUN/x/pc3-prov.toml"
pc3_group pc3-prov env "$RUN/x/pc3-prov.toml" -c 'model_provider="zpcli"' \
  -c "model_providers.zpcli={ name = \"zpcli\", base_url = \"$MOCK_URL/zp-prov-cli/v1\", env_key = \"MOCK_API_KEY\", wire_api = \"responses\" }"

printf '[model_providers.mock]\nauth = { command = "/bin/sh", args = ["-c", "echo project >>%s/auth-who; printf %%s %s"] }\n' \
  "$RUN" "$DUMMY_KEY" >"$RUN/x/pc3-auth.toml"
pc3_group pc3-auth auth "$RUN/x/pc3-auth.toml" \
  -c "model_providers.mock.auth={ command = \"/bin/sh\", args = [\"-c\", \"echo cli >>$RUN/auth-who; printf %s $DUMMY_KEY\"] }"

cat >"$RUN/x/pc3-main.toml" <<EOF
model = "zp-model-project"
developer_instructions = "zp-dev-project"

[model_providers.mock]
http_headers = { X-Zp-Who = "project" }
supports_websockets = false

[skills]
config = [{ path = "$RUN/skills/zp-skill/SKILL.md", enabled = false }]

[mcp_servers.zygo]
command = "/bin/sh"
args = ["-c", "echo project >>$RUN/mcp-who; exec python3 -I $PREMISES_DIR/mcp_probe.py"]
EOF
pc3_group pc3-main env "$RUN/x/pc3-main.toml" -c 'model="zp-model-cli"' \
  -c 'model_providers.mock.http_headers={ X-Zp-Who = "cli" }' -c 'developer_instructions="zp-dev-cli"' \
  -c "skills.config=[{ path = \"$RUN/skills/zp-skill/SKILL.md\", enabled = true }]" \
  -c "mcp_servers.zygo={ command = \"/bin/sh\", args = [\"-c\", \"echo cli >>$RUN/mcp-who; exec python3 -I $PREMISES_DIR/mcp_probe.py\"] }" \
  -c 'model_providers.mock.supports_websockets=true'

# PC3: agent files and hooks in CODEX_HOME against the project.
cx_home pc3-agents env
mkdir -p "$CX_HOME/agents" "$R/.codex/agents"
for _w in home project; do
  _d=$CX_HOME
  [ "$_w" = home ] || _d=$R/.codex
  printf 'name = "zygo-probe"\ndescription = "zp-agent-%s"\nmodel = "zp-agent-model-%s"\ndeveloper_instructions = "zp-agent-dev-%s"\n' \
    "$_w" "$_w" "$_w" >"$_d/agents/zygo-probe.toml"
done
cx_run pc3-agents
cx_home pc3-hooks env
mkdir -p "$R/.codex"
hook_json "echo home >>$RUN/hooks-who" >"$CX_HOME/hooks.json"
hook_json "echo project >>$RUN/hooks-who" >"$R/.codex/hooks.json"
cx_run pc3-hooks --dangerously-bypass-hook-trust

# PC4: hook trust.
pc4_run pc4-none
pc4_run pc4-bypass --dangerously-bypass-hook-trust
CX_UNTRUSTED=1
pc4_run pc4-untrusted --dangerously-bypass-hook-trust
CX_UNTRUSTED=""
pc4_run pc4-trust --trust
# --trust on a project with no trust_level at all (F3a): what it writes.
CX_UNTRUSTED=none
pc4_run pc4-trust-new --trust
CX_UNTRUSTED=""
# shellcheck disable=SC2034 # EXTRA_ENV is read by lib.sh's run_codex
EXTRA_ENV="RUST_LOG=debug"
pc4_run pc4-log
# shellcheck disable=SC2034 # as above
EXTRA_ENV=""
# The hooks feature flag: none, and the deprecated name (PC-6 saw its warning).
CX_FEAT=""
pc4_run pc4-feature-default --dangerously-bypass-hook-trust
CX_FEAT=codex_hooks
pc4_run pc4-feature-legacy --dangerously-bypass-hook-trust
CX_FEAT=hooks
# pc4-tui (F3b): the four sources, no bypass flag, the interactive TUI: approve a hook prompt if one
# shows; then pc4-tui-after: exec, no flag, on the same CODEX_HOME (does the approval persist).
cx_home pc4-tui env
pc4_sources pc4-tui
# G3: groups that share one command (another group, a matcher, a timeout, another event, the project
# file): which inputs change trusted_hash.
cxpy variants pc4-tui "$CX_HOME/hooks.json" "$R/.codex/hooks.json"
rm -f "$RUN/trust-pc4-tui"
pc4_before pc4-tui
tui_hooks pc4-tui
pc4_after pc4-tui
cxpy tuilog pc4-tui "$CX_HOME"
# G3: the hash rule from the records the TUI wrote; then the negative control (a record with a wrong
# hash) and, with a rule, the positive control (a record the part computed for a command Codex never
# saw); both exec with no flag.
_rule=$(cxpy hashrule pc4-tui "$CX_HOME")
[ ! -e "$RUN/trust-pc4-tui" ] || mv "$RUN/trust-pc4-tui" "$RUN/trust-pc4-tui.tui"
pc4_before pc4-tui-after
cx_run pc4-tui-after
[ ! -e "$RUN/trust-pc4-tui" ] || mv "$RUN/trust-pc4-tui" "$RUN/trust-pc4-tui-after"
[ ! -e "$RUN/trust-pc4-tui.tui" ] || mv "$RUN/trust-pc4-tui.tui" "$RUN/trust-pc4-tui"
pc4_after pc4-tui-after pc4-tui
cx_home pc4-ctl-neg env
hook_toml "echo ctl-neg >>$RUN/trust-pc4-ctl-neg" >>"$CX_HOME/config.toml"
printf '\n[hooks.state."%s/config.toml:session_start:0:0"]\ntrusted_hash = "sha256:%s"\n' "$(n2_realpath "$CX_HOME")" \
  0000000000000000000000000000000000000000000000000000000000000000 >>"$CX_HOME/config.toml"
rm -f "$RUN/trust-pc4-ctl-neg"
pc4_before pc4-ctl-neg
cx_run pc4-ctl-neg
pc4_after pc4-ctl-neg
if [ -n "$_rule" ] && [ -s "$RUN/x/ctl-pos.state" ]; then
  cx_home pc4-ctl-pos env
  hook_toml "echo ctl-pos >>$RUN/trust-pc4-ctl-pos" >>"$CX_HOME/config.toml"
  cat "$RUN/x/ctl-pos.state" >>"$CX_HOME/config.toml"
  rm -f "$RUN/trust-pc4-ctl-pos"
  pc4_before pc4-ctl-pos
  cx_run pc4-ctl-pos
  pc4_after pc4-ctl-pos
fi
# pc4-prewritten: the source scenario's CODEX_HOME rebuilt at the same path (the record key and hash
# may name it), with the hooks.state tables Codex wrote, no flag.
_src=$(cxpy form pc4-tui pc4-tui-after pc4-trust pc4-trust-new pc4-none pc4-bypass pc4-untrusted pc4-log)
if [ -n "$_src" ]; then
  case "$_src" in pc4-untrusted) CX_UNTRUSTED=1 ;; pc4-trust-new) CX_UNTRUSTED=none ;; esac
  cx_home "$_src" env
  CX_UNTRUSTED=""
  pc4_sources "$_src"
  cat "$RUN/sum/hooks-state.toml" >>"$CX_HOME/config.toml"
  pc4_before pc4-prewritten
  [ ! -e "$RUN/trust-$_src" ] || mv "$RUN/trust-$_src" "$RUN/trust-$_src.first"
  cx_run pc4-prewritten
  [ ! -e "$RUN/trust-$_src" ] || mv "$RUN/trust-$_src" "$RUN/trust-pc4-prewritten"
  [ ! -e "$RUN/trust-$_src.first" ] || mv "$RUN/trust-$_src.first" "$RUN/trust-$_src"
  pc4_after pc4-prewritten "$_src"
  echo "$_src" >"$RUN/out/pc4-prewritten.src"
fi
HONORED=0
if [ -e "$RUN/trust-pc4-prewritten" ]; then HONORED=1; fi

# Strict config: two controls, then the planted zygo keys, one key less per refusal.
printf 'zp_unknown_key = 1\n' >"$RUN/x/strict-control.toml"
cx_home strict-control env "$RUN/x/strict-control.toml"
cx_run strict-control --strict-config
cx_home strict-control-off env "$RUN/x/strict-control.toml"
cx_run strict-control-off
_removed="" _rcv=false _n=1
while :; do
  cxpy strict-gen "$RUN/strict.toml" "$_removed" "$_rcv" "$MOCK_URL" "$PREMISES_DIR" "$DUMMY_KEY" "$HONORED"
  cx_fresh
  CX_HOME=$RUN/cxh-strict-$_n
  rm -rf "$CX_HOME"
  mkdir -p "$CX_HOME"
  cp "$RUN/strict.toml" "$CX_HOME/config.toml"
  cp "$RUN/strict.toml" "$RUN/out/strict-$_n.toml"
  cx_run "strict-$_n" --strict-config
  _v=$(cxpy strict-judge "strict-$_n" "$_removed" "$_rcv" "$HONORED")
  case "$_v" in
    "key remote_compaction")
      if [ "$_rcv" = false ]; then _rcv='"unsupported"'; else _removed="$_removed remote_compaction"; fi ;;
    "key "*)
      _ids=$(echo "${_v#key }" | tr , ' ')
      for _id in $_ids; do
        case " $_removed " in *" $_id "*) break 2 ;; esac
        _removed="$_removed $_id"
      done ;;
    *) break ;;
  esac
  _n=$((_n + 1))
  [ "$_n" -le 25 ] || break
done

A_URL=$MOCK_URL

# PC5 probe: the tools the first request offers.
pc5_home pc5-probe
pc5_run pc5-probe

# ================================ restart: phase B rules ================================
stop_mock
mkdir -p "$RUN/rec-a"
for _f in "$RUN"/rec/[0-9]*.json; do [ ! -e "$_f" ] || mv "$_f" "$RUN/rec-a/"; done
CX_REC=rec
cx_tools
printf '[]\n' >"$RUN/rules.json"
: >>"$RUN/out/skipped.txt"
_big='{"input_tokens": 50000, "output_tokens": 10, "total_tokens": 50010}'
for _s in pc3-env-cli pc3-env-proj pc3-env-both pc12-default pc12-all pc12-exclude pc12-case pc12-core pc12-none; do
  b_shell "$_s" env
done
for _s in pc3-compact-cli pc3-compact-proj pc3-compact-both pc3-compact-v2 pc3-window-cli pc3-window-proj pc3-window-both; do
  b_shell "$_s" "echo zp" "$_big"
done
b_shell pc5-shell "echo zp-shell"
b_shell pc5-deny "echo ran >>$R/zp-ran-deny"
b_shell pc5-deny-block "echo ran >>$R/zp-ran-deny2"
b_shell pc5-post-block "echo zp-orig"
b_shell pc5-match "echo zp-match"
for _x in str obj mcp; do
  b_shell "pc5-rep-shell-$_x" "echo zp-orig"
  b_mcp "pc5-rep-mcp-$_x"
done
_patch=$(printf '*** Begin Patch\n*** Add File: zp.txt\n+zp\n*** End Patch')
if b_need "$PATCH_TOOL" patch pc5-patch; then
  if [ "$PATCH_KIND" = custom ]; then
    cx_sse "$RUN/sse/pc5-patch.sse" custom "$PATCH_TOOL" call_pc5_patch "$_patch"
  else
    cx_sse "$RUN/sse/pc5-patch.sse" call "$PATCH_TOOL" call_pc5_patch \
      "$(python3 -I -c 'import json, sys; print(json.dumps({"input": sys.argv[1]}))' "$_patch")"
  fi
  cxpy rule "$RUN/rules.json" pc5-patch "$RUN/sse/pc5-patch.sse"
fi
b_mcp pc5-mcp
b_mcp pc5-mcp-noapprove
if b_need "$SPAWN_TOOL" spawn pc5-sub && b_need "$SPAWN_ARG" spawn-argument pc5-sub; then
  cx_sse "$RUN/sse/pc5-sub.sse" call "$SPAWN_TOOL" call_pc5_sub "{\"$SPAWN_ARG\": \"zp-pc5-sub-child\"}" "$SPAWN_NS"
  cxpy rule "$RUN/rules.json" pc5-sub "$RUN/sse/pc5-sub.sse"
  # F6: the parent's next request (it holds the spawn's output) gets a sleep call, so the parent
  # turn lasts until the child has stopped (wait_agent needs the child's id, unknown in advance).
  if [ -n "$SHELL_TOOL" ]; then
    _y=""
    [ "$SHELL_YIELD" != 1 ] || _y=20000
    cx_sse "$RUN/sse/pc5-sub-wait.sse" call "$SHELL_TOOL" call_pc5_sub_wait "$(cxpy shellargs "$SHELL_ARG" "$SHELL_ARRAY" "sleep 15" "$_y")"
    cxpy rule "$RUN/rules.json" pc5-sub-wait "$RUN/sse/pc5-sub-wait.sse" "" 'function_call_output[^{}]*"call_pc5_sub"'
  fi
fi
start_mock "$RUN/rules.json"

# ================================ phase B: tool calls and hook outputs ================================
# PC3 precedence, phase B groups.
if b_ok pc3-env-cli; then
  printf '[shell_environment_policy]\nset = { ZP_WHO = "project" }\n' >"$RUN/x/pc3-env.toml"
  pc3_group pc3-env env "$RUN/x/pc3-env.toml" -c 'shell_environment_policy.set={ ZP_WHO = "cli" }'
fi
if b_ok pc3-compact-cli; then
  printf 'model_auto_compact_token_limit = 100000000\ncompact_prompt = "zp-compact-project"\n\n[model_providers.mock.capabilities]\nremote_compaction = "unsupported"\n' \
    >"$RUN/x/pc3-compact.toml"
  pc3_group pc3-compact env "$RUN/x/pc3-compact.toml" -c model_auto_compact_token_limit=1000 \
    -c 'compact_prompt="zp-compact-cli"' -c 'model_providers.mock.capabilities.remote_compaction="unsupported"'
  cx_home pc3-compact-v2 env
  cx_project "$R" "$RUN/x/pc3-compact.toml"
  cx_run pc3-compact-v2 -c model_auto_compact_token_limit=1000 -c 'compact_prompt="zp-compact-cli"' \
    -c 'model_providers.mock.capabilities.remote_compaction="v2"'
  printf 'model_context_window = 100000000\ncompact_prompt = "zp-compact-window"\n\n[model_providers.mock.capabilities]\nremote_compaction = "unsupported"\n' \
    >"$RUN/x/pc3-window.toml"
  pc3_group pc3-window env "$RUN/x/pc3-window.toml" -c model_context_window=2000 \
    -c 'compact_prompt="zp-compact-window"' -c 'model_providers.mock.capabilities.remote_compaction="v2"'
fi

# PC5: payloads per tool class.
for _s in pc5-shell pc5-patch pc5-mcp pc5-sub; do
  b_ok "$_s" || continue
  pc5_home "$_s"
  pc5_run "$_s"
done
# PC5: an MCP call with no approval mode (G4): PermissionRequest and Codex's refusal text.
if b_ok pc5-mcp-noapprove; then
  pc5_home_noapp pc5-mcp-noapprove
  pc5_run pc5-mcp-noapprove
fi
# PC5: matchers (F4).
if b_ok pc5-match; then
  pc5_home_match pc5-match
  pc5_run pc5-match
fi
# PC5: output shapes.
hook_rule pc5-ctx-start SessionStart '{"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": "zp-ctx-start"}}'
hook_rule pc5-ctx-prompt UserPromptSubmit '{"hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "additionalContext": "zp-ctx-prompt"}}'
hook_rule pc5-deny PreToolUse '{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "zp-deny"}}'
hook_rule pc5-deny-block PreToolUse '{"decision": "block", "reason": "zp-deny2"}'
hook_rule pc5-stop-block Stop '{"decision": "block", "reason": "zp-block"}'
# F5: each replacement shape on the shell tool and on an MCP tool.
hook_rule pc5-rep-shell-str PostToolUse '{"hookSpecificOutput": {"hookEventName": "PostToolUse", "updatedToolOutput": "zp-replaced"}}'
hook_rule pc5-rep-shell-obj PostToolUse '{"hookSpecificOutput": {"hookEventName": "PostToolUse", "updatedToolOutput": {"content": [{"type": "text", "text": "zp-replaced"}]}}}'
hook_rule pc5-rep-shell-mcp PostToolUse '{"hookSpecificOutput": {"hookEventName": "PostToolUse", "updatedMCPToolOutput": {"content": [{"type": "text", "text": "zp-replaced"}]}}}'
for _x in str obj mcp; do cp "$RUN/hk-rules/pc5-rep-shell-$_x.json" "$RUN/hk-rules/pc5-rep-mcp-$_x.json"; done
hook_rule pc5-post-block PostToolUse '{"decision": "block", "reason": "zp-post"}'
for _p in pc5-ctx-start:SessionStart pc5-ctx-prompt:UserPromptSubmit pc5-stop-block:Stop pc5-deny:PreToolUse \
  pc5-deny-block:PreToolUse pc5-post-block:PostToolUse pc5-rep-shell-str:PostToolUse pc5-rep-shell-obj:PostToolUse \
  pc5-rep-shell-mcp:PostToolUse pc5-rep-mcp-str:PostToolUse pc5-rep-mcp-obj:PostToolUse pc5-rep-mcp-mcp:PostToolUse; do
  _s=${_p%%:*} _e=${_p#*:}
  case "$_s" in pc5-ctx-* | pc5-stop-block) ;; *) b_ok "$_s" || continue ;; esac
  pc5_home "$_s" "$_e" "$RUN/hk-rules/$_s.json"
  # G11: the replacement runs log at debug level, for Codex's reason behind "PostToolUse Failed".
  # shellcheck disable=SC2034 # EXTRA_ENV is read by lib.sh's run_codex
  case "$_s" in pc5-rep-*) EXTRA_ENV="RUST_LOG=debug" ;; *) EXTRA_ENV="" ;; esac
  pc5_run "$_s"
done
# shellcheck disable=SC2034 # as above
EXTRA_ENV=""

# PC12: shell_environment_policy.
for _s in pc12-default pc12-all pc12-exclude pc12-case pc12-core pc12-none; do
  b_ok "$_s" || continue
  case "$_s" in
    pc12-default) : >"$RUN/x/$_s.toml" ;;
    pc12-all) printf '[shell_environment_policy]\ninherit = "all"\nexclude = []\n' >"$RUN/x/$_s.toml" ;;
    pc12-exclude) printf '[shell_environment_policy]\ninherit = "all"\nexclude = ["MOCK_*", "HTTPS_PROXY"]\nset = { ZP_SET = "zp-set" }\n' >"$RUN/x/$_s.toml" ;;
    pc12-case) printf '[shell_environment_policy]\ninherit = "all"\nexclude = ["mock_api_key"]\n' >"$RUN/x/$_s.toml" ;;
    pc12-core) printf '[shell_environment_policy]\ninherit = "core"\n' >"$RUN/x/$_s.toml" ;;
    pc12-none) printf '[shell_environment_policy]\ninherit = "none"\n' >"$RUN/x/$_s.toml" ;;
  esac
  cx_home "$_s" env "$RUN/x/$_s.toml"
  cx_run "$_s"
done

stop_mock

# ================================ summaries and publish ================================
cxpy analyze "$CODEX_VERSION" "$RUN/tools.env" "$R" "$HONORED" "$A_URL" "$PREMISES_DIR"
while read -r _n _p; do cp "$_p" "$RUN/sum/hooks/$_n"; done <"$RUN/sum/hooks/.payloads"
rm -f "$RUN/sum/hooks/.payloads"
(cd "$RUN/sum" && find config trust hooks -type f) | while read -r _f; do
  scrub "$RUN/sum/$_f" >"$RUN/fx/$_f"
done
[ -z "$(find "$RUN/fx" -type l)" ] || { echo "premises: a link in the fixtures"; exit 1; }
# shellcheck disable=SC2153 # CODEX_FX is set by lib.sh
rm -rf "$CODEX_FX/config" "$CODEX_FX/trust" "$CODEX_FX/hooks"
mkdir -p "$CODEX_FX"
cp -R "$RUN/fx/." "$CODEX_FX/"
codex_record config
echo "codex config: ok (codex $CODEX_VERSION)"

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
N2_AGENTS=codex
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
import json, os, re, sys

RUN = sys.argv[1]
CMD = sys.argv[2]
A = sys.argv[3:]
q = json.dumps
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
    for p in seen:
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
    r = {"name": s, "path": r"/v1/responses", "body": "zp-%s(?![-A-Za-z0-9])" % re.escape(s), "sse": sse, "times": 1}
    if len(A) > 3 and A[3]:
        r["usage"] = json.loads(A[3])
    rules.append(r)
    wjson(rules_p, rules)


def cmd_shellargs():
    arg, arr, text = A
    print(json.dumps({arg: ["/bin/sh", "-c", text] if arr == "1" else text}))


def cmd_pc5toml():
    out, s, prem = A[:3]
    ev, rules = (A[3], A[4]) if len(A) > 4 else ("", "")
    L = []
    for e in EVENTS:
        c = "python3 -I %s/hook_log.py %s/hk/%s/%s" % (prem, RUN, s, e)
        if e == ev:
            c += " " + rules
        L += ["[[hooks.%s]]" % e, "[[hooks.%s.hooks]]" % e, 'type = "command"', "command = %s" % q(c), ""]
    L += ["[mcp_servers.probe]", 'command = "python3"', "args = %s" % q(["-I", prem + "/mcp_probe.py"]), ""]
    with open(out, "w") as f:
        f.write("\n".join(L))


def tool_names(body):
    found = []

    def walk(o):
        if isinstance(o, dict):
            if isinstance(o.get("name"), str):
                found.append(o)
            for v in o.values():
                if isinstance(v, (dict, list)):
                    walk(v)
        elif isinstance(o, list):
            for v in o:
                walk(v)
    walk(body.get("tools") or [])
    return found


def cmd_tools():
    out = A[0]
    first = None
    for p in A[1:]:
        r = json.load(open(p))
        if is_resp(r) and isinstance(r.get("body"), dict):
            first = r
            break
    v = dict(SHELL_TOOL="", SHELL_ARG="", SHELL_ARRAY="", PATCH_TOOL="", PATCH_KIND="", SPAWN_TOOL="",
             SPAWN_ARG="", MCP_TOOL="")
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
            break
    if "apply_patch" in by:
        v["PATCH_TOOL"] = "apply_patch"
        v["PATCH_KIND"] = "custom" if by["apply_patch"].get("type") == "custom" else "function"
    for n, t in by.items():
        if "spawn" in n:
            v["SPAWN_TOOL"] = n
            v["SPAWN_ARG"] = next((a for a in ("message", "prompt", "task", "input") if a in props(t)), "")
            break
    v["MCP_TOOL"] = next((n for n in by if "probe_tool" in n), "")
    v["ALL_TOOLS"] = ",".join(sorted(by))
    with open(out, "w") as f:
        for k, x in v.items():
            if not re.fullmatch(r"[A-Za-z0-9_.,:-]*", x):
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


def first_err(s):
    ls = lines(os.path.join(RUN, "out", s + ".err"))
    return ls[0] if ls else "no stderr (exit %s)" % rc(s)


def key_hit(line, keys):
    """The key a stderr line names: a whole dotted key (longest first), else a quoted alias."""
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
    else:
        errs = lines(os.path.join(RUN, "out", s + ".err"))
        hit = next(((l, key_hit(l, keys)) for l in errs if key_hit(l, keys)), None)
        if hit:
            v.update(verdict="key", key=hit[1][0], line=hit[0])
        else:
            v.update(verdict="other", line=errs[0] if errs else "no request recorded (exit %s)" % rc(s))
    with open(os.path.join(RUN, "sum", "strict.jsonl"), "a") as f:
        f.write(json.dumps(v) + "\n")
    print(v["verdict"] + (" " + v["key"] if v["verdict"] == "key" else ""))


def control_line(s):
    if rc(s) == "0" and resp(s):
        return "accepted"
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
        refused = [r for r in runs if r["verdict"] == "key" and r["key"] == k[0]]
        if k[0] == "remote_compaction":
            first = [r for r in refused if r["remote_compaction"] == "false"]
            second = [r for r in runs if r["remote_compaction"] != "false" and "remote_compaction" not in r["removed"]]
            if second:
                sr = [r for r in second if r["verdict"] == "key" and r["key"] == "remote_compaction"]
                unsupported = "refused: " + sr[0]["line"] if sr else (
                    "accepted" if second[-1]["verdict"] == "accepted" else "undecided (config: refused: %s)" % second[-1]["line"])
            refused = first
        val = k[4] if k[0] != "remote_compaction" else "false"
        if refused:
            verdict = "refused: " + refused[0]["line"]
        elif last and last["verdict"] == "accepted":
            verdict = "accepted"
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
        if not recs(s):
            return "none"
        return "cli" if any(r.get("method") == "GET" and "websocket" in (r.get("headers") or {}).get("upgrade", "").lower()
                            for r in recs(s)) else "project"

    def env(s):
        rs = resp(s)
        t = tool_outputs(rs[1]) if len(rs) > 1 else ""
        return "cli" if "ZP_WHO=cli" in t else "project" if "ZP_WHO=project" in t else "none"

    def threshold(s):
        if not resp(s):
            return "none"
        return "cli" if compaction(s)[0] else "project"

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
        ("pc3-main", "skills.config"): marker("zp-skill-desc", None, "project"),
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
        elif not e["project_only"]:
            e["verdict"] = "project-ignored"
        else:
            e["verdict"] = "none"
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


PC4 = ["pc4-none", "pc4-bypass", "pc4-untrusted", "pc4-trust", "pc4-log", "pc4-feature-default", "pc4-feature-legacy"]


def pc4():
    scen, where = {}, []
    for s in PC4 + ["pc4-prewritten"]:
        if not ran(s):
            scen[s] = {"ran": False, "why": skipped().get(s) or "not-run: no record form"}
            continue
        _, keys = state_tables(rd(os.path.join(RUN, "out", s + ".config.toml")))
        scen[s] = {"ran": True, "exit": rc(s), "request": bool(resp(s)),
                   "sources_run": sorted(set(lines(os.path.join(RUN, "trust-" + s)))), "hooks_state_keys": keys}
        if keys:
            where.append("%s: CODEX_HOME/config.toml [hooks.state.\"<key>\"]" % s)
    pre = scen["pc4-prewritten"]
    if not pre["ran"]:
        honored = "not-run"
    else:
        src = rd(os.path.join(RUN, "out", "pc4-prewritten.src")).strip()
        pre["form_from"] = src
        honored = bool(set(pre["sources_run"]) & {"home-inline", "home-json", "project-inline", "project-json"})
    bypass = sorted(set(scen["pc4-bypass"].get("sources_run", [])) - set(scen["pc4-none"].get("sources_run", [])))
    if honored is True:
        r, w = "verified", "hook trust records written before launch are honored"
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
        "prewritten_honored": honored, "result": r, "why": w})
    out = []
    for s in PC4 + ["pc4-prewritten"]:
        if not ran(s):
            continue
        out.append("== %s files" % s)
        out += lines(os.path.join(RUN, "out", s + ".files"))
        out.append("== %s config.toml hooks tables" % s)
        keep = False
        for l in rd(os.path.join(RUN, "out", s + ".config.toml")).splitlines():
            if re.match(r"\s*\[", l):
                keep = bool(re.match(r"\s*\[\[?\s*hooks", l))
            if keep:
                out.append(l)
    with open(os.path.join(RUN, "sum", "trust", "codex-home-after.txt"), "w") as f:
        f.write("\n".join(out or ["no PC4 run"]) + "\n")
    logs = []
    src = [os.path.join(RUN, "out", "pc4-log.err")]
    ld = os.path.join(RUN, "out", "pc4-log.log")
    for d, _, fs in os.walk(ld):
        src += [os.path.join(d, f) for f in sorted(fs)]
    for p in src:
        for l in rd(p).splitlines():
            if re.search("hook", l, re.I) and re.search("trust|hash", l, re.I):
                logs.append(l)
    with open(os.path.join(RUN, "sum", "trust", "log-lines.txt"), "w") as f:
        f.write("\n".join(logs[:200] or ["none: no pc4-log line holds hook and trust or hash"]) + "\n")


PC5_CLASS = {"pc5-shell": "shell", "pc5-patch": "patch", "pc5-mcp": "mcp", "pc5-sub": "spawn"}
PC5_ORDER = ["pc5-probe", "pc5-shell", "pc5-patch", "pc5-mcp", "pc5-sub", "pc5-ctx-start", "pc5-ctx-prompt",
             "pc5-deny", "pc5-deny-block", "pc5-stop-block", "pc5-post-replace", "pc5-post-block"]
SHAPES = {
    "pc5-ctx-start": ("context", "SessionStart"), "pc5-ctx-prompt": ("context", "UserPromptSubmit"),
    "pc5-deny": ("deny", "PreToolUse"), "pc5-deny-block": ("deny", "PreToolUse"),
    "pc5-stop-block": ("block", "Stop"), "pc5-post-replace": ("replacement", "PostToolUse"),
    "pc5-post-block": ("block", "PostToolUse")}


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
            for s, c in PC5_CLASS.items():
                ps = payloads(s, e)
                if ps:
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
    for s, c in PC5_CLASS.items():
        names = set()
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
             "mcp": nf(env.get("MCP_TOOL"), "mcp"), "all_tools": [t for t in env.get("ALL_TOOLS", "").split(",") if t],
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
        elif s == "pc5-post-replace":
            t = tool_outputs(rs[1]) if len(rs) > 1 else ""
            o.update(accepted="zp-replaced" in t and "zp-orig" not in t,
                     evidence="request 2 tool output holds zp-replaced: %s, zp-orig: %s" % ("zp-replaced" in t, "zp-orig" in t))
        else:
            hit = len(rs) > 1 and "zp-post" in btext(rs[1])
            o.update(accepted=hit, evidence="zp-post in request 2: %s" % hit)
        outs[s] = o
        kinds[kind] = kinds.get(kind, False) or o["accepted"]
    need = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "SubagentStart", "SubagentStop"]
    missing = [e for e in need if not isinstance(events[e], list)] + \
              ["%s shape" % k for k in ("context", "deny", "block", "replacement") if not kinds.get(k)]
    wjson(os.path.join(RUN, "sum", "hooks", "outputs.json"), {
        "events": events, "shapes": outs, "result": "failed" if missing else "verified",
        "why": "missing: " + ", ".join(missing) if missing else "every needed event has a payload and each kind an accepted shape"})


PC12 = ["pc12-default", "pc12-exclude", "pc12-case", "pc12-core", "pc12-none"]


def pc12():
    out = {}
    for s in PC12:
        rs = resp(s)
        t = tool_outputs(rs[1]) if len(rs) > 1 else None
        if t is None:
            out[s] = {"names": None, "why": why_not(s) if not rs else "no tool output in request 2"}
            continue
        out[s] = {"names": sorted(set(re.findall(r"(?m)^([A-Za-z_][A-Za-z0-9_]*)=", t)))}
        if s == "pc12-exclude":
            out[s]["zp_set"] = "ZP_SET=zp-set" in t
    ex, de = out["pc12-exclude"].get("names"), out["pc12-default"].get("names")
    ok = ex is not None and de is not None and "MOCK_API_KEY" not in ex and "HTTPS_PROXY" not in ex \
        and out["pc12-exclude"].get("zp_set") and "MOCK_API_KEY" in de
    out["result"] = "verified" if ok else "failed"
    out["why"] = "exclude and set apply in tool shells; the default passes MOCK_API_KEY" if ok else \
        "pc12-exclude or pc12-default did not show the expected names" if ex is not None and de is not None else \
        "no tool output: " + (out["pc12-exclude"].get("why") or out["pc12-default"].get("why") or "")
    wjson(os.path.join(RUN, "sum", "config", "shell-env.json"), out)


def cmd_analyze():
    version, tools_env, r_dir, with_state, url, prem = A[:6]
    pc3()
    pc4()
    pc5(tools_env, r_dir)
    pc12()
    strict_report(version, with_state == "1", url, prem)


{"home": cmd_home, "sse": cmd_sse, "rule": cmd_rule, "shellargs": cmd_shellargs, "pc5toml": cmd_pc5toml,
 "tools": cmd_tools, "form": cmd_form, "strict-gen": cmd_strict_gen, "strict-judge": cmd_strict_judge,
 "analyze": cmd_analyze}[CMD]()
PY
cxpy() { python3 -I "$RUN/cx.py" "$RUN" "$@"; }

# CX_FEAT: the [features] flag cx_home sets to true (empty: none; pc4-feature-* measure the flag).
CX_FEAT=hooks
# cx_fresh: no project .codex folder of an earlier scenario is left in R or $RUN/other.
cx_fresh() { rm -rf "$R/.codex" "$RUN/other/.codex"; }
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
cx_count() { ls "$RUN/rec" | grep -c '^[0-9][0-9]*\.json$' || true; }
# cx_run <scenario> <codex exec args...>: one codex exec with prompt zp-<scenario>; out/<s>.out,
# .err, .rc, .seq (<record folder> <first> <last>); the side files a run writes (auth-who, mcp-who,
# hooks-who) move to out/<s>.<name>; project folders are kept in proj/<s>/; the package copy is removed.
CX_REC=rec-a
cx_run() {
  _s=$1
  shift
  _b=$(cx_count)
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
  case "$CODEX_EXE" in "$RUN"/codex-pkg.*/p/*) rm -rf "${CODEX_EXE%/p/*}" ;; esac
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
# cx_sse <file> text <text> | call <name> <call_id> <arguments json> | custom <name> <call_id> <input>
cx_sse() { _o=$1 _k=$2; shift 2; cxpy sse "$_o" "$_k" "$@"; }
# cx_tools: tools.env from pc5-probe's first request (SHELL_TOOL, SHELL_ARG, SHELL_ARRAY, PATCH_TOOL,
# PATCH_KIND, SPAWN_TOOL, SPAWN_ARG, MCP_TOOL; empty when not offered).
cx_tools() {
  # shellcheck disable=SC2046
  cxpy tools "$RUN/tools.env" $(cx_reqs pc5-probe)
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
# pc4_after <scenario>: keep the CODEX_HOME config.toml, its file list and its log folder.
pc4_after() {
  cp "$CX_HOME/config.toml" "$RUN/out/$1.config.toml"
  (cd "$CX_HOME" && find . -type f | sed 's|^\./||' | LC_ALL=C sort) >"$RUN/out/$1.files"
  [ ! -d "$CX_HOME/log" ] || cp -R "$CX_HOME/log" "$RUN/out/$1.log"
}
pc4_run() {
  _s4=$1
  shift
  cx_home "$_s4" env
  pc4_sources "$_s4"
  rm -f "$RUN/trust-$_s4"
  cx_run "$_s4" "$@"
  pc4_after "$_s4"
}
# pc5_home <scenario> [<event> <hook rules file>]: CODEX_HOME with all fourteen events logged and the
# probe MCP server.
pc5_home() {
  cxpy pc5toml "$RUN/x/$1.toml" "$1" "$PREMISES_DIR" ${2:+"$2" "$3"}
  cx_home "$1" env "$RUN/x/$1.toml"
}
# hook_rule <scenario> <event> <output json>: hook_log.py rules file, one use.
hook_rule() { printf '[{"event": "%s", "times": 1, "output": %s}]\n' "$2" "$3" >"$RUN/hk-rules/$1.json"; }
pc5_run() { cx_run "$1" --dangerously-bypass-hook-trust; }

# ================================ phase A: no tool calls ================================
printf '[]\n' >"$RUN/rules.json"
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
EXTRA_ENV="RUST_LOG=debug"
pc4_run pc4-log
EXTRA_ENV=""
# The hooks feature flag: none, and the deprecated name (PC-6 saw its warning).
CX_FEAT=""
pc4_run pc4-feature-default --dangerously-bypass-hook-trust
CX_FEAT=codex_hooks
pc4_run pc4-feature-legacy --dangerously-bypass-hook-trust
CX_FEAT=hooks
# pc4-prewritten: the source scenario's CODEX_HOME rebuilt at the same path (the record key and hash
# may name it), with the hooks.state tables Codex wrote, no flag.
_src=$(cxpy form pc4-trust pc4-none pc4-bypass pc4-untrusted pc4-log)
if [ -n "$_src" ]; then
  [ "$_src" != pc4-untrusted ] || CX_UNTRUSTED=1
  cx_home "$_src" env
  CX_UNTRUSTED=""
  pc4_sources "$_src"
  cat "$RUN/sum/hooks-state.toml" >>"$CX_HOME/config.toml"
  [ ! -e "$RUN/trust-$_src" ] || mv "$RUN/trust-$_src" "$RUN/trust-$_src.first"
  cx_run pc4-prewritten
  [ ! -e "$RUN/trust-$_src" ] || mv "$RUN/trust-$_src" "$RUN/trust-pc4-prewritten"
  [ ! -e "$RUN/trust-$_src.first" ] || mv "$RUN/trust-$_src.first" "$RUN/trust-$_src"
  pc4_after pc4-prewritten
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
      case " $_removed " in *" ${_v#key } "*) break ;; esac
      _removed="$_removed ${_v#key }" ;;
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
for _s in pc3-env-cli pc3-env-proj pc3-env-both pc12-default pc12-exclude pc12-case pc12-core pc12-none; do
  b_shell "$_s" env
done
for _s in pc3-compact-cli pc3-compact-proj pc3-compact-both pc3-compact-v2 pc3-window-cli pc3-window-proj pc3-window-both; do
  b_shell "$_s" "echo zp" "$_big"
done
b_shell pc5-shell "echo zp-shell"
b_shell pc5-deny "echo ran >>$R/zp-ran-deny"
b_shell pc5-deny-block "echo ran >>$R/zp-ran-deny2"
b_shell pc5-post-replace "echo zp-orig"
b_shell pc5-post-block "echo zp-orig"
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
if b_need "$MCP_TOOL" mcp pc5-mcp; then
  cx_sse "$RUN/sse/pc5-mcp.sse" call "$MCP_TOOL" call_pc5_mcp '{}'
  cxpy rule "$RUN/rules.json" pc5-mcp "$RUN/sse/pc5-mcp.sse"
fi
if b_need "$SPAWN_TOOL" spawn pc5-sub && b_need "$SPAWN_ARG" spawn-argument pc5-sub; then
  cx_sse "$RUN/sse/pc5-sub.sse" call "$SPAWN_TOOL" call_pc5_sub "{\"$SPAWN_ARG\": \"zp-pc5-sub-child\"}"
  cxpy rule "$RUN/rules.json" pc5-sub "$RUN/sse/pc5-sub.sse"
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
# PC5: output shapes.
hook_rule pc5-ctx-start SessionStart '{"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": "zp-ctx-start"}}'
hook_rule pc5-ctx-prompt UserPromptSubmit '{"hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "additionalContext": "zp-ctx-prompt"}}'
hook_rule pc5-deny PreToolUse '{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "zp-deny"}}'
hook_rule pc5-deny-block PreToolUse '{"decision": "block", "reason": "zp-deny2"}'
hook_rule pc5-stop-block Stop '{"decision": "block", "reason": "zp-block"}'
hook_rule pc5-post-replace PostToolUse '{"hookSpecificOutput": {"hookEventName": "PostToolUse", "updatedToolOutput": "zp-replaced"}}'
hook_rule pc5-post-block PostToolUse '{"decision": "block", "reason": "zp-post"}'
for _p in pc5-ctx-start:SessionStart pc5-ctx-prompt:UserPromptSubmit pc5-stop-block:Stop pc5-deny:PreToolUse \
  pc5-deny-block:PreToolUse pc5-post-replace:PostToolUse pc5-post-block:PostToolUse; do
  _s=${_p%%:*} _e=${_p#*:}
  case "$_s" in pc5-ctx-* | pc5-stop-block) ;; *) b_ok "$_s" || continue ;; esac
  pc5_home "$_s" "$_e" "$RUN/hk-rules/$_s.json"
  pc5_run "$_s"
done

# PC12: shell_environment_policy.
for _s in pc12-default pc12-exclude pc12-case pc12-core pc12-none; do
  b_ok "$_s" || continue
  case "$_s" in
    pc12-default) : >"$RUN/x/$_s.toml" ;;
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
rm -rf "$CODEX_FX/config" "$CODEX_FX/trust" "$CODEX_FX/hooks"
mkdir -p "$CODEX_FX"
cp -R "$RUN/fx/." "$CODEX_FX/"
codex_record config
echo "codex config: ok (codex $CODEX_VERSION)"

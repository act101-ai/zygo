#!/usr/bin/env python3
"""Analyses of live-workspace.sh (node P2) over saved hook inputs and recorded requests.

workspace_analyze.py pw2-sub <hook dir>
    SessionStart inputs between the first dispatched SubagentStart and the SubagentStop of the same
    agent_id (sub-check 2.3). JSON: {"inside", "without_agent_id", "sources", "first"}.
workspace_analyze.py pw7 <hook dir>
    Turn ends of the main loop and SubagentStart/SubagentStop pairs (sub-checks 7.3-7.5). JSON:
    {"turns", "open", "open_prompts", "disp", "disp_bad", "disp_kinds", "internal", "internal_bad",
     "internal_kinds", "internal_compaction", "last_start", "last_stop"}.
workspace_analyze.py lead-next <record dir> <tool id>
    The first recorded request that holds the tool_result of <tool id> and is not Claude Code's
    prompt-suggestion request (its last user message starts with "[SUGGESTION MODE:"). Prints its
    path, or nothing.
workspace_analyze.py tool-result <request file> <tool id> <reason>
    What the tool_result of <tool id> in that request says, without its text. JSON:
    {"tool_use_id", "is_error", "holds_reason", "reason_only"}.

A missing or empty folder gives zero counts, so every pass test of the script fails on it.
"""
import json
import os
import sys

SUGGESTION = "[SUGGESTION MODE:"


def hooks(d):
    names = sorted(f for f in os.listdir(d) if f[:1].isdigit() and f.endswith(".json")) if os.path.isdir(d) else []
    out = []
    for f in names:
        try:
            out.append((f, json.load(open(os.path.join(d, f)))))
        except (OSError, ValueError):
            continue
    return out


def pw2_sub(d):
    ev = hooks(d)
    lo = next((i for i, (_, e) in enumerate(ev) if e.get("hook_event_name") == "SubagentStart" and e.get("agent_type")), None)
    hi = None
    if lo is not None:
        aid = ev[lo][1].get("agent_id")
        hi = next((i for i, (_, e) in enumerate(ev) if i > lo and e.get("hook_event_name") == "SubagentStop" and e.get("agent_id") == aid), None)
    if hi is None:
        return {"inside": 0, "without_agent_id": 0, "sources": "none", "first": "none"}
    ins = [i for i in range(lo, hi) if ev[i][1].get("hook_event_name") == "SessionStart"]
    bad = [i for i in ins if "agent_id" not in ev[i][1]]
    src = ",".join(sorted({str(ev[i][1].get("source")) for i in ins})) or "none"
    return {"inside": len(ins), "without_agent_id": len(bad), "sources": src, "first": ev[ins[0]][0] if ins else "none"}


def pw7(d):
    ev = hooks(d)
    main = [e for _, e in ev if "agent_id" not in e and e.get("hook_event_name") in ("UserPromptSubmit", "Stop", "StopFailure")]
    turns, open_prompts = 0, []
    for i, e in enumerate(main):
        if e.get("hook_event_name") == "UserPromptSubmit":
            turns += 1
            nxt = main[i + 1].get("hook_event_name") if i + 1 < len(main) else None
            if nxt not in ("Stop", "StopFailure"):
                open_prompts.append(str(e.get("prompt", "?")))
    start, stop, typ, compaction = {}, {}, {}, set()
    in_compact = False
    for f, e in ev:
        n, a = e.get("hook_event_name"), e.get("agent_id")
        if n == "PreCompact":
            in_compact = True
        elif n == "PostCompact":
            in_compact = False
        if n in ("SubagentStart", "SubagentStop"):
            (start if n == "SubagentStart" else stop).setdefault(a, f)
            typ[a] = e.get("agent_type") or typ.get(a, "")
            if n == "SubagentStop" and in_compact:
                compaction.add(a)
    ids = sorted(set(start) | set(stop), key=lambda a: start.get(a) or stop.get(a))
    disp = [a for a in ids if typ.get(a)]
    internal = [a for a in ids if not typ.get(a)]

    def bad(group):
        return [a for a in group if a not in start or a not in stop]

    def kinds(group):
        return ",".join(sorted({"start-only" if a in start else "stop-only" for a in bad(group)})) or "none"

    last = disp[-1] if disp else None
    return {"turns": turns, "open": len(open_prompts), "open_prompts": open_prompts,
            "disp": len(disp), "disp_bad": len(bad(disp)), "disp_kinds": kinds(disp),
            "internal": len(internal), "internal_bad": len(bad(internal)), "internal_kinds": kinds(internal),
            "internal_compaction": len([a for a in internal if a in compaction]),
            "last_start": start.get(last) or "none", "last_stop": stop.get(last) or "none"}


def results_of(body, tool_id):
    out = []
    for m in body.get("messages") or []:
        c = m.get("content")
        if m.get("role") == "user" and isinstance(c, list):
            out += [b for b in c if isinstance(b, dict) and b.get("type") == "tool_result" and b.get("tool_use_id") == tool_id]
    return out


def last_user_text(body):
    for m in reversed(body.get("messages") or []):
        if m.get("role") != "user":
            continue
        c = m.get("content")
        if isinstance(c, str):
            return c
        if isinstance(c, list):
            return " ".join(b.get("text", "") for b in c if isinstance(b, dict) and b.get("type") == "text")
        return ""
    return ""


def body_of(path):
    try:
        b = json.load(open(path)).get("body")
    except (OSError, ValueError):
        return None
    if isinstance(b, str):
        try:
            b = json.loads(b)
        except ValueError:
            return None
    return b if isinstance(b, dict) else None


def lead_next(d, tool_id):
    names = sorted(f for f in os.listdir(d) if f[:1].isdigit() and f.endswith(".json")) if os.path.isdir(d) else []
    for f in names:
        p = os.path.join(d, f)
        b = body_of(p)
        if b and results_of(b, tool_id) and not last_user_text(b).lstrip().startswith(SUGGESTION):
            return p
    return ""


def tool_result(path, tool_id, reason):
    b = body_of(path) or {}
    r = results_of(b, tool_id)
    if not r:
        return {"tool_use_id": None, "is_error": None, "holds_reason": False, "reason_only": False}
    c = r[0].get("content")
    text = c if isinstance(c, str) else " ".join(x.get("text", "") for x in c or [] if isinstance(x, dict))
    return {"tool_use_id": tool_id, "is_error": r[0].get("is_error", False),
            "holds_reason": reason in text, "reason_only": text.strip() == reason}


def main(a):
    if a[:1] == ["pw2-sub"] and len(a) == 2:
        print(json.dumps(pw2_sub(a[1])))
    elif a[:1] == ["pw7"] and len(a) == 2:
        print(json.dumps(pw7(a[1])))
    elif a[:1] == ["lead-next"] and len(a) == 3:
        print(lead_next(a[1], a[2]))
    elif a[:1] == ["tool-result"] and len(a) == 4:
        print(json.dumps(tool_result(a[1], a[2], a[3]), indent=2))
    else:
        sys.exit("usage: workspace_analyze.py pw2-sub <dir> | pw7 <dir> | lead-next <dir> <tool id> | tool-result <file> <tool id> <reason>")


if __name__ == "__main__":
    main(sys.argv[1:])

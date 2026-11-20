#!/usr/bin/env python3
"""summarize.py <rec dir>: one JSON line per recorded request (for evidence summaries).

Fields: seq, path, reply rule, x-claude-code-* headers, model, message count, the last
non-system message's first 160 characters, and selected body fields."""
import json
import os
import re
import sys

d = sys.argv[1]
# Replace the run's paths before the 160-character cut, so that scrub.py never sees a cut path.
run = os.path.realpath(os.path.dirname(os.path.abspath(d)))
PAIRS = []
for sub, repl in (("repo", "/repo"), ("home", "/home/user"), ("", "/tmp/run")):
    src = os.path.join(run, sub) if sub else run
    for f in (src, src.replace("/private/tmp/", "/tmp/", 1)):
        PAIRS.append((f, repl))
        PAIRS.append((re.sub(r"[^A-Za-z0-9]", "-", f), "-" + repl.strip("/").replace("/", "-")))


def cut(s):
    for a, b in PAIRS:
        s = s.replace(a, b)
    return s[:160]


out = []
for f in sorted(os.listdir(d)):
    if not (f.endswith(".json") and f[0].isdigit()):
        continue
    r = json.load(open(os.path.join(d, f)))
    b = r.get("body") if isinstance(r.get("body"), dict) else {}
    msgs = [m for m in b.get("messages") or [] if m.get("role") != "system"]
    last = cut(json.dumps(msgs[-1])) if msgs else None
    out.append({
        "seq": r["seq"], "path": r["path"], "reply": r["reply"],
        "hint_headers": {k: v for k, v in sorted(r["headers"].items()) if k.startswith("x-claude-code-") and k not in ("x-claude-code-session-id", "x-claude-code-prompt-id")},
        "model": b.get("model"), "messages": len(b.get("messages") or []),
        "thinking": b.get("thinking"), "output_config": b.get("output_config"),
        "context_management": b.get("context_management"), "max_tokens": b.get("max_tokens"),
        "tools": len(b.get("tools") or []),
        "last_message": last,
    })
print(json.dumps(out, indent=1, ensure_ascii=False))

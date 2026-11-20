#!/usr/bin/env python3
"""steps.py <sse dir> <prefix> <rules out> <steps.json> [<extra rules.json>]

Turns a scripted tool sequence into SSE files and mock rules. steps.json:
  {"trigger": "<regex on the first request's last message>",
   "header": {...}, "no_header": [...],      # request selector for every step
   "extra": {...},                            # more rule keys for every step (e.g. "body")
   "steps": [{"tool": "Bash", "input": {...}, "usage": {...}?, "iterations": bool?}, ...,
             {"text": "done", "usage": {...}?}]}
Step k (k > 0) is the reply to the request whose last message holds the tool_use id of step k-1.
Writes <sse dir>/<prefix>-<k>.sse; appends rules to <rules out> (a JSON list).
"""
import json
import os
import subprocess
import sys

sse_dir, prefix, rules_out, steps_file = sys.argv[1:5]
spec = json.load(open(steps_file))
here = os.path.dirname(os.path.abspath(__file__))
rules = json.load(open(rules_out)) if os.path.exists(rules_out) else []
prev = spec["trigger"]
for k, st in enumerate(spec["steps"]):
    path = os.path.join(sse_dir, "%s-%02d.sse" % (prefix, k))
    args = [sys.executable, "-I", os.path.join(here, "sse.py")]
    if "tool" in st:
        tid = "toolu_%s_%02d_%s" % (prefix.replace("-", "_"), k, st["tool"].lower())
        args += ["tool", st["tool"], tid, json.dumps(st["input"])]
    else:
        tid = None
        args += ["text", st["text"]]
    if "usage" in st:
        args += ["--usage", json.dumps(st["usage"])]
    if st.get("iterations"):
        args.append("--iterations")
    with open(path, "w") as f:
        subprocess.check_call(args, stdout=f)
    rule = {"name": "%s-%02d" % (prefix, k), "sse": path, "last": prev, "times": 1,
            "no_header": ["x-claude-code-compaction"] + spec.get("no_header", [])}
    if spec.get("header"):
        rule["header"] = spec["header"]
    rule.update(spec.get("extra", {}))
    rules.append(rule)
    prev = tid
json.dump(rules, open(rules_out, "w"), indent=1)

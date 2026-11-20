#!/usr/bin/env python3
"""Hook command for the N2 experiments: hook_log.py <log dir> [<responses.json>]

Saves the hook's stdin as <log dir>/<nnnn>-<event>[-<tool>]-<main|sub>.json and, when a
response rule matches (event, optional tool regex, optional "main"/"sub", "times"),
prints its JSON "output" on stdout. A rule with "rewrite": [[[key, ...], value], ...]
prints the event's tool_response with those fields replaced, as updatedToolOutput. Rule uses are counted in <log dir>/.uses.json.
Exit 0 always.
"""
import fcntl
import json
import os
import re
import sys

d = sys.argv[1]
os.makedirs(d, exist_ok=True)
raw = sys.stdin.read()
try:
    inp = json.loads(raw)
except ValueError:
    inp = {"_raw": raw}
event = inp.get("hook_event_name", "Unknown")
tool = inp.get("tool_name")
who = "sub" if "agent_id" in inp else "main"
lock = open(os.path.join(d, ".lock"), "w")
fcntl.flock(lock, fcntl.LOCK_EX)
n = len([f for f in os.listdir(d) if f.endswith(".json") and f[0].isdigit()]) + 1
name = "%04d-%s%s-%s.json" % (n, event, "-" + tool if tool else "", who)
with open(os.path.join(d, name), "w") as f:
    json.dump(inp, f, indent=2)
if len(sys.argv) > 2:
    rules = json.load(open(sys.argv[2]))
    up = os.path.join(d, ".uses.json")
    uses = json.load(open(up)) if os.path.exists(up) else {}
    for i, r in enumerate(rules):
        if r["event"] != event:
            continue
        if "tool" in r and not re.search(r["tool"], tool or ""):
            continue
        if "who" in r and r["who"] != who:
            continue
        if "times" in r and uses.get(str(i), 0) >= r["times"]:
            continue
        uses[str(i)] = uses.get(str(i), 0) + 1
        json.dump(uses, open(up, "w"))
        if "rewrite" in r:
            # Replace fields of the real tool_response: [[path...], value] pairs.
            resp = json.loads(json.dumps(inp.get("tool_response")))
            for path, value in r["rewrite"]:
                o = resp
                for k in path[:-1]:
                    o = o[k]
                o[path[-1]] = value
            r = dict(r, output={"hookSpecificOutput": {"hookEventName": "PostToolUse", "updatedToolOutput": resp}})
        out = json.dumps(r["output"])
        with open(os.path.join(d, name[:-5] + ".out"), "w") as f:
            f.write(out)
        print(out)
        break

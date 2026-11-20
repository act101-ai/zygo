#!/bin/sh
# Q5: every x-claude-code-* header with its values per request class, from the captures of the
# other Claude Code runs (run after run-P1.sh, run-P1-interactive.sh, run-P3.sh, run-Q1.sh,
# run-Q4.sh, run-Q6.sh, run-Q7-claude.sh with the same N2_SCRATCH).
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/claude-code"
RUN="$N2_SCRATCH"
python3 -I - "$N2_SCRATCH" >"$N2_SCRATCH/q5.json" <<'PY'
import json, os, re, sys
root = sys.argv[1]
runs = sorted(d for d in os.listdir(root) if re.match(r"^(P1-|P3$|Q1$|Q4$|Q6$|Q7$)", d))
ID = re.compile(r"^[0-9a-f-]{8,}$")
out = {}
n = 0
for run in runs:
    for sub in ("rec", "rec-a1"):
        d = os.path.join(root, run, sub)
        if not os.path.isdir(d) or (sub == "rec-a1"):
            continue
        for f in sorted(os.listdir(d)):
            if not (f[0].isdigit() and f.endswith(".json")):
                continue
            r = json.load(open(os.path.join(d, f)))
            if r["method"] != "POST" or "/v1/messages" not in r["path"] or "count_tokens" in r["path"]:
                continue
            n += 1
            h = r["headers"]
            cls = h.get("x-claude-code-request-class", "(none)")
            c = out.setdefault(cls, {"requests": 0, "headers": {}})
            c["requests"] += 1
            for k, v in h.items():
                if not k.startswith("x-claude-code-") and k != "x-claude-1p-request-class":
                    continue
                e = c["headers"].setdefault(k, {"present_in": 0, "values": set()})
                e["present_in"] += 1
                if ID.match(v) or k in ("x-claude-code-session-id", "x-claude-code-prompt-id"):
                    v = "<id>"
                elif k == "x-claude-code-prev-tool-durations":
                    v = re.sub(r"\d+", "<ms>", v)
                e["values"].add(v)
for c in out.values():
    for e in c["headers"].values():
        e["values"] = sorted(e["values"])
print(json.dumps({"runs": runs, "requests": n, "by_request_class": out}, indent=1))
PY
scrub "$N2_SCRATCH/q5.json" >"$FX/requests/q5-hint-headers.json"
cat "$FX/requests/q5-hint-headers.json"

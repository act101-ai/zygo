#!/bin/sh
# Owner-run live checks #21 and P1 (node N38, spec §20 #21, §19 P1). Needs the Anthropic API key
# in the environment (entered at a hidden prompt, docs/live-checks.md); it stays in the
# environment and is in no argv and no file.
# 21 + P1 end_turn: with soft_context_window = 40000, the turn that crosses it gets the context
#   signal and Claude Code compacts after it (auto).
# P1 tool_use: the zygo compact tool, called by the model, compacts and the loop goes on.
# Spend bound: --max-turns 4 per run (each message needs one or two turns) and 600 s of wall
# clock per run (LIVE_TIMEOUT, live.sh).
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
need_env ANTHROPIC_API_KEY
LIVE_KEYS=ANTHROPIC_API_KEY
LIVE_TIMEOUT=600
live_traps
pinned_agents
FX="$REPO_DIR/fixtures/claude-code/live"
mkdir -p "$FX"

window_run() { # <name>
  live_run "$1"
  cat >"$RUN/zygo.toml" <<T
[[model]]
id = "live-haiku"
soft_context_window = 40000

  [model.facts]
  reasoning = "switchable"
  reasoning_default_on = true
  effort_levels = ["low", "medium", "high", "xhigh", "max"]
  max_output_tokens = 128000
  context_window = 1000000
  sampling = false
  vision = true
  structured_output = "native"
  forced_tool_choice = true
  verified_on = 2026-11-18
  source = "https://platform.claude.com/docs/en/models/haiku-5-5/overview"

  [[model.provider]]
  name = "anthropic"
  base_url = "https://api.anthropic.com"
  dialects = ["anthropic"]
  upstream_model = "claude-haiku-5-5"
  key = "env:ANTHROPIC_API_KEY"
  enabled = true

T
  for r in lead design plan triage review code read; do
    printf '[role.%s]\nmodel = "live-haiku"\n\n' "$r" >>"$RUN/zygo.toml"
  done
  run_zygo config check >/dev/null || { echo "live: zygo config check failed"; exit 1; }
  hook_settings "$RUN/hooks" | jq '. + {permissions: {allow: ["mcp__zygo__compact"], defaultMode: "default"}}' >"$H/.claude/settings.json"
}

# 21 + P1 end_turn
window_run w21
python3 -I - "$RUN/msgs" <<'PY'
import random, sys
r = random.Random(21)
words = "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar".split()
text = []
n = 0
while n < 120000:
    w = r.choice(words)
    text.append(w)
    n += len(w) + 1
with open(sys.argv[1], "w") as f:
    f.write("tracer-21: here is text to keep; reply ok. " + " ".join(text)[:120000] + "\n")
    f.write("Say done.\n")
PY
live_exec python3 -I "$PREMISES_DIR/drive_stream.py" "$RUN/out.jsonl" "$RUN/msgs" -- \
  "$ZYGO_BIN" claude -- -p --max-turns 4 --input-format stream-json --output-format stream-json --verbose >/dev/null 2>"$RUN/err.txt" || true
LIVE_VER=$(usage_version); LIVE_VER=${LIVE_VER:-$PIN_CC}
usage_rows "$FX/21-usage.json"
v=$(python3 -I - "$RUN/usage.json" "$RUN/hooks" <<'PY'
import json, os, sys
rows = json.load(open(sys.argv[1]))
hooks = sys.argv[2]
auto = any(f.endswith(".json") and "PreCompact" in f and json.load(open(os.path.join(hooks, f))).get("trigger") == "auto"
           for f in (os.listdir(hooks) if os.path.isdir(hooks) else []))
tot = lambda r: r["input_tokens"] + r["cache_read_tokens"] + r["cache_write_tokens"]
i = next((k for k, r in enumerate(rows) if r["request_class"] == "main" and tot(r) > 40000), None)
if i is None:
    print("not-crossed|no main row over 40000 prompt tokens (max %d)" % max([tot(r) for r in rows] or [0]))
    sys.exit()
sig = "context_signal" in (rows[i]["adjustments"] or "")
nxt = next((r for r in rows[i + 1:] if r["request_class"] != "auxiliary"), None)
comp = bool(nxt and nxt["compaction"] == 1)
ok = sig and comp and auto
print("%s|crossing row %d (%d tokens) context_signal=%s; next non-auxiliary row compaction=%s; PreCompact trigger auto=%s"
      % ("pass" if ok else "fail", i, tot(rows[i]), sig, comp, auto))
PY
)
r21=${v%%|*}
result 21 "$r21" "${v#*|}"

# P1 tool_use
window_run p1
run_zygo claude -- -p --max-turns 4 "Call the mcp__zygo__compact tool once, then reply with the word done." </dev/null >"$RUN/out.txt" 2>"$RUN/err.txt" || true
usage_rows "$FX/p1-tooluse-usage.json"
v=$(python3 -I - "$RUN/usage.json" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1]))
i = next((k for k, r in enumerate(rows) if "context_compact" in (r["adjustments"] or "")), None)
j = next((k for k in range(i + 1, len(rows)) if rows[k]["request_class"] != "auxiliary" and rows[k]["compaction"] == 1), None) if i is not None else None
m = next((k for k in range(j + 1, len(rows)) if rows[k]["request_class"] == "main"), None) if j is not None else None
ok = None not in (i, j, m)
print("%s|context_compact row %s, compaction row %s, main row after it %s" % ("pass" if ok else "fail", i, j, m))
PY
)
rp=${v%%|*}
if [ "$r21" = pass ] && [ "$rp" = pass ]; then r=pass; elif [ "$r21" = not-crossed ] && [ "$rp" = pass ]; then r=not-crossed; else r=fail; fi
result P1 "$r" "end_turn (from 21): $r21; tool_use: ${v#*|}"
live_done "$FX"

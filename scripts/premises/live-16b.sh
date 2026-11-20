#!/bin/sh
# Owner-run live check #16b (node N38, spec §20 #16b). Needs the OpenAI API key and LIVE_SMALL_MODEL
# (an OpenAI-dialect model id whose real input limit is at most 128,000 tokens) in the
# environment; optional LIVE_SMALL_BASE_URL (default https://api.openai.com).
# zygo is told the model has a 1M window, so a zygo-code subagent reading 26 large files overruns
# the real limit: the vendor's error maps to `prompt is too long`, Claude Code compacts that
# subagent reactively, and it goes on.
# Turn count: 26 files, one Bash call each, the fewest that still cross a 128,000-token limit.
# Each file is about 30,000 characters:
# 417 lines of 12 short common words, at least one token a word, so at least 5,004 tokens a
# file: 26 files are at least 130,104 tokens before the subagent's own prompt and tools; 25
# files (125,100) might not cross.
# Spend bound: the lead runs with --max-turns 3 (the Agent call and the reply); the subagent's
# turns are bounded by the file count, and the whole run by 1800 s of wall clock
# (LIVE_TIMEOUT, live.sh). Cost estimate: docs/live-checks.md.
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
need_env OPENAI_API_KEY
need_env LIVE_SMALL_MODEL
LIVE_KEYS=OPENAI_API_KEY
LIVE_TIMEOUT=1800
live_traps
pinned_agents
FX="$REPO_DIR/fixtures/claude-code/live"
mkdir -p "$FX"

live_run w16b
cat >"$RUN/zygo.toml" <<T
[[model]]
id = "live-small"

  [model.facts]
  reasoning = "none"
  effort_levels = []
  max_output_tokens = 0
  context_window = 1000000
  sampling = false
  vision = false
  structured_output = "none"
  forced_tool_choice = false
  verified_on = $(date +%Y-%m-%d)
  source = "owner-chosen model, live check 16b"

  [[model.provider]]
  name = "openai"
  base_url = "${LIVE_SMALL_BASE_URL:-https://api.openai.com}"
  dialects = ["openai"]
  upstream_model = "$LIVE_SMALL_MODEL"
  key = "env:OPENAI_API_KEY"
  enabled = true

[features]
results = false

T
for r in lead design plan triage review code read; do
  printf '[role.%s]\nmodel = "live-small"\n\n' "$r" >>"$RUN/zygo.toml"
done
run_zygo config check >/dev/null || { echo "live: zygo config check failed"; exit 1; }
hook_settings "$RUN/hooks" | jq '. + {permissions: {allow: ["Bash"], defaultMode: "default"}}' >"$H/.claude/settings.json"
python3 -I - "$R/big" <<'PY'
import os, random, sys
d = sys.argv[1]
os.makedirs(d, exist_ok=True)
r = random.Random(16)
words = "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike".split()
for i in range(1, 27):
    lines, n = [], 0
    while n < 30000:
        l = " ".join(r.choice(words) for _ in range(12))
        lines.append(l)
        n += len(l) + 1
    lines[-1] = "tracer-16b last line of f%02d" % i
    open(os.path.join(d, "f%02d.txt" % i), "w").write("\n".join(lines) + "\n")
PY
rc=0
run_zygo claude -- -p --max-turns 3 --debug-file "$RUN/claude-debug.txt" "Use the zygo-code subagent. It runs cat big/f01.txt through cat big/f26.txt, one Bash call each, in order, then reports the last line of big/f26.txt. Reply with that line." </dev/null >"$RUN/out.txt" 2>"$RUN/err.txt" || rc=$?
LIVE_VER=$(usage_version); LIVE_VER=${LIVE_VER:-$PIN_CC}
n=$(grep -c 'prompt is too long' "$RUN/claude-debug.txt" 2>/dev/null || true)
usage_rows "$FX/16b-usage.json"
v=$(python3 -I - "$RUN/usage.json" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1]))
for aid in sorted({r["agent_id"] for r in rows if r["agent_id"]}):
    rs = [r for r in rows if r["agent_id"] == aid]
    i = next((k for k, r in enumerate(rs) if r["status"] == 400), None)
    j = next((k for k in range(i + 1, len(rs)) if rs[k]["compaction"] == 1), None) if i is not None else None
    m = next((k for k in range(j + 1, len(rs)) if rs[k]["status"] == 200), None) if j is not None else None
    if None not in (i, j, m):
        print("yes|one subagent: 400, then compaction, then 200")
        sys.exit()
print("no|no subagent with 400, then compaction, then 200 (%d rows, statuses %s)" % (len(rows), sorted({r["status"] for r in rows})))
PY
)
if [ "${n:-0}" -ge 1 ] && [ "${v%%|*}" = yes ] && [ "$rc" -eq 0 ]; then r=pass; else r=fail; fi
result 16b "$r" "model $LIVE_SMALL_MODEL; exit $rc; 'prompt is too long' in the debug file: ${n:-0}; ${v#*|}"
live_done "$FX"

#!/bin/sh
# Owner-run live check R-RT (node N38, spec §7.6): the reasoning round trip in a tool loop against
# each vendor, gpt-6.2-sol (OpenAI, translated) then claude-haiku-5-5 (Anthropic, native).
# Needs the Anthropic API key and the OpenAI API key in the environment.
# Spend bound: --max-turns 5 (the task takes 3: two Bash calls and the reply) and 600 s of wall
# clock per vendor (LIVE_TIMEOUT, live.sh).
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
need_env ANTHROPIC_API_KEY
need_env OPENAI_API_KEY
LIVE_KEYS="ANTHROPIC_API_KEY OPENAI_API_KEY"
LIVE_TIMEOUT=600
live_traps
pinned_agents
FX="$REPO_DIR/fixtures/claude-code/live"
mkdir -p "$FX"

rt() { # <vendor> <model id>
  live_run "rt-$1"
  : >"$RUN/zygo.toml"
  for r in lead design plan triage review code read; do
    printf '[role.%s]\nmodel = "%s"\n\n' "$r" "$2" >>"$RUN/zygo.toml"
  done
  run_zygo config check >/dev/null || { echo "live: zygo config check failed"; exit 1; }
  hook_settings "$RUN/hooks" | jq '. + {permissions: {allow: ["Bash"], defaultMode: "default"}}' >"$H/.claude/settings.json"
  rc=0
  run_zygo claude -- -p --max-turns 5 "Run the Bash command echo rt-one, then the Bash command echo rt-two, then reply with the word done." </dev/null >"$RUN/out.txt" 2>"$RUN/err.txt" || rc=$?
  usage_rows "$WORK/rt-$1.json"
  v=$(python3 -I - "$RUN/usage.json" "$rc" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1]))
main = sum(1 for r in rows if r["request_class"] == "main" and r["status"] == 200)
dropped = sum(1 for r in rows if "thinking_dropped" in (r["adjustments"] or ""))
ok = sys.argv[2] == "0" and main >= 3 and dropped == 0
print("%s|exit %s, %d main 200 rows, %d with thinking_dropped" % ("pass" if ok else "fail", sys.argv[2], main, dropped))
PY
)
  eval "r_$1=\${v%%|*}"
  eval "t_$1=\${v#*|}"
}
WORK="$N2_SCRATCH/live-$$/work"
mkdir -p "$WORK"
rt openai gpt-6.2-sol
LIVE_VER=$(usage_version)
rt anthropic claude-haiku-5-5
jq -n --slurpfile o "$WORK/rt-openai.json" --slurpfile a "$WORK/rt-anthropic.json" '{openai: $o[0], anthropic: $a[0]}' >"$FX/rt-usage.json"
LIVE_VER=${LIVE_VER:-$LIVE_CC_VER}
if [ "$r_openai" = pass ] && [ "$r_anthropic" = pass ]; then r=pass; else r=fail; fi
result R-RT "$r" "openai (gpt-6.2-sol): $r_openai, $t_openai; anthropic (claude-haiku-5-5): $r_anthropic, $t_anthropic"
live_done "$FX"

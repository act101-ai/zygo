#!/bin/sh
# Owner-run live checks N14-S and N14-E (node N38, spec §7.6). Needs the Anthropic API key and the
# OpenAI API key in the environment (entered at a hidden prompt, docs/live-checks.md).
# N14-S: zygo rewrites one tool schema per built-in `refuse` rule (strict_schemas on); the
# rewrites are captured at the mock (no key), then each is sent with strict mode to its vendor.
# N14-E: each catalog effort level of gpt-6.2-astra, Responses `reasoning.effort` and chat
# `reasoning_effort`. Keys reach curl on stdin (-H @-), never in argv or a file.
# Usage: sh scripts/premises/live-strict.sh [--capture-only]
#   --capture-only: only the mock capture (no key, no network): prints the captured
#   <family>-<code>.<api> names, found by the (family-code) marker of each probe file.
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
CAPTURE_ONLY=
if [ "${1:-}" = --capture-only ]; then CAPTURE_ONLY=1; else
  need_env ANTHROPIC_API_KEY
  need_env OPENAI_API_KEY
fi
live_traps
pinned_agents
FX="$REPO_DIR/fixtures/claude-code/live"
mkdir -p "$FX"
STRICT="$PREMISES_DIR/live/strict"
# The version the claude-curl.sh stand-in reports (LIVE_PIN_CC): N14-S runs the stand-in, not the installed claude.
STANDIN_CC=$PIN_CC
WORK="$N2_SCRATCH/live-$$/work"
mkdir -p "$WORK"

# Capture (mock, no key): code -> an anthropic row, read -> an <dialect> row; claude-curl.sh as claude.
capture() { # <dialect>
  live_run "strict-$1"
  start_mock
  mock_config "$RUN/zygo.toml"
  cat >>"$RUN/zygo.toml" <<T
[[model]]
id = "live-mock-oa"

  [model.facts]
  reasoning = "always"
  effort_levels = ["low", "medium", "high", "xhigh", "max"]
  max_output_tokens = 128000
  context_window = 922000
  sampling = false
  vision = true
  structured_output = "native"
  forced_tool_choice = false
  verified_on = 2026-11-18
  source = "scripts/premises/live-strict.sh: mock upstream"

  [[model.provider]]
  name = "mock-oa"
  base_url = "$MOCK_URL"
  dialects = ["$1"]
  upstream_model = "live-mock-oa"
  key = "env:ZYGO_LIVE_MOCK_KEY"
  enabled = true

[proxy]
strict_schemas = true
T
  sed -i.bak '/^\[role\.read\]$/{n;s/.*/model = "live-mock-oa"/;}' "$RUN/zygo.toml"
  mkdir -p "$RUN/bin"
  ln -sf "$PREMISES_DIR/live/claude-curl.sh" "$RUN/bin/claude"
  OLD_PATH=$TOOL_PATH; TOOL_PATH="$RUN/bin:$TOOL_PATH"
  LIVE_KEYS="" LIVE_VARS="LIVE_PIN_CC=$STANDIN_CC LIVE_STRICT_DIR=$STRICT" run_zygo claude </dev/null >"$RUN/out.txt" 2>&1 || true
  TOOL_PATH=$OLD_PATH
  stop_mock
  # The rewritten schema of each probe file, by its marker, from the recorded requests.
  python3 -I - "$RUN/rec" "$1" "$WORK" <<'PY'
import json, os, re, sys
rec, dialect, out = sys.argv[1:4]
for f in sorted(os.listdir(rec)):
    if not f[0].isdigit():
        continue
    r = json.load(open(os.path.join(rec, f)))
    b = r.get("body")
    m = re.search(r"\((anthropic|openai)-([a-z_]+)\)", json.dumps(b))
    if not isinstance(b, dict) or not m or not b.get("tools"):
        continue
    t = b["tools"][0]
    if "/v1/messages" in r["path"]:
        api, sch = "messages", t.get("input_schema")
    elif "/v1/responses" in r["path"]:
        api, sch = "responses", t.get("parameters")
    else:
        api, sch = "chat", (t.get("function") or {}).get("parameters")
    if (m.group(1) == "anthropic") != (api == "messages"):
        continue
    json.dump(sch, open(os.path.join(out, "%s-%s.%s.json" % (m.group(1), m.group(2), api)), "w"))
PY
}
capture openai
capture openai-chat
LIVE_KEYS=""
if [ -n "$CAPTURE_ONLY" ]; then
  # 8 anthropic probes (messages) + 5 openai probes x (responses, chat) = 18 rewritten schemas.
  got=$(ls "$WORK" | sed 's/\.json$//' | sort)
  echo "live: captured $(printf '%s\n' "$got" | grep -c .) of 18 rewritten schemas:"
  printf '%s\n' "$got"
  live_done "$FX"
  exit 0
fi

# post <url> <header name> <scheme prefix> <key variable name> <body file> <out file> [<extra header>]
post() {
  eval "k=\${$4}"
  printf '%s: %s%s\n' "$2" "$3" "$k" |
    curl -sS -m 60 -o "$6" -w '%{http_code}' -H @- -H 'content-type: application/json' ${7:+-H "$7"} --data-binary @"$5" "$1" || echo 000
  k=
}
err() { jq -r '.error.message // empty' "$1" 2>/dev/null | head -c 300 >"$1.msg"; scrub "$1.msg"; }

# N14-S
printf '[' >"$WORK/strict.json"
sep=""
n14s=pass
for f in "$STRICT"/*.json; do
  name=$(basename "$f" .json); fam=${name%%-*}; code=${name#*-}
  orig=$(jq -c '.tools[0].input_schema' "$f")
  if [ "$fam" = anthropic ]; then apis=messages; else apis="responses chat"; fi
  for api in $apis; do
    rw="$WORK/$name.$api.json"
    msg=""
    for which in rewritten original; do
      if [ "$which" = rewritten ]; then
        [ -e "$rw" ] || { eval "st_$which=missing"; continue; }
        sch=$(cat "$rw")
      else
        sch=$orig
      fi
      case $api in
        messages)
          jq -n --argjson s "$sch" '{model: "claude-haiku-5-5", max_tokens: 64, tools: [{name: "t", description: "Strict-schema probe.", input_schema: $s, strict: true}], tool_choice: {type: "tool", name: "t"}, messages: [{role: "user", content: "Call t."}]}' >"$WORK/body.json"
          st=$(post https://api.anthropic.com/v1/messages x-api-key "" ANTHROPIC_API_KEY "$WORK/body.json" "$WORK/resp.json" 'anthropic-version: 2023-06-01') ;;
        responses)
          jq -n --argjson s "$sch" '{model: "gpt-6.2-astra", max_output_tokens: 64, tools: [{type: "function", name: "t", description: "Strict-schema probe.", parameters: $s, strict: true}], input: "Call t."}' >"$WORK/body.json"
          st=$(post https://api.openai.com/v1/responses authorization "Bearer " OPENAI_API_KEY "$WORK/body.json" "$WORK/resp.json") ;;
        chat)
          jq -n --argjson s "$sch" '{model: "gpt-6.2-astra", max_completion_tokens: 64, tools: [{type: "function", function: {name: "t", description: "Strict-schema probe.", parameters: $s, strict: true}}], messages: [{role: "user", content: "Call t."}]}' >"$WORK/body.json"
          st=$(post https://api.openai.com/v1/chat/completions authorization "Bearer " OPENAI_API_KEY "$WORK/body.json" "$WORK/resp.json") ;;
      esac
      eval "st_$which=\$st"
      [ "$which" = rewritten ] && msg=$(err "$WORK/resp.json")
    done
    [ "$st_rewritten" = 200 ] || n14s=fail
    jq -n --arg code "$code" --arg family "$fam" --arg api "$api" --arg r "$st_rewritten" --arg o "$st_original" --arg m "$msg" \
      '{code: $code, family: $family, api: $api, rewritten_status: $r, original_status: $o, error_message: $m}' >"$WORK/row.json"
    printf '%s' "$sep" >>"$WORK/strict.json"; cat "$WORK/row.json" >>"$WORK/strict.json"; sep=","
  done
done
printf ']\n' >>"$WORK/strict.json"
jq . "$WORK/strict.json" >"$FX/n14-strict.json"
LIVE_VER=$STANDIN_CC
result N14-S "$n14s" "rewritten schema not 200: $(jq -c '[.[] | select(.rewritten_status != "200") | "\(.family)-\(.code)/\(.api)=\(.rewritten_status)"]' "$FX/n14-strict.json")"

# N14-E
printf '[' >"$WORK/effort.json"
sep=""
n14e=pass
for L in low medium high xhigh max; do
  jq -n --arg l "$L" '{model: "gpt-6.2-astra", max_output_tokens: 32, reasoning: {effort: $l}, input: "Say ok."}' >"$WORK/body.json"
  st=$(post https://api.openai.com/v1/responses authorization "Bearer " OPENAI_API_KEY "$WORK/body.json" "$WORK/resp.json")
  msg=$(err "$WORK/resp.json")
  [ "$st" = 200 ] || n14e=fail
  printf '%s' "$sep" >>"$WORK/effort.json"; sep=","
  jq -n --arg api responses --arg e "$L" --arg s "$st" --arg m "$msg" '{api: $api, effort: $e, status: $s, error_message: $m}' >>"$WORK/effort.json"
  jq -n --arg l "$L" '{model: "gpt-6.2-astra", max_completion_tokens: 32, reasoning_effort: $l, messages: [{role: "user", content: "Say ok."}]}' >"$WORK/body.json"
  st=$(post https://api.openai.com/v1/chat/completions authorization "Bearer " OPENAI_API_KEY "$WORK/body.json" "$WORK/resp.json")
  msg=$(err "$WORK/resp.json")
  [ "$st" = 200 ] || n14e=fail
  printf ',' >>"$WORK/effort.json"
  jq -n --arg api chat --arg e "$L" --arg s "$st" --arg m "$msg" '{api: $api, effort: $e, status: $s, error_message: $m}' >>"$WORK/effort.json"
done
printf ']\n' >>"$WORK/effort.json"
jq . "$WORK/effort.json" >"$FX/n14-effort.json"
result N14-E "$n14e" "not 200: $(jq -c '[.[] | select(.status != "200") | "\(.api)/\(.effort)=\(.status)"]' "$FX/n14-effort.json")"
live_done "$FX"

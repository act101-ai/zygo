#!/bin/sh
# P1: does Claude Code compact before its next request after a streamed response whose
# message_delta usage reports a full window? Cases: end_turn (second turn over stream-json),
# tool_use (loop continues), subagent; each with and without the usage.iterations entry.
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/claude-code"
mkdir -p "$FX/sse" "$FX/requests"
# Reported window: 1,000,000 (at least any window Claude Code assumes; spec §10.2.2 max()).
W=1000000 IN=1000 CW=0
CR=$((W - IN - CW))
USAGE="{\"input_tokens\":$IN,\"cache_creation_input_tokens\":$CW,\"cache_read_input_tokens\":$CR,\"output_tokens\":5}"
for it in iter noiter; do
  flag=""; [ "$it" = iter ] && flag=--iterations
  python3 -I "$PREMISES_DIR/sse.py" text "p1 full window" --usage "$USAGE" $flag >"$FX/sse/p1-endturn-full-$it.sse"
  python3 -I "$PREMISES_DIR/sse.py" tool Bash toolu_p1_bash '{"command":"echo p1-tool-out"}' --usage "$USAGE" $flag >"$FX/sse/p1-tooluse-full-$it.sse"
done
python3 -I "$PREMISES_DIR/sse.py" tool Agent toolu_p1_agent '{"description":"p1 subagent","prompt":"tracer-p1-sub run one command","subagent_type":"claude","run_in_background":false}' >"$FX/sse/p1-dispatch.sse"
python3 -I "$PREMISES_DIR/sse.py" text "p1 done" >"$FX/sse/p1-done.sse"

case_endturn() { # <iter|noiter> [<run name> <model> <sse>]
  new_run "${2:-P1-endturn-$1}"
  cat >"$RUN/rules.json" <<J
[{"name":"signal","no_header":["x-claude-code-compaction"],"header":{"x-claude-code-request-class":"^main$"},"last":"tracer-p1-a","times":1,"sse":"${4:-$FX/sse/p1-endturn-full-$1.sse}"}]
J
  printf '%s\n' "tracer-p1-a first turn" "tracer-p1-b second turn" >"$RUN/msgs"
  start_mock "$RUN/rules.json"
  run_claude_stream "$RUN/out.jsonl" "$RUN/msgs" --model "${3:-claude-opus-5-5}" 2>"$RUN/stderr.txt"
  stop_mock
}

case_tooluse() {
  new_run "P1-tooluse-$1"
  cat >"$RUN/rules.json" <<J
[{"name":"signal","no_header":["x-claude-code-compaction"],"header":{"x-claude-code-request-class":"^main$"},"last":"tracer-p1-t","times":1,"sse":"$FX/sse/p1-tooluse-full-$1.sse"},
 {"name":"done","no_header":["x-claude-code-compaction"],"last":"toolu_p1_bash","times":1,"sse":"$FX/sse/p1-done.sse"}]
J
  start_mock "$RUN/rules.json"
  run_claude -p "tracer-p1-t run one command" --model claude-opus-5-5 --dangerously-skip-permissions </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock
}

case_subagent() {
  new_run "P1-subagent-$1"
  cat >"$RUN/rules.json" <<J
[{"name":"dispatch","no_header":["x-claude-code-compaction","x-claude-code-agent-id"],"header":{"x-claude-code-request-class":"^main$"},"last":"tracer-p1-main","times":1,"sse":"$FX/sse/p1-dispatch.sse"},
 {"name":"sub-signal","no_header":["x-claude-code-compaction"],"header":{"x-claude-code-agent-id":"."},"last":"tracer-p1-sub","times":1,"sse":"$FX/sse/p1-tooluse-full-$1.sse"},
 {"name":"sub-done","no_header":["x-claude-code-compaction"],"header":{"x-claude-code-agent-id":"."},"last":"toolu_p1_bash","times":1,"sse":"$FX/sse/p1-done.sse"},
 {"name":"main-done","no_header":["x-claude-code-compaction","x-claude-code-agent-id"],"last":"toolu_p1_agent","times":1,"sse":"$FX/sse/p1-done.sse"}]
J
  start_mock "$RUN/rules.json"
  run_claude -p "tracer-p1-main dispatch a subagent" --model claude-opus-5-5 --dangerously-skip-permissions </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock
}

for it in iter noiter; do
  case_endturn $it
  python3 -I "$PREMISES_DIR/summarize.py" "$RUN/rec" >"$RUN/summary.json"; scrub "$RUN/summary.json" >"$FX/requests/p1-endturn-$it.summary.json"
  case_tooluse $it
  python3 -I "$PREMISES_DIR/summarize.py" "$RUN/rec" >"$RUN/summary.json"; scrub "$RUN/summary.json" >"$FX/requests/p1-tooluse-$it.summary.json"
  case_subagent $it
  python3 -I "$PREMISES_DIR/summarize.py" "$RUN/rec" >"$RUN/summary.json"; scrub "$RUN/summary.json" >"$FX/requests/p1-subagent-$it.summary.json"
done
# A role name without behavesAs (an id Claude Code does not recognize, assumed window 1,000,000):
# a full 1,000,000 usage, and a control at 300,000 (over a 200k window, under the assumed one).
python3 -I "$PREMISES_DIR/sse.py" text "p1 300k" --iterations \
  --usage '{"input_tokens":1000,"cache_creation_input_tokens":0,"cache_read_input_tokens":299000,"output_tokens":5}' >"$FX/sse/p1-endturn-300k-iter.sse"
case_endturn iter P1-unrecognized-full lead
python3 -I "$PREMISES_DIR/summarize.py" "$RUN/rec" >"$RUN/summary.json"; scrub "$RUN/summary.json" >"$FX/requests/p1-unrecognized-full.summary.json"
scrub "$RUN/stderr.txt" >"$FX/requests/p1-unrecognized-full.stderr.txt"
case_endturn iter P1-unrecognized-300k lead "$FX/sse/p1-endturn-300k-iter.sse"
python3 -I "$PREMISES_DIR/summarize.py" "$RUN/rec" >"$RUN/summary.json"; scrub "$RUN/summary.json" >"$FX/requests/p1-unrecognized-300k.summary.json"
echo "P1 done"

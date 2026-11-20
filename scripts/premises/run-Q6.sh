#!/bin/sh
# Q6: where typed command arguments land in the request: echo-a.md uses $ARGUMENTS,
# echo-b.md does not. Invoked in print mode and interactively (tmux).
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/claude-code"
mkdir -p "$FX/requests" "$FX/q6"
new_run Q6
mkdir -p "$R/.claude/commands"
printf -- '---\ndescription: echo a\n---\nCommand A body. Arguments here: $ARGUMENTS. End of A.\n' >"$R/.claude/commands/echo-a.md"
printf -- '---\ndescription: echo b\n---\nCommand B body without a placeholder.\n' >"$R/.claude/commands/echo-b.md"
cp "$R/.claude/commands/echo-a.md" "$R/.claude/commands/echo-b.md" "$FX/q6/"
start_mock
run_claude -p "/echo-a tracer one" </dev/null >"$RUN/out-a.txt" 2>&1 || true
run_claude -p "/echo-b tracer two" </dev/null >"$RUN/out-b.txt" 2>&1 || true
tmux_claude n2q6
sleep 6
n=$(ls "$RUN/rec" | grep -c '^[0-9]')
tmux_send n2q6 "/echo-a tracer three"
sleep 1; tmux send-keys -t =n2q6: Enter
tmux_wait_req $((n + 1)) 60 || true
sleep 3
n=$(ls "$RUN/rec" | grep -c '^[0-9]')
tmux_send n2q6 "/echo-b tracer four"
sleep 1; tmux send-keys -t =n2q6: Enter
tmux_wait_req $((n + 1)) 60 || true
sleep 3
tmux capture-pane -p -t =n2q6: >"$RUN/screen.txt"
tmux kill-session -t =n2q6 2>/dev/null || true
stop_mock
# The user message blocks of the request that first carries each tracer.
for t in "tracer one" "tracer two" "tracer three" "tracer four"; do
  f=$(grep -l "$t" "$RUN"/rec/[0-9]*.json | head -n 1 || true)
  [ -n "$f" ] || { echo "no request carries '$t'"; continue; }
  slug=$(echo "$t" | tr ' ' '-')
  jq --arg t "$t" '{tracer: $t, request_class: .headers["x-claude-code-request-class"],
     user_blocks: [.body.messages[] | select(.role == "user") | (.content | if type == "string" then [{type: "text", text: .}] else . end)[] | select(.type == "text") | .text | select(startswith("<system-reminder>") | not)]}' "$f" >"$RUN/q6-$slug.json"
  scrub "$RUN/q6-$slug.json" >"$FX/q6/$slug.json"
done
cat "$FX"/q6/tracer-*.json
echo "Q6 done"

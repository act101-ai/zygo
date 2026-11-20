#!/bin/sh
# P1, end_turn case, interactive: the same signal in a tmux-driven interactive session.
. "$(dirname "$0")/lib.sh"
FX="$REPO_DIR/fixtures/claude-code"
new_run P1-interactive
cat >"$RUN/rules.json" <<J
[{"name":"signal","no_header":["x-claude-code-compaction"],"header":{"x-claude-code-request-class":"^main$"},"last":"tracer-p1-a","times":1,"sse":"$FX/sse/p1-endturn-full-iter.sse"}]
J
start_mock "$RUN/rules.json"
tmux_claude n2p1 --model claude-opus-5-5
sleep 6
tmux capture-pane -p -t =n2p1: >"$RUN/screen0.txt"
tmux_send n2p1 "tracer-p1-a first turn"
tmux_wait_req 1 60
sleep 4
tmux_send n2p1 "tracer-p1-b second turn"
sleep 15
tmux capture-pane -p -t =n2p1: >"$RUN/screen1.txt"
tmux send-keys -t =n2p1: C-c; sleep 0.5; tmux send-keys -t =n2p1: C-c; sleep 2
tmux kill-session -t =n2p1 2>/dev/null || true
stop_mock
python3 -I "$PREMISES_DIR/summarize.py" "$RUN/rec" >"$RUN/summary.json"
scrub "$RUN/summary.json" >"$FX/requests/p1-endturn-interactive.summary.json"
echo "P1 interactive done"

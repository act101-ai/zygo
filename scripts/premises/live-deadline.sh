#!/bin/sh
# Evidence for the debug-build finding (node N38, round 2 R11): an interactive Claude Code body
# through a debug build of zygo misses the 200 ms secret-redaction deadline (§16.1) and gets a 503;
# a release build passes. Mock upstream, no key. Needs both builds:
#   cargo build -p zygo && cargo build -p zygo --release
# Fixtures: deadline-debug.json, deadline-release.json (each run's usage rows: status,
# latency_ms, request class, redactions; metadata only).
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps
pinned_agents
FX="$REPO_DIR/fixtures/claude-code/live"
mkdir -p "$FX"

dl_run() { # <build: debug|release>
  live_run "dl$1"
  start_mock; mock_config "$RUN/zygo.toml"
  hook_settings "$RUN/hooks" >"$H/.claude/settings.json"
  ZYGO_BIN="$REPO_DIR/target/$1/zygo"
  tmux_zygo "dl$1" claude
  tmux_wait_text "dl$1" 'zygo ◇' 30 || true
  tmux_send "dl$1" "tracer-dl say hi"
  sleep 15
  tmux capture-pane -p -t "=dl$1:" >"$RUN/pane.txt" 2>/dev/null || true
  tmux kill-session -t "=dl$1" 2>/dev/null || true
  stop_mock
  sqlite3 -json "$H/.local/share/zygo/usage.db" "select request_class, status, latency_ms, input_tokens, redactions from requests order by ts, rowid" >"$RUN/rows.json" 2>/dev/null || true
  [ -s "$RUN/rows.json" ] || echo '[]' >"$RUN/rows.json"
  scrub "$RUN/rows.json" >"$FX/deadline-$1.json"
  LIVE_VER=$(usage_version); LIVE_VER=${LIVE_VER:-$LIVE_CC_VER}
  result "DL-$1" recorded "$(jq -c '[.[] | "\(.request_class // "-")=\(.status)/\(.latency_ms)ms"]' "$RUN/rows.json"); pane shows 503: $(grep -c '503' "$RUN/pane.txt" || true)"
}

dl_run debug
dl_run release
live_done "$FX"

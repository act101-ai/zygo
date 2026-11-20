#!/bin/sh
# Live check N35-D (node N38): a plugin installed in the pinned Claude Code (`claude plugin
# install act-lite@act101-marketplace`; the marketplace act101-ai/act101 registers under that
# name) and also selected in zygo (act-lite, on by default) shows in `zygo doctor`; the place
# and layout of installed_plugins.json, for the default and a custom CLAUDE_CONFIG_DIR.
# Needs the network (GitHub); no mock, no key.
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps
pinned_agents
FX="$REPO_DIR/fixtures/claude-code/live"
mkdir -p "$FX"
WANT='plugin act101:act-lite: also installed in Claude Code as act-lite@act101-marketplace; it would load twice'

doctor_run() { # <name> <config dir, relative to the run's home>
  live_run "$1"
  : >"$RUN/zygo.toml"
  LIVE_CC_DIR="$H/$2"
  mkdir -p "$LIVE_CC_DIR"
  [ "$2" = .claude ] || mv "$H/.claude/.claude.json" "$LIVE_CC_DIR/.claude.json"
  live_exec "$CLAUDE_BIN" plugin marketplace add act101-ai/act101 >"$RUN/cc.txt" 2>&1 || true
  live_exec "$CLAUDE_BIN" plugin install act-lite@act101-marketplace >>"$RUN/cc.txt" 2>&1 || true
  run_zygo doctor >"$RUN/doctor.txt" 2>&1 || true
  LIVE_CC_DIR=
  IP="$H/$2/plugins/installed_plugins.json"
}

doctor_run doctor-default .claude
scrub "$RUN/doctor.txt" >"$FX/doctor-default.txt"
[ -e "$IP" ] && scrub "$IP" >"$FX/installed_plugins.json"
a_line=$(grep -cF "$WANT" "$RUN/doctor.txt" || true)
a_file=$(jq -r 'if has("version") and has("plugins") then "version+plugins" else "other: \(keys)" end' "$IP" 2>/dev/null || echo missing)
a_keys=$(jq -c '.plugins | keys' "$IP" 2>/dev/null || echo '[]')

doctor_run doctor-custom cc-config
scrub "$RUN/doctor.txt" >"$FX/doctor-custom-dir.txt"
b_line=$(grep -cF "$WANT" "$RUN/doctor.txt" || true)
b_file=$(jq -r 'if has("version") and has("plugins") then "version+plugins" else "other: \(keys)" end' "$IP" 2>/dev/null || echo missing)

if [ "$a_line" -ge 1 ] && [ "$b_line" -ge 1 ] && [ "$a_file" = version+plugins ] && [ "$b_file" = version+plugins ]; then r=pass; else r=fail; fi
result N35-D "$r" "default dir: doctor line $a_line, installed_plugins.json $a_file $a_keys; custom CLAUDE_CONFIG_DIR: doctor line $b_line, installed_plugins.json $b_file (form: claude plugin marketplace add / plugin install)"
live_done "$FX"

#!/bin/sh
# Live checks on the pinned OpenCode through zygo, mock upstream (node N38): OC-AUTO, OC-SHAPES,
# OC-THROW. No key; the mock answers only the dummy key. The N2 probe plugin sits in the global
# OpenCode config dir (zygo sets its own OPENCODE_CONFIG_DIR) and logs the tool hook payloads.
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps
pinned_agents
LIVE_VER=$LIVE_OC_VER
FX="$REPO_DIR/fixtures/opencode/live"
mkdir -p "$FX"
TX='"extra": {"body": "\"tools\": \\[\\{"}'

steps() {
  mkdir -p "$RUN/sse"
  printf '%s\n' "$2" >"$RUN/steps-$1.json"
  python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" "$1" "$RUN/rules.json" "$RUN/steps-$1.json"
}
oc_run() { # <name>: a run with the probe plugin in the global config dir
  live_run "$1"
  mkdir -p "$H/.config/opencode/plugins"
  cp "$PREMISES_DIR/opencode/zygo-probe.js" "$H/.config/opencode/plugins/zygo-probe.js"
  LIVE_VARS="N2_OC_LOG=$RUN/plugin-log"
}
probe_loaded() {
  [ -e "$RUN/plugin-log/plugin-init.json" ] && return 0
  result OC-SHAPES fail "probe plugin not loaded from the global config dir"
  exit 1
}

# OC-AUTO: zygo opencode --yolo puts --auto before the run subcommand.
oc_run ocauto
start_mock; mock_config "$RUN/zygo.toml"
rc=0
run_zygo opencode --yolo -- run "tracer-ocauto say hi" </dev/null >"$RUN/out.txt" 2>"$RUN/err.txt" || rc=$?
stop_mock
{ echo "exit $rc"; echo "stderr:"; cat "$RUN/err.txt"; } >"$RUN/oc-auto.txt"
scrub "$RUN/oc-auto.txt" >"$FX/oc-auto.txt"
n=$(body_has 'tracer-ocauto' "$RUN"/rec/[0-9]*.json | wc -l | tr -d ' ')
if [ "$rc" -eq 0 ] && [ "$n" -gt 0 ]; then r=pass; else r=fail; fi
result OC-AUTO "$r" "opencode --auto run …: exit $rc, $n request(s) with the prompt"

# OC-SHAPES: the tool.execute.before/after payloads of six tools. OC-SHAPES and OC-THROW pass
# --auto after the run subcommand (OpenCode 1.18.16 refuses it before run; see OC-AUTO) and run
# as role code (zygo denies edit to the main role, so write and edit would not exist).
oc_run ocshapes
python3 -I "$PREMISES_DIR/make_files.py" "$R/files"
start_web "$R/files"
LIVE_VARS="$LIVE_VARS NODE_EXTRA_CA_CERTS=$WEB_CA"
steps ocs '{"trigger": "tracer-ocshapes go", '"$TX"', "steps": [
  {"tool": "read", "input": {"filePath": "'"$R"'/files/README.txt"}},
  {"tool": "grep", "input": {"pattern": "tracer"}},
  {"tool": "glob", "input": {"pattern": "**/*.txt"}},
  {"tool": "webfetch", "input": {"url": "'"$WEB_URL"'/page.html", "format": "text"}},
  {"tool": "write", "input": {"filePath": "'"$R"'/new.txt", "content": "tracer write"}},
  {"tool": "edit", "input": {"filePath": "'"$R"'/new.txt", "oldString": "tracer write", "newString": "tracer edited"}},
  {"text": "ocshapes done"}]}'
start_mock "$RUN/rules.json"; mock_config "$RUN/zygo.toml"
run_zygo opencode -- run --auto -m zygo/code "tracer-ocshapes go" </dev/null >"$RUN/out.txt" 2>"$RUN/err.txt" || true
stop_mock; stop_web
probe_loaded
missing=""
for t in read grep glob webfetch write edit; do
  for w in before after; do
    f="$RUN/plugin-log/tool.execute.$w-$t.json"
    if [ -e "$f" ]; then scrub "$f" >"$FX/tool.execute.$w-$t.json"; else missing="$missing $w-$t"; fi
  done
done
keys() { [ -e "$RUN/plugin-log/tool.execute.before-$1.json" ] && jq -c '.output.args | keys' "$RUN/plugin-log/tool.execute.before-$1.json" || echo none; }
wf=$(jq -r '.output.output // .output.error // "" | tostring | .[0:60]' "$RUN/plugin-log/tool.execute.after-webfetch.json" 2>/dev/null || echo none)
if [ -z "$missing" ]; then r=pass; else r=fail; fi
result OC-SHAPES "$r" "missing:${missing:- none}; edit args $(keys edit); write args $(keys write); webfetch after: $wf"

# OC-THROW: the redaction guard's deny, thrown in tool.execute.before, blocks a write.
oc_run octhrow
steps oct '{"trigger": "tracer-octhrow go", '"$TX"', "steps": [
  {"tool": "write", "input": {"filePath": "'"$R"'/deny.txt", "content": "[REDACTED:live]"}},
  {"text": "octhrow done"}]}'
start_mock "$RUN/rules.json"; mock_config "$RUN/zygo.toml"
run_zygo opencode -- run --auto -m zygo/code "tracer-octhrow go" </dev/null >"$RUN/out.txt" 2>"$RUN/err.txt" || true
stop_mock
f=$(body_has 'toolu_oct_00_write' "$RUN"/rec/[0-9]*.json | tail -n 1)
denied=""
if [ -n "$f" ]; then
  jq '.body.messages[-1]' "$f" >"$RUN/oc-throw.json"
  scrub "$RUN/oc-throw.json" >"$FX/oc-throw.json"
  grep -q 'marks a secret zygo removed' "$RUN/oc-throw.json" && denied=yes
fi
if [ ! -e "$R/deny.txt" ] && [ -n "$denied" ]; then r=pass; else r=fail; fi
result OC-THROW "$r" "deny.txt exists: $([ -e "$R/deny.txt" ] && echo yes || echo no); deny text in the next request: ${denied:-no}"

live_done "$FX"

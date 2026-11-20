#!/bin/sh
# Smoke run and static captures: `claude -p hi` against the mock records one /v1/messages
# request; `claude --help`, `opencode --help`, the settings JSON schema, and the modelPicker /
# updatedToolOutput validation texts embedded in the pinned Claude Code binary.
. "$(dirname "$0")/lib.sh"
FXC="$REPO_DIR/fixtures/claude-code"
FXO="$REPO_DIR/fixtures/opencode"
mkdir -p "$FXC/requests" "$FXO"
new_run smoke
start_mock
run_claude -p hi </dev/null >"$RUN/out.txt" 2>&1 || true
stop_mock
n=$(ls "$RUN/rec" | grep -c '^[0-9]')
f=$(grep -l '"path": "/v1/messages' "$RUN"/rec/[0-9]*.json | head -n 1)
echo "smoke: $n request(s) recorded; first /v1/messages request: $(basename "$f")"
scrub "$f" >"$FXC/requests/smoke.json"

(cd "$R" && $(agent_env) "$CLAUDE_BIN" --help) >"$RUN/claude-help.txt" 2>&1
scrub "$RUN/claude-help.txt" >"$FXC/help.txt"
(cd "$R" && $(agent_env) "$OPENCODE_BIN" --help) >"$RUN/opencode-help.txt" 2>&1
scrub "$RUN/opencode-help.txt" >"$FXO/help.txt"

# Settings schema: the schemastore file verbatim, and its source, date and checksum beside it.
URL=https://json.schemastore.org/claude-code-settings.json
curl -sSL --max-time 30 -o "$FXC/settings-schema.json" "$URL"
python3 -I -c 'import json,sys; json.load(open(sys.argv[1]))' "$FXC/settings-schema.json"
{ echo "source: $URL"; echo "retrieved: $(date +%Y-%m-%d)"; echo "sha256: $(shasum -a 256 "$FXC/settings-schema.json" | cut -d' ' -f1)"
  echo "note: verbatim (not scrubbed); its example paths are upstream text. modelPicker is absent from it;"
  echo "      the pinned binary's validation texts for modelPicker are in settings-binary-excerpt.txt."; } >"$FXC/settings-schema.source.txt"
# Validation texts in the pinned binary (strings of the executable, selected lines).
B=$(python3 -I -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$CLAUDE_BIN")
strings -n 6 "$B" | grep -E 'must be an object with an "options" array|"replaceBuiltInOptions" must be|map it with behavesAs on a modelPicker row|so it assumes a context window of|PostToolUse hook returned updatedToolOutput that does not match|updatedToolOutput:[a-zA-Z]+\(\)\.optional\(\)\.describe' \
  | sed -E 's/.*(updatedToolOutput:[a-zA-Z]+\(\)\.optional\(\)\.describe\("[^"]*"\)).*/\1/' | awk "length(\$0) < 400" | sort -u >"$RUN/excerpt.txt"
{ echo "# Strings from the Claude Code $("$CLAUDE_BIN" --version | cut -d' ' -f1) executable (strings -n 6 | grep), selected lines."; cat "$RUN/excerpt.txt"; } >"$RUN/excerpt2.txt"
scrub "$RUN/excerpt2.txt" >"$FXC/settings-binary-excerpt.txt"
echo "smoke done"

#!/bin/sh
# Codex base part (node PC-2): the version that runs, the help texts and the pinned config schema.
# It sends no model request. Writes fixtures/codex/help.txt, config.schema.json (once, verbatim,
# from the rust-v0.172.0 tag, K§6.3.7), config.schema.source.txt and runs/base/.
PREMISES_DIR=$(cd "$(dirname "$0")/.." && pwd)
N2_AGENTS=codex
. "$PREMISES_DIR/lib.sh"
URL=https://raw.githubusercontent.com/openai/codex/rust-v0.172.0/codex-rs/core/config.schema.json
mkdir -p "$CODEX_FX"
new_run codex-base
start_mock
codex_env
run_codex --version >"$RUN/version.txt"
for a in "--help" "exec --help" "resume --help"; do
  echo "\$ codex $a"
  # shellcheck disable=SC2086
  run_codex $a 2>&1
  echo
done >"$RUN/help.txt"
stop_mock
scrub "$RUN/help.txt" >"$CODEX_FX/help.txt"
if [ ! -s "$CODEX_FX/config.schema.json" ]; then
  # The only network use of this part: a public file, no credential (-q: no .curlrc; env -i: no HOME).
  env -i PATH=/usr/bin:/bin curl -q -sSfL --max-time 30 -o "$CODEX_FX/config.schema.json" "$URL"
  python3 -I -c 'import json, sys; json.load(open(sys.argv[1]))' "$CODEX_FX/config.schema.json"
  { echo "source: $URL"; echo "retrieved: $(date +%Y-%m-%d)"
    echo "sha256: $(shasum -a 256 "$CODEX_FX/config.schema.json" | cut -d' ' -f1)"
    echo "note: verbatim (not scrubbed); the pinned schema of the rust-v0.172.0 tag (K§6.3.7)."; } >"$CODEX_FX/config.schema.source.txt"
fi
codex_record base
echo "codex base: ok (codex $CODEX_VERSION)"

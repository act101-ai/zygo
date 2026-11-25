#!/bin/sh
# Codex base part (node PC-2): the version that runs, the help texts and the pinned config schema.
# It sends no model request. Writes fixtures/codex/help.txt, config.schema.json (once, verbatim,
# from the rust-v0.172.0 tag, K§6.3.7), config.schema.source.txt and runs/base/.
# The schema is pinned by its sha256 (SCHEMA_SHA): a fetch goes to a temporary file in the run
# folder (a trap removes it on exit or a signal) and is moved into the fixture only when its sha256
# is the pinned one; every run checks that the fixture and the sha256 line of
# config.schema.source.txt both equal SCHEMA_SHA, before Codex runs.
PREMISES_DIR=$(cd "$(dirname "$0")/.." && pwd)
N2_AGENTS=codex
. "$PREMISES_DIR/lib.sh"
URL=https://raw.githubusercontent.com/openai/codex/rust-v0.172.0/codex-rs/core/config.schema.json
SCHEMA_SHA=52fb96b1ff79a51bf9ce31695a4a22d4e80f57591d91bad5747b657f4e612bac
SCHEMA=$CODEX_FX/config.schema.json SOURCE=$CODEX_FX/config.schema.source.txt
mkdir -p "$CODEX_FX"
new_run codex-base
if [ ! -s "$SCHEMA" ]; then
  SCHEMA_TMP=$(mktemp "$RUN/schema.XXXXXX")
  # shellcheck disable=SC2016
  n2_trap_chain 'rm -f "$SCHEMA_TMP"'
  # The only network use of this part: a public file, no credential (-q: no .curlrc; env -i: no HOME).
  env -i PATH=/usr/bin:/bin curl -q -sSfL --max-time 30 -o "$SCHEMA_TMP" "$URL" || exit 1
  got=$(shasum -a 256 "$SCHEMA_TMP" | cut -d' ' -f1)
  [ "$got" = "$SCHEMA_SHA" ] || { echo "premises: the fetched schema sha256 is $got, not the pinned $SCHEMA_SHA"; exit 1; }
  python3 -I -c 'import json, sys; json.load(open(sys.argv[1]))' "$SCHEMA_TMP" || exit 1
  mv "$SCHEMA_TMP" "$SCHEMA"
  { echo "source: $URL"; echo "retrieved: $(date +%Y-%m-%d)"; echo "sha256: $SCHEMA_SHA"
    echo "note: verbatim (not scrubbed); the pinned schema of the rust-v0.172.0 tag (K§6.3.7)."; } >"$SOURCE"
fi
got=$(shasum -a 256 "$SCHEMA" | cut -d' ' -f1)
[ "$got" = "$SCHEMA_SHA" ] || { echo "premises: config.schema.json sha256 is $got, not the pinned $SCHEMA_SHA"; exit 1; }
src=$(sed -n 's/^sha256: //p' "$SOURCE" 2>/dev/null) || src=""
[ "$src" = "$SCHEMA_SHA" ] || { echo "premises: config.schema.source.txt sha256 is '$src', not the pinned $SCHEMA_SHA"; exit 1; }
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
codex_record base
echo "codex base: ok (codex $CODEX_VERSION)"

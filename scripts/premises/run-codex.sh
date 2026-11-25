#!/bin/sh
# Runs the Codex premise parts (node PC-2): codex/<part>.sh, or every codex/*.sh in name order,
# and rewrites their fixtures under fixtures/codex/. A new part needs no edit here.
# Usage: sh scripts/premises/run-codex.sh [<part>]
# Needs: codex on PATH (at or above the pinned minimum, K-C8), python3, curl, shasum.
# N2_SCRATCH: as in lib.sh (set it to keep the captures). Stops at the first failed part, with its status.
set -eu
D=$(cd "$(dirname "$0")" && pwd)
if [ $# -gt 0 ]; then
  case "$1" in '' | *[!A-Za-z0-9_-]*) echo "run-codex: bad part name '$1'"; exit 1 ;; esac
  [ -f "$D/codex/$1.sh" ] || { echo "run-codex: no part $1"; exit 1; }
fi
PREMISES_DIR=$D
N2_AGENTS=codex
. "$D/lib.sh"
export N2_SCRATCH
if [ $# -gt 0 ]; then
  sh "$D/codex/$1.sh"
  exit 0
fi
for f in "$D"/codex/*.sh; do
  [ -e "$f" ] || continue
  echo "== codex/${f##*/}"
  sh "$f"
done

#!/bin/sh
# Runs every N2 experiment in order and rewrites the fixtures. Needs: claude and opencode on
# PATH (the pinned versions in docs/premises.md), python3, jq, tmux, curl, openssl.
# N2_SCRATCH: scratch directory for isolated HOMEs and raw captures (default: mktemp -d).
set -eu
D=$(cd "$(dirname "$0")" && pwd)
: "${N2_SCRATCH:=$(mktemp -d)}"
export N2_SCRATCH
for s in smoke P1 P1-interactive P2 P3 Q1 Q4 Q6 Q7-claude Q5 opencode P6; do
  echo "== run-$s.sh"
  sh "$D/run-$s.sh"
done

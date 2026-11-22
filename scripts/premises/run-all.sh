#!/bin/sh
# Runs every N2 experiment in order and rewrites the fixtures. Needs: claude and opencode on
# PATH (at or above the pinned versions in docs/premises.md), python3, jq, tmux, curl, openssl.
# N2_SCRATCH: scratch directory for isolated HOMEs and raw captures. Set it to keep the captures;
# unset, lib.sh first checks that claude and opencode are on PATH (a failed check makes no dir),
# then makes one, prints its path, removes it after exit 0 and keeps it otherwise.
# A TERM or HUP to this script alone takes effect after the current run-<id>.sh ends (a trap runs
# between commands); Ctrl-C, or kill -TERM -<process group id>, stops both at once.
set -eu
D=$(cd "$(dirname "$0")" && pwd)
. "$D/lib.sh"
export N2_SCRATCH
for s in smoke P1 P1-interactive P2 P3 Q1 Q4 Q6 Q7-claude Q5 opencode P6; do
  echo "== run-$s.sh"
  sh "$D/run-$s.sh"
done

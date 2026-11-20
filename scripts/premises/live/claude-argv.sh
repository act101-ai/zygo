#!/bin/sh
# Stand-in "claude" for live check A-20 (node N38): writes the argument after --agents (when
# present) to $LIVE_ARGV_OUT, then runs the real Claude Code ($LIVE_REAL_CLAUDE) with all arguments.
prev=
for a in "$@"; do
  [ "$prev" = --agents ] && printf '%s\n' "$a" >"$LIVE_ARGV_OUT"
  prev=$a
done
exec "$LIVE_REAL_CLAUDE" "$@"

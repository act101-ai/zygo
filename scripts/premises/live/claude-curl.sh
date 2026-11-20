#!/bin/sh
# Stand-in "claude" for live check N14-S (node N38): posts each request file in $LIVE_STRICT_DIR
# to zygo's proxy as the agent would, so the mock upstream records the strict-schema rewrites.
# The session token is read from its file and reaches curl on stdin, never in argv.
if [ "${1:-}" = --version ]; then
  echo "$LIVE_PIN_CC (Claude Code)"
  exit 0
fi
read -r T <"$ZYGO_SESSION_DIR/token"
SESSION_LINE=$(printf '%s\n' "$ANTHROPIC_CUSTOM_HEADERS" | grep -i '^X-Zygo-Session:')
for f in "$LIVE_STRICT_DIR"/*.json; do
  printf '%s: %s\n%s\n' "x-api""-key" "$T" "$SESSION_LINE" |
    curl -sS -o /dev/null -H @- -H 'content-type: application/json' --data-binary @"$f" "$ANTHROPIC_BASE_URL/v1/messages" || true
done
T=
exit 0

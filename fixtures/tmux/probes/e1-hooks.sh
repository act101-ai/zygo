#!/bin/sh
# e1-hooks.sh <tmux program>: which hook runs, and in which session, for each pane or window close of W§7.4.
#
# A private server (-S in a new folder of mode 0700, -f /dev/null) holds session `ws` with windows 0-3:
# window 0 and 1 one `sleep 600` pane each, window 2 two panes, window 3 two `sleep 600` panes.
# zygo's 8 hooks plus `window-pane-changed` are set on `ws` (session hooks with set-hook -t, window hooks with
# set-hook -w on each window); each hook appends one line `<hook> <session> w<window> name=<window name>` to a log.
# In each case except C, a second session `other` is made after `ws` with no client, so tmux takes `other` as current.
# Each case prints window 2 before and after the event, then the log 1.5 s after the event. The server is killed
# after each case; the folder is removed on exit.
set -u
T="$1"
D=$(mktemp -d "${TMPDIR:-/tmp}/zygo-e1-hooks.XXXXXX")
LOG="$D/log"
trap '"$T" -f /dev/null -S "$D/s" kill-server 2>/dev/null; rm -rf "$D"' EXIT
t() { "$T" -f /dev/null -S "$D/s" "$@"; }
hooks() {
  for e in after-new-window after-split-window client-detached after-kill-pane; do
    t set-hook -t =ws: "$e" "run-shell -b \"echo '$e #{session_name} w#{window_index} name=#{window_name}' >> $LOG\""
  done
  for w in 0 1 2 3; do
    for e in window-layout-changed window-renamed pane-exited window-unlinked window-pane-changed; do
      t set-hook -w -t "=ws:$w" "$e" "run-shell -b \"echo '$e #{session_name} w#{window_index} name=#{window_name}' >> $LOG\""
    done
  done
}
# setup <other: 1|0> <command of pane 2.1>
setup() {
  t new-session -d -s ws -x 200 -y 50 "sleep 600"
  t new-window -t =ws:1 "sleep 600"
  t new-window -t =ws:2 "sleep 600"
  t split-window -t =ws:2 "$2"
  t new-window -t =ws:3 "sleep 600"
  t split-window -t =ws:3 "sleep 600"
  t select-window -t =ws:0
  hooks
  if [ "$1" = 1 ]; then t new-session -d -s other -x 80 -y 24 "sleep 600"; fi
  sleep 1.5
  : > "$LOG"
  t list-panes -t =ws:2 -F 'before: w2 pane #{pane_index} #{pane_current_command} active=#{pane_active} name=#{window_name}'
}
show() {
  sleep 1.5
  echo "--- $1"
  t list-panes -t =ws:2 -F 'after: w2 pane #{pane_index} #{pane_current_command} active=#{pane_active} name=#{window_name}' 2>&1
  echo "log:"
  cat "$LOG"
  t kill-server 2>/dev/null
  sleep 0.3
  : > "$LOG"
}
"$T" -V
echo "script: e1-hooks.sh"
setup 1 "sleep 600"; t kill-pane -t =ws:2.0; show "A other current: kill-pane on 2.0 (not active)"
setup 1 "sleep 600"; t kill-pane -t =ws:2.1; show "B other current: kill-pane on 2.1 (active)"
setup 0 "sleep 600"; t kill-pane -t =ws:2.1; show "C ws only: kill-pane on 2.1 (active)"
setup 1 "sleep 600"; t kill-window -t =ws:3; show "D other current: kill-window on window 3"
setup 1 "sleep 4"; sleep 3; show "E other current: active pane 2.1 (sleep 4) exits"
setup 1 "sleep 4"; t select-pane -t =ws:2.0; sleep 3; show "F other current: pane 2.1 (sleep 4) exits after select-pane 2.0 (not active)"
setup 1 "sleep 600"; t select-pane -t =ws:2.0; show "G other current: select-pane 2.0 only (focus change)"
echo "end"

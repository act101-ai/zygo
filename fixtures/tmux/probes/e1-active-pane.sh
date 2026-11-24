#!/bin/sh
# e1-active-pane.sh <tmux program>: kill-pane by pane id (the form of W§18 #24 and #77) on a pane of window 2,
# with zygo's 8 hooks only, while a second session `other` (made after `ws`, no client) is tmux's current session.
#
# A private server (-S in a new folder of mode 0700, -f /dev/null) holds session `ws` with windows 0-2. Window 2 has
# pane 0 and pane 1; pane 0 is made active with select-pane (as a loadout build leaves it). Each hook appends one line
# `<hook> <session> w<window> name=<window name>` to a log.
# Modes: `same`: both panes of window 2 run `sh`. `diff`: the pane that is closed runs `sh` and the pane that stays
# runs `sleep 600`, so automatic-rename gives the window another name when the closed pane was the active one.
# Cases: pane 1 (not active) is closed, then on a new server pane 0 (active). After the kill and 1.5 s, the log; then
# `split-window -d` in window 0 (the next trigger) and 1.5 s, the log again (it holds the lines of the kill too).
# The server is killed after each case; the folder is removed on exit.
set -u
T="$1"
D=$(mktemp -d "${TMPDIR:-/tmp}/zygo-e1-active.XXXXXX")
LOG="$D/log"
trap '"$T" -f /dev/null -S "$D/s" kill-server 2>/dev/null; rm -rf "$D"' EXIT
t() { "$T" -f /dev/null -S "$D/s" "$@"; }
# setup <command of pane 2.0> <command of pane 2.1>
setup() {
  t new-session -d -s ws -x 200 -y 50 "sleep 600"
  t new-window -t =ws:1 "sleep 600"
  t new-window -t =ws:2 "$1"
  t split-window -t =ws:2 "$2"
  t select-pane -t =ws:2.0
  t select-window -t =ws:0
  for e in after-new-window after-split-window client-detached after-kill-pane; do
    t set-hook -t =ws: "$e" "run-shell -b \"echo '$e #{session_name} w#{window_index} name=#{window_name}' >> $LOG\""
  done
  for w in 0 1 2; do
    for e in window-layout-changed window-renamed pane-exited window-unlinked; do
      t set-hook -w -t "=ws:$w" "$e" "run-shell -b \"echo '$e #{session_name} w#{window_index} name=#{window_name}' >> $LOG\""
    done
  done
  t new-session -d -s other -x 80 -y 24 "sleep 600"
  sleep 1.5
  : > "$LOG"
}
"$T" -V
echo "script: e1-active-pane.sh"
for mode in same diff; do
  for idx in 1 0; do
    if [ "$mode" = same ]; then c0=sh; c1=sh
    elif [ "$idx" = 0 ]; then c0=sh; c1="sleep 600"
    else c0="sleep 600"; c1=sh
    fi
    setup "$c0" "$c1"
    t list-panes -t =ws:2 -F 'before: w2 pane #{pane_index} #{pane_current_command} active=#{pane_active} name=#{window_name}'
    id=$(t display-message -p -t "=ws:2.$idx" '#{pane_id}')
    t kill-pane -t "$id"
    sleep 1.5
    echo "--- $mode: kill-pane on pane $idx"
    t list-panes -t =ws:2 -F 'after: w2 pane #{pane_index} #{pane_current_command} active=#{pane_active} name=#{window_name}'
    echo "log:"
    cat "$LOG"
    t split-window -d -t =ws:0 "sleep 600"
    sleep 1.5
    echo "--- $mode: then split-window -d in window 0"
    echo "log:"
    cat "$LOG"
    t kill-server
    sleep 0.3
  done
done
echo "end"

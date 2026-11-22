#!/bin/sh
# Live premises PW6 and PW11 (node P1, workspaces spec W§17): the tmux commands, options, hooks,
# formats, sizes and "gone" answers that workspaces use (PW6), and a pane program that tmux starts
# from an argument list and that then execs $SHELL (PW11, a stand-in for `zygo ws run`).
# Usage: sh scripts/premises/live-tmux.sh
# Put the tmux to check first on PATH (the minimum is 3.2, AU7). Run by hand only.
#
# What this run does to the machine:
# - Every tmux command goes to a tmux server of its own: `-S <socket>` with the socket in a private
#   folder (mktemp -d, mode 0700) and `-f /dev/null`. The run never calls the default tmux server,
#   sets no global option and no global hook, and kills its own server on every exit path.
# - Pane programs are `sleep`, small /bin/sh programs and, for PW11, each installed shell (sh, bash,
#   zsh, fish) with HOME in the scratch folder, so no shell reads the operator's start files.
# - No agent starts and no network call is made. All files are in the scratch folder, which is
#   removed on exit 0. The only files written in the checkout are fixtures/tmux/pw6-<tmux version>.txt
#   and fixtures/tmux/pw11-<tmux version>.txt.
# - lib.sh needs `claude` and `opencode` on PATH; this script starts neither.
# Limit of PW11: the stand-in launcher sets no signal handling; the zygo launcher's SIGHUP and
# SIGTERM setup before exec (AU88) is not covered. E1 checks again on the real `zygo ws run`.
zp_stop_extra() { :; }
ZP_PRIV=$(mktemp -d "${TMPDIR:-/tmp}/zp1.XXXXXX")
trap 'rm -rf "$ZP_PRIV"' EXIT
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps
SOCK="$ZP_PRIV/s"
TMUX_BIN=""
tm() { LC_ALL=C "$TMUX_BIN" -S "$SOCK" -f /dev/null "$@"; }
# zp_end <status>: stop the sleepers (live-detach.sh), kill the private server, remove the private directory, then live_end.
zp_end() { trap - EXIT INT TERM HUP PIPE; zp_stop_extra; [ -z "$TMUX_BIN" ] || tm kill-server 2>/dev/null || true; rm -rf "$ZP_PRIV"; live_end "$1"; }
trap 'zp_end $?' EXIT; trap 'zp_end 129' HUP; trap 'zp_end 130' INT; trap 'zp_end 141' PIPE; trap 'zp_end 143' TERM
TMUX_BIN=$(command -v tmux) || { echo "live: tmux is not on PATH"; exit 1; }
TV=$("$TMUX_BIN" -V | sed -n 's/^tmux //p')
ver_ge "${TV%[a-z]}" 3.2 || { echo "live: tmux is ${TV:-unknown}; the minimum is 3.2 (AU7); put tmux 3.2 or later first on PATH"; exit 1; }
[ ${#SOCK} -le 100 ] || { echo "live: the socket path is longer than 100 characters; set TMPDIR to a short directory"; exit 1; }
echo "live: tmux $TV"
# wait_for <seconds> <command...>: poll every 0.1 s; 0 when the command succeeds in time, else 1.
wait_for() { _w=$(($1 * 10)); shift; while ! "$@" >/dev/null 2>&1; do _w=$((_w - 1)); [ "$_w" -gt 0 ] || return 1; sleep 0.1; done; }
# srv_stop: kill the private server, wait at most 5 s until it does not answer, remove the socket file.
srv_stop() { tm kill-server 2>/dev/null || true; _s=50; while tm list-sessions >/dev/null 2>&1 && [ "$_s" -gt 0 ]; do _s=$((_s - 1)); sleep 0.1; done; rm -f "$SOCK"; }
FX="$REPO_DIR/fixtures/tmux"; mkdir -p "$FX"

clean() { printf '%s' "$1" | sed "s#$SOCK#SOCKET#g; s#$ZP_PRIV#PRIV#g"; }
# sub <id> <0 = pass, other = fail> <detail>; note <id> <detail> (a note is not part of the verdict)
sub() { N=$((N + 1)); if [ "$2" -eq 0 ]; then printf '%s ok %s\n' "$1" "$(clean "$3")" >>"$OUT"; else BAD="$BAD $1"; printf '%s FAIL %s\n' "$1" "$(clean "$3")" >>"$OUT"; fi; }
note() { printf '%s note %s\n' "$1" "$(clean "$2")" >>"$OUT"; }
GONE="^(can't find session: .*|no server running on .*|error connecting to .* \\((No such file or directory|Connection refused)\\))\$"
gone() { printf '%s\n' "$1" | grep -Eq "$GONE"; }   # the AU87 answers
# t <tmux arguments>: one tmux call whose failure is a result. Sets RC (exit status), O (stdout), E (stderr).
t() { RC=0; O=$(tm "$@" 2>"$RUN/t.err") || RC=$?; E=$(cat "$RUN/t.err"); }
# one <text>: the text on one line (line ends become " | ").
one() { printf '%s' "$1" | tr '\n' '|' | sed 's/|/ | /g'; }
size() { tm display-message -p -t "$1" '#{window_width} #{window_height}' 2>/dev/null || true; }
now() { perl -MTime::HiRes=time -e 'printf "%.3f\n", time'; }

live_run tmux
S=0123456789abcdef
K=kabcdefgh

# ---------------------------------------------------------------------------------------------
# PW6
# ---------------------------------------------------------------------------------------------
OUT="$RUN/pw6.txt"; N=0; BAD=""
echo "tmux $TV" >"$OUT"

# Group A: server and "gone" answers (AU62, AU87, W§8.1 step 9).
srv_stop
p=1; ver_ge "${TV%[a-z]}" 3.2 && p=0
sub 6.01 $p "tmux $TV"

t has-session -t =zp6
p=1; if [ "$RC" -eq 1 ] && gone "$E"; then p=0; fi
sub 6.02 $p "no server: exit $RC; $E"

cat >"$RUN/argv.sh" <<'EOF'
printf '%s\n' "$#" "$@" >"$(dirname "$0")/argv.txt"; exec sleep 600
EOF
rm -f "$R/pwned" "$RUN/argv.txt"
t new-session -d -s zp6 -x 173 -y 47 -c "$R" -- /bin/sh "$RUN/argv.sh" 'a b;$(touch pwned) "q" *' '#{pane_id}' "it's"
w=0; wait_for 5 test -s "$RUN/argv.txt" || w=1
printf '%s\n' 3 'a b;$(touch pwned) "q" *' '#{pane_id}' "it's" >"$RUN/argv.exp"
a=bad; if [ "$w" -eq 0 ] && cmp -s "$RUN/argv.txt" "$RUN/argv.exp"; then a=ok; fi
[ "$w" -eq 0 ] || a="bad (timeout 5 s)"
pw=no; [ ! -e "$R/pwned" ] || pw=yes
p=1; if [ "$RC" -eq 0 ] && [ "$a" = ok ] && [ "$pw" = no ]; then p=0; fi
sub 6.03 $p "new-session with an argument list: exit $RC; arguments unchanged: $a; file pwned: $pw"

t has-session -t =zp6; r1=$RC
r2=0; (TMUX=/nonexistent,1,0 TMUX_TMPDIR=/nonexistent; export TMUX TMUX_TMPDIR; tm has-session -t =zp6) 2>/dev/null || r2=$?
p=1; if [ "$r1" -eq 0 ] && [ "$r2" -eq 0 ]; then p=0; fi
sub 6.04 $p "has-session: exit $r1; with TMUX and TMUX_TMPDIR set to other values: exit $r2"

t display-message -p -t =zp6: '#{socket_path}'
p=1; if [ "$RC" -eq 0 ] && [ "$O" = "$SOCK" ]; then p=0; fi
sub 6.05 $p "socket_path: exit $RC; $O"

t has-session -t =nosuch
p=1; if [ "$RC" -eq 1 ] && [ "$E" = "can't find session: nosuch" ]; then p=0; fi
sub 6.06 $p "missing session: exit $RC; $E"

t list-panes -s -t =nosuch: -F x
p=1; if [ "$RC" -eq 1 ] && gone "$E"; then p=0; fi
sub 6.07 $p "list-panes -s -t =nosuch: exit $RC; $E"

t list-panes -s -t =nosuch -F x; e1=$E
t show-options -v -t =nosuch: @zygo-ws; e2=$E
note 6.08 "list-panes -s -t =nosuch (no colon): $e1; show-options -v -t =nosuch: @zygo-ws: $e2"

t display-message -p -t =zp6: '#{pid}'; pid=$O
case "$pid" in
  '' | *[!0-9]*) sub 6.09 1 "the server gave no process id" ;;
  *)
    kill -KILL "$pid" 2>/dev/null || true
    w=yes; wait_for 5 sh -c "! kill -0 $pid" || w="no (timeout 5 s)"
    t has-session -t =zp6
    p=1; if [ "$w" = yes ] && [ "$RC" -eq 1 ] && gone "$E"; then p=0; fi
    sub 6.09 $p "server killed, socket file left: $w; exit $RC; $E" ;;
esac
rm -f "$SOCK"

# Group B: sizes and layouts (AU96, W§8.1 step 11).
srv_stop
t new-session -d -s zp6 -x 173 -y 47 -- sleep 600
s=$(size =zp6:0)
p=1; if [ "$s" = "173 47" ]; then p=0; fi
sub 6.11 $p "first window after new-session -d -x 173 -y 47: $s"

t set-option -t =zp6: default-size 91x33; r1=$RC
t new-window -d -P -F '#{window_index}' -t =zp6: -- sleep 600; r2=$RC; w=$O
s=$(size "=zp6:$w")
p=1; if [ "$r1" -eq 0 ] && [ "$r2" -eq 0 ] && [ "$s" = "91 33" ]; then p=0; fi
sub 6.12 $p "default-size 91x33, then new-window: exit $r1 $r2; $s"

t resize-window -t =zp6:0 -x 173 -y 47
note 6.13 "resize-window -t =zp6:0 -x 173 -y 47: exit $RC; $(size =zp6:0)"

geo() { tm list-panes -t "$1" -F '#{pane_width}x#{pane_height},#{pane_left},#{pane_top}' 2>/dev/null || true; }
t set-option -t =zp6: default-size 173x47
t new-window -d -P -F '#{window_index}' -t =zp6: -- sleep 600; a=$O
t split-window -d -h -t "=zp6:$a" -- sleep 600
t split-window -d -v -t "=zp6:$a.0" -- sleep 600
t display-message -p -t "=zp6:$a" '#{window_layout}'; L=$O
G1=$(geo "=zp6:$a")
t new-window -d -P -F '#{window_index}' -t =zp6: -- sleep 600; b=$O
t split-window -d -t "=zp6:$b" -- sleep 600
t split-window -d -t "=zp6:$b" -- sleep 600
t select-layout -t "=zp6:$b" "$L"
G2=$(geo "=zp6:$b")
p=1; if [ "$RC" -eq 0 ] && [ -n "$G1" ] && [ "$G1" = "$G2" ]; then p=0; fi
sub 6.14 $p "select-layout <saved layout of 3 panes>: exit $RC; saved panes: $(one "$G1"); built panes: $(one "$G2")"

t new-window -d -P -F '#{window_index}' -t =zp6: -- sleep 600; c=$O
t select-layout -t "=zp6:$c" tiled
p=1; if [ "$RC" -eq 0 ]; then p=0; fi
sub 6.15 $p "select-layout tiled on 1 pane: exit $RC"
t select-layout -t "=zp6:$c" "$L"
note 6.15n "select-layout <layout of 3 panes> on a window of 1 pane: exit $RC"

t set-option -t =zp6: default-size 20x6
t new-window -d -P -F '#{window_index}' -t =zp6: -- sleep 600; d=$O
i=0; p=1; m="no split failed in 10 calls"
while [ "$i" -lt 10 ]; do
  i=$((i + 1))
  t split-window -d -t "=zp6:$d" -- sleep 600
  if [ "$RC" -ne 0 ]; then
    m="exit $RC; $E"
    case "$E" in *"no space for"*) p=0 ;; esac
    break
  fi
done
sub 6.16 $p "split-window in a window of 20x6 until one fails: $m"

# Group C: options, formats, environment (W§7.4 step 2, W§8.1 steps 10-12, W§8.4, AU24, AU55, AU63,
# AU64, AU68, AU99).
srv_stop
rm -rf "$RUN/d#x"; mkdir "$RUN/d#x"; rm -f "$R/pwned" "$R/env.txt"
(ZP6_HIDE=1 ZP6_KEEP=1; export ZP6_HIDE ZP6_KEEP; tm new-session -d -s zp6 -x 173 -y 47 -c "$R" -- sleep 600) || true
t new-session -d -s zp6b -- sleep 600
t display-message -p -t =zp6:0.0 '#{pane_id}'; p0=$O

t set-option -t =zp6: @zygo-ws "$S"; r1=$RC
t show-options -v -t =zp6: @zygo-ws; r2=$RC; v1=$O
t show-options -v -t =zp6b: @zygo-ws; v2=$O
p=1; if [ "$r1" -eq 0 ] && [ "$r2" -eq 0 ] && [ "$v1" = "$S" ] && [ -z "$v2" ]; then p=0; fi
sub 6.17 $p "@zygo-ws with -t =zp6: exit $r1 $r2; read: $v1; read on another session: ${v2:-empty}"

t set-option -t =zp6 @zygo-ws x
note 6.18 "set-option -t =zp6 (no colon): exit $RC; $E"

t set-option -p -t "$p0" @zygo-pane "$K"; r1=$RC
t show-options -p -v -t "$p0" @zygo-pane; r2=$RC; v1=$O
t split-window -d -P -F '#{pane_id}' -t =zp6:0 -- sleep 600; p1=$O
t show-options -p -v -t "$p1" @zygo-pane; v2=$O
p=1; if [ "$r1" -eq 0 ] && [ "$r2" -eq 0 ] && [ "$v1" = "$K" ] && [ -n "$p1" ] && [ -z "$v2" ]; then p=0; fi
sub 6.19 $p "@zygo-pane with set-option -p: exit $r1 $r2; read: $v1; read on a new pane: ${v2:-empty}"

t display-message -p -t "$p0" '#{session_name} #{@zygo-ws}'
p=1; if [ "$RC" -eq 0 ] && [ "$O" = "zp6 $S" ]; then p=0; fi
sub 6.20 $p "display-message -p -t <pane> '#{session_name} #{@zygo-ws}': exit $RC; $O"

t set-option -t =zp6: destroy-unattached off; r1=$RC
t show-options -v -t =zp6: destroy-unattached
p=1; if [ "$r1" -eq 0 ] && [ "$RC" -eq 0 ] && [ "$O" = off ]; then p=0; fi
sub 6.21 $p "destroy-unattached off: exit $r1 $RC; read: $O"

t new-window -d -P -F '#{window_index}' -t =zp6: -- sleep 600; w22=$O
t set-option -w -t "=zp6:$w22" remain-on-exit off; r1=$RC
t show-options -w -v -t "=zp6:$w22" remain-on-exit
p=1; if [ "$r1" -eq 0 ] && [ "$RC" -eq 0 ] && [ "$O" = off ]; then p=0; fi
sub 6.22 $p "remain-on-exit off with set-option -w: exit $r1 $RC; read: $O"

t set-option -t =zp6: remain-on-exit off; r1=$RC
t show-options -w -t =zp6:0 remain-on-exit; v1=$O
t new-window -d -P -F '#{window_index}' -t =zp6: -- sleep 600; x=$O
t show-options -w -t "=zp6:$x" remain-on-exit; v2=$O
note 6.22n "set-option -t =zp6: remain-on-exit off (no -w): exit $r1; window 0: ${v1:-empty}; a later window: ${v2:-empty}"

wname() { tm display-message -p -t "=zp6:$1" '#{window_name}' 2>/dev/null || true; }
t new-window -d -P -F '#{window_index}' -t =zp6: -n 'a##b' -- sleep 600; wa=$O
t new-window -d -P -F '#{window_index}' -t =zp6: -n 'w##(touch pwned)' -c "$R" -- sleep 600; wb=$O
t new-window -d -P -F '#{window_index}' -t =zp6: -n '#{session_name}' -- sleep 600; wc=$O
n1=$(wname "$wa"); n2=$(wname "$wb"); n3=$(wname "$wc")
sleep 0.5
pw=no; [ ! -e "$R/pwned" ] || pw=yes
p=1; if [ "$n1" = 'a#b' ] && [ "$n2" = 'w#(touch pwned)' ] && [ "$n3" = zp6 ] && [ "$pw" = no ]; then p=0; fi
sub 6.26 $p "new-window -n 'a##b', 'w##(touch pwned)', '#{session_name}': names $n1, $n2, $n3; file pwned: $pw"

DX=$(cd "$RUN/d#x" && pwd -P)
cwd_is() { [ "$(tm display-message -p -t "$1" '#{pane_current_path}' 2>/dev/null)" = "$DX" ]; }
t new-window -d -P -F '#{pane_id} #{window_index}' -t =zp6: -c "$RUN/d##x" -- sleep 600; q1=${O%% *}; w27=${O##* }
t split-window -d -P -F '#{pane_id}' -t "=zp6:$w27" -c "$RUN/d##x" -- sleep 600; q2=$O
t new-session -d -s zp6c -c "$RUN/d##x" -- sleep 600
c1=no; c2=no; c3=no
if [ -n "$q1" ] && wait_for 5 cwd_is "$q1"; then c1=yes; fi
if [ -n "$q2" ] && wait_for 5 cwd_is "$q2"; then c2=yes; fi
if wait_for 5 cwd_is =zp6c:; then c3=yes; fi
p=1; if [ "$c1" = yes ] && [ "$c2" = yes ] && [ "$c3" = yes ]; then p=0; fi
sub 6.27 $p "-c <dir>/d##x gives the directory d#x: new-window $c1; split-window $c2; new-session $c3 (a no is a timeout 5 s)"

# The field separator is "|": tmux 3.2a and 3.7c print a tab of a -F format as "_" (6.23t records it),
# so a tab cannot separate the fields. No name or path of this group holds a "|".
TAB=$(printf '\t')
t display-message -p -t =zp6:0 "a${TAB}b"
note 6.23t "a tab in a format: display-message -p 'a<tab>b' prints: $(printf '%s' "$O" | sed "s/$TAB/<tab>/g")"
FMT="#{window_index}|#{window_name}|#{automatic-rename}|#{window_layout}|#{window_width}|#{window_height}|#{window_active}|#{window_zoomed_flag}|#{pane_index}|#{pane_active}|#{@zygo-pane}|#{pane_current_command}|#{pane_current_path}"
t list-panes -s -t =zp6: -F "$FMT"
printf '%s\n' "$O" >"$RUN/panes.txt"
cnt() { awk -F'|' "$@" "$RUN/panes.txt" | wc -l | tr -d ' '; }
f1=$(cnt 'NF != 13')
f2=$(cnt -v k="$K" '$11 == k')
f3=$(cnt '$2 == "a#b" && $3 == "0"'); f3b=$(cnt '$2 == "a#b" && $3 != "0"')
f4=$(cnt '$1 == "0" && $3 == "1"'); f4b=$(cnt '$1 == "0" && $3 != "1"')
f5=$(cnt '$12 != "sleep"')
f6=$(cnt -v w="$w27" -v d="$DX" '$1 == w && $13 == d')
p=1
if [ "$RC" -eq 0 ] && [ "$f1" -eq 0 ] && [ "$f2" -eq 1 ] && [ "$f3" -ge 1 ] && [ "$f3b" -eq 0 ] &&
  [ "$f4" -ge 1 ] && [ "$f4b" -eq 0 ] && [ "$f5" -eq 0 ] && [ "$f6" -ge 1 ]; then p=0; fi
sub 6.23 $p "list-panes -s -t =zp6: -F <13 formats joined by |>: exit $RC; lines without 13 fields: $f1; lines with @zygo-pane $K: $f2; automatic-rename 0 on a#b: $f3 (other: $f3b); automatic-rename 1 on window 0: $f4 (other: $f4b); lines with a command that is not sleep: $f5; lines of the d#x window with its path: $f6"

t list-panes -s -t =zp6 -F x
note 6.23n "list-panes -s -t =zp6 (no colon): exit $RC"

t resize-pane -Z -t "$p0"; r1=$RC
t display-message -p -t "$p0" '#{window_zoomed_flag}'; z=$O
t resize-pane -Z -t "$p0"
p=1; if [ "$r1" -eq 0 ] && [ "$z" = 1 ]; then p=0; fi
sub 6.24 $p "resize-pane -Z: exit $r1; window_zoomed_flag: $z"

t select-window -t "=zp6:$w22"; r1=$RC
t display-message -p -t "=zp6:$w22" '#{window_active}'; v1=$O
t select-window -t =zp6:0
t select-pane -t "$p1"; r2=$RC
t display-message -p -t "$p1" '#{pane_active}'; v2=$O
p=1; if [ "$r1" -eq 0 ] && [ "$r2" -eq 0 ] && [ "$v1" = 1 ] && [ "$v2" = 1 ]; then p=0; fi
sub 6.25 $p "select-window, select-pane: exit $r1 $r2; window_active: $v1; pane_active: $v2"

t set-environment -t =zp6 -r ZP6_HIDE; r1=$RC
t set-environment -t =zp6 ZYGO_WORKSPACE "$S"; r2=$RC
t show-environment -t =zp6; r3=$RC; se=$O
t new-window -d -t =zp6: -c "$R" -- /bin/sh -c 'env | grep -E "^(ZP6_|ZYGO_WORKSPACE=|TMUX)" >env.txt; exec sleep 600'; r4=$RC
w=yes; wait_for 5 test -s "$R/env.txt" || w="no (timeout 5 s)"
[ -e "$R/env.txt" ] || : >"$R/env.txt"
e1=no; e2=no; e3=no; e4=no; e5=no; e6=no
if printf '%s\n' "$se" | grep -qx -- '-ZP6_HIDE'; then e1=yes; fi
if grep -q '^ZP6_HIDE=' "$R/env.txt"; then e2=yes; fi
if grep -qx 'ZP6_KEEP=1' "$R/env.txt"; then e3=yes; fi
if grep -qx "ZYGO_WORKSPACE=$S" "$R/env.txt"; then e4=yes; fi
if grep -qF "TMUX=$SOCK," "$R/env.txt" && grep -q '^TMUX=' "$R/env.txt"; then e5=yes; fi
if grep -q '^TMUX_PANE=%' "$R/env.txt"; then e6=yes; fi
p=1
if [ "$r1" -eq 0 ] && [ "$r2" -eq 0 ] && [ "$r3" -eq 0 ] && [ "$r4" -eq 0 ] && [ "$w" = yes ] && [ "$e1" = yes ] &&
  [ "$e2" = no ] && [ "$e3" = yes ] && [ "$e4" = yes ] && [ "$e5" = yes ] && [ "$e6" = yes ]; then p=0; fi
sub 6.28 $p "set-environment -r, set-environment, show-environment, new-window: exit $r1 $r2 $r3 $r4; pane wrote its environment: $w; show-environment has -ZP6_HIDE: $e1; in the pane: ZP6_HIDE $e2; ZP6_KEEP=1 $e3; ZYGO_WORKSPACE $e4; TMUX starts with the socket path $e5; TMUX_PANE $e6"

# Group D: hooks (W§7.4 triggers, AU54, AU90, W-C3).
srv_stop
mkdir -p "$RUN/h#x"
cat >"$RUN/h#x/hook.sh" <<'EOF'
#!/bin/sh
perl -MTime::HiRes=time -e 'printf "%.3f %s\n", time, join(" ", @ARGV)' "$@" >>"$(dirname "$0")/../hooks.log"
EOF
chmod 0755 "$RUN/h#x/hook.sh"
HP=$(printf '%s' "$RUN/h#x/hook.sh" | sed 's/#/##/g')
hookstr() { printf 'run-shell -b "'"'"'%s'"'"' ws snapshot --ws-id 0123456789abcdef --event %s"' "$HP" "$1"; }
SESSION_HOOKS="after-new-window after-split-window after-kill-pane client-detached"
WINDOW_HOOKS="pane-exited window-unlinked window-layout-changed window-renamed"
sesshooks() { for h in $SESSION_HOOKS; do tm set-hook -t "$1" "$h" "$(hookstr "$h")" || return 1; done; }
winhooks() { for h in $WINDOW_HOOKS; do tm set-hook -w -t "$1" "$h" "$(hookstr "$h")" || return 1; done; }
fired() { wait_for "${2:-3}" grep -q -- "--event $1\$" "$RUN/hooks.log"; }   # fired <hook> [seconds]
# quiet: let the hook programs of the event before end (0.3 s), then empty the log.
quiet() { sleep 0.3; : >"$RUN/hooks.log"; }
yn() { if "$@"; then echo yes; else echo no; fi; }
# client <session>: a tmux client on a pty of its own; its process id goes to LIVE_PIDS and CPID.
client() {
  python3 -I -c 'import os, pty, sys
pid, fd = pty.fork()
if pid == 0:
    os.execvp(sys.argv[1], sys.argv[1:])
while True:
    try:
        if not os.read(fd, 65536):
            break
    except OSError:
        break
os.waitpid(pid, 0)' env -u TMUX LC_ALL=C TERM=xterm-256color "$TMUX_BIN" -S "$SOCK" -f /dev/null attach-session -t "$1" >/dev/null 2>&1 &
  CPID=$!; LIVE_PIDS="$LIVE_PIDS $CPID"
}
# attached <session>: wait at most 5 s until a client of the private server is on that session.
attached() { wait_for 5 sh -c 'LC_ALL=C "$0" -S "$1" -f /dev/null list-clients -F "#{client_session}" | grep -qx "$2"' "$TMUX_BIN" "$SOCK" "$1"; }
# cend <pid>: the client process ends at the detach; one that is still there after 5 s is killed.
cend() { wait_for 5 sh -c "! kill -0 $1" || kill "$1" 2>/dev/null || true; wait "$1" 2>/dev/null || true; }
hooklines() { printf '%s\n' "$1" | grep -c . || true; }
# hooksok <list> <hook names>: 0 when the list has one line for each name, with --event <that name>.
hooksok() { _l=$1; shift; for h in "$@"; do printf '%s\n' "$_l" | grep -Eq -- "^$h\\[[0-9]+\\] .*--event $h([^a-z-]|\$)" || return 1; done; }

t new-session -d -s zp6 -x 173 -y 47 -c "$R" -- sleep 600
t new-session -d -s zp6b -- sleep 600
G0=$(tm show-hooks -g; tm show-options -g; tm show-options -g -w; tm show-options -s) || true
: >"$RUN/hooks.log"

r1=0; sesshooks =zp6: 2>"$RUN/t.err" || r1=1
r2=0; winhooks =zp6:0 2>>"$RUN/t.err" || r2=1
t show-hooks -t =zp6:; r3=$RC; hs=$O
t show-hooks -w -t =zp6:0; r4=$RC; hw=$O
n1=$(hooklines "$hs"); n2=$(hooklines "$hw")
p=1
# shellcheck disable=SC2086
if [ "$r1" -eq 0 ] && [ "$r2" -eq 0 ] && [ "$r3" -eq 0 ] && [ "$r4" -eq 0 ] && [ "$n1" -eq 4 ] && [ "$n2" -eq 4 ] &&
  hooksok "$hs" $SESSION_HOOKS && hooksok "$hw" $WINDOW_HOOKS; then p=0; fi
sub 6.30 $p "set-hook -t =zp6: (4 hooks), set-hook -w -t =zp6:0 (4 hooks): exit $r1 $r2 $r3 $r4; show-hooks lines: $n1; show-hooks -w lines: $n2"

quiet
t split-window -d -t =zp6:0 -- sleep 600
a=$(yn fired after-split-window); b=$(yn fired window-layout-changed)
p=1; if [ "$a" = yes ] && [ "$b" = yes ]; then p=0; fi
sub 6.31 $p "split-window in window 0: after-split-window $a; window-layout-changed $b"

quiet
t kill-pane -t =zp6:0.1
a=$(yn fired after-kill-pane)
p=1; if [ "$a" = yes ]; then p=0; fi
sub 6.32 $p "kill-pane in window 0: after-kill-pane $a"

quiet
t new-window -d -t =zp6: -- sleep 600
a=$(yn fired after-new-window)
r1=0; winhooks =zp6:1 2>/dev/null || r1=1
p=1; if [ "$a" = yes ] && [ "$r1" -eq 0 ]; then p=0; fi
sub 6.33 $p "new-window (window 1): after-new-window $a; set-hook -w on the new window: exit $r1"

quiet
t split-window -d -t =zp6:1 -- sleep 600
a=$(yn fired after-split-window); b=$(yn fired window-layout-changed)
quiet
t kill-pane -t =zp6:1.1
c=$(yn fired after-kill-pane)
p=1; if [ "$a" = yes ] && [ "$b" = yes ] && [ "$c" = yes ]; then p=0; fi
sub 6.34 $p "in window 1: after-split-window $a; window-layout-changed $b; after-kill-pane $c"

quiet
t rename-window -t =zp6:0 r0
a=$(yn fired window-renamed)
quiet
t rename-window -t =zp6:1 r1
b=$(yn fired window-renamed)
p=1; if [ "$a" = yes ] && [ "$b" = yes ]; then p=0; fi
sub 6.35 $p "rename-window: window-renamed in window 0 $a; in window 1 $b"

quiet
t split-window -d -t =zp6:1 -- /bin/sh -c 'sleep 1'
a=$(yn fired pane-exited 5)
p=1; if [ "$a" = yes ]; then p=0; fi
sub 6.36 $p "a pane whose program ends: pane-exited $a"

t split-window -d -t =zp6:0 -- sleep 600
sleep 1
quiet
t select-layout -t =zp6:0 even-horizontal
a=$(yn fired window-layout-changed)
p=1; if [ "$a" = yes ]; then p=0; fi
sub 6.37 $p "select-layout even-horizontal: window-layout-changed $a"

quiet
t kill-window -t =zp6:1
a=$(yn fired window-unlinked)
p=1; if [ "$a" = yes ]; then p=0; fi
sub 6.38 $p "kill-window: window-unlinked $a"
sleep 1
a=no; if grep -q -- '--event after-kill-pane$' "$RUN/hooks.log"; then a=yes; fi
note 6.38n "kill-window: an after-kill-pane line within 1 s: $a"

quiet
t split-window -d -t =zp6b:0 -- sleep 600
t rename-window -t =zp6b:0 other
sleep 1.5
n=$(grep -c . "$RUN/hooks.log" || true)
p=1; if [ "$n" -eq 0 ]; then p=0; fi
sub 6.43 $p "split-window and rename-window in another session: hook lines: $n"

quiet
t split-window -d -t =zp6b:0 -- /bin/sh -c 'sleep 1'
sleep 3
t new-window -d -P -F '#{window_index}' -t =zp6b: -- sleep 600; x=$O
t kill-window -t "=zp6b:$x"
sleep 1.5
names=$(sed -n 's/.*--event //p' "$RUN/hooks.log" | sort -u | tr '\n' ' ' | sed 's/ $//')
note 6.44 "hooks of zp6 that fired for a pane that ended and a window that was killed in another session: ${names:-none}"

client =zp6; c1=$CPID
a=no; if attached zp6; then a=yes; fi
quiet
t detach-client -s =zp6
b=$(yn fired client-detached)
cend "$c1"
p=1; if [ "$a" = yes ] && [ "$b" = yes ]; then p=0; fi
sub 6.39 $p "attach-session -t =zp6 on a pty: attached $a; detach-client: client-detached $b"

client =zp6; c1=$CPID
a=no; if attached zp6; then a=yes; fi
client =zp6b; c2=$CPID
b=no; if attached zp6b; then b=yes; fi
quiet
t detach-client -s =zp6
sleep 1.5
c=no; if grep -q -- '--event client-detached$' "$RUN/hooks.log"; then c=yes; fi
t detach-client -s =zp6b
cend "$c1"; cend "$c2"
note 6.40 "two clients (attached $a, $b), the second on another session; detach-client -s =zp6: a client-detached line within 1.5 s: $c"

client =zp6; c1=$CPID
a=no; if attached zp6; then a=yes; fi
t list-clients -F '#{client_tty}'; c=$O
t switch-client -c "$c" -t =zp6b; r1=$RC
t list-clients -F '#{client_session}'; cs=$O
t detach-client -s =zp6b
cend "$c1"
p=1; if [ "$a" = yes ] && [ "$r1" -eq 0 ] && [ "$cs" = zp6b ]; then p=0; fi
sub 6.41 $p "switch-client -c <client> -t =zp6b: attached $a; exit $r1; client session: $cs"

# 6.46 and 6.47 (notes): the three hooks whose target is gone when the hook runs (after-kill-pane,
# pane-exited, window-unlinked) are looked up in the session that tmux takes as the current one
# (tmux 3.2a notify.c:69-70, cmd-queue.c:618-620): the session with a client, else the newest one.
# dead3: one kill-pane, one pane whose program ends and one kill-window in zp6; prints the result.
dead3() {
  t split-window -d -t =zp6:0 -- sleep 600
  sleep 0.5; quiet
  t kill-pane -t =zp6:0.1
  d1=$(yn fired after-kill-pane 2)
  quiet
  t split-window -d -t =zp6:0 -- /bin/sh -c 'sleep 1'
  d2=$(yn fired pane-exited 4)
  t new-window -d -P -F '#{window_index}' -t =zp6: -- sleep 600; x=$O
  winhooks "=zp6:$x" 2>/dev/null || true
  sleep 0.5; quiet
  t kill-window -t "=zp6:$x"
  d3=$(yn fired window-unlinked 2)
  printf 'after-kill-pane %s; pane-exited %s; window-unlinked %s' "$d1" "$d2" "$d3"
}
client =zp6; c1=$CPID
a=no; if attached zp6; then a=yes; fi
r=$(dead3)
quiet
t split-window -d -t =zp6b:0 -- /bin/sh -c 'sleep 1'
b=$(yn fired pane-exited 4)
note 6.46 "one client, on zp6 (attached $a): events in zp6: $r; a pane that ends in another session runs the pane-exited hook of zp6: $b"
t list-clients -F '#{client_tty}'; c=$O
t switch-client -c "$c" -t =zp6b
t list-clients -F '#{client_session}'; cs=$O
r=$(dead3)
note 6.47 "the client is on another session (client session: $cs): events in zp6: $r"
t detach-client -s =zp6b
cend "$c1"

t new-session -d -s zp6d -- sleep 600
t new-window -d -t =zp6d: -- sleep 600
t set-hook -t =zp6d: window-renamed "$(hookstr window-renamed)"; r1=$RC
t show-hooks -w -t =zp6d:0; n1=$(hooklines "$O")
t show-hooks -w -t =zp6d:1; n2=$(hooklines "$O")
note 6.42 "set-hook -t =zp6d: window-renamed (no -w): exit $r1; show-hooks -w lines of window 0: $n1; of window 1: $n2"

G1=$(tm show-hooks -g; tm show-options -g; tm show-options -g -w; tm show-options -s) || true
p=1; if [ -n "$G0" ] && [ "$G0" = "$G1" ]; then p=0; fi
sub 6.45 $p "global hooks, global session options, global window options and server options are the same after the group: $(yn test "$p" -eq 0)"

# Group E: teardown (AU89, W§9.3 step 11).
# teardown <id> <text>: the session zp6 (2 windows, 3 panes, all hooks) is killed; the pass test of 6.50 and 6.51.
teardown() {
  t new-session -d -s zp6 -x 173 -y 47 -- sleep 600
  t split-window -d -t =zp6:0 -- sleep 600
  t new-window -d -t =zp6: -- sleep 600
  sesshooks =zp6: 2>/dev/null || true
  winhooks =zp6:0 2>/dev/null || true
  winhooks =zp6:1 2>/dev/null || true
  sleep 0.5
  : >"$RUN/hooks.log"
  t kill-session -t =zp6; r1=$RC
  g=yes; wait_for 5 sh -c '! LC_ALL=C "$0" -S "$1" -f /dev/null has-session -t =zp6' "$TMUX_BIN" "$SOCK" || g="no (timeout 5 s)"
  TG=$(now)
  sleep 3
  T1=$(sed -n 's/^\([0-9.]*\) .*--event pane-exited$/\1/p' "$RUN/hooks.log" | head -n 1)
  n=$(grep -c -- '--event pane-exited$' "$RUN/hooks.log" || true)
  in2=none; ok2=0
  if [ -n "$T1" ]; then
    if awk "BEGIN { exit !($TG - $T1 <= 2.0) }"; then in2=yes; else in2=no; ok2=1; fi
  fi
  t has-session -t =zp6
}
srv_stop
t new-session -d -s keep -- sleep 600
teardown
p=1; if [ "$r1" -eq 0 ] && [ "$g" = yes ] && [ "$ok2" -eq 0 ] && [ "$E" = "can't find session: zp6" ]; then p=0; fi
sub 6.50 $p "kill-session with all hooks set, another session left: exit $r1; gone within 5 s: $g; pane-exited lines: $n; within 2 s of the first: $in2; then has-session: $E"

srv_stop
teardown
p=1; if [ "$r1" -eq 0 ] && [ "$g" = yes ] && [ "$ok2" -eq 0 ] && gone "$E"; then p=0; fi
sub 6.51 $p "kill-session with all hooks set, the last session: exit $r1; gone within 5 s: $g; pane-exited lines: $n; within 2 s of the first: $in2; then has-session: $E"

LIVE_VER="tmux $TV"
scrub "$RUN/pw6.txt" >"$FX/pw6-$TV.txt"
if [ -z "$BAD" ]; then result PW6 verified "tmux $TV: $N of $N sub-checks passed; fixtures/tmux/pw6-$TV.txt"
else result PW6 failed "tmux $TV: $((N - $(set -- $BAD; echo $#))) of $N sub-checks passed; failed:$BAD; fixtures/tmux/pw6-$TV.txt"; fi

# ---------------------------------------------------------------------------------------------
# PW11
# ---------------------------------------------------------------------------------------------
OUT="$RUN/pw11.txt"; N=0; BAD=""
: >"$OUT"
cat >"$RUN/launcher.sh" <<'EOF'
#!/bin/sh
printf '%s\n' "$$" "$#" "$@" >"$(dirname "$0")/launcher.txt"
exec "${SHELL:-/bin/sh}"
EOF
printf '%s\n' 7 ws run --ws-id 0123456789abcdef --pane kabcdefgh 'a b;$(touch pwned) *' >"$RUN/launcher.exp"
SHELLS=""
for x in "${SHELL:-/bin/sh}" /bin/sh "$(command -v bash || true)" "$(command -v zsh || true)" "$(command -v fish || true)"; do
  [ -n "$x" ] || continue
  case " $SHELLS " in *" $x "*) continue ;; esac
  SHELLS="$SHELLS $x"
done
: >"$H/.zshrc"
ENVP=$(command -v env)
pane_cmd_is() { [ "$(tm display-message -p -t "$1" '#{pane_current_command}' 2>/dev/null)" = "$2" ]; }
pane_gone() { ! tm list-panes -t =zp11:0 -F '#{pane_id}' 2>/dev/null | grep -qx -- "$1"; }
# typed <pane> <text>: type the text into the pane, then Enter.
typed() { tm send-keys -t "$1" -l "$2" 2>/dev/null || true; sleep 0.3; tm send-keys -t "$1" Enter 2>/dev/null || true; }

for X in $SHELLS; do
  B=$(basename "$X")
  srv_stop
  rm -rf "$RUN/cwd"; mkdir "$RUN/cwd"; rm -f "$RUN/launcher.txt"
  t new-session -d -s zp11 -x 120 -y 30 -c "$RUN/cwd" -- sleep 600
  t split-window -d -P -F '#{pane_id}' -t =zp11:0 -c "$RUN/cwd" -- "$ENVP" -i HOME="$H" PATH="$PATH" TERM=xterm-256color SHELL="$X" /bin/sh "$RUN/launcher.sh" ws run --ws-id 0123456789abcdef --pane kabcdefgh 'a b;$(touch pwned) *'
  P=$O
  if [ "$RC" -ne 0 ] || [ -z "$P" ]; then
    sub "11.$B" 1 "$B: split-window with an argument list: exit $RC; $E"
    continue
  fi
  # a: the argument list reached the program unchanged.
  a=bad
  if wait_for 5 test -s "$RUN/launcher.txt"; then
    sed -n '2,$p' "$RUN/launcher.txt" >"$RUN/launcher.got"
    if cmp -s "$RUN/launcher.got" "$RUN/launcher.exp" && [ ! -e "$RUN/cwd/pwned" ]; then a=ok; fi
  fi
  # b: the pane holds the shell, with the process id of the launcher.
  cmd=$B; wait_for 5 pane_cmd_is "$P" "$B" || cmd=$(tm display-message -p -t "$P" '#{pane_current_command}' 2>/dev/null || echo none)
  t display-message -p -t "$P" '#{pane_pid}'
  k=no; if [ -s "$RUN/launcher.txt" ] && [ "$O" = "$(sed -n 1p "$RUN/launcher.txt")" ]; then k=yes; fi
  sleep 1.5
  # c: the shell is on the pane's terminal and runs a typed command.
  typed "$P" "/bin/sh -c 'test -t 0 && test -t 1 && echo ok >shell-tty'"
  c=no; if wait_for 10 test -s "$RUN/cwd/shell-tty" && [ "$(cat "$RUN/cwd/shell-tty")" = ok ]; then c=yes; fi
  # d: it takes a second command.
  typed "$P" "/bin/sh -c 'echo ok >shell-second'"
  d=no; if wait_for 10 test -s "$RUN/cwd/shell-second" && [ "$(cat "$RUN/cwd/shell-second")" = ok ]; then d=yes; fi
  # e: exit ends the pane.
  typed "$P" exit
  e=no; if wait_for 5 pane_gone "$P"; then e=yes; fi
  p=1; if [ "$a" = ok ] && [ "$k" = yes ] && [ "$cmd" = "$B" ] && [ "$c" = yes ] && [ "$d" = yes ] && [ "$e" = yes ]; then p=0; fi
  sub "11.$B" $p "$B: argv $a; pid kept $k; command $cmd; tty $c; second $d; exit $e"
done

LIVE_VER="tmux $TV"
scrub "$RUN/pw11.txt" >"$FX/pw11-$TV.txt"
if [ -z "$BAD" ]; then result PW11 verified "tmux $TV: $N of $N sub-checks passed; fixtures/tmux/pw11-$TV.txt; stand-in launcher, not the zygo launcher's signal setup (AU88)"
else result PW11 failed "tmux $TV: $((N - $(set -- $BAD; echo $#))) of $N sub-checks passed; failed:$BAD; fixtures/tmux/pw11-$TV.txt; stand-in launcher, not the zygo launcher's signal setup (AU88)"; fi

srv_stop
live_done "$FX"

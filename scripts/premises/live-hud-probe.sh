#!/bin/sh
# Live probe PRB-1 (HUD design H§12 "Probe first"): a temporary statusLine command (hud_probe.py) in the
# real Claude Code, on Linux or macOS, for PH1 PH2 PH3 PH4 PH5 PH6 PH7 PH9 PH11 PH12; PH13 on OpenCode.
# Run by hand only; never from CI or a Rust test.
# Usage: sh scripts/premises/live-hud-probe.sh   (claude and opencode on PATH; tmux 3.2 or later, python3, jq)
# Prints eleven lines, LIVE <id> verified|failed|observed (agent <version>): <text>. Exit 0 when the eleven
# lines are printed (a failed line does not change the exit status); exit 1 when a precondition fails,
# before any agent starts.
#
# What it does to the operator's machine:
# - Claude Code runs as `claude --settings <run file>` (no zygo), twice: in a private tmux server
#   (tmux -L zygo-prb-<pid>, no tmux config file, 200 x 50) and on a pseudo-terminal of 100 columns. The
#   settings file holds only the statusLine command; a --settings statusLine comes before the user's.
#   No settings file of the user is written.
# - Both runs use the real HOME and the caller's CLAUDE_CONFIG_DIR, so Claude Code reads the operator's
#   login, settings, hooks, plugins and MCP servers. DISABLE_AUTOUPDATER=1 is set. No prompt is typed:
#   Claude Code sends only its own start requests.
# - Claude Code asks whether it trusts the run's temporary repository; the script answers yes (it moves the
#   pointer to "Yes, I trust this folder", reads the pane to see it there, then sends Enter). Claude Code
#   keeps that answer and a project folder for the path in its config folder; the script does not delete them.
# - PH13 runs `opencode --help`, `opencode debug --help` and `opencode debug config --pure` with a temporary
#   HOME; OpenCode sends no model request there.
# - The traps end the private tmux server (and so Claude Code in it), the pseudo-terminal session and the
#   background processes, and remove the scratch tree.

for t in tmux python3 jq; do command -v "$t" >/dev/null || { echo "live: $t is not on PATH"; exit 1; }; done
[ $# -eq 0 ] || { echo "usage: sh scripts/premises/live-hud-probe.sh"; exit 1; }
PRB_CC_DIR=${CLAUDE_CONFIG_DIR:-}
unset TMUX TMUX_PANE
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps

# Every tmux call of this script and of lib.sh and live.sh goes to a private server with no config file.
PRB_SOCK="zygo-prb-$$"
tmux() { command tmux -u -L "$PRB_SOCK" -f /dev/null "$@"; }
# prb_end <status>: end the private tmux server (and every agent session in it), remove its socket file,
# then live_end (which kills LIVE_PIDS, removes the scratch tree and exits with <status>).
prb_end() {
  trap - EXIT INT TERM HUP PIPE
  command tmux -L "$PRB_SOCK" -f /dev/null kill-server 2>/dev/null || true
  rm -f "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/$PRB_SOCK" 2>/dev/null || true
  live_end "$1"
}
trap 'prb_end $?' EXIT
trap 'prb_end 129' HUP
trap 'prb_end 130' INT
trap 'prb_end 141' PIPE
trap 'prb_end 143' TERM

pinned_agents
TMUX_VER=$(command tmux -V | sed 's/^tmux //')
ver_ge "$(printf '%s' "$TMUX_VER" | sed 's/[^0-9.].*$//')" 3.2 || { echo "live: tmux is $TMUX_VER; the minimum is 3.2"; exit 1; }
case "$HOME$PRB_CC_DIR$TOOL_PATH$N2_SCRATCH$PREMISES_DIR" in
  *[[:space:]\']*) echo "live: a path (HOME, CLAUDE_CONFIG_DIR, PATH, the scratch dir or the checkout) holds a blank or a quote"; exit 1 ;;
esac

FX="$REPO_DIR/fixtures/claude-code/hud"
mkdir -p "$FX"
LOGO=$(printf '\360\235\230\207\360\235\230\206\360\235\227\264\360\235\227\274')   # U+1D607 U+1D606 U+1D5F4 U+1D5FC
PTR=$(printf '\342\235\257')                                                        # U+276F, the pointer of a Claude Code menu
SCISSORS=$(printf '\342\234\202')                                                   # U+2702
TRUST_YES="Yes, I trust"                                                            # the accept option of the folder trust question
PY="$PREMISES_DIR/hud_probe.py"
PYTHON=$(command -v python3)
hp() { python3 -I "$PY" "$@"; }

live_run probe
P=$RUN
T="$P/tmux"
Q="$P/pty"
mkdir -p "$T/log" "$Q/log" "$P/fx" "$P/oc"
hp settings "$PYTHON" "$PY" "$T" 2 "$T/settings.json"
hp settings "$PYTHON" "$PY" "$Q" 2 "$Q/settings.json"
printf 'verbose\n' >"$T/phase"
printf 'verbose\n' >"$Q/phase"
: >"$P/lines.txt"
rm -f "$FX"/prb-1-*

# line <id> <word> <text>: the LIVE line (live.sh result), also kept for the results fixture.
line() { result "$@" | tee -a "$P/lines.txt"; }
# cc_env: the environment words of each Claude Code launch (the login, settings and plugins of the caller).
cc_env() { echo "HOME=$HOME ${PRB_CC_DIR:+CLAUDE_CONFIG_DIR=$PRB_CC_DIR} PATH=$TOOL_PATH TERM=xterm-256color LANG=C.UTF-8 DISABLE_AUTOUPDATER=1"; }
pane() { tmux capture-pane -p -t "=$1:" 2>/dev/null || true; }
shows() { pane "$1" | grep -qF -- "$2"; }
# wait_until <seconds> <command>: poll every 0.5 s; 1 on timeout.
wait_until() {
  _n=$(($1 * 2)); shift
  _i=0
  until "$@"; do _i=$((_i + 1)); [ "$_i" -lt "$_n" ] || return 1; sleep 0.5; done
}
# trust_line <session>: the pane line of the accept option of the folder trust question (empty: no question).
trust_line() { pane "$1" | grep -F "$TRUST_YES" | tail -n 1; }
# prb_trust <session>: one key step on the folder trust question (LIV-1, live run finding D1: Claude Code
# 2.1.330 starts with the pointer on "No, exit", so Enter alone ends the session). The pointer is not on
# the accept line: Down. The pointer is on the accept line in two readings 0.5 s apart: Enter. A key that
# Claude Code did not take is sent again by the next step. Prints the number of 0.5 s polls it used.
prb_trust() {
  case "$(trust_line "$1")" in
    "") echo 0 ;;
    *"$PTR"*)
      sleep 0.5
      case "$(trust_line "$1")" in *"$PTR"*) tmux send-keys -t "=$1:" Enter || true; sleep 1; echo 3 ;; *) echo 1 ;; esac ;;
    *) tmux send-keys -t "=$1:" Down || true; sleep 1; echo 2 ;;
  esac
}
# prb_wait <session> <text>: the text is in the pane within 60 s; answers the folder trust question with
# yes. 1 when it is not; PRB_WHY then says why.
prb_wait() {
  _i=0
  until shows "$1" "$2"; do
    _p=$(pane "$1")
    case "$_p" in
      *prb-exit:*)
        PRB_WHY="claude ended: $(printf '%s\n' "$_p" | grep -E 'prb-exit:' | sed -n 1p | cut -c1-80)"
        return 1 ;;
      *"$TRUST_YES"*) _i=$((_i + $(prb_trust "$1"))) ;;
    esac
    _i=$((_i + 1)); [ "$_i" -lt 120 ] || { PRB_WHY="timeout: no probe status line in the pane within 60 s"; return 1; }
    sleep 0.5
  done
}
# prb_stop <session>: /exit, at most 10 s for the end, then kill the session.
prb_stop() {
  tmux_send "$1" /exit 2>/dev/null || true
  _i=0
  while tmux has-session -t "=$1" 2>/dev/null && ! shows "$1" prb-exit:; do
    _i=$((_i + 1)); [ "$_i" -lt 20 ] || break
    sleep 0.5
  done
  tmux kill-session -t "=$1" 2>/dev/null || true
}
# phase <name>: the phase file that the probe command reads at its start (temporary file, then rename).
phase() { printf '%s\n' "$1" >"$T/phase.tmp" && mv "$T/phase.tmp" "$T/phase"; }
# cap <phase> <seconds> <escapes 0|1>: captures of session t every 0.5 s.
cap() { hp capture "$PRB_SOCK" t "$T" "$1" "$2" "$3" || true; }
# note <text>: a line on stderr; the run goes on.
note() { echo "live-hud-probe: $*" >&2; }

# ---- tmux's own cell widths of the probe glyphs (session w, no agent) ----
tmux_own w
tmux new-session -d -s w -x 40 -y 5 "sleep 60"
hp widths "$PRB_SOCK" w "$P/tw.json"
tmux kill-session -t "=w" 2>/dev/null || true

# ---- session t: Claude Code with the probe status line, 200 x 50, in tmux; no prompt ----
T_WHY=""
tmux_own t
echo "cd $R && env -i $(cc_env) $CLAUDE_BIN --settings $T/settings.json; echo prb-exit:\$?; sleep 3" >"$P/tmux-t.sh"
tmux new-session -d -s t -x 200 -y 50 -c "$R" "/bin/sh $P/tmux-t.sh"
if prb_wait t "$LOGO"; then
  sleep 3
  cap verbose 1 1
  phase compact
  wait_until 10 shows t "$SCISSORS" || note "no compact row within 10 s"
  sleep 1
  cap compact 1 1
  for x in A B C D; do
    phase "wide$x"
    wait_until 10 shows t "P|wide$x probe" || note "no wide$x rows within 10 s"
    sleep 1
    cap "wide$x" 1 0
  done
  # early after the wide phases: a Claude Code that does not tolerate it ends there, after PH3 to PH9.
  phase early
  wait_until 10 shows t "PH11 early" || note "no early row within 10 s"
  cap early 8 0
  phase slow
  cap slow 20 0
  phase done
  wait_until 10 shows t "PROBE DONE" || note "no PROBE DONE row within 10 s"
  cap done 1 0
else
  T_WHY=$PRB_WHY
fi
prb_stop t

# ---- session p: outside tmux, on a pseudo-terminal of 100 columns; no prompt ----
# shellcheck disable=SC2046
python3 -I "$PY" pty "$Q/log" 100 60 "$R" "$(command -v env)" -i $(cc_env) "$CLAUDE_BIN" --settings "$Q/settings.json" &
LIVE_PIDS=$!
P_RC=0
wait "$LIVE_PIDS" || P_RC=$?
LIVE_PIDS=""
case "$P_RC" in
  0) P_WHY="" ;;
  3) P_WHY="pseudo-terminal session: no probe run within 60 s" ;;
  5) P_WHY="pseudo-terminal session: Claude Code showed the folder trust question, which the script does not answer there" ;;
  *) P_WHY="pseudo-terminal session: the driver ended with status $P_RC" ;;
esac

# ---- PH13: OpenCode help and inline config keys (temporary HOME) ----
hp ph13 "$OPENCODE_BIN" "$P/oc" "$P/ph13.json" || true

# ---- analysis and the LIVE lines ----
hp analyze "$T" "$Q" 200 100 "$P/tw.json" "$P/stdin.json" >"$P/an.json" || echo '{}' >"$P/an.json"
j() { jq -r "$1" "$P/an.json"; }
o() { jq -r "$1" "$P/ph13.json" 2>/dev/null || echo error; }
dig() { case "$1" in '' | *[!0-9]*) return 1 ;; esac; }

if [ -n "$T_WHY" ]; then
  line PH1 failed "$T_WHY"
elif [ "$(j .sgr.ok)" = true ]; then
  line PH1 verified "capture-pane -e (tmux $TMUX_VER), the H§6.3 verbose printf bytes: stripe fg 1,3,6, logo bold fg 15, worktree name fg 172 (38;5;n)"
else
  line PH1 failed "16-color part $(j '.sgr.ok16 // false'), 256-color part $(j '.sgr.ok256 // false'): $(jq -c .sgr "$P/an.json")"
fi

if [ -n "$T_WHY" ]; then
  line PH2 failed "$T_WHY"
elif [ "$(j .two.rows)" = 2 ]; then
  line PH2 verified "the two rows of the H§6.3 verbose printf show as two rows (tmux $TMUX_VER, 200 columns)"
else
  line PH2 failed "the H§6.3 verbose printf shows as $(j .two.rows) row(s): $(jq -c .two "$P/an.json")"
fi

C3=$(j .wide.A.class)
T3="a row of 302 cells in a 200-column pane shows $(j '.wide.A.shown // "-"') cells (ruler $(j '.wide.A.ruler // "-"'), ends in …: $(j '.wide.A.ellipsis // "-"'))"
if [ -n "$T_WHY" ]; then
  line PH3 failed "$T_WHY"
elif [ "$C3" = cut ]; then
  line PH3 verified "cut: $T3; the next line does not go on with the row"
elif [ "$C3" = wrapped ]; then
  line PH3 failed "wrapped: $T3; the next line goes on with the row"
else
  line PH3 failed "not observed: $C3: $(j '.wide.A.why // ""')"
fi

TT=$(j .ph4.tty_t) TP=$(j .ph4.tty_p) CT=$(j .ph4.col_t) CP=$(j .ph4.col_p) LOSS=$(j .ph4.loss)
T4="(a) /dev/tty in the command: tmux 200 columns: $TT, pseudo-terminal 100 columns: $TP; (b) COLUMNS: tmux $CT, pseudo-terminal $CP; (c) status area = pane width - $LOSS at 200 columns (indent $(j .ph4.indent), right gap $(j .ph4.gap))"
if [ "$TT" = 200 ] && [ "$TP" = 100 ] && dig "$CT" && dig "$CP" && [ "$LOSS" = 0 ]; then
  line PH4 verified "$T4"
else
  line PH4 failed "${T_WHY:+$T_WHY; }${P_WHY:+$P_WHY; }$T4"
fi

if jq -e 'length > 0 and all(.[]; (.effort | type) as $t | $t == "string" or ($t == "object" and (.effort.level | type) == "string"))' "$P/stdin.json" >/dev/null 2>&1; then
  line PH5 verified "stdin effort is $(jq -c '.[0].effort' "$P/stdin.json") ($(jq -r '.[0].effort | type' "$P/stdin.json")) in every logged run (tmux and pseudo-terminal); stdin keys of the first run: $(jq -r '.[0].keys | join(" ")' "$P/stdin.json")"
else
  line PH5 failed "${T_WHY:+$T_WHY; }stdin effort shapes: $(j '.ph5.efforts | join(" ")') in $(j .ph5.runs) logged run(s)"
fi

# wide_text <X> <glyph text> <glyph count>: the measured cells of one wide row.
wide_text() { echo "Claude Code counts $2 as $(j ".wide.$1.c // \"-\"") cell(s): the row with $3 shows $(j ".wide.$1.diff // \"-\"") fewer ruler cells than the ASCII row ($(j '.wide.A.ruler // "-"') and $(j ".wide.$1.ruler // \"-\""))"; }
if [ -n "$T_WHY" ]; then
  line PH6 failed "$T_WHY"
elif [ "$(j .wide.B.diff)" = 10 ]; then
  line PH6 verified "$(wide_text B U+F0645 '10 glyphs')"
else
  line PH6 failed "$(wide_text B U+F0645 '10 glyphs')$(j '.wide.B.why // "" | if . == "" then "" else "; " + . end')"
fi

T7="$(wide_text C U+2702 '10 glyphs'); tmux $TMUX_VER width $(j '.tw.C // "-"'); U+FE0F after it in the compact row: $(j .compact.vs16); the terminal font's presentation is not checked"
if [ -n "$T_WHY" ]; then
  line PH7 failed "$T_WHY"
elif [ "$(j .wide.C.diff)" = 10 ] && [ "$(j .tw.C)" = 1 ] && [ "$(j .compact.scissors)" = true ] && [ "$(j .compact.vs16)" = false ]; then
  line PH7 verified "$T7"
else
  line PH7 failed "$T7"
fi

T9="$(wide_text D 'each logo letter' '20 letters'); tmux $TMUX_VER width $(j '.tw.D // "-"'); logo and reflection start at column $(j .two.logo_col) and $(j .two.refl_col); the font's glyphs are not checked"
if [ -n "$T_WHY" ]; then
  line PH9 failed "$T_WHY"
elif [ "$(j .wide.D.diff)" = 20 ] && [ "$(j .tw.D)" = 1 ] && [ "$(j .two.rows)" = 2 ] && [ "$(j .two.logo_col)" -ge 0 ] && [ "$(j .two.logo_col)" = "$(j .two.refl_col)" ]; then
  line PH9 verified "$T9"
else
  line PH9 failed "$T9"
fi

if [ -n "$T_WHY" ]; then
  line PH11 failed "$T_WHY"
elif [ "$(j .ph11.shown)" -ge 3 ] && [ "$(j .ph11.lost)" = 0 ] && [ "$(j .ph11.error)" = false ] && [ "$(j .ph11.after)" = true ]; then
  line PH11 verified "a command that closes stdin unread and exits at once: each early run's line showed (3 or more runs), none lost, no error text, and the status line went on after the phase"
else
  line PH11 failed "a command that closes stdin unread and exits at once: lines shown $(j .ph11.shown) of $(j .ph11.runs) runs, lost $(j .ph11.lost), error text $(j .ph11.error), went on after the phase $(j .ph11.after)"
fi

case "$(j .ph12.class)" in
  kills) T12="Claude Code ends a running command when the next refresh comes" ;;
  waits) T12="Claude Code waits for a running command before it starts the next" ;;
  overlaps) T12="Claude Code starts the next command while the first still runs" ;;
  *) T12="" ;;
esac
SIGS=$(j '.ph12.signals | if length == 0 then "none" else join(",") end')
if [ -n "$T_WHY" ]; then
  line PH12 failed "$T_WHY"
elif [ -n "$T12" ] && [ "$(j .ph12.starts)" -ge 2 ]; then
  line PH12 observed "refreshInterval 2 s, a command that sleeps 5 s: $T12; signals seen: $SIGS; at most $(j .ph12.max_concurrent) command(s) at a time; slow line shown: $(j .ph12.shown)"
else
  line PH12 failed "not observed: $(j .ph12.starts) slow run(s) started in 20 s"
fi

LIVE_VER=$LIVE_OC_VER
K13="statusLine $(o .keys.statusLine), statusline $(o .keys.statusline), status_line $(o .keys.status_line); control: username $(o .control.username), zygoProbeUnknownKey $(o .control.zygoProbeUnknownKey)"
if [ "$(o '.help_hits | length')" = 0 ] && [ "$(o .control.username)" = kept ] && [ "$(o .control.zygoProbeUnknownKey)" = dropped ] &&
  [ "$(o '[.keys[] | select(. != "dropped")] | length')" = 0 ]; then
  line PH13 verified "OpenCode $LIVE_OC_VER: no status line in --help or debug --help; debug config --pure drops the inline keys: $K13"
else
  line PH13 failed "OpenCode $LIVE_OC_VER: help matches: $(o '.help_hits | join("; ")'); inline keys: $K13"
fi
LIVE_VER=$LIVE_CC_VER

# ---- fixtures (RUN is the probe run, so scrub maps its paths) ----
[ "$(grep -c '^LIVE PH' "$P/lines.txt")" = 11 ] || { echo "live: $(grep -c '^LIVE PH' "$P/lines.txt") LIVE lines, not 11"; exit 1; }
hp fixtures "$T" "$Q" "$P/an.json" "$P/ph13.json" "$P/fx" || true
python3 -I -c 'import json, sys; json.dump(dict(zip(["date", "claude_code", "opencode", "tmux", "os"], sys.argv[1:]), pane="200x50", pty="100x50", refresh_interval=2), open(sys.argv[6], "w"), indent=1)' \
  "$(date +%Y-%m-%d)" "$LIVE_CC_VER" "$LIVE_OC_VER" "$TMUX_VER" "$(uname -s)" "$P/fx/run.json"
cp "$P/lines.txt" "$P/fx/results.txt"
for f in "$P"/fx/*; do
  if [ -s "$f" ]; then scrub "$f" >"$FX/prb-1-${f##*/}"; fi
done
live_done "$FX"

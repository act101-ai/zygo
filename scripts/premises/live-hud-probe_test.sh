#!/bin/sh
# Offline test of scripts/premises/live-hud-probe.sh (node PRB-1): the script runs in a copied tree with
# stub claude and opencode programs (a stub Claude Code that runs the --settings statusLine command each
# refreshInterval and draws it, with the folder trust question first), a temporary HOME and a private
# TMUX_TMPDIR. No agent, no network, no login. Not run by CI.
# Usage: sh scripts/premises/live-hud-probe_test.sh   ->  "live-hud-probe tests: ok (<n>)", exit 0
set -u
unset TMUX TMUX_PANE CLAUDECODE CLAUDE_CONFIG_DIR N2_SCRATCH N2_SCRATCH_OWN
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
T=$(mktemp -d)
export TMUX_TMPDIR="$T/tmux"
mkdir -p "$TMUX_TMPDIR"
cleanup() {
  for s in "$TMUX_TMPDIR"/tmux-*/*; do [ -S "$s" ] && tmux -S "$s" -f /dev/null kill-server 2>/dev/null; done
  pkill -f "$T/" 2>/dev/null
  chmod -R u+rwx "$T" 2>/dev/null
  rm -rf "$T"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
n=0 bad=0
ok() { n=$((n + 1)); printf 'ok: %s\n' "$1"; }
no() { n=$((n + 1)); bad=$((bad + 1)); printf 'FAIL: %s (%s)\n' "$1" "$2"; }

# The copied tree: the premise scripts, docs/premises.md (the pins) and an empty fixture folder.
C="$T/repo"
mkdir -p "$C/scripts" "$C/docs" "$C/fixtures/claude-code/hud" "$T/bin" "$T/home/.claude"
cp -R "$REPO/scripts/premises" "$C/scripts/"
cp "$REPO/docs/premises.md" "$C/docs/"
printf '{"model": "x"}\n' >"$T/home/.claude/settings.json"
SUM=$(cksum <"$T/home/.claude/settings.json")

cat >"$T/bin/claude" <<'PY'
#!/usr/bin/env python3
# Stub Claude Code: --version; else the folder trust question (pointer on "No, exit") until a marker file
# exists, then the statusLine command of --settings each refreshInterval: stdin JSON, COLUMNS, a new
# session (no controlling terminal); a run still going at the next refresh gets SIGTERM. Draws the
# output 2 cells from the left and cuts a row at width - 4 with an ellipsis. "/exit" Enter ends it.
import json, os, select, signal, subprocess, sys, termios, time, tty, unicodedata
a = sys.argv[1:]
if a == ["--version"]:
    print(os.environ.get("STUB_CC_VER", "2.1.330") + " (Claude Code)")
    sys.exit(0)
s = json.load(open(a[a.index("--settings") + 1]))["statusLine"]
cmd, every = s["command"], s.get("refreshInterval", 2)
fd = sys.stdin.fileno()
old = termios.tcgetattr(fd)
tty.setraw(fd)
out = sys.stdout


def width():
    return os.get_terminal_size(out.fileno()).columns


def cells(t):
    return sum(2 if unicodedata.east_asian_width(c) in "WF" else 1 for c in t)


def cut(row, w):
    if "\x1b" in row or cells(row) <= w:
        return row
    r = ""
    for c in row:
        if cells(r + c) > w - 1:
            break
        r += c
    return r + "…"


def draw(lines):
    out.write("\x1b[H\x1b[2J" + "\r\n".join(lines) + "\r\n")
    out.flush()


def keys(timeout):
    if select.select([fd], [], [], timeout)[0]:
        return os.read(fd, 1024).decode("utf-8", "replace")
    return ""


mark = os.path.join(os.environ["HOME"], ".stub-trusted")
try:
    if not os.path.exists(mark):
        at = 0
        while True:
            draw([" Quick safety check", "", (" ❯ " if at == 0 else "   ") + "No, exit",
                  (" ❯ " if at == 1 else "   ") + "Yes, I trust this folder"])
            k = keys(5)
            if "\x1b[B" in k:
                at = 1
            elif "\x1b[A" in k:
                at = 0
            elif "\r" in k:
                if at == 0:
                    draw(["Claude Code didn't start because this folder isn't trusted."])
                    sys.exit(1)
                open(mark, "w").close()
                break
    typed, rows, prev, nxt = "", [], None, 0.0
    while True:
        if time.monotonic() >= nxt:
            nxt = time.monotonic() + every
            if prev is not None and prev.poll() is None:
                os.killpg(prev.pid, signal.SIGTERM)
            env = dict(os.environ, COLUMNS=str(width()))
            prev = subprocess.Popen(cmd, shell=True, stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=env,
                                    start_new_session=True)
            try:
                prev.stdin.write(json.dumps({"session_id": "stub", "effort": {"level": "medium"}, "model": {"id": "m"}}).encode())
                prev.stdin.close()
            except BrokenPipeError:
                pass
            try:
                o, _ = prev.communicate(timeout=every * 0.8)
                rows = o.decode("utf-8", "replace").rstrip("\n").split("\n")
            except subprocess.TimeoutExpired:
                pass
        draw(["> " + typed, ""] + ["  " + cut(r, width() - 4) for r in rows] + ["  ? for shortcuts"])
        k = keys(0.2)
        for c in k:
            if c == "\r":
                if typed == "/exit":
                    sys.exit(0)
                typed = ""
            elif c.isprintable():
                typed += c
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
PY
cat >"$T/bin/opencode" <<'PY'
#!/usr/bin/env python3
import json, os, sys
a = sys.argv[1:]
if a in (["--version"], ["-v"]):
    print("1.19.6")
elif a and a[-1] == "--help":
    print("Commands:\n  opencode run [message..]   run opencode with a message")
elif a[:2] == ["debug", "config"]:
    c = json.loads(os.environ.get("OPENCODE_CONFIG_CONTENT", "{}"))
    d = {"agent": {}, "mode": {}, "plugin": [], "command": {}, "username": "unknown"}
    d.update({k: v for k, v in c.items() if k in ("username", "model")})
    print(json.dumps(d))
PY
chmod +x "$T/bin/claude" "$T/bin/opencode"

# run <out file> [NAME=value ...]: the script in the copied tree, stubs first on PATH, no N2_SCRATCH (lib.sh
# makes its own scratch dir under TMPDIR = $T/tmp: removed after exit 0, kept after an error); sets rc.
run() {
  _o=$1; shift
  env HOME="$T/home" PATH="$T/bin:$PATH" TMPDIR="$T/tmp" "$@" sh "$C/scripts/premises/live-hud-probe.sh" >"$_o" 2>&1
  rc=$?
}
# clean_after <case> <exit status>: no private tmux server or socket, no live-<pid> tree, no stub process,
# no scratch dir after exit 0; the settings file of the fake HOME unchanged.
clean_after() {
  _l=$(ls "$TMUX_TMPDIR"/tmux-*/ 2>/dev/null | grep -c zygo-prb- || true)
  _s=$(find "$T/tmp" -mindepth 2 -maxdepth 2 -name 'live-*' | grep -c . || true)
  _d=$(find "$T/tmp" -mindepth 1 -maxdepth 1 | grep -c . || true)
  [ "$2" -ne 0 ] && _d=0
  _p=$(pgrep -f "$T/bin/claude" | grep -c . || true)
  if [ "$_l" = 0 ] && [ "$_s" = 0 ] && [ "$_d" = 0 ] && [ "$_p" = 0 ] && [ "$(cksum <"$T/home/.claude/settings.json")" = "$SUM" ]; then ok "$1 leaves nothing"
  else no "$1 leaves nothing" "sockets $_l, live trees $_s, scratch dirs $_d, stub processes $_p"; fi
  rm -rf "$T/tmp"; mkdir -p "$T/tmp"
}
mkdir -p "$T/tmp"

# L1: the full run with the stubs: eleven lines, the stub's expected results, the fixtures.
run "$T/l1.out"
if [ "$rc" -eq 0 ] && [ "$(grep -c '^LIVE PH' "$T/l1.out")" = 11 ]; then ok "L1 exit 0 and eleven LIVE lines"
else no "L1 exit 0 and eleven LIVE lines" "exit $rc: $(tail -n 5 "$T/l1.out" | tr '\n' '|')"; fi
want="PH1 verified|PH2 verified|PH3 verified|PH4 failed|PH5 verified|PH6 verified|PH7 verified|PH9 verified|PH11 verified|PH12 observed|PH13 verified"
got=$(sed -n 's/^LIVE \(PH[0-9]*\) \([a-z]*\) .*/\1 \2/p' "$T/l1.out" | tr '\n' '|' | sed 's/|$//')
if [ "$got" = "$want" ]; then ok "L2 results with the stub Claude Code"; else no "L2 results with the stub Claude Code" "$got"; fi
if grep -q 'LIVE PH4 failed .*tmux 200 columns: ENXIO.*COLUMNS: tmux 200, pseudo-terminal 100.*pane width - 4 at 200 columns (indent 2, right gap 2)' "$T/l1.out"; then ok "L3 PH4 text"
else no "L3 PH4 text" "$(grep 'LIVE PH4' "$T/l1.out")"; fi
if grep -q 'LIVE PH12 observed .*ends a running command when the next refresh comes; signals seen: TERM' "$T/l1.out"; then ok "L4 PH12 text"
else no "L4 PH12 text" "$(grep 'LIVE PH12' "$T/l1.out")"; fi
fx="analysis.json compact.txt early.txt env-pty.json env-tmux.json events.json ph13.json results.txt run.json slow.txt stdin-pty.json stdin-tmux.json verbose.txt wide.txt"
have=$(cd "$C/fixtures/claude-code/hud" && ls | sed 's/^prb-1-//' | tr '\n' ' ' | sed 's/ $//')
want_fx=$(printf '%s\n' $fx | grep -v '^slow.txt$' | tr '\n' ' ' | sed 's/ $//')
if [ "$have" = "$want_fx" ] || [ "$have" = "$(printf '%s\n' $fx | tr '\n' ' ' | sed 's/ $//')" ]; then ok "L5 fixture files"
else no "L5 fixture files" "$have"; fi
if grep -rqF -e "$T" -e "\"pid\"" "$C/fixtures/claude-code/hud"; then no "L6 no run path and no pid in a fixture" "found"
else ok "L6 no run path and no pid in a fixture"; fi
clean_after L1 "$rc"

# L7: Claude Code below the minimum: exit 1 before any tmux server starts.
run "$T/l7.out" STUB_CC_VER=2.1.327
if [ "$rc" -eq 1 ] && grep -q 'the minimum is 2.1.328' "$T/l7.out"; then ok "L7 claude below the minimum"
else no "L7 claude below the minimum" "exit $rc: $(tail -n 2 "$T/l7.out" | tr '\n' '|')"; fi
clean_after L7 "$rc"

# L8: TERM during session t: exit 143; the traps end the tmux server and remove the scratch tree.
rm -f "$T/home/.stub-trusted"
env HOME="$T/home" PATH="$T/bin:$PATH" TMPDIR="$T/tmp" sh "$C/scripts/premises/live-hud-probe.sh" >"$T/l8.out" 2>&1 &
pid=$!
i=0
until pgrep -f "$T/bin/claude --settings" >/dev/null || [ "$i" -ge 40 ]; do i=$((i + 1)); sleep 0.5; done
sleep 2
kill -TERM "$pid"
wait "$pid"
rc=$?
if [ "$rc" -eq 143 ]; then ok "L8 TERM exits 143"; else no "L8 TERM exits 143" "exit $rc"; fi
sleep 1
clean_after L8 "$rc"

# L9: a blank in the scratch path: exit 1 with the path line.
mkdir -p "$T/a b"
env HOME="$T/home" PATH="$T/bin:$PATH" N2_SCRATCH="$T/a b" sh "$C/scripts/premises/live-hud-probe.sh" >"$T/l9.out" 2>&1
rc=$?
if [ "$rc" -eq 1 ] && grep -q 'holds a blank or a quote' "$T/l9.out"; then ok "L9 blank in a path"
else no "L9 blank in a path" "exit $rc: $(tail -n 2 "$T/l9.out" | tr '\n' '|')"; fi

if [ "$bad" -eq 0 ]; then echo "live-hud-probe tests: ok ($n)"; exit 0; fi
echo "live-hud-probe tests: $bad failed"
exit 1

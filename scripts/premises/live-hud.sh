#!/bin/sh
# Live check P-HUD-1 (node LIV-1; HUD design H§12, H§14 #37): the built status line HUD in the real
# Claude Code, on Linux or macOS. Run by hand only; never from CI or a Rust test.
# Usage: sh scripts/premises/live-hud.sh --zygo <path>   (<path>: cargo build -p zygo --features test-hooks)
#        sh scripts/premises/live-hud.sh --selfcheck      (the analyzer on built-in samples; no agent, no tmux)
# Prints eight lines, LIVE <id> verified|failed (agent <version>): <text>, for PH1 PH2 PH3 PH4 PH5 PH9
# PH12 C7. Exit 0 when the eight lines are printed (a failed line does not change the exit status);
# exit 1 when a precondition fails, before any agent starts.
#
# Parts PH1 to PH12 run `zygo claude --passthrough` on the claude.ai login that Claude Code already
# stores (as live-passthrough.sh; no key, no mock). What they do to the operator's machine:
# - They use the real HOME, and the caller's CLAUDE_CONFIG_DIR when it is set, so Claude Code reads the
#   operator's settings, hooks, plugins and MCP servers. The script edits none of them and adds no hook.
#   DISABLE_AUTOUPDATER=1 and ZYGO_NO_UPDATE_CHECK=1 are set.
# - Claude Code asks whether it trusts the run's temporary worktree; the script answers yes (it moves
#   the pointer to "Yes, I trust this folder", reads the pane to see it there, then sends Enter).
#   Claude Code then keeps an entry for that path under "projects" in its .claude.json, and a transcript
#   folder under <config dir>/projects/. The script does not delete them; the operator may.
# - They spend the operator's claude.ai usage: one short answer (session a), and Claude Code's own
#   start requests in four sessions.
# - zygo writes its state under ~/.local/share/zygo (usage rows stay; the plugin snapshot is refreshed)
#   and ~/.cache/zygo (each session folder, removed by zygo at exit). ZYGO_CONFIG is a file of the run.
# - Every session runs in a private tmux server (tmux -L zygo-hud-<pid>, no tmux config file) or on a
#   pseudo-terminal of the script; the traps end them. No global tmux option or hook is set.
# Part C7 runs on the mock upstream with a temporary HOME, as P-N28-1 (live-claude.sh): a passthrough
# request cannot be recorded.

# hud_py <dir>: writes the three Python helpers of this script into <dir>.
hud_py() {
  cat >"$1/hud_an.py" <<'PY'
import json, os, re, subprocess, sys, unicodedata

LOGO = "\U0001d607\U0001d606\U0001d5f4\U0001d5fc"
REFL = "zʎƃo"
STRIPE = "▌"
CSI = re.compile(r"\x1b\[[0-9;:?]*[A-Za-z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")


def plain(s):
    return CSI.sub("", s)


def cells(s):
    return sum(0 if unicodedata.combining(c) else 2 if unicodedata.east_asian_width(c) in "WF" else 1 for c in s)


def block(path):
    """The pane lines from the last line that holds the logo to the last non-empty line, SGR kept."""
    lines = open(path, encoding="utf-8", errors="replace").read().split("\n")
    at = [i for i, l in enumerate(lines) if LOGO in plain(l)]
    if not at:
        return []
    out = lines[at[-1]:]
    while out and not plain(out[-1]).strip():
        out.pop()
    return out


def attrs(line):
    """[(char, fg, bold, dim)] of one capture-pane -e line; fg is a palette index or None."""
    out, fg, bold, dim, i = [], None, False, False, 0
    while i < len(line):
        m = CSI.match(line, i)
        if not m:
            out.append((line[i], fg, bold, dim))
            i += 1
            continue
        i = m.end()
        if not m.group(0).endswith("m") or m.group(0).startswith("\x1b]"):
            continue
        p = [int(x) if x else 0 for x in re.split("[;:]", m.group(0)[2:-1])]
        k = 0
        while k < len(p):
            c = p[k]
            if c == 0:
                fg, bold, dim = None, False, False
            elif c == 1:
                bold = True
            elif c == 2:
                dim = True
            elif c == 22:
                bold = dim = False
            elif 30 <= c <= 37:
                fg = c - 30
            elif 90 <= c <= 97:
                fg = c - 90 + 8
            elif c == 39:
                fg = None
            elif c == 38 and k + 2 < len(p) and p[k + 1] == 5:
                fg = p[k + 2]
                k += 2
            elif c == 38 and k + 4 < len(p) and p[k + 1] == 2:
                fg = "rgb"
                k += 4
            k += 1
    return out


def cmd_sgr(pane):
    b = block(pane)
    if not b:
        return {"ok": False, "why": "no HUD row"}
    a = attrs(b[0])
    text = "".join(c for c, *_ in a)
    s, l, w = text.find(STRIPE * 3), text.find(LOGO), text.find("hud-work")
    stripe = [a[s + n][1] for n in range(3)] if s >= 0 else []
    logo = [a[l][1], a[l][2]] if l >= 0 else [None, False]
    tree = a[w][1] if w >= 0 else None
    r = {"stripe": stripe, "logo_fg": logo[0], "logo_bold": logo[1], "worktree_fg": tree,
         "stripe_dim": bool(s >= 0 and a[s][3])}
    r["ok16"] = stripe == [1, 3, 6] and logo[0] == 15 and logo[1] is True
    r["ok256"] = tree == 172
    r["ok"] = r["ok16"] and r["ok256"]
    return r


def cmd_two(pane):
    b = [plain(l) for l in block(pane)]
    if not b:
        return {"rows": 0, "logo_col": -1, "refl_col": -1}
    lc = b[0].find(LOGO)
    rc = b[1].find(REFL) if len(b) > 1 else -1
    same = REFL in b[0]
    return {"rows": 2 if rc >= 0 and STRIPE in b[1] and not same else 1, "logo_col": lc, "refl_col": rc}


def cmd_fit(pane, expected, cols):
    b = [plain(l) for l in block(pane)]
    e = plain(open(expected, encoding="utf-8", errors="replace").read()).split("\n")[0].rstrip()
    if not b or not e:
        return {"class": "none", "why": "no HUD row in the pane" if not b else "no expected row"}
    indent = len(b[0]) - len(b[0].lstrip(" "))
    s0 = b[0].strip()
    r = {"indent": indent, "shown_cells": cells(s0), "expected_cells": cells(e), "cols": int(cols)}
    if s0 == e:
        r["class"] = "whole"
        return r
    t = s0[:-1].rstrip() if s0.endswith("…") else s0
    if not t or not e.startswith(t):
        r["class"] = "other"
        return r
    rest = e[len(t):].lstrip()
    w = b[1].strip() if len(b) > 1 else ""
    r["class"] = "wrapped" if w and rest and (rest.startswith(w) or w.startswith(rest)) else "cut"
    if t == s0:
        # The pane does not show blanks at the end of a line: count the blanks of the expected row that follow.
        blanks = len(e[len(t):]) - len(rest)
        r["shown_cells"] = min(cells(s0) + blanks, int(cols) - indent)
    r["gap"] = int(cols) - indent - r["shown_cells"]
    return r


def cmd_ph12(events, bound):
    d = json.load(open(events))
    if isinstance(d, list):
        d = {"runs": d, "dumps": []}
    runs = sorted(d["runs"], key=lambda x: x["start"])
    dumps = sorted(d.get("dumps", []))
    bound = float(bound)
    # Each run writes the dump file once, so the dump count is the run count; ps seldom sees a run of a few ms.
    n = len(dumps) if dumps else len(runs)
    long = [x for x in runs if x["end"] - x["start"] >= 1.0]
    if not long:
        return {"class": "none", "runs": n, "seen_by_ps": len(runs)}
    first = long[0]
    life = round(first["end"] - first["start"], 2)
    # The starts after the slow run: the runs that ps saw, and the dump writes (the slow run writes its own
    # dump right after its stdin read, at most 0.5 s after its start).
    later = [x["start"] for x in runs if x["start"] > first["start"]] + [t for t in dumps if t > first["start"] + 0.5]
    nxt = round(min(later) - first["start"], 2) if later else None
    if life < bound - 0.5:
        c = "kills"
    elif nxt is not None and nxt < life - 0.2:
        c = "overlaps"
    else:
        c = "waits"
    return {"class": c, "life": life, "next_start": nxt, "runs": n, "seen_by_ps": len(runs)}


def cmd_expect(dump, zygo, cwd, columns, out):
    d = json.load(open(dump))
    env = {k: v for k, v in d.get("env", {}).items() if v is not None and k != "COLUMNS"}
    env.update({"HOME": os.environ["HOME"], "PATH": os.environ["PATH"]})
    if columns != "none":
        env["COLUMNS"] = columns
    p = subprocess.run([zygo, "hook", "statusline"], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL, env=env, cwd=cwd, start_new_session=True, timeout=10)
    open(out, "wb").write(p.stdout)
    return {"bytes": len(p.stdout)}


def cmd_rows(pane, count, form):
    """The first <count> lines of the HUD block as text; form: raw (SGR kept) or plain."""
    b = block(pane)[:int(count)]
    return "".join((l if form == "raw" else plain(l)).rstrip() + "\n" for l in b)


if __name__ == "__main__":
    r = globals()["cmd_" + sys.argv[1]](*sys.argv[2:])
    sys.stdout.write(r if isinstance(r, str) else json.dumps(r, ensure_ascii=False) + "\n")
PY
  cat >"$1/hud_poll.py" <<'PY'
import json, os, subprocess, sys, time

# hud_poll.py <out> <zygo path> <seconds> [<dump file>]: samples every 0.05 s. "runs": each process whose
# arguments start with "<zygo path> hook statusline" (first and last sample that saw it). "dumps": the time
# of each new dump file (each run of the hook replaces it once), so a run of a few ms counts too.
out, prefix, seconds = sys.argv[1], sys.argv[2] + " hook statusline", float(sys.argv[3])
dump = sys.argv[4] if len(sys.argv) > 4 else None
t0, seen, dumps, last = time.monotonic(), {}, [], None
while time.monotonic() - t0 < seconds:
    now = round(time.monotonic() - t0, 2)
    if dump:
        try:
            st = os.stat(dump)
            key = (st.st_ino, st.st_mtime_ns)
        except OSError:
            key = None
        if key is not None and key != last:
            dumps.append(now)
            last = key
    ps = subprocess.run(["ps", "-axo", "pid=,args="], capture_output=True, text=True).stdout
    for line in ps.splitlines():
        pid, _, args = line.strip().partition(" ")
        if args.strip().startswith(prefix):
            seen.setdefault(pid, [now, now])[1] = now
    time.sleep(0.05)
runs = [{"n": i + 1, "start": a, "end": b} for i, (a, b) in enumerate(sorted(seen.values()))]
json.dump({"runs": runs, "dumps": dumps}, open(out, "w"))
PY
  cat >"$1/hud_pty.py" <<'PY'
import fcntl, json, os, pty, re, select, shutil, signal, struct, sys, termios, time

CSI = re.compile(rb"\x1b\[[0-9;:?]*[A-Za-z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")
dump, log_copy, cols, wait, cwd = sys.argv[1], sys.argv[2], int(sys.argv[3]), float(sys.argv[4]), sys.argv[5]
argv = sys.argv[6:]
signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
pid, fd = pty.fork()
if pid == 0:
    os.chdir(cwd)
    os.execv(argv[0], argv)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 50, cols, 0, 0))
buf, code = b"", 3


def pump(seconds):
    """Reads the terminal output for `seconds`; False when the child closed the terminal."""
    global buf
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if select.select([fd], [], [], 0.2)[0]:
            try:
                data = os.read(fd, 65536)
            except OSError:
                return False
            if not data:
                return False
            buf = (buf + data)[-65536:]
    return True


try:
    end = time.monotonic() + wait
    while time.monotonic() < end and pump(0.5):
        if os.path.exists(dump) and os.path.getsize(dump) > 0:
            code = 0
            break
        # The folder trust question: not answered here (Enter alone means "No, exit" in Claude Code
        # 2.1.330). A tmux session of the script answers it before this session starts.
        if b"Itrustthisfolder" in re.sub(rb"\s+", b"", CSI.sub(b"", buf)):
            code = 5
            break
    if code == 0:
        pump(1)
        try:
            d = json.load(open(dump)).get("env", {}).get("ZYGO_SESSION_DIR")
            shutil.copyfile(os.path.join(d, "session.log"), log_copy)
        except (OSError, ValueError, TypeError):
            code = 4
        os.write(fd, b"/exit")
        pump(0.5)
        os.write(fd, b"\r")
        end = time.monotonic() + 10
        while time.monotonic() < end and os.waitpid(pid, os.WNOHANG)[0] == 0:
            pump(0.2)
finally:
    for sig in (signal.SIGHUP, signal.SIGKILL):
        try:
            if os.waitpid(pid, os.WNOHANG)[0] != 0:
                break
            os.killpg(pid, sig)
            time.sleep(2)
        except (ProcessLookupError, ChildProcessError):
            break
sys.exit(code)
PY
}

if [ "${1:-}" = --selfcheck ]; then
  set -eu
  PY=$(mktemp -d)
  trap 'rm -rf "$PY"' EXIT
  hud_py "$PY"
  cat >"$PY/hud_an_test.py" <<'PY'
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
T = HERE
E = "\x1b"
LOGO = "\U0001d607\U0001d606\U0001d5f4\U0001d5fc"
REFL = "zʎƃo"
S = "▌"


def run(*a):
    return json.loads(subprocess.run([sys.executable, "-I", os.path.join(HERE, "hud_an.py"), *a], capture_output=True, text=True, check=True).stdout)


def w(name, text):
    p = os.path.join(T, name)
    open(p, "w", encoding="utf-8").write(text)
    return p


n = 0


def ok(name, got, want):
    global n
    assert got == want, (name, got, want)
    n += 1
    print("ok:", name)


# S1: tmux -e form of the compact row, 2-space indent, after a rule line.
row = f"  {E}[31m{S}{E}[33m{S}{E}[36m{S}{E}[39m {E}[1m{E}[97m{LOGO}{E}[0m {E}[1m{E}[34m hud-b{E}[0m {E}[38;5;172m\U000f0645 hud-work{E}[39m  ctx 12k/{E}[2m200k{E}[0m"
p = w("s1.txt", "x\n" + "─" * 40 + "\n" + row + "\n  ⏸ manual mode on\n\n\n")
r = run("sgr", p)
ok("S1 sgr", (r["stripe"], r["logo_fg"], r["logo_bold"], r["worktree_fg"], r["ok"]), ([1, 3, 6], 15, True, 172, True))
# S2: no 256-color: the worktree name in 33.
p = w("s2.txt", row.replace("38;5;172", "33") + "\n")
r = run("sgr", p)
ok("S2 sgr 16 only", (r["ok16"], r["ok256"], r["worktree_fg"]), (True, False, 3))
# S3: two rows, same column.
v1 = f"  {S}{S}{S} {LOGO}  main   local"
v2 = f"  {S}{S}{S} {REFL}  ctx -/-"
ok("S3 two rows", run("two", w("s3.txt", v1 + "\n" + v2 + "\n")), {"rows": 2, "logo_col": 6, "refl_col": 6})
# S4: joined into one row.
ok("S4 one row", run("two", w("s4.txt", v1 + "   " + REFL + " ctx\n"))["rows"], 1)
# S5: fit classes.
exp = w("exp.txt", f"{E}[31m{S}{S}{S}{E}[0m {LOGO} main  ctx 12k/200k  tokens 1k 2k  $0.01")
e = f"{S}{S}{S} {LOGO} main  ctx 12k/200k  tokens 1k 2k  $0.01"
ok("S5 whole", run("fit", w("f1.txt", "  " + e + "\n  footer\n"), exp, "70")["class"], "whole")
r = run("fit", w("f2.txt", "  " + e[:17] + "\n  footer\n"), exp, "19")
ok("S5 cut", (r["class"], r["indent"], r["shown_cells"], r["gap"]), ("cut", 2, 17, 0))
r = run("fit", w("f3.txt", "  " + e[:16] + "…\n"), exp, "19")
ok("S5 cut with ellipsis", (r["class"], r["shown_cells"]), ("cut", 17))
r = run("fit", w("f4.txt", "  " + e[:17] + "\n  " + e[17:34] + "\n  " + e[34:] + "\n"), exp, "19")
ok("S5 wrapped", r["class"], "wrapped")
r = run("fit", w("f7.txt", "  " + e[:14] + "\n"), exp, "19")
ok("S5 cut at a blank", (r["class"], r["shown_cells"], r["gap"]), ("cut", 15, 2))
ok("S5 other", run("fit", w("f5.txt", "  " + e.replace("12k", "13k") + "\n"), exp, "70")["class"], "other")
ok("S5 none", run("fit", w("f6.txt", "nothing\n"), exp, "70")["class"], "none")
# S6: PH12 classes (bound 3).
ev = lambda rows: w("ev.json", json.dumps([{"n": i, "start": a, "end": b} for i, (a, b) in enumerate(rows)]))
ok("S6 overlaps", run("ph12", ev([(1.0, 3.95), (3.0, 3.1), (5.0, 5.1)]), "3")["class"], "overlaps")
ok("S6 waits", run("ph12", ev([(1.0, 3.95), (4.0, 4.1), (6.0, 6.1)]), "3")["class"], "waits")
ok("S6 kills", run("ph12", ev([(1.0, 3.0), (3.0, 3.1)]), "3")["class"], "kills")
ok("S6 none", run("ph12", ev([(1.0, 1.1), (3.0, 3.1)]), "3")["class"], "none")
r = run("ph12", w("ev2.json", json.dumps({"runs": [{"n": 1, "start": 1.0, "end": 3.1}, {"n": 2, "start": 7.0, "end": 9.0}], "dumps": [1.2, 3.05, 5.0, 7.1]})), "3")
ok("S6 kills, starts from the dump writes", (r["class"], r["life"], r["next_start"], r["runs"], r["seen_by_ps"]), ("kills", 2.1, 2.05, 4, 2))
out = subprocess.run([sys.executable, "-I", os.path.join(HERE, "hud_an.py"), "rows", w("r.txt", "a\n" + v1 + "\n" + v2 + "\n  footer\n\n"), "2", "plain"], capture_output=True, text=True, check=True).stdout
ok("S7 rows", out, v1 + "\n" + v2 + "\n")
print(f"live-hud selfcheck: ok ({n})")
PY
  python3 -I "$PY/hud_an_test.py"
  exit 0
fi

[ "${1:-}" = --zygo ] && [ -n "${2:-}" ] && [ $# -eq 2 ] || { echo "usage: sh scripts/premises/live-hud.sh --zygo <path> | --selfcheck"; exit 1; }
[ -f "$2" ] && [ -x "$2" ] || { echo "live: $2 is not an executable file"; exit 1; }
for t in tmux python3 jq git ps; do command -v "$t" >/dev/null || { echo "live: $t is not on PATH"; exit 1; }; done
# The path as std::env::current_exe gives it: the status line command of the session starts with it.
ZYGO_BIN=$(python3 -I -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$2")
HUD_CC_DIR=${CLAUDE_CONFIG_DIR:-}
unset TMUX TMUX_PANE
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps
TERM=xterm-256color

# Every tmux call of this script and of lib.sh and live.sh goes to a private server.
HUD_SOCK="zygo-hud-$$"
tmux() { command tmux -u -L "$HUD_SOCK" -f /dev/null "$@"; }
# hud_end <status>: end the private tmux server (and so every agent session in it), remove its socket
# file, then live_end.
hud_end() {
  trap - EXIT INT TERM HUP PIPE
  command tmux -L "$HUD_SOCK" kill-server 2>/dev/null || true
  rm -f "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/$HUD_SOCK" 2>/dev/null || true
  live_end "$1"
}
trap 'hud_end $?' EXIT
trap 'hud_end 129' HUP
trap 'hud_end 130' INT
trap 'hud_end 141' PIPE
trap 'hud_end 143' TERM

v=$("$CLAUDE_BIN" --version 2>/dev/null || true)
LIVE_CC_VER=$(agent_ver "$v")
agent_ok "$LIVE_CC_VER" "$PIN_CC" || {
  echo "live: claude is $(agent_show "$v"); the minimum is $PIN_CC (docs/premises.md); put a version at or above it first on PATH"; exit 1; }
LIVE_VER=$LIVE_CC_VER
TMUX_VER=$(command tmux -V | sed 's/^tmux //')
ver_ge "$(printf '%s' "$TMUX_VER" | sed 's/[^0-9.].*$//')" 3.2 || { echo "live: tmux is $TMUX_VER; the minimum is 3.2"; exit 1; }
case "$HOME$HUD_CC_DIR$TOOL_PATH$N2_SCRATCH$ZYGO_BIN" in
  *[[:space:]\']*) echo "live: a path (HOME, CLAUDE_CONFIG_DIR, PATH, the scratch dir or the zygo path) holds a blank or a quote"; exit 1 ;;
esac

FX="$REPO_DIR/fixtures/claude-code/hud"
mkdir -p "$FX"
LOGO=$(printf '\360\235\230\207\360\235\230\206\360\235\227\264\360\235\227\274')   # U+1D607 U+1D606 U+1D5F4 U+1D5FC
REFL=$(printf 'z\312\216\306\203o')                                                 # z U+028E U+0183 o
PTR=$(printf '\342\235\257')                                                        # U+276F, the pointer of a Claude Code menu
TRUST_YES="Yes, I trust"                                                            # the accept option of the folder trust question

live_run hud
HUD=$RUN
WT="$HUD/wt/hud-work"
git -C "$R" worktree add -q -b hud-b "$WT"
: >"$HUD/zygo.toml"
printf '[statusline]\nstyle = "verbose"\n' >"$HUD/zygo-verbose.toml"
: >"$HUD/lines.txt"
PY="$HUD/py"
mkdir -p "$PY" "$HUD/probe"
hud_py "$PY"
hud_an() { python3 -I "$PY/hud_an.py" "$@"; }

# The binary has the HUD-9 seams: one hand run of the hook writes the dump file.
(cd "$HUD" && env -i HOME="$HOME" PATH="$TOOL_PATH" ZYGO_SESSION_DIR="$HUD/probe" ZYGO_TEST_HUD_DUMP="$HUD/probe.json" \
  "$ZYGO_BIN" hook statusline </dev/null >/dev/null 2>&1) || true
[ -s "$HUD/probe.json" ] || { echo "live: $ZYGO_BIN has no HUD test seams; build it with: cargo build -p zygo --features test-hooks"; exit 1; }
rm -f "$FX"/p-hud-1-*

# line <id> verified|failed <text>: the LIVE line (live.sh result), also kept for the results fixture.
line() { result "$@" | tee -a "$HUD/lines.txt"; }
# hud_env <config file> <dump file> [NAME=value ...]: the environment words of one passthrough launch.
hud_env() {
  _c=$1 _d=$2; shift 2
  echo "HOME=$HOME ${HUD_CC_DIR:+CLAUDE_CONFIG_DIR=$HUD_CC_DIR} PATH=$TOOL_PATH TERM=xterm-256color LANG=C.UTF-8 DISABLE_AUTOUPDATER=1 ZYGO_NO_UPDATE_CHECK=1 ZYGO_CONFIG=$_c ZYGO_NERD_FONT=1 ZYGO_TEST_HUD_DUMP=$_d $*"
}
# hud_start <session> <columns> <config file> <dump file> [NAME=value ...]: `zygo claude --passthrough`
# in a detached session of the private tmux server, <columns> x 50, in the linked worktree.
hud_start() {
  _s=$1 _w=$2; shift 2
  tmux_own "$_s"
  echo "cd $WT && env -i $(hud_env "$@") $ZYGO_BIN claude --passthrough; echo hud-exit:\$?; sleep 3" >"$HUD/tmux-$_s.sh"
  tmux new-session -d -s "$_s" -x "$_w" -y 50 -c "$WT" "/bin/sh $HUD/tmux-$_s.sh"
}
pane() { tmux capture-pane -p -t "=$1:" 2>/dev/null || true; }
# wait_until <seconds> <command>: poll every 0.5 s; 1 on timeout.
wait_until() {
  _n=$(($1 * 2)); shift
  _i=0
  until "$@"; do _i=$((_i + 1)); [ "$_i" -lt "$_n" ] || return 1; sleep 0.5; done
}
# trust_line <session>: the pane line of the accept option of the folder trust question (empty: no question).
trust_line() { pane "$1" | grep -F "$TRUST_YES" | tail -n 1; }
# hud_trust <session>: one key step on the folder trust question. Claude Code 2.1.330 starts with the
# pointer on "No, exit", so Enter alone ends the session. The pointer is not on the accept line: Down.
# The pointer is on the accept line in two readings of the pane, 0.5 s apart: Enter. A key that Claude
# Code did not take is sent again by the next step. Prints the number of 0.5 s polls that the step used.
hud_trust() {
  case "$(trust_line "$1")" in
    "") echo 0 ;;
    *"$PTR"*)
      sleep 0.5
      case "$(trust_line "$1")" in *"$PTR"*) tmux send-keys -t "=$1:" Enter || true; sleep 1; echo 3 ;; *) echo 1 ;; esac ;;
    *) tmux send-keys -t "=$1:" Down || true; sleep 1; echo 2 ;;
  esac
}
# hud_wait <session>: the HUD row is in the pane within 60 s. Answers the folder trust question with
# yes (hud_trust). 1 when there is no HUD row; HUD_WHY then says why.
hud_wait() {
  _i=0
  until pane "$1" | grep -qF "$LOGO"; do
    _p=$(pane "$1")
    case "$_p" in
      *hud-exit:*)
        HUD_WHY="zygo claude ended: $(printf '%s\n' "$_p" | grep -E '^zygo: |hud-exit:' | sed -n 1p | python3 -I "$PREMISES_DIR/scrub.py" "$HUD" /dev/stdin | cut -c1-160)"
        return 1 ;;
      *"$TRUST_YES"*) _i=$((_i + $(hud_trust "$1"))) ;;
    esac
    _i=$((_i + 1)); [ "$_i" -lt 120 ] || { HUD_WHY="timeout: no HUD row in the pane within 60 s"; return 1; }
    sleep 0.5
  done
}
# hud_stop <session>: /exit, at most 10 s for the end, then kill the session.
hud_stop() {
  tmux_send "$1" /exit 2>/dev/null || true
  _i=0
  while tmux has-session -t "=$1" 2>/dev/null && ! pane "$1" | grep -qF hud-exit:; do
    _i=$((_i + 1)); [ "$_i" -lt 20 ] || break
    sleep 0.5
  done
  tmux kill-session -t "=$1" 2>/dev/null || true
}
# wsrc [<file>]: "<source> <width>" of each width-source line of a session log.
wsrc() { sed -n 's/.*width source \([^,]*\), width \([0-9]*\).*/\1 \2/p' "$@"; }
has_data() { pane "$1" | grep -F "$LOGO" | tail -n 1 | grep -qE 'ctx [0-9]'; }
log_has() { grep -q "$1" "$SD/session.log" 2>/dev/null; }
# wcount: the number of width-source lines in the session log; log_more <n>: there are more than <n>.
wcount() { wsrc "$SD/session.log" 2>/dev/null | grep -c . || true; }
log_more() { [ "$(wcount)" -gt "$1" ]; }

# ---- session a: compact, 200 columns, one short turn; then 70 columns; then 19 columns ----
A_WHY="" SD="" SGR='{"ok":false,"why":"no capture"}' R70='{"class":"none"}' R19='{"class":"none"}' WA="" W19=""
hud_start a 200 "$HUD/zygo.toml" "$HUD/dump-a.json" ZYGO_TEST_REFRESH_INTERVAL=2
if hud_wait a; then
  tmux_send a "Reply with the single word ready. Use no tool." || true
  wait_until 120 has_data a || echo "live-hud: no ctx numbers in the HUD within 120 s; the run goes on" >&2
  sleep 3
  tmux capture-pane -e -p -t "=a:" >"$HUD/pane-a-200.ans" 2>/dev/null || true
  SGR=$(hud_an sgr "$HUD/pane-a-200.ans" 2>/dev/null) || SGR='{"ok":false,"why":"analyzer error"}'
  cp "$HUD/dump-a.json" "$HUD/dump-a.keep.json" 2>/dev/null || true
  SD=$(jq -r '.env.ZYGO_SESSION_DIR // empty' "$HUD/dump-a.keep.json" 2>/dev/null || true)
  if [ -n "$SD" ] && [ -e "$SD/session.log" ]; then
    WA=$(wsrc "$SD/session.log" | sed -n 1p)
    tmux resize-window -t "=a:" -x 70 -y 50 || true
    # The hook logs a width line when the source or the width changes. The source at 70 columns is
    # the one that answers (Claude Code 2.1.330: COLUMNS; /dev/tty does not answer the hook).
    if wait_until 20 log_has 'width source [^,]*, width 70$'; then sleep 3; pane a >"$HUD/pane-a-70.txt"; fi
    N19=$(wcount)
    tmux resize-window -t "=a:" -x 19 -y 50 || true
    # 19 is below the 20-column limit of the hook, so a new line names the source that answers then.
    if wait_until 20 log_more "$N19"; then W19=$(wsrc "$SD/session.log" | sed -n "$((N19 + 1))p"); sleep 3; pane a >"$HUD/pane-a-19.txt"; fi
    cp "$SD/session.log" "$HUD/session-a.log" 2>/dev/null || true
    # The expected rows: the hook run by hand with no terminal, on the session's own data.
    if [ -s "$HUD/pane-a-70.txt" ]; then
      hud_an expect "$HUD/dump-a.keep.json" "$ZYGO_BIN" "$WT" 70 "$HUD/expect-70.txt" >/dev/null 2>&1 || true
      R70=$(hud_an fit "$HUD/pane-a-70.txt" "$HUD/expect-70.txt" 70 2>/dev/null) || R70='{"class":"none"}'
    fi
    if [ -s "$HUD/pane-a-19.txt" ]; then
      case "$W19" in "COLUMNS "*) _c=${W19#COLUMNS } ;; *) _c=none ;; esac
      hud_an expect "$HUD/dump-a.keep.json" "$ZYGO_BIN" "$WT" "$_c" "$HUD/expect-19.txt" >/dev/null 2>&1 || true
      R19=$(hud_an fit "$HUD/pane-a-19.txt" "$HUD/expect-19.txt" 19 2>/dev/null) || R19='{"class":"none"}'
    fi
  fi
else
  A_WHY=$HUD_WHY
fi
hud_stop a

# ---- session b: verbose, 200 columns, no turn ----
B_WHY="" TWO='{"rows":0,"logo_col":-1,"refl_col":-1}'
hud_start b 200 "$HUD/zygo-verbose.toml" "$HUD/dump-b.json"
if hud_wait b; then
  sleep 3
  tmux capture-pane -e -p -t "=b:" >"$HUD/pane-b-200.ans" 2>/dev/null || true
  TWO=$(hud_an two "$HUD/pane-b-200.ans" 2>/dev/null) || TWO='{"rows":0,"logo_col":-1,"refl_col":-1}'
else
  B_WHY=$HUD_WHY
fi
hud_stop b

# ---- session c: refreshInterval 2 s and a status fetch that starts 5 s late; 45 s of process samples ----
python3 -I "$PY/hud_poll.py" "$HUD/events.json" "$ZYGO_BIN" 45 "$HUD/dump-c.json" &
LIVE_PIDS=$!
hud_start c 200 "$HUD/zygo.toml" "$HUD/dump-c.json" ZYGO_TEST_REFRESH_INTERVAL=2 ZYGO_TEST_FETCH_DELAY_MS=5000
hud_wait c || true
wait "$LIVE_PIDS" || true
LIVE_PIDS=""
hud_stop c
P12=$(hud_an ph12 "$HUD/events.json" 3 2>/dev/null) || P12='{"class":"none","runs":0}'

# ---- session d: outside tmux, on a pseudo-terminal of 100 columns, no turn ----
WP=""
# shellcheck disable=SC2046
python3 -I "$PY/hud_pty.py" "$HUD/dump-d.json" "$HUD/session-d.log" 100 60 "$WT" \
  "$(command -v env)" -i $(hud_env "$HUD/zygo.toml" "$HUD/dump-d.json") "$ZYGO_BIN" claude --passthrough &
LIVE_PIDS=$!
D_RC=0
wait "$LIVE_PIDS" || D_RC=$?
LIVE_PIDS=""
case "$D_RC" in
  0) D_WHY="" ;;
  3) D_WHY="pseudo-terminal session: no dump file from the hook within 60 s" ;;
  4) D_WHY="pseudo-terminal session: no copy of session.log" ;;
  5) D_WHY="pseudo-terminal session: Claude Code showed the folder trust question, which the script does not answer there" ;;
  *) D_WHY="pseudo-terminal session: the driver ended with status $D_RC" ;;
esac
[ -s "$HUD/session-d.log" ] && WP=$(wsrc "$HUD/session-d.log" | sed -n 1p)

# ---- the LIVE lines of the passthrough parts ----
j() { printf '%s' "$1" | jq -r "$2"; }
if [ -n "$A_WHY" ]; then
  line PH1 failed "$A_WHY"
elif [ "$(j "$SGR" .ok)" = true ]; then
  line PH1 verified "capture-pane -e (tmux $TMUX_VER): stripe fg $(j "$SGR" '.stripe | join(",")'), logo bold fg 15, worktree name fg 172 (38;5;n)"
else
  line PH1 failed "16-color part $(j "$SGR" '.ok16 // false'), 256-color part $(j "$SGR" '.ok256 // false'): $SGR"
fi
if [ -n "$B_WHY" ]; then
  line PH2 failed "$B_WHY"
elif [ "$(j "$TWO" .rows)" = 2 ]; then
  line PH2 verified "verbose shows two rows: the logo row, then the reflection row (tmux $TMUX_VER, 200 columns)"
else
  line PH2 failed "verbose shows $(j "$TWO" .rows) row(s): $TWO"
fi
C19=$(j "$R19" .class)
T3="at 70 columns the row is $(j "$R70" .class) ($(j "$R70" '.shown_cells // "-"') of $(j "$R70" '.expected_cells // "-"') cells shown); at 19 columns (width source ${W19:-none}) a row of $(j "$R19" '.expected_cells // "-"') cells is $C19"
if [ -n "$A_WHY" ]; then
  line PH3 failed "$A_WHY"
elif [ "$C19" = cut ]; then
  line PH3 verified "cut: $T3"
elif [ "$C19" = wrapped ]; then
  line PH3 failed "wrapped: $T3"
else
  line PH3 failed "not observed: $T3"
fi
CA=$(jq -r '.env.COLUMNS // "unset"' "$HUD/dump-a.keep.json" 2>/dev/null || echo unset)
CP=$(jq -r '.env.COLUMNS // "unset"' "$HUD/dump-d.json" 2>/dev/null || echo unset)
GAP=$(j "$R19" 'if .gap == null then "unknown" else (.indent + .gap | tostring) end')
T4="(a) width source in tmux at 200 columns: ${WA:-none}; on a pseudo-terminal of 100 columns: ${WP:-none}; (b) COLUMNS in the hook: tmux $CA, pseudo-terminal $CP; (c) status area = pane width - $GAP at 19 columns (indent $(j "$R19" '.indent // "-"'), right gap $(j "$R19" '.gap // "-"')), row at 70 columns: $(j "$R70" .class)"
if [ "$WA" = "tty 200" ] && [ "$WP" = "tty 100" ] && [ "$CA" != unset ] && [ "$CP" != unset ] && [ "$GAP" = 0 ]; then
  line PH4 verified "$T4"
else
  line PH4 failed "${A_WHY:+$A_WHY; }${D_WHY:+$D_WHY; }$T4"
fi
if [ ! -s "$HUD/dump-a.keep.json" ]; then
  line PH5 failed "${A_WHY:-no dump file from the hook}"
elif jq -e '(.effort | type) as $t | $t == "string" or ($t == "object" and (.effort.level | type) == "string")' "$HUD/dump-a.keep.json" >/dev/null; then
  line PH5 verified "stdin effort is $(jq -c .effort "$HUD/dump-a.keep.json") ($(jq -r '.effort | type' "$HUD/dump-a.keep.json")); stdin keys: $(jq -r '.stdin_keys | join(" ")' "$HUD/dump-a.keep.json")"
else
  line PH5 failed "stdin effort is $(jq -c .effort "$HUD/dump-a.keep.json") ($(jq -r '.effort | type' "$HUD/dump-a.keep.json")); stdin keys: $(jq -r '.stdin_keys | join(" ")' "$HUD/dump-a.keep.json")"
fi
if [ -n "$B_WHY" ]; then
  line PH9 failed "$B_WHY"
elif [ "$(j "$TWO" .rows)" = 2 ] && [ "$(j "$TWO" .logo_col)" -ge 0 ] && [ "$(j "$TWO" .logo_col)" = "$(j "$TWO" .refl_col)" ]; then
  line PH9 verified "logo and reflection start at column $(j "$TWO" .logo_col) of the pane (tmux $TMUX_VER); the glyphs of the font are not checked"
else
  line PH9 failed "logo column $(j "$TWO" .logo_col), reflection column $(j "$TWO" .refl_col), rows $(j "$TWO" .rows)"
fi
case "$(j "$P12" .class)" in
  kills) _t="Claude Code ends the run early" ;;
  waits) _t="Claude Code waits for the run before it starts the next" ;;
  overlaps) _t="Claude Code starts the next run while the first still goes, and does not end the first" ;;
  *) _t="" ;;
esac
if [ -n "$_t" ]; then
  line PH12 verified "observation, refreshInterval 2 s: the first slow run lived $(j "$P12" .life) s of its 3 s bound; the next run started $(j "$P12" '.next_start // "never"') s after its start: $_t; $(j "$P12" .runs) runs in 45 s"
else
  line PH12 failed "not observed: no status line run of 1 s or more in 45 s ($(j "$P12" .runs) runs seen)"
fi

# ---- fixtures of the passthrough parts (RUN is still the hud run, so scrub maps its paths) ----
# fx <file> <fixture name>: a scrubbed copy, when the file has content.
fx() { if [ -s "$1" ]; then scrub "$1" >"$FX/$2"; fi; }
mask='.env.ZYGO_SESSION_DIR |= (if . == null then null else "[set]" end) | .env.ZYGO_URL |= (if . == null then null else "[set]" end)'
if [ -s "$HUD/pane-a-200.ans" ]; then hud_an rows "$HUD/pane-a-200.ans" 1 raw >"$HUD/fx-compact-200.txt" || true; fi
if [ -s "$HUD/pane-b-200.ans" ]; then hud_an rows "$HUD/pane-b-200.ans" 2 raw >"$HUD/fx-verbose-200.txt" || true; fi
if [ -s "$HUD/pane-a-70.txt" ]; then hud_an rows "$HUD/pane-a-70.txt" 4 plain >"$HUD/fx-compact-70.txt" || true; fi
if [ -s "$HUD/pane-a-19.txt" ]; then hud_an rows "$HUD/pane-a-19.txt" 8 plain >"$HUD/fx-compact-19.txt" || true; fi
if [ -s "$HUD/dump-a.keep.json" ]; then jq "$mask" "$HUD/dump-a.keep.json" >"$HUD/fx-dump-tmux.json" || true; fi
if [ -s "$HUD/dump-d.json" ]; then jq "$mask" "$HUD/dump-d.json" >"$HUD/fx-dump-pty.json" || true; fi
{
  [ -e "$HUD/session-a.log" ] && wsrc "$HUD/session-a.log" | sed 's/^/tmux session a (200, then 70, then 19 columns): /'
  [ -e "$HUD/session-d.log" ] && wsrc "$HUD/session-d.log" | sed 's/^/pseudo-terminal, 100 columns: /'
  true
} >"$HUD/fx-width.txt"
jq -n --argjson sgr "$SGR" --argjson two "$TWO" --argjson c70 "$R70" --argjson c19 "$R19" \
  '{sgr: $sgr, two: $two, "70": $c70, "19": $c19}' >"$HUD/fx-fit.json" || true
if [ -s "$HUD/events.json" ]; then
  jq -n --slurpfile ev "$HUD/events.json" --argjson r "$P12" '{events: $ev[0], result: $r}' >"$HUD/fx-ph12.json" || true
fi
fx "$HUD/fx-compact-200.txt" p-hud-1-compact-200.txt
fx "$HUD/fx-verbose-200.txt" p-hud-1-verbose-200.txt
fx "$HUD/fx-compact-70.txt" p-hud-1-compact-70.txt
fx "$HUD/fx-compact-19.txt" p-hud-1-compact-19.txt
fx "$HUD/expect-70.txt" p-hud-1-expect-70.txt
fx "$HUD/expect-19.txt" p-hud-1-expect-19.txt
fx "$HUD/fx-dump-tmux.json" p-hud-1-dump-tmux.json
fx "$HUD/fx-dump-pty.json" p-hud-1-dump-pty.json
fx "$HUD/fx-width.txt" p-hud-1-width.txt
fx "$HUD/fx-fit.json" p-hud-1-fit.json
fx "$HUD/fx-ph12.json" p-hud-1-ph12.json

# ---- C7: mock upstream and a temporary HOME, as P-N28-1; no model request ----
live_run c7
start_mock
mock_config "$RUN/zygo.toml"
printf '[statusline]\nstyle = "verbose"\n' >>"$RUN/zygo.toml"
LIVE_VARS="ZYGO_NERD_FONT=1 ZYGO_NO_UPDATE_CHECK=1"
tmux_zygo c7 claude --compact 777k
tmux_wait_text c7 "$LOGO" 60 || true
tmux_send c7 "tracer-hud say hi" || true
c7_seen() { [ -n "$(body_has 'tracer-hud' "$RUN"/rec/[0-9]*.json 2>/dev/null)" ]; }
wait_until 90 c7_seen || true
sleep 5
pane c7 >"$RUN/pane.txt"
tmux kill-session -t "=c7" 2>/dev/null || true
stop_mock
N_ALL=$(find "$RUN/rec" -name '[0-9]*.json' | grep -c . || true)
N_TR=$(body_has 'tracer-hud' "$RUN"/rec/[0-9]*.json | grep -c . || true)
LEAK=$(body_has "$LOGO|$REFL|777k|777000" "$RUN"/rec/[0-9]*.json | sed 's#.*/##' | tr '\n' ' ')
if grep -qF "$LOGO" "$RUN/pane.txt"; then SHOWN=yes; else SHOWN=no; fi
if [ "$SHOWN" = yes ] && [ "$N_TR" -gt 0 ] && [ -z "$LEAK" ]; then
  line C7 verified "HUD shown (verbose, --compact 777k); the logo, the reflection, 777k and 777000 are in none of $N_ALL recorded requests ($N_TR hold the tracer prompt); mock upstream"
else
  line C7 failed "HUD shown: $SHOWN; recorded requests: $N_ALL; with the tracer prompt: $N_TR; requests that hold HUD text: ${LEAK:-none}; mock upstream"
fi
jq -n --argjson all "$N_ALL" --argjson tr "$N_TR" --arg leak "$LEAK" \
  '{requests: $all, tracer_requests: $tr, markers: ["logo (U+1D607 U+1D606 U+1D5F4 U+1D5FC)", "reflection (z U+028E U+0183 o)", "777k", "777000"], requests_with_a_marker: ($leak | split(" ") | map(select(. != "")))}' >"$RUN/fx-c7.json" || true
hud_an rows "$RUN/pane.txt" 2 plain >"$RUN/fx-c7-pane.txt" || true
fx "$RUN/fx-c7.json" p-hud-1-c7.json
fx "$RUN/fx-c7-pane.txt" p-hud-1-c7-pane.txt

# ---- the run record and the eight lines ----
jq -n --arg d "$(date +%Y-%m-%d)" --arg cc "$LIVE_CC_VER" --arg t "$TMUX_VER" --arg os "$(uname -s)" \
  '{date: $d, claude_code: $cc, tmux: $t, os: $os}' >"$HUD/run.json"
fx "$HUD/run.json" p-hud-1-run.json
fx "$HUD/lines.txt" p-hud-1-results.txt
live_done "$FX"

#!/usr/bin/env python3
"""Probe status line command and analyzer of node PRB-1 (HUD design H§12 "Probe first").

Run by scripts/premises/live-hud-probe.sh; tested by scripts/premises/hud_probe_test.py. No network.

  hud_probe.py settings <python> <this file> <dir> <refresh> <out>   the --settings JSON for Claude Code
  hud_probe.py statusline <dir>                                    the status line command (phase from <dir>/phase)
  hud_probe.py capture <socket> <session> <dir> <phase> <seconds> <escapes 0|1>
  hud_probe.py widths <socket> <session> <out>                     tmux's own cell width of each probe glyph
  hud_probe.py pty <log dir> <cols> <wait> <cwd> <argv...>         one session on a pseudo-terminal
  hud_probe.py ph13 <opencode> <work dir> <out>                    OpenCode help and inline config keys
  hud_probe.py analyze <tmux dir> <pty dir> <pane cols> <pty cols> <widths json> <stdin out>   JSON on stdout
  hud_probe.py fixtures <tmux dir> <pty dir> <analysis json> <ph13 json> <out dir>
"""
import errno
import fcntl
import glob
import json
import os
import re
import select
import signal
import struct
import subprocess
import sys
import termios
import time

E = "\x1b"
LOGO = "\U0001d607\U0001d606\U0001d5f4\U0001d5fc"
REFL = "zʎƃo"
STRIPE = "▌"
PUA = "\U000f0645"
SCISSORS = "✂"
VS16 = "️"
# The two OP13 printf commands of H§6.3, as bytes (\e -> ESC, %% -> %).
VERBOSE = (f"{E}[31m▌{E}[33m▌{E}[36m▌{E}[0m {E}[1;97m{LOGO}{E}[0m  {E}[1;34m main{E}[0m {E}[38;5;172m{PUA} hud-work{E}[0m {E}[36m↑1{E}[0m   {E}[35m opus-4.7 · high{E}[0m   {E}[36m\U000f0626 fast{E}[0m   {E}[2mlocal{E}[0m\n"
           f"{E}[31m▌{E}[33m▌{E}[36m▌{E}[0m {E}[2;37m{REFL}{E}[0m  ctx 196k/{E}[2m1m{E}[0m {E}[32m▰▰{E}[2m▱▱▱▱▱▱{E}[0m 20%   {E}[2m↑ 1.2m in  ↓ 48k out{E}[0m   {E}[32m$3.41{E}[0m   {E}[2mcache 92%   trimmed 23k tok{E}[0m\n")
COMPACT = (f"{E}[31m▌{E}[33m▌{E}[36m▌{E}[0m {E}[1;97m{LOGO}{E}[0m {E}[1;34m main{E}[0m {E}[38;5;172m{PUA} hud-work{E}[0m  {E}[35m opus-4.7{E}[0m {E}[36m\U000f0626 fast{E}[0m  ctx 196k/{E}[2m1m{E}[0m {E}[32m▰{E}[2m▱▱▱▱{E}[0m  {E}[2m↑1.2m ↓48k{E}[0m  {E}[32m$3.41{E}[0m  {E}[2m\U000f01bc 92%  {SCISSORS} 23k{E}[0m\n")
# 300 ASCII cells: block k (1..30) is the number 10k right-aligned in 10 cells after dots, so the
# number 10k ends at ruler cell 10k.
RULER = "".join("." * (10 - len(str(10 * k))) + str(10 * k) for k in range(1, 31))
# Wide phases: row 1 is a short title, row 2 the measured row (row 2 has the same area in each phase).
WIDE = {"A": ("", 0), "B": (PUA, 10), "C": (SCISSORS, 10), "D": (LOGO, 5)}
CELLS = {"A": 0, "B": 10, "C": 10, "D": 20}   # the glyph cells of row 2 when each code point is 1 cell
ENV_KEEP = ("COLUMNS", "LINES", "TERM", "TERM_PROGRAM", "TERM_PROGRAM_VERSION", "COLORTERM", "NO_COLOR",
            "FORCE_COLOR", "WT_SESSION", "LANG", "LC_ALL", "LC_CTYPE", "CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT")
CSI = re.compile(r"\x1b\[[0-9;:?]*[A-Za-z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")


def wide_rows(x):
    g, n = WIDE[x]
    return f"P|wide{x} probe\n{x}|{g * n}{RULER}\n"


def out_for(phase, pid):
    if phase == "verbose":
        return VERBOSE
    if phase == "compact":
        return COMPACT
    if phase == "early":
        return f"PH11 early {pid}\n"
    if phase.startswith("wide") and phase[4:] in WIDE:
        return wide_rows(phase[4:])
    if phase == "slow":
        return f"PH12 slow {pid}\n"
    if phase == "done":
        return "PROBE DONE\n"
    return f"PROBE {phase}\n"


# ---- the status line command ------------------------------------------------------------------

def event(d, **kv):
    kv["t"] = round(time.time(), 3)
    line = (json.dumps(kv, sort_keys=True) + "\n").encode()
    fd = os.open(os.path.join(d, "events.jsonl"), os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    try:
        os.write(fd, line)
    finally:
        os.close(fd)


def tty_size():
    try:
        fd = os.open("/dev/tty", os.O_RDONLY | os.O_NOCTTY)
    except OSError as e:
        return {"error": errno.errorcode.get(e.errno, str(e.errno))}
    try:
        rows, cols, _, _ = struct.unpack("HHHH", fcntl.ioctl(fd, termios.TIOCGWINSZ, b"\0" * 8))
        return {"cols": cols, "rows": rows}
    except OSError as e:
        return {"error": errno.errorcode.get(e.errno, str(e.errno))}
    finally:
        os.close(fd)


def read_stdin(limit=1 << 20, wait=2.0):
    """At most <limit> bytes until end of file, for at most <wait> seconds."""
    buf, end = b"", time.monotonic() + wait
    while len(buf) < limit:
        left = end - time.monotonic()
        if left <= 0 or not select.select([0], [], [], left)[0]:
            return buf, "deadline"
        chunk = os.read(0, 65536)
        if not chunk:
            return buf, "eof"
        buf += chunk
    return buf[:limit], "limit"


def cmd_statusline(d):
    try:
        phase = open(os.path.join(d, "phase"), encoding="utf-8").read().strip() or "verbose"
    except OSError:
        phase = "verbose"
    pid = os.getpid()
    if phase == "early":
        os.close(0)   # PH11: stdin is closed unread, before anything else
    event(d, pid=pid, phase=phase, event="start")

    def on_signal(n, _f):
        event(d, pid=pid, phase=phase, event="signal", sig=signal.Signals(n).name[3:])
        os._exit(128 + n)

    for s in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP, signal.SIGPIPE):
        signal.signal(s, on_signal)
    raw = None
    if phase == "early":
        stdin, how = "not read", "closed"
    else:
        raw, how = read_stdin()
        try:
            stdin = json.loads(raw.decode("utf-8", "replace"))
        except ValueError:
            stdin = {"unparsed": raw[:200].decode("utf-8", "replace")}
    env = {k: os.environ[k] for k in ENV_KEEP if k in os.environ}
    for k in ("TMUX", "TMUX_PANE"):
        if k in os.environ:
            env[k] = "set"
    rec = {"t": round(time.time(), 3), "pid": pid, "phase": phase, "tty": tty_size(), "env": env,
           "env_names": sorted(os.environ), "stdin": stdin, "stdin_end": how,
           "stdin_bytes": None if raw is None else len(raw)}
    os.makedirs(os.path.join(d, "log"), exist_ok=True)
    name = os.path.join(d, "log", f"run-{time.time_ns()}-{pid}-{phase}.json")
    with open(name + ".tmp", "w", encoding="utf-8") as f:
        json.dump(rec, f)
    os.replace(name + ".tmp", name)
    if phase == "slow":
        for _ in range(10):   # 5 s, one beat each 0.5 s
            event(d, pid=pid, phase=phase, event="beat")
            time.sleep(0.5)
    try:
        sys.stdout.write(out_for(phase, pid))
        sys.stdout.flush()
    except BrokenPipeError:
        event(d, pid=pid, phase=phase, event="stdout-error")
        os._exit(1)
    event(d, pid=pid, phase=phase, event="end")
    return 0


# ---- driving helpers --------------------------------------------------------------------------

def tmux(sock, *args):
    return subprocess.run(["tmux", "-u", "-L", sock, "-f", "/dev/null", *args], capture_output=True,
                          text=True, timeout=10)


def cmd_capture(sock, session, d, phase, seconds, escapes):
    """Captures the pane every 0.5 s for <seconds> (at least once) into <d>/cap/<phase>-NNN.txt."""
    cap = os.path.join(d, "cap")
    os.makedirs(cap, exist_ok=True)
    n = max(1, int(float(seconds) * 2))
    flags = ["-e", "-p"] if escapes == "1" else ["-p"]
    with open(os.path.join(d, "captures.jsonl"), "a", encoding="utf-8") as idx:
        for i in range(n):
            t = time.time()
            p = tmux(sock, "capture-pane", *flags, "-t", f"={session}:")
            if p.returncode != 0:
                break
            f = os.path.join(cap, f"{phase}-{i:03d}.txt")
            open(f, "w", encoding="utf-8").write(p.stdout)
            idx.write(json.dumps({"phase": phase, "file": f, "t": round(t, 3)}) + "\n")
            if i < n - 1:
                time.sleep(max(0.0, t + 0.5 - time.time()))
    return 0


def cmd_widths(sock, session, out):
    """tmux's own width of each glyph (#{w:} of a user option on the probe's own session)."""
    r = {}
    for k, g in (("B", PUA), ("C", SCISSORS), ("D", LOGO[0]), ("ellipsis", "…")):
        a = tmux(sock, "set-option", "-t", f"={session}:", "@zygo_probe_glyph", g)
        b = tmux(sock, "display-message", "-p", "-t", f"={session}:", "#{w:@zygo_probe_glyph}")
        v = b.stdout.strip()
        r[k] = int(v) if a.returncode == 0 and b.returncode == 0 and v.isdigit() else None
    json.dump(r, open(out, "w"))
    return 0


def cmd_pty(logdir, cols, wait, cwd, *argv):
    """Starts argv on a pseudo-terminal of <cols> x 50 in <cwd>; waits <wait> s for a run log in
    <logdir>, then /exit. Exit 0: a run log; 3: none; 5: the folder trust question showed (not
    answered here: Enter alone means "No, exit" in Claude Code 2.1.330)."""
    import pty
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(cwd)
        os.execv(argv[0], list(argv))
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 50, int(cols), 0, 0))
    raw = re.compile(rb"\x1b\[[0-9;:?]*[A-Za-z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")
    state = {"buf": b""}

    def pump(seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if select.select([fd], [], [], 0.2)[0]:
                try:
                    data = os.read(fd, 65536)
                except OSError:
                    return False
                if not data:
                    return False
                state["buf"] = (state["buf"] + data)[-65536:]
        return True

    code = 3
    try:
        end = time.monotonic() + float(wait)
        while time.monotonic() < end and pump(0.5):
            if glob.glob(os.path.join(logdir, "run-*.json")):
                code = 0
                break
            if b"Itrustthisfolder" in re.sub(rb"\s+", b"", raw.sub(b"", state["buf"])):
                code = 5
                break
        if code == 0:
            pump(1)
            os.write(fd, b"/exit")
            pump(0.5)
            os.write(fd, b"\r")
            end = time.monotonic() + 10
            while time.monotonic() < end and os.waitpid(pid, os.WNOHANG)[0] == 0:
                pump(0.2)
    finally:
        # A second TERM (or an INT, a HUP) must not stop the end of the session (round 2, finding 8).
        for s in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            signal.signal(s, signal.SIG_IGN)
        for sig in (signal.SIGHUP, signal.SIGKILL):
            try:
                if os.waitpid(pid, os.WNOHANG)[0] != 0:
                    break
                os.killpg(pid, sig)
                time.sleep(2)
            except (ProcessLookupError, ChildProcessError):
                break
    return code


def cmd_ph13(oc, work, out):
    """PH13: no status line in OpenCode's help texts, and no status line key in its inline config:
    `opencode debug config --pure` keeps a known key (username) and drops an unknown one; each
    candidate key is then dropped (no such key) or kept (OpenCode has that key)."""
    os.makedirs(work, exist_ok=True)
    home = os.path.join(work, "home")
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": home,
           "XDG_CONFIG_HOME": os.path.join(home, ".config"), "XDG_DATA_HOME": os.path.join(home, ".local/share"),
           "XDG_CACHE_HOME": os.path.join(home, ".cache"), "XDG_STATE_HOME": os.path.join(home, ".local/state"),
           "OPENCODE_DISABLE_AUTOUPDATE": "1"}
    for k in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"):
        os.makedirs(env[k], exist_ok=True)
    rx = re.compile(r"(?i)status[ _-]?line")

    def run(args, extra=None):
        try:
            p = subprocess.run([oc, *args], capture_output=True, text=True, timeout=30, cwd=work,
                               env=dict(env, **(extra or {})), stdin=subprocess.DEVNULL)
            return p.returncode, p.stdout, p.stderr
        except subprocess.TimeoutExpired:
            return 124, "", "timeout after 30 s"

    r = {"version": run(["--version"])[1].strip()}
    hits, helps = [], {}
    for args in (["--help"], ["debug", "--help"]):
        rc, o, e = run(args)
        hits += [" ".join(args) + ": " + m.group(0) for m in rx.finditer(o + e)]
        # Round 2, B2: a help text counts only when the call exits 0 with output (a timeout is 124).
        helps[" ".join(args)] = {"rc": rc, "bytes": len((o + e).encode())}
    r["help_hits"] = hits
    r["help"] = helps

    def key(k, value):
        rc, o, e = run(["debug", "config", "--pure"], {"OPENCODE_CONFIG_CONTENT": json.dumps({k: value})})
        try:
            d = json.loads(o)
        except ValueError:
            d = None
        if rc == 0 and isinstance(d, dict):
            return "kept" if k in d else "dropped"
        return "rejected" if rc != 0 and k in (o + e) else f"error (exit {rc})"

    cmd = {"type": "command", "command": "true"}
    r["control"] = {"username": key("username", "zygo-probe"), "zygoProbeUnknownKey": key("zygoProbeUnknownKey", cmd)}
    r["keys"] = {k: key(k, cmd) for k in ("statusLine", "statusline", "status_line")}
    r["help_measured"] = bool(r["version"]) and all(h["rc"] == 0 and h["bytes"] > 0 for h in helps.values())
    r["pass"] = (r["help_measured"] and not hits and r["control"] == {"username": "kept", "zygoProbeUnknownKey": "dropped"}
                 and all(v == "dropped" for v in r["keys"].values()))
    json.dump(r, open(out, "w"), indent=1)
    return 0


# ---- analysis ---------------------------------------------------------------------------------

def plain(s):
    return CSI.sub("", s)


def captures(d, phase):
    out = []
    try:
        for line in open(os.path.join(d, "captures.jsonl"), encoding="utf-8"):
            c = json.loads(line)
            if c["phase"] == phase and os.path.exists(c["file"]):
                out.append((c["t"], open(c["file"], encoding="utf-8", errors="replace").read()))
    except OSError:
        pass
    return out


def block(text):
    """The pane lines from the last line that holds the logo to the last non-empty line, SGR kept."""
    lines = text.split("\n")
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


def an_sgr(text):
    b = block(text)
    if not b:
        return {"ok": False, "ok16": False, "ok256": False, "why": "no logo row"}
    a = attrs(b[0])
    t = "".join(c for c, *_ in a)
    s, l, w = t.find(STRIPE * 3), t.find(LOGO), t.find("hud-work")
    stripe = [a[s + n][1] for n in range(3)] if s >= 0 else []
    logo = [a[l][1], a[l][2]] if l >= 0 else [None, False]
    tree = a[w][1] if w >= 0 else None
    r = {"stripe": stripe, "logo_fg": logo[0], "logo_bold": logo[1], "worktree_fg": tree}
    r["ok16"] = stripe == [1, 3, 6] and logo[0] == 15 and logo[1] is True
    r["ok256"] = tree == 172
    r["ok"] = r["ok16"] and r["ok256"]
    return r


def an_two(text):
    b = [plain(l) for l in block(text)]
    if not b:
        return {"rows": 0, "logo_col": -1, "refl_col": -1}
    lc = b[0].find(LOGO)
    rc = b[1].find(REFL) if len(b) > 1 else -1
    return {"rows": 2 if rc >= 0 and STRIPE in b[1] and REFL not in b[0] else 1, "logo_col": lc, "refl_col": rc}


def an_compact(text):
    t = text
    return {"scissors": SCISSORS in plain(t), "vs16": (SCISSORS + VS16) in t}


def an_wide(x, text, cols):
    """Row 2 of a wide phase: the ruler cells it shows after its glyphs, and cut or wrap."""
    g, n = WIDE[x]
    lines = [plain(l) for l in text.split("\n")]
    at = [i for i, l in enumerate(lines) if l.lstrip(" ").startswith(f"{x}|")]
    if not at:
        return {"class": "none", "why": f"no {x}| row in the pane"}
    i = at[-1]
    line = lines[i].rstrip()
    indent = len(line) - len(line.lstrip(" "))
    rest = line.lstrip(" ")[2:]
    if not rest.startswith(g * n):
        return {"class": "none", "why": f"the {x}| row does not start with its {n} glyphs", "indent": indent}
    t = rest[len(g * n):]
    ell = t.endswith("…")
    t = (t[:-1] if ell else t).rstrip()
    if not RULER.startswith(t):
        return {"class": "other", "why": f"the {x}| row does not show a ruler prefix", "indent": indent}
    nxt = lines[i + 1].strip() if i + 1 < len(lines) else ""
    wrapped = len(nxt) >= 3 and RULER[len(t):].startswith(nxt[:10])
    r = {"ruler": len(t), "ellipsis": ell, "indent": indent, "shown": len(line) - indent, "cols": int(cols)}
    r["class"] = "wrapped" if wrapped else "cut" if len(t) < len(RULER) else "whole"
    if x == "A":
        r["gap"] = int(cols) - len(line)
        r["loss"] = indent + r["gap"]
    return r


def runs(d):
    out = []
    for f in sorted(glob.glob(os.path.join(d, "log", "run-*.json"))):
        try:
            out.append(json.load(open(f)))
        except (OSError, ValueError):
            pass
    return sorted(out, key=lambda r: r["t"])


def events(d):
    out = []
    try:
        for line in open(os.path.join(d, "events.jsonl"), encoding="utf-8"):
            try:
                out.append(json.loads(line))
            except ValueError:
                pass
    except OSError:
        pass
    return sorted(out, key=lambda e: e["t"])


def by_run(evs, phase):
    rs = {}
    for e in evs:
        if e.get("phase") != phase:
            continue
        r = rs.setdefault(e["pid"], {"pid": e["pid"], "start": None, "end": None, "signal": None, "last": None})
        if e["event"] == "start":
            r["start"] = e["t"]
        elif e["event"] == "end":
            r["end"] = e["t"]
        elif e["event"] == "signal":
            r["signal"] = (e["t"], e.get("sig"))
        r["last"] = e["t"]
    return sorted((r for r in rs.values() if r["start"] is not None), key=lambda r: r["start"])


def an_ph11(d):
    """Early runs (round 2, B1): a run that wrote stdout-error or has no end event is lost; a run that
    ended 2.5 s or more before the last capture (its line had time to show) and never showed is lost.
    ok: 3 or more such judged runs, each shown, none lost, no error text, PROBE DONE shown later."""
    allev = events(d)
    rs = by_run(allev, "early")
    pids = {r["pid"] for r in rs}
    errs = {e["pid"] for e in allev if e.get("phase") == "early" and e.get("event") == "stdout-error"} & pids
    caps = captures(d, "early")
    texts = [plain(t) for _, t in caps]
    seen = {int(m.group(1)) for t in texts for m in re.finditer(r"PH11 early (\d+)", t)}
    last = caps[-1][0] if caps else 0
    judged = [r for r in rs if r["end"] is not None and r["end"] <= last - 2.5]
    lost = [r["pid"] for r in rs
            if r["pid"] in errs or r["end"] is None or (r["end"] <= last - 2.5 and r["pid"] not in seen)]
    err = any(re.search(r"(?i)EPIPE|status ?line[^\n]*(error|fail)", t) for t in texts)
    after = any("PROBE DONE" in plain(t) for _, t in captures(d, "done"))
    ok = len(judged) >= 3 and all(r["pid"] in seen for r in judged) and not lost and not err and after
    return {"runs": len(rs), "ended": sum(1 for r in rs if r["end"] is not None),
            "shown": sum(1 for r in rs if r["pid"] in seen), "lost": len(lost), "stdout_errors": len(errs),
            "no_end": sum(1 for r in rs if r["end"] is None), "judged": len(judged), "error": err,
            "after": after, "captures": len(caps), "ok": ok}


def an_ph12(d):
    rs = by_run(events(d), "slow")
    caps = captures(d, "slow")
    shown = any("PH12 slow" in plain(t) for _, t in caps)
    sigs = sorted({r["signal"][1] for r in rs if r["signal"]})
    stop = []
    for r in rs:
        if r["end"] is not None:
            stop.append((r["end"], "end"))
        elif r["signal"]:
            stop.append((r["signal"][0], "signal"))
        else:
            stop.append((r["last"], "lost"))
    cls = "none"
    for i in range(len(rs) - 1):
        t_stop, how = stop[i]
        nxt = rs[i + 1]["start"]
        if how == "end":
            cls = "overlaps" if nxt < t_stop - 0.2 else "waits"
        elif how == "signal":
            cls = "kills" if t_stop - rs[i]["start"] < 4.5 else "waits"
        # how == "lost" (no end, no signal: a dead run, round 2): not observed, class stays "none"
        break
    pts = sorted([(r["start"], 1) for r in rs] + [(s[0], -1) for s in stop], key=lambda p: (p[0], p[1]))
    cur = maxc = 0
    for _, dlt in pts:
        cur += dlt
        maxc = max(maxc, cur)
    lives = [round(stop[i][0] - r["start"], 2) for i, r in enumerate(rs)]
    return {"starts": len(rs), "ends": sum(1 for s in stop if s[1] == "end"), "signals": sigs,
            "no_end": sum(1 for s in stop if s[1] == "lost"), "max_concurrent": maxc, "class": cls,
            "shown": shown, "lives": lives}


def an_stdin(t_runs, p_runs):
    out = []
    for sess, rs in (("tmux", t_runs), ("pty", p_runs)):
        for r in rs:
            s = r.get("stdin")
            if s == "not read":   # early runs close stdin unread
                continue
            ok = isinstance(s, dict) and "unparsed" not in s
            out.append({"session": sess, "phase": r["phase"], "parsed": ok, "keys": list(s) if ok else [],
                        "effort": s.get("effort") if ok else None})
    return out


def effort_ok(e):
    return isinstance(e, str) or (isinstance(e, dict) and isinstance(e.get("level"), str))


def cmd_analyze(td, pd, cols, pcols, widths, stdin_out):
    t_runs, p_runs = runs(td), runs(pd)
    st = an_stdin(t_runs, p_runs)
    json.dump(st, open(stdin_out, "w"))
    v = captures(td, "verbose")
    c = captures(td, "compact")
    r = {"sgr": an_sgr(v[-1][1]) if v else {"ok": False, "ok16": False, "ok256": False, "why": "no verbose capture"},
         "two": an_two(v[-1][1]) if v else {"rows": 0, "logo_col": -1, "refl_col": -1},
         "compact": an_compact(c[-1][1]) if c else {"scissors": False, "vs16": False}}
    try:
        r["tw"] = json.load(open(widths))
    except (OSError, ValueError):
        r["tw"] = {}
    wide = {}
    for x in "ABCD":
        caps = captures(td, f"wide{x}")
        wide[x] = an_wide(x, caps[-1][1], cols) if caps else {"class": "none", "why": f"no wide{x} capture"}
    ra = wide["A"].get("ruler")
    for x in "BCD":
        rx = wide[x].get("ruler")
        if ra is not None and rx is not None:
            wide[x]["diff"] = ra - rx
            q, rem = divmod(ra - rx, CELLS[x])
            wide[x]["c"] = q if rem == 0 else round((ra - rx) / CELLS[x], 2)
    r["wide"] = wide

    def first(rs, k):
        return rs[0][k] if rs else None

    tt, pt = first(t_runs, "tty"), first(p_runs, "tty")
    te, pe = first(t_runs, "env") or {}, first(p_runs, "env") or {}
    show = lambda t: "none" if t is None else str(t["cols"]) if "cols" in t else t["error"]
    r["ph4"] = {"tty_t": show(tt), "tty_p": show(pt), "col_t": te.get("COLUMNS", "unset"),
                "col_p": pe.get("COLUMNS", "unset"), "indent": wide["A"].get("indent"),
                "gap": wide["A"].get("gap"), "loss": wide["A"].get("loss")}
    parsed = [s for s in st if s["parsed"]]
    r["ph5"] = {"runs": len(parsed), "unparsed": len(st) - len(parsed),
                "sessions": [x for x in ("tmux", "pty") if any(s["session"] == x for s in parsed)],
                "first_keys": parsed[0]["keys"] if parsed else [], "first_effort": parsed[0]["effort"] if parsed else None,
                "efforts": sorted({json.dumps(s["effort"], sort_keys=True) for s in parsed})}
    # Round 2, finding 1: one run whose stdin cannot be parsed fails PH5.
    r["ph5"]["ok"] = bool(parsed) and len(parsed) == len(st) and all(effort_ok(s["effort"]) for s in parsed)
    r["ph11"] = an_ph11(td)
    sizes = [x["stdin_bytes"] for x in t_runs + p_runs if isinstance(x.get("stdin_bytes"), int)]
    r["ph11"]["stdin_max_bytes"] = max(sizes) if sizes else None
    r["ph12"] = an_ph12(td)
    r["runs"] = {"tmux": len(t_runs), "pty": len(p_runs)}
    # Round 2: every gate of the LIVE word, here so that the offline tests reach it (PH13: cmd_ph13).
    two, tw, ph4, cmp = r["two"], r["tw"], r["ph4"], r["compact"]
    r["pass"] = {
        "PH1": r["sgr"].get("ok") is True,
        "PH2": two["rows"] == 2,
        "PH3": wide["A"].get("class") == "cut",
        "PH4": ph4["tty_t"] == str(int(cols)) and ph4["tty_p"] == str(int(pcols)) and str(ph4["col_t"]).isdigit()
               and str(ph4["col_p"]).isdigit() and ph4["loss"] == 0,
        "PH5": r["ph5"]["ok"],
        "PH6": wide["B"].get("diff") == 10,
        "PH7": wide["C"].get("diff") == 10 and tw.get("C") == 1 and cmp["scissors"] is True and cmp["vs16"] is False,
        "PH9": wide["D"].get("diff") == 20 and tw.get("D") == 1 and two["rows"] == 2 and two["logo_col"] >= 0
               and two["logo_col"] == two["refl_col"],
        "PH11": r["ph11"]["ok"],
        "PH12": r["ph12"]["class"] in ("kills", "waits", "overlaps") and r["ph12"]["starts"] >= 2,
    }
    json.dump(r, sys.stdout, ensure_ascii=False)
    sys.stdout.write("\n")
    return 0


def cmd_fixtures(td, pd, an, ph13, out):
    """Writes the fixture sources (not yet scrubbed) into <out>: no whole stdin, no pid, no path."""
    os.makedirs(out, exist_ok=True)
    a = json.load(open(an))

    def rows(text, n, keep):
        b = block(text)[:n]
        return "".join((l if keep else plain(l)).rstrip() + "\n" for l in b)

    v, c = captures(td, "verbose"), captures(td, "compact")
    if v:
        open(os.path.join(out, "verbose.txt"), "w").write(rows(v[-1][1], 2, True))
    if c:
        open(os.path.join(out, "compact.txt"), "w").write(rows(c[-1][1], 1, True))
    w = []
    for x in "ABCD":
        caps = captures(td, f"wide{x}")
        if not caps:
            continue
        lines = [plain(l).rstrip() for l in caps[-1][1].split("\n")]
        at = [i for i, l in enumerate(lines) if l.lstrip(" ").startswith(f"P|wide{x}")]
        if at:
            w.append(f"# wide{x}\n" + "\n".join(lines[at[-1]:at[-1] + 3]) + "\n")
    if w:
        open(os.path.join(out, "wide.txt"), "w").write("".join(w))
    for ph, pat in (("early", r"PH11 early \d+"), ("slow", r"PH12 slow \d+")):
        seen, nums = [], {}
        for _, t in captures(td, ph):
            for m in re.finditer(pat, plain(t)):
                k = m.group(0)
                if k not in nums:
                    nums[k] = len(nums) + 1
                    seen.append(re.sub(r"\d+$", f"<run {nums[k]}>", k))
        if seen:
            open(os.path.join(out, f"{ph}.txt"), "w").write("\n".join(seen) + "\n")
    t0 = None
    evs = []
    pids = {}
    for e in events(td):
        t0 = e["t"] if t0 is None else t0
        pids.setdefault(e["pid"], len(pids) + 1)
        evs.append({"t": round(e["t"] - t0, 2), "run": pids[e["pid"]], "phase": e["phase"], "event": e["event"],
                    **({"sig": e["sig"]} if "sig" in e else {})})
    json.dump(evs, open(os.path.join(out, "events.json"), "w"))
    for sess, d in (("tmux", td), ("pty", pd)):
        rs = runs(d)
        if not rs:
            continue
        r0 = rs[0]
        json.dump({"phase": r0["phase"], "keys": list(r0["stdin"]) if isinstance(r0["stdin"], dict) else r0["stdin"],
                   "effort": r0["stdin"].get("effort") if isinstance(r0["stdin"], dict) else None,
                   "stdin_end": r0.get("stdin_end"), "runs": len(rs)},
                  open(os.path.join(out, f"stdin-{sess}.json"), "w"), indent=1, ensure_ascii=False)
        json.dump({"tty": r0["tty"], "env": r0["env"],
                   "env_names": [n for n in r0["env_names"] if n not in r0["env"]]},
                  open(os.path.join(out, f"env-{sess}.json"), "w"), indent=1)
    json.dump(a, open(os.path.join(out, "analysis.json"), "w"), indent=1, ensure_ascii=False)
    try:
        json.dump(json.load(open(ph13)), open(os.path.join(out, "ph13.json"), "w"), indent=1)
    except (OSError, ValueError):
        pass
    return 0


def cmd_settings(python, me, d, refresh, out):
    cmd = f"'{python}' -I '{me}' statusline '{d}'"
    json.dump({"statusLine": {"type": "command", "command": cmd, "refreshInterval": int(refresh)}}, open(out, "w"))
    return 0


if __name__ == "__main__":
    sys.exit(globals()["cmd_" + sys.argv[1]](*sys.argv[2:]))

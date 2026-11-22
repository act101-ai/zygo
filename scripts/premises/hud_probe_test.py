#!/usr/bin/env python3
"""Tests of scripts/premises/hud_probe.py (node PRB-1). No agent, no network, no tmux.
Usage: python3 -I scripts/premises/hud_probe_test.py   ->  "hud_probe tests: ok (<n>)", exit 0"""
import importlib.util
import json
import os
import re
import signal
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
PROBE = os.path.join(HERE, "hud_probe.py")
spec = importlib.util.spec_from_file_location("hud_probe", PROBE)
hp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hp)
E = "\x1b"
n = bad = 0


def check(name, cond, why=""):
    global n, bad
    n += 1
    if cond:
        print("ok:", name)
    else:
        bad += 1
        print(f"FAIL: {name} ({why})")


def probe(*args, **kw):
    return subprocess.run([sys.executable, "-I", PROBE, *args], capture_output=True, **kw)


def rundir():
    d = tempfile.mkdtemp(dir=T)
    os.makedirs(os.path.join(d, "log"))
    return d


def setphase(d, p):
    open(os.path.join(d, "phase"), "w").write(p + "\n")


def logs(d):
    return [json.load(open(os.path.join(d, "log", f))) for f in sorted(os.listdir(os.path.join(d, "log")))]


def evs(d):
    return [json.loads(l) for l in open(os.path.join(d, "events.jsonl"))]


def cap(d, phase, texts, t0=1000.0, step=0.5):
    os.makedirs(os.path.join(d, "cap"), exist_ok=True)
    with open(os.path.join(d, "captures.jsonl"), "a") as idx:
        for i, t in enumerate(texts):
            f = os.path.join(d, "cap", f"{phase}-{i:03d}.txt")
            open(f, "w").write(t)
            idx.write(json.dumps({"phase": phase, "file": f, "t": t0 + i * step}) + "\n")


def ev(d, rows):
    with open(os.path.join(d, "events.jsonl"), "a") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")


T = tempfile.mkdtemp()
try:
    # H1: the probe bytes equal the two printf commands of H§6.3.
    text = open(os.path.join(REPO, "docs/specs/2026-11-21-statusline-hud-design.md"), encoding="utf-8").read()
    sec = text[text.index("### 6.3 Mockups"):text.index("### 6.4 Colors")]
    pf = re.findall(r"^printf '(.*)'$", sec, re.M)
    conv = [p.replace("\\e", E).replace("\\n", "\n").replace("%%", "%") for p in pf]
    check("H1 VERBOSE and COMPACT equal the H§6.3 printf bytes", len(conv) == 2 and conv[0] == hp.VERBOSE and conv[1] == hp.COMPACT,
          f"{len(conv)} printf lines")

    # H2: verbose phase: output bytes, stdin logged, env kept and omitted, no terminal.
    d = rundir()
    setphase(d, "verbose")
    tok = "probe" + "-secret-" + "4f1c"
    env = {"PATH": os.environ["PATH"], "COLUMNS": "88", "PROBE_SECRET": tok, "TMUX": "x"}
    p = probe("statusline", d, input=json.dumps({"effort": "high", "session_id": "s1"}).encode(), env=env,
              start_new_session=True, timeout=10)
    r = logs(d)[0]
    blob = "".join(open(os.path.join(r0, f), errors="replace").read() for r0, _, fs in os.walk(d) for f in fs)
    check("H2 verbose: bytes, stdin, env allowlist, no terminal",
          p.returncode == 0 and p.stdout.decode() == hp.VERBOSE and r["stdin"] == {"effort": "high", "session_id": "s1"}
          and r["env"].get("COLUMNS") == "88" and "PROBE_SECRET" not in r["env"] and "PROBE_SECRET" in r["env_names"]
          and r["env"].get("TMUX") == "set" and "error" in r["tty"] and tok not in blob and r["stdin_end"] == "eof",
          f"rc {p.returncode} rec {r}")

    # H3: early phase: stdin closed unread (the writer's pipe stays open), at once.
    d = rundir()
    setphase(d, "early")
    t0 = time.monotonic()
    pr = subprocess.Popen([sys.executable, "-I", PROBE, "statusline", d], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          start_new_session=True)
    out, _ = pr.communicate(timeout=10)
    pr.stdin = None
    dt = time.monotonic() - t0
    r = logs(d)[0]
    check("H3 early: stdin not read, one line, under 2 s",
          pr.returncode == 0 and out.decode() == f"PH11 early {pr.pid}\n" and r["stdin"] == "not read" and dt < 2,
          f"rc {pr.returncode} out {out!r} dt {dt:.2f}")

    # H4: slow phase, SIGTERM after 1 s: exit 143, events start, beat and signal TERM, no end.
    d = rundir()
    setphase(d, "slow")
    pr = subprocess.Popen([sys.executable, "-I", PROBE, "statusline", d], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          start_new_session=True)
    pr.stdin.write(b"{}")
    pr.stdin.close()
    time.sleep(1.0)
    pr.send_signal(signal.SIGTERM)
    pr.wait(timeout=10)
    kinds = [e["event"] for e in evs(d)]
    sig = [e.get("sig") for e in evs(d) if e["event"] == "signal"]
    check("H4 slow + SIGTERM: exit 143, start, beat, signal TERM, no end",
          pr.returncode == 143 and kinds[0] == "start" and "beat" in kinds and sig == ["TERM"] and "end" not in kinds,
          f"rc {pr.returncode} events {kinds}")

    # H5: wide phases: two rows, the measured row is <X>| + glyphs + the 300-cell ruler.
    d = rundir()
    ok = len(hp.RULER) == 300 and hp.RULER.endswith("300") and hp.RULER[:20] == "........10........20"
    for x in "ABCD":
        setphase(d, "wide" + x)
        o = probe("statusline", d, input=b"{}", timeout=10).stdout.decode().split("\n")
        g, k = hp.WIDE[x]
        ok = ok and len(o) == 3 and o[0] == f"P|wide{x} probe" and o[1] == f"{x}|{g * k}{hp.RULER}" and o[2] == ""
    check("H5 wide rows and ruler", ok)

    # H6: analysis of a cut capture with 1-cell glyphs: cut, c = 1, loss = indent + gap.
    d = rundir()
    W = 120
    area = W - 4

    def row(x, c=1):
        g, k = hp.WIDE[x]
        glyph_cells = hp.CELLS[x] * c
        r = area - 2 - glyph_cells - 1
        return f"  {x}|{g * k}{hp.RULER[:r]}…"

    for x in "ABCD":
        cap(d, "wide" + x, [f"x\n  P|wide{x} probe\n{row(x)}\n  ? for shortcuts\n"])
    a = {x: hp.an_wide(x, open(os.path.join(d, "cap", f"wide{x}-000.txt")).read(), W) for x in "ABCD"}
    check("H6 cut, ruler cells, loss 4", a["A"]["class"] == "cut" and a["A"]["ruler"] == area - 3 and a["A"]["loss"] == 4
          and a["A"]["indent"] == 2 and a["A"]["gap"] == 2 and all(a[x]["class"] == "cut" for x in "BCD")
          and a["A"]["ruler"] - a["B"]["ruler"] == 10 and a["A"]["ruler"] - a["D"]["ruler"] == 20, str(a))

    # H7: a glyph counted 2 cells gives diff 20 (B); a wrapped row is "wrapped".
    b2 = hp.an_wide("B", f"  P|wideB probe\n{row('B', 2)}\n", W)
    full = f"A|{hp.RULER}"
    wr = hp.an_wide("A", "  P|wideA probe\n  " + full[:W - 4] + "\n  " + full[W - 4:W - 4 + 50] + "\n", W)
    check("H7 2-cell glyph and wrap", a["A"]["ruler"] - b2["ruler"] == 20 and wr["class"] == "wrapped", f"{b2} {wr}")

    # H8: SGR of the verbose bytes as tmux shows them; 16-color only fails the 256-color part.
    pane = "\n".join("  " + l for l in hp.VERBOSE.split("\n"))
    s = hp.an_sgr(pane)
    s2 = hp.an_sgr(pane.replace("38;5;172", "33"))
    t2 = hp.an_two(pane)
    check("H8 sgr and two rows", s["ok"] and not s2["ok256"] and s2["ok16"] and t2 == {"rows": 2, "logo_col": 6, "refl_col": 6},
          f"{s} {s2} {t2}")

    # H9: compact: scissors present, no VS16; with VS16 found.
    check("H9 compact scissors and VS16", hp.an_compact(hp.COMPACT) == {"scissors": True, "vs16": False}
          and hp.an_compact(hp.COMPACT.replace(hp.SCISSORS, hp.SCISSORS + hp.VS16))["vs16"] is True)

    # H10: PH11: 5 early runs, each shown; later the done phase shows its row.
    d = rundir()
    rows = []
    for i in range(5):
        rows += [{"t": 1000 + 2 * i, "pid": 100 + i, "phase": "early", "event": "start"},
                 {"t": 1000.1 + 2 * i, "pid": 100 + i, "phase": "early", "event": "end"}]
    ev(d, rows)
    cap(d, "early", [f"  PH11 early {100 + i // 4}\n" for i in range(20)], t0=1000.3)
    cap(d, "done", ["  PROBE DONE\n"], t0=1040)
    r = hp.an_ph11(d)
    lost_d = rundir()
    ev(lost_d, rows)
    cap(lost_d, "early", ["  PH11 early 100\n"] * 20, t0=1000.3)
    r2 = hp.an_ph11(lost_d)
    check("H10 PH11 shown, lost, after", r["shown"] == 5 and r["lost"] == 0 and r["after"] and not r["error"]
          and r2["lost"] >= 3 and not r2["after"], f"{r} {r2}")

    # H11: PH12 classes: kills (signal at 2.1 s), waits (end, then start), overlaps (start before end).
    def ph12(rows):
        dd = rundir()
        ev(dd, [dict(e, phase="slow") for e in rows])
        return hp.an_ph12(dd)

    k = ph12([{"t": 0, "pid": 1, "event": "start"}, {"t": 2.1, "pid": 1, "event": "signal", "sig": "TERM"},
              {"t": 2.15, "pid": 2, "event": "start"}, {"t": 4.2, "pid": 2, "event": "signal", "sig": "TERM"}])
    w = ph12([{"t": 0, "pid": 1, "event": "start"}, {"t": 5.1, "pid": 1, "event": "end"},
              {"t": 6, "pid": 2, "event": "start"}, {"t": 11, "pid": 2, "event": "end"}])
    o = ph12([{"t": 0, "pid": 1, "event": "start"}, {"t": 2, "pid": 2, "event": "start"},
              {"t": 5.1, "pid": 1, "event": "end"}, {"t": 7.1, "pid": 2, "event": "end"}])
    check("H11 PH12 kills, waits, overlaps", k["class"] == "kills" and k["signals"] == ["TERM"] and k["max_concurrent"] == 1
          and w["class"] == "waits" and o["class"] == "overlaps" and o["max_concurrent"] == 2, f"{k} {w} {o}")

    # H12: PH13 with stub opencode programs: a strict stub (verified case) and a stub that keeps statusLine.
    def stub(path, keep):
        open(path, "w").write(f"""#!{sys.executable}
import json, os, sys
a = sys.argv[1:]
if a == ["--version"]: print("1.19.6")
elif a and a[-1] == "--help": print("Commands:\\n  opencode run   run opencode")
elif a[:2] == ["debug", "config"]:
    c = json.loads(os.environ.get("OPENCODE_CONFIG_CONTENT", "{{}}"))
    known = {keep!r}
    out = {{"agent": {{}}, "username": "unknown"}}
    out.update({{k: v for k, v in c.items() if k in known}})
    print(json.dumps(out))
""")
        os.chmod(path, 0o755)

    stub(os.path.join(T, "oc1"), ["username", "model"])
    stub(os.path.join(T, "oc2"), ["username", "statusLine"])
    probe("ph13", os.path.join(T, "oc1"), os.path.join(T, "w1"), os.path.join(T, "p1.json"), timeout=60)
    probe("ph13", os.path.join(T, "oc2"), os.path.join(T, "w2"), os.path.join(T, "p2.json"), timeout=60)
    p1, p2 = json.load(open(os.path.join(T, "p1.json"))), json.load(open(os.path.join(T, "p2.json")))
    check("H12 PH13 dropped keys, control, and a kept key",
          p1["version"] == "1.19.6" and p1["help_hits"] == [] and p1["control"] == {"username": "kept", "zygoProbeUnknownKey": "dropped"}
          and set(p1["keys"].values()) == {"dropped"} and p2["keys"]["statusLine"] == "kept", f"{p1} {p2}")

    # H13: analyze and fixtures end to end on a synthetic run: no pid, no stdin value but effort and keys.
    td, pd = rundir(), rundir()
    rec = lambda d, ph, cols, tty: open(os.path.join(d, "log", f"run-1-1-{ph}.json"), "w").write(json.dumps(
        {"t": 1, "pid": 4242, "phase": ph, "tty": tty, "env": {"COLUMNS": cols}, "env_names": ["COLUMNS", "HOME"],
         "stdin": {"effort": {"level": "medium"}, "session_id": "s-77", "cwd": "/x"}, "stdin_end": "eof"}))
    rec(td, "verbose", "120", {"error": "ENXIO"})
    rec(pd, "verbose", "100", {"error": "ENXIO"})
    cap(td, "verbose", [pane])
    cap(td, "compact", ["  " + hp.COMPACT])
    for x in "ABCD":
        cap(td, "wide" + x, [f"  P|wide{x} probe\n{row(x)}\n"], t0=2000)
    ev(td, [{"t": 3000, "pid": 4242, "phase": "slow", "event": "start"}])
    json.dump({"B": 1, "C": 1, "D": 1, "ellipsis": 1}, open(os.path.join(T, "tw.json"), "w"))
    out = probe("analyze", td, pd, str(W), "100", os.path.join(T, "tw.json"), os.path.join(T, "stdin.json"), timeout=30)
    an = json.loads(out.stdout)
    json.dump(an, open(os.path.join(T, "an.json"), "w"))
    probe("fixtures", td, pd, os.path.join(T, "an.json"), os.path.join(T, "p1.json"), os.path.join(T, "fx"), timeout=30)
    fx = {f: open(os.path.join(T, "fx", f)).read() for f in os.listdir(os.path.join(T, "fx"))}
    st = json.load(open(os.path.join(T, "stdin.json")))
    check("H13 analyze and fixtures",
          an["ph4"] == {"tty_t": "ENXIO", "tty_p": "ENXIO", "col_t": "120", "col_p": "100", "indent": 2, "gap": 2, "loss": 4}
          and an["wide"]["B"]["diff"] == 10 and an["wide"]["D"]["c"] == 1 and an["wide"]["A"]["shown"] == W - 4
          and an["sgr"]["ok"] and len(st) == 2
          and all(s["effort"] == {"level": "medium"} for s in st)
          and {"verbose.txt", "compact.txt", "wide.txt", "events.json", "stdin-tmux.json", "env-tmux.json",
               "stdin-pty.json", "env-pty.json", "analysis.json", "ph13.json"} <= set(fx)
          and not any("4242" in v or "s-77" in v for v in fx.values()), f"{an} {sorted(fx)}")

    # H14: settings JSON: the command, refreshInterval, no padding key.
    probe("settings", "/usr/bin/python3", PROBE, "/tmp/run/t", "2", os.path.join(T, "s.json"), timeout=10)
    sj = json.load(open(os.path.join(T, "s.json")))
    check("H14 settings", sj == {"statusLine": {"type": "command", "refreshInterval": 2,
                                                "command": f"'/usr/bin/python3' -I '{PROBE}' statusline '/tmp/run/t'"}}, str(sj))
finally:
    subprocess.run(["rm", "-rf", T])

if bad:
    print(f"hud_probe tests: {bad} failed")
    sys.exit(1)
print(f"hud_probe tests: ok ({n})")

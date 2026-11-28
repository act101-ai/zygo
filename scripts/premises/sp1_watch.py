#!/usr/bin/env python3
"""Watcher and judge of live-sp1.sh (node MS-23; mail sync design S§20 SP1). Python 3 stdlib only, Linux /proc.

  sp1_watch.py watch --out <jsonl> --stop <file> [--every <s>] [--ws <id>:<state dir>]... [--busy <label>:<activity.json>]...
    Polls /proc every --every seconds (default 0.002) until <file> exists, and appends one JSON line per change:
    - {"k": "start", "t", "boot"}: the watcher start; boot = time.time() - /proc/uptime (start times of processes
      are boot + starttime / CLK_TCK);
    - {"k": "hook", "t", "pid", "ppid", "pcomm", "event", "agent", "ws", "start"} for a new process whose argv is
      `zygo hook workspace ... --event <e>`; {"k": "hook_gone", "t", "pid"} when it has ended (gone from /proc, or a
      zombie: exited and not yet reaped; its children are re-parented at the exit);
    - {"k": "child", "t", "pid", "ppid", "pcomm", "sid", "pgid", "ws", "start", "env"} for a new process whose argv
      is `<zygo> mail sync --ws-id <id> --background`; env holds only the names of ENV_NAMES found in its environ,
      never a value; {"k": "child_ppid", "t", "pid", "ppid", "pcomm"} when its parent changes (the parent ended);
      {"k": "child_gone", "t", "pid"} when it is gone;
    - {"k": "busy", "t", "label", "value"}: the `busy` value of an activity.json (true, false or null) changed;
    - {"k": "last_start", "t", "ws", "value"} and {"k": "state", "t", "ws", "mtime"}: mail/sync/last_start text and
      the mtime of mail/sync/state.json of a workspace changed.
  sp1_watch.py judge --watch <jsonl> --rounds <jsonl> [--trace <git trace file>]
    Prints one line per verdict, tab-separated: <sub id> TAB <0 = ok | 1 = FAIL | note> TAB <text>. Times are
    offsets from the round's prompt second T; no path, URL, user or host name is printed.

Rounds file, one JSON object per round: {"id": "R1", "sub": "1.1", "acc": true, "ws": <id>, "member": "cc",
"member_label": <the --busy label of the member>, "remote": "SSH-agent", "T": <int>, "end": <float>, "state":
<state.json object or null>, "errors": [<check names>], "signs": <int or null>, "branch": "zygo/mail", "attempt": 1}.
acc true: an acceptance round (ok or FAIL); false: a note. Rounds of one remote with acc true: the last one counts
for 1.3.
"""
import argparse
import datetime
import json
import os
import re
import sys
import time

ENV_NAMES = ("SSH_AUTH_SOCK", "HOME", "PATH", "DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR", "GIT_TRACE",
             "GIT_SSH_COMMAND", "GIT_ASKPASS", "SSH_ASKPASS")
TICK = os.sysconf("SC_CLK_TCK") if hasattr(os, "sysconf") else 100
YOUNG = 0.5     # s: a process younger than this is read at each poll (it can still exec)
REFRESH = 2.0   # s: every pid is read again at this period


# --- pure parts (unit-tested in sp1_watch_test.py) -------------------------------------------------------------

def classify(argv):
    """("hook", event, agent) for `zygo hook workspace ... --event <e>`, ("child", ws_id) for
    `<zygo> mail sync --ws-id <id> --background`, else None. argv[0] must name a zygo program."""
    if len(argv) < 2 or os.path.basename(argv[0]) != "zygo":
        return None
    a = argv[1:]
    if len(a) >= 2 and a[0] == "hook" and a[1] == "workspace" and "--event" in a:
        i = a.index("--event")
        if i + 1 < len(a):
            agent = a[a.index("--agent") + 1] if "--agent" in a and a.index("--agent") + 1 < len(a) else "claude-code"
            return ("hook", a[i + 1], agent)
    if len(a) >= 4 and a[0] == "mail" and a[1] == "sync" and a[2] == "--ws-id" and "--background" in a[4:]:
        return ("child", a[3])
    return None


def parse_stat(text):
    """(comm, ppid, pgid, sid, starttime ticks, state) of a /proc/<pid>/stat text (comm may hold spaces and parens)."""
    lp, rp = text.index("("), text.rindex(")")
    comm = text[lp + 1:rp]
    f = text[rp + 2:].split()
    # state field 3, ppid 4, pgrp 5, session 6, starttime 22
    return comm, int(f[1]), int(f[2]), int(f[3]), int(f[19]), f[0]


def env_names(raw):
    """The names of ENV_NAMES present in a NUL-separated environ block (bytes); values are never kept."""
    out = []
    for item in raw.split(b"\0"):
        n = item.split(b"=", 1)[0].decode("ascii", "replace")
        if n in ENV_NAMES and n not in out:
            out.append(n)
    return sorted(out)


def trace_epoch(hms, ref):
    """Unix time of a git trace time of day "HH:MM:SS.ffffff" (local time), the candidate day nearest to ref."""
    h, m, s = hms.split(":")
    sec = int(h) * 3600 + int(m) * 60 + float(s)
    day = datetime.datetime.fromtimestamp(ref).replace(hour=0, minute=0, second=0, microsecond=0)
    best = None
    for d in (-1, 0, 1):
        t = (day + datetime.timedelta(days=d)).timestamp() + sec
        if best is None or abs(t - ref) < abs(best - ref):
            best = t
    return best


TRACE_TIME = re.compile(r"^(\d\d:\d\d:\d\d\.\d+) ")
SSH_LINE = re.compile(r"run_command:.*ssh -o BatchMode=yes.*-p '?443'? .*ssh\.github\.com|"
                      r"run_command:.*ssh -o BatchMode=yes.*ssh\.github\.com.*-p '?443'?")
HELPER_LINE = re.compile(r"run_command:.*gh auth git-credential['\"]? get")


def trace_hits(lines, lo, hi, rx):
    """Times (Unix) of the trace lines matching rx whose time is in [lo, hi]."""
    out = []
    for ln in lines:
        m = TRACE_TIME.match(ln)
        if not m or not rx.search(ln):
            continue
        t = trace_epoch(m.group(1), lo)
        if lo <= t <= hi:
            out.append(t)
    return out


def fmt(x):
    return "%+.2f s" % x


def yn(b):
    return "yes" if b else "no"


def judge_round(r, ev):
    """The verdict of one round: (ok, text, child) from the watcher events ev (list of dicts, time order)."""
    T, ws = r["T"], r["ws"]
    end = r.get("end") or (T + 600)
    hooks = {e["pid"]: dict(e) for e in ev if e["k"] == "hook"}
    for e in ev:
        if e["k"] == "hook_gone" and e["pid"] in hooks:
            hooks[e["pid"]]["gone"] = e["t"]
    kids = [dict(e) for e in ev if e["k"] == "child" and e["ws"] == ws and e["start"] >= T - 0.05 and e["t"] <= end]
    if not kids:
        return False, "no sync child of this workspace started after the prompt second T", None
    c = kids[0]
    c["gone"] = next((e["t"] for e in ev if e["k"] == "child_gone" and e["pid"] == c["pid"]), None)
    c["reparent"] = next((e for e in ev if e["k"] == "child_ppid" and e["pid"] == c["pid"]), None)
    parent = hooks.get(c["ppid"])
    busy = [e for e in ev if e["k"] == "busy" and e["label"] == r["member_label"] and T - 0.05 <= e["t"] <= end]
    bt = next((e["t"] for e in busy if e["value"] is True), None)
    bf = next((e["t"] for e in busy if e["value"] is False and bt is not None and e["t"] >= bt), None)
    others = [h for h in hooks.values() if h.get("ws") == ws and T - 0.05 <= h["start"] <= c["start"] and h["event"] != "prompt"]
    if parent is not None:
        event = parent["event"]
        hook_end = parent.get("gone")
        outlived = hook_end is not None and c["gone"] is not None and hook_end < c["gone"]
        pnote = "parent hook pid seen: event %s, started %s, gone %s" % (
            event, fmt(parent["start"] - T), fmt(hook_end - T) if hook_end is not None else "never")
    else:
        # The hook starts the child before it writes activity.json (crates/zygo-hooks/src/workspace.rs: start_sync,
        # then the event); the prompt hook writes busy true, the stop hook busy false. A child that started at or
        # before the busy-true sample, after T, came from the prompt hook.
        if bt is not None and c["start"] <= bt:
            event = "prompt (by time: the child started at %s, not later than busy true at %s, which the prompt hook writes after it starts the child)" % (
                fmt(c["start"] - T), fmt(bt - T))
        else:
            event = "unknown (no hook sampled; child start %s, busy true %s)" % (fmt(c["start"] - T), fmt(bt - T) if bt is not None else "not seen")
        if c["reparent"] is not None:
            outlived = c["gone"] is not None and c["reparent"]["t"] < c["gone"]
            pnote = "parent not sampled alive; the child's parent changed to %s at %s (its parent ended)" % (
                c["reparent"]["pcomm"], fmt(c["reparent"]["t"] - T))
        else:
            # First seen already re-parented: its parent had ended before the first sample.
            outlived = c["pcomm"] != "zygo" and c["gone"] is not None
            pnote = "first seen with parent %s (not a zygo hook: its parent had already ended)" % c["pcomm"]
        near = [h for h in hooks.values() if h.get("ws") == ws and h["event"] == "prompt" and T - 0.05 <= h["start"] <= c["start"]]
        if near:
            h = near[-1]
            pnote += "; sampled prompt hook of this workspace: started %s, ended %s" % (
                fmt(h["start"] - T), fmt(h["gone"] - T) if h.get("gone") is not None else "not seen")
    st = r.get("state") or {}
    last_ok, last_error = st.get("last_ok"), st.get("last_error")
    verdict = (st.get("public") or {}).get("verdict", "none")
    order = bf is not None and c["gone"] is not None and bf < c["gone"]
    sid_ok = c["sid"] == c["pid"]
    ok_state = isinstance(last_ok, int) and last_ok > T and last_error is None
    ok_event = event.startswith("prompt") and not others
    ok = sid_ok and outlived and order and ok_state and ok_event
    text = ("%s %s/%s (branch %s, attempt %d): prompt at T; child seen at %s, started %s, sid = pid: %s; %s; "
            "started by: %s%s; turn end (busy false) at %s; child gone at %s; turn ended before the child: %s; "
            "parent ended before the child: %s; state.json last_ok = %s, last_error %s; public verdict %s") % (
        r["id"], r["member"], r["remote"], r.get("branch", "?"), r.get("attempt", 1), fmt(c["t"] - T), fmt(c["start"] - T),
        yn(sid_ok), pnote, event, "; other hook events before the child: " + ", ".join(sorted({h["event"] for h in others})) if others else "",
        fmt(bf - T) if bf is not None else "not seen", fmt(c["gone"] - T) if c["gone"] is not None else "not seen",
        yn(order), yn(outlived), ("T%+d" % (last_ok - T)) if isinstance(last_ok, int) else "absent",
        "absent" if last_error is None else "= " + str(last_error), verdict)
    if r.get("errors"):
        text += "; errors.log checks: " + ", ".join(r["errors"])
    return ok, text, c


def order_shown(r, events):
    """"yes" when the round's child was seen and its turn ended before it, else "no"."""
    _, text, c = judge_round(r, events)
    return "yes" if c is not None and "turn ended before the child: yes" in text else "no"


def judge(events, rounds, trace_lines):
    out = []
    git = {}
    envs = {}
    last = {}
    for r in rounds:
        if r.get("acc"):
            last[r["sub"]] = r["id"], r.get("attempt", 1)
    for r in rounds:
        ok, text, c = judge_round(r, events)
        if r.get("acc") and last[r["sub"]] != (r["id"], r.get("attempt", 1)):
            out.append(("3.6", "note", "attempt repeated on a new branch (S§20 SP1 fallback): " + text))
        else:
            out.append((r["sub"], ("0" if ok else "1") if r.get("acc") else "note", text))
        if c is not None:
            envs["%s#%d" % (r["id"], r.get("attempt", 1))] = c.get("env", [])
            hi = c["gone"] if c["gone"] is not None else r.get("end") or c["start"] + 300
            git[(r["id"], r.get("attempt", 1))] = (len(trace_hits(trace_lines, c["start"] - 0.05, hi + 0.05, SSH_LINE)),
                            len(trace_hits(trace_lines, c["start"] - 0.05, hi + 0.05, HELPER_LINE)), r.get("signs"), r["remote"])
    # 1.3: the git path of the acceptance rounds (the last attempt of each)
    acc = {}
    for r in rounds:
        if r.get("acc"):
            acc[r["remote"]] = r["id"], r.get("attempt", 1)
    parts, ok = [], True
    for remote, (rid, att) in sorted(acc.items()):
        s, h, n, _ = git.get((rid, att), (0, 0, None, remote))
        if remote.startswith("SSH"):
            good = s >= 1 and (n or 0) >= 1 and h == 0
            parts.append("%s %s: trace lines `ssh -o BatchMode=yes` to ssh.github.com port 443 in the child window: %d; "
                         "agent sign requests in the round: %s; gh credential helper calls: %d" % (rid, remote, s, n, h))
        else:
            good = h >= 1 and s == 0
            parts.append("%s %s: trace lines `gh auth git-credential get` in the child window: %d; ssh calls: %d" % (rid, remote, h, s))
        ok = ok and good
    if acc:
        out.append(("1.3", "0" if ok else "1", "git path: " + "; ".join(parts)))
    for r in rounds:
        key = (r["id"], r.get("attempt", 1))
        if not r.get("acc") and key in git:
            s, h, n, remote = git[key]
            out.append(("3.5", "note", "%s %s git path: ssh lines %d, helper lines %d, agent sign requests %s" % (r["id"], remote, s, h, n)))
    if envs:
        out.append(("3.3", "note", "child environment names present (values not read): " +
                    "; ".join("%s: %s" % (k, " ".join(v) or "none") for k, v in envs.items())))
    return out


# --- the watcher ------------------------------------------------------------------------------------------------

def read(path, mode="r"):
    try:
        with open(path, mode) as f:
            return f.read()
    except OSError:
        return None


def watch(a):
    up = float(read("/proc/uptime").split()[0])
    boot = time.time() - up
    wss = [x.split(":", 1) for x in a.ws]
    busy = [x.split(":", 1) for x in a.busy]
    out = open(a.out, "a", buffering=1)

    def put(**o):
        o["t"] = round(time.time(), 3)
        out.write(json.dumps(o, sort_keys=True) + "\n")

    put(k="start", boot=round(boot, 3))
    seen = {}      # pid -> (kind, ppid): hooks and children alive
    old = set()    # pids of other processes older than YOUNG s: not read again until the next refresh
    refresh = time.time() + REFRESH
    vals = {}
    while not os.path.exists(a.stop):
        alive = set()
        now = time.time()
        if now >= refresh:   # a pid can be used again by a new process: read every pid again from time to time
            old.clear()
            refresh = now + REFRESH
        for d in os.listdir("/proc"):
            if not d.isdigit():
                continue
            pid = int(d)
            if pid in old:
                continue
            if pid in seen:
                kind, ppid = seen[pid]
                st = read("/proc/%d/stat" % pid)
                # A process that exited and is not yet reaped stays in /proc as a zombie (state Z): it has ended,
                # and its children were re-parented at its exit.
                if not st or parse_stat(st)[5] == "Z":
                    continue
                alive.add(pid)
                if kind == "child":
                    if st:
                        np = parse_stat(st)[1]
                        if np != ppid:
                            pst = read("/proc/%d/stat" % np) if np > 0 else None
                            seen[pid] = (kind, np)
                            put(k="child_ppid", pid=pid, ppid=np, pcomm=parse_stat(pst)[0] if pst else "?")
                continue
            st = read("/proc/%d/stat" % pid)
            raw = read("/proc/%d/cmdline" % pid, "rb")
            if not st or not raw:
                continue
            comm, ppid, pgid, sid, start, state = parse_stat(st)
            if state == "Z":
                continue
            t0 = round(boot + start / TICK, 3)
            argv = [x.decode("utf-8", "replace") for x in raw.rstrip(b"\0").split(b"\0")]
            c = classify(argv)
            if c is None:
                # A young process can still exec a zygo program: read it again at the next poll.
                if now - t0 > YOUNG:
                    old.add(pid)
                continue
            pst = read("/proc/%d/stat" % ppid)
            pcomm = parse_stat(pst)[0] if pst else "?"
            envb = read("/proc/%d/environ" % pid, "rb") or b""
            ws = ""
            for item in envb.split(b"\0"):
                if item.startswith(b"ZYGO_WORKSPACE="):
                    ws = item.split(b"=", 1)[1].decode("ascii", "replace")
            alive.add(pid)
            seen[pid] = (c[0], ppid)
            if c[0] == "hook":
                put(k="hook", pid=pid, ppid=ppid, pcomm=pcomm, event=c[1], agent=c[2], ws=ws, start=t0)
            else:
                put(k="child", pid=pid, ppid=ppid, pcomm=pcomm, sid=sid, pgid=pgid, ws=c[1], start=t0, env=env_names(envb))
        for pid in [p for p in seen if p not in alive]:
            put(k=seen[pid][0] + "_gone", pid=pid)
            del seen[pid]
            old.add(pid)   # a zombie is not read again until the next refresh
        for label, path in busy:
            txt = read(path)
            v = None
            if txt:
                try:
                    v = json.loads(txt).get("busy")
                except ValueError:
                    v = None
            if vals.get(("b", label), "unset") != v:
                vals[("b", label)] = v
                put(k="busy", label=label, value=v)
        for ws, d in wss:
            ls = read(os.path.join(d, "mail", "sync", "last_start"))
            ls = ls.strip() if ls else None
            if vals.get(("l", ws), "unset") != ls:
                vals[("l", ws)] = ls
                put(k="last_start", ws=ws, value=ls)
            try:
                m = os.stat(os.path.join(d, "mail", "sync", "state.json")).st_mtime
            except OSError:
                m = None
            if vals.get(("s", ws), "unset") != m:
                vals[("s", ws)] = m
                put(k="state", ws=ws, mtime=m)
        time.sleep(a.every)
    put(k="stop")


def main():
    p = argparse.ArgumentParser()
    sp = p.add_subparsers(dest="cmd", required=True)
    w = sp.add_parser("watch")
    w.add_argument("--out", required=True)
    w.add_argument("--stop", required=True)
    w.add_argument("--every", type=float, default=0.002)
    w.add_argument("--ws", action="append", default=[])
    w.add_argument("--busy", action="append", default=[])
    j = sp.add_parser("judge")
    j.add_argument("--watch", required=True)
    j.add_argument("--rounds", required=True)
    j.add_argument("--trace", default="")
    j.add_argument("--order", default="", help="print only yes or no: the turn of the last round with this id ended before its child")
    a = p.parse_args()
    if a.cmd == "watch":
        watch(a)
        return
    events = [json.loads(x) for x in open(a.watch) if x.strip()]
    rounds = [json.loads(x) for x in open(a.rounds) if x.strip()]
    trace = (read(a.trace) or "").splitlines() if a.trace else []
    if a.order:
        r = [x for x in rounds if x["id"] == a.order][-1]
        sys.stdout.write(order_shown(r, events) + "\n")
        return
    for sub, word, text in judge(events, rounds, trace):
        sys.stdout.write("%s\t%s\t%s\n" % (sub, word, text.replace("\t", " ").replace("\n", " ")))


if __name__ == "__main__":
    main()

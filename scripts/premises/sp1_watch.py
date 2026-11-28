#!/usr/bin/env python3
"""Watcher, waits and judge of live-sp1.sh (node MS-23; mail sync design S§20 SP1). Python 3 stdlib only, Linux /proc.

  sp1_watch.py watch --out <jsonl> --stop <file> [--every <s>] [--root <pid>] [--agent-sock <path>] [--cfg <json>]
    Until <file> exists, it repeats: one full scan (stat and cmdline of every /proc/<pid>, read again at each scan,
    no cache: a process is classified again after an exec; the run tree; side processes; TCP connections), then
    FAST_SPAN seconds of fast polls (the children of every agent process of the run tree and of every live hook,
    from /proc/<pid>/task/*/children; the state of each seen hook and child; the activity and sync files).
    <json> (read again when its mtime changes, may be absent at the start): {"ws": [[<id>, <state dir>], ...],
    "busy": [[<label>, <activity.json>], ...]}. Each JSON line has "t" (when it was written) and "prev" (the start
    of the poll before the one that saw it): the change it records happened in (prev, t].
    - {"k": "start", "boot", "subreaper", "fast_span"}: boot = time.time() - CLOCK_BOOTTIME (process start times
      are boot + starttime / CLK_TCK, CLK_TCK resolution); subreaper = the pid of the user's `systemd --user`;
    - {"k": "hook", "pid", "ppid", "pcomm", "event", "agent", "ws", "start"} for a new process whose argv is
      `zygo hook workspace ... --event <e>` (ws from its ZYGO_WORKSPACE); {"k": "hook_gone", "pid"} when it has
      ended (gone from /proc, or a zombie: exited and not yet reaped; its children are re-parented at the exit);
      {"k": "hook_exec", "pid"} when a "hook" was a hook's fork that then exec'd a sync child (its child line
      follows; the judge drops that hook line);
    - {"k": "child", "pid", "ppid", "pcomm", "sid", "pgid", "ws", "start", "env", "sock"} for a new process whose
      argv is `<zygo> mail sync --ws-id <id> --background`; env holds only the names of ENV_NAMES found in its
      environ, never a value; sock is "match" when its SSH_AUTH_SOCK equals --agent-sock, "other" when it is set to
      something else, "absent" when it is not set; {"k": "child_ppid", "pid", "ppid", "pcomm"} when its parent
      changes; {"k": "child_gone", "pid"} when it has ended;
    - {"k": "proc", "pid", "ppid", "sid", "name", "sub", "chain", "in_child"} once for each process of the run whose
      program is one of SIDE_NAMES (git, ssh, gh, curl, ...): a descendant of --root, or of a sync child's session.
      name is the program's base name, sub a git subcommand word (letters and dashes only, else ""), chain the
      parent program names up to --root, in_child true when its session id is a sync child's pid; no other argv;
    - {"k": "conn", "remote", "pid", "name", "chain", "in_child"} once for each new TCP connection (ESTABLISHED or
      SYN_SENT, remote not loopback) whose socket a process of the run holds (full scans only);
    - {"k": "busy", "label", "value"}: the `busy` value of an activity.json (true, false or null) changed;
    - {"k": "last_start", "ws", "value"} and {"k": "state", "ws", "mtime"}: mail/sync/last_start text and the mtime
      of mail/sync/state.json of a workspace changed;
    - {"k": "stop", "samples", "full", "polls", "poll", "agents", "max_start_skew"}: the full scans and fast polls
      with their gaps (max, p99, mean), and the largest start time later than the time its process was seen.
  sp1_watch.py wait-turn --watch <jsonl> --label <label> --after <T> --timeout <s>
    Exit 0 when a busy true event of <label> later than <T> and then a busy false event are in the file.
  sp1_watch.py wait-child --watch <jsonl> --ws <id> --after <T> --timeout <s> [--gone]
    Exit 0 when a child of <ws> that started at or after <T> is in the file (with --gone: and its child_gone).
  sp1_watch.py judge --watch <jsonl> --rounds <jsonl> [--trace <git trace2 event file>] [--order <round id>]
    Prints one line per verdict, tab-separated: <sub id> TAB <0 = ok | 1 = FAIL | note> TAB <text>. Times are
    offsets from the round's prompt second T, with 3 decimals; no path, URL, user or host name is printed.
  sp1_watch.py side --watch <jsonl> --trace <file> --files <json> --lo <unix s> --hi <unix s>
    One line: <0 | 1> TAB <text>: the side-traffic check (side_traffic) for the rounds' window [lo, hi].
  sp1_watch.py kh-add --file <known_hosts> --line <file with the run's line> --save <copy>   (node FX-38)
    One word or reason (kh_add): "added" (exit 0) only after the bytes of the file that <known_hosts> names (through
    a link) were saved to <copy> and the line was appended, flushed and fsync'd; "present" (exit 0, not changed);
    "failed: <reason>" (exit 1; an errno name, no path).
  sp1_watch.py kh-remove --file <known_hosts> --line <file with the run's line> --start <copy>   (node FX-38)
    One line: <result> TAB <lines missing> TAB <lines added by other processes> (kh_remove, then kh_compare of
    <copy> against the file that <known_hosts> names). The last exact copy of the line is deleted in place: a link
    stays a link, its target keeps its inode, mode and every other byte. Exit 0 for "removed" with 0 lines missing.
  sp1_watch.py acc --state <workspaces folder> --out <file> <id>...   (node FX-38)
    The acceptance command of MS-23 for this run's ids only: <state>/<id>/mail/sync/state.json of each id into
    <file> (acceptance_text); prints note 3.4 (acc_note), one entry per id. No other workspace is read.

Rounds file, one JSON object per round: {"id": "R1", "sub": "1.1", "acc": true, "ws": <id>, "member": "cc",
"member_label": <the busy label>, "arm": "ssh-agent" | "ssh-key" | "https", "T": <int>, "end": <float>, "state":
<state.json object or null>, "errors": [[<unix s>, <text>], ...], "signs": <int or null>, "delivered": <bool>,
"branch": "zygo/mail", "attempt": 1}. acc true: an acceptance round (ok or FAIL); the last attempt of a sub id counts,
an earlier attempt becomes note 3.6.
"""
import argparse
import collections
import datetime
import errno
import json
import os
import re
import sys
import time

ENV_NAMES = ("SSH_AUTH_SOCK", "HOME", "PATH", "DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR", "XDG_CONFIG_HOME",
             "GIT_TRACE2_EVENT", "GIT_SSH_COMMAND", "GIT_ASKPASS", "SSH_ASKPASS")
SIDE_NAMES = ("git", "ssh", "gh", "curl", "wget", "scp", "sftp", "git-remote-https", "git-remote-http",
              "git-upload-pack", "git-receive-pack")
TICK_HZ = os.sysconf("SC_CLK_TCK") if hasattr(os, "sysconf") else 100
TICK = 1.0 / TICK_HZ   # resolution of a /proc start time
SUB = re.compile(r"^[a-z][a-z-]*$")


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


def sock_state(raw, want):
    """"match", "other" or "absent": the SSH_AUTH_SOCK of an environ block against the run agent's socket path."""
    for item in raw.split(b"\0"):
        if item.startswith(b"SSH_AUTH_SOCK="):
            v = item.split(b"=", 1)[1].decode("utf-8", "replace")
            return "match" if want and v == want else "other"
    return "absent"


def proc_name(argv, comm):
    """The base name of a process's program (argv[0], else comm)."""
    return os.path.basename(argv[0]) if argv and argv[0] else comm


def git_sub(argv):
    """A git subcommand word of argv (letters and dashes only), else ""."""
    if len(argv) >= 2 and os.path.basename(argv[0]) == "git" and SUB.match(argv[1]):
        return argv[1]
    return ""


def tcp_remote(hexaddr):
    """(ip text, port, loopback) of a /proc/net/tcp or tcp6 remote address field."""
    a, p = hexaddr.split(":")
    port = int(p, 16)
    if len(a) == 8:
        b = bytes.fromhex(a)[::-1]
        ip = ".".join(str(x) for x in b)
        return ip, port, b[0] == 127
    words = [bytes.fromhex(a[i:i + 8])[::-1] for i in range(0, 32, 8)]
    raw = b"".join(words)
    if raw[:12] == b"\0" * 10 + b"\xff\xff":   # IPv4-mapped
        ip = ".".join(str(x) for x in raw[12:])
        return ip, port, raw[12] == 127
    ip = ":".join("%x" % int.from_bytes(raw[i:i + 2], "big") for i in range(0, 16, 2))
    return ip, port, raw == b"\0" * 15 + b"\1"


def iso_epoch(s):
    """Unix time of a trace2 "time" value (UTC, "...Z")."""
    return datetime.datetime.strptime(s.rstrip("Z"), "%Y-%m-%dT%H:%M:%S.%f").replace(tzinfo=datetime.timezone.utc).timestamp()


def trace2_procs(lines):
    """The git processes of a trace2 event file: {sid: {"t": start time, "argv", "ancestry", "starts": [child_start
    events]}}. A nested git has a sid with "/" (its parent git's sid first)."""
    procs = {}
    for ln in lines:
        try:
            e = json.loads(ln)
        except ValueError:
            continue
        sid = e.get("sid")
        if not sid:
            continue
        p = procs.setdefault(sid, {"t": None, "argv": [], "ancestry": [], "starts": []})
        ev = e.get("event")
        if ev == "start":
            p["t"] = iso_epoch(e["time"])
            p["argv"] = e.get("argv", [])
        elif ev == "cmd_ancestry":
            p["ancestry"] = e.get("ancestry", [])
        elif ev == "child_start":
            p["starts"].append({"t": iso_epoch(e["time"]), "class": e.get("child_class", ""),
                                "argv": e.get("argv", []), "shell": e.get("use_shell", False)})
    return procs


def from_child(procs, sid):
    """True when the top git of sid was started by a sync child: its ancestry starts with zygo (the child), then
    systemd or init (the child was re-parented to the user's systemd or to pid 1 when its hook exited) or zygo (the
    hook had not exited yet)."""
    top = procs.get(sid.split("/")[0], {})
    anc = top.get("ancestry", [])
    return len(anc) >= 2 and anc[0] == "zygo" and anc[1] in ("systemd", "init", "zygo")


NET_SUBS = ("clone", "fetch", "pull", "push", "ls-remote", "remote-https", "remote-http", "remote-ssh", "archive")


def git_path(procs, lo, hi):
    """Counts of the child's git calls whose start time is in [lo, hi]: ssh starts, ssh starts with BatchMode=yes,
    ssh starts to ssh.github.com port 443, ssh starts to github.com (not ssh.github.com), credential helper calls,
    gh helper calls."""
    c = {"ssh": 0, "batch": 0, "ssh443": 0, "ssh22": 0, "helper": 0, "gh": 0, "gits": 0}
    for sid, p in procs.items():
        if not from_child(procs, sid) or p["t"] is None or not lo <= p["t"] <= hi:
            continue
        c["gits"] += 1
        for s in p["starts"]:
            words = " ".join(s["argv"])
            if s["class"] == "transport/ssh":
                c["ssh"] += 1
                c["batch"] += "BatchMode=yes" in words
                if re.search(r"(^|\s)(\S+@)?ssh\.github\.com(\s|$)", words) and re.search(r"-p '?443'?(\s|$)", words):
                    c["ssh443"] += 1
                elif re.search(r"(^|\s)(\S+@)?github\.com(\s|$)", words):
                    c["ssh22"] += 1
            elif re.search(r"(^|\s)(get|store|erase)$", words):   # git runs every credential helper as "<helper> <action>"
                c["helper"] += 1
                c["gh"] += "gh auth git-credential" in words
    return c


def git_word(argv):
    """The git subcommand of argv after its global options (-C <dir>, -c <k=v>, --flags), else "?"."""
    i = 1
    while i < len(argv):
        x = argv[i]
        if x in ("-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path"):
            i += 2
        elif x.startswith("-"):
            i += 1
        else:
            return x if SUB.match(x) else "?"
    return "?"


def side_git(procs, lo=None, hi=None):
    """Git processes not started by a sync child, whose start time is in [lo, hi] (None: no bound): (network
    ones, local ones), each a sorted list of "<parent>:<subcommand> xN" texts. A network one: a fetch-like
    subcommand or an ssh or remote-http child; a helper call ("<helper> get|store|erase") is shown as
    "+helper"."""
    net, loc = {}, {}
    for sid, p in procs.items():
        if "/" in sid or from_child(procs, sid):
            continue
        if p["t"] is not None and ((lo is not None and p["t"] < lo) or (hi is not None and p["t"] > hi)):
            continue
        sub = git_word(p["argv"])
        par = p["ancestry"][0] if p["ancestry"] else "?"
        starts = [s for q, x in procs.items() if q == sid or q.startswith(sid + "/") for s in x["starts"]]
        remote = sub in NET_SUBS or any(s["class"] in ("transport/ssh", "remote-https", "remote-http") for s in starts)
        helper = any(s["class"] != "transport/ssh" and re.search(r"(^|\s)(get|store|erase)$", " ".join(s["argv"])) for s in starts)
        d = net if remote else loc
        k = "%s:%s%s" % (par, sub, "+helper" if helper else "")
        d[k] = d.get(k, 0) + 1
    fmt_ = lambda d: sorted("%s x%d" % (k, v) for k, v in d.items())
    return fmt_(net), fmt_(loc)


def fmt(x):
    return "%+.3f s" % x


def yn(b):
    return "yes" if b else "no"


def judge_round(r, ev, procs=None):
    """The verdict of one round: (ok, text, child) from the watcher events ev (time order) and the trace2 git
    processes (trace2_procs)."""
    procs = procs or {}
    T, ws = r["T"], r["ws"]
    end = r.get("end") or (T + 600)
    start_ev = next((e for e in ev if e["k"] == "start"), {})
    subreaper = start_ev.get("subreaper")
    execd = {e["pid"] for e in ev if e["k"] == "hook_exec"}   # a hook's fork seen before its exec: not a hook
    hooks = {e["pid"]: dict(e) for e in ev if e["k"] == "hook" and e["pid"] not in execd}
    for e in ev:
        if e["k"] == "hook_gone" and e["pid"] in hooks and "gone" not in hooks[e["pid"]]:
            hooks[e["pid"]]["gone"], hooks[e["pid"]]["gone_prev"] = e["t"], e.get("prev", e["t"])
    kids = [dict(e) for e in ev if e["k"] == "child" and e["ws"] == ws and e["start"] >= T - 0.05 and e["t"] <= end]
    if not kids:
        return False, "%s %s/%s: no sync child of this workspace started after the prompt second T=%d" % (
            r["id"], r["member"], r["arm"], T), None
    c = kids[0]
    g = next((e for e in ev if e["k"] == "child_gone" and e["pid"] == c["pid"]), None)
    c["gone"], c["gone_prev"] = (g["t"], g.get("prev", g["t"])) if g else (None, None)
    c["reparent"] = next((e for e in ev if e["k"] == "child_ppid" and e["pid"] == c["pid"]), None)
    busy = [e for e in ev if e["k"] == "busy" and e["label"] == r["member_label"] and T - 0.05 <= e["t"] <= end]
    btv = next((e for e in busy if e["value"] is True), None)
    bfv = next((e for e in busy if e["value"] is False and btv is not None and e["t"] >= btv["t"]), None)
    bt, bf = (btv["t"] if btv else None), (bfv["t"] if bfv else None)
    bt_prev, bf_prev = (btv.get("prev", bt) if btv else None), (bfv.get("prev", bf) if bfv else None)
    cs = c["start"]   # the child started in [cs, cs + TICK)

    # Parent ended before the child (finding 6): evidence only.
    parent = hooks.get(c["ppid"])
    reaper = c["ppid"] == 1 or (subreaper is not None and c["ppid"] == subreaper) or (subreaper is None and c["pcomm"] == "systemd")
    if parent is not None and parent.get("gone") is not None and c["gone_prev"] is not None and parent["gone"] <= c["gone_prev"]:
        ended, pnote = True, "its parent, the sampled %s hook, ended in (%s, %s]" % (
            parent["event"], fmt(parent["gone_prev"] - T), fmt(parent["gone"] - T))
    elif reaper:
        ended, pnote = c["gone"] is not None, "first sampled at %s with parent pid %s (%s, the subreaper): re-parented, so its parent had ended" % (
            fmt(c["t"] - T), "1" if c["ppid"] == 1 else "of the user's systemd", c["pcomm"])
    elif c["reparent"] is not None and c["reparent"].get("ppid") != c["ppid"]:
        ended = c["gone"] is not None and c["reparent"]["t"] <= (c["gone_prev"] or 0)
        pnote = "its parent changed from pid %d to %s in (%s, %s]: its parent ended" % (
            c["ppid"], c["reparent"]["pcomm"], fmt(c["reparent"].get("prev", c["reparent"]["t"]) - T), fmt(c["reparent"]["t"] - T))
    else:
        ended, pnote = False, "first sampled with a live parent %s (pid %d) that is not a sampled hook, and no change seen: no evidence that its parent ended" % (
            c["pcomm"], c["ppid"])

    # Attribution (finding 7): measured margins with their bounds, or fail.
    prompts = [h for h in hooks.values() if h.get("ws") == ws and h["event"] == "prompt" and T - 0.05 <= h["start"] <= cs + TICK]
    others = [h for h in hooks.values() if h.get("ws") == ws and h["event"] != "prompt" and T - 0.05 <= h["start"] <= cs + TICK]
    if parent is not None:
        how = "parent" if parent["event"] == "prompt" else "parent:" + parent["event"]
        anote = "its parent pid is the sampled %s hook (started %s)" % (parent["event"], fmt(parent["start"] - T))
    elif prompts and not others:
        h = prompts[-1]
        how = "time"
        anote = ("a sampled prompt hook of this workspace started %s (%s resolution) and no other hook of it before the "
                 "child; child start %s - prompt hook start = %s (%s)") % (
            fmt(h["start"] - T), "+/-%.3f s" % TICK, fmt(cs - T), "%.3f s" % (cs - h["start"]), "+/-%.3f s" % TICK)
    elif not others and bt_prev is not None and cs + TICK <= bt_prev:
        how = "busy"
        anote = "the child started before %s, and busy true (written by the prompt hook after it starts the child) was written in (%s, %s]" % (
            fmt(cs + TICK - T), fmt(bt_prev - T), fmt(bt - T))
    else:
        how = "unknown"
        anote = "no sampled prompt hook as parent or before the child, other hook events before it: %s; child start %s, busy true in %s" % (
            ", ".join(sorted({h["event"] for h in others})) or "none", fmt(cs - T),
            "(%s, %s]" % (fmt(bt_prev - T), fmt(bt - T)) if bt is not None else "not seen")
    ok_event = how in ("parent", "time", "busy")

    order = bf is not None and c["gone_prev"] is not None and bf <= c["gone_prev"]
    sid_ok = c["sid"] == c["pid"] and c.get("pgid", c["pid"]) == c["pid"]
    st = r.get("state") or {}
    last_ok, last_error = st.get("last_ok"), st.get("last_error")
    verdict = (st.get("public") or {}).get("verdict", "none")
    ok_state = isinstance(last_ok, int) and last_ok > T and last_error is None
    errs = [e for e in r.get("errors") or [] if T <= e[0] <= end]
    delivered = r.get("delivered", False)

    # Git path, agent and environment per arm (findings 2, 11).
    hi = c["gone"] if c["gone"] is not None else end
    gp = git_path(procs, cs - 0.05, hi + 0.05)
    signs = r.get("signs")
    arm = r["arm"]
    sock = c.get("sock", "absent")
    if arm == "ssh-agent":
        ok_git = gp["ssh"] >= 1 and gp["batch"] == gp["ssh"] and gp["ssh443"] == gp["ssh"] and gp["helper"] == 0 and (signs or 0) >= 1 and sock == "match"
    elif arm == "ssh-key":
        ok_git = gp["ssh"] >= 1 and gp["batch"] == gp["ssh"] and gp["ssh22"] == gp["ssh"] and gp["helper"] == 0 and (signs or 0) == 0 and sock == "absent"
    else:
        ok_git = gp["helper"] >= 1 and gp["gh"] == gp["helper"] and gp["ssh"] == 0 and (signs or 0) == 0
    gtext = ("git calls of the child (trace2, ancestry zygo then systemd): %d git processes; ssh starts %d (BatchMode=yes %d; "
             "to ssh.github.com port 443 %d; to github.com %d); credential helper calls %d (gh %d); run agent sign requests %s; "
             "SSH_AUTH_SOCK of the child: %s") % (gp["gits"], gp["ssh"], gp["batch"], gp["ssh443"], gp["ssh22"], gp["helper"],
                                                   gp["gh"], signs, {"match": "the run agent's socket", "other": "another socket",
                                                                    "absent": "not set"}[sock])

    ok = sid_ok and ended and ok_event and order and ok_state and not errs and delivered and ok_git
    text = ("%s %s/%s (branch %s, attempt %d, T=%d): prompt delivered to the mock: %s; child pid = sid = pgid: %s, started %s "
            "(+/-%.3f s), first sampled %s; parent ended before the child: %s (%s); started by the prompt hook: %s (%s); "
            "turn end (busy false) in (%s, %s]; child end in (%s, %s]; turn ended before the child: %s; state.json "
            "last_ok = %s, last_error %s; public verdict %s; errors.log lines in the round: %d; %s; git path ok: %s") % (
        r["id"], r["member"], arm, r.get("branch", "?"), r.get("attempt", 1), T, yn(delivered), yn(sid_ok), fmt(cs - T), TICK,
        fmt(c["t"] - T), yn(ended), pnote, yn(ok_event), anote,
        fmt(bf_prev - T) if bf is not None else "?", fmt(bf - T) if bf is not None else "not seen",
        fmt(c["gone_prev"] - T) if c["gone"] is not None else "?", fmt(c["gone"] - T) if c["gone"] is not None else "not seen",
        yn(order), ("T%+d" % (last_ok - T)) if isinstance(last_ok, int) else "absent",
        "absent" if last_error is None else "= " + str(last_error), verdict, len(errs), gtext, yn(ok_git))
    if errs:
        text += "; errors.log: " + " | ".join(e[1] for e in errs)
    return ok, text, c


def order_shown(r, events):
    """"yes" when the round's child was seen and its turn ended before it, else "no"."""
    _, text, c = judge_round(r, events)
    return "yes" if c is not None and "turn ended before the child: yes" in text else "no"


def judge(events, rounds, trace_lines):
    procs = trace2_procs(trace_lines)
    out, envs = [], {}
    last = {}
    for r in rounds:
        if r.get("acc"):
            last[r["sub"]] = r["id"], r.get("attempt", 1)
    for r in rounds:
        ok, text, c = judge_round(r, events, procs)
        if c is not None:
            envs["%s#%d" % (r["id"], r.get("attempt", 1))] = c.get("env", [])
        if r.get("acc") and last[r["sub"]] != (r["id"], r.get("attempt", 1)):
            out.append(("3.6", "note", "attempt repeated on a new branch (S§20 SP1 fallback): " + text))
        else:
            out.append((r["sub"], ("0" if ok else "1") if r.get("acc") else "note", text))
    if envs:
        out.append(("3.3", "note", "child environment names present (values not read): " +
                    "; ".join("%s: %s" % (k, " ".join(v) or "none") for k, v in envs.items())))
    stop = next((e for e in events if e["k"] == "stop"), None)
    if stop:
        f, q = stop["full"], stop["poll"]
        out.append(("3.7", "note", ("watcher: %d full scans of /proc (stat and cmdline of every process), gap mean %.4f s, p99 "
                    "%.4f s, max %.4f s; between them %d fast polls (children of the %d agent processes and of live hooks, "
                    "hooks and children, activity and sync files), gap mean %.5f s, p99 %.4f s, max %.4f s; each event "
                    "happened in (prev, t] of its line; /proc start times have %.3f s resolution, boot time from "
                    "CLOCK_BOOTTIME; largest start time after the time it was seen: %.4f s") % (
                        stop["samples"], f["mean"], f["p99"], f["max"], stop["polls"], stop["agents"], q["mean"], q["p99"],
                        q["max"], TICK, stop["max_start_skew"])))
    return out


def side_traffic(events, trace_lines, files, lo, hi):
    """The 1.8 verdict and its text (finding 1). Fails on: a git network call, or an ssh, gh, curl or wget process,
    or a TCP connection off the machine, of a run process outside the sync children in the rounds' window [lo, hi];
    a git network call of Claude Code or OpenCode (a marketplace clone) at any time; files: {"known_marketplaces":
    bool, "git_dirs": int} of the run config folders. Outside the window the same calls are listed, not judged."""
    procs = trace2_procs(trace_lines)
    net_all, loc_all = side_git(procs)
    agent_net = [k for k in net_all if k.split(":")[0] in ("claude", "node", "opencode", "bun")]
    net_in, loc_in = side_git(procs, lo, hi)
    ins = lambda e: lo <= e["t"] <= hi
    ps = [e for e in events if e["k"] == "proc" and e["name"] != "git" and not e["in_child"]]
    show_p = lambda es: sorted({"%s(%s)" % (e["name"], "<".join(e["chain"][:3])) for e in es})
    conns = [e for e in events if e["k"] == "conn" and not e["in_child"]]

    def show_c(es):
        n = {}
        for e in es:
            k = "%s %s" % (e["name"], e["remote"])
            n[k] = n.get(k, 0) + 1
        return sorted("%s x%d" % (k, v) for k, v in n.items())
    p_in, c_in = [e for e in ps if ins(e)], [e for e in conns if ins(e)]
    child_ps = sorted({e["name"] for e in events if e["k"] == "proc" and e["in_child"]})
    child_c = sorted({"%s %s" % (e["name"], e["remote"]) for e in events if e["k"] == "conn" and e["in_child"]})
    ok = not net_in and not p_in and not c_in and not agent_net and not files.get("known_marketplaces") and \
        files.get("git_dirs", 0) == 0
    text = ("in the rounds' window, outside the sync children: git network calls (trace2): %s; ssh, gh, curl or wget "
            "processes: %s; TCP connections off the machine: %s; local git calls (parent:subcommand): %s. Git network "
            "calls of Claude Code or OpenCode in the whole run: %s; run-local known_marketplaces.json present: %s; .git "
            "folders under the run config folders: %d. Whole run, outside the sync children (listed, not judged): git "
            "network calls %s; processes %s; TCP connections %s. Inside the sync children: programs %s; connections %s") % (
        ", ".join(net_in) or "none", ", ".join(show_p(p_in)) or "none", ", ".join(show_c(c_in)) or "none",
        ", ".join(loc_in) or "none", ", ".join(agent_net) or "none", yn(files.get("known_marketplaces")),
        files.get("git_dirs", 0), ", ".join(net_all) or "none", ", ".join(show_p(ps)) or "none",
        ", ".join(show_c(conns)) or "none", ", ".join(child_ps) or "none", ", ".join(child_c) or "none")
    return ok, text


# --- known_hosts: the run's one line, added and removed through a link (node FX-38) -----------------------------

def ename(e):
    """The errno name of an OSError (no path: the text can reach the fixture)."""
    return errno.errorcode.get(e.errno, "errno %s" % e.errno)


def kh_line(line_file):
    with open(line_file, "rb") as f:
        return f.read().rstrip(b"\n")


def kh_add(path, line_file, save):
    """Append the run's line (the one line of <line_file>) to the file that <path> names (through a link).

    "present": the file already holds the line (not changed); "added": the bytes of the file were saved to <save>,
    then the line was appended, flushed and fsync'd; "failed: <reason>": else (no file, not a regular file, no line
    end at its end, the copy or the append failed). "added" is returned only after the append succeeded.
    """
    try:
        add = kh_line(line_file)
    except OSError as e:
        return "failed: the run's line could not be read (%s)" % ename(e)
    if not add or b"\n" in add:
        return "failed: the run's line file does not hold one line"
    real = os.path.realpath(path)
    if not os.path.isfile(real):
        return "failed: known_hosts is not a regular file"
    try:
        with open(real, "rb") as f:
            text = f.read()
    except OSError as e:
        return "failed: known_hosts could not be read (%s)" % ename(e)
    if text and not text.endswith(b"\n"):
        return "failed: known_hosts does not end in a line end; the run does not change it"
    if add in text.split(b"\n"):
        return "present"
    try:
        fd = os.open(save, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
        with os.fdopen(fd, "wb") as f:
            f.write(text)
    except OSError as e:
        return "failed: the copy of known_hosts could not be saved (%s)" % ename(e)
    try:
        with open(real, "ab") as f:
            f.write(add + b"\n")
            f.flush()
            os.fsync(f.fileno())
    except OSError as e:
        try:
            with open(real, "rb") as f:
                same = f.read() == text
        except OSError:
            same = False
        return "failed: the append failed (%s)%s" % (
            ename(e), "" if same else "; known_hosts is not equal to the saved copy")
    return "added"


def kh_remove(path, line_file):
    """Delete the last exact copy of the run's line from the file that <path> names, in place, through a link.

    The link stays a link; the target keeps its inode and mode; every other byte is unchanged. "removed"; "the
    added line is not in the file" (nothing changed); "failed: <reason>".
    """
    try:
        add = kh_line(line_file)
    except OSError as e:
        return "failed: the run's line could not be read (%s)" % ename(e)
    real = os.path.realpath(path)
    if not os.path.isfile(real):
        return "failed: known_hosts is not a regular file"
    try:
        with open(real, "rb") as f:
            if add not in f.read().split(b"\n"):
                return "the added line is not in the file"
        with open(real, "r+b") as f:
            lines = f.read().split(b"\n")
            idx = [i for i, x in enumerate(lines) if x == add]
            if not idx:
                return "the added line is not in the file"
            del lines[idx[-1]]
            f.seek(0)
            f.write(b"\n".join(lines))
            f.truncate()
            f.flush()
            os.fsync(f.fileno())
    except OSError as e:
        return "failed: known_hosts could not be rewritten (%s)" % ename(e)
    return "removed"


def kh_compare(start, path):
    """(lines missing, lines added by other processes): the saved copy against the file that <path> names."""
    def lines(p):
        with open(p, "rb") as f:
            x = f.read().split(b"\n")
        return collections.Counter(x[:-1] if x and x[-1] == b"" else x)
    a, b = lines(start), lines(os.path.realpath(path))
    return sum((a - b).values()), sum((b - a).values())


# --- note 3.4: the acceptance command, only this run's workspaces (node FX-38) ---------------------------------

WSID = re.compile(r"^[A-Za-z0-9_-]{1,128}$")


def acceptance_text(state, ids):
    """For each id of this run (sorted): the path of <state>/<id>/mail/sync/state.json, its text, a blank line.

    Only these ids are read; an id that is not a plain name or has no state.json gives no entry.
    """
    out = []
    for i in sorted(set(ids)):
        if not WSID.match(i):
            continue
        p = os.path.join(state, i, "mail", "sync", "state.json")
        t = read(p)
        if t is None:
            continue
        out.append("%s\n%s\n" % (p, t))
    return "\n".join(out)


def acc_note(text, mine):
    """Note 3.4: one entry per id of <mine> (sorted; id shortened; URL hashes and blob ids left out)."""
    txt, got, i = text.split("\n"), {}, 0
    while i < len(txt):
        p = txt[i]
        if p.endswith("/mail/sync/state.json"):
            ws = os.path.basename(os.path.dirname(os.path.dirname(os.path.dirname(p))))
            j, buf = i + 1, []
            while j < len(txt) and not txt[j].endswith("/mail/sync/state.json"):
                buf.append(txt[j])
                j += 1
            got[ws] = "\n".join(buf)
            i = j
        else:
            i += 1
    out = []
    for ws in sorted(set(mine)):
        if ws not in got:
            out.append("%s… (this run): no state.json" % ws[:8])
            continue
        try:
            s = json.loads(got[ws])
        except ValueError:
            s = None
        if not isinstance(s, dict):
            out.append("%s… (this run): unreadable" % ws[:8])
        else:
            out.append("%s… (this run): last_ok %s, last_error %s, public %s, pushed %s" % (
                ws[:8], s.get("last_ok"), "absent" if s.get("last_error") is None else s["last_error"],
                (s.get("public") or {}).get("verdict"), " ".join(sorted((s.get("pushed") or {}).keys())) or "none"))
    return "; ".join(out) or "no state.json"


# --- the watcher ------------------------------------------------------------------------------------------------

def read(path, mode="r"):
    try:
        with open(path, mode) as f:
            return f.read()
    except OSError:
        return None


def user_systemd():
    for d in os.listdir("/proc"):
        if d.isdigit():
            raw = read("/proc/%s/cmdline" % d, "rb") or b""
            st = read("/proc/%s/status" % d) or ""
            if os.path.basename(raw.split(b"\0")[0].decode("utf-8", "replace")) == "systemd" and b"--user" in raw and \
                    ("Uid:\t%d\t" % os.getuid()) in st:
                return int(d)
    return None


def rd(path, limit=1 << 16):
    """The bytes of a /proc file (one read), or None."""
    try:
        fd = os.open(path, os.O_RDONLY)
    except OSError:
        return None
    try:
        return os.read(fd, limit)
    except OSError:
        return None
    finally:
        os.close(fd)


def argv_of(raw):
    return [x.decode("utf-8", "replace") for x in raw.rstrip(b"\0").split(b"\0")] if raw else []


AGENT_NAMES = ("claude", "opencode")
FAST_SPAN = 0.008   # seconds of fast polls between two full scans


def watch(a):
    # process start times count clock ticks from boot on CLOCK_BOOTTIME: boot is read from the same clock (ns)
    boot = time.time() - time.clock_gettime(time.CLOCK_BOOTTIME)
    wss, busy, cfg_m = [], [], None
    foreign = {}    # socket inode -> full scans in which no run process held it (skipped after 3)
    out = open(a.out, "a", buffering=1)
    clock = {"poll": time.time(), "full": time.time()}

    def put(prev, **o):
        o["t"], o["prev"] = round(time.time(), 4), round(prev, 4)
        out.write(json.dumps(o, sort_keys=True) + "\n")

    put(clock["poll"], k="start", boot=round(boot, 4), subreaper=user_systemd(), fast_span=FAST_SPAN)
    seen = {}       # pid -> [kind, ppid, start ticks] of hooks and children
    side = set()    # pids reported as "proc"
    inodes = set()  # socket inodes reported
    vals = {}
    full_gaps, poll_gaps, skew = Gaps(), Gaps(), [0.0]
    agents, comm_of = set(), {}

    def new_proc(pid, st, raw, prev):
        """Classify a process not yet in seen; emit its hook or child line."""
        if b"zygo" not in raw:
            return
        argv = argv_of(raw)
        c = classify(argv)
        if c is None:
            return
        comm, ppid, pgid, sid, start, pstate = st
        if pstate == "Z":
            return
        envb = rd("/proc/%d/environ" % pid, 1 << 20) or b""
        ws = ""
        for item in envb.split(b"\0"):
            if item.startswith(b"ZYGO_WORKSPACE="):
                ws = item.split(b"=", 1)[1].decode("ascii", "replace")
        t0 = boot + start / TICK_HZ
        skew[0] = max(skew[0], t0 - time.time())   # a start after the time it was seen: the clock error
        seen[pid] = [c[0], ppid, start]
        pst = rd("/proc/%d/stat" % ppid)
        pcomm = parse_stat(pst.decode("utf-8", "replace"))[0] if pst else comm_of.get(ppid, "?")
        if c[0] == "hook":
            put(prev, k="hook", pid=pid, ppid=ppid, pcomm=pcomm, event=c[1], agent=c[2], ws=ws, start=round(t0, 4))
        else:
            put(prev, k="child", pid=pid, ppid=ppid, pcomm=pcomm, sid=sid, pgid=pgid, ws=c[1], start=round(t0, 4),
                env=env_names(envb), sock=sock_state(envb, a.agent_sock))

    def check_seen(prev):
        """Hooks and children: gone (or a zombie: exited, not reaped) and parent changes."""
        for pid in list(seen):
            kind, ppid, start = seen[pid]
            s = rd("/proc/%d/stat" % pid)
            st = parse_stat(s.decode("utf-8", "replace")) if s else None
            if st is None or st[5] == "Z" or st[4] != start:
                put(prev, k=kind + "_gone", pid=pid)
                del seen[pid]
            elif kind == "hook":
                # a hook's fork shows the hook's argv until it execs: classified again, it may be a sync child
                c = classify(argv_of(rd("/proc/%d/cmdline" % pid) or b""))
                if c is not None and c[0] == "child":
                    put(prev, k="hook_exec", pid=pid)
                    del seen[pid]
                    new_proc(pid, st, rd("/proc/%d/cmdline" % pid) or b"", prev)
            elif kind == "child" and st[1] != ppid:
                seen[pid][1] = st[1]
                ps = rd("/proc/%d/stat" % st[1])
                put(prev, k="child_ppid", pid=pid, ppid=st[1], pcomm=parse_stat(ps.decode("utf-8", "replace"))[0] if ps else "?")

    def check_files(prev):
        for label, path in busy:
            txt = rd(path)
            v = None
            if txt:
                try:
                    v = json.loads(txt).get("busy")
                except ValueError:
                    v = None
            if vals.get(("b", label), "unset") != v:
                vals[("b", label)] = v
                put(prev, k="busy", label=label, value=v)
        for ws, d in wss:
            ls = rd(os.path.join(d, "mail", "sync", "last_start"))
            ls = ls.decode("ascii", "replace").strip() if ls else None
            if vals.get(("l", ws), "unset") != ls:
                vals[("l", ws)] = ls
                put(prev, k="last_start", ws=ws, value=ls)
            try:
                m = os.stat(os.path.join(d, "mail", "sync", "state.json")).st_mtime
            except OSError:
                m = None
            if vals.get(("s", ws), "unset") != m:
                vals[("s", ws)] = m
                put(prev, k="state", ws=ws, mtime=m)

    def fast_poll(prev):
        """The children of every agent process and live hook, read from /proc/<pid>/task/*/children."""
        for p in list(agents) + [x for x, v in seen.items() if v[0] == "hook"]:
            try:
                tasks = os.listdir("/proc/%d/task" % p)
            except OSError:
                agents.discard(p)
                continue
            for t in tasks:
                kids = rd("/proc/%d/task/%s/children" % (p, t))
                for k in (kids or b"").split():
                    pid = int(k)
                    if pid in seen:
                        continue
                    s, raw = rd("/proc/%d/stat" % pid), rd("/proc/%d/cmdline" % pid)
                    if s and raw:
                        try:
                            new_proc(pid, parse_stat(s.decode("utf-8", "replace")), raw, prev)
                        except (ValueError, IndexError):
                            pass
        check_seen(prev)
        check_files(prev)

    while not os.path.exists(a.stop):
        # --- full scan: every process, stat and cmdline read again at each scan ---
        now = time.time()
        full_gaps.add(now - clock["full"])
        prev_full, prev_poll = clock["full"], clock["poll"]
        if a.cfg:
            try:
                m = os.stat(a.cfg).st_mtime
            except OSError:
                m = None
            if m is not None and m != cfg_m:
                try:
                    c = json.loads(rd(a.cfg) or b"")
                    wss, busy, cfg_m = c.get("ws", []), c.get("busy", []), m
                except ValueError:
                    pass   # being written; read again at the next scan
        info = {}
        for d in os.listdir("/proc"):
            if not d.isdigit():
                continue
            s = rd("/proc/%s/stat" % d)
            raw = rd("/proc/%s/cmdline" % d)
            if not s:
                continue
            try:
                info[int(d)] = (parse_stat(s.decode("utf-8", "replace")), raw or b"")
            except (ValueError, IndexError):
                continue
        comm_of.clear()
        comm_of.update((p, v[0][0]) for p, v in info.items())
        for pid, (st, raw) in info.items():
            if pid not in seen:
                new_proc(pid, st, raw, prev_full)
        check_seen(prev_poll)
        children = {p for p, v in seen.items() if v[0] == "child"}
        run = set()
        if a.root:
            kids = {}
            for pid, (st, _) in info.items():
                kids.setdefault(st[1], []).append(pid)
            todo = [a.root]
            while todo:
                p = todo.pop()
                if p in run:
                    continue
                run.add(p)
                todo.extend(kids.get(p, []))
        for pid, (st, _) in info.items():
            if st[3] in children:
                run.add(pid)
        names = {p: proc_name(argv_of(info[p][1].split(b"\0", 1)[0]), info[p][0][0]) for p in run}
        agents.clear()
        agents.update(p for p in run if names[p] in AGENT_NAMES or info[p][0][0] in AGENT_NAMES)

        def chain(pid):
            out_, p = [], info.get(pid, ((None, 0),))[0][1]
            while p and p in info and p != a.root and len(out_) < 8:
                out_.append(proc_name(argv_of(info[p][1].split(b"\0", 1)[0]), info[p][0][0]))
                p = info[p][0][1]
            return out_

        for pid in run:
            if pid in side:
                continue
            st, raw = info[pid]
            if names[pid] in SIDE_NAMES:
                side.add(pid)
                put(prev_full, k="proc", pid=pid, ppid=st[1], sid=st[3], name=names[pid], sub=git_sub(argv_of(raw)),
                    chain=chain(pid), in_child=st[3] in children)
        if a.root:
            fresh = {}
            for f in ("/proc/net/tcp", "/proc/net/tcp6"):
                for ln in (rd(f, 1 << 22) or b"").decode("ascii", "replace").splitlines()[1:]:
                    x = ln.split()
                    if len(x) < 10 or x[3] not in ("01", "02") or x[9] == "0" or x[9] in inodes or foreign.get(x[9], 0) >= 3:
                        continue
                    ip, port, lo = tcp_remote(x[2])
                    if not lo:
                        fresh[x[9]] = "%s:%d" % (ip, port)
            if fresh:
                for pid in run:
                    try:
                        fds = os.listdir("/proc/%d/fd" % pid)
                    except OSError:
                        continue
                    for fd in fds:
                        try:
                            link = os.readlink("/proc/%d/fd/%s" % (pid, fd))
                        except OSError:
                            continue
                        if link.startswith("socket:[") and link[8:-1] in fresh and link[8:-1] not in inodes:
                            ino = link[8:-1]
                            inodes.add(ino)
                            st = info[pid][0]
                            put(prev_full, k="conn", remote=fresh[ino], pid=pid, name=names[pid], chain=chain(pid),
                                in_child=st[3] in children)
                for ino in fresh:
                    if ino not in inodes:
                        foreign[ino] = foreign.get(ino, 0) + 1
        check_files(prev_poll)
        clock["full"] = clock["poll"] = now
        # --- fast polls until the next full scan ---
        end = time.time() + FAST_SPAN
        while time.time() < end:
            t = time.time()
            poll_gaps.add(t - clock["poll"])
            fast_poll(clock["poll"])
            clock["poll"] = t
            time.sleep(0.0002)   # gives the CPU back between polls
        if a.every > 0:
            time.sleep(a.every)

    put(clock["poll"], k="stop", samples=full_gaps.n, full=full_gaps.stats(), polls=poll_gaps.n, poll=poll_gaps.stats(),
        agents=len(agents), max_start_skew=round(skew[0], 4))


class Gaps:
    """Count, mean, max and p99 (0.1 ms bins) of the gaps between polls, in constant memory."""

    def __init__(self):
        self.n, self.total, self.max, self.bins = 0, 0.0, 0.0, {}

    def add(self, g):
        self.n += 1
        self.total += g
        self.max = max(self.max, g)
        b = int(g * 10000)
        self.bins[b] = self.bins.get(b, 0) + 1

    def stats(self):
        if not self.n:
            return {"max": 0.0, "p99": 0.0, "mean": 0.0}
        want, seen, p99 = self.n * 0.99, 0, 0.0
        for b in sorted(self.bins):
            seen += self.bins[b]
            if seen >= want:
                p99 = (b + 1) / 10000.0
                break
        return {"max": round(self.max, 4), "p99": round(p99, 4), "mean": round(self.total / self.n, 5)}


def load(path):
    out = []
    for x in (read(path) or "").splitlines():
        try:
            out.append(json.loads(x))
        except ValueError:
            pass   # a line being written
    return out


def wait_loop(timeout, test):
    end = time.time() + timeout
    while time.time() < end:
        if test():
            return 0
        time.sleep(0.05)
    return 1


def main():
    p = argparse.ArgumentParser()
    sp = p.add_subparsers(dest="cmd", required=True)
    w = sp.add_parser("watch")
    w.add_argument("--out", required=True)
    w.add_argument("--stop", required=True)
    w.add_argument("--every", type=float, default=0.0)
    w.add_argument("--root", type=int, default=0)
    w.add_argument("--agent-sock", default="")
    w.add_argument("--cfg", default="")
    t = sp.add_parser("wait-turn")
    t.add_argument("--watch", required=True)
    t.add_argument("--label", required=True)
    t.add_argument("--after", type=float, required=True)
    t.add_argument("--timeout", type=float, required=True)
    c = sp.add_parser("wait-child")
    c.add_argument("--watch", required=True)
    c.add_argument("--ws", required=True)
    c.add_argument("--after", type=float, required=True)
    c.add_argument("--timeout", type=float, required=True)
    c.add_argument("--gone", action="store_true")
    j = sp.add_parser("judge")
    j.add_argument("--watch", required=True)
    j.add_argument("--rounds", required=True)
    j.add_argument("--trace", default="")
    j.add_argument("--order", default="")
    s = sp.add_parser("side")
    s.add_argument("--watch", required=True)
    s.add_argument("--trace", default="")
    s.add_argument("--files", required=True, help="JSON: {known_marketplaces: bool, git_dirs: int}")
    s.add_argument("--lo", type=float, required=True)
    s.add_argument("--hi", type=float, required=True)
    ka = sp.add_parser("kh-add")
    ka.add_argument("--file", required=True)
    ka.add_argument("--line", required=True)
    ka.add_argument("--save", required=True)
    kr = sp.add_parser("kh-remove")
    kr.add_argument("--file", required=True)
    kr.add_argument("--line", required=True)
    kr.add_argument("--start", required=True)
    ac = sp.add_parser("acc")
    ac.add_argument("--state", required=True)
    ac.add_argument("--out", required=True)
    ac.add_argument("ids", nargs="*")
    a = p.parse_args()
    if a.cmd == "kh-add":
        r = kh_add(a.file, a.line, a.save)
        sys.stdout.write(r + "\n")
        sys.exit(0 if r in ("added", "present") else 1)
    if a.cmd == "kh-remove":
        r = kh_remove(a.file, a.line)
        try:
            miss, more = kh_compare(a.start, a.file)
        except OSError:
            miss, more = "?", "?"
        sys.stdout.write("%s\t%s\t%s\n" % (r, miss, more))
        sys.exit(0 if r == "removed" and miss == 0 else 1)
    if a.cmd == "acc":
        text = acceptance_text(a.state, a.ids)
        with open(a.out, "w") as f:
            f.write(text)
        sys.stdout.write(acc_note(text, a.ids) + "\n")
        return
    if a.cmd == "watch":
        watch(a)
        return
    if a.cmd == "wait-turn":
        def turn():
            ev = load(a.watch)
            bt = next((e["t"] for e in ev if e["k"] == "busy" and e["label"] == a.label and e["value"] is True and e["t"] > a.after), None)
            return bt is not None and any(e["k"] == "busy" and e["label"] == a.label and e["value"] is False and e["t"] >= bt for e in ev)
        sys.exit(wait_loop(a.timeout, turn))
    if a.cmd == "wait-child":
        def child():
            ev = load(a.watch)
            k = next((e for e in ev if e["k"] == "child" and e["ws"] == a.ws and e["start"] >= a.after - 0.05), None)
            return k is not None and (not a.gone or any(e["k"] == "child_gone" and e["pid"] == k["pid"] for e in ev))
        sys.exit(wait_loop(a.timeout, child))
    events = load(a.watch)
    trace = (read(a.trace) or "").splitlines() if a.trace else []
    if a.cmd == "side":
        ok, text = side_traffic(events, trace, json.loads(a.files), a.lo, a.hi)
        sys.stdout.write("%s\t%s\n" % ("0" if ok else "1", text))
        return
    rounds = load(a.rounds)
    if a.order:
        r = [x for x in rounds if x["id"] == a.order][-1]
        sys.stdout.write(order_shown(r, events) + "\n")
        return
    for sub, word, text in judge(events, rounds, trace):
        sys.stdout.write("%s\t%s\t%s\n" % (sub, word, text.replace("\t", " ").replace("\n", " ")))


if __name__ == "__main__":
    main()

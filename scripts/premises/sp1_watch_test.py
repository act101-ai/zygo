#!/usr/bin/env python3
"""Tests of sp1_watch.py (node MS-23): python3 -I scripts/premises/sp1_watch_test.py -> "sp1_watch tests: ok (<n>)".

Pure parts only: argv classes, /proc stat parsing, environ names, git trace times, and the judge on authored
watcher events. No process is started, nothing is read from /proc, no network.
"""
import datetime
import os
import sys

sys.dont_write_bytecode = True   # no __pycache__ in the checkout
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sp1_watch as w  # noqa: E402

n = 0
bad = 0


def check(name, got, want):
    global n, bad
    n += 1
    if got == want:
        print("ok: %s" % name)
    else:
        bad += 1
        print("FAIL: %s: got %r, want %r" % (name, got, want))


# classify
check("claude hook", w.classify(["/x/zygo", "hook", "workspace", "--event", "prompt"]), ("hook", "prompt", "claude-code"))
check("opencode hook", w.classify(["/x/zygo", "hook", "workspace", "--agent", "opencode", "--event", "prompt"]),
      ("hook", "prompt", "opencode"))
check("other hook", w.classify(["/x/zygo", "hook", "prompt"]), None)
check("sync child", w.classify(["/x/zygo", "mail", "sync", "--ws-id", "abc123", "--background"]), ("child", "abc123"))
check("cli sync", w.classify(["/x/zygo", "mail", "sync", "--ws-id", "abc123"]), None)
check("not zygo", w.classify(["/usr/bin/sh", "-c", "zygo mail sync --ws-id a --background"]), None)
check("empty", w.classify([]), None)

# parse_stat: comm with a space and a paren
stat = "4242 (zy go) x) S 4000 4242 4242 0 -1 4194560 1 0 0 0 0 0 0 0 20 0 1 0 98765 1000 10"
check("stat fields", w.parse_stat(stat), ("zy go) x", 4000, 4242, 4242, 98765, "S"))

# env_names: names only, sorted, unknown names left out
raw = b"HOME=/h\0SSH_AUTH_SOCK=/s\0SECRET=v\0PATH=/p\0GIT_TRACE=/t\0"
check("env names", w.env_names(raw), ["GIT_TRACE", "HOME", "PATH", "SSH_AUTH_SOCK"])

# trace_epoch: the day nearest to the reference, also across midnight
ref = datetime.datetime(2026, 11, 27, 12, 0, 0).timestamp()
check("trace same day", w.trace_epoch("12:00:05.500000", ref), ref + 5.5)
late = datetime.datetime(2026, 11, 27, 23, 59, 59).timestamp()
check("trace after midnight", round(w.trace_epoch("00:00:01.000000", late) - late, 3), 2.0)

# trace_hits
lines = [
    "12:00:02.000000 run-command.c:673       trace: run_command: unset GIT_PREFIX; 'ssh -o BatchMode=yes' -p 443 git@ssh.github.com 'git-upload-pack '\\''/o/r.git'\\'''",
    "12:00:03.000000 run-command.c:673       trace: run_command: '/usr/bin/gh auth git-credential get'",
    "12:00:09.000000 run-command.c:673       trace: run_command: '/usr/bin/gh auth git-credential get'",
    "12:00:04.000000 git.c:476               trace: built-in: git push",
]
check("ssh hits in window", len(w.trace_hits(lines, ref + 1, ref + 5, w.SSH_LINE)), 1)
check("helper hits in window", len(w.trace_hits(lines, ref + 1, ref + 5, w.HELPER_LINE)), 1)
check("helper hits outside", len(w.trace_hits(lines, ref + 10, ref + 20, w.HELPER_LINE)), 0)

# judge: one acceptance round whose hook ends, then the turn, then the child
T = 1000


def ev_round(ws, label, child_gone, busy_false, hook_event="prompt", reparent=True):
    e = [
        {"k": "busy", "label": label, "value": False, "t": T - 5},
        {"k": "hook", "pid": 10, "ppid": 9, "pcomm": "claude", "event": hook_event, "agent": "claude-code", "ws": ws,
         "start": T + 0.55, "t": T + 0.56},
        {"k": "child", "pid": 11, "ppid": 10, "pcomm": "zygo", "sid": 11, "pgid": 11, "ws": ws, "start": T + 0.57,
         "env": ["HOME", "SSH_AUTH_SOCK"], "t": T + 0.58},
        {"k": "busy", "label": label, "value": True, "t": T + 0.6},
        {"k": "hook_gone", "pid": 10, "t": T + 0.62},
    ]
    if reparent:
        e.append({"k": "child_ppid", "pid": 11, "ppid": 1, "pcomm": "systemd", "t": T + 0.62})
    e += [{"k": "busy", "label": label, "value": False, "t": T + busy_false},
          {"k": "child_gone", "pid": 11, "t": T + child_gone}]
    return sorted(e, key=lambda x: x["t"])


def rnd(**kw):
    r = {"id": "R1", "sub": "1.1", "acc": True, "ws": "w1", "member": "cc", "member_label": "w1/cc",
         "remote": "SSH-agent", "T": T, "end": T + 30, "state": {"last_ok": T + 4}, "errors": [], "signs": 2,
         "branch": "zygo/mail", "attempt": 1}
    r.update(kw)
    return r


ok, text, c = w.judge_round(rnd(), ev_round("w1", "w1/cc", 4.0, 1.5))
check("round passes", ok, True)
check("round text: order", "turn ended before the child: yes" in text, True)
check("round text: parent", "parent ended before the child: yes" in text, True)
check("round text: last_ok", "last_ok = T+4" in text, True)
ok, text, _ = w.judge_round(rnd(), ev_round("w1", "w1/cc", 1.0, 1.5))
check("child ends before turn: fail", (ok, "turn ended before the child: no" in text), (False, True))
ok, text, _ = w.judge_round(rnd(state={"last_ok": T + 4, "last_error": "fetch"}), ev_round("w1", "w1/cc", 4.0, 1.5))
check("last_error: fail", (ok, "last_error = fetch" in text), (False, True))
ok, _, _ = w.judge_round(rnd(state={"last_ok": T}), ev_round("w1", "w1/cc", 4.0, 1.5))
check("last_ok not later than T: fail", ok, False)
ok, text, _ = w.judge_round(rnd(), ev_round("w1", "w1/cc", 4.0, 1.5, hook_event="stop"))
check("child of another event: fail", (ok, "started by: stop" in text), (False, True))
nohook = [e for e in ev_round("w1", "w1/cc", 4.0, 1.5) if e["k"] not in ("hook", "hook_gone")]
ok, text, _ = w.judge_round(rnd(), nohook)
check("hook not sampled, reparent seen, child before busy true", (ok, "its parent ended" in text, "prompt (by time" in text),
      (True, True, True))
late = [dict(e, start=T + 0.7) if e["k"] == "child" else e for e in nohook]
ok, text, _ = w.judge_round(rnd(), late)
check("hook not sampled, child after busy true: fail", (ok, "started by: unknown" in text), (False, True))
first_reparented = [dict(e, ppid=1, pcomm="systemd") if e["k"] == "child" else e for e in nohook if e["k"] != "child_ppid"]
ok, text, _ = w.judge_round(rnd(), first_reparented)
check("first seen re-parented", (ok, "its parent had already ended" in text), (True, True))
ok, text, _ = w.judge_round(rnd(), [])
check("no child: fail", (ok, "no sync child" in text), (False, True))
check("order query", w.order_shown(rnd(), ev_round("w1", "w1/cc", 1.0, 1.5)), "no")

# judge: a repeated attempt becomes note 3.6; 1.3 reads the trace of the last attempt
ev = ev_round("w1", "w1/cc", 4.0, 1.5)
trace = ["%s run-command.c:673 trace: run_command: 'ssh -o BatchMode=yes' -p 443 git@ssh.github.com x" %
         datetime.datetime.fromtimestamp(T + 2).strftime("%H:%M:%S.000000")]
out = w.judge(ev, [rnd(attempt=1, T=T - 100, end=T - 70), rnd(attempt=2)], trace)
subs = [(s, wd) for s, wd, _ in out]
check("attempts: note then verdict", subs[:2], [("3.6", "note"), ("1.1", "0")])
check("1.3 from the trace", [(s, wd) for s, wd, _ in out if s == "1.3"], [("1.3", "0")])
check("3.3 env names", any(s == "3.3" and "SSH_AUTH_SOCK" in t for s, _, t in out), True)

if bad == 0:
    print("sp1_watch tests: ok (%d)" % n)
    sys.exit(0)
print("sp1_watch tests: %d of %d failed" % (bad, n))
sys.exit(1)

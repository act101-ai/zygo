#!/usr/bin/env python3
"""Tests of sp1_watch.py (node MS-23): python3 -I scripts/premises/sp1_watch_test.py -> "sp1_watch tests: ok (<n>)".

Pure parts only: argv classes, /proc stat and tcp parsing, environ names, git trace2 events, the judge and the
side-traffic check on authored watcher events; the known_hosts add and remove and the note 3.4 reader (node FX-38)
on files in a temporary folder (never the real ~/.ssh or HOME); the shell steps of sp1_kh.sh and the acc command
run as child processes with HOME in that folder. No agent or git is started, nothing is read from /proc, no network.
"""
import datetime
import errno
import hashlib
import json
import os
import resource
import shutil
import signal
import subprocess
import sys
import tempfile

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

# env_names and sock_state: names only, values never kept
raw = b"HOME=/h\0SSH_AUTH_SOCK=/s\0SECRET=v\0PATH=/p\0GIT_TRACE2_EVENT=/t\0"
check("env names", w.env_names(raw), ["GIT_TRACE2_EVENT", "HOME", "PATH", "SSH_AUTH_SOCK"])
check("sock match", w.sock_state(raw, "/s"), "match")
check("sock other", w.sock_state(raw, "/run/agent"), "other")
check("sock absent", w.sock_state(b"HOME=/h\0", "/s"), "absent")

# git_sub, proc_name, tcp_remote
check("git sub", w.git_sub(["/usr/bin/git", "push", "origin"]), "push")
check("git sub not a word", w.git_sub(["git", "-C", "/x", "push"]), "")
check("proc name", w.proc_name(["/usr/bin/ssh", "-T"], "ssh"), "ssh")
check("tcp v4", w.tcp_remote("0100007F:0016"), ("127.0.0.1", 22, True))
check("tcp v4 remote", w.tcp_remote("7193C08C:01BB"), ("140.192.147.113", 443, False))
check("tcp v6 mapped", w.tcp_remote("0000000000000000FFFF00007193C08C:01BB"), ("140.192.147.113", 443, False))
check("tcp v6 loopback", w.tcp_remote("00000000000000000000000001000000:0050")[2], True)

# trace2
T = 1000.0


def iso(t):
    return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def t2(sid, t, argv, anc, starts=()):
    out = [json.dumps({"event": "start", "sid": sid, "time": iso(t), "argv": argv}),
           json.dumps({"event": "cmd_ancestry", "sid": sid, "time": iso(t), "ancestry": anc})]
    for (dt, cls, sargv) in starts:
        out.append(json.dumps({"event": "child_start", "sid": sid, "time": iso(t + dt), "child_class": cls,
                               "use_shell": cls != "transport/ssh", "argv": sargv}))
    return out


SSH443 = ("transport/ssh", ["ssh -o BatchMode=yes", "-o", "SendEnv=GIT_PROTOCOL", "-p", "443", "git@" "ssh.github.com",
                            "git-upload-pack 'o/r.git'"])
SSH22 = ("transport/ssh", ["ssh -o BatchMode=yes", "-o", "SendEnv=GIT_PROTOCOL", "git@" "github.com", "git-upload-pack 'o/r.git'"])
SSHNB = ("transport/ssh", ["ssh", "-p", "443", "git@" "ssh.github.com", "git-upload-pack 'o/r.git'"])
HELP = ("?", ["/usr/bin/gh auth git-credential get"])
CHILD = ["zygo", "systemd", "init"]


def tr(*starts, t=T + 2, anc=CHILD, sub="fetch", sid="20261127T000000.000000Z-H1-P1"):
    return t2(sid, t, ["git", sub, "origin"], anc, [(0.01, c, a) for c, a in starts])


check("iso epoch", round(w.iso_epoch(iso(T + 1.25)), 3), T + 1.25)
p = w.trace2_procs(tr(SSH443, HELP) + ["not json"])
check("trace2 starts", len(p["20261127T000000.000000Z-H1-P1"]["starts"]), 2)
check("from child", w.from_child(p, "20261127T000000.000000Z-H1-P1"), True)
check("not from child", w.from_child(w.trace2_procs(tr(anc=["zsh", "claude", "zygo"])), "20261127T000000.000000Z-H1-P1"), False)
check("git path 443", w.git_path(w.trace2_procs(tr(SSH443)), T, T + 5),
      {"ssh": 1, "batch": 1, "ssh443": 1, "ssh22": 0, "helper": 0, "gh": 0, "gits": 1})
check("git path 22", w.git_path(w.trace2_procs(tr(SSH22)), T, T + 5)["ssh22"], 1)
check("git path outside window", w.git_path(w.trace2_procs(tr(SSH443)), T + 3, T + 5)["gits"], 0)
check("git path helper", w.git_path(w.trace2_procs(tr(HELP, ("?", ["/usr/bin/gh auth git-credential store"]))), T, T + 5)["helper"], 2)
check("git path other helper counted", w.git_path(w.trace2_procs(tr(("?", ["git credential-cache get"]))), T, T + 5),
      {"ssh": 0, "batch": 0, "ssh443": 0, "ssh22": 0, "helper": 1, "gh": 0, "gits": 1})

# judge: one acceptance round whose hook ends, then the turn, then the child
SUBREAPER = 1373


def ev_round(ws, label, child_gone, busy_false, hook_event="prompt", reparent=True, sock="match"):
    e = [
        {"k": "start", "boot": 0, "subreaper": SUBREAPER, "t": T - 10, "prev": T - 10},
        {"k": "busy", "label": label, "value": False, "t": T - 5, "prev": T - 5.002},
        {"k": "hook", "pid": 10, "ppid": 9, "pcomm": "claude", "event": hook_event, "agent": "claude-code", "ws": ws,
         "start": T + 0.55, "t": T + 0.56, "prev": T + 0.553},
        {"k": "child", "pid": 11, "ppid": 10, "pcomm": "zygo", "sid": 11, "pgid": 11, "ws": ws, "start": T + 0.57,
         "env": ["HOME", "SSH_AUTH_SOCK"], "sock": sock, "t": T + 0.58, "prev": T + 0.573},
        {"k": "busy", "label": label, "value": True, "t": T + 0.6, "prev": T + 0.593},
        {"k": "hook_gone", "pid": 10, "t": T + 0.62, "prev": T + 0.613},
    ]
    if reparent:
        e.append({"k": "child_ppid", "pid": 11, "ppid": SUBREAPER, "pcomm": "systemd", "t": T + 0.62, "prev": T + 0.613})
    e += [{"k": "busy", "label": label, "value": False, "t": T + busy_false, "prev": T + busy_false - 0.007},
          {"k": "child_gone", "pid": 11, "t": T + child_gone, "prev": T + child_gone - 0.007}]
    return sorted(e, key=lambda x: x["t"])


def rnd(**kw):
    r = {"id": "R1", "sub": "1.1", "acc": True, "ws": "w1", "member": "cc", "member_label": "w1/cc",
         "arm": "ssh-agent", "T": int(T), "end": T + 30, "state": {"last_ok": int(T) + 4}, "errors": [], "signs": 2,
         "delivered": True, "branch": "zygo/mail", "attempt": 1}
    r.update(kw)
    return r


AG = w.trace2_procs(tr(SSH443))
ok, text, c = w.judge_round(rnd(), ev_round("w1", "w1/cc", 4.0, 1.5), AG)
check("round passes", ok, True)
check("round text: order", "turn ended before the child: yes" in text, True)
check("round text: parent", "parent ended before the child: yes" in text, True)
check("round text: last_ok", "last_ok = T+4" in text, True)
check("round text: 3 decimals", "+0.570 s" in text, True)
ok, text, _ = w.judge_round(rnd(), ev_round("w1", "w1/cc", 1.0, 1.5), AG)
check("child ends before turn: fail", (ok, "turn ended before the child: no" in text), (False, True))
ok, text, _ = w.judge_round(rnd(state={"last_ok": int(T) + 4, "last_error": "fetch"}), ev_round("w1", "w1/cc", 4.0, 1.5), AG)
check("last_error: fail", (ok, "last_error = fetch" in text), (False, True))
ok, _, _ = w.judge_round(rnd(state={"last_ok": int(T)}), ev_round("w1", "w1/cc", 4.0, 1.5), AG)
check("last_ok not later than T: fail", ok, False)
ok, text, _ = w.judge_round(rnd(), ev_round("w1", "w1/cc", 4.0, 1.5, hook_event="stop"), AG)
check("child of another event: fail", (ok, "started by the prompt hook: no" in text), (False, True))
ok, text, _ = w.judge_round(rnd(delivered=False), ev_round("w1", "w1/cc", 4.0, 1.5), AG)
check("prompt not delivered: fail", (ok, "prompt delivered to the mock: no" in text), (False, True))
ok, text, _ = w.judge_round(rnd(errors=[[int(T) + 2, "sync: push failed"]]), ev_round("w1", "w1/cc", 4.0, 1.5), AG)
check("errors.log entry in the window: fail", (ok, "errors.log lines in the round: 1" in text), (False, True))
ok, _, _ = w.judge_round(rnd(errors=[[int(T) - 50, "old"]]), ev_round("w1", "w1/cc", 4.0, 1.5), AG)
check("errors.log entry before the window: ok", ok, True)

nohook = [e for e in ev_round("w1", "w1/cc", 4.0, 1.5) if e["k"] not in ("hook", "hook_gone")]
ok, text, _ = w.judge_round(rnd(), nohook, AG)
check("hook not sampled, reparent seen, child before busy true", (ok, "its parent ended" in text, "busy true" in text),
      (True, True, True))
late = [dict(e, start=T + 0.7) if e["k"] == "child" else e for e in nohook]
ok, text, _ = w.judge_round(rnd(), late, AG)
check("hook not sampled, child after busy true: fail", (ok, "started by the prompt hook: no" in text), (False, True))
first_reparented = [dict(e, ppid=SUBREAPER, pcomm="systemd") if e["k"] == "child" else e for e in nohook if e["k"] != "child_ppid"]
ok, text, _ = w.judge_round(rnd(), first_reparented, AG)
check("first seen re-parented to the subreaper", (ok, "re-parented" in text), (True, True))
ok, text, _ = w.judge_round(rnd(), [])
check("no child: fail", (ok, "no sync child" in text), (False, True))
check("order query", w.order_shown(rnd(), ev_round("w1", "w1/cc", 1.0, 1.5)), "no")

# the reviewer's p1 cases (finding 6) and the negatives: each must fail
base = [{"k": "busy", "label": "w1/cc", "value": False, "t": T - 5}, {"k": "busy", "label": "w1/cc", "value": True, "t": T + 0.6},
        {"k": "busy", "label": "w1/cc", "value": False, "t": T + 0.7}, {"k": "child_gone", "pid": 11, "t": T + 4}]


def ch(**kw):
    c = {"k": "child", "pid": 11, "ppid": 1, "pcomm": "systemd", "sid": 11, "pgid": 11, "ws": "w1", "start": T + 0.55,
         "env": [], "sock": "match", "t": T + 0.56}
    c.update(kw)
    return sorted(base + [c], key=lambda e: e["t"])


check("p1 control: reparented to pid 1 passes", w.judge_round(rnd(), ch(), AG)[0], True)
for name, kw in [("p1 A shell parent alive", {"ppid": 500, "pcomm": "bash"}),
                 ("p1 B agent parent alive", {"ppid": 400, "pcomm": "claude"}),
                 ("p1 C start equals busy true", {"start": T + 0.60}),
                 ("p1 E sid not pid", {"sid": 5}),
                 ("p1 I zygo parent alive", {"pcomm": "zygo", "ppid": 77})]:
    check(name + ": fail", w.judge_round(rnd(), ch(**kw), AG)[0], False)
forked = ch() + [{"k": "hook", "pid": 11, "ppid": 10, "pcomm": "zygo", "event": "stop", "agent": "claude-code", "ws": "w1",
                  "start": T + 0.55, "t": T + 0.551}, {"k": "hook_exec", "pid": 11, "t": T + 0.555}]
ok, text, _ = w.judge_round(rnd(), sorted(forked, key=lambda e: e["t"]), AG)
check("a hook's fork seen before its exec is not a hook", (ok, "stop" in text), (True, False))
withsub = [{"k": "start", "boot": 0, "subreaper": SUBREAPER, "t": T - 10}] + ch(ppid=900, pcomm="systemd")
check("systemd parent that is not the subreaper: fail", w.judge_round(rnd(), withsub, AG)[0], False)
other_first = ch() + [{"k": "hook", "pid": 20, "ppid": 9, "pcomm": "claude", "event": "tool-end", "agent": "claude-code",
                       "ws": "w1", "start": T + 0.40, "t": T + 0.41}]
ok, text, _ = w.judge_round(rnd(), sorted(other_first, key=lambda e: e["t"]), AG)
check("other hook event before the child: fail", (ok, "tool-end" in text), (False, True))
prompt_first = ch() + [{"k": "hook", "pid": 20, "ppid": 9, "pcomm": "claude", "event": "prompt", "agent": "claude-code",
                        "ws": "w1", "start": T + 0.50, "t": T + 0.51}]
ok, text, _ = w.judge_round(rnd(), sorted(prompt_first, key=lambda e: e["t"]), AG)
check("prompt hook sampled before the child, not parent: time attribution", (ok, "child start" in text and "0.050 s" in text),
      (True, True))

# 1.3 per arm (findings 2, 11)
ev = ev_round("w1", "w1/cc", 4.0, 1.5)
evk = ev_round("w1", "w1/cc", 4.0, 1.5, sock="absent")
check("agent arm: ssh without the agent sign: fail", w.judge_round(rnd(signs=0), ev, AG)[0], False)
check("agent arm: socket not from the run agent: fail", w.judge_round(rnd(), ev_round("w1", "w1/cc", 4.0, 1.5, sock="other"), AG)[0], False)
check("agent arm: ssh without BatchMode: fail", w.judge_round(rnd(), ev, w.trace2_procs(tr(SSHNB)))[0], False)
check("agent arm: helper call on SSH: fail", w.judge_round(rnd(), ev, w.trace2_procs(tr(SSH443, HELP)))[0], False)
check("agent arm: no git call: fail", w.judge_round(rnd(), ev, {})[0], False)
check("key arm passes", w.judge_round(rnd(arm="ssh-key", signs=0), evk, w.trace2_procs(tr(SSH22)))[0], True)
check("key arm: agent signs: fail", w.judge_round(rnd(arm="ssh-key", signs=1), evk, w.trace2_procs(tr(SSH22)))[0], False)
check("key arm: socket set: fail", w.judge_round(rnd(arm="ssh-key", signs=0), ev, w.trace2_procs(tr(SSH22)))[0], False)
check("key arm: to port 443: fail", w.judge_round(rnd(arm="ssh-key", signs=0), evk, w.trace2_procs(tr(SSH443)))[0], False)
check("https arm passes", w.judge_round(rnd(arm="https", signs=0), evk, w.trace2_procs(tr(HELP)))[0], True)
check("https arm: no helper call: fail", w.judge_round(rnd(arm="https", signs=0), evk, w.trace2_procs(tr()))[0], False)
check("https arm: ssh call: fail", w.judge_round(rnd(arm="https", signs=0), evk, w.trace2_procs(tr(HELP, SSH443)))[0], False)
check("https arm: other helper: fail", w.judge_round(rnd(arm="https", signs=0), evk,
                                                    w.trace2_procs(tr(("?", ["git credential-store get"]))))[0], False)
check("git of the agent, not the child, does not count",
      w.judge_round(rnd(), ev, w.trace2_procs(tr(SSH443, anc=["zsh", "claude"])))[0], False)

# judge: a repeated attempt becomes note 3.6; the watcher stats become note 3.7
stats = {"max": 0.02, "p99": 0.009, "mean": 0.007}
ev = ev_round("w1", "w1/cc", 4.0, 1.5) + [{"k": "stop", "samples": 900, "full": stats, "polls": 9000, "poll": stats, "agents": 6,
                                           "max_start_skew": 0.0, "t": T + 40, "prev": T + 39.99}]
lines = tr(SSH443)
out = w.judge(ev, [rnd(attempt=1, T=int(T) - 100, end=T - 70), rnd(attempt=2)], lines)
subs = [(s, wd) for s, wd, _ in out]
check("attempts: note then verdict", subs[:2], [("3.6", "note"), ("1.1", "0")])
check("3.3 env names", any(s == "3.3" and "SSH_AUTH_SOCK" in t for s, _, t in out), True)
check("3.7 watcher stats", any(s == "3.7" and "900 full scans" in t and "9000 fast polls" in t for s, _, t in out), True)

# side traffic (finding 1): judged in the rounds' window [LO, HI]
LO, HI = T, T + 30
check("git word after options", w.git_word(["git", "--no-optional-locks", "-c", "a=b", "-C", "/x", "fetch", "--quiet"]), "fetch")
check("git word none", w.git_word(["git", "--version"]), "?")
child_ev = [{"k": "proc", "pid": 30, "ppid": 11, "sid": 11, "name": "ssh", "sub": "", "chain": ["git", "zygo"], "in_child": True, "t": T + 2},
            {"k": "conn", "remote": "140.82.112.35:443", "pid": 30, "name": "ssh", "chain": ["git", "zygo"], "in_child": True, "t": T + 2}]
clean = {"known_marketplaces": False, "git_dirs": 0}
ok, text = w.side_traffic(child_ev, lines + tr(sub="rev-parse", anc=["zsh", "claude"], sid="S2"), clean, LO, HI)
check("side: child traffic only passes", (ok, "zsh:rev-parse x1" in text), (True, True))
ok, text = w.side_traffic(child_ev, lines + tr(sub="clone", anc=["claude", "tmux"], sid="S3", t=T - 100), clean, LO, HI)
check("side: marketplace clone before the window: fail", (ok, "claude:clone x1" in text), (False, True))
ok, _ = w.side_traffic(child_ev, lines + tr(SSH22, sub="ls-files", anc=["node"], sid="S4"), clean, LO, HI)
check("side: git with an ssh transport outside the child: fail", ok, False)
ok, text = w.side_traffic(child_ev, lines + tr(("remote-https", ["git", "remote-https", "origin"]), sub="fetch",
                                               anc=["zygo", "tmux: server"], sid="S5", t=T - 50), clean, LO, HI)
check("side: zygo fetch before the window: listed, passes", (ok, "zygo:fetch x1" in text), (True, True))
ok, _ = w.side_traffic(child_ev, lines + tr(("remote-https", ["git", "remote-https", "origin"]), sub="fetch",
                                            anc=["zygo", "tmux: server"], sid="S6"), clean, LO, HI)
check("side: zygo fetch in the window: fail", ok, False)
ok, text = w.side_traffic(child_ev + [{"k": "proc", "pid": 40, "ppid": 9, "sid": 9, "name": "gh", "sub": "", "chain": ["claude"],
                                       "in_child": False, "t": T + 3}], lines, clean, LO, HI)
check("side: gh outside the child: fail", (ok, "gh(claude)" in text), (False, True))
npm = {"k": "conn", "remote": "2606:4700:0:0:0:0:6810:722:443", "pid": 50, "name": "opencode", "chain": ["zygo"], "in_child": False}
check("side: connection outside the child in the window: fail", w.side_traffic(child_ev + [dict(npm, t=T + 5)], lines, clean, LO, HI)[0], False)
ok, text = w.side_traffic(child_ev + [dict(npm, t=T - 5)], lines, clean, LO, HI)
check("side: connection before the window: listed, passes", (ok, "opencode 2606:4700:0:0:0:0:6810:722:443 x1" in text), (True, True))
check("side: known_marketplaces present: fail", w.side_traffic([], [], {"known_marketplaces": True, "git_dirs": 0}, LO, HI)[0], False)
check("side: .git under config: fail", w.side_traffic([], [], {"known_marketplaces": False, "git_dirs": 1}, LO, HI)[0], False)

# known_hosts add and remove (node FX-38): temporary folders only, never the real ~/.ssh or HOME; the shell steps
# (sp1_kh.sh) run under sh with HOME set to a temporary folder
RUNL = b"[ssh.github.com]:443 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl"
L1 = b"github.com ssh-ed25519 AAAAone"
L2 = b"gitlab.com ssh-ed25519 AAAAtwo"
HERE = os.path.dirname(os.path.abspath(__file__))
TMP = tempfile.mkdtemp(prefix="sp1-kh-")
TMP = os.path.realpath(TMP)


def kh_case(name, target_bytes):
    """<TMP>/<name>: the target file, ssh/known_hosts (a relative link to it), the run's line file kh443, and the
    paths of the saved copy and the run-state file (both absent)."""
    d = os.path.join(TMP, name)
    os.makedirs(os.path.join(d, "ssh"))
    tgt = os.path.join(d, "target")
    with open(tgt, "wb") as f:
        f.write(target_bytes)
    os.chmod(tgt, 0o644)
    link = os.path.join(d, "ssh", "known_hosts")
    os.symlink("../target", link)
    lf = os.path.join(d, "kh443")
    with open(lf, "wb") as f:
        f.write(RUNL + b"\n")
    return d, tgt, link, lf, os.path.join(d, "known_hosts.start"), os.path.join(d, "kh.state")


def rb(p):
    with open(p, "rb") as f:
        return f.read()


def fsize_limited(n, fn):
    """fn() with RLIMIT_FSIZE n (a write past n bytes gives EFBIG; SIGXFSZ ignored), both restored after."""
    lim = resource.getrlimit(resource.RLIMIT_FSIZE)
    old = signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
    resource.setrlimit(resource.RLIMIT_FSIZE, (n, lim[1]))
    try:
        return fn()
    finally:
        resource.setrlimit(resource.RLIMIT_FSIZE, lim)
        signal.signal(signal.SIGXFSZ, old)


def sh(script, *args, premises=HERE):
    """Run <script> under sh with HOME and TMPDIR in TMP and PREMISES_DIR=<premises>; (exit, stdout)."""
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": os.path.join(TMP, "home"), "TMPDIR": TMP,
           "PREMISES_DIR": premises, "LANG": "C.UTF-8"}
    os.makedirs(env["HOME"], exist_ok=True)
    p = subprocess.run(["sh", "-c", script, "sh"] + list(args), env=env, stdout=subprocess.PIPE,
                       stderr=subprocess.STDOUT, timeout=60)
    return p.returncode, p.stdout.decode("utf-8", "replace")


try:
    # the run's line between two other lines: the file was rewritten during the run (its prefix is not the saved
    # copy), so the last exact copy of the line is removed; the target is byte-equal to its content before the add
    before = L1 + b"\n" + L2 + b"\n"
    d, tgt, link, lf, save, state = kh_case("between", before)
    check("kh_add before the rewrite", w.kh_add(link, lf, save, state), "added")
    with open(tgt, "wb") as f:
        f.write(L1 + b"\n" + RUNL + b"\n" + L2 + b"\n")
    res = w.kh_remove(state, lf)
    check("kh_remove_through_a_symlinked_known_hosts",
          (res, os.path.islink(link), os.readlink(link), rb(tgt), os.stat(tgt).st_mode & 0o7777, w.kh_compare(save, tgt)),
          ("removed", True, "../target", before, 0o644, (0, 0)))

    # add, then remove, through the link: the target is byte-equal to its content before the add; the run state
    # names the resolved target and the saved copy's size and hash
    d, tgt, link, lf, save, state = kh_case("roundtrip", before)
    a = w.kh_add(link, lf, save, state)
    mid = rb(tgt)
    with open(state) as f:
        st = json.load(f)
    r = w.kh_remove(state, lf)
    check("kh_add_then_remove_through_a_link_is_byte_equal",
          (a, mid, rb(save), st["target"], st["size"], st["sha256"] == hashlib.sha256(before).hexdigest(), r,
           os.path.islink(link), rb(tgt), w.kh_compare(save, w.kh_target(state))),
          ("added", before + RUNL + b"\n", before, tgt, len(before), True, "removed", True, before, (0, 0)))

    # another process adds a line after the run's line: the remove keeps it, the comparison names it
    d, tgt, link, lf, save, state = kh_case("other", L1 + b"\n")
    a = w.kh_add(link, lf, save, state)
    with open(link, "ab") as f:
        f.write(L2 + b"\n")
    r = w.kh_remove(state, lf)
    check("kh_remove_keeps_other_lines_added_during_the_run",
          (a, r, rb(tgt), os.path.islink(link), w.kh_compare(save, tgt)),
          ("added", "removed", L1 + b"\n" + L2 + b"\n", True, (0, 1)))

    # an append to a read-only target fails: not added; the remove then changes nothing (skipped as root: root
    # can write a 0444 file)
    if os.geteuid() == 0:
        print("skip: kh_add_failed_append_is_not_recorded_as_added (euid 0 can write a read-only file)")
    else:
        d, tgt, link, lf, save, state = kh_case("readonly", L1 + b"\n")
        os.chmod(tgt, 0o444)
        a = w.kh_add(link, lf, save, state)
        r = w.kh_remove(state, lf)
        check("kh_add_failed_append_is_not_recorded_as_added",
              (a.startswith("failed: "), r, rb(tgt), os.path.islink(link), os.stat(tgt).st_mode & 0o7777),
              (True, "the added line is not in the file", L1 + b"\n", True, 0o444))
        os.chmod(tgt, 0o644)

    # an append whose write fails at the flush before any byte (RLIMIT_FSIZE at the target's size): not added
    d, tgt, link, lf, save, state = kh_case("fsize", L1 + b"\n")
    a = fsize_limited(len(L1) + 1, lambda: w.kh_add(link, lf, save, state))
    check("kh_add_flush_failure_is_not_added", (a.startswith("failed: the append failed (EFBIG)"), rb(tgt)), (True, L1 + b"\n"))

    # a partial append (EFBIG after 10 bytes of the line): not added; the remove cuts the fragment
    d, tgt, link, lf, save, state = kh_case("partial", L1 + b"\n")
    a = fsize_limited(len(L1) + 1 + 10, lambda: w.kh_add(link, lf, save, state))
    part = rb(tgt)
    r = w.kh_remove(state, lf)
    check("kh_add_partial_append_is_restored",
          (a.startswith("failed: the append failed (EFBIG)"), part, r, rb(tgt), os.path.islink(link)),
          (True, L1 + b"\n" + RUNL[:10], "removed a partial append (10 bytes)", L1 + b"\n", True))

    # the fsync fails after the line was written: not added; the remove takes the whole line out
    def eio(fd):
        raise OSError(errno.EIO, "injected")
    d, tgt, link, lf, save, state = kh_case("fsync", L1 + b"\n")
    a = w.kh_add(link, lf, save, state, fsync=eio)
    r = w.kh_remove(state, lf)
    check("kh_add_fsync_failure_is_restored", (a.startswith("failed: the append failed (EIO)"), r, rb(tgt)),
          (True, "removed", L1 + b"\n"))

    # the link is pointed at another file between the add and the remove: the remove works on the recorded target
    d, tgt, link, lf, save, state = kh_case("retarget", L1 + b"\n")
    a = w.kh_add(link, lf, save, state)
    other = os.path.join(d, "other")
    with open(other, "wb") as f:
        f.write(L2 + b"\n" + RUNL + b"\n")
    os.unlink(link)
    os.symlink("../other", link)
    r = w.kh_remove(state, lf)
    check("kh_remove_uses_the_recorded_target_after_a_retarget",
          (a, r, rb(tgt), rb(other), os.readlink(link)),
          ("added", "removed", L1 + b"\n", L2 + b"\n" + RUNL + b"\n", "../other"))

    # the rewrite is a rename onto the resolved target: mode kept, link kept, no temporary file left
    d, tgt, link, lf, save, state = kh_case("mode", L1 + b"\n")
    os.chmod(tgt, 0o600)
    w.kh_add(link, lf, save, state)
    r = w.kh_remove(state, lf)
    check("kh_remove_renames_onto_the_target",
          (r, os.stat(tgt).st_mode & 0o7777, os.path.islink(link), sorted(os.listdir(d))),
          ("removed", 0o600, True, ["kh.state", "kh443", "known_hosts.start", "ssh", "target"]))

    # the target changes while the new text is written: read again and redo once; a second change: failure, no write
    d, tgt, link, lf, save, state = kh_case("race1", L1 + b"\n")
    w.kh_add(link, lf, save, state)
    calls = []

    def once():
        calls.append(1)
        if len(calls) == 1:
            with open(tgt, "ab") as f:
                f.write(L2 + b"\n")
    r = w.kh_remove(state, lf, before_rename=once)
    check("kh_remove_redoes_once_after_a_change", (r, rb(tgt), len(calls)), ("removed", L1 + b"\n" + L2 + b"\n", 2))
    d, tgt, link, lf, save, state = kh_case("race2", L1 + b"\n")
    w.kh_add(link, lf, save, state)

    def always():
        with open(tgt, "ab") as f:
            f.write(b"x\n")
    r = w.kh_remove(state, lf, before_rename=always)
    check("kh_remove_fails_after_a_second_change",
          (r, rb(tgt), sorted(os.listdir(d))),
          ("failed: known_hosts changed while it was rewritten (twice)", L1 + b"\n" + RUNL + b"\nx\nx\n",
           ["kh.state", "kh443", "known_hosts.start", "ssh", "target"]))

    # the line is there before the run: present, the file is not changed, no run state
    d, tgt, link, lf, save, state = kh_case("present", L1 + b"\n" + RUNL + b"\n")
    check("kh_add_present_line", (w.kh_add(link, lf, save, state), rb(tgt), os.path.exists(state)),
          ("present", L1 + b"\n" + RUNL + b"\n", False))

    # a file without a line end at its end is not changed
    d, tgt, link, lf, save, state = kh_case("noeol", L1)
    a = w.kh_add(link, lf, save, state)
    check("kh_add_no_line_end", (a.startswith("failed: "), rb(tgt), os.path.exists(state)), (True, L1, False))

    # no known_hosts file: not created
    d, tgt, link, lf, save, state = kh_case("missing", b"")
    os.unlink(tgt)
    a = w.kh_add(link, lf, save, state)
    check("kh_add_missing_file", (a.startswith("failed: "), os.path.exists(tgt), os.path.exists(state)), (True, False, False))

    # no run state: the remove says so and changes nothing
    check("kh_remove_without_a_run_state", w.kh_remove(os.path.join(TMP, "none.state"), lf).startswith("failed: "), True)

    # shell: sp1_kh_add and sp1_kh_restore (sp1_kh.sh, sourced by live-sp1.sh); the teardown's tab parse
    d, tgt, link, lf, save, state = kh_case("shell", L1 + b"\n")
    rc, out = sh('. "$PREMISES_DIR/sp1_kh.sh"; sp1_kh_add "$1" "$2"; printf "%s\\n" "$KHR"; '
                 'printf "x\\n" >>"$1"; sp1_kh_restore "$2"; printf "%s|%s|%s|%s\\n" "$?" "$KH_R" "$KH_MISS" "$KH_MORE"',
                 link, d)
    check("sh: sp1_kh_add then sp1_kh_restore (tab parse)", (rc, out, rb(tgt)),
          (0, "added\n0|removed|0|1\n", L1 + b"\nx\n"))
    fake = os.path.join(TMP, "fake-premises")
    os.makedirs(fake)
    shutil.copy(os.path.join(HERE, "sp1_kh.sh"), fake)
    with open(os.path.join(fake, "sp1_watch.py"), "w") as f:
        f.write("import sys\nsys.exit(1)\n")
    d, tgt, link, lf, save, state = kh_case("shell-q", L1 + b"\n")
    with open(state, "w") as f:
        f.write("{}")
    rc, out = sh('. "$PREMISES_DIR/sp1_kh.sh"; sp1_kh_restore "$1"; printf "%s|%s|%s|%s\\n" "$?" "$KH_R" "$KH_MISS" "$KH_MORE"',
                 d, premises=fake)
    check("sh: sp1_kh_restore with no kh-remove output: the ? fallback", (rc, out),
          (1, "1|not checked (kh-remove gave no result)|?|?\n"))
    d, tgt, link, lf, save, state = kh_case("shell-none", L1 + b"\n")
    rc, out = sh('. "$PREMISES_DIR/sp1_kh.sh"; sp1_kh_restore "$1"; printf "%s|%s\\n" "$?" "$KH_R"', d)
    check("sh: sp1_kh_restore without a run state does nothing", (rc, out, rb(tgt)), (0, "0|\n", L1 + b"\n"))

    # the signal window: TERM arrives while the command substitution of kh-add runs (the shell runs the trap after
    # it, before the result is assigned); the trap's restore still takes the line out
    d, tgt, link, lf, save, state = kh_case("signal", L1 + b"\n")
    rc, out = sh('. "$PREMISES_DIR/sp1_kh.sh"; '
                 'trap \'sp1_kh_restore "$2"; printf "%s|%s|%s\\n" "$KH_R" "$KH_MISS" "${KHR:-unset}"; exit 143\' TERM; '
                 'KHR=$(python3 -I "$PREMISES_DIR/sp1_watch.py" kh-add --file "$1" --line "$2/kh443" '
                 '--save "$2/known_hosts.start" --state "$2/kh.state"; kill -TERM $$); echo "not reached: $KHR"',
                 link, d)
    check("sh: a signal between kh-add and the shell's record: restored", (rc, out, rb(tgt), os.path.islink(link)),
          (143, "removed|0|unset\n", L1 + b"\n", True))

    # note 3.4: only this run's workspaces are read
    st = os.path.join(TMP, "state", "workspaces")
    mine, other = "bc10a6a9" + "0" * 18, "e77e1d00" + "1" * 18

    def put(wid, raw):
        os.makedirs(os.path.join(st, wid, "mail", "sync"), exist_ok=True)
        with open(os.path.join(st, wid, "mail", "sync", "state.json"), "wb") as f:
            f.write(raw)
    put(mine, json.dumps({"last_ok": 1795846625, "public": {"verdict": "private"}, "pushed": {"ack/sp1ssh.json": "x"}}).encode())
    put(other, json.dumps({"last_ok": 1700000000, "last_error": "OTHER-ERROR-TEXT", "public": {"verdict": "OTHER-VERDICT"},
                           "pushed": {"registry/OTHER-HOST.json": "y"}}).encode())
    acc = w.acceptance_text(st, [mine])
    note = w.acc_note(st, [mine])
    check("acceptance_note_reads_only_this_runs_workspaces",
          (any(x in acc + note for x in ("OTHER-ERROR-TEXT", "OTHER-VERDICT", "OTHER-HOST", "1700000000", other)),
           "not this run" in note,
           note),
          (False, False, "bc10a6a9… (this run): last_ok 1795846625, last_error absent, public private, pushed ack/sp1ssh.json"))
    check("acceptance note: an id with no state.json", w.acc_note(st, ["f" * 26]), "ffffffff… (this run): no state.json")
    check("acceptance note: an invalid id", (w.acc_note(st, ["../" + other]), w.acceptance_text(st, ["../" + other])),
          ("invalid id (this run)", ""))
    # malformed state files: "unreadable" for that id only, the others still read
    bad_ids = {"a1" * 13: b"{not json", "a2" * 13: b"\xff\xfe{}", "a3" * 13: b'{"public": "x"}', "a4" * 13: b'{"pushed": ["a"]}',
               "a5" * 13: b"[1, 2]",
               "a6" * 13: b'{\n "last_error": "x",\n "note": "/h/u/workspaces/' + other.encode() + b'/mail/sync/state.json"\n}'}
    for k, v in bad_ids.items():
        put(k, v)
    os.makedirs(os.path.join(st, "a7" * 13, "mail", "sync", "state.json"))
    ids = sorted(bad_ids) + ["a7" * 13, mine]
    note = w.acc_note(st, ids)
    want = "; ".join(["a1a1a1a1… (this run): unreadable", "a2a2a2a2… (this run): unreadable",
                      "a3a3a3a3… (this run): unreadable", "a4a4a4a4… (this run): unreadable",
                      "a5a5a5a5… (this run): unreadable",
                      "a6a6a6a6… (this run): last_ok None, last_error x, public None, pushed none",
                      "a7a7a7a7… (this run): unreadable",
                      "bc10a6a9… (this run): last_ok 1795846625, last_error absent, public private, pushed ack/sp1ssh.json"])
    check("acceptance note: a malformed state.json is unreadable for that id only", note, want)
    # the acc command: ids as arguments, the acceptance file, the note on stdout
    out_f = os.path.join(TMP, "acceptance.txt")
    p = subprocess.run([sys.executable, "-I", os.path.join(HERE, "sp1_watch.py"), "acc", "--state", st, "--out", out_f,
                        mine, "f" * 26], stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60)
    acc_file = rb(out_f).decode()
    check("acc command: this run's ids only",
          (p.returncode, p.stdout.decode(), p.stderr, other in acc_file, mine in acc_file, "OTHER" in acc_file),
          (0, "bc10a6a9… (this run): last_ok 1795846625, last_error absent, public private, pushed ack/sp1ssh.json; "
              "ffffffff… (this run): no state.json\n", b"", False, True, False))
finally:
    for root, dirs, files in os.walk(TMP):
        for x in dirs + files:
            p = os.path.join(root, x)
            if not os.path.islink(p):
                os.chmod(p, 0o700)
    shutil.rmtree(TMP)

if bad == 0:
    print("sp1_watch tests: ok (%d)" % n)
    sys.exit(0)
print("sp1_watch tests: %d of %d failed" % (bad, n))
sys.exit(1)

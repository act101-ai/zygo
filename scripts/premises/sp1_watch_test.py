#!/usr/bin/env python3
"""Tests of sp1_watch.py (node MS-23): python3 -I scripts/premises/sp1_watch_test.py -> "sp1_watch tests: ok (<n>)".

Pure parts only: argv classes, /proc stat and tcp parsing, environ names, git trace2 events, the judge and the
side-traffic check on authored watcher events; the known_hosts add and remove and the note 3.4 reader (node FX-38)
on files in a temporary folder (never the real ~/.ssh or HOME). No process is started, nothing is read from /proc,
no network.
"""
import datetime
import json
import os
import shutil
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

# known_hosts add and remove (node FX-38): temporary folders only, never the real ~/.ssh or HOME
RUNL = b"[ssh.github.com]:443 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl"
L1 = b"github.com ssh-ed25519 AAAAone"
L2 = b"gitlab.com ssh-ed25519 AAAAtwo"
TMP = tempfile.mkdtemp(prefix="sp1-kh-")


def kh_case(name, target_bytes):
    """A folder <TMP>/<name> with the target file and ssh/known_hosts, a relative link to it; the run's line file."""
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
    return d, tgt, link, lf


def rb(p):
    with open(p, "rb") as f:
        return f.read()


try:
    # the run's line between two other lines; before the add the target was L1, L2
    d, tgt, link, lf = kh_case("between", L1 + b"\n" + RUNL + b"\n" + L2 + b"\n")
    ino = os.stat(tgt).st_ino
    res = w.kh_remove(link, lf)
    check("kh_remove_through_a_symlinked_known_hosts",
          (res, os.path.islink(link), os.readlink(link), rb(tgt), os.stat(tgt).st_ino, os.stat(tgt).st_mode & 0o7777),
          ("removed", True, "../target", L1 + b"\n" + L2 + b"\n", ino, 0o644))

    # add, then remove, through the link: the target is byte-equal to its content before the add
    before = L1 + b"\n" + L2 + b"\n"
    d, tgt, link, lf = kh_case("roundtrip", before)
    save = os.path.join(d, "start")
    a = w.kh_add(link, lf, save)
    mid = rb(tgt)
    r = w.kh_remove(link, lf)
    check("kh_add_then_remove_through_a_link_is_byte_equal",
          (a, mid, rb(save), r, os.path.islink(link), rb(tgt), w.kh_compare(save, link)),
          ("added", before + RUNL + b"\n", before, "removed", True, before, (0, 0)))

    # another process adds a line after the run's line: the remove keeps it, the comparison names it
    d, tgt, link, lf = kh_case("other", L1 + b"\n")
    save = os.path.join(d, "start")
    a = w.kh_add(link, lf, save)
    with open(link, "ab") as f:
        f.write(L2 + b"\n")
    r = w.kh_remove(link, lf)
    check("kh_remove_keeps_other_lines_added_during_the_run",
          (a, r, rb(tgt), os.path.islink(link), w.kh_compare(save, link)),
          ("added", "removed", L1 + b"\n" + L2 + b"\n", True, (0, 1)))

    # an append to a read-only target fails: not added; the remove then changes nothing
    d, tgt, link, lf = kh_case("readonly", L1 + b"\n")
    os.chmod(tgt, 0o444)
    a = w.kh_add(link, lf, os.path.join(d, "start"))
    r = w.kh_remove(link, lf)
    check("kh_add_failed_append_is_not_recorded_as_added",
          (a.startswith("failed: "), r == "removed", rb(tgt), os.path.islink(link)),
          (True, False, L1 + b"\n", True))
    os.chmod(tgt, 0o644)

    # an append whose flush fails (a link to /dev/full: ENOSPC at the flush): not added
    d = os.path.join(TMP, "full")
    os.makedirs(d)
    os.symlink("/dev/full", os.path.join(d, "known_hosts"))
    with open(os.path.join(d, "kh443"), "wb") as f:
        f.write(RUNL + b"\n")
    a = w.kh_add(os.path.join(d, "known_hosts"), os.path.join(d, "kh443"), os.path.join(d, "start"))
    check("kh_add_flush_failure_is_not_added", a.startswith("failed: "), True)

    # the line is there before the run: present, the file is not changed
    d, tgt, link, lf = kh_case("present", L1 + b"\n" + RUNL + b"\n")
    check("kh_add_present_line", (w.kh_add(link, lf, os.path.join(d, "start")), rb(tgt)), ("present", L1 + b"\n" + RUNL + b"\n"))

    # a file without a line end at its end is not changed
    d, tgt, link, lf = kh_case("noeol", L1)
    a = w.kh_add(link, lf, os.path.join(d, "start"))
    check("kh_add_no_line_end", (a.startswith("failed: "), rb(tgt)), (True, L1))

    # no known_hosts file: not created
    d = os.path.join(TMP, "missing")
    os.makedirs(d)
    with open(os.path.join(d, "kh443"), "wb") as f:
        f.write(RUNL + b"\n")
    a = w.kh_add(os.path.join(d, "known_hosts"), os.path.join(d, "kh443"), os.path.join(d, "start"))
    check("kh_add_missing_file", (a.startswith("failed: "), os.path.exists(os.path.join(d, "known_hosts"))), (True, False))

    # note 3.4: only this run's workspaces are read
    st = os.path.join(TMP, "state", "workspaces")
    mine, other = "bc10a6a9" + "0" * 18, "e77e1d00" + "1" * 18
    for wid, s in ((mine, {"last_ok": 1795846625, "public": {"verdict": "private"}, "pushed": {"ack/sp1ssh.json": "x"}}),
                   (other, {"last_ok": 1700000000, "last_error": "OTHER-ERROR-TEXT",
                            "public": {"verdict": "OTHER-VERDICT"}, "pushed": {"registry/OTHER-HOST.json": "y"}})):
        os.makedirs(os.path.join(st, wid, "mail", "sync"))
        with open(os.path.join(st, wid, "mail", "sync", "state.json"), "w") as f:
            json.dump(s, f)
    acc = w.acceptance_text(st, [mine])
    note = w.acc_note(acc, [mine])
    check("acceptance_note_reads_only_this_runs_workspaces",
          (any(x in acc + note for x in ("OTHER-ERROR-TEXT", "OTHER-VERDICT", "OTHER-HOST", "1700000000", other)),
           "not this run" in note,
           note),
          (False, False, "bc10a6a9… (this run): last_ok 1795846625, last_error absent, public private, pushed ack/sp1ssh.json"))
    check("acceptance note: an id with no state.json", w.acc_note(w.acceptance_text(st, ["f" * 26]), ["f" * 26]),
          "ffffffff… (this run): no state.json")
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

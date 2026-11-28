#!/usr/bin/env python3
"""Tests of scripts/premises/pool_analyze.py (nodes WD-21, WD-22). Offline: no agent, no network, no process.
Usage: python3 -I scripts/premises/pool_analyze_test.py   ->  "pool_analyze tests: ok (<n>)", exit 0

Each case builds a run folder that passes every sub-check, changes one thing, and checks which sub-checks fail."""
import importlib.util
import json
import os
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("pool_analyze", os.path.join(HERE, "pool_analyze.py"))
pa = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pa)
n = bad = 0
M = ["ana", "ben", "ctl"]
KEY = {"ana": "kana00001", "ben": "kben00001", "ctl": "kctl00001"}
ZS = {"ana": "ZSANA", "ben": "ZSBEN", "ctl": "ZSCTL"}
HASH = {"ana": "aaaaaaaaaaaa", "ben": "bbbbbbbbbbbb", "ctl": "cccccccccccc"}
STORED = {"ana": 20000, "ben": 32000, "ctl": 44000}   # view 6000 each: 3500, 6500, 9500 tokens -> 3k, 6k, 9k
TRIM = {"ana": "3k", "ben": "6k", "ctl": "9k"}
GUARD = "`[REDACT" "ED:<id>]` marks a secret zygo removed; never write it into a file."
LOGO = "\U0001d607\U0001d606\U0001d5f4\U0001d5fc"   # the HUD's mathematical "zygo"
T0 = 1000.0


def check(name, cond, why=""):
    global n, bad
    n += 1
    if cond:
        print("ok:", name)
    else:
        bad += 1
        print("FAIL: %s (%s)" % (name, why))


def jw(p, o):
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w") as f:
        json.dump(o, f)


def tw(p, t):
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w") as f:
        f.write(t)


def hud_row(m, trim):
    return "  ▌▌▌ %s %s  master  ctx -/-  ↑0 ↓0  ≈$0.00  ✂ %s  \U000f0150 0m" % (LOGO, m, trim)


def pane(m, trim=None, extra=None):
    rows = ["❯ tracer-pr-%s go" % m, "", "─" * 40, "❯ ", "─" * 40]
    rows.append(hud_row(m, trim or TRIM[m]))
    rows.append("  ⏵⏵ auto mode on (shift+tab to cycle)")
    rows += extra or []
    return "\n".join(rows + ["", ""]) + "\n"


def build(root):
    """A run folder in which every sub-check passes. Returns (run, home, workspace.json)."""
    run = os.path.join(root, "run")
    home = os.path.join(run, "home")
    for d in ("pool/a", "pool/b", ".claude"):
        os.makedirs(os.path.join(home, d))
    os.makedirs(os.path.join(run, "repo"))
    tw(os.path.join(run, "members.txt"), "".join("%s %s %%%d %d %s\n" % (m, KEY[m], i, 100 + i, ZS[m]) for i, m in enumerate(M)))
    wj = os.path.join(run, "workspace.json")
    cd = {"ana": os.path.join(home, "pool", "a"), "ben": os.path.join(home, "pool", "b"), "ctl": None}
    jw(wj, {"members": {KEY[m]: {"config_dir": cd[m], "launch": {"passthrough": True}, "agent_session": "s-" + m} for m in M}})
    jw(os.path.join(run, "expect.json"), {"a": HASH["ana"], "b": HASH["ben"], "c": HASH["ctl"]})
    tw(os.path.join(run, "up.txt"), "zygo: ana, ben run in passthrough on pool %s/pool\nzygo: repo is up\n" % home)
    ccd = {"ana": "pool/a", "ben": "pool/b", "ctl": None}
    procs = [{"pid": 1, "prog": "tmux", "member": None, "home_is_temp": True, "claude_config_dir": None, "git_env_diff": [],
              "real_dir_vars": [], "real_dir_files": 0, "remote_tcp": 0}]
    for i, m in enumerate(M):
        for prog, c in (("zygo", None), ("claude", ccd[m])):
            procs.append({"pid": 10 * i + (2 if prog == "claude" else 1), "prog": prog, "member": m, "home_is_temp": True,
                          "claude_config_dir": c, "git_env_diff": [], "real_dir_vars": [], "real_dir_files": 0, "remote_tcp": 0})
    jw(os.path.join(run, "proc-1.json"), procs)
    jw(os.path.join(run, "proc-2.json"), procs)
    rec = []
    for m in M:
        c = {"client_who": "launcher", "client_prog": "zygo", "pane": KEY[m], "ws": "w"}
        u = {"client_who": "under-launcher", "client_prog": "claude", "pane": KEY[m], "ws": "w"}
        rec.append(dict(kind="connect", target="api.anthropic.com:443", **c))
        for _ in range(3):
            rec.append(dict(kind="request", path="/v1/messages", auth_sha256_12=HASH[m], status=200, **c))
        rec.append(dict(kind="request", path="/api/oauth/profile", auth_sha256_12=HASH[m], status=404, **u))
        rec.append(dict(kind="request", path="/api/directory/server-urls", auth_sha256_12=None, status=404, **u))
    rec.append({"kind": "refused", "target": "github.com:443", "client_who": "under-launcher", "client_prog": "git-remote-https", "pane": KEY["ana"]})
    tw(os.path.join(run, "tls-rec.jsonl"), "".join(json.dumps(r) + "\n" for r in rec))
    tw(os.path.join(run, "ssh-path.log"), "-T -o BatchMode=yes git" "@github.com\n")
    jw(os.path.join(run, "usage.json"), [{"session": ZS[m], "role_served": "subscription", "passthrough": 1, "status": 200} for m in M])
    jw(os.path.join(run, "trims.json"), [{"session": ZS[m], "tool": "Read", "stored_bytes": STORED[m], "view_bytes": 6000,
                                          "result_id": "RID" + m.upper()} for m in M])
    i = 0
    for m in M:
        h = {"x-live-who": "launcher", "x-live-pane": KEY[m]}
        bodies = [
            {"tools": [{"name": "Read"}, {"name": "ToolSearch"}], "messages": [{"role": "user", "content": [
                {"type": "text", "text": "deferred tools:\nmcp__zygo__compact\nmcp__zygo__result_view\nskills: zygo:zygo-cli"}]}]},
            {"tools": [{"name": "Read"}], "messages": [{"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_pr_%s_00_read" % m,
                 "content": "line 1\n[zygo trimmed this output: 200 lines, 20000 bytes; only part of it is shown. The full output "
                            "is saved as result RID%s. To read the rest, call result_view with id \"RID%s\".]" % (m.upper(), m.upper())}]}]},
            {"tools": [{"name": "Read"}], "messages": [{"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_pr_%s_01_write" % m, "is_error": True,
                 "content": "PreToolUse:Write hook error: " + GUARD}]}]},
        ]
        for b in bodies:
            i += 1
            jw(os.path.join(run, "rec", "%04d.json" % i), {"headers": dict(h), "body": b})
    act = []
    tw(os.path.join(run, "prompts.txt"), "".join("%s %.3f\n" % (m, T0 + 10 * k) for k, m in enumerate(M)))
    for k, m in enumerate(M):
        t = T0 + 10 * k
        act += [{"t": t + 0.5, "m": m, "busy": True, "tools": 0, "ev": "write"}, {"t": t + 0.6, "m": m, "busy": True, "tools": 1, "ev": "write"},
                {"t": t + 0.9, "m": m, "busy": False, "tools": 0, "ev": "write"}, {"t": T0 + 100, "m": m, "busy": False, "tools": 0, "ev": "write"},
                {"t": T0 + 101, "m": m, "busy": None, "tools": None, "ev": "gone"}]
    tw(os.path.join(run, "activity.jsonl"), "".join(json.dumps(a) + "\n" for a in act))
    jw(os.path.join(run, "down.json"), {"t": T0 + 99})
    for m in M:
        tw(os.path.join(run, "pane-%s.txt" % m), pane(m))
        tw(os.path.join(run, "menu-%s.txt" % m), "❯ /zygo:\n  ❯ /zygo:ls   (zygo) x\n    /zygo:zygo-cli   (zygo) y\n")
        u = os.path.join(run, "userhooks-%s" % ("ctl" if m == "ctl" else m[0]))
        jw(os.path.join(u, "0001-UserPromptSubmit-main.json"), {})
        jw(os.path.join(u, "0002-SessionEnd-main.json"), {})
    return run, home, wj


def failed(change=None):
    """The failed sub ids after `change(run, home, wj)` on a fresh passing run folder."""
    root = tempfile.mkdtemp()
    try:
        run, home, wj = build(root)
        if change:
            change(run, home, wj)
        out = pa.main(run, home, wj)
        return sorted(l.split("\t")[1] for l in out if l.split("\t")[2] == "1"), out
    finally:
        shutil.rmtree(root)


def edit_jsonl(path, fn):
    rows = [json.loads(x) for x in open(path)]
    rows = fn(rows)
    tw(path, "".join(json.dumps(r) + "\n" for r in rows))


def rec_edit(run, k, fn):
    p = os.path.join(run, "rec", "%04d.json" % k)
    o = json.load(open(p))
    fn(o)
    jw(p, o)


def proc_edit(run, fn):
    for f in ("proc-1.json", "proc-2.json"):
        p = os.path.join(run, f)
        rows = json.load(open(p))
        fn(rows)
        jw(p, rows)


f0, out0 = failed()
check("baseline: every sub-check passes", f0 == [], "\n".join(out0))
check("baseline: 14 sub-checks and 2 notes", len([l for l in out0 if l.split("\t")[2] != "note"]) == 14
      and len([l for l in out0 if l.split("\t")[2] == "note"]) == 2, len(out0))
check("short: the status line number form", [pa.short(x) for x in (0, 999, 1000, 3500, 999999, 1000000, 1500000)]
      == ["0", "999", "1k", "3k", "999k", "1m", "1.5m"])


def wrong_hash(run, home, wj):
    def fn(rows):
        for r in rows:
            if r.get("kind") == "request" and r["pane"] == KEY["ana"] and r["path"] == "/v1/messages":
                r["auth_sha256_12"] = "dddddddddddd"
                break
        return rows
    edit_jsonl(os.path.join(run, "tls-rec.jsonl"), fn)
f, _ = failed(wrong_hash)
check("PR1: a wrong hash in ana's pane fails 1.3 only", f == ["1.3"], f)


def c_in_pool(run, home, wj):
    def fn(rows):
        for r in rows:
            if r.get("kind") == "request" and r["pane"] == KEY["ana"] and r["client_who"] == "under-launcher":
                r["auth_sha256_12"] = HASH["ctl"]
                break
        return rows
    edit_jsonl(os.path.join(run, "tls-rec.jsonl"), fn)
f, _ = failed(c_in_pool)
check("PR1: hash C (default folder) in ana's pane fails 1.3 and 1.5", f == ["1.3", "1.5"], f)


def two_panes(run, home, wj):
    def fn(rows):
        rows.append({"kind": "request", "path": "/v1/messages", "auth_sha256_12": HASH["ana"], "status": 200,
                     "client_who": "launcher", "client_prog": "zygo", "pane": KEY["ben"], "ws": "w"})
        return rows
    edit_jsonl(os.path.join(run, "tls-rec.jsonl"), fn)
f, _ = failed(two_panes)
check("PR1: hash A in ana's and ben's panes fails 1.3, 1.4 and 1.5 (each hash only in its own pane)", f == ["1.3", "1.4", "1.5"], f)


def no_pane(run, home, wj):
    def fn(rows):
        rows.append({"kind": "request", "path": "/v1/messages", "auth_sha256_12": HASH["ben"], "status": 200,
                     "client_who": "other", "client_prog": "x", "pane": None, "ws": None})
        return rows
    edit_jsonl(os.path.join(run, "tls-rec.jsonl"), fn)
f, _ = failed(no_pane)
check("PR1: hash B from no member pane fails 1.4 and 1.5", f == ["1.4", "1.5"], f)


def no_launcher(run, home, wj):
    edit_jsonl(os.path.join(run, "tls-rec.jsonl"), lambda rows: [r for r in rows if not (r.get("pane") == KEY["ctl"] and r.get("client_who") == "launcher" and r.get("kind") == "request")])
f, _ = failed(no_launcher)
check("PR1: no /v1/messages from ctl's launcher fails 1.5", f == ["1.5"], f)


def ssh_used(run, home, wj):
    tw(os.path.join(run, "ssh-git.log"), "git" "@github.com git-upload-pack 'anthropics/x.git'\n")
f, _ = failed(ssh_used)
check("PR1: a non-empty git ssh wrapper log fails 1.6", f == ["1.6"], f)


def git_reached(run, home, wj):
    g = os.path.join(home, "pool", "a", "plugins", "marketplaces", "m", ".git")
    os.makedirs(os.path.join(g, "refs", "remotes", "origin"))
    tw(os.path.join(g, "refs", "remotes", "origin", "main"), "0" * 40 + "\n")
f, out = failed(git_reached)
check("PR1: a .git folder with fetched refs under a config folder fails 1.6", f == ["1.6"], f)


def git_empty(run, home, wj):
    os.makedirs(os.path.join(home, ".claude", "plugins", "marketplaces", "m", ".git", "objects", "pack"))
f, out = failed(git_empty)
check("PR1: a .git folder that reached nothing passes 1.6", f == [], f)


def env_dropped(run, home, wj):
    proc_edit(run, lambda rows: rows.append({"pid": 99, "prog": "git", "member": "ana", "home_is_temp": True, "claude_config_dir": None,
                                             "git_env_diff": ["GIT_CONFIG_GLOBAL"], "real_dir_vars": [], "real_dir_files": 0, "remote_tcp": 0}))
f, _ = failed(env_dropped)
check("PR1: a git process without the run's git config fails 1.2", f == ["1.2"], f)


def env_extended(run, home, wj):
    proc_edit(run, lambda rows: rows.append({"pid": 99, "prog": "git", "member": "ana", "home_is_temp": True, "claude_config_dir": None,
                                             "git_env_diff": ["GIT_SSH", "GIT_SSH_COMMAND=WRAPPER -o BatchMode=yes"], "real_dir_vars": [],
                                             "real_dir_files": 0, "remote_tcp": 0}))
f, _ = failed(env_extended)
check("PR1: a git process with GIT_SSH dropped and the wrapper extended passes 1.2", f == [], f)


def env_cd_wrapper(run, home, wj):
    proc_edit(run, lambda rows: rows.append({"pid": 98, "prog": "git-remote-https", "member": "ben", "home_is_temp": True,
                                             "claude_config_dir": None, "real_dir_vars": [], "real_dir_files": 0, "remote_tcp": 0,
                                             "git_env_diff": ["GIT_SSH", "GIT_SSH_COMMAND=CDPATH= cd -- <path> <path> || cd <path> || exit 1; "
                                                              "WRAPPER -o BatchMode=yes -o StrictHostKeyChecking=yes"]}))
f, _ = failed(env_cd_wrapper)
check("PR1: Claude Code's git command form (a cd, then the wrapper) passes 1.2", f == [], f)


def env_real_ssh(run, home, wj):
    proc_edit(run, lambda rows: rows.append({"pid": 98, "prog": "git", "member": "ben", "home_is_temp": True,
                                             "claude_config_dir": None, "real_dir_vars": [], "real_dir_files": 0, "remote_tcp": 0,
                                             "git_env_diff": ["GIT_SSH", "GIT_SSH_COMMAND=SSH-PROGRAM -o BatchMode=yes WRAPPER"]}))
f, _ = failed(env_real_ssh)
check("PR1: a git command that names another ssh program fails 1.2", f == ["1.2"], f)


def env_claude(run, home, wj):
    proc_edit(run, lambda rows: [r.update(git_env_diff=["GIT_SSH"]) for r in rows if r["prog"] == "claude" and r["member"] == "ben"])
f, _ = failed(env_claude)
check("PR1: a Claude Code process without GIT_SSH fails 1.2", f == ["1.2"], f)


def real_dir(run, home, wj):
    proc_edit(run, lambda rows: rows[1].update(real_dir_files=1))
f, _ = failed(real_dir)
check("PR1: an open file under the operator's config folder fails 1.2", f == ["1.2"], f)


def remote_tcp(run, home, wj):
    proc_edit(run, lambda rows: rows[2].update(remote_tcp=1))
f, _ = failed(remote_tcp)
check("PR1: a non-loopback TCP connection fails 1.2", f == ["1.2"], f)


def no_hud(run, home, wj):
    tw(os.path.join(run, "pane-ana.txt"), pane("ana").replace(hud_row("ana", TRIM["ana"]), "  plain row"))
f, _ = failed(no_hud)
check("PR2: no HUD row in ana's pane fails 2.4 only", f == ["2.4"], f)


def user_sl(run, home, wj):
    tw(os.path.join(run, "pane-ben.txt"), pane("ben", extra=["  USER-SL-b"]))
f, _ = failed(user_sl)
check("PR2: a USER-SL row fails 2.4 only", f == ["2.4"], f)


def other_numbers(run, home, wj):
    tw(os.path.join(run, "pane-ana.txt"), pane("ana", trim=TRIM["ben"]))
f, _ = failed(other_numbers)
check("PR2: ana's HUD showing ben's trimmed value fails 2.4 only", f == ["2.4"], f)


def other_name(run, home, wj):
    tw(os.path.join(run, "pane-ben.txt"), pane("ben").replace(" ben ", " ana ", 1))
f, _ = failed(other_name)
check("PR2: ben's HUD naming ana fails 2.4 only", f == ["2.4"], f)


def no_session_data(run, home, wj):
    jw(os.path.join(run, "trims.json"), [])
    for m in M:
        tw(os.path.join(run, "pane-%s.txt" % m), pane(m, trim="0"))
f, _ = failed(no_session_data)
check("PR2: a HUD with no session data (all 0) fails 2.4 and 2.6", f == ["2.4", "2.6"], f)


def tmux_hint(run, home, wj):
    tw(os.path.join(run, "pane-ana.txt"), pane("ana").replace("⏵⏵", "⏵⏵", 1).replace(
        hud_row("ana", TRIM["ana"]), hud_row("ana", TRIM["ana"]) + "          tmux focus-events off · add 'set -g focus-events on'"))
f, _ = failed(tmux_hint)
check("PR2: a trailing tmux hint in one pane is ignored", f == [], f)


def not_error(run, home, wj):
    rec_edit(run, 3, lambda o: o["body"]["messages"][0]["content"][0].update(is_error=False))
f, _ = failed(not_error)
check("PR2: a denied Write without is_error true fails 2.3 only", f == ["2.3"], f)


def under_launcher(run, home, wj):
    for k in (1, 2, 3):
        rec_edit(run, k, lambda o: o["headers"].update({"x-live-who": "under-launcher"}))
f, _ = failed(under_launcher)
check("PR2: ana's requests not from its launcher fail 2.3, 2.5, 2.6 and 2.7", f == ["2.3", "2.5", "2.6", "2.7"], f)


def write_done(run, home, wj):
    tw(os.path.join(run, "repo", "pr-ctl.txt"), "x")
f, _ = failed(write_done)
check("PR2: a written file fails 2.3 only", f == ["2.3"], f)


def no_trim_view(run, home, wj):
    rec_edit(run, 5, lambda o: o["body"]["messages"][0]["content"][0].update(content="full text, not trimmed"))
f, _ = failed(no_trim_view)
check("PR2: ben's Read result without zygo's view fails 2.6 only", f == ["2.6"], f)


def no_mcp(run, home, wj):
    rec_edit(run, 7, lambda o: o["body"]["messages"][0]["content"][0].update(text="skills: zygo:zygo-cli"))
f, _ = failed(no_mcp)
check("PR2: ctl's request without zygo's MCP tools fails 2.7 only", f == ["2.7"], f)


def menu_diff(run, home, wj):
    tw(os.path.join(run, "menu-ben.txt"), "❯ /zygo:\n  ❯ /zygo:ls   (zygo) x\n")
f, _ = failed(menu_diff)
check("PR2: a different /zygo: menu fails 2.5 only", f == ["2.5"], f)


def no_session_end(run, home, wj):
    os.remove(os.path.join(run, "userhooks-b", "0002-SessionEnd-main.json"))
f, _ = failed(no_session_end)
check("PR2: no SessionEnd input in pool/b fails 2.8 only", f == ["2.8"], f)


def no_end_write(run, home, wj):
    edit_jsonl(os.path.join(run, "activity.jsonl"), lambda rows: [r for r in rows if not (r["m"] == "ctl" and r["t"] >= T0 + 99)])
f, _ = failed(no_end_write)
check("PR2: no activity.json write after zygo down fails 2.8 only", f == ["2.8"], f)


def no_busy(run, home, wj):
    edit_jsonl(os.path.join(run, "activity.jsonl"), lambda rows: [r for r in rows if not (r["m"] == "ana" and r["busy"] is True)])
f, _ = failed(no_busy)
check("PR2: no busy true for ana fails 2.2 only", f == ["2.2"], f)


def no_session(run, home, wj):
    o = json.load(open(wj))
    o["members"][KEY["ben"]]["agent_session"] = None
    jw(wj, o)
f, _ = failed(no_session)
check("PR2: no agent session for ben fails 2.1 only", f == ["2.1"], f)


def ctl_in_pool(run, home, wj):
    tw(os.path.join(run, "up.txt"), "zygo: ana, ben, ctl run in passthrough on pool %s/pool\n" % home)
f, _ = failed(ctl_in_pool)
check("PR1: ctl named in the pool line fails 1.1 only", f == ["1.1"], f)

if bad:
    print("pool_analyze tests: %d failed" % bad)
    sys.exit(1)
print("pool_analyze tests: ok (%d)" % n)

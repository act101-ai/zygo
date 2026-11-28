#!/usr/bin/env python3
"""The judge of live-pool.sh (PR1, PR2; nodes WD-21, WD-22): pool_analyze.py <run dir> <temporary HOME> <workspace.json>

Reads the files that live-pool.sh writes in the run folder and prints one line per sub-check or note, tab-separated:
<id> <sub id> <0 = pass | 1 = fail | note> <detail>. It also writes <run dir>/status-rows.json (the status line rows
of each pane). Python 3 stdlib only; no network, no process is started.

Run folder files: members.txt (<name> <pane key> <tmux pane> <pane pid> <zygo session> per line), expect.json
(hash prefix per folder a, b, c), up.txt, proc-1.json and proc-2.json (process snapshots), tls-rec.jsonl (recorder
lines), ssh.log (the ssh wrapper's log), usage.json and trims.json (usage rows), rec/<n>.json (requests at the
mock), activity.jsonl (activity.json writes), prompts.txt (<name> <time>), down.json (the time zygo down started),
pane-<name>.txt, menu-<name>.txt, userhooks-<a|b|ctl>/ (hook_log.py inputs of each folder's own hooks), repo/.
"""
import glob
import json
import os
import re
import sys
import unicodedata

M = ["ana", "ben", "ctl"]
TAG = {"ana": "a", "ben": "b", "ctl": "c"}
HNAME = {"ana": "A", "ben": "B", "ctl": "C"}
FOLDER = {"ana": "pool/a", "ben": "pool/b", "ctl": "default"}
GUARD = "marks a secret zygo removed"
BYTES_PER_TOKEN = 4   # zygo_core::tokens::BYTES_PER_TOKEN


def short(n):
    """zygo-hooks statusline `short`: the number form of the status line."""
    if n < 1000:
        return str(n)
    if n < 1_000_000:
        return "%dk" % (n // 1000)
    unit, suffix = (1_000_000, "m") if n < 1_000_000_000 else (1_000_000_000, "b")
    if n % unit == 0:
        return "%d%s" % (n // unit, suffix)
    return "%d.%d%s" % (n // unit, n % unit // (unit // 10), suffix)


def trimmed_text(rows):
    """The ✂ value the HUD shows for these trims rows: short(ceil(sum(max(stored - view, 0)) / 4))."""
    b = sum(max(r["stored_bytes"] - r["view_bytes"], 0) for r in rows)
    return short(-(-b // BYTES_PER_TOKEN))


# A HUD row (NFKC folds the logo's mathematical letters): the stripe, the logo, the member name, ..., the trimmed segment.
HUD = re.compile(r"^\s*▌+\s+zygo\s+(?P<name>\S+)\s+.*?✂\s+(?P<trim>\S+)")
# A tmux hint that Claude Code may append at the right of the row, after a wide gap.
TMUX_HINT = re.compile(r"\s{2,}tmux .*$")


def load(p, dflt):
    try:
        with open(p) as f:
            return json.load(f)
    except (OSError, ValueError):
        return dflt


def lines(p):
    try:
        with open(p) as f:
            return [json.loads(x) for x in f if x.strip()]
    except (OSError, ValueError):
        return []


def text(p):
    try:
        with open(p, encoding="utf-8", errors="replace") as f:
            return f.read()
    except OSError:
        return ""


def yn(b):
    return "yes" if b else "no"


def status_rows(pane_text):
    """The non-empty rows below the last horizontal rule of a pane capture (the prompt box), right-stripped."""
    t = pane_text.splitlines()
    idx = [i for i, l in enumerate(t) if re.fullmatch(r"\s*─{20,}\s*", l)]
    rows = [l.rstrip() for l in (t[idx[-1] + 1:] if idx else [])]
    return [r for r in rows if r.strip()]


def hud(rows):
    """(name, trim) of the first HUD row, or None."""
    for r in rows:
        m = HUD.match(unicodedata.normalize("NFKC", r))
        if m:
            return m.group("name"), m.group("trim")
    return None


def masked(rows, name, trim):
    """Rows with the tmux hint, the member's name and trimmed value, and every digit masked."""
    out = []
    for r in rows:
        r = TMUX_HINT.sub("", unicodedata.normalize("NFKC", r))
        if name:
            r = re.sub(r"(?<!\S)%s(?!\S)" % re.escape(name), "NAME", r)
        if trim:
            r = re.sub(r"(✂\s+)%s(?!\S)" % re.escape(trim), r"\1TRIM", r)
        out.append(re.sub(r"[0-9]", "9", r))
    return out


def git_dirs(home):
    """(.git folders under home, of which under a config folder, of which reached): reached = holds fetched refs or
    objects (packed-refs, refs/remotes/*, FETCH_HEAD, or a pack)."""
    cfg = [os.path.join(home, "pool", "a"), os.path.join(home, "pool", "b"), os.path.join(home, ".claude")]
    found, in_cfg, reached = [], 0, []
    for root, dirs, _files in os.walk(home):
        if ".git" in dirs:
            g = os.path.join(root, ".git")
            found.append(g)
            if any(g.startswith(c + os.sep) for c in cfg):
                in_cfg += 1
            remotes = [f for _r, _d, fs in os.walk(os.path.join(g, "refs", "remotes")) for f in fs]
            packs = glob.glob(os.path.join(g, "objects", "pack", "*.pack"))
            if remotes or packs or os.path.exists(os.path.join(g, "packed-refs")) or os.path.exists(os.path.join(g, "FETCH_HEAD")):
                reached.append(os.path.relpath(g, home))
            dirs.remove(".git")
    return len(found), in_cfg, reached


def main(run, home, wj):
    out = []
    sub = lambda i, s, ok, d: out.append("%s\t%s\t%s\t%s" % (i, s, 0 if ok else 1, d))
    note = lambda i, s, d: out.append("%s\t%s\tnote\t%s" % (i, s, d))
    mem = {}
    for x in text(os.path.join(run, "members.txt")).splitlines():
        if x.strip():
            n, k, pane, ppid, zs = x.split()
            mem[n] = {"key": k, "pane": pane, "ppid": int(ppid), "zs": zs}
    key2m = {v["key"]: n for n, v in mem.items()}
    w = load(wj, {})
    ms = w.get("members", {})
    ex = load(os.path.join(run, "expect.json"), {})
    own = {n: ex.get(TAG[n]) for n in M}
    hname = {own[n]: HNAME[n] for n in M}
    hn = lambda h: hname.get(h, "other" if h else "none")
    fmt = lambda d: ", ".join("%s %d" % (k, d[k]) for k in sorted(d)) or "none"
    rel = lambda p: "default" if not p else (os.path.relpath(p, home) if p.startswith(home + "/") else "OUTSIDE-TEMP-HOME")

    def count(items):
        d = {}
        for i in items:
            d[i] = d.get(i, 0) + 1
        return d

    # ---- PR1 ----
    up = text(os.path.join(run, "up.txt"))
    pline = next((l for l in up.splitlines() if "run in passthrough on pool" in l), "")
    pnames = re.findall(r"[a-z]+", pline.split(" run in passthrough")[0].replace("zygo:", "")) if pline else []
    m_of = lambda n: ms.get(mem.get(n, {}).get("key", ""), {})
    cd = {n: m_of(n).get("config_dir") for n in M}
    pt = {n: (m_of(n).get("launch") or {}).get("passthrough") for n in M}
    ok = (pnames == ["ana", "ben"] and cd["ana"] == os.path.join(home, "pool", "a") and cd["ben"] == os.path.join(home, "pool", "b")
          and not cd["ctl"] and all(pt[n] is True for n in M))
    sub("PR1", "1.1", ok, "zygo up line \"%s\"; member config_dir ana=%s ben=%s ctl=%s; launch.passthrough ana=%s ben=%s ctl=%s"
        % (pline.replace(home, "HOME"), rel(cd["ana"]), rel(cd["ben"]), rel(cd["ctl"]), pt["ana"], pt["ben"], pt["ctl"]))

    s1, s2 = load(os.path.join(run, "proc-1.json"), []), load(os.path.join(run, "proc-2.json"), [])
    allrows = s1 + s2
    cl = {n: [r for r in s1 if r.get("prog") == "claude" and r.get("member") == n] for n in M}
    ccd = {n: sorted({str(r.get("claude_config_dir")) for r in cl[n]}) for n in M}
    temp_all = bool(allrows) and all(r.get("home_is_temp") for r in allrows)
    real_vars = sorted({v for r in allrows for v in r.get("real_dir_vars", [])})
    real_files = sum(r.get("real_dir_files", 0) for r in allrows)
    diffs = count("%s: %s" % (r.get("prog"), " ".join(r.get("git_env_diff", ["?"]))) for r in allrows if r.get("git_env_diff", ["?"]))

    def env_ok(r):
        """All four variables as the run set them. A git process that Claude Code starts may drop GIT_SSH and put the
        wrapper inside its own GIT_SSH_COMMAND (Claude Code 2.1.332: a cd, then the wrapper with -o options), if that
        command runs the wrapper and names no other ssh program (git's config and the wrapper stay in force)."""
        d = r.get("git_env_diff", ["?"])
        if not str(r.get("prog", "")).startswith("git"):
            return not d

        def cmd_ok(x):
            words = x[len("GIT_SSH_COMMAND="):].split()
            return "WRAPPER" in words and "SSH-PROGRAM" not in words and "ssh" not in words
        return all(x == "GIT_SSH" or (x.startswith("GIT_SSH_COMMAND=") and cmd_ok(x)) for x in d)
    git_env = bool(allrows) and all(env_ok(r) for r in allrows)
    remote = sum(r.get("remote_tcp", 0) for r in allrows)
    ok = (all(len(cl[n]) == 1 for n in M) and ccd["ana"] == ["pool/a"] and ccd["ben"] == ["pool/b"] and ccd["ctl"] == ["None"]
          and temp_all and not real_vars and real_files == 0 and git_env and remote == 0)
    sub("PR1", "1.2", ok, "Claude Code processes ana %d, ben %d, ctl %d with CLAUDE_CONFIG_DIR %s, %s, %s; processes under the private tmux "
        "server in two snapshots (idle before the prompts, after the turns): %d and %d; in all of them: HOME = the temporary HOME %s, "
        "the run's GIT_CONFIG_GLOBAL, GIT_CONFIG_NOSYSTEM, GIT_SSH_COMMAND and GIT_SSH in force %s (processes that differ, by program: %s); variables whose "
        "value names the operator's config folder: %s; open files under it: %d; TCP connections to a non-loopback address: %d"
        % (len(cl["ana"]), len(cl["ben"]), len(cl["ctl"]), "/".join(ccd["ana"]) or "-", "/".join(ccd["ben"]) or "-",
           "unset" if ccd["ctl"] == ["None"] else ("/".join(ccd["ctl"]) or "-"), len(s1), len(s2), yn(temp_all), yn(git_env),
           fmt(diffs), ", ".join(real_vars) or "none", real_files, remote))

    rec = lines(os.path.join(run, "tls-rec.jsonl"))
    reqs = [r for r in rec if r.get("kind") == "request"]
    pane_of = lambda r: key2m.get(r.get("pane"))
    per = {n: [r for r in reqs if pane_of(r) == n] for n in M}
    lmsg = {n: sum(1 for r in per[n] if r.get("client_who") == "launcher" and str(r.get("path", "")).startswith("/v1/messages")) for n in M}
    hs = {n: count(hn(r.get("auth_sha256_12")) for r in per[n]) for n in M}
    who = {n: count(str(r.get("client_who")) for r in per[n]) for n in M}
    # A hash outside its own member's pane (another member's pane, or no member pane).
    outside = {n: sum(1 for r in reqs if r.get("auth_sha256_12") == own[n] and pane_of(r) != n) for n in M}
    # Per pane: every request with an Authorization value carries the member's own hash (requests with no
    # Authorization, "none", carry no hash), and at least one /v1/messages request came from the member's launcher.
    pane_ok = {n: lmsg[n] >= 1 and HNAME[n] in hs[n] and set(hs[n]) <= {HNAME[n], "none"} for n in M}
    for n, s in (("ana", "1.3"), ("ben", "1.4")):
        sub("PR1", s, pane_ok[n] and outside[n] == 0,
            "%s (%s): requests whose tunnel belongs to %s's pane (by kind: %s): %d, of which /v1/messages from its launcher %d; "
            "by hash: %s (own hash(%s) = %s; none = no Authorization); requests with hash(%s) outside %s's pane: %d"
            % (n, FOLDER[n], n, fmt(who[n]), len(per[n]), lmsg[n], fmt(hs[n]), HNAME[n], own[n], HNAME[n], n, outside[n]))
    distinct = len({own[n] for n in M}) == 3 and None not in own.values()
    us = load(os.path.join(run, "usage.json"), [])
    ur = {n: [u for u in us if u.get("session") == mem.get(n, {}).get("zs")] for n in M}
    uok = all(ur[n] and all(u.get("passthrough") == 1 and u.get("status") == 200 for u in ur[n]) for n in M)
    roles = sorted({str(u.get("role_served")) for n in M for u in ur[n]})
    ok = pane_ok["ctl"] and outside["ctl"] == 0 and distinct and all(outside[n] == 0 for n in M) and uok
    sub("PR1", "1.5", ok, "ctl (default folder): requests in its pane (by kind: %s): %d, /v1/messages from its launcher %d, by hash: %s; "
        "the three hashes differ: %s; requests with a member's hash outside that member's pane: A %d, B %d, C %d; usage rows ana %d, "
        "ben %d, ctl %d, all passthrough = 1 and status 200: %s (role_served: %s)"
        % (fmt(who["ctl"]), len(per["ctl"]), lmsg["ctl"], fmt(hs["ctl"]), yn(distinct), outside["ana"], outside["ben"], outside["ctl"],
           len(ur["ana"]), len(ur["ben"]), len(ur["ctl"]), yn(uok), ", ".join(roles) or "none"))
    sgit = [l for l in text(os.path.join(run, "ssh-git.log")).splitlines() if l.strip()]
    spath = [l for l in text(os.path.join(run, "ssh-path.log")).splitlines() if l.strip()]
    ng, ncfg, reached = git_dirs(home)
    sub("PR1", "1.6", not sgit and not reached,
        "git's ssh wrapper (GIT_SSH_COMMAND, GIT_SSH) log: %d lines; .git folders under the temporary HOME: %d (%d under a config "
        "folder), of which reached a remote: %d%s; direct ssh calls to the ssh on the run's PATH (a wrapper that exits 1, so no ssh "
        "ran): %d (%s)"
        % (len(sgit), ng, ncfg, len(reached), (" (" + ", ".join(reached) + ")") if reached else "", len(spath),
           "; ".join("%s x%d" % (k.replace("@", " at "), v) for k, v in sorted(count(spath).items())) or "none"))
    others = [r for r in reqs if not (r.get("client_who") == "launcher" and str(r.get("path", "")).startswith("/v1/messages"))]
    oth = {}
    for r in others:
        oth.setdefault("%s %s %s" % (pane_of(r) or "-", r.get("client_prog"), r.get("path")), []).append(hn(r.get("auth_sha256_12")))
    refused = count("%s (%s %s)" % (r.get("target"), r.get("client_who"), r.get("client_prog")) for r in rec if r.get("kind") == "refused")
    tlse = count("%s %s" % (r.get("client_who"), r.get("client_prog")) for r in rec if r.get("kind") == "tls_error")
    conns = count("%s %s %s" % (pane_of(r) or "-", r.get("client_who"), r.get("client_prog")) for r in rec if r.get("kind") == "connect")
    note("PR1", "1.7", "CONNECT api.anthropic.com:443 by pane, kind and program: %s; failed TLS handshakes: %s; requests that are not "
         "/v1/messages from a launcher, by pane, program and path: %s; refused CONNECT targets: %s"
         % (fmt(conns), fmt(tlse), "; ".join("%s: %s" % (k, fmt(count(v))) for k, v in sorted(oth.items())) or "none", fmt(refused)))

    # ---- PR2 ----
    asess = {n: bool(m_of(n).get("agent_session")) for n in M}
    sub("PR2", "2.1", all(asess.values()), "SessionStart (zygo's member entry): agent_session in workspace.json: "
        + ", ".join("%s %s" % (n, yn(asess[n])) for n in M))

    act = lines(os.path.join(run, "activity.jsonl"))
    pr = {}
    for x in text(os.path.join(run, "prompts.txt")).splitlines():
        if x.strip():
            n, t = x.split()
            pr[n] = float(t)

    def tf(n):
        t0 = pr.get(n, float("inf"))
        ev = [e for e in act if e.get("m") == n and e.get("t", 0) >= t0 and e.get("ev") == "write"]
        t1 = next((e["t"] for e in ev if e.get("busy") is True), None)
        t2 = next((e["t"] for e in ev if t1 is not None and e.get("busy") is False and e["t"] > t1), None)
        tools = [e.get("tools") for e in ev if t1 is not None and (t2 is None or e["t"] <= t2)]
        return t1, t2, t0, tools
    b = {n: tf(n) for n in M}
    okb = {n: b[n][0] is not None and b[n][1] is not None for n in M}
    sub("PR2", "2.2", all(okb.values()), "UserPromptSubmit and Stop (zygo's activity.json): busy true, then false, after the prompt: "
        + ", ".join(("%s yes (true at +%.2f s, false at +%.2f s)" % (n, b[n][0] - b[n][2], b[n][1] - b[n][2])) if okb[n] else "%s no" % n for n in M))

    recs = [load(f, {}) for f in sorted(glob.glob(os.path.join(run, "rec", "[0-9]*.json")))]
    mine = lambda o, n: (o.get("headers", {}).get("x-live-who") == "launcher" and key2m.get(o.get("headers", {}).get("x-live-pane")) == n)

    def result_of(n, tid):
        """(text, is_error) of the tool_result for tool id tid in a request from n's launcher, or (None, None)."""
        for o in recs:
            body = o.get("body")
            if not mine(o, n) or not isinstance(body, dict):
                continue
            for msg in body.get("messages") or []:
                c = msg.get("content") if isinstance(msg, dict) else None
                for item in c if isinstance(c, list) else []:
                    if isinstance(item, dict) and item.get("type") == "tool_result" and item.get("tool_use_id") == tid:
                        return json.dumps(item.get("content"), ensure_ascii=False), item.get("is_error") is True
        return None, None
    wr = {n: result_of(n, "toolu_pr_%s_01_write" % n) for n in M}
    gtext = {}
    for n in M:
        m = re.search(GUARD + r"[^\"\\]*", wr[n][0] or "")
        gtext[n] = m.group(0) if m else None
    absent = {n: not os.path.exists(os.path.join(run, "repo", "pr-%s.txt" % n)) for n in M}
    ok = all(gtext[n] and wr[n][1] is True and absent[n] for n in M) and len(set(gtext.values())) == 1
    sub("PR2", "2.3", ok, "PreToolUse (zygo's guard): the Write of the redaction marker, in a request from the member's launcher, has a "
        "tool result with is_error true and the guard text: " + ", ".join("%s %s" % (n, yn(bool(gtext[n]) and wr[n][1] is True)) for n in M)
        + "; same text for all three: %s (\"%s\"); file absent: %s"
        % (yn(len(set(gtext.values())) == 1), gtext.get("ctl") or "none", ", ".join("%s %s" % (n, yn(absent[n])) for n in M)))

    trims = load(os.path.join(run, "trims.json"), [])
    tr = {n: [t for t in trims if t.get("session") == mem.get(n, {}).get("zs")] for n in M}
    want = {n: trimmed_text(tr[n]) for n in M}
    panes = {n: text(os.path.join(run, "pane-%s.txt" % n)) for n in M}
    sr = {n: status_rows(panes[n]) for n in M}
    hd = {n: hud(sr[n]) for n in M}
    user_sl = {n: "USER-SL" in panes[n] for n in M}
    own_hud = {n: hd[n] is not None and hd[n][0] == n and hd[n][1] == want[n] for n in M}
    wdistinct = len(set(want.values())) == 3
    mk = {n: masked(sr[n], n, hd[n][1] if hd[n] else None) for n in M}
    same = all(mk[n] == mk["ctl"] for n in M)
    ok = all(sr[n] for n in M) and all(own_hud.values()) and wdistinct and same and not any(user_sl.values())
    sub("PR2", "2.4", ok, "status line: HUD row (stripe, logo, member name, trimmed segment) shows the member's own name and its own "
        "session's trimmed value: " + ", ".join("%s %s (shows %s, its session %s)" % (n, yn(own_hud[n]), ("%s ✂ %s" % hd[n]) if hd[n] else "no HUD row", want[n]) for n in M)
        + "; the three values differ: %s; rows equal to ctl's after masking the name, the trimmed value, the digits and a trailing tmux hint: "
        "ana %s, ben %s; USER-SL (each folder's own statusLine) in a pane: %s"
        % (yn(wdistinct), yn(mk["ana"] == mk["ctl"]), yn(mk["ben"] == mk["ctl"]), ", ".join("%s %s" % (n, yn(user_sl[n])) for n in M)))

    def menu(n):
        return sorted(set(re.findall(r"/zygo:[A-Za-z0-9_-]+", text(os.path.join(run, "menu-%s.txt" % n)))))
    mn = {n: menu(n) for n in M}

    def first_tools(n):
        for o in recs:
            body = o.get("body")
            if mine(o, n) and isinstance(body, dict) and body.get("tools"):
                return body
        return None
    ft = {n: first_tools(n) for n in M}
    sk = {n: ft[n] is not None and "zygo:zygo-cli" in json.dumps(ft[n]) for n in M}
    ok = bool(mn["ctl"]) and all(mn[n] == mn["ctl"] for n in M) and all(sk.values())
    sub("PR2", "2.5", ok, "--plugin-dir: /zygo: menu entries: ctl %d, the same in ana: %s, in ben: %s; the skill zygo:zygo-cli in the "
        "member's first request with tools: %s; menu: %s"
        % (len(mn["ctl"]), yn(mn["ana"] == mn["ctl"]), yn(mn["ben"] == mn["ctl"]), ", ".join("%s %s" % (n, yn(sk[n])) for n in M), " ".join(mn["ctl"])))

    rd = {n: result_of(n, "toolu_pr_%s_00_read" % n) for n in M}
    rrow = {n: [t for t in tr[n] if t.get("tool") == "Read"] for n in M}
    marker = {n: bool(rd[n][0]) and bool(re.search(r"zygo (trimmed|shaped) this output", rd[n][0] or ""))
              and any(t.get("result_id") and t["result_id"] in (rd[n][0] or "") for t in rrow[n]) for n in M}
    ok = all(rrow[n] and marker[n] for n in M)
    sub("PR2", "2.6", ok, "PostToolUse (zygo's trim hook): the Read of a file in the member's folder gave a trims row for the member's "
        "session: " + ", ".join("%s %d" % (n, len(rrow[n])) for n in M) + "; the Read's tool result, in a request from the member's launcher, "
        "is zygo's view with that row's result id: " + ", ".join("%s %s" % (n, yn(marker[n])) for n in M))

    # Claude Code 2.1.332 defers MCP tools: their names are listed in the messages for ToolSearch, or sent as tools.
    zt = {n: sorted(set(re.findall(r"mcp__zygo__[a-z_]+", json.dumps(ft[n])))) if ft[n] else [] for n in M}
    ok = bool(zt["ctl"]) and all(zt[n] == zt["ctl"] for n in M)
    sub("PR2", "2.7", ok, "--mcp-config: zygo's MCP tools named in the member's first request with tools (as tools, or in the deferred "
        "tool list for ToolSearch): ctl %d, the same in ana: %s, in ben: %s (%s)"
        % (len(zt["ctl"]), yn(zt["ana"] == zt["ctl"]), yn(zt["ben"] == zt["ctl"]), " ".join(zt["ctl"]) or "none"))

    down = load(os.path.join(run, "down.json"), {}).get("t")
    uend = {n: len(glob.glob(os.path.join(run, "userhooks-%s" % ("ctl" if n == "ctl" else TAG[n]), "*-SessionEnd-*.json"))) for n in M}
    aw = {n: [e for e in act if e.get("m") == n and e.get("ev") == "write" and down is not None and e.get("t", 0) >= down] for n in M}
    ok = down is not None and all(uend[n] >= 1 and aw[n] for n in M)
    sub("PR2", "2.8", ok, "SessionEnd at zygo down: the folder's own SessionEnd hook ran: " + ", ".join("%s %d" % (n, uend[n]) for n in M)
        + "; zygo's session-end hook rewrote activity.json after zygo down started (no other writer then: the turn had ended, "
        "and zygo down only reads it): " + ", ".join("%s %s" % (n, yn(bool(aw[n]))) for n in M))

    uh = {n: len(glob.glob(os.path.join(run, "userhooks-%s" % ("ctl" if n == "ctl" else TAG[n]), "*-UserPromptSubmit-*.json"))) for n in M}
    note("PR2", "2.9", "each folder's own UserPromptSubmit hook ran beside zygo's: " + ", ".join("%s %d" % (n, uh[n]) for n in M)
         + "; tools_running values in activity.json writes during the turn: " + "; ".join("%s %s" % (n, b[n][3]) for n in M))

    with open(os.path.join(run, "status-rows.json"), "w") as f:
        json.dump({n: sr[n] for n in M}, f, indent=1, ensure_ascii=False)
    return out


if __name__ == "__main__":
    print("\n".join(main(*sys.argv[1:4])))

#!/bin/sh
# Codex resume and update part (node PC-6): PC10 and PC16 of K§6.3.7, against the mock with no zygo
# between (K-D25). PC10: the SessionStart hook's session_id against the rollout id, and
# `codex resume <id>` (the TUI, on a pseudo-terminal) in a new CODEX_HOME whose sessions folder is a
# symbolic link to a shared scratch store (K-D22); `codex exec resume` and a CODEX_HOME with no link
# are recorded beside it. PC16: check_for_update_on_startup at its default and the release-source
# request at a plain TUI start (run e): the CONNECT records of run e that are absent from a control
# TUI run with the key false (run f) and from the exec runs, refused by the mock.
# Six runs (a..f) in one run folder against one mock with no rules (the default "ok" reply); each
# TUI run passes --no-daemon (no shared background server outside the run folder).
# Writes fixtures/codex/workspace/, fixtures/codex/update/ and fixtures/codex/runs/resume/; one UUID
# mapping (uuid-NNNN) covers all of them, so equal placeholders are equal ids. Prints the two
# "Observed" lines of docs/work/phase4/nodes/PC-6.md last. Exit 0 whatever PC10 and PC16 measured;
# 1 on a harness failure ("codex resume: <why>", a run the harness refused among them); 3 on a
# mock abort. Wait bounds (0.5 s polls up to a limit) are observation limits, not assertions (K-C5).
# It kills only processes whose command line holds its own $RUN/codex-pkg.
PREMISES_DIR=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck disable=SC2034  # read by lib.sh
N2_AGENTS=codex
# shellcheck disable=SC1091  # lib.sh is linted on its own
. "$PREMISES_DIR/lib.sh"
T1=pc10-tracer-one T2=pc10-tracer-two T3=pc10-tracer-three T4=pc10-tracer-four T5=pc16-tracer-five
T6=pc16-tracer-six
PTY_PID=""

# pc6_cleanup: stop the pseudo-terminal helper and any Codex of this run folder (traps call it).
pc6_cleanup() {
  [ -z "${PTY_PID:-}" ] || kill "$PTY_PID" 2>/dev/null || true
  pkill -f "$RUN/codex-pkg" 2>/dev/null || true
}

new_run codex-resume
n2_trap_chain pc6_cleanup
start_mock
STORE=$RUN/store/sessions
RR=$(cd "$R" && pwd -P)
mkdir -p "$STORE" "$RUN/hooks" "$RUN/hooks.d" "$RUN/out/workspace" "$RUN/out/update"
printf '%s\n' '#!/bin/sh' "cat >\"\$(mktemp \"$RUN/hooks/XXXXXX\")\"" >"$RUN/hook.sh"
chmod 755 "$RUN/hook.sh"
: >"$RUN/marks.txt"

# pc6_home <name> <link 0|1> [update-key off: 1]: CX_HOME=$RUN/<name> with its config.toml; with
# link 1, sessions -> $STORE; with a third argument 1, check_for_update_on_startup = false (the
# control of run e), else the key is absent.
pc6_home() {
  CX_HOME=$RUN/$1
  mkdir -p "$CX_HOME"
  [ "$2" = 0 ] || ln -s "$STORE" "$CX_HOME/sessions"
  _k=""
  [ "${3:-0}" = 0 ] || _k='check_for_update_on_startup = false'
  printf '%s\n' 'model = "gpt-mock"' 'model_provider = "mock"' ${_k:+"$_k"} '' \
    '[model_providers.mock]' 'name = "mock"' "base_url = \"$MOCK_URL/v1\"" 'env_key = "MOCK_API_KEY"' \
    'wire_api = "responses"' '' '[features]' 'codex_hooks = true' 'hooks = true' '' \
    "[projects.\"$RR\"]" 'trust_level = "trusted"' '' '[[hooks.SessionStart]]' '[[hooks.SessionStart.hooks]]' \
    'type = "command"' "command = \"$RUN/hook.sh\"" >"$CX_HOME/config.toml"
}

# pc6_seq: the highest mock record number so far (0 when none).
pc6_seq() {
  _s=$(find "$RUN/rec" -name '[0-9]*.json' | sed -n 's|.*/0*\([0-9][0-9]*\)\.json$|\1|p' | sort -n | tail -n 1)
  echo "${_s:-0}"
}

# pc6_mark <name> <before|after>: "<name> <before|after> <highest record seq> <connect lines>" to
# marks.txt; after a run, its hook files move to hooks.d/<name>/.
pc6_mark() {
  _c=0
  [ ! -e "$RUN/rec/connect.txt" ] || _c=$(wc -l <"$RUN/rec/connect.txt" | tr -d ' ')
  echo "$1 $2 $(pc6_seq) $_c" >>"$RUN/marks.txt"
  if [ "$2" = after ]; then
    mkdir -p "$RUN/hooks.d/$1"
    find "$RUN/hooks" -type f -exec mv {} "$RUN/hooks.d/$1/" \;
  fi
}

# pc6_used: the number of run_codex records in cx-used.d (run_codex writes one after codex_env
# accepts the run and before Codex starts).
pc6_used() { find "$RUN/cx-used.d" -type f 2>/dev/null | wc -l | tr -d ' '; }

# pc6_wait <name> <polls> <used before> <output file>: wait for CX_PID up to <polls> 0.5 s polls,
# then kill this run folder's Codex (_rc=killed); else _rc = its status. The harness refused the
# run (in the background subshell) when it left no new cx-used.d record (n2_daemon_flag, codex_env)
# or its output holds a "premises:" line (also run_codex's daemon check after the record; Codex
# never prints one): exit 1.
pc6_wait() {
  _i=0 _rc=""
  while kill -0 "$CX_PID" 2>/dev/null; do
    _i=$((_i + 1))
    if [ "$_i" -gt "$2" ]; then pkill -f "$RUN/codex-pkg" 2>/dev/null || true; _rc=killed; break; fi
    sleep 0.5
  done
  _w=0
  wait "$CX_PID" || _w=$?
  [ -n "$_rc" ] || _rc=$_w
  check_abort
  _why=$(tr -d '\r' <"$4" | grep -a '^premises:' | tail -n 1) || true
  if [ "$(pc6_used)" -le "$3" ] || [ -n "$_why" ]; then
    echo "codex resume: run $1: the harness refused it before Codex started: $_why"
    exit 1
  fi
}

# pc6_exec <name> <codex args...>: one non-interactive run, bounded (240 polls).
pc6_exec() {
  _n=$1; shift
  pc6_mark "$_n" before
  _u=$(pc6_used)
  run_codex "$@" </dev/null >"$RUN/$_n.out" 2>"$RUN/$_n.err" &
  CX_PID=$!
  pc6_wait "$_n" 240 "$_u" "$RUN/$_n.out"
  pc6_mark "$_n" after
  echo "$_n exit $_rc" >>"$RUN/marks.txt"
}

# pc6_seen <after seq> <tracer>: 0 when a record after that seq is a /v1/responses request that
# holds the tracer.
pc6_seen() {
  python3 -I - "$RUN/rec" "$1" "$2" <<'PY'
import glob, json, os, sys
d, after, tracer = sys.argv[1], int(sys.argv[2]), sys.argv[3]
for p in glob.glob(os.path.join(d, "[0-9]*.json")):
    try:
        r = json.load(open(p))
    except ValueError:
        continue
    if r.get("seq", 0) > after and str(r.get("path", "")).split("?")[0].endswith("/v1/responses") \
            and tracer in json.dumps(r.get("body"), ensure_ascii=False):
        sys.exit(0)
sys.exit(1)
PY
}

# pc6_tui <name> <tracer> <codex args...>: one TUI run on a pseudo-terminal. The helper holds the
# master side, records the output in <name>.raw and answers the cursor-position (ESC [ 6 n) and
# device-attribute (ESC [ c) queries. <name>.stop makes it send F2 (the warnings panel; the raw
# offset at that point goes to <name>.f2), Esc, three Ctrl-C with drains between, and exit.
pc6_tui() {
  _n=$1 _t=$2; shift 2
  rm -f "$RUN/$_n.pty" "$RUN/$_n.stop" "$RUN/$_n.f2"
  python3 -I - "$RUN/$_n.pty" "$RUN/$_n.raw" "$RUN/$_n.stop" "$RUN/$_n.f2" <<'PY' &
import fcntl, os, select, struct, sys, termios, time
pty_file, raw, stop, f2 = sys.argv[1:5]
m, s = os.openpty()
fcntl.ioctl(s, termios.TIOCSWINSZ, struct.pack("HHHH", 50, 200, 0, 0))
with open(pty_file + ".tmp", "w") as f:
    f.write(os.ttyname(s) + "\n")
os.rename(pty_file + ".tmp", pty_file)
tail = b""
ANSWERS = ((b"\x1b[6n", b"\x1b[1;1R"), (b"\x1b[c", b"\x1b[?1;2c"))


def pump(out, t):
    global tail
    r, _, _ = select.select([m], [], [], t)
    if not r:
        return
    try:
        d = os.read(m, 65536)
    except OSError:
        time.sleep(t)
        return
    if not d:
        return
    out.write(d)
    out.flush()
    buf = tail + d
    for q, a in ANSWERS:
        i = buf.find(q)
        while i != -1:
            if i + len(q) > len(tail):  # not answered with the earlier read
                os.write(m, a)
            i = buf.find(q, i + 1)
    tail = buf[-16:]


def drain(out, secs):
    end = time.time() + secs
    while time.time() < end:
        pump(out, 0.2)


start = time.time()
with open(raw, "ab") as out:
    while not os.path.exists(stop) and time.time() - start < 300:
        pump(out, 0.2)
    with open(f2, "w") as f:
        f.write("%d\n" % out.tell())
    for key, secs in ((b"\x1bOQ", 2), (b"\x1b", 0.5), (b"\x03", 1), (b"\x03", 1), (b"\x03", 3)):
        os.write(m, key)
        drain(out, secs)
os.close(s)
os.close(m)
PY
  PTY_PID=$!
  _i=0
  while [ ! -s "$RUN/$_n.pty" ]; do
    _i=$((_i + 1)); [ $_i -le 20 ] || { echo "codex resume: no pseudo-terminal"; exit 1; }
    sleep 0.5
  done
  PTY=$(cat "$RUN/$_n.pty")
  pc6_mark "$_n" before
  _b=$(pc6_seq)
  _u=$(pc6_used)
  # shellcheck disable=SC2094  # the terminal is Codex's input and output
  run_codex "$@" <"$PTY" >"$PTY" 2>&1 &
  CX_PID=$!
  _i=0 _seen=0
  while [ $_i -lt 180 ]; do
    if pc6_seen "$_b" "$_t"; then _seen=1; break; fi
    kill -0 "$CX_PID" 2>/dev/null || break
    sleep 0.5; _i=$((_i + 1))
  done
  if [ $_seen = 1 ]; then
    _i=0
    while [ $_i -lt 20 ] && ! grep -rqF "$_t" "$CX_HOME/sessions/" 2>/dev/null; do sleep 0.5; _i=$((_i + 1)); done
  fi
  touch "$RUN/$_n.stop"
  # The helper's stop keys take 7.5 s; 40 polls leave Codex 20 s to end.
  pc6_wait "$_n" 40 "$_u" "$RUN/$_n.raw"
  wait "$PTY_PID" || true
  PTY_PID=""
  pc6_mark "$_n" after
  echo "$_n exit $_rc" >>"$RUN/marks.txt"
}

pc6_home cx-a 1
pc6_exec a exec --json --dangerously-bypass-hook-trust "Reply ok. $T1"
ID=$(find "$STORE" -name 'rollout-*.jsonl' |
  sed -n 's/.*-\([0-9a-f]\{8\}-[0-9a-f]\{4\}-[0-9a-f]\{4\}-[0-9a-f]\{4\}-[0-9a-f]\{12\}\)\.jsonl$/\1/p' | head -n 1)
[ -n "$ID" ] || ID=00000000-0000-0000-0000-000000000000
pc6_home cx-b 1
pc6_exec b exec --json --dangerously-bypass-hook-trust resume "$ID" "Reply ok. $T2"
pc6_home cx-c 1
pc6_tui c "$T3" resume --no-daemon --dangerously-bypass-hook-trust "$ID" "Reply ok. $T3"
pc6_home cx-d 0
pc6_exec d exec --json --dangerously-bypass-hook-trust resume "$ID" "Reply ok. $T4"
pc6_home cx-e 0
pc6_tui e "$T5" --no-daemon --dangerously-bypass-hook-trust "Reply ok. $T5"
pc6_home cx-f 0 1
pc6_tui f "$T6" --no-daemon --dangerously-bypass-hook-trust "Reply ok. $T6"

python3 -I - "$RUN" "$ID" "$T1" "$T2" "$T3" "$T4" "$T5" "$CODEX_FX/config.schema.json" "$T6" <<'PY'
import glob, json, os, re, sys
run, rid, t1, t2, t3, t4, t5, schema, t6 = sys.argv[1:10]
store = os.path.join(run, "store", "sessions")
ws, up = os.path.join(run, "out", "workspace"), os.path.join(run, "out", "update")
LOOP = ("127.0.0.1", "localhost", "[::1]")


def put(d, name, text):
    with open(os.path.join(d, name), "w") as f:
        f.write(text)


win, exits = {}, {}
for line in open(os.path.join(run, "marks.txt")):
    w = line.split()
    if w[1] == "before":
        win[w[0]] = [int(w[2]), int(w[3]), int(w[2]), int(w[3])]
    elif w[1] == "after":
        win[w[0]][2:] = [int(w[2]), int(w[3])]
    elif w[1] == "exit":
        exits[w[0]] = w[2]
recs = []
for p in glob.glob(os.path.join(run, "rec", "[0-9]*.json")):
    try:
        recs.append(json.load(open(p)))
    except ValueError:
        pass
recs.sort(key=lambda r: r.get("seq", 0))
cp = os.path.join(run, "rec", "connect.txt")
conn = open(cp).read().splitlines() if os.path.exists(cp) else []


def requests(n):
    b, _, a, _ = win.get(n, [0, 0, 0, 0])
    return [r for r in recs if b < r.get("seq", 0) <= a
            and str(r.get("path", "")).split("?")[0].endswith("/v1/responses")]


def connects(n):
    _, b, _, a = win.get(n, [0, 0, 0, 0])
    return conn[b:a]


def first_text(n):
    q = requests(n)
    return json.dumps(q[0].get("body"), ensure_ascii=False) if q else ""


def yn(v):
    return "yes" if v else "no"


def hosts(lines):
    return ",".join(x.split()[0] for x in lines) or "none"


# Hooks, per run.
hooks, ss = [], {}
for n in "abcdef":
    ids = []
    for p in sorted(glob.glob(os.path.join(run, "hooks.d", n, "*"))):
        text = open(p, errors="replace").read()
        try:
            pay = json.loads(text)
        except ValueError:
            pay = text
        hooks.append({"run": n, "payload": pay})
        if isinstance(pay, dict) and pay.get("hook_event_name", "SessionStart") == "SessionStart" and pay.get("session_id"):
            ids.append(str(pay["session_id"]))
    ss[n] = ids

# The rollout of run a.
rollouts = sorted(glob.glob(os.path.join(store, "**", "rollout-*.jsonl"), recursive=True))
meta_line, meta_id = "", ""
for p in rollouts:
    if p.endswith("-" + rid + ".jsonl"):
        with open(p) as f:
            meta_line = f.readline().strip()
        try:
            meta_id = str(json.loads(meta_line).get("payload", {}).get("id", ""))
        except (ValueError, AttributeError):
            meta_id = ""
        break
thread_id = ""
for line in open(os.path.join(run, "a.out"), errors="replace"):
    try:
        e = json.loads(line)
    except ValueError:
        continue
    if isinstance(e, dict) and e.get("type") == "thread.started":
        thread_id = str(e.get("thread_id", ""))
        break

nil = "00000000-0000-0000-0000-000000000000"
put(ws, "ids.txt", "rollout_id=%s\nrollout_meta_id=%s\nexec_thread_id=%s\n" % (rid, meta_id or "none", thread_id or "none")
    + "".join("session_start_%s=%s\n" % (n, ",".join(ss[n]) or "none") for n in "abcdef"))

in_store = False
for d, _, files in os.walk(store):
    for f in files:
        try:
            if t3 in open(os.path.join(d, f), errors="replace").read():
                in_store = True
        except OSError:
            pass
links = all(os.path.islink(os.path.join(run, c, "sessions"))
            and os.path.realpath(os.path.join(run, c, "sessions")) == os.path.realpath(store)
            for c in ("cx-a", "cx-b", "cx-c"))
eq = bool(ss["a"]) and rid != nil and all(i == rid for i in ss["a"]) and meta_id == rid
s = {
    "session_start_fired_a": yn(glob.glob(os.path.join(run, "hooks.d", "a", "*"))),
    "session_id_equals_rollout_id": yn(eq),
    "exec_thread_id_equals_rollout_id": yn(rid != nil and thread_id == rid),
    "session_start_c_equals_rollout_id": yn(bool(ss["c"]) and rid != nil and all(i == rid for i in ss["c"])),
    "exec_resume_exit": exits.get("b", "none"),
    "exec_resume_history": yn(t1 in first_text("b") and t2 in first_text("b")),
    "tui_resume_exit": exits.get("c", "none"),
    "tui_resume_history": yn(t1 in first_text("c") and t3 in first_text("c")),
    "tui_resume_in_store": yn(in_store),
    "links_kept": yn(links),
    "store_rollouts": str(len(rollouts)),
    "no_link_resume_exit": exits.get("d", "none"),
    "no_link_resume_history": yn(t1 in first_text("d") and t4 in first_text("d")),
}
s["pc10"] = "verified" if s["tui_resume_exit"] == "0" and all(s[k] == "yes" for k in (
    "session_id_equals_rollout_id", "exec_thread_id_equals_rollout_id", "session_start_c_equals_rollout_id",
    "tui_resume_history", "tui_resume_in_store", "links_kept")) else "failed"
put(ws, "summary.txt", "".join("%s=%s\n" % kv for kv in s.items()))
put(ws, "hooks.jsonl", "".join(json.dumps(h, ensure_ascii=False) + "\n" for h in hooks))
if meta_line:
    put(ws, "rollout-meta.json", meta_line + "\n")
for n in "abcd":
    q = requests(n)
    if q:
        put(ws, "request-%s.json" % n, json.dumps(q[0], indent=2, ensure_ascii=False) + "\n")

out = []
for top in ("cx-a", "cx-b", "cx-c", "cx-d", "cx-e", "cx-f", "store"):
    root = os.path.join(run, top)
    rows = []
    for d, dirs, files in os.walk(root):
        for n in dirs + files:
            p = os.path.join(d, n)
            rel = top + "/" + os.path.relpath(p, root)
            if os.path.islink(p):
                rows.append(rel + (" -> store" if os.path.realpath(p) == os.path.realpath(store) else " -> other"))
            elif n in files:
                rows.append(rel)
    out += sorted(rows)
put(ws, "codex-home-files.txt", "".join(r + "\n" for r in out))

ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b.", re.S)


def screen(n, part):
    """The screen text of run n before F2 (part 0) or from F2 on (part 1: the warnings panel)."""
    p = os.path.join(run, n + ".raw")
    t = open(p, "rb").read() if os.path.exists(p) else b""
    try:
        cut = int(open(os.path.join(run, n + ".f2")).read().strip())
    except (OSError, ValueError):
        cut = len(t)
    t = (t[:cut] if part == 0 else t[cut:]).decode("utf-8", "replace")
    return ANSI.sub("", t).replace("\r", "")


put(ws, "tui-c.txt", screen("c", 0))
put(ws, "tui-c-warnings.txt", screen("c", 1))
for n in "ef":
    put(up, "tui-%s.txt" % n, screen(n, 0))
    put(up, "tui-%s-warnings.txt" % n, screen(n, 1))
ce, cf = connects("e"), connects("f")
put(up, "connect-e.txt", "".join(x + "\n" for x in ce))
put(up, "connect-f.txt", "".join(x + "\n" for x in cf))
cfg = open(os.path.join(run, "cx-e", "config.toml")).read()
put(up, "config.toml", cfg)
cfg_f = open(os.path.join(run, "cx-f", "config.toml")).read()
put(up, "config-control.toml", cfg_f)


def default_true(o):
    if isinstance(o, dict):
        v = o.get("check_for_update_on_startup")
        if isinstance(v, dict) and "Defaults to `true`" in str(v.get("description", "")):
            return True
        return any(default_true(x) for x in o.values())
    if isinstance(o, list):
        return any(default_true(x) for x in o)
    return False


try:
    sd = "true" if default_true(json.load(open(schema))) else "unknown"
except (OSError, ValueError):
    sd = "unknown"


def counts(n):
    """host:port -> the number of run n's connect lines to it (CONNECT or a proxy request), loopback
    hosts left out (their ports differ per run)."""
    out = {}
    for x in connects(n):
        hp = x.split()[0] if x.split() else ""
        if hp and hp.rpartition(":")[0].lower() not in LOOP:
            out[hp] = out.get(hp, 0) + 1
    return out


def fmt(c):
    return ",".join("%s=%d" % kv for kv in sorted(c.items())) or "none"


exec_lines = connects("a") + connects("b") + connects("d")
cnt = {n: counts(n) for n in "abcdef"}
# The release-source request, by per-request counts (lead's call, 2026-11-25): a host:port that
# run e (key at its default) reaches more often than the control f (key false), with the same
# positive difference in the other key-default TUI run c, and no exec run (a, b, d) reaching it
# more often than f. A CONNECT shows the host only: the path is not visible.
rs, diff = [], {}
for hp in sorted(cnt["e"]):
    dv = cnt["e"][hp] - cnt["f"].get(hp, 0)
    if dv > 0 and cnt["c"].get(hp, 0) - cnt["f"].get(hp, 0) == dv \
            and all(cnt[x].get(hp, 0) - cnt["f"].get(hp, 0) <= 0 for x in "abd"):
        rs.append(hp)
        diff[hp] = dv
PROPOSED = "registry.npmjs.org"  # K§6.5.4: https://registry.npmjs.org/@openai/codex/latest
UPD = r"(?m)^\s*check_for_update_on_startup\s*=\s*(\S+)"
mf = re.search(UPD, cfg_f)
u = {
    "config_key": "present" if re.search(UPD, cfg) else "absent",
    "schema_default": sd,
    "tui_start_exit": exits.get("e", "none"),
    "tui_start_connects": hosts(ce),
    "control_config_key": mf.group(1) if mf else "absent",
    "control_exit": exits.get("f", "none"),
    "control_connects": hosts(cf),
    "exec_connects": hosts(exec_lines),
}
for n in "cefabd":
    u["connect_counts_" + n] = fmt(cnt[n])
u.update({
    "release_source_hosts": ",".join(rs) or "none",
    "release_source_excess": ",".join("%s=+%d" % (h, diff[h]) for h in rs) or "none",
    "release_source_request": "seen" if rs else "not-seen",
    "release_source_path": "not visible (CONNECT)",
    "release_source_proposed": PROPOSED,
    "release_source_matches_proposed": yn(any(h.rpartition(":")[0].lower() == PROPOSED for h in rs)),
})
u["pc16"] = "verified" if (u["config_key"] == "absent" and sd == "true" and u["control_config_key"] == "false"
                           and rs) else "failed"
put(up, "summary.txt", "".join("%s=%s\n" % kv for kv in u.items()))

# One UUID mapping over every output file (workspace, then update; files in name order), before
# scrub: equal placeholders are equal ids across all of this part's fixtures. "uuid-NNNN" is not a
# UUID, so scrub's per-file numbering leaves it alone. A UUID in the run folder's own path is left
# as it is, so scrub still maps that path (to /tmp/run, /home/user, /repo). The bounds are hex-only,
# not \b, so an id behind a word prefix (Codex's msg_<uuid> item ids) is mapped too.
UUID = re.compile(r"(?<![0-9A-Fa-f-])[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}(?![0-9A-Fa-f-])")
keep = set(UUID.findall(run + " " + os.path.realpath(run)))
umap = {}


def uid(m):
    u = m.group(0)
    return u if u in keep else umap.setdefault(u, "uuid-%04d" % (len(umap) + 1))


for d in (ws, up):
    for f in sorted(os.listdir(d)):
        p = os.path.join(d, f)
        put(d, f, UUID.sub(uid, open(p, errors="replace").read()))
PY

stop_mock
for d in workspace update; do
  rm -rf "${CODEX_FX:?}/$d"
  mkdir -p "$CODEX_FX/$d"
  for f in "$RUN/out/$d"/*; do
    [ -e "$f" ] || continue
    scrub "$f" >"$CODEX_FX/$d/${f##*/}"
  done
done
codex_record resume

# pc6_paths <dir>: the fixture paths of its nontrivial files, summary.txt first.
pc6_paths() {
  _l=""
  for f in "$CODEX_FX/$1/summary.txt" "$CODEX_FX/$1"/*; do
    _r=fixtures/codex/$1/${f##*/}
    case " $_l " in *" $_r "*) continue ;; esac
    [ -f "$f" ] && [ "$(wc -c <"$f" | tr -d ' ')" -gt 2 ] || continue
    case "$(tr -d ' \t\r\n' <"$f")" in '{}' | '[]') continue ;; esac
    _l=${_l:+$_l }$_r
  done
  echo "$_l"
}
today=$(date +%Y-%m-%d)
echo "- PC10: $(sed -n 's/^pc10=//p' "$CODEX_FX/workspace/summary.txt") $today: $(pc6_paths workspace)"
echo "  Scope: codex resume <id> (TUI, pseudo-terminal) in a new CODEX_HOME whose sessions links to a shared store; exec resume and a CODEX_HOME with no link recorded beside it."
echo "- PC16: $(sed -n 's/^pc16=//p' "$CODEX_FX/update/summary.txt") $today: $(pc6_paths update)"
_x=$(sed -n 's/^release_source_excess=//p' "$CODEX_FX/update/summary.txt")
if [ "$_x" = none ]; then
  _rs="no controlled count difference (no release-source request seen)"
else
  _w=""
  for _it in $(printf '%s' "$_x" | tr ',' ' '); do
    _h=${_it%=+*} _d=${_it##*=+}
    if [ "$_d" = 1 ]; then _w="${_w:+$_w, }one extra $_h CONNECT"; else _w="${_w:+$_w, }$_d extra $_h CONNECTs"; fi
  done
  _rs="release source inferred from a controlled count difference ($_w when the key is on), path not visible"
fi
echo "  Scope: Codex's own default against the mock with no zygo between (K-D25); $_rs; under a zygo launch: P4-9 and P4-11; the notice text: LC-CX1 (owner-run)."

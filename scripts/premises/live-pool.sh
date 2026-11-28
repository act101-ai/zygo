#!/bin/sh
# Live checks PR1 and PR2 (nodes WD-21, WD-22; workspace definition spec R§19 PR1, PR2, R§9.5), in one run:
# PR1: two pool members of one workspace, on two pool directories logged in to two accounts, run in
#   passthrough and each sends its own directory's login.
# PR2: with CLAUDE_CONFIG_DIR set to a pool directory, zygo's --settings hooks, the status line and
#   --plugin-dir act as with the default directory.
# Usage: sh scripts/premises/live-pool.sh
#   One LIVE PR1 line and one LIVE PR2 line, exit 0. At the end of the run (a failed line included) it copies
#   fixtures/claude-code/pool/pr1.txt, pr2.txt and pr2-panes.txt into the checkout.
#   N2_SCRATCH=<folder> (lib.sh): the run folder stays in <folder>/live-<pid> (the temporary HOME, the recorded
#   requests, the hook logs, the pane text), except the run CA, its keys and the synthetic login files, which
#   are deleted at cleanup on every exit.
# Run by hand only, with claude and opencode on PATH at or above the pinned versions (the caller adds the
# OpenCode folder), tmux 3.2 or later, jq, python3, openssl, sqlite3 and perl, and a release build of zygo
# (cargo build -p zygo --release, or ZYGO_BIN=<path>).
#
# What this run does to the machine:
# - It reads no login of the operator: HOME is a temporary folder in the scratch folder, no CLAUDE_CONFIG_DIR
#   is passed to the tmux server or to zygo, and each of the three Claude Code config folders ($H/pool/a,
#   $H/pool/b and the default $H/.claude) gets its own synthetic claude.ai login file (an access token of
#   the claude.ai form built at run time from random bytes, expiry 30 days ahead, so no refresh is tried).
#   Only the sha256 prefixes of the three "Bearer <token>" values leave the python step that makes them.
# - No request leaves the machine. The tmux server and zygo get HTTPS_PROXY = tls_recorder.py on loopback
#   and SSL_CERT_FILE = a CA made for this run only. The recorder opens only CONNECT api.anthropic.com:443,
#   terminates TLS with a leaf signed by that CA, records the sha256 prefix of each Authorization value and
#   the client process, and forwards /v1/messages to mock_upstream.py with the dummy key (authored replies).
#   Every other CONNECT host gets 403 (recorded). Claude Code's own direct calls go to the same proxy: they
#   fail the TLS handshake unless its runtime trusts the run CA through SSL_CERT_FILE; either way they are
#   recorded by client process and kept apart from the members' launchers.
# - The run CA, its key, the leaf key and the three synthetic login files are deleted at cleanup on every
#   exit, also with N2_SCRATCH set.
# - Every tmux command uses a server of its own (-S <socket in a private folder>, -f /dev/null). zygo starts
#   one workspace (folder repo) with three claude-code members on that server, each in its own window:
#   ana and ben (pool = true; pool $H/pool) and ctl (passthrough = true, the default config folder). At the
#   end zygo down --now --wait and zygo ws forget remove it, then the server, the recorder, the mock and the
#   activity watcher are ended and the private folder is removed.
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
. "$PREMISES_DIR/workspace_lib.sh"   # clean, sub, note, rest, pw_ticks, jv, finish
live_traps
set +e   # a failed command is a result here; every wait and every test handles its own status
ZP_PRIV="" SOCK="" TMUX_BIN="" BASE="" STARTED=0 SP="" name="" WS="" WJ="" U="" REAL_HOME=$HOME
REAL_CCD=$(printenv CLAUDE_CONFIG_DIR || true)
MEMBERS="ana ben ctl"
tm() { LC_ALL=C "$TMUX_BIN" -S "$SOCK" -f /dev/null "$@"; }
# secrets_end: the run CA, the keys and the synthetic login files (every exit path).
secrets_end() {
  [ -z "${RUN:-}" ] || { rm -rf "$RUN/tls"; for _d in "$RUN/home/pool/a" "$RUN/home/pool/b" "$RUN/home/.claude"; do rm -f "$_d/.credentials.json"; done; }
}
# pool_end <status>: the workspace when one was started, the private server, every process this script
# started, the run secrets, the private folder; then live_end.
pool_end() {
  trap - EXIT INT TERM HUP PIPE
  [ "$STARTED" -eq 0 ] || rclean
  [ -z "$SOCK" ] || tm kill-server 2>/dev/null
  for _p in $LIVE_PIDS; do kill "$_p" 2>/dev/null; done
  [ -z "$LIVE_PIDS" ] || sleep 1
  for _p in $LIVE_PIDS; do kill -KILL "$_p" 2>/dev/null; done
  secrets_end
  [ -z "$ZP_PRIV" ] || rm -rf "$ZP_PRIV"
  live_end "$1"
}
trap 'pool_end $?' EXIT; trap 'pool_end 129' HUP; trap 'pool_end 130' INT; trap 'pool_end 141' PIPE; trap 'pool_end 143' TERM

TMUX_BIN=$(command -v tmux) || { echo "live: tmux is not on PATH"; exit 1; }
TVL=$("$TMUX_BIN" -V)
TV=$(printf '%s\n' "$TVL" | sed -n 's/^tmux //p')
ver_ge "${TV%[a-z]}" 3.2 || { echo "live: tmux is ${TV:-unknown}; the minimum is 3.2 (AU7); put tmux 3.2 or later first on PATH"; exit 1; }
for _t in jq python3 perl openssl sqlite3 git; do command -v "$_t" >/dev/null || { echo "live: $_t is not on PATH"; exit 1; }; done
[ -z "$LIVE_KEYS" ] || { echo "live: this check takes no key"; exit 1; }
pinned_agents
case "$N2_SCRATCH$CLAUDE_BIN$OPENCODE_BIN$HOME$ZYGO_BIN$PREMISES_DIR" in
  *"'"* | *" "* | *'"'* | *\\*) echo "live: a path holds a quote, a backslash or a space; this script cannot use it"; exit 1 ;;
esac
{ [ -f "$ZYGO_BIN" ] && [ -x "$ZYGO_BIN" ]; } || { echo "live: ZYGO_BIN is not an executable file; build zygo or set ZYGO_BIN"; exit 1; }
ZP_PRIV=$(mktemp -d "${TMPDIR:-/tmp}/zpool.XXXXXX") || exit 1
SOCK="$ZP_PRIV/s"
[ ${#SOCK} -le 100 ] || { echo "live: the socket path is longer than 100 characters; set TMPDIR to a short directory"; exit 1; }
ZV=$("$ZYGO_BIN" --version 2>/dev/null | sed -n 1p)
echo "live: Claude Code $LIVE_CC_VER (pin $PIN_CC), ${ZV:-zygo ?}, $TVL"

BASE="$N2_SCRATCH/live-$$"
REC="$BASE/pool-rec"; FXT="$BASE/fx"; mkdir -p "$REC" "$FXT"
OUT="$REPO_DIR/fixtures/claude-code/pool"
PW_TICKS=$(pw_ticks)
MAIN='"no_header": ["x-claude-code-agent-id"], "extra": {"body": "\"tools\": \\[\\{"}'
MARK="[REDACT""ED:live]"   # the redaction marker form of OC-THROW, built from two parts

# --- helpers (bodies of live-mail.sh) ---------------------------------------------------------
wait_for() { _w=$(($1 * PW_TICKS)); shift; while ! "$@" >/dev/null 2>&1; do _w=$((_w - 1)); [ "$_w" -gt 0 ] || return 1; sleep 0.1; done; }
nap() { _n=$(($1 * PW_TICKS)); while [ "$_n" -gt 0 ]; do sleep 0.1; _n=$((_n - 1)); done; }
steps() {
  mkdir -p "$RUN/sse"
  printf '%s\n' "$2" >"$RUN/steps-$1.json"
  python3 -I "$PREMISES_DIR/steps.py" "$RUN/sse" "$1" "$RUN/rules.json" "$RUN/steps-$1.json"
}
PTR=$(printf '\342\235\257')   # U+276F, the pointer of a Claude Code menu
TRUST_YES="Yes, I trust"
trust_line() { tm capture-pane -p -t "$1" 2>/dev/null | grep -F "$TRUST_YES" | tail -n 1; }
cc_trust() {
  case "$(trust_line "$1")" in
    "") ;;
    *"$PTR"*) sleep 0.5; case "$(trust_line "$1")" in *"$PTR"*) tm send-keys -t "$1" Enter; sleep 1 ;; esac ;;
    *) tm send-keys -t "$1" Down; sleep 1 ;;
  esac
}
say() { tm send-keys -t "$1" -l "$2"; sleep 0.5; tm send-keys -t "$1" Enter; }
now() { python3 -I -c 'import time; print("%.3f" % time.time())'; }
wait_port() { _i=0; while [ ! -s "$1" ]; do _i=$((_i + 1)); [ $_i -lt 100 ] || return 1; sleep 0.1; done; }

# penv <cmd>: the environment of the tmux server and of zygo: temporary HOME, no CLAUDE_CONFIG_DIR, the recorder
# as HTTPS proxy (loopback exempt), the run CA as the only trusted CA.
penv() { env -i PATH="$PB" HOME="$H" USER="$U" LOGNAME="$U" SHELL=/bin/sh TERM=xterm-256color LANG=C.UTF-8 TMPDIR="$RUN/tmp" \
  HTTPS_PROXY="http://127.0.0.1:$RP" https_proxy="http://127.0.0.1:$RP" NO_PROXY=127.0.0.1,localhost,::1 no_proxy=127.0.0.1,localhost,::1 \
  SSL_CERT_FILE="$RUN/tls/ca.pem" DISABLE_AUTOUPDATER=1 ZYGO_NO_UPDATE_CHECK=1 ZYGO_CONFIG="$RUN/zygo.toml" "$@"; }
wsphase() { (cd "$R" && penv "$ZYGO_BIN" workspace) 2>/dev/null | sed -n '1s/^\(zygo: \)\{0,1\}workspace [^:]*: //p'; }
rclean() {
  [ "$(wsphase)" = down ] || (cd "$R" && penv TMUX="$SOCK,$SP,0" "$ZYGO_BIN" down --now --wait -- "$name") >>"$RUN/clean.txt" 2>&1
  penv "$ZYGO_BIN" ws forget -- "$name" >>"$RUN/clean.txt" 2>&1
  tm kill-server 2>/dev/null
  STARTED=0
}
mval() { jq -r --arg k "$1" ".members[\$k].$2 // empty" "$WJ" 2>/dev/null; }   # <pane key> <field>
key_of() { jq -r --arg n "$1" '[.panes[] | select(.kind == "claude-code" and .name == $n) | .key] | if length == 1 then .[0] else empty end' "$WJ" 2>/dev/null; }
ready() { for _m in $MEMBERS; do eval "_k=\$K_$_m"; [ -n "$(mval "$_k" agent_session)" ] || return 1; done; }
busy_seen() { grep -q "\"m\": \"$1\", \"busy\": $2" "$RUN/activity.jsonl" 2>/dev/null; }   # <member> <true|false>
busy_after() { python3 -I - "$RUN/activity.jsonl" "$1" "$2" <<'PY'
import json, sys
f, m, t0 = sys.argv[1], sys.argv[2], float(sys.argv[3])
ev = [json.loads(x) for x in open(f)] if f else []
ev = [e for e in ev if e["m"] == m and e["t"] >= t0]
t = next((e["t"] for e in ev if e["busy"]), None)
sys.exit(0 if t is not None and any(not e["busy"] and e["t"] > t for e in ev) else 1)
PY
}
req_has() { [ -n "$(body_has "$1" "$RUN"/rec/[0-9]*.json 2>/dev/null)" ]; }

# --- setup ------------------------------------------------------------------------------------
U=$(id -un)
live_run pool
rm -f "$H/.claude/.claude.json"   # new_run's dummy-key state file; the default folder's state is $H/.claude.json
name=$(basename "$R")
mkdir -p "$H/pool/a" "$H/pool/b" "$RUN/tls" "$RUN/tmp" "$RUN/bin"
PB="$RUN/bin"
for _t in claude git tmux python3 jq sqlite3; do ln -s "$(command -v "$_t")" "$PB/$_t"; done
PB="$PB:/usr/bin:/bin"

# T1, T2, T3: three synthetic logins (hashes only out), the state files, the user settings of each folder.
RR=$(cd "$R" && pwd -P)
python3 -I - "$H" "$R" "$RR" "$RUN/expect.json" "$PREMISES_DIR/hook_log.py" "$RUN" <<'PY'
import hashlib, json, os, secrets, sys, time
h, r, rr, expect, hook, run = sys.argv[1:7]
pfx = "sk-ant" + "-oat01-"
rpfx = "sk-ant" + "-ort01-"
proj = {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True}
state = {"hasCompletedOnboarding": True, "numStartups": 5, "theme": "dark", "projects": {r: proj, rr: proj}}
out = {}
for tag, d, sp in (("a", os.path.join(h, "pool", "a"), os.path.join(h, "pool", "a", ".claude.json")),
                   ("b", os.path.join(h, "pool", "b"), os.path.join(h, "pool", "b", ".claude.json")),
                   ("c", os.path.join(h, ".claude"), os.path.join(h, ".claude.json"))):
    os.makedirs(d, exist_ok=True)
    tok = pfx + "pr" + tag + secrets.token_hex(44)
    cred = {"claudeAiOauth": {"accessToken": tok, "refreshToken": rpfx + "pr" + tag + secrets.token_hex(40),
            "expiresAt": int(time.time() * 1000) + 30 * 86400 * 1000,
            "scopes": ["user:inference", "user:profile", "user:sessions:claude_code"],
            "subscriptionType": "max", "rateLimitTier": "default_claude_max_5x"}}
    p = os.path.join(d, ".credentials.json")
    fd = os.open(p, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        json.dump(cred, f)
    out[tag] = hashlib.sha256(("Bearer " + tok).encode()).hexdigest()[:12]
    json.dump(state, open(sp, "w"))
    name = "ctl" if tag == "c" else tag
    settings = {"statusLine": {"type": "command", "command": "printf USER-SL-" + name},
                "hooks": {"UserPromptSubmit": [{"hooks": [{"type": "command",
                          "command": "python3 -I %s %s/userhooks-%s" % (hook, run, name)}]}]}}
    json.dump(settings, open(os.path.join(d, "settings.json"), "w"))
json.dump(out, open(expect, "w"))
PY
# T4: the run CA and the leaf for api.anthropic.com (deleted at cleanup)
(
  cd "$RUN/tls" || exit 1
  openssl req -x509 -newkey rsa:2048 -nodes -days 2 -subj "/CN=zygo live pool CA" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign" -keyout ca-key.pem -out ca.pem &&
  openssl req -newkey rsa:2048 -nodes -subj "/CN=api.anthropic.com" -keyout leaf-key.pem -out leaf.csr &&
  printf 'subjectAltName=DNS:api.anthropic.com\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n' >ext &&
  openssl x509 -req -in leaf.csr -CA ca.pem -CAkey ca-key.pem -CAcreateserial -days 2 -extfile ext -out leaf.pem
) >"$RUN/openssl.log" 2>&1 || { echo "live: openssl could not make the run CA"; exit 1; }
# T5: the mock with one scripted turn per member: a Write of the redaction marker (zygo's PreToolUse guard
# denies it), then a text reply.
for m in $MEMBERS; do
  steps "pr-$m" '{"trigger": "tracer-pr-'"$m"' go", '"$MAIN"', "steps": [{"tool": "Write", "input": {"file_path": "'"$R/pr-$m.txt"'", "content": "'"$MARK"'"}}, {"text": "pr '"$m"' done"}]}'
done
start_mock "$RUN/rules.json"
LIVE_PIDS="$LIVE_PIDS $MOCK_PID"
# T6: the TLS recorder
RECORDER_DUMMY="$DUMMY_KEY" python3 -I "$PREMISES_DIR/tls_recorder.py" --cert "$RUN/tls/leaf.pem" --key "$RUN/tls/leaf-key.pem" \
  --upstream "$MOCK_URL" --out "$RUN/tls-rec.jsonl" --port-file "$RUN/rport" >"$RUN/recorder.log" 2>&1 &
LIVE_PIDS="$LIVE_PIDS $!"
wait_port "$RUN/rport" || { echo "live: the recorder did not start"; exit 1; }
RP=$(cat "$RUN/rport")
# T7: the zygo config: the pool and the loadout (one window per member)
cat >"$RUN/zygo.toml" <<T
[workspace]
pool = "$H/pool"

[loadout.pr]
[[loadout.pr.window]]
name = "ana"
[[loadout.pr.window.pane]]
kind = "claude-code"
name = "ana"
pool = true
args = ["--strict-mcp-config"]
[[loadout.pr.window]]
name = "ben"
[[loadout.pr.window.pane]]
kind = "claude-code"
name = "ben"
pool = true
args = ["--strict-mcp-config"]
[[loadout.pr.window]]
name = "ctl"
[[loadout.pr.window.pane]]
kind = "claude-code"
name = "ctl"
passthrough = true
args = ["--strict-mcp-config"]
T

# --- the run ----------------------------------------------------------------------------------
ALL="PR1:1.1 PR1:1.2 PR1:1.3 PR1:1.4 PR1:1.5 PR2:2.1 PR2:2.2 PR2:2.3 PR2:2.4 PR2:2.5"
part_pool() {
  penv "$TMUX_BIN" -S "$SOCK" -f /dev/null new-session -d -s hold -x 200 -y 50 /bin/sleep 3600 || { rest "tmux did not start the hold session" $ALL; return; }
  SP=$(tm display-message -p '#{pid}')
  tm set-option -g default-size 200x50
  STARTED=1
  (cd "$R" && penv TMUX="$SOCK,$SP,0" "$ZYGO_BIN" up --loadout pr --no-attach) >"$RUN/up.txt" 2>&1; echo $? >"$RUN/up.rc"
  if [ "$(cat "$RUN/up.rc")" -ne 0 ]; then rest "zygo up exit $(cat "$RUN/up.rc"): $(sed -n 1p "$RUN/up.txt")" $ALL; return; fi
  WS=$(tm show-environment -t "=zygo-$name:" ZYGO_WORKSPACE 2>/dev/null | sed -n 's/^ZYGO_WORKSPACE=//p')
  WJ="$H/.local/state/zygo/workspaces/$WS/workspace.json"
  PANES=$(tm list-panes -s -t "=zygo-$name:" -F '#{pane_id} #{pane_pid} #{@zygo-pane}')
  for m in $MEMBERS; do
    k=$(key_of "$m")
    p=$(printf '%s\n' "$PANES" | awk -v k="$k" '$3 == k { print $1; exit }')
    pp=$(printf '%s\n' "$PANES" | awk -v k="$k" '$3 == k { print $2; exit }')
    eval "K_$m=\$k P_$m=\$p PP_$m=\$pp"
    [ -n "$k" ] && [ -n "$p" ] || { rest "zygo up exit 0, but no pane key or tmux pane for $m (workspace id found: $([ -n "$WS" ] && echo yes || echo no))" $ALL; return; }
  done
  _c=$((120 * PW_TICKS / 5))
  until ready; do
    for m in $MEMBERS; do eval "cc_trust \"\$P_$m\""; done
    _c=$((_c - 1)); [ "$_c" -gt 0 ] || break; sleep 0.5
  done
  ready || { rest "timeout 120 s: ana, ben and ctl do not all have an agent session" $ALL; return; }
  # The keys, panes and processes of each member, for the judge.
  : >"$RUN/members.txt"
  for m in $MEMBERS; do
    eval "k=\$K_$m p=\$P_$m pp=\$PP_$m"
    printf '%s %s %s %s %s\n' "$m" "$k" "$p" "$pp" "$(mval "$k" zygo_session)" >>"$RUN/members.txt"
  done
  # The activity watcher: every 20 ms, a line for each change of busy in each member's activity.json.
  _args=""
  for m in $MEMBERS; do eval "_args=\"\$_args $m=$H/.cache/zygo/sessions/\$(mval \"\$K_$m\" zygo_session)/activity.json\""; done
  python3 -I - "$RUN/activity.jsonl" $_args <<'PY' &
import json, os, sys, time
out, pairs = sys.argv[1], [a.split("=", 1) for a in sys.argv[2:]]
last = {}
ppid = os.getppid()
while os.getppid() == ppid:
    for m, p in pairs:
        try:
            b = json.load(open(p)).get("busy")
        except (OSError, ValueError):
            continue
        if last.get(m) != b:
            last[m] = b
            with open(out, "a") as f:
                f.write(json.dumps({"t": round(time.time(), 3), "m": m, "busy": b}) + "\n")
    time.sleep(0.02)
PY
  LIVE_PIDS="$LIVE_PIDS $!"
  # Process snapshot 1 (all members idle): environ and open files of every process under the private server.
  proc_snap "$RUN/proc-1.json"
  nap 5
  for m in $MEMBERS; do
    eval "p=\$P_$m"
    t0=$(now); echo "$m $t0" >>"$RUN/prompts.txt"
    say "$p" "tracer-pr-$m go"
    b1=1; wait_for 60 busy_seen "$m" true && b1=0
    b2=1; [ "$b1" -eq 0 ] && wait_for 120 busy_after "$m" "$t0" && b2=0
    r1=1; wait_for 60 req_has "toolu_pr_${m}_00_write" && r1=0
    echo "$m busy=$b1 idle=$b2 toolresult=$r1 end=$(now)" >>"$RUN/turns.txt"
    nap 3
  done
  proc_snap "$RUN/proc-2.json"
  for m in $MEMBERS; do eval "tm capture-pane -p -t \"\$P_$m\" >\"$RUN/pane-$m.txt\""; done
  # The /zygo: menu of each pane (typed, captured, then cleared).
  for m in $MEMBERS; do
    eval "p=\$P_$m"
    tm send-keys -t "$p" -l "/zygo:"; nap 2
    tm capture-pane -p -t "$p" >"$RUN/menu-$m.txt"
    tm send-keys -t "$p" Escape; sleep 0.5; tm send-keys -t "$p" C-u
  done
  sqlite3 -json "$H/.local/share/zygo/usage.db" "select session, role_served, passthrough, status, agent_version from requests order by ts, rowid" >"$RUN/usage.json" 2>/dev/null
  [ -s "$RUN/usage.json" ] || echo '[]' >"$RUN/usage.json"
  judge
}

# proc_snap <out>: for every process under the private tmux server: its member (the pane whose process it is
# or descends from), whether HOME is the temporary HOME, the
# CLAUDE_CONFIG_DIR value (relative to it), the names of variables whose value names the real config folder,
# and the count of open files under it. Names and counts only; no value of the real folder is written.
proc_snap() {
  python3 -I - "$1" "$SP" "$H" "$RUN/members.txt" "$REAL_HOME/.claude" "$REAL_CCD" <<'PY'
import json, os, sys
out, root, home, members, *bad = sys.argv[1:]
bad = [b for b in bad if b]
panes = {}
for x in open(members):
    n, _k, _p, ppid, _z = x.split()
    panes[int(ppid)] = n
kids, parent = {}, {}
for d in os.listdir("/proc"):
    if d.isdigit():
        try:
            st = open("/proc/%s/stat" % d).read()
            pp = int(st[st.rfind(")") + 2:].split()[1])
            kids.setdefault(pp, []).append(int(d))
            parent[int(d)] = pp
        except (OSError, IndexError, ValueError):
            pass
todo, procs = [int(root)], []
while todo:
    p = todo.pop()
    procs.append(p)
    todo.extend(kids.get(p, []))
rows = []
for p in sorted(procs):
    try:
        env = dict(x.split(b"=", 1) for x in open("/proc/%d/environ" % p, "rb").read().split(b"\0") if b"=" in x)
        argv = [a.decode("utf-8", "replace") for a in open("/proc/%d/cmdline" % p, "rb").read().split(b"\0") if a]
    except OSError:
        continue
    env = {k.decode(): v.decode("utf-8", "replace") for k, v in env.items()}
    files = []
    try:
        for fd in os.listdir("/proc/%d/fd" % p):
            try:
                files.append(os.readlink("/proc/%d/fd/%s" % (p, fd)))
            except OSError:
                pass
    except OSError:
        pass
    ccd = env.get("CLAUDE_CONFIG_DIR")
    q, member = p, None
    while q and q > 1:
        if q in panes:
            member = panes[q]
            break
        q = parent.get(q)
    rows.append({"pid": p, "prog": os.path.basename(argv[0]) if argv else None, "member": member,
                 "home_is_temp": env.get("HOME") == home,
                 "claude_config_dir": None if ccd is None else (os.path.relpath(ccd, home) if ccd.startswith(home + "/") else "OUTSIDE-TEMP-HOME"),
                 "real_dir_vars": sorted(k for k, v in env.items() if any(b in v for b in bad)),
                 "real_dir_files": sum(1 for f in files if any(f.startswith(b) for b in bad))})
json.dump(rows, open(out, "w"), indent=1)
PY
}

# judge: sub-check and note lines of PR1 and PR2 from the run files; printed as <id> <sub> <rc|note> <detail>.
judge() {
  python3 -I - "$RUN" "$H" "$WJ" "$MARK" >"$RUN/judge.txt" <<'PY'
import glob, json, os, re, sys, unicodedata
run, home, wj, mark = sys.argv[1:5]
L = []
def sub(i, s, ok, d): L.append("%s\t%s\t%s\t%s" % (i, s, 0 if ok else 1, d))
def note(i, s, d): L.append("%s\t%s\tnote\t%s" % (i, s, d))
def load(p, dflt):
    try:
        return json.load(open(p))
    except (OSError, ValueError):
        return dflt
def lines(p):
    try:
        return [json.loads(x) for x in open(p) if x.strip()]
    except OSError:
        return []
yn = lambda b: "yes" if b else "no"
M = ["ana", "ben", "ctl"]
mem = {}
for x in open(os.path.join(run, "members.txt")):
    n, k, pane, ppid, zs = x.split()
    mem[n] = {"key": k, "pane": pane, "ppid": int(ppid), "zs": zs}
key2m = {v["key"]: n for n, v in mem.items()}
w = load(wj, {})
ex = load(os.path.join(run, "expect.json"), {})
tag = {"ana": "a", "ben": "b", "ctl": "c"}
hname = {ex.get("a"): "A", ex.get("b"): "B", ex.get("c"): "C"}
up = open(os.path.join(run, "up.txt")).read()
pline = next((l for l in up.splitlines() if "run in passthrough on pool" in l), "")

# PR1 1.1: pool assignment and passthrough
ms = w.get("members", {})
cd = {n: (ms.get(mem[n]["key"], {}).get("config_dir")) for n in M}
lcd = {n: ((ms.get(mem[n]["key"], {}).get("launch") or {}).get("config_dir")) for n in M}
pt = {n: (ms.get(mem[n]["key"], {}).get("launch") or {}).get("passthrough") for n in M}
rel = lambda p: "default" if not p else (os.path.relpath(p, home) if p.startswith(home + "/") else "OUTSIDE-TEMP-HOME")
pnames = re.findall(r"[a-z]+", pline.split(" run in passthrough")[0].replace("zygo:", "")) if pline else []
ok = (pnames == ["ana", "ben"]
      and cd["ana"] == os.path.join(home, "pool", "a") and cd["ben"] == os.path.join(home, "pool", "b") and not cd["ctl"]
      and all(pt[n] is True for n in M))
sub("PR1", "1.1", ok, "zygo up exit 0; line \"%s\"; member config_dir ana=%s ben=%s ctl=%s (launch.config_dir ana=%s ben=%s ctl=%s); launch.passthrough ana=%s ben=%s ctl=%s"
    % (pline.replace(home, "HOME"), rel(cd["ana"]), rel(cd["ben"]), rel(cd["ctl"]), rel(lcd["ana"]), rel(lcd["ben"]), rel(lcd["ctl"]), pt["ana"], pt["ben"], pt["ctl"]))

# PR1 1.2: each member's Claude Code process: CLAUDE_CONFIG_DIR; every process under the server: temp HOME, no real dir
s1, s2 = load(os.path.join(run, "proc-1.json"), []), load(os.path.join(run, "proc-2.json"), [])
cl = {n: [r for r in s1 if r["prog"] == "claude" and r["member"] == n] for n in M}
ccd = {n: sorted({str(r["claude_config_dir"]) for r in cl[n]}) for n in M}
allrows = s1 + s2
temp_all = all(r["home_is_temp"] for r in allrows)
real_vars = sorted({v for r in allrows for v in r["real_dir_vars"]})
real_files = sum(r["real_dir_files"] for r in allrows)
ok = (all(len(cl[n]) >= 1 for n in M) and ccd["ana"] == ["pool/a"] and ccd["ben"] == ["pool/b"] and ccd["ctl"] == ["None"]
      and temp_all and not real_vars and real_files == 0)
sub("PR1", "1.2", ok, "Claude Code processes (one per member: ana %d, ben %d, ctl %d): CLAUDE_CONFIG_DIR ana=%s ben=%s ctl=%s; "
    "processes under the private tmux server, two snapshots (idle before the prompts, after the turns): %d and %d, HOME = the temporary HOME in all: %s; "
    "variables whose value names the real config folder: %s; open files under it: %d"
    % (len(cl["ana"]), len(cl["ben"]), len(cl["ctl"]), "/".join(ccd["ana"]) or "-", "/".join(ccd["ben"]) or "-",
       "unset" if ccd["ctl"] == ["None"] else "/".join(ccd["ctl"]) or "-", len(s1), len(s2), yn(temp_all), ", ".join(real_vars) or "none", real_files))

# PR1 1.3-1.6: the recorder
rec = lines(os.path.join(run, "tls-rec.jsonl"))
reqs = [r for r in rec if r["kind"] == "request"]
mreq = lambda r: r["client_who"] == "launcher" and r["path"].startswith("/v1/messages")
def hashes(rows):
    out = {}
    for r in rows:
        k = hname.get(r["auth_sha256_12"], "other" if r["auth_sha256_12"] else "none")
        out[k] = out.get(k, 0) + 1
    return out
per = {n: [r for r in reqs if mreq(r) and key2m.get(r["pane"]) == n] for n in M}
hs = {n: hashes(per[n]) for n in M}
fmt = lambda h: ", ".join("%s %d" % (k, h[k]) for k in sorted(h)) or "none"
own = {"ana": "A", "ben": "B", "ctl": "C"}
for n, s in (("ana", "1.3"), ("ben", "1.4")):
    me = own[n]
    ok = len(per[n]) >= 1 and set(hs[n]) == {me}
    sub("PR1", s, ok, "%s (pool/%s): %d /v1/messages requests from %s's launcher at the recorder, all with hash(%s) = %s: %s; by hash: %s"
        % (n, tag[n], len(per[n]), n, me, ex.get(tag[n]), yn(ok), fmt(hs[n])))
distinct = len({ex.get("a"), ex.get("b"), ex.get("c")}) == 3
crossA = sum(1 for r in reqs if r["auth_sha256_12"] == ex.get("a") and key2m.get(r["pane"]) not in ("ana",))
crossB = sum(1 for r in reqs if r["auth_sha256_12"] == ex.get("b") and key2m.get(r["pane"]) not in ("ben",))
us = load(os.path.join(run, "usage.json"), [])
zs = {mem[n]["zs"]: n for n in M}
ur = {n: [u for u in us if u.get("session") == mem[n]["zs"]] for n in M}
uok = all(ur[n] and all(u.get("passthrough") == 1 and u.get("status") == 200 for u in ur[n]) for n in M)
roles = sorted({str(u.get("role_served")) for n in M for u in ur[n]})
ok = distinct and crossA == 0 and crossB == 0 and uok
sub("PR1", "1.5", ok, "hash(A), hash(B), hash(C) differ: %s; requests anywhere at the recorder with hash(A) not from ana's launcher: %d, with hash(B) not from ben's launcher: %d; "
    "usage rows ana %d, ben %d, ctl %d, all passthrough = 1 and status 200: %s (role_served: %s)"
    % (yn(distinct), crossA, crossB, len(ur["ana"]), len(ur["ben"]), len(ur["ctl"]), yn(uok), ", ".join(roles) or "none"))
others = [r for r in reqs if not mreq(r)]
oth = {}
for r in others:
    k = "%s %s %s" % (r["client_who"], r["client_prog"], r["path"])
    oth.setdefault(k, []).append(hname.get(r["auth_sha256_12"], "other" if r["auth_sha256_12"] else "none"))
refused = {}
for r in rec:
    if r["kind"] == "refused":
        k = "%s (%s %s)" % (r["target"], r["client_who"], r["client_prog"])
        refused[k] = refused.get(k, 0) + 1
tlse = {}
for r in rec:
    if r["kind"] == "tls_error":
        k = "%s %s, pane %s" % (r["client_who"], r["client_prog"], key2m.get(r["pane"], "-"))
        tlse[k] = tlse.get(k, 0) + 1
conns = {}
for r in rec:
    if r["kind"] == "connect":
        k = "%s %s %s" % (key2m.get(r["pane"], "-"), r["client_who"], r["client_prog"])
        conns[k] = conns.get(k, 0) + 1
note("PR1", "1.6", "ctl (default folder): %d /v1/messages requests from its launcher, by hash: %s; CONNECT api.anthropic.com:443 by member, kind and program: %s; "
     "TLS handshakes that failed: %s; requests not from a launcher or not /v1/messages: %s; refused CONNECT targets: %s"
     % (len(per["ctl"]), fmt(hs["ctl"]), ", ".join("%s %d" % (k, conns[k]) for k in sorted(conns)) or "none",
        ", ".join("%s: %d" % (k, tlse[k]) for k in sorted(tlse)) or "none",
        "; ".join("%s: %s" % (k, fmt({h: v.count(h) for h in set(v)})) for k, v in sorted(oth.items())) or "none",
        ", ".join("%s %d" % (k, refused[k]) for k in sorted(refused)) or "none"))

# PR2 2.1: SessionStart (agent_session)
asess = {n: bool(ms.get(mem[n]["key"], {}).get("agent_session")) for n in M}
sub("PR2", "2.1", all(asess.values()), "SessionStart (zygo's member entry): agent_session in workspace.json: ctl %s, ana %s, ben %s" % (yn(asess["ctl"]), yn(asess["ana"]), yn(asess["ben"])))
# PR2 2.2: UserPromptSubmit and Stop (busy true, then false, after the prompt)
act = lines(os.path.join(run, "activity.jsonl"))
pr = dict(x.split() for x in open(os.path.join(run, "prompts.txt")))
def tf(n):
    t0 = float(pr.get(n, "inf"))
    ev = [e for e in act if e["m"] == n and e["t"] >= t0]
    t1 = next((e["t"] for e in ev if e["busy"]), None)
    t2 = next((e["t"] for e in ev if t1 is not None and not e["busy"] and e["t"] > t1), None)
    return t1, t2, t0
b = {n: tf(n) for n in M}
okb = {n: b[n][0] is not None and b[n][1] is not None for n in M}
sub("PR2", "2.2", all(okb.values()), "UserPromptSubmit and Stop (zygo's activity.json, polled every 20 ms): busy true then false after the prompt: "
    + ", ".join("%s %s (true at +%.2f s, false at +%.2f s)" % (n, yn(okb[n]), (b[n][0] or 0) - b[n][2], (b[n][1] or 0) - b[n][2]) if okb[n] else "%s no" % n for n in M))
# PR2 2.3: PreToolUse (zygo's redaction guard denies the Write of the marker)
recs = []
for f in sorted(glob.glob(os.path.join(run, "rec", "[0-9]*.json"))):
    o = load(f, {})
    recs.append(o)
def tool_result(n):
    tid = "toolu_pr_%s_00_write" % n
    for o in recs:
        body = o.get("body")
        if not isinstance(body, dict):
            continue
        msgs = body.get("messages") or []
        last = json.dumps(msgs[-1]) if msgs else ""
        if tid in last and "tool_result" in last:
            m = re.search(r"marks a secret zygo removed[^\"\\\\]*", last)
            return o.get("headers", {}).get("x-live-pane"), (m.group(0) if m else None), ('"is_error": true' in last)
    return None, None, None
tr = {n: tool_result(n) for n in M}
absent = {n: not os.path.exists(os.path.join(run, "repo", "pr-%s.txt" % n)) for n in M}
texts = {tr[n][1] for n in M}
ok = all(tr[n][1] and absent[n] and key2m.get(tr[n][0]) == n for n in M) and len(texts) == 1
sub("PR2", "2.3", ok, "PreToolUse (zygo's guard): the Write of the redaction marker denied, the tool result holds the guard text: "
    + ", ".join("%s %s" % (n, yn(bool(tr[n][1]))) for n in M) + "; tool result from the member's own launcher: "
    + ", ".join("%s %s" % (n, yn(key2m.get(tr[n][0]) == n)) for n in M) + "; same text for all three: %s (\"%s\"); file absent: %s"
    % (yn(len(texts) == 1), (sorted(t for t in texts if t) or ["none"])[0], ", ".join("%s %s" % (n, yn(absent[n])) for n in M)))
# PR2 2.4: the status line
def status_rows(p):
    try:
        t = open(p).read().splitlines()
    except OSError:
        return []
    idx = [i for i, l in enumerate(t) if re.fullmatch(r"\s*─{20,}\s*", l)]
    rows = [l.rstrip() for l in (t[idx[-1] + 1:] if idx else [])]
    return [r for r in rows if r.strip()]
sr = {n: status_rows(os.path.join(run, "pane-%s.txt" % n)) for n in M}
# digits and the member name masked (the HUD names its member)
mask = lambda rows: [re.sub(r"\b(ana|ben|ctl)\b", "NAME", re.sub(r"[0-9]", "9", r)) for r in rows]
user_sl = {n: "USER-SL" in open(os.path.join(run, "pane-%s.txt" % n)).read() for n in M}
# the HUD writes its logo in mathematical letters; NFKC folds them to ASCII
zrow = {n: any("zygo" in unicodedata.normalize("NFKC", r) for r in sr[n]) for n in M}
same = all(mask(sr[n]) == mask(sr["ctl"]) for n in M)
ok = all(zrow.values()) and same and not any(user_sl.values())
sub("PR2", "2.4", ok, "status line: zygo's HUD row in the pane: ctl %s, ana %s, ben %s; rows below the prompt box equal to ctl's after masking digits and the member name: ana %s, ben %s; "
    "USER-SL (each folder's own statusLine) in a pane: ctl %s, ana %s, ben %s"
    % (yn(zrow["ctl"]), yn(zrow["ana"]), yn(zrow["ben"]), yn(mask(sr["ana"]) == mask(sr["ctl"])), yn(mask(sr["ben"]) == mask(sr["ctl"])),
       yn(user_sl["ctl"]), yn(user_sl["ana"]), yn(user_sl["ben"])))
json.dump({n: sr[n] for n in M}, open(os.path.join(run, "status-rows.json"), "w"), indent=1, ensure_ascii=False)
# PR2 2.5: --plugin-dir: the /zygo: menu and the skill in the first main request
def menu(n):
    try:
        return sorted(set(re.findall(r"/zygo:[A-Za-z0-9_-]+", open(os.path.join(run, "menu-%s.txt" % n)).read())))
    except OSError:
        return []
mn = {n: menu(n) for n in M}
def skill(n):
    for o in recs:
        if key2m.get(o.get("headers", {}).get("x-live-pane")) != n:
            continue
        body = o.get("body")
        if isinstance(body, dict) and body.get("tools"):
            return "zygo:zygo-cli" in json.dumps(body)
    return False
sk = {n: skill(n) for n in M}
ok = bool(mn["ctl"]) and all(mn[n] == mn["ctl"] for n in M) and all(sk.values())
sub("PR2", "2.5", ok, "--plugin-dir: /zygo: menu entries ctl [%s], ana [%s], ben [%s]; the skill zygo:zygo-cli in the member's first request with tools: ctl %s, ana %s, ben %s"
    % (" ".join(mn["ctl"]), " ".join(mn["ana"]), " ".join(mn["ben"]), yn(sk["ctl"]), yn(sk["ana"]), yn(sk["ben"])))
# PR2 2.6: the folder's own user hook merges (note)
uh = {n: len(glob.glob(os.path.join(run, "userhooks-%s" % n, "*-UserPromptSubmit-*.json"))) for n in ("a", "b", "ctl")}
note("PR2", "2.6", "each folder's own UserPromptSubmit hook (settings.json) ran beside zygo's: ctl folder %d input(s), pool/a %d, pool/b %d" % (uh["ctl"], uh["a"], uh["b"]))
print("\n".join(L))
PY
  while IFS="$(printf '\t')" read -r _i _s _r _d; do
    [ -n "$_i" ] || continue
    if [ "$_r" = note ]; then note "$_i" "$_s" "$_d"; else sub "$_i" "$_s" "$_r" "$_d"; fi
  done <"$RUN/judge.txt"
}

part_pool
# The status rows and menus of the three panes as one fixture (digits kept).
python3 -I - "$RUN" >"$RUN/pr2-panes.txt" <<'PY'
import json, os, re, sys
run = sys.argv[1]
try:
    sr = json.load(open(os.path.join(run, "status-rows.json")))
except (OSError, ValueError):
    sr = {}
for n in ("ctl", "ana", "ben"):
    print("== %s: status line rows (below the prompt box)" % n)
    for r in sr.get(n, []):
        print(r)
    try:
        m = [l.rstrip() for l in open(os.path.join(run, "menu-%s.txt" % n)) if "/zygo:" in l]
    except OSError:
        m = []
    print("== %s: lines with /zygo: after typing /zygo:" % n)
    for l in m:
        print(l)
PY
rclean
HEAD="Claude Code $LIVE_CC_VER (pin $PIN_CC), ${ZV:-zygo ?}, $TVL, $(uname -s); upstream: tls_recorder.py for api.anthropic.com:443 (run CA), then mock_upstream.py; synthetic logins"
finish PR1 "$LIVE_CC_VER" "$FXT/pr1.txt" "$HEAD" "1.1 1.2 1.3 1.4 1.5" "$OUT/pr1.txt"
finish PR2 "$LIVE_CC_VER" "$FXT/pr2.txt" "$HEAD" "2.1 2.2 2.3 2.4 2.5" "$OUT/pr2.txt"
scrub "$RUN/pr2-panes.txt" >"$FXT/pr2-panes.txt"
mkdir -p "$OUT" && cp "$FXT/pr1.txt" "$FXT/pr2.txt" "$FXT/pr2-panes.txt" "$OUT"/ || { echo "live: the fixtures could not be copied to the repository"; exit 1; }
live_done "$OUT"

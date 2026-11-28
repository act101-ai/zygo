#!/bin/sh
# Live checks PR1 and PR2 (nodes WD-21, WD-22; workspace definition spec R§19 PR1, PR2, R§9.5), in one run:
# PR1: two pool members of one workspace, on two pool directories logged in to two accounts, run in
#   passthrough and each sends its own directory's login.
# PR2: with CLAUDE_CONFIG_DIR set to a pool directory, zygo's --settings hooks, the status line, --plugin-dir
#   and --mcp-config act as with the default directory.
# Usage: sh scripts/premises/live-pool.sh
#   One LIVE PR1 line and one LIVE PR2 line, exit 0. At the end of the run (a failed line included) it copies
#   fixtures/claude-code/pool/pr1.txt, pr2.txt and pr2-panes.txt into the checkout. The judge is pool_analyze.py.
#   N2_SCRATCH=<folder> (lib.sh): the run folder stays in <folder>/live-<pid> (the temporary HOME, the recorded
#   requests, the hook logs, the pane text), except the run CA, its keys and the synthetic login files, which
#   are deleted at cleanup on every exit.
#   POOL_TEST_PAUSE=<file> (cleanup test only): the first rclean writes <file> and waits until <file> is removed
#   (at most 120 s), so a signal can be sent during it.
# Run by hand only, with claude and opencode on PATH at or above the pinned versions (the caller adds the
# OpenCode folder), tmux 3.2 or later, jq, python3, openssl, sqlite3, git, perl and timeout, and a release build of
# zygo (cargo build -p zygo --release, or ZYGO_BIN=<path>).
#
# What this run does to the machine, and how each limit is enforced:
# - No login of the operator is read. HOME is a temporary folder in the scratch folder; no CLAUDE_CONFIG_DIR is
#   passed to the tmux server or to zygo; each of the three Claude Code config folders ($H/pool/a, $H/pool/b
#   and the default $H/.claude) gets its own synthetic claude.ai login file (an access token of the claude.ai
#   form built at run time from random bytes, expiry 30 days ahead, so no refresh is tried). git reads only a
#   run-local global config (GIT_CONFIG_GLOBAL, GIT_CONFIG_NOSYSTEM=1). ssh, which finds its folder from the
#   passwd entry and not from HOME, never runs: GIT_SSH_COMMAND and GIT_SSH are a wrapper that logs its arguments
#   (ssh-git.log) and exits 1, the ssh on the run's PATH is a second wrapper that does the same (ssh-path.log; Claude
#   Code 2.1.332 calls ssh directly to test GitHub access before it clones its plugin marketplace), and the git config
#   rewrites GitHub's two ssh remote forms (user git, host github.com) to https://github.com/. The judge checks, for
#   every process under the private tmux server, the temporary HOME, the git and ssh variables, and that no variable
#   value and no open file names the operator's config folder (~/.claude and the caller's CLAUDE_CONFIG_DIR).
# - No request leaves the machine in this run. The tmux server, zygo and the members get HTTPS_PROXY =
#   tls_recorder.py on loopback and SSL_CERT_FILE = a CA made for this run only. The recorder opens only
#   CONNECT api.anthropic.com:443, terminates TLS with a leaf signed by that CA, records the sha256 prefix of
#   each Authorization value and the client process, and forwards /v1/messages to mock_upstream.py with the
#   dummy key (authored replies). Every other CONNECT target gets 403 (recorded); git's https remotes go to the
#   same proxy. This is enforced by the environment, not by a network sandbox: a program that ignores
#   HTTPS_PROXY and does not use git could connect directly. The judge checks that the ssh wrapper was never
#   called, that no .git folder under the temporary HOME reached a remote, and that no process under the
#   private tmux server held a TCP connection to a non-loopback address at the two process snapshots.
# - The run CA, its key, the leaf key and the three synthetic login files are made under umask 077 and deleted
#   at cleanup on every exit (first, before the workspace is stopped), also with N2_SCRATCH set.
# - Every tmux command uses a server of its own (-S <socket in a private folder>; the server reads a run-local
#   config: focus-events on, default-size 200x50). zygo starts one workspace (folder repo) with three
#   claude-code members on that server, each in its own window: ana and ben (pool = true; pool $H/pool) and
#   ctl (passthrough = true, the default config folder). At the end zygo down --now --wait (at most 60 s) and
#   zygo ws forget remove it, then the server, the recorder, the mock and the activity watcher are ended and
#   the private folder is removed.
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
. "$PREMISES_DIR/workspace_lib.sh"   # clean, sub, note, rest, pw_ticks, jv, finish
live_traps
set +e   # a failed command is a result here; every wait and every test handles its own status
ZP_PRIV="" SOCK="" TMUX_BIN="" BASE="" STARTED=0 SP="" name="" WS="" WJ="" U="" REAL_HOME=$HOME PAUSED=0
REAL_CCD=$(printenv CLAUDE_CONFIG_DIR || true)
MEMBERS="ana ben ctl"
tm() { LC_ALL=C "$TMUX_BIN" -S "$SOCK" -f /dev/null "$@"; }
# secrets_end: the run CA, the keys and the synthetic login files (every exit path).
secrets_end() {
  [ -z "${RUN:-}" ] || { rm -rf "$RUN/tls"; for _d in "$RUN/home/pool/a" "$RUN/home/pool/b" "$RUN/home/.claude"; do rm -f "$_d/.credentials.json"; done; }
}
# pool_end <status>: first the processes this script started and the run secrets, then the workspace when one was
# started (zygo down bounded to 60 s), the private server and the private folder; then live_end.
pool_end() {
  trap - EXIT INT TERM HUP PIPE
  for _p in $LIVE_PIDS; do kill "$_p" 2>/dev/null; done
  [ -z "$LIVE_PIDS" ] || sleep 1
  for _p in $LIVE_PIDS; do kill -KILL "$_p" 2>/dev/null; done
  secrets_end
  [ "$STARTED" -eq 0 ] || rclean
  [ -z "$SOCK" ] || tm kill-server 2>/dev/null
  [ -z "$ZP_PRIV" ] || rm -rf "$ZP_PRIV"
  live_end "$1"
}
trap 'pool_end $?' EXIT; trap 'pool_end 129' HUP; trap 'pool_end 130' INT; trap 'pool_end 141' PIPE; trap 'pool_end 143' TERM

TMUX_BIN=$(command -v tmux) || { echo "live: tmux is not on PATH"; exit 1; }
TVL=$("$TMUX_BIN" -V)
TV=$(printf '%s\n' "$TVL" | sed -n 's/^tmux //p')
ver_ge "${TV%[a-z]}" 3.2 || { echo "live: tmux is ${TV:-unknown}; the minimum is 3.2 (AU7); put tmux 3.2 or later first on PATH"; exit 1; }
for _t in jq python3 perl openssl sqlite3 git timeout; do command -v "$_t" >/dev/null || { echo "live: $_t is not on PATH"; exit 1; }; done
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

# penv <cmd>: the environment of the tmux server and of zygo (members inherit it): temporary HOME, no
# CLAUDE_CONFIG_DIR, the recorder as HTTPS proxy (loopback exempt), the run CA as the only trusted CA, the run's
# git config and the ssh wrapper.
penv() { env -i PATH="$PB" HOME="$H" USER="$U" LOGNAME="$U" SHELL=/bin/sh TERM=xterm-256color LANG=C.UTF-8 TMPDIR="$RUN/tmp" \
  HTTPS_PROXY="http://127.0.0.1:$RP" https_proxy="http://127.0.0.1:$RP" NO_PROXY=127.0.0.1,localhost,::1 no_proxy=127.0.0.1,localhost,::1 \
  SSL_CERT_FILE="$RUN/tls/ca.pem" GIT_CONFIG_GLOBAL="$RUN/gitconfig" GIT_CONFIG_NOSYSTEM=1 GIT_SSH_COMMAND="$SSHW" GIT_SSH="$SSHW" \
  DISABLE_AUTOUPDATER=1 ZYGO_NO_UPDATE_CHECK=1 ZYGO_CONFIG="$RUN/zygo.toml" "$@"; }
wsphase() { (cd "$R" && penv "$ZYGO_BIN" workspace) 2>/dev/null | sed -n '1s/^\(zygo: \)\{0,1\}workspace [^:]*: //p'; }
# rclean: stop the workspace (bounded to 60 s), forget it, stop the private server. With POOL_TEST_PAUSE set, the
# first call writes that file and waits until it is removed (at most 120 s; the cleanup test).
rclean() {
  if [ -n "${POOL_TEST_PAUSE:-}" ] && [ "$PAUSED" -eq 0 ]; then
    PAUSED=1; : >"$POOL_TEST_PAUSE"
    _i=1200; while [ -e "$POOL_TEST_PAUSE" ] && [ "$_i" -gt 0 ]; do sleep 0.1; _i=$((_i - 1)); done
  fi
  [ "$(wsphase)" = down ] || (cd "$R" && penv TMUX="$SOCK,$SP,0" timeout 60 "$ZYGO_BIN" down --now --wait -- "$name") >>"$RUN/clean.txt" 2>&1
  penv timeout 30 "$ZYGO_BIN" ws forget -- "$name" >>"$RUN/clean.txt" 2>&1
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
ev = [json.loads(x) for x in open(f)]
ev = [e for e in ev if e["m"] == m and e["t"] >= t0 and e["ev"] == "write"]
t = next((e["t"] for e in ev if e["busy"]), None)
sys.exit(0 if t is not None and any(not e["busy"] and e["t"] > t for e in ev) else 1)
PY
}
req_has() { [ -n "$(body_has "$1" "$RUN"/rec/[0-9]*.json 2>/dev/null)" ]; }
# hud_set <tmux pane>: the pane's HUD row shows a trimmed value other than 0
hud_set() { tm capture-pane -p -t "$1" 2>/dev/null | grep -q '✂ [1-9]'; }

# --- setup ------------------------------------------------------------------------------------
U=$(id -un)
live_run pool
rm -f "$H/.claude/.claude.json"   # new_run's dummy-key state file; the default folder's state is $H/.claude.json
name=$(basename "$R")
mkdir -p "$H/pool/a" "$H/pool/b" "$RUN/tls" "$RUN/tmp" "$RUN/bin"
# Two ssh wrappers; each logs its arguments and exits 1, so no ssh runs. SSHW is GIT_SSH_COMMAND and GIT_SSH (log
# ssh-git.log: git's own ssh transport); the ssh on the run's PATH logs to ssh-path.log (a direct ssh call).
mkdir -p "$RUN/sshw"
SSHW="$RUN/sshw/ssh-git"
printf '#!/bin/sh\nprintf "parent %%s: %%s\\n" "$(cat /proc/$PPID/comm)" "$*" >>"%s"\nexit 1\n' "$RUN/ssh-git.log" >"$SSHW"
printf '#!/bin/sh\nprintf "parent %%s: %%s\\n" "$(cat /proc/$PPID/comm)" "$*" >>"%s"\nexit 1\n' "$RUN/ssh-path.log" >"$RUN/bin/ssh"
chmod 755 "$SSHW" "$RUN/bin/ssh"
for _t in claude git tmux python3 jq sqlite3; do ln -s "$(command -v "$_t")" "$RUN/bin/$_t"; done
PB="$RUN/bin:/usr/bin:/bin"
# The run's git global config: GitHub ssh remotes rewritten to https (the proxy), an identity, and ssh.variant set
# (git does not probe the ssh program with "-G" to find its variant). The ssh forms are built from parts, since
# check-premises.sh reads <user>@<host> as an e-mail address.
GH="git@""github.com"
printf '[user]\n\tname = t\n\temail = t@t\n[url "https://github.com/"]\n\tinsteadOf = %s:\n\tinsteadOf = ssh://%s/\n[ssh]\n\tvariant = ssh\n' \
  "$GH" "$GH" >"$RUN/gitconfig"
# The private tmux server's config: no focus-events hint from Claude Code, wide detached sessions.
printf 'set -g focus-events on\nset -g default-size 200x50\n' >"$RUN/tmux.conf"

# T1, T2, T3: three synthetic logins (hashes only out), the state files, the user settings of each folder
# (statusLine and UserPromptSubmit and SessionEnd hooks), and the file each member reads (sizes differ, so each
# session's trimmed value differs).
RR=$(cd "$R" && pwd -P)
(umask 077; python3 -I - "$H" "$R" "$RR" "$RUN/expect.json" "$PREMISES_DIR/hook_log.py" "$RUN" <<'PY'
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
    hk = [{"hooks": [{"type": "command", "command": "python3 -I %s %s/userhooks-%s" % (hook, run, name)}]}]
    settings = {"statusLine": {"type": "command", "command": "printf USER-SL-" + name},
                "hooks": {"UserPromptSubmit": hk, "SessionEnd": hk}}
    json.dump(settings, open(os.path.join(d, "settings.json"), "w"))
json.dump(out, open(expect, "w"))
for m, n in (("ana", 200), ("ben", 320), ("ctl", 440)):
    with open(os.path.join(r, "read-%s.txt" % m), "w") as f:
        for i in range(n):
            f.write(("pool read line %04d for %s " % (i, m)).ljust(99, ".") + "\n")
PY
) || { echo "live: the logins could not be made"; exit 1; }
# T4: the run CA and the leaf for api.anthropic.com (umask 077; deleted at cleanup)
(
  umask 077
  cd "$RUN/tls" || exit 1
  openssl req -x509 -newkey rsa:2048 -nodes -days 2 -subj "/CN=zygo live pool CA" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign" -keyout ca-key.pem -out ca.pem &&
  openssl req -newkey rsa:2048 -nodes -subj "/CN=api.anthropic.com" -keyout leaf-key.pem -out leaf.csr &&
  printf 'subjectAltName=DNS:api.anthropic.com\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid,issuer\n' >ext &&
  openssl x509 -req -in leaf.csr -CA ca.pem -CAkey ca-key.pem -CAcreateserial -days 2 -extfile ext -out leaf.pem &&
  chmod 644 ca.pem
) >"$RUN/openssl.log" 2>&1 || { echo "live: openssl could not make the run CA"; exit 1; }
# T5: the mock with one scripted turn per member: a Read of the member's file (zygo's PostToolUse trim hook stores
# it), a Write of the redaction marker (zygo's PreToolUse guard denies it), then a text reply.
for m in $MEMBERS; do
  steps "pr-$m" '{"trigger": "tracer-pr-'"$m"' go", '"$MAIN"', "steps": [{"tool": "Read", "input": {"file_path": "'"$R/read-$m.txt"'"}}, {"tool": "Write", "input": {"file_path": "'"$R/pr-$m.txt"'", "content": "'"$MARK"'"}}, {"text": "pr '"$m"' done"}]}'
done
start_mock "$RUN/rules.json"
LIVE_PIDS="$LIVE_PIDS $MOCK_PID"
# T6: the TLS recorder
RECORDER_DUMMY="$DUMMY_KEY" python3 -I "$PREMISES_DIR/tls_recorder.py" --cert "$RUN/tls/leaf.pem" --key "$RUN/tls/leaf-key.pem" \
  --upstream "$MOCK_URL" --out "$RUN/tls-rec.jsonl" --port-file "$RUN/rport" >"$RUN/recorder.log" 2>&1 &
LIVE_PIDS="$LIVE_PIDS $!"
wait_port "$RUN/rport" || { echo "live: the recorder did not start"; exit 1; }
RP=$(cat "$RUN/rport")
# T7: the zygo config: the pool and the loadout (one window per member; no extra Claude Code arguments)
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
[[loadout.pr.window]]
name = "ben"
[[loadout.pr.window.pane]]
kind = "claude-code"
name = "ben"
pool = true
[[loadout.pr.window]]
name = "ctl"
[[loadout.pr.window.pane]]
kind = "claude-code"
name = "ctl"
passthrough = true
T

# proc_snap <out>: for every process under the private tmux server: its member (the pane whose process it is or
# descends from), whether HOME is the temporary HOME, the CLAUDE_CONFIG_DIR value (relative to it), whether the run's
# git config and ssh wrapper variables are set, the names of variables whose value names the operator's config
# folder, the count of open files under it, and the count of its TCP connections to a non-loopback address.
# Names and counts only; no value of the operator's folder is written.
proc_snap() {
  python3 -I - "$1" "$SP" "$H" "$RUN/members.txt" "$RUN/gitconfig" "$SSHW" "$REAL_HOME/.claude" "$REAL_CCD" <<'PY'
import json, os, sys
out, root, home, members, gitcfg, sshw, *bad = sys.argv[1:]
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
# inode -> remote address is loopback (established connections only, state 01)
remote = {}
for f, v6 in (("/proc/net/tcp", False), ("/proc/net/tcp6", True)):
    try:
        rows = open(f).read().splitlines()[1:]
    except OSError:
        continue
    for line in rows:
        p = line.split()
        if len(p) < 10 or p[3] != "01":
            continue
        addr = p[2].split(":")[0]
        if v6:
            loop = addr == "00000000000000000000000001000000" or (addr.startswith("0000000000000000FFFF0000") and addr[-2:] == "7F")
        else:
            loop = addr[-2:] == "7F"
        remote[p[9]] = loop
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
    socks = [f[8:-1] for f in files if f.startswith("socket:[")]
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
                 "git_env_diff": sorted(k + ("" if k not in env else "=" + " ".join(
                                            "WRAPPER" if w.strip("'\";") == sshw else
                                            ("SSH-PROGRAM" if os.path.basename(w.strip("'\";")) == "ssh" else "<path>") if "/" in w else w
                                            for w in env[k].split()))
                                        for k, v in (("GIT_CONFIG_GLOBAL", gitcfg), ("GIT_CONFIG_NOSYSTEM", "1"),
                                                     ("GIT_SSH_COMMAND", sshw), ("GIT_SSH", sshw)) if env.get(k) != v),
                 "real_dir_vars": sorted(k for k, v in env.items() if any(b in v for b in bad)),
                 "real_dir_files": sum(1 for f in files if any(f.startswith(b) for b in bad)),
                 "remote_tcp": sum(1 for s in socks if remote.get(s) is False)})
json.dump(rows, open(out, "w"), indent=1)
PY
}

# --- the run ----------------------------------------------------------------------------------
ALL="PR1:1.1 PR1:1.2 PR1:1.3 PR1:1.4 PR1:1.5 PR1:1.6 PR2:2.1 PR2:2.2 PR2:2.3 PR2:2.4 PR2:2.5 PR2:2.6 PR2:2.7 PR2:2.8"
part_pool() {
  penv "$TMUX_BIN" -S "$SOCK" -f "$RUN/tmux.conf" new-session -d -s hold -x 200 -y 50 /bin/sleep 3600 || { rest "tmux did not start the hold session" $ALL; return 1; }
  SP=$(tm display-message -p '#{pid}')
  STARTED=1
  (cd "$R" && penv TMUX="$SOCK,$SP,0" "$ZYGO_BIN" up --loadout pr --no-attach) >"$RUN/up.txt" 2>&1; echo $? >"$RUN/up.rc"
  if [ "$(cat "$RUN/up.rc")" -ne 0 ]; then rest "zygo up exit $(cat "$RUN/up.rc"): $(sed -n 1p "$RUN/up.txt")" $ALL; return 1; fi
  WS=$(tm show-environment -t "=zygo-$name:" ZYGO_WORKSPACE 2>/dev/null | sed -n 's/^ZYGO_WORKSPACE=//p')
  WJ="$H/.local/state/zygo/workspaces/$WS/workspace.json"
  PANES=$(tm list-panes -s -t "=zygo-$name:" -F '#{pane_id} #{pane_pid} #{@zygo-pane}')
  for m in $MEMBERS; do
    k=$(key_of "$m")
    p=$(printf '%s\n' "$PANES" | awk -v k="$k" '$3 == k { print $1; exit }')
    pp=$(printf '%s\n' "$PANES" | awk -v k="$k" '$3 == k { print $2; exit }')
    eval "K_$m=\$k P_$m=\$p PP_$m=\$pp"
    [ -n "$k" ] && [ -n "$p" ] || { rest "zygo up exit 0, but no pane key or tmux pane for $m (workspace id found: $([ -n "$WS" ] && echo yes || echo no))" $ALL; return 1; }
  done
  _c=$((120 * PW_TICKS / 5))
  until ready; do
    for m in $MEMBERS; do eval "cc_trust \"\$P_$m\""; done
    _c=$((_c - 1)); [ "$_c" -gt 0 ] || break; sleep 0.5
  done
  ready || { rest "timeout 120 s: ana, ben and ctl do not all have an agent session" $ALL; return 1; }
  # The keys, panes and processes of each member, for the judge.
  : >"$RUN/members.txt"
  for m in $MEMBERS; do
    eval "k=\$K_$m p=\$P_$m pp=\$PP_$m"
    printf '%s %s %s %s %s\n' "$m" "$k" "$p" "$pp" "$(mval "$k" zygo_session)" >>"$RUN/members.txt"
  done
  # The activity watcher: every 5 ms, one line for each rewrite of a member's activity.json (a new inode or mtime),
  # with its busy and tools_running values, and one line when the file is gone.
  _args=""
  for m in $MEMBERS; do eval "_args=\"\$_args $m=$H/.cache/zygo/sessions/\$(mval \"\$K_$m\" zygo_session)/activity.json\""; done
  python3 -I - "$RUN/activity.jsonl" $_args <<'PY' &
import json, os, sys, time
out, pairs = sys.argv[1], [a.split("=", 1) for a in sys.argv[2:]]
last = {}
ppid = os.getppid()
def put(o):
    with open(out, "a") as f:
        f.write(json.dumps(o) + "\n")
while os.getppid() == ppid:
    for m, p in pairs:
        try:
            st = os.stat(p)
            sig = (st.st_ino, st.st_mtime_ns, st.st_size)
        except OSError:
            if last.get(m) not in (None, "gone"):
                last[m] = "gone"
                put({"t": round(time.time(), 3), "m": m, "busy": None, "tools": None, "ev": "gone"})
            continue
        if last.get(m) == sig:
            continue
        try:
            a = json.load(open(p))
        except (OSError, ValueError):
            continue
        last[m] = sig
        put({"t": round(time.time(), 3), "m": m, "busy": a.get("busy"), "tools": a.get("tools_running", 0), "ev": "write"})
    time.sleep(0.005)
PY
  LIVE_PIDS="$LIVE_PIDS $!"
  # Process snapshot 1 (all members idle).
  proc_snap "$RUN/proc-1.json"
  nap 5
  for m in $MEMBERS; do
    eval "p=\$P_$m"
    t0=$(now); echo "$m $t0" >>"$RUN/prompts.txt"
    say "$p" "tracer-pr-$m go"
    b1=1; wait_for 60 busy_seen "$m" true && b1=0
    b2=1; [ "$b1" -eq 0 ] && wait_for 120 busy_after "$m" "$t0" && b2=0
    r1=1; wait_for 60 req_has "toolu_pr_${m}_01_write" && r1=0
    h1=1; wait_for 30 hud_set "$p" && h1=0
    echo "$m busy=$b1 idle=$b2 toolresult=$r1 hud=$h1 end=$(now)" >>"$RUN/turns.txt"
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
  sqlite3 -json "$H/.local/share/zygo/usage.db" "select session, tool, stored_bytes, view_bytes, result_id from trims order by ts, rowid" >"$RUN/trims.json" 2>/dev/null
  [ -s "$RUN/trims.json" ] || echo '[]' >"$RUN/trims.json"
  # zygo down (SessionEnd of each member), with the watcher still running; zygo ws forget removes workspace.json,
  # so the judge reads a copy.
  cp "$WJ" "$RUN/workspace.json"
  WJ="$RUN/workspace.json"
  printf '{"t": %s}\n' "$(now)" >"$RUN/down.json"
  rclean
  nap 1
  return 0
}

# judge: pool_analyze.py over the run files; each line becomes a sub or note line.
judge() {
  python3 -I "$PREMISES_DIR/pool_analyze.py" "$RUN" "$H" "$WJ" >"$RUN/judge.txt" 2>"$RUN/judge.err" || rest "pool_analyze.py failed: $(sed -n '$p' "$RUN/judge.err")" $ALL
  while IFS="$(printf '\t')" read -r _i _s _r _d; do
    [ -n "$_i" ] || continue
    if [ "$_r" = note ]; then note "$_i" "$_s" "$_d"; else sub "$_i" "$_s" "$_r" "$_d"; fi
  done <"$RUN/judge.txt"
}

part_pool && judge
# The status rows and menus of the three panes as one fixture (digits kept).
python3 -I - "$RUN" >"$RUN/pr2-panes.txt" <<'PY'
import json, os, sys
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
HEAD="Claude Code $LIVE_CC_VER (pin $PIN_CC), ${ZV:-zygo ?}, $TVL, $(uname -s); upstream: tls_recorder.py for api.anthropic.com:443 (run CA), then mock_upstream.py; synthetic logins"
finish PR1 "$LIVE_CC_VER" "$FXT/pr1.txt" "$HEAD" "1.1 1.2 1.3 1.4 1.5 1.6" "$OUT/pr1.txt"
finish PR2 "$LIVE_CC_VER" "$FXT/pr2.txt" "$HEAD" "2.1 2.2 2.3 2.4 2.5 2.6 2.7 2.8" "$OUT/pr2.txt"
scrub "$RUN/pr2-panes.txt" >"$FXT/pr2-panes.txt"
mkdir -p "$OUT" && cp "$FXT/pr1.txt" "$FXT/pr2.txt" "$FXT/pr2-panes.txt" "$OUT"/ || { echo "live: the fixtures could not be copied to the repository"; exit 1; }
live_done "$OUT"

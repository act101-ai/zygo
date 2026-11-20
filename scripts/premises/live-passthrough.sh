#!/bin/sh
# Live check #28 (node N38, spec §20 #28): one `zygo claude --passthrough -p` answer on the
# claude.ai login that Claude Code already stores; no login token and no zygo session token in
# any zygo file. No mock and no key.
# Usage: sh scripts/premises/live-passthrough.sh [--scan-only [<session id>]]
#   --scan-only: only the read-only whole-store scan (below), over the existing zygo data and
#   cache dirs; no agent runs. It writes its scratch dir, and rewrites the committed
#   fixtures/claude-code/live/28-store-scan.txt only when the scan passes. With a session id, it
#   scans as after that session's run (other live sessions' token files are not counted).
#
# What this run does to the operator's machine:
# - It uses the real HOME and no CLAUDE_CONFIG_DIR, so Claude Code reads the operator's ~/.claude
#   (settings, hooks, MCP servers) and its normal login (the macOS Keychain item), and refreshes
#   that login as it normally would. The script edits nothing under ~/.claude and adds no hook.
#   DISABLE_AUTOUPDATER=1 is set, so Claude Code does not update itself during the run.
# - Claude Code itself writes on the real HOME: a transcript folder under ~/.claude/projects/
#   (named after the run's temporary repository path) and possibly an entry for that path under
#   "projects" in ~/.claude.json. The script does not delete them; the operator may delete the
#   transcript folder.
# - It spends the operator's claude.ai usage on one short answer.
# - zygo writes its own state under the real ~/.local/share/zygo (usage rows stay there; the
#   plugin snapshot under ~/.local/share/zygo/plugins is created or refreshed) and ~/.cache/zygo
#   (the session folder, removed by zygo at exit): zygo has no variable to move them. ZYGO_CONFIG
#   points at an empty file, so the operator's zygo config is not read.
# - Variables of an enclosing Claude Code session (CLAUDECODE, CLAUDE_CODE_*, ...) are dropped,
#   as in a plain terminal.
# Run it after the pinned Claude Code is first on PATH (docs/live-checks.md).
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps
FX="$REPO_DIR/fixtures/claude-code/live"
mkdir -p "$FX"
DATA="$HOME/.local/share/zygo"
CACHE="$HOME/.cache/zygo"
SESS="$CACHE/sessions"
DB="$DATA/usage.db"
# The two prefixes the leak scans look for, each built from two parts (no literal in this file).
P_LOGIN="sk-ant""-o"
P_SESSION="zyg""_s_"

# store_scan <out> [<session id>]: both prefixes over every file of zygo's data dir and cache dir
# (raw usage.db, usage.db-wal and usage.db-shm included). A symbolic link is followed only when its
# resolved target is inside one of the two dirs; any other link is refused: named as a failure,
# not read, and nothing under it is listed (U3). Writes names and counts only to <out> (paths shown
# from ~), prints them, and returns 1 when a dir is missing or holds no file, when a file or dir
# cannot be read, when a link is refused, or on a hit.
# One exception, only with a session id: the token file of another session that is still live
# holds its own token by design (§12.3) and is not counted. "Live" means: its sessions/<other>/url
# is exactly http://127.0.0.1:<port>[/] or http://[::1]:<port>[/], and <url>/healthz answers 2xx
# with no proxy (zygo's proxy answers 404 on /). Any other url counts as not live, so its token
# file is counted (U2). Every other token file is stale and counted.
store_scan() {
  out=$1 sid=${2:-}
  : >"$RUN/scan-err.txt"
  python3 -I - "$RUN/scan-files.0" "$RUN/scan-refused.txt" "$RUN/scan-err.txt" "$DATA" "$CACHE" <<'PY' >"$RUN/scan-counts.txt"
import os, stat, sys
files_out, refused_out, err_out, *roots = sys.argv[1:]
real = [os.path.realpath(r) for r in roots]
inside = lambda p: any(q == r or q.startswith(r + os.sep) for r in real for q in [os.path.realpath(p)])
files, refused, errs, seen = [], [], [], set()
def walk(d):
    rd = os.path.realpath(d)
    if rd in seen:
        return
    seen.add(rd)
    try:
        names = sorted(os.listdir(d))
    except OSError:
        errs.append(d)
        return
    for n in names:
        p = os.path.join(d, n)
        # lstat, not os.path.is*: those hide an OSError (a dir that can be listed but not searched) as False.
        try:
            st = os.lstat(p)
            if stat.S_ISLNK(st.st_mode):
                if not inside(p):
                    refused.append(p)
                    continue
                try:
                    st = os.stat(p)
                except FileNotFoundError:
                    continue  # dangling link: nothing to read
        except OSError:
            errs.append(p)
            continue
        if stat.S_ISDIR(st.st_mode):
            walk(p)
        elif stat.S_ISREG(st.st_mode):
            files.append(p)
for r in roots:
    if os.path.isdir(r):
        walk(r)
open(files_out, "wb").write(b"".join(os.fsencode(f) + b"\0" for f in files))
open(refused_out, "w").write("".join(p + "\n" for p in refused))
open(err_out, "a").write("".join("cannot read: " + p + "\n" for p in errs))
for r in roots:
    print(sum(1 for f in files if f.startswith(r + os.sep)))
PY
  nd=$(sed -n 1p "$RUN/scan-counts.txt") nc=$(sed -n 2p "$RUN/scan-counts.txt")
  h_login=$(xargs -0 grep -aFl -e "$P_LOGIN" -- /dev/null <"$RUN/scan-files.0" 2>>"$RUN/scan-err.txt" | sed "s#^$HOME/#~/#" || true)
  h_sess=$(xargs -0 grep -aFl -e "$P_SESSION" -- /dev/null <"$RUN/scan-files.0" 2>/dev/null | sed "s#^$HOME/#~/#" || true)
  errs=$(grep -c . "$RUN/scan-err.txt" || true)
  refused=$(sed "s#^$HOME/#~/#" "$RUN/scan-refused.txt")
  empty=""
  for d in "$DATA" "$CACHE"; do
    if [ ! -d "$d" ]; then empty="$empty ${d#"$HOME"/} (missing)"; fi
  done
  [ -d "$DATA" ] && [ "${nd:-0}" -eq 0 ] && empty="$empty ${DATA#"$HOME"/} (no file)"
  [ -d "$CACHE" ] && [ "${nc:-0}" -eq 0 ] && empty="$empty ${CACHE#"$HOME"/} (no file)"
  raw=$(find "$DATA" -maxdepth 1 -type f -name 'usage.db*' 2>/dev/null | sed "s#^$DATA/##" | sort | tr '\n' ' ' | sed 's/ $//')
  other=""
  if [ -n "$sid" ]; then
    for f in $(printf '%s\n' "$h_sess" | grep -E '^~/\.cache/zygo/sessions/[^/]+/token$' | grep -vF "/sessions/$sid/"); do
      d="$SESS/$(basename "$(dirname "$f")")"
      u=$(cat "$d/url" 2>/dev/null || true)
      case $u in *"
"*) u="" ;; esac
      printf '%s\n' "$u" | grep -Eqx 'http://(127\.0\.0\.1|\[::1\]):[0-9]+/?' || continue
      if curl -q --noproxy '*' --proto =http -f -s -m 2 -o /dev/null -- "${u%/}/healthz" 2>/dev/null; then other="$other$f
"; fi
    done
  fi
  bad=$(printf '%s\n%s\n' "$h_login" "$h_sess" | grep -v '^$' | grep -vxF -e "$(printf '%s' "${other:-none}")" || true)
  {
    echo "store scan $(date +%Y-%m-%d): grep -aF, login token prefix and zygo session token prefix"
    echo "scanned: ~/.local/share/zygo (${nd:-0} files) and ~/.cache/zygo (${nc:-0} files), symbolic links followed only inside these dirs, raw ${raw:-none}"
    echo "read errors: $errs"
    echo "refused links (target outside the zygo dirs, not read): $(printf '%s' "$refused" | grep -c . || true)"
    [ -z "$refused" ] || printf '%s\n' "$refused"
    [ -z "$empty" ] || echo "missing or empty:$empty"
    echo "login prefix hits: $(printf '%s' "$h_login" | grep -c . || true)"
    echo "session prefix hits: $(printf '%s' "$h_sess" | grep -c . || true)"
    [ -z "$other" ] || { echo "live sessions' token files (by design, not counted):"; printf '%s' "$other"; }
    [ -z "$bad" ] || { echo "hits:"; printf '%s\n' "$bad"; }
  } >"$out"
  cat "$out"
  [ -z "$bad" ] && [ "$errs" -eq 0 ] && [ -z "$empty" ] && [ -z "$refused" ]
}

if [ "${1:-}" = --scan-only ]; then
  live_run scan
  rc=0
  store_scan "$RUN/scan.txt" "${2:-}" || rc=$?
  # The committed record is rewritten only when the scan passes (U3).
  if [ "$rc" -eq 0 ]; then scrub "$RUN/scan.txt" >"$FX/28-store-scan.txt"; else echo "live: scan failed; $FX/28-store-scan.txt not written" | sed "s#$REPO_DIR/##"; fi
  live_done "$FX"
  exit $rc
fi

pinned_agents
live_run p28
: >"$RUN/zygo.toml"
# The session id comes from the run's own usage row (rows after this rowid), not from the folder
# listing. During the run every new session folder's session.log is copied (every 0.2 s, so the
# last lines before zygo removes the folder at exit may be missed); the copy of the folder whose
# name is that id is the one scanned.
before=$(sqlite3 "$DB" "select coalesce(max(rowid), 0) from requests" 2>/dev/null || echo 0)
ls "$SESS" 2>/dev/null | sort >"$RUN/before.txt" || true
(
  for v in $(env | sed -n 's/^\(CLAUDE[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$v"; done
  export ZYGO_CONFIG="$RUN/zygo.toml" DISABLE_AUTOUPDATER=1
  cd "$R"
  exec "$ZYGO_BIN" claude --passthrough -- -p "Reply with the single word ready." </dev/null >"$RUN/out.txt" 2>"$RUN/err.txt"
) &
PID=$!
LIVE_PIDS="${LIVE_PIDS:-} $PID"
mkdir -p "$RUN/logs"
while kill -0 "$PID" 2>/dev/null; do
  for d in $(ls "$SESS" 2>/dev/null | sort | comm -13 "$RUN/before.txt" -); do
    [ -e "$SESS/$d/session.log" ] && mkdir -p "$RUN/logs/$d" && cp "$SESS/$d/session.log" "$RUN/logs/$d/session.log" 2>/dev/null
  done
  sleep 0.2
done
rc=0
wait "$PID" || rc=$?
LIVE_PIDS=$(printf '%s\n' $LIVE_PIDS | grep -vx "$PID" | tr '\n' ' ' || true)
ID=$(sqlite3 "$DB" "select session from requests where rowid > $before and passthrough = 1 order by rowid limit 1" 2>/dev/null || true)
LIVE_VER=$(sqlite3 "$DB" "select agent_version from requests where session = '$ID' order by ts desc limit 1" 2>/dev/null || true)
LIVE_VER=${LIVE_VER:-$PIN_CC}

if [ "$rc" -eq 2 ] && grep -q 'zygo cannot launch with it' "$RUN/err.txt"; then
  line=$(grep 'zygo cannot launch with it' "$RUN/err.txt" | head -n 1 | sed 's/^zygo: //; s/; zygo cannot launch with it//')
  result 28 fail "(preflight: $(echo "$line" | python3 -I "$PREMISES_DIR/scrub.py" "$RUN" /dev/stdin))"
  live_done "$FX"
  exit 0
fi

sqlite3 -json "$DB" "select role_served, passthrough, status, estimated_usd, agent_version from requests where session = '$ID' order by ts, rowid" >"$RUN/28-usage.json" 2>/dev/null || true
[ -s "$RUN/28-usage.json" ] || echo '[]' >"$RUN/28-usage.json"
scrub "$RUN/28-usage.json" >"$RUN/28-usage.scrubbed.json"

# Leak scan: both prefixes, over the files of this run that exist; the result names each one.
sqlite3 "$DB" ".dump requests" 2>/dev/null | grep -F "${ID:-no-session-id}" >"$RUN/dump.txt" || true
log=""
[ -n "$ID" ] && [ -e "$RUN/logs/$ID/session.log" ] && { cp "$RUN/logs/$ID/session.log" "$RUN/session.log"; log=yes; }
set -- "$RUN/dump.txt" "$RUN/out.txt" "$RUN/err.txt" "$RUN/28-usage.scrubbed.json"
scanned="usage rows (.dump requests of the session), stdout, stderr, the capture"
[ -n "$log" ] && { set -- "$@" "$RUN/session.log"; scanned="$scanned, session.log (copy polled every 0.2 s)"; }
if [ -n "$ID" ] && [ -d "$SESS/$ID" ]; then set -- "$@" "$SESS/$ID"; scanned="$scanned, the session folder (left after exit)"; fi
leaks=$(grep -ralF -e "$P_LOGIN" -e "$P_SESSION" "$@" 2>/dev/null | sed "s#^$RUN/##; s#^$HOME/#~/#" | tr '\n' ' ')

# Whole-store scan (R2): zygo's data and cache dirs, after the run.
st=0
store_scan "$RUN/scan.txt" "$ID" || st=$?

last=$(jq -c '.[-1] // {}' "$RUN/28-usage.json")
rows=$(jq 'length' "$RUN/28-usage.json")
ok=$(jq -r '(length > 0) and (.[-1].role_served == "subscription") and (.[-1].passthrough == 1) and (.[-1].status == 200) and (.[-1].estimated_usd == 0)' "$RUN/28-usage.json")
reply=$(tr -d '\r\n' <"$RUN/out.txt" | cut -c1-40)
if [ -z "$leaks" ] && [ "$st" -eq 0 ]; then
  cp "$RUN/28-usage.scrubbed.json" "$FX/28-usage.json"
  scrub "$RUN/scan.txt" >"$FX/28-store-scan.txt"
fi
if [ -z "$ID" ]; then
  result 28 fail "exit $rc, reply \"$reply\"; no usage row of a passthrough session after the run"
elif [ -z "$log" ]; then
  result 28 fail "(no session.log copy) exit $rc, reply \"$reply\"; $rows usage row(s), last $last; leak scan over $scanned: ${leaks:-none}"
elif [ "$rc" -eq 0 ] && [ -n "$reply" ] && [ "$ok" = true ] && [ -z "$leaks" ] && [ "$st" -eq 0 ]; then
  result 28 pass "reply \"$reply\", exit 0; $rows usage row(s), last $last; leak scan (login and session token prefixes) over $scanned: none; whole-store scan: none"
else
  result 28 fail "exit $rc, reply \"$reply\"; $rows usage row(s), last $last; leak scan over $scanned: ${leaks:-none}; whole-store scan: $([ "$st" -eq 0 ] && echo none || echo hits)${leaks:+ (capture not kept)}"
fi
live_done "$FX"

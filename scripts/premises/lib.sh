# Shared helpers for the N2 premise experiments. Source it from run-<id>.sh.
# N2_SCRATCH: scratch directory for isolated HOMEs and raw captures. Set by the caller: kept. Unset
# or empty: lib.sh makes one (mktemp -d) and prints its path; on exit it removes it after status 0
# and keeps it (named on stderr) otherwise. live.sh's live_end does the same.
# Nothing here reads the operator's own agent config, login or memory files.
set -eu
: "${PREMISES_DIR:=$(cd "$(dirname "$0")" && pwd)}"
REPO_DIR=$(cd "$PREMISES_DIR/../.." && pwd)
# n2_scratch_end <status>: for the scratch dir lib.sh made itself (never a caller-set one): after
# status 0 remove it (a failure is named on stderr), else keep it and name it; then exit <status>.
n2_scratch_end() {
  trap - EXIT INT TERM HUP
  if [ -n "${N2_SCRATCH_OWN:-}" ]; then
    if [ "$1" -eq 0 ]; then
      chmod -R u+rwx "$N2_SCRATCH_OWN" 2>/dev/null || true
      rm -rf "$N2_SCRATCH_OWN" 2>/dev/null || echo "premises: could not remove $N2_SCRATCH_OWN" >&2
    else
      echo "premises: kept $N2_SCRATCH_OWN (exit $1)" >&2
    fi
  fi
  exit "$1"
}
# Ownership never comes from the environment: the first source in this process clears it (and its
# export attribute); _n2_lib_pid (not exported) tells a re-source in this process.
if [ "${_n2_lib_pid:-}" != "$$" ]; then
  unset N2_SCRATCH_OWN _n2_lib_pid
  N2_SCRATCH_OWN="" _n2_lib_pid=$$
fi
if [ -z "${N2_SCRATCH:-}" ]; then
  trap 'n2_scratch_end $?' EXIT
  trap 'n2_scratch_end 129' HUP
  trap 'n2_scratch_end 130' INT
  trap 'n2_scratch_end 143' TERM
  # Ownership is assigned in the statement that makes the dir (a signal inside mktemp: accepted).
  N2_SCRATCH_OWN=$(mktemp -d) || exit 1
  N2_SCRATCH=$N2_SCRATCH_OWN
  echo "premises: scratch $N2_SCRATCH (removed on exit 0, kept otherwise; set N2_SCRATCH to keep it)" >&2
fi
DUMMY_KEY="dummy-""key"
CLAUDE_BIN=$(command -v claude) || { echo "premises: claude is not on PATH"; exit 1; }
OPENCODE_BIN=$(command -v opencode) || { echo "premises: opencode is not on PATH"; exit 1; }
TOOL_PATH="$(dirname "$CLAUDE_BIN"):$(dirname "$OPENCODE_BIN"):$(dirname "$(command -v node 2>/dev/null || echo /usr/bin/true)"):/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin"

# new_run <name>: fresh run dir with home/, repo/ (a git repository) and rec/.
new_run() {
  RUN="$N2_SCRATCH/$1"
  rm -rf "$RUN"
  mkdir -p "$RUN/home/.claude" "$RUN/home/.config" "$RUN/home/.local/share" "$RUN/home/.cache" "$RUN/repo" "$RUN/rec"
  H="$RUN/home"
  R="$RUN/repo"
  git -C "$R" init -q
  git -C "$R" -c user.name=t -c user.email=t@t commit -q --allow-empty -m init
  # Skip onboarding, approve the dummy key and trust the repo (interactive runs only need it).
  RR=$(cd "$R" && pwd -P)
  python3 -I - "$H/.claude/.claude.json" "$DUMMY_KEY" "$RR" "$R" <<'PY'
import json, sys
p, key, rr, r = sys.argv[1:5]
proj = {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True}
json.dump({"hasCompletedOnboarding": True, "numStartups": 5, "theme": "dark",
           "customApiKeyResponses": {"approved": [key[-20:]], "rejected": []},
           "projects": {rr: proj, r: proj}}, open(p, "w"))
PY
}

# start_mock [rules.json]: start the mock for this run; sets MOCK_URL and MOCK_PID.
start_mock() {
  rm -f "$RUN/port"
  MOCK_KEY="$DUMMY_KEY" python3 -I "$PREMISES_DIR/mock_upstream.py" --dir "$RUN/rec" \
    ${1:+--rules "$1"} --port-file "$RUN/port" >"$RUN/mock.log" 2>&1 &
  MOCK_PID=$!
  i=0
  while [ ! -s "$RUN/port" ]; do i=$((i + 1)); [ $i -lt 100 ] || { echo "mock did not start"; exit 1; }; sleep 0.1; done
  MOCK_URL="http://127.0.0.1:$(cat "$RUN/port")"
}

stop_mock() {
  kill "$MOCK_PID" 2>/dev/null || true
  wait "$MOCK_PID" 2>/dev/null || true
  MOCK_PID=
  check_abort
}

check_abort() {
  if [ -e "$RUN/rec/ABORT" ]; then
    echo "ABORT: $(cat "$RUN/rec/ABORT")" >&2
    exit 3
  fi
}

# agent_env: the isolated environment, then the experiment's own NAME=value words.
# NO_API_KEY=1 leaves out ANTHROPIC_API_KEY (apiKeyHelper supplies it); NO_HINT=1 leaves out
# CLAUDE_CODE_GATEWAY_HINT_HEADERS (the one run that shows its effect).
agent_env() {
  echo env -i PATH="$TOOL_PATH" TERM="${TERM:-xterm-256color}" HOME="$H" \
    CLAUDE_CONFIG_DIR="$H/.claude" XDG_CONFIG_HOME="$H/.config" XDG_DATA_HOME="$H/.local/share" \
    XDG_CACHE_HOME="$H/.cache" ANTHROPIC_BASE_URL="$MOCK_URL" \
    $([ -n "${NO_API_KEY:-}" ] || echo ANTHROPIC_API_KEY="$DUMMY_KEY") \
    $([ -n "${NO_HINT:-}" ] || echo CLAUDE_CODE_GATEWAY_HINT_HEADERS=1)
}

# EXTRA_ENV: the experiment's own NAME=value words (no spaces in values).
EXTRA_ENV=""

# run_claude <claude args>: run Claude Code isolated, in the run's repo.
run_claude() {
  (cd "$R" && $(agent_env) $EXTRA_ENV "$CLAUDE_BIN" "$@")
  check_abort
}

# run_claude_stream <out.jsonl> <msgs file, one message per line> <claude args>:
# several user turns over stream-json, each sent after the previous turn's result.
run_claude_stream() {
  out=$1 msgs=$2; shift 2
  (cd "$R" && python3 -I "$PREMISES_DIR/drive_stream.py" "$out" "$msgs" -- $(agent_env) $EXTRA_ENV "$CLAUDE_BIN" -p --input-format stream-json --output-format stream-json --verbose "$@") || true
  check_abort
}

# scrub <file>: write a fixture copy to stdout with machine paths and prompt text omitted.
scrub() {
  python3 -I "$PREMISES_DIR/scrub.py" "$RUN" "$1"
}

# tmux_own <session>: a session of that name that this script did not start is left alone (exit 1);
# one it started is killed and started again. The name is recorded in LIVE_TMUX (live.sh's live_end
# kills those sessions).
LIVE_TMUX=""
tmux_own() {
  if tmux has-session -t "=$1" 2>/dev/null; then
    case " $LIVE_TMUX " in *" $1 "*) tmux kill-session -t "=$1" ;; *) echo "premises: tmux session $1 exists; not started"; exit 1 ;; esac
  fi
  case " $LIVE_TMUX " in *" $1 "*) ;; *) LIVE_TMUX="$LIVE_TMUX $1" ;; esac
}

# tmux_claude <session> <claude args>: start Claude Code interactively in a detached tmux
# session (isolated environment, the run's repo). Use tmux_send / tmux_wait_req after it.
tmux_claude() {
  s=$1; shift
  tmux_own "$s"
  # The command runs from a file under /bin/sh (the tmux default shell may be zsh, which
  # treats a word like design[1m] as a failed glob).
  echo "cd '$R' && $(agent_env) $EXTRA_ENV $CLAUDE_BIN $*; sleep 2" >"$RUN/tmux-$s.sh"
  tmux new-session -d -s "$s" -x 200 -y 50 -c "$R" "/bin/sh $RUN/tmux-$s.sh"
}
tmux_send() { tmux send-keys -t "=$1:" -l "$2"; sleep 0.5; tmux send-keys -t "=$1:" Enter; }
# tmux_wait_req <n> [seconds]: wait until the mock has recorded at least n requests.
tmux_wait_req() {
  i=0
  while [ "$(ls "$RUN/rec" | grep -c '^[0-9].*\.json$')" -lt "$1" ]; do
    i=$((i + 1)); [ $i -lt $((${2:-60} * 2)) ] || { echo "timeout waiting for request $1" >&2; return 1; }
    check_abort; sleep 0.5
  done
}

# start_web <dir>: a static HTTPS server on 127.0.0.1 for WebFetch (which upgrades http to
# https) with a throwaway self-signed certificate; sets WEB_URL, WEB_PID, WEB_CA.
start_web() {
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$RUN/web-key.pem" -out "$RUN/web-cert.pem" -days 2 -subj "/CN=127.0.0.1" -addext "subjectAltName=IP:127.0.0.1" >/dev/null 2>&1
  WEB_CA="$RUN/web-cert.pem"
  python3 -I "$PREMISES_DIR/https_static.py" "$1" "$WEB_CA" "$RUN/web-key.pem" "$RUN/web-port" >"$RUN/web.log" 2>&1 &
  WEB_PID=$!
  i=0
  while [ ! -s "$RUN/web-port" ]; do i=$((i + 1)); [ $i -lt 100 ] || { echo "web did not start"; exit 1; }; sleep 0.1; done
  WEB_URL="https://127.0.0.1:$(cat "$RUN/web-port")"
  EXTRA_ENV="$EXTRA_ENV NODE_EXTRA_CA_CERTS=$WEB_CA"
}
stop_web() { kill "$WEB_PID" 2>/dev/null || true; WEB_PID=; }

# hook_settings <log dir> [<responses.json>]: settings JSON that logs every zygo-relevant hook event.
hook_settings() {
  cmd="python3 -I $PREMISES_DIR/hook_log.py $1${2:+ $2}"
  python3 -I - "$cmd" <<'PY'
import json, sys
cmd = sys.argv[1]
h = [{"type": "command", "command": cmd}]
tool_events = ["PreToolUse", "PostToolUse", "PostToolUseFailure"]
other = ["SessionStart", "UserPromptSubmit", "PreCompact", "PostCompact", "SubagentStart", "SubagentStop", "Stop"]
hooks = {e: [{"matcher": "*", "hooks": h}] for e in tool_events}
hooks.update({e: [{"hooks": h}] for e in other})
print(json.dumps({"disableAllHooks": False, "hooks": hooks}))
PY
}

# publish_sse <dest dir>: scrubbed copies of the run's authored SSE replies ($RUN/sse) as fixtures.
publish_sse() {
  mkdir -p "$1"
  for f in "$RUN"/sse/*.sse; do [ -e "$f" ] && scrub "$f" >"$1/$(basename "$f")"; done
  return 0
}

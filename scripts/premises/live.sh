# Live-check helpers (node N38). Source it after lib.sh from scripts/premises/live-*.sh.
# Run by hand only; never from CI or a Rust test. Provider keys come only from the operator's
# environment (LIVE_KEYS names them); they are never written as NAME=value words, so they are in
# no argv and no file. Every live script calls live_traps (EXIT, INT, TERM, HUP, PIPE) after sourcing.
. "$PREMISES_DIR/ver.sh"   # ver_ge (node PRB-2)
: "${ZYGO_BIN:=$REPO_DIR/target/release/zygo}"   # cargo build -p zygo --release (a debug build misses
# the 200 ms secret-redaction deadline on an interactive Claude Code body and answers 503)
PIN_CC=$(sed -n 's/^Claude Code: *//p' "$REPO_DIR/docs/premises.md" | head -n 1)
PIN_OC=$(sed -n 's/^OpenCode: *//p' "$REPO_DIR/docs/premises.md" | head -n 1)
LIVE_KEYS=${LIVE_KEYS:-}
LIVE_VARS=${LIVE_VARS:-}
LIVE_VER=${LIVE_VER:-$PIN_CC}
LIVE_TIMEOUT=${LIVE_TIMEOUT:-}   # seconds; live_exec bounds its command by it when set
LIVE_PIDS=""   # background processes a script starts itself (besides MOCK_PID and WEB_PID)

# live_end <status>: kill the mock and web servers and the processes and tmux sessions this
# script started (by saved PID and exact session name), and each running timeout wrapper's
# process group (TERM to the group first, then the wrapper), delete its scratch tree live-<pid> when lib.sh made the scratch dir (a caller-set dir keeps it, without its pg-* files), then n2_scratch_end (lib.sh: a scratch dir lib.sh made is removed after status 0, else kept).
# A recorded group is killed only when its leader still leads it (ps: pgid of the leader PID = the
# group id) and, while the recorded wrapper is alive, is that wrapper's child (ppid = wrapper PID).
# (A Ctrl-C reaches the wrapper too, which may exit first and orphan the leader.) A reused PID is
# left alone.
live_end() {
  trap - EXIT INT TERM HUP PIPE
  for f in "$N2_SCRATCH/live-$$"/pg-*; do
    [ -f "$f" ] || continue
    wp="" pg=""
    read -r wp pg <"$f" || true
    [ -n "$wp" ] && [ -n "$pg" ] || continue
    [ "$(ps -o pgid= -p "$pg" 2>/dev/null | tr -d ' ')" = "$pg" ] || continue
    if kill -s 0 "$wp" 2>/dev/null && [ "$(ps -o ppid= -p "$pg" 2>/dev/null | tr -d ' ')" != "$wp" ]; then continue; fi
    kill -s TERM -- "-$pg" 2>/dev/null || true
    kill "$wp" 2>/dev/null || true
  done
  for p in ${MOCK_PID:-} ${WEB_PID:-} $LIVE_PIDS; do kill "$p" 2>/dev/null || true; done
  for s in $LIVE_TMUX; do tmux kill-session -t "=$s" 2>/dev/null || true; done
  # A caller-set scratch dir (lib.sh: N2_SCRATCH_OWN is empty) keeps live-<pid> with its captures; only
  # its pg-* files go. A dir lib.sh made loses live-<pid> here. A failed removal must not stop the trap
  # under set -e: name it, keep the status, go on.
  if [ -z "${N2_SCRATCH_OWN:-}" ]; then
    rm -f "$N2_SCRATCH/live-$$"/pg-* 2>/dev/null || true
  elif [ -e "$N2_SCRATCH/live-$$" ]; then
    chmod -R u+rwx "$N2_SCRATCH/live-$$" 2>/dev/null || true
    rm -rf "$N2_SCRATCH/live-$$" 2>/dev/null || echo "premises: could not remove $N2_SCRATCH/live-$$" >&2
  fi
  n2_scratch_end "$1"
}
# live_traps: live_end on EXIT, INT, TERM, HUP and PIPE (a signal exits 128 + its number; bash runs
# an EXIT trap with status 0 after an untrapped PIPE, so PIPE is trapped too). Stale pg-* files of
# an earlier process with the same PID are removed first.
live_traps() {
  rm -f "$N2_SCRATCH/live-$$"/pg-* 2>/dev/null || true
  trap 'live_end $?' EXIT
  trap 'live_end 129' HUP
  trap 'live_end 130' INT
  trap 'live_end 141' PIPE
  trap 'live_end 143' TERM
}

# agent_ver <--version output>: CR removed; nothing when more than one line is left; else the first
# word (leading blanks skipped) with a build suffix (from the first +) cut; a -prerelease stays.
agent_ver() {
  _av_t=$(printf '%s\n' "$1" | tr -d '\r')
  [ "$(printf '%s\n' "$_av_t" | wc -l | tr -d ' ')" = 1 ] || return 0
  printf '%s\n' "$_av_t" | sed 's/^[[:blank:]]*//; s/[[:blank:]].*$//; s/+.*$//'
}
# agent_ok <version word> <min> (author's call, 2026-11-21, SemVer order): a release passes at or
# above min; a prerelease <core>-<pre> passes only when <core> is above min; an empty <pre> fails.
agent_ok() {
  case "$1" in
    *-) return 1 ;;
    *-*) ver_ge "${1%%-*}" "$2" && ! ver_ge "$2" "${1%%-*}" ;;
    *) ver_ge "$1" "$2" ;;
  esac
}
# agent_show <--version output>: its first line, printable ASCII only (C0 and C1 controls, DEL and
# every other byte removed).
agent_show() { printf '%s\n' "$1" | head -n 1 | LC_ALL=C tr -cd ' -~'; }
# pinned_agents: the claude and opencode on PATH are at or above the pinned versions (operator,
# 2026-11-21: a pin is a minimum). Sets and exports LIVE_CC_VER and LIVE_OC_VER (agent_ver of each
# --version output) and sets LIVE_VER = LIVE_CC_VER.
pinned_agents() {
  v=$("$CLAUDE_BIN" --version 2>/dev/null || true)
  LIVE_CC_VER=$(agent_ver "$v")
  agent_ok "$LIVE_CC_VER" "$PIN_CC" || {
    echo "live: claude is $(agent_show "$v"); the minimum is $PIN_CC (docs/premises.md); put a version at or above it first on PATH"; exit 1; }
  v=$("$OPENCODE_BIN" --version 2>/dev/null || true)
  LIVE_OC_VER=$(agent_ver "$v")
  agent_ok "$LIVE_OC_VER" "$PIN_OC" || {
    echo "live: opencode is $(agent_show "$v"); the minimum is $PIN_OC (docs/premises.md); put a version at or above it first on PATH"; exit 1; }
  LIVE_VER=$LIVE_CC_VER
  export LIVE_CC_VER LIVE_OC_VER
}

# need_env <NAME>: stop before any network call when NAME is empty. Never prints a value.
need_env() {
  eval "v=\${$1:-}"
  [ -n "$v" ] || { echo "live: $1 is not set; this check calls the real provider"; exit 1; }
  v=
}

# live_run <name>: new_run under the per-script prefix the EXIT trap deletes.
live_run() { new_run "live-$$/$1"; }

# live_exec <cmd> <args>: the isolated environment (keys named in LIVE_KEYS kept, inherited).
live_exec() (
  keys=$LIVE_KEYS vars=$LIVE_VARS to=${LIVE_TIMEOUT:-} pgdir="$N2_SCRATCH/live-$$"
  for v in $(env | sed -n 's/^\([A-Za-z_][A-Za-z0-9_]*\)=.*/\1/p'); do
    case " $keys " in *" $v "*) continue ;; esac
    unset "$v" 2>/dev/null || true
  done
  export PATH="$TOOL_PATH" TERM="${TERM:-xterm-256color}" HOME="$H" CLAUDE_CONFIG_DIR="${LIVE_CC_DIR:-$H/.claude}" \
    XDG_CONFIG_HOME="$H/.config" XDG_DATA_HOME="$H/.local/share" XDG_CACHE_HOME="$H/.cache" \
    XDG_STATE_HOME="$H/.local/state" OPENCODE_DISABLE_AUTOUPDATE=1 DISABLE_AUTOUPDATER=1 ZYGO_CONFIG="$RUN/zygo.toml" \
    ZYGO_LIVE_MOCK_KEY="$DUMMY_KEY"
  # shellcheck disable=SC2086
  [ -z "$vars" ] || export $vars
  cd "$R"
  [ -n "$to" ] || exec "$@"
  # A wall-clock bound (spend bound of the owner scripts): the command runs in its own process
  # group; after LIVE_TIMEOUT seconds the whole group gets TERM, then KILL, and the exit is 124.
  # The file <scratch>/live-<script pid>/pg-<wrapper pid> holds "<wrapper pid> <group id>" while
  # the command runs, so live_end kills the group on INT, TERM and HUP ($$ is the script's PID
  # in this subshell too).
  mkdir -p "$pgdir"
  exec perl -e '
    my $t = shift; my $f = shift . "/pg-$$"; my $p = fork; defined $p or die "live: fork: $!\n";
    if (!$p) { setpgrp(0, 0); exec { $ARGV[0] } @ARGV; exit 127 }
    setpgrp($p, $p);
    if (open(my $h, ">", $f)) { print $h "$$ $p\n"; close $h }
    $SIG{ALRM} = sub { print STDERR "live: timeout after ${t}s\n"; kill "TERM", -$p; sleep 2; kill "KILL", -$p; unlink $f; exit 124 };
    alarm $t; waitpid($p, 0); my $s = $?; unlink $f; exit($s & 127 ? 128 + ($s & 127) : $s >> 8);' "$to" "$pgdir" "$@"
)

# run_zygo <args>: zygo in the isolated environment.
run_zygo() {
  rc=0
  live_exec "$ZYGO_BIN" "$@" || rc=$?
  check_abort
  return $rc
}

# tmux_zygo <session> <args>: zygo interactively in a detached tmux session (as tmux_claude).
tmux_zygo() {
  [ -z "$LIVE_KEYS" ] || { echo "live: tmux runs take no key"; exit 1; }
  s=$1; shift
  tmux_own "$s"   # lib.sh: a session this script did not start is left alone
  # Each argument single-quoted for /bin/sh (a word like design[1m] stays one literal word).
  args=""
  for a in "$@"; do args="$args '$(printf '%s' "$a" | sed "s/'/'\\\\''/g")'"; done
  echo "cd '$R' && env -i PATH='$TOOL_PATH' TERM='${TERM:-xterm-256color}' HOME='$H' CLAUDE_CONFIG_DIR='$H/.claude' XDG_CONFIG_HOME='$H/.config' XDG_DATA_HOME='$H/.local/share' XDG_CACHE_HOME='$H/.cache' XDG_STATE_HOME='$H/.local/state' OPENCODE_DISABLE_AUTOUPDATE=1 DISABLE_AUTOUPDATER=1 ZYGO_CONFIG='$RUN/zygo.toml' ZYGO_LIVE_MOCK_KEY='$DUMMY_KEY' $LIVE_VARS '$ZYGO_BIN'$args; sleep 2" >"$RUN/tmux-$s.sh"
  tmux -f /dev/null new-session -d -s "$s" -x 200 -y 50 -c "$R" "/bin/sh $RUN/tmux-$s.sh"
}

# tmux_wait_text <session> <fixed text> [seconds]: poll the pane every 0.5 s.
tmux_wait_text() {
  i=0
  until tmux capture-pane -p -t "=$1:" 2>/dev/null | grep -qF -- "$2"; do
    i=$((i + 1)); [ $i -lt $((${3:-30} * 2)) ] || return 1
    sleep 0.5
  done
}

# mock_config <file>: one model live-mock (the claude-haiku-5-5 facts) at the mock; every role on it.
mock_config() {
  cat >"$1" <<T
[[model]]
id = "live-mock"

  [model.facts]
  reasoning = "switchable"
  reasoning_default_on = true
  effort_levels = ["low", "medium", "high", "xhigh", "max"]
  max_output_tokens = 128000
  context_window = 1000000
  sampling = false
  vision = true
  structured_output = "native"
  forced_tool_choice = true
  verified_on = 2026-11-18
  source = "scripts/premises/live.sh: mock upstream"

  [[model.provider]]
  name = "mock"
  base_url = "$MOCK_URL"
  dialects = ["anthropic"]
  upstream_model = "live-mock"
  key = "env:ZYGO_LIVE_MOCK_KEY"
  enabled = true

T
  for r in lead design plan triage review code read; do
    printf '[role.%s]\nmodel = "live-mock"\n\n' "$r" >>"$1"
  done
}

# usage_rows <out>: the run's usage rows (metadata only), scrubbed.
usage_rows() {
  # No usage store or no requests table (zygo wrote no row): "no usage rows", an empty list, go on.
  if [ ! -e "$H/.local/share/zygo/usage.db" ] ||
    ! sqlite3 -json "$H/.local/share/zygo/usage.db" "select request_class, compaction, agent_id, agent_type, agent_version, role_requested, role_served, model, status, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, adjustments from requests order by ts, rowid" >"$RUN/usage.json" 2>/dev/null; then
    echo "no usage rows"
    : >"$RUN/usage.json"
  fi
  [ -s "$RUN/usage.json" ] || echo '[]' >"$RUN/usage.json"
  scrub "$RUN/usage.json" >"$1"
}

# usage_version: agent_version of the run's last usage row (empty when none).
usage_version() {
  sqlite3 "$H/.local/share/zygo/usage.db" "select agent_version from requests order by ts desc, rowid desc limit 1" 2>/dev/null || true
}

# body_has <regex> <json files>: names of the files whose JSON (non-ASCII unescaped) matches.
body_has() {
  python3 -I - "$@" <<'PY'
import json, re, sys
rx = re.compile(sys.argv[1])
for f in sys.argv[2:]:
    try:
        o = json.load(open(f))
    except (OSError, ValueError):
        continue
    if rx.search(json.dumps(o, ensure_ascii=False)):
        print(f)
PY
}

# result <id> pass|fail|not-crossed <text>: one LIVE line, also appended to $N2_SCRATCH/results.txt.
result() {
  id=$1 r=$2; shift 2
  echo "LIVE $id $r (agent ${LIVE_VER:-?}): $*" | tee -a "$N2_SCRATCH/results.txt"
}

# live_done <fixture dir>: the end of every live script (captures already copied out).
live_done() {
  echo "live: fixtures in ${1#"$REPO_DIR"/}; scratch removed on exit unless N2_SCRATCH is set"
}

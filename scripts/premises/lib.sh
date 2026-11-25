# Shared helpers for the N2 premise experiments. Source it from run-<id>.sh or codex/<part>.sh.
# N2_AGENTS: the agents this runner needs (from claude, opencode, codex; default "claude opencode"),
# set by the runner itself before sourcing; one exported from the environment is refused.
# lib.sh checks that each is on PATH and sets CLAUDE_BIN, OPENCODE_BIN or CODEX_BIN.
# N2_SCRATCH: scratch directory for isolated HOMEs and raw captures. Set by the caller: kept, with
# everything in it. Unset or empty: after the PATH checks pass, lib.sh makes one
# (mktemp -d) and prints its path; on exit it removes it after status 0 and keeps it (named on
# stderr) otherwise. live.sh's live_end does the same; it removes live-<pid> only from a dir lib.sh made.
# Codex runs use run_codex (codex_env, then codex_record <part> after stop_mock).
# Nothing here reads the operator's own agent config, login or memory files.
set -eu
: "${PREMISES_DIR:=$(cd "$(dirname "$0")" && pwd)}"
REPO_DIR=$(cd "$PREMISES_DIR/../.." && pwd)
# The caller's HOME, before any run changes it: the owner's ~/.codex is $OWNER_HOME/.codex.
OWNER_HOME=${HOME:-}
# The PATH checks come before the scratch dir, so a failed check makes no dir.
# Only the runner's own (unexported) N2_AGENTS counts: one from the environment would turn off the
# claude and opencode checks of every runner that does not set it.
if env | grep -q '^N2_AGENTS='; then
  echo "premises: N2_AGENTS comes from the environment; only the runner sets it (unset N2_AGENTS)"
  exit 1
fi
CLAUDE_BIN="" OPENCODE_BIN="" CODEX_BIN=""
for _n2_a in ${N2_AGENTS:-claude opencode}; do
  case "$_n2_a" in
    claude) CLAUDE_BIN=$(command -v claude) || { echo "premises: claude is not on PATH"; exit 1; } ;;
    opencode) OPENCODE_BIN=$(command -v opencode) || { echo "premises: opencode is not on PATH"; exit 1; } ;;
    codex) CODEX_BIN=$(command -v codex) || { echo "premises: codex is not on PATH"; exit 1; } ;;
    *) echo "premises: unknown agent '$_n2_a' in N2_AGENTS"; exit 1 ;;
  esac
done
. "$PREMISES_DIR/ver.sh"
# n2_scratch_end <status>: for the scratch dir lib.sh made itself (never a caller-set one): after
# status 0 remove it (a failure is named on stderr), else keep it and name it; then exit <status>.
n2_scratch_end() {
  trap - EXIT INT TERM HUP PIPE
  n2_mock_kill
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
  # Never an inherited pid: n2_mock_kill kills MOCK_PID.
  MOCK_PID=""
fi
# n2_mock_kill: stop the mock start_mock started, if it still runs (traps call it).
n2_mock_kill() {
  [ -n "${MOCK_PID:-}" ] || return 0
  kill "$MOCK_PID" 2>/dev/null || true
  wait "$MOCK_PID" 2>/dev/null || true
  MOCK_PID=
}
if [ -z "${N2_SCRATCH:-}" ]; then
  # PIPE too: bash runs an EXIT trap with status 0 after an untrapped PIPE, which would remove the dir.
  trap 'n2_scratch_end $?' EXIT
  trap 'n2_scratch_end 129' HUP
  trap 'n2_scratch_end 130' INT
  trap 'n2_scratch_end 141' PIPE
  trap 'n2_scratch_end 143' TERM
  # Ownership is assigned in the statement that makes the dir (a signal inside mktemp: accepted).
  # The template keeps TMPDIR in force (Darwin's mktemp -d alone prefers its per-user temp dir).
  N2_SCRATCH_OWN=$(mktemp -d "${TMPDIR:-/tmp}/tmp.XXXXXXXXXX") || exit 1
  N2_SCRATCH=$N2_SCRATCH_OWN
  echo "premises: scratch $N2_SCRATCH (removed on exit 0, kept otherwise; set N2_SCRATCH to keep it)" >&2
fi
DUMMY_KEY="dummy-""key"
TOOL_PATH=""
for _n2_b in "$CLAUDE_BIN" "$OPENCODE_BIN" "$CODEX_BIN"; do [ -z "$_n2_b" ] || TOOL_PATH="$TOOL_PATH$(dirname "$_n2_b"):"; done
TOOL_PATH="$TOOL_PATH$(dirname "$(command -v node 2>/dev/null || echo /usr/bin/true)"):/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin"

# new_run <name>: fresh run dir with home/, repo/ (a git repository) and rec/.
new_run() {
  RUN="$N2_SCRATCH/$1"
  rm -rf "$RUN"
  mkdir -p "$RUN/home/.claude" "$RUN/home/.config" "$RUN/home/.local/share" "$RUN/home/.cache" "$RUN/repo" "$RUN/rec"
  H="$RUN/home"
  R="$RUN/repo"
  CX_HOME="$H/.codex" CX_RUN="" CX_USED=""
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
  n2_mock_trap
  i=0
  while [ ! -s "$RUN/port" ]; do i=$((i + 1)); [ $i -lt 100 ] || { echo "mock did not start"; exit 1; }; sleep 0.1; done
  MOCK_URL="http://127.0.0.1:$(cat "$RUN/port")"
}

# n2_mock_trap: EXIT, HUP, INT, PIPE and TERM first stop the mock (n2_mock_kill), then run the trap
# that was set before (lib.sh's n2_scratch_end, live.sh's live_end, a runner's own) with the same
# status, or exit (128 + signal number for a signal). A trap that already calls n2_mock_kill is
# kept. The traps are read with `trap >file` (dash prints none inside $(trap)); a trap command of
# more than one line is not chained (none in this repo). A runner that sets a trap after start_mock
# replaces the chain: call live_traps before start_mock.
n2_mock_trap() {
  trap >"$RUN/.n2-traps"
  for _n2_s in EXIT HUP INT PIPE TERM; do
    _n2_q=$(sed -n "s/^trap -- \(.*\) \(SIG\)\{0,1\}$_n2_s\$/\1/p" "$RUN/.n2-traps")
    _n2_prev=""
    [ -z "$_n2_q" ] || eval "_n2_prev=$_n2_q"
    case "$_n2_prev" in *n2_mock_kill*) continue ;; esac
    case "$_n2_s" in EXIT) _n2_def='exit $_n2_rc' ;; HUP) _n2_def='exit 129' ;; INT) _n2_def='exit 130' ;;
      PIPE) _n2_def='exit 141' ;; TERM) _n2_def='exit 143' ;; esac
    # The if keeps $? for the earlier trap: a bare (exit <status>) would end the trap under set -e.
    # shellcheck disable=SC2064
    trap "_n2_rc=\$?; n2_mock_kill; if (exit \$_n2_rc); then ${_n2_prev:-$_n2_def}; else ${_n2_prev:-$_n2_def}; fi" "$_n2_s"
  done
  rm -f "$RUN/.n2-traps"
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

# --- Codex (node PC-2) ---------------------------------------------------------------------------
# CODEX_MIN: the pinned version is a minimum (K-C8). CODEX_FX: the Codex fixture root.
# CX_HOME: the CODEX_HOME of the next run_codex (new_run sets $H/.codex; a part may set another
# folder under $RUN). CODEX_EXE and CODEX_VERSION are set once per process by codex_env.
# CX_USED: one line per run_codex of this run folder ("<MOCK_URL> <CODEX_EXE> <CODEX_ENV words>
# <EXTRA_ENV words>"); codex_record derives isolation.txt from it.
CODEX_MIN=0.172.0
: "${CODEX_FX:=$REPO_DIR/fixtures/codex}"
CX_HOME="" CX_RUN="" CX_USED="" CODEX_EXE="" CODEX_VERSION="" CODEX_ENV=""
# CODEX_EXTRA_ALLOW: CODEX_* names EXTRA_ENV may set. Empty: no part needs one yet; a part that
# does adds the name here with its reason (it must not move Codex's state out of the run folder).
CODEX_EXTRA_ALLOW=""

# n2_realpath <path>: the real path (the path need not exist).
n2_realpath() { python3 -I -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$1"; }
# n2_under <path> <base>: 0 when the real path of <path> is under the real path of <base>/.
n2_under() {
  _u_p=$(n2_realpath "$1") && _u_b=$(n2_realpath "$2") || return 1
  case "$_u_p" in "$_u_b"/*) return 0 ;; esac
  return 1
}
# n2_plain <name> <value>: exit 1 unless the value is one CODEX_ENV word (no blank, no glob, no
# other shell character): CODEX_ENV is expanded unquoted.
n2_plain() {
  case "$2" in
    '' | *[!A-Za-z0-9_./:@%+,=-]*) echo "premises: $1 holds a blank or a glob character: '$2'"; exit 1 ;;
  esac
}

# codex_exe: set CODEX_EXE, always a copy in $N2_SCRATCH/codex-pkg, so PATH's first entry is under
# $N2_SCRATCH. A codex whose real file starts with "#!" (a wrapper script) is refused. When the real
# path is under the owner's ~/.codex and its folder is named bin, the copy is the package folder
# above bin/ (only under ~/.codex/packages); else the file alone. The copy is made under a temporary
# name and renamed into place; a package copy that holds a symbolic link resolving outside the copy
# is refused and deleted (links are kept as links, never followed). A complete copy (the
# executable exists and is executable) is reused; any other codex-pkg is replaced.
# Nothing else in ~/.codex is read.
codex_exe() {
  [ -z "$CODEX_EXE" ] || return 0
  _cx_real=$(n2_realpath "$CODEX_BIN")
  if [ "$(head -c 2 "$_cx_real")" = '#!' ]; then
    echo "premises: codex at $CODEX_BIN is a script (#!), not the program itself"; exit 1
  fi
  _cx_dir=$(dirname "$_cx_real") _cx_pkg="" _cx_rel=${_cx_real##*/}
  if n2_under "$_cx_real" "$OWNER_HOME/.codex" && [ "${_cx_dir##*/}" = bin ]; then
    _cx_pkg=$(dirname "$_cx_dir")
    n2_under "$_cx_pkg" "$OWNER_HOME/.codex/packages" ||
      { echo "premises: the codex package folder is not under ~/.codex/packages"; exit 1; }
    _cx_rel=bin/$_cx_rel
  fi
  CODEX_EXE=$N2_SCRATCH/codex-pkg/$_cx_rel
  [ ! -f "$CODEX_EXE" ] || [ ! -x "$CODEX_EXE" ] || return 0
  rm -rf "$N2_SCRATCH/codex-pkg"
  _cx_tmp=$(mktemp -d "$N2_SCRATCH/codex-pkg.XXXXXX") || exit 1
  if [ -n "$_cx_pkg" ]; then
    cp -R "$_cx_pkg" "$_cx_tmp/p" || { rm -rf "$_cx_tmp"; exit 1; }
    _cx_link=$(python3 -I - "$_cx_tmp/p" <<'PY'
import os, sys
root = os.path.realpath(sys.argv[1])
for d, dirs, files in os.walk(root):
    for n in sorted(dirs + files):
        p = os.path.join(d, n)
        t = os.path.realpath(p)
        if os.path.islink(p) and t != root and not t.startswith(root + "/"):
            print(os.path.relpath(p, root))
            sys.exit(0)
PY
    ) || { rm -rf "$_cx_tmp"; exit 1; }
    if [ -n "$_cx_link" ]; then
      rm -rf "$_cx_tmp"
      echo "premises: the codex package copy holds a link out of it: $_cx_link"; exit 1
    fi
  else
    mkdir "$_cx_tmp/p" && cp "$_cx_real" "$_cx_tmp/p/$_cx_rel" || { rm -rf "$_cx_tmp"; exit 1; }
  fi
  mv "$_cx_tmp/p" "$N2_SCRATCH/codex-pkg" && rmdir "$_cx_tmp"
}

# n2_extra_check: each EXTRA_ENV word is NAME=value; no name codex_env sets or that moves Codex's
# state, trust or network (PATH TERM HOME TMPDIR MOCK_API_KEY, any *_PROXY or NO_PROXY in either
# case, SSL_CERT_*, CURL_CA_BUNDLE, REQUESTS_CA_BUNDLE, any XDG_*, any CODEX_* not in
# CODEX_EXTRA_ALLOW); no value (or :,;= separated part) that starts with ~ or resolves under the
# owner's home, except under the run folder.
n2_extra_check() {
  for _cx_w in $EXTRA_ENV; do
    _cx_n=${_cx_w%%=*}
    case "$_cx_w" in *=*) ;; *) _cx_n="" ;; esac
    case "$_cx_n" in '' | [0-9]* | *[!A-Za-z0-9_]*) echo "premises: EXTRA_ENV word '$_cx_w' is not NAME=value"; exit 1 ;; esac
    case "$(printf '%s' "$_cx_n" | tr a-z A-Z)" in
      PATH | TERM | HOME | TMPDIR | MOCK_API_KEY | *_PROXY | NO_PROXY | SSL_CERT_* | CURL_CA_BUNDLE | REQUESTS_CA_BUNDLE | XDG_*)
        echo "premises: EXTRA_ENV may not set $_cx_n"; exit 1 ;;
      CODEX_*)
        case " $CODEX_EXTRA_ALLOW " in *" $_cx_n "*) ;; *) echo "premises: EXTRA_ENV may not set $_cx_n"; exit 1 ;; esac ;;
    esac
  done
  [ -n "$EXTRA_ENV" ] && [ -n "$OWNER_HOME" ] || return 0
  # shellcheck disable=SC2086
  _cx_n=$(python3 -I - "$OWNER_HOME" "$RUN" $EXTRA_ENV <<'PY'
import os, re, sys
home, run = (os.path.realpath(p) for p in sys.argv[1:3])
def under(p, b):
    return p == b or p.startswith(b.rstrip("/") + "/")
for w in sys.argv[3:]:
    n, _, v = w.partition("=")
    for part in [v] + re.split(r"[:,;=]", v):
        if part.startswith("~"):
            print(n); sys.exit(0)
        if part.startswith("/"):
            p = os.path.realpath("/" + part.lstrip("/"))
            if home != "/" and under(p, home) and not under(p, run):
                print(n); sys.exit(0)
PY
  ) || exit 1
  [ -z "$_cx_n" ] || { echo "premises: EXTRA_ENV $_cx_n points under the owner's home"; exit 1; }
}

# codex_env: after new_run and start_mock. Checks the isolation (each failure prints its message
# and exits 1 before any codex process starts); writes $CX_HOME/config.toml when it is missing
# (provider "mock", base_url at the mock) and refuses one whose base_url is not "$MOCK_URL/v1"
# (after a mock restart, a part removes or rewrites its config.toml); sets CODEX_ENV: env -i with
# scratch HOME, CODEX_HOME, XDG and TMPDIR folders, the dummy key as MOCK_API_KEY, and HTTPS_PROXY,
# HTTP_PROXY and ALL_PROXY at the mock (K§6.3.7). Once per process: CODEX_VERSION >= CODEX_MIN.
codex_env() {
  CX_HOME=${CX_HOME:-$H/.codex}
  _cx_term=${TERM:-xterm-256color}
  n2_plain RUN "$RUN"; n2_plain HOME "$H"; n2_plain CODEX_HOME "$CX_HOME"; n2_plain TERM "$_cx_term"
  n2_under "$H" "$RUN" || { echo "premises: codex HOME is not under the run folder"; exit 1; }
  n2_under "$CX_HOME" "$RUN" || { echo "premises: CODEX_HOME is not under the run folder"; exit 1; }
  if n2_under "$RUN" "$OWNER_HOME/.codex"; then echo "premises: the run folder is under ~/.codex"; exit 1; fi
  n2_extra_check
  [ -n "${MOCK_URL:-}" ] && [ -n "${MOCK_PID:-}" ] || { echo "premises: codex_env needs start_mock first"; exit 1; }
  n2_plain MOCK_URL "$MOCK_URL"
  codex_exe
  n2_plain CODEX_EXE "$CODEX_EXE"
  if n2_under "$CODEX_EXE" "$OWNER_HOME/.codex"; then echo "premises: the codex executable is under ~/.codex"; exit 1; fi
  mkdir -p "$CX_HOME" "$RUN/tmp" "$H/.local/state"
  if [ -e "$CX_HOME/config.toml" ]; then
    _cx_bu=$(sed -n 's/^[[:space:]]*base_url[[:space:]]*=[[:space:]]*"\(.*\)"[[:space:]]*$/\1/p' "$CX_HOME/config.toml")
    [ "$_cx_bu" = "$MOCK_URL/v1" ] || { echo "premises: config.toml base_url is '${_cx_bu:-none}', not '$MOCK_URL/v1'"; exit 1; }
  else
    printf '%s\n' 'model = "gpt-mock"' 'model_provider = "mock"' '' \
      '[model_providers.mock]' 'name = "mock"' "base_url = \"$MOCK_URL/v1\"" 'env_key = "MOCK_API_KEY"' \
      'wire_api = "responses"' >"$CX_HOME/config.toml"
  fi
  CODEX_ENV="env -i PATH=$(dirname "$CODEX_EXE"):/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin TERM=$_cx_term HOME=$H CODEX_HOME=$CX_HOME XDG_CONFIG_HOME=$H/.config XDG_DATA_HOME=$H/.local/share XDG_CACHE_HOME=$H/.cache XDG_STATE_HOME=$H/.local/state TMPDIR=$RUN/tmp MOCK_API_KEY=$DUMMY_KEY HTTPS_PROXY=$MOCK_URL HTTP_PROXY=$MOCK_URL ALL_PROXY=$MOCK_URL"
  if [ -z "$CODEX_VERSION" ]; then
    _cx_v=$($CODEX_ENV "$CODEX_EXE" --version | head -n 1)
    _cx_v=${_cx_v##* }
    case "$_cx_v" in
      '' | *[!0-9.]* | .* | *. | *..* | *.*.*.*) echo "premises: codex --version gave '$_cx_v', not x.y.z"; exit 1 ;;
      *.*.*) ;;
      *) echo "premises: codex --version gave '$_cx_v', not x.y.z"; exit 1 ;;
    esac
    ver_ge "$_cx_v" "$CODEX_MIN" || { echo "premises: codex $_cx_v is below the minimum $CODEX_MIN"; exit 1; }
    CODEX_VERSION=$_cx_v
  fi
  CX_RUN=$RUN
}

# run_codex <codex args>: codex_env, then codex isolated in the run's repo; returns codex's status.
# Each run adds its line to CX_USED.
run_codex() {
  codex_env
  CX_USED="${CX_USED:+$CX_USED
}$MOCK_URL $CODEX_EXE $CODEX_ENV $EXTRA_ENV"
  _cx_rc=0
  # shellcheck disable=SC2086
  (cd "$R" && $CODEX_ENV $EXTRA_ENV "$CODEX_EXE" "$@") || _cx_rc=$?
  check_abort
  return "$_cx_rc"
}

# codex_record <part>: after stop_mock. Replaces $CODEX_FX/runs/<part>/ with isolation.txt and
# connect.txt (the scrubbed record of the proxy requests the mock refused; empty when none).
# isolation.txt is derived from CX_USED (what every run_codex of this run folder handed to codex):
#   home=scratch          every HOME is under the run folder (else outside-run)
#   codex_home=scratch    every CODEX_HOME is under the run folder (else outside-run)
#   codex_version=<x.y.z>
#   codex_exe=scratch     every executable is under $N2_SCRATCH (outside-owner-home: not under the
#                         owner's home; else owner-home); never a path
#   env_names=<names>     the sorted names of every variable handed to codex
#   proxy=mock            HTTPS_PROXY, HTTP_PROXY and ALL_PROXY each equal that run's MOCK_URL
#                         (else other)
# Any value other than scratch, outside-owner-home or mock is written, named on stdout, and
# codex_record exits 1.
codex_record() {
  case "$1" in '' | *[!A-Za-z0-9_-]*) echo "premises: bad part name '$1'"; exit 1 ;; esac
  [ -n "${CX_RUN:-}" ] && [ "$CX_RUN" = "$RUN" ] && [ -n "$CX_USED" ] || { echo "premises: codex_record: no codex run in this run folder"; exit 1; }
  _cx_out="$CODEX_FX/runs/$1"
  rm -rf "$_cx_out"
  mkdir -p "$_cx_out"
  if [ -e "$RUN/rec/connect.txt" ]; then scrub "$RUN/rec/connect.txt" >"$_cx_out/connect.txt"; else : >"$_cx_out/connect.txt"; fi
  python3 -I - "$_cx_out/isolation.txt" "$RUN" "$N2_SCRATCH" "$OWNER_HOME" "$CODEX_VERSION" "$CX_USED" <<'PY'
import os, sys
out, run, scratch, home, version, used = sys.argv[1:7]
rp = os.path.realpath
def under(p, b):
    return bool(p) and bool(b) and rp(p).startswith(rp(b).rstrip("/") + "/")
bad, names, homes, cxhomes, exes, proxy = [], set(), [], [], [], "mock"
for line in used.splitlines():
    if not line.strip():
        continue
    mock, exe, *words = line.split()
    exes.append(exe)
    env = {}
    for w in words:
        if w in ("env", "-i"):
            continue
        n, _, v = w.partition("=")
        names.add(n)
        env[n] = v
    homes.append(env.get("HOME", ""))
    cxhomes.append(env.get("CODEX_HOME", ""))
    for n in ("HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"):
        if env.get(n) != mock:
            proxy = "other"
            bad.append("%s is not the mock" % n)
def where(label, values):
    out = "scratch"
    for v in values:
        if not under(v, run):
            out = "outside-run"
            bad.append("%s %s is not under the run folder" % (label, v or "''"))
    return out
h, ch = where("HOME", homes), where("CODEX_HOME", cxhomes)
exe = "scratch"
for e in exes:
    if under(e, scratch):
        continue
    if under(e, home):
        exe = "owner-home"
        bad.append("the codex executable is under the owner's home")
    elif exe == "scratch":
        exe = "outside-owner-home"
with open(out, "w") as f:
    f.write("home=%s\ncodex_home=%s\ncodex_version=%s\ncodex_exe=%s\nenv_names=%s\nproxy=%s\n"
            % (h, ch, version, exe, " ".join(sorted(names)), proxy))
for b in dict.fromkeys(bad):
    print("premises: codex_record: " + b)
sys.exit(1 if bad else 0)
PY
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
  tmux -f /dev/null new-session -d -s "$s" -x 200 -y 50 -c "$R" "/bin/sh $RUN/tmux-$s.sh"
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

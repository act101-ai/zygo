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
# MOCK_ALT_KEY (PC-10): only a part sets it, as a shell variable built at run time; one from the
# environment is refused.
if env | grep -q '^MOCK_ALT_KEY='; then
  echo "premises: MOCK_ALT_KEY comes from the environment; a part sets it as a shell variable at run time (unset MOCK_ALT_KEY)"
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
  CX_HOME="$H/.codex"
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

# start_mock [rules.json]: start the mock for this run; sets MOCK_URL and MOCK_PID. Refused (exit 1)
# while the mock of an earlier start_mock still runs: stop_mock first.
# MOCK_ALT_KEY (PC-10, optional): a second credential the mock accepts like the dummy key; each
# request record then says which one it carried ("credential": "primary" or "alt", never a
# value). A part sets it as a shell variable, built at run time (never a literal in the repo),
# before start_mock; one from the environment is refused when lib.sh is sourced.
start_mock() {
  if [ -n "${MOCK_PID:-}" ] && kill -0 "$MOCK_PID" 2>/dev/null; then
    echo "premises: start_mock: the mock (pid $MOCK_PID) still runs; stop_mock first"; exit 1
  fi
  rm -f "$RUN/port"
  MOCK_KEY="$DUMMY_KEY" MOCK_ALT_KEY="${MOCK_ALT_KEY:-}" python3 -I "$PREMISES_DIR/mock_upstream.py" --dir "$RUN/rec" \
    ${1:+--rules "$1"} --port-file "$RUN/port" >"$RUN/mock.log" 2>&1 &
  MOCK_PID=$!
  n2_mock_trap
  i=0
  while [ ! -s "$RUN/port" ]; do i=$((i + 1)); [ $i -lt 100 ] || { echo "mock did not start"; exit 1; }; sleep 0.1; done
  MOCK_URL="http://127.0.0.1:$(cat "$RUN/port")"
}

# n2_trap_chain <command>: EXIT, HUP, INT, PIPE and TERM first run <command>, then the trap that
# was set before (lib.sh's n2_scratch_end, live.sh's live_end, a runner's own) with the same status,
# or exit (128 + signal number for a signal). The earlier trap is held once, in _n2_tp<n> (a chain
# grows by one command per level). A chain that already holds <command> at any level is kept. The
# traps are read with `trap >file` under $RUN (dash prints none inside $(trap)); a trap command of
# more than one line is not chained (none in this repo). A runner that sets a trap after this
# replaces the chain: call live_traps before start_mock.
n2_trap_chain() {
  trap >"$RUN/.n2-traps"
  for _n2_s in EXIT HUP INT PIPE TERM; do
    _n2_q=$(sed -n "s/^trap -- \(.*\) \(SIG\)\{0,1\}$_n2_s\$/\1/p" "$RUN/.n2-traps")
    _n2_prev=""
    [ -z "$_n2_q" ] || eval "_n2_prev=$_n2_q"
    _n2_all=$_n2_prev _n2_t=$_n2_prev
    while _n2_v=$(printf '%s\n' "$_n2_t" | sed -n 's/.* n2_trap_run \$_n2_rc "\$\(_n2_tp[0-9]*\)"$/\1/p') && [ -n "$_n2_v" ]; do
      eval "_n2_t=\${$_n2_v:-}"; _n2_all="$_n2_all
$_n2_t"
    done
    case "$_n2_all" in *"$1"*) continue ;; esac
    case "$_n2_s" in EXIT) _n2_def='exit $_n2_rc' ;; HUP) _n2_def='exit 129' ;; INT) _n2_def='exit 130' ;;
      PIPE) _n2_def='exit 141' ;; TERM) _n2_def='exit 143' ;; esac
    _n2_tn=$((${_n2_tn:-0} + 1))
    eval "_n2_tp$_n2_tn=\${_n2_prev:-\$_n2_def}"
    # shellcheck disable=SC2064
    trap "_n2_rc=\$?; $1; n2_trap_run \$_n2_rc \"\$_n2_tp$_n2_tn\"" "$_n2_s"
  done
  rm -f "$RUN/.n2-traps"
}
# n2_trap_run <status> <command>: run the earlier trap with $? = <status>. The if sets $? (in each
# branch, the condition's status): a bare (exit <status>) would end the trap under set -e.
n2_trap_run() { if (exit "$1"); then eval "$2"; else eval "$2"; fi; }
# n2_mock_trap: the traps first stop the mock (n2_mock_kill).
n2_mock_trap() { n2_trap_chain n2_mock_kill; }

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

# scrub <file>: write a fixture copy to stdout with machine paths and prompt text omitted. One id
# map per part ($N2_SCRATCH/scrub-ids.json, PC-11): a Codex item id or ULID gets the same placeholder
# in every file of the part, and different ids different ones.
scrub() {
  SCRUB_IDMAP="$N2_SCRATCH/scrub-ids.json" python3 -I "$PREMISES_DIR/scrub.py" "$RUN" "$1"
}

# --- Codex (node PC-2) ---------------------------------------------------------------------------
# CODEX_MIN: the pinned version is a minimum (K-C8). CODEX_FX: the Codex fixture root.
# CX_HOME: the CODEX_HOME of the next run_codex (new_run sets $H/.codex; a part may set another
# folder under $RUN). Each codex_env sets CODEX_EXE (a fresh copy in its own $RUN/codex-pkg.XXXXXX)
# and CODEX_VERSION (that copy's --version). $RUN/cx-used.d/: one file of one line per run_codex
# of this run folder ("<version> <MOCK_URL> <CODEX_EXE> <CODEX_ENV words> <EXTRA_ENV words>"), so
# a run in a subshell ($(...), a pipeline, a background job) counts and concurrent runs never share
# a file; codex_record derives isolation.txt from them.
# Daemons (PC-10): a Codex TUI run can start "codex app-server --listen unix:// --managed-daemon"
# that outlives the run, with its socket in the per-uid folder /private/tmp/codex-daemon-<uid>
# (/tmp/... elsewhere), the folder the owner's own Codex uses. TUI runs must pass --no-daemon
# (`codex --help`: "Run without the shared background server"). codex_env refuses to start Codex
# while a codex app-server runs outside the run folder or a daemon folder holds a socket;
# codex_record exits 1 while a process of the run folder still runs.
CODEX_MIN=0.172.0
: "${CODEX_FX:=$REPO_DIR/fixtures/codex}"
CX_HOME="" CODEX_EXE="" CODEX_VERSION="" CODEX_ENV=""
# EXTRA_ENV_ALLOW: the only names EXTRA_ENV may set for a Codex run, each with its reason. A part
# that needs another name adds it here with its reason (it must not move Codex's state, trust or
# network out of the run folder or the mock).
#   RUST_LOG        Codex's log filter (more log lines in the run's own CODEX_HOME/log).
#   RUST_BACKTRACE  a backtrace on a Codex panic (stderr only).
EXTRA_ENV_ALLOW="RUST_LOG RUST_BACKTRACE"

# N2_DAEMON_DIRS: the folders that hold Codex's per-uid daemon folder (codex-daemon-<uid>).
# Assigned here, so a value from the environment is never used; lib_test.sh reassigns it after
# sourcing lib.sh (with n2_procs) to point under its own folder.
N2_DAEMON_DIRS="/private/tmp /tmp"

# n2_procs <daemon|left>: n2_scan with no root (every process of this user). Defined here, so the
# environment cannot replace it; lib_test.sh redefines it after sourcing lib.sh.
n2_procs() { n2_scan "$1" ""; }

# n2_scan <daemon|left> <root>: scan this user's processes (ps; the working folder from
# /proc/<pid>/cwd, else lsof) and the daemon socket folders; print one "premises: ..." line per
# finding and return 1 when there is one. It fails closed (a line, return 1) when ps fails or the
# working folders cannot be read: the lookup must give the working folder of this shell ($$) itself,
# lsof must end within 120 s, and on /proc any error but a process that has ended counts. A
# process is "of this run" when its command line holds the run folder or its working folder is
# inside it. The scanning shell, its parents and the processes the scan starts are skipped; a
# pipeline sibling of the caller is not (call codex_record from the part's shell, not in a
# pipeline).
#   daemon (codex_env, run_codex): each process that is not of this run whose arguments hold the
#     word app-server and whose program is codex (the file name of argv[0] or of ps comm holds
#     "codex", or the command line holds a path ending in /codex before a blank); another
#     session's too (one real run at a time).
#   left (codex_record): each process of this run.
#   both: each socket under <N2_DAEMON_DIRS entry>/codex-daemon-<uid> (it may be stale; never
#     deleted), and a daemon folder that cannot be read.
# No process is stopped and no file is removed. <root> (lib_test.sh only): only processes whose
# command line or working folder holds it are named.
n2_scan() {
  N2_P_MODE=$1 N2_P_ROOT=$2 N2_P_RUN=$RUN N2_P_SELF=$$ N2_P_DIRS=$N2_DAEMON_DIRS python3 -I -c '
import errno, os, re, shutil, stat, subprocess, sys
e = os.environ
mode, run, root, self_pid = e["N2_P_MODE"], e["N2_P_RUN"], e["N2_P_ROOT"], int(e["N2_P_SELF"])
pre, post = ("premises: ", "; codex not started") if mode == "daemon" else ("premises: codex_record: ", "")
def fail(why):
    print(pre + why + post)
    sys.exit(1)
def forms(p):
    return {p.rstrip("/") + "/", os.path.realpath(p).rstrip("/") + "/"}
def under(p, bases):
    return bool(p) and any((p.rstrip("/") + "/").startswith(b) for b in bases)
runs, roots = forms(run), (forms(root) if root else set())
def ps(cols):
    try:
        r = subprocess.run(["ps", "-A", "-ww"] + [x for c in cols for x in ("-o", c + "=")],
                           capture_output=True, text=True, errors="replace", timeout=120)
    except (OSError, subprocess.SubprocessError) as x:
        fail("ps failed (%s)" % x)
    if r.returncode != 0:
        fail("ps failed (exit %d)" % r.returncode)
    return r.stdout.splitlines()
procs, comm = {}, {}
for line in ps(["pid", "ppid", "uid", "args"]):
    t = line.split(None, 3)
    if len(t) >= 3 and t[0].isdigit() and t[1].isdigit() and t[2].isdigit():
        procs[int(t[0])] = (int(t[1]), int(t[2]), t[3] if len(t) == 4 else "")
for line in ps(["pid", "comm"]):
    t = line.split(None, 1)
    if len(t) == 2 and t[0].isdigit():
        comm[int(t[0])] = t[1]
me, uid = os.getpid(), os.getuid()
skip = set()
for p in (me, self_pid):
    while p in procs and p not in skip:
        skip.add(p)
        p = procs[p][0]
skip.add(me)
CODEX_PATH = re.compile(r"(.*?(?:/|^)codex(?:\.exe)?)(?=\s|$)")
cand = {}
for pid, (pp, puid, args) in procs.items():
    if pid in skip or pp == me or puid != uid:
        continue
    words = args.split() or ["?"]
    m = CODEX_PATH.match(args)
    exe = m.group(1) if m else words[0]
    if mode == "daemon":
        if "app-server" not in words[1:]:
            continue
        if not (m or "codex" in os.path.basename(words[0]).lower()
                or "codex" in os.path.basename(comm.get(pid, "")).lower()):
            continue
        if any(r in args for r in runs):
            continue
    cand[pid] = (args, exe)
def cwds(pids):
    """pid -> working folder, for the candidates and this shell; fails closed."""
    pids = set(pids) | {self_pid}
    out = {}
    if os.path.isdir("/proc/self/fd"):
        for pid in pids:
            try:
                out[pid] = os.readlink("/proc/%d/cwd" % pid)
            except OSError as x:
                if x.errno != errno.ENOENT:
                    fail("cannot read process working folders (pid %d: %s)" % (pid, x.strerror))
    else:
        lsof = shutil.which("lsof")
        if not lsof:
            fail("cannot read process working folders (no /proc, no lsof)")
        try:
            r = subprocess.run([lsof, "-n", "-P", "-a", "-d", "cwd", "-p", ",".join(map(str, sorted(pids))), "-F", "pn"],
                               capture_output=True, text=True, errors="replace", timeout=120)
        except subprocess.TimeoutExpired:
            fail("cannot read process working folders (lsof did not end in 120 s)")
        except OSError as x:
            fail("cannot read process working folders (lsof: %s)" % x.strerror)
        if r.returncode != 0 and not r.stdout.strip():
            fail("cannot read process working folders (lsof exit %d, no output)" % r.returncode)
        pid = None
        for line in r.stdout.splitlines():
            if line[:1] == "p":
                pid = int(line[1:]) if line[1:].isdigit() else None
            elif line[:1] == "n" and pid is not None:
                out[pid] = line[1:]
    if self_pid not in out:
        fail("cannot read process working folders (no working folder for this shell, pid %d)" % self_pid)
    return out
cw = cwds(cand)
out = []
for pid, (args, exe) in sorted(cand.items()):
    cwd = cw.get(pid, "")
    if roots and not (any(r in args for r in roots) or under(cwd, roots)):
        continue
    if mode == "daemon":
        if not under(cwd, runs):
            out.append("premises: a codex app-server runs outside this run folder (pid %d, %s); codex not started; it is left running" % (pid, exe))
    elif any(r in args for r in runs):
        out.append(pre + "a process of this run folder still runs (pid %d, %s); stop it (TUI runs pass --no-daemon)" % (pid, exe))
    elif under(cwd, runs):
        out.append(pre + "a process with its cwd in this run folder still runs (pid %d, %s); stop it (TUI runs pass --no-daemon)" % (pid, exe))
seen = set()
for base in e["N2_P_DIRS"].split():
    d = "%s/codex-daemon-%d" % (base.rstrip("/"), uid)
    if os.path.realpath(d) in seen:
        continue
    seen.add(os.path.realpath(d))
    errs = []
    for dp, _, fs in os.walk(d, onerror=errs.append):
        for f in sorted(fs):
            q = os.path.join(dp, f)
            try:
                st = os.lstat(q)
            except FileNotFoundError:
                continue
            except OSError as x:
                errs.append(x)
                continue
            if stat.S_ISSOCK(st.st_mode):
                out.append(pre + "%s is a socket (a Codex daemon, or a stale socket of one; not deleted)%s" % (q, post))
    for x in errs:
        if not isinstance(x, FileNotFoundError):
            out.append(pre + "cannot read %s (%s)%s" % (x.filename or d, x.strerror, post))
for o in out:
    print(o)
sys.exit(1 if out else 0)'
}

# n2_realpath <path>: the real path (the path need not exist).
n2_realpath() { python3 -I -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$1"; }
# n2_under <path> <base>: 0 when the real path of <path> is under the real path of <base>/, letter
# case and all. For a path that must be inside <base>: another case is refused (stricter).
n2_under() {
  _u_p=$(n2_realpath "$1") && _u_b=$(n2_realpath "$2") || return 1
  case "$_u_p" in "$_u_b"/*) return 0 ;; esac
  return 1
}
# n2_under_fold <path> <base>: as n2_under, comparing NFC-normalized, case-folded real paths. For a
# path that must stay out of <base> (the owner's ~/.codex): a macOS folder ignores letter case and
# the Unicode normalization form, so ~/.CODEX is ~/.codex and Jose+U+0301 is José; on a file system
# that keeps them apart this only refuses more.
n2_under_fold() {
  python3 -I -c 'import os, sys, unicodedata
p, b = (unicodedata.normalize("NFC", os.path.realpath(a)).casefold() for a in sys.argv[1:3])
sys.exit(0 if p.startswith(b.rstrip("/") + "/") else 1)' "$1" "$2"
}
# n2_native <file>: 0 when `file -b` names an executable of this host's own format: Mach-O ...
# executable on Darwin, ELF ... executable on Linux, PE32 on Windows (Cygwin, MinGW, MSYS). The
# magic bytes alone are not enough: macOS answers ENOEXEC for a text file that starts with some of
# them, and env then runs it with /bin/sh. Anything else (data, text, a script, a library) is refused.
n2_native() {
  _nv=$(file -b "$1" | head -n 1) || return 1
  case "$(uname -s):$_nv" in
    Darwin:Mach-O*executable* | Linux:ELF*executable* | CYGWIN*:PE32* | MINGW*:PE32* | MSYS*:PE32*) return 0 ;;
  esac
  return 1
}
# n2_plain <name> <value>: exit 1 unless the value is one CODEX_ENV word (no blank, no glob, no
# other shell character): CODEX_ENV is expanded unquoted.
n2_plain() {
  case "$2" in
    '' | *[!A-Za-z0-9_./:@%+,=-]*) echo "premises: $1 holds a blank or a glob character: '$2'"; exit 1 ;;
  esac
}

# codex_exe: set CODEX_EXE to a fresh copy in a folder of its own, $RUN/codex-pkg.XXXXXX/p, made on
# every call (nothing is reused between runs or processes, and no call removes another's copy), so
# PATH's first entry is in the run folder. When the real path of CODEX_BIN is under the owner's
# ~/.codex and its folder is named bin, the copy is the package folder above bin/ (only under
# ~/.codex/packages); else the file alone. A package copy that holds a symbolic link resolving
# outside the copy is refused and deleted (links are kept as links, never followed). The copied
# executable must be a native executable of this host (n2_native): a script, with or without #!,
# is refused. Nothing else in ~/.codex is read.
codex_exe() {
  _cx_real=$(n2_realpath "$CODEX_BIN")
  _cx_dir=$(dirname "$_cx_real") _cx_pkg="" _cx_rel=${_cx_real##*/}
  if n2_under_fold "$_cx_real" "$OWNER_HOME/.codex" && [ "${_cx_dir##*/}" = bin ]; then
    _cx_pkg=$(dirname "$_cx_dir")
    n2_under_fold "$_cx_pkg" "$OWNER_HOME/.codex/packages" ||
      { echo "premises: the codex package folder is not under ~/.codex/packages"; exit 1; }
    _cx_rel=bin/$_cx_rel
  fi
  _cx_tmp=$(mktemp -d "$RUN/codex-pkg.XXXXXX") || exit 1
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
  if [ -L "$_cx_tmp/p/$_cx_rel" ] || ! n2_native "$_cx_tmp/p/$_cx_rel"; then
    rm -rf "$_cx_tmp"
    echo "premises: codex at $CODEX_BIN is not a native program (Mach-O, ELF or PE)"; exit 1
  fi
  CODEX_EXE=$_cx_tmp/p/$_cx_rel
}

# n2_extra_check: each EXTRA_ENV word is NAME=value; no name codex_env sets or that moves Codex's
# state, trust or network (PATH TERM TZ HOME TMPDIR MOCK_API_KEY, any *_PROXY or NO_PROXY in either
# case, SSL_CERT_*, CURL_CA_BUNDLE, REQUESTS_CA_BUNDLE, any XDG_*, any CODEX_* not in
# EXTRA_ENV_ALLOW); then every name must be in EXTRA_ENV_ALLOW, and every value may hold only
# A-Za-z0-9_.:@%+,=- and no "..": no /, \ or ~, so no value names a path.
n2_extra_check() {
  for _cx_w in $EXTRA_ENV; do
    _cx_n=${_cx_w%%=*} _cx_v=${_cx_w#*=}
    case "$_cx_w" in *=*) ;; *) _cx_n="" ;; esac
    case "$_cx_n" in '' | [0-9]* | *[!A-Za-z0-9_]*) echo "premises: EXTRA_ENV word '$_cx_w' is not NAME=value"; exit 1 ;; esac
    case "$(printf '%s' "$_cx_n" | tr a-z A-Z)" in
      PATH | TERM | TZ | HOME | TMPDIR | MOCK_API_KEY | *_PROXY | NO_PROXY | SSL_CERT_* | CURL_CA_BUNDLE | REQUESTS_CA_BUNDLE | XDG_*)
        echo "premises: EXTRA_ENV may not set $_cx_n"; exit 1 ;;
      CODEX_*)
        case " $EXTRA_ENV_ALLOW " in *" $_cx_n "*) ;; *) echo "premises: EXTRA_ENV may not set $_cx_n"; exit 1 ;; esac ;;
    esac
    case " $EXTRA_ENV_ALLOW " in *" $_cx_n "*) ;; *) echo "premises: EXTRA_ENV name $_cx_n is not in EXTRA_ENV_ALLOW"; exit 1 ;; esac
    case "$_cx_v" in
      *[!A-Za-z0-9_.:@%+,=-]* | *..*)
        printf '%s\n' "premises: EXTRA_ENV $_cx_n value may hold only A-Za-z0-9_.:@%+,=- and no ..: '$_cx_v'"; exit 1 ;;
    esac
  done
}

# codex_env: after new_run and start_mock. Checks the isolation (each failure prints its message
# and exits 1 before any codex process starts); writes $CX_HOME/config.toml when it is missing
# (provider "mock", base_url at the mock) and refuses one whose base_url is not "$MOCK_URL/v1"
# (after a mock restart, a part removes or rewrites its config.toml); sets CODEX_ENV: env -i with
# scratch HOME, CODEX_HOME, XDG and TMPDIR folders, the dummy key as MOCK_API_KEY, and HTTPS_PROXY,
# HTTP_PROXY and ALL_PROXY at the mock (K§6.3.7), TERM the constant xterm-256color, TZ the constant
# UTC (PC-10: no host timezone in a request). No SHELL (PC-11): Codex takes the login shell from
# the system user record, not from $SHELL, so scrub.py masks the shell name instead; this is
# inferred from the captures (zsh with no SHELL in the environment), not measured. Every call,
# after its fresh copy: CODEX_VERSION (that copy's --version) >= CODEX_MIN.
codex_env() {
  CX_HOME=${CX_HOME:-$H/.codex}
  n2_plain RUN "$RUN"; n2_plain HOME "$H"; n2_plain CODEX_HOME "$CX_HOME"
  n2_under "$H" "$RUN" || { echo "premises: codex HOME is not under the run folder"; exit 1; }
  n2_under "$CX_HOME" "$RUN" || { echo "premises: CODEX_HOME is not under the run folder"; exit 1; }
  if n2_under_fold "$RUN" "$OWNER_HOME/.codex"; then echo "premises: the run folder is under ~/.codex"; exit 1; fi
  n2_extra_check
  [ -n "${MOCK_URL:-}" ] && [ -n "${MOCK_PID:-}" ] || { echo "premises: codex_env needs start_mock first"; exit 1; }
  n2_plain MOCK_URL "$MOCK_URL"
  codex_exe
  n2_procs daemon || exit 1
  n2_plain CODEX_EXE "$CODEX_EXE"
  if n2_under_fold "$CODEX_EXE" "$OWNER_HOME/.codex"; then echo "premises: the codex executable is under ~/.codex"; exit 1; fi
  mkdir -p "$CX_HOME" "$RUN/tmp" "$H/.local/state"
  if [ -e "$CX_HOME/config.toml" ]; then
    _cx_bu=$(sed -n 's/^[[:space:]]*base_url[[:space:]]*=[[:space:]]*"\(.*\)"[[:space:]]*$/\1/p' "$CX_HOME/config.toml")
    [ "$_cx_bu" = "$MOCK_URL/v1" ] || { echo "premises: config.toml base_url is '${_cx_bu:-none}', not '$MOCK_URL/v1'"; exit 1; }
  else
    # Written whole, then renamed: a concurrent run never reads a part of it.
    _cx_cf=$(mktemp "$CX_HOME/config.toml.XXXXXX") || exit 1
    printf '%s\n' 'model = "gpt-mock"' 'model_provider = "mock"' '' \
      '[model_providers.mock]' 'name = "mock"' "base_url = \"$MOCK_URL/v1\"" 'env_key = "MOCK_API_KEY"' \
      'wire_api = "responses"' >"$_cx_cf"
    mv -f "$_cx_cf" "$CX_HOME/config.toml"
  fi
  CODEX_ENV="env -i PATH=$(dirname "$CODEX_EXE"):/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin TERM=xterm-256color TZ=UTC HOME=$H CODEX_HOME=$CX_HOME XDG_CONFIG_HOME=$H/.config XDG_DATA_HOME=$H/.local/share XDG_CACHE_HOME=$H/.cache XDG_STATE_HOME=$H/.local/state TMPDIR=$RUN/tmp MOCK_API_KEY=$DUMMY_KEY HTTPS_PROXY=$MOCK_URL HTTP_PROXY=$MOCK_URL ALL_PROXY=$MOCK_URL"
  _cx_v=$($CODEX_ENV "$CODEX_EXE" --version | head -n 1)
  _cx_v=${_cx_v##* }
  case "$_cx_v" in
    '' | *[!0-9.]* | .* | *. | *..* | *.*.*.*) echo "premises: codex --version gave '$_cx_v', not x.y.z"; exit 1 ;;
    *.*.*) ;;
    *) echo "premises: codex --version gave '$_cx_v', not x.y.z"; exit 1 ;;
  esac
  ver_ge "$_cx_v" "$CODEX_MIN" || { echo "premises: codex $_cx_v is below the minimum $CODEX_MIN"; exit 1; }
  CODEX_VERSION=$_cx_v
}

# n2_daemon_flag <codex args>: exit 1 unless the run cannot start the shared daemon. The global
# options of `codex --help` (fixtures/codex/help.txt) are skipped, with the value of each that
# takes one (-c --config --enable --disable --remote --remote-auth-token-env -i --image -m --model
# --local-provider -p --profile -s --sandbox -C --cd --add-dir -a --ask-for-approval; "--opt=value"
# is one word); the first other word is the subcommand. It passes when the subcommand is exec, e or
# app-server; when --help, -h, --version or -V stands at the subcommand position or right after the
# subcommand; or when --no-daemon is a global flag or follows the subcommand. Any other run (plain
# codex, a prompt, resume, fork, ...) is a TUI run and must pass --no-daemon.
n2_daemon_flag() {
  _df_sub="" _df_skip=0 _df_after=0
  for _df_a in "$@"; do
    if [ "$_df_skip" = 1 ]; then _df_skip=0; continue; fi
    if [ -z "$_df_sub" ]; then
      case "$_df_a" in
        --no-daemon | --help | -h | --version | -V) return 0 ;;
        -c | --config | --enable | --disable | --remote | --remote-auth-token-env | -i | --image | -m | --model | \
          --local-provider | -p | --profile | -s | --sandbox | -C | --cd | --add-dir | -a | --ask-for-approval) _df_skip=1 ;;
        -*) ;;
        *) _df_sub=$_df_a
           case "$_df_sub" in exec | e | app-server) return 0 ;; esac ;;
      esac
    else
      _df_after=$((_df_after + 1))
      case "$_df_a" in
        --no-daemon) return 0 ;;
        --help | -h | --version | -V) [ "$_df_after" != 1 ] || return 0 ;;
      esac
    fi
  done
  echo "premises: run_codex: 'codex${_df_sub:+ $_df_sub}' is a TUI run; pass --no-daemon"; exit 1
}

# run_codex <codex args>: n2_daemon_flag, codex_env, then codex isolated in the run's repo (the
# daemon check again right before it starts); returns codex's status. Each run first writes its
# line to a file of its own in $RUN/cx-used.d (also from a subshell).
run_codex() {
  n2_daemon_flag "$@"
  codex_env
  mkdir -p "$RUN/cx-used.d"
  _cx_rec=$(mktemp "$RUN/cx-used.d/XXXXXX") || exit 1
  printf '%s\n' "$CODEX_VERSION $MOCK_URL $CODEX_EXE $CODEX_ENV $EXTRA_ENV" >"$_cx_rec"
  _cx_rc=0
  n2_procs daemon || exit 1
  # shellcheck disable=SC2086
  (cd "$R" && $CODEX_ENV $EXTRA_ENV "$CODEX_EXE" "$@") || _cx_rc=$?
  check_abort
  return "$_cx_rc"
}

# codex_record <part>: after stop_mock. Replaces $CODEX_FX/runs/<part>/ with isolation.txt and
# connect.txt (the scrubbed record of the proxy requests the mock refused; empty when none).
# isolation.txt is derived from $RUN/cx-used.d (what every run_codex of this run folder handed to
# codex, from any subshell):
#   home=scratch          every HOME is under the run folder (else outside-run)
#   codex_home=scratch    every CODEX_HOME is under the run folder (else outside-run)
#   codex_version=<x.y.z> the version every run used (several: all of them, comma-separated)
#   run_versions=<list>   the version of each run, one per run_codex, sorted, comma-separated
#   codex_exe=scratch     every file handed to env is in the run folder: a native executable of
#                         this host (codex_env checks it with n2_native) copied there for that run.
#                         It says nothing of what that program reads; reads of ~/.codex are kept
#                         out by the scratch HOME and CODEX_HOME (outside-owner-home: not under
#                         the owner's home, case-folded; else owner-home); never a path
#   env_names=<names>     the sorted names of every variable handed to codex
#   proxy=mock            HTTPS_PROXY, HTTP_PROXY and ALL_PROXY each equal that run's MOCK_URL
#                         (else other)
#   daemon=none           n2_procs left found no process of this run and no daemon socket (else
#                         found; each finding is named on stdout)
# Any value other than scratch, outside-owner-home, mock or none, more than one version, or a
# version below CODEX_MIN (or not x.y.z), is written, named on stdout, and codex_record exits 1.
# First, before it touches the fixture folder, codex_record exits 1 when a file in the run folder
# (the codex copies aside) holds a <timezone> element (also JSON-escaped, \/ slashes, or as &lt;
# &gt; entities) whose value is not UTC (any case; Etc/UTC, GMT, Etc/GMT, UTC+0, UTC+00:00 count
# as UTC), naming the file and the value; or when a file or folder there cannot be read. Limit:
# compressed content is not scanned (Codex's text captures are not compressed; SQLite text pages
# are read as bytes).
codex_record() {
  case "$1" in '' | *[!A-Za-z0-9_-]*) echo "premises: bad part name '$1'"; exit 1 ;; esac
  [ -n "$(ls -A "$RUN/cx-used.d" 2>/dev/null)" ] || { echo "premises: codex_record: no codex run in this run folder"; exit 1; }
  python3 -I - "$RUN" <<'PY'
import os, re, sys
run = sys.argv[1]
OPEN = r"(?:<|\\u003[cC]|&lt;)timezone(?:>|\\u003[eE]|&gt;)"
CLOSE = r"(?:<|\\u003[cC]|&lt;)\\?/timezone"
TZ = re.compile(OPEN + r"((?:\\/|[^<&\\\n]){1,200})" + CLOSE)
UTC = {"utc", "etc/utc", "gmt", "etc/gmt", "utc+0", "utc+00:00"}
bad = []
def err(x):
    bad.append("cannot read %s (%s)" % (os.path.relpath(x.filename or run, run), x.strerror))
for dp, dn, fs in os.walk(run, onerror=err):
    dn[:] = sorted(x for x in dn if not x.startswith("codex-pkg."))
    for fn in sorted(fs):
        q = os.path.join(dp, fn)
        try:
            if os.path.islink(q) or not os.path.isfile(q):
                continue
            with open(q, "rb") as fh:
                text = fh.read().decode("utf-8", "replace")
        except OSError as x:
            err(x)
            continue
        for m in TZ.finditer(text):
            v = m.group(1).replace("\\/", "/").strip()
            if v.lower() not in UTC:
                bad.append("%s holds <timezone>%s</timezone>, not UTC" % (os.path.relpath(q, run), v))
for b in dict.fromkeys(bad):
    print("premises: codex_record: " + b)
sys.exit(1 if bad else 0)
PY
  _cx_dm=$(n2_procs left) && _cx_d=none || _cx_d=found
  [ -z "$_cx_dm" ] || printf '%s\n' "$_cx_dm"
  _cx_out="$CODEX_FX/runs/$1"
  rm -rf "$_cx_out"
  mkdir -p "$_cx_out"
  if [ -e "$RUN/rec/connect.txt" ]; then scrub "$RUN/rec/connect.txt" >"$_cx_out/connect.txt"; else : >"$_cx_out/connect.txt"; fi
  python3 -I - "$_cx_out/isolation.txt" "$RUN" "$OWNER_HOME" "$RUN/cx-used.d" "$CODEX_MIN" "$_cx_d" <<'PY'
import os, sys, unicodedata
out, run, home, used, cmin, daemon = sys.argv[1:7]
rp = os.path.realpath
def under(p, b):
    return bool(p) and bool(b) and rp(p).startswith(rp(b).rstrip("/") + "/")
def fold(p):
    return unicodedata.normalize("NFC", rp(p)).casefold()
def under_fold(p, b):
    return bool(p) and bool(b) and fold(p).startswith(fold(b).rstrip("/") + "/")
def vt(v):
    t = v.split(".")
    return tuple(int(x) for x in t) if len(t) == 3 and all(x.isdigit() for x in t) else None
bad, names, versions, homes, cxhomes, exes, proxy = [], set(), [], [], [], [], "mock"
lines = []
for n in sorted(os.listdir(used)):
    lines += open(os.path.join(used, n)).read().splitlines()
for line in lines:
    if not line.strip():
        continue
    version, mock, exe, *words = line.split()
    versions.append(version)
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
    if under(e, run):
        continue
    if under_fold(e, home):
        exe = "owner-home"
        bad.append("the codex executable is under the owner's home")
    elif exe == "scratch":
        exe = "outside-owner-home"
runs = sorted(versions, key=lambda v: (vt(v) or (), v))
versions = list(dict.fromkeys(runs))
if len(versions) > 1:
    bad.append("more than one codex version: " + ",".join(versions))
for v in versions:
    if vt(v) is None:
        bad.append("codex version '%s' is not x.y.z" % v)
    elif vt(v) < vt(cmin):
        bad.append("codex %s is below the minimum %s" % (v, cmin))
with open(out, "w") as f:
    f.write("home=%s\ncodex_home=%s\ncodex_version=%s\nrun_versions=%s\ncodex_exe=%s\nenv_names=%s\nproxy=%s\ndaemon=%s\n"
            % (h, ch, ",".join(versions), ",".join(runs), exe, " ".join(sorted(names)), proxy, daemon))
for b in dict.fromkeys(bad):
    print("premises: codex_record: " + b)
sys.exit(1 if bad or daemon != "none" else 0)
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

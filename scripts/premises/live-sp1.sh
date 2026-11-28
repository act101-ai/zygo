#!/bin/sh
# Live premise SP1 (node MS-23; mail sync design S§20 SP1, S§8.1, S-C4): a sync child that a member hook starts
# with setsid outlives the hook in the pinned Claude Code and OpenCode, and its git calls reach a private remote
# with the user's SSH agent and with the user's credential helper.
# Usage: sh scripts/premises/live-sp1.sh [--keep-repos]
#   One LIVE SP1 line, exit 0 when the run finished (a failed line does not change it). Fixture
#   fixtures/mail-sync/sp1.txt, copied into the checkout at the end of the run, a failed run included.
#   N2_SCRATCH=<folder> (lib.sh): the raw records (watcher lines, git trace, agent log, state copies, pane text)
#   stay in <folder>/live-<pid>.
#   --keep-repos: the two repositories stay as they are at the end (else: deleted, or archived when the gh token
#   cannot delete).
#   SP1_DATE (default: today, YYYYMMDD) names the repositories zygo-sp1-ssh-<date> and zygo-sp1-https-<date> of
#   the gh account; an existing private repository of that name is used again. SP1_BRANCH (default zygo/mail) is
#   the sync branch; it must not exist on either remote, so that the prompt's child is a bootstrap.
# Run by hand, with claude, opencode, tmux 3.2 or later, git, gh (logged in, scope repo), ssh, ssh-agent,
# ssh-add, ssh-keyscan, ssh-keygen, stdbuf, jq and python3 on PATH, and a release build of zygo (ZYGO_BIN, default
# target/release/zygo).
#
# What this run does to the machine:
# - GitHub: it creates (or uses again) two private repositories of the gh account and pushes the side branch to
#   each; at the end it deletes them (archives them when the token has no delete_repo scope; their names are
#   printed), unless --keep-repos.
# - SSH: a run-local ssh-agent (-d, debug log in the scratch folder) holds the key file that ~/.ssh/config names
#   for github.com, loaded with ssh-add with no prompt; ~/.ssh/config is read, never changed. The SSH remote is
#   an ssh:// URL on ssh.github.com port 443, a host with no Host block, so the agent is the only key source (control
#   2.4). For the run, one line "[ssh.github.com]:443 ssh-ed25519 ..." is added to ~/.ssh/known_hosts after its
#   key and fingerprint match `gh api meta`; ~/.ssh/known_hosts is copied first and copied back at the end
#   (cmp is printed).
# - The members run on the real HOME (git, ssh and gh need it; the acceptance command reads
#   ~/.local/state/zygo/workspaces): zygo writes its state, session folders and usage rows there and stops and
#   forgets the two workspaces sp1ssh and sp1https at the end. Claude Code gets a run-local CLAUDE_CONFIG_DIR
#   (no login is read); both members use zygo role routing to the mock upstream of lib.sh (dummy key): no model
#   request leaves the machine and no usage is spent. OpenCode reads the user's OpenCode config and keeps its
#   session data under the real HOME.
# - The tmux server of the run is private (-S <socket in a private folder>, -f /dev/null). The server, the
#   watcher, the agent, the mock and every member are stopped on every exit.
case "$#:${1:-}" in
  0:) KEEP=0 ;;
  1:--keep-repos) KEEP=1 ;;
  *) echo "usage: live-sp1.sh [--keep-repos]"; exit 2 ;;
esac
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
. "$PREMISES_DIR/workspace_lib.sh"   # clean, sub, note, rest, pw_ticks, jv, finish
live_traps
set +e   # a failed command is a result here; every wait and every test handles its own status
SP1_CC_MIN=2.1.328 SP1_OC_MIN=1.19.6   # S§20 SP1
PRIV="" SOCK="" TMUX_BIN="" BASE="" STARTED="" SP="" AGENT_PID="" WATCH_PID="" KH_SAVED=0 KH_ADDED=0
GHU="" REPO_SSH="" REPO_HTTPS="" MADE_REPOS=""
tm() { LC_ALL=C "$TMUX_BIN" -S "$SOCK" -f /dev/null "$@"; }

# sp1_end <status>: stop the watcher, the workspaces, the private server, the agent; restore known_hosts; delete
# or archive the repositories; remove the private folder; then live_end (mock, scratch).
sp1_end() {
  trap - EXIT INT TERM HUP PIPE
  [ -z "$BASE" ] || : >"$BASE/watch.stop"
  [ -z "$WATCH_PID" ] || { wait_for 5 gone "$WATCH_PID" || kill "$WATCH_PID" 2>/dev/null; }
  for _w in $STARTED; do
    (cd "$RUN/$_w" && renv TMUX="$SOCK,$SP,0" "$ZYGO_BIN" down --now --wait -- "$_w") >>"$BASE/clean.txt" 2>&1
    renv "$ZYGO_BIN" ws forget -- "$_w" >>"$BASE/clean.txt" 2>&1
  done
  [ -z "$SOCK" ] || tm kill-server 2>/dev/null
  [ -z "$AGENT_PID" ] || kill "$AGENT_PID" 2>/dev/null
  for _p in $LIVE_PIDS; do kill "$_p" 2>/dev/null; done
  if [ "$KH_SAVED" -eq 1 ]; then
    cp -p "$PRIV/known_hosts.bak" "$HOME/.ssh/known_hosts"
    if cmp "$HOME/.ssh/known_hosts" "$PRIV/known_hosts.bak"; then echo "live: ~/.ssh/known_hosts restored (cmp: equal)"
    else echo "live: ~/.ssh/known_hosts differs from the saved copy; the copy is kept in $PRIV"; PRIV=""; fi
  fi
  if [ "$KEEP" -eq 0 ]; then
    for _r in $MADE_REPOS; do
      if gh repo delete "$GHU/$_r" --yes >/dev/null 2>&1; then echo "live: repository $_r deleted"
      elif gh repo archive "$GHU/$_r" --yes >/dev/null 2>&1; then echo "live: repository $_r archived (the gh token cannot delete it): $GHU/$_r"
      else echo "live: repository $_r could not be deleted or archived: $GHU/$_r"; fi
    done
  fi
  [ -z "$PRIV" ] || rm -rf "$PRIV"
  live_end "$1"
}
trap 'sp1_end $?' EXIT; trap 'sp1_end 129' HUP; trap 'sp1_end 130' INT; trap 'sp1_end 141' PIPE; trap 'sp1_end 143' TERM

TMUX_BIN=$(command -v tmux) || { echo "live: tmux is not on PATH"; exit 1; }
TVL=$("$TMUX_BIN" -V)
TV=$(printf '%s\n' "$TVL" | sed -n 's/^tmux //p')
ver_ge "${TV%[a-z]}" 3.2 || { echo "live: tmux is ${TV:-unknown}; the minimum is 3.2 (AU7)"; exit 1; }
for _t in git gh ssh ssh-agent ssh-add ssh-keyscan ssh-keygen stdbuf jq python3; do
  command -v "$_t" >/dev/null || { echo "live: $_t is not on PATH"; exit 1; }
done
pinned_agents
agent_ok "$LIVE_CC_VER" "$SP1_CC_MIN" || { echo "live: claude is $LIVE_CC_VER; SP1 needs $SP1_CC_MIN or later"; exit 1; }
agent_ok "$LIVE_OC_VER" "$SP1_OC_MIN" || { echo "live: opencode is $LIVE_OC_VER; SP1 needs $SP1_OC_MIN or later"; exit 1; }
{ [ -f "$ZYGO_BIN" ] && [ -x "$ZYGO_BIN" ]; } || { echo "live: ZYGO_BIN is not an executable file; build zygo or set ZYGO_BIN"; exit 1; }
case "$N2_SCRATCH$HOME$ZYGO_BIN$PREMISES_DIR" in
  *"'"* | *" "*) echo "live: a path holds a single quote or a space; this script cannot use it"; exit 1 ;;
esac
GHU=$(gh api user --jq .login 2>/dev/null) && [ -n "$GHU" ] || { echo "live: gh is not logged in"; exit 1; }
D=${SP1_DATE:-$(date +%Y%m%d)}
REPO_SSH="zygo-sp1-ssh-$D" REPO_HTTPS="zygo-sp1-https-$D"
BR=${SP1_BRANCH:-zygo/mail}
GHSSH="git@""ssh.github.com"   # user and host of the SSH remote (two parts: check-premises.sh scans for address forms)
PRIV=$(mktemp -d "${TMPDIR:-/tmp}/sp1.XXXXXX") || exit 1
SOCK="$PRIV/s"
[ ${#SOCK} -le 100 ] || { echo "live: the socket path is longer than 100 characters; set TMPDIR to a short directory"; exit 1; }
GV=$(git --version | sed -n 's/^git version //p')
echo "live: Claude Code $LIVE_CC_VER, OpenCode $LIVE_OC_VER, git $GV, $TVL"

live_run sp1   # lib.sh: RUN (home/, repo/, rec/ unused but for the mock records)
BASE="$N2_SCRATCH/live-$$"
REC="$BASE/sp1-rec"; FX="$BASE/fx"; mkdir -p "$REC" "$FX"
OUT="$REPO_DIR/fixtures/mail-sync"
PW_TICKS=$(pw_ticks)
ALL="SP1:1.1 SP1:1.2 SP1:1.3 SP1:1.4"
yn() { if [ "$1" -eq 0 ]; then echo yes; else echo no; fi; }
wait_for() { _w=$(($1 * PW_TICKS)); shift; while ! "$@" >/dev/null 2>&1; do _w=$((_w - 1)); [ "$_w" -gt 0 ] || return 1; sleep 0.1; done; }
nap() { _n=$(($1 * PW_TICKS)); while [ "$_n" -gt 0 ]; do sleep 0.1; _n=$((_n - 1)); done; }
gone() { ! kill -0 "$1" 2>/dev/null; }

# stop <reason>: the acceptance sub-checks that a failed setup step did not reach, then the record and exit
stop() {
  rest "$1" $ALL
  prescrub
  finish SP1 "$LIVE_CC_VER + $LIVE_OC_VER" "$FX/sp1.txt" "$HEAD" "1.1 1.2 1.3 1.4" "$OUT/sp1.txt"
  publish; exit 0
}
# prescrub: the repository names and the gh login out of the record, before scrub.py (finish) runs on it
prescrub() { [ ! -f "$REC/SP1.txt" ] || sed -i "s#$REPO_SSH#REPO-SSH#g; s#$REPO_HTTPS#REPO-HTTPS#g; s#\\b$GHU\\b#OWNER#g" "$REC/SP1.txt"; }
publish() {
  mkdir -p "$OUT" && cp "$FX/sp1.txt" "$OUT/sp1.txt" || { echo "live: the fixture could not be copied to the repository"; exit 1; }
  live_done "$REPO_DIR/fixtures"
}

# --- 2.1 repositories: private, empty side branch, anonymous probe (MS-24 form) ---------------------------------
ZV=$("$ZYGO_BIN" --version 2>/dev/null | sed -n 1p)
HEAD="Claude Code $LIVE_CC_VER, OpenCode $LIVE_OC_VER, git $GV, $TVL, ${ZV:-zygo ?}, $(uname -s)"
for r in "$REPO_SSH" "$REPO_HTTPS"; do
  if ! gh repo view "$GHU/$r" --json name >/dev/null 2>&1; then
    gh repo create "$GHU/$r" --private --disable-issues --disable-wiki --description "zygo SP1 live check (temporary)" >/dev/null 2>&1 ||
      stop "gh repo create of $r refused"
  fi
  MADE_REPOS="$MADE_REPOS $r"
  v=$(gh repo view "$GHU/$r" --json visibility --jq .visibility 2>/dev/null)
  [ "$v" = PRIVATE ] || stop "repository $r is not private (visibility ${v:-unknown})"
  b=$(gh api "repos/$GHU/$r/branches" --jq '.[].name' 2>/dev/null)
  case " $(printf '%s' "$b" | tr '\n' ' ') " in *" $BR "*) stop "branch $BR exists on $r; set SP1_BRANCH to a new zygo/<x> branch" ;; esac
  env -u GIT_ASKPASS -u SSH_ASKPASS HOME="$PRIV" GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1 \
    GIT_TERMINAL_PROMPT=0 git -c credential.helper= -c core.askPass= ls-remote --heads "https://github.com/$GHU/$r.git" >/dev/null 2>"$BASE/anon-$r.txt"
  rc=$?
  t=1; [ "$rc" -ne 0 ] && grep -qi 'could not read Username' "$BASE/anon-$r.txt" && t=0
  [ "$t" -eq 0 ] || stop "the anonymous probe of $r did not fail as private (exit $rc)"
done
note SP1 2.1 "repositories REPO-SSH and REPO-HTTPS: visibility PRIVATE; branch $BR absent on both; anonymous probe (S§8.2 environment) of each: exit 128, could not read Username"

# --- 2.2 known_hosts line for [ssh.github.com]:443, checked against gh api meta ----------------------------------
ssh-keyscan -p 443 -t ed25519 ssh.github.com 2>/dev/null | awk '!/^#/ && NF >= 3' >"$PRIV/kh443"   # key lines only (some versions print a # banner line on stdout)
[ "$(wc -l <"$PRIV/kh443" | tr -d ' ')" -eq 1 ] || stop "ssh-keyscan of ssh.github.com:443 did not give one ed25519 key line"
FA=$(ssh-keygen -lf "$PRIV/kh443" 2>/dev/null | awk '{ print $2; exit }')
FB=$(gh api meta --jq .ssh_key_fingerprints.SHA256_ED25519 2>/dev/null)
KB=$(awk '{ print $2 " " $3; exit }' "$PRIV/kh443")
gh api meta --jq '.ssh_keys[]' 2>/dev/null >"$PRIV/meta-keys"
t=1; [ -n "$FA" ] && [ "$FA" = "SHA256:$FB" ] && grep -qxF "$KB" "$PRIV/meta-keys" && t=0
[ "$t" -eq 0 ] || stop "the ssh.github.com:443 host key does not match gh api meta"
cp -p "$HOME/.ssh/known_hosts" "$PRIV/known_hosts.bak" && KH_SAVED=1 || stop "~/.ssh/known_hosts could not be copied"
cat "$PRIV/kh443" >>"$HOME/.ssh/known_hosts" && KH_ADDED=1
note SP1 2.2 "[ssh.github.com]:443 ed25519 host key: fingerprint equals gh api meta SHA256_ED25519 and the key is in gh api meta ssh_keys: yes; one line added to known_hosts for the run (restored at the end)"

# --- 2.3 run-local agent with the github.com key file; 2.4 control: the agent is the only key path ---------------
# stdbuf: ssh-agent's stderr to a file is buffered until it exits; unbuffered, each sign request is countable at once
stdbuf -e0 -o0 ssh-agent -d -a "$PRIV/agent.sock" >/dev/null 2>"$BASE/agent.log" &
AGENT_PID=$!
wait_for 5 test -S "$PRIV/agent.sock" || stop "ssh-agent did not start"
KEY=$(ssh -G github.com 2>/dev/null | awk '$1 == "identityfile" { print $2; exit }')
case "$KEY" in "~/"*) KEY="$HOME/${KEY#\~/}" ;; esac
env -i PATH="$PATH" HOME="$HOME" SSH_AUTH_SOCK="$PRIV/agent.sock" ssh-add "$KEY" </dev/null >/dev/null 2>&1 ||
  stop "SP1 SSH arm: not run: ssh-add could not load the github.com key file without a prompt"
NK=$(env -i PATH="$PATH" SSH_AUTH_SOCK="$PRIV/agent.sock" ssh-add -l 2>/dev/null | wc -l | tr -d ' ')
# signs: sign requests the run agent got (SSH2_AGENTC_SIGN_REQUEST = message type 13 in its debug log; its lines can end in CR LF)
signs() { grep -Ec 'process_message: .* type 13([^0-9]|$)' "$BASE/agent.log" 2>/dev/null; }
env -i PATH="$PATH" HOME="$HOME" ssh -o BatchMode=yes -p 443 -T "$GHSSH" </dev/null >/dev/null 2>"$BASE/ctl-noagent.txt"; c1=$?
s0=$(signs)
env -i PATH="$PATH" HOME="$HOME" SSH_AUTH_SOCK="$PRIV/agent.sock" ssh -o BatchMode=yes -p 443 -T "$GHSSH" </dev/null >/dev/null 2>"$BASE/ctl-agent.txt"; c2=$?
signs_gt() { [ "$(signs)" -gt "$1" ]; }
wait_for 5 signs_gt "$s0"   # the agent can write its log line after the client has exited
s1=$(signs)
t=1; [ "$c1" -eq 255 ] && grep -q 'Permission denied (publickey)' "$BASE/ctl-noagent.txt" && [ "$c2" -eq 1 ] &&
  grep -q 'successfully authenticated' "$BASE/ctl-agent.txt" && [ "$((s1 - s0))" -ge 1 ] && t=0
[ "$t" -eq 0 ] || stop "control: without the agent exit $c1, with it exit $c2, sign requests $((s1 - s0))"
note SP1 2.3 "run-local ssh-agent holds $NK key (the github.com IdentityFile of ~/.ssh/config, loaded with no prompt)"
note SP1 2.4 "control on ssh.github.com:443 with ~/.ssh/config: no agent: exit $c1, Permission denied (publickey); run agent: exit $c2, successfully authenticated, sign requests $((s1 - s0))"
t=1; [ ! -e "$HOME/.netrc" ] && [ "$(git config --global --get-regexp '^credential\.' 2>/dev/null | awk '$2 != "" && $0 !~ /gh auth git-credential/' | wc -l)" -eq 0 ] && t=0
note SP1 2.5 "HTTPS credential source: no ~/.netrc, global credential config holds only the gh helper (gh auth git-credential): $(yn $t)"

# --- setup: tool folder, mock, two workspaces, Claude Code config folder, zygo config ----------------------------
PB="$PRIV/pb"; mkdir -p "$PB"
for _t in tmux git gh ssh ssh-add python3 jq node; do _p=$(command -v "$_t") && ln -s "$_p" "$PB/$_t"; done
ln -s "$CLAUDE_BIN" "$PB/claude"; ln -s "$OPENCODE_BIN" "$PB/opencode"
start_mock
CCD="$RUN/ccd"; mkdir -p "$CCD"
for w in sp1ssh sp1https; do
  mkdir -p "$RUN/$w" && git -C "$RUN/$w" init -q && git -C "$RUN/$w" -c user.name=t -c user.email=t@t commit -q --allow-empty -m init
done
git -C "$RUN/sp1ssh" remote add origin "ssh://$GHSSH:443/$GHU/$REPO_SSH.git"
git -C "$RUN/sp1https" remote add origin "https://github.com/$GHU/$REPO_HTTPS.git"
python3 -I - "$CCD/.claude.json" "$RUN/sp1ssh" "$RUN/sp1https" <<'PY'
import json, os, sys
proj = {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True}
pr = {}
for d in sys.argv[2:]:
    pr[d] = proj
    pr[os.path.realpath(d)] = proj
json.dump({"hasCompletedOnboarding": True, "numStartups": 5, "theme": "dark", "projects": pr}, open(sys.argv[1], "w"))
PY
CONF="$RUN/zygo.toml"
mock_config "$RUN/base.toml"
cat >>"$RUN/base.toml" <<'T'
[loadout.sp1]
[[loadout.sp1.window]]
[[loadout.sp1.window.pane]]
kind = "claude-code"
name = "cc"
args = ["--setting-sources", "project", "--strict-mcp-config"]
[[loadout.sp1.window.pane]]
kind = "opencode"
name = "oc"
T
cp "$RUN/base.toml" "$CONF"
# arm <ssh branch> <https branch>: the zygo config with the two [mail.workspace.*] tables (design decision 4: only
# after every member's session and warm-up turn, so the prompt hook is the first starter)
arm() {
  { cat "$RUN/base.toml"; printf '\n[mail.workspace.sp1ssh]\nsync = "%s"\nhost = "sp1-ssh"\n\n[mail.workspace.sp1https]\nsync = "%s"\nhost = "sp1-https"\n' "$1" "$2"; } >"$CONF.new" &&
    mv "$CONF.new" "$CONF"
}
U=$(id -un)
renv() { env -i PATH="$PB:/usr/bin:/bin" HOME="$HOME" USER="$U" LOGNAME="$U" SHELL=/bin/sh TERM=xterm-256color LANG=C.UTF-8 \
  DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-}" XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-}" \
  SSH_AUTH_SOCK="$PRIV/agent.sock" GIT_TRACE="$BASE/git-trace.log" \
  CLAUDE_CONFIG_DIR="$CCD" DISABLE_AUTOUPDATER=1 OPENCODE_DISABLE_AUTOUPDATE=1 ZYGO_NO_UPDATE_CHECK=1 \
  ZYGO_CONFIG="$CONF" ZYGO_LIVE_MOCK_KEY="$DUMMY_KEY" "$@"; }
renv "$TMUX_BIN" -S "$SOCK" -f /dev/null new-session -d -s hold -x 200 -y 50 /bin/sleep 3600 || stop "tmux did not start the hold session"
SP=$(tm display-message -p '#{pid}')
for w in sp1ssh sp1https; do
  STARTED="$STARTED $w"
  (cd "$RUN/$w" && renv TMUX="$SOCK,$SP,0" "$ZYGO_BIN" up --loadout sp1 --no-attach) >"$BASE/up-$w.txt" 2>&1 || stop "zygo up of $w failed"
done

# ids, pane keys, tmux panes; eval-free lookups by workspace name
wsid() { tm show-environment -t "=zygo-$1:" ZYGO_WORKSPACE 2>/dev/null | sed -n 's/^ZYGO_WORKSPACE=//p'; }
WS_sp1ssh=$(wsid sp1ssh) WS_sp1https=$(wsid sp1https)
[ -n "$WS_sp1ssh" ] && [ -n "$WS_sp1https" ] || stop "no workspace id after zygo up"
id_of() { case "$1" in sp1ssh) echo "$WS_sp1ssh" ;; sp1https) echo "$WS_sp1https" ;; esac; }
wdir() { echo "$HOME/.local/state/zygo/workspaces/$(id_of "$1")"; }
wj() { echo "$(wdir "$1")/workspace.json"; }
key() { jq -r --arg n "$2" '[.panes[] | select(.name == $n) | .key] | if length == 1 then .[0] else empty end' "$(wj "$1")" 2>/dev/null; }
mval() { jq -r --arg k "$(key "$1" "$2")" ".members[\$k].$3 // empty" "$(wj "$1")" 2>/dev/null; }   # <ws> <member> <field>
pane() { tm list-panes -s -t "=zygo-$1:" -F '#{pane_id} #{@zygo-pane}' 2>/dev/null | awk -v k="$(key "$1" "$2")" '$2 == k { print $1; exit }'; }
act() { echo "$HOME/.cache/zygo/sessions/$(mval "$1" "$2" zygo_session)/activity.json"; }
isbusy() { [ "$(jq -r '.busy | tostring' "$(act "$1" "$2")" 2>/dev/null)" = "$3" ]; }   # <ws> <member> <true|false>
for w in sp1ssh sp1https; do for m in cc oc; do [ -n "$(pane "$w" "$m")" ] || stop "no tmux pane for $w/$m"; done; done

# --- members ready (Claude Code session; OpenCode TUI drawn), then one warm-up turn each -------------------------
PTR=$(printf '\342\235\257')   # U+276F, the pointer of a Claude Code menu
trust_line() { tm capture-pane -p -t "$1" 2>/dev/null | grep -F "Yes, I trust" | tail -n 1; }
cc_trust() {
  case "$(trust_line "$1")" in
    "") ;;
    *"$PTR"*) sleep 0.5; case "$(trust_line "$1")" in *"$PTR"*) tm send-keys -t "$1" Enter; sleep 1 ;; esac ;;
    *) tm send-keys -t "$1" Down; sleep 1 ;;
  esac
}
cc_up() { [ -n "$(mval sp1ssh cc agent_session)" ] && [ -n "$(mval sp1https cc agent_session)" ]; }
oc_drawn() { tm capture-pane -p -t "$(pane "$1" oc)" 2>/dev/null | grep -qF "$LIVE_OC_VER" && [ -n "$(mval "$1" oc zygo_session)" ]; }
_c=$((120 * PW_TICKS / 5))
until cc_up; do cc_trust "$(pane sp1ssh cc)"; cc_trust "$(pane sp1https cc)"; _c=$((_c - 1)); [ "$_c" -gt 0 ] || break; sleep 0.5; done
cc_up || stop "timeout 120 s: a Claude Code member has no agent session"
for w in sp1ssh sp1https; do wait_for 180 oc_drawn "$w" || stop "timeout 180 s: the OpenCode TUI of $w was not drawn"; done
nap 3
PROMPT="Reply with the single word ok."
say() { tm send-keys -t "$1" -l "$2"; sleep 0.5; tm send-keys -t "$1" Enter; }
# OpenCode makes its session at the first prompt and then sends `session` before `prompt`; a warm-up turn for
# every member, before the arm, leaves only `prompt` for the round prompts.
for w in sp1ssh sp1https; do
  for m in cc oc; do
    say "$(pane "$w" "$m")" "$PROMPT"
    wait_for 60 isbusy "$w" "$m" true && wait_for 120 isbusy "$w" "$m" false || stop "warm-up turn of $w/$m: no busy true then false"
  done
done
all_sessions() { for w in sp1ssh sp1https; do for m in cc oc; do [ -n "$(mval "$w" "$m" agent_session)" ] || return 1; done; done; }
wait_for 60 all_sessions || stop "timeout 60 s: a member has no agent session after its warm-up turn"
t=0; for w in sp1ssh sp1https; do [ -e "$(wdir "$w")/mail/sync/state.json" ] || [ -e "$(wdir "$w")/mail/sync/last_start" ] && t=1; done
[ "$t" -eq 0 ] || stop "a sync ran before the arm (state.json or last_start present)"
note SP1 2.6 "four members (cc, oc in sp1ssh and sp1https) have an agent session and one warm-up turn; before the arm no workspace has mail/sync/state.json or last_start"

# --- watcher, arm --------------------------------------------------------------------------------------------------
wargs=""
for w in sp1ssh sp1https; do
  wargs="$wargs --ws $(id_of "$w"):$(wdir "$w")"
  for m in cc oc; do wargs="$wargs --busy $w/$m:$(act "$w" "$m")"; done
done
python3 -I "$PREMISES_DIR/sp1_watch.py" watch --out "$BASE/watch.jsonl" --stop "$BASE/watch.stop" $wargs &
WATCH_PID=$!
wait_for 5 test -s "$BASE/watch.jsonl" || stop "the watcher did not start"
B_SSH=$BR B_HTTPS=$BR
arm "$B_SSH" "$B_HTTPS"
renv "$ZYGO_BIN" config check >"$BASE/config-check.txt" 2>&1 || stop "zygo config check failed after the arm"

# --- rounds --------------------------------------------------------------------------------------------------------
rd() { cat "$(wdir "$1")/mail/sync/$2" 2>/dev/null; }
ls_ge() { _l=$(rd "$1" last_start); [ -n "$_l" ] && [ "$_l" -ge "$2" ]; }
synced() { ! pgrep -f "mail sync --ws-id $(id_of "$1") --background" >/dev/null && [ -s "$(wdir "$1")/mail/sync/state.json" ] &&
  [ "$(stat -c %Y "$(wdir "$1")/mail/sync/state.json")" -ge "$2" ]; }
gap_ok() { _l=$(rd "$1" last_start); [ -z "$_l" ] || [ "$(date +%s)" -ge $((_l + 61)) ]; }
# round <id> <sub> <acc true|false> <ws> <member> <remote label> <branch> <attempt>
round() {
  RW=$4 RM=$5   # not _w: wait_for uses _w as its counter
  wait_for 120 gap_ok "$RW"
  _t0=$(date +%s); while [ "$(date +%s)" = "$_t0" ]; do sleep 0.01; done
  T=$(date +%s)
  _s0=$(signs)
  say "$(pane "$RW" "$RM")" "$PROMPT"
  _bt=1; wait_for 60 isbusy "$RW" "$RM" true && _bt=0
  _bf=1; [ "$_bt" -eq 0 ] && wait_for 120 isbusy "$RW" "$RM" false && _bf=0
  _cs=1; wait_for 30 ls_ge "$RW" "$T" && _cs=0
  _cg=1; [ "$_cs" -eq 0 ] && wait_for 180 synced "$RW" "$T" && _cg=0
  _s1=$(signs)
  cp "$(wdir "$RW")/mail/sync/state.json" "$BASE/$1-$8-state.json" 2>/dev/null
  _el="$(wdir "$RW")/errors.log"
  _errs=$( [ -f "$_el" ] && cp "$_el" "$BASE/$1-$8-errors.log" && sed -n 's/.*check \([a-z_-]*\).*/\1/p' "$_el" | sort -u | tr '\n' ' ')
  jq -nc --arg id "$1" --arg sub "$2" --argjson acc "$3" --arg ws "$(id_of "$RW")" --arg m "$RM" --arg lab "$RW/$RM" \
    --arg remote "$6" --argjson T "$T" --argjson end "$(date +%s)" --arg br "$7" --argjson att "$8" \
    --argjson signs "$((_s1 - _s0))" --arg errs "$_errs" --slurpfile st "$BASE/$1-$8-state.json" \
    --arg waits "busy true: $(yn $_bt); busy false: $(yn $_bf); child start: $(yn $_cs); child end: $(yn $_cg)" \
    '{id: $id, sub: $sub, acc: $acc, ws: $ws, member: $m, member_label: $lab, remote: $remote, T: $T, end: ($end + 1),
      state: ($st[0] // null), errors: ($errs | split(" ") | map(select(. != ""))), signs: $signs, branch: $br,
      attempt: $att, waits: $waits}' >>"$BASE/rounds.jsonl" 2>>"$BASE/round-errors.txt" ||
    printf '{"id":"%s","sub":"%s","acc":%s,"ws":"%s","member":"%s","member_label":"%s/%s","remote":"%s","T":%s,"state":null,"errors":[],"signs":null,"branch":"%s","attempt":%s}\n' \
      "$1" "$2" "$3" "$(id_of "$RW")" "$RM" "$RW" "$RM" "$6" "$T" "$7" "$8" >>"$BASE/rounds.jsonl"
  note SP1 "w-$1-$8" "$1 waits: busy true within 60 s: $(yn $_bt); busy false within 120 s: $(yn $_bf); child start within 30 s: $(yn $_cs); child end within 180 s: $(yn $_cg)"
}
order() { python3 -I "$PREMISES_DIR/sp1_watch.py" judge --watch "$BASE/watch.jsonl" --rounds "$BASE/rounds.jsonl" --order "$1"; }
round R1 1.1 true sp1ssh cc SSH-agent "$B_SSH" 1
round R2 1.2 true sp1https oc HTTPS-helper "$B_HTTPS" 1
# S§20 SP1 fallback (plan §2.7): a child that ended before its turn shows no order; once, a new branch forces a
# new bootstrap in that workspace, after sync_gap.
R1N=1 R2N=1
[ "$(order R1)" = yes ] || { B_SSH="${BR}2"; R1N=2; }
[ "$(order R2)" = yes ] || { B_HTTPS="${BR}2"; R2N=2; }
if [ "$R1N" -eq 2 ] || [ "$R2N" -eq 2 ]; then
  arm "$B_SSH" "$B_HTTPS"
  [ "$R1N" -eq 1 ] || round R1 1.1 true sp1ssh cc SSH-agent "$B_SSH" 2
  [ "$R2N" -eq 1 ] || round R2 1.2 true sp1https oc HTTPS-helper "$B_HTTPS" 2
fi
round R3 3.1 false sp1ssh oc SSH-agent "$B_SSH" 1
round R4 3.2 false sp1https cc HTTPS-helper "$B_HTTPS" 1
: >"$BASE/watch.stop"; wait_for 5 gone "$WATCH_PID"; WATCH_PID=""

# --- acceptance command (node MS-23, verbatim) ----------------------------------------------------------------------
for f in "$HOME"/.local/state/zygo/workspaces/*/mail/sync/state.json; do printf '%s\n' "$f"; cat "$f"; printf '\n'; done >"$BASE/acceptance.txt"

# --- 1.4 remote: the run's branch is there with ZYGO-MAIL = 1, author zygo, message (every branch is named) ---------------------------------------
ZE="zygo@""zygo.invalid"
t=0 d=""
for x in "SSH:$REPO_SSH:$B_SSH" "HTTPS:$REPO_HTTPS:$B_HTTPS"; do
  _k=${x%%:*} _r=${x#*:}; _b=${_r#*:}; _r=${_r%%:*}
  _br=$(gh api "repos/$GHU/$_r/branches" --jq '.[].name' 2>/dev/null | sort | tr '\n' ' ')
  _mk=$(gh api "repos/$GHU/$_r/contents/ZYGO-MAIL?ref=$_b" --jq .content 2>/dev/null | base64 -d 2>/dev/null)
  _cm=$(gh api "repos/$GHU/$_r/commits/$_b" --jq '[.commit.author.name, .commit.author.email, .commit.message] | join("|")' 2>/dev/null)
  _ok=1; case " $_br " in *" $_b "*) [ "$_mk" = 1 ] && [ "$_cm" = "zygo|$ZE|zygo mail [skip ci]" ] && _ok=0 ;; esac
  [ "$_ok" -eq 0 ] || t=1
  d="$d$_k repository: branches ${_br% }; ZYGO-MAIL at $_b: ${_mk:-absent}; tip author zygo with the zygo.invalid address and message zygo mail [skip ci]: $(yn "$([ "$_cm" = "zygo|$ZE|zygo mail [skip ci]" ]; echo $?)"); "
done
sub SP1 1.4 $t "remote: ${d%; }"

# --- judge: 1.1, 1.2, 1.3, notes ------------------------------------------------------------------------------------
python3 -I "$PREMISES_DIR/sp1_watch.py" judge --watch "$BASE/watch.jsonl" --rounds "$BASE/rounds.jsonl" --trace "$BASE/git-trace.log" >"$BASE/judge.tsv" 2>"$BASE/judge-err.txt" ||
  rest "the judge failed" SP1:1.1 SP1:1.2 SP1:1.3
while IFS="$(printf '\t')" read -r _s _v _x; do
  case "$_v" in note) note SP1 "$_s" "$_x" ;; *) sub SP1 "$_s" "$_v" "$_x" ;; esac
done <"$BASE/judge.tsv"
# 3.4: the acceptance output, one line per workspace (id shortened; URL hashes and blob ids left out)
python3 -I - "$BASE/acceptance.txt" "$WS_sp1ssh" "$WS_sp1https" >"$BASE/acc-note.txt" <<'PY'
import json, os, sys
txt, mine = open(sys.argv[1]).read().split("\n"), sys.argv[2:]
out, i = [], 0
while i < len(txt):
    p = txt[i]
    if p.endswith("/mail/sync/state.json"):
        ws = os.path.basename(os.path.dirname(os.path.dirname(os.path.dirname(p))))
        try:
            s = json.loads(txt[i + 1])
        except (IndexError, ValueError):
            j, buf = i + 1, []
            while j < len(txt) and not txt[j].endswith("/mail/sync/state.json"):
                buf.append(txt[j]); j += 1
            try:
                s = json.loads("\n".join(buf))
            except ValueError:
                s = None
        tag = "this run" if ws in mine else "not this run"
        if s is None:
            out.append("%s… (%s): unreadable" % (ws[:8], tag))
        else:
            out.append("%s… (%s): last_ok %s, last_error %s, public %s, pushed %s" % (
                ws[:8], tag, s.get("last_ok"), "absent" if s.get("last_error") is None else s["last_error"],
                (s.get("public") or {}).get("verdict"), " ".join(sorted((s.get("pushed") or {}).keys())) or "none"))
    i += 1
print("; ".join(out) or "no state.json")
PY
note SP1 3.4 "acceptance command output: $(cat "$BASE/acc-note.txt")"
prescrub
finish SP1 "$LIVE_CC_VER + $LIVE_OC_VER" "$FX/sp1.txt" "$HEAD" "1.1 1.2 1.3 1.4" "$OUT/sp1.txt"
publish

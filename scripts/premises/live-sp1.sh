#!/bin/sh
# Live premise SP1 (node MS-23; mail sync design S§20 SP1, S§8.1, S-C4): a sync child that a member hook starts
# with setsid outlives the hook in the pinned Claude Code and OpenCode, and its git calls reach a private remote in
# three arms: (a) ssh-agent: an SSH remote with a run-local ssh-agent whose socket is only in the environment of
# `zygo up` (it must reach the child through zygo); (b) ssh-key: an SSH remote on github.com with the user's
# ~/.ssh/config (its key file, IdentityAgent none) and no agent; (c) https: an HTTPS remote with the user's git
# credential helper (gh). Each arm runs one round in Claude Code and one in OpenCode.
# Usage: sh scripts/premises/live-sp1.sh [--keep-repos]
#   One LIVE SP1 line, exit 0 when the run finished (a failed line does not change it). Fixture
#   fixtures/mail-sync/sp1.txt, copied into the checkout at the end of the run, a failed run included.
#   N2_SCRATCH=<folder> (lib.sh): the raw records (watcher lines, git trace2 events, agent log, state copies) stay
#   in <folder>/live-<pid>.
#   --keep-repos: the repositories this run created stay as they are at the end (else: deleted, or archived when
#   the gh token cannot delete).
#   SP1_DATE (default: today YYYYMMDD and the letter r with the process id) names the repositories
#   zygo-sp1-{ssh,key,https}-<SP1_DATE> of the gh account. A name that exists (archived or not) stops the run
#   before any write; the run never changes a repository that it did not create. SP1_BRANCH (default zygo/mail)
#   is the sync branch.
#   SP1_TEST_PAUSE=<s> (tests of the cleanup only): a pause of <s> seconds after the setup (known_hosts line,
#   agent, mock, tmux server; no repository yet) and between the cleanup steps, for a signal test.
# Run by hand, with claude, opencode, tmux 3.2 or later, git, gh (logged in, scope repo), ssh, ssh-agent,
# ssh-add, ssh-keyscan, ssh-keygen, stdbuf, timeout, jq and python3 on PATH, and a release build of zygo
# (ZYGO_BIN, default target/release/zygo).
#
# What this run does to the machine:
# - GitHub: it creates three private repositories of the gh account (new names only) and the members' sync pushes
#   the sync branch to each; at the end it deletes them (archives them when the token has no delete_repo scope;
#   their names are printed), unless --keep-repos.
# - SSH: a run-local ssh-agent (-d, debug log in the scratch folder) holds the key file that ~/.ssh/config names
#   for github.com, loaded with ssh-add with no prompt; ~/.ssh/config is read, never changed. One line
#   "[ssh.github.com]:443 ssh-ed25519 ..." is added to ~/.ssh/known_hosts after its key and fingerprint match
#   `gh api meta` (sp1_watch.py kh-add): when ~/.ssh/known_hosts is a link, the file it names is read and changed and
#   the link stays; a copy of the file's bytes is saved first; the line counts as added only after the append was
#   written, flushed and fsync'd (a failed append is recorded as not added, the copy is kept, the run stops). At the
#   end (kh-remove) only that line is removed (exact match, the last copy), in place in the same file (a link stays a
#   link; the file keeps its inode, mode and every other byte), and that file is compared with the saved copy.
# - The members run on the real HOME (git, ssh and gh need it; the acceptance command reads only this run's
#   workspaces under ~/.local/state/zygo/workspaces, by id): zygo writes its state, session folders and usage rows there and stops and
#   forgets the three workspaces at the end. Claude Code gets a fresh CLAUDE_CONFIG_DIR (no login file in it) with
#   CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 and the official marketplace auto-install off (environment and the
#   config's own "attempted, policy_blocked" state). OpenCode gets fresh XDG config, data, state and cache folders
#   (no auth.json in them); the config folder links gh and git to the user's ~/.config/gh and ~/.config/git so
#   that git and its credential helper work, and holds a copy of the package files (package.json, lock file,
#   node_modules; not opencode.json) of ~/.config/opencode. OpenCode itself also looks for ~/.opencode/opencode.json.
#   OpenCode installs @opencode-ai/plugin with npm's library into each config folder without a lock file (the zygo
#   session folder too); npm_config_offline=true and npm_config_registry=<closed loopback port> keep that
#   install on the machine (it fails there; OpenCode logs it and goes on). Model requests go to the mock upstream of lib.sh (zygo role routing,
#   dummy key); the run checks that each round's prompt reached the mock.
# - The tmux server of the run is private (-S <socket in a private folder>, -f /dev/null). Every git under it
#   writes trace2 events to the scratch folder (GIT_TRACE2_EVENT). The cleanup order on every exit: watcher;
#   sync config removed and the run's sync children killed; agent; mock; the known_hosts line; the repositories;
#   zygo down and ws forget (timeout 60 each); the tmux server. INT, TERM, HUP and PIPE are ignored during the
#   cleanup, so that a second signal cannot skip a step.
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
PRIV="" SOCK="" TMUX_BIN="" BASE="" STARTED="" SP="" AGENT_PID="" WATCH_PID="" KH_ADDED=0 CONF="" WSIDS=""
GHU="" MADE_REPOS="" TD=0 PW_TICKS=10
WS_LIST="sp1ssh sp1key sp1https"
tm() { LC_ALL=C "$TMUX_BIN" -S "$SOCK" -f /dev/null "$@"; }
wait_for() { _w=$(($1 * PW_TICKS)); shift; while ! "$@" >/dev/null 2>&1; do _w=$((_w - 1)); [ "$_w" -gt 0 ] || return 1; sleep 0.1; done; }
gone() { ! kill -0 "$1" 2>/dev/null; }
yn() { if [ "$1" -eq 0 ]; then echo yes; else echo no; fi; }
# tp <step>: the SP1_TEST_PAUSE pause (wait: a signal ends it at once while the traps are set)
tp() { case "${SP1_TEST_PAUSE:-}" in "" | *[!0-9]*) ;; *) echo "live: test pause $SP1_TEST_PAUSE s ($1)"; sleep "$SP1_TEST_PAUSE" & wait $! ;; esac; }
say_clean() { echo "live: $1"; [ -z "$BASE" ] || printf '%s\n' "$1" >>"$BASE/cleanup.txt"; }
kids_left() { _n=0; for _i in $WSIDS; do pgrep -f "mail sync --ws-id $_i --background" >/dev/null && _n=$((_n + 1)); done; echo "$_n"; }
no_kids() { [ "$(kids_left)" -eq 0 ]; }

# teardown: the cleanup, once (also on the normal path before the record is finished); signals ignored meanwhile
teardown() {
  [ "$TD" -eq 0 ] || return 0
  TD=1
  trap '' INT TERM HUP PIPE
  [ -z "$BASE" ] || : >"$BASE/watch.stop"
  [ -z "$WATCH_PID" ] || { wait_for 5 gone "$WATCH_PID" || kill "$WATCH_PID" 2>/dev/null; }
  WATCH_PID=""
  # 1. no new sync child (config without [mail.workspace.*]), then the run's children, the agent, the mock
  [ -z "$CONF" ] || [ ! -f "$RUN/base.toml" ] || cp "$RUN/base.toml" "$CONF"
  for _i in $WSIDS; do pkill -f "mail sync --ws-id $_i --background" 2>/dev/null; done
  wait_for 10 no_kids || for _i in $WSIDS; do pkill -KILL -f "mail sync --ws-id $_i --background" 2>/dev/null; done
  if [ -n "$AGENT_PID" ]; then
    kill "$AGENT_PID" 2>/dev/null; wait_for 5 gone "$AGENT_PID" || kill -KILL "$AGENT_PID" 2>/dev/null
    say_clean "run ssh-agent stopped: $(gone "$AGENT_PID" && echo yes || echo no)"; AGENT_PID=""
  fi
  [ -z "${MOCK_PID:-}" ] || { kill "$MOCK_PID" 2>/dev/null; wait_for 5 gone "$MOCK_PID"; }
  tp cleanup-1
  # 2. the added known_hosts line only (exact match, the last one), in place in the file that ~/.ssh/known_hosts names
  # (a link stays a link), then that file against the copy saved before the line was added (kh-remove, sp1_watch.py)
  if [ "$KH_ADDED" -eq 1 ]; then
    IFS="$(printf '\t')" read -r _r _miss _more <<EOF
$(python3 -I "$PREMISES_DIR/sp1_watch.py" kh-remove --file "$HOME/.ssh/known_hosts" --line "$PRIV/kh443" --start "$PRIV/known_hosts.start")
EOF
    say_clean "known_hosts: added line ${_r:-not checked (kh-remove failed)}; against the copy saved before the line was added: lines missing ${_miss:-?}, lines added by other processes ${_more:-?}"
    if [ "$_r" = removed ] && [ "$_miss" = 0 ]; then KH_ADDED=0; else echo "live: the saved known_hosts copy is kept in $PRIV"; PRIV_KEEP=1; fi
    tp cleanup-2
  fi
  # 3. the repositories this run created (only those)
  if [ -n "$MADE_REPOS" ]; then
    if [ "$KEEP" -eq 1 ]; then say_clean "repositories kept (--keep-repos):$(printf ' %s' $MADE_REPOS)"
    else
      for _r in $MADE_REPOS; do
        if gh repo delete "$GHU/$_r" --yes >/dev/null 2>&1; then say_clean "repository $_r deleted"
        elif gh repo archive "$GHU/$_r" --yes >/dev/null 2>&1; then say_clean "repository $_r archived (the gh token cannot delete it)"; echo "live: archived $GHU/$_r"
        else say_clean "repository $_r could not be deleted or archived"; echo "live: left $GHU/$_r"; fi
      done
    fi
    MADE_REPOS=""
  fi
  # 4. workspaces, the private server, children a down started
  for _w in $STARTED; do
    (cd "$RUN/$_w" && renv TMUX="$SOCK,$SP,0" timeout 60 "$ZYGO_BIN" down --now --wait -- "$_w") >>"$BASE/clean.txt" 2>&1
    renv timeout 60 "$ZYGO_BIN" ws forget -- "$_w" >>"$BASE/clean.txt" 2>&1
  done
  STARTED=""
  [ -z "$SOCK" ] || tm kill-server 2>/dev/null
  for _i in $WSIDS; do pkill -KILL -f "mail sync --ws-id $_i --background" 2>/dev/null; done
  for _p in $LIVE_PIDS; do kill "$_p" 2>/dev/null; done
  say_clean "sync children of the run left: $(kids_left); mock stopped: $( [ -z "${MOCK_PID:-}" ] || gone "$MOCK_PID"; yn $?)"
  [ -z "$PRIV" ] || [ "${PRIV_KEEP:-0}" -eq 1 ] || rm -rf "$PRIV"
}
sp1_end() { trap '' INT TERM HUP PIPE; trap - EXIT; teardown; live_end "$1"; }
trap 'sp1_end $?' EXIT; trap 'sp1_end 129' HUP; trap 'sp1_end 130' INT; trap 'sp1_end 141' PIPE; trap 'sp1_end 143' TERM

TMUX_BIN=$(command -v tmux) || { echo "live: tmux is not on PATH"; exit 1; }
TVL=$("$TMUX_BIN" -V)
TV=$(printf '%s\n' "$TVL" | sed -n 's/^tmux //p')
ver_ge "${TV%[a-z]}" 3.2 || { echo "live: tmux is ${TV:-unknown}; the minimum is 3.2 (AU7)"; exit 1; }
for _t in git gh ssh ssh-agent ssh-add ssh-keyscan ssh-keygen stdbuf timeout jq python3; do
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
D=${SP1_DATE:-$(date +%Y%m%d)r$$}
REPO_SSH="zygo-sp1-ssh-$D" REPO_KEY="zygo-sp1-key-$D" REPO_HTTPS="zygo-sp1-https-$D"
BR=${SP1_BRANCH:-zygo/mail}
GHSSH="git@""ssh.github.com" GHKEY="git@""github.com"   # user and host of the SSH remotes (two parts: check-premises.sh scans for address forms)
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
SUBS="1.1 1.2 1.3 1.4 1.5 1.6 1.7 1.8"
ALL="SP1:1.1 SP1:1.2 SP1:1.3 SP1:1.4 SP1:1.5 SP1:1.6 SP1:1.7 SP1:1.8"

# stop <reason>: the cleanup, the acceptance sub-checks that a failed step did not reach, then the record and exit
stop() {
  echo "live: stop: $1"
  rest "$1" $ALL
  teardown
  [ ! -s "$BASE/cleanup.txt" ] || note SP1 3.8 "cleanup: $(tr '\n' ';' <"$BASE/cleanup.txt" | sed 's/;$//; s/;/; /g')"
  prescrub
  finish SP1 "$LIVE_CC_VER + $LIVE_OC_VER" "$FX/sp1.txt" "$HEAD" "$SUBS" "$OUT/sp1.txt"
  publish; exit 0
}
# prescrub: the repository names and the gh login out of the record, before scrub.py (finish) runs on it
prescrub() {
  [ ! -f "$REC/SP1.txt" ] ||
    sed -i "s#$REPO_SSH#REPO-SSH#g; s#$REPO_KEY#REPO-KEY#g; s#$REPO_HTTPS#REPO-HTTPS#g; s#\\b$GHU\\b#OWNER#g" "$REC/SP1.txt"
}
publish() {
  mkdir -p "$OUT" && cp "$FX/sp1.txt" "$OUT/sp1.txt" || { echo "live: the fixture could not be copied to the repository"; exit 1; }
  live_done "$REPO_DIR/fixtures"
}
ZV=$("$ZYGO_BIN" --version 2>/dev/null | sed -n 1p)
HEAD="Claude Code $LIVE_CC_VER, OpenCode $LIVE_OC_VER, git $GV, $TVL, ${ZV:-zygo ?}, $(uname -s)"

# --- 2.1 repository names: none may exist (read only; nothing is written before this passes) ---------------------
for r in "$REPO_SSH" "$REPO_KEY" "$REPO_HTTPS"; do
  a=$(gh api "repos/$GHU/$r" --jq .archived 2>"$BASE/view-$r.txt"); rc=$?
  if [ "$rc" -eq 0 ]; then
    case "$a" in true) stop "repository $r exists and is archived; nothing was changed; set SP1_DATE to a new suffix" ;;
      *) stop "repository $r exists; nothing was changed; set SP1_DATE to a new suffix" ;; esac
  fi
  grep -q 'HTTP 404' "$BASE/view-$r.txt" || stop "could not tell whether repository $r exists (gh api exit $rc)"
done

# --- 2.2 known_hosts line for [ssh.github.com]:443, checked against gh api meta ----------------------------------
ssh-keyscan -p 443 -t ed25519 ssh.github.com 2>/dev/null | awk '!/^#/ && NF >= 3' >"$PRIV/kh443"   # key lines only (some versions print a # banner line on stdout)
[ "$(wc -l <"$PRIV/kh443" | tr -d ' ')" -eq 1 ] || stop "ssh-keyscan of ssh.github.com:443 did not give one ed25519 key line"
FA=$(ssh-keygen -lf "$PRIV/kh443" 2>/dev/null | awk '{ print $2; exit }')
FB=$(gh api meta --jq .ssh_key_fingerprints.SHA256_ED25519 2>/dev/null)
KB=$(awk '{ print $2 " " $3; exit }' "$PRIV/kh443")
gh api meta --jq '.ssh_keys[]' 2>/dev/null >"$PRIV/meta-keys"
t=1; [ -n "$FA" ] && [ "$FA" = "SHA256:$FB" ] && grep -qxF "$KB" "$PRIV/meta-keys" && t=0
[ "$t" -eq 0 ] || stop "the ssh.github.com:443 host key does not match gh api meta"
# kh-add (sp1_watch.py): through a link, on the file it names; the bytes saved first; "added" only after the append
# was written, flushed and fsync'd; "present" changes nothing; a failure is recorded as not added and stops the run
KHR=$(python3 -I "$PREMISES_DIR/sp1_watch.py" kh-add --file "$HOME/.ssh/known_hosts" --line "$PRIV/kh443" --save "$PRIV/known_hosts.start")
case "$KHR" in
  added) KH_ADDED=1; KHN="added" ;;
  present) KHN="already present before the run (not added, not removed)" ;;
  *) KHN="not added: ${KHR#failed: }"; PRIV_KEEP=1 ;;
esac
note SP1 2.2 "[ssh.github.com]:443 ed25519 host key: fingerprint equals gh api meta SHA256_ED25519 and the key is in gh api meta ssh_keys: yes; the line: $KHN"
case "$KHR" in added | present) ;; *) echo "live: the copy of ~/.ssh/known_hosts saved before the append (if any) is kept in $PRIV"; stop "~/.ssh/known_hosts: the line was not added (${KHR#failed: })" ;; esac

# --- 2.3 run-local agent with the github.com key file; 2.4 controls ------------------------------------------------
KEY=$(ssh -G github.com 2>/dev/null | awk '$1 == "identityfile" { print $2; exit }')
case "$KEY" in "~/"*) KEY="$HOME/${KEY#\~/}" ;; esac
IA=$(ssh -G github.com 2>/dev/null | awk '$1 == "identityagent" { print $2; exit }')
KEYPP=1; ssh-keygen -y -P "" -f "$KEY" >/dev/null 2>&1 && KEYPP=0   # 0: the key file has no passphrase
# stdbuf: ssh-agent's stderr to a file is buffered until it exits; unbuffered, each sign request is countable at once
stdbuf -e0 -o0 ssh-agent -d -a "$PRIV/agent.sock" >/dev/null 2>"$BASE/agent.log" &
AGENT_PID=$!
wait_for 5 test -S "$PRIV/agent.sock" || stop "ssh-agent did not start"
env -i PATH="$PATH" HOME="$HOME" SSH_AUTH_SOCK="$PRIV/agent.sock" ssh-add "$KEY" </dev/null >/dev/null 2>&1 ||
  stop "ssh-agent arm: not run: ssh-add could not load the github.com key file without a prompt"
NK=$(env -i PATH="$PATH" SSH_AUTH_SOCK="$PRIV/agent.sock" ssh-add -l 2>/dev/null | wc -l | tr -d ' ')
# signs: sign requests the run agent got (SSH2_AGENTC_SIGN_REQUEST = message type 13 in its debug log; its lines can end in CR LF)
signs() { grep -Ec 'process_message: .* type 13([^0-9]|$)' "$BASE/agent.log" 2>/dev/null; }
signs_gt() { [ "$(signs)" -gt "$1" ]; }
env -i PATH="$PATH" HOME="$HOME" ssh -o BatchMode=yes -p 443 -T "$GHSSH" </dev/null >/dev/null 2>"$BASE/ctl-noagent.txt"; c1=$?
s0=$(signs)
env -i PATH="$PATH" HOME="$HOME" SSH_AUTH_SOCK="$PRIV/agent.sock" ssh -o BatchMode=yes -p 443 -T "$GHSSH" </dev/null >/dev/null 2>"$BASE/ctl-agent.txt"; c2=$?
wait_for 5 signs_gt "$s0"   # the agent can write its log line after the client has exited
s1=$(signs)
env -i PATH="$PATH" HOME="$HOME" SSH_AUTH_SOCK="$PRIV/agent.sock" ssh -o BatchMode=yes -T "$GHKEY" </dev/null >/dev/null 2>"$BASE/ctl-key.txt"; c3=$?
sleep 1; s2=$(signs)
t=1; [ "$c1" -eq 255 ] && grep -q 'Permission denied (publickey)' "$BASE/ctl-noagent.txt" && [ "$c2" -eq 1 ] &&
  grep -q 'successfully authenticated' "$BASE/ctl-agent.txt" && [ "$((s1 - s0))" -ge 1 ] && t=0
[ "$t" -eq 0 ] || stop "control: ssh.github.com:443 without the agent exit $c1, with it exit $c2, sign requests $((s1 - s0))"
KEYARM="the key file has no passphrase"
if [ "$KEYPP" -ne 0 ]; then KEYARM="fails by design: BatchMode with a passphrase key (finding; the ssh-key rounds are notes)"
elif [ "$c3" -ne 1 ] || ! grep -q 'successfully authenticated' "$BASE/ctl-key.txt" || [ "$s2" -ne "$s1" ]; then
  stop "control: github.com with ~/.ssh/config (IdentityAgent ${IA:-unset}) exit $c3, run agent sign requests $((s2 - s1))"
fi
note SP1 2.3 "run-local ssh-agent holds $NK key (the github.com IdentityFile of ~/.ssh/config, loaded with no prompt)"
note SP1 2.4 "controls: ssh.github.com:443 (no Host block) with ~/.ssh/config and no agent: exit $c1, Permission denied (publickey); with the run agent: exit $c2, successfully authenticated, sign requests $((s1 - s0)). github.com with ~/.ssh/config (IdentityAgent ${IA:-unset}, its key file) and the run agent's socket set: exit $c3, sign requests $((s2 - s1)); $KEYARM"

# --- setup: tool folder, mock, Claude Code config folder, XDG folders, the environment of the members -----------
PB="$PRIV/pb"; mkdir -p "$PB"
for _t in tmux git gh ssh ssh-add python3 jq node rg timeout; do _p=$(command -v "$_t") && ln -s "$_p" "$PB/$_t"; done
ln -s "$CLAUDE_BIN" "$PB/claude"; ln -s "$OPENCODE_BIN" "$PB/opencode"
start_mock
CCD="$RUN/ccd"; XDG="$RUN/xdg"
mkdir -p "$CCD" "$XDG/config" "$XDG/data" "$XDG/state" "$XDG/cache"
[ ! -e "$HOME/.config/gh" ] || ln -s "$HOME/.config/gh" "$XDG/config/gh"
[ ! -e "$HOME/.config/git" ] || ln -s "$HOME/.config/git" "$XDG/config/git"
# OpenCode installs @opencode-ai/plugin from npm into a config folder that has no node_modules and lock file; a
# copy of the user's (package files only, no opencode.json) leaves it nothing to fetch in this folder
mkdir -p "$XDG/config/opencode"
for _f in package.json package-lock.json node_modules; do
  [ ! -e "$HOME/.config/opencode/$_f" ] || cp -R "$HOME/.config/opencode/$_f" "$XDG/config/opencode/"
done
for w in $WS_LIST; do
  mkdir -p "$RUN/$w" && git -C "$RUN/$w" init -q && git -C "$RUN/$w" -c user.name=t -c user.email=t@t commit -q --allow-empty -m init
done
git -C "$RUN/sp1ssh" remote add origin "ssh://$GHSSH:443/$GHU/$REPO_SSH.git"
git -C "$RUN/sp1key" remote add origin "$GHKEY:$GHU/$REPO_KEY.git"
git -C "$RUN/sp1https" remote add origin "https://github.com/$GHU/$REPO_HTTPS.git"
# officialMarketplaceAutoInstall*: the state Claude Code itself writes when the auto-install is blocked by policy
python3 -I - "$CCD/.claude.json" "$RUN/sp1ssh" "$RUN/sp1key" "$RUN/sp1https" <<'PY'
import json, os, sys
proj = {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True}
pr = {}
for d in sys.argv[2:]:
    pr[d] = proj
    pr[os.path.realpath(d)] = proj
json.dump({"hasCompletedOnboarding": True, "numStartups": 5, "theme": "dark", "projects": pr,
           "officialMarketplaceAutoInstallAttempted": True, "officialMarketplaceAutoInstalled": False,
           "officialMarketplaceAutoInstallFailReason": "policy_blocked"}, open(sys.argv[1], "w"))
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
# arm <ssh branch> <key branch> <https branch>: the zygo config with the three [mail.workspace.*] tables (design
# decision 4: only after every member's session and warm-up turn, so the prompt hook is the first starter)
B_sp1ssh=$BR B_sp1key=$BR B_sp1https=$BR
arm() {
  { cat "$RUN/base.toml"
    for _a in $WS_LIST; do eval "_b=\$B_$_a"; printf '\n[mail.workspace.%s]\nsync = "%s"\nhost = "%s"\n' "$_a" "$_b" "$_a"; done
  } >"$CONF.new" && mv "$CONF.new" "$CONF"
}
U=$(id -un)
renv() { env -i PATH="$PB:/usr/bin:/bin" HOME="$HOME" USER="$U" LOGNAME="$U" SHELL=/bin/sh TERM=xterm-256color LANG=C.UTF-8 \
  DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-}" XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-}" \
  XDG_CONFIG_HOME="$XDG/config" XDG_DATA_HOME="$XDG/data" XDG_STATE_HOME="$XDG/state" XDG_CACHE_HOME="$XDG/cache" \
  GIT_TRACE2_EVENT="$BASE/trace2.jsonl" \
  CLAUDE_CONFIG_DIR="$CCD" CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL=1 \
  DISABLE_AUTOUPDATER=1 OPENCODE_DISABLE_AUTOUPDATE=1 OPENCODE_DISABLE_MODELS_FETCH=1 OPENCODE_DISABLE_CLAUDE_CODE=1 \
  OPENCODE_DISABLE_EXTERNAL_SKILLS=1 OPENCODE_DISABLE_LSP_DOWNLOAD=1 OPENCODE_DISABLE_DEFAULT_PLUGINS=1 \
  npm_config_offline=true npm_config_registry=http://127.0.0.1:9 \
  ZYGO_NO_UPDATE_CHECK=1 ZYGO_CONFIG="$CONF" ZYGO_LIVE_MOCK_KEY="$DUMMY_KEY" "$@"; }

# 2.5: the HTTPS credential source, as git sees it in the members' environment (every origin of a credential key)
renv git config --show-scope --show-origin --get-regexp '^credential\.' >"$BASE/cred-origins.txt" 2>&1
renv git config --get-urlmatch credential.helper https://github.com >"$BASE/cred-url.txt" 2>&1
CO=$(awk -v h="$HOME" '{ gsub(h, "~"); print $1 " " $2 " " $3 }' "$BASE/cred-origins.txt" | tr '\n' ';' | sed 's/;$//; s/;/; /g')
CU=$(tr '\n' ' ' <"$BASE/cred-url.txt" | sed 's/ $//')
t=1; [ ! -e "$HOME/.netrc" ] && t=0
note SP1 2.5 "HTTPS credential source in the members' environment: git config --show-scope --show-origin --get-regexp '^credential\\.': ${CO:-none}; git config --get-urlmatch credential.helper https://github.com: ${CU:-none}; no ~/.netrc: $(yn $t)"

# --- the private tmux server, the watcher (from before the first member), the test pause ----------------------
renv "$TMUX_BIN" -S "$SOCK" -f /dev/null new-session -d -s hold -x 200 -y 50 /bin/sleep 3600 || stop "tmux did not start the hold session"
SP=$(tm display-message -p '#{pid}')
python3 -I "$PREMISES_DIR/sp1_watch.py" watch --out "$BASE/watch.jsonl" --stop "$BASE/watch.stop" --root "$SP" \
  --agent-sock "$PRIV/agent.sock" --cfg "$BASE/watch-cfg.json" &
WATCH_PID=$!
wait_for 5 test -s "$BASE/watch.jsonl" || stop "the watcher did not start"
tp setup

# --- 2.1 repositories: created by this run (new names), private, anonymous probe (MS-24 form) -------------------
for r in "$REPO_SSH" "$REPO_KEY" "$REPO_HTTPS"; do
  gh repo create "$GHU/$r" --private --disable-issues --disable-wiki --description "zygo SP1 live check (temporary)" >/dev/null 2>"$BASE/create-$r.txt" ||
    stop "gh repo create of $r refused"
  MADE_REPOS="$MADE_REPOS $r"
  v=$(gh repo view "$GHU/$r" --json visibility --jq .visibility 2>/dev/null)
  [ "$v" = PRIVATE ] || stop "repository $r is not private (visibility ${v:-unknown})"
  env -u GIT_ASKPASS -u SSH_ASKPASS HOME="$PRIV" GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1 \
    GIT_TERMINAL_PROMPT=0 git -c credential.helper= -c core.askPass= ls-remote --heads "https://github.com/$GHU/$r.git" >/dev/null 2>"$BASE/anon-$r.txt"
  rc=$?
  t=1; [ "$rc" -ne 0 ] && grep -qi 'could not read Username' "$BASE/anon-$r.txt" && t=0
  [ "$t" -eq 0 ] || stop "the anonymous probe of $r did not fail as private (exit $rc)"
done
note SP1 2.1 "repositories REPO-SSH, REPO-KEY and REPO-HTTPS: none existed before the run (gh api: HTTP 404); created by this run, visibility PRIVATE, empty; anonymous probe (S§8.2 environment) of each: exit 128, could not read Username"

# --- workspaces: SSH_AUTH_SOCK only in the environment of the `zygo up` of sp1ssh ------------------------------
for w in $WS_LIST; do
  STARTED="$STARTED $w"
  if [ "$w" = sp1ssh ]; then
    (cd "$RUN/$w" && renv SSH_AUTH_SOCK="$PRIV/agent.sock" TMUX="$SOCK,$SP,0" "$ZYGO_BIN" up --loadout sp1 --no-attach) >"$BASE/up-$w.txt" 2>&1
  else
    (cd "$RUN/$w" && renv TMUX="$SOCK,$SP,0" "$ZYGO_BIN" up --loadout sp1 --no-attach) >"$BASE/up-$w.txt" 2>&1
  fi || stop "zygo up of $w failed"
done
SG=$(tm show-environment -g SSH_AUTH_SOCK 2>&1)

# ids, pane keys, tmux panes; eval-free lookups by workspace name
wsid() { tm show-environment -t "=zygo-$1:" ZYGO_WORKSPACE 2>/dev/null | sed -n 's/^ZYGO_WORKSPACE=//p'; }
WS_sp1ssh=$(wsid sp1ssh) WS_sp1key=$(wsid sp1key) WS_sp1https=$(wsid sp1https)
[ -n "$WS_sp1ssh" ] && [ -n "$WS_sp1key" ] && [ -n "$WS_sp1https" ] || stop "no workspace id after zygo up"
WSIDS="$WS_sp1ssh $WS_sp1key $WS_sp1https"
id_of() { case "$1" in sp1ssh) echo "$WS_sp1ssh" ;; sp1key) echo "$WS_sp1key" ;; sp1https) echo "$WS_sp1https" ;; esac; }
wdir() { echo "$HOME/.local/state/zygo/workspaces/$(id_of "$1")"; }
wj() { echo "$(wdir "$1")/workspace.json"; }
key() { jq -r --arg n "$2" '[.panes[] | select(.name == $n) | .key] | if length == 1 then .[0] else empty end' "$(wj "$1")" 2>/dev/null; }
mval() { jq -r --arg k "$(key "$1" "$2")" ".members[\$k].$3 // empty" "$(wj "$1")" 2>/dev/null; }   # <ws> <member> <field>
pane() { tm list-panes -s -t "=zygo-$1:" -F '#{pane_id} #{@zygo-pane}' 2>/dev/null | awk -v k="$(key "$1" "$2")" '$2 == k { print $1; exit }'; }
act() { echo "$HOME/.cache/zygo/sessions/$(mval "$1" "$2" zygo_session)/activity.json"; }
for w in $WS_LIST; do for m in cc oc; do [ -n "$(pane "$w" "$m")" ] || stop "no tmux pane for $w/$m"; done; done

# --- members ready (Claude Code session; OpenCode TUI drawn) ------------------------------------------------------
PTR=$(printf '\342\235\257')   # U+276F, the pointer of a Claude Code menu
trust_line() { tm capture-pane -p -t "$1" 2>/dev/null | grep -F "Yes, I trust" | tail -n 1; }
cc_trust() {
  case "$(trust_line "$1")" in
    "") ;;
    *"$PTR"*) sleep 0.5; case "$(trust_line "$1")" in *"$PTR"*) tm send-keys -t "$1" Enter; sleep 1 ;; esac ;;
    *) tm send-keys -t "$1" Down; sleep 1 ;;
  esac
}
cc_up() { for _a in $WS_LIST; do [ -n "$(mval "$_a" cc agent_session)" ] || return 1; done; }
oc_drawn() { tm capture-pane -p -t "$(pane "$1" oc)" 2>/dev/null | grep -qF "$LIVE_OC_VER" && [ -n "$(mval "$1" oc zygo_session)" ]; }
_c=$((120 * PW_TICKS / 5))
until cc_up; do for _a in $WS_LIST; do cc_trust "$(pane "$_a" cc)"; done; _c=$((_c - 1)); [ "$_c" -gt 0 ] || break; sleep 0.5; done
cc_up || stop "timeout 120 s: a Claude Code member has no agent session"
for w in $WS_LIST; do wait_for 180 oc_drawn "$w" || stop "timeout 180 s: the OpenCode TUI of $w was not drawn"; done

# the watcher's workspaces and activity files; its first busy lines are the values at that time
python3 -I - "$BASE/watch-cfg.json" "$(for w in $WS_LIST; do printf '%s %s ' "$(id_of "$w")" "$(wdir "$w")"; done)" \
  "$(for w in $WS_LIST; do for m in cc oc; do printf '%s/%s %s ' "$w" "$m" "$(act "$w" "$m")"; done; done)" <<'PY'
import json, os, sys
ws, busy = sys.argv[2].split(), sys.argv[3].split()
tmp = sys.argv[1] + ".tmp"
json.dump({"ws": [ws[i:i + 2] for i in range(0, len(ws), 2)], "busy": [busy[i:i + 2] for i in range(0, len(busy), 2)]}, open(tmp, "w"))
os.replace(tmp, sys.argv[1])
PY
busy_lines() { [ "$(grep -c '"k": "busy"' "$BASE/watch.jsonl")" -ge 6 ]; }
wait_for 10 busy_lines || stop "the watcher did not read the six activity files"
W="python3 -I $PREMISES_DIR/sp1_watch.py"
turn() { $W wait-turn --watch "$BASE/watch.jsonl" --label "$1/$2" --after "$3" --timeout "$4"; }   # <ws> <member> <after> <s>
say() { tm send-keys -t "$1" -l "$2"; sleep 0.5; tm send-keys -t "$1" Enter; }
delivered() { [ -n "$(body_has "$1" "$RUN"/rec/*.json)" ]; }

# --- one warm-up turn each (OpenCode makes its session at the first prompt and then sends `session` before
# `prompt`; after a warm-up turn for every member, before the arm, only `prompt` is left for the round prompts) --
for w in $WS_LIST; do
  for m in cc oc; do
    tok="warm-$w-$m-$(date +%s)"
    t0=$(date +%s.%N)
    say "$(pane "$w" "$m")" "Reply with the single word ok. ($tok)"
    turn "$w" "$m" "$t0" 180 || stop "warm-up turn of $w/$m: no busy true then false within 180 s (watcher lines)"
    wait_for 10 delivered "$tok" || stop "warm-up turn of $w/$m: the prompt did not reach the mock"
  done
done
all_sessions() { for _a in $WS_LIST; do for _m in cc oc; do [ -n "$(mval "$_a" "$_m" agent_session)" ] || return 1; done; done; }
wait_for 60 all_sessions || stop "timeout 60 s: a member has no agent session after its warm-up turn"
t=0; for w in $WS_LIST; do [ -e "$(wdir "$w")/mail/sync/state.json" ] || [ -e "$(wdir "$w")/mail/sync/last_start" ] && t=1; done
[ "$t" -eq 0 ] || stop "a sync ran before the arm (state.json or last_start present)"
note SP1 2.6 "six members (cc, oc in sp1ssh, sp1key and sp1https) have an agent session and one warm-up turn, each prompt seen by the mock; before the arm no workspace has mail/sync/state.json or last_start; tmux server global SSH_AUTH_SOCK: $(case "$SG" in -SSH_AUTH_SOCK | *"unknown variable"*) echo absent ;; *) echo present ;; esac)"

# --- arm, rounds ----------------------------------------------------------------------------------------------------
arm
renv "$ZYGO_BIN" config check >"$BASE/config-check.txt" 2>&1 || stop "zygo config check failed after the arm"
rd() { cat "$(wdir "$1")/mail/sync/$2" 2>/dev/null; }
gap_ok() { _l=$(rd "$1" last_start); [ -z "$_l" ] || [ "$(date +%s)" -ge $((_l + 61)) ]; }
arm_of() { case "$1" in sp1ssh) echo ssh-agent ;; sp1key) echo ssh-key ;; sp1https) echo https ;; esac; }
# round <id> <sub> <ws> <member> <attempt>
round() {
  RW=$3 RM=$4 RA=$(arm_of "$3"); eval "RB=\$B_$3"
  RACC=true; [ "$RA" != ssh-key ] || [ "$KEYPP" -eq 0 ] || RACC=false
  wait_for 120 gap_ok "$RW"
  _t0=$(date +%s); while [ "$(date +%s)" = "$_t0" ]; do sleep 0.01; done
  T=$(date +%s)
  _s0=$(signs)
  _tok="round-$1-$5-$T"
  say "$(pane "$RW" "$RM")" "Reply with the single word ok. ($_tok)"
  _bt=1; turn "$RW" "$RM" "$T" 180 && _bt=0
  _dl=false; wait_for 10 delivered "$_tok" && _dl=true
  _cs=1; $W wait-child --watch "$BASE/watch.jsonl" --ws "$(id_of "$RW")" --after "$T" --timeout 30 && _cs=0
  _cg=1; [ "$_cs" -eq 0 ] && $W wait-child --watch "$BASE/watch.jsonl" --ws "$(id_of "$RW")" --after "$T" --timeout 180 --gone && _cg=0
  sleep 1
  _s1=$(signs)
  cp "$(wdir "$RW")/mail/sync/state.json" "$BASE/$1-$5-state.json" 2>/dev/null || echo null >"$BASE/$1-$5-state.json"
  cp "$(wdir "$RW")/errors.log" "$BASE/$1-$5-errors.log" 2>/dev/null || : >"$BASE/$1-$5-errors.log"
  python3 -I - "$BASE/rounds.jsonl" "$1" "$2" "$RACC" "$(id_of "$RW")" "$RM" "$RW/$RM" "$RA" "$T" "$(date +%s)" \
    "$BASE/$1-$5-state.json" "$BASE/$1-$5-errors.log" "$((_s1 - _s0))" "$_dl" "$RB" "$5" <<'PY'
import json, sys
(out, rid, sub, acc, ws, member, label, arm, T, end, stf, errf, signs, dl, br, att) = sys.argv[1:]
try:
    st = json.load(open(stf))
except ValueError:
    st = None
errs = []
for ln in open(errf, errors="replace"):
    p = ln.rstrip("\n").split(" ", 1)
    if len(p) == 2 and p[0].isdigit():
        errs.append([int(p[0]), p[1]])
r = {"id": rid, "sub": sub, "acc": acc == "true", "ws": ws, "member": member, "member_label": label, "arm": arm,
     "T": int(T), "end": int(end) + 1, "state": st, "errors": errs, "signs": int(signs), "delivered": dl == "true",
     "branch": br, "attempt": int(att)}
open(out, "a").write(json.dumps(r, sort_keys=True) + "\n")
PY
  note SP1 "w-$1-$5" "$1 ($RW/$RM, $RA, attempt $5) waits: busy true then false within 180 s: $(yn $_bt); child start within 30 s: $(yn $_cs); child end within 180 s: $(yn $_cg)"
}
order() { $W judge --watch "$BASE/watch.jsonl" --rounds "$BASE/rounds.jsonl" --order "$1"; }
# br_absent <repo> <branch>: the branch is not on the remote (gh api)
br_absent() { case " $(gh api "repos/$GHU/$1/branches" --jq '.[].name' 2>/dev/null | tr '\n' ' ') " in *" $2 "*) return 1 ;; esac; }
repo_of() { case "$1" in sp1ssh) echo "$REPO_SSH" ;; sp1key) echo "$REPO_KEY" ;; sp1https) echo "$REPO_HTTPS" ;; esac; }
# phase 1: the bootstrap rounds; phase 2, after sync_gap: the same arms in the other agent
set -- R1:1.1:sp1ssh:cc R2:1.2:sp1key:oc R3:1.3:sp1https:cc
for x in "$@"; do IFS=: read -r a b c d <<EOF
$x
EOF
  round "$a" "$b" "$c" "$d" 1
done
# S§20 SP1 fallback (plan §2.7): a bootstrap child that ended before its turn shows no order; once, on a new branch
# (checked absent first), after sync_gap, the round is run again
again=""
for x in "$@"; do IFS=: read -r a b c d <<EOF
$x
EOF
  [ "$(order "$a")" = yes ] && continue
  if br_absent "$(repo_of "$c")" "${BR}2"; then eval "B_$c=\${BR}2"; again="$again $x"
  else note SP1 3.6 "$a: no order shown, and the fallback branch ${BR}2 exists on the remote: not run again"; fi
done
if [ -n "$again" ]; then
  arm
  for x in $again; do IFS=: read -r a b c d <<EOF
$x
EOF
    round "$a" "$b" "$c" "$d" 2
  done
fi
for x in R4:1.4:sp1ssh:oc R5:1.5:sp1key:cc R6:1.6:sp1https:oc; do IFS=: read -r a b c d <<EOF
$x
EOF
  round "$a" "$b" "$c" "$d" 1
done
: >"$BASE/watch.stop"; wait_for 5 gone "$WATCH_PID"; WATCH_PID=""

# --- acceptance command (node MS-23), only this run's workspaces (node FX-38: no other workspace's state is read) ---
# acc (sp1_watch.py): <state>/<id>/mail/sync/state.json of each id of $WSIDS into acceptance.txt; prints note 3.4
$W acc --state "$HOME/.local/state/zygo/workspaces" --out "$BASE/acceptance.txt" $WSIDS >"$BASE/acc-note.txt" 2>"$BASE/acc-err.txt" ||
  echo "the acc step failed (exit $?)" >"$BASE/acc-note.txt"

# --- 1.7 remote: the run's branch is there with ZYGO-MAIL = 1, author zygo, message (every branch is named) ---------
ZE="zygo@""zygo.invalid"
t=0 d=""
for w in $WS_LIST; do
  _r=$(repo_of "$w"); eval "_b=\$B_$w"
  _br=$(gh api "repos/$GHU/$_r/branches" --jq '.[].name' 2>/dev/null | sort | tr '\n' ' ')
  _mk=$(gh api "repos/$GHU/$_r/contents/ZYGO-MAIL?ref=$_b" --jq .content 2>/dev/null | base64 -d 2>/dev/null)
  _cm=$(gh api "repos/$GHU/$_r/commits/$_b" --jq '[.commit.author.name, .commit.author.email, .commit.message] | join("|")' 2>/dev/null)
  _ok=1; case " $_br " in *" $_b "*) [ "$_mk" = 1 ] && [ "$_cm" = "zygo|$ZE|zygo mail [skip ci]" ] && _ok=0 ;; esac
  [ "$_ok" -eq 0 ] || t=1
  d="$d$(arm_of "$w") repository: branches ${_br% }; ZYGO-MAIL at $_b: ${_mk:-absent}; tip author zygo with the zygo.invalid address and message zygo mail [skip ci]: $(yn "$([ "$_cm" = "zygo|$ZE|zygo mail [skip ci]" ]; echo $?)"); "
done
sub SP1 1.7 $t "remote: ${d%; }"

# --- judge: 1.1 to 1.6, notes; 1.8 side traffic ---------------------------------------------------------------------
$W judge --watch "$BASE/watch.jsonl" --rounds "$BASE/rounds.jsonl" --trace "$BASE/trace2.jsonl" >"$BASE/judge.tsv" 2>"$BASE/judge-err.txt" ||
  rest "the judge failed" SP1:1.1 SP1:1.2 SP1:1.3 SP1:1.4 SP1:1.5 SP1:1.6
while IFS="$(printf '\t')" read -r _s _v _x; do
  case "$_v" in note) note SP1 "$_s" "$_x" ;; *) sub SP1 "$_s" "$_v" "$_x" ;; esac
done <"$BASE/judge.tsv"
KM=false; [ ! -e "$CCD/plugins/known_marketplaces.json" ] || KM=true
GD=$(find "$CCD" "$XDG" -name .git 2>/dev/null | wc -l | tr -d ' ')
LO=$(jq -s 'map(.T) | min' "$BASE/rounds.jsonl") HI=$(jq -s 'map(.end) | max' "$BASE/rounds.jsonl")
$W side --watch "$BASE/watch.jsonl" --trace "$BASE/trace2.jsonl" --files "{\"known_marketplaces\": $KM, \"git_dirs\": $GD}" \
  --lo "${LO:-0}" --hi "${HI:-0}" >"$BASE/side.tsv" 2>"$BASE/side-err.txt"
IFS="$(printf '\t')" read -r _v _x <"$BASE/side.tsv"
sub SP1 1.8 "${_v:-1}" "side traffic (rounds' window: first round T to last round end, $((${HI:-0} - ${LO:-0})) s; git: trace2 events of every git started under the private tmux server or by zygo up, from the server start; processes and TCP connections: watcher scans of the tmux server's descendants and of the sync children's sessions, from the server start): ${_x:-the side check failed}"
# 3.4: the acceptance output (written by acc above), one entry per workspace of this run (id shortened; URL hashes
# and blob ids left out)
note SP1 3.4 "acceptance command output: $(cat "$BASE/acc-note.txt")"
teardown
note SP1 3.8 "cleanup: $(tr '\n' ';' <"$BASE/cleanup.txt" | sed 's/;$//; s/;/; /g')"
prescrub
finish SP1 "$LIVE_CC_VER + $LIVE_OC_VER" "$FX/sp1.txt" "$HEAD" "$SUBS" "$OUT/sp1.txt"
publish

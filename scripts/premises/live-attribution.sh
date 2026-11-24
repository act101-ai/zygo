#!/bin/sh
# Live check FIX-21 (spec §8.5, §13.4; operator, 2026-11-22: there is never Claude attribution): Claude Code with zygo's
# settings file commits in a throwaway git repository, and the commit message holds no Claude attribution.
# Usage: sh scripts/premises/live-attribution.sh [login bare mock]   (default: all three)
#   login: `zygo claude --passthrough -- ... -p <ask to commit>` with `--model haiku`, on the claude.ai login Claude Code
#     already stores. Pass: one new commit, and its message holds none of the §13.4 attribution forms.
#   bare: the same ask with `--bare`. zygo refuses `--passthrough` with `--bare` (§12.3: bare mode reads only API keys);
#     the part records that refusal, then runs Claude Code alone, `claude --bare --settings <zygo's two attribution
#     keys>`, on the same login. Pass: one new commit with no attribution form; a run that cannot sign in is a fail
#     with Claude Code's own message.
#   mock: no login and no key. Claude Code alone on the mock upstream (the control, Claude Code's default attribution),
#     then zygo (not passthrough) on the mock with `-p`, without and with `--bare`. Pass: the control's requests hold
#     Claude Code's commit attribution text (the Anthropic noreply address in angle brackets, or the generated line with
#     its link), and none of zygo's requests do.
#
# What the login and bare parts do to the operator's machine, as live-passthrough.sh (#28): the real HOME and no
# CLAUDE_CONFIG_DIR, so Claude Code reads the operator's ~/.claude (settings, hooks, MCP servers) and its login; Claude
# Code writes a transcript folder under ~/.claude/projects/ for each run repository; zygo writes under
# ~/.local/share/zygo and ~/.cache/zygo; ZYGO_CONFIG points at an empty file; variables of an enclosing Claude Code
# session (CLAUDECODE, CLAUDE_CODE_*, ...) are dropped. Spend: at most 8 turns on haiku in each of the two sign-in runs.
# Tools: Write, and Bash only for `git add` and `git commit` (no --yolo). The mock part uses a temporary HOME.
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps
pinned_agents
FX="$REPO_DIR/fixtures/claude-code/live"
mkdir -p "$FX"
ASK="Create a file named hello.txt that holds the word hi. Then stage it with git add and commit it with git commit. Write the commit message yourself."
TOOLS="Write,Bash(git add:*),Bash(git commit:*)"
# Claude Code's own commit attribution text in a request: the noreply address in angle brackets, or the linked
# generated line. zygo's session sentence names both forms without the brackets and the link, so it does not match.
RX='<noreply@anthropic\.com>|Generated with \[Claude Code\]\('

# forms <file>: the §13.4 attribution forms in <file>, one name per line; nothing when it holds none.
forms() {
  python3 -I - "$1" <<'PY'
import re, sys
t = open(sys.argv[1], encoding="utf-8", errors="replace").read().lower()
for name, rx in [("co-author", r"co-authored-by[ \t]*[:=][^<\n]*\bclaude\b"), ("noreply", r"noreply@anthropic\.com"),
                 ("generated", r"generated (with|by)[ \t]*\[?claude\b"), ("robot", "\U0001f916[ \t️]*generated"),
                 ("session-trailer", r"claude-session:"), ("session-link", r"https://claude\.ai/code/\w")]:
    if re.search(rx, t):
        print(name)
PY
}

# signin_run <cmd...>: <cmd> in the run repository on the real HOME, no CLAUDE* variables; stdout in $RUN/out.txt,
# stderr in $RUN/err.txt, the exit status in RC.
signin_run() {
  RC=0
  (
    for v in $(env | sed -n 's/^\(CLAUDE[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$v"; done
    export ZYGO_CONFIG="$RUN/zygo.toml" DISABLE_AUTOUPDATER=1
    cd "$R"
    exec "$@" </dev/null >"$RUN/out.txt" 2>"$RUN/err.txt"
  ) || RC=$?
}

# new_repo <name>: a fresh run with a repository that has a commit identity and an empty zygo config.
new_repo() {
  live_run "$1"
  git -C "$R" config user.name t
  git -C "$R" config user.email t@t
  : >"$RUN/zygo.toml"
}

# commit_result <id> <fixture name> <how>: one LIVE line and a scrubbed fixture for the commit the run made.
commit_result() {
  n=$(git -C "$R" rev-list --count HEAD 2>/dev/null || echo 0)
  git -C "$R" log -1 --format=%B >"$RUN/msg.txt" 2>/dev/null || : >"$RUN/msg.txt"
  found=$(forms "$RUN/msg.txt" | tr '\n' ' ' | sed 's/ $//')
  python3 -I - "$RUN/msg.txt" "$RUN/out.txt" "$RUN/err.txt" "$3" "$RC" "$n" "$found" >"$RUN/result.json" <<'PY'
import json, sys
msg, out, err, how, rc, n, found = sys.argv[1:8]
print(json.dumps({"how": how, "exit": int(rc), "commits_after_init": int(n) - 1,
                  "message": open(msg, errors="replace").read() if int(n) > 1 else None,
                  "attribution_forms": found.split() if found else [],
                  "stdout": open(out, errors="replace").read()[-2000:],
                  "stderr": open(err, errors="replace").read()[-2000:]}, indent=2, ensure_ascii=False))
PY
  scrub "$RUN/result.json" >"$FX/$2"
  first=$(sed -n 1p "$RUN/msg.txt" | cut -c1-72)
  if [ "$n" -gt 1 ] && [ -z "$found" ]; then
    result "$1" pass "$3: exit $RC, one new commit, message \"$first\", no attribution form"
  elif [ "$n" -gt 1 ]; then
    result "$1" fail "$3: exit $RC, new commit \"$first\" holds: $found"
  else
    result "$1" fail "$3: exit $RC, no new commit; stdout: $(tr '\n' ' ' <"$RUN/out.txt" | cut -c1-200); stderr: $(tr '\n' ' ' <"$RUN/err.txt" | cut -c1-200)"
  fi
}

part_login() {
  new_repo login
  signin_run "$ZYGO_BIN" claude --passthrough -- --allowedTools "$TOOLS" --model haiku --max-turns 8 -p "$ASK"
  commit_result FIX21-LOGIN fix21-login.json "zygo claude --passthrough, --model haiku"
}

part_bare() {
  new_repo bare-zygo
  signin_run "$ZYGO_BIN" claude --passthrough -- --bare --allowedTools "$TOOLS" --model haiku --max-turns 8 -p "$ASK"
  refusal=$(tr '\n' ' ' <"$RUN/err.txt" | cut -c1-200)
  result FIX21-BARE-ZYGO recorded "zygo claude --passthrough -- --bare: exit $RC, \"$refusal\""
  new_repo bare
  printf '{"attribution": {"commit": "", "pr": "", "sessionUrl": false}, "includeCoAuthoredBy": false}\n' >"$RUN/keys.json"
  signin_run "$CLAUDE_BIN" --bare --settings "$RUN/keys.json" --allowedTools "$TOOLS" --model haiku --max-turns 8 -p "$ASK"
  commit_result FIX21-BARE fix21-bare.json "claude --bare --settings <zygo's attribution keys>, --model haiku"
}

# mock_hits: the recorded requests of the run that hold Claude Code's attribution text, and all recorded requests.
mock_hits() {
  hits=$(body_has "$RX" "$RUN"/rec/[0-9]*.json 2>/dev/null | wc -l | tr -d ' ')
  all=$(ls "$RUN"/rec/[0-9]*.json 2>/dev/null | wc -l | tr -d ' ')
}

part_mock() {
  live_run mock-control
  start_mock
  run_claude -p "tracer-fix21 go" </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock
  mock_hits
  c_hits=$hits c_all=$all
  # Recorded only: Claude Code alone with `--bare` (no zygo keys), to show what bare mode does by itself.
  live_run mock-control-bare
  start_mock
  run_claude --bare -p "tracer-fix21 go" </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock
  mock_hits
  cb_hits=$hits cb_all=$all
  live_run mock-zygo
  start_mock; mock_config "$RUN/zygo.toml"
  run_zygo claude -- -p "tracer-fix21 go" </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock
  mock_hits
  z_hits=$hits z_all=$all
  live_run mock-bare
  start_mock; mock_config "$RUN/zygo.toml"
  run_zygo claude -- --bare -p "tracer-fix21 go" </dev/null >"$RUN/out.txt" 2>&1 || true
  stop_mock
  mock_hits
  b_hits=$hits b_all=$all
  printf '{"control_claude_alone": {"requests": %s, "with_attribution_text": %s}, "control_claude_alone_bare": {"requests": %s, "with_attribution_text": %s}, "zygo": {"requests": %s, "with_attribution_text": %s}, "zygo_bare": {"requests": %s, "with_attribution_text": %s}}\n' \
    "$c_all" "$c_hits" "$cb_all" "$cb_hits" "$z_all" "$z_hits" "$b_all" "$b_hits" >"$FX/fix21-mock.json"
  text="control (Claude Code alone): $c_hits of $c_all requests hold the attribution text; control --bare (recorded): $cb_hits of $cb_all; zygo: $z_hits of $z_all; zygo --bare: $b_hits of $b_all"
  if [ "$c_hits" -gt 0 ] && [ "$z_all" -gt 0 ] && [ "$z_hits" -eq 0 ] && [ "$b_all" -gt 0 ] && [ "$b_hits" -eq 0 ]; then
    result FIX21-MOCK pass "$text"
  else
    result FIX21-MOCK fail "$text"
  fi
}

for p in ${*:-login bare mock}; do "part_$p"; done
live_done "$FX"

# SX-9 — Live check P3-15: the operator records the ST1-ST4 evidence by hand

**Spec:** docs/specs/2026-11-24-session-text-design.md §12.2, §12.1 (table ST1-ST4); T-C3, T-C6
**Files:** `fixtures/claude-code/live/st-hooks.jsonl`, `fixtures/claude-code/live/st-evidence.txt`, `fixtures/opencode/live/st-transform.jsonl` (the operator runs the check; this node commits the scrubbed evidence)
**Consumes:** `docs/live-checks.md#P3-15` (SX-8)
**Produces:** `fixtures/claude-code/live/st-hooks.jsonl`, `fixtures/claude-code/live/st-evidence.txt`, `fixtures/opencode/live/st-transform.jsonl` (written by the operator in step 6, committed by this node), and the `claude --version` and `opencode --version` lines
**Acceptance:** `for f in fixtures/claude-code/live/st-hooks.jsonl fixtures/claude-code/live/st-evidence.txt fixtures/opencode/live/st-transform.jsonl; do test -s "$f" || exit 1; done && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh` → exit 0; and the lead reads each of ST1, ST2, ST3 and ST4 as `verified` or `failed` by T§12.2 "Reading the result" (an ST1 or ST2 line with `no-transcript` is not decided, and the check is run again)

## Plan

Unit: phase3:SX-9:plan

**Split (operator decision, 2026-11-25).** The lead session runs the live check itself, on this machine (Part A). This replaces "the operator runs the check" in this node and "never by an agent" at `docs/work/phase3/graph.md:110`. A zygo-code implementer verifies, reads, records and commits (Part B). Part C says what a `failed` or not-decided ST line means for the graph.

Facts this plan rests on:
- The commands are the six `sh` blocks of `docs/live-checks.md` section P3-15 (`docs/live-checks.md:148-260`), a copy of T§12.2 (`docs/specs/2026-11-24-session-text-design.md:267`). In order: block 1 = step 1, block 2 = step 2, block 3 = step 3, block 4 = step 4, block 5 = step 6, block 6 = step 8. Step 5 (typed lines) and step 7 (hand-off) have no block. A3 extracts the blocks from the file, so the lead runs them byte for byte; only the `<provider>/<MAIN>` and `<provider>/<SUB>` tokens of block 4 are replaced (A7), and each later script gets one first line that sets `S`, because the Bash tool does not keep shell variables between calls.
- Environment. The lead's shell is a Claude Code session: it has `CLAUDECODE`, `CLAUDE_CONFIG_DIR` (not the default `~/.claude`), `CLAUDE_CODE_CHILD_SESSION`, `CLAUDE_CODE_MESSAGING_SOCKET`, `CLAUDE_CODE_MESSAGING_TOKEN` and `ZYGO_*` variables. P3-15 is written for the operator's terminal: the normal login in `~/.claude` and `~/.claude.json` (its intro and step 8), and no zygo. So every block runs through `/private/tmp/sx9-p315/clean.sh` (A3), which empties the environment except `HOME`, `USER`, `LOGNAME`, `PATH`, `SHELL`, `TERM`, `TMPDIR` and `LANG`. This is the "environment emptied of every variable" rule of `docs/live-checks.md:12` ("Method"). With that environment, `claude auth status` gave `"loggedIn": true`, `"authMethod": "claude.ai"` (Claude Code 2.1.332, 2026-11-25).
- Versions on this machine (2026-11-25): `claude --version` → `2.1.332 (Claude Code)` (`~/.local/bin/claude`, a native binary, not a zygo wrapper); `opencode --version` → `1.18.16`. The minimums are `Claude Code: 2.1.328`, `OpenCode: 1.18.16` (`docs/premises.md:5-6`); `scripts/premises/ver.sh` `ver_ge` compares them.
- OpenCode models (step 4). `opencode auth list` shows OpenRouter, Z.AI Coding Plan, Moonshot AI and OpenAI. Only OpenRouter has a Sonnet id and a Haiku id: `opencode models` lists `openrouter/anthropic/claude-sonnet-4.6` and `openrouter/anthropic/claude-haiku-4.5`. So: provider `openrouter`, `MAIN` = `anthropic/claude-sonnet-4.6`, `SUB` = `anthropic/claude-haiku-4.5`. The probe logs `input.model.id` (block 4), which is the id after `openrouter/`. Spend: three short turns and one compaction, billed to the OpenRouter key. The global OpenCode config (`~/.config/opencode/opencode.jsonc`) has no `plugin` key and names no zygo, so no zygo runs in OpenCode.
- Step 5 types into the OpenCode TUI. The lead cannot type into a TUI from the Bash tool, so it runs block 4 in a private tmux server (`tmux -L p315 -f /dev/null`, tmux 3.8 at `/opt/homebrew/bin/tmux`) and types the step 5 lines with `send-keys -l`, then `Enter`.
- `scripts/premises/scrub.py <run dir> <file>` writes the scrubbed copy to stdout (`scrub.py:2,40`). A JSONL file with more than one line is not one JSON value, so it takes the text path and stays one object per line (`scrub.py:150-155`). It maps the run folder (both `/var` and `/private/var` forms, and the `-`-joined form Claude Code uses for project folder names) to `/tmp/run`, the home folder to `/home/user`, the user name to `user`, host names to `host`, e-mail addresses to `[e-mail omitted]`, UUIDs and `ses_` ids to numbered placeholders, the same id to the same placeholder in one file (`scrub.py:86-137`). So the `session` values in `st-transform.jsonl` can still be compared.
- `scripts/check-premises.sh` scans `fixtures/` for `/Users/` (`:162`), `/private/var/` and `/var/folders/` (`:163-164`), e-mail addresses (`:165`), each word of `N2_FORBIDDEN_NAMES` (`:189`) and the machine's host names (`:192-200`). It has no ST rows yet (`IDS`, `WIDS` only, `:13,15,58`); SX-10 adds them (T§12.1 item 4). On origin/phase-3 (`a6fb3da3`) the acceptance's script part exits 0.
- `fixtures/claude-code/live/` and `fixtures/opencode/live/` exist; none of the three fixture files exists yet.
- Raw output: everything stays in `/private/tmp/sx9-p315` (outside the repository): the extracted blocks, the scripts, every stdout and stderr, the pane captures, a copy of the run folder `$S` (`S-copy/`, with the unscrubbed `hooks.jsonl`, `evidence.txt` and `oc.jsonl`) and `handoff.txt`.

### Part A — lead session (live run)

- [x] A1. Go to the node worktree: `cd "$(git rev-parse --path-format=absolute --git-common-dir)/../.worktrees/phase3/SX-9-node"`. Check: `git branch --show-current` prints `phase3/SX-9--SX-9`; `git status --porcelain` prints nothing; `git log --oneline -1 --grep='^SX-9: plan'` prints this plan's commit; `ls fixtures/claude-code/live/st-hooks.jsonl fixtures/claude-code/live/st-evidence.txt fixtures/opencode/live/st-transform.jsonl` fails for all three (no file yet).
- [x] A2. Check versions, tools and models.
  - `v=$(claude --version | awk '{print $1}') && echo "claude $v" && sh -c '. scripts/premises/ver.sh && ver_ge "$1" 2.1.328' sh "$v" && echo cc-ok` → prints `claude <version>` and `cc-ok`.
  - `o=$(opencode --version) && echo "opencode $o" && sh -c '. scripts/premises/ver.sh && ver_ge "$1" 1.18.16' sh "$o" && echo oc-ok` → prints `opencode <version>` and `oc-ok`.
  - `command -v git python3 tmux` → three paths.
  - `env -i HOME="$HOME" USER="$USER" LOGNAME="$LOGNAME" PATH="$PATH" TERM=xterm-256color TMPDIR="$TMPDIR" claude auth status | grep -E '"loggedIn": true'` → one line.
  - `env -i HOME="$HOME" USER="$USER" LOGNAME="$LOGNAME" PATH="$PATH" TERM=xterm-256color TMPDIR="$TMPDIR" opencode models openrouter | grep -xE 'openrouter/anthropic/claude-(sonnet-4\.6|haiku-4\.5)'` → exactly the two lines `openrouter/anthropic/claude-haiku-4.5` and `openrouter/anthropic/claude-sonnet-4.6`.
  - Pass: all five. If one fails, stop and fix the cause (version on `PATH`, login, provider); do not continue.
  - Ran instead (see Deviation in `## Result`): the two version checks as written (`2.1.332 (Claude Code)` ≥ 2.1.328, `1.18.16` ≥ 1.18.16); the Claude Code login was proven by A4 (both subagents replied); the `openrouter` model check was replaced by the glm launch `zygo opencode --profile glm -- -m zygo/lead` (MAIN = `zygo/lead`, SUB = `zygo/code`), whose prompt box showed `Build · GLM-5.3 Z.AI`.
- [x] A3. Make the scratch folder, the clean-environment wrapper and the extracted blocks:

```sh
test ! -e /private/tmp/sx9-p315 && mkdir -p /private/tmp/sx9-p315/blocks && cat > /private/tmp/sx9-p315/clean.sh <<'EOF'
exec env -i HOME="$HOME" USER="$USER" LOGNAME="$LOGNAME" PATH="$PATH" SHELL="$SHELL" TERM=xterm-256color TMPDIR="$TMPDIR" LANG=en_US.UTF-8 "$@"
EOF
python3 -I -c 'import re,sys; t=open(sys.argv[1]).read(); s=t[t.index("## P3-15 "):]; s=s[:s.index("\n## ",1)]; b=re.findall(r"```sh\n(.*?)```", s, re.S); print(len(b)); [open("%s/b%d.sh"%(sys.argv[2],i+1),"w").write(x) for i,x in enumerate(b)]' docs/live-checks.md /private/tmp/sx9-p315/blocks
```

  Check: the Python command prints `6`; `head -n 1 /private/tmp/sx9-p315/blocks/b1.sh` prints `S=$(mktemp -d "${TMPDIR:-/tmp}/p315.XXXXXX") && cd "$S" && git init -q`; `head -n 1 /private/tmp/sx9-p315/blocks/b5.sh` starts `python3 -I scripts/premises/scrub.py`; `head -n 1 /private/tmp/sx9-p315/blocks/b6.sh` prints `rm -rf ~/.claude/projects/*p315*`.
- [x] A4. Run steps 1, 2 and 3 (Claude Code, ST1 and ST2) as one script, so that `S` carries from step 1 to steps 2 and 3. Bash tool timeout 600000 ms:

```sh
cat /private/tmp/sx9-p315/blocks/b1.sh /private/tmp/sx9-p315/blocks/b2.sh /private/tmp/sx9-p315/blocks/b3.sh > /private/tmp/sx9-p315/cc.sh && cd /private/tmp/sx9-p315 && sh /private/tmp/sx9-p315/clean.sh /bin/sh /private/tmp/sx9-p315/cc.sh > /private/tmp/sx9-p315/cc.out 2> /private/tmp/sx9-p315/cc.err < /dev/null; echo "exit $?"
```

  Check: prints `exit 0`; `sed -n 1p /private/tmp/sx9-p315/cc.out > /private/tmp/sx9-p315/S.txt && test -d "$(cat /private/tmp/sx9-p315/S.txt)" && cat /private/tmp/sx9-p315/S.txt` prints a folder whose name starts `p315.` (step 1's `echo "$S"`); `cat "$(cat /private/tmp/sx9-p315/S.txt)/evidence.txt"` prints one line starting `general-purpose ` and one starting `p315:probe ` (step 3). If the exit is not 0 or a line is missing, read `cc.out` and `cc.err`, run step 8 for this `S` (A11 commands), move the folder aside (`mv /private/tmp/sx9-p315 /private/tmp/sx9-p315-failed1`) and repeat from A3.
- [x] A5. Write the OpenCode script (block 4 with the chosen models, `S` set from `S.txt`):

```sh
{ echo 'S=$(cat /private/tmp/sx9-p315/S.txt)'; sed -e 's#<provider>/<MAIN>#openrouter/anthropic/claude-sonnet-4.6#g' -e 's#<provider>/<SUB>#openrouter/anthropic/claude-haiku-4.5#g' /private/tmp/sx9-p315/blocks/b4.sh; } > /private/tmp/sx9-p315/oc.sh
```

  Check: `grep -c '<provider>' /private/tmp/sx9-p315/oc.sh` prints `0`; `grep -o 'openrouter/anthropic/claude-sonnet-4.6' /private/tmp/sx9-p315/oc.sh | wc -l` prints `2`; `grep -o 'openrouter/anthropic/claude-haiku-4.5' /private/tmp/sx9-p315/oc.sh | wc -l` prints `1`.
  - Ran instead (see Deviation in `## Result`): `oc.sh` is one line, `cd "$S" && P315_LOG="$S/oc.jsonl" zygo opencode --profile glm -- -m zygo/lead` (MAIN = `zygo/lead`, SUB = `zygo/code`), with block 4's `probe.js` copied unchanged into `$S/.opencode/plugins/`. Check result: no `<provider>`, no `openrouter`, no `OPENCODE_CONFIG_DIR` or `OPENCODE_CONFIG_CONTENT` in `oc.sh`.
- [x] A6. Start step 4 in a private tmux server with the clean environment: `sh /private/tmp/sx9-p315/clean.sh tmux -L p315 -f /dev/null new-session -d -s oc -x 200 -y 50 /bin/sh /private/tmp/sx9-p315/oc.sh`. Check: `tmux -L p315 has-session -t oc` exits 0, and `tmux -L p315 capture-pane -p -t oc > /private/tmp/sx9-p315/pane-0.txt` shows the OpenCode prompt box (read the file). If the pane shows an error instead, save it, `tmux -L p315 kill-server`, fix the cause and repeat A6.
- [x] A7. Step 5, first line: `tmux -L p315 send-keys -t oc -l 'Use the general subagent to read probe.py and tell me its first line.'` then `tmux -L p315 send-keys -t oc Enter`. Wait until the main turn has finished (poll with `tmux -L p315 capture-pane -p -t oc`, or with Monitor and an until-loop on `grep -qF 'import json, sys'` over that capture). Then `tmux -L p315 capture-pane -p -J -S - -t oc > /private/tmp/sx9-p315/pane-1.txt`. Check: record the answer as `yes` if `pane-1.txt` shows the reply with the first line of `probe.py` (`import json, sys`), else `no`; `grep -c '"hook":"transform"' "$(cat /private/tmp/sx9-p315/S.txt)/oc.jsonl"` prints 1 or more.
- [x] A8. Step 5, compaction: `tmux -L p315 send-keys -t oc -l '/compact'` then `tmux -L p315 send-keys -t oc Enter`. Wait until `grep -q '"hook":"compacting"' "$(cat /private/tmp/sx9-p315/S.txt)/oc.jsonl"` succeeds and the pane shows the summary. Then `tmux -L p315 capture-pane -p -J -S - -t oc > /private/tmp/sx9-p315/pane-2.txt`. Check: `grep -c '"hook":"compacting"' "$(cat /private/tmp/sx9-p315/S.txt)/oc.jsonl"` prints 1 or more. If `/compact` opened a menu instead of running, press `Enter` once more and wait again.
- [x] A9. Step 5, last turn and exit: `tmux -L p315 send-keys -t oc -l 'Reply OK.'` then `tmux -L p315 send-keys -t oc Enter`; wait until the pane shows the reply; `tmux -L p315 capture-pane -p -J -S - -t oc > /private/tmp/sx9-p315/pane-3.txt`; then `tmux -L p315 send-keys -t oc -l '/exit'` and `tmux -L p315 send-keys -t oc Enter`. Check: `tmux -L p315 has-session -t oc` exits non-zero (OpenCode ended, so the session and server ended). If it still runs after the exit, `tmux -L p315 kill-server`, then the same check.
- [x] A10. Step 6 (capture), from the node worktree root:

```sh
{ echo 'S=$(cat /private/tmp/sx9-p315/S.txt)'; cat /private/tmp/sx9-p315/blocks/b5.sh; } > /private/tmp/sx9-p315/capture.sh && sh /private/tmp/sx9-p315/clean.sh /bin/sh /private/tmp/sx9-p315/capture.sh > /private/tmp/sx9-p315/capture.out 2> /private/tmp/sx9-p315/capture.err; echo "exit $?"
```

  Check: prints `exit 0`; `grep -E '^[0-9]+\.[0-9]+\.[0-9]+( \(Claude Code\))?$' /private/tmp/sx9-p315/capture.out` prints the two version lines (`<version> (Claude Code)` and the OpenCode version); `git status --porcelain` prints exactly `?? fixtures/claude-code/live/st-evidence.txt`, `?? fixtures/claude-code/live/st-hooks.jsonl`, `?? fixtures/opencode/live/st-transform.jsonl`.
- [x] A11. Step 7 (hand-off) and step 8 (cleanup).
  - Keep the raw run folder: `cp -R "$(cat /private/tmp/sx9-p315/S.txt)" /private/tmp/sx9-p315/S-copy`. Check: `ls /private/tmp/sx9-p315/S-copy/hooks.jsonl /private/tmp/sx9-p315/S-copy/evidence.txt /private/tmp/sx9-p315/S-copy/oc.jsonl` lists three files.
  - Write the hand-off (step 7). With the A7 answer `yes`: `{ grep -E '^[0-9]+\.[0-9]+\.[0-9]+( \(Claude Code\))?$' /private/tmp/sx9-p315/capture.out; echo 'provider: openrouter'; echo 'MAIN: anthropic/claude-sonnet-4.6'; echo 'SUB: anthropic/claude-haiku-4.5'; echo 'subagent answer shown in step 5: yes'; echo "date: $(date +%Y-%m-%d)"; } > /private/tmp/sx9-p315/handoff.txt`. With the answer `no`, the same command with `step 5: no` in place of `step 5: yes`. Check: `cat /private/tmp/sx9-p315/handoff.txt` prints 7 lines, the fifth ends `yes` or `no`.
  - Step 8. Block 6 edits `~/.claude.json`, and step 8 says to close every Claude Code session first. The lead session uses its own `CLAUDE_CONFIG_DIR`, not `~/.claude.json`; other sessions of the operator may use it. Ask the operator whether a Claude Code session on the default config folder is open. If none: `{ echo 'S=$(cat /private/tmp/sx9-p315/S.txt)'; cat /private/tmp/sx9-p315/blocks/b6.sh; } > /private/tmp/sx9-p315/cleanup.sh && sh /private/tmp/sx9-p315/clean.sh /bin/sh /private/tmp/sx9-p315/cleanup.sh > /private/tmp/sx9-p315/cleanup.out 2>&1; echo "exit $?"` → `exit 0`, and `cleanup.out` has a line starting `removed [`; delete `~/.claude.json.bak-p315` once Claude Code starts normally. If one is open: run only `/bin/sh -c 'rm -rf ~/.claude/projects/*p315*'` and `rm -rf "$(cat /private/tmp/sx9-p315/S.txt)"`, and tell the operator to run block 6's second and third lines (`cp ~/.claude.json …`, `python3 -I -c …`) later. Check: `test ! -e "$(cat /private/tmp/sx9-p315/S.txt)"` exits 0; `/bin/sh -c 'ls -d ~/.claude/projects/*p315*'` finds nothing.
  - Ran instead: the copy as written (three files listed); the hand-off with the glm values in place of the `openrouter` lines (`provider: zygo (profile glm, Z.AI)`, `MAIN: zygo/lead (glm-5.3)`, `SUB: zygo/code (glm-5.3-flash)`, `subagent answer shown in step 5: yes`; 7 lines); step 8 by the full block 6 (`cleanup.out` is `removed []`).
- [x] A12. Hand off to the implementer: the node worktree `.worktrees/phase3/SX-9-node` as it is (three fixtures uncommitted) and the folder `/private/tmp/sx9-p315` (kept).

### Part B — implementer (zygo-code)

Work in `.worktrees/phase3/SX-9-node`, branch `phase3/SX-9--SX-9`. Do not run Claude Code, OpenCode or any block of P3-15.

- [x] B1. Check the hand-off: `git branch --show-current` prints `phase3/SX-9--SX-9`; `git status --porcelain` prints exactly the three `??` lines of A10; `cat /private/tmp/sx9-p315/handoff.txt` prints 7 lines (Claude Code version, OpenCode version, provider, MAIN, SUB, answer `yes` or `no`, date). If a check fails, stop and report to the lead (Part A is not done).
- [x] B2. Fixtures not empty: `for f in fixtures/claude-code/live/st-hooks.jsonl fixtures/claude-code/live/st-evidence.txt fixtures/opencode/live/st-transform.jsonl; do test -s "$f" || echo "empty $f"; done` prints nothing.
- [x] B3. Fixtures scrubbed. Each command prints nothing:
  - `grep -niE "$(id -un)|/Users/|/private/var/|/var/folders/" fixtures/claude-code/live/st-hooks.jsonl fixtures/claude-code/live/st-evidence.txt fixtures/opencode/live/st-transform.jsonl`
  - `for h in "$(hostname)" "$(hostname -s)" "$(scutil --get LocalHostName)" "$(scutil --get ComputerName)"; do grep -niF -- "$h" fixtures/claude-code/live/st-hooks.jsonl fixtures/claude-code/live/st-evidence.txt fixtures/opencode/live/st-transform.jsonl; done`
  - `grep -nE '[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}' fixtures/claude-code/live/st-hooks.jsonl fixtures/claude-code/live/st-evidence.txt fixtures/opencode/live/st-transform.jsonl`
  - If one prints a line, stop and report it to the lead; do not edit the fixture by hand.
- [x] B4. Read ST1 and ST2 by T§12.2 "Reading the result": `cat fixtures/claude-code/live/st-evidence.txt`. Each line is `<agent_type> transcript|no-transcript canary:<kinds>|canary:none`.
  - ST1 (the `general-purpose` line): `transcript` and `canary:` with at least one entry → `verified`; `transcript` and `canary:none` → `failed`; `no-transcript` → not decided.
  - ST2 (the `p315:probe` line): the same rule.
  - A missing line (the agent was not called) is neither case of the spec: record it as not decided and report it.
- [x] B5. Read ST3 and ST4: `python3 -I -c 'import json,sys; [print(n, d.get("hook"), d.get("model"), d.get("session"), ",".join(d.get("keys") or [])) for n, d in enumerate(map(json.loads, open(sys.argv[1])), 1)]' fixtures/opencode/live/st-transform.jsonl` (line number, hook, model, session, keys; the file order is the order of the calls).
  - MAIN and SUB: for this run, MAIN = `lead` and SUB = `code` (the probe logs the model id without the `zygo/` provider; Deviation in `## Result`). `anthropic/claude-sonnet-4.6` and `anthropic/claude-haiku-4.5` apply to the planned `openrouter` run.
  - ST3 `verified`: the first `transform` line with model MAIN after the compaction request completes (not the compaction request itself), with the same session as the `compacting` line when both lines have one (T§12.1 ST3: "the first main-model request after a compaction"; T§9.3). `failed`: no such line, while a `transform` line before the `compacting` line exists. Neither → not decided.
  - ST4 `verified`: a `transform` line with model SUB. `failed`: none, and `handoff.txt` says `subagent answer shown in step 5: yes`. Neither → not decided.
  - Copy the `keys` of the first `transform` line and of the `compacting` line (information for the SX-10 rows).
- [x] B6. If any of ST1-ST4 is not decided: stop here, commit nothing, and report the lines to the lead (C3).
- [x] B7. Record the result: tick every Part A box that the hand-off proves and B1-B5, then append to this file (after Part C) a section `## Result` with these lines, the values from `handoff.txt`, B4 and B5:
  - `- Run: <date>, on the operator's machine, by the lead session (operator decision 2026-11-25).`
  - `- \`claude --version\`: <line 1 of handoff.txt>; \`opencode --version\`: <line 2>.`
  - `- OpenCode models: provider \`openrouter\`, MAIN \`anthropic/claude-sonnet-4.6\`, SUB \`anthropic/claude-haiku-4.5\`; subagent answer shown in step 5: <yes|no>.` For this run the values are lines 3-6 of `handoff.txt` (provider `zygo` with profile `glm`, MAIN `zygo/lead`, SUB `zygo/code`), and a `- Deviation:` line follows it; the `openrouter` values apply to the planned run.
  - `- ST1: <verified|failed> — \`fixtures/claude-code/live/st-evidence.txt\` line \`<the general-purpose line>\`.`
  - `- ST2: <verified|failed> — the same file, line \`<the p315:probe line>\`.`
  - `- ST3: <verified|failed> — \`fixtures/opencode/live/st-transform.jsonl\`: <line numbers of the compacting line and the deciding transform line, with their sessions>.`
  - `- ST4: <verified|failed> — the same file: <line number of the SUB transform line, or "no transform line with model <SUB>">.`
  - `- keys: transform \`<keys>\`; compacting \`<keys>\`.`
  - Check: `grep -c '^- ST[1-4]: \(verified\|failed\) ' docs/work/phase3/nodes/SX-9.md` prints `4`; the file has no `/Users/`, no user name, no e-mail address.
- [x] B8. Acceptance: `for f in fixtures/claude-code/live/st-hooks.jsonl fixtures/claude-code/live/st-evidence.txt fixtures/opencode/live/st-transform.jsonl; do test -s "$f" || exit 1; done && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh` → exit 0, last line starts `check-premises: ok`. Run it in a subshell (`sh -c '…'`) so the `exit 1` cannot end the session's shell.
- [x] B9. Scope: `git status --porcelain` prints exactly ` M docs/work/phase3/nodes/SX-9.md` and the three `??` fixture lines.
- [x] B10. Commit and push: `git add fixtures/claude-code/live/st-hooks.jsonl fixtures/claude-code/live/st-evidence.txt fixtures/opencode/live/st-transform.jsonl docs/work/phase3/nodes/SX-9.md`, then `git commit -m "SX-9: ST1-ST4 evidence" -m "<the Zygo-Session and Zygo-Work trailers of the implementer's brief, one per line>"` with no Claude attribution, then `git push -u origin phase3/SX-9--SX-9`. Check: `git show --stat --format= HEAD` lists the four files and nothing else; `git status -sb` shows the branch tracking `origin/phase3/SX-9--SX-9` with nothing ahead.
- [ ] B11. Report: the commit hash, the four ST lines of `## Result`, the two version lines, and each `failed` id with its C2 line.

### Part C — a `failed` or not-decided ST line

A `failed` line passes this node (acceptance: each of ST1-ST4 `verified` or `failed`). SX-10 depends on SX-9 (`docs/work/phase3/graph.md:257`) and records the rows and the code change of each result (T§12.1 items 1-5); FX-9 depends on SX-10 (`graph.md:266`).

- [ ] C1. ST1 or ST2 `failed`: documented behavior is contradicted. The lead reports it to the operator; no code changes by itself (T§12.1 table, T-C6 at `docs/specs/2026-11-24-session-text-design.md:49`). SX-10 still records the row and adds the dated note `*(Note YYYY-MM-DD, P3-15, STn: <result>)*` to T§12.1 (item 3).
- [ ] C2. ST3 or ST4 result, consumed by SX-10 (T§12.1 table; T§17 check 14 at `session-text-design.md:442`):
  - ST3 `verified`: no change. ST3 `failed`: fallback F3 (T§9.3): the plugin's `experimental.session.compacting` handler also runs `session-start --event subagent` and pushes the rule line and its output into `output.context` (`crates/zygo-launch/assets/opencode-plugin.js`).
  - ST4 `verified`: the OpenCode `zygo-<role>` prompts lose the mechanics text (T§7.2 item 3; `crates/zygo-launch/src/opencode.rs`). ST4 `failed`: the `--event subagent` branch and its tests are removed (T§7.2 item 3; `crates/zygo-hooks/src/session_start.rs`, `opencode-plugin.js`).
  - Report line per `failed` id (B11): `SX-10 <id> failed: <the T§12.1 "If failed" cell>`.
- [ ] C3. Not decided (an ST1 or ST2 line with `no-transcript`, a missing line, or ST3/ST4 with neither condition true): no row (T§12.2 "Reading the result"), the node is not done, and SX-10 cannot start. The lead runs the check again: `mv /private/tmp/sx9-p315 /private/tmp/sx9-p315-run1`, `rm fixtures/claude-code/live/st-hooks.jsonl fixtures/claude-code/live/st-evidence.txt fixtures/opencode/live/st-transform.jsonl` in the node worktree, and repeat Part A from A3; then Part B from B1.

## Result

- Run: 2026-11-25, on the operator's machine, by the lead session (operator decision 2026-11-25).
- `claude --version`: 2.1.332 (Claude Code); `opencode --version`: 1.18.16.
- OpenCode models: provider zygo (profile `glm`, Z.AI), MAIN `zygo/lead` (glm-5.3), SUB `zygo/code` (glm-5.3-flash); subagent answer shown in step 5: yes.
- Deviation: OpenCode models changed from OpenRouter/Anthropic to glm on Z.AI by operator instruction (2026-11-25); the launch was `zygo opencode --profile glm -- -m zygo/lead` with the probe plugin in the run folder's `.opencode/plugins/`, because zygo sets its own OPENCODE_CONFIG_DIR.
  - Block 4's `OPENCODE_CONFIG_CONTENT` (`small_model` = MAIN, `general` → SUB) was not used. `small_model` = `zygo/read` and `general` → `code` came from zygo's own OpenCode config (`crates/zygo-launch/src/opencode.rs:18`, `:153-157`). So the title request (model `read`, line 1) is told apart from a subagent request (model `code`) because `read` ≠ `code`.
  - zygo's OpenCode plugin was loaded beside the probe and changes `output.system` on every `zygo/` request. This departs from "It runs no zygo" in `docs/live-checks.md:152` (P3-15 intro). It cannot write probe lines: its transform only pushes into `output.system` (`crates/zygo-launch/assets/opencode-plugin.js:186-191`), and only the probe appends to `P315_LOG`.
  - Four OpenCode attempts failed before the recorded run. In each failed log the main session has two `transform` lines on one model and then nothing: no SUB line and no `compacting` line, so the run stopped after the first turn.
    - openrouter: model `anthropic/claude-sonnet-4.6`, two `transform` lines, then none.
    - Z.AI coding plan: model `glm-5.4`, two `transform` lines, then none.
    - Z.AI direct: model `glm-5.3`, two `transform` lines, then none.
    - Z.AI bad model ref: model `glm-5.3`, two `transform` lines, then none.
- ST1: verified — `fixtures/claude-code/live/st-evidence.txt` line `general-purpose transcript canary:attachment,attachment,api-request-blob`.
- ST2: verified — the same file, line `p315:probe transcript canary:attachment,attachment,api-request-blob`.
- ST3: verified — `fixtures/opencode/live/st-transform.jsonl`: compacting line 6 (session `ses_placeholder0001`), deciding transform line 8 with model `lead` (session `ses_placeholder0001`), the first main-model request after compaction finished (after `Compaction · lead` in the pane, before `Reply OK.`). Line 7 (model `lead`, 151 ms after line 6) is the compaction summary request itself, not the first request after it (T§12.1 ST3, T§9.3).
- ST4: verified — the same file: line 3 (transform, model `code`, session `ses_placeholder0002`; line 4 also).
- keys: transform `model,sessionID`; compacting `sessionID`.

# FIX-4 — live scripts state the agents they need; lib.sh checks only those

**Spec:** `docs/specs/2026-11-18-zygo-design.md` §19, §19a (premise harness; `docs/premises.md`, Method) and §20 #16b, #21, #28, #40 (live checks; `docs/live-checks.md`, Method); found defect, graph Log 2026-11-21
**Files:** `scripts/premises/lib.sh`, `scripts/premises/lib_test.sh`, `scripts/premises/live.sh`, `scripts/premises/ver_test.sh`, `docs/premises.md`, `docs/live-checks.md`
**Consumes:** —
**Produces:** `N2_AGENTS`: space-separated list of `claude` and `opencode`, set by the script as a plain shell variable (not exported) before it sources `scripts/premises/lib.sh`; not set or empty means both. `n2_need <claude|opencode>` (`scripts/premises/lib.sh`). `live_needs <claude|opencode>` (`scripts/premises/live.sh`)
**Acceptance:** `sh scripts/premises/ver_test.sh && sh scripts/premises/lib_test.sh && sh scripts/check-premises.sh && sh scripts/premises/live-selftest.sh` → exit 0 with `ver tests: ok (53)`, `lib tests: ok (38)`, `check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7)` and `live selftest: ok (54)`

## Plan

Work in the node's checkout (`.worktrees/phase2/FIX-4`). Run each command from the repository root. Each commit message ends, after one empty line, with the two adjacent lines `Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ` and `Zygo-Work: FIX-4`. In each commit, tick (`- [x]`) the steps of this file that are done. A step names a line by its quoted text, not by its number. Do not start a real agent session and do not run an installed `zygo`. The four test commands use stub programs only. Do not put a stand-in `opencode` program on `PATH`: the tests make their own stubs in their own temporary directories.

Facts of the live code (read before you start):

- `scripts/premises/lib.sh` stops with exit 1 when `claude` or `opencode` is not on `PATH` (the two lines after the comment `# The PATH checks come before the scratch dir`). `TOOL_PATH` uses `dirname` of both programs.
- `scripts/premises/live.sh`: `pinned_agents` starts both programs with `--version` and checks both. `LIVE_VER` starts as `PIN_CC`. `result` prints `LIVE <id> <result> (agent <LIVE_VER>): <text>` and appends that line to `$N2_SCRATCH/results.txt`. `live_end` writes no record.
- `scripts/premises/ver_test.sh` sources `live.sh` without `lib.sh` and sets `CLAUDE_BIN` and `OPENCODE_BIN` itself, under `set -u`. So `live.sh` must read `N2_AGENTS` as `${N2_AGENTS:-claude opencode}`.
- `scripts/premises/lib_test.sh` runs each child with `env PATH="$CP" …`, which passes the environment of the test to the child. `mkbin` makes the stub programs.
- Scripts that source `lib.sh` today: `run-all.sh`, `run-smoke.sh` (uses both agents), `run-opencode.sh` and `live-opencode.sh` (use OpenCode only), and 18 scripts that use Claude Code only (`run-P1.sh`, `run-P1-interactive.sh`, `run-P2.sh`, `run-P3.sh`, `run-P6.sh`, `run-Q1.sh`, `run-Q4.sh`, `run-Q5.sh`, `run-Q6.sh`, `run-Q7-claude.sh`, `live-claude.sh`, `live-16b.sh`, `live-deadline.sh`, `live-doctor.sh`, `live-passthrough.sh`, `live-rt.sh`, `live-strict.sh`, `live-window.sh`). This node changes none of them: each names no agent, so each needs both, as today.
- Counts before the change, read on 2026-11-21: `ver tests: ok (50)`, `lib tests: ok (27)`, `live selftest: ok (54)`.
- dash is not installed on this machine, and `/bin/sh` is bash. The new cases run under bash only.

Design (author's call where the brief gives no value):

- **Variable.** `N2_AGENTS`. Values: `claude`, `opencode`, or both, separated by spaces, in any order. Not set or empty: both. The script sets it on a line before the line that sources `lib.sh`, and does not export it. After the source, `lib.sh` has set it to one of three exact values: `claude opencode`, `claude`, `opencode`.
- **Errors, all before the scratch dir is made, message on stdout as the PATH messages.**
  - `N2_AGENTS` is in the environment: `premises: N2_AGENTS is in the environment; only a script sets it (run: unset N2_AGENTS)`, exit 2. Reason: an inherited value would change the need of a script that names no agent. Accepted limit: a script that sets the variable in a shell that also has it exported gets the same message.
  - A character that is not `a-z` or a space: `premises: N2_AGENTS is not a space-separated list of claude and opencode`, exit 2. This also stops a `*` before the word split.
  - An unknown name: `premises: N2_AGENTS names an unknown agent '<name>' (known: claude, opencode)`, exit 2.
  - Only spaces: `premises: N2_AGENTS names no agent (known: claude, opencode)`, exit 2.
  - A named agent that is not on `PATH`: the messages of today, exit 1.
- **One rule for an agent that is not named.** It is not looked up, not started and not checked, it gets no value, and no row or line is printed for it. A helper whose job is to start it exits 2.
  - `lib.sh`: `CLAUDE_BIN` or `OPENCODE_BIN` is the empty string. `TOOL_PATH` has no entry for it (not `.`).
  - `lib.sh`: `run_claude`, `run_claude_stream` and `tmux_claude` call `n2_need claude` first. `n2_need` prints `premises: this step needs <agent>, but N2_AGENTS is '<value>'` on stderr and exits 2. Without this, `tmux_claude` would write a command line with an empty program word.
  - `live.sh`: `pinned_agents` does not start it and does not check it. `LIVE_CC_VER` or `LIVE_OC_VER` is the empty string, and both are exported as today. `LIVE_VER` is `LIVE_CC_VER`, or `LIVE_OC_VER` when `claude` is not named.
  - `live.sh`: the start value of `LIVE_VER` is `PIN_CC`, or `PIN_OC` when `claude` is not named.
- **The record.** `result` prints the line of today when the run needs both agents: `LIVE <id> <result> (agent <version>): <text>`. When the run needs one agent, the line is `LIVE <id> <result> (agent <version>; needs only <agent>): <text>`. The same line goes to stdout and to `results.txt`. So a line without `needs only` is a run that checked both agents, and every line written before this node stays correct.
- **No change** to `scripts/premises/run-all.sh` (its comment stays true: it names no agent), to `scripts/premises/live-selftest.sh` (it has no case on this; it is in the acceptance check to show that nothing else changed), or to `scripts/check-premises.sh`.

Interfaces (as they exist now):

- Uses: `pinned_agents`, `agent_ver <--version output>`, `agent_ok <version word> <min>`, `agent_show <--version output>` (`scripts/premises/live.sh`, node PRB-2). The contract field for consumed names is `—`: PRB-2 gives these names in bullet lines below its Produces label, and the graph validator reads only the label line (`FIX-4 consumes pinned_agents from PRB-2, which does not produce it`). PRB-2 is DONE and merged.
- Gives: `N2_AGENTS`, `n2_need <claude|opencode>`, `live_needs <claude|opencode>` (exit status 0 when the script needs that agent, else 1).

### Task 1: the failing tests

- [ ] **1.1** `scripts/premises/lib_test.sh`: change the line `unset N2_SCRATCH N2_SCRATCH_OWN` to `unset N2_SCRATCH N2_SCRATCH_OWN N2_AGENTS`.
- [ ] **1.2** `scripts/premises/lib_test.sh`, `mkbin`: change the comment `# mkbin <dir> <with claude: 0|1>` to `# mkbin <dir> <with claude: 0|1> [<with opencode: 0|1>; default 1]`. Replace the two lines that write and `chmod` `"$1/opencode"` with this one line:

```sh
  if [ "${3:-1}" = 1 ]; then printf '#!/bin/sh\necho 0\n' >"$1/opencode"; chmod +x "$1/opencode"; fi
```

  Below the line `mkbin "$T/nocc" 0` add the line `mkbin "$T/nooc" 1 0`.
- [ ] **1.3** `scripts/premises/lib_test.sh`, `prep`: in its comment, change `and resets CP and NR.` to `and resets CP, NR and PRE` and add the comment line `# (PRE: one line that the child script runs before it sources lib.sh; node FIX-4).` below it. Change `XENV="" CP=$T/bin NR=0` to `XENV="" CP=$T/bin NR=0 PRE=""`.
- [ ] **1.4** `scripts/premises/lib_test.sh`, `child`: between the lines `echo "PREMISES_DIR='$HERE'"` and `echo ". '$HERE/lib.sh'"` add:

```sh
    [ -z "$PRE" ] || printf '%s\n' "$PRE"
```

- [ ] **1.5** `scripts/premises/lib_test.sh`: directly above the line that starts `# --- R: run-all.sh` add this block, followed by one empty line:

```sh
# --- A: N2_AGENTS, the agents a script needs (node FIX-4) ----------------------------------------
# nodir <case> <stdout line>: stdout holds that line, no dir was made, stderr is empty.
nodir() {
  grep -qxF -- "$2" "$T/$1.out" || { why="stdout: $(tr '\n' '|' <"$T/$1.out")"; return 1; }
  [ -z "$(ls -A "$T/tmp-$1")" ] || { why="$T/tmp-$1 holds: $(ls -A "$T/tmp-$1")"; return 1; }
  [ ! -s "$ERR" ] || { why="stderr: $(tr '\n' '|' <"$ERR")"; return 1; }
}
# vact <case>: the action that writes "[CLAUDE_BIN][OPENCODE_BIN][N2_AGENTS][TOOL_PATH]" to $T/<case>.v.
vact() { printf '%s' "echo \"[\$CLAUDE_BIN][\$OPENCODE_BIN][\$N2_AGENTS][\$TOOL_PATH]\" >'$T/$1.v'; exit 0"; }
# vals <case> <want>: $T/<case>.v holds <want>, and the own dir was removed.
vals() {
  [ "$(cat "$T/$1.v" 2>/dev/null)" = "$2" ] || { why=".v: $(cat "$T/$1.v" 2>/dev/null)"; return 1; }
  removed "$1"
}
# TP: the end of TOOL_PATH in a child (no node on its PATH, so the node entry is /usr/bin).
TP=/usr/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin

c=A1; prep $c own; ERR=$T/$c.err; CP=$T/nooc; child $c 0 'exit 0'; run $c
res "A1 no N2_AGENTS: both are needed, stops when opencode is not on PATH" 1 nodir $c 'premises: opencode is not on PATH'

c=A2; prep $c own; ERR=$T/$c.err; CP=$T/nooc; PRE='N2_AGENTS="claude"'; child $c 0 "$(vact $c)"; run $c
res "A2 claude only passes without opencode on PATH" 0 vals $c "[$T/nooc/claude][][claude][$T/nooc:$TP]"

c=A3; prep $c own; ERR=$T/$c.err; CP=$T/nocc; PRE='N2_AGENTS="opencode"'; child $c 0 "$(vact $c)"; run $c
res "A3 opencode only passes without claude on PATH" 0 vals $c "[][$T/nocc/opencode][opencode][$T/nocc:$TP]"

c=A4; prep $c own; ERR=$T/$c.err; PRE='N2_AGENTS="claude nosuch"'; child $c 0 'exit 0'; run $c
res "A4 an unknown agent name exits 2" 2 nodir $c "premises: N2_AGENTS names an unknown agent 'nosuch' (known: claude, opencode)"

c=A5; prep $c own; ERR=$T/$c.err; XENV="N2_AGENTS=claude"; child $c 0 'exit 0'; run $c
res "A5 N2_AGENTS from the environment exits 2" 2 nodir $c 'premises: N2_AGENTS is in the environment; only a script sets it (run: unset N2_AGENTS)'

c=A6; prep $c own; ERR=$T/$c.err; PRE='N2_AGENTS="opencode  claude claude"'; child $c 0 "$(vact $c)"; run $c
res "A6 the value is put in the order claude opencode, each name once" 0 vals $c "[$T/bin/claude][$T/bin/opencode][claude opencode][$T/bin:$T/bin:$TP]"

c=A7; prep $c own; ERR=$T/$c.err; PRE='N2_AGENTS="  "'; child $c 0 'exit 0'; run $c
res "A7 a blank value exits 2" 2 nodir $c 'premises: N2_AGENTS names no agent (known: claude, opencode)'

c=A8; prep $c own; ERR=$T/$c.err; PRE='N2_AGENTS="claude *"'; child $c 0 'exit 0'; run $c
res "A8 a character that is not a-z or a space exits 2" 2 nodir $c 'premises: N2_AGENTS is not a space-separated list of claude and opencode'

# rec <case> <line>: results.txt and stdout hold exactly that LIVE line; the caller's dir is kept.
rec() {
  [ "$(cat "$T/keep-$1/results.txt" 2>/dev/null)" = "$2" ] || { why="results.txt: $(tr '\n' '|' <"$T/keep-$1/results.txt")"; return 1; }
  [ "$(cat "$T/$1.out")" = "$2" ] || { why="stdout: $(tr '\n' '|' <"$T/$1.out")"; return 1; }
  kept "$1"
}
c=A9; prep $c caller; ERR=$T/$c.err; CP=$T/nooc; PRE='N2_AGENTS="claude"'
child $c 1 'result X1 pass one; exit 0'; run $c
res "A9 the LIVE line of a claude-only run names the one agent" 0 rec $c 'LIVE X1 pass (agent 2.1.328; needs only claude): one'

c=A10; prep $c caller; ERR=$T/$c.err; CP=$T/nocc; PRE='N2_AGENTS="opencode"'
child $c 1 'result X1 pass one; exit 0'; run $c
res "A10 the LIVE line of an opencode-only run names the one agent" 0 rec $c 'LIVE X1 pass (agent 1.18.16; needs only opencode): one'

c=A11; prep $c caller; ERR=$T/$c.err; CP=$T/nocc; PRE='N2_AGENTS="opencode"'
child $c 0 "r=0; (run_claude a) || r=\$?; s=0; (run_claude_stream o m) || s=\$?; t=0; (tmux_claude x a) || t=\$?; echo \"\$r \$s \$t\" >'$T/A11.v'; exit 0"
run $c
a11() {
  [ "$(cat "$T/A11.v" 2>/dev/null)" = "2 2 2" ] || { why=".v: $(cat "$T/A11.v" 2>/dev/null)"; return 1; }
  l="premises: this step needs claude, but N2_AGENTS is 'opencode'"
  [ "$(cat "$ERR")" = "$l
$l
$l" ] || { why="stderr: $(tr '\n' '|' <"$ERR")"; return 1; }
  [ -f "$T/keep-A11/mine" ] || { why="$T/keep-A11/mine is gone"; return 1; }
}
res "A11 a helper that starts claude exits 2 when claude is not named" 0 a11
```

  The default record (both agents, the line of today) stays covered by the existing case L11.
- [ ] **1.6** `scripts/premises/lib_test.sh`, header comment: after the line `# live_traps and live_end, and under run-all.sh.` add the line `# Also N2_AGENTS (node FIX-4): the agents a script needs, cases A1-A11.`
- [ ] **1.7** `scripts/premises/ver_test.sh`: change the line `unset N2_SCRATCH N2_SCRATCH_OWN` to `unset N2_SCRATCH N2_SCRATCH_OWN N2_AGENTS`. Directly above the line that starts `# --- K: check-premises.sh agent rows` add this block, followed by one empty line:

```sh
# P16-P18 (node FIX-4): pinned_agents checks only the agents that N2_AGENTS names; the version
# variable of an agent that is not named is empty, and its program is not started.
pdefaults; printf '%s\n' '1.15.7' >"$T/oc.txt"; N2_AGENTS=claude; prun; unset N2_AGENTS
if pcheck "P16 claude only: opencode below the pin is not read" 0 "=2.1.330 2.1.330 "; then
  if [ "$(cat "$T/p.out")" = "2.1.330 2.1.330 ${NL}2.1.330 " ]; then ok "P16 claude only: opencode below the pin is not read"
  else no "P16 claude only: opencode below the pin is not read" "output: $(tr '\n' '|' <"$T/p.out")"; fi
fi

pdefaults; printf '%s\n' '2.1.327 (Claude Code)' >"$T/cc.txt"; N2_AGENTS=opencode; prun; unset N2_AGENTS
if pcheck "P17 opencode only: claude below the pin is not read" 0 "=1.18.16  1.18.16"; then
  if [ "$(cat "$T/p.out")" = "1.18.16  1.18.16${NL} 1.18.16" ]; then ok "P17 opencode only: claude below the pin is not read"
  else no "P17 opencode only: claude below the pin is not read" "output: $(tr '\n' '|' <"$T/p.out")"; fi
fi

pdefaults; printf '%s\n' '1.15.7' >"$T/oc.txt"; N2_AGENTS=opencode; prun; unset N2_AGENTS
pcheck "P18 opencode only: opencode below the pin" 1 "live: opencode is 1.15.7; the minimum is 1.18.16" && ok "P18 opencode only: opencode below the pin"
```

  The expected texts of P16 and P17 hold a space at the end of a line (`"2.1.330 2.1.330 "`, `"2.1.330 "`) and two spaces in `"1.18.16  1.18.16"`: an empty version variable is between them. Keep them exactly.
- [ ] **1.8** Run `sh scripts/premises/lib_test.sh`. Expected: exit 1, last line `lib tests: 10 failed`. `A1` is `ok` (the behavior of today). `A2` to `A11` are `FAIL`. Each `L`, `E`, `C` and `R` case is `ok`.
- [ ] **1.9** Run `sh scripts/premises/ver_test.sh`. Expected: exit 1, last line `ver tests: 2 failed`. `P16` and `P17` are `FAIL` with the text `the minimum is`. `P18` is `ok` (the behavior of today).

### Task 2: the mechanism

- [ ] **2.1** `scripts/premises/lib.sh`, header: replace the four comment lines from `# N2_SCRATCH: scratch directory for isolated HOMEs` to `# live_end does the same.` with:

```sh
# N2_AGENTS (node FIX-4): the agents this script needs, a space-separated list of claude and
# opencode. The script sets it as a plain shell variable (not exported) before it sources this
# file; not set or empty means both. Only a named agent is looked up on PATH here and held to its
# minimum version by live.sh's pinned_agents. For an agent that is not named: CLAUDE_BIN or
# OPENCODE_BIN is empty, TOOL_PATH has no entry for it, and a helper that starts it exits 2
# (n2_need). After the source, N2_AGENTS is "claude opencode", "claude" or "opencode".
# N2_SCRATCH: scratch directory for isolated HOMEs and raw captures. Set by the caller: kept. Unset
# or empty: after the N2_AGENTS and PATH checks pass, lib.sh makes one (mktemp -d) and prints
# its path; on exit it removes it after status 0 and keeps it (named on stderr) otherwise. live.sh's
# live_end does the same.
```

- [ ] **2.2** `scripts/premises/lib.sh`: replace the comment line `# The PATH checks come before the scratch dir, so a failed check makes no dir.` and the two lines below it (`CLAUDE_BIN=…`, `OPENCODE_BIN=…`) with:

```sh
# The N2_AGENTS and PATH checks come before the scratch dir, so a failed check makes no dir.
# N2_AGENTS never comes from the environment: an inherited value would change the need of a script
# that names no agent.
if env | grep -q '^N2_AGENTS='; then
  echo "premises: N2_AGENTS is in the environment; only a script sets it (run: unset N2_AGENTS)"; exit 2
fi
case "${N2_AGENTS:-}" in
  *[!a-z\ ]*) echo "premises: N2_AGENTS is not a space-separated list of claude and opencode"; exit 2 ;;
esac
_n2_cc="" _n2_oc=""
for _n2_a in ${N2_AGENTS:-claude opencode}; do
  case "$_n2_a" in
    claude) _n2_cc=1 ;;
    opencode) _n2_oc=1 ;;
    *) echo "premises: N2_AGENTS names an unknown agent '$_n2_a' (known: claude, opencode)"; exit 2 ;;
  esac
done
[ -n "$_n2_cc$_n2_oc" ] || { echo "premises: N2_AGENTS names no agent (known: claude, opencode)"; exit 2; }
N2_AGENTS="${_n2_cc:+claude}${_n2_cc:+${_n2_oc:+ }}${_n2_oc:+opencode}"
CLAUDE_BIN="" OPENCODE_BIN=""
[ -z "$_n2_cc" ] || CLAUDE_BIN=$(command -v claude) || { echo "premises: claude is not on PATH"; exit 1; }
[ -z "$_n2_oc" ] || OPENCODE_BIN=$(command -v opencode) || { echo "premises: opencode is not on PATH"; exit 1; }
# n2_need <claude|opencode>: exit 2 when the script did not name that agent in N2_AGENTS.
n2_need() {
  case " $N2_AGENTS " in
    *" $1 "*) ;;
    *) echo "premises: this step needs $1, but N2_AGENTS is '$N2_AGENTS'" >&2; exit 2 ;;
  esac
}
```

- [ ] **2.3** `scripts/premises/lib.sh`, the `TOOL_PATH=` line: replace its start `TOOL_PATH="$(dirname "$CLAUDE_BIN"):$(dirname "$OPENCODE_BIN"):$(dirname` with `TOOL_PATH="${CLAUDE_BIN:+$(dirname "$CLAUDE_BIN"):}${OPENCODE_BIN:+$(dirname "$OPENCODE_BIN"):}$(dirname`. Change nothing after it. With both agents the value is the value of today.
- [ ] **2.4** `scripts/premises/lib.sh`: add the line `  n2_need claude` as the first line of the body of `run_claude`, of `run_claude_stream` and of `tmux_claude`.
- [ ] **2.5** `scripts/premises/live.sh`: directly below the line `. "$PREMISES_DIR/ver.sh"   # ver_ge (node PRB-2)` add:

```sh
# live_needs <claude|opencode> (node FIX-4): 0 when the script needs that agent (lib.sh: N2_AGENTS;
# not set means both), else 1.
live_needs() {
  case " ${N2_AGENTS:-claude opencode} " in *" $1 "*) return 0 ;; esac
  return 1
}
```

  Replace the line `LIVE_VER=${LIVE_VER:-$PIN_CC}` with:

```sh
if live_needs claude; then LIVE_VER=${LIVE_VER:-$PIN_CC}; else LIVE_VER=${LIVE_VER:-$PIN_OC}; fi
```

- [ ] **2.6** `scripts/premises/live.sh`: replace the three comment lines above `pinned_agents() {` and the whole function with:

```sh
# pinned_agents: each agent the script needs (live_needs) is at or above its pinned version
# (operator, 2026-11-21: a pin is a minimum). Sets and exports LIVE_CC_VER and LIVE_OC_VER
# (agent_ver of each --version output) and sets LIVE_VER = LIVE_CC_VER, or LIVE_OC_VER when claude
# is not needed. An agent that is not needed is not started and not checked, and its variable is
# empty (node FIX-4).
pinned_agents() {
  LIVE_CC_VER="" LIVE_OC_VER=""
  if live_needs claude; then
    v=$("$CLAUDE_BIN" --version 2>/dev/null || true)
    LIVE_CC_VER=$(agent_ver "$v")
    agent_ok "$LIVE_CC_VER" "$PIN_CC" || {
      echo "live: claude is $(agent_show "$v"); the minimum is $PIN_CC (docs/premises.md); put a version at or above it first on PATH"; exit 1; }
  fi
  if live_needs opencode; then
    v=$("$OPENCODE_BIN" --version 2>/dev/null || true)
    LIVE_OC_VER=$(agent_ver "$v")
    agent_ok "$LIVE_OC_VER" "$PIN_OC" || {
      echo "live: opencode is $(agent_show "$v"); the minimum is $PIN_OC (docs/premises.md); put a version at or above it first on PATH"; exit 1; }
  fi
  LIVE_VER=${LIVE_CC_VER:-$LIVE_OC_VER}
  export LIVE_CC_VER LIVE_OC_VER
}
```

- [ ] **2.7** `scripts/premises/live.sh`: replace the comment line above `result() {` and the whole function with:

```sh
# result <id> pass|fail|not-crossed <text>: one LIVE line, also appended to $N2_SCRATCH/results.txt.
# A run that needs both agents prints "(agent <version>)"; a run that needs one agent prints
# "(agent <version>; needs only <agent>)" (node FIX-4), so its line is not read as a full run.
result() {
  id=$1 r=$2; shift 2
  _lr_only=""
  live_needs claude || _lr_only="; needs only opencode"
  live_needs opencode || _lr_only="; needs only claude"
  echo "LIVE $id $r (agent ${LIVE_VER:-?}$_lr_only): $*" | tee -a "$N2_SCRATCH/results.txt"
}
```

- [ ] **2.8** Run `sh scripts/premises/lib_test.sh`. Expected: exit 0, last line `lib tests: ok (38)`.
- [ ] **2.9** Run `sh scripts/premises/ver_test.sh`. Expected: exit 0, last line `ver tests: ok (53)`.
- [ ] **2.10** Commit `scripts/premises/lib.sh`, `scripts/premises/lib_test.sh`, `scripts/premises/live.sh`, `scripts/premises/ver_test.sh` and this file: `fix(premises): a script states the agents it needs (N2_AGENTS); lib.sh checks only those`.

### Task 3: the docs

- [ ] **3.1** `docs/premises.md`, the `- Scratch:` item of `## Method`: replace `checks that `claude` and `opencode` are on `PATH`, then makes a scratch folder` with `checks that each agent the script needs is on `PATH`, then makes a scratch folder`. Directly below that item add this item, as one line:

```markdown
- Agents a script needs (node FIX-4): a script needs `claude` and `opencode`, unless it sets `N2_AGENTS` (a space-separated list of `claude` and `opencode`) as a plain shell variable before it sources `lib.sh`. `lib.sh` looks up only a named agent on `PATH`, and `pinned_agents` (`live.sh`) checks the minimum version of a named agent only. A helper that starts an agent the script did not name exits 2. An unknown name, a blank value, or an `N2_AGENTS` that comes from the environment exits 2. Each `run-<id>.sh` script and each `live-*.sh` script of node N38 names no agent, so each needs both.
```

- [ ] **3.2** `docs/live-checks.md`, the `- Setup, once per shell:` item: replace its last sentence ``Each script stops unless `claude --version` and `opencode --version` are at or above the minimum versions above.`` with:

```markdown
Each script stops unless each agent it needs is on `PATH` and its `--version` output is at or above the minimum version above. A script needs `claude` and `opencode`, unless it sets `N2_AGENTS` before it sources `lib.sh` (node FIX-4; `docs/premises.md`, Method). The scripts of node N38 set nothing, so each needs both.
```

- [ ] **3.3** `docs/live-checks.md`, the `- Cleanup:` item: after its last sentence (it ends with ``(`sh scripts/premises/lib_test.sh`).``) add, on the same line, one space and this sentence:

```markdown
The node FIX-4 cases (`N2_AGENTS`: lib_test A1-A11, ver_test P16-P18) also ran under bash only.
```

  If dash is on `PATH` when you do this step, run both test files with dash too and write what ran.
- [ ] **3.4** `docs/live-checks.md`, `## Owner-run checks`: replace ``(each prints `LIVE <id> <result> (agent <version>): …` lines to paste into the table, and writes its fixtures)`` with:

```markdown
(each prints `LIVE <id> <result> (agent <version>): …` lines to paste into the table, and writes its fixtures; a script that needs only one agent prints `(agent <version>; needs only <agent>)` in that place, so its line is not read as a run with both agents)
```

- [ ] **3.5** Run `sh scripts/check-premises.sh`. Expected: exit 0, last line `check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7)`.
- [ ] **3.6** Commit `docs/premises.md`, `docs/live-checks.md` and this file: `docs(premises): the agents a script needs (N2_AGENTS) in the method text`.

### Task 4: verification

- [ ] **4.1** Acceptance. Run `sh scripts/premises/ver_test.sh && sh scripts/premises/lib_test.sh && sh scripts/check-premises.sh && sh scripts/premises/live-selftest.sh`. Expected: exit 0 with `ver tests: ok (53)`, `lib tests: ok (38)`, `check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7)` and `live selftest: ok (54)`. The self-test prints `Terminated` and `Hangup` job lines of the shell during its T1 cases; they are not failures.
- [ ] **4.2** The defect itself, on this machine (Claude Code on `PATH`, no `opencode`). This starts only `claude --version`. Run `sh -c 'N2_AGENTS="claude"; PREMISES_DIR=$PWD/scripts/premises; . scripts/premises/lib.sh; . scripts/premises/live.sh; live_traps; pinned_agents; result ZZ pass "claude only"'`. Expected: exit 0; stderr has one `premises: scratch …` line; stdout is `LIVE ZZ pass (agent <the claude version on PATH>; needs only claude): claude only`. Then run the same command without `N2_AGENTS="claude"; `. Expected: exit 1 with `premises: opencode is not on PATH`. On a machine that has `opencode`, write in the commit message of step 4.3 that the second run could not show the stop.
- [ ] **4.3** Commit this file with all steps ticked: `docs(work): FIX-4 plan steps done`.

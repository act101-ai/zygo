# PRB-2 — Premise docs: pins are minimums; live scratch dir removed on exit

**Spec:** Operator decision 2026-11-21 (graph Log): a pinned agent version is a minimum, not an exact version. Controller decision (graph Log 2026-11-21): PRB-2 owns the whole "pin is a minimum" change, code and docs, so that they agree when it merges. PRB-1 depends on PRB-2 and fixes the `LIVE_VER` sites in the `live-*.sh` scripts. Defects:
- `docs/premises.md:3-6`, `docs/live-checks.md:5-6,11,56,61` and `scripts/premises/run-all.sh:3` describe the Claude Code and OpenCode versions as exact pins.
- `scripts/premises/live.sh:49-58` (`pinned_agents`) accepts only the exact pin.
- `scripts/check-premises.sh:73` requires each row version to equal the header version.
- `scripts/premises/lib.sh:7` and `scripts/premises/run-all.sh:7` create `N2_SCRATCH` with `mktemp -d`, and nothing removes it. `live_end` (`live.sh:36`) removes only `live-$$`.

**Files:** `docs/premises.md`, `docs/live-checks.md`, `scripts/premises/ver.sh`, `scripts/premises/ver_test.sh`, `scripts/premises/lib.sh`, `scripts/premises/lib_test.sh`, `scripts/premises/live.sh`, `scripts/premises/run-all.sh`, `scripts/check-premises.sh`
**Consumes:** —
**Produces:**
- `ver_ge <have> <min>` (`scripts/premises/ver.sh`).
- `agent_ver`, `agent_ok`, `agent_show`, and `pinned_agents` that sets and exports `LIVE_CC_VER` and `LIVE_OC_VER` (`scripts/premises/live.sh`).
- `n2_scratch_end <status>` and `N2_SCRATCH_OWN` (never exported) (`scripts/premises/lib.sh`).

**Acceptance:** `sh scripts/premises/ver_test.sh && sh scripts/premises/lib_test.sh && sh scripts/check-premises.sh && sh scripts/premises/live-selftest.sh` → exit 0 with `ver tests: ok (50)`, `lib tests: ok (27)`, `check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7)` and `live selftest: ok (54)` (both last strings read on 2026-11-21 before any change).
- `ver_test.sh` shows the following. `ver_ge` compares dotted numbers of any length, drops leading zeros, and changes no variable of the caller. `pinned_agents` accepts any version at or above the pin. It removes CR, leading and trailing blanks, and a build suffix, the same way for both agents. A prerelease passes only when its version is above the pin. It rejects multi-line or non-numeric output, and its error message shows only the first line of the output, without control characters. It exports the versions it found. `check-premises.sh` accepts an agent row version at or above its header, rejects a lower or non-numeric one, and keeps the exact match for act-lite rows.
- `lib_test.sh` shows the following. A dir that `lib.sh` made is removed after exit 0, also under `live_traps` and under `run-all.sh`. After an error, TERM or HUP, that dir is kept, and a `premises: kept <dir> (exit <n>)` line is printed. A failed removal is named on stderr and the exit status is kept. A second source of `lib.sh` keeps the first ownership. A caller-set `N2_SCRATCH` is kept, also for a child of `run-all.sh`, and also when the environment holds a matching `N2_SCRATCH_OWN`. `N2_SCRATCH_OWN` is never exported. `results.txt` is not truncated.
- `docs/premises.md` and `docs/live-checks.md` state each agent version as a minimum ("2.1.328 or later", "1.18.16 or later").

## Plan

All paths are relative to the node's checkout (`.worktrees/phase2/PRB-2`). The tests are POSIX sh. dash is not installed on this machine, and `/bin/sh` is bash, so the new tests run only under bash here. This plan does not change the "tested under `dash`" sentence in `docs/live-checks.md`, because that sentence is about `live-selftest.sh`. This plan adds no CI step. A step cites lines by their quoted text, so the citations stay correct after an earlier edit.

**Design.**
- **`ver.sh`** defines only `ver_ge`. Sourcing it has no side effects and does no PATH check. Both arguments must be dotted numbers (digits and single dots: not empty, no leading, trailing or double dot), otherwise `ver_ge` returns 1. It splits on dots and drops leading zeros. A missing or all-zero field counts as empty, which means 0. It compares each field first by length, then digit by digit, in pure POSIX sh, so there is no limit on the size of a number. All its variables start with `_vg_`. `live.sh` and `check-premises.sh` both source it.
- **`agent_ver` (author's call, 2026-11-21).**
  - It removes CR.
  - If the output then has more than one line, it returns nothing.
  - Otherwise it skips leading blanks, takes the first word, and cuts a build suffix (from the first `+`). A prerelease suffix (from the first `-`) stays.
  - `agent_ok <word> <min>`: a release passes when `ver_ge <word> <min>`. A prerelease `<core>-<pre>` passes only when `<core>` is above the minimum (`ver_ge core min` and not `ver_ge min core`), because in SemVer a prerelease is lower than its release. So `1.19.0-beta` passes against `1.18.16`, `1.18.16-beta` fails, and `2.1.328+build.5` passes against `2.1.328`. `pinned_agents` uses `agent_ok` for both agents.
  - `agent_show` formats the output for an error message: the first line only, with CR and every other control character removed.
- **`check-premises.sh`.** The at-or-above rule applies only to the Claude Code and OpenCode rows. The act-lite rows keep the exact match.
- **Scratch dir.**
  - `lib.sh` owns a dir only when this shell's `lib.sh` made it.
  - On the first source in a process, `lib.sh` runs `unset N2_SCRATCH_OWN` (this also drops an export attribute inherited from the environment), then sets `N2_SCRATCH_OWN=""`. The guard variable `_n2_lib_pid`, never exported, holds `$$` and marks a source as a re-source. A value inherited from another process differs from this process's `$$`. A subshell has the same `$$`, and it also has a copy of the same state.
  - When `N2_SCRATCH` is unset or empty, `lib.sh` first sets traps on EXIT, INT, TERM and HUP that call `n2_scratch_end`. Then `N2_SCRATCH_OWN=$(mktemp -d) || exit 1` makes the dir and assigns ownership in one statement, then `N2_SCRATCH=$N2_SCRATCH_OWN`, and the path is printed to stderr once. Accepted window: a signal that arrives after `mktemp` makes the dir but before the command substitution returns leaves that dir unowned (not removed).
  - `n2_scratch_end <status>` removes the dir only when the status is 0: it runs `chmod -R u+rwx`, then `rm -rf`. If the removal fails, it prints `premises: could not remove <dir>` and keeps the status.
  - For a non-zero status, including a signal, `n2_scratch_end` keeps the dir and prints `premises: kept <dir> (exit <n>)`.
  - A second source of `lib.sh` in the same shell finds `_n2_lib_pid` = `$$` and `N2_SCRATCH` set, so it changes nothing.
  - A caller-set `N2_SCRATCH` gets no trap and is kept.
  - `results.txt` is never truncated. It collects the `LIVE` lines of every script that runs with that dir, as `docs/live-checks.md` says.
- **With `live.sh`.** `live_traps` replaces the `lib.sh` traps. `live_end` ends with `n2_scratch_end "$1"` instead of `exit "$1"`. So a live script always removes `live-$$` (as it does today), and then removes or keeps the dir that `lib.sh` made, according to its status.
- **`run-all.sh`.** It sources `lib.sh` in place of its own `mktemp` line, then exports `N2_SCRATCH`, so each `run-<id>.sh` child gets a caller-set dir and keeps it. The signal is not forwarded to the child. Running children in the background would give them `/dev/null` as stdin and SIGINT ignored, which `run-P1-interactive.sh` must not get. The header documents the delay instead, and case R5 tests the process-group method.
- **Docs.** The bare header lines `Claude Code: 2.1.328` and `OpenCode: 1.18.16` in `docs/premises.md` stay unchanged. Four readers parse them as a bare version: `live.sh` (`PIN_CC=`, `PIN_OC=`), `check-premises.sh` (`V_CC=`, `V_OC=`), `check-conformance.sh` (`header`), and `crates/zygo-config/tests/builtin.rs` (`minimum_agent_versions_match_premises`). The "or later" wording goes in the prose. Nothing reads the header lines in `docs/live-checks.md`, so they take the wording directly.

**Test conventions (both test files).** POSIX sh, `set -u`, in the style of `live-selftest.sh`:
- Output: one `ok: <case> <name>` or `FAIL: <case> <name> (<why>)` line per case. The last line is `<name> tests: ok (<n>)` with exit 0, or `<name> tests: <k> failed` with exit 1.
- Header comment: node PRB-2, the usage line, and "no agent, no network; not run by CI".
- Setup: `unset N2_SCRATCH N2_SCRATCH_OWN`; `HERE` = the script's dir; `REPO` = `$HERE/../..`; `SH=$(command -v dash || command -v sh)`; `T=$(mktemp -d)`; an EXIT trap runs `chmod -R u+rwx "$T"; rm -rf "$T"`; `trap 'exit 130' INT`; `trap 'exit 143' TERM`.
- Bounded wait: a background case polls for its `.ready` file 50 times at 0.1 s. If the file never appears, the case prints `FAIL: <case> … (not ready in 5 s)`, sends KILL to the child (to its process group for R5), and the test goes on to the next case.

Steps:

- [x] 1. Write `scripts/premises/ver_test.sh` (usage `sh scripts/premises/ver_test.sh`; last line `ver tests: ok (45)`).
  - Setup: `PREMISES_DIR=$HERE`, `REPO_DIR=$REPO`, `N2_SCRATCH=$T/s`, `CLAUDE_BIN=$T/claude` (`#!/bin/sh` + `cat '<T>/cc.txt'`), `OPENCODE_BIN=$T/opencode` (`cat '<T>/oc.txt'`).
  - A V case or X case FAILs with `ver.sh missing` when `$HERE/ver.sh` does not exist. Otherwise it runs `( . "$HERE/ver.sh"; ver_ge <a> <b> )` and checks the exit status exactly: 0 or 1, never another value.
  - V1-V11: `ver_ge` returns 0 for (2.1.330, 2.1.328), (2.1.328, 2.1.328), (2.2, 2.1.328), (2.1.0, 2.1), (2.1, 2.1.0), (2.1.1000, 2.1.328), (10.0.0, 9.9.9), (1.18.16, 1.18.16), (2.01, 2.1), (2.1, 2.01), (9007199254740993, 9007199254740992).
  - V12-V25: `ver_ge` returns 1 for (2.1.327, 2.1.328), (1.15.7, 1.18.16), (1.9.0, 1.18.16), ("", 2.1.328), ("2.1.330 (Claude Code)", 2.1.328), (v2.1.330, 2.1.328), (2..1, 2.1), (2.1.330, ""), ("2.1.330<newline>2.1.331", 2.1.328), (2.1.99999999999999999, 2.1.100000000000000000), (9007199254740992, 9007199254740993), (.1, 1), (1., 1), (1..2, 1).
  - X1: `( . "$HERE/ver.sh"; a=keep b=keep c=keep d=keep i=keep m=keep n=keep p=keep q=keep v=keep x=keep; ver_ge 2.1.330 2.1.328; ver_ge 2.01.9 2.1.10 || true; echo "$a $b $c $d $i $m $n $p $q $v $x" )` → output: `keep` 11 times, separated by spaces. These are the short names that a naive version would use. `ver_ge` may set only its own `_vg_*` names.
  - X2: `env PATH= "$SH" -c ". '$HERE/ver.sh'; command -v ver_ge >/dev/null && echo ok"` → exit 0, output exactly `ok` (no PATH check, no other output).
  - P cases: before each case, `cc.txt` = `2.1.330 (Claude Code)` and `oc.txt` = `1.18.16`, unless the case sets them with `printf`. Each P case runs `( . "$HERE/live.sh"; pinned_agents; echo "$LIVE_VER $LIVE_CC_VER $LIVE_OC_VER"; sh -c 'echo "$LIVE_CC_VER $LIVE_OC_VER"' )` with `.out` and the exit status.
  - P1: defaults → exit 0, output exactly the two lines `2.1.330 2.1.330 1.18.16` and `2.1.330 1.18.16` (the versions are exported).
  - P2: cc `2.1.328 (Claude Code)`, oc `1.18.20 (OpenCode)` → exit 0, first line `2.1.328 2.1.328 1.18.20`.
  - P3: cc `2.1.327 (Claude Code)` → exit 1, output has `live: claude is 2.1.327 (Claude Code); the minimum is 2.1.328`.
  - P4: oc `1.15.7` → exit 1, output has `live: opencode is 1.15.7; the minimum is 1.18.16`.
  - P5: cc empty → exit 1, output has `the minimum is 2.1.328`.
  - P6: cc = the two lines `2.1.328 (Claude Code)` and `extra` → exit 1, output has `live: claude is 2.1.328 (Claude Code); the minimum is 2.1.328`, and `.out` does not contain `extra`.
  - P7: oc `unknown` → exit 1, output has `the minimum is 1.18.16`.
  - P8: cc `printf '\t 2.1.330 (Claude Code) \r\n'`, oc `printf '1.18.16\r\n'` → exit 0, first line `2.1.330 2.1.330 1.18.16`.
  - P9: oc `1.19.0-beta` → exit 0, first line `2.1.330 2.1.330 1.19.0-beta`.
  - P10: oc `1.18.16-beta` → exit 1, output has `live: opencode is 1.18.16-beta; the minimum is 1.18.16` (a prerelease of the minimum is lower).
  - P11: cc `printf '2.1.330\r\nextra\n'` → exit 1, output has `live: claude is 2.1.330; the minimum is 2.1.328`.
  - P12: cc `2.1.328+build.5 (Claude Code)` → exit 0, first line `2.1.328 2.1.328 1.18.16` (build suffix ignored).
  - P13: cc `printf '2.1.0\033[31mX\r\n'` → exit 1; output has `live: claude is 2.1.0[31mX; the minimum is 2.1.328`; `.out` contains no ESC byte and no CR.
  - K cases: build a tree at `$T/k`: `scripts/check-premises.sh`, `scripts/premises/ver.sh`, `docs/specs/2026-11-18-zygo-design.md`, `docs/conformance.md`, and `fixtures/` (`cp -R`), all copied from `$REPO`. If a source file is missing, each K case FAILs with `<file> missing`. For each case, write `$T/k/docs/premises.md` = `sed "s/^\(| <id> | [^|]* | <agent> | \)<old> |/\1<V> |/" "$REPO/docs/premises.md"`. FAIL the case if `grep -c "^| <id> | .* | <agent> | <V> |"` is not 1. Then run `sh "$T/k/scripts/check-premises.sh"`. The `<old>` values are `2\.1\.328`, `1\.18\.16` and `1\.2\.3`.
  - K1: P1, Claude Code, `2.1.330` → exit 0, output has `check-premises: ok`.
  - K2: P1, Claude Code, `2.1.327` → exit 1, output has `P1: version '2.1.327' is below the header's '2.1.328' for Claude Code`.
  - K3: P1, Claude Code, `2.1.33a` → exit 1, output has `P1: version '2.1.33a' is below the header's '2.1.328'`.
  - K4: P4, OpenCode, `1.18.20` → exit 0, output has `check-premises: ok`.
  - K5: P6b, act-lite, `1.2.4` → exit 1, output has `P6b: version '1.2.4' differs from the header's '1.2.3' for act-lite`.
  Check: `sh -n scripts/premises/ver_test.sh` → exit 0.
- [x] 2. Write `scripts/premises/lib_test.sh` (usage `sh scripts/premises/lib_test.sh`; last line `lib tests: ok (21)`).
  - Setup: `$T/bin` holds stub `claude` and `opencode` (`#!/bin/sh` + `echo 0`), symlinks to `command -v` of `mktemp rm mkdir chmod dirname sleep cat cp sed head`, and `sh` → `$SH`. `$T/nocc` is the same without `claude`.
  - "Own" case: `TMPDIR=$T/tmp-<case>` (created first), with `N2_SCRATCH` unset. `<dir>` is the path in `<case>.dir`, and it must be under `$T/tmp-<case>/`. The first line of `<case>.err` must be `premises: scratch <dir> (removed on exit 0, kept otherwise; set N2_SCRATCH to keep it)`.
    - "Removed" means: `<dir>` does not exist, `ls -A "$T/tmp-<case>"` is empty, and `.err` has only that one line.
    - "Kept with n" means: `<dir>` exists, and the last line of `.err` is `premises: kept <dir> (exit <n>)`.
  - "Caller" case: `N2_SCRATCH=$T/keep-<case>`, created first with a file `mine`. "Kept" means `mine` still exists and `.err` is empty.
  - Helper `child <case> <live: 0|1> <action>` writes `$T/<case>.sh` with these lines: `PREMISES_DIR='<HERE>'`; `. '<HERE>/lib.sh'`; if live is 1, `. '<HERE>/live.sh'` and `live_traps`; `echo "$N2_SCRATCH" >'<T>/<case>.dir'`; `<action>`. It runs as `env PATH="$T/bin" TMPDIR=… [N2_SCRATCH=…] "$SH" "$T/<case>.sh" >"$T/<case>.out" 2>"$T/<case>.err"`, and the exit status is kept.
  - A signal case uses the action `echo $$ >'<T>/<case>.pid'; sleep 30 & echo $! >'<T>/<case>.sleep'; : >'<T>/<case>.ready'; wait`. It runs the child in the background, waits for `.ready` (bounded), sends the signal to the child's PID, runs `wait` to get the exit status, then kills the saved sleep PID.
  - Cases L1-L11 (live 0):
    - L1: own, `exit 0` → exit 0, removed.
    - L2: own, `false` → exit 1, kept with 1.
    - L3: own, `PATH=$T/nocc` → exit 1; `.out` has `premises: claude is not on PATH`; `$T/tmp-L3` holds exactly one entry `<e>`; `.err` ends with `premises: kept $T/tmp-L3/<e> (exit 1)`.
    - L4: own, TERM → exit 143, kept with 143.
    - L5: own, HUP → exit 129, kept with 129.
    - L6: caller, `exit 0` → exit 0, kept.
    - L7: caller, TERM → exit 143, kept.
    - L8: own, `mkdir "$N2_SCRATCH/ro"; : >"$N2_SCRATCH/ro/f"; chmod 500 "$N2_SCRATCH/ro"; exit 0` → exit 0, removed.
    - L9: own, `chmod 500 "$TMPDIR"; exit 0` → exit 0; the last line of `.err` is `premises: could not remove <dir>`; `<dir>` exists. Afterwards the test runs `chmod 700 "$T/tmp-L9"`. When `id -u` is 0, rm cannot fail, so print `ok: L9 skipped (root)`.
    - L10: own, `. '<HERE>/lib.sh'; [ "$N2_SCRATCH_OWN" = "$N2_SCRATCH" ] && [ "$N2_SCRATCH" = "$(cat '<T>/L10.dir')" ] && echo same >'<T>/L10.same'; exit 0` → exit 0; `L10.same` is `same`; removed (so `.err` has only one `premises: scratch` line).
    - L11: caller, `results.txt` first = `LIVE OLD`, action `echo 'LIVE NEW' >>"$N2_SCRATCH/results.txt"` → exit 0; `results.txt` is exactly the two lines `LIVE OLD` and `LIVE NEW`.
  - Cases E1-E2 (live 0; ownership is never inherited and never exported):
    - E1: `env N2_SCRATCH=$T/keep-E1 N2_SCRATCH_OWN=$T/keep-E1` (dir created first with `mine`), action `sh -c 'echo "${N2_SCRATCH_OWN:-none}"'; exit 0` → exit 0; `.out` is exactly `none`; kept.
    - E2: own, action `sh -c 'echo "${N2_SCRATCH_OWN:-none}"'; exit 0` → exit 0; `.out` is exactly `none`; removed.
  - Cases C1-C3 (live 1, composition with `live_traps` and `live_end`):
    - C1: own, `mkdir -p "$N2_SCRATCH/live-$$"; exit 0` → exit 0, removed.
    - C2: own, TERM → exit 143, kept with 143, and `<dir>/live-<pid from C2.pid>` does not exist.
    - C3: caller, `mkdir -p "$N2_SCRATCH/live-$$"; echo $$ >'<T>/C3.pid'; exit 0` → exit 0, kept, and `$T/keep-C3/live-<that pid>` does not exist.
  - Helper `rall <case>`: `mkdir $T/<case>-pd`; `cp "$HERE/run-all.sh" "$HERE/lib.sh" "$T/<case>-pd/"`. For each id in `smoke P1 P1-interactive P2 P3 Q1 Q4 Q6 Q7-claude Q5 opencode P6` (the `for s in` list of `run-all.sh`), write `$T/<case>-pd/run-<id>.sh` = `echo "$N2_SCRATCH" >>'<T>/<case>.seen'`; a case can replace one stub. It runs `env PATH="$T/bin" TMPDIR=… [N2_SCRATCH=…] "$SH" "$T/<case>-pd/run-all.sh"` with `.out`, `.err` and the exit status. `<dir>` here is the first line of `.seen`. Cases R1-R5:
    - R1: own → exit 0; `.seen` has 12 lines, all `<dir>`; removed.
    - R2: own, `run-P2.sh` also does `exit 3` → exit 3; `.seen` has 4 lines; kept with 3.
    - R3: caller → exit 0; `.seen` has 12 lines, each `$T/keep-R3`; kept.
    - R4: own. `run-smoke.sh` is a real child: `. "$(dirname "$0")/lib.sh"; echo "$N2_SCRATCH" >>'<T>/R4.seen'; : >"$N2_SCRATCH/child-mark"`. `run-P1.sh` also does `[ -f "$N2_SCRATCH/child-mark" ] && echo kept >'<T>/R4.kept'`. Expected: exit 0; `R4.kept` is `kept`; `.seen` has 12 lines, all `<dir>`; removed.
    - R5: own. `run-smoke.sh` = `: >'<T>/R5.ready'; sleep 30`. Start `perl -e 'setpgrp(0, 0); exec @ARGV or die' env PATH=… TMPDIR=… "$SH" "$T/R5-pd/run-all.sh" &`, wait for `.ready` (bounded), send `kill -s TERM -- -<pid>`, then `wait` → exit 143; `<dir>` = the only entry in `$T/tmp-R5`; kept with 143.
  Check: `sh -n scripts/premises/lib_test.sh` → exit 0.
- [x] 3. Run `sh scripts/premises/ver_test.sh; echo "rc=$?"` and `sh scripts/premises/lib_test.sh; echo "rc=$?"`. Check:
  - `ver_test.sh`: every case prints `FAIL:`. V and X fail because `ver.sh` is missing; P fail because of the old exact rule and the old messages; K fail because `scripts/premises/ver.sh` is missing from the tree. Last lines `ver tests: 45 failed` and `rc=1`.
  - `lib_test.sh`: `ok:` only for L6, L7, L11, C3 and R3. E1 fails because the inherited `N2_SCRATCH_OWN` reaches the child, and E2 fails because the dir is left. Last lines `lib tests: 16 failed` and `rc=1`.
- [x] 4. Write `scripts/premises/ver.sh`:
  ```sh
  # ver_ge <have> <min> (node PRB-2): 0 when both are dotted numbers (digits and single dots) and
  # have >= min, else 1. Fields compare as integers of any length (leading zeros dropped, a missing
  # field is 0). Sourcing this file has no side effect; ver_ge sets only _vg_* variables.
  ver_ge() {
    for _vg_x in "$1" "$2"; do
      case "$_vg_x" in '' | .* | *. | *..* | *[!0-9.]*) return 1 ;; esac
    done
    _vg_a=$1. _vg_b=$2.
    while [ -n "$_vg_a$_vg_b" ]; do
      _vg_p=${_vg_a%%.*} _vg_q=${_vg_b%%.*}
      _vg_a=${_vg_a#*.} _vg_b=${_vg_b#*.}
      while [ "${_vg_p#0}" != "$_vg_p" ]; do _vg_p=${_vg_p#0}; done
      while [ "${_vg_q#0}" != "$_vg_q" ]; do _vg_q=${_vg_q#0}; done
      [ ${#_vg_p} -gt ${#_vg_q} ] && return 0
      [ ${#_vg_p} -lt ${#_vg_q} ] && return 1
      while [ -n "$_vg_p" ]; do
        _vg_c=${_vg_p%"${_vg_p#?}"} _vg_d=${_vg_q%"${_vg_q#?}"}
        [ "$_vg_c" -gt "$_vg_d" ] && return 0
        [ "$_vg_c" -lt "$_vg_d" ] && return 1
        _vg_p=${_vg_p#?} _vg_q=${_vg_q#?}
      done
    done
    return 0
  }
  ```
  Check: `sh -n scripts/premises/ver.sh` → exit 0.
- [x] 5. In `scripts/premises/lib.sh`, replace the comment line `# N2_SCRATCH: scratch directory for isolated HOMEs and raw captures (default: mktemp -d).` with these three lines:
  `# N2_SCRATCH: scratch directory for isolated HOMEs and raw captures. Set by the caller: kept. Unset`
  `# or empty: lib.sh makes one (mktemp -d) and prints its path; on exit it removes it after status 0`
  `# and keeps it (named on stderr) otherwise. live.sh's live_end does the same.`
  Replace the line `: "${N2_SCRATCH:=$(mktemp -d)}"` with:
  ```sh
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
  ```
  No other line changes. Check: `sh -n scripts/premises/lib.sh` → exit 0.
- [x] 6. In `scripts/premises/live.sh`:
  - (a) Directly before the line `: "${ZYGO_BIN:=$REPO_DIR/target/release/zygo}"`, add `. "$PREMISES_DIR/ver.sh"   # ver_ge (node PRB-2)`.
  - (b) In the `live_end` comment, change `delete its scratch tree, exit.` to `delete its scratch tree, then n2_scratch_end (lib.sh: a scratch dir lib.sh made is removed after status 0, else kept).`
  - (c) In `live_end`, change the line `  exit "$1"` to `  n2_scratch_end "$1"`.
  - (d) Replace everything from the comment `# pinned_agents: the claude and opencode on PATH are the pinned versions.` to the closing `}` of `pinned_agents` with:
  ```sh
  # agent_ver <--version output>: CR removed; nothing when more than one line is left; else the first
  # word (leading blanks skipped) with a build suffix (from the first +) cut; a -prerelease stays.
  agent_ver() {
    _av_t=$(printf '%s\n' "$1" | tr -d '\r')
    [ "$(printf '%s\n' "$_av_t" | wc -l | tr -d ' ')" = 1 ] || return 0
    printf '%s\n' "$_av_t" | sed 's/^[[:blank:]]*//; s/[[:blank:]].*$//; s/+.*$//'
  }
  # agent_ok <version word> <min> (author's call, 2026-11-21, SemVer order): a release passes at or
  # above min; a prerelease <core>-<pre> passes only when <core> is above min.
  agent_ok() {
    case "$1" in
      *-*) ver_ge "${1%%-*}" "$2" && ! ver_ge "$2" "${1%%-*}" ;;
      *) ver_ge "$1" "$2" ;;
    esac
  }
  # agent_show <--version output>: its first line with CR and other control characters removed.
  agent_show() { printf '%s\n' "$1" | head -n 1 | tr -d '[:cntrl:]'; }
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
  ```
  Check: `sh -n scripts/premises/live.sh` → exit 0.
- [x] 7. In `scripts/check-premises.sh`:
  - Directly after the line `cd "$(dirname "$0")/.." || exit 1`, add `. scripts/premises/ver.sh   # ver_ge (node PRB-2)`.
  - Replace the line `    [ "$v" = "$want" ] || fail "$id: version '$v' differs from the header's '$want' for $a"` with:
  ```sh
      # A pinned agent version is a minimum (operator, 2026-11-21); act-lite stays exact.
      case "$a" in
        'Claude Code' | 'OpenCode') ver_ge "$v" "$want" || fail "$id: version '$v' is below the header's '$want' for $a, or not a dotted number" ;;
        *) [ "$v" = "$want" ] || fail "$id: version '$v' differs from the header's '$want' for $a" ;;
      esac
  ```
  Check: `sh -n scripts/check-premises.sh` → exit 0.
- [x] 8. In `scripts/premises/run-all.sh`:
  - Replace `# PATH (the pinned versions in docs/premises.md), python3, jq, tmux, curl, openssl.` with `# PATH (at or above the pinned versions in docs/premises.md), python3, jq, tmux, curl, openssl.`
  - Replace `# N2_SCRATCH: scratch directory for isolated HOMEs and raw captures (default: mktemp -d).` with these four lines:
    `# N2_SCRATCH: scratch directory for isolated HOMEs and raw captures. Set it to keep the captures;`
    `# unset, lib.sh makes one, prints its path, removes it after exit 0 and keeps it otherwise.`
    `# A TERM or HUP to this script alone takes effect after the current run-<id>.sh ends (a trap runs`
    `# between commands); Ctrl-C, or kill -TERM -<process group id>, stops both at once.`
  - Replace `: "${N2_SCRATCH:=$(mktemp -d)}"` with `. "$D/lib.sh"`.
  - The `set -eu`, `D=`, `export N2_SCRATCH` and `for` lines stay.
  Check: `sh -n scripts/premises/run-all.sh` → exit 0.
- [x] 9. Run `sh scripts/premises/ver_test.sh` and `sh scripts/premises/lib_test.sh`. Check: `ver tests: ok (45)`, exit 0; `lib tests: ok (21)`, exit 0.
- [x] 10. Run `sh scripts/premises/live-selftest.sh`. It sources `lib.sh`, `live.sh` and now `ver.sh`, with a caller-set `N2_SCRATCH`. Its T1 and U5 cases run `live_end`, and T4, T5 and T8 run `check-premises.sh` in a copied tree that includes `scripts/premises/ver.sh`. Check: last line `live selftest: ok (54)`, exit 0.
- [x] 11. Edit `docs/premises.md`:
  - Line 3: replace `Pinned versions, one dated result per premise.` with `Pinned versions, one dated result per premise. A pinned agent version is a minimum (operator, 2026-11-21): Claude Code 2.1.328 or later, OpenCode 1.18.16 or later. The header lines below give each minimum as a bare version (scripts and tests read them). An agent row's version is the minimum or a later version that the check ran on; an act-lite row's version is the release checked.`
  - In `## Method`, after the bullet that starts `- \`scripts/check-premises.sh\` checks this file`, add the bullet `- Scratch: when \`N2_SCRATCH\` is not set, \`lib.sh\` (and \`run-all.sh\` through it) makes a scratch folder and prints its path; it removes the folder after exit 0 and keeps it after an error or a signal, with a \`premises: kept <folder> (exit <n>)\` line. Set \`N2_SCRATCH\` to keep the captures (isolated HOMEs, raw requests) in every case.`
  - The header lines `Claude Code: 2.1.328`, `OpenCode: 1.18.16` and `act-lite: 1.2.3` stay.
  Check: `grep -cF -- '<text>' docs/premises.md` → `1` for each of `Claude Code 2.1.328 or later, OpenCode 1.18.16 or later.`, `an act-lite row's version is the release checked.` and `- Scratch: when`. Also, `grep -nxE 'Claude Code: 2\.1\.328|OpenCode: 1\.18\.16|act-lite: 1\.2\.3' docs/premises.md` → exactly those three lines.
- [x] 12. Edit `docs/live-checks.md`:
  - E1: after `Checks on the real pinned agents, one dated result per check.`, add ` A pinned agent version is a minimum (operator, 2026-11-21).`
  - E2: `Claude Code: 2.1.328` → `Claude Code: 2.1.328 or later`.
  - E3: `OpenCode: 1.18.16` → `OpenCode: 1.18.16 or later`.
  - E4: `then the pinned Claude Code first on \`PATH\`:` → `then Claude Code 2.1.328 or later first on \`PATH\` (the commands below link 2.1.328):`.
  - E5: `Each script stops unless \`claude --version\` and \`opencode --version\` are the pinned versions above.` → `Each script stops unless \`claude --version\` and \`opencode --version\` are at or above the minimum versions above.`
  - E6: `and the pinned Claude Code: \`npm install -g @anthropic-ai/claude-code@2.1.328\`` → `and Claude Code 2.1.328 or later (the pinned version: \`npm install -g @anthropic-ai/claude-code@2.1.328\`)`.
  - E7: `# claude prints 2.1.328 (Claude Code)` → `# claude prints 2.1.328 (Claude Code) or a later version`.
  - The `results.txt` sentence (`When done, remove the keys …`) and the "tested under `dash`" sentence stay.
  Check by exact text:
  - `grep -nxF 'Claude Code: 2.1.328 or later' docs/live-checks.md` → one line, and the same for `OpenCode: 1.18.16 or later`.
  - `grep -cF -- '<new text>' docs/live-checks.md` → `1` for each of: `A pinned agent version is a minimum (operator, 2026-11-21).`, `then Claude Code 2.1.328 or later first on`, `are at or above the minimum versions above.`, `and Claude Code 2.1.328 or later (the pinned version:`, `# claude prints 2.1.328 (Claude Code) or a later version`.
  - `grep -cF -- '<old text>' docs/live-checks.md` → `0` for `then the pinned Claude Code first on` and for `are the pinned versions above.`
- [x] 13. Run the checks for the other readers of `docs/premises.md`. Check: `sh scripts/check-conformance.sh` → `check-conformance: ok`; `cargo test -p zygo-config --test builtin minimum_agent_versions_match_premises` → `test result: ok. 1 passed`.
- [x] 14. Run the Acceptance: `sh scripts/premises/ver_test.sh && sh scripts/premises/lib_test.sh && sh scripts/check-premises.sh && sh scripts/premises/live-selftest.sh; echo "rc=$?"`. Check: the output has `ver tests: ok (45)`, `lib tests: ok (21)`, `check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7)` and `live selftest: ok (54)`, and ends with `rc=0`. The checks of steps 11 and 12 also still pass.
- [x] 15. Commit.
  - If `$ZYGO_SESSION` is empty, stop and ask the operator for the session id.
  - Tick steps 1-15 in `docs/work/phase2/nodes/PRB-2.md`.
  - Run `git add docs/premises.md docs/live-checks.md scripts/premises/ver.sh scripts/premises/ver_test.sh scripts/premises/lib.sh scripts/premises/lib_test.sh scripts/premises/live.sh scripts/premises/run-all.sh scripts/check-premises.sh docs/work/phase2/nodes/PRB-2.md`.
  - Run `git commit -m "premises: agent pins are minimums in docs and checks; lib.sh removes its own scratch dir after exit 0" --trailer "Zygo-Session: $ZYGO_SESSION" --trailer "Zygo-Work: PRB-2"`.
  Check: `git show --stat HEAD` lists exactly those ten files; `git log -1 --format=%B` ends with `Zygo-Session: <value of $ZYGO_SESSION>` and `Zygo-Work: PRB-2`; `git status --short` is empty.

### Round 2

Diff review of `fcab8dc` (coordinator, 2026-11-21). Each code fix has a case that failed first: before the fixes, `ver_test.sh` printed `ver tests: 4 failed` (V27, V28, P14, P15) and `lib_test.sh` printed `lib tests: 5 failed` (L3, L13, C4, C5, C6). The new counts are `ver tests: ok (50)` and `lib tests: ok (27)`; the Acceptance counts are updated to match.

- [x] R2-1 (M1) SIGPIPE: bash runs the EXIT trap with status 0 after an untrapped PIPE, so the own dir was removed. `lib.sh` and `live_traps` trap PIPE with status 141; `n2_scratch_end` and `live_end` clear PIPE too. Cases L13 (lib.sh alone) and C4 (through `live_traps`): a script writes to a pipe whose reader has exited → exit 141, kept, `premises: kept <dir> (exit 141)`.
- [x] R2-2 (M2) `live_end`: `chmod -R u+rwx` on `live-$$`, then `rm -rf … || echo "premises: could not remove …" >&2`, so set -e does not stop the trap and `n2_scratch_end` always runs with the status. Cases C5 (a read-only subdir under `live-$$`, exit 0 → exit 0, all removed) and C6 (caller dir made read-only → exit 0, `premises: could not remove <dir>/live-<pid>`; skipped as root).
- [x] R2-3 (L1) `ver_ge` returns 1 for any field longer than 32 digits, before the loops. Cases V26 (a 32-digit field compares, 0), V27 (33 digits → 1), V28 (a 20000-digit field → 1 within 2 s).
- [x] R2-4 (L2) `agent_show` keeps only printable ASCII (`LC_ALL=C tr -cd ' -~'`), so C1 controls and DEL go. Case P14 (U+009B and DEL in `claude --version` → message `2.1.0XY`, no byte 0x7F-0xFF in the output).
- [x] R2-5 (L3) `agent_ok` rejects an empty prerelease (`<core>-`). Case P15 (`1.19.0-` → exit 1).
- [x] R2-6 (L4) `lib.sh` runs the claude and opencode PATH checks before `mktemp`. L3 now expects no dir under `$T/tmp-L3` and an empty stderr. The `lib.sh` and `run-all.sh` header comments and the `docs/premises.md` Scratch bullet say so.
- [x] R2-7 (L5) Tests: E1 and E2 check `env | grep '^N2_SCRATCH_OWN='` in the child (E1 fails when the `unset` is removed: checked on a mutated copy). E3: an inherited `N2_SCRATCH_OWN` names another dir while `lib.sh` makes its own → only the own dir is removed. L11 writes `results.txt` twice through `result` and checks the old line and both new lines. L12: INT on `lib.sh` alone → exit 130, kept (signal cases start through `perl`, which resets INT to its default; a background command of a non-interactive shell starts with INT ignored). Under bash, L12 also passes without the INT trap, because bash runs the EXIT trap with status 130 after SIGINT.
- [x] R2-8 (L6) `docs/live-checks.md` Cleanup bullet: the traps list PIPE; the older cleanup paths were tested under `dash`; the PRB-2 changes ran under bash only. `docs/premises.md` states that a prerelease of a minimum (1.18.16-beta) and an empty prerelease are rejected.
- [x] R2-9 Run the Acceptance alone, `sh scripts/check-conformance.sh` and `cargo test -p zygo-config --test builtin minimum_agent_versions_match_premises`; commit with the same trailers.

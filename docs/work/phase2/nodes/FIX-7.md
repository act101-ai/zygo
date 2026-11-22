# FIX-7 — live-check framework: no tmux socket left, no operator tmux config, a caller-set scratch keeps live-<pid>, live-hud comment path

**Spec:** `docs/live-checks.md` (P-HUD-1 method, line 94: the trust entry is for the main repository path); `docs/premises.md:17` ("Set `N2_SCRATCH` to keep the captures … in every case"); `scripts/premises/lib.sh:2-5`; found by LIV-1 (D7; the selftest socket) and by P1 (live_end, tmux config); reverses PRB-2's choice "a live script always removes `live-$$`" (`docs/work/phase2/nodes/PRB-2.md:43`, `lib_test.sh` C3) for a caller-set scratch dir only
**Files:** `scripts/premises/live-selftest.sh`, `scripts/premises/live-hud.sh`, `scripts/premises/live.sh`, `scripts/premises/lib.sh`, `scripts/premises/lib_test.sh`, `scripts/premises/selftest_test.sh`
**Consumes:** —
**Produces:** —
**Acceptance:** `for f in live-selftest.sh live-hud.sh live.sh lib.sh lib_test.sh selftest_test.sh; do sh -n "scripts/premises/$f" || exit 1; done && sh scripts/premises/lib_test.sh && sh scripts/premises/selftest_test.sh && sh scripts/premises/live-hud.sh --selfcheck && sh scripts/check-premises.sh` → exit 0, with `lib tests: ok (<n>)` where n is one more than before (C6), `selftest tests: ok (2)` and `live-hud selfcheck: ok (17)`

## Plan

Work in the node's checkout. Run each command from the repository root. Each commit message ends, after one empty line, with the two adjacent lines `Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ` and `Zygo-Work: FIX-7`. In each commit, tick (`- [x]`) the steps of this file that are done. Stage only the files of the contract and this file. No step starts an agent.

Facts of the live code (read 2026-11-22, main at 0be75eb):

- `live-selftest.sh:37` `SOCK="n38selftest$$"`. `cleanup` (`:40-47`) kills the private server (`:44`) and removes `$T`, but not the socket file `${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/$SOCK`. Traps: EXIT → `cleanup` (`:48`); INT, TERM, HUP → `exit 130|143|129` (`:49-51`), which runs the EXIT trap. The T6 shim (`:291`) is `exec "$TMUX_REAL" -L "$SOCK" "$@"`, with no `-f /dev/null`.
- `live-hud.sh:395-402` (`hud_end`) already removes its socket file; its `tmux` function (`:394`) has `-f /dev/null`. `live-hud.sh:15-18` comment: "Claude Code then keeps an entry for that path under "projects" in its .claude.json, and a transcript folder under <config dir>/projects/."
- Server starts without `-f /dev/null`: `lib.sh:146` (`tmux_claude`) and `live.sh:148` (`tmux_zygo`). P1's `live-tmux.sh` and `live-detach.sh` already use `-f /dev/null` and a private socket folder.
- `live_end` (`live.sh:23-43`) removes `$N2_SCRATCH/live-$$` in every case (`:38-41`). `N2_SCRATCH_OWN` is empty exactly when the caller set `N2_SCRATCH` (`lib.sh:29-32`, `:41`). `lib_test.sh` C3 (`:247-253`) asserts the removal for a caller dir; C1 and C2 (own dir) assert it for an own dir and do not change.
- `lib_test.sh` helpers: `prep`, `child`, `run`, `sigact`, `sigrun`, `res`, `kept` (`:33-135`).

Design calls: (a) `cleanup` removes the socket file after `kill-server`, so every exit path that runs the EXIT trap removes it; (b) a caller-set scratch dir keeps `live-<pid>` with everything in it, except its `pg-*` bookkeeping files, and prints nothing (as `n2_scratch_end` does for a caller dir); an own dir is unchanged; (c) every server start of the framework gets `-f /dev/null`; where a wrapper already adds it (`live-hud.sh:394`, the selftest shim), tmux gets `-f /dev/null` two times, which step 3.1 checks first. Limit: `-f` has no effect when the operator's default tmux server already runs; `lib.sh` and `live.sh` keep the default server (out of scope).

### Task 1: the failing tests

- [x] **1.1** Create `scripts/premises/selftest_test.sh`:

```sh
#!/bin/sh
# Test of live-selftest.sh (node FIX-7): its private tmux server leaves no socket file, after a whole
# run and after TERM while the server runs. No agent (the self-test uses stubs).
# Usage: sh scripts/premises/selftest_test.sh   ->  "selftest tests: ok (2)", exit 0
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
SOCKDIR="${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)"
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
n=0 bad=0
ok() { n=$((n + 1)); printf 'ok: %s\n' "$1"; }
no() { n=$((n + 1)); bad=$((bad + 1)); printf 'FAIL: %s\n' "$1"; }
# 1: a whole run. exec keeps the PID, so the socket name is n38selftest<pid>.
sh -c 'echo "$$" >"$1"; exec sh "$2"' sh "$T/pid1" "$HERE/live-selftest.sh" >"$T/run1.out" 2>&1
rc=$?
p=$(cat "$T/pid1")
if [ "$rc" -eq 0 ] && [ ! -e "$SOCKDIR/n38selftest$p" ]; then ok "whole run: exit 0, no socket n38selftest$p"
else no "whole run: exit $rc; socket n38selftest$p exists: $([ -e "$SOCKDIR/n38selftest$p" ] && echo yes || echo no)"; fi
# 2: TERM while the private server runs (part T6).
sh -c 'echo "$$" >"$1"; exec sh "$2"' sh "$T/pid2" "$HERE/live-selftest.sh" >"$T/run2.out" 2>&1 &
bp=$!
i=0; while [ ! -s "$T/pid2" ] && [ $i -lt 50 ]; do i=$((i + 1)); sleep 0.1; done
p=$(cat "$T/pid2")
i=0; while [ ! -e "$SOCKDIR/n38selftest$p" ] && [ $i -lt 1500 ]; do i=$((i + 1)); sleep 0.2; done
if [ ! -e "$SOCKDIR/n38selftest$p" ]; then
  kill "$bp" 2>/dev/null; wait "$bp"; no "TERM: the socket n38selftest$p did not appear in 300 s"
else
  kill -s TERM "$p"; wait "$bp"; rc=$?
  if [ "$rc" -eq 143 ] && [ ! -e "$SOCKDIR/n38selftest$p" ]; then ok "TERM: exit 143, no socket n38selftest$p"
  else no "TERM: exit $rc; socket n38selftest$p exists: $([ -e "$SOCKDIR/n38selftest$p" ] && echo yes || echo no)"; fi
fi
if [ "$bad" -eq 0 ]; then echo "selftest tests: ok ($n)"; exit 0; fi
echo "selftest tests: $bad of $n failed"; exit 1
```

- [x] **1.2** In `lib_test.sh`, replace case C3 (`:247-253`) with these two cases:

```sh
c=C3; prep $c caller; ERR=$T/$c.err
child $c 1 "mkdir -p \"\$N2_SCRATCH/live-\$\$\"; : >\"\$N2_SCRATCH/live-\$\$/capture\"; : >\"\$N2_SCRATCH/live-\$\$/pg-1\"; echo \$\$ >'$T/C3.pid'; exit 0"; run $c
c3() {
  kept C3 || return 1
  L=$T/keep-C3/live-$(cat "$T/C3.pid")
  [ -f "$L/capture" ] || { why="live-<pid>/capture is gone"; return 1; }
  [ ! -e "$L/pg-1" ] || { why="live-<pid>/pg-1 is left"; return 1; }
}
res "C3 live_end keeps live-<pid> in a caller dir, without its pg-* files" 0 c3

c=C6; prep $c caller; ERR=$T/$c.err
child $c 1 "mkdir -p \"\$N2_SCRATCH/live-\$\$\"; : >\"\$N2_SCRATCH/live-\$\$/capture\"; $(sigact C6)"; sigrun $c TERM
c6() {
  kept C6 || return 1
  [ -f "$T/keep-C6/live-$(cat "$T/C6.pid")/capture" ] || { why="live-<pid>/capture is gone"; return 1; }
}
res "C6 live_end keeps live-<pid> in a caller dir after TERM" 143 c6
```

  Correction (round 1, by running): `lib_test.sh` already had a case C6 (`:263-276`, a caller dir whose `live-<pid>` could not be removed, which then printed `premises: could not remove …`). After 2.4 a caller dir never has `live-<pid>` removed, so that case no longer applies. It is now C7, in a dir lib.sh made (`prep C7 own`, `chmod 500 "$N2_SCRATCH"`, exit 0): the last stderr line is `premises: could not remove <dir>/live-<pid>`, and the dir is gone after (n2_scratch_end makes it writable again). It passes before and after 2.4. The count goes from 27 to 28.

- [x] **1.3** Run `sh scripts/premises/selftest_test.sh; echo rc=$?`. Expected: two `FAIL:` lines (the socket file exists after each run), `rc=1`. Remove the two leaked socket files by name: `rm -f "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)"/n38selftest<pid>` for each PID that the FAIL lines name. Result: `FAIL: whole run: exit 0; socket n38selftest3378464 exists: yes`, `FAIL: TERM: exit 143; socket n38selftest3412884 exists: yes`, `rc=1`; both files removed. (Run with the OpenCode directory on PATH.)
- [x] **1.4** Run `sh scripts/premises/lib_test.sh; echo rc=$?`. Expected: `FAIL: C3 …` (`live-<pid>/capture is gone`) and `FAIL: C6 …`, `rc=1`. Result: as expected (`lib tests: 2 failed`, rc=1); the other 26 cases passed.

### Task 2: the fixes

- [x] **2.1** In `live-selftest.sh`, after the line `[ -z "$TMUX_REAL" ] || "$TMUX_REAL" -L "$SOCK" kill-server 2>/dev/null` (`:44`), add `rm -f "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/$SOCK" 2>/dev/null`. Change the shim (`:291`) to `printf '#!/bin/sh\nexec "%s" -L "%s" -f /dev/null "$@"\n' "$TMUX_REAL" "$SOCK" >"$T/tbin/tmux"; chmod +x "$T/tbin/tmux"`. Change the header lines `:29-30` to `# - T6: tmux targets are exact and sessions this script did not start are left alone (a private` / `#   tmux server with no config file, through a tmux shim on PATH; only that server is killed, and its socket file is removed).`
  Correction (round 1, by running): after 2.4, the whole self-test exited 1: `FAIL: T1 INT|TERM|HUP: scratch tree left`. T1 (`:88-121`) runs with a caller-set `N2_SCRATCH` and asserted the old PRB-2 removal of `live-<pid>`. T1 now asserts the new contract: `live-<pid>/t1` is still there and no `pg-*` file is left (the killed perl wrapper does not unlink its own `pg-*` file, so only live_end removes it). The ok text is `group killed, live-<pid> kept without pg-* files, exit <rc>`, and the header line `:15` says the same.
- [x] **2.2** In `live-hud.sh`, replace `:17-18` with these four lines:
  `#   Claude Code 2.1.330 then keeps the answer as an entry for the main repository of that worktree`
  `#   (<scratch>/live-<pid>/hud/repo) under "projects" in its .claude.json, a transcript folder for the`
  `#   worktree path and a memory folder for the repository path under <config dir>/projects/. The script`
  `#   does not delete them; the operator may.`
- [x] **2.3** In `lib.sh:146`, change `tmux new-session` to `tmux -f /dev/null new-session`. In `live.sh:148`, the same.
- [x] **2.4** In `live.sh`, replace `:37-41` with:

```sh
  # A caller-set scratch dir (lib.sh: N2_SCRATCH_OWN is empty) keeps live-<pid> with its captures; only
  # its pg-* files go. A dir lib.sh made loses live-<pid> here. A failed removal must not stop the trap
  # under set -e: name it, keep the status, go on.
  if [ -z "${N2_SCRATCH_OWN:-}" ]; then
    rm -f "$N2_SCRATCH/live-$$"/pg-* 2>/dev/null || true
  elif [ -e "$N2_SCRATCH/live-$$" ]; then
    chmod -R u+rwx "$N2_SCRATCH/live-$$" 2>/dev/null || true
    rm -rf "$N2_SCRATCH/live-$$" 2>/dev/null || echo "premises: could not remove $N2_SCRATCH/live-$$" >&2
  fi
```

Change the comment at `:16-18` so its third line says: `# process group (TERM to the group first, then the wrapper), delete its scratch tree live-<pid> when lib.sh made the scratch dir (a caller-set dir keeps it, without its pg-* files), then n2_scratch_end (lib.sh: a scratch dir lib.sh made is removed after status 0, else kept).`
  Addition (round 1): `live_done` (`live.sh:237`) printed `scratch removed on exit`, which is not true for a caller-set dir. It now prints `scratch removed on exit unless N2_SCRATCH is set`. No test or fixture reads this text.
- [x] **2.5** In `lib.sh`, replace `:2-5` with these four lines:
  `# N2_SCRATCH: scratch directory for isolated HOMEs and raw captures. Set by the caller: kept, with`
  `# everything in it. Unset or empty: after the claude and opencode PATH checks pass, lib.sh makes one`
  `# (mktemp -d) and prints its path; on exit it removes it after status 0 and keeps it (named on`
  `# stderr) otherwise. live.sh's live_end does the same; it removes live-<pid> only from a dir lib.sh made.`

### Task 3: checks and commit

- [x] **3.1** Run `tmux -L fix7probe$$ -f /dev/null -f /dev/null new-session -d true; echo rc=$?; tmux -L fix7probe$$ kill-server 2>/dev/null; rm -f "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/fix7probe$$"`. Expected: `rc=0` (tmux accepts `-f` two times; this is the path of `live-hud.sh` and of the selftest shim through `tmux_claude` and `tmux_zygo`). If rc is not 0, stop and report the tmux version and the error text. Result: tmux 3.7c, `rc=0`; no socket file is left (`fix7probe3451750`).
- [x] **3.2** Run `sh scripts/premises/selftest_test.sh`. Expected: `selftest tests: ok (2)`. Run `sh scripts/premises/lib_test.sh`. Expected: `lib tests: ok (<n>)`. Result: `selftest tests: ok (2)` (sockets n38selftest3589500 and n38selftest3620747 gone; afterwards `pgrep -f '[l]ive-selftest'` finds no process) and `lib tests: ok (28)`.
- [x] **3.3** Commit the six files of the contract and this file: `fix(premises): no tmux socket or operator tmux config in the live framework; a caller-set scratch keeps live-<pid>`.

### Task 4: verification

- [ ] **4.1** Run the acceptance check of the contract. Expected: exit 0 with the three lines of the contract.
- [ ] **4.2** Commit this file with all steps ticked: `docs(work): FIX-7 plan steps done`.

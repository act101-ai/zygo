# TF-2 — CI runs the premise test scripts

**Spec:** none (CI defect); PC-13 round-2 review G3; Global constraints "No CI test runs a real `claude`, `opencode` or `codex` binary" and "No test … may pass or fail by wall-clock time"
**Files:** `.github/workflows/ci.yml`, `scripts/premises/lib_test.sh` (CI-friendly waits and its "not run by CI" header line only), `scripts/premises/lib.sh` (round 1 F3: the port-file waits; CI run 1: n2_scan's /proc owner check), `scripts/premises/scrub_test.py` (CI run 1: the SCRUB_FIXED case only)
**Consumes:** —
**Produces:** CI job `premises` (round 1 F5; was job `test`) runs the five premise test scripts on `ubuntu-latest` and `macos-latest`
**Acceptance:** each command run alone from the repository root: `test "$(grep -oE 'scripts/(premises/scrub_test\.py|premises/rescrub_test\.py|test-check-premises\.sh|premises/lib_test\.sh|check-premises\.sh)' .github/workflows/ci.yml | sort -u | wc -l)" -eq 5` → exit 0 (ci.yml names each of the five scripts); `python3 -I scripts/premises/scrub_test.py` → exit 0; `python3 -I scripts/premises/rescrub_test.py` → exit 0; `sh scripts/test-check-premises.sh` → exit 0; `sh scripts/premises/lib_test.sh` → exit 0; `N2_FORBIDDEN_NAMES=zzcinameq sh scripts/check-premises.sh` → exit 0; then the CI run of the node's pull request is green on ubuntu, macOS and Windows, with the five new steps run on ubuntu and macOS. No real agent runs in CI (every script uses stubs or none); no network beyond what the scripts already use (127.0.0.1 only).

## Planning notes

- The defect: `.github/workflows/ci.yml:52-59` (job `test`) runs only `cargo test --workspace --locked --no-fail-fast`, `sh scripts/test-install.sh` and `pwsh scripts/test-install.ps1`. No workflow runs `scripts/premises/scrub_test.py`, `scripts/premises/rescrub_test.py`, `scripts/test-check-premises.sh`, `scripts/premises/lib_test.sh` or `scripts/check-premises.sh`. `test-check-premises.sh` broke on phase-4 after PC-12 (the real `Codex: 0.172.0` header, PC-13 planning notes) and nobody saw it until PC-13.
- Runs after PC-13: PC-13 repairs `test-check-premises.sh` and adds `rescrub_test.py` (on `origin/phase-4--PC-13`, not yet on `phase-4`). Before PC-13 merges, the CI step for `test-check-premises.sh` fails and `rescrub_test.py` does not exist.
- CI configuration changes are allowed. This node changes no registry standard text, tier or bar exception (K§6.2.1), and no boundary exception list, so it needs no operator stop.
- What each script needs in CI:
  - `scrub_test.py`: `python3` only (on every runner). It reads the host and user names at run time (`scrub_test.py:21-23`) and calls `scutil` only where it exists (`:183-190`, macOS). No network.
  - `rescrub_test.py` (PC-13): `python3` only; it builds a temporary fixtures tree and pops `N2_FORBIDDEN_NAMES` itself. No network.
  - `test-check-premises.sh`: POSIX `sh`, `awk`, `sed`, `grep`, `cp`, `mktemp`; it runs `check-premises.sh` on temporary copies and sets what each case needs. No network.
  - `check-premises.sh`: POSIX `sh`, `awk`, `grep`, and `shasum` or `sha256sum` (`check-premises.sh:263-266`); `N2_FORBIDDEN_NAMES` is optional (`:6-8`, `:395`). CI sets the fixed word `N2_FORBIDDEN_NAMES=zzcinameq`, not the runner user: the runner user on ubuntu and macOS is `runner`, and `fixtures/claude-code/options-2.1.331.txt:127` holds `--runner-declares`, a whole-word hit for the scan (word class `A-Za-z0-9_`, `:374`), so `$(id -un)` fails on CI. The fixed word runs the scan path; the host-name scan (`:375-394`) runs on the runner's own host names as it does locally.
  - `lib_test.sh`: no tmux, no real agent, no network beyond `127.0.0.1` (its mock and `HTTPS_PROXY` cases). It needs `cc` (it compiles the stub `codex`, `lib_test.sh:31-56`; gcc on ubuntu, clang on macOS), `perl` (`:120`, `:916`), `python3`, `git`, `shasum`, `file`, `ps`, `uname`, and `lsof` where there is no `/proc` (`:60-62`, macOS). `dash` is used when installed (`:14`; ubuntu and macOS both have it, macOS as `/bin/dash`, so lib_test uses dash on both). The root-only cases skip under root (`:214`, `:318`); runners are not root. Its header says "not run by CI" (`:7`).
  - `lib_test.sh` timing: `waitready` gives up after 50 polls 0.1 s apart (`:106-114`) and the case then fails with `NR=1` (`:127-131`, `:918`); three inline loops give up after 100 polls (`:994`, `:1046`, `:1131`). On a slow runner a case can fail by wall-clock time. The same rule as the Global constraint applies: wait on the ready file with no count limit; the CI job timeout is the bound.
- OS matrix (`ci.yml:43`, `ubuntu-latest`, `macos-latest`, `windows-latest`): the five steps run on `ubuntu-latest` and `macos-latest` and are skipped on `windows-latest` (`if: matrix.os != 'windows-latest'`, as `ci.yml:55`). Reasons: the three sh scripts are POSIX `sh` and `lib_test.sh` needs `cc`, `ps`, symbolic links and `/proc` or `lsof`; `check-premises.sh:42` refuses CRLF in `docs/premises.md`, which a Windows checkout with `core.autocrlf` gives; the premise harness that `scrub.py` serves runs on unix only (`scripts/premises/lib.sh`), so the two python tests skip Windows too. Running any of them on Windows is out of scope.
- CI paths (`ci.yml:3-32`): `docs/premises.md`, `docs/live-checks.md`, `docs/conformance.md` and `docs/specs/2026-11-18-zygo-design.md` already run CI. `check-premises.sh` also reads `docs/specs/2026-11-20-workspaces-design.md` (`check-premises.sh:16`), which does not run CI today; add it to both lists.
- Files overlap: no open node (PC-7, PC-13, RS-12, BS-1, CC-0 to CC-16) lists `.github/workflows/ci.yml` in Files; only CF-4 (DONE) did. `main` has no commit on `ci.yml` after `phase-4` (`git log origin/phase-4..origin/main -- .github/workflows/ci.yml` is empty), so CC-0's merge does not touch it. `lib_test.sh` is in no open node's Files. Edges: PC-13 → TF-2 → BS-1.

## Plan

Capability: `mechanical` (CI steps, one wait helper, no new test logic).

### Steps

- [x] 1. Confirm PC-13 is merged into `phase-4` (`scripts/premises/rescrub_test.py` exists; `sh scripts/test-check-premises.sh` exits 0). Run each of the five Acceptance script commands locally. Pass: each exits 0; note each final line.
- [x] 2. In `scripts/premises/lib_test.sh`, make `waitready` (`:106-114`) wait with no count limit, and remove the `$i -lt 100` limit from the three inline loops (`:994`, `:1046`, `:1131`). Keep each case's assertions and expected results. Change the header line 7 "not run by CI" to say CI runs it on ubuntu and macOS. Run `sh scripts/premises/lib_test.sh`. Pass: exit 0, the same case count as step 1.
- [x] 3. Add one CI step per script to job `test` in `.github/workflows/ci.yml`, after "Install script (sh)", each with `if: matrix.os != 'windows-latest'`, in this order (a failed step stops the later ones, so the run is fail-fast and names the failing script):
  - `Premise scrub tests`: `python3 -I scripts/premises/scrub_test.py`;
  - `Premise re-scrub tests`: `python3 -I scripts/premises/rescrub_test.py`;
  - `Premise check tests`: `sh scripts/test-check-premises.sh`;
  - `Premise harness tests`: `sh scripts/premises/lib_test.sh`;
  - `Premise check`: `sh scripts/check-premises.sh` with `env: N2_FORBIDDEN_NAMES: zzcinameq`.
- [x] 4. Add `- 'docs/specs/2026-11-20-workspaces-design.md'` to the `push` and the `pull_request` path lists of `ci.yml`, beside `docs/specs/2026-11-18-zygo-design.md`.
- [x] 5. Run the Acceptance `grep` check. Pass: exit 0 (5 names). Run `git diff origin/phase-4 -- .github/workflows/ci.yml scripts/premises/lib_test.sh`. Pass: only the steps, the path lines, the wait loops and the header line changed.
- [x] 6. Commit `ci: TF-2 run the premise test scripts` with trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:TF-2`; push the node branch; open the pull request. Pass: CI green on ubuntu, macOS and Windows; on ubuntu and macOS the five steps ran and passed; on Windows they show as skipped. Do not rerun CI without the operator's word.

### Execution notes

- Step 1 (base `e4fb2e37`, PC-13 merged): `scrub tests: ok (158)`; `rescrub tests: ok (172)`;
  `test-check-premises: ok (173 cases)`; `lib tests: ok (157)`; `check-premises: ok (P1 … PC18)`
  with `N2_FORBIDDEN_NAMES=zzcinameq`.
- Step 2: `lib tests: ok (157)` after the change, the same count. The callers of `waitready` keep
  their not-ready branches (`sigrun`, R5, B4, X34, X48, and the `NR` message "not ready in 5 s" in
  `res`); with no count limit these branches no longer run. They are left as they are, because the
  plan keeps each case's assertions unchanged and these lines are outside the plan's change list.
  (Superseded by round 1 F1 and F2 below.)
- Step 5: each of the five script names occurs once in `ci.yml` (`grep -c` per name → 1); the
  diff holds only the five steps, the two path lines, `waitready`, the three inline loops and the
  header line (rewrapped to the file's line width).

## Round 1 review

Each finding is fixed. Code in `096d07ae` (F1-F3) and `d31c85a4` (F4, F5); F6 and this block in
the docs commit that follows them.

- F1 (important), fixed in `096d07ae`: the waits hung when the producer died. `waitready <file>
  <pid>` polls until the file exists or the pid is dead; a dead pid re-checks the file once (0 if
  it exists, else 1). Each caller passes its producer: sigrun and R5 the background run (`bp`),
  `fprep` the server (`SRV`), B4 base.sh (`bp`), X34 and X48 the stub daemon. The child loops of
  X36, X39 (one wait per daemon) and X46 use `waitin`, the same wait with the pid read from the
  case's dpid file. New cases, none timed: W1 (producer exits at once without the file:
  `waitready` returns 1), W2 (producer writes the file and exits: 0), W3 (`waitin` in a child:
  exit 2 and the message on stderr), W6 (sigrun with a child that exits before it is ready:
  `NR=1`).
- F2 (important), fixed in `096d07ae`: `res` says "exited before it was ready" (with the stderr)
  in place of "not ready in 5 s". X34 and X48 record a FAIL with `no` in place of a bare echo;
  `fprep` sets `NR=1` when the server dies before its port file; the child loops exit 2 with
  "<name> exited before it was ready" on stderr, which `res` shows as "exit 2, want <n>".
- F3 (nit), fixed in `096d07ae`: `lib.sh` `start_mock` and `start_web` wait with `n2_wait_port`
  (port file, or the server's death; no count limit); a server that dies still fails at once with
  "mock did not start" / "web did not start" (new cases W4, W5). `srv.py` writes its port file to
  a temporary name and renames it; the `sleep 0.1` after the wait is gone. B4 sends TERM as soon
  as `B4.ready` exists: the server writes it after it accepted base.sh's request and sent the
  partial body, so base.sh has made its schema temporary and set its trap; the `sleep 0.3` is
  gone. Sleeps left in `lib.sh` and `lib_test.sh`, none a timing guess on the test path:
  - poll intervals inside liveness loops: `lib.sh` `n2_wait_port`, `lib_test.sh` `waitready` and
    `waitin`;
  - `lib.sh` `tmux_claude` (`sleep 2`, keeps the pane open after the agent exits),
    `tmux_send` (`sleep 0.5` between the text and Enter) and `tmux_wait_req` (0.5 s poll with a
    count limit): live tmux runs with a real agent only; no CI case calls them;
  - (`sleep 30` in `sigact` and R5 and `time.sleep(30)` in `srv.py` were left here; CI run 2
    below replaced them.)
- F4 (nit), fixed in `d31c85a4`: a comment at the Premise check step says CI uses the fixed name
  `zzcinameq`, which only exercises the scan path, and that the operator's local run with the real
  user name is the privacy check.
- F5 (nit), fixed in `d31c85a4`: the five steps moved from job `test` to job `premises`
  (`matrix.os: [ubuntu-latest, macos-latest]`, `timeout-minutes: 60`, its own checkout, no
  Windows `if:`), parallel with `test`. The path filters are unchanged.
- F6 (nit), fixed in the docs commit: the planning note said macOS falls back to `sh`; macOS has
  `/bin/dash`, and lib_test uses it there.
- Verification after the fixes: `lib tests: ok (163)` (157 + W1-W6); `scrub tests: ok (158)`;
  `rescrub tests: ok (172)`; `check-premises: ok (P1 … PC18)` with
  `N2_FORBIDDEN_NAMES=zzcinameq`; `test-check-premises: ok (173 cases)`; `grep -c` of each of the
  five script names in `ci.yml` → 1.

## CI run 1 (run 45833839265, head `88e3082a`)

The first CI run of these scripts failed on ubuntu and macOS. Each cause is fixed in this round.

- Ubuntu, L13 and C4 (exit 1, want 141; stderr `echo: I/O error`), fixed in `061cb830`: the child
  got EPIPE, not SIGPIPE. GitHub's Linux runner starts the job with SIGPIPE ignored, an ignored
  signal is inherited, and dash cannot undo an ignore it started with. Confirmed locally:
  `lib_test.sh` started under perl with `$SIG{PIPE} = "IGNORE"` fails L13 and C4 the same way, and
  passes after the fix. `piperun` starts the child through perl with `$SIG{PIPE} = "DEFAULT"`;
  the assertions are unchanged.
- Ubuntu, X3, X5, X8, X29, X32, B1, X36, X38b, X39, X40, X46, X50 ("cannot read process working
  folders (pid N: Permission denied)"), fixed in `75710576`: the /proc branch of `n2_scan`
  (`lib.sh`) counted every readlink error but ENOENT, and Linux refuses `/proc/<pid>/cwd` of a
  process that is not wholly this user's. Now, when a cwd cannot be read, `n2_scan` reads
  `/proc/<pid>/status`: a Uid: field (real, effective, saved, fs) of another uid skips the
  process; a process wholly this user's, an unreadable status, or no Uid: line still fails closed
  (the line now names the process by its ps comm). The macOS lsof path is unchanged. `N2_PROC`
  (default `/proc`, assigned at source time like `N2_DAEMON_DIRS`) lets Y1 and Y2 use a stub tree:
  Y1 (another uid) passes, Y2 (this uid) fails closed; with the skip removed, Y1 fails. Not
  confirmed: which runner processes these were. If one is wholly the runner user's but refused
  (a non-dumpable process, such as systemd's `(sd-pam)`), this fix does not cover it and the
  failure line names it.
- macOS, scrub_test "fixed user and host names: output not as wanted", fixed in `aa0025a8`: the
  case wrote the runner's own user and host names into its input and wanted them unchanged, so the
  result depended on the machine. Which rule changed them on the runner is not confirmed (the case
  prints no machine name, and the log does not show the host name); `runner` with two guessed runner
  host names gives no hit for the SCRUB_FIXED rules locally. A host name holding a lower-case UUID
  (the UUID rule) would explain it; that is a guess. The case now patches `getuser`, `gethostname` and `scutil` to give
  `zzmachuserq` and `zzmachhostq(.local)`, as the short-host case does.
- Verification: `lib tests: ok (165)` (163 + Y1, Y2); `lib tests: ok (163)` at `061cb830` with
  SIGPIPE ignored at the start;

## CI run 2 (run 45839465098, head `92b7ff34`)

Results: `test` passed on ubuntu, macOS and Windows; `premises` (ubuntu) failed, and
`premises` (macOS) was cancelled at the 60-minute backstop. The SIGPIPE and scrub_test fixes
held on both runners.

- Ubuntu, the same 12 lib_test cases: the refused process was `pid 1329, systemd` (the runner
  user's own `systemd --user`; every Uid field is this uid). Decision: skip such a process only
  when it started before the part. Fixed in `d09a3cd5`: `new_run` records `N2_PART_MARK`
  (`n2_mark`: clock ticks since boot from `<proc>/uptime`, else epoch seconds). `n2_scan` skips an
  unreadable process of this user only when its start (`/proc/<pid>/stat` field 22, or `ps lstart`
  without /proc) is before the mark; one that started during the part, or whose start is not
  known, fails closed. On the lsof path, a live, non-zombie candidate with no working folder is now
  treated the same way (before, it was taken as outside the run without a failure). A skipped
  process keeps an unknown working folder, so daemon mode still names it. Cases Y3, Y4 (stub
  /proc tree) and Y5, Y6 (stub lsof); Y3 and Y6 fail before the change.
- macOS timeout. Decision: lsof for this uid only (`-a -u <uid> -d cwd -F pn`), fixed in
  `d09a3cd5`, and a 90-minute backstop, fixed in `d7937ff6`. One lookup on the development Mac
  (622 processes of this uid), measured with a scratch script: the old `-p <pids>` form 0.14 s
  and 0.14 s, the `-u` form 0.14 s and 0.14 s. lsof is not what made the runner slow: X46 runs a
  real lsof scan with no mock and took 0.6 s on the runner, while each case that calls
  `start_mock` took about 35 s (X11, X12, X27, X38, X45, with no mock, took under 0.4 s). The
  likely cause is outside this node's files: `mock_upstream.py` uses `ThreadingHTTPServer`, whose
  `server_bind` calls `socket.getfqdn`, a reverse DNS lookup that can stall on the runner. Not
  confirmed. At about 35 s per mock case (about 85 such cases) plus 17 minutes for
  `test-check-premises.sh`, the macOS job needs about 70 minutes, near the 90-minute backstop.
- Fixed in `9058b70c`: `sleep 30` in `sigact` and R5 and `time.sleep(30)` in `srv.py` became
  `signal.pause()` (no time bound); each is still stopped by its case (sigrun's kill by pid file,
  R5's TERM to the process group, B4's kill of the server); no such process is left after a run.
- Verification: `lib tests: ok (169)` (165 + Y3-Y6).
  `scrub tests: ok (158)`; the other checks as above.

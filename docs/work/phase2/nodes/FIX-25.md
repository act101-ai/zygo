# FIX-25 — up tests do not race on the tmux wrapper script (Text file busy)

**Spec:** test harness defect report, refuted by its root cause. `crates/zygo-workspace/tests/up.rs` `restore_first_window_name_resumes_and_reports` failed once on the base branch (`d715b11`, 2026-11-23) with `zygo: tmux: /tmp/zygo-mux-925221-27-9a5d4224/tmux: line 2: /usr/bin/tmux: Text file busy` (exit 1 vs 0) and passed on the next run. The busy file was `/usr/bin/tmux`, not the wrapper: pacman upgraded tmux 3.7_c → 3.8 at 2026-11-23 00:05:44 (`/var/log/pacman.log`; the new `/usr/bin/tmux` inode has birth time 00:05:44.150), inside that run (21.75 s, output returned 00:05:46.996). No code in the repository can cause or remove this failure (evidence under `## Plan`).
**Files:** —
**Consumes:** —
**Produces:** —
**Acceptance:** `sh -c 'for i in $(seq 20); do cargo test -p zygo-workspace --locked --test up || exit 1; done' && cargo test -p zygo-workspace --locked --test down` → exit 0: `--test up` passes 20 runs in a row (no added timeout), then `--test down` passes

## Plan

### Root cause (planned 2026-11-23)

The failing `execve` was of `/usr/bin/tmux`, done by the wrapper's shell. The cause was the system package manager, which wrote a new `/usr/bin/tmux` while the run was in progress. No test caused it.

1. **The shell ran the wrapper; `execve` of the real binary failed.** `PrivateServer::start` (`crates/zygo-workspace/src/mux/mod.rs:116-121`) writes the wrapper `#!/bin/sh\nexec '<program>' -S '<socket>' -f /dev/null "$@"\n`. `/bin/sh` is bash (`/bin/sh -> bash`). The message `<wrapper>: line 2: /usr/bin/tmux: Text file busy` is bash's report for a failed `exec` at line 2 of the script. The wrapper was therefore already running, and `execve("/usr/bin/tmux")` returned ETXTBSY. If the wrapper itself were busy, the spawn in Rust would fail and the message would have no `line 2:`. Reproduced in the scratchpad: a copy of tmux held open for write (`exec 3>>copy`) gives exactly `<wrapper>: line 2: <copy>: Text file busy`, exit 126; when the descriptor is closed, the same wrapper prints `tmux 3.8`, exit 0.
2. **ETXTBSY on an `execve` means the target was open for writing.** `/usr/bin/tmux` is `root:root 0755` and the tests run as uid 1000. No test process can open it for writing, so no test, helper or thread in this crate can make it busy. No test installs a fake tmux at that path: `require_tmux` (`mod.rs:59-69`) resolves the first `tmux` on the inherited `PATH`, which here is `/usr/bin/tmux`. The stand-in scripts of `up.rs` (`script`, `up.rs:68-73`) are written in `TempDir`s under other names.
3. **The writer was pacman.** `/var/log/pacman.log`: `[2026-11-23T00:05:44-0800] [ALPM] upgraded tmux (3.7_c-1.1 -> 3.8-1.1)`. `stat /usr/bin/tmux`: Birth `2026-11-23 00:05:44.150064757 -0800`. The failed run (`cargo test -p zygo-workspace --locked --test up 2>&1`, session transcript) was sent at 08:05:20.831Z and its output returned at 08:05:46.996Z (00:05:46.996 local), `finished in 21.75s`. The test binary therefore ran from about 00:05:25 to 00:05:47, and the write of the new binary at 00:05:44.15 is inside that window. Under libalpm, a `execve` of the path between the creation of the new inode and the close of its write descriptor gets ETXTBSY.
4. **The wrapper race that the node title names is already closed.** Every `up.rs` test takes `serial()` (`up.rs:3-4`). `PrivateServer::start` runs the wrapper with `-V` until it succeeds (`mod.rs:126-139`). A successful `execve` of the wrapper proves that its write count was 0 at that moment. The parent's descriptor was closed when `std::fs::write` returned, and Rust opens files `O_CLOEXEC`. Thus no process can hold the wrapper open for writing after the first `-V` succeeds, and no later exec of the wrapper can get ETXTBSY.

### Reproduction

- Exact message shape: reproduced deterministically (point 1 above).
- Original failure: not reproducible from code. It needs the system binary to be replaced during a run. Changing `/usr/bin/tmux` needs root, and the operator rules do not let a test do that.
- 20-run loop on `worktree-zygo-phase-2` at `04b8249`, with no tmux upgrade in progress (2026-11-23 00:17:16 to 00:25:41, other agents' test runs in parallel on the machine): 20 of 20 runs `test result: ok. 47 passed; 0 failed`.

### Decision

No code change. The node's premise (a race between test threads on the wrapper script) is gone, so the node is `DROPPED` per execute-work §3.1. A fix inside the tests cannot remove a race against a root process that replaces a system binary. For example, a private copy of tmux would read the same file while pacman writes it, and an ETXTBSY retry would be a retry-until, which the brief forbids. To add a rule that tests must not run during a system upgrade needs operator approval (rules notice). This plan does not propose such a rule.

### Steps (controller)

- [ ] 1. Set FIX-25 to `DROPPED(Text file busy came from pacman replacing /usr/bin/tmux at 2026-11-23 00:05:44 during the run; no repo code can write that root-owned file; nodes/FIX-25.md)` in `docs/work/phase2/graph.md`. Check: `python3 /home/kagemartin/.cache/zygo/sessions/01M80K6VKVFKHN736B0QCWTTKZ/plugins/act101/skills/work-graph/scripts/work_graph.py check docs/work/phase2/graph.md` → `docs/work/phase2/graph.md: ok`.
- [ ] 2. Re-edge FIX-24 (its Depends lists FIX-25): remove FIX-25 from its Depends, because FIX-25 changes no file that FIX-24 needs. Remove FIX-25 from the **Next:** line. Check: the same validator → `ok`.
- [ ] 3. Record a Log line in `graph.md` (at most 300 characters): `FIX-25 DROPPED: ETXTBSY on /usr/bin/tmux from pacman tmux 3.8 upgrade (00:05:44) during the base run; wrapper race already closed (mod.rs:126-139); up 20/20 pass.` Commit `graph.md` and this file with `git commit -F <message file>` and the env `GIT_AUTHOR_NAME=kagemartin GIT_AUTHOR_EMAIL=1011970+kagemartin@users.noreply.github.com GIT_COMMITTER_NAME=kagemartin GIT_COMMITTER_EMAIL=1011970+kagemartin@users.noreply.github.com`, with no attribution line. The message ends with `Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ` and `Zygo-Work: FIX-25`, each on its own line.

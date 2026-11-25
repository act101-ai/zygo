# FX-5 — Piped send test asserts send-then-read fails (Linux foreign-reader flake)

**Spec:** `docs/work/phase3/nodes/DR-11.md` :84 (rule 4: the reply is read before the next request is written) and :147 (`send` does `write_all` + `flush`, and an error (EPIPE) gives `Failed`; `Disconnected` (EOF: git ended) gives `Failed`); review focus 75 (no new test spawns `claude`/`opencode` or reads a clock). The spec does not change.
**Files:** `crates/zygo-work/tests/decision_git.rs`, `crates/zygo-work/tests/piped_epipe.rs`
**Consumes:** —
**Produces:** —
**Acceptance:** `env -u ZYGO_SESSION_DIR cargo test -p zygo-work --locked --test decision_git --test piped_epipe` → all pass; piped_epipe holds exactly one test; the tests `piped_send_after_the_child_closed_stdin_fails` and `piped_send_then_read_fails_with_a_foreign_reader` both exist; no `matches!(p.send(` assertion stands alone in that file; the change adds no sleep in Rust.

## Plan

### Defect (found in CI)

`piped_send_after_the_child_closed_stdin_fails` (`crates/zygo-work/tests/decision_git.rs:453-460`) fails intermittently on ubuntu, for example CI run 45544809983: `assertion failed: matches!(p.send(b"x\n"), Err(RunError::Failed))`. The order of events:

1. On Linux a pipe write gets EPIPE only when no process holds the read end.
2. Another test thread spawns a child (posix_spawn or fork) while the read end of this test's stdin pipe is still in the parent. That child holds a copy until its `execve`, where O_CLOEXEC closes it.
3. When `send` (`crates/zygo-work/src/repo.rs:335-337`, `write_all` + `flush`) runs in that window, the write goes into the pipe buffer and succeeds.

The product is correct. DR-11 :147 says only that an EPIPE error gives `Failed`. Rule 4 (:84) reads the reply before the next request, and `ProcessGit::ask` does this (`crates/zygo-work/src/gate/git.rs:222-223`). After the child exits, `read_line` gets EOF, the channel reports `Disconnected` (`repo.rs:385`), and the result is `Failed`. So the observable contract is "send, then read, fails", and the fix goes in the test.

### Premises checked against the live code (branch `phase3/FX-5` at `9835c417`)

- `decision_git.rs:455-460`: the test runs `sh("exec 0<&-; printf 'closed\\n'")`, reads `closed`, then asserts `matches!(p.send(b"x\n"), Err(RunError::Failed))` alone.
- `sh` (`decision_git.rs:431-435`) calls `run_piped` with `far()`, a deadline one hour away; no test reads a clock.
- `spawn_piped` puts the child in its own process group (`repo.rs:276-278`, `process_group(0)`). A non-interactive `sh` keeps a background job in that group.
- Dropping `Piped` drops its `Running` field (`repo.rs:297-298`); `Drop for Running` (`repo.rs:131-135`) calls `stop` (`repo.rs:106-114`), which, when `waitid` succeeds, calls `kill_tree` (`crates/zygo-config/src/repo_file.rs:120-124`): `kill_group` first, then reap. The unreaped leader keeps the group id valid, so the background `sleep 30` of the new test dies with the group and outlives no test.
- `read_line` maps `Disconnected` to `Failed` (`repo.rs:385`); the reader thread ends at EOF, which comes when every holder of the stdout write end has exited or closed it. The new test sends the background `sleep`'s stdout and stderr to `/dev/null`, so EOF comes when `sh` exits.

### Steps

- [x] **1. Add the foreign-reader test, red.** After `piped_send_after_the_child_closed_stdin_fails`, add a `#[cfg(unix)] #[test]` named `piped_send_then_read_fails_with_a_foreign_reader`. It holds the read end in a background process on purpose, then asserts the OLD single-send form:
  ```rust
  let mut p = sh("sleep 30 <&0 >/dev/null 2>&1 & exec 0<&-; printf 'closed\\n'");
  assert_eq!(p.read_line(16).unwrap(), b"closed");
  assert!(matches!(p.send(b"x\n"), Err(RunError::Failed)));
  ```
  Run `env -u ZYGO_SESSION_DIR cargo test -p zygo-work --locked --test decision_git piped_send_then_read_fails_with_a_foreign_reader`. Check: it fails at the `send` assertion (the write succeeds because `sleep` holds the read end). This proves the old assertion is wrong and does not depend on timing.
  Result (red): `panicked at crates/zygo-work/tests/decision_git.rs:468:5: assertion failed: matches!(p.send(b"x\n"), Err(RunError::Failed))`.
- [x] **2. Make it green with the send-then-read assertion.** Replace the last line of the new test with:
  ```rust
  let r = p.send(b"x\n").and_then(|()| p.read_line(16).map(drop));
  assert!(matches!(r, Err(RunError::Failed)));
  ```
  Run the same command. Check: it passes, and it returns at once (the `sleep 30` dies with the process group when `p` drops).
- [x] **3. Fix the original test.** In `piped_send_after_the_child_closed_stdin_fails`, replace `assert!(matches!(p.send(b"x\n"), Err(RunError::Failed)));` with the same two lines as step 2. Check: `grep -n 'matches!(p.send(' crates/zygo-work/tests/decision_git.rs` prints nothing.
- [x] **4. Run the acceptance.** `env -u ZYGO_SESSION_DIR cargo test -p zygo-work --locked --test decision_git` → all pass. Check: `git diff --stat` lists only `crates/zygo-work/tests/decision_git.rs`; the Rust lines added contain no `sleep` and no `Instant`/`Duration` read (the only `sleep` is in the shell script string, as the foreign reader).
- [x] **5. Commit.** `FX-5: piped send tests assert send-then-read fails`, with the brief's `Zygo-Session` and `Zygo-Work` trailers, no attribution. Push the node branch.

### Review round 1

- [x] **F1. dash-safe foreign reader.** When job control is off, dash points a background job's fd 0 at /dev/null before it applies the job's redirections, so `sleep 30 <&0 &` had no pipe on Linux. Copy the pipe to fd 3 first (`exec 3<&0; … <&3 3<&-`). Probe (scratchpad `fx5-probe.py`, a pipe on stdin, read `closed`, then write): old script under dash gives EPIPE (no reader); new script under dash and under bash, the write succeeds (foreign reader present).
- [x] **F3. `cat` as the reader.** The script is `exec 3<&0; cat <&3 >/dev/null 2>&1 3<&- & exec 0<&- 3<&-; printf 'closed\n'`. `cat` blocks until the write end closes, so it does not depend on a timer. Red with the old single-send assertion under cargo (macOS `sh`): `panicked at crates/zygo-work/tests/decision_git.rs:469:5: assertion failed: matches!(p.send(b"x\n"), Err(RunError::Failed))`. Green with send-then-read. `pgrep -x cat` finds nothing after the run (the group kill on drop ends it, also after the panic).
- [x] **F2. Single-test EPIPE binary.** New `crates/zygo-work/tests/piped_epipe.rs` with exactly one test, `piped_send_after_the_child_closed_stdin_fails_with_epipe`, which asserts the bare `send` gives `Failed` (DR-11 :147). One test means one test thread, so no foreign child holds the read end. Red with a local, uncommitted edit that made `send` return `Ok(())` on a write error: `panicked at crates/zygo-work/tests/piped_epipe.rs:21:5: assertion failed: matches!(p.send(b"x\n"), Err(RunError::Failed))`; `repo.rs` reverted.

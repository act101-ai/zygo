# FX-2 — Decision writer lock: unlock before the close (a forked child keeps a closed flock)

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§11.3 (the verb holds an exclusive lock on `<git common dir>/zygo-decisions.lock` from before the re-read until after the rename; DR-5 round 2) and D§17 (no wall-clock test); review focus 39 (the re-read-then-rename race, tests in DR-5); review focus 75 (no new test reads a clock)
**Files:** `crates/zygo-work/src/decisions/edit.rs`, `docs/work/phase3/nodes/FX-2.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `cargo test -p zygo-work --locked --lib decisions::edit` → the test `the_lock_is_released_while_a_forked_child_holds_the_descriptor` exists and all `decisions::edit` tests pass on ubuntu, macOS and Windows; no new sleep, timer, clock read or retry.

## Plan

### Defect (found in RT-1's CI, execute-work §5.4)

ubuntu CI run 45530476489 failed: `decisions::edit::tests::the_record_is_locked_from_the_re_read_to_the_rename` at `edit.rs:297`, `File::open(&l).unwrap().try_lock().is_ok()` was false after `apply_with` returned. Cause: `lock()` takes `flock(LOCK_EX)` (`f.lock()`) on an `O_CLOEXEC` file and returns the `File`; `apply_with` holds it as `_lock` and releases it only by the drop (the close). A flock belongs to the open file description, so a close releases it only when the last descriptor of that description closes. A child spawned by another thread (posix_spawn or fork) holds a copy of every descriptor from the fork until its execve, where `O_CLOEXEC` closes it. When `_lock` drops in that window, the lock stays held until the child execs. Proof from triage: a stress harness gave 4932 `WouldBlock` in 200000 iterations with a spawner thread running, and 0 with `unlock()` before the drop; the deterministic fork test below fails without the fix and passes with it.

Production impact: none in practice. `zygo decision` calls `apply` on the main thread with no other spawning thread (`crates/zygo/src/cmd/decision.rs:410`, `:566`). The fix is still correct: D§11.3 says the lock ends after the rename, and only an explicit unlock makes that true whatever other threads do.

Spec: D§11.3 (line 464) says the lock is held "from before the re-read until after the rename"; it says nothing about a drop or a close. The fix agrees with it, so no spec text changes.

### Premises checked against the live code (branch `phase3/FX-2` at `3764722`, `edit.rs` blob `fcba8fe`, 319 lines)

- `fn lock(path: &Path) -> Result<fs::File, EditError>` at `edit.rs:148-165` (doc comment 146-147): opens with `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC` on unix (line 155), `FILE_FLAG_OPEN_REPARSE_POINT` on Windows, then `f.lock()` (line 163), `Ok(f)` (line 164).
- `apply_with` keeps `let _lock = lock(lock_file)?;` (line 178); the lock is dropped when `apply_with` returns, after `write`'s rename. No other caller of `lock` (`grep -n "lock(" edit.rs`).
- `mod tests` (line 241) already imports `std::fs::{File, TryLockError}`; `put` (252) and `OPEN` give a record and its lock path. The new test goes after `the_record_is_locked_from_the_re_read_to_the_rename` (ends line 299).
- `libc` is a `[target.'cfg(unix)'.dependencies]` of zygo-work (`crates/zygo-work/Cargo.toml:28-29`), so `#[cfg(unix)]` test code can use it; `tempfile` is a dev-dependency (line 26). No Cargo change.
- Toolchain 1.98 (`rust-toolchain.toml`): `File::lock`, `File::try_lock` and `File::unlock` are stable (since 1.89). On Windows `unlock` is `UnlockFile`; the guard is the same code on every platform.
- The new test forks in the `before_rename` hook, while the lock is held: the child gets a copy of the lock descriptor and blocks in `read` on a pipe until the parent closes the write end, which the parent does only after its `try_lock` check. So the check sees the exact window of the defect, with no timer.

### Test and fix land in one commit

Without the fix, the new test's child keeps a copy of every descriptor of the test process until the parent closes the pipe, so sibling tests that run in parallel and rely on a close to release their own flock fail too. The red run is therefore done with the test alone and only observed; the fix is added before anything is committed.

### Steps

- [x] **1. Add the failing-first test** in `crates/zygo-work/src/decisions/edit.rs` `mod tests`, directly after `the_record_is_locked_from_the_re_read_to_the_rename` (after line 299):
  ```rust
      /// D§11.3: the lock is released when `apply` returns, even while a child process forked by another thread still
      /// holds a copy of the lock file's descriptor (from fork to exec every spawned child holds one; O_CLOEXEC closes it
      /// only at exec). A close releases a `flock` only with the last descriptor of the open file; an unlock releases it.
      #[cfg(unix)]
      #[test]
      fn the_lock_is_released_while_a_forked_child_holds_the_descriptor() {
          let t = tempfile::tempdir().unwrap();
          let (p, l) = put(t.path());
          let mut fds = [0; 2];
          assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
          let mut child = 0;
          let mut hook = || {
              child = unsafe { libc::fork() };
              if child == 0 {
                  // The child: only async-signal-safe calls; it waits until the parent closes the write end.
                  let mut b = 0u8;
                  unsafe {
                      libc::close(fds[1]);
                      libc::read(fds[0], (&mut b as *mut u8).cast(), 1);
                      libc::_exit(0);
                  }
              }
          };
          let o = Outcome::Overturn { reason: "x".into() };
          let r = apply_with(&p, &l, OPEN.as_bytes(), &o, "2026-12-01", &mut || {}, &mut hook);
          let free = File::open(&l).unwrap().try_lock().is_ok();
          unsafe {
              libc::close(fds[1]);
              libc::waitpid(child, std::ptr::null_mut(), 0);
              libc::close(fds[0]);
          }
          assert!(child > 0);
          assert_eq!(r, Ok(()));
          assert!(free, "the lock is still held through the child's copy of the descriptor");
      }
  ```
  The cleanup (close, `waitpid`) runs before the asserts, so a red run leaves no child process.
- [x] **2. See it fail red (observe only; do not commit).** `cargo test -p zygo-work --locked --lib decisions::edit::tests::the_lock_is_released_while_a_forked_child_holds_the_descriptor -- --exact` → FAILED with `the lock is still held through the child's copy of the descriptor`. A run of the whole `decisions::edit` filter at this point may also fail sibling tests (the child holds their descriptors); that is expected and is why step 3 comes before any commit. Then confirm `pgrep -fl zygo_work` lists no leftover test process.
- [x] **3. Add the guard.** In `crates/zygo-work/src/decisions/edit.rs` insert above line 146 (the doc comment of `lock`):
  ```rust
  /// The held lock of [`lock`]. Drop unlocks before the close: a close releases a `flock` only with the last descriptor
  /// of the open file, and a child process spawned by another thread holds a copy until its exec.
  struct Held(fs::File);

  impl Drop for Held {
      fn drop(&mut self) {
          let _ = self.0.unlock();
      }
  }

  ```
  then change line 148 to `fn lock(path: &Path) -> Result<Held, EditError> {` and line 164 `Ok(f)` to `Ok(Held(f))`. The existing doc comment of `lock` (lines 146-147) must sit directly above `fn lock`, not above `struct Held`: inserting the struct at line 148 as the triage copy does would join both doc comments onto `Held`. So insert the struct and `impl` above line 146, and change only the signature line. `apply_with` keeps `let _lock = lock(lock_file)?;` unchanged.
- [x] **4. See it pass.** `cargo test -p zygo-work --locked --lib decisions::edit` → all pass, the new test among them. Then `cargo test -p zygo-work --locked --lib` → all pass (siblings no longer affected). `cargo clippy -p zygo-work --locked --all-targets -- -D warnings` → clean. `git diff --stat` lists only `edit.rs` and this node file. `git diff crates/zygo-work/src/decisions/edit.rs | grep -nE "sleep|Instant|SystemTime|Duration|retry"` → no output. Windows: the test is `#[cfg(unix)]` and the guard has no platform code; the windows-latest CI job is the check.
- [x] **5. Commit** test and fix together: `FX-2: unlock the decision writer lock before the close`, with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:FX-2`, each on its own line; no attribution. Push the node branch.

### Notes for review

- The test's `libc::pipe` ends are not close-on-exec (macOS has no `pipe2`). A child that another test thread spawns between `pipe` and the parent's `close(fds[1])` inherits the write end; the forked child then waits until that sibling child exits too. That wait is bounded by the sibling's own lifetime and no zygo-work lib test spawns a long-lived child (`grep -rn "Command::new" crates/zygo-work/src` → only `sources.rs:50`). Kept as triage wrote it.

### Results (round 1)

- Red (step 2, test alone, before the guard): `test decisions::edit::tests::the_lock_is_released_while_a_forked_child_holds_the_descriptor ... FAILED`, panicked at `crates/zygo-work/src/decisions/edit.rs:335:9`: `the lock is still held through the child's copy of the descriptor`; `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 6 filtered out`. No test process left after the run (`ps -axo pid,ppid,command | grep deps/zygo_work-` → none).
- Green (step 4): `cargo test -p zygo-work --locked --lib decisions::edit` → `test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out`. `cargo test -p zygo-work --locked --lib` → `test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`. No test process left after the runs.
- `git diff --stat` before the commit: only `edit.rs` (48 insertions, 2 deletions) and this node file. The clock/retry grep on the `edit.rs` diff → no output.
- `cargo clippy -p zygo-work --locked --all-targets -- -D warnings` → 2 errors, both in `crates/zygo-work/src/gate/transition.rs` (`clippy::large_enum_variant` at line 10, `clippy::collapsible_if` at line 158). That file is outside this node and unchanged by it; `edit.rs` has no clippy finding. Reported, not fixed here.

### Review round 1

Accepted:

- **F1. `waitpid` of a child that was never forked.** `child` started at 0 and the cleanup called `waitpid(child, …)` unconditionally. If `apply_with` failed before the hook ran, `waitpid(0)` reaped any child in the process group; if `fork` failed, `waitpid(-1)` reaped any child. Either can steal the `git` child of `repo::tests::find_checkout_tells_timeout_from_not_a_repository` (`repo.rs:635`, `group=false`) and make it report a false timeout. Fix: the cleanup waits only when `child > 0`.
- **F2. A panic skipped the cleanup.** A panic in `apply_with` or at `File::open(&l).unwrap()` skipped the close/`waitpid` block: the child stayed blocked in `read` and both pipe ends stayed open in the parent. Fix: a drop guard `Reap` local to the test owns `fds` and `child`; on drop it closes the write end, waits for the child only if `child > 0`, and closes the read end. The happy path takes `try_lock` (the `free` result) while the child still holds the descriptor, then drops the guard before the asserts. The forked child still makes only async-signal-safe calls (`close`, `read`, `_exit`).

Refuted:

- **F3. Pipe ends inherited by a child of a sibling test thread.** The wait is bounded by that sibling child's own lifetime; already recorded in "Notes for review".
- **F4. EINTR on `read` / `waitpid`.** The forked child installs no signal handler that can interrupt its `read`, and the test process installs none that interrupts the parent's `waitpid`; no EINTR path exists.
- **F5. Non-`Ok` release paths.** `_lock` is a scope binding in `apply_with`; every return, `?` and unwind drops `Held`, which unlocks. Correct by construction.

Results:

- Red with the mutant (`Held::drop` made a no-op, not committed, restored after the run): `cargo test -p zygo-work --locked --lib decisions::edit` → `the_lock_is_released_while_a_forked_child_holds_the_descriptor ... FAILED`, panicked at `edit.rs:360:9`: `the lock is still held through the child's copy of the descriptor`. `the_record_is_locked_from_the_re_read_to_the_rename` also failed (`edit.rs:307:9`): the forked child of the parallel test holds its descriptor, as step 2 predicted. `test result: FAILED. 2 passed; 2 failed`.
- Green: `cargo test -p zygo-work --locked --lib decisions::edit` → `test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out`. `cargo test -p zygo-work --locked --lib` → `test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`.
- No leftover test process after the red and green runs (`ps -U <uid> | grep zygo_work` → none).
